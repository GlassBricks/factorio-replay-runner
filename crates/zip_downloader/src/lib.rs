pub mod security;
pub mod services;

use std::{
    fs::File,
    path::{Path, PathBuf},
};

pub use security::SecurityConfig;
use services::{FileDownloadHandle, FileServiceDyn};
pub use services::{FileMeta, FileService};

use anyhow::Result;
use log::{debug, error, info};
use tempfile::NamedTempFile;

pub struct DownloadedFile {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("No valid download link found in input")]
    NoLinkFound,

    #[error("Security violation: {0}")]
    SecurityError(anyhow::Error),

    #[error("Service error (retryable): {0}")]
    ServiceError(anyhow::Error),

    #[error("File not accessible: {0}")]
    FileNotAccessible(String),

    /// Considered fatal
    #[error("Other error: {0}")]
    Other(anyhow::Error),
}

impl From<std::io::Error> for DownloadError {
    fn from(err: std::io::Error) -> Self {
        DownloadError::Other(err.into())
    }
}

type DynFileService = Box<dyn FileServiceDyn>;

pub struct FileDownloaderBuilder {
    pub services: Vec<DynFileService>,
    pub security_config: SecurityConfig,
}

pub struct FileDownloader {
    services: Vec<DynFileService>,
    security_config: SecurityConfig,
}

impl FileDownloaderBuilder {
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
            security_config: SecurityConfig::default(),
        }
    }

    pub fn with_security_config(mut self, security_config: SecurityConfig) -> Self {
        self.security_config = security_config;
        self
    }

    pub fn add_service(mut self, service: impl FileService + 'static) -> Self {
        self.services.push(Box::new(service));
        self
    }

    pub fn build(self) -> FileDownloader {
        assert!(!self.services.is_empty(), "No services configured");
        FileDownloader {
            services: self.services,
            security_config: self.security_config,
        }
    }
}

impl FileDownloader {
    pub fn builder() -> FileDownloaderBuilder {
        FileDownloaderBuilder::new()
    }

    pub fn security_config(&self) -> &SecurityConfig {
        &self.security_config
    }

    pub fn set_security_config(&mut self, config: SecurityConfig) {
        self.security_config = config;
    }

    pub fn service_count(&self) -> usize {
        self.services.len()
    }

    pub async fn download_zip(
        &mut self,
        input: &str,
        out_file_or_path: &Path,
    ) -> Result<DownloadedFile, DownloadError> {
        let result = self.do_download_zip(input, out_file_or_path).await;
        match &result {
            Ok(zip) => info!(
                "Successfully downloaded {} to {}",
                zip.name,
                zip.path.display()
            ),
            Err(err) => error!("Failed to download: {}", err),
        };
        result
    }

    pub async fn download_zip_to_temp(
        &mut self,
        input: &str,
    ) -> Result<(NamedTempFile, DownloadedFile), DownloadError> {
        let temp_file = NamedTempFile::new()?;
        let downloaded_file = self.download_zip(input, temp_file.path()).await?;
        Ok((temp_file, downloaded_file))
    }

    async fn do_download_zip(
        &mut self,
        input: &str,
        out_file: &Path,
    ) -> Result<DownloadedFile, DownloadError> {
        info!("Starting download");

        let mut download_handle = Self::get_download_handle(&mut self.services, input)?;
        info!("Found {download_handle}");

        info!("Getting file info");
        let file_info = download_handle
            .get_file_info()
            .await
            .map_err(DownloadError::ServiceError)?;

        debug!("File info: {file_info:?}");
        info!("Running initial checks");
        security::validate_file_info(&file_info, &self.security_config)
            .map_err(DownloadError::SecurityError)?;

        info!("Downloading file");

        let file_path = if out_file.is_dir() {
            out_file.join(file_info.name.as_str())
        } else {
            out_file.to_path_buf()
        };

        download_handle
            .download(&file_path)
            .await
            .map_err(DownloadError::ServiceError)?;

        info!("Running file checks");
        let mut reopened_file = File::open(&file_path)?;
        security::validate_downloaded_file(&mut reopened_file, &file_info, &self.security_config)
            .map_err(DownloadError::SecurityError)?;

        Ok(DownloadedFile {
            name: file_info.name,
            path: file_path,
        })
    }

    fn get_download_handle<'a>(
        services: &'a mut [DynFileService],
        input: &str,
    ) -> Result<Box<dyn FileDownloadHandle + 'a>, DownloadError> {
        info!("Input is: {}", input);
        services
            .iter_mut()
            .find_map(|service| service.detect_link(input))
            .ok_or(DownloadError::NoLinkFound)
    }
}

impl Default for FileDownloaderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for FileDownloader {
    fn default() -> Self {
        Self::builder().build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_util::MockService;

    #[test]
    fn test_file_downloader_creation() {
        let config = SecurityConfig {
            max_file_size: 1024,
            ..Default::default()
        };
        let downloader = FileDownloader::builder()
            .add_service(MockService)
            .with_security_config(config)
            .build();
        assert_eq!(downloader.service_count(), 1);

        assert_eq!(downloader.security_config.max_file_size, 1024);
    }

    #[tokio::test]
    async fn test_no_links_detected() {
        let mut downloader = FileDownloader::builder().add_service(MockService).build();
        let result = downloader.download_zip_to_temp("no links here").await;

        assert!(matches!(result, Err(DownloadError::NoLinkFound)));
    }

    #[test]
    fn test_validate_file_info() {
        let security_config = SecurityConfig::default();

        let valid_file_info = FileMeta {
            name: "test.zip".to_string(),
            size: 1000,
        };

        assert!(security::validate_file_info(&valid_file_info, &security_config).is_ok());

        let too_large_file_info = FileMeta {
            name: "test.zip".to_string(),
            size: 200 * 1024 * 1024, // Larger than default 100MB limit
        };

        let result = security::validate_file_info(&too_large_file_info, &security_config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("File size") && err.to_string().contains("exceeds maximum")
        );
    }
}
