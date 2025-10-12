use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;
use std::time::Duration;
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
use replay_script::{ExitSignal, MsgLevel, ReplayMsg};
use tokio::time::{Instant, sleep};

use crate::config::RunRules;

#[derive(Clone, Copy)]
pub struct ReplayReport {
    pub max_msg_level: MsgLevel,
    pub win_condition_not_completed: bool,
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
    run_and_log_replay(&instance, &installed_save_path, log_path, rules).await
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
    rules: &RunRules,
) -> Result<ReplayReport, FactorioError> {
    info!("Starting replay");
    info!("Writing to: {}", log_path.display());
    let mut process = instance.spawn_replay(installed_save_path)?;
    let (max_msg_level, exited_via_script) = record_output(&mut process, log_path).await?;

    let exit_status = process.wait().await?;
    if !exit_status.success() && !exited_via_script {
        return Err(FactorioError::ProcessExitedUnsuccessfully {
            exit_code: exit_status.code(),
        });
    }

    copy_factorio_log(instance, log_path)?;

    let win_condition_not_completed =
        rules.replay_scripts.win_on_scenario_finished && !exited_via_script;

    if win_condition_not_completed {
        let mut log_file = File::options().append(true).open(log_path)?;
        writeln!(
            log_file,
            "VERIFICATION FAILED: win_on_scenario_finished enabled but scenario never completed"
        )?;
    }

    Ok(ReplayReport {
        max_msg_level,
        win_condition_not_completed,
    })
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
) -> Result<(MsgLevel, bool), FactorioError> {
    let mut log_file = File::create(log_path)?;
    let mut stream = msg_stream(process);

    let mut max_level = MsgLevel::Info;
    let timeout_duration = Duration::from_secs(300);
    let mut last_message_time = Instant::now();
    let mut exited_successfully = false;

    loop {
        let time_since_last_msg = last_message_time.elapsed();
        let remaining_time = timeout_duration
            .checked_sub(time_since_last_msg)
            .unwrap_or(Duration::ZERO);

        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(StreamItem::Message(msg)) => {
                        writeln!(log_file, "{}", msg)?;
                        max_level = max_level.max(msg.level);
                        last_message_time = Instant::now();
                    }
                    Some(StreamItem::Exit(exit)) => {
                        writeln!(log_file, "{}", exit)?;
                        drop(stream);
                        process.terminate();
                        exited_successfully = true;
                        break;
                    }
                    None => break,
                }
            }
            _ = sleep(remaining_time), if remaining_time > Duration::ZERO => {
                drop(stream);
                process.terminate();
                return Err(FactorioError::ReplayTimeout);
            }
        }
    }

    if exited_successfully {
        info!("Replay exited successfully via exit signal");
    }

    Ok((max_level, exited_successfully))
}

enum StreamItem {
    Message(ReplayMsg),
    Exit(ExitSignal),
}

fn msg_stream(process: &mut FactorioProcess) -> Pin<Box<dyn Stream<Item = StreamItem> + '_>> {
    let mut reader = process.stdout_reader().unwrap();
    Box::pin(async_stream::stream! {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let line = line.trim_end();
                    if let Ok(exit) = ExitSignal::from_str(line) {
                        log::info!("{exit}");
                        yield StreamItem::Exit(exit);
                        break;
                    } else if let Ok(msg) = ReplayMsg::from_str(line) {
                        log::info!("{msg}");
                        yield StreamItem::Message(msg);
                    } else {
                        log::debug!("{line}");
                    }
                }
                Err(_) => continue,
            }
        };
    })
}
