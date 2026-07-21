//! Multi-Project TUI persistent restoration manifest (Milestone 004).
//!
//! This module defines the on-disk manifest that the TUI persists
//! between sessions to remember which project tabs the user intended
//! to have open. It is a **frontend convenience layer**: the daemon
//! catalog and canonical project/workspace/session bindings remain
//! authoritative. The manifest may record what the user intended to
//! reopen; it MUST NOT assert that any of those identities still
//! exist or are still bound the same way.
//!
//! ## Invariants
//!
//! - No `ProjectTabId` durability guarantee. Frontend-local tab IDs
//!   are regenerated per process; the manifest carries canonical
//!   daemon IDs plus a stable order key, not runtime object identity.
//! - Paths, cwd, compat directories, secrets, credentials, provider
//!   headers, prompts, messages, tool output, file bodies, diffs,
//!   logs, terminal frames, and environment values are NEVER
//!   persisted.
//! - The format is versioned, bounded, additive, and corruption
//!   tolerant. Unknown additive fields are ignored. Unsupported
//!   future major versions produce an actionable diagnostic.
//! - All string fields are length-limited on serialization and
//!   deserialization. The total file size is bounded.
//! - Duplicate project records are deterministically deduplicated.
//! - Malformed IDs are rejected before any daemon request.
//! - File writes are atomic, permission-safe, size-bounded, and do
//!   not follow untrusted symlinks.
//!
//! See `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md`
//! for the full specification.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Current manifest schema major version. A persisted manifest with a
/// higher major version produces a fallback diagnostic and is never
/// silently loaded.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Largest manifest payload size (in bytes) accepted on load. The
/// format is additive, so this guards against malicious or corrupt
/// files. The cap is intentionally generous — typical manifests with
/// a handful of tabs serialize to a few kilobytes.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// Maximum number of persisted tabs. The runtime cap
/// (`MAX_OPEN_PROJECT_TABS`) is the authoritative limit; this
/// constant provides an early bound during validation so a corrupt
/// manifest cannot allocate unbounded memory before rejection.
pub const MAX_PERSISTED_TABS: usize = 32;

/// Length cap for display-only label hints. Persisted labels are
/// never authoritative — they are display hints only.
pub const MAX_PERSISTED_LABEL_LEN: usize = 128;

/// Length cap for selected model/agent ids in the manifest. Bounded
/// so a corrupt file cannot smuggle a 64KB string into the request
/// path. The runtime catalog enforces its own length limits on
/// daemon responses.
pub const MAX_PERSISTED_MODEL_ID_LEN: usize = 128;

/// Length cap for project_id/workspace_id/session_id strings in the
/// manifest. The protocol layer accepts raw UUID-shaped strings; we
/// cap defensively to reject obvious junk.
pub const MAX_PERSISTED_ID_LEN: usize = 128;

/// Length cap for daemon instance hints. Used for diagnostic
/// association only; never authoritative.
pub const MAX_PERSISTED_DAEMON_HINT_LEN: usize = 128;

/// Length cap for preference string values.
pub const MAX_PERSISTED_PREF_VALUE_LEN: usize = 64;

/// File name for the persisted manifest inside the TUI state root.
pub const MANIFEST_FILE_NAME: &str = "tab_manifest.json";

/// File name for the temporary write buffer used during atomic
/// rename. Suffix `.tmp` is intentional so accidental edits do not
/// rename a non-JSON file into place.
pub const MANIFEST_TEMP_FILE_NAME: &str = "tab_manifest.json.tmp";

/// Stable order key assigned to a tab when none was recorded. Used
/// only for deterministic normalization during validation; never
/// persisted as authoritative.
pub const ORDER_KEY_FALLBACK: &str = "0000-00-00T00:00:00Z#000000";

