# Resilience Module

The `resilience` module provides fault tolerance patterns.

## Overview

**Location**: `src/resilience/`

**Key Responsibilities**:
- Circuit breaker pattern for provider fault tolerance

## Circuit Breaker

### CircuitBreaker

```rust
struct CircuitBreakerInner {
    name: String,
    state: TokioRwLock<CircuitState>,       // Uses TokioRwLock, not AtomicU8
    failure_count: TokioRwLock<usize>,
    success_count: TokioRwLock<usize>,
    last_failure_time: TokioRwLock<Option<Instant>>,
    half_open_start_time: TokioRwLock<Option<Instant>>,  // Set when Open→HalfOpen transition occurs
    failure_threshold: usize,
    timeout_secs: u64,
    success_threshold: usize,
    max_half_open_duration: Duration,        // 30 seconds, controls HalfOpen→Open timeout
}

pub struct CircuitBreaker {
    inner: Arc<CircuitBreakerInner>,
}
```

**State Enum**:
```rust
pub enum CircuitState {
    Closed,    // Normal operation
    Open,      // Failing, reject requests
    HalfOpen,  // Testing if recovered
}
```

### State Transitions

```
            failure_threshold exceeded
     ┌─────────────────────────────────────┐
     ▼                                     │
 ┌─────────┐                              │
 │ Closed  │──────────────────────────────►│
 │ (normal)│◄──────────────────────────────│
 └─────────┘     success in HalfOpen      │
     ▲                ▲                    │
     │                │                    │
     │ recovery_timeout elapsed           │
     │                │                    │
     └────────┬───────┘                    │
              │                            │
              │ failure                    │
              ▼                            │
         ┌─────────┐                       │
         │  Open   │───────────────────────┘
         │(reject) │          │
         └─────────┘          │
              ▲                │ timeout after max_half_open_duration
              │                │ (HalfOpen → Open, circuit returns error)
              │ failure        │
              ▼                ▼
         ┌─────────┐     ┌───────────┐
         │ HalfOpen │────►│   Open    │
         │(testing)│     │ (reject)  │
         └─────────┘     └───────────┘
```

The `call()` method (circuit.rs:101-137) checks for HalfOpen timeout before executing the operation:
- If `max_half_open_duration` (30s default) elapses while in HalfOpen state, transitions to Open and returns `CircuitError::Open`
- This ensures the circuit doesn't stay in HalfOpen indefinitely when the underlying service remains unavailable

### is_available() Implementation

The `is_available()` method uses a write lock from the start to avoid TOCTOU race conditions:

```rust
pub async fn is_available(&self) -> bool {
    let mut state = self.inner.state.write().await;
    match *state {
        CircuitState::Closed | CircuitState::HalfOpen => true,
        CircuitState::Open => {
            if let Some(last_failure) = *self.inner.last_failure_time.read().await {
                let timeout = Duration::from_secs(self.inner.timeout_secs);
                if last_failure.elapsed() >= timeout {
                    *state = CircuitState::HalfOpen;
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
```

### Usage

```rust
pub async fn call<F, R, E>(&self, op: F) -> Result<R, E>
where
    F: Future<Output = Result<R, E>>,
    E: From<CircuitError>,
{
    if !self.is_available().await {
        let state = *self.inner.state.read().await;
        if state == CircuitState::Open {
            return Err(CircuitError::Open(self.inner.name.clone()).into());
        }
    }

    let result = op.await;

    match &result {
        Ok(_) => self.record_success().await,
        Err(_) => self.record_failure().await,
    }

    result
}
```

### record_success() and record_failure()

The `record_success()` method (circuit.rs):
- **Closed**: Resets failure_count to 0
- **HalfOpen**: Increments success_count; transitions to Closed when success_threshold reached
- **Open**: No action

The `record_failure()` method (circuit.rs):
- **Closed**: Increments failure_count; transitions to Open when failure_threshold exceeded
- **HalfOpen**: Transitions to Open immediately
- **Open**: No action

### FallbackProvider Integration

`FallbackProvider` (provider/fallback.rs) creates a `CircuitBreaker` for each provider:
- Default parameters: `failure_threshold=3`, `timeout_secs=60`, `success_threshold=2`
- Checks `is_available()` before calling provider
- Records success/failure after each call
- Exponential backoff between providers: `2^i` seconds (i=0→1s, i=1→2s, i=2→4s...), capped at 30s
- HalfOpen→Open timeout: 30s default via `max_half_open_duration`

## See Also

- [provider.md](provider.md) - Uses circuit breaker for API calls via FallbackProvider
- `.opencode/skills/resilience/SKILL.md` - Skill with usage examples