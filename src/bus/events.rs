use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
    /// Permission was requested for a tool/path combination.
    PermissionRequested { tool: String, path: Option<String> },
    /// Permission was granted, optionally persisting the decision.
    PermissionGranted { tool: String, persist: bool },
    /// Permission was denied.
    PermissionDenied { tool: String },
    /// An MCP server connected.
    McpServerConnected { name: String },
    /// An MCP server disconnected.
    McpServerDisconnected { name: String },
    /// MCP server's available tools changed.
    McpToolListChanged { name: String },
    /// Application configuration changed.
    ConfigChanged,
    /// Todo list was updated.
    TodoUpdated { session_id: String },
    /// The agent was changed.
    AgentChanged { name: String },
    /// The model was changed.
    ModelChanged { model: String },
    /// Session compaction was triggered.
    CompactionTriggered { session_id: String },
    /// An error occurred.
    Error { message: String },
    /// An informational message.
    Info { message: String },
    /// A question is pending user response.
    QuestionPending {
        session_id: String,
        questions: String,
    },
    /// A question was answered by the user.
    QuestionAnswered { session_id: String, answers: String },
    /// A permission is pending user decision.
    PermissionPending {
        session_id: String,
        perm_id: String,
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
            AppEvent::PermissionRequested { .. } => "permission:requested",
            AppEvent::PermissionGranted { .. } => "permission:granted",
            AppEvent::PermissionDenied { .. } => "permission:denied",
            AppEvent::McpServerConnected { .. } => "mcp:connected",
            AppEvent::McpServerDisconnected { .. } => "mcp:disconnected",
            AppEvent::McpToolListChanged { .. } => "mcp:tool_list_changed",
            AppEvent::ConfigChanged => "config:changed",
            AppEvent::TodoUpdated { .. } => "todo:updated",
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
        }
    }
}
