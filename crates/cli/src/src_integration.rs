use anyhow::Result;
use factorio_manager::expected_mods::ExpectedMods;
use log::info;

use factorio_manager::save_file::{SaveFile, WrittenSaveFile};
use speedrun_api::api;
use speedrun_api::api::AsyncQuery;
use speedrun_api::SpeedrunApiClientAsync;
use std::fs::File;
use std::path::Path;
use zip_downloader::FileDownloader;

use factorio_manager::factorio_install_dir::FactorioInstallDir;

use crate::config::{RunRules, SrcRunRules};
use crate::run_replay::{ReplayReport, run_replay};

pub async fn run_replay_from_src_run(
    downloader: &mut FileDownloader,
    run_id: &str,
    factorio_dir: &FactorioInstallDir,
    rules: &SrcRunRules,
    output_dir: &Path,
) -> Result<ReplayReport> {
    let working_dir = output_dir.join(run_id);
    std::fs::create_dir_all(&working_dir)?;

    info!("Fetching run data (https://speedrun.com/runs/{})", run_id);
    let run = fetch_src_run(run_id).await?;
    let (run_rules, expected_mods) = select_rules(&run, rules)?;

    info!("Downloading save file");
    let mut save_file = download_save_from_description(downloader, &working_dir, run).await?;

    let output_path = working_dir.join("output.log");
    run_replay(
        factorio_dir,
        &mut save_file,
        run_rules,
        expected_mods,
        &output_path,
    )
    .await
}

async fn download_save_from_description(
    downloader: &mut FileDownloader,
    working_dir: &Path,
    run: speedrun_api::types::Run<'_>,
) -> Result<WrittenSaveFile> {
    let description = run
        .comment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Comment with link needed for run {}", run.id))?;

    // look for link in description
    let save_file_info = downloader.download_zip(description, working_dir).await?;

    let save_path = working_dir.join(save_file_info.name);
    let save_file = SaveFile::new(File::open(&save_path)?)?;
    Ok(WrittenSaveFile(save_path, save_file))
}

fn select_rules<'a>(
    run: &speedrun_api::types::Run<'_>,
    rules: &'a SrcRunRules,
) -> Result<(&'a RunRules, &'a ExpectedMods)> {
    let game_id = run.game.to_string();
    let category_id = run.category.to_string();

    let game_config = rules.games.get(&game_id).ok_or_else(|| {
        anyhow::anyhow!("No rules configured for game ID: {}", game_id)
    })?;

    let category_config = game_config.categories.get(&category_id).ok_or_else(|| {
        anyhow::anyhow!("No rules configured for category ID: {}", category_id)
    })?;

    let game_name = game_config.name.as_deref().unwrap_or(&game_id);
    let category_name = category_config.name.as_deref().unwrap_or(&category_id);
    info!("Game: {}, Category: {}", game_name, category_name);

    let run_rules = &category_config.run_rules;
    let expected_mods = run_rules
        .expected_mods_override
        .as_ref()
        .unwrap_or(&game_config.expected_mods);

    Ok((run_rules, expected_mods))
}

async fn fetch_src_run(run_id: &'_ str) -> Result<speedrun_api::types::Run<'_>> {
    let client = SpeedrunApiClientAsync::new().unwrap();

    let query = api::runs::Run::builder().id(run_id).build()?;
    let result = query.query_async(&client).await?;
    Ok(result)
}
