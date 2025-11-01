use anyhow::{Context, Result};
use log::{error, info};
use std::sync::Arc;
use tokio::sync::Notify;

use super::database::types::Run;
use super::run_processing::{RunProcessingContext, download_and_run_replay};

#[derive(Debug)]
pub enum ProcessResult {
    Processed,
    NoWork,
}

pub async fn process_runs_loop(ctx: RunProcessingContext, work_notify: Arc<Notify>) -> Result<()> {
    info!("Starting run processor");

    loop {
        match find_run_to_process(&ctx).await {
            Ok(ProcessResult::Processed) => {}
            Err(e) => {
                error!("Run processing iteration failed: {:#}", e);
            }
            Ok(ProcessResult::NoWork) => {
                info!("No more runs available - sleeping");
                work_notify.notified().await;
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
        .get_next_run_to_process(&allowed_game_categories)
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

    let game_category = ctx
        .speedrun_ops
        .format_game_category(&run.game_id, &run.category_id)
        .await;

    if run.retry_count > 0 {
        info!(
            "Processing run {} for {} (retry {}/{})",
            run.run_id, game_category, run.retry_count, ctx.retry_config.max_attempts
        );
    } else {
        info!("Processing run {} for {}", run.run_id, game_category);
    }

    let result = download_and_run_replay(
        &ctx.speedrun_ops.client,
        &run.run_id,
        run_rules,
        expected_mods,
        &ctx.install_dir,
        &ctx.output_dir,
    )
    .await;

    info!("Saving replay result");
    ctx.db
        .process_replay_result(&run.run_id, result, &ctx.retry_config)
        .await?;
    info!("Run {} finished successfully", run.run_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::config::SrcRunRules;
    use crate::daemon::database::connection::Database;
    use crate::daemon::database::types::NewRun;
    use crate::daemon::retry::RetryConfig;
    use crate::daemon::speedrun_api::{SpeedrunClient, SpeedrunOps};
    use std::collections::HashMap;
    use std::path::PathBuf;

    async fn create_test_ctx() -> RunProcessingContext {
        let db = Database::in_memory().await.unwrap();
        let client = SpeedrunClient::new().unwrap();
        let speedrun_ops = SpeedrunOps::new(&client);
        let src_rules = SrcRunRules {
            games: HashMap::new(),
        };
        RunProcessingContext {
            db,
            speedrun_ops,
            src_rules,
            install_dir: PathBuf::from("/tmp/test"),
            output_dir: PathBuf::from("/tmp/test_output"),
            retry_config: RetryConfig::default(),
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

    #[tokio::test]
    async fn test_process_run_logs_initial_vs_retry() {
        let ctx = create_test_ctx().await;

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run_logging", "game1", "cat1", submitted_date);
        ctx.db.insert_run(new_run).await.unwrap();

        let run = ctx.db.get_run("run_logging").await.unwrap().unwrap();
        assert_eq!(run.retry_count, 0);

        let next_retry_at = chrono::Utc::now() - chrono::Duration::hours(1);
        ctx.db
            .mark_run_error("run_logging", "test error")
            .await
            .unwrap();
        ctx.db
            .schedule_retry("run_logging", 2, "retryable", next_retry_at)
            .await
            .unwrap();

        let run_with_retries = ctx.db.get_run("run_logging").await.unwrap().unwrap();
        assert_eq!(run_with_retries.retry_count, 2);
    }
}
