use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Wire shape of an explicit execution target.
///
/// The canonical forms are `{"kind":"local"}` and
/// `{"kind":"eggwork_node","node_id":"<table key>"}`. The node id names an
/// entry of the daemon's `[eggwork.nodes]` table — never an endpoint, TLS
/// key path, or any other credential-bearing locator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ExecutionTargetDto {
    /// Run on the local executor. The historical default.
    Local,
    /// Run on the named Eggwork node.
    EggworkNode {
        /// The `[eggwork.nodes]` table key of the target node.
        node_id: String,
    },
    /// The submitted target could not be decoded as a known target.
    ///
    /// This variant exists so that a malformed target produces a typed
    /// refusal from the daemon instead of failing the decode of the
    /// enclosing request. Transports drop frames they cannot decode
    /// (see `src/core/transport/daemon_socket.rs`), so a decode failure
    /// here would leave the caller waiting forever for a reply.
    ///
    /// The daemon never echoes this variant back to a client.
    Invalid {
        /// Human-readable reason the target was refused.
        reason: String,
    },
}

impl ExecutionTargetDto {
    /// Decode a wire value into a typed target without ever failing.
    ///
    /// Unknown kinds, a missing or empty node id, and any key beyond the
    /// documented shape decode to [`ExecutionTargetDto::Invalid`]. The
    /// caller turns that into a typed refusal.
    pub fn from_json_value(value: &serde_json::Value) -> Self {
        let Some(object) = value.as_object() else {
            return Self::Invalid {
                reason: "execution target must be an object".to_owned(),
            };
        };
        let Some(kind) = object.get("kind").and_then(serde_json::Value::as_str) else {
            return Self::Invalid {
                reason: "execution target is missing a string 'kind'".to_owned(),
            };
        };
        match kind {
            "local" => {
                let extra: Vec<&str> = object
                    .keys()
                    .filter(|key| key.as_str() != "kind")
                    .map(String::as_str)
                    .collect();
                if extra.is_empty() {
                    Self::Local
                } else {
                    Self::Invalid {
                        reason: format!(
                            "execution target 'local' does not accept {}",
                            extra.join(", ")
                        ),
                    }
                }
            }
            "eggwork_node" => {
                let extra: Vec<&str> = object
                    .keys()
                    .filter(|key| key.as_str() != "kind" && key.as_str() != "node_id")
                    .map(String::as_str)
                    .collect();
                if !extra.is_empty() {
                    return Self::Invalid {
                        reason: format!(
                            "execution target 'eggwork_node' does not accept {}",
                            extra.join(", ")
                        ),
                    };
                }
                let Some(node_id) = object.get("node_id").and_then(serde_json::Value::as_str)
                else {
                    return Self::Invalid {
                        reason: "execution target 'eggwork_node' requires a string 'node_id'"
                            .to_owned(),
                    };
                };
                if node_id.trim().is_empty() {
                    return Self::Invalid {
                        reason: "execution target 'eggwork_node' requires a non-empty 'node_id'"
                            .to_owned(),
                    };
                }
                Self::EggworkNode {
                    node_id: node_id.to_owned(),
                }
            }
            other => Self::Invalid {
                reason: format!("unknown execution target kind '{other}'"),
            },
        }
    }
}

impl<'de> Deserialize<'de> for ExecutionTargetDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self::from_json_value(&value))
    }
}

/// How strongly a remote resource limit must hold on the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteLimitEnforcementDto {
    /// The node must enforce the limit or the submission is refused.
    Required,
    /// The node may apply the limit; a node that cannot says so.
    BestEffort,
}

/// One remote resource limit on the wire.
///
/// The unit is carried by the *field* that holds this value, never by the
/// value itself:
///
/// - `memory_bytes` — bytes of resident memory.
/// - `cpu_quota_milli_percent` — milli-percent of one CPU's quota. This is
///   **not** a CPU-time budget: `1000` means one full CPU (`100.0%`).
/// - `pids` — process/thread count.
///
/// This is a physically-unit limit, deliberately distinct from the
/// abstract scheduler tokens (`cpu_weight`, `memory_mb_hint`) carried by
/// the job's resource request. The two never convert into one another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RemoteResourceLimitDto {
    /// The limit value in the dimension's own unit.
    pub value: u64,
    /// How strongly the limit must hold.
    pub enforcement: RemoteLimitEnforcementDto,
}

