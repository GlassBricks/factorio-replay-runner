use crate::{FileMeta, services::FileService};
use anyhow::Result;
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use std::{path::Path, process::Command};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

fn create_curl_command(url: &str) -> Command {
    let mut cmd = Command::new("curl");
    cmd.arg("-s")
        .arg("-L")
        .arg("-H")
        .arg(format!("User-Agent: {}", USER_AGENT))
        .arg("-H")
        .arg("Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8")
        .arg("-H")
        .arg("Accept-Language: en-US,en;q=0.9")
        .arg("-H")
        .arg("DNT: 1")
        .arg("-H")
        .arg("Upgrade-Insecure-Requests: 1")
        .arg("-H")
        .arg("Sec-Fetch-Dest: document")
        .arg("-H")
        .arg("Sec-Fetch-Mode: navigate")
        .arg("-H")
        .arg("Sec-Fetch-Site: none")
        .arg("-H")
        .arg("Sec-Fetch-User: ?1")
        .arg(url);
    cmd
}

lazy_static! {
    static ref SPEEDRUN_URL_PATTERN: Regex = Regex::new(
        r"https://(?:www\.)?speedrun\.com/static/resource/[a-zA-Z0-9]+\.zip(?:\?[^\s#]*)?"
    )
    .unwrap();
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeedrunFileId(String);

impl SpeedrunFileId {
    pub fn new(url: String) -> Self {
        Self(url)
    }

    pub fn url(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SpeedrunFileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

async fn get_file_info(file_id: &SpeedrunFileId) -> Result<FileMeta> {
    // Use curl to get just the headers
    let output = create_curl_command(file_id.url()).arg("-I").output()?;

    if !output.status.success() {
        // If HEAD request fails, fall back to extracting from URL
        let name = file_id
            .url()
            .split('/')
            .next_back()
            .and_then(|s| s.split('?').next())
            .unwrap_or("unknown.zip")
            .to_string();

        return Ok(FileMeta {
            name: name.clone(),
            size: 0,
            is_zip: name.to_lowercase().ends_with(".zip"),
        });
    }

    let headers = String::from_utf8_lossy(&output.stdout);

    let name = headers
        .lines()
        .find(|line| line.to_lowercase().starts_with("content-disposition:"))
        .and_then(|line| {
            line.split("filename=")
                .nth(1)
                .map(|s| s.trim_matches('"').trim())
        })
        .unwrap_or_else(|| {
            file_id
                .url()
                .split('/')
                .next_back()
                .and_then(|s| s.split('?').next())
                .unwrap_or("unknown.zip")
        })
        .to_string();

    let size = headers
        .lines()
        .find(|line| line.to_lowercase().starts_with("content-length:"))
        .and_then(|line| line.split(':').nth(1).and_then(|s| s.trim().parse().ok()))
        .unwrap_or(0);

    let content_type = headers
        .lines()
        .find(|line| line.to_lowercase().starts_with("content-type:"))
        .map(|line| line.to_lowercase())
        .unwrap_or_default();

    let is_zip = content_type.contains("zip") || name.to_lowercase().ends_with(".zip");

    Ok(FileMeta { name, size, is_zip })
}

async fn download_file(file_id: &SpeedrunFileId, dest: &Path) -> Result<()> {
    let output = create_curl_command(file_id.url())
        .arg("-o")
        .arg(dest)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to download file with curl: {}", stderr);
    }

    Ok(())
}

pub struct SpeedrunService;

impl SpeedrunService {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SpeedrunService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FileService for SpeedrunService {
    type FileId = SpeedrunFileId;

    fn service_name() -> &'static str {
        "speedrun"
    }

    fn detect_link(input: &str) -> Option<Self::FileId> {
        SPEEDRUN_URL_PATTERN
            .find(input)
            .map(|m| SpeedrunFileId::new(m.as_str().to_string()))
    }

    async fn get_file_info(&mut self, file_id: &Self::FileId) -> Result<FileMeta> {
        get_file_info(file_id).await
    }

    async fn download(&mut self, file_id: &Self::FileId, dest: &Path) -> Result<()> {
        download_file(file_id, dest).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    const TEST_URL: &str = "https://www.speedrun.com/static/resource/1d4e2.zip?v=6d7a0c5";

    #[test]
    fn test_detect_link() {
        let test_cases = [
            (TEST_URL, Some(SpeedrunFileId::new(TEST_URL.to_string()))),
            (
                "https://speedrun.com/static/resource/abc123.zip",
                Some(SpeedrunFileId::new(
                    "https://speedrun.com/static/resource/abc123.zip".to_string(),
                )),
            ),
            (
                &format!("Check out this replay: {} - it's amazing!", TEST_URL),
                Some(SpeedrunFileId::new(TEST_URL.to_string())),
            ),
            ("https://example.com/not-a-speedrun-link.zip", None),
            ("https://www.speedrun.com/factorio", None),
            ("just some text", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(SpeedrunService::detect_link(input), expected);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_file_info() {
        let mut service = SpeedrunService::new();
        let file_id = SpeedrunFileId::new(TEST_URL.to_string());

        let file_info = service.get_file_info(&file_id).await.unwrap();
        assert!(file_info.name.ends_with(".zip"));
        assert!(file_info.is_zip);
        // Name might be from Content-Disposition header or URL
        assert!(file_info.name.contains("Steelaxe") || file_info.name == "1d4e2.zip");
    }

    #[tokio::test]
    #[ignore]
    async fn test_download() {
        let mut service = SpeedrunService::new();
        let file_id = SpeedrunFileId::new(TEST_URL.to_string());

        let temp_file = NamedTempFile::new().unwrap();

        service.download(&file_id, temp_file.path()).await.unwrap();

        assert!(temp_file.path().exists());
        let metadata = std::fs::metadata(temp_file.path()).unwrap();
        assert!(metadata.len() > 0);
        assert!(metadata.len() > 800_000); // Should be around 890KB based on our test
    }

    #[tokio::test]
    #[ignore]
    async fn test_file_info_and_download_integration() {
        let mut service = SpeedrunService::new();
        let file_id = SpeedrunFileId::new(TEST_URL.to_string());

        // Test file info
        let file_info = service.get_file_info(&file_id).await.unwrap();
        assert!(file_info.is_zip);

        // Test download
        let temp_file = NamedTempFile::new().unwrap();

        service.download(&file_id, temp_file.path()).await.unwrap();

        let metadata = std::fs::metadata(temp_file.path()).unwrap();
        assert!(metadata.len() > 0);
    }

    #[tokio::test]
    #[ignore]
    async fn test_file_downloader_integration() {
        use crate::FileDownloader;

        let service = SpeedrunService::new();
        let mut downloader = FileDownloader::builder().add_service(service).build();

        match downloader.download_zip_to_temp(TEST_URL).await {
            Ok((file, info)) => {
                assert!(info.name.ends_with(".zip"));
                assert!(file.path().exists());

                let metadata = std::fs::metadata(file.path()).unwrap();
                assert!(metadata.len() > 800_000); // Should be around 890KB
            }
            Err(e) => {
                panic!("FileDownloader integration test failed: {}", e);
            }
        }
    }
}
