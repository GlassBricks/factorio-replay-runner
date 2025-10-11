use anyhow::{Context, Result};
use log::info;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Notify, watch};

use crate::config::{DaemonConfig, GameConfig};
use crate::database::connection::Database;

mod poller;
mod processor;

pub use poller::poll_speedrun_com_loop;
pub use processor::process_runs_loop;

pub async fn run_daemon(
    config: DaemonConfig,
    game_configs: HashMap<String, GameConfig>,
    install_dir: PathBuf,
    output_dir: PathBuf,
) -> Result<()> {
    info!("Starting daemon with config: {:?}", config);
    info!("Monitoring {} game(s)", game_configs.len());

    let db = Database::new(&config.database_path)
        .await
        .context("Failed to initialize database")?;

    std::fs::create_dir_all(&install_dir)?;
    std::fs::create_dir_all(&output_dir)?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    setup_shutdown_handler(shutdown_tx)?;

    let work_notify = Arc::new(Notify::new());

    info!("Daemon started successfully");

    let poller_task = poll_speedrun_com_loop(
        db.clone(),
        config.clone(),
        game_configs.clone(),
        work_notify.clone(),
        shutdown_rx.clone(),
    );

    let processor_task = process_runs_loop(
        db,
        game_configs,
        install_dir,
        output_dir,
        work_notify,
        shutdown_rx,
    );

    let (poller_result, processor_result) = tokio::join!(poller_task, processor_task);

    poller_result.or(processor_result)?;

    info!("Daemon shutting down");
    Ok(())
}

fn setup_shutdown_handler(shutdown_tx: watch::Sender<bool>) -> Result<()> {
    use tokio::signal;

    let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
        .context("Failed to register SIGINT handler")?;
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
        .context("Failed to register SIGTERM handler")?;

    tokio::spawn(async move {
        tokio::select! {
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
        }
        let _ = shutdown_tx.send(true);
    });

    Ok(())
}