/// A persisted project tab entry. Records canonical daemon
/// identity and lightweight presentation intent. The persisted
/// format deliberately omits frontend-local `ProjectTabId` (it is
/// regenerated per process) and omits any path or cwd.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedProjectTab {
    /// Daemon-typed project id (canonical UUID-shaped string).
    /// When missing or empty the entry is skipped during restore.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Daemon-typed workspace id, if the user had explicitly chosen
    /// one. The restore coordinator validates this against the
    /// project's current workspace list.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Daemon-typed session id. Restored only if the session still
    /// exists and is bound to the same project.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Display-only label hint. Never used as identity.
    #[serde(default)]
    pub label_hint: Option<String>,
    /// Display-only selected model id. The restore coordinator
    /// validates against the model catalog and falls back to the
    /// user's default on missing/unsupported ids.
    #[serde(default)]
    pub selected_model_id: Option<String>,
    /// Display-only selected agent name. Validated against the
    /// agent registry at restore time.
    #[serde(default)]
    pub selected_agent: Option<String>,
    /// Stable order key used to sort tabs on restore. Persisted as
    /// a sortable string (lexicographic on UTF-8). When missing, the
    /// manifest position is used as the order key.
    #[serde(default)]
    pub order_key: Option<String>,
}

/// Bounded presentation preferences. The manifest only carries UI
/// intent that is safe to remember across restarts; nothing in
/// `preferences` may influence daemon authority decisions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestPreferences {
    /// Optional UI hint for the last visible sidebar state. Used by
    /// the TUI to restore the user's preferred sidebar visibility
    /// without persisting path or session identifiers.
    #[serde(default)]
    pub sidebar_visible: Option<bool>,
}

/// Persisted manifest schema. Versioned additive structure with a
/// strict major version at the top level. New fields MUST be
/// `Option<T>` and `#[serde(default)]` so old manifests deserialize
/// cleanly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiWorkspaceManifest {
    /// Schema major version. A higher number than
    /// `MANIFEST_SCHEMA_VERSION` produces an actionable fallback
    /// diagnostic and the manifest is not loaded.
    pub schema_version: u32,
    /// Wall-clock timestamp (ISO-8601 UTC, when written). Diagnostic
    /// only; not authoritative.
    #[serde(default)]
    pub written_at: Option<String>,
    /// Optional, non-authoritative daemon identifier hint. Used
    /// solely for the "this manifest was written by another daemon
    /// instance" diagnostic. Never trusted.
    #[serde(default)]
    pub daemon_instance_hint: Option<String>,
    /// Ordered persisted tabs. Always bounded by
    /// `MAX_PERSISTED_TABS` after validation.
    #[serde(default)]
    pub ordered_tabs: Vec<PersistedProjectTab>,
    /// Active project id at write time. Validated by the restore
    /// coordinator before being promoted to live state.
    #[serde(default)]
    pub active_project_id: Option<String>,
    /// Active session id at write time. Validated by the restore
    /// coordinator before being promoted to live state.
    #[serde(default)]
    pub active_session_id: Option<String>,
    /// Bounded presentation preferences.
    #[serde(default)]
    pub preferences: ManifestPreferences,
}

impl Default for TuiWorkspaceManifest {
    fn default() -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            written_at: None,
            daemon_instance_hint: None,
            ordered_tabs: Vec::new(),
            active_project_id: None,
            active_session_id: None,
            preferences: ManifestPreferences::default(),
        }
    }
}

/// Outcome of a manifest load attempt. Either a validated manifest
/// or a structured diagnostic describing why the manifest was
/// rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestLoadOutcome {
    /// The manifest file was absent. The TUI starts in its compat
    /// single-tab mode and no restored intent is applied.
    Absent,
    /// The manifest was successfully loaded and validated.
    Loaded(TuiWorkspaceManifest),
    /// The manifest was rejected. The TUI falls back to safe empty
    /// state and the diagnostic is recorded for operator visibility.
    Rejected(ManifestDiagnostic),
}

