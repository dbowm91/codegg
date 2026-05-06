//! Circuit breaker implementation for provider resilience.

use std::time::{Duration, Instant};
use tokio::sync::RwLock as TokioRwLock;
use tracing::instrument;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug)]
pub enum CircuitError {
    Open(String),
}

impl std::fmt::Display for CircuitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitError::Open(name) => write!(f, "circuit breaker open for {}", name),
        }
    }
}

impl std::error::Error for CircuitError {}

pub struct CircuitBreaker {
    name: String,
    state: TokioRwLock<CircuitState>,
    failure_count: TokioRwLock<usize>,
    success_count: TokioRwLock<usize>,
    last_failure_time: TokioRwLock<Option<Instant>>,
    failure_threshold: usize,
    timeout_secs: u64,
    success_threshold: usize,
}

impl CircuitBreaker {
    pub fn new(
        name: impl Into<String>,
        failure_threshold: usize,
        timeout_secs: u64,
        success_threshold: usize,
    ) -> Self {
        Self {
            name: name.into(),
            state: TokioRwLock::new(CircuitState::Closed),
            failure_count: TokioRwLock::new(0),
            success_count: TokioRwLock::new(0),
            last_failure_time: TokioRwLock::new(None),
            failure_threshold,
            timeout_secs,
            success_threshold,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn state(&self) -> CircuitState {
        *self.state.read().await
    }

    pub async fn is_available(&self) -> bool {
        let state = self.state.read().await;
        if *state == CircuitState::Open {
            if let Some(last_failure) = *self.last_failure_time.read().await {
                let timeout = Duration::from_secs(self.timeout_secs);
                if last_failure.elapsed() >= timeout {
                    drop(state);
                    let mut s = self.state.write().await;
                    *s = CircuitState::HalfOpen;
                    tracing::info!("circuit breaker {} transitioned to HalfOpen", self.name);
                    return true;
                }
            }
            false
        } else {
            true
        }
    }

    #[instrument(skip(self, op), fields(breaker_name = %self.name))]
    pub async fn call<F, R, E>(&self, op: F) -> Result<R, E>
    where
        F: core::future::Future<Output = Result<R, E>>,
        E: From<CircuitError>,
    {
        if !self.is_available().await {
            let state = *self.state.read().await;
            if state == CircuitState::Open {
                return Err(CircuitError::Open(self.name.clone()).into());
            }
        }

        let result = op.await;

        match &result {
            Ok(_) => self.record_success().await,
            Err(_) => self.record_failure().await,
        }

        result
    }

    pub async fn record_success(&self) {
        let mut state = self.state.write().await;
        match *state {
            CircuitState::Closed => {
                *self.failure_count.write().await = 0;
            }
            CircuitState::HalfOpen => {
                let mut count = self.success_count.write().await;
                *count += 1;
                if *count >= self.success_threshold {
                    *state = CircuitState::Closed;
                    *self.failure_count.write().await = 0;
                    *self.success_count.write().await = 0;
                    tracing::info!("circuit breaker {} transitioned to Closed", self.name);
                }
            }
            CircuitState::Open => {}
        }
    }

    pub async fn record_failure(&self) {
        let mut state = self.state.write().await;
        *self.last_failure_time.write().await = Some(Instant::now());
        match *state {
            CircuitState::Closed => {
                let mut count = self.failure_count.write().await;
                *count += 1;
                if *count >= self.failure_threshold {
                    *state = CircuitState::Open;
                    tracing::warn!(
                        "circuit breaker {} transitioned to Open after {} failures",
                        self.name,
                        self.failure_threshold
                    );
                }
            }
            CircuitState::HalfOpen => {
                *state = CircuitState::Open;
                *self.success_count.write().await = 0;
                tracing::warn!(
                    "circuit breaker {} transitioned to Open after HalfOpen failure",
                    self.name
                );
            }
            CircuitState::Open => {}
        }
    }
}

impl Clone for CircuitBreaker {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            state: TokioRwLock::new(*self.state.blocking_read()),
            failure_count: TokioRwLock::new(*self.failure_count.blocking_read()),
            success_count: TokioRwLock::new(*self.success_count.blocking_read()),
            last_failure_time: TokioRwLock::new(*self.last_failure_time.blocking_read()),
            failure_threshold: self.failure_threshold,
            timeout_secs: self.timeout_secs,
            success_threshold: self.success_threshold,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_starts_closed() {
        let cb = CircuitBreaker::new("test", 5, 60, 2);
        assert_eq!(cb.state().await, CircuitState::Closed);
        assert!(cb.is_available().await);
    }

    #[tokio::test]
    async fn test_circuit_breaker_failure_transitions_to_open() {
        let cb = CircuitBreaker::new("test", 3, 60, 2);

        for _ in 0..3 {
            let _: Result<(), CircuitError> = cb
                .call(async { Err(CircuitError::Open("test".to_string())) })
                .await;
        }

        assert_eq!(cb.state().await, CircuitState::Open);
        assert!(!cb.is_available().await);
    }

    #[tokio::test]
    async fn test_circuit_breaker_success_resets_failures() {
        let cb = CircuitBreaker::new("test", 3, 60, 2);

        let _: Result<(), CircuitError> = cb.call(async { Ok(()) }).await;
        let _: Result<(), CircuitError> = cb
            .call(async { Err(CircuitError::Open("test".to_string())) })
            .await;
        let _: Result<(), CircuitError> = cb
            .call(async { Err(CircuitError::Open("test".to_string())) })
            .await;

        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_err_returns_circuit_error_when_open() {
        let cb = CircuitBreaker::new("test", 1, 60, 2);

        let _: Result<(), CircuitError> = cb
            .call(async { Err(CircuitError::Open("test".to_string())) })
            .await;

        let result: Result<(), CircuitError> = cb.call(async { Ok(()) }).await;

        assert!(matches!(result, Err(CircuitError::Open(_))));
    }
}
