use anyhow::Result;
use factorio_manager::expected_mods::ExpectedMods;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

use crate::config::RunRules;
use crate::daemon::retry::RetryConfig;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PollingConfig {
    #[serde(default = "default_poll_interval_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_lookback_days")]
    pub lookback_days: u64,
}

fn default_lookback_days() -> u64 {
    30
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BotNotifierConfig {
    pub bot_url: String,
    pub auth_token: String,
    #[serde(default = "default_retry_interval_seconds")]
    pub retry_interval_seconds: u64,
}

fn default_retry_interval_seconds() -> u64 {
    1800
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonConfig {
    #[serde(default = "default_game_rules_file")]
    pub game_rules_file: PathBuf,
    #[serde(default = "default_install_dir")]
    pub install_dir: PathBuf,
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    #[serde(default = "default_database_path")]
    pub database_path: PathBuf,
    #[serde(default)]
    pub polling: PollingConfig,
    #[serde(default)]
    pub retry: RetryConfig,
    #[serde(default)]
    pub bot_notifier: Option<BotNotifierConfig>,
}

fn default_game_rules_file() -> PathBuf {
    PathBuf::from("./speedrun_rules.yaml")
}

fn default_install_dir() -> PathBuf {
    PathBuf::from("./factorio_installs")
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("./src_runs")
}

fn default_poll_interval_seconds() -> u64 {
    3600
}

fn default_database_path() -> PathBuf {
    PathBuf::from("run_verification.db")
}

#[derive(Clone, Deserialize, Serialize)]
pub struct SrcRunRules {
    pub games: HashMap<String, GameConfig>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameConfig {
    pub expected_mods: ExpectedMods,
    pub categories: HashMap<String, CategoryConfig>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct CategoryConfig {
    #[serde(flatten)]
    pub run_rules: RunRules,
}

impl SrcRunRules {
    pub fn resolve_rules(
        &self,
        game_id: &str,
        category_id: &str,
    ) -> Result<(&RunRules, &ExpectedMods)> {
        let game_config = self
            .games
            .get(game_id)
            .ok_or_else(|| anyhow::anyhow!("No configuration found for game={}", game_id))?;

        let category_config = game_config.categories.get(category_id).ok_or_else(|| {
            anyhow::anyhow!("No configuration found for category={}", category_id)
        })?;

        let run_rules = &category_config.run_rules;
        let expected_mods = run_rules
            .expected_mods_override
            .as_ref()
            .unwrap_or(&game_config.expected_mods);

        Ok((run_rules, expected_mods))
    }
}
