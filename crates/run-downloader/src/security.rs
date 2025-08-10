use anyhow::{Result, anyhow};
use std::path::Path;
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub max_file_size: u64,
    pub download_timeout: Duration,
    pub max_extracted_size: u64,
    pub max_zip_entries: usize,
    pub allowed_extensions: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            max_file_size: 100 * 1024 * 1024,           // 100 MB
            download_timeout: Duration::from_secs(120), // 2 minutes
            max_extracted_size: 500 * 1024 * 1024,      // 500 MB
            max_zip_entries: 1000,
            allowed_extensions: vec![".zip".to_string()],
        }
    }
}

impl SecurityConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
        self
    }

    pub fn download_timeout(mut self, timeout: Duration) -> Self {
        self.download_timeout = timeout;
        self
    }

    pub fn max_extracted_size(mut self, size: u64) -> Self {
        self.max_extracted_size = size;
        self
    }

    pub fn max_zip_entries(mut self, entries: usize) -> Self {
        self.max_zip_entries = entries;
        self
    }

    pub fn allowed_extensions(mut self, extensions: Vec<String>) -> Self {
        self.allowed_extensions = extensions;
        self
    }
}

pub fn validate_file_size(size: u64, config: &SecurityConfig) -> Result<()> {
    if size > config.max_file_size {
        warn!(
            "File size {} exceeds maximum allowed {}",
            size, config.max_file_size
        );
        return Err(anyhow!(
            "File size {} exceeds maximum allowed {} bytes",
            size,
            config.max_file_size
        ));
    }
    debug!("File size {} is within limits", size);
    Ok(())
}

/// Validate file extension against allowed extensions
pub fn validate_file_extension(filename: &str, config: &SecurityConfig) -> Result<()> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{}", ext.to_lowercase()));

    if let Some(ext) = extension {
        if config.allowed_extensions.contains(&ext) {
            debug!("File extension {} is allowed", ext);
            return Ok(());
        }
    }

    warn!("File extension not allowed for file: {}", filename);
    Err(anyhow!(
        "File extension not allowed for file: {}. Allowed extensions: {:?}",
        filename,
        config.allowed_extensions
    ))
}

pub fn validate_url_is_https(url: &str) -> Result<()> {
    if !url.starts_with("https://") {
        warn!("Non-HTTPS URL rejected: {}", url);
        return Err(anyhow!("Only HTTPS URLs are allowed"));
    }
    debug!("URL security validated: {}", url);
    Ok(())
}

pub fn validate_content_type(content_type: Option<&str>) -> Result<()> {
    let allowed_types = [
        "application/zip",
        "application/x-zip-compressed",
        "application/octet-stream",
    ];

    match content_type {
        Some(ct) => {
            let content_type_lower = ct.to_lowercase();
            if allowed_types
                .iter()
                .any(|&allowed| content_type_lower.contains(allowed))
            {
                debug!("Content type {} is allowed", ct);
                Ok(())
            } else {
                warn!("Invalid content type: {}", ct);
                Err(anyhow!("Invalid content type: {}", ct))
            }
        }
        None => {
            warn!("No content type provided");
            Err(anyhow!("No content type provided"))
        }
    }
}

/// Validate ZIP file structure
pub fn validate_zip_file(file_path: &Path, config: &SecurityConfig) -> Result<()> {
    use std::fs::File;
    use zip::ZipArchive;

    let file = File::open(file_path).map_err(|e| anyhow!("IO error: {}", e))?;
    let mut archive = ZipArchive::new(file).map_err(|e| anyhow!("ZIP archive error: {}", e))?;

    debug!("Validating ZIP file with {} entries", archive.len());

    // Check number of entries
    if archive.len() > config.max_zip_entries {
        warn!(
            "ZIP file has {} entries, exceeds maximum {}",
            archive.len(),
            config.max_zip_entries
        );
        return Err(anyhow!(
            "ZIP file has {} entries, exceeds maximum {}",
            archive.len(),
            config.max_zip_entries
        ));
    }

    let mut total_uncompressed_size = 0u64;

    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| anyhow!("ZIP archive error: {}", e))?;

        validate_zip_entry_path(entry.name())?;

        total_uncompressed_size += entry.size();

        // Check if individual file is too large
        if entry.size() > config.max_extracted_size {
            warn!(
                "ZIP entry '{}' size {} exceeds maximum {}",
                entry.name(),
                entry.size(),
                config.max_extracted_size
            );
            return Err(anyhow!(
                "ZIP entry '{}' size {} exceeds maximum {}",
                entry.name(),
                entry.size(),
                config.max_extracted_size
            ));
        }
    }

    if total_uncompressed_size > config.max_extracted_size {
        warn!(
            "Total uncompressed size {} exceeds maximum {}",
            total_uncompressed_size, config.max_extracted_size
        );
        return Err(anyhow!(
            "Total uncompressed size {} exceeds maximum {}",
            total_uncompressed_size,
            config.max_extracted_size
        ));
    }

    debug!("ZIP file validation passed");
    Ok(())
}

