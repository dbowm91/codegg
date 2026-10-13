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
