use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::daemon::database::connection::Database;
use crate::daemon::speedrun_api::{SpeedrunClient, SpeedrunOps};

mod cleanup;
mod common;
mod errors;
mod list;
mod queue;
mod reset;
mod show;
mod stats;

pub use cleanup::CleanupArgs;
pub use errors::ErrorsArgs;
pub use list::ListArgs;
pub use queue::QueueArgs;
pub use reset::ResetArgs;
pub use show::ShowArgs;
pub use stats::StatsArgs;

#[derive(Args)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub subcommand: QuerySubcommand,

    #[arg(long, default_value = "run_verification.db")]
    pub database: PathBuf,
}

#[derive(Subcommand)]
pub enum QuerySubcommand {
    List(ListArgs),
    Show(ShowArgs),
    Stats(StatsArgs),
    Queue(QueueArgs),
    Errors(ErrorsArgs),
    Reset(ResetArgs),
    Cleanup(CleanupArgs),
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
            errors::handle_errors(&db, &speedrun_ops, errors_args).await
        }
        QuerySubcommand::Reset(reset_args) => reset::handle_reset(&db, reset_args).await,
        QuerySubcommand::Cleanup(cleanup_args) => {
            cleanup::handle_cleanup(&db, &speedrun_ops, cleanup_args).await
        }
    }
}
