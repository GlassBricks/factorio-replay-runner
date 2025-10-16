use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::ErrorClass;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetryConfig {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_initial_backoff_secs")]
    initial_backoff_secs: u64,
    #[serde(default = "default_max_backoff_secs")]
    max_backoff_secs: u64,
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: f64,
}

fn default_max_attempts() -> u32 {
    8
}

fn default_initial_backoff_secs() -> u64 {
    60
}

fn default_max_backoff_secs() -> u64 {
    3600
}

fn default_backoff_multiplier() -> f64 {
    2.0
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            initial_backoff_secs: default_initial_backoff_secs(),
            max_backoff_secs: default_max_backoff_secs(),
            backoff_multiplier: default_backoff_multiplier(),
        }
    }
}

impl RetryConfig {
    #[allow(dead_code)]
    pub fn initial_backoff(&self) -> Duration {
        Duration::from_secs(self.initial_backoff_secs)
    }

    #[allow(dead_code)]
    pub fn max_backoff(&self) -> Duration {
        Duration::from_secs(self.max_backoff_secs)
    }
}

#[allow(dead_code)]
pub fn calculate_next_retry(
    retry_count: u32,
    error_class: &ErrorClass,
    config: &RetryConfig,
) -> Option<DateTime<Utc>> {
    match error_class {
        ErrorClass::Final => None,
        ErrorClass::RateLimited {
            retry_after: Some(retry_after),
        } => {
            let delay = ChronoDuration::from_std(*retry_after).ok()?;
            Some(Utc::now() + delay)
        }
        ErrorClass::RateLimited { retry_after: None } | ErrorClass::Retryable => {
            if retry_count + 1 >= config.max_attempts {
                return None;
            }

            let base_delay = config.initial_backoff().as_secs_f64();
            let multiplier = config.backoff_multiplier;
            let max_delay = config.max_backoff().as_secs_f64();

            let delay_secs = (base_delay * multiplier.powi(retry_count as i32)).min(max_delay);
            let delay = ChronoDuration::seconds(delay_secs as i64);

            Some(Utc::now() + delay)
        }
    }
}

#[allow(dead_code)]
pub fn error_class_to_string(error_class: &ErrorClass) -> &'static str {
    match error_class {
        ErrorClass::Final => "final",
        ErrorClass::Retryable => "retryable",
        ErrorClass::RateLimited { .. } => "rate_limited",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 8);
        assert_eq!(config.initial_backoff().as_secs(), 60);
        assert_eq!(config.max_backoff().as_secs(), 3600);
        assert_eq!(config.backoff_multiplier, 2.0);
    }

    #[test]
    fn test_final_error_returns_none() {
        let config = RetryConfig::default();
        let result = calculate_next_retry(0, &ErrorClass::Final, &config);
        assert_eq!(result, None);
    }

    #[test]
    fn test_max_attempts_exceeded() {
        let config = RetryConfig::default();
        let result = calculate_next_retry(6, &ErrorClass::Retryable, &config);
        assert!(result.is_some());

        let result = calculate_next_retry(7, &ErrorClass::Retryable, &config);
        assert_eq!(result, None);

        let result = calculate_next_retry(8, &ErrorClass::Retryable, &config);
        assert_eq!(result, None);
    }

    #[test]
    fn test_exponential_backoff_calculation() {
        let config = RetryConfig::default();
        let now = Utc::now();

        let result = calculate_next_retry(0, &ErrorClass::Retryable, &config).unwrap();
        let delay = (result - now).num_seconds();
        assert!((59..=61).contains(&delay));

        let result = calculate_next_retry(1, &ErrorClass::Retryable, &config).unwrap();
        let delay = (result - now).num_seconds();
        assert!((119..=121).contains(&delay));

        let result = calculate_next_retry(2, &ErrorClass::Retryable, &config).unwrap();
        let delay = (result - now).num_seconds();
        assert!((239..=241).contains(&delay));
    }

    #[test]
    fn test_max_backoff_cap() {
        let config = RetryConfig::default();
        let now = Utc::now();

        let result = calculate_next_retry(6, &ErrorClass::Retryable, &config).unwrap();
        let delay = (result - now).num_seconds();
        assert!((3599..=3601).contains(&delay));
    }

    #[test]
    fn test_rate_limited_with_retry_after() {
        let config = RetryConfig::default();
        let now = Utc::now();
        let retry_after = Duration::from_secs(300);

        let result = calculate_next_retry(
            0,
            &ErrorClass::RateLimited {
                retry_after: Some(retry_after),
            },
            &config,
        )
        .unwrap();

        let delay = (result - now).num_seconds();
        assert!((299..=301).contains(&delay));
    }

    #[test]
    fn test_rate_limited_without_retry_after_uses_exponential_backoff() {
        let config = RetryConfig::default();
        let now = Utc::now();

        let result =
            calculate_next_retry(0, &ErrorClass::RateLimited { retry_after: None }, &config)
                .unwrap();

        let delay = (result - now).num_seconds();
        assert!((59..=61).contains(&delay));
    }

    #[test]
    fn test_rate_limited_without_retry_after_respects_max_attempts() {
        let config = RetryConfig::default();

        let result =
            calculate_next_retry(6, &ErrorClass::RateLimited { retry_after: None }, &config);

        assert!(result.is_some());

        let result =
            calculate_next_retry(7, &ErrorClass::RateLimited { retry_after: None }, &config);

        assert_eq!(result, None);
    }

    #[test]
    fn test_error_class_to_string() {
        assert_eq!(error_class_to_string(&ErrorClass::Final), "final");
        assert_eq!(error_class_to_string(&ErrorClass::Retryable), "retryable");
        assert_eq!(
            error_class_to_string(&ErrorClass::RateLimited { retry_after: None }),
            "rate_limited"
        );
        assert_eq!(
            error_class_to_string(&ErrorClass::RateLimited {
                retry_after: Some(Duration::from_secs(60))
            }),
            "rate_limited"
        );
    }

    #[test]
    fn test_custom_config() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_backoff_secs: 10,
            max_backoff_secs: 100,
            backoff_multiplier: 3.0,
        };

        let now = Utc::now();

        let result = calculate_next_retry(0, &ErrorClass::Retryable, &config).unwrap();
        let delay = (result - now).num_seconds();
        assert!((9..=11).contains(&delay));

        let result = calculate_next_retry(1, &ErrorClass::Retryable, &config).unwrap();
        let delay = (result - now).num_seconds();
        assert!((29..=31).contains(&delay));

        let result = calculate_next_retry(2, &ErrorClass::Retryable, &config);
        assert_eq!(result, None);

        let result = calculate_next_retry(3, &ErrorClass::Retryable, &config);
        assert_eq!(result, None);
    }

    #[test]
    fn test_backoff_reaches_max() {
        let config = RetryConfig {
            max_attempts: 10,
            initial_backoff_secs: 10,
            max_backoff_secs: 100,
            backoff_multiplier: 2.0,
        };

        let now = Utc::now();

        let result = calculate_next_retry(5, &ErrorClass::Retryable, &config).unwrap();
        let delay = (result - now).num_seconds();
        assert!((99..=101).contains(&delay));

        let result = calculate_next_retry(6, &ErrorClass::Retryable, &config).unwrap();
        let delay = (result - now).num_seconds();
        assert!((99..=101).contains(&delay));
    }
}
