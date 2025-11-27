use anyhow::{Context, Result};
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::database::types::RunStatus;
use crate::daemon::speedrun_api::SpeedrunOps;
use crate::query::common::{
    RunDisplay, RunFilterArgs, format_runs_as_table, resolve_game_category,
};

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

    /// Skip confirmation prompt
    #[arg(long)]
    pub force: bool,
}

pub async fn handle_reset(db: &Database, ops: &SpeedrunOps, args: ResetArgs) -> Result<()> {
    let filter = args.filter.to_filter()?;
    let runs = db.query_runs(filter).await?;

    if runs.is_empty() {
        println!("No runs found matching the criteria");
        return Ok(());
    }

    let mut run_displays = Vec::new();
    for run in &runs {
        let (game_name, category_name) =
            resolve_game_category(ops, &run.game_id, &run.category_id).await;
        run_displays.push(RunDisplay {
            run,
            game_name,
            category_name,
        });
    }

    println!("Found {} run(s) matching the criteria:\n", runs.len());
    println!("{}\n", format_runs_as_table(&run_displays));

    if !args.force {
        println!(
            "Are you sure you want to reset {} run(s) to discovered status? (y/N): ",
            runs.len()
        );
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .context("Failed to read user input")?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Reset cancelled");
            return Ok(());
        }
    }

    for run in &runs {
        db.update_run_status(&run.run_id, RunStatus::Discovered, None)
            .await?;

        if args.clear_error {
            db.clear_retry_fields(&run.run_id).await?;
        }
    }

    println!("Reset {} run(s)", runs.len());
    if args.clear_error {
        println!("Cleared retry fields");
    }

    Ok(())
}
