use crate::services::{FileMeta, FileService};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use lazy_static::lazy_static;
use regex::Regex;
use std::path::Path;
use tokio::io::AsyncWriteExt as _;

const DRIVE_PUBLIC_BASE: &str = "https://drive.google.com/uc?export=download&id";

fn public_download_url(file_id: &str) -> String {
    format!("{}={}", DRIVE_PUBLIC_BASE, file_id)
}

async fn get_file_info(client: &reqwest::Client, file_id: &str) -> Result<FileMeta> {
    let url = public_download_url(file_id);
    let response = client.head(&url).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get file metadata: HTTP {}", response.status());
    }

    let headers = response.headers();

    let name = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(|disposition| {
            disposition
                .split("filename=")
                .nth(1)
                .map(|s| s.trim_matches('"'))
        })
        .unwrap_or("unknown")
        .to_string();

    let size = headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let content_type = headers.get("content-type").and_then(|v| v.to_str().ok());

    let is_zip = content_type
        .map(|s| s.to_lowercase().contains("zip"))
        .unwrap_or(false)
        || name.to_lowercase().ends_with(".zip");

    Ok(FileMeta { name, size, is_zip })
}

async fn download_file_streaming(file_id: &str, dest: &Path) -> Result<()> {
    let client = reqwest::Client::new();
    let url = public_download_url(file_id);
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download file: HTTP {}", response.status());
    }

    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        file.write_all(&chunk?).await?;
    }

    file.flush().await?;

    Ok(())
}

lazy_static! {
    static ref GOOGLE_DRIVE_URL_PATTERNS: [Regex; 2] = [
        Regex::new(r"https://drive\.google\.com/file/d/([a-zA-Z0-9_-]+)").unwrap(),
        Regex::new(r"https://drive\.google\.com/open\?id=([a-zA-Z0-9_-]+)").unwrap(),
    ];
}

pub struct GoogleDriveService;

impl GoogleDriveService {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GoogleDriveService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FileService for GoogleDriveService {
    type FileId = String;

    fn service_name() -> &'static str {
        "google_drive"
    }

    fn detect_link(input: &str) -> Option<Self::FileId> {
        GOOGLE_DRIVE_URL_PATTERNS.iter().find_map(|pattern| {
            pattern
                .captures(input)
                .and_then(|captures| captures.get(1))
                .map(|id| id.as_str().to_string())
        })
    }

    async fn get_file_info(&mut self, file_id: &Self::FileId) -> Result<FileMeta> {
        let client = reqwest::Client::new();
        get_file_info(&client, file_id).await
    }

    async fn download(&mut self, file_id: &Self::FileId, dest: &Path) -> Result<()> {
        download_file_streaming(file_id, dest).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

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
            ("https://example.com/not-a-drive-link", None),
            ("just some text", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(GoogleDriveService::detect_link(input), expected);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_file_info() {
        let mut service = GoogleDriveService::new();
        let file_info = service
            .get_file_info(&TEST_FILE_ID.to_string())
            .await
            .unwrap();
        assert_eq!(file_info.name, "foo.zip");
        assert_eq!(file_info.size, 119);
        assert!(file_info.is_zip);
    }

    #[tokio::test]
    #[ignore]
    async fn test_download() {
        let mut service = GoogleDriveService::new();
        let temp_file = NamedTempFile::new().unwrap();
        service
            .download(&TEST_FILE_ID.to_string(), temp_file.path())
            .await
            .unwrap();

        assert!(temp_file.path().exists());
        let metadata = std::fs::metadata(temp_file.path()).unwrap();
        assert_eq!(metadata.len(), 119);
    }
}
