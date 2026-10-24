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

/// One message of the multi-turn transcript. `turn_id` tags the owning
/// turn so the renderer can group the transcript by turn; `messages`
/// stays the current turn's flat list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMessageView {
    pub message_id: String,
    pub turn_id: String,
    pub role: String,
    pub text: String,
    pub truncated: bool,
    pub created_at: i64,
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
    /// Provider-reported cached/reused input tokens for this turn.
    pub cached_tokens: Option<usize>,
    /// The context window the agent resolved for this turn. `None` means
    /// unresolved; the renderer must not substitute a default window.
    pub context_limit: Option<usize>,
    /// Reasoning token count the daemon reported for this turn. The
    /// reasoning *text* is `VisibilityClass::Internal` and never crosses
    /// the bridge, so this count is the whole of what the renderer may
    /// know about the model's reasoning.
    pub reasoning_tokens: Option<usize>,
}

/// Tool execution summary. Raw arguments/output never cross: `summary`
/// carries the daemon's summary line (or truncated preview marker) and
/// `has_artifact` marks output available behind an opaque handle.
///
/// `arguments` and `output` are the same withheld/summarised strings
/// `summary` is built from, split out so a renderer can render a tool
/// call's parts separately. Both come from the same sanitising helpers,
/// so neither can carry a raw inline value.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSummaryView {
    pub tool_id: String,
    pub tool_name: String,
    pub status: String,
    pub summary: String,
    pub has_artifact: bool,
    pub arguments: String,
    pub output: String,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub duration_ms: Option<u64>,
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
///
/// `child_instance_id` is the durable session-owned identity of the child
/// and `parent_task_id` its nesting edge; the per-turn agent tree keeps
/// legacy nodes that carry no durable child identity.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSummaryView {
    pub task_id: u64,
    pub agent: String,
    pub description: String,
    pub status: String,
    pub result_summary: Option<String>,
    pub child_instance_id: String,
    pub parent_task_id: Option<u64>,
    pub originating_turn_id: Option<String>,
    pub display_name: Option<String>,
    pub run_id: Option<String>,
}

/// Durable delegated agent run summary. No prompt, transcript, mailbox
/// body, hidden reasoning, credential, or raw log crosses; the canonical
/// DTO says so at `crates/codegg-protocol/src/projection/dto.rs:544-548`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunView {
    pub run_id: String,
    pub task_id: String,
    pub agent: String,
    pub status: String,
    pub depth: u32,
    pub parent_run_id: Option<String>,
    pub worktree_id: Option<String>,
    pub branch: Option<String>,
    pub result_commit: Option<String>,
    pub group_id: Option<String>,
    pub control_status: String,
    pub attention_required: bool,
    pub progress: Option<String>,
    pub terminal_summary: Option<String>,
    pub failure_class: Option<String>,
    pub updated_at: i64,
}

/// Background tool program summary (lifecycle from submission to
/// terminal state).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProgramView {
    pub program_id: String,
    pub job_id: String,
    pub state: String,
    pub phase: Option<String>,
    pub language: String,
    pub parent_turn_id: Option<String>,
    pub calls_completed: u32,
    pub child_jobs_running: u32,
    pub submitted_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub failure_class: Option<String>,
    pub terminal_handle: Option<String>,
    pub last_progress: Option<String>,
}

/// Managed worktree summary. The canonical root backing a worktree is
/// not part of this DTO.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeView {
    pub worktree_id: String,
    pub owner_run_id: Option<String>,
    pub branch: Option<String>,
    pub state: String,
    pub health: String,
    pub dirty: bool,
    pub conflicted: bool,
    pub retained_for_attention: bool,
    pub updated_at: i64,
}

/// Durable run-group summary. `member_count` replaces the canonical
/// `member_run_ids` list: the daemon states that member ids are opaque
/// references and that consumers use the run summaries for member detail
/// (`crates/codegg-protocol/src/projection/dto.rs:675-677`), so the
/// renderer joins on `AgentRunView::group_id` instead of carrying a
/// second copy of every member id.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunGroupView {
    pub group_id: String,
    pub owner_run_id: String,
    pub owner_kind: String,
    pub status: String,
    pub join_policy: String,
    pub member_count: usize,
    pub successful: usize,
    pub failed: usize,
    pub active: usize,
    pub winner_run_id: Option<String>,
    pub cancel_remaining_on_satisfaction: bool,
    pub updated_at: i64,
}

