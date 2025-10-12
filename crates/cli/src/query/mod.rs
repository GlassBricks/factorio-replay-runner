use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::database::connection::Database;
use crate::database::types::{RunFilter, RunStatus};

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
}

pub async fn handle_query_command(args: QueryArgs) -> Result<()> {
    let db = Database::new(&args.database).await?;

    match args.subcommand {
        QuerySubcommand::List(list_args) => handle_list(&db, list_args).await,
        QuerySubcommand::Show(show_args) => handle_show(&db, show_args).await,
        QuerySubcommand::Stats(stats_args) => handle_stats(&db, stats_args).await,
    }
}

async fn handle_list(db: &Database, args: ListArgs) -> Result<()> {
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
        limit: args.limit,
        offset: args.offset,
    };

    let runs = db.query_runs(filter).await?;

    if runs.is_empty() {
        match format {
            formatter::OutputFormat::Json => println!("[]"),
            formatter::OutputFormat::Csv => {
                println!(
                    "run_id,game_id,category_id,submitted_date,status,retry_count,error_class,error_message,next_retry_at,created_at,updated_at"
                )
            }
            formatter::OutputFormat::Table => {
                println!("No runs found matching the criteria")
            }
        }
        return Ok(());
    }

    let output = match format {
        formatter::OutputFormat::Table => formatter::format_runs_as_table(&runs),
        formatter::OutputFormat::Json => formatter::format_runs_as_json(&runs)?,
        formatter::OutputFormat::Csv => formatter::format_runs_as_csv(&runs)?,
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

async fn handle_show(db: &Database, args: ShowArgs) -> Result<()> {
    let run = db
        .get_run(&args.run_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Run not found: {}", args.run_id))?;

    if args.format == "json" {
        println!("{}", formatter::format_run_as_json(&run)?);
        return Ok(());
    }

    println!("Run Details");
    println!("===========");
    println!();
    println!("Run ID:          {}", run.run_id);
    println!("Game ID:         {}", run.game_id);
    println!("Category ID:     {}", run.category_id);
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
