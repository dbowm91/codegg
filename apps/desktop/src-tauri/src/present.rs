//! Bounded session-projection presentation (M004 WP C).
//!
//! This module derives renderer presentation DTOs from the canonical
//! [`SessionProjectionSnapshot`](codegg_protocol::projection::snapshot::SessionProjectionSnapshot)
//! published by the shared [`SessionProjectionDriver`](codegg_client::SessionProjectionDriver).
//! It owns no projection state, no cursor authority, and no reducer:
//! every function here is a pure bounded projection of an already
//! visibility-sanitized canonical snapshot into a renderer-safe view.
//!
//! Least-privilege rules enforced in this module:
//! - only `VisibilityClass::Public` messages with `User`/`Assistant`/`Tool`
//!   roles are rendered; `Reasoning`/`System` roles and non-public
//!   visibility never cross the bridge even if a future reducer leaks one;
//! - only `VisibilityClass::Public` tools are rendered, and the tool
//!   arguments/output that cross are the daemon's withheld or summarised
//!   forms, never a raw inline value;
//! - raw tool arguments/output are never serialized: tools cross as
//!   summary lines plus artifact-handle markers only;
//! - filesystem paths never cross: run `log_dir` is dropped, permission
//!   `path` becomes a display-only scope summary that is never sent back
//!   as authority;
//! - only genuinely pending permissions and questions cross: the daemon
//!   keeps a resolved item in its deque, and a resolved item must not
//!   render as an actionable request;
//! - every collection is tail-capped so the renderer observes bounded
//!   arrays; sequence/cursor fields are diagnostic and must never become
//!   projection authority in the renderer.

use codegg_client::{DriverSnapshotView, DriverState};
use codegg_protocol::projection::dto::{
    AgentRunGroupSummaryProjection, AgentRunSummary, AgentTreeNodeProjection,
    ConvergenceSummaryProjection, MessageProjection, MessageRole, PermissionProjection,
    PermissionStatus, PluginCatalogueEntryProjection, QuestionProjection, RunProjection,
    ShellRunProjection, ToolArgumentProjection, ToolContractProjection, ToolOutputProjection,
    ToolProgramSummary, ToolProjection, TurnProjection, VisibilityClass, WorktreeSummaryProjection,
};
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;
use serde::Serialize;

use super::bridge::{
    AgentRunView, ArtifactHandleView, ConvergenceView, CursorDiagnosticView, JobSummaryView,
    MessageView, PendingPermissionView, PendingQuestionView, PluginCatalogueEntryView,
    RunGroupView, RunSummaryView, SessionPresentationView, ShellRunView, SubagentSummaryView,
    ToolContractView, ToolProgramView, ToolSummaryView, TranscriptMessageView, TurnSummaryView,
    WorktreeView,
};

/// Renderer cap on visible messages from the current/most-recent turn.
/// Older messages are dropped from the view (count reported in
/// `truncated_messages`); the canonical snapshot retains its own bound.
pub const MAX_VISIBLE_MESSAGES: usize = 100;
/// Renderer cap on tool summaries from the current/most-recent turn.
pub const MAX_VISIBLE_TOOLS: usize = 20;
/// Renderer cap on run/job summaries.
pub const MAX_VISIBLE_RUNS: usize = 10;
pub const MAX_VISIBLE_JOBS: usize = 10;
/// Renderer cap on subagent and agent-tree summaries. The daemon caps the
/// session liveness authority at `MAX_PROJECTION_SESSION_SUBAGENTS` = 32
/// (crates/codegg-protocol/src/projection/limits.rs:70) and the per-turn
/// tree at `MAX_PROJECTION_SUBAGENTS` = 16
/// (crates/codegg-protocol/src/projection/limits.rs:67).
pub const MAX_VISIBLE_SUBAGENTS: usize = 10;
/// Renderer cap on recent (completed) turn summaries.
pub const MAX_RECENT_TURNS: usize = 5;
/// Renderer cap on transcript messages across every turn. Mirrors the
/// daemon's per-turn message cap (`MAX_PROJECTION_MESSAGES`,
/// crates/codegg-protocol/src/projection/limits.rs:24): the host never
/// carries more messages in the whole transcript than the daemon retains
/// for a single turn.
pub const MAX_TRANSCRIPT_MESSAGES: usize = 256;
/// Renderer cap on recent turns contributing transcript messages. The
/// daemon keeps up to `MAX_PROJECTION_RECENT_TOOLS` = 32 turn slots
/// (crates/codegg-protocol/src/projection/limits.rs:28); the host reads
/// the same five turns its `MAX_RECENT_TURNS` summary cap uses.
pub const MAX_TRANSCRIPT_TURNS: usize = 5;
/// Renderer cap on durable delegated agent run summaries. Daemon cap:
/// `MAX_PROJECTION_AGENT_RUNS` = 64
/// (crates/codegg-protocol/src/projection/limits.rs:112).
pub const MAX_VISIBLE_AGENT_RUNS: usize = 16;
/// Renderer cap on background tool program summaries. Daemon cap:
/// `MAX_PROJECTION_TOOL_PROGRAMS` = 32
/// (crates/codegg-protocol/src/projection/limits.rs:50).
pub const MAX_VISIBLE_TOOL_PROGRAMS: usize = 16;
/// Renderer cap on managed worktree summaries. Daemon cap:
/// `MAX_PROJECTION_WORKTREES` = 32
/// (crates/codegg-protocol/src/projection/limits.rs:116).
pub const MAX_VISIBLE_WORKTREES: usize = 16;
/// Renderer cap on durable run-group summaries. Daemon cap:
/// `MAX_PROJECTION_RUN_GROUPS` = 32
/// (crates/codegg-protocol/src/projection/limits.rs:120).
pub const MAX_VISIBLE_RUN_GROUPS: usize = 16;
/// Renderer cap on bounded convergence summaries. Daemon cap:
/// `MAX_PROJECTION_CONVERGENCES` = 32
/// (crates/codegg-protocol/src/projection/limits.rs:123).
pub const MAX_VISIBLE_CONVERGENCES: usize = 16;
/// Renderer cap on human-shell command runs. Daemon cap:
/// `MAX_PROJECTION_SHELL_RUNS` = 32
/// (crates/codegg-protocol/src/projection/limits.rs:54).
pub const MAX_VISIBLE_SHELL_RUNS: usize = 16;
/// Renderer cap on tool contracts. Daemon cap:
/// `MAX_PROJECTION_TOOL_CONTRACTS` = 64
/// (crates/codegg-protocol/src/projection/limits.rs:57).
pub const MAX_VISIBLE_TOOL_CONTRACTS: usize = 32;
/// Renderer cap on published plugin catalogue entries. Daemon cap:
/// `MAX_PROJECTION_PLUGIN_CATALOGUE` = 64
/// (crates/codegg-protocol/src/projection/limits.rs).
pub const MAX_VISIBLE_PLUGIN_CATALOGUE: usize = 32;
/// Renderer cap on surfaced artifact handles.
pub const MAX_ARTIFACT_HANDLES: usize = 16;
/// Renderer cap on pending permission/question summaries per view.
pub const MAX_PENDING_PERMISSIONS: usize = 16;
pub const MAX_PENDING_QUESTIONS: usize = 16;

/// Canonical wire spelling of a serializable projection enum
/// (`snake_case` via serde, matching the daemon contract).
fn enum_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|json| json.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into())
}

pub fn driver_state_name(state: DriverState) -> &'static str {
    match state {
        DriverState::Connecting => "connecting",
        DriverState::Subscribing => "subscribing",
        DriverState::Attached => "attached",
        DriverState::Resyncing => "resyncing",
        DriverState::Disconnected => "disconnected",
        DriverState::Unavailable => "unavailable",
    }
}

/// `true` when a canonical payload may cross to the renderer.
///
/// The daemon classifies every projected payload as `Public`,
/// `ClientLocal`, `Internal`, or `Sensitive`
/// (`crates/codegg-protocol/src/projection/dto.rs:21-44`). `Internal` is
/// "never serialised into a projection event" and `Sensitive` is
/// "redacted before leaving the daemon" (same lines), so neither may
/// reach a renderer. The daemon's own client-facing snapshot filter
/// keeps only `Public` messages and tools
/// (`crates/codegg-protocol/src/projection/consumer.rs:767-772`); the
/// host re-checks that class on every field it copies, because the host
/// is a second boundary between the daemon and the renderer and a
/// snapshot that arrives over-classified must still be filtered here.
fn visibility_crosses(visibility: VisibilityClass) -> bool {
    matches!(visibility, VisibilityClass::Public)
}

/// `true` when a canonical message may cross to the renderer:
/// public visibility and a conversational role. Reasoning and system
/// content never render, even if a future reducer marks them public.
fn message_is_visible(message: &MessageProjection) -> bool {
    if !visibility_crosses(message.visibility) {
        return false;
    }
    matches!(
        message.role,
        MessageRole::User | MessageRole::Assistant | MessageRole::Tool
    )
}