impl RemoteResourceLimitDto {
    /// Decode a wire value into a limit without ever failing.
    ///
    /// A value that is absent, not a number, or not an unsigned integer
    /// decodes to `0`, which the daemon refuses: a limit of zero is not
    /// "unlimited", it is an invalid limit. An unrecognized `enforcement`
    /// keyword decodes to `required` — the stricter reading — so an
    /// unknown keyword can never quietly weaken a limit. An unrecognized
    /// *key* is refused as well, so a credential-bearing locator cannot
    /// ride along inside a limit.
    pub fn from_json_value(value: &serde_json::Value) -> Self {
        let raw = value.as_object();
        if let Some(raw) = raw {
            let unknown: Vec<&str> = raw
                .keys()
                .map(String::as_str)
                .filter(|key| !matches!(*key, "value" | "enforcement"))
                .collect();
            if !unknown.is_empty() {
                return Self {
                    value: 0,
                    enforcement: RemoteLimitEnforcementDto::Required,
                };
            }
        }
        let limit = raw.and_then(|object| object.get("value"));
        let value = limit.and_then(serde_json::Value::as_u64).unwrap_or(0);
        let enforcement = match raw
            .and_then(|object| object.get("enforcement"))
            .and_then(serde_json::Value::as_str)
        {
            Some("best_effort") => RemoteLimitEnforcementDto::BestEffort,
            _ => RemoteLimitEnforcementDto::Required,
        };
        Self { value, enforcement }
    }
}

impl<'de> Deserialize<'de> for RemoteResourceLimitDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self::from_json_value(&value))
    }
}

/// A set of remote resource limits on the wire.
///
/// An absent dimension means the dimension is *not requested*. The whole
/// set is optional: omitting it preserves the pre-M003 behavior of running
/// remote work without resource limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RemoteResourceLimitsDto {
    /// A well-formed set of limits.
    Limits {
        /// Resident-memory limit in bytes.
        memory_bytes: Option<RemoteResourceLimitDto>,
        /// CPU quota limit in milli-percent of one CPU.
        cpu_quota_milli_percent: Option<RemoteResourceLimitDto>,
        /// Process/thread limit.
        pids: Option<RemoteResourceLimitDto>,
    },
    /// The submitted limits could not be understood.
    ///
    /// This variant exists so that a malformed limit produces a typed
    /// refusal from the daemon instead of failing the decode of the
    /// enclosing request. Transports drop frames they cannot decode, so a
    /// decode failure here would leave the caller waiting forever.
    ///
    /// The daemon never echoes this variant back to a client.
    Invalid {
        /// Human-readable reason the limits were refused.
        reason: String,
    },
}

impl RemoteResourceLimitsDto {
    /// Decode a wire value into a typed limit set without ever failing.
    ///
    /// An unrecognized key is an error rather than something to ignore: a
    /// client that sent `cpu_weight` here meant to constrain the node, and
    /// silently dropping the key would run the job unconstrained.
    pub fn from_json_value(value: &serde_json::Value) -> Self {
        let Some(object) = value.as_object() else {
            return Self::Invalid {
                reason: "remote resource limits must be an object".to_owned(),
            };
        };
        let known = ["kind", "memory_bytes", "cpu_quota_milli_percent", "pids"];
        let unknown: Vec<&str> = object
            .keys()
            .map(String::as_str)
            .filter(|key| !known.contains(key))
            .collect();
        if !unknown.is_empty() {
            return Self::Invalid {
                reason: format!(
                    "remote resource limits do not accept {}; expected memory_bytes, cpu_quota_milli_percent or pids",
                    unknown.join(", ")
                ),
            };
        }
        // The wire form is tagged, so `kind` is either absent (a bare
        // object) or exactly "limits". Anything else is a shape this
        // decoder does not understand.
        match object.get("kind").and_then(serde_json::Value::as_str) {
            None | Some("limits") => {}
            Some(other) => {
                return Self::Invalid {
                    reason: format!("unknown remote resource limits kind '{other}'"),
                };
            }
        }
        Self::Limits {
            // An explicit `null` dimension is an *absent* dimension, not a
            // zero limit. Treating it as zero would turn a request that
            // simply omits a dimension into one the daemon refuses.
            memory_bytes: object
                .get("memory_bytes")
                .filter(|value| !value.is_null())
                .map(RemoteResourceLimitDto::from_json_value),
            cpu_quota_milli_percent: object
                .get("cpu_quota_milli_percent")
                .filter(|value| !value.is_null())
                .map(RemoteResourceLimitDto::from_json_value),
            pids: object
                .get("pids")
                .filter(|value| !value.is_null())
                .map(RemoteResourceLimitDto::from_json_value),
        }
    }

    /// `true` when the set carries no dimension at all.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Limits {
                memory_bytes,
                cpu_quota_milli_percent,
                pids,
            } => memory_bytes.is_none() && cpu_quota_milli_percent.is_none() && pids.is_none(),
            Self::Invalid { .. } => false,
        }
    }
}

impl<'de> Deserialize<'de> for RemoteResourceLimitsDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self::from_json_value(&value))
    }
}

