use std::io::{Read, Seek};

use anyhow::{Context, Result};

use crate::{factorio_installation::FactorioInstallation, replay_file::ReplayFile, replay_script};

impl FactorioInstallation {
    pub fn add_modified_save(&self, replay_file: &mut ReplayFile<impl Read + Seek>) -> Result<()> {
        let mut out_file = self
            .create_save_file(replay_file.save_name())
            .context("Failed to create save file")?;
        replay_file
            .write_with_replay_script_to(&mut out_file, replay_script::REPLAY_SCRIPT_CONTROL_LUA)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factorio_installation::FactorioInstallation;
    use crate::replay_file::ReplayFile;

    #[async_std::test]
    async fn test_add_modified_save() -> Result<()> {
        let factorio_installation = FactorioInstallation::test_installation().await;
        let mut replay_file = ReplayFile::get_test_replay_file()?;
        factorio_installation
            .add_modified_save(&mut replay_file)
            .unwrap();

        let mut written_replay_file =
            factorio_installation.read_save_file(replay_file.save_name())?;

        assert_eq!(written_replay_file.save_name(), replay_file.save_name());
        assert_eq!(
            written_replay_file.control_lua_contents()?,
            replay_script::REPLAY_SCRIPT_CONTROL_LUA
        );

        Ok(())
    }
}
