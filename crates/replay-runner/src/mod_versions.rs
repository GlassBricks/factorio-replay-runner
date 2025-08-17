use anyhow::{Context, Result};
use log::trace;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

use crate::{factorio_install_dir::VersionStr, factorio_instance::FactorioInstance};

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
    fn read_mod_list(&self) -> Result<Vec<ModOption>> {
        let path = self.install_dir().join("mods/mod-list.json");
        let content = fs::read_to_string(&path)?;
        let mod_list = serde_yaml::from_str::<ModList>(&content)?;
        Ok(mod_list.mods)
    }

    pub async fn get_mod_versions(&mut self, save_name: &str) -> Result<ModVersions> {
        self.get_output(&["--sync-mods", save_name]).await?;

        trace!("Synced mods with command");

        let mod_versions = self
            .read_mod_list()
            .with_context(|| "Reading mod-list.json")?
            .into_iter()
            .filter(|mod_option| mod_option.enabled)
            .filter_map(|mod_option| {
                Some((
                    mod_option.name,
                    mod_option
                        .version
                        .and_then(|version| VersionStr::try_from(version).ok()),
                ))
            })
            .collect::<HashMap<_, _>>();

        Ok(mod_versions)
    }
}
