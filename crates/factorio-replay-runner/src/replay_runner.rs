use std::io::{Read, Seek};

use anyhow::{Context, Result};
use async_std::io::BufReadExt;

use crate::{
    factorio_installation::{FactorioInstallation, FactorioProcess},
    replay_script,
    save_file::SaveFile,
};

impl FactorioProcess {
    pub async fn collect_lines_with_prefix(
        &mut self,
        prefix: &str,
        log_to_stdout: bool,
    ) -> Result<Vec<String>> {
        let mut stdout = self.stdout_reader()?;
        let mut buf = String::with_capacity(256);
        let mut res = Vec::new();
        loop {
            let line = stdout.read_line(&mut buf).await;
            match line {
                Ok(0) => break,
                Ok(_) => {
                    if log_to_stdout {
                        print!("{}", buf);
                    }
                    if buf.starts_with(prefix) {
                        res.push(buf[prefix.len()..].trim_end().to_string());
                    }
                }
                Err(e) => {
                    eprintln!("Error reading line: {}", e);
                    continue;
                }
            }
            buf.clear();
        }
        Ok(res)
    }
}

const DEFAULT_MSG_PREFIX: &str = "REPLAY_SCRIPT:";
impl FactorioInstallation {
    fn add_save_with_installed_replay_script(
        &self,
        save_file: &mut SaveFile<impl Read + Seek>,
    ) -> Result<()> {
        let mut out_file = self
            .create_save_file(save_file.save_name())
            .context("Failed to create save file")?;
        save_file
            .install_replay_script_to(&mut out_file, replay_script::REPLAY_SCRIPT_CONTROL_LUA)?;
        Ok(())
    }

    fn spawn_replay(&self, save_name: &str) -> Result<FactorioProcess> {
        self.spawn(&["--run-replay", save_name])
    }

    async fn run_replay(&self, save_name: &str) -> Result<Vec<String>> {
        self.spawn_replay(save_name)?
            .collect_lines_with_prefix(DEFAULT_MSG_PREFIX, true)
            .await
    }

    pub async fn setup_and_run_replay(
        &self,
        save: &mut SaveFile<impl Read + Seek>,
    ) -> Result<Vec<String>> {
        self.add_save_with_installed_replay_script(save)?;
        self.run_replay(save.save_name()).await
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::factorio_installation::FactorioInstallation;
    use crate::save_file::SaveFile;

    #[async_std::test]
    async fn test_add_modified_save() -> Result<()> {
        let factorio_installation = FactorioInstallation::get_test_installation().await;
        let mut save_file = SaveFile::get_test_save_file()?;
        let old_control_lua_contents = save_file.get_control_lua_contents()?.to_string();
        factorio_installation.add_save_with_installed_replay_script(&mut save_file)?;

        let mut written_save_file = factorio_installation.read_save_file(save_file.save_name())?;

        assert_eq!(written_save_file.save_name(), save_file.save_name());
        let control_lua_contents = written_save_file.get_control_lua_contents()?;

        anyhow::ensure!(
            control_lua_contents.starts_with(&old_control_lua_contents),
            "Must contain old control lua contents as start"
        );
        anyhow::ensure!(
            control_lua_contents.ends_with(replay_script::REPLAY_SCRIPT_CONTROL_LUA),
            "Must contain replay script control lua contents as end"
        );

        Ok(())
    }

    #[async_std::test]
    async fn test_spawn_replay() -> Result<()> {
        let factorio = FactorioInstallation::get_test_installation().await;
        let mut save = SaveFile::get_test_save_file()?;

        let result = factorio.setup_and_run_replay(&mut save).await?;
        dbg!(&result);
        Ok(())
    }
}
