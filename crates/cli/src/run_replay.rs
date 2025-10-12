use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;
use std::{fs::File, io::Write, path::Path};

use anyhow::Result;
use factorio_manager::error::FactorioError;
use factorio_manager::factorio_instance::{FactorioInstance, FactorioProcess};
use factorio_manager::save_file::SaveFile;
use factorio_manager::{
    expected_mods::{ExpectedMods, check_expected_mods},
    factorio_install_dir::FactorioInstallDir,
    save_file::WrittenSaveFile,
};
use futures::{AsyncBufReadExt, Stream, StreamExt};
use log::{debug, info};
use replay_script::{MsgLevel, ReplayMsg};

use crate::config::RunRules;

#[derive(Clone, Copy)]
pub struct ReplayReport {
    pub max_msg_level: MsgLevel,
}

impl ReplayReport {
    pub fn to_exit_code(self) -> i32 {
        match self.max_msg_level {
            MsgLevel::Info => 0,
            MsgLevel::Warn => 1,
            MsgLevel::Error => 2,
        }
    }
}

pub async fn run_replay(
    install_dir: &FactorioInstallDir,
    WrittenSaveFile(save_path, save_file): &mut WrittenSaveFile,
    rules: &RunRules,
    expected_mods: &ExpectedMods,
    log_path: &Path,
) -> Result<ReplayReport, FactorioError> {
    info!("=== Run Information ===");
    info!("Save file: {}", save_path.display());

    let version = save_file.get_factorio_version()?;
    info!("Save version: {}", version);

    let mut instance = get_instance(install_dir, save_file).await?;
    perform_pre_run_checks(&mut instance, save_path, expected_mods).await?;
    let installed_save_path = install_replay_script(save_path, save_file, rules).await?;
    run_and_log_replay(&instance, &installed_save_path, log_path).await
}

async fn get_instance(
    install_dir: &FactorioInstallDir,
    save_file: &mut SaveFile<File>,
) -> Result<FactorioInstance, FactorioError> {
    let version = save_file.get_factorio_version()?;
    install_dir.get_or_download_factorio(version).await
}

async fn perform_pre_run_checks(
    instance: &mut FactorioInstance,
    save_path: &Path,
    expected_mods: &ExpectedMods,
) -> Result<(), FactorioError> {
    info!("Performing pre-run checks");
    let mod_versions = instance.get_mod_versions(save_path).await?;
    check_expected_mods(expected_mods, &mod_versions)?;
    info!("Pre-run checks passed");
    Ok(())
}

async fn install_replay_script(
    save_path: &Path,
    save_file: &mut SaveFile<File>,
    rules: &RunRules,
) -> Result<PathBuf, FactorioError> {
    info!("Installing replay script");
    let replay_script = &rules.replay_scripts;
    debug!("Enabled checks: {:?}", replay_script);
    let installed_save_path = save_path.with_extension("installed.zip");
    save_file.install_replay_script_to(&mut File::create(&installed_save_path)?, replay_script)?;
    Ok(installed_save_path)
}

async fn run_and_log_replay(
    instance: &FactorioInstance,
    installed_save_path: &Path,
    log_path: &Path,
) -> Result<ReplayReport, FactorioError> {
    info!("Starting replay");
    info!("Writing to: {}", log_path.display());
    let mut process = instance.spawn_replay(installed_save_path)?;
    let max_msg_level = record_output(&mut process, log_path).await?;

    let exit_status = process.wait().await?;
    if !exit_status.success() {
        return Err(FactorioError::ProcessExitedUnsuccessfully {
            exit_code: exit_status.code(),
        });
    }

    copy_factorio_log(instance, log_path)?;

    Ok(ReplayReport { max_msg_level })
}

fn copy_factorio_log(instance: &FactorioInstance, log_path: &Path) -> Result<(), FactorioError> {
    let factorio_log = instance.log_file_path();
    factorio_log
        .exists()
        .then(|| {
            let output_dir = log_path.parent().unwrap();
            let dest_path = output_dir.join("factorio-current.log");
            std::fs::copy(&factorio_log, &dest_path)
                .map(|_| info!("Copied factorio log to: {}", dest_path.display()))
        })
        .transpose()?;
    Ok(())
}

/// returns when stdout closes.
async fn record_output(
    process: &mut FactorioProcess,
    log_path: &Path,
) -> Result<MsgLevel, FactorioError> {
    let mut log_file = File::create(log_path)?;
    let mut msgs = msg_stream(process);

    let mut max_level = MsgLevel::Info;

    while let Some(msg) = msgs.next().await {
        writeln!(log_file, "{}", msg)?;
        max_level = max_level.max(msg.level);
    }
    Ok(max_level)
}

fn msg_stream(process: &mut FactorioProcess) -> Pin<Box<dyn Stream<Item = ReplayMsg> + '_>> {
    // ^ that's a fun type
    let mut reader = process.stdout_reader().unwrap();
    Box::pin(async_stream::stream! {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let line = line.trim_end();
                    if let Ok(msg) = ReplayMsg::from_str(line) {
                        log::info!("{msg}");
                        yield msg;
                    } else {
                        log::debug!("{line}");
                    }
                }
                Err(_) => continue,
            }
        };
    })
}
