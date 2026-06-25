//! Preview artifact registry.
//!
//! Tracks preview-only mutation artifacts (rename, formatting, code
//! action) with provenance, original hashes, and staleness tracking.
//!
//! # Lifecycle
//!
//! Every preview artifact follows this lifecycle:
//!
//! ```text
//! created -> inspectable -> applicable candidate
//! created -> stale -> recompute or discard
//! created -> expired -> discard
//! created -> applied by external mutating path -> historical/applied marker or removal
//! created -> cleared by user -> removed
//! ```
//!
//! # Cap
//!
//! The registry has a default cap of [`DEFAULT_MAX_ENTRIES`] entries.
//! When the cap is exceeded, the oldest entries are evicted first.
//! Users can also clear entries manually via [`PreviewArtifactRegistry::clear`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::context::LspPreviewArtifact;

/// Atomic counter for generating unique preview IDs.
static PREVIEW_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Default maximum number of preview artifacts kept in the registry.
/// Oldest entries are evicted when this cap is exceeded.
pub const DEFAULT_MAX_ENTRIES: usize = 32;

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

/// A single registered preview artifact.
#[derive(Debug, Clone)]
pub struct PreviewArtifactEntry {
    /// Unique identifier for this entry.
    pub id: String,
    /// The preview artifact.
    pub artifact: LspPreviewArtifact,
    /// File paths affected by this preview.
    pub file_edits: Vec<String>,
    /// Original file hashes before the preview was computed.
    pub original_hashes: HashMap<String, String>,
    /// Whether the base content has changed since creation.
    pub stale_base: bool,
    /// Per-file stale details: file path -> (expected_hash, actual_hash).
    /// Populated by [`PreviewArtifactRegistry::refresh_staleness`].
    pub stale_files: Vec<StaleFileInfo>,
    /// Server/capability provenance string.
    pub capability_provenance: String,
    /// Creation timestamp (millis since epoch).
    pub created_at: u64,
    /// Whether this preview has been applied via the mutating apply path.
    pub applied: bool,
}

/// Per-file stale-base evidence for a preview artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleFileInfo {
    /// File path that diverged from the original hash.
    pub file: String,
    /// Expected hash at preview creation time.
    pub expected_hash: String,
    /// Actual hash on disk at refresh time.
    pub actual_hash: String,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Registry of preview-only artifacts.
#[derive(Debug, Clone)]
pub struct PreviewArtifactRegistry {
    entries: Vec<PreviewArtifactEntry>,
    /// Maximum number of entries kept. Oldest entries are evicted when exceeded.
    max_entries: usize,
}

impl PreviewArtifactRegistry {
    /// Create an empty registry with the default cap.
    pub fn new() -> Self {
        Self::with_max_entries(DEFAULT_MAX_ENTRIES)
    }

