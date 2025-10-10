use factorio_manager::expected_mods::ExpectedMods;
use replay_script::ReplayScripts;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonConfig {
    pub poll_interval_seconds: u64,
    pub database_path: PathBuf,
    pub cutoff_date: String,
}

#[derive(Deserialize, Serialize)]
pub struct SrcRunRules {
    pub games: HashMap<String, GameConfig>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub expected_mods: ExpectedMods,
    pub categories: HashMap<String, CategoryConfig>,
}

#[derive(Deserialize, Serialize)]
pub struct CategoryConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(flatten)]
    pub run_rules: RunRules,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunRules {
    #[serde(rename = "expected_mods")]
    pub expected_mods_override: Option<ExpectedMods>,
    #[serde(flatten)]
    pub replay_scripts: ReplayScripts,
}
