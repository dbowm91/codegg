//! Session models and core traits.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// Legacy string projection. Internal typed session relations use
    /// [`crate::identity::ProjectId`]; this field remains wire/storage
    /// compatible until a later additive migration.
    pub project_id: String,
    /// Legacy string projection of the registered workspace identity.
    pub workspace_id: Option<String>,
    pub parent_id: Option<String>,
    pub slug: String,
    /// Filesystem locator retained for compatibility; it is not a project ID.
    pub directory: String,
    pub title: String,
    pub version: String,
    pub share_url: Option<String>,
    pub summary_additions: Option<i64>,
    pub summary_deletions: Option<i64>,
    pub summary_files: Option<i64>,
    pub summary_diffs: Option<serde_json::Value>,
    pub revert: Option<serde_json::Value>,
    pub permission: Option<serde_json::Value>,
    pub tags: Vec<String>,
    /// Provider Connections Milestone 3: stable daemon-owned connection
    /// identity. When set, the session resolves its runtime provider through
    /// `ConnectionManager` instead of the legacy `provider` registration path.
    /// This is the authoritative selection field; legacy `agent`/`model`
    /// strings remain readable through the compatibility adapter only.
    pub provider_connection_id: Option<String>,
    /// Provider Connections Milestone 3: revision of the selected connection
    /// at the time of selection. Stale revisions cause a typed conflict on
    /// update and force re-resolution through the daemon.
    pub provider_connection_revision: Option<u64>,
    /// Provider Connections Milestone 3: revision of the model catalog that
    /// contained the selected `selected_model_id`. Stale catalog revisions
    /// surface as a typed diagnostic rather than silently choosing another
    /// model.
    pub model_catalog_revision: Option<String>,
    /// Provider Connections Milestone 3: model identifier selected for this
    /// session. Distinct from the legacy `model` field; this is a stable ID
    /// drawn from the connection's bounded catalog.
    pub selected_model_id: Option<String>,
    /// Legacy agent name; retained for compatibility and not used as
    /// authoritative provider identity.
    pub agent: Option<String>,
    /// Legacy `"provider/model"` string; retained for compatibility. The
    /// authoritative selection lives in `provider_connection_id` +
    /// `selected_model_id`.
    pub model: Option<String>,
    pub time_created: i64,
    pub time_updated: i64,
    pub time_compacting: Option<i64>,
    pub time_archived: Option<i64>,
    pub time_deleted: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSession {
    /// Legacy string projection; do not derive a durable project identity from
    /// this field.
    pub project_id: String,
    /// Filesystem locator retained for compatibility.
    pub directory: String,
    pub title: Option<String>,
    pub parent_id: Option<String>,
    /// Legacy string projection of the registered workspace identity.
    pub workspace_id: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub tags: Option<Vec<String>>,
    /// Provider Connections Milestone 3: optional initial connection ID.
    pub provider_connection_id: Option<String>,
    /// Provider Connections Milestone 3: optional initial connection
    /// revision at the time of session creation.
    pub provider_connection_revision: Option<u64>,
    /// Provider Connections Milestone 3: optional initial model catalog
    /// revision.
    pub model_catalog_revision: Option<String>,
    /// Provider Connections Milestone 3: optional initial model ID.
    pub selected_model_id: Option<String>,
}

impl Default for CreateSession {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            directory: String::new(),
            title: None,
            parent_id: None,
            workspace_id: None,
            agent: None,
            model: None,
            tags: None,
            provider_connection_id: None,
            provider_connection_revision: None,
            model_catalog_revision: None,
            selected_model_id: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateSession {
    pub title: Option<String>,
    pub share_url: Option<String>,
    pub summary_additions: Option<i64>,
    pub summary_deletions: Option<i64>,
    pub summary_files: Option<i64>,
    pub summary_diffs: Option<serde_json::Value>,
    pub revert: Option<serde_json::Value>,
    pub permission: Option<serde_json::Value>,
    pub tags: Option<Vec<String>>,
    pub time_compacting: Option<i64>,
    pub time_archived: Option<i64>,
    /// Provider Connections Milestone 3: optional connection selection
    /// update. When set, `provider_connection_revision` and
    /// `model_catalog_revision` must accompany this field so a stale write
    /// can be rejected without mutating another session's selection.
    pub provider_connection_id: Option<Option<String>>,
    pub provider_connection_revision: Option<Option<u64>>,
    pub model_catalog_revision: Option<Option<String>>,
    pub selected_model_id: Option<Option<String>>,
}

/// Provider Connections Milestone 3: explicit outcome of resolving a legacy
/// `provider/model` session string against the current durable connection
/// catalog. The legacy string is preserved verbatim on the session row and
/// never silently chooses another credentialed endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LegacyResolution {
    /// Legacy string was empty / unset; no resolution was needed.
    Unset,
    /// A single durable connection matches the legacy provider kind. The
    /// resolved identity is returned for caller-side migration diagnostics.
    Resolved {
        connection_id: String,
        revision: u64,
        model_id: Option<String>,
    },
    /// No active connection matches the legacy provider kind. The legacy
    /// string is preserved verbatim; the caller should request an explicit
    /// selection or open the `/connect` workflow.
    UnresolvedLegacyProvider { provider_kind: String },
    /// Multiple active connections could match the legacy provider kind.
    /// No selection is performed; the caller must choose explicitly.
    AmbiguousLegacyProvider {
        provider_kind: String,
        candidates: Vec<String>,
    },
    /// Exactly one connection matches but is disabled; selection is
    /// refused.
    DisabledLegacyConnection {
        provider_kind: String,
        connection_id: String,
    },
    /// Exactly one connection matches but has no usable credential.
    MissingCredentialLegacyConnection {
        provider_kind: String,
        connection_id: String,
    },
}

impl LegacyResolution {
    /// Returns the resolved connection ID if the outcome is `Resolved`.
    pub fn resolved_connection_id(&self) -> Option<&str> {
        match self {
            LegacyResolution::Resolved { connection_id, .. } => Some(connection_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAnalytics {
    pub total_sessions: u64,
    pub total_messages: u64,
    pub total_tool_calls: u64,
    pub avg_session_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub session_id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
    pub position: i64,
    pub time_created: i64,
    pub time_updated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItemInput {
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionEntry {
    pub project_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: String,
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cost_usd: f64,
    pub timestamp: i64,
}

#[async_trait::async_trait]
pub trait SessionSummaryProvider: Send + Sync {
    async fn generate_summary(&self, conversation: &str) -> Result<String, crate::error::AppError>;
    async fn generate_title(&self, conversation: &str) -> Result<String, crate::error::AppError>;
}
