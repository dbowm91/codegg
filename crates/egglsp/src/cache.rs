//! Bounded in-memory semantic context cache (Phase 12).
//!
//! Provides TTL-aware, generation-aware caching of [`LspContextPacket`]
//! results keyed by workspace, server, operation, request fingerprint,
//! file content hashes, capability state, and budget. Entries are
//! evicted by count and byte budget (LRU within constraints).

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::context::{LspContextBudget, LspContextPacket, LspContextRequest, LspEvidenceFreshness};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the semantic context cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspCacheConfig {
    /// Storage mode (disabled or memory-only).
    pub mode: LspCacheMode,
    /// Maximum number of cache entries.
    pub max_entries: usize,
    /// Maximum total bytes across all entries (estimated via serialization).
    pub max_bytes: usize,
    /// Time-to-live in seconds for cache entries.
    pub ttl_seconds: u64,
}

impl Default for LspCacheConfig {
    fn default() -> Self {
        Self {
            mode: LspCacheMode::default(),
            max_entries: 64,
            max_bytes: 4 * 1024 * 1024,
            ttl_seconds: 300,
        }
    }
}

// ---------------------------------------------------------------------------
// Storage mode
// ---------------------------------------------------------------------------

/// Storage mode for the semantic context cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspCacheMode {
    /// Caching is disabled.
    #[default]
    Disabled,
    /// Entries are stored in memory only.
    Memory,
}

// ---------------------------------------------------------------------------
// Cache key
// ---------------------------------------------------------------------------

/// A cache key encoding all inputs that determine whether a cached
/// result is reusable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspCacheKey {
    workspace_root: PathBuf,
    server_id: String,
    operation: String,
    request_fingerprint: String,
    input_hashes: BTreeMap<PathBuf, String>,
    capability_fingerprint: Option<String>,
    budget_fingerprint: String,
    /// Pre-computed hash of all fields for fast Eq/Hash.
    #[serde(skip)]
    precomputed_hash: u64,
}

impl LspCacheKey {
    /// Create a new key with a precomputed hash.
    fn new(
        workspace_root: PathBuf,
        server_id: String,
        operation: String,
        request_fingerprint: String,
        input_hashes: BTreeMap<PathBuf, String>,
        capability_fingerprint: Option<String>,
        budget_fingerprint: String,
    ) -> Self {
        let precomputed_hash = Self::compute_hash(
            &workspace_root,
            &server_id,
            &operation,
            &request_fingerprint,
            &input_hashes,
            &capability_fingerprint,
            &budget_fingerprint,
        );
        Self {
            workspace_root,
            server_id,
            operation,
            request_fingerprint,
            input_hashes,
            capability_fingerprint,
            budget_fingerprint,
            precomputed_hash,
        }
    }

    fn compute_hash(
        workspace_root: &Path,
        server_id: &str,
        operation: &str,
        request_fingerprint: &str,
        input_hashes: &BTreeMap<PathBuf, String>,
        capability_fingerprint: &Option<String>,
        budget_fingerprint: &str,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();
        workspace_root.hash(&mut hasher);
        server_id.hash(&mut hasher);
        operation.hash(&mut hasher);
        request_fingerprint.hash(&mut hasher);
        for (path, hash) in input_hashes {
            path.hash(&mut hasher);
            hash.hash(&mut hasher);
        }
        capability_fingerprint.hash(&mut hasher);
        budget_fingerprint.hash(&mut hasher);
        hasher.finish()
    }

    /// Workspace root this key targets.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// File content hashes captured at key creation time.
    pub fn input_hashes(&self) -> &BTreeMap<PathBuf, String> {
        &self.input_hashes
    }
}

impl Hash for LspCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.precomputed_hash.hash(state);
    }
}

impl PartialEq for LspCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.precomputed_hash == other.precomputed_hash
            && self.workspace_root == other.workspace_root
            && self.server_id == other.server_id
            && self.operation == other.operation
            && self.request_fingerprint == other.request_fingerprint
            && self.input_hashes == other.input_hashes
            && self.capability_fingerprint == other.capability_fingerprint
            && self.budget_fingerprint == other.budget_fingerprint
    }
}

impl Eq for LspCacheKey {}

// ---------------------------------------------------------------------------
// Cache key builder
// ---------------------------------------------------------------------------

/// Builder for constructing [`LspCacheKey`] instances.
pub struct LspCacheKeyBuilder {
    workspace_root: PathBuf,
    server_id: String,
    operation: String,
    request_fingerprint: String,
    input_hashes: BTreeMap<PathBuf, String>,
    capability_fingerprint: Option<String>,
    budget_fingerprint: String,
}

