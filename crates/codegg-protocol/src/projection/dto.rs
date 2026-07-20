//! Bounded projection DTOs.
//!
//! Every type in this module is a *frontend-neutral* summary or
//! reference. Large bodies, raw logs, render frames, and provider
//! private reasoning are deliberately absent.
//!
//! All string fields honour [`crate::projection::limits::MAX_PROJECTION_STRING_BYTES`].
//! Collections honour the per-collection caps declared in
//! [`crate::projection::limits`]. When the reducer or an adapter
//! receives a payload that exceeds the cap, it MUST truncate or
//! replace the field with a handle variant instead of panicking.

use serde::{Deserialize, Serialize};

use crate::projection::limits::{
    truncate_str, MAX_PROJECTION_RUN_SUMMARY_BYTES, MAX_PROJECTION_STRING_BYTES,
};

/// Visibility classification carried on every projection DTO.
///
/// The redactor uses this classification to decide whether the field
/// is safe to share. The full policy lands in a later milestone; this
/// milestone only exposes the typed seam.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VisibilityClass {
    /// Visible to any frontend client that subscribes to the
    /// projection stream. This is the default for assistant text,
    /// user text, tool names, and tool status.
    #[default]
    Public,
    /// Visible to the active client only. Used for subagent task ids
    /// and diagnostics that may reveal internal sequencing.
    ClientLocal,
    /// Internal: never serialised into a projection event. Reserved
    /// for fields the reducer drops before publishing.
    Internal,
    /// Sensitive: must be redacted before leaving the daemon. The
    /// reducer replaces such fields with `[REDACTED:<rule>]` markers
    /// or handle placeholders.
    Sensitive,
}

