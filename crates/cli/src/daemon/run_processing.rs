use anyhow::Result;
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
use crate::daemon::bot_notifier::BotNotifierHandle;
use crate::daemon::config::SrcRunRules;
use crate::daemon::database::connection::Database;
use crate::daemon::retry::RetryConfig;
use crate::daemon::speedrun_api::{ApiError, SpeedrunClient, SpeedrunOps};
use crate::error::ErrorClass;
use crate::error::RunProcessingError;
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
    pub bot_notifier: Option<BotNotifierHandle>,
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
        info!("Fetching run description");
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
    ) -> Result<WrittenSaveFile, RunProcessingError> {
        info!("Downloading save file");
        let save_file_info = self
            .downloader
            .download_zip(description, working_dir)
            .await?;

        let save_path = working_dir.join(save_file_info.name);
        let file = File::open(&save_path).map_err(|e| {
            RunProcessingError::from(factorio_manager::error::FactorioError::IoError(e))
        })?;
        let save_file = SaveFile::new(file).map_err(RunProcessingError::from)?;

        Ok(WrittenSaveFile(save_path, save_file))
    }

    pub async fn download_run_save(
        &mut self,
        run_id: &str,
        working_dir: &Path,
    ) -> Result<WrittenSaveFile, RunProcessingError> {
        let description = self.fetch_run_description(run_id).await?;
        self.download_save(&description, working_dir).await
    }
}

pub async fn download_and_run_replay(
    client: &SpeedrunClient,
    run_id: &str,
    run_rules: &RunRules,
    expected_mods: &ExpectedMods,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ReplayReport, RunProcessingError> {
    let working_dir = output_dir.join(run_id);
    std::fs::create_dir_all(&working_dir)
        .map_err(|e| RunProcessingError::from_error(ErrorClass::Retryable, &e))?;

    let mut processor = RunProcessor::new(client)
        .map_err(|e| RunProcessingError::from_error(ErrorClass::Retryable, &e))?;
    let mut save_file = processor.download_run_save(run_id, &working_dir).await?;

    let result = run_replay_with_save(&mut save_file, run_rules, expected_mods, install_dir).await;
    cleanup_save_files(&save_file.0);
    result
}

async fn run_replay_with_save(
    save_file: &mut WrittenSaveFile,
    run_rules: &RunRules,
    expected_mods: &ExpectedMods,
    install_dir: &Path,
) -> Result<ReplayReport, RunProcessingError> {
    let version = save_file.1.get_factorio_version()?;
    if version < MIN_FACTORIO_VERSION {
        return Err(FactorioError::VersionTooOld { version }.into());
    }

    let install_dir = FactorioInstallDir::new_or_create(install_dir)?;
    let log_path = save_file.0.with_file_name("output.log");

    run_replay(&install_dir, save_file, run_rules, expected_mods, &log_path)
        .await
        .map_err(RunProcessingError::from)
}

fn cleanup_save_files(save_path: &Path) {
    let installed_path = save_path.with_extension("installed.zip");
    for path in [save_path, installed_path.as_path()] {
        if let Err(e) = std::fs::remove_file(path) {
            log::warn!("Failed to clean up {}: {}", path.display(), e);
        }
    }
}
