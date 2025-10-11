use anyhow::{Context, Result};
use chrono::DateTime;
use chrono::Utc;
use factorio_manager::expected_mods::ExpectedMods;
use factorio_manager::factorio_install_dir::FactorioInstallDir;
use factorio_manager::save_file::{SaveFile, WrittenSaveFile};
use futures::StreamExt as _;
use log::debug;
use log::info;
use speedrun_api::SpeedrunApiClientAsync;
use speedrun_api::api;
use speedrun_api::api::AsyncQuery;
use speedrun_api::api::PagedEndpointExt as _;
use std::fs::File;
use std::path::Path;
use zip_downloader::FileDownloader;
use zip_downloader::services::dropbox::DropboxService;
use zip_downloader::services::gdrive::GoogleDriveService;
use zip_downloader::services::speedrun::SpeedrunService;

use crate::config::RunRules;
use crate::database::types::NewRun;
use crate::run_replay::{ReplayReport, run_replay};

pub struct RunProcessor {
    downloader: FileDownloader,
}

impl RunProcessor {
    pub fn new() -> Result<Self> {
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

        Ok(Self { downloader })
    }

    async fn fetch_run_description(&self, run_id: &str) -> Result<String> {
        info!("Fetching run data for {}", run_id);
        let client = SpeedrunApiClientAsync::new()?;
        let query = api::runs::Run::builder().id(run_id).build()?;
        let run: speedrun_api::types::Run<'_> = query.query_async(&client).await?;

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

impl Default for RunProcessor {
    fn default() -> Self {
        Self::new().expect("Failed to create RunProcessor")
    }
}

pub async fn fetch_run_metadata(run_id: &str) -> Result<(String, String)> {
    info!("Fetching run metadata for {}", run_id);
    let client = SpeedrunApiClientAsync::new()?;
    let query = api::runs::Run::builder().id(run_id).build()?;
    let run: speedrun_api::types::Run<'_> = query.query_async(&client).await?;

    let game_id = run.game.to_string();
    let category_id = run.category.to_string();

    Ok((game_id, category_id))
}

pub async fn poll_game_category(
    game_id: &str,
    category_id: &str,
    cutoff_date: &DateTime<Utc>,
) -> Result<Vec<NewRun>> {
    info!(
        "Polling for new runs: game={}, category={}",
        game_id, category_id
    );

    let client = SpeedrunApiClientAsync::new()?;

    let endpoint = api::runs::Runs::builder()
        .game(game_id)
        .category(category_id)
        .orderby(api::runs::RunsSorting::Submitted)
        .direction(api::Direction::Asc)
        .build()?;

    let mut stream = endpoint.stream(&client);
    let mut new_runs = Vec::new();

    while let Some(result) = stream.next().await {
        let run: speedrun_api::types::Run = result?;
        if let Some(submitted_dt) = run.submitted
            && let Ok(submitted_date) = parse_datetime(&submitted_dt)
            && (submitted_date >= *cutoff_date)
        {
            let new_run = NewRun::new(run.id.to_string(), game_id, category_id, submitted_date);

            new_runs.push(new_run);
        };
    }

    debug!("Found {} new runs", new_runs.len());
    Ok(new_runs)
}

fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc))
}

pub async fn download_and_run_replay(
    run_id: &str,
    run_rules: &RunRules,
    expected_mods: &ExpectedMods,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ReplayReport> {
    let working_dir = output_dir.join(run_id);
    std::fs::create_dir_all(&working_dir)?;

    let mut processor = RunProcessor::new()?;
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
