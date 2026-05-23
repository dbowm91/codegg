---
name: provider-caching
description: LLM Response Caching Guide
version: 1.1.0
tags: [provider, caching, performance]
---

# Provider Caching Guide

This skill covers the provider response caching system in opencode-rs.

## Architecture

The `ProviderCache` in `src/provider/cache.rs` provides:
- **Concurrent storage**: Uses `DashMap` for thread-safe access
- **Cache key**: `(provider_name, model, input_hash)`
- **Cache value**: `(response_text, timestamp_secs, ttl_secs)`

## Implementation

```rust
use dashmap::DashMap;

struct CacheEntry {
    response_text: String,
    timestamp_secs: u64,
    ttl_secs: u64,
}

pub struct ProviderCache {
    cache: DashMap<(String, String, u64), CacheEntry>,
}

impl ProviderCache {
    pub fn new() -> Self;
    pub fn get(&self, provider: &str, model: &str, input: &str) -> Option<String>;
    pub fn put(&self, provider: &str, model: &str, input: &str, response: &str, ttl_secs: u64);
    pub fn clear(&self);  // Removes expired entries
}
```

## Current Status

**Note**: `ProviderCache` is implemented in `src/provider/cache.rs` but is **not yet wired into the provider system**. The cache struct exists and is functional, but there's no automatic caching integration in `src/provider/mod.rs`.

To enable caching, choose one of these approaches:

### Option A - Wrap existing providers

Add cache field to provider implementations:
```rust
impl OpenAiProvider {
    pub fn new(config: OpenAiCompatibleConfig, cache: Option<Arc<ProviderCache>>) -> Self {
        Self { config, cache }
    }
}
```

### Option B - Middleware pattern (Recommended)

Create `CachingProvider` wrapper that adds caching layer:
```rust
pub struct CachingProvider {
    inner: Box<dyn Provider>,
    cache: Arc<ProviderCache>,
}

impl CachingProvider {
    pub fn new(inner: Box<dyn Provider>, cache: Arc<ProviderCache>) -> Self {
        Self { inner, cache }
    }
}

#[async_trait]
impl Provider for CachingProvider {
    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let cache_key = self.generate_cache_key(request);

        // Check cache first
        if let Some(cached) = self.cache.get(self.id(), &request.model, &cache_key) {
            return Ok(EventStream::from_text(cached));
        }

        // Call provider and cache result
        let stream = self.inner.stream(request).await?;
        // ... collect and cache response
    }
}
```

### Option C - Per-request caching

Direct calls to cache before/after provider requests:
```rust
let cache_key = cache_key(request);
if let Some(cached) = cache.get(provider_id, &model, &cache_key) {
    return cached;
}
let response = provider.stream(request).await?;
cache.put(provider_id, &model, &cache_key, &response, ttl);
```

## Configuration

To enable in config schema (`src/config/schema.rs`):
```rust
pub struct ProviderConfig {
    pub cache_enabled: Option<bool>,      // default: false
    pub cache_ttl_seconds: Option<u64>,   // default: 300
}
```

## Key Differences from Original Design

- Uses `std::hash::Hasher` (DefaultHasher) instead of `fxhash::hash64`
- No `lazy_static!` - `ProviderCache` uses `DashMap` which handles concurrent access
- `clear()` is a self-contained method that removes expired entries

## Files

| File | Purpose |
|------|---------|
| `src/provider/cache.rs` | `ProviderCache` struct with get/put/clear methods |
| `src/provider/mod.rs` | Provider trait and registry - cache integration needed here |