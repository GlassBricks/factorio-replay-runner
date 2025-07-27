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
    pub is_public: bool,
}

#[async_trait]
pub trait FileService: Send + Sync {
    type FileId: Debug + Clone + Send + Sync;

    fn detect_links(&self, input: &str) -> Vec<Self::FileId>;

    async fn download(&self, file_id: &Self::FileId, dest_path: &Path) -> Result<(), ServiceError>;

    async fn get_file_info(&self, file_id: &Self::FileId) -> Result<FileInfo, ServiceError>;

    fn service_name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ServiceError;

    // Mock service for testing
    struct MockService;

    #[async_trait]
    impl FileService for MockService {
        type FileId = String;

        fn detect_links(&self, input: &str) -> Vec<Self::FileId> {
            if input.contains("mock://") {
                vec!["test_id".to_string()]
            } else {
                vec![]
            }
        }

        async fn download(
            &self,
            _file_id: &Self::FileId,
            _dest_path: &Path,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn get_file_info(&self, _file_id: &Self::FileId) -> Result<FileInfo, ServiceError> {
            Ok(FileInfo {
                name: "test.zip".to_string(),
                size: 1000,
                mime_type: Some("application/zip".to_string()),
                is_public: true,
            })
        }

        fn service_name(&self) -> &'static str {
            "mock"
        }
    }
}
