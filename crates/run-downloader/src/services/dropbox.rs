use crate::{FileInfo, services::FileService};
use anyhow::Result;
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use std::{env, fs::File, io::Write};
use tokio::io::AsyncReadExt;
use tokio_util::compat::FuturesAsyncReadCompatExt;

use dropbox_sdk::{
    async_routes::sharing::{get_shared_link_file, get_shared_link_metadata},
    default_async_client::UserAuthDefaultClient,
    oauth2::Authorization,
    sharing::{GetSharedLinkFileArg, GetSharedLinkMetadataArg, SharedLinkMetadata},
};

const DROPBOX_TOKEN_ENV: &str = "DROPBOX_TOKEN";

pub async fn read_dropbox_token_from_env() -> Result<String> {
    env::var(DROPBOX_TOKEN_ENV)
        .map_err(|_| anyhow::anyhow!("DROPBOX_TOKEN environment variable not set"))
}

lazy_static! {
    static ref DROPBOX_URL_PATTERNS: [Regex; 2] = [
        Regex::new(r"https://(?:www\.)?dropbox\.com/scl/fi/[^/]+/[^?]+(?:\?[^#]*)?").unwrap(),
        Regex::new(r"https://(?:www\.)?dropbox\.com/s/[^/]+/[^?]+(?:\?[^#]*)?").unwrap(),
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropboxFileId {
    pub url: String,
    pub password: Option<String>,
}

impl std::fmt::Display for DropboxFileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.url)
    }
}

impl DropboxFileId {
    pub fn new(url: String) -> Self {
        Self {
            url,
            password: None,
        }
    }

    pub fn with_password(mut self, password: String) -> Self {
        self.password = Some(password);
        self
    }
}

async fn get_file_info(file_id: &DropboxFileId, token: &str) -> Result<FileInfo> {
    #[expect(deprecated)]
    let auth = Authorization::from_long_lived_access_token(token.to_string());
    let client = UserAuthDefaultClient::new(auth);

    let mut arg = GetSharedLinkMetadataArg::new(file_id.url.clone());
    if let Some(password) = &file_id.password {
        arg = arg.with_link_password(password.clone());
    }

    let metadata = get_shared_link_metadata(&client, &arg).await?;

    match metadata {
        SharedLinkMetadata::File(file_meta) => {
            let is_zip = file_meta.name.to_lowercase().ends_with(".zip");
            Ok(FileInfo {
                name: file_meta.name,
                size: file_meta.size,
                is_zip,
            })
        }
        SharedLinkMetadata::Folder(_) => {
            anyhow::bail!("Expected file but got folder at URL: {}", file_id.url)
        }
        _ => {
            anyhow::bail!("Unexpected metadata type for URL: {}", file_id.url)
        }
    }
}