/// Bounds applied to project catalog payloads at the protocol boundary.
pub const MAX_PROJECT_LIST_ITEMS: usize = 128;
pub const MAX_PROJECT_WORKSPACES: usize = 64;
pub const MAX_PROJECT_TAGS: usize = 64;
pub const MAX_PROJECT_TEXT_LENGTH: usize = 1_024;
pub const MAX_PROJECT_PATH_LENGTH: usize = 4_096;

/// Canonical project/workspace context used by identity-aware protocol
/// requests and responses.
///
/// Legacy session identity and locator fields remain alongside this DTO for
/// wire compatibility. `locator_summary` is display-safe compatibility data,
/// not an identity authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectContextDto {
    pub project_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub repository_id: Option<String>,
    #[serde(default)]
    pub binding_state: Option<String>,
    #[serde(default)]
    pub binding_revision: Option<u64>,
    #[serde(default)]
    pub locator_summary: Option<String>,
}

/// Canonical session binding with explicitly named legacy locator data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBindingDto {
    pub project_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub repository_id: Option<String>,
    #[serde(default)]
    pub binding_state: Option<String>,
    #[serde(default)]
    pub binding_revision: Option<u64>,
    #[serde(default)]
    pub compatibility_directory: Option<String>,
}

/// Bounded project metadata used by catalog list and lifecycle responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSummaryDto {
    pub project_id: String,
    pub display_name: String,
    pub lifecycle: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub time_last_opened_at: Option<i64>,
    pub registration_source: String,
    #[serde(default)]
    pub archived_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Workspace metadata nested in a project response. The root is a bounded
/// display locator; it is never an identity field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectWorkspaceSummaryDto {
    pub workspace_id: String,
    pub display_name: String,
    #[serde(default)]
    pub canonical_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectHealthLayerDto {
    pub state: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

/// Path-free aggregate health for one explicit project/workspace scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectHealthDto {
    pub project_id: String,
    pub workspace_id: String,
    pub overall: String,
    pub catalog: ProjectHealthLayerDto,
    pub workspace: ProjectHealthLayerDto,
    pub assets: ProjectHealthLayerDto,
    pub services: ProjectHealthLayerDto,
    #[serde(default)]
    pub diagnostics: Vec<String>,
    /// Durable catalog health, when a project-get response includes it.
    #[serde(default)]
    pub durable: Option<ProjectHealthRecordDto>,
}

/// Durable, probe-free health record stored by the project catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectHealthRecordDto {
    pub project_id: String,
    pub status: String,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    pub source: String,
    pub evaluated_at: i64,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDetailsDto {
    pub project: ProjectSummaryDto,
    #[serde(default)]
    pub workspaces: Vec<ProjectWorkspaceSummaryDto>,
    pub session_count: usize,
    #[serde(default)]
    pub health: Option<ProjectHealthRecordDto>,
}

/// Local registration input. A workspace must already be registered; paths
/// are intentionally absent so a locator cannot become project identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRegisterRequestDto {
    pub workspace_id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub repository_id: Option<String>,
    #[serde(default = "default_project_registration_source")]
    pub source: String,
}

/// Explicitly confirmed destruction request. The session count captured by
/// the confirmation screen is rechecked atomically by the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDeleteRequestDto {
    pub project_id: String,
    pub expected_session_count: usize,
}

/// Rename and rebind a project to an already registered workspace root.
/// `expected_updated_at` is the optimistic concurrency token shown to the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectUpdateRequestDto {
    pub project_id: String,
    pub display_name: String,
    pub root: String,
    pub expected_updated_at: i64,
}

fn default_project_registration_source() -> String {
    "protocol".to_string()
}

