use anyhow::{Context, Result};
use async_std::io::BufReadExt;
use async_std::prelude::StreamExt;
use replay_script::ReplayMsg;
use std::io::{Read, Seek};
use std::str::FromStr;

use crate::factorio_instance::FactorioProcess;
use crate::{factorio_instance::FactorioInstance, save_file::SaveFile};

pub struct ReplayLog {
    pub messages: Vec<ReplayMsg>,
}

impl FactorioProcess {
    pub async fn collect_replay_log(&mut self) -> Result<ReplayLog> {
        let mut lines = self.stdout_reader()?.lines();
        let mut messages = Vec::new();
        while let Some(line) = lines.next().await {
            let Ok(line) = line else { continue };
            if let Ok(msg) = ReplayMsg::from_str(&line) {
                messages.push(msg);
            }
        }

        Ok(ReplayLog { messages })
    }
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

    pub async fn install_and_run_replay(
        &self,
        save_file: &mut SaveFile<impl Read + Seek>,
        replay_script: &str,
    ) -> Result<ReplayLog> {
        self.add_save_with_installed_replay_script(save_file, replay_script)?;
        let mut process = self.spawn_replay(save_file.save_name())?;
        process.collect_replay_log().await
    }
}
