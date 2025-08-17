use crate::{
    ServiceError,
    services::{FileInfo, FileService},
};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use lazy_static::lazy_static;
use regex::Regex;
use std::{fs::File, io::Write};
use tokio::io::AsyncWriteExt;
use yup_oauth2::{
    AccessToken, ServiceAccountAuthenticator, ServiceAccountKey,
    authenticator::DefaultAuthenticator,
};

const DRIVE_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/drive.readonly";
const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_PUBLIC_BASE: &str = "https://drive.google.com/uc?export=download&id";

pub async fn read_service_account_key_from_file<P: AsRef<std::path::Path>>(
    path: P,
) -> Result<ServiceAccountKey> {
    Ok(yup_oauth2::read_service_account_key(path).await?)
}

pub async fn read_service_account_key_from_env() -> Result<ServiceAccountKey> {
    read_service_account_key_from_file(std::env::var("GOOGLE_SERVICE_ACCOUNT_PATH")?).await
}

async fn build_authenticated_request(
    client: &reqwest::Client,
    url: &str,
    access_token: Option<&AccessToken>,
) -> Result<reqwest::RequestBuilder> {
    let mut request = client.get(url);

    if let Some(token) = access_token {
        request = request.bearer_auth(token.token().unwrap_or_default());
    }

    Ok(request)
}

fn api_metadata_url(file_id: &str) -> String {
    format!(
        "{}/{}?fields=id,name,size,mimeType",
        DRIVE_API_BASE, file_id
    )
}

fn api_download_url(file_id: &str) -> String {
    format!("{}/{}?alt=media", DRIVE_API_BASE, file_id)
}

fn public_download_url(file_id: &str) -> String {
    format!("{}={}", DRIVE_PUBLIC_BASE, file_id)
}

async fn get_authenticated_file_info(
    client: &reqwest::Client,
    file_id: &str,
    token: &AccessToken,
) -> Result<FileInfo> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct ApiResponse {
        name: String,
        size: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    }

    let url = api_metadata_url(file_id);
    let request = build_authenticated_request(client, &url, Some(token)).await?;
    let response = request.send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get file metadata: HTTP {}", response.status());
    }

    let api_response: ApiResponse = response.json().await?;

    Ok(FileInfo {
        name: api_response.name,
        size: api_response.size.parse().unwrap_or(0),
        is_zip: api_response.mime_type.to_lowercase().contains("zip"),
    })
}

async fn get_unauthenticated_file_info(
    client: &reqwest::Client,
    file_id: &str,
) -> Result<FileInfo> {
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

    Ok(FileInfo { name, size, is_zip })
}

async fn download_file_streaming(
    file_id: &str,
    dest: &mut File,
    token: Option<&AccessToken>,
) -> Result<()> {
    let client = reqwest::Client::new();

    let url = match token {
        Some(_) => api_download_url(file_id),
        None => public_download_url(file_id),
    };

    let request = build_authenticated_request(&client, &url, token).await?;
    let response = request.send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download file: HTTP {}", response.status());
    }

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        dest.write_all(&chunk?)?;
    }
    dest.flush()?;

    Ok(())
}

lazy_static! {
    static ref GOOGLE_DRIVE_URL_PATTERNS: [Regex; 2] = [
        Regex::new(r"https://drive\.google\.com/file/d/([a-zA-Z0-9_-]+)").unwrap(),
        Regex::new(r"https://drive\.google\.com/open\?id=([a-zA-Z0-9_-]+)").unwrap(),
    ];
}

pub struct GoogleDriveService {
    service_account_key: Option<ServiceAccountKey>,
    authenticator: Option<DefaultAuthenticator>,
}

impl GoogleDriveService {
    pub fn new(service_account_key: Option<ServiceAccountKey>) -> Self {
        Self {
            service_account_key,
            authenticator: None,
        }
    }

    pub async fn from_env() -> anyhow::Result<Self> {
        let service_account_key = read_service_account_key_from_env().await?;
        Ok(Self::new(Some(service_account_key)))
    }

    async fn get_authenticator(&mut self) -> Result<Option<&DefaultAuthenticator>, ServiceError> {
        if self.authenticator.is_none() && self.service_account_key.is_some() {
            let authenticator =
                ServiceAccountAuthenticator::builder(self.service_account_key.clone().unwrap())
                    .build()
                    .await
                    .map_err(ServiceError::retryable)?;
            self.authenticator = Some(authenticator);
        }
        Ok(self.authenticator.as_ref())
    }

    async fn get_token(&mut self) -> Result<Option<AccessToken>, ServiceError> {
        let Some(auth) = self.get_authenticator().await? else {
            return Ok(None);
        };
        auth.token(&[DRIVE_READONLY_SCOPE])
            .await
            .map(Some)
            .map_err(ServiceError::retryable)
    }

    fn classify_error(error_msg: &str) -> ServiceError {
        if error_msg.contains("403") || error_msg.contains("401") {
            ServiceError::retryable(anyhow::anyhow!("{}", error_msg))
        } else if error_msg.contains("404") {
            ServiceError::fatal(anyhow::anyhow!("{}", error_msg))
        } else {
            ServiceError::retryable(anyhow::anyhow!("{}", error_msg))
        }
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

    async fn get_file_info(&mut self, file_id: &Self::FileId) -> Result<FileInfo, ServiceError> {
        let token = self.get_token().await?;
        let client = reqwest::Client::new();

        let result = match token.as_ref() {
            Some(token) => get_authenticated_file_info(&client, file_id, token).await,
            None => get_unauthenticated_file_info(&client, file_id).await,
        };

        result.map_err(|e| Self::classify_error(&e.to_string()))
    }

    async fn download(
        &mut self,
        file_id: &Self::FileId,
        dest: &mut File,
    ) -> Result<(), ServiceError> {
        let token = self.get_token().await?;

        download_file_streaming(file_id, dest, token.as_ref())
            .await
            .map_err(|e| Self::classify_error(&e.to_string()))?;

        Ok(())
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

    async fn file_info_test(service: &mut GoogleDriveService) {
        let file_info = service
            .get_file_info(&TEST_FILE_ID.to_string())
            .await
            .unwrap();
        assert_eq!(file_info.name, "foo.zip");
        assert_eq!(file_info.size, 119);
        assert!(file_info.is_zip);
    }

    async fn download_test(service: &mut GoogleDriveService) {
        let temp_file = NamedTempFile::new().unwrap();
        service
            .download(
                &TEST_FILE_ID.to_string(),
                &mut std::fs::File::create(temp_file.path()).unwrap(),
            )
            .await
            .unwrap();

        assert!(temp_file.path().exists());
        let metadata = std::fs::metadata(temp_file.path()).unwrap();
        assert_eq!(metadata.len(), 119);
    }

    #[tokio::test]
    #[ignore]
    async fn test_service_unauthenticated() {
        let mut service = GoogleDriveService::new(None);
        file_info_test(&mut service).await;
        download_test(&mut service).await;
    }

    #[tokio::test]
    #[ignore]
    async fn test_service_authenticated() {
        dotenvy::dotenv().ok();
        let Ok(mut service) = GoogleDriveService::from_env().await else {
            if std::env::var("SKIP_SERVICE_TESTS").is_ok() {
                return;
            }
            panic!("Failed to create service");
        };
        file_info_test(&mut service).await;
        download_test(&mut service).await;
    }
}
