use log::trace;
use serde::Deserialize;
use std::fs;
use std::{collections::HashMap, path::Path};

use crate::{
    error::FactorioError, factorio_install_dir::VersionStr, factorio_instance::FactorioInstance,
};

#[derive(Deserialize, Debug)]
struct ModList {
    mods: Vec<ModOption>,
}

#[derive(Deserialize, Debug)]
struct ModOption {
    name: String,
    enabled: bool,
    version: Option<String>,
}

pub type ModVersions = HashMap<String, Option<VersionStr>>;

impl FactorioInstance {
    fn read_mod_list(&self) -> Result<Vec<ModOption>, FactorioError> {
        let path = self.install_dir().join("mods/mod-list.json");
        let content =
            fs::read_to_string(&path).map_err(|e| FactorioError::ModInfoReadFailed(e.into()))?;
        let mod_list: ModList = serde_yaml::from_str(&content)
            .map_err(|e| FactorioError::ModInfoReadFailed(e.into()))?;
        Ok(mod_list.mods)
    }

    pub async fn get_mod_versions(
        &mut self,
        save_path: &Path,
    ) -> Result<ModVersions, FactorioError> {
        self.run_and_get_output(&["--sync-mods", save_path.to_str().unwrap()])
            .await?;

        trace!("Synced mods with command");

        let mod_versions = self
            .read_mod_list()?
            .into_iter()
            .filter(|mod_option| mod_option.enabled)
            .map(|mod_option| {
                (
                    mod_option.name,
                    mod_option
                        .version
                        .and_then(|version| VersionStr::try_from(version).ok()),
                )
            })
            .collect::<HashMap<_, _>>();

        Ok(mod_versions)
    }
}
