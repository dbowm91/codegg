---
name: resilience
description: Circuit breaker and resilience patterns in opencode-rs
version: 1.0.0
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

### Public API (Updated 2026-05-02)

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
    // Record success/failure manually if not using call()
    breaker.record_success().await;   // Now public (2026-05-02)
    breaker.record_failure().await;   // Now public (2026-05-02)
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
3. If all fail, return last error
4. Log each fallback attempt

## Related Skills

- See `.opencode/skills/provider/SKILL.md` for provider system