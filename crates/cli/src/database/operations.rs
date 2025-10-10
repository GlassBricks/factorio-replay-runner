use super::connection::Database;
use super::types::{NewRun, PollState, Run, RunStatus};
use anyhow::Result;
use chrono::Utc;

pub async fn insert_run(db: &Database, new_run: NewRun) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let status = RunStatus::Discovered.to_string();

    sqlx::query!(
        r#"
        INSERT INTO runs (
            run_id, game_id, category_id, runner_name, submitted_date,
            status, error_message, verification_status, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, NULL, NULL, ?, ?)
        "#,
        new_run.run_id,
        new_run.game_id,
        new_run.category_id,
        new_run.runner_name,
        new_run.submitted_date,
        status,
        now,
        now
    )
    .execute(db.pool())
    .await?;

    Ok(())
}

async fn update_run_status(
    db: &Database,
    run_id: &str,
    status: RunStatus,
    error_message: Option<&str>,
    verification_status: Option<&str>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let status_str = status.to_string();

    sqlx::query!(
        r#"
        UPDATE runs
        SET status = ?, error_message = ?, verification_status = ?, updated_at = ?
        WHERE run_id = ?
        "#,
        status_str,
        error_message,
        verification_status,
        now,
        run_id
    )
    .execute(db.pool())
    .await?;

    Ok(())
}

pub async fn mark_run_processing(db: &Database, run_id: &str) -> Result<()> {
    update_run_status(db, run_id, RunStatus::Processing, None, None).await
}

pub async fn mark_run_passed(db: &Database, run_id: &str) -> Result<()> {
    update_run_status(db, run_id, RunStatus::Passed, None, Some("passed")).await
}

pub async fn mark_run_failed(db: &Database, run_id: &str, error_message: &str) -> Result<()> {
    update_run_status(
        db,
        run_id,
        RunStatus::Failed,
        Some(error_message),
        Some("failed"),
    )
    .await
}

pub async fn mark_run_error(db: &Database, run_id: &str, error_message: &str) -> Result<()> {
    update_run_status(db, run_id, RunStatus::Error, Some(error_message), None).await
}

pub async fn mark_run_skipped(db: &Database, run_id: &str) -> Result<()> {
    update_run_status(db, run_id, RunStatus::Skipped, None, None).await
}

pub async fn get_run(db: &Database, run_id: &str) -> Result<Option<Run>> {
    let run = sqlx::query_as!(
        Run,
        r#"
        SELECT run_id, game_id, category_id, runner_name, submitted_date,
               status, error_message, verification_status, created_at, updated_at
        FROM runs
        WHERE run_id = ?
        "#,
        run_id
    )
    .fetch_optional(db.pool())
    .await?;

    Ok(run)
}

pub async fn get_next_discovered_run(db: &Database) -> Result<Option<Run>> {
    let status = RunStatus::Discovered.to_string();

    let run = sqlx::query_as!(
        Run,
        r#"
        SELECT run_id, game_id, category_id, runner_name, submitted_date,
               status, error_message, verification_status, created_at, updated_at
        FROM runs
        WHERE status = ?
        ORDER BY submitted_date ASC
        LIMIT 1
        "#,
        status
    )
    .fetch_optional(db.pool())
    .await?;

    Ok(run)
}

pub async fn upsert_poll_state(
    db: &Database,
    game_id: &str,
    category_id: &str,
    last_poll_time: &str,
    last_poll_success: &str,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO poll_state (game_id, category_id, last_poll_time, last_poll_success)
        VALUES (?, ?, ?, ?)
        ON CONFLICT (game_id, category_id)
        DO UPDATE SET last_poll_time = ?, last_poll_success = ?
        "#,
        game_id,
        category_id,
        last_poll_time,
        last_poll_success,
        last_poll_time,
        last_poll_success
    )
    .execute(db.pool())
    .await?;

    Ok(())
}

pub async fn get_poll_state(
    db: &Database,
    game_id: &str,
    category_id: &str,
) -> Result<Option<PollState>> {
    let state = sqlx::query_as!(
        PollState,
        r#"
        SELECT game_id, category_id, last_poll_time, last_poll_success
        FROM poll_state
        WHERE game_id = ? AND category_id = ?
        "#,
        game_id,
        category_id
    )
    .fetch_optional(db.pool())
    .await?;

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_insert_and_get_run() {
        let db = Database::in_memory().await.unwrap();

        let new_run = NewRun::new("run123", "game_id_1", "cat_id_1", "2024-01-01T00:00:00Z")
            .with_runner("speedrunner");

        insert_run(&db, new_run).await.unwrap();

        let run = get_run(&db, "run123").await.unwrap().unwrap();
        assert_eq!(run.run_id, "run123");
        assert_eq!(run.game_id, "game_id_1");
        assert_eq!(run.category_id, "cat_id_1");
        assert_eq!(run.runner_name, Some("speedrunner".to_string()));
        assert_eq!(run.run_status().unwrap(), RunStatus::Discovered);
    }

    #[tokio::test]
    async fn test_update_run_status() {
        let db = Database::in_memory().await.unwrap();

        let new_run = NewRun::new("run123", "game_id_1", "cat_id_1", "2024-01-01T00:00:00Z");
        insert_run(&db, new_run).await.unwrap();

        mark_run_processing(&db, "run123").await.unwrap();
        let run = get_run(&db, "run123").await.unwrap().unwrap();
        assert_eq!(run.run_status().unwrap(), RunStatus::Processing);

        mark_run_passed(&db, "run123").await.unwrap();
        let run = get_run(&db, "run123").await.unwrap().unwrap();
        assert_eq!(run.run_status().unwrap(), RunStatus::Passed);
        assert_eq!(run.verification_status, Some("passed".to_string()));
    }

    #[tokio::test]
    async fn test_get_next_discovered_run() {
        let db = Database::in_memory().await.unwrap();

        insert_run(
            &db,
            NewRun::new("run1", "game_id_1", "cat_id_1", "2024-01-03T00:00:00Z"),
        )
        .await
        .unwrap();
        insert_run(
            &db,
            NewRun::new("run2", "game_id_1", "cat_id_1", "2024-01-01T00:00:00Z"),
        )
        .await
        .unwrap();
        insert_run(
            &db,
            NewRun::new("run3", "game_id_1", "cat_id_1", "2024-01-02T00:00:00Z"),
        )
        .await
        .unwrap();

        let next_run = get_next_discovered_run(&db).await.unwrap().unwrap();
        assert_eq!(next_run.run_id, "run2");
    }

    #[tokio::test]
    async fn test_poll_state_operations() {
        let db = Database::in_memory().await.unwrap();

        upsert_poll_state(
            &db,
            "game_id_1",
            "cat_id_1",
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .await
        .unwrap();

        let state = get_poll_state(&db, "game_id_1", "cat_id_1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.game_id, "game_id_1");
        assert_eq!(state.category_id, "cat_id_1");

        upsert_poll_state(
            &db,
            "game_id_1",
            "cat_id_1",
            "2024-01-02T00:00:00Z",
            "2024-01-02T00:00:00Z",
        )
        .await
        .unwrap();

        let state = get_poll_state(&db, "game_id_1", "cat_id_1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.last_poll_time, "2024-01-02T00:00:00Z");
    }
}
