use crate::error::ServiceError;
use crate::logging;
use crate::services::{FileInfo, FileService};
use anyhow::anyhow;
use async_trait::async_trait;
use google_drive::{Client, ClientError, traits::FileOps};
use lazy_static::lazy_static;
use regex::Regex;
use std::path::Path;
use tracing::{debug, error, info, instrument};

lazy_static! {
    static ref GOOGLE_DRIVE_URL_REGEX: Regex = Regex::new(
        r"(?:https://)?(?:drive\.google\.com/(?:file/d/|open\?id=)|docs\.google\.com/file/d/)([a-zA-Z0-9_-]+)"
    ).unwrap();
}

fn detect_google_drive_links(input: &str) -> Vec<String> {
    let mut file_ids = Vec::new();

    for cap in GOOGLE_DRIVE_URL_REGEX.captures_iter(input) {
        if let Some(file_id) = cap.get(1) {
            let id = file_id.as_str().to_string();
            debug!("Found Google Drive file ID: {}", id);
            file_ids.push(id);
        }
    }

    info!("Detected {} Google Drive links", file_ids.len());
    file_ids
}

pub struct GoogleDriveService {
    client: Client,
}

impl GoogleDriveService {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl FileService for GoogleDriveService {
    type FileId = String;

    fn detect_links(&self, input: &str) -> Vec<Self::FileId> {
        detect_google_drive_links(input)
    }

    #[instrument(skip(self))]
    async fn get_file_info(&self, file_id: &Self::FileId) -> Result<FileInfo, ServiceError> {
        let _span = logging::service_operation_span(self.service_name(), "get_file_info");

        debug!("Getting file info for Google Drive file: {}", file_id);

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
            .map_err(|e| {
                error!("Failed to get Google Drive file info: {}", e);
                Self::convert_client_error(e)
            })?;

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

        // Check if file is publicly accessible by looking at permissions
        // We'll be conservative and assume it's not public unless we can confirm
        let is_public = !file.permissions.is_empty();

        let file_info = FileInfo {
            name,
            size,
            mime_type,
            is_public,
        };

        debug!("Retrieved Google Drive file info: {:?}", file_info);
        Ok(file_info)
    }

    #[instrument(skip(self))]
    async fn download(&self, file_id: &Self::FileId, dest_path: &Path) -> Result<(), ServiceError> {
        let _span = logging::download_span(self.service_name(), file_id);

        info!("Starting download of Google Drive file: {}", file_id);

        // Download file content using the google_drive crate
        let response = self
            .client
            .files()
            .download_by_id(file_id)
            .await
            .map_err(|e| {
                error!("Failed to download from Google Drive: {}", e);
                Self::convert_client_error(e)
            })?;

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

        info!(
            "Successfully downloaded Google Drive file to {:?}",
            dest_path
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_detection() {
        let test_cases = vec![
            (
                "https://drive.google.com/file/d/1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms/view",
                "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
            ),
            (
                "https://drive.google.com/open?id=1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
                "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
            ),
            (
                "https://docs.google.com/file/d/1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
                "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
            ),
        ];

        for (url, expected_id) in test_cases {
            let links = detect_google_drive_links(url);
            assert_eq!(links.len(), 1);
            assert_eq!(links[0], expected_id);
        }
    }

    #[test]
    fn test_no_links_detected() {
        let no_link_text = "This is just some text without any Google Drive links";
        let links = detect_google_drive_links(no_link_text);
        assert!(links.is_empty());
    }

    #[test]
    fn test_multiple_links() {
        let text = "Check out these files: https://drive.google.com/file/d/123/view and https://drive.google.com/open?id=456";
        let links = detect_google_drive_links(text);
        assert_eq!(links.len(), 2);
        assert!(links.contains(&"123".to_string()));
        assert!(links.contains(&"456".to_string()));
    }
}
