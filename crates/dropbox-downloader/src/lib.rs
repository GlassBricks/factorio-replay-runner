use anyhow::Result;
use dropbox_sdk::{
    async_routes::files::{download, get_metadata},
    default_async_client::UserAuthDefaultClient,
    files::{DownloadArg, GetMetadataArg, Metadata},
    oauth2::Authorization,
};
use regex::Regex;
use serde::Deserialize;
use std::{env, path::Path};
use tokio::{fs::File, io::AsyncWriteExt};
use tokio_util::compat::FuturesAsyncReadCompatExt;

pub mod service;

const DROPBOX_TOKEN_ENV: &str = "DROPBOX_TOKEN";

#[derive(Debug, Clone, Deserialize)]
pub struct FileMetadata {
    pub id: String,
    pub name: String,
    pub size: u64,
    #[serde(rename = ".tag")]
    pub tag: String,
}

pub fn read_dropbox_token_from_env() -> Result<String> {
    env::var(DROPBOX_TOKEN_ENV)
        .map_err(|_| anyhow::anyhow!("DROPBOX_TOKEN environment variable not set"))
}

pub fn extract_path_from_url(url: &str) -> Option<String> {
    let patterns = [
        r"https://(?:www\.)?dropbox\.com/scl/fi/[^/]+/([^?]+)",
        r"https://(?:www\.)?dropbox\.com/s/[^/]+/([^?]+)",
    ];

    for pattern in &patterns {
        if let Ok(regex) = Regex::new(pattern) {
            if let Some(captures) = regex.captures(url) {
                if let Some(filename) = captures.get(1) {
                    return Some(format!("/{}", filename.as_str()));
                }
            }
        }
    }
    None
}

pub async fn get_file_metadata(file_path: &str, token: Option<String>) -> Result<FileMetadata> {
    let token = match token {
        Some(t) => t,
        None => read_dropbox_token_from_env()?,
    };

    #[expect(deprecated)]
    let auth = Authorization::from_long_lived_access_token(token);
    let client = UserAuthDefaultClient::new(auth);

    let arg = GetMetadataArg::new(file_path.to_string());

    let metadata = get_metadata(&client, &arg).await?;

    match metadata {
        Metadata::File(file_meta) => Ok(FileMetadata {
            id: file_meta.id,
            name: file_meta.name,
            size: file_meta.size,
            tag: "file".to_string(),
        }),
        Metadata::Folder(_) => {
            anyhow::bail!("Expected file but got folder at path: {}", file_path)
        }
        Metadata::Deleted(_) => {
            anyhow::bail!("File not found (deleted): {}", file_path)
        }
    }
}

pub async fn download_dropbox_file(
    file_path: &str,
    dest_path: &Path,
    token: Option<String>,
) -> Result<()> {
    let token = match token {
        Some(t) => t,
        None => read_dropbox_token_from_env()?,
    };

    let auth = Authorization::from_long_lived_access_token(token);
    let client = UserAuthDefaultClient::new(auth);

    let arg = DownloadArg::new(file_path.to_string());

    let response = download(&client, &arg, None, None).await?;

    let mut file = File::create(dest_path).await?;

    if let Some(reader) = response.body {
        let mut compat_reader = reader.compat();
        tokio::io::copy(&mut compat_reader, &mut file).await?;
        file.flush().await?;
    } else {
        anyhow::bail!("No response body received from Dropbox");
    }

    Ok(())
}
