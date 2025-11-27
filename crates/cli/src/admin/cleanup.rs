use anyhow::{Context, Result};
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::speedrun_api::SpeedrunOps;
use crate::query::common::{
    RunDisplay, RunFilterArgs, format_runs_as_table, resolve_game_category,
};

#[derive(Args)]
pub struct CleanupArgs {
    #[command(flatten)]
    pub filter: RunFilterArgs,

    /// Show what would be deleted without actually deleting
    #[arg(long)]
    pub dry_run: bool,

    /// Skip confirmation prompt
    #[arg(long)]
    pub force: bool,
}

pub async fn handle_cleanup(db: &Database, ops: &SpeedrunOps, args: CleanupArgs) -> Result<()> {
    if !args.filter.has_any_filter() {
        return Err(anyhow::anyhow!(
            "At least one filter must be specified (--older-than, --newer-than, or --status)"
        ));
    }

    let filter = args.filter.with_unlimited().to_filter()?;
    let runs_to_delete = db.query_runs(filter).await?;

    if runs_to_delete.is_empty() {
        println!("No runs match the specified criteria");
        return Ok(());
    }

    let mut run_displays = Vec::new();
    for run in &runs_to_delete {
        let (game_name, category_name) =
            resolve_game_category(ops, &run.game_id, &run.category_id).await;
        run_displays.push(RunDisplay {
            run,
            game_name,
            category_name,
        });
    }

    println!(
        "Found {} run(s) matching the criteria:\n",
        runs_to_delete.len()
    );
    println!("{}\n", format_runs_as_table(&run_displays));

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
