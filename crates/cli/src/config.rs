use factorio_manager::expected_mods::ExpectedMods;
use replay_script::ReplayScripts;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunRules {
    #[serde(rename = "expected_mods")]
    pub expected_mods_override: Option<ExpectedMods>,
    #[serde(flatten)]
    pub replay_scripts: ReplayScripts,
}
