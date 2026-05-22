# Resilience Module Override

This file contains resilience-specific guidance and overrides root AGENTS.md.

## CircuitBreaker API (Updated 2026-05-22)

### Public Methods

- `new(name, failure_threshold, timeout_secs, success_threshold)` - Create a new circuit breaker
- `name()` - Get the circuit breaker name
- `state()` - Get current state (Closed, Open, HalfOpen)
- `is_available()` - Check if circuit allows calls (considers Open state timeout)
- `call(op)` - Execute an operation through the circuit breaker
- `record_success()` - Record a successful call (public as of 2026-05-02)
- `record_failure()` - Record a failed call (public as of 2026-05-02)

### CircuitError to ProviderError Conversion (2026-05-22)

`CircuitBreaker::call()` returns `CircuitError::Open(name)` which automatically converts to `ProviderError::CircuitOpen(name)` via the `From` trait in `error.rs:198-206`:

```rust
impl From<crate::resilience::circuit::CircuitError> for ProviderError {
    fn from(e: CircuitError) -> Self {
        match e {
            CircuitError::Open(name) => ProviderError::CircuitOpen(name),
        }
    }
}
```

### Integration with FallbackProvider (Updated 2026-05-22)

`FallbackProvider` creates a `CircuitBreaker` for each provider and uses `ProviderError::CircuitOpen` when a circuit is open:

```rust
// In FallbackProvider::stream():
if let Some(cb) = self.circuit_breakers.get(i) {
    if !cb.is_available().await {
        last_error = Some(ProviderError::CircuitOpen(provider.name().to_string()));
        continue;
    }
    // Call provider...
    // Record success/failure
}
```

`ProviderError::CircuitOpen` is mapped to HTTP 502 Bad Gateway in the error module's `IntoResponse` implementation.

### State Transitions

- **Closed**: Requests pass through. On `failure_threshold` failures, transition to Open.
- **Open**: Requests fail immediately with `CircuitError::Open`. After `timeout_secs`, transition to HalfOpen.
- **HalfOpen**: Allow one request. If success, transition to Closed. If failure, transition to Open.