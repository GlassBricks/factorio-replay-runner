use crate::services::{FileMeta, FileService};
use crate::DownloadError;
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

async fn get_file_info(client: &reqwest::Client, file_id: &str) -> Result<FileMeta, DownloadError> {
    get_file_info_from_headers(client, file_id).await
}

async fn get_file_info_from_headers(
    client: &reqwest::Client,
    file_id: &str,
) -> Result<FileMeta, DownloadError> {
    let url = public_download_url(file_id);

    let response = client.get(&url).send().await.map_err(|e| {
        DownloadError::ServiceError(anyhow::Error::from(e).context("Failed to send request to Google Drive"))
    })?;

    if !response.status().is_success() {
        return Err(DownloadError::FileNotAccessible(anyhow::anyhow!(
            "HTTP {} from Google Drive",
            response.status()
        )));
    }

    let headers = response.headers();

    if let Some(content_type) = headers.get("content-type")
        && let Ok(ct) = content_type.to_str()
        && ct.contains("text/html")
    {
        return Err(DownloadError::FileNotAccessible(anyhow::anyhow!(
            "File requires authentication or is not publicly shared"
        )));
    }
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

    Ok(FileMeta { name, size })
}

async fn download_file_streaming(file_id: &str, dest: &Path) -> Result<(), DownloadError> {
    let client = reqwest::Client::new();
    let url = public_download_url(file_id);
    let response = client.get(&url).send().await.map_err(|e| {
        DownloadError::ServiceError(anyhow::Error::from(e).context("Failed to send request to Google Drive"))
    })?;

    if !response.status().is_success() {
        return Err(DownloadError::FileNotAccessible(anyhow::anyhow!(
            "HTTP {} from Google Drive",
            response.status()
        )));
    }

    if let Some(content_type) = response.headers().get("content-type")
        && let Ok(ct) = content_type.to_str()
        && ct.contains("text/html")
    {
        return Err(DownloadError::FileNotAccessible(anyhow::anyhow!(
            "File requires authentication or is not publicly shared. \
             Please ensure the file is publicly accessible"
        )));
    }

    let mut file = tokio::fs::File::create(dest).await.map_err(DownloadError::IoError)?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| {
            DownloadError::ServiceError(anyhow::Error::from(e).context("Failed to read response stream"))
        })?;
        file.write_all(&bytes).await.map_err(DownloadError::IoError)?;
    }

    file.flush().await.map_err(DownloadError::IoError)?;

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

    async fn get_file_info(&mut self, file_id: &Self::FileId) -> Result<FileMeta, DownloadError> {
        let client = reqwest::Client::new();
        get_file_info(&client, file_id).await
    }

    async fn download(&mut self, file_id: &Self::FileId, dest: &Path) -> Result<(), DownloadError> {
        download_file_streaming(file_id, dest).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::NamedTempFile;

    const TEST_FILE_ID: &str = "1mFrMybb8RsSrg4KTx6C3wp1xPdD4nAeI";

    #[test]
    fn test_detect_link() {
        let test_cases = [
            (
                "https://drive.google.com/file/d/1mFrMybb8RsSrg4KTx6C3wp1xPdD4nAeI/view?usp=sharing",
                Some("1mFrMybb8RsSrg4KTx6C3wp1xPdD4nAeI".to_string()),
            ),
            (
                "https://drive.google.com/open?id=1mFrMybb8RsSrg4KTx6C3wp1xPdD4nAeI",
                Some("1mFrMybb8RsSrg4KTx6C3wp1xPdD4nAeI".to_string()),
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
        let mut service = GoogleDriveService;
        let result = service.get_file_info(&TEST_FILE_ID.to_string()).await;

        match result {
            Ok(file_info) => {
                println!(
                    "File info: name={}, size={}",
                    file_info.name, file_info.size
                );
                assert!(file_info.name.ends_with(".zip"));
                assert!(file_info.size > 0);
            }
            Err(e) => {
                let error_msg = e.to_string();
                println!("Got expected error: {}", error_msg);
                assert!(
                    error_msg.contains("File not accessible")
                        || error_msg.contains("authentication required"),
                    "Expected access error, got: {}",
                    error_msg
                );
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_debug_headers() {
        let client = reqwest::Client::new();
        let url = public_download_url(TEST_FILE_ID);
        let response = client.get(&url).send().await.unwrap();

        println!("Status: {}", response.status());
        println!("URL: {}", response.url());
        println!("\nHeaders:");
        for (name, value) in response.headers() {
            println!("  {}: {:?}", name, value);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_download() {
        let mut service = GoogleDriveService;
        let temp_file = NamedTempFile::new().unwrap();
        let result = service
            .download(&TEST_FILE_ID.to_string(), temp_file.path())
            .await;

        match result {
            Ok(_) => {
                assert!(temp_file.path().exists());
                let metadata = std::fs::metadata(temp_file.path()).unwrap();
                println!("Downloaded file size: {}", metadata.len());

                let mut first_bytes = vec![0u8; 4];
                std::fs::File::open(temp_file.path())
                    .unwrap()
                    .read_exact(&mut first_bytes)
                    .unwrap();
                println!("First 4 bytes: {:?}", first_bytes);

                assert!(metadata.len() > 0);
                assert_eq!(&first_bytes[0..2], b"PK", "File should be a ZIP file");
            }
            Err(e) => {
                let error_msg = e.to_string();
                println!("Got expected error: {}", error_msg);
                assert!(
                    error_msg.contains("File not accessible")
                        || error_msg.contains("authentication required"),
                    "Expected access error, got: {}",
                    error_msg
                );
            }
        }
    }
}
