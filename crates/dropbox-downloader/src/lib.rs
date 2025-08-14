use anyhow::Result;
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use run_downloader::{ServiceError, services::FileService};
use std::{env, fs::File, io::Write};
use tokio::io::AsyncReadExt;
use tokio_util::compat::FuturesAsyncReadCompatExt;

use dropbox_sdk::{
    async_routes::files::{download, get_metadata},
    default_async_client::UserAuthDefaultClient,
    files::{DownloadArg, GetMetadataArg, Metadata},
    oauth2::Authorization,
};

const DROPBOX_TOKEN_ENV: &str = "DROPBOX_TOKEN";

pub async fn read_dropbox_token_from_env() -> Result<String> {
    env::var(DROPBOX_TOKEN_ENV)
        .map_err(|_| anyhow::anyhow!("DROPBOX_TOKEN environment variable not set"))
}

lazy_static! {
    static ref DROPBOX_URL_PATTERNS: [Regex; 2] = [
        Regex::new(r"https://(?:www\.)?dropbox\.com/scl/fi/[^/]+/([^?]+)").unwrap(),
        Regex::new(r"https://(?:www\.)?dropbox\.com/s/[^/]+/([^?]+)").unwrap(),
    ];
}

async fn get_file_info(file_path: &str, token: &str) -> Result<run_downloader::FileInfo> {
    #[expect(deprecated)]
    let auth = Authorization::from_long_lived_access_token(token.to_string());
    let client = UserAuthDefaultClient::new(auth);

    let arg = GetMetadataArg::new(file_path.to_string());
    let metadata = get_metadata(&client, &arg).await?;

    match metadata {
        Metadata::File(file_meta) => {
            let is_zip = file_meta.name.to_lowercase().ends_with(".zip");
            Ok(run_downloader::FileInfo {
                name: file_meta.name,
                size: file_meta.size,
                is_zip,
            })
        }
        Metadata::Folder(_) => {
            anyhow::bail!("Expected file but got folder at path: {}", file_path)
        }
        Metadata::Deleted(_) => {
            anyhow::bail!("File not found (deleted): {}", file_path)
        }
    }
}

async fn download_file(file_path: &str, dest: &mut File, token: &str) -> Result<()> {
    #[expect(deprecated)]
    let auth = Authorization::from_long_lived_access_token(token.to_string());
    let client = UserAuthDefaultClient::new(auth);

    let arg = DownloadArg::new(file_path.to_string());
    let response = download(&client, &arg, None, None).await?;

    if let Some(reader) = response.body {
        let mut compat_reader = reader.compat();
        let mut buffer = [0; 8192];

        loop {
            match compat_reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    dest.write_all(&buffer[..n])?;
                }
                Err(e) => {
                    anyhow::bail!("Read error: {}", e);
                }
            }
        }
        dest.flush()?;
    } else {
        anyhow::bail!("No response body received from Dropbox");
    }

    Ok(())
}

pub struct DropboxService {
    token: Option<String>,
}

impl DropboxService {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }

    pub async fn from_env() -> anyhow::Result<Self> {
        let token = read_dropbox_token_from_env().await?;
        Ok(Self::new(Some(token)))
    }

    async fn ensure_token(&self) -> Result<String, ServiceError> {
        match &self.token {
            Some(token) => Ok(token.clone()),
            None => read_dropbox_token_from_env()
                .await
                .map_err(|e| ServiceError::fatal(e)),
        }
    }

    fn classify_error(error_msg: &str) -> ServiceError {
        if error_msg.contains("401")
            || error_msg.contains("403")
            || error_msg.contains("unauthorized")
        {
            ServiceError::retryable(anyhow::anyhow!("{}", error_msg))
        } else if error_msg.contains("404") || error_msg.contains("not_found") {
            ServiceError::fatal(anyhow::anyhow!("{}", error_msg))
        } else {
            ServiceError::retryable(anyhow::anyhow!("{}", error_msg))
        }
    }
}

#[async_trait]
impl FileService for DropboxService {
    type FileId = String;

    fn service_name() -> &'static str {
        "dropbox"
    }

    fn detect_link(input: &str) -> Option<Self::FileId> {
        DROPBOX_URL_PATTERNS.iter().find_map(|pattern| {
            pattern
                .captures(input)
                .and_then(|captures| captures.get(1))
                .map(|filename| format!("/{}", filename.as_str()))
        })
    }

    async fn get_file_info(
        &mut self,
        file_id: &Self::FileId,
    ) -> Result<run_downloader::services::FileInfo, ServiceError> {
        let token = self.ensure_token().await?;

        get_file_info(file_id, &token)
            .await
            .map_err(|e| Self::classify_error(&e.to_string()))
    }

    async fn download(
        &mut self,
        file_id: &Self::FileId,
        dest: &mut File,
    ) -> Result<(), ServiceError> {
        let token = self.ensure_token().await?;

        download_file(file_id, dest, &token)
            .await
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
            ("https://example.com/not-a-dropbox-link", None),
            ("just some text", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(DropboxService::detect_link(input), expected);
        }
    }

    async fn file_info_test(service: &mut DropboxService) {
        let file_info = service
            .get_file_info(&TEST_FILE_PATH.to_string())
            .await
            .unwrap();
        assert_eq!(file_info.name, "foo.zip");
        assert_eq!(file_info.size, 119);
        assert!(file_info.is_zip);
    }

    async fn download_test(service: &mut DropboxService) {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(temp_file.path())
            .unwrap();
        service
            .download(&TEST_FILE_PATH.to_string(), &mut file)
            .await
            .unwrap();

        assert!(temp_file.path().exists());
        let metadata = std::fs::metadata(temp_file.path()).unwrap();
        assert_eq!(metadata.len(), 119);
    }

    #[tokio::test]
    async fn test_service_with_token() {
        let Ok(mut service) = DropboxService::from_env().await else {
            // Skip test if no DROPBOX_TOKEN is available
            eprintln!("Skipping Dropbox integration test - DROPBOX_TOKEN not set");
            return;
        };

        file_info_test(&mut service).await;
        download_test(&mut service).await;
    }

    #[tokio::test]
    async fn test_file_downloader_integration() {
        let Ok(service) = DropboxService::from_env().await else {
            eprintln!("Skipping FileDownloader integration test - DROPBOX_TOKEN not set");
            return;
        };

        let mut downloader = run_downloader::FileDownloader::builder()
            .add_service(service)
            .build();

        let test_url = "https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0";

        match downloader.download_zip(test_url).await {
            Ok(downloaded_zip) => {
                assert_eq!(downloaded_zip.service_name, "dropbox");
                assert_eq!(downloaded_zip.file_info.name, "foo.zip");
                assert_eq!(downloaded_zip.file_info.size, 119);
                assert!(downloaded_zip.file_info.is_zip);
                assert!(downloaded_zip.file.path().exists());

                let metadata = std::fs::metadata(downloaded_zip.file.path()).unwrap();
                assert_eq!(metadata.len(), 119);

                println!("✅ FileDownloader integration test passed!");
            }
            Err(e) => {
                eprintln!("❌ FileDownloader integration test failed: {}", e);
            }
        }
    }
}
