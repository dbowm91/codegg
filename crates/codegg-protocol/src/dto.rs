use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Simplified session DTO for protocol messages.
/// Matches the wire format of `session::models::Session`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub slug: String,
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
    pub time_created: i64,
    pub time_updated: i64,
    #[serde(default)]
    pub time_compacting: Option<i64>,
    #[serde(default)]
    pub time_archived: Option<i64>,
    #[serde(default)]
    pub time_deleted: Option<i64>,
}

/// Simplified message DTO for protocol messages.
/// Matches the wire format of `session::message::Message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: MessageData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
/// `SessionSnapshot`. Clients decide whether to surface workspace metadata
/// in their UI by inspecting `ServerCapabilities.workspace_registration`
/// during the `ClientHello`/`ServerHello` handshake.
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
}
