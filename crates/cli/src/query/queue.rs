use anyhow::Result;
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::database::types::{RunFilter, RunStatus};

#[derive(Args)]
pub struct QueueArgs {
    // No arguments needed
}

pub async fn handle_queue(db: &Database, _args: QueueArgs) -> Result<()> {
    let discovered_filter = RunFilter {
        status: Some(RunStatus::Discovered),
        ..Default::default()
    };
    let discovered_runs = db.query_runs(discovered_filter).await?;

    let retry_filter = RunFilter {
        status: Some(RunStatus::Error),
        ..Default::default()
    };
    let error_runs = db.query_runs(retry_filter).await?;
    let retry_scheduled: Vec<_> = error_runs
        .iter()
        .filter(|r| r.next_retry_at.is_some())
        .collect();

    println!("=== Queue ===");
    println!("Pending Runs:      {}", discovered_runs.len());
    println!("Scheduled Retries: {}", retry_scheduled.len());

    if let Some(next_retry) = retry_scheduled.iter().filter_map(|r| r.next_retry_at).min() {
        let local_time = next_retry.with_timezone(&chrono::Local);
        println!(
            "Next Retry At:     {}",
            local_time.format("%Y-%m-%d %H:%M:%S %Z")
        );
    }
    Ok(())
}
