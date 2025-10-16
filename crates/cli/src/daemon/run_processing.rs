use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use factorio_manager::error::FactorioError;
use factorio_manager::expected_mods::ExpectedMods;
use factorio_manager::factorio_install_dir::{FactorioInstallDir, VersionStr};
use factorio_manager::save_file::{SaveFile, WrittenSaveFile};
use log::info;
use std::fs::File;
use std::path::{Path, PathBuf};
use zip_downloader::FileDownloader;
use zip_downloader::services::dropbox::DropboxService;
use zip_downloader::services::gdrive::GoogleDriveService;
use zip_downloader::services::speedrun::SpeedrunService;

use crate::config::RunRules;
use crate::daemon::config::SrcRunRules;
use crate::daemon::database::connection::Database;
use crate::daemon::database::types::NewRun;
use crate::daemon::retry::RetryConfig;
use crate::daemon::speedrun_api::{ApiError, RunsQuery, SpeedrunClient, SpeedrunOps};
use crate::error::ClassifiedError;
use crate::error::ErrorClass;
use crate::run_replay::{ReplayReport, run_replay};

const MIN_FACTORIO_VERSION: VersionStr = VersionStr::new(2, 0, 65);

#[derive(Clone)]
pub struct RunProcessingContext {
    pub db: Database,
    pub speedrun_ops: SpeedrunOps,
    pub src_rules: SrcRunRules,
    pub install_dir: PathBuf,
    pub output_dir: PathBuf,
    pub retry_config: RetryConfig,
}

impl RunProcessingContext {
    pub fn client(&self) -> &SpeedrunClient {
        &self.speedrun_ops.client
    }
}

pub struct RunProcessor<'a> {
    downloader: FileDownloader,
    client: &'a SpeedrunClient,
}

impl<'a> RunProcessor<'a> {
    pub fn new(client: &'a SpeedrunClient) -> Result<Self> {
        let downloader = FileDownloader::builder()
            .add_service(GoogleDriveService::new())
            .add_service(DropboxService::new())
            .add_service(SpeedrunService::new())
            .build();

        Ok(Self { downloader, client })
    }

    async fn fetch_run_description(&self, run_id: &str) -> Result<String, ApiError> {
        info!("Fetching run data for {}", run_id);
        let run = self.client.get_run(run_id).await?;

        let description = run.comment.as_ref().ok_or_else(|| {
            ApiError::MissingField(format!("Comment with link needed for run {}", run_id))
        })?;

        Ok(description.to_string())
    }

    async fn download_save(
        &mut self,
        description: &str,
        working_dir: &Path,
    ) -> Result<WrittenSaveFile, ClassifiedError> {
        info!("Downloading save file");
        let save_file_info = self
            .downloader
            .download_zip(description, working_dir)
            .await?;

        let save_path = working_dir.join(save_file_info.name);
        let file = File::open(&save_path).map_err(|e| {
            ClassifiedError::from(factorio_manager::error::FactorioError::IoError(e))
        })?;
        let save_file = SaveFile::new(file).map_err(ClassifiedError::from)?;

        Ok(WrittenSaveFile(save_path, save_file))
    }

    pub async fn download_run_save(
        &mut self,
        run_id: &str,
        working_dir: &Path,
    ) -> Result<WrittenSaveFile, ClassifiedError> {
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
        .ok_or_else(|| ApiError::MissingField("Run has no submitted date".to_string()))?;
    let submitted_date = super::speedrun_api::parse_datetime(&submitted_date)?;

    Ok((run_id, game_id, category_id, submitted_date))
}

pub async fn poll_game_category(
    client: &SpeedrunClient,
    game_id: &str,
    category_id: &str,
    cutoff_date: &DateTime<Utc>,
) -> Result<Vec<NewRun>, ApiError> {
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
            let submitted_date = super::speedrun_api::parse_datetime(&submitted_dt).ok()?;
            (submitted_date > *cutoff_date)
                .then(|| NewRun::new(run.id, game_id, category_id, submitted_date))
        })
        .collect();

    info!("Found {} new runs", new_runs.len());
    Ok(new_runs)
}

pub async fn download_and_run_replay(
    client: &SpeedrunClient,
    run_id: &str,
    run_rules: &RunRules,
    expected_mods: &ExpectedMods,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ReplayReport, ClassifiedError> {
    info!("=== Processing Run ===");
    info!("Run ID: {}", run_id);

    let working_dir = output_dir.join(run_id);
    std::fs::create_dir_all(&working_dir)
        .map_err(|e| ClassifiedError::from_error(ErrorClass::Retryable, &e))?;

    let mut processor = RunProcessor::new(client)
        .map_err(|e| ClassifiedError::from_error(ErrorClass::Retryable, &e))?;
    let mut save_file = processor.download_run_save(run_id, &working_dir).await?;

    let version = save_file.1.get_factorio_version()?;
    if version < MIN_FACTORIO_VERSION {
        return Err(FactorioError::VersionTooOld { version }.into());
    }

    let install_dir = FactorioInstallDir::new_or_create(install_dir)?;
    let log_path = working_dir.join("output.log");

    let report = run_replay(
        &install_dir,
        &mut save_file,
        run_rules,
        expected_mods,
        &log_path,
    )
    .await?;

    Ok(report)
}
