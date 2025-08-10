use anyhow::{Context, Result};
use async_std::io::BufReadExt;
use async_std::prelude::StreamExt;
use replay_script::ReplayMsg;
use std::io::{Read, Seek};
use std::str::FromStr;

use crate::factorio_install_dir::FactorioInstallDir;
use crate::factorio_instance::FactorioProcess;
use crate::rules::{Rules, check_expected_mods};
use crate::{factorio_instance::FactorioInstance, save_file::SaveFile};

pub struct ReplayLog {
    pub messages: Vec<ReplayMsg>,
    pub exit_success: bool,
}

pub enum ReplayRunResult {
    PreRunCheckFailed { cause: String },
    Success(ReplayLog),
}

impl FactorioInstance {
    fn spawn_replay(&self, save_name: &str) -> Result<FactorioProcess> {
        self.spawn(&["--run-replay", save_name])
    }

    fn add_save_with_installed_replay_script(
        &self,
        save_file: &mut SaveFile<impl Read + Seek>,
        replay_script: &str,
    ) -> Result<()> {
        let mut out_file = self
            .create_save_file(save_file.save_name())
            .context("Failed to create save file")?;
        save_file.install_replay_script_to(&mut out_file, replay_script)?;
        Ok(())
    }
}

impl FactorioProcess {
    pub async fn collect_replay_log(&mut self) -> Result<ReplayLog> {
        let mut lines = self.stdout_reader()?.lines();
        let mut messages = Vec::new();
        while let Some(line) = lines.next().await {
            let Ok(line) = line else { continue };
            println!("{line}");
            if let Ok(msg) = ReplayMsg::from_str(&line) {
                messages.push(msg);
            }
        }

        let exit_status = self.wait().await?;

        Ok(ReplayLog {
            messages,
            exit_success: exit_status.success(),
        })
    }
}

async fn run_replay_internal(
    instance: &FactorioInstance,
    save_file: &mut SaveFile<impl Read + Seek>,
    replay_script: &str,
) -> Result<ReplayLog> {
    println!("Installing replay script");
    instance.add_save_with_installed_replay_script(save_file, replay_script)?;
    println!("Starting replay");
    let mut process = instance.spawn_replay(save_file.save_name())?;
    let result = process.collect_replay_log().await?;
    println!("Finished replay");
    Ok(result)
}

pub async fn run_replay_with_rules(
    install_dir: &FactorioInstallDir,
    save_file: &mut SaveFile<impl Read + Seek>,
    rules: &Rules,
) -> Result<ReplayRunResult> {
    let version = save_file.get_factorio_version()?;
    let mut instance = install_dir.get_or_download_factorio(version).await?;

    println!("Performing pre-run checks");
    instance.delete_saves_dir()?;
    instance.add_save_with_installed_replay_script(save_file, "")?;

    let mod_versions = instance
        .get_mod_versions(save_file.save_name())
        .await
        .context("Failed to get mod versions")?;

    if let Err(err) = check_expected_mods(&rules.expected_mods, &mod_versions) {
        return Ok(ReplayRunResult::PreRunCheckFailed {
            cause: err.to_string(),
        });
    }

    println!("Pre-run checks passed, running replay");
    let replay_script = rules.checks.to_string();
    let replay_log = run_replay_internal(&instance, save_file, &replay_script).await?;

    Ok(ReplayRunResult::Success(replay_log))
}
