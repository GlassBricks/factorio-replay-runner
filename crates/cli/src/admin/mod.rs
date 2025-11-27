use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::daemon::database::connection::Database;
use crate::daemon::speedrun_api::{SpeedrunClient, SpeedrunOps};

mod cleanup;
mod reset;

pub use cleanup::CleanupArgs;
pub use reset::{ResetArgs, ResetRunArgs};

#[derive(Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub subcommand: AdminSubcommand,

    /// SQLite database file path
    #[arg(long, default_value = "run_verification.db")]
    pub database: PathBuf,
}

#[derive(Subcommand)]
pub enum AdminSubcommand {
    /// Reset a single run to discovered status
    ResetRun(ResetRunArgs),
    /// Reset runs matching query to discovered status
    Reset(ResetArgs),
    /// Delete runs matching criteria
    Cleanup(CleanupArgs),
}

pub async fn handle_admin_command(args: AdminArgs) -> Result<()> {
    let db = Database::new(&args.database).await?;
    let speedrun_client = SpeedrunClient::new().context("Failed to create speedrun client")?;
    let speedrun_ops = SpeedrunOps::new(&speedrun_client).with_db(db.clone());

    match args.subcommand {
        AdminSubcommand::ResetRun(reset_args) => reset::handle_reset_run(&db, reset_args).await,
        AdminSubcommand::Reset(reset_args) => reset::handle_reset(&db, reset_args).await,
        AdminSubcommand::Cleanup(cleanup_args) => {
            cleanup::handle_cleanup(&db, &speedrun_ops, cleanup_args).await
        }
    }
}