/// Structured diagnostic describing a manifest rejection. Each
/// variant is bounded so a corrupt file cannot inject unbounded
/// text into the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestDiagnostic {
    /// Manifest exceeded the size cap before parse. Always
    /// recoverable; the manifest is dropped on the floor.
    Oversized { bytes: usize },
    /// Manifest could not be read from disk (permission denied,
    /// missing directory, etc).
    Unreadable { reason: String },
    /// Manifest major schema version is higher than this build
    /// supports. The file is preserved for forward compatibility
    /// but not loaded.
    UnsupportedMajor { on_disk: u32 },
    /// Manifest is structurally invalid JSON.
    InvalidJson { reason: String },
    /// Manifest structure parsed but field validation failed. The
    /// file is preserved so the operator can inspect it.
    InvalidFields { reason: String },
    /// Manifest contained a path/cwd attempt that must never be
    /// persisted. The file is quarantined.
    ForbiddenIdentity { reason: String },
}

impl ManifestDiagnostic {
    /// Short, bounded, UI-safe message suitable for a toast or
    /// diagnostic surface. Never includes secret-bearing text.
    pub fn short_message(&self) -> &'static str {
        match self {
            Self::Oversized { .. } => "Manifest oversized; using fresh state.",
            Self::Unreadable { .. } => "Manifest unreadable; using fresh state.",
            Self::UnsupportedMajor { .. } => {
                "Manifest written by a newer Codegg version; using fresh state."
            }
            Self::InvalidJson { .. } => "Manifest corrupt; using fresh state.",
            Self::InvalidFields { .. } => "Manifest fields invalid; using fresh state.",
            Self::ForbiddenIdentity { .. } => {
                "Manifest contained forbidden identity fields; quarantined."
            }
        }
    }
}

/// Truncate a string to a maximum number of characters, returning
/// the truncated copy. Used both on serialization (to avoid
/// persisting unbounded strings) and on deserialization (to avoid
/// allocating unbounded memory before validation can reject the
/// file).
pub fn bounded_string(input: Option<&str>, max_chars: usize) -> Option<String> {
    let s = input?;
    if s.chars().count() <= max_chars {
        Some(s.to_string())
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        Some(truncated)
    }
}

/// Validate and bound a manifest in-place. Returns `Ok(())` when
/// the manifest is safe to use; otherwise a `ManifestDiagnostic`
/// describing the failure. The validator is idempotent and does
/// not mutate the manifest on error.
pub fn validate_manifest(manifest: &mut TuiWorkspaceManifest) -> Result<(), ManifestDiagnostic> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(ManifestDiagnostic::UnsupportedMajor {
            on_disk: manifest.schema_version,
        });
    }

    manifest.written_at = bounded_string(manifest.written_at.as_deref(), 64);
    manifest.daemon_instance_hint = bounded_string(
        manifest.daemon_instance_hint.as_deref(),
        MAX_PERSISTED_DAEMON_HINT_LEN,
    );

    if manifest.ordered_tabs.len() > MAX_PERSISTED_TABS {
        manifest.ordered_tabs.truncate(MAX_PERSISTED_TABS);
    }

    for tab in manifest.ordered_tabs.iter_mut() {
        tab.project_id =
            bounded_string(tab.project_id.as_deref(), MAX_PERSISTED_ID_LEN).and_then(non_empty);
        tab.workspace_id =
            bounded_string(tab.workspace_id.as_deref(), MAX_PERSISTED_ID_LEN).and_then(non_empty);
        tab.session_id =
            bounded_string(tab.session_id.as_deref(), MAX_PERSISTED_ID_LEN).and_then(non_empty);
        tab.label_hint =
            bounded_string(tab.label_hint.as_deref(), MAX_PERSISTED_LABEL_LEN).and_then(non_empty);
        tab.selected_model_id =
            bounded_string(tab.selected_model_id.as_deref(), MAX_PERSISTED_MODEL_ID_LEN)
                .and_then(non_empty);
        tab.selected_agent =
            bounded_string(tab.selected_agent.as_deref(), MAX_PERSISTED_MODEL_ID_LEN)
                .and_then(non_empty);
        tab.order_key =
            bounded_string(tab.order_key.as_deref(), MAX_PERSISTED_LABEL_LEN).and_then(non_empty);
    }

    manifest.active_project_id =
        bounded_string(manifest.active_project_id.as_deref(), MAX_PERSISTED_ID_LEN)
            .and_then(non_empty);
    manifest.active_session_id =
        bounded_string(manifest.active_session_id.as_deref(), MAX_PERSISTED_ID_LEN)
            .and_then(non_empty);

    // Reject any tab that has neither a project_id nor any other
    // identifying field. An empty tab entry cannot be restored and
    // would silently be dropped during restore; we drop it here so
    // the validator output is explicit.
    manifest
        .ordered_tabs
        .retain(|t| t.project_id.is_some() || t.session_id.is_some() || t.workspace_id.is_some());

    // Deterministic dedup by project_id, preserving the first
    // occurrence's order.
    let mut seen = std::collections::HashSet::new();
    manifest.ordered_tabs.retain(|t| match &t.project_id {
        Some(pid) => seen.insert(pid.clone()),
        None => true,
    });

    Ok(())
}