/// Simplified session DTO for protocol messages.
/// Matches the wire format of `session::models::Session`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// Legacy string projection of the internal `ProjectId` relation.
    pub project_id: String,
    #[serde(default)]
    /// Legacy string projection of the internal `WorkspaceId` relation.
    pub workspace_id: Option<String>,
    /// Canonical project/workspace binding. Absent on legacy sessions.
    #[serde(default)]
    pub binding: Option<SessionBindingDto>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub slug: String,
    /// Filesystem locator retained for wire compatibility; not a project ID.
    pub directory: String,
    pub title: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub share_url: Option<String>,
    #[serde(default)]
    pub summary_additions: Option<i64>,
    #[serde(default)]
    pub summary_deletions: Option<i64>,
    #[serde(default)]
    pub summary_files: Option<i64>,
    #[serde(default)]
    pub summary_diffs: Option<serde_json::Value>,
    #[serde(default)]
    pub revert: Option<serde_json::Value>,
    #[serde(default)]
    pub permission: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Provider Connections Milestone 3: stable daemon-owned connection
    /// identity for this session. `None` for sessions that still use the
    /// legacy `agent`/`model` compatibility adapter.
    #[serde(default)]
    pub provider_connection_id: Option<String>,
    /// Provider Connections Milestone 3: revision of the selected connection
    /// at the time of selection. Stale revisions are rejected on update.
    #[serde(default)]
    pub provider_connection_revision: Option<u64>,
    /// Provider Connections Milestone 3: revision of the model catalog that
    /// contained the selected model ID.
    #[serde(default)]
    pub model_catalog_revision: Option<String>,
    /// Provider Connections Milestone 3: model ID selected for this session.
    #[serde(default)]
    pub selected_model_id: Option<String>,
    /// Legacy agent name. Retained for compatibility only.
    #[serde(default)]
    pub agent: Option<String>,
    /// Legacy `"provider/model"` string. Retained for compatibility only.
    #[serde(default)]
    pub model: Option<String>,
    pub time_created: i64,
    pub time_updated: i64,
    #[serde(default)]
    pub time_compacting: Option<i64>,
    #[serde(default)]
    pub time_archived: Option<i64>,
    #[serde(default)]
    pub time_deleted: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_context_round_trips_all_identity_fields() {
        let context = ProjectContextDto {
            project_id: "project-1".into(),
            workspace_id: "workspace-1".into(),
            repository_id: Some("repository-1".into()),
            binding_state: Some("bound".into()),
            binding_revision: Some(7),
            locator_summary: Some("project workspace".into()),
        };

        let json = serde_json::to_string(&context).unwrap();
        let decoded: ProjectContextDto = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, context);
    }

    #[test]
    fn legacy_session_fixture_defaults_canonical_binding() {
        let session: Session = serde_json::from_str(
            r#"{
                "id":"session-1",
                "project_id":"legacy-project",
                "directory":"/tmp/legacy",
                "title":"Legacy",
                "time_created":1,
                "time_updated":2
            }"#,
        )
        .unwrap();

        assert_eq!(session.binding, None);
        assert_eq!(session.workspace_id, None);
    }

    #[test]
    fn session_binding_round_trips_canonical_ids_and_compatibility_locator() {
        let binding = SessionBindingDto {
            project_id: "project-1".into(),
            workspace_id: "workspace-1".into(),
            repository_id: None,
            binding_state: Some("bound".into()),
            binding_revision: Some(3),
            compatibility_directory: Some("/tmp/legacy".into()),
        };

        let json = serde_json::to_string(&binding).unwrap();
        let decoded: SessionBindingDto = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, binding);
    }
}

/// Simplified message DTO for protocol messages.
/// Matches the wire format of `session::message::Message`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: MessageData,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageData {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(rename = "messageID")]
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub parts: Vec<PartInfo>,
    /// Author of this message. Absent on rows imported from other tools,
    /// which predate the field; readers fall back to inferring it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<MessageRole>,
}

/// Author of a persisted transcript message.
///
/// Mirrors `codegg_core::session::message::MessageRole`; the two types are
/// kept in step by the JSON round-trip in `message_to_dto`, which requires
/// identical serde attributes on both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(flatten)]
    pub data: PartData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartData {
    Text {
        text: String,
    },
    Reasoning {
        reasoning: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(default)]
        output: Option<String>,
        status: ToolStatus,
    },
    Image {
        url: String,
    },
    File {
        path: String,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Error,
}

/// Simplified agent DTO for protocol messages.
/// Matches the wire format of `agent::Agent`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub mode: AgentMode,
    #[serde(default)]
    pub mode_name: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub steps: Option<usize>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub permissions: HashMap<String, String>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub thinking_budget: Option<usize>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    #[default]
    Primary,
    Subagent,
    All,
}

