use anyhow::Result;
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::database::types::RunStatus;
use crate::query::common::RunFilterArgs;

#[derive(Args)]
pub struct ResetRunArgs {
    /// Speedrun.com run ID
    pub run_id: String,

    /// Also clear error message and retry count
    #[arg(long)]
    pub clear_error: bool,
}

pub async fn handle_reset_run(db: &Database, args: ResetRunArgs) -> Result<()> {
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

#[derive(Args)]
pub struct ResetArgs {
    #[command(flatten)]
    pub filter: RunFilterArgs,

    /// Also clear error message and retry count
    #[arg(long)]
    pub clear_error: bool,
}

pub async fn handle_reset(db: &Database, args: ResetArgs) -> Result<()> {
    let filter = args.filter.with_unlimited().to_filter()?;
    let runs = db.query_runs(filter).await?;

    if runs.is_empty() {
        println!("No runs found matching the criteria");
        return Ok(());
    }

    println!("Resetting {} runs to discovered status...", runs.len());

    for run in &runs {
        db.update_run_status(&run.run_id, RunStatus::Discovered, None)
            .await?;

        if args.clear_error {
            db.clear_retry_fields(&run.run_id).await?;
        }
    }

    println!("Reset {} runs", runs.len());
    if args.clear_error {
        println!("Cleared retry fields");
    }

    Ok(())
}
