use super::connection::Database;
use super::types::{NewRun, Run, RunStatus};
use anyhow::Result;
use chrono::{DateTime, Utc};
use log::{error, info, warn};
use replay_script::MsgLevel;

use crate::run_replay::ReplayReport;

impl Database {
    pub async fn insert_run(&self, new_run: NewRun) -> Result<()> {
        let now = Utc::now();
        let status = RunStatus::Discovered;

        sqlx::query!(
            r#"
            INSERT INTO runs (
                run_id, game_id, category_id, submitted_date,
                status, error_message, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, NULL, ?, ?)
            "#,
            new_run.run_id,
            new_run.game_id,
            new_run.category_id,
            new_run.submitted_date,
            status,
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

    #[allow(dead_code)]
    pub async fn get_run(&self, run_id: &str) -> Result<Option<Run>> {
        let run = sqlx::query_as!(
            Run,
            r#"
            SELECT run_id, game_id, category_id,
                   submitted_date as "submitted_date: chrono::DateTime<Utc>",
                   status as "status: RunStatus",
                   error_message,
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

    pub async fn get_next_discovered_run(
        &self,
        allowed_game_categories: &[(String, String)],
    ) -> Result<Option<Run>> {
        let status = RunStatus::Discovered;

        (!allowed_game_categories.is_empty())
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("No game/category configurations provided"))?;

        let runs = sqlx::query_as!(
            Run,
            r#"
            SELECT run_id, game_id, category_id,
                   submitted_date as "submitted_date: chrono::DateTime<Utc>",
                   status as "status: RunStatus",
                   error_message,
                   created_at as "created_at: chrono::DateTime<Utc>",
                   updated_at as "updated_at: chrono::DateTime<Utc>"
            FROM runs
            WHERE status = ?
            ORDER BY submitted_date ASC
            "#,
            status
        )
        .fetch_all(self.pool())
        .await?;

        let run = runs.into_iter().find(|run| {
            allowed_game_categories
                .iter()
                .any(|(game_id, cat_id)| run.game_id == *game_id && run.category_id == *cat_id)
        });

        Ok(run)
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
        result: Result<ReplayReport>,
    ) -> Result<()> {
        match result {
            Ok(report) if report.exited_successfully => match report.max_msg_level {
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
            },
            Ok(_) => {
                let error_msg = "Replay did not exit successfully";
                self.mark_run_error(run_id, error_msg).await?;
                error!("Run {} error: {}", run_id, error_msg);
            }
            Err(e) => {
                let error_msg = format!("Failed to process run: {:#}", e);
                self.mark_run_error(run_id, &error_msg).await?;
                error!("Run {} error: {}", run_id, error_msg);
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
    async fn test_get_next_discovered_run() {
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
        let next_run = db.get_next_discovered_run(&allowed).await.unwrap().unwrap();
        assert_eq!(next_run.run_id, "run2");

        let filtered_out = vec![("game_id_1".to_string(), "cat_id_2".to_string())];
        let no_run = db.get_next_discovered_run(&filtered_out).await.unwrap();
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
}
