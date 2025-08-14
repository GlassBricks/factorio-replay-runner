use crate::error::ServiceError;
use async_trait::async_trait;
use std::fmt::{Debug, Display};
use std::fs::File;
use tempfile::NamedTempFile;

/// Information about a file from a sharing service
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub is_zip: bool,
}

#[async_trait]
pub trait FileService: Send + Sync {
    type FileId: Send + Sync + Display;
    fn service_name() -> &'static str;

    fn detect_link(input: &str) -> Option<Self::FileId>;

    async fn get_file_info(&mut self, file_id: &Self::FileId) -> Result<FileInfo, ServiceError>;

    async fn download(
        &mut self,
        file_id: &Self::FileId,
        dest: &mut File,
    ) -> Result<(), ServiceError>;
}

#[async_trait]
pub trait FileDownloadHandle: Send + Sync + Display {
    async fn get_file_info(&mut self) -> Result<FileInfo, ServiceError>;
    async fn download(&mut self, dest: &mut File) -> Result<(), ServiceError>;
    async fn download_to_tmp(&mut self) -> Result<NamedTempFile, ServiceError> {
        let Ok(mut tmp_file) = NamedTempFile::new() else {
            return Err(ServiceError::retryable(anyhow::anyhow!(
                "Failed to create temporary file"
            )));
        };
        self.download(tmp_file.as_file_mut()).await?;
        Ok(tmp_file)
    }
    fn service_name(&self) -> &str;
}

#[async_trait]
pub trait FileServiceDyn: Send + Sync {
    fn service_name(&self) -> &str;
    fn detect_link<'a>(&'a mut self, input: &str) -> Option<Box<dyn FileDownloadHandle + 'a>>;
}

struct FileIdWrapper<'a, T: FileService> {
    service: &'a mut T,
    file_id: T::FileId,
}

#[async_trait]
impl<'a, T: FileService> FileDownloadHandle for FileIdWrapper<'a, T> {
    async fn get_file_info(&mut self) -> Result<FileInfo, ServiceError> {
        self.service.get_file_info(&self.file_id).await
    }
    async fn download(&mut self, dest: &mut File) -> Result<(), ServiceError> {
        self.service.download(&self.file_id, dest).await
    }
    fn service_name(&self) -> &str {
        self.service.service_name()
    }
}

impl<T: FileService> Display for FileIdWrapper<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} link: {}", self.service.service_name(), self.file_id)
    }
}

impl<T: FileService> FileServiceDyn for T {
    fn service_name(&self) -> &str {
        Self::service_name()
    }

    fn detect_link<'a>(&'a mut self, input: &str) -> Option<Box<dyn FileDownloadHandle + 'a>> {
        let link = Self::detect_link(input)?;
        Some(Box::new(FileIdWrapper {
            service: self,
            file_id: link,
        }))
    }
}

#[cfg(test)]
pub mod test_util {
    use super::*;

    #[derive(Debug)]
    pub struct MockService;

    #[async_trait]
    impl FileService for MockService {
        type FileId = String;

        fn service_name() -> &'static str {
            "mock"
        }

        async fn download(
            &mut self,
            _file_id: &Self::FileId,
            _dest: &mut File,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn get_file_info(
            &mut self,
            _file_id: &Self::FileId,
        ) -> Result<FileInfo, ServiceError> {
            Ok(FileInfo {
                name: "test.zip".to_string(),
                size: 1000,
                is_zip: true,
            })
        }

        fn detect_link(input: &str) -> Option<Self::FileId> {
            input.contains("mock://").then(|| "test_id".to_string())
        }
    }
}
