use gdrive_downloader::service::GoogleDriveService;
use run_downloader::{FileDownloader, SecurityConfig};

const TEST_GOOGLE_DRIVE_URL: &str =
    "https://drive.google.com/file/d/1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC/view?usp=sharing";
const EXPECTED_FILE_NAME: &str = "foo.zip";
const EXPECTED_FILE_SIZE: u64 = 119;
const EXPECTED_CONTENT: &str = "Hello!";
const EXPECTED_MIME_TYPE: &str = "application/octet-stream";

#[tokio::test]
async fn test_run_downloader_with_google_drive_service() {
    // Create Google Drive service without authentication
    let google_drive_service = GoogleDriveService::new(None);

    // Create file downloader with Google Drive service
    let mut downloader = FileDownloader::builder()
        .add_service(google_drive_service)
        .with_security_config(SecurityConfig::new().max_file_size(1024 * 1024)) // 1MB limit
        .build();

    // Test downloading the file
    let result = downloader.download_zip(TEST_GOOGLE_DRIVE_URL).await;

    match result {
        Ok(downloaded_zip) => {
            assert_download_successful(&downloaded_zip);
            assert_file_content_correct(downloaded_zip.path());
        }
        Err(e) => panic!("Download failed: {}", e),
    }
}

#[tokio::test]
async fn test_run_downloader_with_invalid_url() {
    let google_drive_service = GoogleDriveService::new(None);

    let mut downloader = FileDownloader::builder()
        .add_service(google_drive_service)
        .build();

    // Test with invalid URL that doesn't contain Google Drive links
    let result = downloader
        .download_zip("https://example.com/not-a-drive-link")
        .await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        run_downloader::DownloadError::NoLinkFound
    ));
}

#[tokio::test]
async fn test_run_downloader_multiple_google_drive_urls() {
    let google_drive_service = GoogleDriveService::new(None);

    let mut downloader = FileDownloader::builder()
        .add_service(google_drive_service)
        .build();

    let test_urls = [
        "https://drive.google.com/file/d/1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC/view?usp=sharing",
        "https://drive.google.com/open?id=1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC",
        "Text with embedded URL: https://drive.google.com/file/d/1iqtxaPd4xAquu0uUbA9p1hCdYrXBGPRC/view more text",
    ];

    for url in test_urls {
        let result = downloader.download_zip(url).await;
        assert!(result.is_ok(), "Failed to download from URL: {}", url);

        let downloaded_zip = result.unwrap();
        assert_basic_download_info(&downloaded_zip);
    }
}

#[tokio::test]
async fn test_run_downloader_security_validation() {
    let google_drive_service = GoogleDriveService::new(None);

    // Set a very small file size limit to trigger security validation
    let security_config = SecurityConfig::new().max_file_size(100); // 100 bytes, smaller than our 119-byte test file

    let mut downloader = FileDownloader::builder()
        .add_service(google_drive_service)
        .with_security_config(security_config)
        .build();

    let result = downloader.download_zip(TEST_GOOGLE_DRIVE_URL).await;

    assert_security_error_occurred(result);
}

fn assert_download_successful(downloaded_zip: &run_downloader::DownloadedZip) {
    assert_eq!(downloaded_zip.service_name(), "google_drive");
    assert_eq!(downloaded_zip.file_info().name, EXPECTED_FILE_NAME);
    assert_eq!(downloaded_zip.file_info().size, EXPECTED_FILE_SIZE);
    assert_eq!(
        downloaded_zip.file_info().mime_type,
        Some(EXPECTED_MIME_TYPE.to_string())
    );

    assert!(downloaded_zip.path().exists());
    let metadata = std::fs::metadata(downloaded_zip.path()).unwrap();
    assert_eq!(metadata.len(), EXPECTED_FILE_SIZE);
}

fn assert_basic_download_info(downloaded_zip: &run_downloader::DownloadedZip) {
    assert_eq!(downloaded_zip.service_name(), "google_drive");
    assert_eq!(downloaded_zip.file_info().name, EXPECTED_FILE_NAME);
}

fn assert_file_content_correct(zip_path: &std::path::Path) {
    use std::io::Read;
    use zip::ZipArchive;

    let file = std::fs::File::open(zip_path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut file_in_zip = archive.by_name("foo.txt").unwrap();
    let mut contents = String::new();
    file_in_zip.read_to_string(&mut contents).unwrap();
    assert_eq!(contents.trim(), EXPECTED_CONTENT);
}

fn assert_security_error_occurred(
    result: Result<run_downloader::DownloadedZip, run_downloader::DownloadError>,
) {
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(
        error,
        run_downloader::DownloadError::SecurityError(_)
    ));
}