    /// Create an empty registry with a custom cap.
    pub fn with_max_entries(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries: max_entries.max(1),
        }
    }

    /// Register a new preview artifact and return its ID.
    /// If the registry is at capacity, the oldest entry is evicted first.
    pub fn register(
        &mut self,
        artifact: LspPreviewArtifact,
        edits: Vec<String>,
        hashes: HashMap<String, String>,
        provenance: String,
    ) -> String {
        let seq = PREVIEW_COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let id = format!("preview-{seq}-{now}");

        self.entries.push(PreviewArtifactEntry {
            id: id.clone(),
            artifact,
            file_edits: edits,
            original_hashes: hashes,
            stale_base: false,
            stale_files: Vec::new(),
            capability_provenance: provenance,
            created_at: now,
            applied: false,
        });

        // Evict oldest entries when cap is exceeded.
        let overflow = self.entries.len().saturating_sub(self.max_entries);
        if overflow > 0 {
            self.entries.drain(..overflow);
        }

        id
    }

    /// Retrieve an entry by ID.
    pub fn get(&self, id: &str) -> Option<&PreviewArtifactEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Retrieve a mutable reference to an entry by ID.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut PreviewArtifactEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Return the last `n` entries (newest last).
    pub fn recent(&self, n: usize) -> Vec<&PreviewArtifactEntry> {
        let start = self.entries.len().saturating_sub(n);
        self.entries[start..].iter().collect()
    }

    /// Remove an entry by ID. Returns `true` if the entry was found and removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() < before
    }

    /// Remove all entries from the registry.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Mark an entry as applied via the external mutating apply path.
    pub fn mark_applied(&mut self, id: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.applied = true;
        }
    }

    /// Mark an entry's base as stale.
    pub fn mark_stale(&mut self, id: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.stale_base = true;
        }
    }

    /// Re-check staleness for an entry by re-hashing its affected files
    /// on disk and comparing against the original hashes.
    ///
    /// Returns the updated stale-base status, or `None` if the entry was
    /// not found. Per-file stale details are written to the entry's
    /// `stale_files` field.
    pub fn refresh_staleness(&mut self, id: &str) -> Option<bool> {
        let entry = self.entries.iter_mut().find(|e| e.id == id)?;

        let mut stale_files = Vec::new();
        for (file_path, expected_hash) in &entry.original_hashes {
            let actual_hash = std::fs::read(file_path)
                .ok()
                .map(|bytes| {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    bytes.hash(&mut hasher);
                    format!("{:016x}", hasher.finish())
                })
                .unwrap_or_else(|| "missing".to_string());

            if &actual_hash != expected_hash {
                stale_files.push(StaleFileInfo {
                    file: file_path.clone(),
                    expected_hash: expected_hash.clone(),
                    actual_hash,
                });
            }
        }

        entry.stale_base = !stale_files.is_empty();
        entry.stale_files = stale_files;

        Some(entry.stale_base)
    }

    /// Returns `true` if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of registered entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Maximum number of entries the registry will hold.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Number of entries that are stale (have `stale_base == true`).
    pub fn stale_count(&self) -> usize {
        self.entries.iter().filter(|e| e.stale_base).count()
    }

    /// Number of entries that have been applied.
    pub fn applied_count(&self) -> usize {
        self.entries.iter().filter(|e| e.applied).count()
    }

    /// Preview kind label derived from the artifact.
    pub fn preview_kind(entry: &PreviewArtifactEntry) -> &'static str {
        match &entry.artifact {
            LspPreviewArtifact::Rename { .. } => "rename",
            LspPreviewArtifact::Formatting { .. } => "formatting",
            LspPreviewArtifact::CodeAction { .. } => "code_action",
        }
    }

    /// Human-readable title/description from the artifact.
    pub fn preview_title(entry: &PreviewArtifactEntry) -> &str {
        match &entry.artifact {
            LspPreviewArtifact::Rename { description, .. }
            | LspPreviewArtifact::Formatting { description, .. }
            | LspPreviewArtifact::CodeAction { description, .. } => description.as_str(),
        }
    }

    /// Edit count from the artifact.
    pub fn preview_edit_count(entry: &PreviewArtifactEntry) -> usize {
        match &entry.artifact {
            LspPreviewArtifact::Rename { edit_count, .. } => *edit_count,
            LspPreviewArtifact::Formatting { .. } => 0,
            LspPreviewArtifact::CodeAction { .. } => 0,
        }
    }

    /// Populate `packet.preview_ids` from registry entries that match
    /// the packet's previews by kind. Returns the number of IDs populated.
    pub fn populate_preview_ids(&self, packet: &mut crate::context::LspContextPacket) -> usize {
        let mut count = 0;
        for preview in &packet.previews {
            let matching =
                self.entries.iter().rev().find(|e| {
                    std::mem::discriminant(&e.artifact) == std::mem::discriminant(preview)
                });
            if let Some(entry) = matching {
                packet.preview_ids.push(entry.id.clone());
                count += 1;
            } else {
                packet.preview_ids.push(String::new());
            }
        }
        count
    }
}

