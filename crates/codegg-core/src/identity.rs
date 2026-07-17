//! Typed domain identities and path-independent relation contracts.
//!
//! Identity values are opaque strings. They are generated as UUIDv4 values,
//! but persisted values are validated by the shared lexical contract rather
//! than by UUID parsing so future stores can use stable, non-UUID identifiers
//! without changing the domain boundary. Paths are locators and are never
//! accepted as identity input.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

pub use crate::workspace::WorkspaceId;

/// Maximum UTF-8 byte length accepted for a persisted or protocol identity.
pub const MAX_ID_LENGTH: usize = 128;

/// Marker for the future static path-derived-project-identity guard.
///
/// This is intentionally a narrow seam, not an enabled repository-wide
/// prohibition. Later project-storage work can make a guard scan for this
/// marker and reject path-to-identity conversions at their call sites.
pub const PATH_IDENTITY_GUARD_MARKER: &str =
    "identity-path-guard: project IDs must not be derived from paths";

/// Reasons an untrusted identity string is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityParseReason {
    Empty,
    TooLong,
    PathLike,
    Nul,
    Control,
    Whitespace,
    InvalidCharacter,
}

impl fmt::Display for IdentityParseReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "value is empty",
            Self::TooLong => "value exceeds the maximum length",
            Self::PathLike => "value contains a path separator",
            Self::Nul => "value contains a NUL byte",
            Self::Control => "value contains a control character",
            Self::Whitespace => "value contains whitespace",
            Self::InvalidCharacter => "value contains an unsupported character",
        };
        f.write_str(message)
    }
}

/// A safe, kind-aware parse failure for a domain identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid {kind}: {reason}")]
pub struct IdentityParseError {
    kind: &'static str,
    reason: IdentityParseReason,
}

impl IdentityParseError {
    /// The identity kind that failed validation, such as `project_id`.
    pub const fn kind(self) -> &'static str {
        self.kind
    }

    /// The bounded, non-secret reason for rejection.
    pub const fn reason(self) -> IdentityParseReason {
        self.reason
    }
}

/// Validate the common identity lexical contract without allocating.
pub(crate) fn validate_identity(kind: &'static str, value: &str) -> Result<(), IdentityParseError> {
    let reason = if value.is_empty() {
        Some(IdentityParseReason::Empty)
    } else if value.len() > MAX_ID_LENGTH {
        Some(IdentityParseReason::TooLong)
    } else if value.bytes().any(|byte| byte == b'\0') {
        Some(IdentityParseReason::Nul)
    } else if value.contains('/') || value.contains('\\') {
        Some(IdentityParseReason::PathLike)
    } else if value.chars().any(char::is_control) {
        Some(IdentityParseReason::Control)
    } else if value.chars().any(char::is_whitespace) {
        Some(IdentityParseReason::Whitespace)
    } else if value
        .bytes()
        .any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'))
    {
        Some(IdentityParseReason::InvalidCharacter)
    } else {
        None
    };

    match reason {
        Some(reason) => Err(IdentityParseError { kind, reason }),
        None => Ok(()),
    }
}

macro_rules! typed_identity {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Generate a fresh opaque identity.
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }

            /// Parse a persisted or protocol identity before accepting it.
            pub fn parse(value: &str) -> Result<Self, $crate::identity::IdentityParseError> {
                $crate::identity::validate_identity($kind, value)?;
                Ok(Self(value.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = $crate::identity::IdentityParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = $crate::identity::IdentityParseError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct IdentityVisitor;

                impl<'de> Visitor<'de> for IdentityVisitor {
                    type Value = $name;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("a bounded opaque identity string")
                    }

                    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
                    where
                        E: de::Error,
                    {
                        $name::parse(value).map_err(E::custom)
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                    where
                        E: de::Error,
                    {
                        $name::parse(value).map_err(E::custom)
                    }

                    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
                    where
                        E: de::Error,
                    {
                        $name::parse(&value).map_err(E::custom)
                    }
                }

                deserializer.deserialize_str(IdentityVisitor)
            }
        }
    };
}