/// Bounded summary of a single project inside a projection snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectSummaryProjection {
    pub project_id: String,
    pub display_name: String,
    pub lifecycle: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub time_last_opened_at: Option<i64>,
    pub registration_source: String,
    pub archived_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl ProjectSummaryProjection {
    /// Apply projection bounds to a freshly constructed summary.
    pub fn normalise(&mut self) {
        self.display_name =
            truncate_str(&self.display_name, MAX_PROJECTION_STRING_BYTES).into_owned();
        if let Some(desc) = self.description.as_deref() {
            self.description = Some(truncate_str(desc, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
        for tag in &mut self.tags {
            *tag = truncate_str(tag, MAX_PROJECTION_STRING_BYTES).into_owned();
        }
        self.lifecycle = truncate_str(&self.lifecycle, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.registration_source =
            truncate_str(&self.registration_source, MAX_PROJECTION_STRING_BYTES).into_owned();
    }
}

/// Bounded summary of a workspace inside a projection snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSummaryProjection {
    pub workspace_id: String,
    pub canonical_root: String,
    pub display_name: String,
    pub created_at: i64,
    pub last_opened_at: i64,
    pub archived_at: Option<i64>,
    pub active_sessions: usize,
    pub services_active: bool,
    pub active_leases: usize,
    pub config_revision: u64,
    pub health: WorkspaceHealthProjection,
}

impl WorkspaceSummaryProjection {
    pub fn normalise(&mut self) {
        self.canonical_root =
            truncate_str(&self.canonical_root, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.display_name =
            truncate_str(&self.display_name, MAX_PROJECTION_STRING_BYTES).into_owned();
    }
}

/// Bounded workspace health summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorkspaceHealthProjection {
    pub overall: String,
    pub catalog_state: String,
    pub workspace_state: String,
    pub assets_state: String,
    pub services_state: String,
    pub diagnostics: Vec<String>,
}

impl WorkspaceHealthProjection {
    pub fn normalise(&mut self) {
        self.overall = truncate_str(&self.overall, MAX_PROJECTION_STRING_BYTES).into_owned();
        for diag in &mut self.diagnostics {
            *diag = truncate_str(diag, MAX_PROJECTION_STRING_BYTES).into_owned();
        }
    }
}

/// Bounded summary of a session inside a projection snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummaryProjection {
    pub session_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub title: String,
    pub status: String,
    pub selected_model: Option<String>,
    pub selected_agent: Option<String>,
    pub has_active_turn: bool,
    pub pending_permission_count: usize,
    pub pending_question_count: usize,
    pub input_tokens: Option<usize>,
    pub output_tokens: Option<usize>,
    pub active_subagents: usize,
    pub time_created_at: Option<i64>,
    pub time_updated_at: Option<i64>,
    pub recent_summary: Option<String>,
}

impl SessionSummaryProjection {
    pub fn normalise(&mut self) {
        self.title = truncate_str(&self.title, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.status = truncate_str(&self.status, MAX_PROJECTION_STRING_BYTES).into_owned();
        if let Some(model) = self.selected_model.as_deref() {
            self.selected_model =
                Some(truncate_str(model, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
        if let Some(agent) = self.selected_agent.as_deref() {
            self.selected_agent =
                Some(truncate_str(agent, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
        if let Some(summary) = self.recent_summary.as_deref() {
            self.recent_summary =
                Some(truncate_str(summary, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned());
        }
    }
}

/// Bounded projection of a single turn inside a session.
///
/// The reducer preserves one active turn per session and up to
/// `MAX_PROJECTION_RECENT_TOOLS` tools inside that turn. Older
/// completed turns collapse into the [`SessionSummaryProjection::recent_summary`]
/// field of the session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnProjection {
    pub turn_id: String,
    pub status: TurnStatus,
    pub started_at: i64,
    pub updated_at: i64,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub messages: Vec<MessageProjection>,
    pub tools: Vec<ToolProjection>,
    pub pending_permissions: Vec<PermissionProjection>,
    pub pending_questions: Vec<QuestionProjection>,
    pub agent_tree: Vec<AgentTreeNodeProjection>,
    pub subagent_count: usize,
    pub input_tokens: Option<usize>,
    pub output_tokens: Option<usize>,
}

impl TurnProjection {
    pub fn normalise(&mut self) {
        self.stop_reason = self
            .stop_reason
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.error = self
            .error
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        for message in &mut self.messages {
            message.normalise();
        }
        for tool in &mut self.tools {
            tool.normalise();
        }
        for perm in &mut self.pending_permissions {
            perm.normalise();
        }
        for question in &mut self.pending_questions {
            question.normalise();
        }
    }
}

/// Lifecycle status for a [`TurnProjection`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    #[default]
    Starting,
    Active,
    AwaitingPermission,
    AwaitingQuestion,
    Completing,
    Completed,
    Failed,
    Cancelled,
}

/// Bounded message projection. Tool messages carry their
/// `tool_call_id` so the reducer can pair them with the originating
/// [`ToolProjection`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageProjection {
    pub message_id: String,
    pub parent_turn_id: String,
    pub role: MessageRole,
    pub text: String,
    pub tool_call_id: Option<String>,
    pub visibility: VisibilityClass,
    pub created_at: i64,
    pub truncated: bool,
}

impl MessageProjection {
    pub fn normalise(&mut self) {
        let bounded = truncate_str(&self.text, MAX_PROJECTION_STRING_BYTES);
        if bounded.len() < self.text.len() {
            self.text = bounded.into_owned();
            self.truncated = true;
        }
    }
}

/// Logical role for a [`MessageProjection`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    #[default]
    Assistant,
    Tool,
    System,
    Reasoning,
}

/// Bounded projection of a tool invocation and (eventually) result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolProjection {
    pub tool_id: String,
    pub tool_name: String,
    pub status: ToolStatus,
    pub arguments: ToolArgumentProjection,
    pub output: ToolOutputProjection,
    pub visibility: VisibilityClass,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub duration_ms: Option<u64>,
}

impl ToolProjection {
    pub fn normalise(&mut self) {
        self.tool_name = truncate_str(&self.tool_name, MAX_PROJECTION_STRING_BYTES).into_owned();
        match &mut self.arguments {
            ToolArgumentProjection::Inline { arguments } => {
                let bounded = truncate_str(arguments, MAX_PROJECTION_STRING_BYTES);
                if bounded.len() < arguments.len() {
                    *arguments = bounded.into_owned();
                }
            }
            ToolArgumentProjection::Summary { summary } => {
                *summary = truncate_str(summary, MAX_PROJECTION_STRING_BYTES).into_owned();
            }
            ToolArgumentProjection::TruncatedArguments { .. }
            | ToolArgumentProjection::Handle { .. } => {}
        }
        match &mut self.output {
            ToolOutputProjection::Pending => {}
            ToolOutputProjection::Inline { output } => {
                let bounded = truncate_str(output, MAX_PROJECTION_STRING_BYTES);
                if bounded.len() < output.len() {
                    *output = bounded.into_owned();
                }
            }
            ToolOutputProjection::Summary { summary } => {
                *summary = truncate_str(summary, MAX_PROJECTION_STRING_BYTES).into_owned();
            }
            ToolOutputProjection::TruncatedOutput { .. } | ToolOutputProjection::Handle { .. } => {}
        }
    }
}

/// Lifecycle status for a [`ToolProjection`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    #[default]
    Started,
    Completed,
    Failed,
    Cancelled,
}

/// How a tool's raw arguments are represented in the projection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolArgumentProjection {
    /// Raw arguments inline, bounded by
    /// [`crate::projection::limits::MAX_PROJECTION_TOOL_ARGS_BYTES`].
    Inline { arguments: String },
    /// A bounded summary line describing the arguments (e.g.
    /// `path=src/main.rs mode=rw`).
    Summary { summary: String },
    /// The arguments exceeded the bound; only the original byte
    /// count and a truncated preview remain.
    TruncatedArguments {
        original_bytes: usize,
        preview: String,
    },
    /// The arguments live behind a handle (e.g. a RunStore artifact).
    Handle { handle: String, byte_length: u64 },
}

impl Default for ToolArgumentProjection {
    fn default() -> Self {
        Self::Summary {
            summary: String::new(),
        }
    }
}

/// How a tool's output is represented in the projection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOutputProjection {
    /// The tool has not yet produced output.
    Pending,
    /// Raw output inline, bounded by
    /// [`crate::projection::limits::MAX_PROJECTION_TOOL_OUTPUT_BYTES`].
    Inline { output: String },
    /// A bounded summary line (e.g. `ok 4 line(s)`).
    Summary { summary: String },
    /// The output exceeded the bound; only the original byte count
    /// and a truncated preview remain.
    TruncatedOutput {
        original_bytes: usize,
        preview: String,
    },
    /// The output lives behind a handle (e.g. a RunStore artifact).
    Handle { handle: String, byte_length: u64 },
}

#[allow(clippy::derivable_impls)]
impl Default for ToolOutputProjection {
    fn default() -> Self {
        Self::Pending
    }
}

/// Bounded projection of a pending or recently resolved permission
/// request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionProjection {
    pub permission_id: String,
    pub tool: String,
    pub path: Option<String>,
    pub status: PermissionStatus,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

impl PermissionProjection {
    pub fn normalise(&mut self) {
        self.tool = truncate_str(&self.tool, MAX_PROJECTION_STRING_BYTES).into_owned();
        if let Some(path) = self.path.as_deref() {
            self.path = Some(truncate_str(path, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
    }
}

/// Lifecycle status for a [`PermissionProjection`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    #[default]
    Pending,
    Allowed,
    Denied,
}

/// Bounded projection of a pending or recently resolved interactive
/// question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestionProjection {
    pub question_id: String,
    pub header: Option<String>,
    pub prompt: String,
    pub status: PermissionStatus,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

impl QuestionProjection {
    pub fn normalise(&mut self) {
        self.prompt = truncate_str(&self.prompt, MAX_PROJECTION_STRING_BYTES).into_owned();
        if let Some(header) = self.header.as_deref() {
            self.header = Some(truncate_str(header, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
    }
}

/// Placeholder node in the agent tree. The agent hierarchy is owned by
/// a later subsystem; until then the projection tracks the parent /
/// child relationships and stable task ids produced by the daemon
/// without claiming durable agent-run semantics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTreeNodeProjection {
    /// Stable task id assigned by the daemon. Frontend-local; never
    /// reused after the projection snapshot evicts the node.
    pub task_id: u64,
    pub agent: String,
    pub description: String,
    pub status: AgentTreeStatus,
    pub parent_task_id: Option<u64>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub result_summary: Option<String>,
}

impl AgentTreeNodeProjection {
    pub fn normalise(&mut self) {
        self.agent = truncate_str(&self.agent, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.description =
            truncate_str(&self.description, MAX_PROJECTION_STRING_BYTES).into_owned();
        if let Some(summary) = self.result_summary.as_deref() {
            self.result_summary =
                Some(truncate_str(summary, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned());
        }
    }
}

/// Lifecycle status for an [`AgentTreeNodeProjection`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentTreeStatus {
    #[default]
    Running,
    Completed,
    Failed,
}

/// Bounded projection of a run (test, command, script).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunProjection {
    pub run_id: String,
    pub kind: String,
    pub command: String,
    pub status: String,
    pub summary: String,
    pub job_id: Option<String>,
    pub log_dir: Option<String>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub artifact_count: usize,
    pub pinned: bool,
}

impl RunProjection {
    pub fn normalise(&mut self) {
        self.kind = truncate_str(&self.kind, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.command = truncate_str(&self.command, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.status = truncate_str(&self.status, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.summary = truncate_str(&self.summary, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned();
        if let Some(dir) = self.log_dir.as_deref() {
            self.log_dir = Some(truncate_str(dir, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
    }
}

/// Bounded projection of a durable job (Phase 4 contract).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobProjection {
    pub job_id: String,
    pub workspace_id: String,
    pub kind: String,
    pub state: String,
    pub summary: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub active_attempt_id: Option<String>,
    pub error_class: Option<String>,
    pub updated_at: i64,
}

impl JobProjection {
    pub fn normalise(&mut self) {
        self.kind = truncate_str(&self.kind, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.state = truncate_str(&self.state, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.summary = truncate_str(&self.summary, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned();
        self.error_class = self
            .error_class
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
    }
}

/// Bounded reference to a runtime artifact (output, log, projection).
///
/// Carries only an opaque handle and the bounded size / kind so
/// consumers can request the body through an authorised API without
/// embedding it inline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactHandleProjection {
    pub artifact_id: String,
    pub kind: String,
    pub byte_length: u64,
    pub run_id: Option<String>,
    pub created_at: i64,
    pub preview: Option<String>,
}

impl ArtifactHandleProjection {
    pub fn normalise(&mut self) {
        self.kind = truncate_str(&self.kind, MAX_PROJECTION_STRING_BYTES).into_owned();
        if let Some(preview) = self.preview.as_deref() {
            self.preview = Some(truncate_str(preview, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
    }
}

/// File-change summary inside a turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileChangeProjection {
    Created { path: String },
    Modified { path: String },
    Deleted { path: String },
    Renamed { from: String, to: String },
}

impl FileChangeProjection {
    pub fn path(&self) -> &str {
        match self {
            FileChangeProjection::Created { path }
            | FileChangeProjection::Modified { path }
            | FileChangeProjection::Deleted { path } => path,
            FileChangeProjection::Renamed { to, .. } => to,
        }
    }
    pub fn normalise(&mut self) {
        match self {
            FileChangeProjection::Created { path }
            | FileChangeProjection::Modified { path }
            | FileChangeProjection::Deleted { path } => {
                *path = truncate_str(path, MAX_PROJECTION_STRING_BYTES).into_owned();
            }
            FileChangeProjection::Renamed { from, to } => {
                *from = truncate_str(from, MAX_PROJECTION_STRING_BYTES).into_owned();
                *to = truncate_str(to, MAX_PROJECTION_STRING_BYTES).into_owned();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_summary_truncates_long_strings() {
        let mut s = SessionSummaryProjection {
            session_id: "s".into(),
            project_id: "p".into(),
            workspace_id: "w".into(),
            title: "a".repeat(MAX_PROJECTION_STRING_BYTES + 32),
            status: "active".into(),
            selected_model: Some("m".into()),
            selected_agent: Some("a".into()),
            has_active_turn: false,
            pending_permission_count: 0,
            pending_question_count: 0,
            input_tokens: None,
            output_tokens: None,
            active_subagents: 0,
            time_created_at: None,
            time_updated_at: None,
            recent_summary: None,
        };
        s.normalise();
        assert!(s
            .title
            .ends_with(crate::projection::limits::TRUNCATION_MARKER));
        assert!(s.title.len() <= MAX_PROJECTION_STRING_BYTES);
    }

    #[test]
    fn tool_arguments_and_output_truncate() {
        let long = "x".repeat(MAX_PROJECTION_STRING_BYTES + 16);
        let mut t = ToolProjection {
            tool_id: "t".into(),
            tool_name: "n".into(),
            status: ToolStatus::Started,
            arguments: ToolArgumentProjection::Inline {
                arguments: long.clone(),
            },
            output: ToolOutputProjection::Inline { output: long },
            visibility: VisibilityClass::Public,
            started_at: None,
            completed_at: None,
            duration_ms: None,
        };
        t.normalise();
        match t.arguments {
            ToolArgumentProjection::Inline { arguments } => {
                assert!(arguments.len() <= MAX_PROJECTION_STRING_BYTES);
            }
            other => panic!("unexpected arguments variant: {:?}", other),
        }
        match t.output {
            ToolOutputProjection::Inline { output } => {
                assert!(output.len() <= MAX_PROJECTION_STRING_BYTES);
            }
            other => panic!("unexpected output variant: {:?}", other),
        }
    }
}
