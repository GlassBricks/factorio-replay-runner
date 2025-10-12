use super::connection::Database;
use super::types::{NewRun, Run, RunStatus};
use anyhow::Result;
use chrono::{DateTime, Utc};
use log::{error, info, warn};
use replay_script::MsgLevel;
use sqlx::Row;

use crate::error::ClassifiedError;
use crate::retry::{RetryConfig, calculate_next_retry, error_class_to_string};
use crate::run_replay::ReplayReport;

impl Database {
    pub async fn insert_run(&self, new_run: NewRun) -> Result<()> {
        let now = Utc::now();
        let status = RunStatus::Discovered;
        let retry_count: u32 = 0;

        sqlx::query!(
            r#"
            INSERT INTO runs (
                run_id, game_id, category_id, submitted_date,
                status, error_message, retry_count, next_retry_at, error_class,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, NULL, ?, NULL, NULL, ?, ?)
            "#,
            new_run.run_id,
            new_run.game_id,
            new_run.category_id,
            new_run.submitted_date,
            status,
            retry_count,
            now,
            now
        )
        .execute(self.pool())
        .await?;

        Ok(())
    }

    async fn update_run_status(
        &self,
        run_id: &str,
        status: RunStatus,
        error_message: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now();

        sqlx::query!(
            r#"
            UPDATE runs
            SET status = ?, error_message = ?, updated_at = ?
            WHERE run_id = ?
            "#,
            status,
            error_message,
            now,
            run_id
        )
        .execute(self.pool())
        .await?;

        Ok(())
    }

    pub async fn mark_run_processing(&self, run_id: &str) -> Result<()> {
        self.update_run_status(run_id, RunStatus::Processing, None)
            .await
    }

    pub async fn mark_run_passed(&self, run_id: &str) -> Result<()> {
        self.update_run_status(run_id, RunStatus::Passed, None)
            .await
    }

    pub async fn mark_run_needs_review(&self, run_id: &str) -> Result<()> {
        self.update_run_status(run_id, RunStatus::NeedsReview, None)
            .await
    }

    pub async fn mark_run_failed(&self, run_id: &str) -> Result<()> {
        self.update_run_status(run_id, RunStatus::Failed, None)
            .await
    }

    pub async fn mark_run_error(&self, run_id: &str, error_message: &str) -> Result<()> {
        self.update_run_status(run_id, RunStatus::Error, Some(error_message))
            .await
    }

    pub async fn mark_run_permanently_failed(&self, run_id: &str, error_class: &str) -> Result<()> {
        let now = Utc::now();

        sqlx::query!(
            r#"
            UPDATE runs
            SET next_retry_at = NULL, error_class = ?, updated_at = ?
            WHERE run_id = ?
            "#,
            error_class,
            now,
            run_id
        )
        .execute(self.pool())
        .await?;

        Ok(())
    }

    pub async fn schedule_retry(
        &self,
        run_id: &str,
        retry_count: u32,
        error_class: &str,
        next_retry_at: DateTime<Utc>,
    ) -> Result<()> {
        let now = Utc::now();

        sqlx::query!(
            r#"
            UPDATE runs
            SET retry_count = ?, error_class = ?, next_retry_at = ?, updated_at = ?
            WHERE run_id = ?
            "#,
            retry_count,
            error_class,
            next_retry_at,
            now,
            run_id
        )
        .execute(self.pool())
        .await?;

        Ok(())
    }

