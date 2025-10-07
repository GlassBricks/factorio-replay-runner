use crate::{FileMeta, services::FileService};
use anyhow::Result;
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use std::{env, path::Path};
use tokio::io::AsyncReadExt;
use tokio_util::compat::FuturesAsyncReadCompatExt;

use dropbox_sdk::{
    async_routes::sharing::{get_shared_link_file, get_shared_link_metadata},
    default_async_client::UserAuthDefaultClient,
    oauth2::Authorization,
    sharing::{GetSharedLinkFileArg, SharedLinkMetadata},
};

const DROPBOX_TOKEN_ENV: &str = "DROPBOX_TOKEN";

pub async fn read_dropbox_token_from_env() -> Result<String> {
    env::var(DROPBOX_TOKEN_ENV)
        .map_err(|_| anyhow::anyhow!("DROPBOX_TOKEN environment variable not set"))
}

lazy_static! {
    static ref DROPBOX_URL_PATTERNS: [Regex; 2] = [
        Regex::new(r"https://(?:www\.)?dropbox\.com/scl/fi/[^/]+/[^?\s]+(?:\?[^\s#]*)?").unwrap(),
        Regex::new(r"https://(?:www\.)?dropbox\.com/s/[^/]+/[^?\s]+(?:\?[^\s#]*)?").unwrap(),
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropboxFileId(GetSharedLinkFileArg);
impl DropboxFileId {
    pub fn new(arg: String) -> Self {
        Self(GetSharedLinkFileArg::new(arg))
    }

    pub fn inner(&self) -> &GetSharedLinkFileArg {
        &self.0
    }
}

impl std::fmt::Display for DropboxFileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.url)
    }
}

async fn get_file_info(file_id: &DropboxFileId, token: &str) -> Result<FileMeta> {
    #[expect(deprecated)]
    let auth = Authorization::from_long_lived_access_token(token.to_string());
    let client = UserAuthDefaultClient::new(auth);

    let arg = file_id.inner();
    let metadata = get_shared_link_metadata(&client, arg).await?;

    match metadata {
        SharedLinkMetadata::File(file_meta) => {
            let is_zip = file_meta.name.to_lowercase().ends_with(".zip");
            Ok(FileMeta {
                name: file_meta.name,
                size: file_meta.size,
                is_zip,
            })
        }
        SharedLinkMetadata::Folder(_) => {
            anyhow::bail!(
                "Expected file but got folder at URL: {}",
                file_id.inner().url
            )
        }
        _ => {
            anyhow::bail!("Unexpected metadata type for URL: {}", file_id.inner().url)
        }
    }
}

async fn download_file(file_id: &DropboxFileId, dest: &Path, token: &str) -> Result<()> {
    use std::{fs::File, io::Write};

    #[expect(deprecated)]
    let auth = Authorization::from_long_lived_access_token(token.to_string());
    let client = UserAuthDefaultClient::new(auth);

    let response = get_shared_link_file(&client, file_id.inner(), None, None).await?;

    if let Some(reader) = response.body {
        let mut dest_file = File::create(dest)?;
        let mut compat_reader = reader.compat();
        let mut buffer = [0; 8192];

        loop {
            match compat_reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    dest_file.write_all(&buffer[..n])?;
                }
                Err(e) => {
                    anyhow::bail!("Read error: {}", e);
                }
            }
        }
        dest_file.flush()?;
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
            pattern
                .find(input)
                .map(|m| DropboxFileId::new(m.as_str().to_string()))
        })
    }

    async fn get_file_info(&mut self, file_id: &Self::FileId) -> Result<FileMeta> {
        let token = self.ensure_token().await?;
        get_file_info(file_id, &token).await
    }

    async fn download(&mut self, file_id: &Self::FileId, dest: &Path) -> Result<()> {
        let token = self.ensure_token().await?;
        download_file(file_id, dest, &token).await
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
        service.download(&file_id, temp_file.path()).await?;

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

        match file_info_test(&mut service, TEST_URL).await {
            Ok(_) => {
                download_test(&mut service, TEST_URL).await.unwrap();
            }
            Err(_) => {
                panic!("Test failed - likely due to expired token or missing sharing.read scope");
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_file_downloader_integration() -> anyhow::Result<()> {
        dotenvy::dotenv().ok();

        let Ok(service) = DropboxService::from_env().await else {
            eprintln!("Skipping FileDownloader integration test - DROPBOX_TOKEN not set");
            return Ok(());
        };

        let mut downloader = FileDownloader::builder().add_service(service).build();

        let (file, info) = downloader.download_zip_to_temp(TEST_URL).await?;
        assert_eq!(info.name, "foo.zip");
        assert!(file.path().exists());

        let metadata = std::fs::metadata(file.path()).unwrap();
        assert_eq!(metadata.len(), 119);
        Ok(())
    }
}
