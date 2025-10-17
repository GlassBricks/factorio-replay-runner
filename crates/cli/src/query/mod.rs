use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::daemon::database::connection::Database;
use crate::daemon::database::types::{Run, RunFilter, RunStatus};
use crate::daemon::speedrun_api::{SpeedrunClient, SpeedrunOps};

mod formatter;

#[derive(Args)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub subcommand: QuerySubcommand,

    #[arg(long, default_value = "run_verification.db")]
    pub database: PathBuf,
}

#[derive(Subcommand)]
pub enum QuerySubcommand {
    List(ListArgs),
    Show(ShowArgs),
    Stats(StatsArgs),
    Queue(QueueArgs),
    Errors(ErrorsArgs),
    Reset(ResetArgs),
    Cleanup(CleanupArgs),
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(long)]
    pub status: Option<String>,

    #[arg(long)]
    pub game_id: Option<String>,

    #[arg(long)]
    pub category_id: Option<String>,

    #[arg(long, default_value = "50")]
    pub limit: u32,

    #[arg(long, default_value = "0")]
    pub offset: u32,

    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args)]
pub struct ShowArgs {
    pub run_id: String,

    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct StatsArgs {
    #[arg(long)]
    pub game_id: Option<String>,

    #[arg(long)]
    pub category_id: Option<String>,

    #[arg(long)]
    pub since: Option<String>,
}

#[derive(Args)]
pub struct QueueArgs {}

#[derive(Args)]
pub struct ErrorsArgs {
    #[arg(long, default_value = "20")]
    pub limit: u32,

    #[arg(long)]
    pub error_class: Option<String>,

    #[arg(long)]
    pub group_by: Option<String>,
}

#[derive(Args)]
pub struct ResetArgs {
    pub run_id: String,

    #[arg(long)]
    pub clear_error: bool,
}

#[derive(Args)]
pub struct CleanupArgs {
    #[arg(long)]
    pub before: Option<String>,

