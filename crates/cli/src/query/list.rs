use anyhow::Result;
use clap::Args;

use crate::daemon::database::connection::Database;
use crate::daemon::speedrun_api::SpeedrunOps;

use super::common::{RunFilterArgs, query_and_display_runs};

#[derive(Args)]
pub struct ListArgs {
    #[command(flatten)]
    pub filter: RunFilterArgs,
}

pub async fn handle_list(db: &Database, ops: &SpeedrunOps, args: ListArgs) -> Result<()> {
    let filter = args.filter.to_filter()?;
    query_and_display_runs(db, ops, filter).await
}
