use crate::DownloadError;
use crate::services::{FileMeta, FileService};
use async_trait::async_trait;
use futures::StreamExt;
use lazy_static::lazy_static;
use regex::Regex;
use std::path::Path;

lazy_static! {
    static ref DROPBOX_URL_PATTERNS: [Regex; 2] = [
        Regex::new(r"https://(?:www\.)?dropbox\.com/scl/fi/[^/]+/[^?\s]+(?:\?[^\s#]*)?").unwrap(),
        Regex::new(r"https://(?:www\.)?dropbox\.com/s/[^/]+/[^?\s]+(?:\?[^\s#]*)?").unwrap(),
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropboxFileId(String);

impl DropboxFileId {
    pub fn new(url: String) -> Self {
        Self(url)
    }

    pub fn url(&self) -> &str {
        &self.0
    }

    fn to_direct_download_url(&self) -> String {
        self.0
            .replace("?dl=0", "?dl=1")
            .replace("www.dropbox.com", "dl.dropboxusercontent.com")
    }
}

impl std::fmt::Display for DropboxFileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

async fn get_file_info(file_id: &DropboxFileId) -> Result<FileMeta, DownloadError> {
    let client = reqwest::Client::new();
    let url = file_id.to_direct_download_url();
    let response = client.head(&url).send().await.map_err(|e| {
        DownloadError::ServiceError(
            anyhow::Error::from(e).context("Failed to send request to Dropbox"),
        )
    })?;

    if !response.status().is_success() {
        return Err(DownloadError::FileNotAccessible(anyhow::anyhow!(
            "HTTP {} from Dropbox",
            response.status()
        )));
    }

    let headers = response.headers();

    let name = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(|disposition| {
            disposition
                .split("filename=")
                .nth(1)
                .and_then(|s| s.split(';').next())
                .map(|s| s.trim_matches('"'))
        })
        .or_else(|| file_id.url().split('/').find(|s| s.ends_with(".zip")))
        .unwrap_or("unknown.zip")
        .to_string();

    let size = headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Ok(FileMeta { name, size })
}

async fn download_file(file_id: &DropboxFileId, dest: &Path) -> Result<(), DownloadError> {
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::new();
    let url = file_id.to_direct_download_url();
    let response = client.get(&url).send().await.map_err(|e| {
        DownloadError::ServiceError(
            anyhow::Error::from(e).context("Failed to send request to Dropbox"),
        )
    })?;

    if !response.status().is_success() {
        return Err(DownloadError::FileNotAccessible(anyhow::anyhow!(
            "HTTP {} from Dropbox",
            response.status()
        )));
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(DownloadError::IoError)?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| {
            DownloadError::ServiceError(
                anyhow::Error::from(e).context("Failed to read response stream"),
            )
        })?;
        file.write_all(&bytes)
            .await
            .map_err(DownloadError::IoError)?;
    }

    file.flush().await.map_err(DownloadError::IoError)?;

    Ok(())
}

pub struct DropboxService;

impl DropboxService {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DropboxService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FileService for DropboxService {
    type FileId = DropboxFileId;

    fn service_name() -> &'static str {
        "dropbox"
    }

    fn detect_link(input: &str) -> Option<Self::FileId> {
        DROPBOX_URL_PATTERNS.iter().find_map(|pattern| {
            pattern
                .find(input)
                .map(|m| DropboxFileId::new(m.as_str().to_string()))
        })
    }

    async fn get_file_info(&mut self, file_id: &Self::FileId) -> Result<FileMeta, DownloadError> {
        get_file_info(file_id).await
    }

    async fn download(&mut self, file_id: &Self::FileId, dest: &Path) -> Result<(), DownloadError> {
        download_file(file_id, dest).await
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::FileDownloader;

    const TEST_URL: &str = "https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0";

    #[test]
    fn test_detect_link() {
        const TEST_URL_2: &str = "https://www.dropbox.com/s/abc123/test.zip?dl=0";
        let test_cases = [
            (TEST_URL, Some(DropboxFileId::new(TEST_URL.to_string()))),
            (TEST_URL_2, Some(DropboxFileId::new(TEST_URL_2.to_string()))),
            (
                &format!("Check out this link : {} It do be cool", TEST_URL),
                Some(DropboxFileId::new(TEST_URL.to_string())),
            ),
            (
                &format!("Check out this link : {} It do be cool", TEST_URL_2),
                Some(DropboxFileId::new(TEST_URL_2.to_string())),
            ),
            ("https://example.com/not-a-dropbox-link", None),
            ("just some text", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(DropboxService::detect_link(input), expected);
        }
    }

    async fn file_info_test(
        service: &mut DropboxService,
        test_url: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file_id = DropboxFileId::new(test_url.to_string());
        let file_info = service.get_file_info(&file_id).await?;
        assert_eq!(file_info.name, "foo.zip");
        assert_eq!(file_info.size, 119);
        Ok(())
    }

    async fn download_test(
        service: &mut DropboxService,
        test_url: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file_id = DropboxFileId::new(test_url.to_string());
        let temp_file = tempfile::NamedTempFile::new()?;
        service.download(&file_id, temp_file.path()).await?;

        assert!(temp_file.path().exists());
        let metadata = std::fs::metadata(temp_file.path())?;
        assert_eq!(metadata.len(), 119);
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_service() {
        let mut service = DropboxService::new();

        file_info_test(&mut service, TEST_URL).await.unwrap();
        download_test(&mut service, TEST_URL).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_file_downloader_integration() -> anyhow::Result<()> {
        let service = DropboxService::new();
        let mut downloader = FileDownloader::builder().add_service(service).build();

        let (file, info) = downloader.download_zip_to_temp(TEST_URL).await?;
        assert_eq!(info.name, "foo.zip");
        assert!(file.path().exists());

        let metadata = std::fs::metadata(file.path()).unwrap();
        assert_eq!(metadata.len(), 119);
        Ok(())
    }
}