impl LspCacheKeyBuilder {
    /// Create a new builder with the required fields.
    pub fn new(
        workspace_root: impl Into<PathBuf>,
        server_id: impl Into<String>,
        operation: impl Into<String>,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            server_id: server_id.into(),
            operation: operation.into(),
            request_fingerprint: String::new(),
            input_hashes: BTreeMap::new(),
            capability_fingerprint: None,
            budget_fingerprint: String::new(),
        }
    }

    /// Set the request fingerprint from an [`LspContextRequest`].
    ///
    /// Serializes the request to JSON for deterministic fingerprinting.
    pub fn with_request(mut self, request: &LspContextRequest) -> Self {
        self.request_fingerprint =
            serde_json::to_string(request).unwrap_or_else(|_| "unserializable".into());
        self
    }

    /// Add a file content hash.
    pub fn with_file_hash(mut self, path: impl Into<PathBuf>, hash: impl Into<String>) -> Self {
        self.input_hashes.insert(path.into(), hash.into());
        self
    }

    /// Set the capability fingerprint.
    pub fn with_capability_fingerprint(mut self, fp: impl Into<String>) -> Self {
        self.capability_fingerprint = Some(fp.into());
        self
    }

    /// Set the budget fingerprint from an [`LspContextBudget`].
    pub fn with_budget(mut self, budget: &LspContextBudget) -> Self {
        self.budget_fingerprint =
            serde_json::to_string(budget).unwrap_or_else(|_| "unserializable".into());
        self
    }

    /// Build the final [`LspCacheKey`].
    pub fn build(self) -> LspCacheKey {
        LspCacheKey::new(
            self.workspace_root,
            self.server_id,
            self.operation,
            self.request_fingerprint,
            self.input_hashes,
            self.capability_fingerprint,
            self.budget_fingerprint,
        )
    }
}

// ---------------------------------------------------------------------------
// Cache entry
// ---------------------------------------------------------------------------

/// A single cache entry with metadata for TTL and eviction.
pub struct LspCacheEntry {
    /// The cached context packet.
    pub packet: LspContextPacket,
    /// When this entry was created.
    pub created_at: Instant,
    /// When this entry was last accessed.
    pub last_used: Instant,
    /// The key that produced this entry.
    pub key: LspCacheKey,
    /// Number of times this entry has been accessed.
    pub hit_count: usize,
    /// Freshness of the evidence at insertion time.
    pub original_freshness: LspEvidenceFreshness,
    /// Server generation at time of collection.
    pub server_generation_at_collect: Option<u64>,
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Cache statistics snapshot.
#[derive(Debug, Clone, Default)]
pub struct LspCacheStats {
    /// Current number of entries.
    pub entries: usize,
    /// Estimated total bytes across all entries.
    pub bytes: usize,
    /// Total cache hits.
    pub hits: usize,
    /// Total cache misses (not found or stale).
    pub misses: usize,
    /// Misses due to stale file hashes or TTL expiry.
    pub stale_misses: usize,
    /// Total evictions (by count or byte budget).
    pub evictions: usize,
}

// ---------------------------------------------------------------------------
// Internal stats
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct CacheStatsInternal {
    hits: usize,
    misses: usize,
    stale_misses: usize,
    evictions: usize,
}

// ---------------------------------------------------------------------------
// Freshness determination
// ---------------------------------------------------------------------------

/// Determine the freshness of a cache entry given the current state.
///
/// Returns [`LspEvidenceFreshness::Unknown`] when the entry should be
/// treated as a miss (file hash mismatch or TTL expired).
pub fn cache_hit_freshness(
    entry: &LspCacheEntry,
    current_server_generation: Option<u64>,
    file_hashes: &BTreeMap<PathBuf, String>,
) -> LspEvidenceFreshness {
    // Check file hash freshness: if any file hash differs, the entry is stale.
    for (path, expected_hash) in &entry.key.input_hashes {
        match file_hashes.get(path) {
            Some(actual_hash) if actual_hash == expected_hash => {}
            _ => {
                return LspEvidenceFreshness::Unknown;
            }
        }
    }

    // Check TTL.
    let ttl = std::time::Duration::from_secs(300); // default; actual TTL checked in get()
    if entry.created_at.elapsed() > ttl {
        return LspEvidenceFreshness::Unknown;
    }

    // Check server generation.
    if let (Some(current_gen), Some(entry_gen)) = (
        current_server_generation,
        entry.server_generation_at_collect,
    ) {
        if current_gen != entry_gen {
            return LspEvidenceFreshness::RetainedAfterRestart;
        }
    }

    // Preserve original freshness.
    entry.original_freshness
}

// ---------------------------------------------------------------------------
// Semantic cache
// ---------------------------------------------------------------------------

/// Bounded in-memory semantic context cache.
///
/// Caches [`LspContextPacket`] results keyed by workspace, server,
/// operation, request fingerprint, file content hashes, capability
/// state, and budget. Supports TTL-based expiry, file-hash invalidation,
/// server generation tracking, and LRU-style eviction.
pub struct LspSemanticCache {
    config: LspCacheConfig,
    entries: HashMap<LspCacheKey, LspCacheEntry>,
    order: VecDeque<LspCacheKey>,
    stats: CacheStatsInternal,
}

impl LspSemanticCache {
    /// Create a new cache with the given configuration.
    pub fn new(config: LspCacheConfig) -> Self {
        Self {
            config,
            entries: HashMap::new(),
            order: VecDeque::new(),
            stats: CacheStatsInternal::default(),
        }
    }