    pub async fn clear_retry_fields(&self, run_id: &str) -> Result<()> {
        let now = Utc::now();
        let retry_count: u32 = 0;

        sqlx::query!(
            r#"
            UPDATE runs
            SET retry_count = ?, next_retry_at = NULL, error_class = NULL, updated_at = ?
            WHERE run_id = ?
            "#,
            retry_count,
            now,
            run_id
        )
        .execute(self.pool())
        .await?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_run(&self, run_id: &str) -> Result<Option<Run>> {
        let run = sqlx::query_as!(
            Run,
            r#"
            SELECT run_id, game_id, category_id,
                   submitted_date as "submitted_date: chrono::DateTime<Utc>",
                   status as "status: RunStatus",
                   error_message,
                   retry_count as "retry_count: u32",
                   next_retry_at as "next_retry_at: chrono::DateTime<Utc>",
                   error_class,
                   created_at as "created_at: chrono::DateTime<Utc>",
                   updated_at as "updated_at: chrono::DateTime<Utc>"
            FROM runs
            WHERE run_id = ?
            "#,
            run_id
        )
        .fetch_optional(self.pool())
        .await?;

        Ok(run)
    }

    pub async fn get_next_run_to_process(
        &self,
        allowed_game_categories: &[(String, String)],
    ) -> Result<Option<Run>> {
        if allowed_game_categories.is_empty() {
            return Ok(None);
        }

        let now = Utc::now();
        let discovered_status = RunStatus::Discovered;
        let error_status = RunStatus::Error;
        let conditions = allowed_game_categories
            .iter()
            .map(|_| "(game_id = ? AND category_id = ?)")
            .collect::<Vec<_>>()
            .join(" OR ");

        let query_str = format!(
            r#"
            SELECT run_id, game_id, category_id, submitted_date, status,
                   error_message, retry_count, next_retry_at, error_class,
                   created_at, updated_at
            FROM runs
            WHERE (
                (status = ? AND ({}))
                OR (status = ? AND next_retry_at IS NOT NULL AND next_retry_at <= ? AND ({}))
            )
            ORDER BY submitted_date ASC
            LIMIT 1
            "#,
            conditions, conditions
        );

        let mut query = sqlx::query(&query_str).bind(discovered_status);

        for (game_id, cat_id) in allowed_game_categories {
            query = query.bind(game_id).bind(cat_id);
        }

        query = query.bind(error_status).bind(now);

        for (game_id, cat_id) in allowed_game_categories {
            query = query.bind(game_id).bind(cat_id);
        }

        let row = query.fetch_optional(self.pool()).await?;

        row.map(|r| {
            Ok::<_, sqlx::Error>(Run {
                run_id: r.try_get("run_id")?,
                game_id: r.try_get("game_id")?,
                category_id: r.try_get("category_id")?,
                submitted_date: r.try_get("submitted_date")?,
                status: r.try_get("status")?,
                error_message: r.try_get("error_message")?,
                retry_count: r.try_get("retry_count")?,
                next_retry_at: r.try_get("next_retry_at")?,
                error_class: r.try_get("error_class")?,
                created_at: r.try_get("created_at")?,
                updated_at: r.try_get("updated_at")?,
            })
        })
        .transpose()
        .map_err(Into::into)
    }

    pub async fn get_latest_submitted_date(
        &self,
        game_id: &str,
        category_id: &str,
    ) -> Result<Option<DateTime<Utc>>> {
        let result = sqlx::query!(
            r#"
            SELECT MAX(submitted_date) as "latest: chrono::DateTime<Utc>"
            FROM runs
            WHERE game_id = ? AND category_id = ?
            "#,
            game_id,
            category_id
        )
        .fetch_one(self.pool())
        .await?;

        Ok(result.latest)
    }

    pub async fn process_replay_result(
        &self,
        run_id: &str,
        result: Result<ReplayReport, ClassifiedError>,
        retry_config: &RetryConfig,
    ) -> Result<()> {
        match result {
            Ok(report) => {
                self.clear_retry_fields(run_id).await?;
                match report.max_msg_level {
                    MsgLevel::Info => {
                        self.mark_run_passed(run_id).await?;
                        info!("Run {} passed verification", run_id);
                    }
                    MsgLevel::Warn => {
                        self.mark_run_needs_review(run_id).await?;
                        warn!("Run {} passed with warnings (needs review)", run_id);
                    }
                    MsgLevel::Error => {
                        self.mark_run_failed(run_id).await?;
                        warn!("Run {} failed verification", run_id);
                    }
                }
            }
            Err(e) => {
                self.mark_run_error(run_id, &e.message).await?;

                let run = self.get_run(run_id).await?.ok_or_else(|| {
                    anyhow::anyhow!("Run {} not found after marking error", run_id)
                })?;

                let next_retry = calculate_next_retry(run.retry_count, &e.class, retry_config);

                let error_class_str = error_class_to_string(&e.class);
                match next_retry {
                    Some(next_retry_at) => {
                        let new_retry_count = run.retry_count + 1;
                        self.schedule_retry(
                            run_id,
                            new_retry_count,
                            error_class_str,
                            next_retry_at,
                        )
                        .await?;
                        error!(
                            "Run {} error (attempt {}/{}): {} - will retry at {}",
                            run_id,
                            new_retry_count,
                            retry_config.max_attempts,
                            e.message,
                            next_retry_at.format("%Y-%m-%d %H:%M:%S UTC")
                        );
                    }
                    None => {
                        self.mark_run_permanently_failed(run_id, error_class_str)
                            .await?;
                        error!(
                            "Run {} permanently failed after {} attempts: {}",
                            run_id, run.retry_count, e.message
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_insert_and_get_run() {
        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run123", "game_id_1", "cat_id_1", submitted_date);

        db.insert_run(new_run).await.unwrap();

        let run = db.get_run("run123").await.unwrap().unwrap();
        assert_eq!(run.run_id, "run123");
        assert_eq!(run.game_id, "game_id_1");
        assert_eq!(run.category_id, "cat_id_1");
        assert_eq!(run.status, RunStatus::Discovered);
        assert_eq!(run.retry_count, 0);
        assert_eq!(run.next_retry_at, None);
        assert_eq!(run.error_class, None);
    }

    #[tokio::test]
    async fn test_update_run_status() {
        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run123", "game_id_1", "cat_id_1", submitted_date);
        db.insert_run(new_run).await.unwrap();

        db.mark_run_processing("run123").await.unwrap();
        let run = db.get_run("run123").await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Processing);

        db.mark_run_passed("run123").await.unwrap();
        let run = db.get_run("run123").await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Passed);
    }

    #[tokio::test]
    async fn test_get_next_run_to_process() {
        let db = Database::in_memory().await.unwrap();

        db.insert_run(NewRun::new(
            "run1",
            "game_id_1",
            "cat_id_1",
            "2024-01-03T00:00:00Z".parse().unwrap(),
        ))
        .await
        .unwrap();
        db.insert_run(NewRun::new(
            "run2",
            "game_id_1",
            "cat_id_1",
            "2024-01-01T00:00:00Z".parse().unwrap(),
        ))
        .await
        .unwrap();
        db.insert_run(NewRun::new(
            "run3",
            "game_id_1",
            "cat_id_1",
            "2024-01-02T00:00:00Z".parse().unwrap(),
        ))
        .await
        .unwrap();

        let allowed = vec![("game_id_1".to_string(), "cat_id_1".to_string())];
        let next_run = db.get_next_run_to_process(&allowed).await.unwrap().unwrap();
        assert_eq!(next_run.run_id, "run2");

        let filtered_out = vec![("game_id_1".to_string(), "cat_id_2".to_string())];
        let no_run = db.get_next_run_to_process(&filtered_out).await.unwrap();
        assert!(no_run.is_none());
    }

    #[tokio::test]
    async fn test_get_latest_submitted_date() {
        let db = Database::in_memory().await.unwrap();

        let latest = db
            .get_latest_submitted_date("game_id_1", "cat_id_1")
            .await
            .unwrap();
        assert_eq!(latest, None);

        db.insert_run(NewRun::new(
            "run1",
            "game_id_1",
            "cat_id_1",
            "2024-01-03T00:00:00Z".parse().unwrap(),
        ))
        .await
        .unwrap();
        db.insert_run(NewRun::new(
            "run2",
            "game_id_1",
            "cat_id_1",
            "2024-01-01T00:00:00Z".parse().unwrap(),
        ))
        .await
        .unwrap();
        db.insert_run(NewRun::new(
            "run3",
            "game_id_1",
            "cat_id_1",
            "2024-01-05T00:00:00Z".parse().unwrap(),
        ))
        .await
        .unwrap();

        let latest = db
            .get_latest_submitted_date("game_id_1", "cat_id_1")
            .await
            .unwrap()
            .unwrap();
        let expected: DateTime<Utc> = "2024-01-05T00:00:00Z".parse().unwrap();
        assert_eq!(latest, expected);

        let other_category = db
            .get_latest_submitted_date("game_id_1", "other_cat")
            .await
            .unwrap();
        assert_eq!(other_category, None);
    }

    #[tokio::test]
    async fn test_retry_fields_initialized_correctly() {
        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run_retry_test", "game1", "cat1", submitted_date);

        db.insert_run(new_run).await.unwrap();

        let run = db.get_run("run_retry_test").await.unwrap().unwrap();
        assert_eq!(run.retry_count, 0);
        assert_eq!(run.next_retry_at, None);
        assert_eq!(run.error_class, None);
    }

    #[tokio::test]
    async fn test_retry_fields_persist_across_status_changes() {
        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run_persist_test", "game1", "cat1", submitted_date);

        db.insert_run(new_run).await.unwrap();

        db.mark_run_processing("run_persist_test").await.unwrap();
        let run = db.get_run("run_persist_test").await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Processing);
        assert_eq!(run.retry_count, 0);
        assert_eq!(run.next_retry_at, None);

        db.mark_run_passed("run_persist_test").await.unwrap();
        let run = db.get_run("run_persist_test").await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Passed);
        assert_eq!(run.retry_count, 0);
        assert_eq!(run.next_retry_at, None);
    }

    #[tokio::test]
    async fn test_schedule_retry() {
        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run_schedule", "game1", "cat1", submitted_date);
        db.insert_run(new_run).await.unwrap();

        db.mark_run_error("run_schedule", "test error")
            .await
            .unwrap();

        let next_retry_at = "2024-01-01T01:00:00Z".parse().unwrap();
        db.schedule_retry("run_schedule", 1, "retryable", next_retry_at)
            .await
            .unwrap();

        let run = db.get_run("run_schedule").await.unwrap().unwrap();
        assert_eq!(run.retry_count, 1);
        assert_eq!(run.next_retry_at, Some(next_retry_at));
        assert_eq!(run.error_class, Some("retryable".to_string()));
        assert_eq!(run.status, RunStatus::Error);
    }

    #[tokio::test]
    async fn test_mark_run_permanently_failed() {
        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run_permanent", "game1", "cat1", submitted_date);
        db.insert_run(new_run).await.unwrap();

        db.mark_run_error("run_permanent", "test error")
            .await
            .unwrap();

        let next_retry_at = "2024-01-01T01:00:00Z".parse().unwrap();
        db.schedule_retry("run_permanent", 1, "retryable", next_retry_at)
            .await
            .unwrap();

        db.mark_run_permanently_failed("run_permanent", "retryable")
            .await
            .unwrap();

        let run = db.get_run("run_permanent").await.unwrap().unwrap();
        assert_eq!(run.next_retry_at, None);
        assert_eq!(run.status, RunStatus::Error);
    }

    #[tokio::test]
    async fn test_clear_retry_fields() {
        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run_clear", "game1", "cat1", submitted_date);
        db.insert_run(new_run).await.unwrap();

        db.mark_run_error("run_clear", "test error").await.unwrap();

        let next_retry_at = "2024-01-01T01:00:00Z".parse().unwrap();
        db.schedule_retry("run_clear", 3, "retryable", next_retry_at)
            .await
            .unwrap();

        db.clear_retry_fields("run_clear").await.unwrap();

        let run = db.get_run("run_clear").await.unwrap().unwrap();
        assert_eq!(run.retry_count, 0);
        assert_eq!(run.next_retry_at, None);
        assert_eq!(run.error_class, None);
    }

    #[tokio::test]
    async fn test_get_next_run_to_process_with_retry_eligible() {
        let db = Database::in_memory().await.unwrap();

        let old_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_date = "2024-01-05T00:00:00Z".parse().unwrap();

        db.insert_run(NewRun::new("run_old", "game1", "cat1", old_date))
            .await
            .unwrap();
        db.insert_run(NewRun::new("run_new", "game1", "cat1", new_date))
            .await
            .unwrap();

        db.mark_run_error("run_old", "test error").await.unwrap();

        let past_retry_time = Utc::now() - chrono::Duration::hours(1);
        db.schedule_retry("run_old", 1, "retryable", past_retry_time)
            .await
            .unwrap();

        let allowed = vec![("game1".to_string(), "cat1".to_string())];
        let next_run = db.get_next_run_to_process(&allowed).await.unwrap().unwrap();

        assert_eq!(next_run.run_id, "run_old");
    }

    #[tokio::test]
    async fn test_get_next_run_to_process_retry_not_yet_ready() {
        let db = Database::in_memory().await.unwrap();

        let old_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_date = "2024-01-05T00:00:00Z".parse().unwrap();

        db.insert_run(NewRun::new("run_old", "game1", "cat1", old_date))
            .await
            .unwrap();
        db.insert_run(NewRun::new("run_new", "game1", "cat1", new_date))
            .await
            .unwrap();

        db.mark_run_error("run_old", "test error").await.unwrap();

        let future_retry_time = Utc::now() + chrono::Duration::hours(1);
        db.schedule_retry("run_old", 1, "retryable", future_retry_time)
            .await
            .unwrap();

        let allowed = vec![("game1".to_string(), "cat1".to_string())];
        let next_run = db.get_next_run_to_process(&allowed).await.unwrap().unwrap();

        assert_eq!(next_run.run_id, "run_new");
    }

    #[tokio::test]
    async fn test_get_next_run_to_process_ordering() {
        let db = Database::in_memory().await.unwrap();

        db.insert_run(NewRun::new(
            "run_2024_01_03",
            "game1",
            "cat1",
            "2024-01-03T00:00:00Z".parse().unwrap(),
        ))
        .await
        .unwrap();
        db.insert_run(NewRun::new(
            "run_2024_01_01",
            "game1",
            "cat1",
            "2024-01-01T00:00:00Z".parse().unwrap(),
        ))
        .await
        .unwrap();
        db.insert_run(NewRun::new(
            "run_2024_01_04",
            "game1",
            "cat1",
            "2024-01-04T00:00:00Z".parse().unwrap(),
        ))
        .await
        .unwrap();

        db.mark_run_error("run_2024_01_01", "test error")
            .await
            .unwrap();
        let past_time = Utc::now() - chrono::Duration::hours(1);
        db.schedule_retry("run_2024_01_01", 1, "retryable", past_time)
            .await
            .unwrap();

        let allowed = vec![("game1".to_string(), "cat1".to_string())];
        let next_run = db.get_next_run_to_process(&allowed).await.unwrap().unwrap();

        assert_eq!(next_run.run_id, "run_2024_01_01");
    }

    #[tokio::test]
    async fn test_process_replay_result_with_retry() {
        use crate::error::{ClassifiedError, ErrorClass};
        use crate::retry::RetryConfig;

        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run_retry_result", "game1", "cat1", submitted_date);
        db.insert_run(new_run).await.unwrap();

        let error = ClassifiedError {
            class: ErrorClass::Retryable,
            message: "Network error".to_string(),
        };
        let config = RetryConfig::default();

        db.process_replay_result("run_retry_result", Err(error), &config)
            .await
            .unwrap();

        let run = db.get_run("run_retry_result").await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Error);
        assert_eq!(run.retry_count, 1);
        assert!(run.next_retry_at.is_some());
        assert_eq!(run.error_class, Some("retryable".to_string()));
    }

    #[tokio::test]
    async fn test_process_replay_result_final_error() {
        use crate::error::{ClassifiedError, ErrorClass};
        use crate::retry::RetryConfig;

        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run_final", "game1", "cat1", submitted_date);
        db.insert_run(new_run).await.unwrap();

        let error = ClassifiedError {
            class: ErrorClass::Final,
            message: "Invalid save file".to_string(),
        };
        let config = RetryConfig::default();

        db.process_replay_result("run_final", Err(error), &config)
            .await
            .unwrap();

        let run = db.get_run("run_final").await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Error);
        assert_eq!(run.retry_count, 0);
        assert_eq!(run.next_retry_at, None);
        assert_eq!(run.error_class, Some("final".to_string()));
    }

    #[tokio::test]
    async fn test_process_replay_result_success_clears_retry() {
        use crate::retry::RetryConfig;
        use replay_script::MsgLevel;

        let db = Database::in_memory().await.unwrap();

        let submitted_date = "2024-01-01T00:00:00Z".parse().unwrap();
        let new_run = NewRun::new("run_success_clear", "game1", "cat1", submitted_date);
        db.insert_run(new_run).await.unwrap();

        db.mark_run_error("run_success_clear", "test error")
            .await
            .unwrap();
        let next_retry = Utc::now() + chrono::Duration::hours(1);
        db.schedule_retry("run_success_clear", 2, "retryable", next_retry)
            .await
            .unwrap();

        let report = ReplayReport {
            max_msg_level: MsgLevel::Info,
        };
        let config = RetryConfig::default();

        db.process_replay_result("run_success_clear", Ok(report), &config)
            .await
            .unwrap();

        let run = db.get_run("run_success_clear").await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Passed);
        assert_eq!(run.retry_count, 0);
        assert_eq!(run.next_retry_at, None);
        assert_eq!(run.error_class, None);
    }
}