/// Validate ZIP entry path for path traversal attacks
fn validate_zip_entry_path(path: &str) -> Result<()> {
    if path.contains("..") {
        warn!("Path traversal attempt detected: {}", path);
        return Err(anyhow!("Path traversal attempt detected: {}", path));
    }

    if path.starts_with('/') || path.starts_with('\\') {
        warn!("Absolute path detected: {}", path);
        return Err(anyhow!("Absolute path detected: {}", path));
    }

    if path.len() >= 2 && path.chars().nth(1) == Some(':') {
        warn!("Windows drive path detected: {}", path);
        return Err(anyhow!("Windows drive path detected: {}", path));
    }

    debug!("ZIP entry path '{}' is safe", path);
    Ok(())
}

pub fn validate_zip_magic_number(file_path: &Path) -> Result<()> {
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(file_path).map_err(|e| anyhow!("IO error: {}", e))?;
    let mut buffer = [0u8; 4];
    file.read_exact(&mut buffer)
        .map_err(|e| anyhow!("IO error: {}", e))?;

    // ZIP file magic numbers
    const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04]; // "PK\x03\x04"
    const ZIP_EMPTY_MAGIC: [u8; 4] = [0x50, 0x4B, 0x05, 0x06]; // "PK\x05\x06"
    const ZIP_SPANNED_MAGIC: [u8; 4] = [0x50, 0x4B, 0x07, 0x08]; // "PK\x07\x08"

    if buffer == ZIP_MAGIC || buffer == ZIP_EMPTY_MAGIC || buffer == ZIP_SPANNED_MAGIC {
        debug!("ZIP magic number validated");
        Ok(())
    } else {
        warn!("Invalid ZIP magic number: {:?}", buffer);
        Err(anyhow!("File is not a valid ZIP file"))
    }
}

pub fn validate_file_info(
    file_info: &crate::services::FileInfo,
    config: &SecurityConfig,
) -> Result<()> {
    let _span = crate::logging::security_validation_span("file_info");

    validate_file_size(file_info.size, config)?;

    validate_file_extension(&file_info.name, config)?;

    debug!("File info validation passed for: {}", file_info.name);
    Ok(())
}