/// Provider message DTO for protocol messages.
/// Matches the wire format of `provider::Message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum ProviderMessage {
    System {
        content: String,
    },
    User {
        content: Vec<ContentPart>,
    },
    Assistant {
        content: Vec<ContentPart>,
        #[serde(default)]
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

impl Default for ProviderMessage {
    fn default() -> Self {
        ProviderMessage::User {
            content: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text { text: String },
    Image { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Session template DTO.
/// Matches the wire format of `config::schema::SessionTemplate`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionTemplate {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub instructions: Option<Vec<String>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Wire-format snapshot of a registered workspace.
///
/// Phase 2 of the single-daemon plan adds this as a first-class peer of
/// `SessionSnapshot`. Phase 3 adds optional service-health fields so
/// remote clients can show whether a workspace has an active service
/// bundle and what its current lease accounting looks like.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub workspace_id: String,
    pub canonical_root: String,
    pub display_name: String,
    pub created_at: i64,
    pub last_opened_at: i64,
    #[serde(default)]
    pub archived_at: Option<i64>,
    pub active_sessions: usize,
    /// Phase 3: true when a workspace service bundle is currently
    /// active in the daemon. False when only the registry row exists.
    #[serde(default)]
    pub services_active: bool,
    /// Phase 3: active-lease count for the workspace service bundle.
    #[serde(default)]
    pub active_leases: usize,
    /// Phase 3: configuration snapshot revision.
    #[serde(default)]
    pub config_revision: u64,
}

/// Wire-format health snapshot for a workspace service bundle.
///
/// Phase 3: a single, redacted view of an active workspace service's
/// state. Includes the workspace id, the canonical root, the current
/// config revision, and active-lease accounting. Remote clients can
/// render this in their status bar without ever asking the daemon for
/// raw filesystem paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceServiceHealthDto {
    pub workspace_id: String,
    pub canonical_root: String,
    pub display_name: String,
    pub activated_at: i64,
    pub last_used_at: i64,
    pub active_leases: usize,
    pub config_revision: u64,
}

/// Bounded operator-facing projection of a daemon-managed worktree.  The
/// locator is included for local operator inspection; ownership remains
/// represented by opaque IDs and is never inferred from the path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedWorktreeDto {
    pub worktree_id: String,
    pub project_id: String,
    pub repository_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub node_id: Option<String>,
    pub repository_root: String,
    pub path: String,
    #[serde(default)]
    pub branch: Option<String>,
    pub base_commit: String,
    pub managed: bool,
    pub state: String,
    pub health: String,
    pub lease_generation: u64,
    #[serde(default)]
    pub owner_run_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Wire-format config diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDiagnosticDto {
    pub severity: String,
    pub source: String,
    pub message: String,
}

/// Run query parameters for the `RunList` request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunQueryDto {
    pub kinds: Vec<String>,
    pub statuses: Vec<String>,
    pub limit: usize,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
}

/// Compact run summary for `RunList`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummaryDto {
    pub run_id: String,
    pub kind: String,
    pub status: String,
    pub started_at_ms: i64,
    #[serde(default)]
    pub completed_at_ms: Option<i64>,
    pub command: String,
    pub workspace_id: Option<String>,
}

/// Full run record for `RunGet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecordDto {
    pub run_id: String,
    pub kind: String,
    pub status: String,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub command: String,
    pub argv: Vec<String>,
    pub backend_family: Option<String>,
    pub backend_detail: Option<String>,
    pub workspace_id: Option<String>,
    pub artifacts: Vec<RunArtifactSummaryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunArtifactSummaryDto {
    pub artifact_id: String,
    pub kind: String,
    pub size: u64,
    pub sha256: Option<String>,
}

// ── Phase 4: Durable Jobs and Schedules DTOs ────────────────────────

/// Compact job summary for `JobList`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSummaryDto {
    pub job_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub kind: String,
    pub priority: String,
    pub state: String,
    pub attempt_count: u32,
    #[serde(default)]
    pub current_attempt_id: Option<String>,
    #[serde(default)]
    pub schedule_id: Option<String>,
    #[serde(default)]
    pub cancel_requested_at: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
    /// Identity of the node this job targets.
    ///
    /// `None` for local jobs. This is identity only: no endpoint, key
    /// path, or other credential-bearing locator is ever exposed.
    #[serde(default)]
    pub target_node_id: Option<String>,
}

/// Full job record for `JobGet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecordDto {
    pub job_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub kind: String,
    pub source: serde_json::Value,
    pub priority: String,
    pub payload: serde_json::Value,
    pub state: String,
    #[serde(default)]
    pub current_attempt_id: Option<String>,
    pub attempt_count: u32,
    pub retry_policy: serde_json::Value,
    pub idempotency: String,
    #[serde(default)]
    pub not_before: Option<i64>,
    #[serde(default)]
    pub deadline: Option<i64>,
    #[serde(default)]
    pub schedule_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub terminal_at_ms: Option<i64>,
    #[serde(default)]
    pub cancel_requested_at: Option<i64>,
    #[serde(default)]
    pub cancel_reason: Option<String>,
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
    /// Identity of the node this job targets.
    ///
    /// `None` for local jobs. This is identity only: no endpoint, key
    /// path, or other credential-bearing locator is ever exposed.
    #[serde(default)]
    pub target_node_id: Option<String>,
}

