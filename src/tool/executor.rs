use std::time::Duration;

use serde_json::Value;
use tokio::time::sleep;

use crate::error::ToolError;

pub struct ToolExecutor {
    max_attempts: usize,
    base_delay: Duration,
    max_delay: Duration,
}

impl ToolExecutor {
    pub fn new(max_attempts: usize) -> Self {
        Self {
            max_attempts,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }

    pub fn with_delays(mut self, base_delay: Duration, max_delay: Duration) -> Self {
        self.base_delay = base_delay;
        self.max_delay = max_delay;
        self
    }

    pub async fn execute_with_retry<F, Fut>(&self, f: F) -> Result<Value, ToolError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<Value, ToolError>>,
    {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) if e.is_retryable() && attempt < self.max_attempts => {
                    let delay = self.calculate_delay(attempt);
                    sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn calculate_delay(&self, attempt: usize) -> Duration {
        let multiplier = u64::pow(2, (attempt.saturating_sub(1)) as u32);
        let base_ms = self.base_delay.as_millis() as u64;
        let exponential_ms = base_ms * multiplier;
        let max_ms = self.max_delay.as_millis() as u64;
        let capped_ms = std::cmp::min(exponential_ms, max_ms);
        let jitter = capped_ms / 2;
        Duration::from_millis(capped_ms + jitter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic;

    #[tokio::test]
    async fn test_retry_success_first_attempt() {
        let executor = ToolExecutor::new(3);
        let call_count = Arc::new(atomic::AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let result = executor
            .execute_with_retry(move || {
                let count = Arc::clone(&call_count_clone);
                async move {
                    count.fetch_add(1, atomic::Ordering::SeqCst);
                    Ok(serde_json::json!({ "success": true }))
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(call_count.load(atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_failure_after_retries() {
        let executor = ToolExecutor::new(3);
        let call_count = Arc::new(atomic::AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let result = executor
            .execute_with_retry(move || {
                let count = Arc::clone(&call_count_clone);
                async move {
                    count.fetch_add(1, atomic::Ordering::SeqCst);
                    Err(ToolError::Io("test error".to_string()))
                }
            })
            .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_non_retryable_no_retries() {
        let executor = ToolExecutor::new(3);
        let call_count = Arc::new(atomic::AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let result = executor
            .execute_with_retry(move || {
                let count = Arc::clone(&call_count_clone);
                async move {
                    count.fetch_add(1, atomic::Ordering::SeqCst);
                    Err(ToolError::NotFound("test error".to_string()))
                }
            })
            .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(atomic::Ordering::SeqCst), 1);
    }
}