/// `true` when a canonical tool may cross to the renderer. A tool
/// carries the same visibility classification as a message, so the
/// host applies the same gate: a non-public tool's arguments and output
/// never reach the renderer, not even in summarised form.
fn tool_is_visible(tool: &ToolProjection) -> bool {
    visibility_crosses(tool.visibility)
}

fn message_view(message: &MessageProjection) -> MessageView {
    MessageView {
        message_id: message.message_id.clone(),
        role: enum_name(&message.role),
        text: message.text.clone(),
        truncated: message.truncated,
    }
}

fn turn_summary(turn: &TurnProjection) -> TurnSummaryView {
    TurnSummaryView {
        turn_id: turn.turn_id.clone(),
        status: enum_name(&turn.status),
        updated_at: turn.updated_at,
        stop_reason: turn.stop_reason.clone(),
        error: turn.error.clone(),
        message_count: turn.messages.len(),
        tool_count: turn.tools.len(),
        pending_permissions: turn
            .pending_permissions
            .iter()
            .filter(|permission| permission.status == PermissionStatus::Pending)
            .count(),
        pending_questions: turn
            .pending_questions
            .iter()
            .filter(|question| question.status == PermissionStatus::Pending)
            .count(),
        input_tokens: turn.input_tokens,
        output_tokens: turn.output_tokens,
        cached_tokens: turn.cached_tokens,
        context_limit: turn.context_limit,
        // Reasoning *text* is `VisibilityClass::Internal`
        // (crates/codegg-protocol/src/projection/reducer.rs:401-410), so
        // the token count is the whole of what may cross.
        reasoning_tokens: turn.reasoning_tokens,
    }
}

/// Summarize tool arguments without leaking raw values: inline raw
/// arguments collapse to a marker (full text stays behind the
/// artifact path); daemon summaries and truncated previews pass
/// through as display strings. A handle argument surfaces as an
/// artifact handle for the excerpt path.
fn arguments_summary(tool: &ToolProjection) -> (String, Option<ArtifactHandleView>) {
    match &tool.arguments {
        ToolArgumentProjection::Inline { .. } => ("arguments withheld".into(), None),
        ToolArgumentProjection::Summary { summary } => (summary.clone(), None),
        ToolArgumentProjection::TruncatedArguments {
            original_bytes,
            preview,
        } => (
            format!("arguments truncated ({original_bytes} bytes): {preview}"),
            None,
        ),
        ToolArgumentProjection::Handle {
            handle,
            byte_length,
        } => (
            format!("arguments behind artifact handle ({byte_length} bytes)"),
            Some(ArtifactHandleView {
                handle: handle.clone(),
                byte_length: *byte_length,
            }),
        ),
    }
}

fn tool_summary(tool: &ToolProjection) -> (ToolSummaryView, Vec<ArtifactHandleView>) {
    let (arguments, args_handle) = arguments_summary(tool);
    let (output_summary, output_handle) = match &tool.output {
        ToolOutputProjection::Pending => ("pending".to_string(), None),
        ToolOutputProjection::Inline { .. } => ("output withheld".to_string(), None),
        ToolOutputProjection::Summary { summary } => (summary.clone(), None),
        ToolOutputProjection::TruncatedOutput {
            original_bytes,
            preview,
        } => (
            format!("output truncated ({original_bytes} bytes): {preview}"),
            None,
        ),
        ToolOutputProjection::Handle {
            handle,
            byte_length,
        } => (
            format!("output behind artifact handle ({byte_length} bytes)"),
            Some(ArtifactHandleView {
                handle: handle.clone(),
                byte_length: *byte_length,
            }),
        ),
    };
    let mut handles = Vec::new();
    if let Some(handle) = args_handle {
        handles.push(handle);
    }
    if let Some(handle) = output_handle {
        handles.push(handle);
    }
    let has_artifact = !handles.is_empty();
    (
        ToolSummaryView {
            tool_id: tool.tool_id.clone(),
            tool_name: tool.tool_name.clone(),
            status: enum_name(&tool.status),
            summary: format!("{arguments} / {output_summary}"),
            has_artifact,
            // The same withheld/summarised strings the `summary` line is
            // built from, split out so a renderer can lay a tool call out
            // in parts. Neither can carry a raw inline value.
            arguments,
            output: output_summary,
            started_at: tool.started_at,
            completed_at: tool.completed_at,
            duration_ms: tool.duration_ms,
        },
        handles,
    )
}

fn permission_view(permission: &PermissionProjection) -> PendingPermissionView {
    PendingPermissionView {
        permission_id: permission.permission_id.clone(),
        tool: permission.tool.clone(),
        // Display-only scope hint. Never authority: WP E responses carry
        // the opaque permission id and choice, never this string.
        scope_summary: permission.path.clone(),
        status: enum_name(&permission.status),
    }
}

fn question_view(question: &QuestionProjection) -> PendingQuestionView {
    PendingQuestionView {
        question_id: question.question_id.clone(),
        header: question.header.clone(),
        prompt: question.prompt.clone(),
        status: enum_name(&question.status),
    }
}

/// One subagent node. The session liveness authority and the per-turn
/// agent tree carry the same node DTO, so one mapper serves both.
fn subagent_view(node: &AgentTreeNodeProjection) -> SubagentSummaryView {
    SubagentSummaryView {
        task_id: node.task_id,
        agent: node.agent.clone(),
        description: node.description.clone(),
        status: enum_name(&node.status),
        result_summary: node.result_summary.clone(),
        child_instance_id: node.child_instance_id.clone(),
        parent_task_id: node.parent_task_id,
        originating_turn_id: node.originating_turn_id.clone(),
        display_name: node.display_name.clone(),
        run_id: node.run_id.clone(),
    }
}

/// One durable delegated agent run. The canonical DTO already excludes
/// prompt, transcript, mailbox body, hidden reasoning, credentials, and
/// raw logs (`crates/codegg-protocol/src/projection/dto.rs:544-548`); the
/// host additionally drops the workspace and repository identity, which
/// the renderer has no use for.
fn agent_run_view(run: &AgentRunSummary) -> AgentRunView {
    AgentRunView {
        run_id: run.run_id.clone(),
        task_id: run.task_id.clone(),
        agent: run.agent.clone(),
        status: run.status.clone(),
        depth: run.depth,
        parent_run_id: run.parent_run_id.clone(),
        worktree_id: run.worktree_id.clone(),
        branch: run.branch.clone(),
        result_commit: run.result_commit.clone(),
        group_id: run.group_id.clone(),
        control_status: run.control_status.clone(),
        attention_required: run.attention_required,
        progress: run.progress.clone(),
        terminal_summary: run.terminal_summary.clone(),
        failure_class: run.failure_class.clone(),
        updated_at: run.updated_at,
    }
}

/// One background tool program.
fn tool_program_view(program: &ToolProgramSummary) -> ToolProgramView {
    ToolProgramView {
        program_id: program.program_id.clone(),
        job_id: program.job_id.clone(),
        state: program.state.clone(),
        phase: program.phase.clone(),
        language: program.language.clone(),
        parent_turn_id: program.parent_turn_id.clone(),
        calls_completed: program.calls_completed,
        child_jobs_running: program.child_jobs_running,
        submitted_at: program.submitted_at,
        started_at: program.started_at,
        completed_at: program.completed_at,
        failure_class: program.failure_class.clone(),
        terminal_handle: program.terminal_handle.clone(),
        last_progress: program.last_progress.clone(),
    }
}

/// One managed worktree.
fn worktree_view(worktree: &WorktreeSummaryProjection) -> WorktreeView {
    WorktreeView {
        worktree_id: worktree.worktree_id.clone(),
        owner_run_id: worktree.owner_run_id.clone(),
        branch: worktree.branch.clone(),
        state: worktree.state.clone(),
        health: worktree.health.clone(),
        dirty: worktree.dirty,
        conflicted: worktree.conflicted,
        retained_for_attention: worktree.retained_for_attention,
        updated_at: worktree.updated_at,
    }
}

/// One durable run group. `member_run_ids` becomes a count: the daemon
/// states that member ids are opaque references and that consumers use
/// the run summaries for member detail
/// (`crates/codegg-protocol/src/projection/dto.rs:675-677`), so the
/// renderer joins on `AgentRunView::group_id` instead of carrying a
/// second copy of every member id.
fn run_group_view(group: &AgentRunGroupSummaryProjection) -> RunGroupView {
    RunGroupView {
        group_id: group.group_id.clone(),
        owner_run_id: group.owner_run_id.clone(),
        owner_kind: group.owner_kind.clone(),
        status: group.status.clone(),
        join_policy: group.join_policy.clone(),
        member_count: group.member_run_ids.len(),
        successful: group.successful,
        failed: group.failed,
        active: group.active,
        winner_run_id: group.winner_run_id.clone(),
        cancel_remaining_on_satisfaction: group.cancel_remaining_on_satisfaction,
        updated_at: group.updated_at,
    }
}

