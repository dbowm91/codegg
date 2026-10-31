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

/// One staged human-shell evidence, named by the run the daemon owns.
///
/// M007-I (plan 022, work package F): the frontend names the **run** and the
/// question. It never carries the bytes: the daemon reads its own run bytes,
/// assembles the evidence and redacts it, so the evidence is the daemon's.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanShellPromotionStageView {
    /// The durable identity the staged promotion will carry.
    pub promotion_id: String,
    /// The daemon's run id for the shell command whose output is the evidence.
    pub source_id: String,
    /// The question the human asked about the evidence.
    pub question: String,
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

/// One human-shell command run the daemon published for the session.
///
/// The run's **output** is deliberately absent: the daemon serves the
/// bytes on demand through its own read, and the projection publishes
/// the listing. The `cwd` is the run's own working directory as the
/// daemon recorded it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellRunView {
    pub run_id: String,
    pub command: String,
    pub cwd: String,
    /// How the run was issued: `bang` or `bang_bang`.
    pub origin: String,
    /// `running`, `exited`, `timed_out`, `cancelled`, `failed` or `lost`.
    pub status: String,
}

/// One tool contract the session's own registry holds.
///
/// The contract's **schemas** are deliberately absent: the projection
/// publishes the policy a frontend renders, not the tool's input
/// grammar. These four fields are the ones the TUI's `/tool-contracts`
/// renders, so the two frontends say the same thing about the same tool.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolContractView {
    pub name: String,
    pub caller_policy: String,
    pub effect_class: String,
    pub idempotency: String,
}

/// One plugin in the catalogue the daemon published for the session.
///
/// M007-G (ADR-0057): the daemon's plugin registry is the single
/// authority for the catalogue, and the desktop renders what it published.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginCatalogueEntryView {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub trust: String,
    pub runtime_kind: String,
    pub command_count: usize,
    pub hook_count: usize,
    pub diagnostic_count: usize,
    pub has_error: bool,
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

/// The daemon's answer to one skill-promotion request.
///
/// M007-I (plan 022, work package F): the daemon owns the durable
/// promotion record, so the desktop asks and renders — it holds no
/// promotion state of its own. The view carries what the daemon said and
/// nothing the desktop would have to infer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPromotionView {
    /// The action the daemon performed.
    pub action: String,
    /// Whether the durable record changed.
    pub changed: bool,
    /// The request's identity, when the action produced one.
    pub request_id: Option<String>,
    /// The bounded draft prompt, when the action staged a request.
    pub draft_prompt: Option<String>,
    /// The proposals, when the action listed or read them.
    pub proposals: Option<Vec<SkillProposalSummaryView>>,
    /// The proposal in full, when the action read or previewed one.
    pub detail: Option<SkillProposalDetailView>,
    /// The published proposal's relative path, when the action published one.
    pub published_relative_path: Option<String>,
}

/// One proposal, as the daemon reports it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillProposalSummaryView {
    pub proposal_id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub revision: u64,
}

/// One proposal in full, as the daemon reports it.
///
/// The body is the model's own `SKILL.md` draft. The human reads it before
/// approving it, which is why the detail carries it and the listing does
/// not. The daemon bounds the body, so the bridge carries a bounded
/// document too.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillProposalDetailView {
    pub proposal_id: String,
    pub name: String,
    pub status: String,
    pub revision: u64,
    pub target_scope: String,
    pub skill_markdown: String,
    pub diagnostics: Vec<SkillProposalDiagnosticView>,
    pub published_relative_path: Option<String>,
}

/// One diagnostic a proposal carries.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillProposalDiagnosticView {
    pub severity: String,
    pub reason: String,
    pub location: Option<String>,
}

