//! Basic usage example for the run-downloader crate
//!
//! This example demonstrates how to set up and use the FileDownloader
//! to download files from Google Drive.

use run_downloader::{FileDownloader, GoogleDriveService, SecurityConfig};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_downloader::logging::init_logging();

    let security_config = SecurityConfig::new()
        .max_file_size(200 * 1024 * 1024) // 200 MB max
        .download_timeout(Duration::from_secs(300)) // 5 minute timeout
        .max_extracted_size(1024 * 1024 * 1024) // 1 GB max extracted
        .max_zip_entries(2000) // Allow up to 2000 files in ZIP
        .allowed_extensions(vec![".zip".to_string(), ".ZIP".to_string()]);

    // Create services with credentials
    // In practice, these would come from environment variables or config files
    // Google Drive now requires OAuth2 credentials instead of just an API key
    let google_drive_service = GoogleDriveService::new(google_drive::Client::new(
        std::env::var("GOOGLE_DRIVE_CLIENT_ID")
            .unwrap_or_else(|_| "your_client_id_here".to_string()),
        std::env::var("GOOGLE_DRIVE_CLIENT_SECRET")
            .unwrap_or_else(|_| "your_client_secret_here".to_string()),
        std::env::var("GOOGLE_DRIVE_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:8080/callback".to_string()),
        std::env::var("GOOGLE_DRIVE_TOKEN")
            .unwrap_or_else(|_| "your_access_token_here".to_string()),
        std::env::var("GOOGLE_DRIVE_REFRESH_TOKEN")
            .unwrap_or_else(|_| "your_refresh_token_here".to_string()),
    ));

    let downloader = FileDownloader::builder()
        .with_security_config(security_config)
        .add_service(google_drive_service)
        .build();

    println!(
        "FileDownloader configured with {} services",
        downloader.service_count()
    );

    // Example input text containing file sharing links
    let input_text = r#"
        Check out this awesome Factorio replay:
        https://drive.google.com/file/d/1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms/view?usp=sharing
    "#;

    // Attempt to download
    match downloader.download_run(input_text).await {
        Ok(downloaded_run) => {
            println!("✅ Successfully downloaded file!");
            println!("  📁 Path: {:?}", downloaded_run.path());
            println!("  📊 File info: {:?}", downloaded_run.file_info());
            println!("  🔧 Service used: {}", downloaded_run.service_name());

            // The file is now available at downloaded_run.path()
            // You can process it further (e.g., extract ZIP contents)

            // Clean up the temporary file when done
            if let Err(e) = std::fs::remove_file(downloaded_run.path()) {
                eprintln!("⚠️  Warning: Failed to clean up temp file: {}", e);
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to download file: {}", e);
            match e {
                run_downloader::DownloadError::NoLinkFound => {
                    eprintln!("   💡 Make sure the input contains valid Google Drive links");
                }
                run_downloader::DownloadError::SecurityError(reason) => {
                    eprintln!("   🔒 Security validation failed: {}", reason);
                }
                run_downloader::DownloadError::ServiceError(service_err) => {
                    if service_err.is_retryable() {
                        eprintln!("   🔄 Retryable service error: {}", service_err);
                    } else {
                        eprintln!("   💀 Fatal service error: {}", service_err);
                    }
                }
                run_downloader::DownloadError::Other(e) => {
                    eprintln!("   ❓ Other error: {}", e);
                }
            }
        }
    }

    Ok(())
}
