use std::io::{Read, Seek};

use anyhow::{Context, Result};

use crate::{factorio_installation::FactorioInstallation, replay_script, save_file::SaveFile};

impl FactorioInstallation {
    pub fn add_modified_save(&self, save_file: &mut SaveFile<impl Read + Seek>) -> Result<()> {
        let mut out_file = self
            .create_save_file(save_file.save_name())
            .context("Failed to create save file")?;
        save_file.write_with_modified_control_to(
            &mut out_file,
            replay_script::REPLAY_SCRIPT_CONTROL_LUA,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factorio_installation::FactorioInstallation;
    use crate::save_file::SaveFile;

    #[async_std::test]
    async fn test_add_modified_save() -> Result<()> {
        let factorio_installation = FactorioInstallation::get_test_install().await;
        let mut save_file = SaveFile::get_test_save_file()?;
        factorio_installation
            .add_modified_save(&mut save_file)
            .unwrap();

        let mut written_save_file = factorio_installation.read_save_file(save_file.save_name())?;

        assert_eq!(written_save_file.save_name(), save_file.save_name());
        assert_eq!(
            written_save_file.control_lua_contents()?,
            replay_script::REPLAY_SCRIPT_CONTROL_LUA
        );

        Ok(())
    }
}
