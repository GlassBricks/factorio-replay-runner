use anyhow::{Context, Result};
use log::{error, info};
use std::sync::Arc;
use tokio::sync::{Notify, watch};

use crate::database::types::Run;
use crate::run_processing::{RunProcessingContext, download_and_run_replay};

#[derive(Debug)]
pub enum ProcessResult {
    Processed,
    NoWork,
}

pub async fn process_runs_loop(
    ctx: RunProcessingContext,
    work_notify: Arc<Notify>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    info!("Starting run processor (event-driven)");

    loop {
        match find_run_to_process(&ctx).await {
            Ok(ProcessResult::Processed) => {}
            Err(e) => {
                error!("Run processing iteration failed: {:#}", e);
            }
            Ok(ProcessResult::NoWork) => {
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
    }
}

pub async fn find_run_to_process(ctx: &RunProcessingContext) -> Result<ProcessResult> {
    let allowed_game_categories: Vec<(String, String)> = ctx
        .src_rules
        .games
        .iter()
        .flat_map(|(game_id, config)| {
            config
                .categories
                .keys()
                .map(|cat_id| (game_id.clone(), cat_id.clone()))
        })
        .collect();

    let Some(run) = ctx
        .db
        .get_next_discovered_run(&allowed_game_categories)
        .await?
    else {
        return Ok(ProcessResult::NoWork);
    };
    process_run(ctx, run).await?;
    Ok(ProcessResult::Processed)
}

async fn process_run(ctx: &RunProcessingContext, run: Run) -> Result<()> {
    let (run_rules, expected_mods) = ctx
        .src_rules
        .resolve_rules(&run.game_id, &run.category_id)
        .context("Failed to resolve rules for run")?;

    ctx.db
        .mark_run_processing(&run.run_id)
        .await
        .context("Failed to mark run as processing")?;

    info!("Processing run: {}", run.run_id);

    let result = download_and_run_replay(
        ctx.client(),
        &run.run_id,
        run_rules,
        expected_mods,
        &ctx.install_dir,
        &ctx.output_dir,
    )
    .await;

    ctx.db.process_replay_result(&run.run_id, result).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SrcRunRules;
    use crate::database::connection::Database;
    use crate::database::types::NewRun;
    use crate::speedrun_api::SpeedrunClient;
    use std::collections::HashMap;
    use std::path::PathBuf;

    async fn create_test_ctx() -> RunProcessingContext {
        let db = Database::in_memory().await.unwrap();
        let client = SpeedrunClient::new().unwrap();
        let speedrun_ops = crate::speedrun_api::SpeedrunOps::new(&client);
        let src_rules = SrcRunRules {
            games: HashMap::new(),
        };
        RunProcessingContext {
            db,
            speedrun_ops,
            src_rules,
            install_dir: PathBuf::from("/tmp/test"),
            output_dir: PathBuf::from("/tmp/test_output"),
        }
    }

    #[tokio::test]
    async fn test_poll_runs_no_discovered_runs() {
        let ctx = create_test_ctx().await;

        let result = find_run_to_process(&ctx).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ProcessResult::NoWork));
    }

    #[tokio::test]
    async fn test_poll_runs_missing_config() {
        let ctx = create_test_ctx().await;
        let new_run = NewRun::new(
            "run123",
            "game1",
            "cat1",
            "2024-01-01T00:00:00Z".parse().unwrap(),
        );
        ctx.db.insert_run(new_run).await.unwrap();

        let result = find_run_to_process(&ctx).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ProcessResult::NoWork));
    }
}
