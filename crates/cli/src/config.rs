use factorio_manager::expected_mods::ExpectedMods;
use replay_script::ReplayScripts;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonConfig {
    pub poll_interval_seconds: u64,
    pub database_path: PathBuf,
    pub speedrun_rules_path: PathBuf,
    pub cutoff_date: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: 300,
            database_path: PathBuf::from("./run_verification.db"),
            speedrun_rules_path: PathBuf::from("speedrun_rules.yaml"),
            cutoff_date: "2025-01-01".to_string(),
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct SrcRunRules {
    #[serde(flatten)]
    pub games: HashMap<String, GameRules>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameRules {
    pub expected_mods: ExpectedMods,
    pub categories: HashMap<String, CategoryRules>,
}

#[derive(Deserialize, Serialize)]
pub struct CategoryRules {
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
