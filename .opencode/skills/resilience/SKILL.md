---
name: resilience
description: Circuit breaker and resilience patterns in opencode-rs
version: 1.1.0
tags:
  - resilience
  - circuit-breaker
  - provider
  - fallback
---

# Resilience System Guide

This skill covers resilience mechanisms in opencode-rs for provider fault tolerance.

## Overview

The resilience module provides:
- **Circuit Breaker**: Three-state circuit breaker to prevent cascade failures
- **Provider Fallback**: Sequential fallback chain when providers fail

## Circuit Breaker (`src/resilience/circuit.rs`)

### States

- **Closed**: Requests pass through. Track failures. On `failure_threshold`, transition to Open.
- **Open**: Requests fail immediately with `CircuitOpen` error. After `timeout_secs`, transition to HalfOpen.
- **HalfOpen**: Allow one request. If success, transition to Closed. If failure, transition to Open.

### Implementation (Updated 2026-05-22)

The circuit breaker uses `TokioRwLock` for all state fields (not `AtomicU8` as older documentation claimed).

`is_available()` uses write lock from the start to avoid TOCTOU race:

```rust
pub async fn is_available(&self) -> bool {
    let mut state = self.inner.state.write().await;  // Write lock from start
    match *state {
        CircuitState::Closed | CircuitState::HalfOpen => true,
        CircuitState::Open => {
            if let Some(last_failure) = *self.inner.last_failure_time.read().await {
                let timeout = Duration::from_secs(self.inner.timeout_secs);
                if last_failure.elapsed() >= timeout {
                    *state = CircuitState::HalfOpen;  // Direct state transition
                    return true;
                }
            }
            false
        }
    }
}
```

### Public API

```rust
use crate::resilience::CircuitBreaker;

let breaker = CircuitBreaker::new(
    "anthropic",
    5,    // failure_threshold
    60,   // timeout_secs
    2,    // success_threshold
);

// Check if circuit allows calls
if breaker.is_available().await {
    breaker.record_success().await;   // Record successful call
    breaker.record_failure().await;   // Record failed call
}

// Or use call() to wrap operations automatically
let result = breaker.call(async {
    provider.stream(request).await
}).await;
```

### Integration with FallbackProvider (Updated 2026-05-02)

The `FallbackProvider` now creates a `CircuitBreaker` for each provider:
- Before calling a provider, checks if its circuit is available
- Records successes/failures to update circuit state
- Prevents cascading failures when providers are down

```rust
// In FallbackProvider::stream():
if let Some(cb) = self.circuit_breakers.get(i) {
    if !cb.is_available().await {
        // Skip this provider, try next in chain
    }
    // Call provider...
    // Record success/failure
}
```

## Provider Fallback (`src/provider/fallback.rs`)

### Usage

```rust
use crate::provider::FallbackProvider;

let fallback = FallbackProvider::new(
    vec![openai_provider, anthropic_provider],
    vec![429, 500, 502, 503, 504],  // retryable status codes
);

let stream = fallback.stream(&request).await;
```

### Behavior

1. Try primary provider
2. If error matches status codes, try next in chain
3. If circuit breaker is open for a provider, skip to next
4. If all fail, return last error
5. Log each fallback attempt

### CircuitOpen Error (2026-05-22)

When a circuit breaker is open, `FallbackProvider` returns `ProviderError::CircuitOpen`:

```rust
if !cb.is_available().await {
    last_error = Some(ProviderError::CircuitOpen(provider.name().to_string()));
    continue;
}
```

`CircuitError::Open` from `CircuitBreaker::call()` automatically converts to `ProviderError::CircuitOpen` via the `From` trait in `error.rs:198-206`.

## Related Skills

- See `.opencode/skills/provider/SKILL.md` for provider system