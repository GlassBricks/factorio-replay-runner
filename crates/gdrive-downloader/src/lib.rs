use anyhow::Result;
use serde::Deserialize;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use yup_oauth2::{AccessToken, ServiceAccountAuthenticator, ServiceAccountKey};

pub mod service;

const DRIVE_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/drive.readonly";
const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_PUBLIC_BASE: &str = "https://drive.google.com/uc?export=download&id";

#[derive(Debug, Clone, Deserialize)]
pub struct FileMetadata {
    pub id: String,
    pub name: String,
    pub size: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

pub async fn read_service_account_key_from_file<P: AsRef<Path>>(
    path: P,
) -> Result<ServiceAccountKey> {
    Ok(yup_oauth2::read_service_account_key(path).await?)
}

pub fn parse_service_account_key(json: &str) -> Result<ServiceAccountKey> {
    Ok(yup_oauth2::parse_service_account_key(json)?)
}

async fn create_authenticator(service_account_key: &ServiceAccountKey) -> Result<AccessToken> {
    let auth = ServiceAccountAuthenticator::builder(service_account_key.clone())
        .build()
        .await?;

    let token = auth.token(&[DRIVE_READONLY_SCOPE]).await?;
    Ok(token)
}

async fn build_authenticated_request(
    client: &reqwest::Client,
    url: &str,
    service_account_key: Option<&ServiceAccountKey>,
) -> Result<reqwest::RequestBuilder> {
    let mut request = client.get(url);

    if let Some(key) = service_account_key {
        let token = create_authenticator(key).await?;
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

fn extract_filename_from_disposition(disposition: &str) -> Option<&str> {
    if !disposition.contains("filename=") {
        return None;
    }

    disposition
        .split("filename=")
        .nth(1)?
        .trim_matches('"')
        .into()
}

fn extract_header_value(headers: &reqwest::header::HeaderMap, key: &str) -> String {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

async fn get_authenticated_metadata(
    client: &reqwest::Client,
    file_id: &str,
    key: &ServiceAccountKey,
) -> Result<FileMetadata> {
    let url = api_metadata_url(file_id);
    let request = build_authenticated_request(client, &url, Some(key)).await?;
    let response = request.send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get file metadata: HTTP {}", response.status());
    }

    Ok(response.json().await?)
}

async fn get_unauthenticated_metadata(
    client: &reqwest::Client,
    file_id: &str,
) -> Result<FileMetadata> {
    let url = public_download_url(file_id);
    let response = client.head(&url).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get file metadata: HTTP {}", response.status());
    }

    let headers = response.headers();
    let name = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(extract_filename_from_disposition)
        .unwrap_or("unknown")
        .to_string();

    let size = extract_header_value(headers, "content-length");
    let mime_type = extract_header_value(headers, "content-type");
    let mime_type = if mime_type.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime_type
    };

    Ok(FileMetadata {
        id: file_id.to_string(),
        name,
        size: if size.is_empty() {
            "0".to_string()
        } else {
            size
        },
        mime_type,
    })
}

pub async fn get_file_metadata(
    file_id: &str,
    service_account_key: Option<ServiceAccountKey>,
) -> Result<FileMetadata> {
    let client = reqwest::Client::new();

    match service_account_key {
        Some(ref key) => get_authenticated_metadata(&client, file_id, key).await,
        None => get_unauthenticated_metadata(&client, file_id).await,
    }
}

async fn build_download_request(
    client: &reqwest::Client,
    file_id: &str,
    service_account_key: Option<&ServiceAccountKey>,
) -> Result<reqwest::RequestBuilder> {
    let url = match service_account_key {
        Some(_) => api_download_url(file_id),
        None => public_download_url(file_id),
    };

    build_authenticated_request(client, &url, service_account_key).await
}

async fn download_response_to_file(
    response: reqwest::Response,
    output_path: &Path,
) -> Result<usize> {
    if !response.status().is_success() {
        anyhow::bail!("Failed to download file: HTTP {}", response.status());
    }

    let bytes = response.bytes().await?;
    let byte_count = bytes.len();

    let mut file = File::create(output_path)?;
    file.write_all(&bytes)?;

    Ok(byte_count)
}

pub async fn download_gdrive_file(
    file_id: &str,
    output_path: &Path,
    service_account_key: Option<ServiceAccountKey>,
) -> Result<usize> {
    let client = reqwest::Client::new();
    let request = build_download_request(&client, file_id, service_account_key.as_ref()).await?;
    let response = request.send().await?;

    download_response_to_file(response, output_path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_FILE_ID: &str = "1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC";
    const EXPECTED_FILE_SIZE: usize = 119;

    fn expected_metadata() -> FileMetadata {
        FileMetadata {
            id: TEST_FILE_ID.to_string(),
            name: "foo.zip".to_string(),
            size: EXPECTED_FILE_SIZE.to_string(),
            mime_type: "application/octet-stream".to_string(),
        }
    }

    #[tokio::test]
    async fn test_get_file_metadata_unauthenticated() {
        let metadata = get_file_metadata(TEST_FILE_ID, None).await.unwrap();
        let expected = expected_metadata();

        assert_eq!(metadata.id, expected.id);
        assert_eq!(metadata.name, expected.name);
        assert_eq!(metadata.size, expected.size);
        assert_eq!(metadata.mime_type, expected.mime_type);
    }

    #[tokio::test]
    async fn test_download_gdrive_file_unauthenticated() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let byte_count = download_gdrive_file(TEST_FILE_ID, temp_file.path(), None)
            .await
            .unwrap();

        assert_eq!(byte_count, EXPECTED_FILE_SIZE);
        assert_file_exists_with_size(temp_file.path(), EXPECTED_FILE_SIZE);
    }

    #[tokio::test]
    async fn test_download_gdrive_file_content() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let byte_count = download_gdrive_file(TEST_FILE_ID, temp_file.path(), None)
            .await
            .unwrap();

        let zip_content = extract_zip_content(temp_file.path(), "foo.txt");

        assert_eq!(byte_count, EXPECTED_FILE_SIZE);
        assert_eq!(zip_content.trim(), "Hello!");
    }

    fn assert_file_exists_with_size(path: &Path, expected_size: usize) {
        assert!(path.exists());
        let metadata = std::fs::metadata(path).unwrap();
        assert_eq!(metadata.len() as usize, expected_size);
    }

    fn extract_zip_content(zip_path: &Path, entry_name: &str) -> String {
        use std::io::Read;
        use zip::ZipArchive;

        let file = std::fs::File::open(zip_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut file_in_zip = archive.by_name(entry_name).unwrap();
        let mut contents = String::new();
        file_in_zip.read_to_string(&mut contents).unwrap();
        contents
    }
}
