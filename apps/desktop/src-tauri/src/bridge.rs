use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSnapshot {
    pub state: String,
    pub daemon_id: Option<String>,
    pub protocol_version: Option<u32>,
    pub uptime_seconds: Option<u64>,
    pub active_sessions: Option<usize>,
    pub error: Option<String>,
    pub connection_generation: u64,
}

impl ConnectionSnapshot {
    pub fn disconnected(error: Option<String>, connection_generation: u64) -> Self {
        Self {
            state: "disconnected".into(),
            daemon_id: None,
            protocol_version: None,
            uptime_seconds: None,
            active_sessions: None,
            error,
            connection_generation,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub project_id: String,
    pub display_name: String,
    pub lifecycle: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopEvent {
    pub version: u8,
    pub event_seq: u64,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInfo {
    pub subscription_id: String,
    pub connection_generation: u64,
}

/// Explicit desktop route identity (M004 WP B). Renderers echo
/// `route_generation` back on mutating calls; the host drops any
/// completion for a superseded generation. Canonical workspace roots
/// never appear in this surface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTokenView {
    pub connection_generation: u64,
    pub route_generation: u64,
    pub project_id: String,
    pub workspace_id: String,
    pub session_id: Option<String>,
}

/// Workspace display entry. The canonical root backing this workspace
/// stays Rust-host data and is not part of this DTO.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    pub workspace_id: String,
    pub display_name: String,
}

/// Authorized project detail: workspaces from `ProjectGet`, never from
/// path enumeration.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetailView {
    pub project_id: String,
    pub display_name: String,
    pub workspaces: Vec<WorkspaceView>,
    pub session_count: usize,
    pub route_token: RouteTokenView,
}

/// Bounded session summary. Identity only; no message bodies.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryView {
    pub session_id: String,
    pub title: String,
    pub project_id: String,
    pub workspace_id: Option<String>,
}

/// Bounded session list for the current route.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListView {
    pub sessions: Vec<SessionSummaryView>,
    pub route_token: RouteTokenView,
}

/// Opened/created session bound to the captured route.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session: SessionSummaryView,
    pub route_token: RouteTokenView,
}

/// Accepted prompt intent (M004 WP D). The command resolves `Ok` only
/// once the daemon accepts the turn; failures are `Err` strings and
/// leave the renderer draft intact.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptSubmitView {
    pub intent_id: String,
    pub route_token: RouteTokenView,
}

/// One visible projection message. Text is the canonical (already
/// bounded) text; only public `user`/`assistant`/`tool` roles cross.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub message_id: String,
    pub role: String,
    pub text: String,
    pub truncated: bool,
}

/// Coarse turn state for the current/most-recent turn and recent turns.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSummaryView {
    pub turn_id: String,
    pub status: String,
    pub updated_at: i64,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub message_count: usize,
    pub tool_count: usize,
    pub pending_permissions: usize,
    pub pending_questions: usize,
    pub input_tokens: Option<usize>,
    pub output_tokens: Option<usize>,
}

/// Tool execution summary. Raw arguments/output never cross: `summary`
/// carries the daemon's summary line (or truncated preview marker) and
/// `has_artifact` marks output available behind an opaque handle.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSummaryView {
    pub tool_id: String,
    pub tool_name: String,
    pub status: String,
    pub summary: String,
    pub has_artifact: bool,
}

/// Run summary. `log_dir` is deliberately absent: filesystem paths never
/// cross the bridge.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryView {
    pub run_id: String,
    pub kind: String,
    pub status: String,
    pub summary: String,
}

/// Durable job summary (opaque ids only).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSummaryView {
    pub job_id: String,
    pub kind: String,
    pub state: String,
    pub summary: String,
}

/// Delegated subagent activity summary.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSummaryView {
    pub task_id: u64,
    pub agent: String,
    pub description: String,
    pub status: String,
    pub result_summary: Option<String>,
}

/// Pending (or recently resolved) permission. `scope_summary` is a
/// display-only string derived from the daemon path; it is never sent
/// back as authority (WP E responds carry the opaque id + choice only).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermissionView {
    pub permission_id: String,
    pub tool: String,
    pub scope_summary: Option<String>,
    pub status: String,
}

/// Pending (or recently resolved) question.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingQuestionView {
    pub question_id: String,
    pub header: Option<String>,
    pub prompt: String,
    pub status: String,
}

/// Opaque artifact handle backing truncated tool output. The renderer
/// may request a bounded excerpt; it may never fabricate a handle.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactHandleView {
    pub handle: String,
    pub byte_length: u64,
}

/// Controller lease summary. Populated by WP E from `SessionControlGet`;
/// `None` until then. The principal is display-only: responses
/// revalidate daemon-side and never accept renderer-supplied identity.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerSummaryView {
    pub turn_id: String,
    pub controller_principal: String,
    pub revision: u64,
}

/// Diagnostic cursor metadata. Non-authority: the renderer must never
/// use these fields as projection cursor state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorDiagnosticView {
    pub event_seq: u64,
    pub driver_cursor_seq: Option<u64>,
    pub subscription_known: bool,
}

/// Bounded presentation of one session projection for the renderer.
/// Atomic-replace semantics: each view is an independent owned value;
/// a resync publishes a wholly new view rather than patching the old.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPresentationView {
    pub session_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub state: String,
    pub turn: Option<TurnSummaryView>,
    pub messages: Vec<MessageView>,
    pub truncated_messages: usize,
    pub tools: Vec<ToolSummaryView>,
    pub runs: Vec<RunSummaryView>,
    pub jobs: Vec<JobSummaryView>,
    pub subagents: Vec<SubagentSummaryView>,
    pub recent_turns: Vec<TurnSummaryView>,
    pub pending_permissions: Vec<PendingPermissionView>,
    pub pending_questions: Vec<PendingQuestionView>,
    pub controller: Option<ControllerSummaryView>,
    pub artifact_handles: Vec<ArtifactHandleView>,
    pub cursor: CursorDiagnosticView,
    pub resync_reason: Option<String>,
}
