# Resilience Module

The `resilience` module provides fault tolerance patterns.

## Overview

**Location**: `src/resilience/`

**Key Responsibilities**:
- Circuit breaker pattern
- Retry mechanisms
- Rate limiting

## Circuit Breaker

### CircuitBreaker

```rust
pub struct CircuitBreaker {
    state: Arc<AtomicU8>,
    failure_count: Arc<AtomicUsize>,
    last_failure: Arc<Mutex<Instant>>,
    config: CircuitBreakerConfig,
}

#[derive(Clone, Copy, Debug)]
pub enum CircuitState {
    Closed,    // Normal operation
    Open,      // Failing, reject requests
    HalfOpen,  // Testing if recovered
}

pub struct CircuitBreakerConfig {
    pub failure_threshold: usize,
    pub recovery_timeout: Duration,
    pub half_open_max_calls: usize,
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
    │ recovery_timeout elapsed            │
    │                │                    │
    └────────┬───────┘                    │
             │                            │
             │ failure                    │
             ▼                            │
        ┌─────────┐                       │
        │  Open   │───────────────────────┘
        │(reject) │
        └─────────┘
```

### Usage

```rust
pub async fn call_with_circuit<F, Fut>(cb: &CircuitBreaker, f: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    match cb.state() {
        CircuitState::Open => Err(Error::CircuitOpen),
        CircuitState::HalfOpen | CircuitState::Closed => {
            let result = f().await;
            cb.record_result(&result);
            result
        }
    }
}
```

## Rate Limiting

```rust
pub struct RateLimiter {
    tokens: Arc<Mutex<f64>>,
    last_refill: Instant,
    config: RateLimitConfig,
}

pub struct RateLimitConfig {
    pub requests_per_second: f64,
    pub burst: usize,
}
```

## See Also

- [provider.md](provider.md) - Uses circuit breaker for API calls
