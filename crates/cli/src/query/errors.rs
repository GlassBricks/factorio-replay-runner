use clap::Args;

use super::common::RunFilterArgs;

#[derive(Args)]
pub struct ErrorsArgs {
    #[command(flatten)]
    pub filter: RunFilterArgs,
}

impl ErrorsArgs {
    pub fn into_filter_with_error_status(self) -> RunFilterArgs {
        self.filter.with_status("error")
    }
}
