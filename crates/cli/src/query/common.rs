use anyhow::{Context, Result};
use comfy_table::{Cell, Table};

use crate::daemon::database::types::{Run, RunStatus};
use crate::daemon::speedrun_api::SpeedrunOps;

pub(super) struct RunDisplay<'a> {
    pub run: &'a Run,
    pub game_name: String,
    pub category_name: String,
}

pub(super) fn format_runs_as_table(runs: &[RunDisplay]) -> String {
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

pub(super) async fn resolve_game_category(
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

pub(super) fn parse_status(s: &str) -> Result<RunStatus> {
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

pub(super) fn format_status(status: &RunStatus) -> String {
    match status {
        RunStatus::Discovered => "discovered".to_string(),
        RunStatus::Processing => "processing".to_string(),
        RunStatus::Passed => "passed".to_string(),
        RunStatus::NeedsReview => "needs_review".to_string(),
        RunStatus::Failed => "failed".to_string(),
        RunStatus::Error => "error".to_string(),
    }
}

pub(super) fn parse_datetime(date_str: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    if let Ok(dt) = date_str.parse() {
        return Ok(dt);
    }

    let with_time = format!("{}T00:00:00Z", date_str);
    with_time.parse().context(
        "Invalid date format. Expected ISO 8601 format (e.g., 2025-01-01T00:00:00Z or 2025-01-01)",
    )
}

pub(super) fn group_and_display<F>(
    error_runs: &[Run],
    limit: u32,
    title: &str,
    unique_label: &str,
    key_extractor: F,
    display_group: impl Fn(&str, &[&Run]),
) -> Result<()>
where
    F: Fn(&Run) -> String,
{
    use std::collections::HashMap;

    let mut groups: HashMap<String, Vec<&Run>> = HashMap::new();
    for run in error_runs {
        groups.entry(key_extractor(run)).or_default().push(run);
    }

    let mut sorted_groups: Vec<_> = groups.iter().collect();
    sorted_groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    println!("{}", title);
    println!("{}", "=".repeat(title.len()));
    println!();
    println!("Total {}: {}", unique_label, sorted_groups.len());
    println!();

    for (key, runs) in sorted_groups.iter().take(limit as usize) {
        display_group(key, runs);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_datetime_full_format() {
        let result = parse_datetime("2025-01-01T00:00:00Z");
        assert!(result.is_ok());
        let dt = result.unwrap();
        assert_eq!(dt.to_rfc3339(), "2025-01-01T00:00:00+00:00");
    }

    #[test]
    fn test_parse_datetime_date_only() {
        let result = parse_datetime("2025-01-01");
        assert!(result.is_ok());
        let dt = result.unwrap();
        assert_eq!(dt.to_rfc3339(), "2025-01-01T00:00:00+00:00");
    }

    #[test]
    fn test_parse_datetime_with_timezone() {
        let result = parse_datetime("2025-01-01T12:30:45+05:00");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_datetime_invalid_format() {
        let result = parse_datetime("not-a-date");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid date format")
        );
    }
}