    /// Look up a cached packet by key.
    ///
    /// Returns `None` if the entry is missing, TTL-expired, has stale
    /// file hashes, or the server generation has changed (in which case
    /// the stale entry is removed).
    pub fn get(
        &mut self,
        key: &LspCacheKey,
        current_server_generation: Option<u64>,
        file_hashes: &BTreeMap<PathBuf, String>,
    ) -> Option<&LspContextPacket> {
        if self.config.mode != LspCacheMode::Memory {
            return None;
        }

        // Phase 1: gather validity data under a short-lived borrow.
        let validity = {
            let entry = self.entries.get(key)?;
            let ttl = std::time::Duration::from_secs(self.config.ttl_seconds);
            let ttl_expired = entry.created_at.elapsed() > ttl;
            let stale_files: Vec<PathBuf> = entry
                .key
                .input_hashes
                .iter()
                .filter(|(path, expected)| {
                    !file_hashes
                        .get(*path)
                        .is_some_and(|actual| actual == *expected)
                })
                .map(|(path, _)| path.clone())
                .collect();
            let gen_mismatch = match (
                current_server_generation,
                entry.server_generation_at_collect,
            ) {
                (Some(cur), Some(cached)) => cur != cached,
                _ => false,
            };
            (ttl_expired, stale_files, gen_mismatch)
        };
        // `self` is fully available again after the block above.

        // Phase 2: act on the validity data.
        if validity.0 {
            tracing::debug!("cache miss: TTL expired for key {:?}", key);
            self.stats.misses += 1;
            self.stats.stale_misses += 1;
            self.remove_by_key(key);
            return None;
        }

        if let Some(path) = validity.1.first() {
            tracing::debug!("cache miss: file hash mismatch for {}", path.display());
            self.stats.misses += 1;
            self.stats.stale_misses += 1;
            self.remove_by_key(key);
            return None;
        }

        if validity.2 {
            tracing::debug!("cache miss: server generation mismatch");
            self.stats.misses += 1;
            self.stats.stale_misses += 1;
            self.remove_by_key(key);
            return None;
        }

        // Phase 3: cache hit — update metadata and return.
        let entry = self.entries.get_mut(key).unwrap();
        entry.last_used = Instant::now();
        entry.hit_count += 1;

        // Move to back of order (most recently used).
        self.order.retain(|k| k != key);
        self.order.push_back(key.clone());

        self.stats.hits += 1;

        tracing::debug!(
            "cache hit for {:?} ({} items)",
            key.operation,
            entry.packet.items.len()
        );
        Some(&entry.packet)
    }

    /// Insert a packet into the cache.
    pub fn insert(
        &mut self,
        key: LspCacheKey,
        packet: LspContextPacket,
        original_freshness: LspEvidenceFreshness,
        server_generation: Option<u64>,
    ) {
        if self.config.mode != LspCacheMode::Memory {
            return;
        }

        let now = Instant::now();
        let entry = LspCacheEntry {
            packet,
            created_at: now,
            last_used: now,
            hit_count: 0,
            original_freshness,
            server_generation_at_collect: server_generation,
            key: key.clone(),
        };

        // If key already exists, remove old entry first.
        if self.entries.contains_key(&key) {
            self.remove_by_key(&key);
        }

        self.entries.insert(key.clone(), entry);
        self.order.push_back(key);

        self.evict_if_needed();
    }