pub fn validate_downloaded_file(
    file_path: &Path,
    file_info: &crate::services::FileInfo,
    config: &SecurityConfig,
) -> Result<()> {
    let _span = crate::logging::security_validation_span("downloaded_file");

    validate_zip_magic_number(file_path)?;

    validate_zip_file(file_path, config)?;

    let actual_size = std::fs::metadata(file_path)
        .map_err(|e| anyhow!("IO error: {}", e))?
        .len();
    if actual_size != file_info.size {
        warn!(
            "File size mismatch: expected {}, got {}",
            file_info.size, actual_size
        );
        return Err(anyhow!(
            "File size mismatch: expected {}, got {}",
            file_info.size,
            actual_size
        ));
    }

    debug!("Downloaded file validation passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_security_config_builder() {
        let config = SecurityConfig::new()
            .max_file_size(50 * 1024 * 1024)
            .download_timeout(Duration::from_secs(60))
            .max_zip_entries(500);

        assert_eq!(config.max_file_size, 50 * 1024 * 1024);
        assert_eq!(config.download_timeout, Duration::from_secs(60));
        assert_eq!(config.max_zip_entries, 500);
    }

    #[test]
    fn test_validate_file_size() {
        let config = SecurityConfig::default();

        assert!(validate_file_size(1000, &config).is_ok());
        assert!(validate_file_size(config.max_file_size + 1, &config).is_err());
    }

    #[test]
    fn test_validate_file_extension() {
        let config = SecurityConfig::default();

        assert!(validate_file_extension("test.zip", &config).is_ok());
        assert!(validate_file_extension("test.ZIP", &config).is_ok());
        assert!(validate_file_extension("test.txt", &config).is_err());
        assert!(validate_file_extension("test", &config).is_err());
    }

    #[test]
    fn test_validate_url_security() {
        assert!(validate_url_is_https("https://example.com").is_ok());
        assert!(validate_url_is_https("http://example.com").is_err());
        assert!(validate_url_is_https("ftp://example.com").is_err());
    }

    #[test]
    fn test_validate_content_type() {
        assert!(validate_content_type(Some("application/zip")).is_ok());
        assert!(validate_content_type(Some("application/x-zip-compressed")).is_ok());
        assert!(validate_content_type(Some("application/octet-stream")).is_ok());
        assert!(validate_content_type(Some("text/plain")).is_err());
        assert!(validate_content_type(None).is_err());
    }

    #[test]
    fn test_validate_zip_entry_path() {
        assert!(validate_zip_entry_path("normal/path/file.txt").is_ok());
        assert!(validate_zip_entry_path("../../../etc/passwd").is_err());
        assert!(validate_zip_entry_path("/absolute/path").is_err());
        assert!(validate_zip_entry_path("\\windows\\path").is_err());
        assert!(validate_zip_entry_path("C:\\windows\\path").is_err());
    }

    #[test]
    fn test_create_test_zip() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use zip::write::{FileOptions, ZipWriter};

        // Create a test ZIP file
        let temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.reopen().unwrap());

        // Add a small file
        zip.start_file("test.txt", FileOptions::<()>::default())
            .unwrap();
        zip.write_all(b"Hello, world!").unwrap();
        zip.finish().unwrap();

        // Test with default config - should pass
        let config = SecurityConfig::default();
        assert!(validate_zip_file(temp_file.path(), &config).is_ok());
    }

    #[test]
    fn test_zip_validation_too_many_entries() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use zip::write::{FileOptions, ZipWriter};

        // Create a test ZIP file with multiple entries
        let temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.reopen().unwrap());

        // Add 5 files
        for i in 0..5 {
            zip.start_file(&format!("file{}.txt", i), FileOptions::<()>::default())
                .unwrap();
            zip.write_all(b"test content").unwrap();
        }
        zip.finish().unwrap();

        // Test with very low limit - should fail
        let config = SecurityConfig::new().max_zip_entries(3);
        let result = validate_zip_file(temp_file.path(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("ZIP file has 5 entries, exceeds maximum 3")
        );
    }

    #[test]
    fn test_zip_validation_file_too_large() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use zip::write::{FileOptions, ZipWriter};

        // Create a test ZIP file with a larger file
        let temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.reopen().unwrap());

        zip.start_file("large.txt", FileOptions::<()>::default())
            .unwrap();
        let large_content = vec![b'A'; 2000]; // 2KB file
        zip.write_all(&large_content).unwrap();
        zip.finish().unwrap();

        // Test with very low size limit - should fail
        let config = SecurityConfig::new().max_extracted_size(1000); // 1KB limit
        let result = validate_zip_file(temp_file.path(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("size 2000 exceeds maximum 1000"));
    }

    #[test]
    fn test_zip_validation_total_size_exceeded() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use zip::write::{FileOptions, ZipWriter};

        // Create a test ZIP file with multiple files
        let temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.reopen().unwrap());

        // Add 3 files of 800 bytes each = 2400 bytes total
        for i in 0..3 {
            zip.start_file(&format!("file{}.txt", i), FileOptions::<()>::default())
                .unwrap();
            let content = vec![b'A'; 800];
            zip.write_all(&content).unwrap();
        }
        zip.finish().unwrap();

        // Test with total size limit of 2000 bytes - should fail
        let config = SecurityConfig::new().max_extracted_size(2000);
        let result = validate_zip_file(temp_file.path(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Total uncompressed size 2400 exceeds maximum 2000")
        );
    }

    #[test]
    fn test_zip_validation_path_traversal() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use zip::write::{FileOptions, ZipWriter};

        // Create a test ZIP file with path traversal
        let temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.reopen().unwrap());

        zip.start_file("../../../etc/passwd", FileOptions::<()>::default())
            .unwrap();
        zip.write_all(b"malicious content").unwrap();
        zip.finish().unwrap();

        // Should fail due to path traversal
        let config = SecurityConfig::default();
        let result = validate_zip_file(temp_file.path(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Path traversal attempt detected"));
    }

    #[test]
    fn test_security_error_types() {
        use crate::services::FileInfo;

        let config = SecurityConfig::new().max_file_size(100);

        let large_file_info = FileInfo {
            name: "test.zip".to_string(),
            size: 200,
            mime_type: Some("application/zip".to_string()),
        };

        let result = validate_file_info(&large_file_info, &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("File size 200 exceeds maximum allowed 100 bytes")
        );
    }
}
