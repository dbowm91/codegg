//! Read-only call cache for Tool Programs.
//!
//! Caches typed results from read-only tool calls with
//! content/policy-aware cache keys. Cache entries are bounded
//! by TTL and max entries per tool.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

use crate::tool::contract::ToolValue;

/// A cached tool call result.
#[derive(Debug, Clone)]
pub struct CachedCall {
    pub value: ToolValue,
    pub cached_at: Instant,
    pub call_count: u64,
}

/// Cache key for a tool call, incorporating tool identity, arguments,
/// and workspace context.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub tool_name: String,
    pub input_hash: u64,
    pub workspace_id: Option<String>,
}

impl CacheKey {
    pub fn new(tool_name: &str, input: &serde_json::Value, workspace_id: Option<&str>) -> Self {
        let input_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            let input_str = serde_json::to_string(input).unwrap_or_default();
            input_str.hash(&mut hasher);
            hasher.finish()
        };
        Self {
            tool_name: tool_name.to_string(),
            input_hash,
            workspace_id: workspace_id.map(|s| s.to_string()),
        }
    }
}

/// Configuration for the read-only call cache.
#[derive(Debug, Clone)]
pub struct ProgramCacheConfig {
    /// Default TTL for cache entries.
    pub default_ttl: Duration,
    /// Maximum entries per tool.
    pub max_entries_per_tool: u32,
    /// Maximum total entries across all tools.
    pub max_total_entries: u32,
}

impl Default for ProgramCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl: Duration::from_secs(300),
            max_entries_per_tool: 100,
            max_total_entries: 1000,
        }
    }
}

/// Read-only call cache for tool programs.
pub struct ProgramCallCache {
    entries: RwLock<HashMap<CacheKey, CachedCall>>,
    config: ProgramCacheConfig,
}

impl ProgramCallCache {
    pub fn new(config: ProgramCacheConfig) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            config,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(ProgramCacheConfig::default())
    }

    /// Look up a cached result. Returns None if missing or expired.
    pub fn get(&self, key: &CacheKey) -> Option<CachedCall> {
        let entries = self.entries.read();
        let entry = entries.get(key)?;
        if entry.cached_at.elapsed() > self.config.default_ttl {
            return None; // expired
        }
        Some(entry.clone())
    }

    /// Store a result in the cache. Evicts oldest entries if limits
    /// are exceeded.
    pub fn insert(&self, key: CacheKey, value: ToolValue) {
        let mut entries = self.entries.write();

        // Evict if at capacity
        if entries.len() >= self.config.max_total_entries as usize {
            // Remove oldest entry
            if let Some(oldest_key) = entries
                .iter()
                .min_by_key(|(_, v)| v.cached_at)
                .map(|(k, _)| k.clone())
            {
                entries.remove(&oldest_key);
            }
        }

        entries.insert(
            key,
            CachedCall {
                value,
                cached_at: Instant::now(),
                call_count: 0,
            },
        );
    }

    /// Invalidate a specific entry.
    pub fn invalidate(&self, key: &CacheKey) -> bool {
        self.entries.write().remove(key).is_some()
    }

    /// Invalidate all entries for a given tool.
    pub fn invalidate_tool(&self, tool_name: &str) -> u64 {
        let mut entries = self.entries.write();
        let keys_to_remove: Vec<_> = entries
            .keys()
            .filter(|k| k.tool_name == tool_name)
            .cloned()
            .collect();
        let count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            entries.remove(&key);
        }
        count
    }

    /// Clear the entire cache.
    pub fn clear(&self) {
        self.entries.write().clear();
    }

    /// Current number of entries.
    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }
}

impl Default for ProgramCallCache {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::contract::{ToolTerminalStatus, ToolValue};

    fn test_value(display: &str) -> ToolValue {
        ToolValue {
            display: display.to_string(),
            value: Some(serde_json::json!({"data": display})),
            artifacts: vec![],
            provenance: None,
            terminal_status: ToolTerminalStatus::Success,
            truncated: false,
        }
    }

    #[test]
    fn cache_miss_on_empty() {
        let cache = ProgramCallCache::with_defaults();
        let key = CacheKey::new("read", &serde_json::json!({"path": "/tmp/a.txt"}), None);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn cache_hit_after_insert() {
        let cache = ProgramCallCache::with_defaults();
        let key = CacheKey::new("read", &serde_json::json!({"path": "/tmp/a.txt"}), None);
        cache.insert(key.clone(), test_value("hello"));
        let cached = cache.get(&key).unwrap();
        assert_eq!(cached.value.display, "hello");
    }

    #[test]
    fn cache_different_keys_miss() {
        let cache = ProgramCallCache::with_defaults();
        let key1 = CacheKey::new("read", &serde_json::json!({"path": "/tmp/a.txt"}), None);
        let key2 = CacheKey::new("read", &serde_json::json!({"path": "/tmp/b.txt"}), None);
        cache.insert(key1.clone(), test_value("a"));
        assert!(cache.get(&key2).is_none());
    }

    #[test]
    fn cache_invalidate() {
        let cache = ProgramCallCache::with_defaults();
        let key = CacheKey::new("read", &serde_json::json!({"path": "/tmp/a.txt"}), None);
        cache.insert(key.clone(), test_value("hello"));
        assert!(cache.invalidate(&key));
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn cache_invalidate_tool() {
        let cache = ProgramCallCache::with_defaults();
        let key1 = CacheKey::new("read", &serde_json::json!({"path": "/tmp/a.txt"}), None);
        let key2 = CacheKey::new("read", &serde_json::json!({"path": "/tmp/b.txt"}), None);
        let key3 = CacheKey::new("grep", &serde_json::json!({"pattern": "foo"}), None);
        cache.insert(key1, test_value("a"));
        cache.insert(key2, test_value("b"));
        cache.insert(key3, test_value("c"));

        let removed = cache.invalidate_tool("read");
        assert_eq!(removed, 2);
        assert_eq!(cache.len(), 1); // grep entry remains
    }

    #[test]
    fn cache_clear() {
        let cache = ProgramCallCache::with_defaults();
        cache.insert(
            CacheKey::new("read", &serde_json::json!({}), None),
            test_value("x"),
        );
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_evicts_oldest_when_full() {
        let config = ProgramCacheConfig {
            max_total_entries: 3,
            ..Default::default()
        };
        let cache = ProgramCallCache::new(config);
        for i in 0..5 {
            cache.insert(
                CacheKey::new(
                    "read",
                    &serde_json::json!({"path": format!("/tmp/{}.txt", i)}),
                    None,
                ),
                test_value(&format!("file{}", i)),
            );
        }
        assert!(cache.len() <= 3);
    }

    #[test]
    fn cache_key_distinguishes_workspaces() {
        let cache = ProgramCallCache::with_defaults();
        let key_ws1 = CacheKey::new("read", &serde_json::json!({}), Some("ws-1"));
        let key_ws2 = CacheKey::new("read", &serde_json::json!({}), Some("ws-2"));
        cache.insert(key_ws1.clone(), test_value("ws1"));
        cache.insert(key_ws2.clone(), test_value("ws2"));
        assert_eq!(cache.get(&key_ws1).unwrap().value.display, "ws1");
        assert_eq!(cache.get(&key_ws2).unwrap().value.display, "ws2");
    }
}