    /// Remove a specific entry. Returns `true` if the entry existed.
    pub fn remove(&mut self, key: &LspCacheKey) -> bool {
        self.remove_by_key(key)
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        let count = self.entries.len();
        self.entries.clear();
        self.order.clear();
        tracing::debug!("cache cleared: {} entries removed", count);
    }

    /// Clear all entries for a given workspace root.
    ///
    /// Returns the number of entries removed.
    pub fn clear_for_root(&mut self, root: &Path) -> usize {
        let keys_to_remove: Vec<LspCacheKey> = self
            .entries
            .keys()
            .filter(|k| k.workspace_root() == root)
            .cloned()
            .collect();

        let count = keys_to_remove.len();
        for key in &keys_to_remove {
            self.remove_by_key(key);
        }

        tracing::debug!(
            "cache cleared for root {}: {} entries removed",
            root.display(),
            count
        );
        count
    }

    /// Return a snapshot of current cache statistics.
    pub fn stats(&self) -> LspCacheStats {
        let bytes: usize = self
            .entries
            .values()
            .map(|e| Self::estimate_entry_bytes(&e.packet))
            .sum();

        LspCacheStats {
            entries: self.entries.len(),
            bytes,
            hits: self.stats.hits,
            misses: self.stats.misses,
            stale_misses: self.stats.stale_misses,
            evictions: self.stats.evictions,
        }
    }

    /// Returns `true` if the cache is enabled (Memory mode).
    pub fn is_enabled(&self) -> bool {
        self.config.mode == LspCacheMode::Memory
    }

    // -- Private helpers ---------------------------------------------------

    /// Remove an entry by key. Returns `true` if it existed.
    fn remove_by_key(&mut self, key: &LspCacheKey) -> bool {
        let removed = self.entries.remove(key).is_some();
        if removed {
            self.order.retain(|k| k != key);
        }
        removed
    }

    /// Enforce max_entries and max_bytes by evicting oldest entries.
    fn evict_if_needed(&mut self) {
        // Evict by count.
        while self.entries.len() > self.config.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                if self.entries.remove(&oldest).is_some() {
                    self.stats.evictions += 1;
                    tracing::debug!("cache eviction (count limit): removed entry");
                }
            } else {
                break;
            }
        }

        // Evict by bytes.
        let total_bytes: usize = self
            .entries
            .values()
            .map(|e| Self::estimate_entry_bytes(&e.packet))
            .sum();

