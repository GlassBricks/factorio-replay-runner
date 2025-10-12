use anyhow::{Context, Result};
use chrono::DateTime;
use chrono::Utc;
use factorio_manager::expected_mods::ExpectedMods;
use factorio_manager::factorio_install_dir::FactorioInstallDir;
use factorio_manager::save_file::{SaveFile, WrittenSaveFile};
use log::debug;
use log::info;
use std::fs::File;
use std::path::Path;
use zip_downloader::FileDownloader;
use zip_downloader::services::dropbox::DropboxService;
use zip_downloader::services::gdrive::GoogleDriveService;
use zip_downloader::services::speedrun::SpeedrunService;

use crate::config::RunRules;
use crate::database::types::NewRun;
use crate::run_replay::{ReplayReport, run_replay};
use crate::speedrun_api::{RunsQuery, SpeedrunClient};

pub struct RunProcessor<'a> {
    downloader: FileDownloader,
    client: &'a SpeedrunClient,
}

impl<'a> RunProcessor<'a> {
    pub fn new(client: &'a SpeedrunClient) -> Result<Self> {
        if std::env::var("AUTO_DOWNLOAD_RUNS").is_err() {
            anyhow::bail!(
                "Not downloading runs for security reasons. set AUTO_DOWNLOAD_RUNS=1 to acknowledge risks and enable automatic download"
            );
        }

        let downloader = FileDownloader::builder()
            .add_service(GoogleDriveService::new())
            .add_service(DropboxService::new())
            .add_service(SpeedrunService::new())
            .build();

        Ok(Self { downloader, client })
    }

    async fn fetch_run_description(&self, run_id: &str) -> Result<String> {
        info!("Fetching run data for {}", run_id);
        let run = self.client.get_run(run_id).await?;

        let description = run
            .comment
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Comment with link needed for run {}", run_id))?;

        Ok(description.to_string())
    }

    async fn download_save(
        &mut self,
        description: &str,
        working_dir: &Path,
    ) -> Result<WrittenSaveFile> {
        info!("Downloading save file");
        let save_file_info = self
            .downloader
            .download_zip(description, working_dir)
            .await
            .context("Failed to download save file")?;

        let save_path = working_dir.join(save_file_info.name);
        let save_file = SaveFile::new(File::open(&save_path)?)?;

        Ok(WrittenSaveFile(save_path, save_file))
    }

    pub async fn download_run_save(
        &mut self,
        run_id: &str,
        working_dir: &Path,
    ) -> Result<WrittenSaveFile> {
        let description = self.fetch_run_description(run_id).await?;
        self.download_save(&description, working_dir).await
    }
}

pub async fn fetch_run_details(
    client: &SpeedrunClient,
    run_id: &str,
) -> Result<(String, String, String, DateTime<Utc>)> {
    info!("Fetching run details for {}", run_id);
    let run = client.get_run(run_id).await?;

    let game_id = run.game;
    let category_id = run.category;
    let run_id = run.id;
    let submitted_date = run
        .submitted
        .ok_or_else(|| anyhow::anyhow!("Run has no submitted date"))?;
    let submitted_date = crate::speedrun_api::parse_datetime(&submitted_date)?;

    Ok((run_id, game_id, category_id, submitted_date))
}

pub async fn poll_game_category(
    client: &SpeedrunClient,
    game_id: &str,
    category_id: &str,
    cutoff_date: &DateTime<Utc>,
) -> Result<Vec<NewRun>> {
    info!(
        "Polling for new runs: game={}, category={}",
        game_id, category_id
    );

    let query = RunsQuery::new()
        .game(game_id)
        .category(category_id)
        .orderby("submitted")
        .direction("asc");

    let runs = client.stream_runs(&query).await?;

    let new_runs: Vec<NewRun> = runs
        .into_iter()
        .filter_map(|run| {
            let submitted_dt = run.submitted?;
            let submitted_date = crate::speedrun_api::parse_datetime(&submitted_dt).ok()?;
            (submitted_date > *cutoff_date)
                .then(|| NewRun::new(run.id, game_id, category_id, submitted_date))
        })
        .collect();

    debug!("Found {} new runs", new_runs.len());
    Ok(new_runs)
}

pub async fn download_and_run_replay(
    client: &SpeedrunClient,
    run_id: &str,
    run_rules: &RunRules,
    expected_mods: &ExpectedMods,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ReplayReport> {
    info!("=== Processing Run ===");
    info!("Run ID: {}", run_id);

    let working_dir = output_dir.join(run_id);
    std::fs::create_dir_all(&working_dir)?;

    let mut processor = RunProcessor::new(client)?;
    let mut save_file = processor.download_run_save(run_id, &working_dir).await?;

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
