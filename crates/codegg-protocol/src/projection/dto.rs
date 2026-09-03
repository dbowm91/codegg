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
use std::collections::VecDeque;

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
    pub messages: VecDeque<MessageProjection>,
    pub tools: VecDeque<ToolProjection>,
    pub pending_permissions: VecDeque<PermissionProjection>,
    pub pending_questions: VecDeque<QuestionProjection>,
    pub agent_tree: VecDeque<AgentTreeNodeProjection>,
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

/// Bounded, frontend-neutral summary of a durable delegated agent run.
///
/// This is derived from the authoritative run, worktree, result, and group
/// stores. It contains no prompt, transcript, mailbox body, hidden
/// reasoning, credentials, or raw logs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRunSummary {
    pub run_id: String,
    pub task_id: String,
    pub parent_run_id: Option<String>,
    pub agent: String,
    pub status: String,
    pub depth: u32,
    pub worktree_id: Option<String>,
    pub branch: Option<String>,
    pub base_commit: Option<String>,
    pub result_commit: Option<String>,
    pub validation_summary: Option<String>,
    pub group_id: Option<String>,
    pub attention_required: bool,
    pub terminal_summary: Option<String>,
    pub control_status: String,
    pub progress: Option<String>,
    pub failure_class: Option<String>,
    pub updated_at: i64,
}

pub type AgentRunSummaryProjection = AgentRunSummary;

