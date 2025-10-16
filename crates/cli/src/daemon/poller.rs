use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use log::{error, info};
use std::sync::Arc;
use tokio::sync::Notify;

use super::config::PollingConfig;
use super::run_processing::{RunProcessingContext, poll_game_category};

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

    let new_runs = poll_game_category(ctx.client(), game_id, category_id, &latest_submitted_date)
        .await
        .context("Failed to poll game category from API")?;

    let discovered_count = new_runs.len();

    for new_run in &new_runs {
        if let Err(e) = ctx.db.insert_run(new_run.clone()).await {
            error!("Failed to insert run into database: {:#}", e);
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
