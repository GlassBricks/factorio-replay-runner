use anyhow::Result;
use replay_runner::rules::{RunRules, SrcRunRules};
use replay_runner::save_file::SaveFile;
use run_downloader::FileDownloader;
use speedrun_api::SpeedrunApiClientAsync;
use speedrun_api::api::AsyncQuery;
use std::fs::File;
use std::path::Path;

use replay_runner::factorio_install_dir::FactorioInstallDir;
use replay_runner::replay_runner::ReplayLog;

use crate::run_replay;

pub type RemoteReplayResult = anyhow::Result<ReplayLog>;

pub async fn run_replay_from_src_run(
    downloader: &mut FileDownloader,
    run_id: &str,
    factorio_dir: &FactorioInstallDir,
    rules: &SrcRunRules,
    output_dir: &Path,
) -> RemoteReplayResult {
    let working_dir = output_dir.join(run_id);
    std::fs::create_dir_all(&working_dir)?;

    let run = fetch_src_run(run_id).await?;
    let run_rules = select_rules(&run, &rules)?;
    let mut save_file = download_run_from_description(downloader, &working_dir, run).await?;

    let output_path = working_dir.join("output.log");
    let result = run_replay(&factorio_dir, &mut save_file, &run_rules, &output_path).await?;

    Ok(result)
}

async fn download_run_from_description(
    downloader: &mut FileDownloader,
    working_dir: &std::path::PathBuf,
    run: speedrun_api::types::Run<'_>,
) -> Result<SaveFile<File>, anyhow::Error> {
    let description = run
        .comment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Comment with link needed for run {}", run.id))?;
    let save_file = download_run_zip(downloader, &description, working_dir).await?;
    Ok(save_file)
}

async fn download_run_zip(
    downloader: &mut FileDownloader,
    description: &str,
    working_dir: &Path,
) -> Result<SaveFile<File>> {
    let save_file_info = downloader.download_zip(&description, &working_dir).await?;

    let save_path = working_dir.join(save_file_info.name);
    let file = File::open(save_path)?;
    let save_file = SaveFile::new(file)?;
    Ok(save_file)
}

fn select_rules<'a>(
    run: &speedrun_api::types::Run<'_>,
    rules: &'a SrcRunRules,
) -> Result<&'a RunRules> {
    let game_rules = rules
        .games
        .get(&run.game.to_string())
        .ok_or_else(|| anyhow::anyhow!("Game rules not configured for {}", run.game))?;

    let category_rules = game_rules
        .categories
        .get(&run.category.to_string())
        .ok_or_else(|| anyhow::anyhow!("Category rules not configured for {}", run.category))?;

    let rules = &category_rules.run_rules;
    Ok(rules)
}

async fn fetch_src_run(run_id: &str) -> Result<speedrun_api::types::Run> {
    let client = SpeedrunApiClientAsync::new().unwrap();

    let query = speedrun_api::api::runs::Run::builder().id(run_id).build()?;
    let result = query.query_async(&client).await?;
    Ok(result)
}
