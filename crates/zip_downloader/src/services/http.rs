use crate::DownloadError;
use crate::security::SecurityConfig;
use crate::services::{FileDownloadHandle, FileMeta, FileServiceDyn};
use anyhow::Context;
use async_trait::async_trait;
use futures::StreamExt;
use std::path::Path;
use url::Url;

fn build_client(config: &SecurityConfig) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(config.connect_timeout)
        .timeout(config.download_timeout)
        .redirect(reqwest::redirect::Policy::limited(config.max_redirects))
        .build()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpFileId(String);

impl std::fmt::Display for HttpFileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn is_domain_allowed(host: &str, allowed_domains: &[String]) -> bool {
    allowed_domains
        .iter()
        .any(|domain| host == domain || host.ends_with(&format!(".{domain}")))
}

fn find_https_url(input: &str) -> Option<Url> {
    input
        .split_whitespace()
        .filter(|word| word.starts_with("https://"))
        .find_map(|word| Url::parse(word).ok())
}

fn filename_from_content_disposition(header: &str) -> Option<&str> {
    header
        .split("filename=")
        .nth(1)
        .and_then(|s| s.split(';').next())
        .map(|s| s.trim_matches('"'))
}

fn filename_from_url(url: &Url) -> &str {
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|s| !s.is_empty())
        .unwrap_or("download.zip")
}

pub struct HttpService {
    allowed_domains: Vec<String>,
}

impl HttpService {
    pub fn new(allowed_domains: Vec<String>) -> Self {
        Self { allowed_domains }
    }

    fn detect_allowed_url(&self, input: &str) -> Option<HttpFileId> {
        let url = find_https_url(input)?;
        let host = url.host_str()?;
        is_domain_allowed(host, &self.allowed_domains).then(|| HttpFileId(url.to_string()))
    }

    async fn get_file_info(
        &self,
        file_id: &HttpFileId,
        config: &SecurityConfig,
    ) -> Result<FileMeta, DownloadError> {
        let client = build_client(config)
            .context("Failed to build HTTP client")
            .map_err(DownloadError::ServiceError)?;

        let response = client
            .head(&file_id.0)
            .send()
            .await
            .context("Failed to send HEAD request")
            .map_err(DownloadError::ServiceError)?;

        if !response.status().is_success() {
            return Err(DownloadError::FileNotAccessible(anyhow::anyhow!(
                "HTTP {} from {}",
                response.status(),
                file_id.0,
            )));
        }

        let headers = response.headers();

        let name = headers
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .and_then(filename_from_content_disposition)
            .map(str::to_string)
            .unwrap_or_else(|| {
                let url = Url::parse(&file_id.0).expect("already validated URL");
                filename_from_url(&url).to_string()
            });

        let size = headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        Ok(FileMeta { name, size })
    }

    async fn download(
        &self,
        file_id: &HttpFileId,
        dest: &Path,
        config: &SecurityConfig,
    ) -> Result<(), DownloadError> {
        use tokio::io::AsyncWriteExt;

        let client = build_client(config)
            .context("Failed to build HTTP client")
            .map_err(DownloadError::ServiceError)?;

        let response = client
            .get(&file_id.0)
            .send()
            .await
            .context("Failed to send GET request")
            .map_err(DownloadError::ServiceError)?;

        if !response.status().is_success() {
            return Err(DownloadError::FileNotAccessible(anyhow::anyhow!(
                "HTTP {} from {}",
                response.status(),
                file_id.0,
            )));
        }

        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(DownloadError::IoError)?;
        let mut stream = response.bytes_stream();
        let mut total_bytes = 0u64;

        while let Some(chunk) = stream.next().await {
            let bytes = chunk
                .context("Failed to read response stream")
                .map_err(DownloadError::ServiceError)?;
            total_bytes += bytes.len() as u64;
            if total_bytes > config.max_file_size {
                return Err(DownloadError::SecurityViolation(anyhow::anyhow!(
                    "Download exceeded maximum size of {} bytes",
                    config.max_file_size
                )));
            }
            file.write_all(&bytes)
                .await
                .map_err(DownloadError::IoError)?;
        }

        file.flush().await.map_err(DownloadError::IoError)?;
        Ok(())
    }
}

impl FileServiceDyn for HttpService {
    fn service_name(&self) -> &str {
        "http"
    }