impl Default for PreviewArtifactRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_artifact() -> LspPreviewArtifact {
        LspPreviewArtifact::Rename {
            description: "rename foo -> bar".to_string(),
            edit_count: 1,
            patches: Vec::new(),
        }
    }

    #[test]
    fn test_register_and_retrieve() {
        let mut reg = PreviewArtifactRegistry::new();
        let id = reg.register(
            make_artifact(),
            vec!["a.rs".to_string()],
            HashMap::new(),
            "rust-analyzer".to_string(),
        );

        assert!(!id.is_empty());
        let entry = reg.get(&id).unwrap();
        assert_eq!(entry.file_edits, vec!["a.rs"]);
        assert!(!entry.applied);
        assert!(!entry.stale_base);
        assert!(entry.stale_files.is_empty());
        assert_eq!(entry.capability_provenance, "rust-analyzer");
    }

    #[test]
    fn test_recent_returns_last_n() {
        let mut reg = PreviewArtifactRegistry::new();
        for i in 0..5 {
            reg.register(
                LspPreviewArtifact::Formatting {
                    description: format!("fmt {i}"),
                    content_hash: None,
                    patches: Vec::new(),
                },
                vec![],
                HashMap::new(),
                "server".to_string(),
            );
        }

        let recent = reg.recent(3);
        assert_eq!(recent.len(), 3);
        match &recent[0].artifact {
            LspPreviewArtifact::Formatting {
                description: ref s, ..
            } => assert!(s.contains("fmt 2")),
            _ => panic!("expected Formatting"),
        }
        match &recent[2].artifact {
            LspPreviewArtifact::Formatting {
                description: ref s, ..
            } => assert!(s.contains("fmt 4")),
            _ => panic!("expected Formatting"),
        }
    }

    #[test]
    fn test_mark_stale() {
        let mut reg = PreviewArtifactRegistry::new();
        let id = reg.register(
            make_artifact(),
            vec![],
            HashMap::new(),
            "server".to_string(),
        );

        assert!(!reg.get(&id).unwrap().stale_base);
        reg.mark_stale(&id);
        assert!(reg.get(&id).unwrap().stale_base);
    }

    #[test]
    fn test_empty_registry() {
        let reg = PreviewArtifactRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.get("nonexistent").is_none());
        assert!(reg.recent(5).is_empty());
    }

    #[test]
    fn test_registry_len() {
        let mut reg = PreviewArtifactRegistry::new();
        assert_eq!(reg.len(), 0);

        reg.register(make_artifact(), vec![], HashMap::new(), "s".to_string());
        assert_eq!(reg.len(), 1);

        reg.register(
            LspPreviewArtifact::CodeAction {
                description: "action".to_string(),
                kind: None,
                patches: Vec::new(),
            },
            vec![],
            HashMap::new(),
            "s".to_string(),
        );
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn test_recent_from_empty() {
        let reg = PreviewArtifactRegistry::new();
        assert!(reg.recent(10).is_empty());
    }

    #[test]
    fn test_recent_clamps_to_available() {
        let mut reg = PreviewArtifactRegistry::new();
        reg.register(make_artifact(), vec![], HashMap::new(), "s".to_string());
        let recent = reg.recent(100);
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn test_mark_stale_nonexistent_is_noop() {
        let mut reg = PreviewArtifactRegistry::new();
        reg.mark_stale("nonexistent");
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registered_entry_applied_field_is_false() {
        let mut reg = PreviewArtifactRegistry::new();
        let id = reg.register(
            make_artifact(),
            vec!["a.rs".to_string()],
            HashMap::new(),
            "server".to_string(),
        );
        let entry = reg.get(&id).unwrap();
        assert!(
            !entry.applied,
            "Phase 5 preview artifacts must never be marked as applied"
        );
    }

    #[test]
    fn test_populate_preview_ids_matches_by_discriminant() {
        use crate::context::{LspContextPacket, LspContextPacketMode, LspContextRequest};

        let mut reg = PreviewArtifactRegistry::new();
        // Register a Rename artifact.
        let id = reg.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 2,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            HashMap::new(),
            "server".to_string(),
        );

        let mut packet = LspContextPacket {
            request: LspContextRequest::File {
                file: std::path::PathBuf::from("a.rs"),
                line_ranges: vec![],
                include_symbols: false,
                include_diagnostics: false,
            },
            items: vec![],
            previews: vec![
                LspPreviewArtifact::Rename {
                    description: "other".to_string(),
                    edit_count: 1,
                    patches: Vec::new(),
                },
                LspPreviewArtifact::Formatting {
                    description: "fmt".to_string(),
                    content_hash: None,
                    patches: Vec::new(),
                },
            ],
            preview_ids: vec![],
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: vec![],
            truncation: Default::default(),
        };

        let matched = reg.populate_preview_ids(&mut packet);
        assert_eq!(matched, 1, "only the Rename preview should match");
        assert_eq!(packet.preview_ids.len(), 2, "one ID per preview slot");
        assert_eq!(
            packet.preview_ids[0], id,
            "Rename should get the registered ID"
        );
        assert!(
            packet.preview_ids[1].is_empty(),
            "Formatting should get an empty ID (no match)"
        );
    }

    #[test]
    fn test_preview_entry_never_mutates_disk() {
        let mut reg = PreviewArtifactRegistry::new();
        let hashes = HashMap::from([("src/lib.rs".to_string(), "original_hash".to_string())]);
        let id = reg.register(
            LspPreviewArtifact::Formatting {
                description: "format src/lib.rs".to_string(),
                content_hash: Some("new_hash".to_string()),
                patches: Vec::new(),
            },
            vec!["src/lib.rs".to_string()],
            hashes.clone(),
            "rust-analyzer".to_string(),
        );
        let entry = reg.get(&id).unwrap();
        // Original hashes are preserved, not overwritten.
        assert_eq!(entry.original_hashes, hashes);
        // The entry is marked as not applied — the preview is purely informational.
        assert!(!entry.applied);
        // stale_base starts false (no disk mutation has occurred).
        assert!(!entry.stale_base);
    }

    // -----------------------------------------------------------------------
    // Phase 8: remove / clear / mark_applied / refresh_staleness / cap tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_remove_existing_entry() {
        let mut reg = PreviewArtifactRegistry::new();
        let id = reg.register(
            make_artifact(),
            vec!["a.rs".to_string()],
            HashMap::new(),
            "server".to_string(),
        );
        assert_eq!(reg.len(), 1);
        assert!(reg.remove(&id));
        assert_eq!(reg.len(), 0);
        assert!(reg.get(&id).is_none());
    }

    #[test]
    fn test_remove_nonexistent_entry() {
        let mut reg = PreviewArtifactRegistry::new();
        assert!(!reg.remove("nonexistent"));
    }

    #[test]
    fn test_clear_removes_all_entries() {
        let mut reg = PreviewArtifactRegistry::new();
        for i in 0..5 {
            reg.register(
                LspPreviewArtifact::Rename {
                    description: format!("r{i}"),
                    edit_count: 1,
                    patches: Vec::new(),
                },
                vec![],
                HashMap::new(),
                "s".to_string(),
            );
        }
        assert_eq!(reg.len(), 5);
        reg.clear();
        assert_eq!(reg.len(), 0);
        assert!(reg.is_empty());
    }

    #[test]
    fn test_mark_applied_sets_flag() {
        let mut reg = PreviewArtifactRegistry::new();
        let id = reg.register(
            make_artifact(),
            vec![],
            HashMap::new(),
            "server".to_string(),
        );
        assert!(!reg.get(&id).unwrap().applied);
        reg.mark_applied(&id);
        assert!(reg.get(&id).unwrap().applied);
    }

    #[test]
    fn test_mark_applied_nonexistent_is_noop() {
        let mut reg = PreviewArtifactRegistry::new();
        reg.mark_applied("nonexistent");
        assert!(reg.is_empty());
    }

    #[test]
    fn test_cap_evicts_oldest_when_full() {
        let mut reg = PreviewArtifactRegistry::with_max_entries(3);
        let id1 = reg.register(
            LspPreviewArtifact::Rename {
                description: "first".to_string(),
                edit_count: 1,
                patches: Vec::new(),
            },
            vec![],
            HashMap::new(),
            "s".to_string(),
        );

        let _id2 = reg.register(
            LspPreviewArtifact::Rename {
                description: "second".to_string(),
                edit_count: 1,
                patches: Vec::new(),
            },
            vec![],
            HashMap::new(),
            "s".to_string(),
        );
        let _id3 = reg.register(
            LspPreviewArtifact::Rename {
                description: "third".to_string(),
                edit_count: 1,
                patches: Vec::new(),
            },
            vec![],
            HashMap::new(),
            "s".to_string(),
        );
        assert_eq!(reg.len(), 3);

        // Register a fourth — should evict the oldest.
        let _id4 = reg.register(
            LspPreviewArtifact::Rename {
                description: "fourth".to_string(),
                edit_count: 1,
                patches: Vec::new(),
            },
            vec![],
            HashMap::new(),
            "s".to_string(),
        );
        assert_eq!(reg.len(), 3);
        assert!(reg.get(&id1).is_none(), "oldest should be evicted");
    }

    #[test]
    fn test_cap_evicts_multiple_when_oversized() {
        let mut reg = PreviewArtifactRegistry::with_max_entries(2);
        for i in 0..5 {
            reg.register(
                LspPreviewArtifact::Rename {
                    description: format!("r{i}"),
                    edit_count: 1,
                    patches: Vec::new(),
                },
                vec![],
                HashMap::new(),
                "s".to_string(),
            );
        }
        assert_eq!(reg.len(), 2);
        // Only the last two should remain.
        let recent = reg.recent(2);
        assert_eq!(recent.len(), 2);
        match &recent[0].artifact {
            LspPreviewArtifact::Rename { description, .. } => assert_eq!(description, "r3"),
            _ => panic!("expected Rename"),
        }
        match &recent[1].artifact {
            LspPreviewArtifact::Rename { description, .. } => assert_eq!(description, "r4"),
            _ => panic!("expected Rename"),
        }
    }

    #[test]
    fn test_stale_count_and_applied_count() {
        let mut reg = PreviewArtifactRegistry::new();
        let id1 = reg.register(make_artifact(), vec![], HashMap::new(), "s".to_string());
        let id2 = reg.register(make_artifact(), vec![], HashMap::new(), "s".to_string());
        assert_eq!(reg.stale_count(), 0);
        assert_eq!(reg.applied_count(), 0);

        reg.mark_stale(&id1);
        reg.mark_applied(&id2);
        assert_eq!(reg.stale_count(), 1);
        assert_eq!(reg.applied_count(), 1);
    }

    #[test]
    fn test_preview_kind_and_title_helpers() {
        let mut reg = PreviewArtifactRegistry::new();
        let rename_id = reg.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 3,
                patches: Vec::new(),
            },
            vec![],
            HashMap::new(),
            "s".to_string(),
        );

        let fmt_id = reg.register(
            LspPreviewArtifact::Formatting {
                description: "format a.rs".to_string(),
                content_hash: None,
                patches: Vec::new(),
            },
            vec![],
            HashMap::new(),
            "s".to_string(),
        );
        let ca_id = reg.register(
            LspPreviewArtifact::CodeAction {
                description: "organize imports".to_string(),
                kind: Some("source.organizeImports".to_string()),
                patches: Vec::new(),
            },
            vec![],
            HashMap::new(),
            "s".to_string(),
        );

        let rename_entry = reg.get(&rename_id).unwrap();
        assert_eq!(
            PreviewArtifactRegistry::preview_kind(rename_entry),
            "rename"
        );
        assert_eq!(
            PreviewArtifactRegistry::preview_title(rename_entry),
            "foo -> bar"
        );
        assert_eq!(PreviewArtifactRegistry::preview_edit_count(rename_entry), 3);

        let fmt_entry = reg.get(&fmt_id).unwrap();
        assert_eq!(
            PreviewArtifactRegistry::preview_kind(fmt_entry),
            "formatting"
        );

        let ca_entry = reg.get(&ca_id).unwrap();
        assert_eq!(
            PreviewArtifactRegistry::preview_kind(ca_entry),
            "code_action"
        );
    }

    #[test]
    fn test_refresh_staleness_unchanged_file_remains_fresh() {
        let mut reg = PreviewArtifactRegistry::new();
        let hashes = HashMap::new(); // empty hashes — no files to check
        let id = reg.register(
            make_artifact(),
            vec!["a.rs".to_string()],
            hashes,
            "server".to_string(),
        );
        let result = reg.refresh_staleness(&id);
        assert_eq!(result, Some(false));
        assert!(!reg.get(&id).unwrap().stale_base);
        assert!(reg.get(&id).unwrap().stale_files.is_empty());
    }

    #[test]
    fn test_refresh_staleness_nonexistent_returns_none() {
        let mut reg = PreviewArtifactRegistry::new();
        assert_eq!(reg.refresh_staleness("nonexistent"), None);
    }

    #[test]
    fn test_max_entries_accessor() {
        let reg = PreviewArtifactRegistry::with_max_entries(16);
        assert_eq!(reg.max_entries(), 16);
    }

    #[test]
    fn test_default_max_entries() {
        let reg = PreviewArtifactRegistry::new();
        assert_eq!(reg.max_entries(), DEFAULT_MAX_ENTRIES);
    }

    #[test]
    fn test_get_mut_allows_modification() {
        let mut reg = PreviewArtifactRegistry::new();
        let id = reg.register(
            make_artifact(),
            vec![],
            HashMap::new(),
            "server".to_string(),
        );
        {
            let entry = reg.get_mut(&id).unwrap();
            entry.stale_base = true;
        }
        assert!(reg.get(&id).unwrap().stale_base);
    }
}
