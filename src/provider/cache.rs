use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;

type CacheKey = (String, String, u64);

struct CacheEntry {
    response_text: String,
    timestamp_secs: u64,
    ttl_secs: u64,
}

pub struct ProviderCache {
    cache: DashMap<CacheKey, CacheEntry>,
}

impl Default for ProviderCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderCache {
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
        }
    }

    fn hash_input(input: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        input.hash(&mut hasher);
        hasher.finish()
    }

    pub fn get(&self, provider: &str, model: &str, input: &str) -> Option<String> {
        let input_hash = Self::hash_input(input);
        let key = (provider.to_string(), model.to_string(), input_hash);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if let Some(entry) = self.cache.get(&key) {
            if now - entry.timestamp_secs < entry.ttl_secs {
                return Some(entry.response_text.clone());
            } else {
                drop(entry);
                self.cache.remove(&key);
            }
        }
        None
    }

    pub fn put(&self, provider: &str, model: &str, input: &str, response: &str, ttl_secs: u64) {
        let input_hash = Self::hash_input(input);
        let key = (provider.to_string(), model.to_string(), input_hash);
        let timestamp_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.cache.insert(
            key,
            CacheEntry {
                response_text: response.to_string(),
                timestamp_secs,
                ttl_secs,
            },
        );
    }

    pub fn evict_expired(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.cache
            .retain(|_, entry| now - entry.timestamp_secs < entry.ttl_secs);
    }
}
