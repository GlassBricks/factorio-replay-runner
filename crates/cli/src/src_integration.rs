use anyhow::Result;
use factorio_manager::factorio_install_dir::FactorioInstallDir;
use log::info;
use std::path::Path;

use crate::config::SrcRunRules;
use crate::run_processing::RunProcessor;
use crate::run_replay::{ReplayReport, run_replay};

pub async fn run_replay_from_src_run(
    run_id: &str,
    factorio_dir: &FactorioInstallDir,
    rules: &SrcRunRules,
    output_dir: &Path,
) -> Result<ReplayReport> {
    let working_dir = output_dir.join(run_id);
    std::fs::create_dir_all(&working_dir)?;

    info!("Fetching run data (https://speedrun.com/runs/{})", run_id);
    let mut processor = RunProcessor::new()?;

    let (game_id, category_id) = processor.fetch_run_metadata(run_id).await?;

    let game_config = rules
        .games
        .get(&game_id)
        .ok_or_else(|| anyhow::anyhow!("No rules configured for game ID: {}", game_id))?;

    let category_config = game_config.categories.get(&category_id).ok_or_else(|| {
        anyhow::anyhow!("No rules configured for category ID: {}", category_id)
    })?;

    let game_name = game_config.name.as_deref().unwrap_or(&game_id);
    let category_name = category_config.name.as_deref().unwrap_or(&category_id);
    info!("Game: {}, Category: {}", game_name, category_name);

    let (run_rules, expected_mods) = rules.resolve_rules(&game_id, &category_id)?;

    let mut save_file = processor.fetch_and_download_run(run_id, &working_dir).await?;

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
