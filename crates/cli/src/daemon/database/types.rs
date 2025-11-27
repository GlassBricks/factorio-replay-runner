use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum RunStatus {
    Discovered,
    Processing,
    Passed,
    NeedsReview,
    Failed,
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct RunFilter {
    pub status: Option<RunStatus>,
    pub game_id: Option<String>,
    pub category_id: Option<String>,
    pub since_date: Option<DateTime<Utc>>,
    pub before_date: Option<DateTime<Utc>>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[allow(dead_code)]
pub struct Run {
    pub run_id: String,
    pub game_id: String,
    pub category_id: String,
    pub submitted_date: DateTime<Utc>,
    pub status: RunStatus,
    pub error_message: Option<String>,
    pub retry_count: u32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub error_class: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewRun {
    pub run_id: String,
    pub game_id: String,
    pub category_id: String,
    pub submitted_date: DateTime<Utc>,
}

impl NewRun {
    pub fn new(
        run_id: impl Into<String>,
        game_id: impl Into<String>,
        category_id: impl Into<String>,
        submitted_date: DateTime<Utc>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            game_id: game_id.into(),
            category_id: category_id.into(),
            submitted_date,
        }
    }
}
