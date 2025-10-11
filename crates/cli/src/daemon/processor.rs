use anyhow::{Context, Result};
use log::{error, info, warn};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Notify, watch};
use zip_downloader::FileDownloader;
use zip_downloader::services::dropbox::DropboxService;
use zip_downloader::services::gdrive::GoogleDriveService;
use zip_downloader::services::speedrun::SpeedrunService;

use crate::config::{GameConfig, RunRules};
use crate::database::connection::Database;
use crate::database::operations::{
    get_next_discovered_run, mark_run_error, mark_run_failed, mark_run_needs_review,
    mark_run_passed, mark_run_processing,
};
use crate::database::types::Run;
use crate::run_replay::{ReplayReport, run_replay};
use factorio_manager::expected_mods::ExpectedMods;
use factorio_manager::factorio_install_dir::FactorioInstallDir;
use factorio_manager::save_file::{SaveFile, WrittenSaveFile};
use replay_script::MsgLevel;

#[derive(Debug)]
pub enum ProcessResult {
    Processed,
    NoWork,
}

pub async fn process_runs_loop(
    db: Database,
    game_configs: HashMap<String, GameConfig>,
    install_dir: PathBuf,
    output_dir: PathBuf,
    work_notify: Arc<Notify>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    info!("Starting run processor (event-driven)");

    loop {
        loop {
            match find_run_to_process(&db, &game_configs, &install_dir, &output_dir).await {
                Ok(ProcessResult::Processed) => continue,
                Ok(ProcessResult::NoWork) => break,
                Err(e) => {
                    error!("Run processing iteration failed: {:#}", e);
                    break;
                }
            }
        }

        info!("No work available - run processor sleeping");

        tokio::select! {
            _ = work_notify.notified() => {
                info!("Run processor woken - checking for work");
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("Run processor shutting down");
                    return Ok(());
                }
            }
        }
    }
}

async fn find_run_to_process(
    db: &Database,
    game_configs: &HashMap<String, GameConfig>,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ProcessResult> {
    let run = match get_next_discovered_run(db).await? {
        Some(run) => run,
        None => return Ok(ProcessResult::NoWork),
    };

    let (run_rules, expected_mods) = find_game_config(game_configs, &run.game_id, &run.category_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No configuration found for game={} category={}",
                run.game_id,
                run.category_id
            )
        })?;

    process_run(db, run, run_rules, expected_mods, install_dir, output_dir).await?;
    Ok(ProcessResult::Processed)
}

async fn process_run(
    db: &Database,
    run: Run,
    run_rules: &RunRules,
    expected_mods: &ExpectedMods,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    let run_id = run.run_id.clone();

    mark_run_processing(db, &run_id)
        .await
        .context("Failed to mark run as processing")?;

    info!("Processing run: {}", run_id);

    let result =
        setup_and_run_replay(&run, run_rules, expected_mods, install_dir, output_dir).await;

    match result {
        Ok(report) if report.exited_successfully => match report.max_msg_level {
            MsgLevel::Info => {
                mark_run_passed(db, &run_id).await?;
                info!("Run {} passed verification", run_id);
            }
            MsgLevel::Warn => {
                mark_run_needs_review(db, &run_id).await?;
                warn!("Run {} passed with warnings (needs review)", run_id);
            }
            MsgLevel::Error => {
                mark_run_failed(db, &run_id).await?;
                warn!("Run {} failed verification", run_id);
            }
        },
        Ok(_) => {
            let error_msg = "Replay did not exit successfully";
            mark_run_error(db, &run_id, error_msg).await?;
            error!("Run {} error: {}", run_id, error_msg);
        }
        Err(e) => {
            let error_msg = format!("Failed to process run: {:#}", e);
            mark_run_error(db, &run_id, &error_msg).await?;
            error!("Run {} error: {}", run_id, error_msg);
        }
    }
    Ok(())
}

