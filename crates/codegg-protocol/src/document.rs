//! Additive, bounded `document.v1` editor-document wire contract.

use serde::{Deserialize, Serialize};

pub const DOCUMENT_PROTOCOL_CAPABILITY: &str = "document.v1";
pub const MAX_DOCUMENT_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_DOCUMENT_EDITS: usize = 256;
pub const MAX_DOCUMENT_INSERT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentTextRangeDto {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentTextEditDto {
    pub range: DocumentTextRangeDto,
    pub insert: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DocumentTransactionDto {
    pub edits: Vec<DocumentTextEditDto>,
}

/// Which authority root confines a document.
///
/// This is a **narrowing** axis, not a widening one. `Workspace` is the
/// ordinary case and confines the document to the resolved workspace root
/// under the usual relative-path rules. `GlobalAgent` confines it to the
/// single user-level agent directory and *additionally* accepts only a bare
/// `<name>.toml` file — no subdirectories, no traversal, no other extension.
///
/// The global scope exists because a global agent config lives outside every
/// workspace, so `Workspace` containment can never reach it. Rather than
/// relaxing that containment, the daemon resolves a second, separately named
/// root whose allowed filename set is a strict subset of what a workspace
/// accepts. A caller cannot widen a workspace document into a global one, and
/// a global document can never address anything but one agent TOML.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DocumentScope {
    /// Confined to the bound workspace root (the default).
    #[default]
    Workspace,
    /// Confined to the user-level agent root, restricted to one `.toml` leaf.
    GlobalAgent,
}

impl DocumentScope {
    /// Whether this scope addresses the user-level agent root.
    pub fn is_global_agent(self) -> bool {
        matches!(self, Self::GlobalAgent)
    }
}

/// Maximum length of the `<name>` stem in a global agent config filename.
pub const MAX_GLOBAL_AGENT_NAME_CHARS: usize = 64;

impl DocumentScope {
    /// Validate `relative_path` against the *narrower* global-agent rule.
    ///
    /// The returned string is the normalized filename. This is intentionally
    /// a whole second policy rather than a relaxed workspace check: a global
    /// agent is one flat file in one directory, so anything that is not a
    /// bare `<name>.toml` is refused before it ever reaches the filesystem
    /// containment checks.
    pub fn validate_global_agent_filename(
        self,
        relative_path: &str,
    ) -> Result<String, &'static str> {
        if !self.is_global_agent() {
            return Err("not a global agent scope");
        }
        let normalized = relative_path.replace('\\', "/");
        // No separators at all: a global agent is a flat file, never nested.
        if normalized.contains('/') || normalized.is_empty() {
            return Err("global agent configs are a single .toml file in the agent directory");
        }
        if !normalized.ends_with(".toml") {
            return Err("global agent configs must end in .toml");
        }
        let stem = &normalized[..normalized.len() - ".toml".len()];
        if stem.is_empty() || stem.len() > MAX_GLOBAL_AGENT_NAME_CHARS {
            return Err("global agent name must be 1-64 characters");
        }
        if !stem
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err("global agent names use letters, digits, `-` and `_` only");
        }
        Ok(normalized)
    }
}

/// Maximum serialized size of one daemon directory-listing response.
///
/// The in-memory walk is already bounded (depth/nodes/examined/name), but
/// deeply nested paths inflate JSON beyond the node count. Responses that
/// would exceed this budget are trimmed from the trailing top-level nodes
/// with an explicit truncation reason rather than failing or growing
/// without bound. Frozen by ADR-0022.
pub const MAX_DIRECTORY_LISTING_RESPONSE_BYTES: usize = 512 * 1024;

/// Maximum components in a listing `prefix`. Matches the walk depth so a
/// prefix can never address a subtree the walker itself could not reach.
pub const MAX_DIRECTORY_LISTING_PREFIX_COMPONENTS: usize = 12;

/// Maximum byte length of one `prefix` component. Matches the walk's
/// per-entry name cap so a prefix can never name an entry the walker
/// would skip.
pub const MAX_DIRECTORY_LISTING_PREFIX_NAME_BYTES: usize = 255;

/// Whether a listed entry can be opened as a document.
///
/// Variant order is the sibling sort order: directories before files.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceDirectoryNodeKindDto {
    Directory,
    File,
}

/// One inert entry in a daemon directory listing.
///
/// `path` is always workspace-relative and `/`-separated, built from
/// validated component names. It carries no file bytes, no absolute
/// daemon path, and no authority of its own: opening it still routes
/// through `DocumentOpen`, which revalidates containment and `file.read`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDirectoryNodeDto {
    pub name: String,
    pub path: String,
    pub depth: usize,
    pub kind: WorkspaceDirectoryNodeKindDto,
    /// `None` for files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<WorkspaceDirectoryNodeDto>>,
}

