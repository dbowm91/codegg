# Error Module Override

This file contains error-specific guidance and overrides root AGENTS.md.

## ProviderError::is_retryable()

Both `ProviderError` and `ToolError` have `is_retryable()` methods for determining if an error should trigger retry logic:

```rust
impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimit
                | ProviderError::Timeout(_)
                | ProviderError::Stream(_)
                | ProviderError::CircuitOpen(_)
        )
    }
}

impl ToolError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_)
        )
    }
}
```

The agent loop uses `ProviderError::is_retryable()` for provider error retry logic (src/agent/loop.rs:808-813).

## CircuitError to ProviderError Conversion

`CircuitBreaker::call()` returns `CircuitError::Open(name)` which automatically converts to `ProviderError::CircuitOpen(name)` via the `From` trait (error.rs:198-206):

```rust
impl From<crate::resilience::circuit::CircuitError> for ProviderError {
    fn from(e: CircuitError) -> Self {
        match e {
            CircuitError::Open(name) => ProviderError::CircuitOpen(name),
        }
    }
}
```

This allows the circuit breaker to propagate circuit-open errors properly through the fallback provider.

## FallbackProvider Circuit Integration

`FallbackProvider` in `src/provider/fallback.rs` creates a `CircuitBreaker` for each provider and uses `ProviderError::CircuitOpen` when a circuit is open:

```rust
if !cb.is_available().await {
    last_error = Some(ProviderError::CircuitOpen(provider.name().to_string()));
    continue;
}
```

The circuit breaker errors map to HTTP 502 Bad Gateway in the `IntoResponse` implementation.

## Session Not Found Errors

Session not found errors should use `StorageError::NotFound` rather than `AppError::Other`:

```rust
// Correct:
.ok_or_else(|| AppError::Storage(StorageError::NotFound(format!("session {}", id))))?;

// Avoid:
.ok_or_else(|| AppError::Other(anyhow::anyhow!("Session not found: {}", id)))?;
```

## AppError HTTP Status Mapping

The `IntoResponse` impl for `AppError` (error.rs:189-263) provides HTTP status codes for all error variants:
- `ConfigError::NotFound` → 404
- `ConfigError::Invalid/Parse/Merge` → 400
- `StorageError::NotFound` → 404
- `ProviderError::Auth` → 401
- `ProviderError::RateLimit` → 429
- `ProviderError::Timeout` → 504
- `ProviderError::CircuitOpen` → 502
- `ToolError::Permission` / `PermissionError::Denied` → 403