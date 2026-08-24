use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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

struct CircuitBreakerInner {
    name: String,
    state: TokioRwLock<CircuitState>,
    failure_count: TokioRwLock<usize>,
    success_count: TokioRwLock<usize>,
    last_failure_time: TokioRwLock<Option<Instant>>,
    half_open_start_time: TokioRwLock<Option<Instant>>,
    half_open_probe: AtomicBool,
    failure_threshold: usize,
    timeout_secs: u64,
    success_threshold: usize,
    max_half_open_duration: Duration,
}

#[derive(Clone)]
pub struct CircuitBreaker {
    inner: Arc<CircuitBreakerInner>,
}

impl CircuitBreaker {
    pub fn new(
        name: impl Into<String>,
        failure_threshold: usize,
        timeout_secs: u64,
        success_threshold: usize,
    ) -> Self {
        Self {
            inner: Arc::new(CircuitBreakerInner {
                name: name.into(),
                state: TokioRwLock::new(CircuitState::Closed),
                failure_count: TokioRwLock::new(0),
                success_count: TokioRwLock::new(0),
                last_failure_time: TokioRwLock::new(None),
                half_open_start_time: TokioRwLock::new(None),
                half_open_probe: AtomicBool::new(false),
                failure_threshold,
                timeout_secs,
                success_threshold,
                max_half_open_duration: Duration::from_secs(30),
            }),
        }
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }

    pub async fn state(&self) -> CircuitState {
        *self.inner.state.read().await
    }

    pub async fn is_available(&self) -> bool {
        let mut state = self.inner.state.write().await;
        match *state {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(last_failure) = *self.inner.last_failure_time.read().await {
                    let timeout = Duration::from_secs(self.inner.timeout_secs);
                    if last_failure.elapsed() >= timeout {
                        *state = CircuitState::HalfOpen;
                        self.inner.half_open_probe.store(false, Ordering::Release);
                        *self.inner.half_open_start_time.write().await = Some(Instant::now());
                        tracing::info!(
                            "circuit breaker {} transitioned to HalfOpen",
                            self.inner.name
                        );
                        return true;
                    }
                }
                false
            }
        }
    }

    #[instrument(skip(self, op), fields(breaker_name = %self.inner.name))]
    pub async fn call<F, R, E>(&self, op: F) -> Result<R, E>
    where
        F: core::future::Future<Output = Result<R, E>>,
        E: From<CircuitError>,
    {
        if !self.is_available().await {
            return Err(CircuitError::Open(self.inner.name.clone()).into());
        }

        let state = *self.inner.state.read().await;
        if state == CircuitState::HalfOpen {
            if self
                .inner
                .half_open_probe
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return Err(CircuitError::Open(self.inner.name.clone()).into());
            }
            if let Some(start_time) = *self.inner.half_open_start_time.read().await {
                if start_time.elapsed() >= self.inner.max_half_open_duration {
                    *self.inner.state.write().await = CircuitState::Open;
                    *self.inner.half_open_start_time.write().await = None;
                    *self.inner.last_failure_time.write().await = None;
                    self.inner.half_open_probe.store(false, Ordering::Release);
                    tracing::warn!(
                        "circuit breaker {} transitioned to Open after HalfOpen timeout",
                        self.inner.name
                    );
                    return Err(CircuitError::Open(self.inner.name.clone()).into());
                }
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
        let mut state = self.inner.state.write().await;
        match *state {
            CircuitState::Closed => {
                *self.inner.failure_count.write().await = 0;
            }
            CircuitState::HalfOpen => {
                let reached_threshold = {
                    let mut count = self.inner.success_count.write().await;
                    *count += 1;
                    *count >= self.inner.success_threshold
                };
                if reached_threshold {
                    *state = CircuitState::Closed;
                    *self.inner.failure_count.write().await = 0;
                    *self.inner.success_count.write().await = 0;
                    *self.inner.last_failure_time.write().await = None;
                    tracing::info!("circuit breaker {} transitioned to Closed", self.inner.name);
                }
                self.inner.half_open_probe.store(false, Ordering::Release);
            }
            CircuitState::Open => {}
        }
    }

    pub async fn record_failure(&self) {
        let mut state = self.inner.state.write().await;
        *self.inner.last_failure_time.write().await = Some(Instant::now());
        match *state {
            CircuitState::Closed => {
                let mut count = self.inner.failure_count.write().await;
                *count += 1;
                if *count >= self.inner.failure_threshold {
                    *state = CircuitState::Open;
                    tracing::warn!(
                        "circuit breaker {} transitioned to Open after {} failures",
                        self.inner.name,
                        self.inner.failure_threshold
                    );
                }
            }
            CircuitState::HalfOpen => {
                *state = CircuitState::Open;
                *self.inner.success_count.write().await = 0;
                self.inner.half_open_probe.store(false, Ordering::Release);
                tracing::warn!(
                    "circuit breaker {} transitioned to Open after HalfOpen failure",
                    self.inner.name
                );
            }
            CircuitState::Open => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test(flavor = "current_thread")]
    async fn half_open_allows_only_one_probe() {
        let breaker = CircuitBreaker::new("test", 1, 60, 1);
        {
            let mut state = breaker.inner.state.write().await;
            *state = CircuitState::HalfOpen;
            *breaker.inner.half_open_start_time.write().await = Some(Instant::now());
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let first_calls = Arc::clone(&calls);
        let first_breaker = breaker.clone();
        let first = tokio::spawn(async move {
            first_breaker
                .call(async move {
                    first_calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Ok::<_, CircuitError>(())
                })
                .await
        });

        for _ in 0..10 {
            if calls.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }

        let second = breaker.call(async { Ok::<_, CircuitError>(()) }).await;
        assert!(matches!(second, Err(CircuitError::Open(_))));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        assert!(first.await.unwrap().is_ok());
    }
}
