use anyhow::{Context, Result};
use std::sync::Arc;

use crate::process_manager::ProcessManager;

pub struct ShutdownCoordinator {
    process_manager: Arc<ProcessManager>,
}

impl ShutdownCoordinator {
    pub fn new(process_manager: Arc<ProcessManager>) -> Self {
        Self {
            process_manager,
        }
    }

    pub fn setup_handlers(&self) -> Result<()> {
        use tokio::signal;

        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
            .context("Failed to register SIGINT handler")?;
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .context("Failed to register SIGTERM handler")?;

        let process_manager = self.process_manager.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = sigint.recv() => {
                        log::info!("Received SIGINT, shutting down...");
                        process_manager.sig_int_all();
                        std::process::exit(130);
                    }
                    _ = sigterm.recv() => {
                        log::info!("Received SIGTERM, shutting down...");
                        process_manager.sig_int_all();
                        std::process::exit(143);
                    }
                }
            }
        });

        Ok(())
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new(Arc::new(ProcessManager::new()))
    }
}
