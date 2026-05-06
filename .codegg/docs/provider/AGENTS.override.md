# Provider Module Override

This file contains provider-specific guidance and overrides root AGENTS.md.

## Token Estimation

Token estimation uses `TokenizerType` enum with model-specific multipliers:

- Claude models: 1.4x multiplier
- Gemini models: 1.2x multiplier
- OpenAI models: 1.0x (cl100k_base)

Use `TokenizerType::for_model(model_name)` to detect the type.

## Known Issues

### debug_log! Macro Performance Bug (HIGH)
**File:** `src/provider/mod.rs:7-17`

The `debug_log!` macro opens a file handle on every invocation, causing severe performance degradation. Each call performs blocking file I/O.

**Recommendation:** Replace with `tracing::debug!` or implement a buffered writer using `std::sync::OnceLock`.

### Missing Exponential Backoff (HIGH)
**File:** `src/provider/fallback.rs`

Documentation claims "Exponential backoff with MAX_RETRY_DELAY of 30s cap" but no such implementation exists. Providers are hammered immediately on rate limit errors without any delay.

**Current behavior:** Immediate retry with no delay between providers.

**Recommendation:** Implement exponential backoff with jitter:
```rust
let delay = std::time::Duration::from_secs(30).min(base_delay * 2^i);
tokio::time::sleep(delay).await;
```