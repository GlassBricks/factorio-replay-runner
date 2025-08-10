use crate::error::ServiceError;
use async_trait::async_trait;
use std::fmt::Debug;
use std::path::Path;

pub mod google_drive;
pub use google_drive::GoogleDriveService;

/// Information about a file from a sharing service
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub mime_type: Option<String>,
}

#[async_trait]
pub trait FileDownloader: Send + Sync {
    fn service_name(&self) -> &str;

    async fn download(&mut self, file_id: &str, dest_path: &Path) -> Result<(), ServiceError>;

    async fn get_file_info(&mut self, file_id: &str) -> Result<FileInfo, ServiceError>;
}

#[async_trait]
pub trait FileServiceDyn: FileDownloader + Send + Sync {
    fn detect_link(&self, input: &str) -> Option<String>;
}

pub trait FileService: FileDownloader + Send + Sync {
    fn detect_links(input: &str) -> Option<String>;
}

impl<T> FileServiceDyn for T
where
    T: FileService + Send + Sync,
{
    fn detect_link(&self, input: &str) -> Option<String> {
        T::detect_links(input)
    }
}

#[cfg(test)]
pub mod test_util {
    use super::*;

    pub struct MockService;

    #[async_trait]
    impl FileService for MockService {
        fn detect_links(input: &str) -> Option<String> {
            input.contains("mock://").then(|| "test_id".to_string())
        }
    }

    #[async_trait]
    impl FileDownloader for MockService {
        fn service_name(&self) -> &str {
            "mock"
        }

        async fn download(
            &mut self,
            _file_id: &str,
            _dest_path: &Path,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn get_file_info(&mut self, _file_id: &str) -> Result<FileInfo, ServiceError> {
            Ok(FileInfo {
                name: "test.zip".to_string(),
                size: 1000,
                mime_type: Some("application/zip".to_string()),
            })
        }
    }
}