/// One bounded convergence summary. Detailed verdict findings stay
/// behind their durable handles, as the canonical DTO states at
/// `crates/codegg-protocol/src/projection/dto.rs:712-713`.
fn convergence_view(convergence: &ConvergenceSummaryProjection) -> ConvergenceView {
    ConvergenceView {
        convergence_id: convergence.convergence_id.clone(),
        owner_summary: convergence.owner_summary.clone(),
        status: convergence.status.clone(),
        cycle_ordinal: convergence.cycle_ordinal,
        max_cycles: convergence.max_cycles,
        remaining_cycles: convergence.remaining_cycles,
        producer_completed: convergence.producer_completed,
        producer_failed: convergence.producer_failed,
        producer_cancelled: convergence.producer_cancelled,
        producer_active: convergence.producer_active,
        verifier_run_id: convergence.verifier_run_id.clone(),
        verdict_kind: convergence.verdict_kind.clone(),
        verdict_summary: convergence.verdict_summary.clone(),
        awaiting_decision: convergence.awaiting_decision,
        terminal_reason_class: convergence.terminal_reason_class.clone(),
        selected_run_id: convergence.selected_run_id.clone(),
        last_finding_count: convergence.last_finding_count,
    }
}

/// One human-shell command run. The run's output bytes never cross: the
/// daemon serves them on demand through its own read, and the projection
/// publishes the listing.
fn shell_run_view(run: &ShellRunProjection) -> ShellRunView {
    ShellRunView {
        run_id: run.run_id.clone(),
        command: run.command.clone(),
        cwd: run.cwd.clone(),
        origin: run.origin.clone(),
        status: run.status.clone(),
    }
}

/// One tool contract the session's own registry holds. The contract's
/// schemas stay behind the daemon: the projection publishes the policy a
/// frontend renders.
fn tool_contract_view(contract: &ToolContractProjection) -> ToolContractView {
    ToolContractView {
        name: contract.name.clone(),
        caller_policy: contract.caller_policy.clone(),
        effect_class: contract.effect_class.clone(),
        idempotency: contract.idempotency.clone(),
    }
}

/// One published plugin, as the renderer shows it.
fn plugin_catalogue_entry_view(entry: &PluginCatalogueEntryProjection) -> PluginCatalogueEntryView {
    PluginCatalogueEntryView {
        plugin_id: entry.plugin_id.clone(),
        name: entry.name.clone(),
        version: entry.version.clone(),
        enabled: entry.enabled,
        trust: entry.trust.clone(),
        runtime_kind: entry.runtime_kind.clone(),
        command_count: entry.command_count,
        hook_count: entry.hook_count,
        diagnostic_count: entry.diagnostic_count,
        has_error: entry.has_error,
    }
}

/// One transcript message, tagged with the turn that owns it.
fn transcript_message_view(
    turn: &TurnProjection,
    message: &MessageProjection,
) -> TranscriptMessageView {
    TranscriptMessageView {
        message_id: message.message_id.clone(),
        turn_id: turn.turn_id.clone(),
        role: enum_name(&message.role),
        text: message.text.clone(),
        truncated: message.truncated,
        created_at: message.created_at,
    }
}

/// Build the multi-turn transcript and the count of messages dropped
/// from its head.
///
/// The daemon keeps its recent turns newest-first
/// (`crates/codegg-protocol/src/projection/reducer.rs:1574-1579`), so the
/// recent turns are walked in reverse to render chronologically; the
/// active turn is appended last because it is the newest turn.
fn transcript(snapshot: &SessionProjectionSnapshot) -> (Vec<TranscriptMessageView>, usize) {
    let mut messages: Vec<TranscriptMessageView> = Vec::new();
    for turn in snapshot
        .recent_turns
        .iter()
        .take(MAX_TRANSCRIPT_TURNS)
        .rev()
    {
        for message in turn
            .messages
            .iter()
            .filter(|message| message_is_visible(message))
        {
            messages.push(transcript_message_view(turn, message));
        }
    }
    if let Some(turn) = snapshot.active_turn.as_ref() {
        for message in turn
            .messages
            .iter()
            .filter(|message| message_is_visible(message))
        {
            messages.push(transcript_message_view(turn, message));
        }
    }
    let truncated = messages.len().saturating_sub(MAX_TRANSCRIPT_MESSAGES);
    if truncated > 0 {
        let skip = messages.len() - MAX_TRANSCRIPT_MESSAGES;
        messages = messages.into_iter().skip(skip).collect();
    }
    (messages, truncated)
}

/// Build a bounded presentation view from a canonical snapshot and the
/// current driver state.
///
/// The "current" turn is the active turn when one exists, otherwise the
/// most-recent completed turn; messages and tool summaries come from it
/// alone. Recent-turn summaries, run/job/subagent summaries, pending
/// permission/question items (active turn first, then recent turns,
/// deduplicated), and artifact handles complete the view.
pub fn present_snapshot(
    snapshot: &SessionProjectionSnapshot,
    state: DriverState,
    resync_reason: Option<String>,
    driver_cursor_seq: Option<u64>,
    subscription_known: bool,
) -> SessionPresentationView {
    let current_turn = snapshot
        .active_turn
        .as_ref()
        .or_else(|| snapshot.recent_turns.front());

    let mut messages: Vec<MessageView> = current_turn
        .map(|turn| {
            turn.messages
                .iter()
                .filter(|message| message_is_visible(message))
                .map(message_view)
                .collect()
        })
        .unwrap_or_default();
    let truncated_messages = messages.len().saturating_sub(MAX_VISIBLE_MESSAGES);
    if truncated_messages > 0 {
        let skip = messages.len() - MAX_VISIBLE_MESSAGES;
        messages = messages.into_iter().skip(skip).collect();
    }

    let mut artifact_handles: Vec<ArtifactHandleView> = Vec::new();
    let tools: Vec<ToolSummaryView> = current_turn
        .map(|turn| {
            // The visibility gate runs before the tail cap, so a
            // non-public tool never occupies a visible slot.
            let visible: Vec<&ToolProjection> = turn
                .tools
                .iter()
                .filter(|tool| tool_is_visible(tool))
                .collect();
            visible
                .into_iter()
                .rev()
                .take(MAX_VISIBLE_TOOLS)
                .rev()
                .map(|tool| {
                    let (summary, handles) = tool_summary(tool);
                    for handle in handles {
                        if artifact_handles.len() < MAX_ARTIFACT_HANDLES {
                            artifact_handles.push(handle);
                        }
                    }
                    summary
                })
                .collect()
        })
        .unwrap_or_default();

    // Pending items: active turn first, then recent turns, deduplicated
    // by id so a carried-over item renders once.
    let mut seen_permissions = std::collections::HashSet::new();
    let mut pending_permissions: Vec<PendingPermissionView> = Vec::new();
    let mut seen_questions = std::collections::HashSet::new();
    let mut pending_questions: Vec<PendingQuestionView> = Vec::new();
    let ordered_turns = snapshot
        .active_turn
        .iter()
        .chain(snapshot.recent_turns.iter())
        .take(MAX_RECENT_TURNS + 1);
    for turn in ordered_turns {
        for permission in &turn.pending_permissions {
            if pending_permissions.len() >= MAX_PENDING_PERMISSIONS {
                break;
            }
            // Only an unresolved item is actionable. The daemon keeps a
            // resolved permission in the deque and rewrites its status
            // in place
            // (crates/codegg-protocol/src/projection/reducer.rs:571-577),
            // so the status check is what separates "waiting for a
            // human" from "already answered".
            if permission.status != PermissionStatus::Pending {
                continue;
            }
            if seen_permissions.insert(permission.permission_id.clone()) {
                pending_permissions.push(permission_view(permission));
            }
        }
        for question in &turn.pending_questions {
            if pending_questions.len() >= MAX_PENDING_QUESTIONS {
                break;
            }
            // Same rule as a permission: the daemon rewrites a resolved
            // question's status in place and leaves it in the deque
            // (crates/codegg-protocol/src/projection/reducer.rs:642-648).
            // A question carries the same `PermissionStatus` enum
            // (crates/codegg-protocol/src/projection/dto.rs:424), so one
            // status check covers both kinds.
            if question.status != PermissionStatus::Pending {
                continue;
            }
            if seen_questions.insert(question.question_id.clone()) {
                pending_questions.push(question_view(question));
            }
        }
    }

    let (transcript, truncated_transcript_messages) = transcript(snapshot);

    SessionPresentationView {
        session_id: snapshot.primary_session_id.clone(),
        project_id: snapshot.project_id.clone(),
        workspace_id: snapshot.workspace_id.clone(),
        state: driver_state_name(state).into(),
        // The selected model and agent are the daemon's own answer for the
        // session; the renderer never derives them.
        selected_model: snapshot.primary_session.selected_model.clone(),
        selected_agent: snapshot.primary_session.selected_agent.clone(),
        turn: current_turn.map(turn_summary),
        messages,
        truncated_messages,
        transcript,
        truncated_transcript_messages,
        tools,
        runs: snapshot
            .runs
            .iter()
            .rev()
            .take(MAX_VISIBLE_RUNS)
            .rev()
            .map(|run: &RunProjection| RunSummaryView {
                run_id: run.run_id.clone(),
                kind: run.kind.clone(),
                status: run.status.clone(),
                summary: run.summary.clone(),
            })
            .collect(),
        jobs: snapshot
            .jobs
            .iter()
            .rev()
            .take(MAX_VISIBLE_JOBS)
            .rev()
            .map(|job| JobSummaryView {
                job_id: job.job_id.clone(),
                kind: job.kind.clone(),
                state: job.state.clone(),
                summary: job.summary.clone(),
            })
            .collect(),
        // Subagent liveness comes from the session-level authority: the
        // canonical snapshot names `subagents` as the liveness authority
        // because, unlike the conversational agent tree, these survive
        // parent-turn replacement
        // (crates/codegg-protocol/src/projection/snapshot.rs:52-55). The
        // per-turn tree is carried alongside it, because it still holds
        // legacy nodes with no durable child identity, which the
        // reducer keeps in the turn-local compatibility view only
        // (crates/codegg-protocol/src/projection/reducer.rs:761-771).
        subagents: snapshot
            .subagents
            .iter()
            .rev()
            .take(MAX_VISIBLE_SUBAGENTS)
            .rev()
            .map(subagent_view)
            .collect(),
        agent_tree: snapshot
            .active_turn
            .as_ref()
            .map(|turn| {
                turn.agent_tree
                    .iter()
                    .rev()
                    .take(MAX_VISIBLE_SUBAGENTS)
                    .rev()
                    .map(subagent_view)
                    .collect()
            })
            .unwrap_or_default(),
        agent_runs: snapshot
            .agent_runs
            .iter()
            .rev()
            .take(MAX_VISIBLE_AGENT_RUNS)
            .rev()
            .map(agent_run_view)
            .collect(),
        tool_programs: snapshot
            .tool_programs
            .iter()
            .rev()
            .take(MAX_VISIBLE_TOOL_PROGRAMS)
            .rev()
            .map(tool_program_view)
            .collect(),
        worktrees: snapshot
            .worktrees
            .iter()
            .rev()
            .take(MAX_VISIBLE_WORKTREES)
            .rev()
            .map(worktree_view)
            .collect(),
        run_groups: snapshot
            .run_groups
            .iter()
            .rev()
            .take(MAX_VISIBLE_RUN_GROUPS)
            .rev()
            .map(run_group_view)
            .collect(),
        convergences: snapshot
            .convergences
            .iter()
            .rev()
            .take(MAX_VISIBLE_CONVERGENCES)
            .rev()
            .map(convergence_view)
            .collect(),
        // The daemon's own registry order, oldest first: the reducer
        // appends a run as it happens, so the newest run is last.
        shell_runs: snapshot
            .shell_runs
            .iter()
            .rev()
            .take(MAX_VISIBLE_SHELL_RUNS)
            .rev()
            .map(shell_run_view)
            .collect(),
        // The contracts keep the order the daemon published them in.
        tool_contracts: snapshot
            .tool_contracts
            .iter()
            .take(MAX_VISIBLE_TOOL_CONTRACTS)
            .map(tool_contract_view)
            .collect(),
        // The plugin catalogue keeps the order the daemon published it in.
        plugin_catalogue: snapshot
            .plugin_catalogue
            .iter()
            .take(MAX_VISIBLE_PLUGIN_CATALOGUE)
            .map(plugin_catalogue_entry_view)
            .collect(),
        recent_turns: snapshot
            .recent_turns
            .iter()
            .take(MAX_RECENT_TURNS)
            .map(turn_summary)
            .collect(),
        // Todos are not part of the canonical projection: they live
        // behind a separate daemon read (`CoreRequest::TodoList`), so the
        // pure presenter carries no list. The host's todo refresh
        // overlays the read list onto the presented view.
        todos: None,
        pending_permissions,
        pending_questions,
        // The controller lease is not part of the canonical projection:
        // the host overlays it onto the presented view from
        // `SessionControlGet`
        // (apps/desktop/src-tauri/src/control.rs:44-52), so the pure
        // presenter leaves it unset.
        controller: None,
        artifact_handles,
        cursor: CursorDiagnosticView {
            event_seq: snapshot.event_seq,
            driver_cursor_seq,
            subscription_known,
        },
        resync_reason,
    }
}

