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

#[derive(Clone)]
pub struct ReplayReport {
    pub max_msg_level: MsgLevel,
    pub win_condition_not_completed: bool,
    pub messages: Vec<String>,
}

impl ReplayReport {
    pub fn to_exit_code(&self) -> i32 {
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
    let version = save_file.get_factorio_version()?;
    info!(
        "=== Running replay ===\nSave file: {}\nSave version: {}",
        save_path.display(),
        version
    );

    let mut instance = get_instance(install_dir, save_file).await?;
    do_pre_run_checks(&mut instance, save_path, expected_mods).await?;
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

async fn do_pre_run_checks(
    instance: &mut FactorioInstance,
    save_path: &Path,
    expected_mods: &ExpectedMods,
) -> Result<(), FactorioError> {
    info!("Doing pre-run checks");
    let mod_versions = instance.get_mod_versions(save_path).await?;
    check_expected_mods(expected_mods, &mod_versions)?;
    debug!("Pre-run checks passed");
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
    info!("Starting replay. Log file at {}", log_path.display());
    let mut log_file = File::create(log_path)?;

    // Phase 1: replay
    let mut process = instance.spawn_replay(installed_save_path)?;
    let output = record_output(&mut process, &mut log_file).await?;

    process.terminate();
    let exit_status = match tokio::time::timeout(Duration::from_secs(5), process.wait()).await {
        Ok(result) => result?,
        Err(_) => {
            process.kill();
            process.wait().await?
        }
    };
    if !exit_status.success() && !output.exited_via_script {
        return Err(FactorioError::ProcessExitedUnsuccessfully {
            exit_code: exit_status.code(),
        });
    }

    // Phase 2: run --benchmark 1 tick on the post-replay save to trigger on_load,
    // which fires afterReplay callbacks (on_init only runs during --run-replay).
    let mut bench_process = instance.spawn_benchmark(installed_save_path, 1)?;
    let bench_output = record_output(&mut bench_process, &mut log_file).await?;
    terminate_and_wait(&mut bench_process).await;

    copy_factorio_log(instance, log_path)?;

    let win_condition_not_completed =
        rules.replay_scripts.win_on_scenario_finished && !output.exited_via_script;

    let max_msg_level = output.max_level.max(bench_output.max_level);
    let mut messages = output.messages;
    messages.extend(bench_output.messages);

    if win_condition_not_completed {
        let msg = "win_on_scenario_finished enabled but scenario never completed";
        messages.push(msg.to_string());
        writeln!(log_file, "VERIFICATION FAILED: {msg}")?;
    }

    Ok(ReplayReport {
        max_msg_level,
        win_condition_not_completed,
        messages,
    })
}

async fn terminate_and_wait(process: &mut FactorioProcess) {
    process.terminate();
    if tokio::time::timeout(Duration::from_secs(5), process.wait())
        .await
        .is_err()
    {
        process.kill();
        process.wait().await.ok();
    }
}

fn copy_factorio_log(instance: &FactorioInstance, log_path: &Path) -> Result<(), FactorioError> {
    let factorio_log = instance.log_file_path();
    factorio_log
        .exists()
        .then(|| {
            let output_dir = log_path.parent().unwrap();
            let dest_path = output_dir.join("factorio-current.log");
            std::fs::copy(&factorio_log, &dest_path)
                .map(|_| debug!("Copied factorio log to: {}", dest_path.display()))
        })
        .transpose()?;
    Ok(())
}

/// returns when stdout closes.
struct RecordOutputResult {
    max_level: MsgLevel,
    exited_via_script: bool,
    messages: Vec<String>,
}

async fn record_output(
    process: &mut FactorioProcess,
    log_file: &mut File,
) -> Result<RecordOutputResult, FactorioError> {
    let mut stream = msg_stream(process);

    let mut max_level = MsgLevel::Info;
    let mut messages = Vec::new();
    let timeout_duration = Duration::from_secs(60);
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
                        if msg.level >= MsgLevel::Warn {
                            messages.push(msg.message.clone());
                        }
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
        info!("Replay finished");
    }

    Ok(RecordOutputResult {
        max_level,
        exited_via_script: exited_successfully,
        messages,
    })
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
                        log::debug!("{msg}");
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