/// Convert a non-empty string to `Some`, returning `None` for empty
/// or whitespace-only strings. Used to avoid persisting empty
/// optional identity fields.
fn non_empty(input: String) -> Option<String> {
    if input.trim().is_empty() {
        None
    } else {
        Some(input)
    }
}

/// Compute the default manifest path under a given state root. The
/// path is `<state_root>/<MANIFEST_FILE_NAME>`. The state root is
/// the user-scoped TUI state directory (typically
/// `~/.config/codegg/tui/` on Linux).
pub fn default_manifest_path(state_root: &Path) -> PathBuf {
    state_root.join(MANIFEST_FILE_NAME)
}

/// Compute the temp-file path used for atomic writes.
pub fn default_temp_path(state_root: &Path) -> PathBuf {
    state_root.join(MANIFEST_TEMP_FILE_NAME)
}

/// Result of serializing a manifest for atomic write. The serialized
/// payload is bounded by `MAX_MANIFEST_BYTES`; callers MUST treat
/// any larger output as a bug.
#[derive(Debug)]
pub struct SerializedManifest {
    /// The serialized JSON bytes.
    pub bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_string_truncates_overlong_input() {
        let big = "a".repeat(MAX_PERSISTED_LABEL_LEN + 10);
        let out = bounded_string(Some(&big), MAX_PERSISTED_LABEL_LEN).unwrap();
        assert_eq!(out.chars().count(), MAX_PERSISTED_LABEL_LEN);
    }