/// One argument form of a command, as the daemon's catalog describes it.
///
/// The form is the classification unit: a command's argument can select a
/// different effect, so the renderer reads the kind off the form.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandFormView {
    /// The form's selector. Empty is the command's default form.
    pub form: String,
    /// `daemon_effect` or `frontend_effect`.
    pub kind: String,
    /// A one-line hint describing the form's argument.
    pub argument_hint: String,
    /// Whether the form requires an argument.
    pub argument_required: bool,
    /// Whether the daemon's effect table serves this form.
    ///
    /// The renderer reads this to show a command the daemon does not serve
    /// as unavailable, instead of dispatching into a refusal.
    pub daemon_served: bool,
    /// The declared frontend action's stable wire name, or `None` for a
    /// daemon-effect form.
    ///
    /// M007-017: the daemon publishes the action's name on the form, so the
    /// renderer dispatches by the daemon's own answer and never guesses an
    /// action from a command spelling. The host forwards it; it does not
    /// derive it, because the daemon owns the action set.
    pub action: Option<String>,
}

/// One command in the daemon's catalog.
///
/// The renderer holds no command list of its own: every name, alias and
/// kind it shows comes from here.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDescriptorView {
    /// The command's stable identity, shared by every frontend.
    pub command_id: String,
    /// The canonical display name, including its leading slash.
    pub name: String,
    /// Alternate spellings that resolve to the same command.
    pub aliases: Vec<String>,
    /// One-line description.
    pub description: String,
    /// The discovery domain.
    pub domain: String,
    /// The kind of the command's default form.
    pub kind: String,
    /// The capability a daemon-effect form requires. Absent for a wholly
    /// frontend-effect command.
    pub capability: Option<String>,
    /// The command's forms.
    pub forms: Vec<CommandFormView>,
}

/// The daemon's command catalog.
///
/// The list is bounded by the daemon, and the version is what an execution
/// must quote: a version the daemon no longer serves is refused rather
/// than executed against a stale classification.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandCatalogView {
    /// The catalog version an execution must quote.
    pub catalog_version: u64,
    /// Every command the daemon knows about.
    pub commands: Vec<CommandDescriptorView>,
}

/// The structured report of one command execution.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandReportView {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub shadowed: usize,
    pub invalid: usize,
}

/// The result of one daemon-effect command execution.
///
/// A refusal produces the same shape with `status` `refused` and an empty
/// `runId`: the daemon refused before it opened a run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecuteView {
    /// The canonical run the execution was audited as.
    pub run_id: String,
    /// The command's stable identity.
    pub command_id: String,
    /// `completed`, `refused`, `failed` or `cancelled`.
    pub status: String,
    /// A one-line summary of the outcome.
    pub summary: String,
    /// The structured output lines.
    pub lines: Vec<String>,
    /// The structured report, when the command produces one.
    pub report: Option<CommandReportView>,
    /// When the execution started.
    pub started_at_ms: i64,
    /// When the execution finished.
    pub completed_at_ms: i64,
    /// The response the command's effect produced, exactly as the daemon's
    /// own request handler serializes it.
    ///
    /// The daemon owns the effect *and* the authoritative data; the
    /// frontend keeps the formatting. Carrying the inner response is what
    /// makes the desktop's presentation parity with the TUI's real rather
    /// than approximate: the renderer can format the same authoritative
    /// data the TUI formats.
    pub response: Option<serde_json::Value>,
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
    /// The session's selected model, as the daemon published it. `None`
    /// means the daemon published no selection, not that the session has
    /// none.
    pub selected_model: Option<String>,
    /// The session's selected agent, as the daemon published it.
    pub selected_agent: Option<String>,
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
    /// The human-shell command runs the daemon published for the session,
    /// oldest first within the bounded window.
    pub shell_runs: Vec<ShellRunView>,
    /// The tool contracts the session's own registry holds, in the order
    /// the daemon published them.
    pub tool_contracts: Vec<ToolContractView>,
    /// The plugin catalogue the daemon published for the session, in the
    /// order the daemon published it.
    pub plugin_catalogue: Vec<PluginCatalogueEntryView>,
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
