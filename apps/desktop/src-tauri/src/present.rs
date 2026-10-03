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
//! - raw tool arguments/output are never serialized: tools cross as
//!   summary lines plus artifact-handle markers only;
//! - filesystem paths never cross: run `log_dir` is dropped, permission
//!   `path` becomes a display-only scope summary that is never sent back
//!   as authority;
//! - every collection is tail-capped so the renderer observes bounded
//!   arrays; sequence/cursor fields are diagnostic and must never become
//!   projection authority in the renderer.

use codegg_client::{DriverSnapshotView, DriverState};
use codegg_protocol::projection::dto::{
    MessageProjection, MessageRole, PermissionProjection, PermissionStatus, QuestionProjection,
    RunProjection, ToolArgumentProjection, ToolOutputProjection, ToolProjection, TurnProjection,
    VisibilityClass,
};
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;
use serde::Serialize;

use super::bridge::{
    ArtifactHandleView, CursorDiagnosticView, JobSummaryView, MessageView, PendingPermissionView,
    PendingQuestionView, RunSummaryView, SessionPresentationView, SubagentSummaryView,
    ToolSummaryView, TurnSummaryView,
};

/// Renderer cap on visible messages from the current/most-recent turn.
/// Older messages are dropped from the view (count reported in
/// `truncated_messages`); the canonical snapshot retains its own bound.
pub const MAX_VISIBLE_MESSAGES: usize = 100;
/// Renderer cap on tool summaries from the current/most-recent turn.
pub const MAX_VISIBLE_TOOLS: usize = 20;
/// Renderer cap on run/job/subagent summaries.
pub const MAX_VISIBLE_RUNS: usize = 10;
pub const MAX_VISIBLE_JOBS: usize = 10;
pub const MAX_VISIBLE_SUBAGENTS: usize = 10;
/// Renderer cap on recent (completed) turn summaries.
pub const MAX_RECENT_TURNS: usize = 5;
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

/// `true` when a canonical message may cross to the renderer:
/// public visibility and a conversational role. Reasoning and system
/// content never render, even if a future reducer marks them public.
fn message_is_visible(message: &MessageProjection) -> bool {
    if message.visibility != VisibilityClass::Public {
        return false;
    }
    matches!(
        message.role,
        MessageRole::User | MessageRole::Assistant | MessageRole::Tool
    )
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
            turn.tools
                .iter()
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
            if seen_permissions.insert(permission.permission_id.clone()) {
                pending_permissions.push(permission_view(permission));
            }
        }
        for question in &turn.pending_questions {
            if pending_questions.len() >= MAX_PENDING_QUESTIONS {
                break;
            }
            if seen_questions.insert(question.question_id.clone()) {
                pending_questions.push(question_view(question));
            }
        }
    }

    SessionPresentationView {
        session_id: snapshot.primary_session_id.clone(),
        project_id: snapshot.project_id.clone(),
        workspace_id: snapshot.workspace_id.clone(),
        state: driver_state_name(state).into(),
        turn: current_turn.map(turn_summary),
        messages,
        truncated_messages,
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
        subagents: snapshot
            .active_turn
            .as_ref()
            .map(|turn| {
                turn.agent_tree
                    .iter()
                    .rev()
                    .take(MAX_VISIBLE_SUBAGENTS)
                    .rev()
                    .map(|node| SubagentSummaryView {
                        task_id: node.task_id,
                        agent: node.agent.clone(),
                        description: node.description.clone(),
                        status: enum_name(&node.status),
                        result_summary: node.result_summary.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        recent_turns: snapshot
            .recent_turns
            .iter()
            .take(MAX_RECENT_TURNS)
            .map(turn_summary)
            .collect(),
        pending_permissions,
        pending_questions,
        // WP E wires `SessionControlGet` into this field.
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
            turn: None,
            messages: Vec::new(),
            truncated_messages: 0,
            tools: Vec::new(),
            runs: Vec::new(),
            jobs: Vec::new(),
            subagents: Vec::new(),
            recent_turns: Vec::new(),
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

    fn turn(id: &str) -> TurnProjection {
        TurnProjection {
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
            input_tokens: Some(10),
            output_tokens: Some(20),
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
                    task_id: index as u64,
                    agent: "worker".into(),
                    description: "work".into(),
                    status: AgentTreeStatus::Running,
                    parent_task_id: None,
                    created_at: 0,
                    completed_at: None,
                    result_summary: None,
                }),
            );
        let mut snapshot = base_snapshot();
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
        assert_eq!(presented.runs.len(), MAX_VISIBLE_RUNS);
        assert_eq!(presented.jobs.len(), MAX_VISIBLE_JOBS);
        // Tails kept: newest entries survive.
        assert_eq!(
            presented.runs.last().expect("run").run_id,
            format!("run-{}", MAX_VISIBLE_RUNS + 3)
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
            "tools",
            "runs",
            "jobs",
            "subagents",
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
                "outputTokens": 20
            },
            "messages": [
                {"messageId": "m-u", "role": "user", "text": "do it", "truncated": false},
                {"messageId": "m-a", "role": "assistant", "text": "on it", "truncated": false}
            ],
            "truncatedMessages": 0,
            "tools": [
                {
                    "toolId": "t-1",
                    "toolName": "read",
                    "status": "completed",
                    "summary": "read Cargo.toml / 128 lines",
                    "hasArtifact": false
                }
            ],
            "runs": [],
            "jobs": [],
            "subagents": [],
            "recentTurns": [],
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
}
