//! Preview artifact registry.
//!
//! Tracks preview-only mutation artifacts (rename, formatting, code
//! action) with provenance, original hashes, and staleness tracking.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::context::LspPreviewArtifact;

/// Atomic counter for generating unique preview IDs.
static PREVIEW_COUNTER: AtomicU64 = AtomicU64::new(1);

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
    /// Server/capability provenance string.
    pub capability_provenance: String,
    /// Creation timestamp (millis since epoch).
    pub created_at: u64,
    /// Whether this preview has been applied (always false in Phase 5).
    pub applied: bool,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Registry of preview-only artifacts.
#[derive(Debug, Clone)]
pub struct PreviewArtifactRegistry {
    entries: Vec<PreviewArtifactEntry>,
}

impl PreviewArtifactRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a new preview artifact and return its ID.
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
            capability_provenance: provenance,
            created_at: now,
            applied: false,
        });

        id
    }

    /// Retrieve an entry by ID.
    pub fn get(&self, id: &str) -> Option<&PreviewArtifactEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Return the last `n` entries.
    pub fn recent(&self, n: usize) -> Vec<&PreviewArtifactEntry> {
        let start = self.entries.len().saturating_sub(n);
        self.entries[start..].iter().collect()
    }

    /// Mark an entry's base as stale.
    pub fn mark_stale(&mut self, id: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.stale_base = true;
        }
    }

    /// Returns `true` if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of registered entries.
    pub fn len(&self) -> usize {
        self.entries.len()
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
                },
                LspPreviewArtifact::Formatting {
                    description: "fmt".to_string(),
                    content_hash: None,
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
}
