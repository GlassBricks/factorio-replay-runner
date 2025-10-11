use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::watch;

use crate::process_manager::ProcessManager;

pub struct ShutdownCoordinator {
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    process_manager: Arc<ProcessManager>,
}

impl ShutdownCoordinator {
    pub fn new(process_manager: Arc<ProcessManager>) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            shutdown_tx,
            shutdown_rx,
            process_manager,
        }
    }

    pub fn setup_handlers(&self) -> Result<()> {
        use tokio::signal;

        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
            .context("Failed to register SIGINT handler")?;
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .context("Failed to register SIGTERM handler")?;

        let shutdown_tx = self.shutdown_tx.clone();
        let process_manager = self.process_manager.clone();

        tokio::spawn(async move {
            tokio::select! {
                _ = sigint.recv() => {
                    log::info!("Received SIGINT, cleaning up processes...");
                }
                _ = sigterm.recv() => {
                    log::info!("Received SIGTERM, cleaning up processes...");
                }
            }

            process_manager.kill_all();
            let _ = shutdown_tx.send(true);
        });

        Ok(())
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.shutdown_rx.clone()
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new(Arc::new(ProcessManager::new()))
    }
}
