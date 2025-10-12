use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::database::connection::Database;

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
}

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

#[derive(Args)]
pub struct ShowArgs {
    pub run_id: String,
}

#[derive(Args)]
pub struct StatsArgs {
    #[arg(long)]
    pub game_id: Option<String>,

    #[arg(long)]
    pub category_id: Option<String>,
}

pub async fn handle_query_command(args: QueryArgs) -> Result<()> {
    let db = Database::new(&args.database).await?;

    match args.subcommand {
        QuerySubcommand::List(list_args) => handle_list(&db, list_args).await,
        QuerySubcommand::Show(show_args) => handle_show(&db, show_args).await,
        QuerySubcommand::Stats(stats_args) => handle_stats(&db, stats_args).await,
    }
}

async fn handle_list(_db: &Database, _args: ListArgs) -> Result<()> {
    println!("List command - not yet implemented");
    Ok(())
}

async fn handle_show(_db: &Database, _args: ShowArgs) -> Result<()> {
    println!("Show command - not yet implemented");
    Ok(())
}

async fn handle_stats(_db: &Database, _args: StatsArgs) -> Result<()> {
    println!("Stats command - not yet implemented");
    Ok(())
}