        if total_bytes > self.config.max_bytes {
            let mut current_bytes = total_bytes;
            while current_bytes > self.config.max_bytes {
                if let Some(oldest) = self.order.pop_front() {
                    if let Some(entry) = self.entries.remove(&oldest) {
                        let entry_bytes = Self::estimate_entry_bytes(&entry.packet);
                        current_bytes = current_bytes.saturating_sub(entry_bytes);
                        self.stats.evictions += 1;
                        tracing::debug!(
                            "cache eviction (bytes limit): removed entry (~{} bytes)",
                            entry_bytes
                        );
                    }
                } else {
                    break;
                }
            }
        }
    }

    /// Approximate the size of a packet in bytes via serialization.
    fn estimate_entry_bytes(packet: &LspContextPacket) -> usize {
        serde_json::to_vec(packet).map(|v| v.len()).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{
        LineRange, LspContextItem, LspContextItemKind, LspContextPacketMode, LspContextScore,
        LspEvidenceProvenance,
    };

    fn make_config_enabled(
        max_entries: usize,
        max_bytes: usize,
        ttl_seconds: u64,
    ) -> LspCacheConfig {
        LspCacheConfig {
            mode: LspCacheMode::Memory,
            max_entries,
            max_bytes,
            ttl_seconds,
        }
    }

    fn make_packet(operation: &str) -> LspContextPacket {
        LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("src/lib.rs")],
                hunks: Vec::new(),
                risk_mode: crate::context::LspRiskMode::default(),
            },
            items: vec![LspContextItem {
                kind: LspContextItemKind::Diagnostic,
                file: PathBuf::from("src/lib.rs"),
                range: Some(LineRange { start: 0, end: 10 }),
                line: Some(0),
                column: None,
                message: format!("{operation} diagnostic"),
                symbol: None,
                source: None,
                provenance: LspEvidenceProvenance {
                    server_id: "rust-analyzer".to_string(),
                    server_generation: Some(1),
                    operation: operation.to_string(),
                    freshness: LspEvidenceFreshness::Fresh,
                    capability_decision: None,
                    document_version: None,
                    age_ms: None,
                    post_restart: false,
                },
                score: LspContextScore {
                    priority: 10,
                    is_hunk_local: false,
                    is_error: false,
                    is_same_file: false,
                    freshness_rank: 0,
                },
                payload: None,
            }],
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::default(),
            workspace_root: Some(PathBuf::from("/workspace")),
            generated_at: None,
            server_id: Some("rust-analyzer".to_string()),
            server_generation: Some(1),
            operational_state: Some("ready".to_string()),
            budget: None,
            notes: Vec::new(),
            truncation: crate::context::LspContextTruncation::default(),
        }
    }

    fn make_key(operation: &str) -> LspCacheKey {
        LspCacheKeyBuilder::new("/workspace", "rust-analyzer", operation).build()
    }

    #[test]
    fn test_cache_disabled_by_default() {
        let config = LspCacheConfig::default();
        assert_eq!(config.mode, LspCacheMode::Disabled);
        let cache = LspSemanticCache::new(config);
        assert!(!cache.is_enabled());
    }

    #[test]
    fn test_cache_insert_and_get() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key = make_key("review");
        let packet = make_packet("review");
        let file_hashes = BTreeMap::new();

        cache.insert(key.clone(), packet, LspEvidenceFreshness::Fresh, Some(1));

        let result = cache.get(&key, Some(1), &file_hashes);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].message, "review diagnostic");

        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.hits, 1);
    }

    #[test]
    fn test_cache_eviction_by_count() {
        let config = make_config_enabled(3, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        for i in 0..5 {
            let key = LspCacheKeyBuilder::new("/ws", "server", format!("op_{i}")).build();
            let packet = make_packet(&format!("op_{i}"));
            cache.insert(key, packet, LspEvidenceFreshness::Fresh, Some(1));
        }

        let stats = cache.stats();
        assert_eq!(stats.entries, 3);
        assert!(stats.evictions >= 2);

        // First two keys should have been evicted.
        let key0 = LspCacheKeyBuilder::new("/ws", "server", "op_0").build();
        let key1 = LspCacheKeyBuilder::new("/ws", "server", "op_1").build();
        let file_hashes = BTreeMap::new();
        assert!(cache.get(&key0, Some(1), &file_hashes).is_none());
        assert!(cache.get(&key1, Some(1), &file_hashes).is_none());
    }

    #[test]
    fn test_cache_eviction_by_bytes() {
        // Tiny byte budget to force eviction.
        let config = make_config_enabled(64, 100, 300);
        let mut cache = LspSemanticCache::new(config);

        for i in 0..5 {
            let key = LspCacheKeyBuilder::new("/ws", "server", format!("op_{i}")).build();
            let packet = make_packet(&format!("op_{i}"));
            cache.insert(key, packet, LspEvidenceFreshness::Fresh, Some(1));
        }

        let stats = cache.stats();
        assert!(
            stats.bytes <= 200,
            "bytes should be bounded, got {}",
            stats.bytes
        );
        assert!(stats.evictions >= 1);
    }

    #[test]
    fn test_cache_ttl_expiry() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 0); // 0-second TTL = always expired
        let mut cache = LspSemanticCache::new(config);

        let key = make_key("review");
        let packet = make_packet("review");
        let file_hashes = BTreeMap::new();

        cache.insert(key.clone(), packet, LspEvidenceFreshness::Fresh, Some(1));

        // Immediately after insert, TTL (0s) is already expired.
        let result = cache.get(&key, Some(1), &file_hashes);
        assert!(result.is_none(), "entry should be expired");

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.stale_misses, 1);
    }

    #[test]
    fn test_cache_file_hash_invalidation() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let mut key_hashes = BTreeMap::new();
        key_hashes.insert(PathBuf::from("src/lib.rs"), "hash_v1".to_string());

        let key = LspCacheKeyBuilder::new("/workspace", "rust-analyzer", "review")
            .with_file_hash("src/lib.rs", "hash_v1")
            .build();
        let packet = make_packet("review");
        cache.insert(key.clone(), packet, LspEvidenceFreshness::Fresh, Some(1));

        // Same hash -> hit.
        let mut current_hashes = BTreeMap::new();
        current_hashes.insert(PathBuf::from("src/lib.rs"), "hash_v1".to_string());
        assert!(cache.get(&key, Some(1), &current_hashes).is_some());

        // Different hash -> miss.
        let mut changed_hashes = BTreeMap::new();
        changed_hashes.insert(PathBuf::from("src/lib.rs"), "hash_v2".to_string());
        assert!(cache.get(&key, Some(1), &changed_hashes).is_none());

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.stale_misses, 1);
    }

    #[test]
    fn test_cache_generation_downgrade() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key = make_key("review");
        let packet = make_packet("review");
        let file_hashes = BTreeMap::new();

        cache.insert(key.clone(), packet, LspEvidenceFreshness::Fresh, Some(2));

        // Same generation -> hit.
        assert!(cache.get(&key, Some(2), &file_hashes).is_some());

        // Different generation -> miss (entry removed).
        assert!(cache.get(&key, Some(3), &file_hashes).is_none());

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.stale_misses, 1);
    }

    #[test]
    fn test_cache_clear() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        for i in 0..3 {
            let key = LspCacheKeyBuilder::new("/ws", "s", format!("op_{i}")).build();
            let packet = make_packet(&format!("op_{i}"));
            cache.insert(key, packet, LspEvidenceFreshness::Fresh, Some(1));
        }

        assert_eq!(cache.stats().entries, 3);
        cache.clear();
        assert_eq!(cache.stats().entries, 0);
    }

    #[test]
    fn test_cache_clear_for_root() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key_a1 = LspCacheKeyBuilder::new("/ws_a", "s", "op_1").build();
        let key_a2 = LspCacheKeyBuilder::new("/ws_a", "s", "op_2").build();
        let key_b = LspCacheKeyBuilder::new("/ws_b", "s", "op_3").build();

        cache.insert(
            key_a1,
            make_packet("op_1"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );
        cache.insert(
            key_a2,
            make_packet("op_2"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );
        cache.insert(
            key_b.clone(),
            make_packet("op_3"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );

        assert_eq!(cache.stats().entries, 3);

        let removed = cache.clear_for_root(Path::new("/ws_a"));
        assert_eq!(removed, 2);
        assert_eq!(cache.stats().entries, 1);

        // Root B entry still exists.
        let file_hashes = BTreeMap::new();
        assert!(cache.get(&key_b, Some(1), &file_hashes).is_some());
    }

    #[test]
    fn test_cache_stats() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key = make_key("review");
        let packet = make_packet("review");
        let file_hashes = BTreeMap::new();

        cache.insert(key.clone(), packet, LspEvidenceFreshness::Fresh, Some(1));
        let _ = cache.get(&key, Some(1), &file_hashes);
        let _ = cache.get(&key, Some(1), &file_hashes);
        // Miss with different generation.
        let _ = cache.get(&key, Some(2), &file_hashes);

        let stats = cache.stats();
        assert_eq!(stats.entries, 0, "entry removed after generation mismatch");
        assert_eq!(stats.bytes, 0);
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.stale_misses, 1);
    }

    #[test]
    fn test_cache_key_different_requests() {
        let key1 = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_request(&LspContextRequest::Review {
                changed_files: vec![PathBuf::from("a.rs")],
                hunks: Vec::new(),
                risk_mode: crate::context::LspRiskMode::default(),
            })
            .build();
        let key2 = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_request(&LspContextRequest::Review {
                changed_files: vec![PathBuf::from("b.rs")],
                hunks: Vec::new(),
                risk_mode: crate::context::LspRiskMode::default(),
            })
            .build();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_different_roots() {
        let key1 = LspCacheKeyBuilder::new("/ws_a", "s", "review").build();
        let key2 = LspCacheKeyBuilder::new("/ws_b", "s", "review").build();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_different_file_hashes() {
        let key1 = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_file_hash("a.rs", "hash1")
            .build();
        let key2 = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_file_hash("a.rs", "hash2")
            .build();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_disabled_mode_ignores_insert() {
        let config = LspCacheConfig::default(); // Disabled
        let mut cache = LspSemanticCache::new(config);

        let key = make_key("review");
        let packet = make_packet("review");
        cache.insert(key.clone(), packet, LspEvidenceFreshness::Fresh, Some(1));

        assert_eq!(cache.stats().entries, 0);
        let file_hashes = BTreeMap::new();
        assert!(cache.get(&key, Some(1), &file_hashes).is_none());
    }

    #[test]
    fn test_cache_lru_ordering() {
        let config = make_config_enabled(2, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key_a = LspCacheKeyBuilder::new("/ws", "s", "a").build();
        let key_b = LspCacheKeyBuilder::new("/ws", "s", "b").build();
        let key_c = LspCacheKeyBuilder::new("/ws", "s", "c").build();

        cache.insert(
            key_a.clone(),
            make_packet("a"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );
        cache.insert(
            key_b.clone(),
            make_packet("b"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );
        // This should evict key_a (oldest).
        cache.insert(
            key_c.clone(),
            make_packet("c"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );

        let file_hashes = BTreeMap::new();
        assert!(
            cache.get(&key_a, Some(1), &file_hashes).is_none(),
            "key_a should be evicted"
        );
        assert!(
            cache.get(&key_b, Some(1), &file_hashes).is_some(),
            "key_b should survive"
        );
        assert!(
            cache.get(&key_c, Some(1), &file_hashes).is_some(),
            "key_c should survive"
        );
    }

    #[test]
    fn test_cache_lru_access_refreshes_order() {
        let config = make_config_enabled(2, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key_a = LspCacheKeyBuilder::new("/ws", "s", "a").build();
        let key_b = LspCacheKeyBuilder::new("/ws", "s", "b").build();
        let key_c = LspCacheKeyBuilder::new("/ws", "s", "c").build();

        cache.insert(
            key_a.clone(),
            make_packet("a"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );
        cache.insert(
            key_b.clone(),
            make_packet("b"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );

        // Access key_a to refresh its LRU position.
        let file_hashes = BTreeMap::new();
        cache.get(&key_a, Some(1), &file_hashes);

        // Insert key_c; should evict key_b (now oldest).
        cache.insert(
            key_c.clone(),
            make_packet("c"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );

        assert!(
            cache.get(&key_a, Some(1), &file_hashes).is_some(),
            "key_a should survive (recently accessed)"
        );
        assert!(
            cache.get(&key_b, Some(1), &file_hashes).is_none(),
            "key_b should be evicted"
        );
        assert!(
            cache.get(&key_c, Some(1), &file_hashes).is_some(),
            "key_c should survive"
        );
    }

    #[test]
    fn test_cache_remove() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key = make_key("review");
        cache.insert(
            key.clone(),
            make_packet("review"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );
        assert_eq!(cache.stats().entries, 1);

        let removed = cache.remove(&key);
        assert!(removed);
        assert_eq!(cache.stats().entries, 0);

        // Removing again returns false.
        let removed_again = cache.remove(&key);
        assert!(!removed_again);
    }

    #[test]
    fn test_cache_key_budget_fingerprint() {
        let budget1 = LspContextBudget {
            max_files: 5,
            ..Default::default()
        };
        let budget2 = LspContextBudget {
            max_files: 10,
            ..Default::default()
        };

        let key1 = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_budget(&budget1)
            .build();
        let key2 = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_budget(&budget2)
            .build();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_capability_fingerprint() {
        let key1 = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_capability_fingerprint("cap_v1")
            .build();
        let key2 = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_capability_fingerprint("cap_v2")
            .build();
        let key_none = LspCacheKeyBuilder::new("/ws", "s", "review").build();
        assert_ne!(key1, key2);
        assert_ne!(key1, key_none);
    }

    #[test]
    fn test_cache_key_precomputed_hash_matches_manual() {
        let key = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_request(&LspContextRequest::Review {
                changed_files: vec![PathBuf::from("a.rs")],
                hunks: Vec::new(),
                risk_mode: crate::context::LspRiskMode::default(),
            })
            .with_file_hash("a.rs", "abc")
            .with_budget(&LspContextBudget::default())
            .build();

        // Verify the precomputed hash matches a fresh computation.
        let manual_hash = LspCacheKey::compute_hash(
            &key.workspace_root,
            &key.server_id,
            &key.operation,
            &key.request_fingerprint,
            &key.input_hashes,
            &key.capability_fingerprint,
            &key.budget_fingerprint,
        );
        assert_eq!(key.precomputed_hash, manual_hash);
    }

    // ── Privacy / security checklist tests ──────────────────────────────

    #[test]
    fn test_cross_root_isolation() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key_a = LspCacheKeyBuilder::new("/workspace_alpha", "s", "review")
            .with_file_hash("main.rs", "hash_a")
            .build();
        let key_b = LspCacheKeyBuilder::new("/workspace_beta", "s", "review")
            .with_file_hash("main.rs", "hash_a")
            .build();

        cache.insert(
            key_a.clone(),
            make_packet("alpha-packet"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );

        let mut file_hashes = BTreeMap::new();
        file_hashes.insert(PathBuf::from("main.rs"), "hash_a".to_string());

        // key_b must NOT hit key_a's entry despite same operation and file hash.
        let result = cache.get(&key_b, Some(1), &file_hashes);
        assert!(
            result.is_none(),
            "cross-root lookup must not return another root's entry"
        );
        // key_a is still retrievable.
        assert!(cache.get(&key_a, Some(1), &file_hashes).is_some());
    }

    #[test]
    fn test_clear_for_root_does_not_affect_other_roots() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key_a = LspCacheKeyBuilder::new("/ws_alpha", "s", "op").build();
        let key_b = LspCacheKeyBuilder::new("/ws_beta", "s", "op").build();

        cache.insert(
            key_a.clone(),
            make_packet("a"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );
        cache.insert(
            key_b.clone(),
            make_packet("b"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );
        assert_eq!(cache.stats().entries, 2);

        cache.clear_for_root(Path::new("/ws_alpha"));
        assert_eq!(cache.stats().entries, 1, "only alpha cleared");

        let file_hashes = BTreeMap::new();
        // key_a was removed by clear_for_root, so get returns None.
        assert!(
            cache.get(&key_a, Some(1), &file_hashes).is_none(),
            "alpha entry removed"
        );
        // key_b was not affected; empty file_hashes match empty input_hashes.
        assert!(
            cache.get(&key_b, Some(1), &file_hashes).is_some(),
            "beta entry intact"
        );
    }

    #[test]
    fn test_clear_removes_all_entries() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        for i in 0..5 {
            let key = LspCacheKeyBuilder::new("/ws", "s", &format!("op_{i}")).build();
            cache.insert(
                key,
                make_packet(&format!("p{i}")),
                LspEvidenceFreshness::Fresh,
                Some(1),
            );
        }
        assert_eq!(cache.stats().entries, 5);

        cache.clear();
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().bytes, 0);
    }

    #[test]
    fn test_disabled_mode_never_stores_anything() {
        let config = LspCacheConfig::default(); // Disabled
        let mut cache = LspSemanticCache::new(config);

        for i in 0..10 {
            let key = LspCacheKeyBuilder::new("/ws", "s", &format!("op_{i}")).build();
            cache.insert(
                key,
                make_packet(&format!("p{i}")),
                LspEvidenceFreshness::Fresh,
                Some(1),
            );
        }
        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.bytes, 0);
    }

    #[test]
    fn test_file_hash_change_invalidates_entry() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_file_hash("main.rs", "v1_hash")
            .build();
        cache.insert(
            key.clone(),
            make_packet("v1"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );

        // Same key, same generation — should hit.
        let mut file_hashes = BTreeMap::new();
        file_hashes.insert(PathBuf::from("main.rs"), "v1_hash".to_string());
        assert!(cache.get(&key, Some(1), &file_hashes).is_some());

        // Different file hash — cache miss (key already encodes the hash, so
        // the caller would construct a different key; but even if the same key
        // is looked up with mismatched file hashes, the entry is valid since
        // the key itself embeds the hash). This test verifies the key
        // discriminates correctly.
        let key_v2 = LspCacheKeyBuilder::new("/ws", "s", "review")
            .with_file_hash("main.rs", "v2_hash")
            .build();
        assert_ne!(key, key_v2, "changed file hash must produce different key");
        let mut file_hashes_v2 = BTreeMap::new();
        file_hashes_v2.insert(PathBuf::from("main.rs"), "v2_hash".to_string());
        assert!(
            cache.get(&key_v2, Some(1), &file_hashes_v2).is_none(),
            "different key must not hit original entry"
        );
    }

    #[test]
    fn test_generation_mismatch_removes_entry() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 300);
        let mut cache = LspSemanticCache::new(config);

        let key = make_key("review");
        cache.insert(
            key.clone(),
            make_packet("review"),
            LspEvidenceFreshness::Fresh,
            Some(1), // generation 1
        );

        // Same generation → cache hit.
        let file_hashes = BTreeMap::new();
        assert!(
            cache.get(&key, Some(1), &file_hashes).is_some(),
            "same generation should hit"
        );

        // Different generation → cache miss and entry removed.
        let result = cache.get(&key, Some(2), &file_hashes);
        assert!(
            result.is_none(),
            "different generation must not return stale entry"
        );
        // Entry is removed from cache.
        assert_eq!(cache.stats().entries, 0);
    }

    #[test]
    fn test_ttl_expiry_returns_none() {
        let config = make_config_enabled(64, 4 * 1024 * 1024, 0); // 0 second TTL
        let mut cache = LspSemanticCache::new(config);

        let key = make_key("review");
        cache.insert(
            key.clone(),
            make_packet("review"),
            LspEvidenceFreshness::Fresh,
            Some(1),
        );

        // Even with same generation, TTL is expired immediately.
        let file_hashes = BTreeMap::new();
        let result = cache.get(&key, Some(1), &file_hashes);
        assert!(result.is_none(), "expired entry must not be returned");
    }
}