/// Bounded convergence summary. Detailed verdict findings stay behind
/// their durable handles, as the canonical DTO states at
/// `crates/codegg-protocol/src/projection/dto.rs:712-713`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvergenceView {
    pub convergence_id: String,
    pub owner_summary: String,
    pub status: String,
    pub cycle_ordinal: u8,
    pub max_cycles: u8,
    pub remaining_cycles: u8,
    pub producer_completed: usize,
    pub producer_failed: usize,
    pub producer_cancelled: usize,
    pub producer_active: usize,
    pub verifier_run_id: Option<String>,
    pub verdict_kind: Option<String>,
    pub verdict_summary: Option<String>,
    pub awaiting_decision: bool,
    pub terminal_reason_class: Option<String>,
    pub selected_run_id: Option<String>,
    pub last_finding_count: usize,
}

/// Pending (or recently resolved) permission. `scope_summary` is a
/// display-only string derived from the daemon path; it is never sent
/// back as authority (WP E responds carry the opaque id + choice only).
/// One todo entry.
///
/// Mirrors the daemon's `TodoItemSnapshot`
/// (`crates/codegg-core/src/bus/events.rs:10-15`): the content is the
/// entry, and the status/priority are the entry's state. The host
/// applies the TUI's own decode rule — a missing status falls back to
/// "pending" and a missing priority to "medium"
/// (`src/tui/app/mod.rs:2459-2480`) — so a partial entry renders with
/// meaningful state instead of an empty one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItemView {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// The route-selected session's todo list.
///
/// Session-scoped by construction: the host reads it for the
/// route-selected session only, and carries the session identity so a
/// renderer can prove the list it renders belongs to the session on
/// screen. This is the TUI's `Ctrl+T` task surface in DOM form.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoListView {
    pub session_id: String,
    pub items: Vec<TodoItemView>,
}

/// Pending (or recently resolved) permission. `scope_summary` is a
/// display-only string derived from the daemon path; it is never sent
/// back as authority (WP E responses carry the opaque id + choice only).
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

/// Bounded artifact excerpt (M004 WP C / §12). Content is daemon-cut
/// to the requested window; `truncated`/`redacted` describe daemon-side
/// shaping, never renderer inference.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactExcerptView {
    pub handle: String,
    pub start: u64,
    pub end: u64,
    pub content_type: String,
    pub content: String,
    pub truncated: bool,
    pub redacted: bool,
}

/// Controller lease summary. The controller lease is not part of the
/// canonical projection: the host overlays it onto the presented view
/// from `SessionControlGet`
/// (`apps/desktop/src-tauri/src/control.rs:44-52`), so the pure
/// presenter leaves it `None`. The principal is display-only: responses
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

/// The read-only git fact sets a desktop client may ask for.
///
/// A narrow capability class, exactly like the daemon's own closed set
/// (`crates/codegg-protocol/src/git.rs:28-38`): the renderer names
/// *which* facts to read and nothing else. It never names a project, a
/// repository, or a path — the project comes from the route and the
/// daemon resolves the repository root from its own durable binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitReadOperation {
    /// The current branch, whether the working tree is dirty, the `HEAD`
    /// commit, and the stash count.
    Status,
    /// A per-file change summary of the working tree against `HEAD`.
    Diff,
    /// The local branches with their upstream tracking information.
    Branches,
}

impl GitReadOperation {
    /// The wire name the renderer sends for this operation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Diff => "diff",
            Self::Branches => "branches",
        }
    }

    /// Parse the renderer's operation name.
    ///
    /// The set is closed: an unknown name is refused rather than
    /// defaulted, so a renderer can never widen the read.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "status" => Ok(Self::Status),
            "diff" => Ok(Self::Diff),
            "branches" => Ok(Self::Branches),
            _ => Err("unsupported git read operation".into()),
        }
    }
}

/// Which git fact set a read carried.
///
/// The daemon tags its answer by the operation that produced it
/// (`crates/codegg-protocol/src/git.rs:63-72`); the view carries the
/// same tag so a renderer switches on one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitReadKind {
    /// The repository's status facts.
    Status,
    /// The working tree's diff summary.
    Diff,
    /// The repository's local branch list.
    Branches,
}

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitChangeKind {
    /// The file was added.
    Added,
    /// The file's content changed.
    Modified,
    /// The file was removed.
    Deleted,
    /// The file was renamed.
    Renamed,
    /// The file was copied.
    Copied,
    /// The read-only fact layer could not classify the change.
    Other,
}

/// The object-generation consistency class of a git read.
///
/// A read is only as trustworthy as the generation it observed, so the class
/// travels with the facts instead of being left to the renderer's memory.
/// The daemon reports what the observation is worth; the view carries the
/// same answer so a panel can say it rather than imply a guarantee it was
/// never given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitConsistency {
    /// The read served objects from one materialized generation.
    ImmutableGeneration,
    /// The read served objects from a capture that was immutable when it was
    /// taken and may now be stale relative to the repository.
    CapturedNotCurrent,
    /// The read served objects directly from a concurrently mutable
    /// repository, so the observation may be torn.
    LiveMutable,
    /// The read consulted no object store, so it observed no object
    /// generation at all.
    NoObjectGeneration,
}

