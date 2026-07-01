use crate::errors::ServiceResult;
use std::time::Duration;
use tokio::time::sleep;

const DEFAULT_MAX_RETRIES: u32 = 10;
const BASE_DELAY_MS: u64 = 500;

pub struct RetryConfig {
    pub max_retries: u32,
    pub base_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            base_delay: Duration::from_millis(BASE_DELAY_MS),
        }
    }
}

pub async fn with_retry<F, Fut, T>(mut operation: F, config: RetryConfig) -> ServiceResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ServiceResult<T>>,
{
    let mut last_error = None;

    for attempt in 1..=config.max_retries + 1 {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) if attempt > config.max_retries => {
                last_error = Some(err);
                break;
            }
            Err(err) if !err.is_retryable() => return Err(err),
            Err(err) => {
                let delay = calculate_delay(attempt, config.base_delay);
                tracing::warn!(
                    attempt = attempt,
                    delay_ms = delay.as_millis(),
                    error = %err,
                    "Retrying after error"
                );
                sleep(delay).await;
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap())
}

fn calculate_delay(attempt: u32, base_delay: Duration) -> Duration {
    let base_ms = base_delay.as_millis() as u64;
    let backoff = base_ms * 2u64.pow(attempt.saturating_sub(1));
    let capped = backoff.min(32000);
    let jitter = (rand::random::<f64>() * 0.25 * capped as f64) as u64;
    Duration::from_millis(capped + jitter)
}
