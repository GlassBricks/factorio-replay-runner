use anyhow::{Context, Result};
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::database::types::RunFilter;
use crate::daemon::speedrun_api::SpeedrunOps;

use super::common::{RunDisplay, format_runs_as_table, parse_status, resolve_game_category};

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
}

pub async fn handle_list(db: &Database, ops: &SpeedrunOps, args: ListArgs) -> Result<()> {
    let status = args
        .status
        .as_ref()
        .map(|s| parse_status(s))
        .transpose()
        .context("Invalid status value")?;

    let filter = RunFilter {
        status,
        game_id: args.game_id,
        category_id: args.category_id,
        since_date: None,
        limit: args.limit,
        offset: args.offset,
    };

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

    let output = format_runs_as_table(&run_displays);
    println!("{}", output);
    Ok(())
}
