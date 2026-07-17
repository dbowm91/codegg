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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
