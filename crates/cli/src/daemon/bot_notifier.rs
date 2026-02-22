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

pub fn run_status_to_bot_status(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Discovered => "pending",
        RunStatus::Processing => "running",
        RunStatus::Passed => "passed",
        RunStatus::NeedsReview => "needs_review",
        RunStatus::Failed => "failed",
        RunStatus::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::config::BotNotifierConfig;
    use crate::daemon::database::types::NewRun;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_config(bot_url: &str) -> BotNotifierConfig {
        BotNotifierConfig {
            bot_url: bot_url.to_string(),
            auth_token: "test-token".to_string(),
            retry_interval_seconds: 1800,
        }
    }

    async fn insert_test_run(db: &Database, run_id: &str, bot_notified: bool) {
        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new(run_id, "game1", "cat1", submitted_date);
        db.insert_run(new_run, bot_notified).await.unwrap();
    }

    #[tokio::test]
    async fn test_successful_notification_sets_bot_notified_true() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/runs/run123/status"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run123", true).await;

        let notifier = BotNotifier::new(&make_config(&mock_server.uri()), db.clone());
        notifier.report_status("run123", "passed", None).await;

        mock_server.verify().await;

        let run = db.get_run("run123").await.unwrap().unwrap();
        assert!(run.bot_notified);
    }

    #[tokio::test]
    async fn test_auth_header_format() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/runs/run123/status"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run123", true).await;

        let notifier = BotNotifier::new(&make_config(&mock_server.uri()), db.clone());
        notifier.report_status("run123", "passed", None).await;

        mock_server.verify().await;
    }

    #[tokio::test]
    async fn test_server_500_leaves_bot_notified_false() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/runs/run500/status"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run500", true).await;

        let notifier = BotNotifier::new(&make_config(&mock_server.uri()), db.clone());
        notifier.report_status("run500", "passed", None).await;

        let run = db.get_run("run500").await.unwrap().unwrap();
        assert!(!run.bot_notified);
    }

    #[tokio::test]
    async fn test_server_unreachable_leaves_bot_notified_false() {
        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_unreachable", true).await;

        let notifier = BotNotifier::new(&make_config("http://127.0.0.1:19999"), db.clone());
        notifier
            .report_status("run_unreachable", "passed", None)
            .await;

        let run = db.get_run("run_unreachable").await.unwrap().unwrap();
        assert!(!run.bot_notified);
    }

    #[tokio::test]
    async fn test_retry_once_notifies_unnotified_runs() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/runs/run_retry/status"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_retry", false).await;

        let notifier = BotNotifier::new(&make_config(&mock_server.uri()), db.clone());
        notifier.retry_once().await;

        mock_server.verify().await;

        let run = db.get_run("run_retry").await.unwrap().unwrap();
        assert!(run.bot_notified);
    }

    #[tokio::test]
    async fn test_bot_notified_flag_db_operations() {
        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_flag", false).await;

        let unnotified = db.get_unnotified_runs().await.unwrap();
        assert_eq!(unnotified.len(), 1);
        assert_eq!(unnotified[0].run_id, "run_flag");

        db.set_bot_notified("run_flag", true).await.unwrap();

        let unnotified = db.get_unnotified_runs().await.unwrap();
        assert!(unnotified.is_empty());

        let run = db.get_run("run_flag").await.unwrap().unwrap();
        assert!(run.bot_notified);
    }

    #[tokio::test]
    async fn test_none_notifier_makes_no_http_calls() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let notifier: Option<BotNotifier> = None;
        if let Some(n) = &notifier {
            n.report_status("run123", "passed", None).await;
        }

        mock_server.verify().await;
    }
}
