pub mod error;
pub mod logging;
pub mod security;
pub mod services;

pub use error::{DownloadError, ServiceError};
pub use security::SecurityConfig;
use services::FileServiceDyn;
pub use services::{FileInfo, FileService};

use anyhow::Result;
use std::{cell::RefCell, path::PathBuf};
use tracing::{error, info, warn};

/// Result of a successful download operation
#[derive(Debug, Clone)]
pub struct DownloadedZip {
    pub path: PathBuf,
    pub file_info: FileInfo,
    pub service_name: String,
}

impl DownloadedZip {
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn file_info(&self) -> &FileInfo {
        &self.file_info
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}

type DynFileService = Box<RefCell<dyn FileServiceDyn>>;

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

    pub fn add_service(mut self, service: impl FileServiceDyn + 'static) -> Self {
        self.services.push(Box::new(RefCell::new(service)));
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
        match result {
            Ok(zip) => {
                info!("Successfully downloaded file to {:?}", zip.path);
                Ok(zip)
            }
            Err(err) => {
                error!("Failed to download file: {}", err);
                Err(err)
            }
        }
    }

    async fn do_download_zip(&mut self, input: &str) -> Result<DownloadedZip, DownloadError> {
        info!("Starting download");

        let (service_box, file_id) = self.detect_service(input)?;
        let mut service = service_box.borrow_mut();

        info!(
            "Found: service '{}', file ID: {}",
            service.service_name(),
            file_id
        );

        info!("Getting file info");
        let file_info = service.get_file_info(&file_id).await?;

        info!("Running security checks");
        security::validate_file_info(&file_info, &self.security_config)?;

        let temp_file = tempfile::NamedTempFile::new().map_err(|e| {
            DownloadError::Other(anyhow::anyhow!("Failed to create temp file: {}", e))
        })?;

        let temp_path = temp_file.path();

        info!("Downloading file");
        service.download(&file_id, temp_path).await?;
        info!("Running security checks");
        security::validate_downloaded_file(temp_path, &file_info, &self.security_config)?;

        let final_path = temp_file
            .keep()
            .map_err(|e| DownloadError::Other(anyhow::anyhow!("Failed to keep temp file: {}", e)))?
            .1;

        let downloaded_run = DownloadedZip {
            path: final_path,
            file_info,
            service_name: service.service_name().to_string(),
        };

        Ok(downloaded_run)
    }

    fn detect_service(&self, input: &str) -> Result<(&DynFileService, String), DownloadError> {
        for service in &self.services {
            if let Some(id) = service.borrow().detect_link_dyn(input) {
                return Ok((service, id));
            }
        }

        warn!("No links found in input by any service");
        Err(DownloadError::NoLinkFound)
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
        let config = SecurityConfig::new().max_file_size(1024);
        let downloader = FileDownloader::builder()
            .add_service(MockService)
            .with_security_config(config)
            .build();
        assert_eq!(downloader.service_count(), 1);

        assert_eq!(downloader.security_config.max_file_size, 1024);
    }

    #[tokio::test]
    async fn test_no_links_detected() {
        logging::init_test_logging();

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
            mime_type: Some("application/zip".to_string()),
        };

        assert!(security::validate_file_info(&valid_file_info, &security_config).is_ok());

        let too_large_file_info = FileInfo {
            name: "test.zip".to_string(),
            size: 200 * 1024 * 1024, // Larger than default 100MB limit
            mime_type: Some("application/zip".to_string()),
        };

        let result = security::validate_file_info(&too_large_file_info, &security_config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("File size") && err.to_string().contains("exceeds maximum")
        );
    }
}