/// Readiness of a daemon directory listing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceDirectoryListStatusDto {
    /// The listing describes the daemon workspace at scan time.
    #[default]
    Ready,
    /// Daemon scan admission is contended; retry explicitly.
    Busy,
    /// The daemon cannot serve a listing (workspace unresolvable,
    /// scan failed, unsupported daemon). Never a local fallback.
    Unavailable,
}

/// C005 (ADR-0022 §3): daemon-owned workspace directory listing request.
///
/// Carries only a project/workspace locator plus an optional
/// workspace-relative `prefix`. The daemon resolves the canonical root
/// itself; no client-supplied absolute root is honored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDirectoryListRequestDto {
    pub project_id: String,
    pub workspace_id: String,
    /// Workspace-relative subdirectory to list. Empty means the root.
    /// Validated by [`Self::validate_prefix`] before any I/O.
    #[serde(default)]
    pub prefix: String,
}

impl WorkspaceDirectoryListRequestDto {
    /// Validate `prefix` without touching the filesystem.
    ///
    /// Returns the normalized prefix (`""` for the root). Refuses
    /// absolute paths, backslash separators, empty/`.`/`..` components,
    /// overlong components, and prefixes deeper than the walk itself.
    pub fn validate_prefix(&self) -> Result<String, &'static str> {
        if self.prefix.is_empty() {
            return Ok(String::new());
        }
        if self.prefix.starts_with('/') || self.prefix.starts_with('\\') {
            return Err("listing prefix must be workspace-relative");
        }
        if self.prefix.contains('\\') {
            return Err("listing prefix must use `/` separators");
        }
        let components: Vec<&str> = self.prefix.split('/').collect();
        if components.len() > MAX_DIRECTORY_LISTING_PREFIX_COMPONENTS {
            return Err("listing prefix is deeper than the maximum walk depth");
        }
        for component in &components {
            if component.is_empty() || *component == "." || *component == ".." {
                return Err("listing prefix must not contain empty or `.`/`..` components");
            }
            if component.len() > MAX_DIRECTORY_LISTING_PREFIX_NAME_BYTES {
                return Err("listing prefix component exceeds the maximum name length");
            }
        }
        Ok(components.join("/"))
    }
}