    fn detect_link<'a>(&'a mut self, input: &str) -> Option<Box<dyn FileDownloadHandle + 'a>> {
        let file_id = self.detect_allowed_url(input)?;
        Some(Box::new(HttpFileIdWrapper {
            service: self,
            file_id,
        }))
    }
}

struct HttpFileIdWrapper<'a> {
    service: &'a mut HttpService,
    file_id: HttpFileId,
}

impl std::fmt::Display for HttpFileIdWrapper<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "http link: {}", self.file_id)
    }
}

#[async_trait]
impl FileDownloadHandle for HttpFileIdWrapper<'_> {
    async fn get_file_info(&mut self, config: &SecurityConfig) -> Result<FileMeta, DownloadError> {
        self.service.get_file_info(&self.file_id, config).await
    }

    async fn download(
        &mut self,
        dest: &Path,
        config: &SecurityConfig,
    ) -> Result<(), DownloadError> {
        self.service.download(&self.file_id, dest, config).await
    }

    fn service_name(&self) -> &str {
        "http"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(domains: &[&str]) -> HttpService {
        HttpService::new(domains.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn detect_exact_domain_match() {
        let svc = service(&["example.com"]);
        let result = svc.detect_allowed_url("https://example.com/file.zip");
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, "https://example.com/file.zip");
    }

    #[test]
    fn detect_subdomain_match() {
        let svc = service(&["example.com"]);
        assert!(
            svc.detect_allowed_url("https://cdn.example.com/file.zip")
                .is_some()
        );
        assert!(
            svc.detect_allowed_url("https://deep.cdn.example.com/file.zip")
                .is_some()
        );
    }

    #[test]
    fn reject_non_allowed_domain() {
        let svc = service(&["example.com"]);
        assert!(
            svc.detect_allowed_url("https://evil.com/file.zip")
                .is_none()
        );
    }

    #[test]
    fn reject_partial_domain_match() {
        let svc = service(&["example.com"]);
        assert!(
            svc.detect_allowed_url("https://notexample.com/file.zip")
                .is_none()
        );
    }

    #[test]
    fn reject_http_scheme() {
        let svc = service(&["example.com"]);
        assert!(
            svc.detect_allowed_url("http://example.com/file.zip")
                .is_none()
        );
    }

    #[test]
    fn detect_url_in_surrounding_text() {
        let svc = service(&["example.com"]);
        let result = svc.detect_allowed_url("check out https://example.com/file.zip here");
        assert!(result.is_some());
    }

    #[test]
    fn reject_empty_allowlist() {
        let svc = service(&[]);
        assert!(
            svc.detect_allowed_url("https://example.com/file.zip")
                .is_none()
        );
    }

    #[test]
    fn reject_non_url_input() {
        let svc = service(&["example.com"]);
        assert!(svc.detect_allowed_url("just some text").is_none());
    }

    #[test]
    fn filename_from_url_path() {
        let url = Url::parse("https://example.com/path/to/replay.zip").unwrap();
        assert_eq!(filename_from_url(&url), "replay.zip");
    }

    #[test]
    fn filename_from_url_no_path() {
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(filename_from_url(&url), "download.zip");
    }

    #[test]
    fn filename_from_disposition_header() {
        assert_eq!(
            filename_from_content_disposition("attachment; filename=\"test.zip\""),
            Some("test.zip")
        );
        assert_eq!(
            filename_from_content_disposition("attachment; filename=test.zip; other=stuff"),
            Some("test.zip")
        );
    }

    // Run m381vvdy comment contains: https://www.thuejk.dk/2026-03-07_WR_8_43_51_with_blueprints.zip
    const THUEJK_RUN_COMMENT: &str =
        "Save file: https://www.thuejk.dk/2026-03-07_WR_8_43_51_with_blueprints.zip";

    #[tokio::test]
    #[ignore]
    async fn test_thuejk_download_integration() {
        use crate::FileDownloader;

        let http_service = HttpService::new(vec!["thuejk.dk".to_string()]);
        let mut downloader = FileDownloader::builder()
            .add_service_dyn(http_service)
            .build();

        let (file, info) = downloader
            .download_zip_to_temp(THUEJK_RUN_COMMENT)
            .await
            .unwrap();

        assert_eq!(info.name, "2026-03-07_WR_8_43_51_with_blueprints.zip");
        assert!(file.path().exists());

        let metadata = std::fs::metadata(file.path()).unwrap();
        assert!(metadata.len() > 100_000);
    }
}
