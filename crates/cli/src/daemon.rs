use anyhow::{Context, Result};
use factorio_manager::shutdown::ShutdownCoordinator;
use log::info;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Notify;

use crate::config::{DaemonConfig, GameConfig};
use crate::database::connection::Database;
use crate::speedrun_api::{SpeedrunClient, SpeedrunOps};

mod poller;
mod processor;

pub use poller::{poll_speedrun_com, poll_speedrun_com_loop};
pub use processor::{ProcessResult, find_run_to_process, process_runs_loop};

pub async fn run_daemon(
    config: DaemonConfig,
    game_configs: HashMap<String, GameConfig>,
    coordinator: ShutdownCoordinator,
) -> Result<()> {
    info!("Starting daemon with config: {:?}", config);
    info!("Monitoring {} game(s)", game_configs.len());

    let db = Database::new(&config.database_path)
        .await
        .context("Failed to initialize database")?;

    let client = SpeedrunClient::new()?;
    let speedrun_ops = SpeedrunOps::new(&client);

    std::fs::create_dir_all(&config.install_dir)?;
    std::fs::create_dir_all(&config.output_dir)?;

    let work_notify = Arc::new(Notify::new());

    info!("Daemon started successfully");

    let poller_task = poll_speedrun_com_loop(
        db.clone(),
        config.clone(),
        game_configs.clone(),
        speedrun_ops,
        work_notify.clone(),
        coordinator.subscribe(),
    );

    let processor_task = process_runs_loop(
        db,
        game_configs,
        client,
        config.install_dir.clone(),
        config.output_dir.clone(),
        work_notify,
        coordinator.subscribe(),
    );

    let (poller_result, processor_result) = tokio::join!(poller_task, processor_task);

    poller_result.or(processor_result)?;

    info!("Daemon shutting down");
    Ok(())
}
