use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use log::{error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Notify, watch};

use crate::config::{DaemonConfig, GameConfig};
use crate::database::connection::Database;
use crate::database::operations::{get_poll_state, insert_run, upsert_poll_state};
use crate::database::types::PollState;
use crate::run_processing::poll_game_category;

pub async fn poll_speedrun_com_loop(
    db: Database,
    config: DaemonConfig,
    game_configs: HashMap<String, GameConfig>,
    work_notify: Arc<Notify>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let poll_interval = std::time::Duration::from_secs(config.poll_interval_seconds);

    info!(
        "Starting speedrun.com poller (interval: {}s)",
        config.poll_interval_seconds
    );

    loop {
        if let Err(e) = poll_speedrun_com(&db, &config, &game_configs, &work_notify).await {
            error!("Speedrun.com poll iteration failed: {:#}", e);
        }

        if interruptible_sleep(poll_interval, &mut shutdown_rx)
            .await
            .is_err()
        {
            info!("Speedrun.com poller shutting down");
            return Ok(());
        }
    }
}

async fn poll_speedrun_com(
    db: &Database,
    config: &DaemonConfig,
    game_configs: &HashMap<String, GameConfig>,
    work_notify: &Notify,
) -> Result<()> {
    let now = Utc::now();
    let poll_interval = Duration::seconds(config.poll_interval_seconds as i64);
    let cutoff_date = DateTime::parse_from_rfc3339(&config.cutoff_date)?.with_timezone(&Utc);

    for (game_id, game_config) in game_configs {
        for category_id in game_config.categories.keys() {
            let poll_state = get_poll_state(db, game_id, category_id).await?;

            if !should_poll(poll_state.as_ref(), poll_interval, now) {
                continue;
            }

            if let Err(e) = poll_category(db, game_id, category_id, cutoff_date, work_notify).await
            {
                error!(
                    "Failed to poll game={} category={}: {:#}",
                    game_id, category_id, e
                );
            }
        }
    }

    Ok(())
}

async fn poll_category(
    db: &Database,
    game_id: &str,
    category_id: &str,
    cutoff_date: DateTime<Utc>,
    work_notify: &Notify,
) -> Result<()> {
    let poll_state = get_poll_state(db, game_id, category_id).await?;
    let now = Utc::now();

    let last_poll_time = poll_state
        .as_ref()
        .map(|state| state.last_poll_time)
        .unwrap_or(cutoff_date);

    let new_runs = poll_game_category(
        game_id,
        category_id,
        &last_poll_time.to_rfc3339(),
        &cutoff_date.to_rfc3339(),
    )
    .await
    .context("Failed to poll game category from API")?;

    let discovered_count = new_runs.len();

    for new_run in &new_runs {
        insert_run(db, new_run.clone())
            .await
            .context("Failed to insert run into database")?;
    }

    upsert_poll_state(db, game_id, category_id, now, now)
        .await
        .context("Failed to update poll state")?;

    if discovered_count > 0 {
        info!(
            "Discovered {} new run(s) for game={} category={}",
            discovered_count, game_id, category_id
        );
        work_notify.notify_one();
    }

    Ok(())
}

fn should_poll(
    poll_state: Option<&PollState>,
    poll_interval: Duration,
    now: DateTime<Utc>,
) -> bool {
    poll_state
        .map(|state| now >= state.last_poll_time + poll_interval)
        .unwrap_or(true)
}

async fn interruptible_sleep(
    duration: std::time::Duration,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<()> {
    tokio::select! {
        _ = tokio::time::sleep(duration) => Ok(()),
        _ = shutdown_rx.changed() => {
            if *shutdown_rx.borrow() {
                Err(anyhow::anyhow!("Shutdown signal received"))
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::connection::Database;

    #[test]
    fn test_should_poll_no_previous_state() {
        let poll_interval = Duration::seconds(3600);
        let now = Utc::now();

        assert!(should_poll(None, poll_interval, now));
    }

    #[test]
    fn test_should_poll_interval_not_elapsed() {
        let poll_interval = Duration::seconds(3600);
        let now = Utc::now();
        let last_poll = now - Duration::seconds(1800);

        let poll_state = PollState {
            game_id: "game1".to_string(),
            category_id: "cat1".to_string(),
            last_poll_time: last_poll,
            last_poll_success: last_poll,
        };

        assert!(!should_poll(Some(&poll_state), poll_interval, now));
    }

    #[test]
    fn test_should_poll_interval_elapsed() {
        let poll_interval = Duration::seconds(3600);
        let now = Utc::now();
        let last_poll = now - Duration::seconds(3601);

        let poll_state = PollState {
            game_id: "game1".to_string(),
            category_id: "cat1".to_string(),
            last_poll_time: last_poll,
            last_poll_success: last_poll,
        };

        assert!(should_poll(Some(&poll_state), poll_interval, now));
    }

    #[test]
    fn test_should_poll_exactly_at_interval() {
        let poll_interval = Duration::seconds(3600);
        let now = Utc::now();
        let last_poll = now - Duration::seconds(3600);

        let poll_state = PollState {
            game_id: "game1".to_string(),
            category_id: "cat1".to_string(),
            last_poll_time: last_poll,
            last_poll_success: last_poll,
        };

        assert!(should_poll(Some(&poll_state), poll_interval, now));
    }

    #[tokio::test]
    async fn test_poll_with_no_game_configs() {
        let db = Database::in_memory().await.unwrap();
        let game_configs = HashMap::new();
        let config = DaemonConfig {
            poll_interval_seconds: 3600,
            database_path: std::path::PathBuf::from(":memory:"),
            cutoff_date: "2024-01-01T00:00:00Z".to_string(),
        };
        let work_notify = Notify::new();

        let result = poll_speedrun_com(&db, &config, &game_configs, &work_notify).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_interruptible_sleep_completes() {
        let (_tx, mut rx) = watch::channel(false);
        let duration = std::time::Duration::from_millis(10);

        let start = std::time::Instant::now();
        let result = interruptible_sleep(duration, &mut rx).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert!(elapsed >= duration);
    }

    #[tokio::test]
    async fn test_interruptible_sleep_interrupted() {
        let (tx, mut rx) = watch::channel(false);
        let duration = std::time::Duration::from_secs(10);

        let handle = tokio::spawn(async move { interruptible_sleep(duration, &mut rx).await });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        tx.send(true).unwrap();

        let result = handle.await.unwrap();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Shutdown signal received");
    }
}
