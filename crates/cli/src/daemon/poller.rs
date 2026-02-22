use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use log::{error, info};
use std::sync::Arc;
use tokio::sync::Notify;

use crate::daemon::SpeedrunOps;
use crate::daemon::database::types::NewRun;
use crate::daemon::speedrun_api::{ApiError, RunsQuery};

use super::config::PollingConfig;
use super::run_processing::RunProcessingContext;

pub async fn poll_speedrun_com_loop(
    ctx: RunProcessingContext,
    config: PollingConfig,
    work_notify: Arc<Notify>,
) -> Result<()> {
    let poll_interval = std::time::Duration::from_secs(config.poll_interval_seconds);

    info!(
        "Starting speedrun.com poller (interval: {}s)",
        config.poll_interval_seconds
    );

    loop {
        if let Err(e) = poll_speedrun_com(&ctx, &config, &work_notify).await {
            error!("Speedrun.com poll iteration failed: {:#}", e);
        }

        tokio::time::sleep(poll_interval).await;
    }
}

pub async fn poll_speedrun_com(
    ctx: &RunProcessingContext,
    config: &PollingConfig,
    work_notify: &Notify,
) -> Result<()> {
    let cutoff_date = DateTime::parse_from_rfc3339(&config.cutoff_date)?.with_timezone(&Utc);

    for (game_id, game_config) in &ctx.src_rules.games {
        for category_id in game_config.categories.keys() {
            if let Err(e) = poll_category(ctx, game_id, category_id, cutoff_date, work_notify).await
            {
                let game_category = ctx
                    .speedrun_ops
                    .format_game_category(game_id, category_id)
                    .await;
                error!("Failed to poll {}: {:#}", game_category, e);
            }
        }
    }

    Ok(())
}

async fn poll_game_category(
    speedrun_ops: &SpeedrunOps,
    game_id: &str,
    category_id: &str,
    cutoff_date: &DateTime<Utc>,
) -> Result<Vec<NewRun>, ApiError> {
    info!(
        "Polling for new runs: game={}, category={}",
        speedrun_ops
            .get_game_name(game_id)
            .await
            .as_ref()
            .map_or(game_id, |name| name.as_str()),
        speedrun_ops
            .get_category_name(category_id)
            .await
            .as_ref()
            .map_or(category_id, |name| name.as_str())
    );

    let query = RunsQuery::new()
        .game(game_id)
        .category(category_id)
        .orderby("submitted")
        .direction("asc");

    let runs = speedrun_ops.client.stream_runs(&query).await?;

    let new_runs: Vec<NewRun> = runs
        .into_iter()
        .filter_map(|run| {
            let submitted_date = run.get_submitted_date().ok()?;
            (submitted_date > *cutoff_date)
                .then(|| NewRun::new(run.id, game_id, category_id, submitted_date))
        })
        .collect();

    info!("Found {} new runs", new_runs.len());
    Ok(new_runs)
}

async fn poll_category(
    ctx: &RunProcessingContext,
    game_id: &str,
    category_id: &str,
    cutoff_date: DateTime<Utc>,
    work_notify: &Notify,
) -> Result<()> {
    let latest_submitted_date = ctx
        .db
        .get_latest_submitted_date(game_id, category_id)
        .await?
        .unwrap_or(cutoff_date);

    let new_runs = poll_game_category(
        &ctx.speedrun_ops,
        game_id,
        category_id,
        &latest_submitted_date,
    )
    .await
    .context("Failed to poll game category from API")?;

    let discovered_count = new_runs.len();

    let notifier_configured = ctx.bot_notifier.is_some();
    for new_run in &new_runs {
        match ctx
            .db
            .insert_run(new_run.clone(), !notifier_configured)
            .await
        {
            Ok(()) => {
                if let Some(notifier) = &ctx.bot_notifier {
                    notifier
                        .report_status(&new_run.run_id, "pending", None)
                        .await;
                }
            }
            Err(e) => {
                error!("Failed to insert run into database: {:#}", e);
            }
        }
    }

    if discovered_count > 0 {
        let game_category = ctx
            .speedrun_ops
            .format_game_category(game_id, category_id)
            .await;
        info!(
            "Discovered {} new run(s) for {}",
            discovered_count, game_category
        );
        work_notify.notify_one();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::config::SrcRunRules;
    use crate::daemon::database::connection::Database;
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
            install_dir: PathBuf::from("./factorio_installs"),
            output_dir: PathBuf::from("./daemon_runs"),
            retry_config: RetryConfig::default(),
            bot_notifier: None,
        }
    }

    #[tokio::test]
    async fn test_poll_with_no_game_configs() {
        let ctx = create_test_ctx().await;
        let config = PollingConfig {
            poll_interval_seconds: 3600,
            cutoff_date: "2024-01-01T00:00:00Z".to_string(),
        };
        let work_notify = Notify::new();

        let result = poll_speedrun_com(&ctx, &config, &work_notify).await;

        assert!(result.is_ok());
    }
}
