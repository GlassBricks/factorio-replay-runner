use crate::daemon::database::{connection::Database, types::RunStatus};
use log::{info, warn};
use reqwest::Client;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::config::BotNotifierConfig;

const MAX_NOTIFY_ATTEMPTS: usize = 5;
const HEARTBEAT_INTERVAL_SECS: u64 = 15 * 60;

#[derive(Clone)]
pub struct BotNotifierHandle {
    tx: mpsc::Sender<String>,
}

impl BotNotifierHandle {
    pub fn new() -> (Self, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel(64);
        (Self { tx }, rx)
    }

    pub fn notify(&self, run_id: String) {
        let _ = self.tx.try_send(run_id);
    }
}

pub async fn run_bot_notifier_actor(
    mut rx: mpsc::Receiver<String>,
    db: Database,
    config: BotNotifierConfig,
    token: CancellationToken,
) -> Result<(), anyhow::Error> {
    let client = Client::new();
    let mut retry_interval =
        tokio::time::interval(Duration::from_secs(config.retry_interval_seconds));
    retry_interval.tick().await;
    let mut heartbeat_interval =
        tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
    heartbeat_interval.tick().await;

    loop {
        tokio::select! {
            Some(run_id) = rx.recv() => {
                notify_run(&db, &client, &config, &run_id).await;
            }
            _ = retry_interval.tick() => {
                retry_unnotified(&db, &client, &config).await;
            }
            _ = heartbeat_interval.tick() => {
                send_heartbeat(&db, &client, &config).await;
            }
            _ = token.cancelled() => {
                info!("Bot notifier shutting down");
                return Ok(());
            }
        }
    }
}

async fn notify_run(db: &Database, client: &Client, config: &BotNotifierConfig, run_id: &str) {
    for _ in 0..MAX_NOTIFY_ATTEMPTS {
        let Some(run) = db.get_run(run_id).await.ok().flatten() else {
            return;
        };
        if run.bot_notified {
            return;
        }

        let status = run_status_to_bot_status(&run.status);
        if !post_status(client, config, run_id, status, run.error_message.as_deref()).await {
            return;
        }

        let updated = db
            .set_bot_notified_if_status(run_id, true, &run.status)
            .await
            .unwrap_or(false);
        if updated {
            return;
        }
    }
}

async fn retry_unnotified(db: &Database, client: &Client, config: &BotNotifierConfig) {
    let runs = match db.get_unnotified_runs().await {
        Ok(runs) => runs,
        Err(e) => {
            warn!("Failed to query unnotified runs: {}", e);
            return;
        }
    };

    if runs.is_empty() {
        return;
    }

    let entries: Vec<serde_json::Value> = runs
        .iter()
        .map(|run| {
            serde_json::json!({
                "runId": run.run_id,
                "status": run_status_to_bot_status(&run.status),
                "message": run.error_message,
            })
        })
        .collect();

    if !post_statuses_bulk(client, config, &entries).await {
        warn!("Bulk notification failed for {} runs", runs.len());
        return;
    }

    for run in &runs {
        let _ = db
            .set_bot_notified_if_status(&run.run_id, true, &run.status)
            .await;
    }

    info!("Bulk notified {} runs", runs.len());
}

async fn post_statuses_bulk(
    client: &Client,
    config: &BotNotifierConfig,
    entries: &[serde_json::Value],
) -> bool {
    let url = format!("{}/api/runs/status", config.bot_url);
    let body = serde_json::json!({ "runs": entries });

    let result = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.auth_token))
        .json(&body)
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => true,
        Ok(resp) => {
            warn!("Bulk notification failed (HTTP {})", resp.status());
            false
        }
        Err(e) => {
            warn!("Bulk notification error: {}", e);
            false
        }
    }
}

async fn send_heartbeat(db: &Database, client: &Client, config: &BotNotifierConfig) {
    let runs = match db.get_non_final_runs().await {
        Ok(runs) => runs,
        Err(e) => {
            warn!("Failed to query non-final runs for heartbeat: {}", e);
            return;
        }
    };

    if runs.is_empty() {
        return;
    }

    let run_ids: Vec<&str> = runs.iter().map(|r| r.run_id.as_str()).collect();
    let url = format!("{}/api/runs/heartbeat", config.bot_url);
    let body = serde_json::json!({ "runIds": run_ids });

    let result = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.auth_token))
        .json(&body)
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => {
            info!("Heartbeat sent for {} runs", run_ids.len());
        }
        Ok(resp) => {
            warn!("Heartbeat failed (HTTP {})", resp.status());
        }
        Err(e) => {
            warn!("Heartbeat error: {}", e);
        }
    }
}