    #[arg(long)]
    pub status: Option<String>,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub force: bool,
}

pub async fn handle_query_command(args: QueryArgs) -> Result<()> {
    let db = Database::new(&args.database).await?;
    let speedrun_client = SpeedrunClient::new().context("Failed to create speedrun client")?;
    let speedrun_ops = SpeedrunOps::new(&speedrun_client).with_db(db.clone());

    match args.subcommand {
        QuerySubcommand::List(list_args) => handle_list(&db, &speedrun_ops, list_args).await,
        QuerySubcommand::Show(show_args) => handle_show(&db, &speedrun_ops, show_args).await,
        QuerySubcommand::Stats(stats_args) => handle_stats(&db, stats_args).await,
        QuerySubcommand::Queue(queue_args) => handle_queue(&db, queue_args).await,
        QuerySubcommand::Errors(errors_args) => {
            handle_errors(&db, &speedrun_ops, errors_args).await
        }
        QuerySubcommand::Reset(reset_args) => handle_reset(&db, reset_args).await,
        QuerySubcommand::Cleanup(cleanup_args) => {
            handle_cleanup(&db, &speedrun_ops, cleanup_args).await
        }
    }
}

async fn resolve_game_category(
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

async fn handle_list(db: &Database, ops: &SpeedrunOps, args: ListArgs) -> Result<()> {
    let status = args
        .status
        .as_ref()
        .map(|s| parse_status(s))
        .transpose()
        .context("Invalid status value")?;

    let format = formatter::OutputFormat::from_str(&args.format)?;

    let filter = RunFilter {
        status,
        game_id: args.game_id,
        category_id: args.category_id,
        since_date: None,
        limit: args.limit,
        offset: args.offset,
    };

    let runs = db.query_runs(filter).await?;

    if runs.is_empty() {
        match format {
            formatter::OutputFormat::Json => println!("[]"),
            formatter::OutputFormat::Csv => {
                println!(
                    "run_id,game_id,game_name,category_id,category_name,submitted_date,status,retry_count,error_class,error_message,next_retry_at,created_at,updated_at"
                )
            }
            formatter::OutputFormat::Table => {
                println!("No runs found matching the criteria")
            }
        }
        return Ok(());
    }

    let output = match format {
        formatter::OutputFormat::Table | formatter::OutputFormat::Csv => {
            let mut run_displays = Vec::new();
            for run in &runs {
                let (game_name, category_name) =
                    resolve_game_category(ops, &run.game_id, &run.category_id).await;
                run_displays.push(formatter::RunDisplay {
                    run,
                    game_name,
                    category_name,
                });
            }
            match format {
                formatter::OutputFormat::Table => formatter::format_runs_as_table(&run_displays),
                formatter::OutputFormat::Csv => formatter::format_runs_as_csv(&run_displays)?,
                _ => unreachable!(),
            }
        }
        formatter::OutputFormat::Json => formatter::format_runs_as_json(&runs)?,
    };

    println!("{}", output);
    Ok(())
}

fn parse_status(s: &str) -> Result<RunStatus> {
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

fn format_status(status: &RunStatus) -> String {
    match status {
        RunStatus::Discovered => "discovered".to_string(),
        RunStatus::Processing => "processing".to_string(),
        RunStatus::Passed => "passed".to_string(),
        RunStatus::NeedsReview => "needs_review".to_string(),
        RunStatus::Failed => "failed".to_string(),
        RunStatus::Error => "error".to_string(),
    }
}

fn parse_datetime(date_str: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    if let Ok(dt) = date_str.parse() {
        return Ok(dt);
    }

    let with_time = format!("{}T00:00:00Z", date_str);
    with_time.parse().context(
        "Invalid date format. Expected ISO 8601 format (e.g., 2025-01-01T00:00:00Z or 2025-01-01)",
    )
}

fn group_and_display<F>(
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

async fn handle_show(db: &Database, ops: &SpeedrunOps, args: ShowArgs) -> Result<()> {
    let run = db
        .get_run(&args.run_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Run not found: {}", args.run_id))?;

    if args.format == "json" {
        println!("{}", formatter::format_run_as_json(&run)?);
        return Ok(());
    }

    let (game_name, category_name) =
        resolve_game_category(ops, &run.game_id, &run.category_id).await;

    println!("Run Details");
    println!("===========");
    println!();
    println!("Run ID:          {}", run.run_id);
    println!("Game:            {} ({})", game_name, run.game_id);
    println!("Category:        {} ({})", category_name, run.category_id);
    println!(
        "Submitted:       {}",
        run.submitted_date.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("Status:          {}", format_status(&run.status));
    println!("Retry Count:     {}", run.retry_count);

    if let Some(error_class) = &run.error_class {
        println!("Error Class:     {}", error_class);
    }

    if let Some(next_retry) = run.next_retry_at {
        println!(
            "Next Retry:      {}",
            next_retry.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    if let Some(error_msg) = &run.error_message {
        println!();
        println!("Error Message:");
        println!("{}", error_msg);
    }

    println!();
    println!(
        "Created:         {}",
        run.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "Updated:         {}",
        run.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!();
    println!("Speedrun.com:    https://speedrun.com/runs/{}", run.run_id);

    Ok(())
}

async fn handle_stats(db: &Database, args: StatsArgs) -> Result<()> {
    let since_date = args.since.as_ref().map(|s| parse_datetime(s)).transpose()?;

    let counts = db.count_runs_by_status().await?;

    let mut filter = RunFilter {
        limit: 1000000,
        ..Default::default()
    };

    if let Some(game_id) = args.game_id {
        filter.game_id = Some(game_id);
    }
    if let Some(category_id) = args.category_id {
        filter.category_id = Some(category_id);
    }
    if let Some(since) = since_date {
        filter.since_date = Some(since);
    }

    let all_runs = db.query_runs(filter).await?;

    let total = all_runs.len();
    let discovered = counts.get(&RunStatus::Discovered).unwrap_or(&0);
    let processing = counts.get(&RunStatus::Processing).unwrap_or(&0);
    let passed = counts.get(&RunStatus::Passed).unwrap_or(&0);
    let needs_review = counts.get(&RunStatus::NeedsReview).unwrap_or(&0);
    let failed = counts.get(&RunStatus::Failed).unwrap_or(&0);
    let error = counts.get(&RunStatus::Error).unwrap_or(&0);

    let retry_counts: Vec<u32> = all_runs.iter().map(|r| r.retry_count).collect();
    let avg_retries = if !retry_counts.is_empty() {
        retry_counts.iter().sum::<u32>() as f64 / retry_counts.len() as f64
    } else {
        0.0
    };
    let max_retries = retry_counts.iter().max().unwrap_or(&0);

    let error_counts: std::collections::HashMap<String, usize> = all_runs
        .iter()
        .filter_map(|r| r.error_class.as_ref())
        .fold(std::collections::HashMap::new(), |mut acc, class| {
            *acc.entry(class.clone()).or_insert(0) += 1;
            acc
        });

    println!("Run Statistics");
    println!("==============");
    println!();
    println!("Total Runs:      {}", total);
    println!();
    println!("By Status:");
    println!("  Discovered:    {}", discovered);
    println!("  Processing:    {}", processing);
    println!("  Passed:        {}", passed);
    println!("  Needs Review:  {}", needs_review);
    println!("  Failed:        {}", failed);
    println!("  Error:         {}", error);
    println!();
    println!("Retry Statistics:");
    println!("  Average:       {:.2}", avg_retries);
    println!("  Maximum:       {}", max_retries);

    if !error_counts.is_empty() {
        println!();
        println!("Error Classes:");
        for (class, count) in error_counts {
            println!("  {:<15} {}", class, count);
        }
    }

    Ok(())
}

async fn handle_queue(db: &Database, _args: QueueArgs) -> Result<()> {
    let discovered_filter = RunFilter {
        status: Some(RunStatus::Discovered),
        limit: 1000,
        ..Default::default()
    };
    let discovered_runs = db.query_runs(discovered_filter).await?;

    let retry_filter = RunFilter {
        status: Some(RunStatus::Error),
        limit: 1000,
        ..Default::default()
    };
    let error_runs = db.query_runs(retry_filter).await?;
    let retry_scheduled: Vec<_> = error_runs
        .iter()
        .filter(|r| r.next_retry_at.is_some())
        .collect();

    println!("Queue Status");
    println!("============");
    println!();
    println!("Pending Runs (Discovered):  {}", discovered_runs.len());
    println!("Scheduled Retries:          {}", retry_scheduled.len());

    if let Some(next_retry) = retry_scheduled.iter().filter_map(|r| r.next_retry_at).min() {
        println!(
            "Next Retry At:              {}",
            next_retry.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    Ok(())
}

async fn handle_errors(db: &Database, ops: &SpeedrunOps, args: ErrorsArgs) -> Result<()> {
    let filter = RunFilter {
        status: Some(RunStatus::Error),
        limit: 1000,
        ..Default::default()
    };

    let mut error_runs = db.query_runs(filter).await?;

    if let Some(error_class) = &args.error_class {
        error_runs.retain(|r| r.error_class.as_deref() == Some(error_class));
    }

    if error_runs.is_empty() {
        println!("No error runs found");
        return Ok(());
    }

    if let Some(group_by) = &args.group_by {
        handle_grouped_errors(ops, &error_runs, group_by, args.limit).await
    } else {
        handle_ungrouped_errors(ops, &error_runs, args.limit).await
    }
}

async fn handle_ungrouped_errors(ops: &SpeedrunOps, error_runs: &[Run], limit: u32) -> Result<()> {
    println!("Recent Errors");
    println!("=============");
    println!();

    for run in error_runs.iter().take(limit as usize) {
        let (game_name, category_name) =
            resolve_game_category(ops, &run.game_id, &run.category_id).await;
        println!("Run ID:       {}", run.run_id);
        println!("Game/Cat:     {} / {}", game_name, category_name);
        println!(
            "Error Class:  {}",
            run.error_class.as_deref().unwrap_or("N/A")
        );
        println!("Retry Count:  {}", run.retry_count);
        if let Some(error_msg) = &run.error_message {
            println!("Error:        {}", error_msg);
        }
        println!();
    }

    Ok(())
}

async fn handle_grouped_errors(
    ops: &SpeedrunOps,
    error_runs: &[Run],
    group_by: &str,
    limit: u32,
) -> Result<()> {
    use std::collections::{HashMap, HashSet};

    let unique_pairs: HashSet<(String, String)> = error_runs
        .iter()
        .map(|r| (r.game_id.clone(), r.category_id.clone()))
        .collect();

    let mut names_map: HashMap<(String, String), (String, String)> = HashMap::new();
    for (game_id, category_id) in unique_pairs {
        let (game_name, category_name) = resolve_game_category(ops, &game_id, &category_id).await;
        names_map.insert((game_id, category_id), (game_name, category_name));
    }

    match group_by {
        "message" => group_by_error_message(error_runs, &names_map, limit),
        "class" => group_by_error_class(error_runs, &names_map, limit),
        "category" => group_by_category(error_runs, &names_map, limit),
        _ => Err(anyhow::anyhow!(
            "Invalid group-by value: '{}'. Valid options are: message, class, category",
            group_by
        )),
    }
}

fn group_by_error_message(
    error_runs: &[Run],
    names_map: &std::collections::HashMap<(String, String), (String, String)>,
    limit: u32,
) -> Result<()> {
    group_and_display(
        error_runs,
        limit,
        "Errors Grouped by Message",
        "unique error messages",
        |run| {
            run.error_message
                .as_deref()
                .unwrap_or("(no message)")
                .to_string()
        },
        |message, runs| {
            println!("Count: {} runs", runs.len());
            println!("Message: {}", message);
            println!("Example runs:");
            for run in runs.iter().take(3) {
                let (game_name, category_name) = names_map
                    .get(&(run.game_id.clone(), run.category_id.clone()))
                    .map(|(g, c)| (g.as_str(), c.as_str()))
                    .unwrap_or((&run.game_id, &run.category_id));
                println!("  - {} ({}/{})", run.run_id, game_name, category_name);
            }
            if runs.len() > 3 {
                println!("  ... and {} more", runs.len() - 3);
            }
            println!();
        },
    )
}

fn group_by_error_class(
    error_runs: &[Run],
    names_map: &std::collections::HashMap<(String, String), (String, String)>,
    limit: u32,
) -> Result<()> {
    group_and_display(
        error_runs,
        limit,
        "Errors Grouped by Class",
        "unique error classes",
        |run| {
            run.error_class
                .as_deref()
                .unwrap_or("(no class)")
                .to_string()
        },
        |class, runs| {
            println!("Class: {}", class);
            println!("Count: {} runs", runs.len());
            println!("Example runs:");
            for run in runs.iter().take(3) {
                let (game_name, category_name) = names_map
                    .get(&(run.game_id.clone(), run.category_id.clone()))
                    .map(|(g, c)| (g.as_str(), c.as_str()))
                    .unwrap_or((&run.game_id, &run.category_id));
                println!("  - {} ({}/{})", run.run_id, game_name, category_name);
                if let Some(msg) = &run.error_message {
                    let preview = msg.chars().take(60).collect::<String>();
                    println!(
                        "    {}",
                        if msg.len() > 60 {
                            format!("{}...", preview)
                        } else {
                            preview
                        }
                    );
                }
            }
            if runs.len() > 3 {
                println!("  ... and {} more", runs.len() - 3);
            }
            println!();
        },
    )
}

fn group_by_category(
    error_runs: &[Run],
    names_map: &std::collections::HashMap<(String, String), (String, String)>,
    limit: u32,
) -> Result<()> {
    use std::collections::HashMap;

    group_and_display(
        error_runs,
        limit,
        "Errors Grouped by Game/Category",
        "unique game/categories",
        |run| format!("{}/{}", run.game_id, run.category_id),
        |_category, runs| {
            if let Some(first_run) = runs.first() {
                let (game_name, category_name) = names_map
                    .get(&(first_run.game_id.clone(), first_run.category_id.clone()))
                    .map(|(g, c)| (g.as_str(), c.as_str()))
                    .unwrap_or((&first_run.game_id, &first_run.category_id));

                println!("Game/Category: {} / {}", game_name, category_name);
                println!("Count: {} runs", runs.len());

                let mut error_class_counts: HashMap<String, usize> = HashMap::new();
                for run in runs.iter() {
                    let class = run.error_class.as_deref().unwrap_or("(no class)");
                    *error_class_counts.entry(class.to_string()).or_insert(0) += 1;
                }

                println!("Error classes:");
                let mut class_list: Vec<_> = error_class_counts.iter().collect();
                class_list.sort_by(|a, b| b.1.cmp(a.1));
                for (class, count) in class_list {
                    println!("  - {}: {} runs", class, count);
                }

                println!("Example runs:");
                for run in runs.iter().take(3) {
                    println!(
                        "  - {} ({})",
                        run.run_id,
                        run.error_class.as_deref().unwrap_or("N/A")
                    );
                }
                if runs.len() > 3 {
                    println!("  ... and {} more", runs.len() - 3);
                }
                println!();
            }
        },
    )
}

async fn handle_reset(db: &Database, args: ResetArgs) -> Result<()> {
    let _run = db
        .get_run(&args.run_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Run not found: {}", args.run_id))?;

    db.update_run_status(&args.run_id, RunStatus::Discovered, None)
        .await?;

    if args.clear_error {
        db.clear_retry_fields(&args.run_id).await?;
    }

    println!("Reset run {} to discovered status", args.run_id);
    if args.clear_error {
        println!("Cleared retry fields");
    }

    Ok(())
}

async fn handle_cleanup(db: &Database, ops: &SpeedrunOps, args: CleanupArgs) -> Result<()> {
    let before_date = args
        .before
        .as_ref()
        .map(|s| parse_datetime(s))
        .transpose()?;

    let status = args
        .status
        .as_ref()
        .map(|s| parse_status(s))
        .transpose()
        .context("Invalid status value")?;

    if before_date.is_none() && status.is_none() {
        return Err(anyhow::anyhow!(
            "At least one filter must be specified (--before or --status)"
        ));
    }

    let runs_to_delete = db.query_runs_for_deletion(before_date, status).await?;

    if runs_to_delete.is_empty() {
        println!("No runs match the specified criteria");
        return Ok(());
    }

    use std::collections::{HashMap, HashSet};

    let unique_pairs: HashSet<(String, String)> = runs_to_delete
        .iter()
        .map(|r| (r.game_id.clone(), r.category_id.clone()))
        .collect();

    let mut names_map: HashMap<(String, String), (String, String)> = HashMap::new();
    for (game_id, category_id) in unique_pairs {
        let (game_name, category_name) = resolve_game_category(ops, &game_id, &category_id).await;
        names_map.insert((game_id, category_id), (game_name, category_name));
    }

    println!(
        "Found {} run(s) matching the criteria:",
        runs_to_delete.len()
    );
    println!();

    for run in &runs_to_delete {
        let (game_name, category_name) = names_map
            .get(&(run.game_id.clone(), run.category_id.clone()))
            .map(|(g, c)| (g.as_str(), c.as_str()))
            .unwrap_or((&run.game_id, &run.category_id));
        println!(
            "  {} - {} / {} - {}",
            run.run_id,
            game_name,
            category_name,
            format_status(&run.status)
        );
    }
    println!();

    if args.dry_run {
        println!("Dry run mode - no runs were deleted");
        return Ok(());
    }

    if !args.force {
        println!(
            "Are you sure you want to delete {} run(s)? (y/N): ",
            runs_to_delete.len()
        );
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .context("Failed to read user input")?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Deletion cancelled");
            return Ok(());
        }
    }

    let run_ids: Vec<String> = runs_to_delete.iter().map(|r| r.run_id.clone()).collect();
    let deleted_count = db.delete_runs(&run_ids).await?;

    println!("Successfully deleted {} run(s)", deleted_count);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::database::types::Run;
    use chrono::Utc;

    fn create_test_run(
        run_id: &str,
        game_id: &str,
        category_id: &str,
        error_class: Option<&str>,
        error_message: Option<&str>,
    ) -> Run {
        let now = Utc::now();
        Run {
            run_id: run_id.to_string(),
            game_id: game_id.to_string(),
            category_id: category_id.to_string(),
            submitted_date: now,
            status: RunStatus::Error,
            error_message: error_message.map(String::from),
            retry_count: 0,
            next_retry_at: None,
            error_class: error_class.map(String::from),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn test_group_by_error_message() {
        use std::collections::HashMap;

        let runs = vec![
            create_test_run("run1", "game1", "cat1", Some("final"), Some("error A")),
            create_test_run("run2", "game1", "cat1", Some("final"), Some("error A")),
            create_test_run("run3", "game1", "cat1", Some("final"), Some("error B")),
        ];

        let mut names_map = HashMap::new();
        names_map.insert(
            ("game1".to_string(), "cat1".to_string()),
            ("Game 1".to_string(), "Category 1".to_string()),
        );

        let result = group_by_error_message(&runs, &names_map, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_group_by_error_class() {
        use std::collections::HashMap;

        let runs = vec![
            create_test_run("run1", "game1", "cat1", Some("final"), Some("error A")),
            create_test_run("run2", "game1", "cat1", Some("retryable"), Some("error B")),
            create_test_run("run3", "game1", "cat1", Some("final"), Some("error C")),
        ];

        let mut names_map = HashMap::new();
        names_map.insert(
            ("game1".to_string(), "cat1".to_string()),
            ("Game 1".to_string(), "Category 1".to_string()),
        );

        let result = group_by_error_class(&runs, &names_map, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_group_by_category() {
        use std::collections::HashMap;

        let runs = vec![
            create_test_run("run1", "game1", "cat1", Some("final"), Some("error A")),
            create_test_run("run2", "game1", "cat1", Some("final"), Some("error B")),
            create_test_run("run3", "game2", "cat2", Some("retryable"), Some("error C")),
        ];

        let mut names_map = HashMap::new();
        names_map.insert(
            ("game1".to_string(), "cat1".to_string()),
            ("Game 1".to_string(), "Category 1".to_string()),
        );
        names_map.insert(
            ("game2".to_string(), "cat2".to_string()),
            ("Game 2".to_string(), "Category 2".to_string()),
        );

        let result = group_by_category(&runs, &names_map, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_group_by_handles_none_values() {
        use std::collections::HashMap;

        let runs = vec![
            create_test_run("run1", "game1", "cat1", None, None),
            create_test_run("run2", "game1", "cat1", Some("final"), Some("error")),
        ];

        let names_map = HashMap::new();

        assert!(group_by_error_message(&runs, &names_map, 10).is_ok());
        assert!(group_by_error_class(&runs, &names_map, 10).is_ok());
        assert!(group_by_category(&runs, &names_map, 10).is_ok());
    }

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
