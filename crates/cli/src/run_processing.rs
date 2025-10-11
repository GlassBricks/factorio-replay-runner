use anyhow::{Context, Result};
use factorio_manager::save_file::{SaveFile, WrittenSaveFile};
use log::info;
use speedrun_api::api;
use speedrun_api::api::AsyncQuery;
use speedrun_api::SpeedrunApiClientAsync;
use std::fs::File;
use std::path::Path;
use zip_downloader::services::dropbox::DropboxService;
use zip_downloader::services::gdrive::GoogleDriveService;
use zip_downloader::services::speedrun::SpeedrunService;
use zip_downloader::FileDownloader;

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

    pub async fn fetch_and_download_run(
        &mut self,
        run_id: &str,
        working_dir: &Path,
    ) -> Result<WrittenSaveFile> {
        info!("Fetching run data for {}", run_id);
        let client = SpeedrunApiClientAsync::new()?;
        let query = api::runs::Run::builder().id(run_id).build()?;
        let run: speedrun_api::types::Run<'_> = query.query_async(&client).await?;

        let description = run.comment.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Comment with link needed for run {}", run_id)
        })?;

        info!("Downloading save file for run {}", run_id);
        let save_file_info = self
            .downloader
            .download_zip(description, working_dir)
            .await
            .context("Failed to download save file")?;

        let save_path = working_dir.join(save_file_info.name);
        let save_file = SaveFile::new(File::open(&save_path)?)?;

        Ok(WrittenSaveFile(save_path, save_file))
    }

    pub async fn fetch_run_metadata(
        &self,
        run_id: &str,
    ) -> Result<(String, String)> {
        info!("Fetching run metadata for {}", run_id);
        let client = SpeedrunApiClientAsync::new()?;
        let query = api::runs::Run::builder().id(run_id).build()?;
        let run: speedrun_api::types::Run<'_> = query.query_async(&client).await?;

        let game_id = run.game.to_string();
        let category_id = run.category.to_string();

        Ok((game_id, category_id))
    }
}

impl Default for RunProcessor {
    fn default() -> Self {
        Self::new().expect("Failed to create RunProcessor")
    }
}