async fn post_status(
    client: &Client,
    config: &BotNotifierConfig,
    run_id: &str,
    status: &str,
    message: Option<&str>,
) -> bool {
    let url = format!("{}/api/runs/{}/status", config.bot_url, run_id);
    let body = serde_json::json!({ "status": status, "message": message });

    let result = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.auth_token))
        .json(&body)
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => {
            info!("Bot notified for run {} with status {}", run_id, status);
            true
        }
        Ok(resp) => {
            warn!(
                "Bot notification failed for run {} (HTTP {})",
                run_id,
                resp.status()
            );
            false
        }
        Err(e) => {
            warn!("Bot notification error for run {}: {}", run_id, e);
            false
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

    async fn insert_test_run(db: &Database, run_id: &str) {
        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new(run_id, "game1", "cat1", submitted_date);
        db.insert_run(new_run).await.unwrap();
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
        insert_test_run(&db, "run123").await;

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        notify_run(&db, &client, &config, "run123").await;

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
        insert_test_run(&db, "run123").await;

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        notify_run(&db, &client, &config, "run123").await;

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
        insert_test_run(&db, "run500").await;

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        notify_run(&db, &client, &config, "run500").await;

        let run = db.get_run("run500").await.unwrap().unwrap();
        assert!(!run.bot_notified);
    }

    #[tokio::test]
    async fn test_server_unreachable_leaves_bot_notified_false() {
        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_unreachable").await;

        let client = Client::new();
        let config = make_config("http://127.0.0.1:19999");
        notify_run(&db, &client, &config, "run_unreachable").await;

        let run = db.get_run("run_unreachable").await.unwrap().unwrap();
        assert!(!run.bot_notified);
    }

    #[tokio::test]
    async fn test_retry_unnotified_sends_bulk_request() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/runs/status"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_retry").await;

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        retry_unnotified(&db, &client, &config).await;

        mock_server.verify().await;

        let run = db.get_run("run_retry").await.unwrap().unwrap();
        assert!(run.bot_notified);
    }

    #[tokio::test]
    async fn test_retry_unnotified_bulk_with_multiple_runs() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/runs/status"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "bulk_1").await;
        insert_test_run(&db, "bulk_2").await;
        insert_test_run(&db, "bulk_3").await;

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        retry_unnotified(&db, &client, &config).await;

        mock_server.verify().await;

        for id in &["bulk_1", "bulk_2", "bulk_3"] {
            let run = db.get_run(id).await.unwrap().unwrap();
            assert!(run.bot_notified, "{} should be notified", id);
        }
    }

    #[tokio::test]
    async fn test_retry_unnotified_bulk_failure_leaves_unnotified() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/runs/status"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "bulk_fail").await;

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        retry_unnotified(&db, &client, &config).await;

        let run = db.get_run("bulk_fail").await.unwrap().unwrap();
        assert!(!run.bot_notified);
    }

    #[tokio::test]
    async fn test_retry_unnotified_skips_when_no_runs() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        retry_unnotified(&db, &client, &config).await;

        mock_server.verify().await;
    }

    #[tokio::test]
    async fn test_bot_notified_flag_db_operations() {
        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_flag").await;

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
    async fn test_handle_notify_sends_to_channel() {
        let (handle, mut rx) = BotNotifierHandle::new();
        handle.notify("run123".to_string());
        let received = rx.recv().await.unwrap();
        assert_eq!(received, "run123");
    }

    #[tokio::test]
    async fn test_notify_skips_already_notified_run() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_already").await;
        db.set_bot_notified("run_already", true).await.unwrap();

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        notify_run(&db, &client, &config, "run_already").await;

        mock_server.verify().await;
    }

    #[tokio::test]
    async fn test_notify_nonexistent_run_does_nothing() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        notify_run(&db, &client, &config, "nonexistent").await;

        mock_server.verify().await;
    }

    #[tokio::test]
    async fn test_set_bot_notified_if_status_matches() {
        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_cond").await;

        let updated = db
            .set_bot_notified_if_status("run_cond", true, &RunStatus::Discovered)
            .await
            .unwrap();
        assert!(updated);

        let run = db.get_run("run_cond").await.unwrap().unwrap();
        assert!(run.bot_notified);
    }

    #[tokio::test]
    async fn test_set_bot_notified_if_status_mismatches() {
        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_mismatch").await;

        let updated = db
            .set_bot_notified_if_status("run_mismatch", true, &RunStatus::Processing)
            .await
            .unwrap();
        assert!(!updated);

        let run = db.get_run("run_mismatch").await.unwrap().unwrap();
        assert!(!run.bot_notified);
    }

    #[tokio::test]
    async fn test_get_non_final_runs_returns_discovered_and_processing() {
        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_disc").await;
        insert_test_run(&db, "run_proc").await;
        insert_test_run(&db, "run_passed").await;
        insert_test_run(&db, "run_failed").await;

        db.mark_run_processing("run_proc").await.unwrap();
        db.mark_run_passed("run_passed").await.unwrap();
        db.mark_run_failed("run_failed", Some("bad")).await.unwrap();

        let non_final = db.get_non_final_runs().await.unwrap();
        let ids: Vec<&str> = non_final.iter().map(|r| r.run_id.as_str()).collect();
        assert!(ids.contains(&"run_disc"));
        assert!(ids.contains(&"run_proc"));
        assert!(!ids.contains(&"run_passed"));
        assert!(!ids.contains(&"run_failed"));
    }

    #[tokio::test]
    async fn test_heartbeat_sends_non_final_run_ids() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/runs/heartbeat"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_hb1").await;
        insert_test_run(&db, "run_hb2").await;
        db.mark_run_passed("run_hb2").await.unwrap();

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        send_heartbeat(&db, &client, &config).await;

        mock_server.verify().await;
    }

    #[tokio::test]
    async fn test_heartbeat_skips_when_no_non_final_runs() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let db = Database::in_memory().await.unwrap();

        let client = Client::new();
        let config = make_config(&mock_server.uri());
        send_heartbeat(&db, &client, &config).await;

        mock_server.verify().await;
    }

    #[tokio::test]
    async fn test_update_run_status_resets_bot_notified() {
        let db = Database::in_memory().await.unwrap();
        insert_test_run(&db, "run_reset").await;

        db.set_bot_notified("run_reset", true).await.unwrap();
        let run = db.get_run("run_reset").await.unwrap().unwrap();
        assert!(run.bot_notified);

        db.update_run_status("run_reset", RunStatus::Processing, None)
            .await
            .unwrap();
        let run = db.get_run("run_reset").await.unwrap().unwrap();
        assert!(!run.bot_notified);
    }
}