impl AgentRunSummary {
    pub fn normalise(&mut self) {
        self.run_id = truncate_str(&self.run_id, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.task_id = truncate_str(&self.task_id, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.parent_run_id = self
            .parent_run_id
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.agent = truncate_str(&self.agent, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.status = truncate_str(&self.status, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.branch = self
            .branch
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.base_commit = self
            .base_commit
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.result_commit = self
            .result_commit
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.validation_summary = self
            .validation_summary
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned());
        self.group_id = self
            .group_id
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.terminal_summary = self
            .terminal_summary
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned());
        self.control_status =
            truncate_str(&self.control_status, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.progress = self
            .progress
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned());
        self.failure_class = self
            .failure_class
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
    }
}

/// Bounded summary of a daemon-owned managed worktree associated with a run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeSummaryProjection {
    pub worktree_id: String,
    pub owner_run_id: Option<String>,
    pub branch: Option<String>,
    pub base_commit: Option<String>,
    pub state: String,
    pub health: String,
    pub dirty: bool,
    pub conflicted: bool,
    pub retained_for_attention: bool,
    pub updated_at: i64,
}

impl WorktreeSummaryProjection {
    pub fn normalise(&mut self) {
        self.worktree_id =
            truncate_str(&self.worktree_id, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.owner_run_id = self
            .owner_run_id
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.branch = self
            .branch
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.base_commit = self
            .base_commit
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.state = truncate_str(&self.state, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.health = truncate_str(&self.health, MAX_PROJECTION_STRING_BYTES).into_owned();
    }
}

/// Bounded summary of a durable run group. Member IDs are opaque references;
/// consumers use the run summaries for member detail.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRunGroupSummaryProjection {
    pub group_id: String,
    pub owner_run_id: String,
    /// Additive owner discriminator. Empty means an older snapshot omitted it.
    #[serde(default)]
    pub owner_kind: String,
    #[serde(default)]
    pub owner_session_id: Option<String>,
    #[serde(default)]
    pub owner_turn_id: Option<String>,
    pub status: String,
    pub join_policy: String,
    pub member_run_ids: Vec<String>,
    pub successful: usize,
    pub failed: usize,
    pub active: usize,
    pub winner_run_id: Option<String>,
    pub cancel_remaining_on_satisfaction: bool,
    pub updated_at: i64,
}

pub type RunGroupSummaryProjection = AgentRunGroupSummaryProjection;

/// Bounded, frontend-neutral convergence summary. Detailed verdict findings
/// and run results remain behind their existing durable handles.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConvergenceSummaryProjection {
    pub convergence_id: String,
    pub owner_summary: String,
    pub status: String,
    pub cycle_ordinal: u8,
    pub max_cycles: u8,
    #[serde(default)]
    pub remaining_cycles: u8,
    pub producer_run_ids: Vec<String>,
    pub producer_completed: usize,
    pub producer_failed: usize,
    pub producer_cancelled: usize,
    pub producer_active: usize,
    pub verifier_run_id: Option<String>,
    pub verdict_kind: Option<String>,
    pub verdict_summary: Option<String>,
    pub awaiting_decision: bool,
    pub terminal_reason_class: Option<String>,
    #[serde(default)]
    pub selected_run_id: Option<String>,
    #[serde(default)]
    pub selected_result_commit: Option<String>,
    #[serde(default)]
    pub last_finding_count: usize,
}

impl ConvergenceSummaryProjection {
    pub fn normalise(&mut self) {
        self.convergence_id =
            truncate_str(&self.convergence_id, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.owner_summary =
            truncate_str(&self.owner_summary, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.status = truncate_str(&self.status, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.producer_run_ids = self
            .producer_run_ids
            .drain(..)
            .take(crate::projection::limits::MAX_PROJECTION_AGENT_RUNS)
            .map(|value| truncate_str(&value, MAX_PROJECTION_STRING_BYTES).into_owned())
            .collect();
        self.verifier_run_id = self
            .verifier_run_id
            .as_deref()
            .map(|value| truncate_str(value, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.verdict_kind = self
            .verdict_kind
            .as_deref()
            .map(|value| truncate_str(value, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.verdict_summary = self
            .verdict_summary
            .as_deref()
            .map(|value| truncate_str(value, MAX_PROJECTION_RUN_SUMMARY_BYTES).into_owned());
        self.terminal_reason_class = self
            .terminal_reason_class
            .as_deref()
            .map(|value| truncate_str(value, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.selected_run_id = self
            .selected_run_id
            .as_deref()
            .map(|value| truncate_str(value, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.selected_result_commit = self
            .selected_result_commit
            .as_deref()
            .map(|value| truncate_str(value, MAX_PROJECTION_STRING_BYTES).into_owned());
    }
}

impl AgentRunGroupSummaryProjection {
    pub fn normalise(&mut self) {
        self.group_id = truncate_str(&self.group_id, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.owner_run_id =
            truncate_str(&self.owner_run_id, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.owner_kind = truncate_str(&self.owner_kind, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.owner_session_id = self
            .owner_session_id
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.owner_turn_id = self
            .owner_turn_id
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
        self.status = truncate_str(&self.status, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.join_policy =
            truncate_str(&self.join_policy, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.member_run_ids = self
            .member_run_ids
            .drain(..)
            .take(crate::projection::limits::MAX_PROJECTION_AGENT_RUNS)
            .map(|s| truncate_str(&s, MAX_PROJECTION_STRING_BYTES).into_owned())
            .collect();
        self.winner_run_id = self
            .winner_run_id
            .as_deref()
            .map(|s| truncate_str(s, MAX_PROJECTION_STRING_BYTES).into_owned());
    }
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

/// Bounded summary of a background tool program inside a projection
/// snapshot. Tracks the full lifecycle from submission through
/// terminal state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolProgramSummary {
    /// Logical program identity.
    pub program_id: String,
    /// Scheduler job ID.
    pub job_id: String,
    /// Current lifecycle state (submitted, admitted, running, etc.).
    pub state: String,
    /// Execution phase within the current state.
    pub phase: Option<String>,
    /// Language (e.g. "restricted_python").
    pub language: String,
    /// Parent turn that submitted this program.
    pub parent_turn_id: Option<String>,
    /// Parent agent that submitted this program.
    pub parent_agent_id: Option<String>,
    /// Number of tool calls completed so far.
    pub calls_completed: u32,
    /// Number of child jobs still running.
    pub child_jobs_running: u32,
    /// Submission timestamp (millis since epoch).
    pub submitted_at: i64,
    /// When execution started, if started.
    pub started_at: Option<i64>,
    /// When the program reached a terminal state, if applicable.
    pub completed_at: Option<i64>,
    /// Failure class (timeout, compile_error, etc.) if failed.
    pub failure_class: Option<String>,
    /// Compact terminal handle for inspection/cancellation.
    pub terminal_handle: Option<String>,
    /// Bounded summary of the last progress update.
    pub last_progress: Option<String>,
}

impl ToolProgramSummary {
    pub fn normalise(&mut self) {
        self.state = truncate_str(&self.state, MAX_PROJECTION_STRING_BYTES).into_owned();
        if let Some(phase) = self.phase.as_deref() {
            self.phase = Some(truncate_str(phase, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
        self.language = truncate_str(&self.language, MAX_PROJECTION_STRING_BYTES).into_owned();
        if let Some(fc) = self.failure_class.as_deref() {
            self.failure_class = Some(truncate_str(fc, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
        if let Some(th) = self.terminal_handle.as_deref() {
            self.terminal_handle = Some(truncate_str(th, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
        if let Some(lp) = self.last_progress.as_deref() {
            self.last_progress = Some(truncate_str(lp, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
    }
}

/// Bounded representation of a single tool call made by a background
/// tool program. Redacted: raw arguments and output bodies are
/// replaced by bounded summaries or handles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolProgramCallSummary {
    /// Sequential call index within the program.
    pub call_index: u32,
    /// Tool that was invoked.
    pub tool_name: String,
    /// Bounded summary of the call arguments (no raw source).
    pub arguments_summary: String,
    /// Bounded summary of the call result (no raw output body).
    pub result_summary: String,
    /// Whether the call succeeded.
    pub success: bool,
    /// Duration in milliseconds, if known.
    pub duration_ms: Option<u64>,
    /// Timestamp when the call started.
    pub started_at: i64,
    /// Timestamp when the call completed, if completed.
    pub completed_at: Option<i64>,
}

impl ToolProgramCallSummary {
    pub fn normalise(&mut self) {
        self.tool_name = truncate_str(&self.tool_name, MAX_PROJECTION_STRING_BYTES).into_owned();
        self.arguments_summary = truncate_str(
            &self.arguments_summary,
            crate::projection::limits::MAX_PROJECTION_TOOL_ARGS_BYTES
                .min(MAX_PROJECTION_STRING_BYTES),
        )
        .into_owned();
        self.result_summary = truncate_str(
            &self.result_summary,
            crate::projection::limits::MAX_PROJECTION_TOOL_OUTPUT_BYTES
                .min(MAX_PROJECTION_STRING_BYTES),
        )
        .into_owned();
    }
}

/// A page of call history for a background tool program. Supports
/// paginated inspection of the call ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolProgramCallPage {
    /// Program this page belongs to.
    pub program_id: String,
    /// Zero-based offset of the first entry in this page.
    pub offset: u32,
    /// Total number of calls in the program.
    pub total_calls: u32,
    /// Whether more pages follow this one.
    pub has_more: bool,
    /// Bounded call entries in this page.
    pub calls: Vec<ToolProgramCallSummary>,
}

impl ToolProgramCallPage {
    pub fn normalise(&mut self) {
        for call in &mut self.calls {
            call.normalise();
        }
    }
}

/// Full detail view of a background tool program. Extends
/// [`ToolProgramSummary`] with manifest metadata, hashes,
/// checkpoint state, and paginated call history.
///
/// This is the response to a `ToolProgramInspect` request. It
/// deliberately does NOT include raw source code or unbounded
/// output bodies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolProgramDetail {
    /// Summary fields (same shape as the projection snapshot entry).
    pub summary: ToolProgramSummary,
    /// Source hash (SHA-256 of the restricted-Python source).
    pub source_hash: Option<String>,
    /// IR hash (SHA-256 of the compiled intermediate representation).
    pub ir_hash: Option<String>,
    /// Checkpoint version at last successful step, if any.
    pub checkpoint_version: Option<u32>,
    /// Manifest metadata: language version, allowed tools, budgets.
    pub manifest_summary: Option<String>,
    /// Artifact handles for program outputs.
    pub artifacts: Vec<ArtifactHandleProjection>,
    /// Total number of tool calls made by the program.
    pub total_calls: u32,
    /// Most recent call page. Use `offset` to request earlier pages.
    pub call_page: Option<ToolProgramCallPage>,
}

impl ToolProgramDetail {
    pub fn normalise(&mut self) {
        self.summary.normalise();
        if let Some(h) = self.source_hash.as_deref() {
            self.source_hash = Some(truncate_str(h, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
        if let Some(h) = self.ir_hash.as_deref() {
            self.ir_hash = Some(truncate_str(h, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
        if let Some(m) = self.manifest_summary.as_deref() {
            self.manifest_summary = Some(truncate_str(m, MAX_PROJECTION_STRING_BYTES).into_owned());
        }
        for artifact in &mut self.artifacts {
            artifact.normalise();
        }
        if let Some(page) = self.call_page.as_mut() {
            page.normalise();
        }
    }
}

/// The notification classification returned to the parent agent.
/// Distinguishes three terminal outcomes as the plan requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationClassification {
    /// The program completed successfully.
    Completed,
    /// The program was incomplete but is recoverable (e.g. timeout
    /// that can be retried, or stall that can be resumed).
    IncompleteRecoverable,
    /// The program reached a terminal failure that is not recoverable
    /// (e.g. compile error, permanent policy denial).
    FailedTerminal,
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

    #[test]
    fn call_summary_truncates_long_fields() {
        let long = "x".repeat(MAX_PROJECTION_STRING_BYTES + 16);
        let mut call = ToolProgramCallSummary {
            call_index: 0,
            tool_name: long.clone(),
            arguments_summary: long.clone(),
            result_summary: long.clone(),
            success: true,
            duration_ms: None,
            started_at: 0,
            completed_at: None,
        };
        call.normalise();
        assert!(call.tool_name.len() <= MAX_PROJECTION_STRING_BYTES);
        assert!(call.arguments_summary.len() <= MAX_PROJECTION_STRING_BYTES);
        assert!(call.result_summary.len() <= MAX_PROJECTION_STRING_BYTES);
    }

    #[test]
    fn call_page_normalises_all_entries() {
        let long = "y".repeat(MAX_PROJECTION_STRING_BYTES + 8);
        let mut page = ToolProgramCallPage {
            program_id: "tp-1".into(),
            offset: 0,
            total_calls: 2,
            has_more: false,
            calls: vec![
                ToolProgramCallSummary {
                    call_index: 0,
                    tool_name: long.clone(),
                    arguments_summary: "ok".into(),
                    result_summary: "ok".into(),
                    success: true,
                    duration_ms: Some(10),
                    started_at: 0,
                    completed_at: Some(10),
                },
                ToolProgramCallSummary {
                    call_index: 1,
                    tool_name: long,
                    arguments_summary: "ok".into(),
                    result_summary: "ok".into(),
                    success: false,
                    duration_ms: None,
                    started_at: 10,
                    completed_at: None,
                },
            ],
        };
        page.normalise();
        for call in &page.calls {
            assert!(call.tool_name.len() <= MAX_PROJECTION_STRING_BYTES);
        }
    }

    #[test]
    fn detail_normalises_summary_and_hashes() {
        let mut detail = ToolProgramDetail {
            summary: ToolProgramSummary {
                program_id: "tp-1".into(),
                job_id: "j-1".into(),
                state: "running".into(),
                phase: None,
                language: "restricted_python".into(),
                parent_turn_id: None,
                parent_agent_id: None,
                calls_completed: 0,
                child_jobs_running: 0,
                submitted_at: 0,
                started_at: None,
                completed_at: None,
                failure_class: None,
                terminal_handle: None,
                last_progress: None,
            },
            source_hash: Some("a".repeat(MAX_PROJECTION_STRING_BYTES + 32)),
            ir_hash: Some("b".repeat(MAX_PROJECTION_STRING_BYTES + 32)),
            checkpoint_version: Some(1),
            manifest_summary: None,
            artifacts: vec![],
            total_calls: 0,
            call_page: None,
        };
        detail.normalise();
        assert!(detail.source_hash.as_ref().unwrap().len() <= MAX_PROJECTION_STRING_BYTES);
        assert!(detail.ir_hash.as_ref().unwrap().len() <= MAX_PROJECTION_STRING_BYTES);
    }

    #[test]
    fn notification_classification_serde_roundtrip() {
        let cases = vec![
            NotificationClassification::Completed,
            NotificationClassification::IncompleteRecoverable,
            NotificationClassification::FailedTerminal,
        ];
        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let back: NotificationClassification = serde_json::from_str(&json).unwrap();
            assert_eq!(back, case);
        }
    }
}
