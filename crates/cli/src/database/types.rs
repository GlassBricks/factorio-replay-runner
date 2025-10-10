use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum RunStatus {
    Discovered,
    Processing,
    Passed,
    Failed,
    Error,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Run {
    pub run_id: String,
    pub game_id: String,
    pub category_id: String,
    pub runner_name: Option<String>,
    pub submitted_date: String,
    pub status: String,
    pub error_message: Option<String>,
    pub verification_status: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Run {
    pub fn run_status(&self) -> Result<RunStatus, strum::ParseError> {
        self.status.parse()
    }

    pub fn verification_status_enum(
        &self,
    ) -> Result<Option<VerificationStatus>, strum::ParseError> {
        self.verification_status
            .as_ref()
            .map(|s| s.parse())
            .transpose()
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PollState {
    pub game_id: String,
    pub category_id: String,
    pub last_poll_time: String,
    pub last_poll_success: String,
}

#[derive(Debug, Clone)]
pub struct NewRun {
    pub run_id: String,
    pub game_id: String,
    pub category_id: String,
    pub runner_name: Option<String>,
    pub submitted_date: String,
}

impl NewRun {
    pub fn new(
        run_id: impl Into<String>,
        game_id: impl Into<String>,
        category_id: impl Into<String>,
        submitted_date: impl Into<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            game_id: game_id.into(),
            category_id: category_id.into(),
            runner_name: None,
            submitted_date: submitted_date.into(),
        }
    }

    pub fn with_runner(mut self, runner_name: impl Into<String>) -> Self {
        self.runner_name = Some(runner_name.into());
        self
    }
}
