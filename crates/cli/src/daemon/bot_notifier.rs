use crate::daemon::database::{connection::Database, types::RunStatus};
use log::{info, warn};
use reqwest::Client;
use std::time::Duration;

use super::config::BotNotifierConfig;

pub struct BotNotifier {
    client: Client,
    bot_url: String,
    auth_token: String,
    db: Database,
    retry_interval: Duration,
}

impl BotNotifier {
    pub fn new(config: &BotNotifierConfig, db: Database) -> Self {
        Self {
            client: Client::new(),
            bot_url: config.bot_url.clone(),
            auth_token: config.auth_token.clone(),
            db,
            retry_interval: Duration::from_secs(config.retry_interval_seconds),
        }
    }

    pub async fn report_status(&self, run_id: &str, status: &str, message: Option<&str>) {
        if let Err(e) = self.db.set_bot_notified(run_id, false).await {
            warn!("Failed to set bot_notified=false for run {}: {}", run_id, e);
        }

        let url = format!("{}/api/runs/{}/status", self.bot_url, run_id);
        let body = serde_json::json!({ "status": status, "message": message });

        let result = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .json(&body)
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                info!("Bot notified for run {} with status {}", run_id, status);
                if let Err(e) = self.db.set_bot_notified(run_id, true).await {
                    warn!("Failed to set bot_notified=true for run {}: {}", run_id, e);
                }
            }
            Ok(resp) => {
                warn!(
                    "Bot notification failed for run {} (HTTP {})",
                    run_id,
                    resp.status()
                );
            }
            Err(e) => {
                warn!("Bot notification error for run {}: {}", run_id, e);
            }
        }
    }

    pub async fn retry_once(&self) {
        let runs = match self.db.get_unnotified_runs().await {
            Ok(runs) => runs,
            Err(e) => {
                warn!("Failed to query unnotified runs: {}", e);
                return;
            }
        };

        for run in runs {
            let status = run_status_to_bot_status(&run.status);
            let message = run.error_message.as_deref();
            self.report_status(&run.run_id, status, message).await;
        }
    }

    pub async fn notification_retry_loop(&self) -> ! {
        loop {
            tokio::time::sleep(self.retry_interval).await;
            self.retry_once().await;
        }
    }
}

fn run_status_to_bot_status(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Discovered => "pending",
        RunStatus::Processing => "running",
        RunStatus::Passed => "passed",
        RunStatus::NeedsReview => "needs_review",
        RunStatus::Failed => "failed",
        RunStatus::Error => "error",
    }
}
