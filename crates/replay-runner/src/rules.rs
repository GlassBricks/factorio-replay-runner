use crate::expected_mods::ExpectedMods;
use replay_script::ReplayScripts;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize, Serialize)]
pub struct SrcRunRules {
    games: HashMap<String, GameRules>,
}

#[derive(Deserialize, Serialize)]
pub struct GameRules {
    categories: HashMap<String, CategoryRules>,
}

/// The same as RunRules for now
#[derive(Deserialize, Serialize)]
pub struct CategoryRules {
    #[serde(flatten)]
    pub run_rules: RunRules,
}

#[derive(Deserialize, Serialize)]
pub struct RunRules {
    pub expected_mods: ExpectedMods,
    pub replay_checks: ReplayScripts,
}