/// C005 (ADR-0022 §3): daemon-owned workspace directory listing result.
///
/// Advisory metadata only: a file may disappear between listing and
/// `DocumentOpen`, which independently reauthorizes. `workspace_id` and
/// `prefix` echo the authorized scope so the TUI can key the response
/// before display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDirectoryListResultDto {
    pub workspace_id: String,
    pub prefix: String,
    #[serde(default)]
    pub nodes: Vec<WorkspaceDirectoryNodeDto>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated_reason: Option<String>,
    /// Directory entries examined, including filtered-out ones.
    #[serde(default)]
    pub examined: usize,
    pub status: WorkspaceDirectoryListStatusDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentSnapshotDto {
    pub document_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub relative_path: String,
    pub revision: u64,
    pub text: String,
    pub dirty: bool,
    pub conflicted: bool,
    pub writer: bool,
    pub disk_base_digest: String,
    /// Which authority root confines this document. Defaults to
    /// [`DocumentScope::Workspace`] so a peer that predates global agent
    /// editing still reads every workspace document correctly.
    #[serde(default)]
    pub scope: DocumentScope,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_prefix_validation_refuses_traversal_and_absolute_paths() {
        let request = |prefix: &str| WorkspaceDirectoryListRequestDto {
            project_id: "p".to_owned(),
            workspace_id: "w".to_owned(),
            prefix: prefix.to_owned(),
        };
        assert_eq!(request("").validate_prefix().as_deref(), Ok(""));
        assert_eq!(request("src").validate_prefix().as_deref(), Ok("src"));
        assert_eq!(
            request("src-tauri/icons").validate_prefix().as_deref(),
            Ok("src-tauri/icons")
        );
        for rejected in [
            "/etc",
            "../escape",
            "a/../../escape",
            "a//b",
            "a/./b",
            "a/",
            ".",
            "..",
            "a\\b",
            "C:\\win",
        ] {
            assert!(
                request(rejected).validate_prefix().is_err(),
                "expected {rejected:?} to be refused"
            );
        }
        let deep = (0..MAX_DIRECTORY_LISTING_PREFIX_COMPONENTS + 1)
            .map(|i| format!("d{i}"))
            .collect::<Vec<_>>()
            .join("/");
        assert!(request(&deep).validate_prefix().is_err());
        let long = "a".repeat(MAX_DIRECTORY_LISTING_PREFIX_NAME_BYTES + 1);
        assert!(request(&long).validate_prefix().is_err());
    }

    #[test]
    fn listing_dto_roundtrip_preserves_status_and_truncation() {
        let result = WorkspaceDirectoryListResultDto {
            workspace_id: "w".to_owned(),
            prefix: "src".to_owned(),
            nodes: vec![WorkspaceDirectoryNodeDto {
                name: "lib.rs".to_owned(),
                path: "src/lib.rs".to_owned(),
                depth: 1,
                kind: WorkspaceDirectoryNodeKindDto::File,
                children: None,
            }],
            truncated: true,
            truncated_reason: Some("stopped at the maximum of 4096 entries".to_owned()),
            examined: 5000,
            status: WorkspaceDirectoryListStatusDto::Ready,
        };
        let round: WorkspaceDirectoryListResultDto =
            serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
        assert_eq!(round, result);
        // An older peer's result without the newer optional fields still
        // decodes instead of failing the whole listing.
        let legacy: WorkspaceDirectoryListResultDto =
            serde_json::from_str(r#"{"workspace_id":"w","prefix":"","status":"busy"}"#)
                .expect("legacy listing must still decode");
        assert_eq!(legacy.status, WorkspaceDirectoryListStatusDto::Busy);
        assert!(legacy.nodes.is_empty());
        assert!(!legacy.truncated);
    }

    #[test]
    fn a_global_agent_is_one_bare_toml_file() {
        assert_eq!(
            DocumentScope::GlobalAgent
                .validate_global_agent_filename("reviewer.toml")
                .as_deref(),
            Ok("reviewer.toml")
        );
        assert_eq!(
            DocumentScope::GlobalAgent
                .validate_global_agent_filename("my-agent_2.toml")
                .as_deref(),
            Ok("my-agent_2.toml")
        );
    }

    #[test]
    fn global_agent_containment_refuses_anything_that_is_not_a_flat_name() {
        // The global root is *narrower* than a workspace, not wider: no
        // traversal, no nesting, no absolute path, no other extension.
        for rejected in [
            "../escape.toml",
            "nested/reviewer.toml",
            "/etc/reviewer.toml",
            "reviewer.md",
            "reviewer",
            ".toml",
            "",
            "..",
            "reviewer.toml/",
        ] {
            assert!(
                DocumentScope::GlobalAgent
                    .validate_global_agent_filename(rejected)
                    .is_err(),
                "expected {rejected:?} to be refused"
            );
        }
    }

    #[test]
    fn global_agent_names_are_bounded() {
        assert!(DocumentScope::GlobalAgent
            .validate_global_agent_filename(&format!(
                "{}.toml",
                "a".repeat(MAX_GLOBAL_AGENT_NAME_CHARS)
            ))
            .is_ok());
        assert!(DocumentScope::GlobalAgent
            .validate_global_agent_filename(&format!(
                "{}.toml",
                "a".repeat(MAX_GLOBAL_AGENT_NAME_CHARS + 1)
            ))
            .is_err());
        // A dot is legal in a workspace filename but not in a global agent
        // name, so `a..b.toml` cannot smuggle a second component through.
        assert!(DocumentScope::GlobalAgent
            .validate_global_agent_filename("a..b.toml")
            .is_err());
    }

    #[test]
    fn only_the_global_scope_applies_the_global_agent_rule() {
        assert!(DocumentScope::Workspace
            .validate_global_agent_filename("reviewer.toml")
            .is_err());
    }

    #[test]
    fn a_scope_omitted_on_the_wire_means_workspace() {
        // A peer that predates global agent editing must keep working, and
        // it must keep getting the *narrow* scope, never an implicit global.
        let decoded: DocumentScope =
            serde_json::from_str("\"workspace\"").expect("workspace decodes");
        assert_eq!(decoded, DocumentScope::Workspace);
        assert_eq!(DocumentScope::default(), DocumentScope::Workspace);
        assert!(!DocumentScope::default().is_global_agent());
    }

    #[test]
    fn a_snapshot_without_a_scope_field_decodes_as_workspace() {
        // `scope` is `#[serde(default)]`, so an older peer's snapshot — which
        // has no such field — decodes instead of failing the whole document.
        let json = r#"{
            "document_id": "doc-1",
            "project_id": "p",
            "workspace_id": "w",
            "relative_path": "src/lib.rs",
            "revision": 1,
            "text": "x",
            "dirty": false,
            "conflicted": false,
            "writer": true,
            "disk_base_digest": "d"
        }"#;
        let dto: DocumentSnapshotDto =
            serde_json::from_str(json).expect("an old snapshot must still decode");
        assert_eq!(dto.scope, DocumentScope::Workspace);
        // Round-tripping keeps it explicit rather than silently dropping it.
        let round: DocumentSnapshotDto =
            serde_json::from_str(&serde_json::to_string(&dto).unwrap()).unwrap();
        assert_eq!(round.scope, DocumentScope::Workspace);
    }
}
