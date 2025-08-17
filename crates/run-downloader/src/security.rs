use anyhow::{Result, anyhow, bail};
use std::io::Read;
use std::time::Duration;
use std::{fs::File, path::Path};

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

fn validate_file_size(size: u64, config: &SecurityConfig) -> Result<()> {
    if size > config.max_file_size {
        bail!(
            "File size {} exceeds maximum allowed {} bytes",
            size,
            config.max_file_size
        );
    }
    Ok(())
}

fn validate_file_extension(filename: &str, config: &SecurityConfig) -> Result<()> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{}", ext.to_lowercase()));

    if let Some(ext) = extension {
        if config.allowed_extensions.contains(&ext) {
            return Ok(());
        }
    }

    bail!(
        "File extension not allowed for file: {}. Allowed extensions: {:?}",
        filename,
        config.allowed_extensions
    );
}

fn validate_zip_file(file: &mut File, config: &SecurityConfig) -> Result<()> {
    let mut buffer = [0u8; 4];
    file.read_exact(&mut buffer)
        .map_err(|e| anyhow!("IO error: {}", e))?;

    if buffer != [0x50, 0x4b, 0x03, 0x04] {
        bail!("Invalid ZIP magic number");
    }

    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| anyhow!("ZIP archive error: {}", e))?;

    // Check number of entries
    if archive.len() > config.max_zip_entries {
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
            return Err(anyhow!(
                "ZIP entry '{}' size {} exceeds maximum {}",
                entry.name(),
                entry.size(),
                config.max_extracted_size
            ));
        }
    }

    if total_uncompressed_size > config.max_extracted_size {
        return Err(anyhow!(
            "Total uncompressed size {} exceeds maximum {}",
            total_uncompressed_size,
            config.max_extracted_size
        ));
    }

    Ok(())
}

/// Validate ZIP entry path for path traversal attacks
fn validate_zip_entry_path(path: &str) -> Result<()> {
    if path.contains("..") {
        bail!("Path traversal attempt detected: {}", path);
    }

    if path.starts_with('/') || path.starts_with('\\') {
        bail!("Absolute path detected: {}", path);
    }

    if path.len() >= 2 && path.chars().nth(1) == Some(':') {
        bail!("Windows drive path detected: {}", path);
    }

    Ok(())
}

fn validate_zip_magic_number(file: &mut File) -> Result<()> {
    let mut buffer = [0u8; 4];
    file.read_exact(&mut buffer)
        .map_err(|e| anyhow!("IO error: {}", e))?;

    // ZIP file magic numbers
    const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04]; // "PK\x03\x04"
    const ZIP_EMPTY_MAGIC: [u8; 4] = [0x50, 0x4B, 0x05, 0x06]; // "PK\x05\x06"
    const ZIP_SPANNED_MAGIC: [u8; 4] = [0x50, 0x4B, 0x07, 0x08]; // "PK\x07\x08"

    if buffer == ZIP_MAGIC || buffer == ZIP_EMPTY_MAGIC || buffer == ZIP_SPANNED_MAGIC {
        Ok(())
    } else {
        bail!("File is not a valid ZIP file");
    }
}

pub fn validate_file_info(
    file_info: &crate::services::FileInfo,
    config: &SecurityConfig,
) -> Result<()> {
    validate_file_size(file_info.size, config)?;
    validate_file_extension(&file_info.name, config)?;
    Ok(())
}

pub fn validate_downloaded_file(
    file: &mut File,
    file_info: &crate::services::FileInfo,
    config: &SecurityConfig,
) -> Result<()> {
    validate_zip_magic_number(file)?;
    validate_zip_file(file, config)?;

    let actual_size = file.metadata()?.len();
    if actual_size != file_info.size {
        bail!(
            "File size mismatch: expected {}, got {}",
            file_info.size,
            actual_size
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::FileInfo;

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
    fn test_validate_zip_entry_path() {
        assert!(validate_zip_entry_path("normal/path/file.txt").is_ok());
        assert!(validate_zip_entry_path("../../../etc/passwd").is_err());
        assert!(validate_zip_entry_path("/absolute/path").is_err());
        assert!(validate_zip_entry_path("\\windows\\path").is_err());
        assert!(validate_zip_entry_path("C:\\windows\\path").is_err());
    }

    use std::io::Write;
    use tempfile::NamedTempFile;
    use zip::write::{FileOptions, ZipWriter};

    #[test]
    fn test_create_test_zip() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.as_file_mut());

        zip.start_file("test.txt", FileOptions::<()>::default())
            .unwrap();
        zip.write_all(b"Hello, world!").unwrap();
        zip.finish().unwrap();

        // Test with default config - should pass
        let config = SecurityConfig::default();
        assert!(validate_zip_file(temp_file.as_file_mut(), &config).is_ok());
    }

    #[test]
    fn test_zip_validation_too_many_entries() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.as_file_mut());

        // Add 5 files
        for i in 0..5 {
            zip.start_file(&format!("file{}.txt", i), FileOptions::<()>::default())
                .unwrap();
            zip.write_all(b"test content").unwrap();
        }
        zip.finish().unwrap();

        // Test with very low limit - should fail
        let config = SecurityConfig::new().max_zip_entries(3);
        let result = validate_zip_file(temp_file.as_file_mut(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("ZIP file has 5 entries, exceeds maximum 3")
        );
    }

    #[test]
    fn test_zip_validation_file_too_large() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.as_file_mut());

        zip.start_file("large.txt", FileOptions::<()>::default())
            .unwrap();
        let large_content = vec![b'A'; 2000]; // 2KB file
        zip.write_all(&large_content).unwrap();
        zip.finish().unwrap();

        // Test with very low size limit - should fail
        let config = SecurityConfig::new().max_extracted_size(1000); // 1KB limit
        let result = validate_zip_file(temp_file.as_file_mut(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("size 2000 exceeds maximum 1000"));
    }

    #[test]
    fn test_zip_validation_total_size_exceeded() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.as_file_mut());

        for i in 0..3 {
            zip.start_file(&format!("file{}.txt", i), FileOptions::<()>::default())
                .unwrap();
            let content = vec![b'A'; 800];
            zip.write_all(&content).unwrap();
        }
        zip.finish().unwrap();

        let config = SecurityConfig::new().max_extracted_size(2000);
        let result = validate_zip_file(temp_file.as_file_mut(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Total uncompressed size 2400 exceeds maximum 2000")
        );
    }

    #[test]
    fn test_zip_validation_path_traversal() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.as_file_mut());

        zip.start_file("../../../etc/passwd", FileOptions::<()>::default())
            .unwrap();
        zip.write_all(b"malicious content").unwrap();
        zip.finish().unwrap();

        // Should fail due to path traversal
        let config = SecurityConfig::default();
        let result = validate_zip_file(temp_file.as_file_mut(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Path traversal attempt detected"));
    }

    #[test]
    fn test_security_error_types() {
        let config = SecurityConfig::new().max_file_size(100);

        let large_file_info = FileInfo {
            name: "test.zip".to_string(),
            size: 200,
            is_zip: true,
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
