use anyhow::Result;
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::database::types::RunStatus;

#[derive(Args)]
pub struct ResetArgs {
    pub run_id: String,

    #[arg(long)]
    pub clear_error: bool,
}

pub async fn handle_reset(db: &Database, args: ResetArgs) -> Result<()> {
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
