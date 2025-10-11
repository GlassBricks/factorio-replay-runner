use anyhow::{Context, Result};
use log::{error, info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Notify, watch};

use crate::config::{GameConfig, SrcRunRules};
use crate::database::connection::Database;
use crate::database::types::Run;
use crate::run_processing::download_and_run_replay;
use replay_script::MsgLevel;

#[derive(Debug)]
pub enum ProcessResult {
    Processed,
    NoWork,
}

pub async fn process_runs_loop(
    db: Database,
    game_configs: HashMap<String, GameConfig>,
    install_dir: PathBuf,
    output_dir: PathBuf,
    work_notify: Arc<Notify>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    info!("Starting run processor (event-driven)");

    loop {
        loop {
            match find_run_to_process(&db, &game_configs, &install_dir, &output_dir).await {
                Ok(ProcessResult::Processed) => continue,
                Ok(ProcessResult::NoWork) => break,
                Err(e) => {
                    error!("Run processing iteration failed: {:#}", e);
                    break;
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

async fn find_run_to_process(
    db: &Database,
    game_configs: &HashMap<String, GameConfig>,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ProcessResult> {
    let run = match db.get_next_discovered_run().await? {
        Some(run) => run,
        None => return Ok(ProcessResult::NoWork),
    };

    let src_rules = SrcRunRules {
        games: game_configs.clone(),
    };

    let (run_rules, expected_mods) = src_rules
        .resolve_rules(&run.game_id, &run.category_id)
        .context("Failed to resolve rules for run")?;

    process_run(db, run, run_rules, expected_mods, install_dir, output_dir).await?;
    Ok(ProcessResult::Processed)
}

async fn process_run(
    db: &Database,
    run: Run,
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
        &run.run_id,
        run_rules,
        expected_mods,
        install_dir,
        output_dir,
    )
    .await;

    match result {
        Ok(report) if report.exited_successfully => match report.max_msg_level {
            MsgLevel::Info => {
                db.mark_run_passed(&run_id).await?;
                info!("Run {} passed verification", run_id);
            }
            MsgLevel::Warn => {
                db.mark_run_needs_review(&run_id).await?;
                warn!("Run {} passed with warnings (needs review)", run_id);
            }
            MsgLevel::Error => {
                db.mark_run_failed(&run_id).await?;
                warn!("Run {} failed verification", run_id);
            }
        },
        Ok(_) => {
            let error_msg = "Replay did not exit successfully";
            db.mark_run_error(&run_id, error_msg).await?;
            error!("Run {} error: {}", run_id, error_msg);
        }
        Err(e) => {
            let error_msg = format!("Failed to process run: {:#}", e);
            db.mark_run_error(&run_id, &error_msg).await?;
            error!("Run {} error: {}", run_id, error_msg);
        }
    }
    Ok(())
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
        let install_dir = std::path::PathBuf::from("/tmp/test");
        let output_dir = std::path::PathBuf::from("/tmp/test_output");

        let result = find_run_to_process(&db, &game_configs, &install_dir, &output_dir).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ProcessResult::NoWork));
    }

    #[tokio::test]
    async fn test_poll_runs_missing_config() {
        let db = Database::in_memory().await.unwrap();
        let game_configs = HashMap::new();
        let install_dir = std::path::PathBuf::from("/tmp/test");
        let output_dir = std::path::PathBuf::from("/tmp/test_output");

        let new_run = NewRun::new(
            "run123",
            "game1",
            "cat1",
            "2024-01-01T00:00:00Z".parse().unwrap(),
        );
        db.insert_run(new_run).await.unwrap();

        let result = find_run_to_process(&db, &game_configs, &install_dir, &output_dir).await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("No configuration found")
                || error_msg.contains("Failed to resolve rules"),
            "Unexpected error message: {}",
            error_msg
        );
    }
}
