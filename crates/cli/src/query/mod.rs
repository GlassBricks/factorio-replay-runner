use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::daemon::database::connection::Database;
use crate::daemon::speedrun_api::{SpeedrunClient, SpeedrunOps};

pub mod common;
mod errors;
mod list;
mod queue;
mod show;
mod stats;

pub use errors::ErrorsArgs;
pub use list::ListArgs;
pub use queue::QueueArgs;
pub use show::ShowArgs;
pub use stats::StatsArgs;

#[derive(Args)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub subcommand: QuerySubcommand,

    /// SQLite database file path
    #[arg(long, default_value = "run_verification.db")]
    pub database: PathBuf,
}

#[derive(Subcommand)]
pub enum QuerySubcommand {
    /// List runs with optional filters
    List(ListArgs),
    /// Show details for a specific run
    Show(ShowArgs),
    /// Display run statistics
    Stats(StatsArgs),
    /// Show pending and scheduled runs
    Queue(QueueArgs),
    /// Show runs with errors
    Errors(ErrorsArgs),
}

pub async fn handle_query_command(args: QueryArgs) -> Result<()> {
    let db = Database::new(&args.database).await?;
    let speedrun_client = SpeedrunClient::new().context("Failed to create speedrun client")?;
    let speedrun_ops = SpeedrunOps::new(&speedrun_client).with_db(db.clone());

    match args.subcommand {
        QuerySubcommand::List(list_args) => list::handle_list(&db, &speedrun_ops, list_args).await,
        QuerySubcommand::Show(show_args) => show::handle_show(&db, &speedrun_ops, show_args).await,
        QuerySubcommand::Stats(stats_args) => stats::handle_stats(&db, stats_args).await,
        QuerySubcommand::Queue(queue_args) => queue::handle_queue(&db, queue_args).await,
        QuerySubcommand::Errors(errors_args) => {
            let filter = errors_args.into_filter_with_error_status().to_filter()?;
            common::query_and_display_runs(&db, &speedrun_ops, filter).await
        }
    }
}
