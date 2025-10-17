use anyhow::{Context, Result};
use clap::Args;
use std::collections::{HashMap, HashSet};

use crate::daemon::database::connection::Database;
use crate::daemon::speedrun_api::SpeedrunOps;

use super::common::{format_status, parse_datetime, parse_status, resolve_game_category};

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

pub async fn handle_cleanup(db: &Database, ops: &SpeedrunOps, args: CleanupArgs) -> Result<()> {
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
