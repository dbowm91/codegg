# Skill: provider-caching

# LLM Response Caching Guide

This skill covers the provider response caching system in opencode-rs.

## Architecture#

The `ProviderCache` in `src/provider/cache.rs` provides:
- **Concurrent storage**: Uses `DashMap` for thread-safe access
- **Cache key**: `(provider_name, model, input_hash)`
- **Cache value**: `(response_text, timestamp_secs, ttl_secs)`

## Implementation#

```rust
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

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

    fn hash_input(input: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        input.hash(&mut hasher);
        hasher.finish()
    }

    pub fn get(&self, provider: &str, model: &str, input: &str) -> Option<String>;

    pub fn put(&self, provider: &str, model: &str, input: &str, response: &str, ttl_secs: u64);

    pub fn clear(&self) {
        // Removes expired entries
        self.cache.retain(|_, entry| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            now - entry.timestamp_secs < entry.ttl_secs
        });
    }
}
```

## Current Status#

**Note**: `ProviderCache` is implemented in `src/provider/cache.rs` but is **not yet wired into the provider system**. The cache struct exists and is functional, but there's no `with_cache` wrapper or automatic caching integration in `src/provider/mod.rs`.

To enable caching:
1. Add cache field to provider implementations or wrap provider calls
2. Add config options `cache_enabled: Option<bool>` and `cache_ttl_seconds: Option<u64>`
3. Call `provider_cache.get()` before provider request and `provider_cache.put()` after

## Configuration#

Add to config schema (`src/config/schema.rs`):
```rust
pub struct ProviderConfig {
    pub cache_enabled: Option<bool>,
    pub cache_ttl_seconds: Option<u64>,
}
```

## Key Differences from Original Design#

- Uses `std::hash::Hasher` (DefaultHasher) instead of `fxhash::hash64`
- No `lazy_static!` - `ProviderCache` uses `DashMap` which handles concurrent access
- `clear()` is a self-contained method that removes expired entries

Base directory for this skill: file:///home/sugarwookie/projects/coder/.opencode/skills/caching
