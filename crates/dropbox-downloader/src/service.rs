use crate::{
    download_dropbox_file, extract_path_from_url, get_file_metadata, read_dropbox_token_from_env,
    FileMetadata,
};
use anyhow::Result;
use async_trait::async_trait;

use run_downloader::{
    services::{FileDownloadService, FileService},
    ServiceError,
};
use std::path::Path;

pub struct DropboxService {
    token: Option<String>,
}

impl DropboxService {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }

    fn extract_file_id(url: &str) -> Option<String> {
        extract_path_from_url(url)
    }

    fn convert_metadata(metadata: FileMetadata) -> run_downloader::FileInfo {
        let name = metadata.name;
        run_downloader::FileInfo {
            name: name.clone(),
            size: metadata.size,
            mime_type: Self::infer_mime_type(&name),
        }
    }

    pub fn infer_mime_type(filename: &str) -> Option<String> {
        let filename_lower = filename.to_lowercase();
        if filename_lower.ends_with(".zip") {
            Some("application/zip".to_string())
        } else if filename_lower.ends_with(".txt") {
            Some("text/plain".to_string())
        } else {
            Some("application/octet-stream".to_string())
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
        error_msg.contains("401") || error_msg.contains("403") || error_msg.contains("unauthorized")
    }

    fn is_not_found_error(error_msg: &str) -> bool {
        error_msg.contains("404") || error_msg.contains("not_found")
    }

    async fn ensure_token(&self) -> Result<String, ServiceError> {
        match &self.token {
            Some(token) => Ok(token.clone()),
            None => read_dropbox_token_from_env().map_err(|e| ServiceError::fatal(e)),
        }
    }
}

#[async_trait]
impl FileService for DropboxService {
    fn detect_link(input: &str) -> Option<String> {
        Self::extract_file_id(input)
    }
}

#[async_trait]
impl FileDownloadService for DropboxService {
    fn service_name(&self) -> &str {
        "dropbox"
    }

    async fn download(&mut self, file_id: &str, dest_path: &Path) -> Result<(), ServiceError> {
        let token = self.ensure_token().await?;

        download_dropbox_file(file_id, dest_path, Some(token))
            .await
            .map(|_| ())
            .map_err(|e| Self::classify_error(&e.to_string()))
    }

    async fn get_file_info(
        &mut self,
        file_id: &str,
    ) -> Result<run_downloader::FileInfo, ServiceError> {
        let token = self.ensure_token().await?;

        get_file_metadata(file_id, Some(token))
            .await
            .map(Self::convert_metadata)
            .map_err(|e| Self::classify_error(&e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_FILE_PATH: &str = "/foo.zip";

    #[test]
    fn test_detect_link() {
        let test_cases = [
            (
                "https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0",
                Some("/foo.zip".to_string()),
            ),
            (
                "https://www.dropbox.com/s/abc123/test.zip?dl=0",
                Some("/test.zip".to_string()),
            ),
            (
                "https://example.com/not-a-dropbox-link",
                None,
            ),
            ("just some text", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(DropboxService::detect_link(input), expected);
        }
    }

    #[test]
    fn test_service_name() {
        let service = DropboxService::new(None);
        assert_eq!(service.service_name(), "dropbox");
    }

    #[test]
    fn test_infer_mime_type() {
        assert_eq!(
            DropboxService::infer_mime_type("test.zip"),
            Some("application/zip".to_string())
        );
        assert_eq!(
            DropboxService::infer_mime_type("test.txt"),
            Some("text/plain".to_string())
        );
        assert_eq!(
            DropboxService::infer_mime_type("unknown"),
            Some("application/octet-stream".to_string())
        );
    }

    #[tokio::test]
    async fn test_service_without_token() {
        let mut service = DropboxService::new(None);

        let result = service.get_file_info(TEST_FILE_PATH).await;
        assert!(result.is_err());

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let result = service.download(TEST_FILE_PATH, temp_file.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_service_with_token() {
        if let Ok(token) = std::env::var("DROPBOX_TOKEN") {
            let mut service = DropboxService::new(Some(token));

            let file_info_result = service.get_file_info(TEST_FILE_PATH).await;
            if let Ok(file_info) = file_info_result {
                assert_eq!(file_info.name, "foo.zip");
                assert_eq!(file_info.size, 119);
                assert_eq!(file_info.mime_type, Some("application/zip".to_string()));

                let temp_file = tempfile::NamedTempFile::new().unwrap();
                let download_result = service.download(TEST_FILE_PATH, temp_file.path()).await;
                assert!(download_result.is_ok());

                assert!(temp_file.path().exists());
                let metadata = std::fs::metadata(temp_file.path()).unwrap();
                assert_eq!(metadata.len(), 119);
            }
        }
    }
}
