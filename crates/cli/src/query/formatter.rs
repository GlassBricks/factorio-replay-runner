use anyhow::Result;
use comfy_table::{Cell, Table};

use crate::database::types::Run;

pub struct RunDisplay<'a> {
    pub run: &'a Run,
    pub game_name: String,
    pub category_name: String,
}

#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Table,
    Json,
    Csv,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "json" => Ok(OutputFormat::Json),
            "csv" => Ok(OutputFormat::Csv),
            _ => Err(anyhow::anyhow!("Invalid format: {}", s)),
        }
    }
}

pub fn format_runs_as_table(runs: &[RunDisplay]) -> String {
    let mut table = Table::new();
    table.set_header(vec![
        "Run ID",
        "Game/Category",
        "Submitted",
        "Status",
        "Retries",
        "Error Class",
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

        table.add_row(vec![
            Cell::new(&run.run_id[..8.min(run.run_id.len())]),
            Cell::new(game_category),
            Cell::new(submitted),
            Cell::new(status),
            Cell::new(retries),
            Cell::new(error_class),
        ]);
    }

    table.to_string()
}

pub fn format_runs_as_json(runs: &[Run]) -> Result<String> {
    serde_json::to_string_pretty(&runs).map_err(Into::into)
}

pub fn format_runs_as_csv(runs: &[RunDisplay]) -> Result<String> {
    let mut wtr = csv::Writer::from_writer(vec![]);

    wtr.write_record([
        "run_id",
        "game_id",
        "game_name",
        "category_id",
        "category_name",
        "submitted_date",
        "status",
        "retry_count",
        "error_class",
        "error_message",
        "next_retry_at",
        "created_at",
        "updated_at",
    ])?;

    for run_display in runs {
        let run = run_display.run;
        wtr.write_record([
            &run.run_id,
            &run.game_id,
            &run_display.game_name,
            &run.category_id,
            &run_display.category_name,
            &run.submitted_date.to_rfc3339(),
            &format_status(&run.status),
            &run.retry_count.to_string(),
            run.error_class.as_deref().unwrap_or(""),
            run.error_message.as_deref().unwrap_or(""),
            &run.next_retry_at
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
            &run.created_at.to_rfc3339(),
            &run.updated_at.to_rfc3339(),
        ])?;
    }

    let data = wtr.into_inner().map_err(|e| anyhow::anyhow!("{}", e))?;
    String::from_utf8(data).map_err(Into::into)
}

pub fn format_run_as_json(run: &Run) -> Result<String> {
    serde_json::to_string_pretty(&run).map_err(Into::into)
}

fn format_status(status: &crate::database::types::RunStatus) -> String {
    use crate::database::types::RunStatus;
    match status {
        RunStatus::Discovered => "discovered".to_string(),
        RunStatus::Processing => "processing".to_string(),
        RunStatus::Passed => "passed".to_string(),
        RunStatus::NeedsReview => "needs_review".to_string(),
        RunStatus::Failed => "failed".to_string(),
        RunStatus::Error => "error".to_string(),
    }
}