typed_identity!(
    /// Stable identity for a logical project. It is independent of paths and
    /// repository/workspace locators.
    ProjectId,
    "project_id"
);
typed_identity!(
    /// Stable identity for a repository associated with a project.
    RepositoryId,
    "repository_id"
);
typed_identity!(
    /// Stable identity for a worktree checkout or equivalent working copy.
    WorktreeId,
    "worktree_id"
);
typed_identity!(
    /// Stable identity for a daemon or remote execution node.
    NodeId,
    "node_id"
);
typed_identity!(
    /// Stable identity for a human, service, or other authorization principal.
    PrincipalId,
    "principal_id"
);
typed_identity!(
    /// Stable identity for an agent execution.
    AgentRunId,
    "agent_run_id"
);
typed_identity!(
    /// Stable identity for a task within an agent execution.
    AgentTaskId,
    "agent_task_id"
);
typed_identity!(
    /// Stable identity for a configured provider connection.
    ProviderConnectionId,
    "provider_connection_id"
);
typed_identity!(
    /// Stable identity for a user or system communication channel.
    ChannelId,
    "channel_id"
);
typed_identity!(
    /// Stable identity for an append-only audit event.
    AuditEventId,
    "audit_event_id"
);

/// Project/repository relation. The repository is optional at the enclosing
/// [`ProjectBinding`] level while a project is being created or resolved.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectRepositoryBinding {
    pub project_id: ProjectId,
    pub repository_id: RepositoryId,
}

/// A path-independent project binding for one workspace registration.
///
/// Multiple values may share the same project and repository IDs while
/// carrying distinct workspace, worktree, or node identities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectBinding {
    pub project_id: ProjectId,
    #[serde(default)]
    pub repository_id: Option<RepositoryId>,
    pub workspace_id: WorkspaceId,
    #[serde(default)]
    pub worktree_id: Option<WorktreeId>,
    #[serde(default)]
    pub node_id: Option<NodeId>,
}

impl ProjectBinding {
    pub fn new(project_id: ProjectId, workspace_id: WorkspaceId) -> Self {
        Self {
            project_id,
            repository_id: None,
            workspace_id,
            worktree_id: None,
            node_id: None,
        }
    }

    pub fn with_repository(mut self, repository_id: RepositoryId) -> Self {
        self.repository_id = Some(repository_id);
        self
    }

    pub fn with_worktree(mut self, worktree_id: WorktreeId) -> Self {
        self.worktree_id = Some(worktree_id);
        self
    }

    pub fn with_node(mut self, node_id: NodeId) -> Self {
        self.node_id = Some(node_id);
        self
    }
}

/// Canonical session relation. Session storage and protocol fields remain
/// string-backed in this milestone; this value is the internal typed seam for
/// the later additive session binding migration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionBinding {
    pub project_id: ProjectId,
    pub workspace_id: WorkspaceId,
}

impl SessionBinding {
    pub fn new(project_id: ProjectId, workspace_id: WorkspaceId) -> Self {
        Self {
            project_id,
            workspace_id,
        }
    }
}

/// Existing session and protocol fields that remain compatibility projections
/// until later storage/protocol migrations can introduce typed fields.
pub mod legacy {
    /// A legacy string field that projects logical project identity.
    pub const PROJECT_ID_FIELD: &str = "project_id";
    /// A string representation of the current workspace binding on a
    /// compatibility boundary.
    pub const WORKSPACE_ID_FIELD: &str = "workspace_id";
    /// A legacy filesystem locator; it is not a project identity.
    pub const DIRECTORY_FIELD: &str = "directory";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum IdentityProjectionField {
        ProjectId,
        WorkspaceId,
        Directory,
    }

