use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use log::{error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Notify, watch};

use crate::config::{DaemonConfig, GameConfig};
use crate::database::connection::Database;
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
    let cutoff_date = DateTime::parse_from_rfc3339(&config.cutoff_date)?.with_timezone(&Utc);

    for (game_id, game_config) in game_configs {
        for category_id in game_config.categories.keys() {
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
    let latest_submitted_date = db
        .get_latest_submitted_date(game_id, category_id)
        .await?
        .unwrap_or(cutoff_date);

    let new_runs = poll_game_category(game_id, category_id, &latest_submitted_date)
        .await
        .context("Failed to poll game category from API")?;

    let discovered_count = new_runs.len();

    for new_run in &new_runs {
        db.insert_run(new_run.clone())
            .await
            .context("Failed to insert run into database")?;
    }

    if discovered_count > 0 {
        info!(
            "Discovered {} new run(s) for game={} category={}",
            discovered_count, game_id, category_id
        );
        work_notify.notify_one();
    }

    Ok(())
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

    #[tokio::test]
    async fn test_poll_with_no_game_configs() {
        let db = Database::in_memory().await.unwrap();
        let game_configs = HashMap::new();
        let config = DaemonConfig {
            game_rules_file: std::path::PathBuf::from("./speedrun_rules.yaml"),
            install_dir: std::path::PathBuf::from("./factorio_installs"),
            output_dir: std::path::PathBuf::from("./daemon_runs"),
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
