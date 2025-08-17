use anyhow::Result;
use replay_runner::rules::{GameRules, RunRules};
use replay_runner::save_file::SaveFile;
use run_downloader::FileDownloader;
use speedrun_api::SpeedrunApiClientAsync;
use speedrun_api::api::AsyncQuery;
use std::fs::File;
use std::path::Path;

use replay_runner::factorio_install_dir::FactorioInstallDir;
use replay_runner::replay_runner::ReplayLog;

pub type RemoteReplayResult = anyhow::Result<ReplayLog>;

pub async fn run_replay_from_src_run(
    downloader: &mut FileDownloader,
    run_id: &str,
    factorio_dir: &FactorioInstallDir,
    rules: &GameRules,
    output_dir: &Path,
) -> RemoteReplayResult {
    let working_dir = output_dir.join(run_id);
    let run = get_src_run(run_id).await?;
    let description = run
        .comment
        .ok_or_else(|| anyhow::anyhow!("Comment with link needed for run {}", run_id))?;

    // let rules: RunRules = select_rules(run, rules);

    std::fs::create_dir_all(&working_dir)?;
    let save_file_info = downloader.download_zip(&description, &working_dir).await?;

    let save_path = working_dir.join(save_file_info.name);
    let file = File::open(save_path)?;

    let save_file = SaveFile::new(file)?;

    todo!()
}

// fn select_rules(run: speedrun_api::types::Run<'_>, rules: &GameRules) -> RunRules {
//     rules.
// }

async fn get_src_run(run_id: &str) -> Result<speedrun_api::types::Run> {
    let client = SpeedrunApiClientAsync::new().unwrap();

    let query = speedrun_api::api::runs::Run::builder().id(run_id).build()?;
    let result = query.query_async(&client).await?;
    Ok(result)
}
