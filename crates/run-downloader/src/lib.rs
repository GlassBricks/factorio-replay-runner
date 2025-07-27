pub mod error;
pub mod logging;
pub mod security;
pub mod services;

pub use error::{DownloadError, ServiceError};
pub use security::SecurityConfig;
pub use services::{FileInfo, FileService, GoogleDriveService};

use anyhow::Result;
use std::path::PathBuf;
use tracing::{debug, error, info, instrument, warn};

/// Result of a successful download operation
#[derive(Debug, Clone)]
pub struct DownloadedRun {
    pub path: PathBuf,
    pub file_info: FileInfo,
    pub service_name: String,
}

impl DownloadedRun {
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

/// Builder for configuring a FileDownloader
pub struct FileDownloaderBuilder {
    pub services: Vec<Box<dyn FileService<FileId = String>>>,
    pub security_config: SecurityConfig,
}

/// Main file downloader that orchestrates multiple file sharing services
pub struct FileDownloader {
    services: Vec<Box<dyn FileService<FileId = String>>>,
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

    pub fn add_service<T>(mut self, service: T) -> Self
    where
        T: FileService<FileId = String> + 'static,
    {
        self.services.push(Box::new(service));
        self
    }

    pub fn set_security_config(&mut self, security_config: SecurityConfig) {
        self.security_config = security_config;
    }

    pub fn push_service<T>(&mut self, service: T)
    where
        T: FileService<FileId = String> + 'static,
    {
        self.services.push(Box::new(service));
    }