async fn setup_and_run_replay(
    run: &Run,
    run_rules: &RunRules,
    expected_mods: &ExpectedMods,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ReplayReport> {
    let working_dir = output_dir.join(&run.run_id);
    std::fs::create_dir_all(&working_dir)?;

    let mut save_file = download_save_file(&run.run_id, &working_dir).await?;

    let install_dir = FactorioInstallDir::new_or_create(install_dir)?;

    let log_path = working_dir.join("output.log");
    run_replay(
        &install_dir,
        &mut save_file,
        run_rules,
        expected_mods,
        &log_path,
    )
    .await
}

async fn download_save_file(run_id: &str, working_dir: &Path) -> Result<WrittenSaveFile> {
    use speedrun_api::SpeedrunApiClientAsync;
    use speedrun_api::api;
    use speedrun_api::api::AsyncQuery;

    info!("Fetching run data for {}", run_id);
    let client = SpeedrunApiClientAsync::new()?;
    let query = api::runs::Run::builder().id(run_id).build()?;
    let api_run: speedrun_api::types::Run<'_> = query.query_async(&client).await?;

    let description = api_run
        .comment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Comment with link needed for run {}", run_id))?;

    info!("Downloading save file for run {}", run_id);
    let mut downloader = FileDownloader::builder()
        .add_service(SpeedrunService)
        .add_service(DropboxService)
        .add_service(GoogleDriveService)
        .build();

    let save_file_info = downloader
        .download_zip(description, working_dir)
        .await
        .context("Failed to download save file")?;

    let save_path = working_dir.join(save_file_info.name);
    let save_file = SaveFile::new(File::open(&save_path)?)?;

    Ok(WrittenSaveFile(save_path, save_file))
}

fn find_game_config<'a>(
    game_configs: &'a HashMap<String, GameConfig>,
    game_id: &str,
    category_id: &str,
) -> Option<(&'a RunRules, &'a ExpectedMods)> {
    game_configs.get(game_id).and_then(|game_config| {
        game_config
            .categories
            .get(category_id)
            .map(|category_config| {
                let run_rules = &category_config.run_rules;
                let expected_mods = run_rules
                    .expected_mods_override
                    .as_ref()
                    .unwrap_or(&game_config.expected_mods);
                (run_rules, expected_mods)
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CategoryConfig;
    use crate::database::connection::Database;
    use crate::database::operations::insert_run;
    use crate::database::types::NewRun;
    use replay_script::ReplayScripts;

    #[test]
    fn test_find_game_config_found() {
        let mut game_configs = HashMap::new();
        let mut categories = HashMap::new();

        let category_config = CategoryConfig {
            name: Some("Any%".to_string()),
            run_rules: RunRules {
                expected_mods_override: None,
                replay_scripts: ReplayScripts::default(),
            },
        };

        categories.insert("cat1".to_string(), category_config);

        let game_config = GameConfig {
            name: Some("Factorio".to_string()),
            expected_mods: ExpectedMods::default(),
            categories,
        };

        game_configs.insert("game1".to_string(), game_config);

        let result = find_game_config(&game_configs, "game1", "cat1");
        assert!(result.is_some());

        let (run_rules, expected_mods) = result.unwrap();
        assert!(expected_mods.is_empty());
        assert_eq!(run_rules.expected_mods_override, None);
    }

    #[test]
    fn test_find_game_config_game_not_found() {
        let game_configs = HashMap::new();
        let result = find_game_config(&game_configs, "nonexistent", "cat1");
        assert!(result.is_none());
    }

    #[test]
    fn test_find_game_config_category_not_found() {
        let mut game_configs = HashMap::new();
        let game_config = GameConfig {
            name: Some("Factorio".to_string()),
            expected_mods: ExpectedMods::default(),
            categories: HashMap::new(),
        };

        game_configs.insert("game1".to_string(), game_config);

        let result = find_game_config(&game_configs, "game1", "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_find_game_config_uses_override_mods() {
        let mut game_configs = HashMap::new();
        let mut categories = HashMap::new();

        let override_mods = ExpectedMods::from(["mod1".to_string()]);

        let category_config = CategoryConfig {
            name: Some("Any%".to_string()),
            run_rules: RunRules {
                expected_mods_override: Some(override_mods.clone()),
                replay_scripts: ReplayScripts::default(),
            },
        };

        categories.insert("cat1".to_string(), category_config);

        let game_config = GameConfig {
            name: Some("Factorio".to_string()),
            expected_mods: ExpectedMods::from(["mod2".to_string()]),
            categories,
        };

        game_configs.insert("game1".to_string(), game_config);

        let result = find_game_config(&game_configs, "game1", "cat1");
        assert!(result.is_some());

        let (_, expected_mods) = result.unwrap();
        assert_eq!(expected_mods, &override_mods);
    }

    #[tokio::test]
    async fn test_poll_runs_no_discovered_runs() {
        let db = Database::in_memory().await.unwrap();
        let game_configs = HashMap::new();
        let install_dir = std::path::PathBuf::from("/tmp/test");
        let output_dir = std::path::PathBuf::from("/tmp/test_output");

        let result = find_run_to_process(&db, &game_configs, &install_dir, &output_dir).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ProcessResult::NoWork));
    }

    #[tokio::test]
    async fn test_poll_runs_missing_config() {
        let db = Database::in_memory().await.unwrap();
        let game_configs = HashMap::new();
        let install_dir = std::path::PathBuf::from("/tmp/test");
        let output_dir = std::path::PathBuf::from("/tmp/test_output");

        let new_run = NewRun::new(
            "run123",
            "game1",
            "cat1",
            "2024-01-01T00:00:00Z".parse().unwrap(),
        );
        insert_run(&db, new_run).await.unwrap();

        let result = find_run_to_process(&db, &game_configs, &install_dir, &output_dir).await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No configuration found")
        );
    }
}
