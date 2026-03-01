use std::fmt;
use std::time::Duration;

use factorio_manager::error::FactorioError;
use zip_downloader::DownloadError;

use crate::daemon::speedrun_api::ApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    Final,
    Retryable,
    RateLimited { retry_after: Option<Duration> },
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct RunProcessingError {
    pub class: ErrorClass,
    pub message: String,
}

impl RunProcessingError {
    pub fn from_error<E: fmt::Display>(class: ErrorClass, error: &E) -> Self {
        Self {
            class,
            message: format!("{:#}", error),
        }
    }
}

impl From<DownloadError> for RunProcessingError {
    fn from(e: DownloadError) -> Self {
        let class = match &e {
            DownloadError::NoLinkFound => ErrorClass::Final,
            DownloadError::SecurityViolation(_) => ErrorClass::Final,
            DownloadError::FileNotAccessible(_) => ErrorClass::Final,
            DownloadError::ServiceError(_) => ErrorClass::Retryable,
            &DownloadError::RateLimited { retry_after, .. } => {
                ErrorClass::RateLimited { retry_after }
            }
            DownloadError::IoError(_) => ErrorClass::Retryable,
        };
        RunProcessingError::from_error(class, &e)
    }
}

impl From<FactorioError> for RunProcessingError {
    fn from(e: FactorioError) -> Self {
        let class = match &e {
            FactorioError::InvalidSaveFile(_) => ErrorClass::Final,
            FactorioError::InvalidVersion(_) => ErrorClass::Final,
            FactorioError::VersionTooOld { .. } => ErrorClass::Final,
            FactorioError::ModMismatch { .. } => ErrorClass::Final,
            FactorioError::ScriptInjectionFailed(_) => ErrorClass::Final,
            FactorioError::FactorioDownloadFailed { .. } => ErrorClass::Retryable,
            FactorioError::ExtractionFailed(_) => ErrorClass::Retryable,
            FactorioError::InstallationNotFound(_) => ErrorClass::Retryable,
            FactorioError::InstallDirError(_) => ErrorClass::Retryable,
            FactorioError::ProcessSpawnFailed(_) => ErrorClass::Retryable,
            FactorioError::ProcessExitedUnsuccessfully { detail, .. } => {
                if detail.is_some() {
                    ErrorClass::Final
                } else {
                    ErrorClass::Retryable
                }
            }
            FactorioError::ModInfoReadFailed(_) => ErrorClass::Retryable,
            FactorioError::ReplayTimeout => ErrorClass::Final,
            FactorioError::IoError(_) => ErrorClass::Retryable,
        };
        RunProcessingError::from_error(class, &e)
    }
}

impl From<ApiError> for RunProcessingError {
    fn from(e: ApiError) -> Self {
        let class = match &e {
            ApiError::NetworkError(_) => ErrorClass::Retryable,
            ApiError::NotFound(_) => ErrorClass::Final,
            ApiError::ParseError(_) => ErrorClass::Retryable,
            ApiError::MissingField(_) => ErrorClass::Final,
        };
        RunProcessingError::from_error(class, &e)
    }
}
