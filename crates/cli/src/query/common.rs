use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args;
use comfy_table::{Cell, Table};

use crate::daemon::database::types::{Run, RunFilter, RunStatus};
use crate::daemon::speedrun_api::SpeedrunOps;

#[derive(Args, Clone, Default)]
pub(crate) struct RunFilterArgs {
    /// Filter by run status (discovered, processing, passed, needs_review, failed, error)
    #[arg(long)]
    pub status: Option<String>,

    /// Filter by speedrun.com game ID
    #[arg(long)]
    pub game_id: Option<String>,

    /// Filter by speedrun.com category ID
    #[arg(long)]
    pub category_id: Option<String>,

    /// Only show runs newer than this duration (e.g., 30d, 1w, 2weeks)
    #[arg(long)]
    pub newer_than: Option<String>,

    /// Only show runs older than this duration (e.g., 30d, 1w, 2weeks)
    #[arg(long)]
    pub older_than: Option<String>,

    /// Maximum number of runs to display
    #[arg(long, default_value = "50")]
    pub limit: u32,

    /// Number of runs to skip
    #[arg(long, default_value = "0")]
    pub offset: u32,
}

impl RunFilterArgs {
    pub fn to_filter(&self) -> Result<RunFilter> {
        let status = self
            .status
            .as_ref()
            .map(|s| parse_status(s))
            .transpose()
            .context("Invalid status value")?;

        let since_date = self
            .newer_than
            .as_ref()
            .map(|s| parse_relative_duration(s))
            .transpose()?;

        let before_date = self
            .older_than
            .as_ref()
            .map(|s| parse_relative_duration(s))
            .transpose()?;

        Ok(RunFilter {
            status,
            game_id: self.game_id.clone(),
            category_id: self.category_id.clone(),
            since_date,
            before_date,
            limit: self.limit,
            offset: self.offset,
        })
    }

    pub fn with_status(mut self, status: &str) -> Self {
        self.status = Some(status.to_string());
        self
    }

    pub fn with_unlimited(mut self) -> Self {
        self.limit = u32::MAX;
        self
    }

    pub fn has_any_filter(&self) -> bool {
        self.status.is_some()
            || self.game_id.is_some()
            || self.category_id.is_some()
            || self.newer_than.is_some()
            || self.older_than.is_some()
    }
}

pub(crate) async fn query_and_display_runs(
    db: &crate::daemon::database::connection::Database,
    ops: &SpeedrunOps,
    filter: RunFilter,
) -> Result<()> {
    let runs = db.query_runs(filter).await?;

    if runs.is_empty() {
        println!("No runs found matching the criteria");
        return Ok(());
    }

    let mut run_displays = Vec::new();
    for run in &runs {
        let (game_name, category_name) =
            resolve_game_category(ops, &run.game_id, &run.category_id).await;
        run_displays.push(RunDisplay {
            run,
            game_name,
            category_name,
        });
    }

    println!("{}", format_runs_as_table(&run_displays));
    Ok(())
}

pub(crate) struct RunDisplay<'a> {
    pub run: &'a Run,
    pub game_name: String,
    pub category_name: String,
}

pub(crate) fn format_runs_as_table(runs: &[RunDisplay]) -> String {
    let mut table = Table::new();
    table.set_header(vec![
        "Run ID",
        "Game/Category",
        "Submitted",
        "Status",
        "Retries",
        "Error Class",
        "Error Reason",
    ]);

    for run_display in runs {
        let run = run_display.run;
        let game_category = format!("{} / {}", run_display.game_name, run_display.category_name);
        let submitted = run.submitted_date.format("%Y-%m-%d %H:%M").to_string();
        let status = format_status(&run.status);
        let retries = if run.retry_count > 0 {
            run.retry_count.to_string()
        } else {
            "-".to_string()
        };
        let error_class = run.error_class.as_deref().unwrap_or("-");
        let error_reason = run
            .error_message
            .as_ref()
            .map(|msg| truncate_str(msg, 40))
            .unwrap_or_else(|| "-".to_string());

        table.add_row(vec![
            Cell::new(&run.run_id[..8.min(run.run_id.len())]),
            Cell::new(game_category),
            Cell::new(submitted),
            Cell::new(status),
            Cell::new(retries),
            Cell::new(error_class),
            Cell::new(error_reason),
        ]);
    }

    table.to_string()
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

pub(crate) async fn resolve_game_category(
    ops: &SpeedrunOps,
    game_id: &str,
    category_id: &str,
) -> (String, String) {
    let game_name = ops
        .get_game_name(game_id)
        .await
        .unwrap_or_else(|_| game_id.to_string());
    let category_name = ops
        .get_category_name(category_id)
        .await
        .unwrap_or_else(|_| category_id.to_string());
    (game_name, category_name)
}

pub(crate) fn parse_status(s: &str) -> Result<RunStatus> {
    match s.to_lowercase().as_str() {
        "discovered" => Ok(RunStatus::Discovered),
        "processing" => Ok(RunStatus::Processing),
        "passed" => Ok(RunStatus::Passed),
        "needs_review" | "needs-review" => Ok(RunStatus::NeedsReview),
        "failed" => Ok(RunStatus::Failed),
        "error" => Ok(RunStatus::Error),
        _ => Err(anyhow::anyhow!("Invalid status: {}", s)),
    }
}

pub(crate) fn format_status(status: &RunStatus) -> String {
    match status {
        RunStatus::Discovered => "discovered".to_string(),
        RunStatus::Processing => "processing".to_string(),
        RunStatus::Passed => "passed".to_string(),
        RunStatus::NeedsReview => "needs_review".to_string(),
        RunStatus::Failed => "failed".to_string(),
        RunStatus::Error => "error".to_string(),
    }
}

pub(crate) fn parse_relative_duration(duration_str: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    let duration = humantime::parse_duration(duration_str)
        .context("Invalid duration format. Examples: 30d, 1w, 2weeks, 1month, 1h30m")?;
    let chrono_duration = chrono::Duration::from_std(duration).context("Duration too large")?;
    Ok(Utc::now() - chrono_duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_duration_days() {
        let result = parse_relative_duration("30d");
        assert!(result.is_ok());
        let dt = result.unwrap();
        let diff = Utc::now() - dt;
        assert!((diff.num_days() - 30).abs() <= 1);
    }

    #[test]
    fn test_parse_relative_duration_weeks() {
        let result = parse_relative_duration("2weeks");
        assert!(result.is_ok());
        let dt = result.unwrap();
        let diff = Utc::now() - dt;
        assert!((diff.num_days() - 14).abs() <= 1);
    }

    #[test]
    fn test_parse_relative_duration_combined() {
        let result = parse_relative_duration("1d12h");
        assert!(result.is_ok());
        let dt = result.unwrap();
        let diff = Utc::now() - dt;
        assert!((diff.num_hours() - 36).abs() <= 1);
    }

    #[test]
    fn test_parse_relative_duration_invalid() {
        let result = parse_relative_duration("invalid");
        assert!(result.is_err());
    }
}
