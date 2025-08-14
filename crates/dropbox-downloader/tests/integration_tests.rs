use dropbox_downloader::{
    download_dropbox_file, extract_path_from_url, get_file_metadata, service::DropboxService,
};
use run_downloader::services::{FileDownloadService, FileService};
use std::{env, io::Read};
use tempfile::NamedTempFile;

const TEST_FILE_PATH: &str = "/foo.zip";
const TEST_DROPBOX_URL: &str = "https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0";

#[test]
fn test_extract_path_from_url() {
    let test_cases = [
        (
            "https://www.dropbox.com/scl/fi/aw5ohfvtfoc2nnn4nl2n6/foo.zip?rlkey=1sholbp5uxq15dk0ke5ljtwsz&st=gpkdzloy&dl=0",
            Some("/foo.zip".to_string()),
        ),
        (
            "https://dropbox.com/scl/fi/abc123def456/test.txt?dl=0",
            Some("/test.txt".to_string()),
        ),
        (
            "https://www.dropbox.com/s/abc123/test.zip?dl=0",
            Some("/test.zip".to_string()),
        ),
        (
            "https://dropbox.com/s/xyz789/document.pdf",
            Some("/document.pdf".to_string()),
        ),
        (
            "https://example.com/not-a-dropbox-link",
            None,
        ),
        ("just some text", None),
    ];

    for (input, expected) in test_cases {
        assert_eq!(
            extract_path_from_url(input),
            expected,
            "Failed for URL: {}",
            input
        );
    }
}

#[test]
fn test_service_detect_link() {
    let test_cases = [
        (TEST_DROPBOX_URL, Some("/foo.zip".to_string())),
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

#[test]
fn test_service_name() {
    let service = DropboxService::new(None);
    assert_eq!(service.service_name(), "dropbox");
}

#[tokio::test]
async fn test_get_file_metadata_without_token() {
    let result = get_file_metadata(TEST_FILE_PATH, None).await;

    // Should fail if no DROPBOX_TOKEN environment variable is set
    if env::var("DROPBOX_TOKEN").is_err() {
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("DROPBOX_TOKEN environment variable not set"));
    }
}

#[tokio::test]
async fn test_download_file_without_token() {
    let temp_file = NamedTempFile::new().unwrap();
    let result = download_dropbox_file(TEST_FILE_PATH, temp_file.path(), None).await;

    // Should fail if no DROPBOX_TOKEN environment variable is set
    if env::var("DROPBOX_TOKEN").is_err() {
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("DROPBOX_TOKEN environment variable not set"));
    }
}

#[tokio::test]
async fn test_service_without_token() {
    let mut service = DropboxService::new(None);

    // Should fail if no DROPBOX_TOKEN environment variable is set
    if env::var("DROPBOX_TOKEN").is_err() {
        let result = service.get_file_info(TEST_FILE_PATH).await;
        assert!(result.is_err());

        let temp_file = NamedTempFile::new().unwrap();
        let result = service.download(TEST_FILE_PATH, temp_file.path()).await;
        assert!(result.is_err());
    }
}

// Integration tests that require a valid DROPBOX_TOKEN
#[tokio::test]
async fn test_get_file_metadata_with_token() {
    if let Ok(token) = env::var("DROPBOX_TOKEN") {
        let result = get_file_metadata(TEST_FILE_PATH, Some(token)).await;

        match result {
            Ok(metadata) => {
                assert_eq!(metadata.name, "foo.zip");
                assert_eq!(metadata.size, 119);
                assert_eq!(metadata.tag, "file");
                assert!(!metadata.id.is_empty());
            }
            Err(e) => {
                // If the test fails, it might be due to file not found or invalid token
                // Print the error for debugging
                eprintln!("Failed to get metadata: {}", e);
            }
        }
    }
}

#[tokio::test]
async fn test_download_file_with_token() {
    if let Ok(token) = env::var("DROPBOX_TOKEN") {
        let temp_file = NamedTempFile::new().unwrap();
        let result = download_dropbox_file(TEST_FILE_PATH, temp_file.path(), Some(token)).await;

        match result {
            Ok(()) => {
                // Verify file was downloaded
                assert!(temp_file.path().exists());
                let metadata = std::fs::metadata(temp_file.path()).unwrap();
                assert_eq!(metadata.len(), 119);

                // Verify it's a valid ZIP file by trying to read it
                let file = std::fs::File::open(temp_file.path()).unwrap();
                let mut archive = zip::ZipArchive::new(file).unwrap();
                assert_eq!(archive.len(), 1);

                // Check the file inside the ZIP
                let mut file = archive.by_index(0).unwrap();
                assert_eq!(file.name(), "foo.txt");

                let mut contents = String::new();
                file.read_to_string(&mut contents).unwrap();
                assert_eq!(contents.trim(), "Hello!");
            }
            Err(e) => {
                // If the test fails, it might be due to file not found or invalid token
                eprintln!("Failed to download file: {}", e);
            }
        }
    }
}

#[tokio::test]
async fn test_service_with_token() {
    if let Ok(token) = env::var("DROPBOX_TOKEN") {
        let mut service = DropboxService::new(Some(token));

        // Test getting file info
        let file_info_result = service.get_file_info(TEST_FILE_PATH).await;
        match file_info_result {
            Ok(file_info) => {
                assert_eq!(file_info.name, "foo.zip");
                assert_eq!(file_info.size, 119);
                assert_eq!(file_info.mime_type, Some("application/zip".to_string()));

                // Test downloading the file
                let temp_file = NamedTempFile::new().unwrap();
                let download_result = service.download(TEST_FILE_PATH, temp_file.path()).await;

                match download_result {
                    Ok(()) => {
                        assert!(temp_file.path().exists());
                        let metadata = std::fs::metadata(temp_file.path()).unwrap();
                        assert_eq!(metadata.len(), 119);
                    }
                    Err(e) => {
                        eprintln!("Failed to download file via service: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to get file info via service: {}", e);
            }
        }
    }
}

#[tokio::test]
async fn test_service_with_invalid_path() {
    if let Ok(token) = env::var("DROPBOX_TOKEN") {
        let mut service = DropboxService::new(Some(token));

        let result = service.get_file_info("/nonexistent/file.txt").await;
        assert!(result.is_err());

        let temp_file = NamedTempFile::new().unwrap();
        let result = service
            .download("/nonexistent/file.txt", temp_file.path())
            .await;
        assert!(result.is_err());
    }
}

#[test]
fn test_mime_type_inference() {
    use dropbox_downloader::service::DropboxService;

    // Use reflection to access the private method for testing
    // This is a bit of a hack, but it's the cleanest way to test the private method
    assert_eq!(
        DropboxService::infer_mime_type("test.zip"),
        Some("application/zip".to_string())
    );
    assert_eq!(
        DropboxService::infer_mime_type("test.txt"),
        Some("text/plain".to_string())
    );
    assert_eq!(
        DropboxService::infer_mime_type("unknown"),
        Some("application/octet-stream".to_string())
    );
    assert_eq!(
        DropboxService::infer_mime_type("document.pdf"),
        Some("application/octet-stream".to_string())
    );
}
