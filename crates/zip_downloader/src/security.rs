use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use std::io::Read;
use std::sync::LazyLock;
use std::{fs::File, path::Path};

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub max_file_size: u64,
    pub max_extracted_size: u64,
    pub max_zip_entries: usize,
    pub allowed_extensions: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            max_file_size: 100 * 1024 * 1024,      // 100 MB
            max_extracted_size: 500 * 1024 * 1024, // 500 MB
            max_zip_entries: 1000,
            allowed_extensions: vec![".zip".to_string()],
        }
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

    if let Some(ext) = extension
        && config.allowed_extensions.contains(&ext)
    {
        return Ok(());
    }

    bail!(
        "File extension not allowed: {}. Allowed extensions: {:?}",
        filename,
        config.allowed_extensions
    );
}

fn validate_zip_file(file: &mut File, config: &SecurityConfig) -> Result<()> {
    let mut archive = zip::ZipArchive::new(file).with_context(|| "Failed to read zip")?;

    // Check number of entries
    if archive.len() > config.max_zip_entries {
        bail!(
            "ZIP file has {} entries, maximum is {}",
            archive.len(),
            config.max_zip_entries
        );
    }

    let mut total_uncompressed_size = 0u64;

    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .with_context(|| format!("Failed to read ZIP entry {}", i))?;

        validate_zip_entry_path(entry.name())?;

        total_uncompressed_size += entry.size();
    }

    if total_uncompressed_size > config.max_extracted_size {
        bail!(
            "Total uncompressed size {} exceeds maximum {}",
            total_uncompressed_size,
            config.max_extracted_size
        );
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
        .with_context(|| "Failed to read file")?;

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
    file_info: &crate::services::FileMeta,
    config: &SecurityConfig,
) -> Result<()> {
    validate_file_name(file_info.name.as_str())?;
    validate_file_size(file_info.size, config)?;
    validate_file_extension(&file_info.name, config)?;
    Ok(())
}

static INVALID_FILE_NAME_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[/\\]").unwrap());

fn validate_file_name(file_name: &str) -> Result<()> {
    ensure!(
        !INVALID_FILE_NAME_REGEX.is_match(file_name),
        "File name {} contains path separators",
        file_name
    );
    Ok(())
}

pub fn validate_downloaded_file(
    file: &mut File,
    file_info: &crate::services::FileMeta,
    config: &SecurityConfig,
) -> Result<()> {
    validate_zip_magic_number(file)?;
    validate_zip_file(file, config)?;

    let actual_size = file.metadata()?.len();
    // Allow size mismatch when expected size is 0 (unknown size from services that can't get metadata)
    if file_info.size != 0 && actual_size != file_info.size {
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
    use crate::FileMeta;

    use super::*;

    #[test]
    fn test_security_config_builder() {
        let config = SecurityConfig {
            max_file_size: 50 * 1024 * 1024,
            max_zip_entries: 500,
            ..Default::default()
        };

        assert_eq!(config.max_file_size, 50 * 1024 * 1024);
        assert_eq!(config.max_zip_entries, 500);
    }

    #[test]
    fn test_validate_file_size() {
        let config = SecurityConfig::default();

        assert!(validate_file_size(1000, &config).is_ok());
        let result = validate_file_size(config.max_file_size + 1, &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("File size 104857601 exceeds maximum allowed 104857600 bytes")
        );
    }

    #[test]
    fn test_validate_file_extension() {
        let config = SecurityConfig::default();

        assert!(validate_file_extension("test.zip", &config).is_ok());
        assert!(validate_file_extension("test.ZIP", &config).is_ok());
        let result = validate_file_extension("test.txt", &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("File extension not allowed: test.txt. Allowed extensions: [\".zip\"]")
        );
        let result = validate_file_extension("test", &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("File extension not allowed: test. Allowed extensions: [\".zip\"]")
        );
    }

    #[test]
    fn test_validate_zip_entry_path() {
        assert!(validate_zip_entry_path("normal/path/file.txt").is_ok());
        let result = validate_zip_entry_path("../../../etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Path traversal attempt detected: ../../../etc/passwd")
        );
        let result = validate_zip_entry_path("/absolute/path");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Absolute path detected: /absolute/path")
        );
        let result = validate_zip_entry_path("\\windows\\path");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Absolute path detected: \\windows\\path")
        );
        let result = validate_zip_entry_path("C:\\windows\\path");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Windows drive path detected: C:\\windows\\path")
        );
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
            zip.start_file(format!("file{}.txt", i), FileOptions::<()>::default())
                .unwrap();
            zip.write_all(b"test content").unwrap();
        }
        zip.finish().unwrap();

        // Test with very low limit - should fail
        let config = SecurityConfig {
            max_zip_entries: 3,
            ..Default::default()
        };
        let result = validate_zip_file(temp_file.as_file_mut(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("ZIP file has 5 entries, maximum is 3")
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
        let config = SecurityConfig {
            max_extracted_size: 1000,
            ..Default::default()
        };
        let result = validate_zip_file(temp_file.as_file_mut(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Total uncompressed size 2000 exceeds maximum 1000")
        );
    }

    #[test]
    fn test_zip_validation_total_size_exceeded() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(temp_file.as_file_mut());

        for i in 0..3 {
            zip.start_file(format!("file{}.txt", i), FileOptions::<()>::default())
                .unwrap();
            let content = vec![b'A'; 800];
            zip.write_all(&content).unwrap();
        }
        zip.finish().unwrap();

        let config = SecurityConfig {
            max_extracted_size: 2000,
            ..Default::default()
        };
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
        assert!(
            err.to_string()
                .contains("Path traversal attempt detected: ../../../etc/passwd")
        );
    }

    #[test]
    fn test_security_error_types() {
        let config = SecurityConfig {
            max_file_size: 100,
            ..Default::default()
        };

        let large_file_info = FileMeta {
            name: "test.zip".to_string(),
            size: 200,
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