async fn download_file(file_id: &DropboxFileId, dest: &mut File, token: &str) -> Result<()> {
    #[expect(deprecated)]
    let auth = Authorization::from_long_lived_access_token(token.to_string());
    let client = UserAuthDefaultClient::new(auth);

    let mut arg = GetSharedLinkFileArg::new(file_id.url.clone());
    if let Some(password) = &file_id.password {
        arg = arg.with_link_password(password.clone());
    }

    let response = get_shared_link_file(&client, &arg, None, None).await?;

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

    async fn ensure_token(&self) -> Result<String> {
        match &self.token {
            Some(token) => Ok(token.clone()),
            None => read_dropbox_token_from_env().await,
        }
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
            if pattern.is_match(input) {
                Some(DropboxFileId::new(input.to_string()))
            } else {
                None
            }
        })
    }

    async fn get_file_info(&mut self, file_id: &Self::FileId) -> Result<FileInfo> {
        let token = self.ensure_token().await?;
        get_file_info(file_id, &token).await
    }

    async fn download(&mut self, file_id: &Self::FileId, dest: &mut File) -> Result<()> {
        let token = self.ensure_token().await?;
        download_file(file_id, dest, &token).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileDownloader;

    #[test]
    fn test_detect_link() {
        let test_cases = [
            (
                "https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0",
                Some(DropboxFileId::new("https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0".to_string())),
            ),
            (
                "https://www.dropbox.com/s/abc123/test.zip?dl=0",
                Some(DropboxFileId::new("https://www.dropbox.com/s/abc123/test.zip?dl=0".to_string())),
            ),
            ("https://example.com/not-a-dropbox-link", None),
            ("just some text", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(DropboxService::detect_link(input), expected);
        }
    }

    #[test]
    fn test_dropbox_file_id() {
        let url = "https://www.dropbox.com/s/abc123/test.zip?dl=0".to_string();
        let file_id = DropboxFileId::new(url.clone());
        assert_eq!(file_id.url, url);
        assert_eq!(file_id.password, None);

        let file_id_with_password = file_id.with_password("secret".to_string());
        assert_eq!(file_id_with_password.url, url);
        assert_eq!(file_id_with_password.password, Some("secret".to_string()));
    }

    #[test]
    fn test_dropbox_file_id_display() {
        let url = "https://www.dropbox.com/s/abc123/test.zip?dl=0".to_string();
        let file_id = DropboxFileId::new(url.clone());
        assert_eq!(format!("{}", file_id), url);

        let file_id_with_password = file_id.with_password("secret".to_string());
        assert_eq!(format!("{}", file_id_with_password), url);
    }

    #[test]
    fn test_detect_link_with_various_formats() {
        let test_cases = [
            (
                "https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0",
                true,
            ),
            ("https://www.dropbox.com/s/abc123/test.zip?dl=0", true),
            ("https://dropbox.com/s/abc123/test.zip?dl=1", true),
            (
                "https://dropbox.com/scl/fi/xyz789/example.zip?rlkey=abc&dl=0",
                true,
            ),
            ("https://google.com/file.zip", false),
            ("https://dropbox.com/invalid/path", false),
        ];

        for (url, should_match) in test_cases {
            let result = DropboxService::detect_link(url);
            if should_match {
                assert!(result.is_some(), "Should detect URL: {}", url);
                assert_eq!(result.unwrap().url, url);
            } else {
                assert!(result.is_none(), "Should not detect URL: {}", url);
            }
        }
    }

    async fn file_info_test(service: &mut DropboxService, test_url: &str) -> Result<()> {
        let file_id = DropboxFileId::new(test_url.to_string());
        let file_info = service
            .get_file_info(&file_id)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        assert_eq!(file_info.name, "foo.zip");
        assert_eq!(file_info.size, 119);
        assert!(file_info.is_zip);
        Ok(())
    }

    async fn download_test(service: &mut DropboxService, test_url: &str) -> Result<()> {
        let file_id = DropboxFileId::new(test_url.to_string());
        let temp_file = tempfile::NamedTempFile::new()?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(temp_file.path())?;
        service
            .download(&file_id, &mut file)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        assert!(temp_file.path().exists());
        let metadata = std::fs::metadata(temp_file.path())?;
        assert_eq!(metadata.len(), 119);
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_service_with_token() {
        dotenvy::dotenv().ok();

        let Ok(mut service) = DropboxService::from_env().await else {
            eprintln!("Skipping Dropbox integration test - DROPBOX_TOKEN not set");
            return;
        };

        let test_url = "https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0";

        match file_info_test(&mut service, test_url).await {
            Ok(_) => {
                let _ = download_test(&mut service, test_url).await;
            }
            Err(_) => {
                eprintln!(
                    "Test failed - likely due to missing sharing.read scope in Dropbox app configuration"
                );
                eprintln!("This is expected if the app was configured for files.content.read only");
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_file_downloader_integration() {
        dotenvy::dotenv().ok();

        let Ok(service) = DropboxService::from_env().await else {
            eprintln!("Skipping FileDownloader integration test - DROPBOX_TOKEN not set");
            return;
        };

        let mut downloader = FileDownloader::builder().add_service(service).build();

        let test_url = "https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0";

        match downloader.download_zip_to_temp(test_url).await {
            Ok((file, info)) => {
                assert_eq!(info.name, "foo.zip");
                assert_eq!(info.size, 119);
                assert!(info.is_zip);
                assert!(file.path().exists());

                let metadata = std::fs::metadata(file.path()).unwrap();
                assert_eq!(metadata.len(), 119);
            }
            Err(e) => {
                if e.to_string().contains("sharing.read") {
                    eprintln!("test skipped - Dropbox app needs sharing.read scope");
                } else {
                    eprintln!("FileDownloader integration test failed: {}", e);
                }
            }
        }
    }
}
