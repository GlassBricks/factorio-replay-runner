use crate::expected_mods::ExpectedMods;
use replay_script::ReplayScripts;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize, Serialize)]
pub struct SrcRunRules {
    #[serde(flatten)]
    pub games: HashMap<String, GameRules>,
}

#[derive(Deserialize, Serialize)]
pub struct GameRules {
    pub expected_mods: ExpectedMods,
    pub categories: HashMap<String, CategoryRules>,
}

/// The same as RunRules for now
#[derive(Deserialize, Serialize)]
pub struct CategoryRules {
    #[serde(flatten)]
    pub run_rules: RunRules,
}

#[derive(Deserialize, Serialize)]
pub struct RunRules {
    #[serde(rename = "expected_mods")]
    pub expected_mods_override: Option<ExpectedMods>,
    #[serde(flatten)]
    pub replay_checks: ReplayScripts,
}
