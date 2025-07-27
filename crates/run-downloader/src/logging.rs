use tracing::{Level, Span};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry, fmt};

pub fn init_logging() {
    init_logging_with_level("info")
}

pub fn init_logging_with_level(level: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("run_downloader={}", level)));

    Registry::default()
        .with(fmt::layer().with_target(false).with_thread_ids(true))
        .with(filter)
        .init();
}

pub fn init_test_logging() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::WARN)
        .with_test_writer()
        .try_init();
}

pub fn download_span(service: &str, file_id: &str) -> Span {
    tracing::info_span!(
        "download",
        service = service,
        file_id = file_id,
        status = tracing::field::Empty
    )
}

pub fn security_validation_span(validation_type: &str) -> Span {
    tracing::debug_span!(
        "security_validation",
        validation_type = validation_type,
        result = tracing::field::Empty
    )
}

pub fn service_operation_span(service: &str, operation: &str) -> Span {
    tracing::debug_span!(
        "service_operation",
        service = service,
        operation = operation,
        result = tracing::field::Empty
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_test_logging() {
        // Should not panic
        init_test_logging();
    }

    #[test]
    fn test_span_creation() {
        init_test_logging();

        // Test that spans can be created without panicking
        let _download = download_span("google_drive", "test_id");
        let _security = security_validation_span("file_size");
        let _service_op = service_operation_span("test_service", "authenticate");
    }
}
