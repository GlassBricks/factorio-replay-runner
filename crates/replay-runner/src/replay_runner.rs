use anyhow::{Context, Result};
use async_std::io::BufReadExt;
use async_std::prelude::StreamExt;
use replay_script::ReplayMsg;
use std::io::{Read, Seek};
use std::str::FromStr;

use crate::factorio_install_dir::FactorioInstallDir;
use crate::factorio_instance::FactorioProcess;
use crate::{factorio_instance::FactorioInstance, save_file::SaveFile};

pub struct ReplayLog {
    pub messages: Vec<ReplayMsg>,
    pub exit_success: bool,
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

pub async fn run_replay(
    install_dir: &FactorioInstallDir,
    save_file: &mut SaveFile<impl Read + Seek>,
    replay_script: &str,
) -> Result<ReplayLog> {
    let version = save_file.get_factorio_version()?;
    let instance = install_dir.get_or_download_factorio(version).await?;
    instance.delete_saves_dir()?;
    println!("Installing replay script");
    instance.add_save_with_installed_replay_script(save_file, replay_script)?;
    println!("Starting replay");
    let mut process = instance.spawn_replay(save_file.save_name())?;
    let result = process.collect_replay_log().await?;
    println!("Finished replay");
    Ok(result)
}