/// Build the current presentation view from a published driver view.
/// With no installed snapshot yet (connecting/subscribing), publishes a
/// skeleton carrying identity + state so the renderer can distinguish
/// "no snapshot yet" from "empty session".
pub fn present_driver(view: &DriverSnapshotView) -> SessionPresentationView {
    match &view.snapshot {
        Some(snapshot) => present_snapshot(
            snapshot,
            view.state,
            view.resync_reason.map(|reason| format!("{reason:?}")),
            view.cursor_seq,
            view.subscription_id.is_some(),
        ),
        None => SessionPresentationView {
            session_id: view.session_id.clone(),
            project_id: String::new(),
            workspace_id: String::new(),
            state: driver_state_name(view.state).into(),
            selected_model: None,
            selected_agent: None,
            turn: None,
            messages: Vec::new(),
            truncated_messages: 0,
            transcript: Vec::new(),
            truncated_transcript_messages: 0,
            tools: Vec::new(),
            runs: Vec::new(),
            jobs: Vec::new(),
            subagents: Vec::new(),
            agent_tree: Vec::new(),
            agent_runs: Vec::new(),
            tool_programs: Vec::new(),
            worktrees: Vec::new(),
            run_groups: Vec::new(),
            convergences: Vec::new(),
            shell_runs: Vec::new(),
            tool_contracts: Vec::new(),
            plugin_catalogue: Vec::new(),
            recent_turns: Vec::new(),
            todos: None,
            pending_permissions: Vec::new(),
            pending_questions: Vec::new(),
            controller: None,
            artifact_handles: Vec::new(),
            cursor: CursorDiagnosticView {
                event_seq: 0,
                driver_cursor_seq: view.cursor_seq,
                subscription_known: view.subscription_id.is_some(),
            },
            resync_reason: view.resync_reason.map(|reason| format!("{reason:?}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::projection::dto::{
        AgentTreeNodeProjection, AgentTreeStatus, JobProjection, MessageRole, PermissionStatus,
        ToolStatus, TurnStatus,
    };
    use std::collections::VecDeque;

    /// One human-shell run as the daemon publishes it.
    fn shell_run(run_id: &str, command: &str, status: &str) -> ShellRunProjection {
        ShellRunProjection {
            run_id: run_id.into(),
            command: command.into(),
            cwd: "/repo".into(),
            origin: "bang".into(),
            status: status.into(),
            started_at_ms: Some(1),
            completed_at_ms: None,
            exit_code: None,
        }
    }

    /// One tool contract as the daemon publishes it.
    fn tool_contract(
        name: &str,
        caller: &str,
        effect: &str,
        idempotency: &str,
    ) -> ToolContractProjection {
        ToolContractProjection {
            name: name.into(),
            caller_policy: caller.into(),
            effect_class: effect.into(),
            idempotency: idempotency.into(),
            implementation_id: format!("impl-{name}"),
        }
    }

    fn message(
        id: &str,
        role: MessageRole,
        visibility: VisibilityClass,
        text: &str,
    ) -> MessageProjection {
        MessageProjection {
            message_id: id.into(),
            parent_turn_id: "turn-1".into(),
            role,
            text: text.into(),
            tool_call_id: None,
            visibility,
            created_at: 0,
            truncated: false,
        }
    }

    fn tool(
        id: &str,
        arguments: ToolArgumentProjection,
        output: ToolOutputProjection,
    ) -> ToolProjection {
        ToolProjection {
            tool_id: id.into(),
            tool_name: "read".into(),
            status: ToolStatus::Completed,
            arguments,
            output,
            visibility: VisibilityClass::Public,
            started_at: None,
            completed_at: None,
            duration_ms: None,
        }
    }

    fn tool_with_visibility(id: &str, visibility: VisibilityClass) -> ToolProjection {
        ToolProjection {
            visibility,
            ..tool(
                id,
                ToolArgumentProjection::Summary {
                    summary: "s".into(),
                },
                ToolOutputProjection::Summary {
                    summary: "o".into(),
                },
            )
        }
    }

    /// A subagent node. `child_instance_id` is empty for a legacy node:
    /// the daemon keeps such a node in the turn-local compatibility view
    /// only.
    fn subagent_node(child_instance_id: &str, task_id: u64) -> AgentTreeNodeProjection {
        AgentTreeNodeProjection {
            child_instance_id: child_instance_id.into(),
            originating_turn_id: Some("turn-1".into()),
            task_id,
            agent: "worker".into(),
            description: "work".into(),
            status: AgentTreeStatus::Running,
            parent_task_id: None,
            created_at: 0,
            completed_at: None,
            result_summary: None,
            display_name: None,
            profile: None,
            usefulness: None,
            run_id: None,
        }
    }

    fn turn(id: &str) -> TurnProjection {
        TurnProjection {
            input_tokens: Some(10),
            output_tokens: Some(20),
            cached_tokens: None,
            reasoning_tokens: None,
            context_limit: None,
            turn_id: id.into(),
            status: TurnStatus::Active,
            started_at: 0,
            updated_at: 7,
            stop_reason: None,
            error: None,
            messages: VecDeque::new(),
            tools: VecDeque::new(),
            pending_permissions: VecDeque::new(),
            pending_questions: VecDeque::new(),
            agent_tree: VecDeque::new(),
            subagent_count: 0,
        }
    }

    fn snapshot_with_turn(
        mut snapshot: SessionProjectionSnapshot,
        turn: TurnProjection,
    ) -> SessionProjectionSnapshot {
        snapshot.active_turn = Some(turn);
        snapshot.event_seq = 42;
        snapshot
    }

    fn base_snapshot() -> SessionProjectionSnapshot {
        let mut snapshot = SessionProjectionSnapshot::empty("session-1", "proj-a", "ws-1");
        snapshot.primary_session.title = "Hello".into();
        snapshot
    }

    #[test]
    fn driver_states_cover_the_renderer_contract() {
        let states = [
            (DriverState::Connecting, "connecting"),
            (DriverState::Subscribing, "subscribing"),
            (DriverState::Attached, "attached"),
            (DriverState::Resyncing, "resyncing"),
            (DriverState::Disconnected, "disconnected"),
            (DriverState::Unavailable, "unavailable"),
        ];
        for (state, name) in states {
            assert_eq!(driver_state_name(state), name);
        }
    }

    #[test]
    fn skeleton_view_marks_no_snapshot_yet() {
        let view = DriverSnapshotView {
            state: DriverState::Subscribing,
            session_id: "session-9".into(),
            generation: 1,
            subscription_id: None,
            cursor_seq: None,
            snapshot: None,
            resync_reason: None,
        };
        let presented = present_driver(&view);
        assert_eq!(presented.state, "subscribing");
        assert_eq!(presented.session_id, "session-9");
        assert!(presented.messages.is_empty());
        assert!(presented.turn.is_none());
        assert!(!presented.cursor.subscription_known);
    }

    #[test]
    fn reasoning_system_and_non_public_messages_never_render() {
        let mut active = turn("turn-1");
        active.messages.extend([
            message(
                "m-user",
                MessageRole::User,
                VisibilityClass::Public,
                "do it",
            ),
            message(
                "m-assistant",
                MessageRole::Assistant,
                VisibilityClass::Public,
                "on it",
            ),
            message(
                "m-reason",
                MessageRole::Reasoning,
                VisibilityClass::Public,
                "secret plan",
            ),
            message(
                "m-system",
                MessageRole::System,
                VisibilityClass::Public,
                "sys prompt",
            ),
            message(
                "m-local",
                MessageRole::Assistant,
                VisibilityClass::ClientLocal,
                "local only",
            ),
            message(
                "m-internal",
                MessageRole::Assistant,
                VisibilityClass::Internal,
                "internal",
            ),
        ]);
        let presented = present_snapshot(
            &snapshot_with_turn(base_snapshot(), active),
            DriverState::Attached,
            None,
            Some(9),
            true,
        );
        let ids: Vec<&str> = presented
            .messages
            .iter()
            .map(|message| message.message_id.as_str())
            .collect();
        assert_eq!(ids, vec!["m-user", "m-assistant"]);
        assert_eq!(presented.turn.as_ref().expect("turn").message_count, 6);
        assert_eq!(presented.cursor.event_seq, 42);
        assert_eq!(presented.cursor.driver_cursor_seq, Some(9));
        // Wire spelling is canonical snake_case.
        assert_eq!(presented.messages[0].role, "user");
        assert_eq!(presented.turn.as_ref().expect("turn").status, "active");
    }

    #[test]
    fn raw_tool_values_never_serialize_and_handles_surface() {
        let secret_args = "SECRET-ARGS-abc123";
        let secret_output = "SECRET-OUTPUT-def456";
        let mut active = turn("turn-1");
        active.tools.extend([
            tool(
                "t-inline",
                ToolArgumentProjection::Inline {
                    arguments: secret_args.into(),
                },
                ToolOutputProjection::Inline {
                    output: secret_output.into(),
                },
            ),
            tool(
                "t-handle",
                ToolArgumentProjection::Summary {
                    summary: "path=src/main.rs mode=rw".into(),
                },
                ToolOutputProjection::Handle {
                    handle: "handle-1".into(),
                    byte_length: 9999,
                },
            ),
        ]);
        let presented = present_snapshot(
            &snapshot_with_turn(base_snapshot(), active),
            DriverState::Attached,
            None,
            None,
            true,
        );
        assert_eq!(presented.tools.len(), 2);
        assert!(!presented.tools[0].has_artifact);
        assert!(presented.tools[1].has_artifact);
        assert_eq!(presented.artifact_handles.len(), 1);
        assert_eq!(presented.artifact_handles[0].handle, "handle-1");
        let json = serde_json::to_value(&presented).expect("json").to_string();
        assert!(!json.contains(secret_args), "raw args leaked");
        assert!(!json.contains(secret_output), "raw output leaked");
        assert!(json.contains("handle-1"), "handle missing");
    }

    #[test]
    fn run_log_dirs_never_serialize() {
        let mut snapshot = base_snapshot();
        snapshot.runs.push_back(RunProjection {
            run_id: "run-1".into(),
            kind: "shell".into(),
            command: "cargo test".into(),
            status: "completed".into(),
            summary: "ok".into(),
            job_id: None,
            log_dir: Some("/secret/daemon/logs/run-1".into()),
            started_at: 0,
            completed_at: None,
            artifact_count: 0,
            pinned: false,
        });
        let presented = present_snapshot(&snapshot, DriverState::Attached, None, None, false);
        assert_eq!(presented.runs.len(), 1);
        assert_eq!(presented.runs[0].run_id, "run-1");
        let json = serde_json::to_value(&presented).expect("json").to_string();
        assert!(!json.contains("/secret/daemon"), "log dir leaked");
        assert!(
            !json.contains("logDir") && !json.contains("log_dir"),
            "log dir key leaked"
        );
    }

    #[test]
    fn collections_are_tail_capped_with_overflow_reported() {
        let mut active = turn("turn-1");
        for index in 0..(MAX_VISIBLE_MESSAGES + 25) {
            active.messages.push_back(message(
                &format!("m-{index}"),
                MessageRole::Assistant,
                VisibilityClass::Public,
                "text",
            ));
        }
        for index in 0..(MAX_VISIBLE_TOOLS + 5) {
            active.tools.push_back(tool(
                &format!("t-{index}"),
                ToolArgumentProjection::Summary {
                    summary: "s".into(),
                },
                ToolOutputProjection::Summary {
                    summary: "o".into(),
                },
            ));
        }
        active
            .agent_tree
            .extend(
                (0..(MAX_VISIBLE_SUBAGENTS + 3)).map(|index| AgentTreeNodeProjection {
                    child_instance_id: format!("child-{index}"),
                    originating_turn_id: Some("turn-1".into()),
                    task_id: index as u64,
                    agent: "worker".into(),
                    description: "work".into(),
                    status: AgentTreeStatus::Running,
                    parent_task_id: None,
                    created_at: 0,
                    completed_at: None,
                    result_summary: None,
                    display_name: None,
                    profile: None,
                    usefulness: None,
                    run_id: None,
                }),
            );
        let mut snapshot = base_snapshot();
        // The session liveness authority carries its own subagents, bounded
        // independently of the per-turn agent tree.
        snapshot
            .subagents
            .extend(
                (0..(MAX_VISIBLE_SUBAGENTS + 3)).map(|index| AgentTreeNodeProjection {
                    child_instance_id: format!("live-child-{index}"),
                    originating_turn_id: Some("turn-1".into()),
                    task_id: index as u64,
                    agent: "worker".into(),
                    description: "work".into(),
                    status: AgentTreeStatus::Running,
                    parent_task_id: None,
                    created_at: 0,
                    completed_at: None,
                    result_summary: None,
                    display_name: None,
                    profile: None,
                    usefulness: None,
                    run_id: None,
                }),
            );
        for index in 0..(MAX_VISIBLE_RUNS + 4) {
            snapshot.runs.push_back(RunProjection {
                run_id: format!("run-{index}"),
                kind: "k".into(),
                command: "c".into(),
                status: "s".into(),
                summary: "s".into(),
                job_id: None,
                log_dir: None,
                started_at: 0,
                completed_at: None,
                artifact_count: 0,
                pinned: false,
            });
        }
        for index in 0..(MAX_VISIBLE_JOBS + 2) {
            snapshot.jobs.push_back(JobProjection {
                job_id: format!("job-{index}"),
                workspace_id: "ws-1".into(),
                kind: "k".into(),
                state: "s".into(),
                summary: "s".into(),
                session_id: None,
                turn_id: None,
                active_attempt_id: None,
                error_class: None,
                updated_at: 0,
            });
        }
        let presented = present_snapshot(
            &snapshot_with_turn(snapshot, active),
            DriverState::Attached,
            None,
            None,
            true,
        );
        assert_eq!(presented.messages.len(), MAX_VISIBLE_MESSAGES);
        assert_eq!(presented.truncated_messages, 25);
        // Tail kept: newest message survives.
        assert_eq!(
            presented.messages.last().expect("last").message_id,
            format!("m-{}", MAX_VISIBLE_MESSAGES + 24)
        );
        assert_eq!(presented.tools.len(), MAX_VISIBLE_TOOLS);
        assert_eq!(presented.subagents.len(), MAX_VISIBLE_SUBAGENTS);
        assert_eq!(presented.agent_tree.len(), MAX_VISIBLE_SUBAGENTS);
        assert_eq!(presented.runs.len(), MAX_VISIBLE_RUNS);
        assert_eq!(presented.jobs.len(), MAX_VISIBLE_JOBS);
        // Tails kept: newest entries survive.
        assert_eq!(
            presented.runs.last().expect("run").run_id,
            format!("run-{}", MAX_VISIBLE_RUNS + 3)
        );
        assert_eq!(
            presented
                .subagents
                .last()
                .expect("subagent")
                .child_instance_id,
            format!("live-child-{}", MAX_VISIBLE_SUBAGENTS + 2)
        );
    }

    #[test]
    fn pending_items_deduplicate_across_turns_and_cap() {
        use codegg_protocol::projection::dto::{PermissionProjection, QuestionProjection};
        let mut active = turn("turn-active");
        active.pending_permissions.push_back(PermissionProjection {
            permission_id: "perm-1".into(),
            tool: "write".into(),
            path: Some("/work/file.txt".into()),
            status: PermissionStatus::Pending,
            created_at: 0,
            resolved_at: None,
        });
        active.pending_questions.push_back(QuestionProjection {
            question_id: "q-1".into(),
            header: Some("Choice".into()),
            prompt: "Pick one".into(),
            status: PermissionStatus::Pending,
            created_at: 0,
            resolved_at: None,
        });
        let mut older = turn("turn-old");
        // Same ids carried over: rendered once.
        older.pending_permissions.push_back(PermissionProjection {
            permission_id: "perm-1".into(),
            tool: "write".into(),
            path: Some("/work/file.txt".into()),
            status: PermissionStatus::Pending,
            created_at: 0,
            resolved_at: None,
        });
        let mut snapshot = base_snapshot();
        snapshot.active_turn = Some(active);
        snapshot.recent_turns.push_back(older);
        let presented = present_snapshot(&snapshot, DriverState::Attached, None, None, true);
        assert_eq!(presented.pending_permissions.len(), 1);
        assert_eq!(presented.pending_questions.len(), 1);
        assert_eq!(presented.pending_permissions[0].permission_id, "perm-1");
        assert_eq!(presented.pending_permissions[0].status, "pending");
        // Scope hint is display-only data, present but never authority.
        assert_eq!(
            presented.pending_permissions[0].scope_summary.as_deref(),
            Some("/work/file.txt")
        );
    }

    #[test]
    fn most_recent_completed_turn_supplies_messages_when_idle() {
        let mut completed = turn("turn-old");
        completed.status = TurnStatus::Completed;
        completed.messages.push_back(message(
            "m-done",
            MessageRole::Assistant,
            VisibilityClass::Public,
            "finished",
        ));
        let mut snapshot = base_snapshot();
        snapshot.recent_turns.push_back(completed);
        let presented = present_snapshot(&snapshot, DriverState::Attached, None, None, true);
        let turn = presented.turn.expect("most-recent turn");
        assert_eq!(turn.turn_id, "turn-old");
        assert_eq!(turn.status, "completed");
        assert_eq!(presented.messages.len(), 1);
        assert!(presented
            .recent_turns
            .iter()
            .any(|recent| recent.turn_id == "turn-old"));
    }

    #[test]
    fn resync_publishes_an_independent_replacement_view() {
        let mut before = turn("turn-1");
        before.messages.push_back(message(
            "m-before",
            MessageRole::Assistant,
            VisibilityClass::Public,
            "stale",
        ));
        let first = present_snapshot(
            &snapshot_with_turn(base_snapshot(), before),
            DriverState::Attached,
            None,
            Some(1),
            true,
        );
        let mut after = turn("turn-1");
        after.messages.push_back(message(
            "m-after",
            MessageRole::Assistant,
            VisibilityClass::Public,
            "fresh",
        ));
        let second = present_snapshot(
            &snapshot_with_turn(base_snapshot(), after),
            DriverState::Resyncing,
            Some("gap".into()),
            Some(2),
            true,
        );
        assert_eq!(second.state, "resyncing");
        assert_eq!(second.resync_reason.as_deref(), Some("gap"));
        // Atomic replace: the new view carries none of the old view's
        // message identity.
        assert!(second
            .messages
            .iter()
            .all(|message| message.message_id != "m-before"));
        assert!(first
            .messages
            .iter()
            .all(|message| message.message_id != "m-after"));
    }

    #[test]
    fn presentation_shape_is_camel_case_and_bounded() {
        let mut active = turn("turn-1");
        active.messages.push_back(message(
            "m-1",
            MessageRole::User,
            VisibilityClass::Public,
            "hello",
        ));
        let presented = present_snapshot(
            &snapshot_with_turn(base_snapshot(), active),
            DriverState::Attached,
            None,
            None,
            true,
        );
        let json = serde_json::to_value(&presented).expect("json");
        for key in [
            "sessionId",
            "projectId",
            "workspaceId",
            "state",
            "turn",
            "messages",
            "truncatedMessages",
            "transcript",
            "truncatedTranscriptMessages",
            "tools",
            "runs",
            "jobs",
            "subagents",
            "agentTree",
            "agentRuns",
            "toolPrograms",
            "worktrees",
            "runGroups",
            "convergences",
            "recentTurns",
            "pendingPermissions",
            "pendingQuestions",
            "controller",
            "artifactHandles",
            "cursor",
            "resyncReason",
        ] {
            assert!(json.get(key).is_some(), "missing bridge key {key}");
        }
        assert_eq!(json["messages"][0]["messageId"], "m-1");
    }

    #[test]
    fn visible_message_tail_is_capped_with_overflow_count() {
        let mut active = turn("turn-1");
        for index in 0..150 {
            active.messages.push_back(message(
                &format!("m-{index}"),
                MessageRole::Assistant,
                VisibilityClass::Public,
                &format!("text {index}"),
            ));
        }
        let presented = present_snapshot(
            &snapshot_with_turn(base_snapshot(), active),
            DriverState::Attached,
            None,
            Some(150),
            true,
        );
        // High-frequency text stays bounded: the newest 100 render.
        assert_eq!(presented.messages.len(), MAX_VISIBLE_MESSAGES);
        assert_eq!(
            presented.messages.first().expect("first").message_id,
            "m-50"
        );
        assert_eq!(presented.messages.last().expect("last").message_id, "m-149");
        assert_eq!(presented.truncated_messages, 50);
        assert_eq!(presented.turn.expect("turn").message_count, 150);
    }

    #[test]
    fn golden_snapshot_fixture_parity() {
        let mut active = turn("turn-1");
        active.messages.push_back(message(
            "m-u",
            MessageRole::User,
            VisibilityClass::Public,
            "do it",
        ));
        active.messages.push_back(message(
            "m-a",
            MessageRole::Assistant,
            VisibilityClass::Public,
            "on it",
        ));
        active.tools.push_back(tool(
            "t-1",
            ToolArgumentProjection::Summary {
                summary: "read Cargo.toml".into(),
            },
            ToolOutputProjection::Summary {
                summary: "128 lines".into(),
            },
        ));
        active.pending_permissions.push_back(PermissionProjection {
            permission_id: "perm-1".into(),
            tool: "write".into(),
            path: Some("/work/file.txt".into()),
            status: PermissionStatus::Pending,
            created_at: 0,
            resolved_at: None,
        });
        let presented = present_snapshot(
            &snapshot_with_turn(base_snapshot(), active),
            DriverState::Attached,
            None,
            Some(9),
            true,
        );
        let json = serde_json::to_value(&presented).expect("json");
        let expected = serde_json::json!({
            "sessionId": "session-1",
            "projectId": "proj-a",
            "workspaceId": "ws-1",
            "state": "attached",
            "selectedModel": null,
            "selectedAgent": null,
            "turn": {
                "turnId": "turn-1",
                "status": "active",
                "updatedAt": 7,
                "stopReason": null,
                "error": null,
                "messageCount": 2,
                "toolCount": 1,
                "pendingPermissions": 1,
                "pendingQuestions": 0,
                "inputTokens": 10,
                "outputTokens": 20,
                "cachedTokens": null,
                "contextLimit": null,
                "reasoningTokens": null
            },
            "messages": [
                {"messageId": "m-u", "role": "user", "text": "do it", "truncated": false},
                {"messageId": "m-a", "role": "assistant", "text": "on it", "truncated": false}
            ],
            "truncatedMessages": 0,
            "transcript": [
                {
                    "messageId": "m-u",
                    "turnId": "turn-1",
                    "role": "user",
                    "text": "do it",
                    "truncated": false,
                    "createdAt": 0
                },
                {
                    "messageId": "m-a",
                    "turnId": "turn-1",
                    "role": "assistant",
                    "text": "on it",
                    "truncated": false,
                    "createdAt": 0
                }
            ],
            "truncatedTranscriptMessages": 0,
            "tools": [
                {
                    "toolId": "t-1",
                    "toolName": "read",
                    "status": "completed",
                    "summary": "read Cargo.toml / 128 lines",
                    "hasArtifact": false,
                    "arguments": "read Cargo.toml",
                    "output": "128 lines",
                    "startedAt": null,
                    "completedAt": null,
                    "durationMs": null
                }
            ],
            "runs": [],
            "jobs": [],
            "subagents": [],
            "agentTree": [],
            "agentRuns": [],
            "toolPrograms": [],
            "worktrees": [],
            "runGroups": [],
            "convergences": [],
            "shellRuns": [],
            "toolContracts": [],
            "pluginCatalogue": [],
            "recentTurns": [],
            "todos": null,
            "pendingPermissions": [
                {
                    "permissionId": "perm-1",
                    "tool": "write",
                    "scopeSummary": "/work/file.txt",
                    "status": "pending"
                }
            ],
            "pendingQuestions": [],
            "controller": null,
            "artifactHandles": [],
            "cursor": {"eventSeq": 42, "driverCursorSeq": 9, "subscriptionKnown": true},
            "resyncReason": null
        });
        assert_eq!(json, expected);
    }

    #[test]
    fn resolved_permissions_and_questions_are_not_surfaced() {
        let mut active = turn("turn-1");
        active.pending_permissions.push_back(PermissionProjection {
            permission_id: "perm-pending".into(),
            tool: "write".into(),
            path: None,
            status: PermissionStatus::Pending,
            created_at: 0,
            resolved_at: None,
        });
        // The daemon rewrites a resolved permission's status in place and
        // leaves it in the deque
        // (crates/codegg-protocol/src/projection/reducer.rs:571-577).
        active.pending_permissions.push_back(PermissionProjection {
            permission_id: "perm-allowed".into(),
            tool: "write".into(),
            path: None,
            status: PermissionStatus::Allowed,
            created_at: 0,
            resolved_at: Some(9),
        });
        active.pending_questions.push_back(QuestionProjection {
            question_id: "q-pending".into(),
            header: None,
            prompt: "pick".into(),
            status: PermissionStatus::Pending,
            created_at: 0,
            resolved_at: None,
        });
        active.pending_questions.push_back(QuestionProjection {
            question_id: "q-denied".into(),
            header: None,
            prompt: "pick".into(),
            status: PermissionStatus::Denied,
            created_at: 0,
            resolved_at: Some(9),
        });
        let presented = present_snapshot(
            &snapshot_with_turn(base_snapshot(), active),
            DriverState::Attached,
            None,
            None,
            true,
        );
        let permission_ids: Vec<&str> = presented
            .pending_permissions
            .iter()
            .map(|permission| permission.permission_id.as_str())
            .collect();
        assert_eq!(permission_ids, vec!["perm-pending"]);
        let question_ids: Vec<&str> = presented
            .pending_questions
            .iter()
            .map(|question| question.question_id.as_str())
            .collect();
        assert_eq!(question_ids, vec!["q-pending"]);
        // The turn summary counts only the unresolved item, so the
        // renderer sees the same answerable set in both places.
        let turn = presented.turn.as_ref().expect("turn");
        assert_eq!(turn.pending_permissions, 1);
        assert_eq!(turn.pending_questions, 1);
    }

    #[test]
    fn subagents_come_from_the_session_liveness_authority() {
        let mut snapshot = base_snapshot();
        snapshot.subagents.push_back(subagent_node("live-child", 7));
        let mut active = turn("turn-1");
        // A legacy node has no durable child identity, so the daemon keeps
        // it in the turn-local compatibility view only
        // (crates/codegg-protocol/src/projection/reducer.rs:761-771).
        active.agent_tree.push_back(subagent_node("", 9));
        let presented = present_snapshot(
            &snapshot_with_turn(snapshot, active),
            DriverState::Attached,
            None,
            None,
            true,
        );
        assert_eq!(presented.subagents.len(), 1);
        assert_eq!(presented.subagents[0].child_instance_id, "live-child");
        assert_eq!(presented.subagents[0].task_id, 7);
        // The per-turn tree is carried alongside, so the legacy node is
        // not dropped.
        assert_eq!(presented.agent_tree.len(), 1);
        assert_eq!(presented.agent_tree[0].child_instance_id, "");
        assert_eq!(presented.agent_tree[0].task_id, 9);
    }

    #[test]
    fn non_public_content_never_crosses_the_bridge() {
        let mut active = turn("turn-1");
        active.messages.push_back(message(
            "m-public",
            MessageRole::Assistant,
            VisibilityClass::Public,
            "public text",
        ));
        active.messages.push_back(message(
            "m-internal",
            MessageRole::Assistant,
            VisibilityClass::Internal,
            "INTERNAL-TEXT",
        ));
        active.messages.push_back(message(
            "m-sensitive",
            MessageRole::Assistant,
            VisibilityClass::Sensitive,
            "SENSITIVE-TEXT",
        ));
        active
            .tools
            .push_back(tool_with_visibility("t-public", VisibilityClass::Public));
        active.tools.push_back(tool_with_visibility(
            "t-internal",
            VisibilityClass::Internal,
        ));
        active.tools.push_back(tool_with_visibility(
            "t-sensitive",
            VisibilityClass::Sensitive,
        ));
        let presented = present_snapshot(
            &snapshot_with_turn(base_snapshot(), active),
            DriverState::Attached,
            None,
            None,
            true,
        );
        let message_ids: Vec<&str> = presented
            .messages
            .iter()
            .map(|message| message.message_id.as_str())
            .collect();
        assert_eq!(message_ids, vec!["m-public"]);
        let tool_ids: Vec<&str> = presented
            .tools
            .iter()
            .map(|tool| tool.tool_id.as_str())
            .collect();
        assert_eq!(tool_ids, vec!["t-public"]);
        // The transcript is a second copy of message content, so it is
        // filtered by the same gate.
        let transcript_ids: Vec<&str> = presented
            .transcript
            .iter()
            .map(|message| message.message_id.as_str())
            .collect();
        assert_eq!(transcript_ids, vec!["m-public"]);
        let json = serde_json::to_value(&presented).expect("json").to_string();
        assert!(!json.contains("INTERNAL-TEXT"), "internal text leaked");
        assert!(!json.contains("SENSITIVE-TEXT"), "sensitive text leaked");
        assert!(!json.contains("t-internal"), "internal tool leaked");
        assert!(!json.contains("t-sensitive"), "sensitive tool leaked");
    }

    #[test]
    fn transcript_spans_turns_oldest_first_with_turn_tags() {
        let mut oldest = turn("turn-oldest");
        oldest.messages.push_back(message(
            "m-1",
            MessageRole::User,
            VisibilityClass::Public,
            "first",
        ));
        let mut middle = turn("turn-middle");
        middle.messages.push_back(message(
            "m-2",
            MessageRole::Assistant,
            VisibilityClass::Public,
            "second",
        ));
        let mut active = turn("turn-active");
        active.messages.push_back(message(
            "m-3",
            MessageRole::Assistant,
            VisibilityClass::Public,
            "third",
        ));
        let mut snapshot = base_snapshot();
        // The daemon keeps its recent turns newest-first.
        snapshot.recent_turns.push_back(middle);
        snapshot.recent_turns.push_back(oldest);
        snapshot.active_turn = Some(active);
        let presented = present_snapshot(&snapshot, DriverState::Attached, None, None, true);
        let tagged: Vec<(&str, &str)> = presented
            .transcript
            .iter()
            .map(|message| (message.turn_id.as_str(), message.message_id.as_str()))
            .collect();
        assert_eq!(
            tagged,
            vec![
                ("turn-oldest", "m-1"),
                ("turn-middle", "m-2"),
                ("turn-active", "m-3"),
            ]
        );
        assert_eq!(presented.truncated_transcript_messages, 0);
        // The flat message list stays the current turn's.
        assert_eq!(presented.messages.len(), 1);
        assert_eq!(presented.messages[0].message_id, "m-3");
    }

    #[test]
    fn turn_summary_carries_the_token_counters() {
        let mut active = turn("turn-1");
        active.input_tokens = Some(11);
        active.output_tokens = Some(22);
        active.cached_tokens = Some(33);
        active.context_limit = Some(4096);
        active.reasoning_tokens = Some(44);
        let presented = present_snapshot(
            &snapshot_with_turn(base_snapshot(), active),
            DriverState::Attached,
            None,
            None,
            true,
        );
        let turn = presented.turn.as_ref().expect("turn");
        assert_eq!(turn.input_tokens, Some(11));
        assert_eq!(turn.output_tokens, Some(22));
        assert_eq!(turn.cached_tokens, Some(33));
        assert_eq!(turn.context_limit, Some(4096));
        assert_eq!(turn.reasoning_tokens, Some(44));
    }

    #[test]
    fn background_activity_surfaces_are_carried() {
        let mut snapshot = base_snapshot();
        snapshot.agent_runs.push_back(AgentRunSummary {
            run_id: "run-1".into(),
            task_id: "task-1".into(),
            agent: "worker".into(),
            status: "running".into(),
            depth: 1,
            parent_run_id: Some("run-root".into()),
            worktree_id: Some("wt-1".into()),
            branch: Some("feature/x".into()),
            result_commit: None,
            group_id: Some("group-1".into()),
            control_status: "running".into(),
            attention_required: true,
            progress: Some("halfway".into()),
            terminal_summary: None,
            failure_class: None,
            updated_at: 11,
            ..AgentRunSummary::default()
        });
        snapshot.tool_programs.push_back(ToolProgramSummary {
            program_id: "prog-1".into(),
            job_id: "job-1".into(),
            state: "running".into(),
            phase: Some("calls".into()),
            language: "restricted_python".into(),
            parent_turn_id: Some("turn-1".into()),
            parent_agent_id: None,
            calls_completed: 2,
            child_jobs_running: 1,
            submitted_at: 5,
            started_at: Some(6),
            completed_at: None,
            failure_class: None,
            terminal_handle: None,
            last_progress: Some("2 calls".into()),
        });
        snapshot.worktrees.push_back(WorktreeSummaryProjection {
            worktree_id: "wt-1".into(),
            owner_run_id: Some("run-1".into()),
            branch: Some("feature/x".into()),
            base_commit: Some("abc123".into()),
            state: "ready".into(),
            health: "ok".into(),
            dirty: true,
            conflicted: false,
            retained_for_attention: false,
            updated_at: 12,
        });
        snapshot
            .run_groups
            .push_back(AgentRunGroupSummaryProjection {
                group_id: "group-1".into(),
                owner_run_id: "run-root".into(),
                owner_kind: "turn".into(),
                status: "running".into(),
                join_policy: "all".into(),
                member_run_ids: vec!["run-1".into(), "run-2".into()],
                successful: 1,
                failed: 0,
                active: 1,
                winner_run_id: None,
                cancel_remaining_on_satisfaction: true,
                updated_at: 13,
                ..AgentRunGroupSummaryProjection::default()
            });
        snapshot
            .convergences
            .push_back(ConvergenceSummaryProjection {
                convergence_id: "conv-1".into(),
                owner_summary: "owner".into(),
                status: "verifying".into(),
                cycle_ordinal: 1,
                max_cycles: 3,
                remaining_cycles: 2,
                producer_completed: 1,
                producer_failed: 0,
                producer_cancelled: 0,
                producer_active: 0,
                verifier_run_id: Some("run-2".into()),
                verdict_kind: Some("pass".into()),
                verdict_summary: Some("looks good".into()),
                awaiting_decision: false,
                terminal_reason_class: None,
                selected_run_id: None,
                last_finding_count: 0,
                ..ConvergenceSummaryProjection::default()
            });
        let presented = present_snapshot(&snapshot, DriverState::Attached, None, None, true);
        assert_eq!(presented.agent_runs.len(), 1);
        assert_eq!(presented.agent_runs[0].run_id, "run-1");
        assert_eq!(presented.agent_runs[0].group_id.as_deref(), Some("group-1"));
        assert!(presented.agent_runs[0].attention_required);
        assert_eq!(presented.tool_programs.len(), 1);
        assert_eq!(presented.tool_programs[0].program_id, "prog-1");
        assert_eq!(presented.tool_programs[0].calls_completed, 2);
        assert_eq!(presented.worktrees.len(), 1);
        assert_eq!(presented.worktrees[0].worktree_id, "wt-1");
        assert!(presented.worktrees[0].dirty);
        assert_eq!(presented.run_groups.len(), 1);
        assert_eq!(presented.run_groups[0].group_id, "group-1");
        // The member id list becomes a count; the renderer joins on
        // `agent_runs[].groupId`.
        assert_eq!(presented.run_groups[0].member_count, 2);
        assert_eq!(presented.convergences.len(), 1);
        assert_eq!(presented.convergences[0].convergence_id, "conv-1");
        assert_eq!(presented.convergences[0].remaining_cycles, 2);
    }

    /// M007-G (ADR-0057): the daemon's published plugin catalogue reaches
    /// the desktop's presentation view, so the desktop's three read-only
    /// plugin forms render what the daemon published.
    #[test]
    fn the_published_plugin_catalogue_reaches_the_presentation_view() {
        let mut snapshot = base_snapshot();
        snapshot
            .plugin_catalogue
            .push_back(PluginCatalogueEntryProjection {
                plugin_id: "plugin:alpha".into(),
                name: "alpha".into(),
                version: "1.2.3".into(),
                enabled: true,
                trust: "builtin".into(),
                runtime_kind: "builtin".into(),
                command_count: 2,
                hook_count: 1,
                diagnostic_count: 0,
                has_error: false,
                source_path: None,
            });

        let presented = present_snapshot(&snapshot, DriverState::Attached, None, None, true);

        assert_eq!(presented.plugin_catalogue.len(), 1);
        assert_eq!(presented.plugin_catalogue[0].plugin_id, "plugin:alpha");
        assert_eq!(presented.plugin_catalogue[0].version, "1.2.3");
        assert!(presented.plugin_catalogue[0].enabled);
        assert_eq!(presented.plugin_catalogue[0].command_count, 2);
        assert_eq!(presented.plugin_catalogue[0].hook_count, 1);
    }

    #[test]
    fn the_published_surfaces_reach_the_presentation_view() {
        let mut snapshot = base_snapshot();
        snapshot.primary_session.selected_model = Some("gpt-5".into());
        snapshot.primary_session.selected_agent = Some("worker".into());
        snapshot
            .shell_runs
            .push_back(shell_run("run-1", "cargo test", "running"));
        snapshot.tool_contracts.push_back(tool_contract(
            "read",
            "direct_only",
            "read_only",
            "idempotent",
        ));
        snapshot.tool_contracts.push_back(tool_contract(
            "bash",
            "direct_or_programmatic",
            "process_exec",
            "non_idempotent",
        ));

        let presented = present_snapshot(&snapshot, DriverState::Attached, None, None, true);

        assert_eq!(presented.selected_model.as_deref(), Some("gpt-5"));
        assert_eq!(presented.selected_agent.as_deref(), Some("worker"));
        assert_eq!(presented.shell_runs.len(), 1);
        assert_eq!(presented.shell_runs[0].run_id, "run-1");
        assert_eq!(presented.shell_runs[0].command, "cargo test");
        assert_eq!(presented.shell_runs[0].status, "running");
        assert_eq!(presented.shell_runs[0].origin, "bang");
        assert_eq!(presented.shell_runs[0].cwd, "/repo");
        assert_eq!(presented.tool_contracts.len(), 2);
        assert_eq!(presented.tool_contracts[0].name, "read");
        assert_eq!(presented.tool_contracts[0].caller_policy, "direct_only");
        assert_eq!(presented.tool_contracts[0].effect_class, "read_only");
        assert_eq!(presented.tool_contracts[0].idempotency, "idempotent");
        assert_eq!(presented.tool_contracts[1].name, "bash");
        assert_eq!(presented.tool_contracts[1].effect_class, "process_exec");
        assert_eq!(presented.tool_contracts[1].idempotency, "non_idempotent");
    }

    #[test]
    fn the_shell_run_window_keeps_the_newest_runs() {
        let mut snapshot = base_snapshot();
        for index in 0..(MAX_VISIBLE_SHELL_RUNS + 2) {
            snapshot
                .shell_runs
                .push_back(shell_run(&format!("run-{index}"), "echo hi", "exited"));
        }

        let presented = present_snapshot(&snapshot, DriverState::Attached, None, None, true);

        assert_eq!(presented.shell_runs.len(), MAX_VISIBLE_SHELL_RUNS);
        // The window is the newest runs, oldest first within it.
        assert_eq!(presented.shell_runs[0].run_id, "run-2");
        assert_eq!(
            presented.shell_runs[MAX_VISIBLE_SHELL_RUNS - 1].run_id,
            format!("run-{}", MAX_VISIBLE_SHELL_RUNS + 1)
        );
    }
}
