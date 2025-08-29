use anyhow::Result;
use itertools::Itertools;
use log::info;
use replay_runner::expected_mods::ExpectedMods;
use replay_runner::rules::{RunRules, SrcRunRules};
use replay_runner::save_file::SaveFile;
use run_downloader::FileDownloader;
use speedrun_api::api;
use speedrun_api::api::AsyncQuery;
use speedrun_api::{SpeedrunApiClientAsync, types};
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

    info!("Fetching run data ( https://speedrun.com/runs/{} )", run_id);
    let run = fetch_src_run(run_id).await?;
    let (game, category) = fetch_game_and_category(&run).await?;
    let (run_rules, expected_mods) = select_rules(&game, &category, &rules)?;
    info!("Downloading save file");
    let mut save_file = download_run_from_description(downloader, &working_dir, run).await?;

    let output_path = working_dir.join("output.log");
    let result = run_replay(
        &factorio_dir,
        &mut save_file,
        &run_rules,
        expected_mods,
        &output_path,
    )
    .await?;

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
    game: &types::Game<'_>,
    category: &types::Category<'_>,
    rules: &'a SrcRunRules,
) -> Result<(&'a RunRules, &'a ExpectedMods)> {
    let orig_game_name = &game.names.international;
    let orig_category_name = &category.name;
    let game_name = normalize_name(orig_game_name);
    let category_name = normalize_name(orig_category_name);

    let game_rules = rules
        .games
        .iter()
        .find(|(key, _)| normalize_name(key) == game_name)
        .map(|(_, rules)| rules)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Game rules not configured for '{}' (normalized: '{}')",
                game.names.international,
                game_name
            )
        })?;

    let category_rules = game_rules
        .categories
        .iter()
        .find(|(key, _)| normalize_name(key) == category_name)
        .map(|(_, rules)| rules)
        .ok_or_else(|| {
            let existing_categories = game_rules.categories.keys() .join(", ");
            anyhow::anyhow!(
                "Category rules not configured for '{}' (normalized: '{}'). Available categories: {}",
                category.name,
                category_name,
                existing_categories
            )
        })?;

    info!("Game: {}, Category: {}", orig_game_name, orig_category_name);

    let run_rules = &category_rules.run_rules;

    let expected_mods = &run_rules
        .expected_mods_override
        .as_ref()
        .unwrap_or(&game_rules.expected_mods);

    Ok((run_rules, expected_mods))
}

fn normalize_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

async fn fetch_src_run(run_id: &str) -> Result<speedrun_api::types::Run> {
    let client = SpeedrunApiClientAsync::new().unwrap();

    let query = api::runs::Run::builder().id(run_id).build()?;
    let result = query.query_async(&client).await?;
    Ok(result)
}

async fn fetch_game_and_category(
    run: &speedrun_api::types::Run<'_>,
) -> Result<(types::Game<'static>, types::Category<'static>)> {
    let client = SpeedrunApiClientAsync::new().unwrap();

    // Fetch game details
    let game_id = run.game.to_string();
    let game_query = api::games::Game::builder().id(&game_id).build()?;
    let game = game_query.query_async(&client).await?;

    // Fetch category details
    let category_id = run.category.to_string();
    let category_query = api::categories::Category::builder()
        .id(&category_id)
        .build()?;
    let category = category_query.query_async(&client).await?;

    Ok((game, category))
}
