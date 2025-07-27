use thiserror::Error;

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("No valid download link found in input")]
    NoLinkFound,

    #[error("Security violation: {0}")]
    SecurityError(#[from] anyhow::Error),

    #[error("Service error: {0}")]
    ServiceError(ServiceError),

    #[error("Other error: {0}")]
    Other(anyhow::Error),
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("Retryable error: {0}")]
    Retryable(anyhow::Error),

    #[error("Fatal error: {0}")]
    Fatal(anyhow::Error),
}

impl ServiceError {
    pub fn retryable(error: impl Into<anyhow::Error>) -> Self {
        Self::Retryable(error.into())
    }

    pub fn fatal(error: impl Into<anyhow::Error>) -> Self {
        Self::Fatal(error.into())
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self, ServiceError::Retryable(_))
    }

    pub fn is_fatal(&self) -> bool {
        matches!(self, ServiceError::Fatal(_))
    }
}

impl From<ServiceError> for DownloadError {
    fn from(error: ServiceError) -> Self {
        DownloadError::ServiceError(error)
    }
}
