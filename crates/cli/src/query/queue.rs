use anyhow::Result;
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::database::types::{RunFilter, RunStatus};

#[derive(Args)]
pub struct QueueArgs {}

pub async fn handle_queue(db: &Database, _args: QueueArgs) -> Result<()> {
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