    #[test]
    fn bounded_string_preserves_short_input() {
        let out = bounded_string(Some("hello"), MAX_PERSISTED_LABEL_LEN).unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    fn bounded_string_handles_none() {
        assert!(bounded_string(None, 10).is_none());
    }

    #[test]
    fn validate_truncates_long_ids() {
        let mut m = TuiWorkspaceManifest::default();
        let huge = "x".repeat(MAX_PERSISTED_ID_LEN + 5);
        m.ordered_tabs.push(PersistedProjectTab {
            project_id: Some(huge),
            workspace_id: None,
            session_id: None,
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: None,
        });
        validate_manifest(&mut m).unwrap();
        let pid = m.ordered_tabs[0].project_id.as_deref().unwrap();
        assert_eq!(pid.chars().count(), MAX_PERSISTED_ID_LEN);
    }

    #[test]
    fn validate_dedups_repeated_project_id() {
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(PersistedProjectTab {
            project_id: Some("proj-A".into()),
            workspace_id: None,
            session_id: None,
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: Some("k1".into()),
        });
        m.ordered_tabs.push(PersistedProjectTab {
            project_id: Some("proj-A".into()),
            workspace_id: None,
            session_id: None,
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: Some("k2".into()),
        });
        m.ordered_tabs.push(PersistedProjectTab {
            project_id: Some("proj-B".into()),
            workspace_id: None,
            session_id: None,
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: Some("k3".into()),
        });
        validate_manifest(&mut m).unwrap();
        let pids: Vec<&str> = m
            .ordered_tabs
            .iter()
            .filter_map(|t| t.project_id.as_deref())
            .collect();
        assert_eq!(pids, vec!["proj-A", "proj-B"]);
    }

    #[test]
    fn validate_rejects_empty_project_id_with_no_other_identity() {
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(PersistedProjectTab {
            project_id: None,
            workspace_id: None,
            session_id: None,
            label_hint: Some("orphan".into()),
            selected_model_id: None,
            selected_agent: None,
            order_key: None,
        });
        validate_manifest(&mut m).unwrap();
        assert!(m.ordered_tabs.is_empty());
    }

    #[test]
    fn validate_rejects_unsupported_major_version() {
        let mut m = TuiWorkspaceManifest::default();
        m.schema_version = MANIFEST_SCHEMA_VERSION + 1;
        let err = validate_manifest(&mut m).unwrap_err();
        assert!(matches!(err, ManifestDiagnostic::UnsupportedMajor { .. }));
    }

    #[test]
    fn validate_caps_persisted_tabs() {
        let mut m = TuiWorkspaceManifest::default();
        for i in 0..(MAX_PERSISTED_TABS + 5) {
            m.ordered_tabs.push(PersistedProjectTab {
                project_id: Some(format!("proj-{i}")),
                workspace_id: None,
                session_id: None,
                label_hint: None,
                selected_model_id: None,
                selected_agent: None,
                order_key: None,
            });
        }
        validate_manifest(&mut m).unwrap();
        assert_eq!(m.ordered_tabs.len(), MAX_PERSISTED_TABS);
    }

    #[test]
    fn manifest_round_trips_with_default() {
        let m = TuiWorkspaceManifest::default();
        let json = serde_json::to_string(&m).unwrap();
        let back: TuiWorkspaceManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn manifest_accepts_unknown_additive_field() {
        let json = r#"{
            "schema_version": 1,
            "ordered_tabs": [],
            "future_field": "ignored"
        }"#;
        let m: TuiWorkspaceManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.schema_version, 1);
        assert!(m.ordered_tabs.is_empty());
    }

    #[test]
    fn manifest_preferences_round_trip() {
        let mut m = TuiWorkspaceManifest::default();
        m.preferences.sidebar_visible = Some(true);
        let json = serde_json::to_string(&m).unwrap();
        let back: TuiWorkspaceManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.preferences.sidebar_visible, Some(true));
    }

    #[test]
    fn manifest_diagnostic_short_messages_are_bounded() {
        for diag in [
            ManifestDiagnostic::Oversized { bytes: 100 },
            ManifestDiagnostic::Unreadable {
                reason: "perm denied".into(),
            },
            ManifestDiagnostic::UnsupportedMajor { on_disk: 99 },
            ManifestDiagnostic::InvalidJson {
                reason: "bad".into(),
            },
            ManifestDiagnostic::InvalidFields { reason: "x".into() },
            ManifestDiagnostic::ForbiddenIdentity { reason: "y".into() },
        ] {
            let msg = diag.short_message();
            assert!(msg.len() < 200, "diagnostic message too long: {msg}");
            assert!(!msg.is_empty());
        }
    }

    #[test]
    fn default_manifest_path_is_under_state_root() {
        let root = PathBuf::from("/tmp/codegg-tui");
        let p = default_manifest_path(&root);
        assert_eq!(p, PathBuf::from("/tmp/codegg-tui/tab_manifest.json"));
    }
}
