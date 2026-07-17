use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Simplified session DTO for protocol messages.
/// Matches the wire format of `session::models::Session`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// Legacy string projection of the internal `ProjectId` relation.
    pub project_id: String,
    #[serde(default)]
    /// Legacy string projection of the internal `WorkspaceId` relation.
    pub workspace_id: Option<String>,
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
