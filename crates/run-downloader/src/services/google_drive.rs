use crate::{
    ServiceError,
    services::{FileInfo, FileService},
};
use anyhow::anyhow;
use async_trait::async_trait;
use google_drive::{Client, ClientError, traits::FileOps};
use lazy_static::lazy_static;
use regex::Regex;
use std::path::Path;

use super::FileDownloadService;
pub struct GoogleDriveService {
    client: Client,
}

impl GoogleDriveService {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl FileService for GoogleDriveService {
    fn detect_link(input: &str) -> Option<String> {
        lazy_static! {
            static ref GOOGLE_DRIVE_URL_REGEX: Regex = Regex::new(
                r"(?:https://)?(?:drive\.google\.com/(?:file/d/|open\?id=)|docs\.google\.com/file/d/)([a-zA-Z0-9_-]+)"
            ).unwrap();
        }

        let cap = GOOGLE_DRIVE_URL_REGEX.captures(input)?;
        let id = cap.get(1)?.as_str().to_string();
        return Some(id);
    }
}

#[async_trait]
impl FileDownloadService for GoogleDriveService {
    async fn get_file_info(&mut self, file_id: &str) -> Result<FileInfo, ServiceError> {
        // Get file metadata using the google_drive crate
        let response = self
            .client
            .files()
            .get(
                file_id, false, // acknowledge_abuse
                "",    // include_permissions_for_view
                true,  // supports_all_drives
                false, // supports_team_drives
            )
            .await
            .map_err(Self::convert_client_error)?;

        let file = response.body;
        // Extract file information
        let name = if file.name.is_empty() {
            "unknown.zip".to_string()
        } else {
            file.name
        };
        let size = file.size as u64;
        let mime_type = if file.mime_type.is_empty() {
            None
        } else {
            Some(file.mime_type)
        };

        let file_info = FileInfo {
            name,
            size,
            mime_type,
        };

        Ok(file_info)
    }

    async fn download(&mut self, file_id: &str, dest_path: &Path) -> Result<(), ServiceError> {
        // Download file content using the google_drive crate
        let response = self
            .client
            .files()
            .download_by_id(file_id)
            .await
            .map_err(Self::convert_client_error)?;

        let bytes = response.body;

        // Validate content type if available
        if let Some(content_type) = response
            .headers
            .get("content-type")
            .and_then(|ct| ct.to_str().ok())
        {
            crate::security::validate_content_type(Some(content_type))
                .map_err(|e| ServiceError::fatal(e))?;
        }

        // Write to destination file
        std::fs::write(dest_path, bytes)
            .map_err(|e| ServiceError::fatal(anyhow!("Failed to write file: {}", e)))?;

        Ok(())
    }

    fn service_name(&self) -> &'static str {
        "google_drive"
    }
}

impl GoogleDriveService {
    fn convert_client_error(error: ClientError) -> ServiceError {
        match error {
            ClientError::ReqwestError(req_err) => {
                if let Some(status) = req_err.status() {
                    match status.as_u16() {
                        404 => ServiceError::fatal(anyhow!("File not found or access denied")),
                        401 | 403 => {
                            ServiceError::fatal(anyhow!("Authentication failed: Access denied"))
                        }
                        429 => ServiceError::retryable(anyhow!("API rate limit exceeded")),
                        500..=599 => {
                            ServiceError::retryable(anyhow!("Server error: HTTP {}", status))
                        }
                        _ => ServiceError::retryable(anyhow!("HTTP error: {}", status)),
                    }
                } else {
                    ServiceError::retryable(anyhow!("Network error: {}", req_err))
                }
            }
            ClientError::HttpError { status, error, .. } => match status.as_u16() {
                404 => ServiceError::fatal(anyhow!("File not found or access denied")),
                401 | 403 => ServiceError::fatal(anyhow!("Authentication failed: {}", error)),
                429 => ServiceError::retryable(anyhow!("API rate limit exceeded")),
                500..=599 => {
                    ServiceError::retryable(anyhow!("Server error: HTTP {}: {}", status, error))
                }
                _ => ServiceError::retryable(anyhow!("HTTP error {}: {}", status, error)),
            },
            ClientError::SerdeJsonError(e) => {
                ServiceError::fatal(anyhow!("JSON parsing error: {}", e))
            }
            ClientError::EmptyRefreshToken => {
                ServiceError::fatal(anyhow!("Authentication failed: Empty refresh token"))
            }
            ClientError::DriveNotFound { name } => {
                ServiceError::fatal(anyhow!("Drive not found: {}", name))
            }
            _ => ServiceError::fatal(anyhow!("Google Drive client error: {}", error)),
        }
    }
}
