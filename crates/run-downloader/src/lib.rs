pub mod error;
pub mod security;
pub mod services;

pub use error::{DownloadError, ServiceError};
pub use security::SecurityConfig;
use services::{FileDownloadHandle, FileServiceDyn};
pub use services::{FileInfo, FileService};

use anyhow::Result;
use tempfile::NamedTempFile;
use tracing::{error, info};

/// Result of a successful download operation
#[derive(Debug)]
pub struct DownloadedZip {
    pub file: NamedTempFile,
    pub file_info: FileInfo,
    pub service_name: String,
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

    pub async fn download_zip(&mut self, input: &str) -> Result<DownloadedZip, DownloadError> {
        let result = self.do_download_zip(input).await;
        match &result {
            Ok(zip) => info!(
                "Successfully downloaded file to {}",
                zip.file.path().display()
            ),
            Err(err) => error!("Failed to download: {}", err),
        };
        result
    }

    async fn do_download_zip(&mut self, input: &str) -> Result<DownloadedZip, DownloadError> {
        info!("Starting download");

        let mut file_handle = Self::get_file_handle(&mut self.services, input)?;
        info!("Found {file_handle}");

        info!("Getting file info");
        let file_info = file_handle.get_file_info().await?;

        info!("Running initial checks");
        security::validate_file_info(&file_info, &self.security_config)?;

        info!("Downloading file");
        let file = file_handle.download_to_tmp().await?;

        info!("Running file checks");
        let mut re_file = file
            .reopen()
            .map_err(|err| DownloadError::Other(err.into()))?;
        security::validate_downloaded_file(&mut re_file, &file_info, &self.security_config)?;

        let downloaded_run = DownloadedZip {
            file,
            file_info,
            service_name: file_handle.service_name().to_string(),
        };

        Ok(downloaded_run)
    }

    fn get_file_handle<'a>(
        services: &'a mut [DynFileService],
        input: &str,
    ) -> Result<Box<dyn FileDownloadHandle + 'a>, DownloadError> {
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
        let result = downloader.download_zip("no links here").await;

        assert!(matches!(result, Err(DownloadError::NoLinkFound)));
    }

    #[test]
    fn test_validate_file_info() {
        let security_config = SecurityConfig::default();

        let valid_file_info = FileInfo {
            name: "test.zip".to_string(),
            size: 1000,
            is_zip: true,
        };

        assert!(security::validate_file_info(&valid_file_info, &security_config).is_ok());

        let too_large_file_info = FileInfo {
            name: "test.zip".to_string(),
            size: 200 * 1024 * 1024, // Larger than default 100MB limit
            is_zip: true,
        };

        let result = security::validate_file_info(&too_large_file_info, &security_config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("File size") && err.to_string().contains("exceeds maximum")
        );
    }
}
