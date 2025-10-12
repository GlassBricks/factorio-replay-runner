use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    Final,
    Retryable,
    RateLimited { retry_after: Option<Duration> },
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ClassifiedError {
    pub class: ErrorClass,
    pub message: String,
}

impl ClassifiedError {
    pub fn from_error<E: fmt::Display>(class: ErrorClass, error: &E) -> Self {
        Self {
            class,
            message: format!("{:#}", error),
        }
    }
}