    pub fn build(self) -> FileDownloader {
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

    #[instrument(skip(self))]
    pub async fn download_run(&self, input: &str) -> Result<DownloadedRun, DownloadError> {
        info!(
            "Starting download process with {} services",
            self.services.len()
        );

        if self.services.is_empty() {
            error!("No services configured");
            return Err(DownloadError::NoLinkFound);
        }

        let (service, file_ids) = self.detect_links_any(input)?;
        let file_id = &file_ids[0];

        info!(
            "Using service '{}' to download file ID: {}",
            service.service_name(),
            file_id
        );

        let file_info = service.get_file_info(file_id).await?;

        security::validate_file_info(&file_info, &self.security_config)?;

        let temp_file = tempfile::NamedTempFile::new().map_err(|e| {
            DownloadError::Other(anyhow::anyhow!("Failed to create temp file: {}", e))
        })?;

        let temp_path = temp_file.path();

        service.download(file_id, temp_path).await?;

        security::validate_downloaded_file(temp_path, &file_info, &self.security_config)?;

        let final_path = temp_file
            .keep()
            .map_err(|e| DownloadError::Other(anyhow::anyhow!("Failed to keep temp file: {}", e)))?
            .1;

        let downloaded_run = DownloadedRun {
            path: final_path,
            file_info,
            service_name: service.service_name().to_string(),
        };

        info!("Successfully downloaded file to {:?}", downloaded_run.path);
        Ok(downloaded_run)
    }

    fn detect_links_any(
        &self,
        input: &str,
    ) -> Result<(&dyn FileService<FileId = String>, Vec<String>), DownloadError> {
        debug!(
            "Attempting link detection with {} services",
            self.services.len()
        );

        for service in &self.services {
            let file_ids = service.detect_links(input);

            if !file_ids.is_empty() {
                debug!(
                    "Service '{}' detected {} file ID(s)",
                    service.service_name(),
                    file_ids.len()
                );
                return Ok((service.as_ref(), file_ids));
            }
        }

        warn!("No valid links found in input by any service");
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
    use crate::services::FileService;
    use anyhow::anyhow;
    use async_trait::async_trait;
    use std::path::Path;

    struct MockFileService {
        should_detect: bool,
    }

    #[async_trait]
    impl FileService for MockFileService {
        type FileId = String;

        fn detect_links(&self, input: &str) -> Vec<Self::FileId> {
            if self.should_detect && input.contains("mock://") {
                vec!["test_id".to_string()]
            } else {
                vec![]
            }
        }

        async fn download(
            &self,
            _file_id: &Self::FileId,
            dest_path: &Path,
        ) -> Result<(), ServiceError> {
            // Create a fake ZIP file for testing
            let fake_zip = b"PK\x03\x04\x14\x00\x00\x00\x08\x00"; // ZIP header
            std::fs::write(dest_path, fake_zip)
                .map_err(|e| ServiceError::fatal(anyhow!("Write failed: {}", e)))?;
            Ok(())
        }

        async fn get_file_info(&self, _file_id: &Self::FileId) -> Result<FileInfo, ServiceError> {
            Ok(FileInfo {
                name: "test.zip".to_string(),
                size: 10, // Small size to match our fake ZIP
                mime_type: Some("application/zip".to_string()),
                is_public: true,
            })
        }

        fn service_name(&self) -> &'static str {
            "mock"
        }
    }

    #[test]
    fn test_file_downloader_creation() {
        let downloader = FileDownloader::builder().build();
        assert_eq!(downloader.service_count(), 0);

        let config = SecurityConfig::new().max_file_size(1024);
        let downloader = FileDownloader::builder()
            .with_security_config(config)
            .build();
        assert_eq!(downloader.security_config.max_file_size, 1024);
    }

    #[test]
    fn test_add_service() {
        let mock_service = MockFileService {
            should_detect: true,
        };

        let downloader = FileDownloader::builder().add_service(mock_service).build();
        assert_eq!(downloader.service_count(), 1);
    }

    #[tokio::test]
    async fn test_no_services_configured() {
        logging::init_test_logging();

        let downloader = FileDownloader::builder().build();
        let result = downloader.download_run("test input").await;

        assert!(matches!(result, Err(DownloadError::NoLinkFound)));
    }

    #[tokio::test]
    async fn test_no_links_detected() {
        logging::init_test_logging();

        let mock_service = MockFileService {
            should_detect: false,
        };

        let downloader = FileDownloader::builder().add_service(mock_service).build();
        let result = downloader.download_run("no links here").await;

        assert!(matches!(result, Err(DownloadError::NoLinkFound)));
    }

    #[test]
    fn test_validate_file_info() {
        let downloader = FileDownloader::builder().build();

        let valid_file_info = FileInfo {
            name: "test.zip".to_string(),
            size: 1000,
            mime_type: Some("application/zip".to_string()),
            is_public: true,
        };

        assert!(
            security::validate_file_info(&valid_file_info, downloader.security_config()).is_ok()
        );

        let too_large_file_info = FileInfo {
            name: "test.zip".to_string(),
            size: 200 * 1024 * 1024, // Larger than default 100MB limit
            mime_type: Some("application/zip".to_string()),
            is_public: true,
        };

        let result =
            security::validate_file_info(&too_large_file_info, downloader.security_config());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("File size") && err.to_string().contains("exceeds maximum")
        );
    }

    #[test]
    fn test_builder_patterns() {
        let mock_service1 = MockFileService {
            should_detect: true,
        };
        let mock_service2 = MockFileService {
            should_detect: false,
        };

        // Test fluent builder pattern
        let config = SecurityConfig::new().max_file_size(2048);
        let downloader1 = FileDownloader::builder()
            .with_security_config(config)
            .add_service(mock_service1)
            .build();

        assert_eq!(downloader1.service_count(), 1);
        assert_eq!(downloader1.security_config().max_file_size, 2048);

        // Test mutable builder pattern
        let mut builder = FileDownloader::builder();
        builder.set_security_config(SecurityConfig::new().max_file_size(4096));
        builder.push_service(mock_service2);
        let downloader2 = builder.build();

        assert_eq!(downloader2.service_count(), 1);
        assert_eq!(downloader2.security_config().max_file_size, 4096);
    }
}
