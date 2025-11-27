use anyhow::Result;
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::speedrun_api::SpeedrunOps;

use super::common::{format_status, resolve_game_category};

#[derive(Args)]
pub struct ShowArgs {
    /// Speedrun.com run ID
    pub run_id: String,
}

pub async fn handle_show(db: &Database, ops: &SpeedrunOps, args: ShowArgs) -> Result<()> {
    let run = db
        .get_run(&args.run_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Run not found: {}", args.run_id))?;

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
