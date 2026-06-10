use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Snapshot of a single todo item suitable for TUI display.
///
/// Mirrors the fields that the TUI sidebar needs to render and check off
/// a todo entry. Kept deliberately small so the bus event stays cheap to
/// fan out to subscribers (TUI, remote clients, loggers).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItemSnapshot {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// Snapshot of a goal for TUI display and remote clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoalSnapshot {
    pub id: String,
    pub session_id: String,
    pub project_id: String,
    pub title: String,
    pub objective: String,
    pub status: String,
    pub current_phase: Option<String>,
    pub progress_summary: String,
    pub next_action: Option<String>,
    pub completion_criteria: Vec<String>,
    pub open_questions: Vec<String>,
    pub budget: GoalBudgetSnapshot,
    pub usage: GoalUsageSnapshot,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GoalBudgetSnapshot {
    pub max_turns: Option<i64>,
    pub max_model_tokens: Option<i64>,
    pub max_tool_calls: Option<i64>,
    /// Wall-clock budget in seconds. Mirrors `max_model_tokens` /
    /// `max_tool_calls` as a third enforcement axis.
    pub max_wallclock_secs: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GoalUsageSnapshot {
    pub turns_used: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tool_calls: i64,
    /// Wall-clock seconds spent on the active goal. Reset when the goal
    /// transitions to a non-active state.
    pub wallclock_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppEvent {
    /// A new session was created.
    SessionCreated { id: String, project_id: String },
    /// Session metadata was updated.
    SessionUpdated { id: String },
    /// Session was archived.
    SessionArchived { id: String },
    /// Session was forked into a new session.
    SessionForked { parent_id: String, child_id: String },
    /// Session was shared via URL.
    SessionShared { id: String, url: String },
    /// Session sharing was disabled.
    SessionUnshared { id: String },
    /// Session was reverted to a previous message.
    SessionReverted { id: String, to_message: String },
    /// A new message was added to the session.
    MessageAdded {
        session_id: String,
        message_id: String,
    },
    /// A message was deleted from the session.
    MessageDeleted {
        session_id: String,
        message_id: String,
    },
    /// A tool was invoked.
    ToolCalled { tool: String, session_id: String },
    /// A tool finished execution.
    ToolResult {
        tool_id: String,
        tool_name: String,
        session_id: String,
        output: String,
        success: bool,
    },
    /// An MCP server connected.
    McpServerConnected { name: String },
    /// An MCP server disconnected.
    McpServerDisconnected { name: String },
    /// MCP server's available tools changed.
    McpToolListChanged { name: String },
    /// Application configuration changed.
    ConfigChanged,
    /// Todo list was updated. Carries the full snapshot so the TUI can
    /// render without re-reading from the agent loop.
    TodoUpdated {
        session_id: String,
        revision: u64,
        items: Vec<TodoItemSnapshot>,
    },
    /// The active goal was created, modified, or transitioned.
    ///
    /// Published by the goal create/clear/done/pause/resume handlers. A
    /// `None` snapshot signals that there is no active goal for the
    /// session (e.g. after `GoalClear`).
    GoalUpdated {
        session_id: String,
        goal: Box<Option<GoalSnapshot>>,
    },
    /// A goal's usage counters were advanced (turn finished, tool calls
    /// executed, etc). The TUI uses this to keep the budget meter live
    /// without re-reading the database.
    GoalUsageUpdated {
        session_id: String,
        goal_id: String,
        usage: GoalUsageSnapshot,
        budget: GoalBudgetSnapshot,
    },
    /// A goal was transitioned to `BudgetLimited` because one of the
    /// budget axes was exceeded. The agent should wrap up immediately.
    GoalBudgetLimited {
        session_id: String,
        goal_id: String,
        reason: String,
    },
    /// A goal was marked `Complete` by the model.
    GoalCompleted {
        session_id: String,
        goal_id: String,
        evidence: String,
    },
    /// The agent was changed.
    AgentChanged { name: String },
    /// The model was changed (e.g. via auto-routing).
    ModelChanged { model: String, complexity: String },
    /// Session compaction was triggered.
    CompactionTriggered {
        session_id: String,
        tokens_before: usize,
        tokens_after: usize,
    },
    /// An error occurred.
    Error { message: String },
    /// An informational message.
    Info { message: String },
    /// A question is pending user response.
    QuestionPending {
        session_id: String,
        question_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        questions: String,
    },
    /// A question was answered by the user.
    QuestionAnswered { session_id: String, answers: String },
    /// A permission is pending user decision.
    PermissionPending {
        session_id: String,
        perm_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        tool: String,
        path: Option<String>,
        args: Option<serde_json::Value>,
    },
    /// A file diff is pending user review.
    DiffPending {
        session_id: String,
        path: String,
        old_content: String,
        new_content: String,
    },
    /// User accepted or rejected a diff.
    DiffResponded {
        session_id: String,
        path: String,
        accepted: bool,
    },
    /// User responded to a permission request.
    PermissionResponded {
        session_id: String,
        tool: String,
        allowed: bool,
    },
    /// Text delta received during streaming.
    TextDelta {
        session_id: Arc<str>,
        delta: Arc<str>,
    },
    /// Reasoning delta received during extended thinking.
    ReasoningDelta { session_id: Arc<str>, delta: String },
    /// A tool call started execution.
    ToolCallStarted {
        session_id: String,
        tool_name: String,
        tool_id: String,
        arguments: String,
    },
    /// The agent finished processing.
    AgentFinished {
        session_id: String,
        stop_reason: String,
        input_tokens: Option<usize>,
        output_tokens: Option<usize>,
        cached_tokens: Option<usize>,
        reasoning_tokens: Option<usize>,
    },
    /// A file was modified externally.
    FileChanged {
        path: String,
        action: String,
        old_content: Option<String>,
    },
    /// A subagent started.
    SubagentStarted {
        session_id: String,
        task_id: u64,
        agent: String,
        description: String,
    },
    /// A subagent sent progress.
    SubagentProgress {
        session_id: String,
        task_id: u64,
        agent: String,
        message: String,
    },
    /// A subagent completed.
    SubagentCompleted {
        session_id: String,
        task_id: u64,
        agent: String,
        result_summary: String,
    },
    /// A subagent failed.
    SubagentFailed {
        session_id: String,
        task_id: u64,
        agent: String,
        error: String,
    },
    /// Context window usage updated.
    ContextUpdated {
        session_id: String,
        context_tokens: usize,
        context_limit: usize,
    },
}

impl AppEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            AppEvent::SessionCreated { .. } => "session:created",
            AppEvent::SessionUpdated { .. } => "session:updated",
            AppEvent::SessionArchived { .. } => "session:archived",
            AppEvent::SessionForked { .. } => "session:forked",
            AppEvent::SessionShared { .. } => "session:shared",
            AppEvent::SessionUnshared { .. } => "session:unshared",
            AppEvent::SessionReverted { .. } => "session:reverted",
            AppEvent::MessageAdded { .. } => "message:added",
            AppEvent::MessageDeleted { .. } => "message:deleted",
            AppEvent::ToolCalled { .. } => "tool:called",
            AppEvent::ToolResult { .. } => "tool:result",
            AppEvent::McpServerConnected { .. } => "mcp:connected",
            AppEvent::McpServerDisconnected { .. } => "mcp:disconnected",
            AppEvent::McpToolListChanged { .. } => "mcp:tool_list_changed",
            AppEvent::ConfigChanged => "config:changed",
            AppEvent::TodoUpdated { .. } => "todo:updated",
            AppEvent::GoalUpdated { .. } => "goal:updated",
            AppEvent::GoalUsageUpdated { .. } => "goal:usage_updated",
            AppEvent::GoalBudgetLimited { .. } => "goal:budget_limited",
            AppEvent::GoalCompleted { .. } => "goal:completed",
            AppEvent::AgentChanged { .. } => "agent:changed",
            AppEvent::ModelChanged { .. } => "model:changed",
            AppEvent::CompactionTriggered { .. } => "compaction:triggered",
            AppEvent::Error { .. } => "error",
            AppEvent::Info { .. } => "info",
            AppEvent::QuestionPending { .. } => "question:pending",
            AppEvent::QuestionAnswered { .. } => "question:answered",
            AppEvent::PermissionPending { .. } => "permission:pending",
            AppEvent::PermissionResponded { .. } => "permission:responded",
            AppEvent::DiffPending { .. } => "diff:pending",
            AppEvent::DiffResponded { .. } => "diff:responded",
            AppEvent::TextDelta { .. } => "text:delta",
            AppEvent::ReasoningDelta { .. } => "reasoning:delta",
            AppEvent::ToolCallStarted { .. } => "tool_call:started",
            AppEvent::AgentFinished { .. } => "agent:finished",
            AppEvent::FileChanged { .. } => "file:changed",
            AppEvent::SubagentStarted { .. } => "subagent:started",
            AppEvent::SubagentProgress { .. } => "subagent:progress",
            AppEvent::SubagentCompleted { .. } => "subagent:completed",
            AppEvent::SubagentFailed { .. } => "subagent:failed",
            AppEvent::ContextUpdated { .. } => "context:updated",
        }
    }
}