/// The status facts of one repository.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusView {
    pub branch: String,
    pub is_dirty: bool,
    pub commit_hash: Option<String>,
    pub stash_count: usize,
    /// The consistency class the read was obtained under.
    pub consistency: GitConsistency,
}

/// One changed file in a diff summary. The path is repository-relative;
/// no filesystem root ever crosses the bridge.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitChangedFileView {
    pub path: String,
    pub kind: GitChangeKind,
}

/// A bounded summary of the working tree's changes against `HEAD`.
///
/// Count-and-name only: the daemon never sends patch text, so the view
/// stays bounded however large the diff is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffView {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub files: Vec<GitChangedFileView>,
    pub total_files: usize,
    pub truncated: bool,
    /// The consistency class the read was obtained under.
    pub consistency: GitConsistency,
}

/// One local branch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchView {
    pub name: String,
    pub is_current: bool,
    pub is_detached: bool,
    pub upstream: Option<String>,
    pub ahead: Option<i32>,
    pub behind: Option<i32>,
    pub head: Option<String>,
}

/// A bounded list of the repository's local branches.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchListView {
    pub branches: Vec<GitBranchView>,
    pub total: usize,
    pub truncated: bool,
    /// The consistency class the read was obtained under.
    pub consistency: GitConsistency,
}

/// The facts of one read-only git read.
///
/// Exactly one payload is set and the others are absent. The daemon
/// tags its answer by the operation that produced it, so a payload that
/// does not match the requested operation is a failed read, never a
/// partial one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitReadView {
    pub kind: GitReadKind,
    pub status: Option<GitStatusView>,
    pub diff: Option<GitDiffView>,
    pub branches: Option<GitBranchListView>,
}

/// The namespace a skill was discovered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// A skill found under the project's own roots.
    Project,
    /// A skill found under a user-level (global) root.
    Global,
    /// A skill contributed by an installed plugin.
    Plugin,
    /// A skill found under a user-configured extra root.
    Configured,
    /// A compatibility skill shipped with CodeGG itself.
    Native,
}

/// One discovered skill, described without its body or its location.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummaryView {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub content_digest: String,
    pub resource_count: usize,
}

/// The skills the daemon discovered for one project.
///
/// Metadata only: no skill body and no filesystem path crosses the
/// bridge, and the entry list stays inside the daemon's own bound.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogueView {
    pub project_id: String,
    pub skills: Vec<SkillSummaryView>,
    pub total: usize,
    pub truncated: bool,
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
    /// The current (or most-recent) turn's messages, flat.
    pub messages: Vec<MessageView>,
    pub truncated_messages: usize,
    /// The multi-turn transcript: the recent turns' messages plus the
    /// active turn's, oldest first, each tagged with its `turn_id`.
    pub transcript: Vec<TranscriptMessageView>,
    pub truncated_transcript_messages: usize,
    pub tools: Vec<ToolSummaryView>,
    pub runs: Vec<RunSummaryView>,
    pub jobs: Vec<JobSummaryView>,
    /// Session-owned child instances — the projection's liveness
    /// authority for subagents.
    pub subagents: Vec<SubagentSummaryView>,
    /// The active turn's per-turn agent tree, kept alongside `subagents`
    /// because it still carries legacy nodes with no durable child
    /// identity.
    pub agent_tree: Vec<SubagentSummaryView>,
    pub agent_runs: Vec<AgentRunView>,
    pub tool_programs: Vec<ToolProgramView>,
    pub worktrees: Vec<WorktreeView>,
    pub run_groups: Vec<RunGroupView>,
    pub convergences: Vec<ConvergenceView>,
    pub recent_turns: Vec<TurnSummaryView>,
    /// The session's todo list.
    ///
    /// `None` means *this view carries no todo list*, not "the session
    /// has no todos". Todos are not part of the canonical projection:
    /// they live behind a separate daemon read (`CoreRequest::TodoList`),
    /// so only the host's todo refresh populates this field and a plain
    /// projection push leaves it unset. A renderer must therefore keep
    /// the last list it was given rather than reading an absent list as
    /// an empty one.
    pub todos: Option<TodoListView>,
    pub pending_permissions: Vec<PendingPermissionView>,
    pub pending_questions: Vec<PendingQuestionView>,
    pub controller: Option<ControllerSummaryView>,
    pub artifact_handles: Vec<ArtifactHandleView>,
    pub cursor: CursorDiagnosticView,
    pub resync_reason: Option<String>,
}
