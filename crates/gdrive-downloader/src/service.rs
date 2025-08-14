use crate::{create_authenticator, download_gdrive_file, get_file_metadata, FileMetadata};
use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use run_downloader::{
    services::{FileDownloadService, FileService},
    ServiceError,
};
use std::path::Path;
use yup_oauth2::{AccessToken, ServiceAccountKey};

const GOOGLE_DRIVE_URL_PATTERNS: &[&str] = &[
    r"https://drive\.google\.com/file/d/([a-zA-Z0-9_-]+)",
    r"https://drive\.google\.com/open\?id=([a-zA-Z0-9_-]+)",
    r"https://docs\.google\.com/.*?/d/([a-zA-Z0-9_-]+)",
];

pub struct GoogleDriveService {
    service_account_key: Option<ServiceAccountKey>,
    cached_token: Option<AccessToken>,
}

impl GoogleDriveService {
    pub fn new(service_account_key: Option<ServiceAccountKey>) -> Self {
        Self {
            service_account_key,
            cached_token: None,
        }
    }

    async fn get_cached_token(&mut self) -> Result<Option<AccessToken>, ServiceError> {
        if self.service_account_key.is_none() {
            return Ok(None);
        }

        if self.is_token_valid() {
            return Ok(self.cached_token.clone());
        }

        let key = self.service_account_key.clone().unwrap();
        self.refresh_token(&key).await
    }

    fn is_token_valid(&self) -> bool {
        self.cached_token
            .as_ref()
            .map(|token| token.token().is_some())
            .unwrap_or(false)
    }

    async fn refresh_token(
        &mut self,
        key: &ServiceAccountKey,
    ) -> Result<Option<AccessToken>, ServiceError> {
        match create_authenticator(key).await {
            Ok(token) => {
                self.cached_token = Some(token.clone());
                Ok(Some(token))
            }
            Err(e) => Err(ServiceError::retryable(e)),
        }
    }

    fn extract_file_id(url: &str) -> Option<String> {
        GOOGLE_DRIVE_URL_PATTERNS
            .iter()
            .find_map(|pattern| Self::try_extract_with_pattern(url, pattern))
    }

    fn try_extract_with_pattern(url: &str, pattern: &str) -> Option<String> {
        let regex = Regex::new(pattern).ok()?;
        regex
            .captures(url)
            .and_then(|captures| captures.get(1))
            .map(|id| id.as_str().to_string())
    }

    fn convert_metadata(metadata: FileMetadata) -> run_downloader::FileInfo {
        run_downloader::FileInfo {
            name: metadata.name,
            size: metadata.size.parse().unwrap_or(0),
            mime_type: Some(metadata.mime_type),
        }
    }

    fn classify_error(error_msg: &str) -> ServiceError {
        if Self::is_auth_error(error_msg) {
            ServiceError::retryable(anyhow::anyhow!("{}", error_msg))
        } else if Self::is_not_found_error(error_msg) {
            ServiceError::fatal(anyhow::anyhow!("{}", error_msg))
        } else {
            ServiceError::retryable(anyhow::anyhow!("{}", error_msg))
        }
    }

    fn is_auth_error(error_msg: &str) -> bool {
        error_msg.contains("403") || error_msg.contains("401")
    }

    fn is_not_found_error(error_msg: &str) -> bool {
        error_msg.contains("404")
    }
}

#[async_trait]
impl FileService for GoogleDriveService {
    fn detect_link(input: &str) -> Option<String> {
        Self::extract_file_id(input)
    }
}

#[async_trait]
impl FileDownloadService for GoogleDriveService {
    fn service_name(&self) -> &str {
        "google_drive"
    }

    async fn download(&mut self, file_id: &str, dest_path: &Path) -> Result<(), ServiceError> {
        self.get_cached_token().await?;

        download_gdrive_file(file_id, dest_path, self.service_account_key.clone())
            .await
            .map(|_| ())
            .map_err(|e| Self::classify_error(&e.to_string()))
    }

    async fn get_file_info(
        &mut self,
        file_id: &str,
    ) -> Result<run_downloader::FileInfo, ServiceError> {
        self.get_cached_token().await?;

        get_file_metadata(file_id, self.service_account_key.clone())
            .await
            .map(Self::convert_metadata)
            .map_err(|e| Self::classify_error(&e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEST_FILE_ID: &str = "1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC";

    #[test]
    fn test_detect_link() {
        let test_cases = [
            (
                "https://drive.google.com/file/d/1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC/view?usp=sharing",
                Some("1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC".to_string()),
            ),
            (
                "https://drive.google.com/open?id=1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC",
                Some("1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC".to_string()),
            ),
            (
                "https://docs.google.com/document/d/1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC/edit",
                Some("1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC".to_string()),
            ),
            ("https://example.com/not-a-drive-link", None),
            ("just some text", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(GoogleDriveService::detect_link(input), expected);
        }
    }

    #[tokio::test]
    async fn test_service_unauthenticated() {
        let mut service = GoogleDriveService::new(None);

        let file_info = service.get_file_info(TEST_FILE_ID).await.unwrap();
        assert_file_info_is_correct(&file_info);

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        service
            .download(TEST_FILE_ID, temp_file.path())
            .await
            .unwrap();

        assert_file_downloaded_correctly(temp_file.path());
    }

    fn assert_file_info_is_correct(file_info: &run_downloader::FileInfo) {
        assert_eq!(file_info.name, "foo.zip");
        assert_eq!(file_info.size, 119);
        assert_eq!(
            file_info.mime_type,
            Some("application/octet-stream".to_string())
        );
    }

    fn assert_file_downloaded_correctly(path: &std::path::Path) {
        assert!(path.exists());
        let metadata = std::fs::metadata(path).unwrap();
        assert_eq!(metadata.len(), 119);
    }

    #[test]
    fn test_service_name() {
        let service = GoogleDriveService::new(None);
        assert_eq!(service.service_name(), "google_drive");
    }
}