    impl IdentityProjectionField {
        pub const fn field_name(self) -> &'static str {
            match self {
                Self::ProjectId => PROJECT_ID_FIELD,
                Self::WorkspaceId => WORKSPACE_ID_FIELD,
                Self::Directory => DIRECTORY_FIELD,
            }
        }
    }

    pub fn is_identity_projection_field(field: &str) -> bool {
        matches!(
            field,
            PROJECT_ID_FIELD | WORKSPACE_ID_FIELD | DIRECTORY_FIELD
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet};

    use super::*;

    macro_rules! identity_round_trip_tests {
        ($($name:ident => $fixture:literal),+ $(,)?) => {
            $(
                let parsed = $name::parse($fixture).expect("fixture should be valid");
                assert_eq!(parsed.as_str(), $fixture);
                assert_eq!(parsed.to_string(), $fixture);
                assert_eq!(parsed.clone().into_string(), $fixture);
                let json = serde_json::to_string(&parsed).expect("serialize");
                assert_eq!(json, format!("\"{}\"", $fixture));
                let decoded: $name = serde_json::from_str(&json).expect("deserialize");
                assert_eq!(decoded, parsed);
                assert_eq!(<$name as std::str::FromStr>::from_str($fixture).unwrap(), parsed);
            )+
        };
    }

    #[test]
    fn every_identity_round_trips_as_a_string() {
        identity_round_trip_tests!(
            ProjectId => "project-fixture",
            RepositoryId => "repository_fixture",
            WorktreeId => "worktree-fixture",
            NodeId => "node-fixture",
            PrincipalId => "principal-fixture",
            AgentRunId => "agent-run-fixture",
            AgentTaskId => "agent-task-fixture",
            ProviderConnectionId => "provider-connection-fixture",
            ChannelId => "channel-fixture",
            AuditEventId => "audit-event-fixture",
            WorkspaceId => "workspace-fixture",
        );
    }

    #[test]
    fn generated_ids_are_valid_and_unique_in_a_contention_sample() {
        let handles = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    (0..256)
                        .map(|_| ProjectId::new())
                        .collect::<Vec<ProjectId>>()
                })
            })
            .collect::<Vec<_>>();
        let mut seen = HashSet::new();
        for handle in handles {
            for id in handle.join().expect("identity generator thread") {
                assert!(ProjectId::parse(id.as_str()).is_ok());
                assert!(seen.insert(id));
            }
        }
        assert_eq!(seen.len(), 8 * 256);
    }

    #[test]
    fn identities_support_ordering_and_hash_map_keys() {
        let mut ordered = BTreeSet::new();
        ordered.insert(ProjectId::parse("project-b").unwrap());
        ordered.insert(ProjectId::parse("project-a").unwrap());
        assert_eq!(
            ordered
                .into_iter()
                .map(|id| id.into_string())
                .collect::<Vec<_>>(),
            vec!["project-a", "project-b"]
        );

        let project = ProjectId::parse("project-a").unwrap();
        let mut values = HashSet::new();
        values.insert(project.clone());
        assert!(values.contains(&project));
    }

    #[test]
    fn invalid_values_are_rejected_before_becoming_owned() {
        let invalid = [
            ("", IdentityParseReason::Empty),
            ("a/b", IdentityParseReason::PathLike),
            (r"a\\b", IdentityParseReason::PathLike),
            ("a\0b", IdentityParseReason::Nul),
            ("a\nb", IdentityParseReason::Control),
            ("a b", IdentityParseReason::Whitespace),
            ("a.b", IdentityParseReason::InvalidCharacter),
            ("a:b", IdentityParseReason::InvalidCharacter),
        ];
        for (value, reason) in invalid {
            let error = ProjectId::parse(value).expect_err(value);
            assert_eq!(error.kind(), "project_id");
            assert_eq!(error.reason(), reason);
        }

        let oversized = "x".repeat(MAX_ID_LENGTH + 1);
        let error = ProjectId::parse(&oversized).expect_err("oversized");
        assert_eq!(error.reason(), IdentityParseReason::TooLong);
    }

    #[test]
    fn project_and_session_relations_are_path_independent() {
        let project = ProjectId::parse("project-1").unwrap();
        let repository = RepositoryId::parse("repo-1").unwrap();
        let workspace_a = WorkspaceId::parse("workspace-a").unwrap();
        let workspace_b = WorkspaceId::parse("workspace-b").unwrap();

        let binding_a =
            ProjectBinding::new(project.clone(), workspace_a).with_repository(repository.clone());
        let binding_b =
            ProjectBinding::new(project.clone(), workspace_b).with_repository(repository.clone());
        assert_eq!(binding_a.project_id, binding_b.project_id);
        assert_eq!(binding_a.repository_id, binding_b.repository_id);
        assert_ne!(binding_a.workspace_id, binding_b.workspace_id);

        let session = SessionBinding::new(project, binding_a.workspace_id.clone());
        assert_eq!(session.workspace_id, binding_a.workspace_id);
        assert!(ProjectId::parse("/tmp/project").is_err());

        let json = serde_json::to_string(&binding_a).expect("serialize binding");
        let decoded: ProjectBinding = serde_json::from_str(&json).expect("deserialize binding");
        assert_eq!(decoded, binding_a);

        let json = serde_json::to_string(&session).expect("serialize session binding");
        let decoded: SessionBinding = serde_json::from_str(&json).expect("deserialize binding");
        assert_eq!(decoded, session);
    }

    #[test]
    fn legacy_fields_are_explicitly_classified() {
        use legacy::{is_identity_projection_field, IdentityProjectionField};

        assert_eq!(
            IdentityProjectionField::ProjectId.field_name(),
            "project_id"
        );
        assert!(is_identity_projection_field("project_id"));
        assert!(is_identity_projection_field("directory"));
        assert!(!is_identity_projection_field("repository_id"));
    }
}