/// Execution attempt record for `JobAttempts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobAttemptDto {
    pub attempt_id: String,
    pub job_id: String,
    pub sequence: u32,
    pub state: String,
    pub daemon_generation: String,
    #[serde(default)]
    pub executor: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub heartbeat_at_ms: Option<i64>,
    #[serde(default)]
    pub started_at_ms: Option<i64>,
    #[serde(default)]
    pub completed_at_ms: Option<i64>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub error_class: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Compact schedule summary for `ScheduleList`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleSummaryDto {
    pub schedule_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub kind: serde_json::Value,
    pub state: String,
    pub overlap_policy: String,
    pub missed_run_policy: serde_json::Value,
    #[serde(default)]
    pub next_run_at_ms: Option<i64>,
    #[serde(default)]
    pub last_occurrence_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Full schedule record for `ScheduleGet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRecordDto {
    pub schedule_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub kind: serde_json::Value,
    pub job_template: serde_json::Value,
    pub state: String,
    pub overlap_policy: String,
    pub missed_run_policy: serde_json::Value,
    #[serde(default)]
    pub next_run_at_ms: Option<i64>,
    #[serde(default)]
    pub last_occurrence_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Identity of the node this schedule's jobs target.
    ///
    /// `None` for local schedules. This is identity only: no endpoint,
    /// key path, or other credential-bearing locator is ever exposed.
    #[serde(default)]
    pub target_node_id: Option<String>,
}

/// C005 (ADR-0031): durable host grant for one background wait, as
/// projected to clients. Identity fields are server-resolved at grant
/// time; the client never supplies them, so the request shapes carry
/// only locators and bounds (see `CoreRequest::ContinuationGrantCreate`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuationGrantDto {
    pub grant_id: String,
    pub notice_job_id: String,
    pub session_id: String,
    pub owner_principal: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub allowed_model: Option<String>,
    #[serde(default)]
    pub allowed_connection: Option<String>,
    pub max_continuations: u32,
    #[serde(default)]
    pub used_continuations: u32,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
    #[serde(default)]
    pub max_duration_ms: Option<i64>,
    pub status: String,
    pub expiry_ms: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Query parameters for `JobList`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct JobQueryDto {
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub limit: u32,
}

/// Parameters for `JobSubmit`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct JobSubmitDto {
    /// Client retry identity. The daemon treats it as opaque and only
    /// applies it within the current daemon generation.
    #[serde(default)]
    pub submission_key: Option<String>,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub kind: String,
    pub priority: String,
    pub source: serde_json::Value,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub timeout_ms: Option<i64>,
    #[serde(default)]
    pub retry_max_attempts: u32,
    #[serde(default)]
    pub retryable_failures: Vec<String>,
    pub idempotency: String,
    #[serde(default)]
    pub not_before_ms: Option<i64>,
    #[serde(default)]
    pub deadline_ms: Option<i64>,
    #[serde(default)]
    pub schedule_id: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
    /// Explicit execution target for this job.
    ///
    /// Omitted (or `null`) means the local executor, which is exactly the
    /// behavior of every client that predates explicit targeting. A remote
    /// target names a configured Eggwork node; the daemon refuses it
    /// unless the principal holds the node-target capability and the
    /// administrator approved that node.
    #[serde(default)]
    pub target: Option<ExecutionTargetDto>,
    /// Optional remote resource limits for a job that targets a node.
    ///
    /// These are physical limits in the node's own units, never the
    /// abstract scheduler tokens. The daemon refuses any request that
    /// exceeds the operator's configured ceiling for the target node, and
    /// refuses any request that would weaken a `required` limit.
    #[serde(default)]
    pub remote_resources: Option<RemoteResourceLimitsDto>,
}

/// Parameters for `ScheduleCreate`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduleCreateDto {
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub kind: serde_json::Value,
    pub job_template: serde_json::Value,
    pub overlap_policy: String,
    pub missed_run_policy: serde_json::Value,
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
}

/// Summary of a daemon generation recovery pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryReportDto {
    pub interrupted_attempts: u32,
    pub requeued_jobs: u32,
    pub terminal_jobs: u32,
    pub schedules_reconciled: u32,
}

/// Outcome of a cancellation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelResultDto {
    pub job_id: String,
    /// One of: "cancelled", "requested", "already_terminal".
    pub outcome: String,
    pub terminal: bool,
}

/// Bounded summary of an edit checkpoint for undo/reapply listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditCheckpointSummaryDto {
    pub id: String,
    pub workspace_id: String,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub batch_seq: i64,
    pub created_at: i64,
    pub file_count: usize,
    pub paths: Vec<String>,
    pub restorable: bool,
}

/// Detail view of a single edit checkpoint (bounded, no file bodies).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditCheckpointDetailDto {
    pub id: String,
    pub workspace_id: String,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub batch_seq: i64,
    pub created_at: i64,
    pub files: Vec<EditCheckpointFileSummaryDto>,
}

/// Per-file summary within a checkpoint detail (path + present/absent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditCheckpointFileSummaryDto {
    pub path: String,
    pub pre_present: bool,
    pub post_present: bool,
}

/// Typed result of a checked undo/reapply operation (bounded, no file bodies).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditRestoreResultDto {
    Applied {
        checkpoint_id: String,
        workspace_id: String,
        session_id: String,
        direction: String,
        restored_paths: Vec<String>,
    },
    Conflict {
        checkpoint_id: String,
        workspace_id: String,
        direction: String,
        stale_paths: Vec<String>,
    },
    NotFound {
        checkpoint_id: String,
    },
    WrongWorkspace {
        checkpoint_id: String,
        expected_workspace: String,
        actual_workspace: String,
    },
    WrongSession {
        checkpoint_id: String,
        expected_session: String,
        actual_session: String,
    },
    PathValidationFailed {
        checkpoint_id: String,
        invalid_paths: Vec<String>,
        reason: String,
    },
    PermissionDenied {
        checkpoint_id: String,
        denied_paths: Vec<String>,
        reason: String,
    },
    PartialFailure {
        checkpoint_id: String,
        workspace_id: String,
        direction: String,
        applied_paths: Vec<String>,
        failed_paths: Vec<String>,
        error: String,
    },
    Unsupported {
        checkpoint_id: String,
        reason: String,
    },
}

#[cfg(test)]
mod execution_target_tests {
    use super::*;

    #[test]
    fn omitted_target_decodes_as_local() {
        let raw = serde_json::json!({
            "workspace_id": "workspace-1",
            "kind": "build",
            "priority": "background",
            "source": {"kind": "interactive"},
            "payload": {"kind": "shell"},
            "idempotency": "safe_repeat"
        });
        let dto: JobSubmitDto = serde_json::from_value(raw).expect("legacy submit decodes");
        assert_eq!(dto.target, None);
    }

    #[test]
    fn explicit_local_target_decodes() {
        let dto: JobSubmitDto = serde_json::from_value(serde_json::json!({
            "workspace_id": "workspace-1",
            "kind": "build",
            "priority": "background",
            "source": {"kind": "interactive"},
            "payload": {"kind": "shell"},
            "idempotency": "safe_repeat",
            "target": {"kind": "local"}
        }))
        .expect("local target decodes");
        assert_eq!(dto.target, Some(ExecutionTargetDto::Local));
    }

    #[test]
    fn named_node_target_decodes() {
        let dto: JobSubmitDto = serde_json::from_value(serde_json::json!({
            "workspace_id": "workspace-1",
            "kind": "build",
            "priority": "background",
            "source": {"kind": "interactive"},
            "payload": {"kind": "shell"},
            "idempotency": "safe_repeat",
            "target": {"kind": "eggwork_node", "node_id": "builder"}
        }))
        .expect("node target decodes");
        assert_eq!(
            dto.target,
            Some(ExecutionTargetDto::EggworkNode {
                node_id: "builder".to_owned()
            })
        );
    }

    #[test]
    fn unknown_kind_decodes_to_invalid_instead_of_failing_the_request() {
        let dto: JobSubmitDto = serde_json::from_value(serde_json::json!({
            "workspace_id": "workspace-1",
            "kind": "build",
            "priority": "background",
            "source": {"kind": "interactive"},
            "payload": {"kind": "shell"},
            "idempotency": "safe_repeat",
            "target": {"kind": "kubernetes_node", "node_id": "builder"}
        }))
        .expect("unknown kind still decodes the request");
        match dto.target {
            Some(ExecutionTargetDto::Invalid { reason }) => {
                assert!(reason.contains("kubernetes_node"), "{reason}");
            }
            other => panic!("expected an invalid target, got {other:?}"),
        }
    }

    #[test]
    fn empty_node_id_decodes_to_invalid() {
        let target = ExecutionTargetDto::from_json_value(&serde_json::json!({
            "kind": "eggwork_node",
            "node_id": "   "
        }));
        assert!(matches!(target, ExecutionTargetDto::Invalid { .. }));
    }

    #[test]
    fn endpoint_override_is_rejected() {
        // A target may name a node, never an endpoint or a key path.
        let target = ExecutionTargetDto::from_json_value(&serde_json::json!({
            "kind": "eggwork_node",
            "node_id": "builder",
            "endpoint": "https://attacker.invalid"
        }));
        match target {
            ExecutionTargetDto::Invalid { reason } => {
                assert!(reason.contains("endpoint"), "{reason}");
            }
            other => panic!("expected an invalid target, got {other:?}"),
        }
    }

    #[test]
    fn missing_node_id_decodes_to_invalid() {
        let target =
            ExecutionTargetDto::from_json_value(&serde_json::json!({"kind": "eggwork_node"}));
        assert!(matches!(target, ExecutionTargetDto::Invalid { .. }));
    }

    #[test]
    fn target_round_trips_through_the_wire_form() {
        let target = ExecutionTargetDto::EggworkNode {
            node_id: "builder".to_owned(),
        };
        let json = serde_json::to_value(&target).unwrap();
        assert_eq!(json["kind"], "eggwork_node");
        assert_eq!(json["node_id"], "builder");
        let decoded: ExecutionTargetDto = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, target);
    }

    // ── M003: remote resource limits ────────────────────────────────────

    #[test]
    fn remote_limits_decode_into_their_dimensions() {
        let limits = RemoteResourceLimitsDto::from_json_value(&serde_json::json!({
            "memory_bytes": {"value": 536870912u64, "enforcement": "required"},
            "cpu_quota_milli_percent": {"value": 1000u64, "enforcement": "best_effort"},
            "pids": {"value": 64u64}
        }));
        let RemoteResourceLimitsDto::Limits {
            memory_bytes,
            cpu_quota_milli_percent,
            pids,
        } = limits
        else {
            panic!("expected a decoded limit set")
        };
        assert_eq!(
            memory_bytes,
            Some(RemoteResourceLimitDto {
                value: 536_870_912,
                enforcement: RemoteLimitEnforcementDto::Required,
            })
        );
        assert_eq!(
            cpu_quota_milli_percent,
            Some(RemoteResourceLimitDto {
                value: 1000,
                enforcement: RemoteLimitEnforcementDto::BestEffort,
            })
        );
        // An absent enforcement keyword decodes to the stricter reading.
        assert_eq!(
            pids,
            Some(RemoteResourceLimitDto {
                value: 64,
                enforcement: RemoteLimitEnforcementDto::Required,
            })
        );
    }

    #[test]
    fn an_unknown_enforcement_keyword_never_decodes_to_best_effort() {
        let limit = RemoteResourceLimitDto::from_json_value(&serde_json::json!({
            "value": 10u64,
            "enforcement": "best-effort"
        }));
        assert_eq!(limit.enforcement, RemoteLimitEnforcementDto::Required);
    }

    #[test]
    fn an_unusable_value_decodes_to_zero_rather_than_failing() {
        // A non-numeric value must not fail the enclosing frame decode:
        // transports drop frames they cannot decode.
        let limit = RemoteResourceLimitDto::from_json_value(&serde_json::json!({
            "value": "536870912"
        }));
        assert_eq!(limit.value, 0);
        let limit = RemoteResourceLimitDto::from_json_value(&serde_json::json!("nope"));
        assert_eq!(limit.value, 0);
    }

    #[test]
    fn an_unknown_dimension_is_refused_rather_than_ignored() {
        // `cpu_weight` is an abstract scheduler token, not a remote limit.
        // Silently ignoring it would run the job unconstrained.
        let limits = RemoteResourceLimitsDto::from_json_value(&serde_json::json!({
            "cpu_weight": 3
        }));
        assert!(matches!(limits, RemoteResourceLimitsDto::Invalid { .. }));
    }

    #[test]
    fn a_non_object_limit_set_is_refused() {
        let limits = RemoteResourceLimitsDto::from_json_value(&serde_json::json!(7));
        assert!(matches!(limits, RemoteResourceLimitsDto::Invalid { .. }));
    }

    #[test]
    fn an_empty_limit_set_carries_no_dimension() {
        let limits = RemoteResourceLimitsDto::from_json_value(&serde_json::json!({}));
        assert!(limits.is_empty());
    }

    #[test]
    fn an_explicit_null_dimension_is_absent_not_a_zero_limit() {
        // A `null` dimension means "not requested". Decoding it as a zero
        // limit would make the daemon refuse a request that merely omits
        // a dimension.
        let limits = RemoteResourceLimitsDto::from_json_value(&serde_json::json!({
            "memory_bytes": {"value": 1024u64},
            "cpu_quota_milli_percent": null,
            "pids": null
        }));
        let RemoteResourceLimitsDto::Limits {
            memory_bytes,
            cpu_quota_milli_percent,
            pids,
        } = limits
        else {
            panic!("expected a decoded limit set")
        };
        assert_eq!(
            memory_bytes,
            Some(RemoteResourceLimitDto {
                value: 1024,
                enforcement: RemoteLimitEnforcementDto::Required,
            })
        );
        assert_eq!(cpu_quota_milli_percent, None);
        assert_eq!(pids, None);
    }

    #[test]
    fn remote_limits_round_trip_through_the_wire_form() {
        let limits = RemoteResourceLimitsDto::Limits {
            memory_bytes: Some(RemoteResourceLimitDto {
                value: 1024,
                enforcement: RemoteLimitEnforcementDto::Required,
            }),
            cpu_quota_milli_percent: None,
            pids: None,
        };
        let json = serde_json::to_value(&limits).unwrap();
        assert_eq!(json["kind"], "limits");
        assert_eq!(json["memory_bytes"]["value"], 1024);
        let decoded: RemoteResourceLimitsDto = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, limits);
    }
}
