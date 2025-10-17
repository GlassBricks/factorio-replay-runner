use anyhow::Result;
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::database::types::{RunFilter, RunStatus};

use super::common::parse_datetime;

#[derive(Args)]
pub struct StatsArgs {
    #[arg(long)]
    pub game_id: Option<String>,

    #[arg(long)]
    pub category_id: Option<String>,

    #[arg(long)]
    pub since: Option<String>,
}

pub async fn handle_stats(db: &Database, args: StatsArgs) -> Result<()> {
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
