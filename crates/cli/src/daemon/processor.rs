use anyhow::{Context, Result};
use log::{error, info};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Notify, watch};

use crate::config::{GameConfig, SrcRunRules};
use crate::database::connection::Database;
use crate::database::types::Run;
use crate::run_processing::download_and_run_replay;
use crate::speedrun_api::SpeedrunClient;

#[derive(Debug)]
pub enum ProcessResult {
    Processed,
    NoWork,
}

pub async fn process_runs_loop(
    db: Database,
    game_configs: HashMap<String, GameConfig>,
    client: SpeedrunClient,
    install_dir: PathBuf,
    output_dir: PathBuf,
    work_notify: Arc<Notify>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    info!("Starting run processor (event-driven)");

    loop {
        loop {
            match find_run_to_process(&db, &game_configs, &client, &install_dir, &output_dir).await {
                Ok(ProcessResult::Processed) => continue,
                Ok(ProcessResult::NoWork) => break,
                Err(e) => {
                    error!("Run processing iteration failed: {:#}", e);
                    continue;
                }
            }
        }

        info!("No work available - run processor sleeping");

        tokio::select! {
            _ = work_notify.notified() => {
                info!("Run processor woken - checking for work");
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("Run processor shutting down");
                    return Ok(());
                }
            }
        }
    }
}

pub async fn find_run_to_process(
    db: &Database,
    game_configs: &HashMap<String, GameConfig>,
    client: &SpeedrunClient,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ProcessResult> {
    let allowed_game_categories = extract_game_category_pairs(game_configs);

    if allowed_game_categories.is_empty() {
        return Ok(ProcessResult::NoWork);
    }

    let run = match db.get_next_discovered_run(&allowed_game_categories).await? {
        Some(run) => run,
        None => return Ok(ProcessResult::NoWork),
    };

    let src_rules = SrcRunRules {
        games: game_configs.clone(),
    };

    let (run_rules, expected_mods) = src_rules
        .resolve_rules(&run.game_id, &run.category_id)
        .context("Failed to resolve rules for run")?;

    process_run(db, run, client, run_rules, expected_mods, install_dir, output_dir).await?;
    Ok(ProcessResult::Processed)
}

fn extract_game_category_pairs(
    game_configs: &HashMap<String, GameConfig>,
) -> Vec<(String, String)> {
    game_configs
        .iter()
        .flat_map(|(game_id, config)| {
            config
                .categories
                .keys()
                .map(|cat_id| (game_id.clone(), cat_id.clone()))
        })
        .collect()
}

async fn process_run(
    db: &Database,
    run: Run,
    client: &SpeedrunClient,
    run_rules: &crate::config::RunRules,
    expected_mods: &factorio_manager::expected_mods::ExpectedMods,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    let run_id = run.run_id.clone();

    db.mark_run_processing(&run_id)
        .await
        .context("Failed to mark run as processing")?;

    info!("Processing run: {}", run_id);

    let result = download_and_run_replay(
        client,
        &run.run_id,
        run_rules,
        expected_mods,
        install_dir,
        output_dir,
    )
    .await;

    db.process_replay_result(&run_id, result).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::connection::Database;
    use crate::database::types::NewRun;

    #[tokio::test]
    async fn test_poll_runs_no_discovered_runs() {
        let db = Database::in_memory().await.unwrap();
        let game_configs = HashMap::new();
        let client = SpeedrunClient::new().unwrap();
        let install_dir = std::path::PathBuf::from("/tmp/test");
        let output_dir = std::path::PathBuf::from("/tmp/test_output");

        let result = find_run_to_process(&db, &game_configs, &client, &install_dir, &output_dir).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ProcessResult::NoWork));
    }

    #[tokio::test]
    async fn test_poll_runs_missing_config() {
        let db = Database::in_memory().await.unwrap();
        let game_configs = HashMap::new();
        let client = SpeedrunClient::new().unwrap();
        let install_dir = std::path::PathBuf::from("/tmp/test");
        let output_dir = std::path::PathBuf::from("/tmp/test_output");

        let new_run = NewRun::new(
            "run123",
            "game1",
            "cat1",
            "2024-01-01T00:00:00Z".parse().unwrap(),
        );
        db.insert_run(new_run).await.unwrap();

        let result = find_run_to_process(&db, &game_configs, &client, &install_dir, &output_dir).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ProcessResult::NoWork));
    }
}
