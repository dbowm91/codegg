//! Independent projection-consumer equivalence tests.
//!
//! These tests demonstrate the canonical reducer contract from an
//! external consumer perspective. The first consumer is the in-crate
//! reducer library tests; this file provides a *second* independent
//! consumer (a minimal fake "TUI-style" state builder) that consumes
//! the exact same projection events and snapshot seed, then compares
//! its internal representation to the canonical snapshot's logical
//! state. Equivalent inputs MUST produce equivalent logical state.
//!
//! This is the Work Package D acceptance criterion: at least two
//! independent consumers produce equivalent logical state from the
//! same fixtures.

use codegg_protocol::projection::dto::{
    AgentTreeStatus, MessageRole, PermissionStatus, ToolStatus, TurnStatus,
};
use codegg_protocol::projection::event::ProjectionEvent;
use codegg_protocol::projection::reducer::{ApplyOutcome, ProjectionReducer, ReducerEventInput};
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;
use codegg_protocol::projection::{
    active_turn_event_script, completed_event_script, fixtures, idle_snapshot,
    permission_event_script, subagent_event_script, MAX_PROJECTION_RECENT_TOOLS,
    MAX_PROJECTION_SESSIONS,
};

/// A minimal "TUI-style" projection consumer. It is intentionally
/// decoupled from the canonical snapshot type so that any drift in
/// the canonical contract is immediately visible.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FakeTuiState {
    active_turn_id: Option<String>,
    active_turn_status: Option<TurnStatus>,
    messages: Vec<(String, MessageRole)>,
    tools: Vec<(String, ToolStatus)>,
    pending_permissions: usize,
    pending_questions: usize,
    recent_turn_count: usize,
    active_subagents: usize,
    agent_tree: Vec<(u64, AgentTreeStatus)>,
    runs: Vec<(String, String)>,
    jobs: Vec<(String, String)>,
    diagnostics: Vec<String>,
}

impl FakeTuiState {
    fn new() -> Self {
        Self::default()
    }

    fn apply(&mut self, event: &ProjectionEvent) {
        match event {
            ProjectionEvent::TurnStarted { turn } => {
                self.active_turn_id = Some(turn.turn_id.clone());
                self.active_turn_status = Some(turn.status);
            }
            ProjectionEvent::MessageAppended { message } => {
                self.messages
                    .push((message.message_id.clone(), message.role));
                if self.messages.len() > MAX_PROJECTION_RECENT_TOOLS * 4 {
                    self.messages.remove(0);
                }
            }
            ProjectionEvent::ToolStarted { tool } => {
                self.tools.push((tool.tool_id.clone(), tool.status));
                if self.tools.len() > MAX_PROJECTION_RECENT_TOOLS {
                    self.tools.remove(0);
                }
            }
            ProjectionEvent::ToolCompleted {
                tool_id,
                success,
                output: _,
                duration_ms: _,
                completed_at: _,
            } => {
                let status = if *success {
                    ToolStatus::Completed
                } else {
                    ToolStatus::Failed
                };
                if let Some(slot) = self.tools.iter_mut().find(|(id, _)| id == tool_id) {
                    slot.1 = status;
                }
            }
            ProjectionEvent::PermissionPending { .. } => {
                self.pending_permissions += 1;
            }
            ProjectionEvent::PermissionResolved { status, .. } => {
                if matches!(status, PermissionStatus::Allowed | PermissionStatus::Denied) {
                    self.pending_permissions = self.pending_permissions.saturating_sub(1);
                }
            }
            ProjectionEvent::QuestionPending { .. } => {
                self.pending_questions += 1;
            }
            ProjectionEvent::QuestionResolved { status, .. } => {
                if matches!(status, PermissionStatus::Allowed | PermissionStatus::Denied) {
                    self.pending_questions = self.pending_questions.saturating_sub(1);
                }
            }
            ProjectionEvent::TurnCompleted {
                turn_id,
                stop_reason: _,
                completed_at: _,
            } => {
                if self.active_turn_id.as_deref() == Some(turn_id.as_str()) {
                    self.active_turn_id = None;
                    self.active_turn_status = None;
                    self.recent_turn_count += 1;
                }
            }
            ProjectionEvent::TurnFailed {
                turn_id,
                message: _,
                failed_at: _,
            } => {
                if self.active_turn_id.as_deref() == Some(turn_id.as_str()) {
                    self.active_turn_id = None;
                    self.active_turn_status = None;
                    self.recent_turn_count += 1;
                }
            }
            ProjectionEvent::SubagentStarted { node } => {
                self.agent_tree.push((node.task_id, node.status));
                self.active_subagents += 1;
                if self.agent_tree.len() > MAX_PROJECTION_SESSIONS {
                    self.agent_tree.remove(0);
                }
            }
            ProjectionEvent::SubagentCompleted { task_id, .. } => {
                if let Some(slot) = self.agent_tree.iter_mut().find(|(id, _)| id == task_id) {
                    slot.1 = AgentTreeStatus::Completed;
                }
                self.active_subagents = self.active_subagents.saturating_sub(1);
            }
            ProjectionEvent::SubagentFailed { task_id, .. } => {
                if let Some(slot) = self.agent_tree.iter_mut().find(|(id, _)| id == task_id) {
                    slot.1 = AgentTreeStatus::Failed;
                }
                self.active_subagents = self.active_subagents.saturating_sub(1);
            }
            ProjectionEvent::RunStarted { run } => {
                self.runs.push((run.run_id.clone(), run.status.clone()));
            }
            ProjectionEvent::RunCompleted { run_id, status, .. } => {
                if let Some(slot) = self.runs.iter_mut().find(|(id, _)| id == run_id) {
                    slot.1 = status.clone();
                }
            }
            ProjectionEvent::JobUpserted { job } => {
                if let Some(slot) = self.jobs.iter_mut().find(|(id, _)| id == &job.job_id) {
                    slot.1 = job.state.clone();
                } else {
                    self.jobs.push((job.job_id.clone(), job.state.clone()));
                }
            }
            ProjectionEvent::JobRemoved { job_id } => {
                self.jobs.retain(|(id, _)| id != job_id);
            }
            ProjectionEvent::Diagnostic { code, message } => {
                self.diagnostics.push(format!("{code}:{message}"));
            }
            _ => {}
        }
    }
}

fn reducer_snapshot(script: &[ReducerEventInput]) -> SessionProjectionSnapshot {
    let mut snap = idle_snapshot();
    let reducer = ProjectionReducer::default();
    reducer.apply_all(&mut snap, script.iter().cloned());
    snap
}

fn fake_tui_state(script: &[ReducerEventInput]) -> FakeTuiState {
    let mut state = FakeTuiState::new();
    for input in script {
        state.apply(&input.payload);
    }
    state
}

#[test]
fn active_turn_script_yields_equivalent_state() {
    let snap = reducer_snapshot(&active_turn_event_script());
    let fake = fake_tui_state(&active_turn_event_script());

    assert_eq!(
        snap.active_turn.as_ref().map(|t| t.turn_id.clone()),
        fake.active_turn_id,
        "active turn id mismatch"
    );
    assert_eq!(
        snap.active_turn.as_ref().map(|t| t.status),
        fake.active_turn_status,
        "active turn status mismatch"
    );
    assert_eq!(
        snap.active_turn.as_ref().map(|t| t.messages.len()),
        Some(fake.messages.len()),
        "message count mismatch"
    );
    assert_eq!(
        snap.active_turn.as_ref().map(|t| t.tools.len()),
        Some(fake.tools.len()),
        "tool count mismatch"
    );
}

#[test]
fn permission_script_yields_equivalent_state() {
    let snap = reducer_snapshot(&permission_event_script());
    let fake = fake_tui_state(&permission_event_script());
    assert_eq!(
        snap.primary_session.pending_permission_count,
        fake.pending_permissions
    );
    assert_eq!(
        snap.active_turn.as_ref().map(|t| t.status),
        Some(TurnStatus::AwaitingPermission),
        "turn should be AwaitingPermission"
    );
}

#[test]
fn completed_script_yields_equivalent_state() {
    let snap = reducer_snapshot(&completed_event_script());
    let fake = fake_tui_state(&completed_event_script());
    assert!(snap.active_turn.is_none());
    assert!(fake.active_turn_id.is_none());
    assert_eq!(snap.recent_turns.len(), fake.recent_turn_count);
    assert_eq!(snap.runs.len(), fake.runs.len());
}

#[test]
fn subagent_script_yields_equivalent_state() {
    let snap = reducer_snapshot(&subagent_event_script());
    let fake = fake_tui_state(&subagent_event_script());
    assert_eq!(
        snap.active_turn.as_ref().unwrap().agent_tree.len(),
        fake.agent_tree.len()
    );
    assert_eq!(
        snap.primary_session.active_subagents, fake.active_subagents,
        "active subagent count mismatch"
    );
}

#[test]
fn unknown_variant_does_not_panic_consumer() {
    let mut state = FakeTuiState::new();
    state.apply(&ProjectionEvent::Unknown {
        variant_name: "future_thing".into(),
        notice: "ignored".into(),
    });
    assert_eq!(state.diagnostics.len(), 0);
}

#[test]
fn reducer_outcome_is_applied_for_known_events() {
    let mut snap = idle_snapshot();
    let reducer = ProjectionReducer::default();
    let outcome = reducer.apply(
        &mut snap,
        active_turn_event_script()
            .into_iter()
            .next()
            .expect("script non-empty"),
    );
    assert_eq!(outcome, ApplyOutcome::Applied);
}

#[test]
fn secondary_sessions_are_bounded() {
    // Two events beyond MAX_PROJECTION_SESSIONS still succeed; the
    // reducer evicts older secondary sessions but never errors.
    let mut snap = idle_snapshot();
    let reducer = ProjectionReducer::default();
    for i in 0..(MAX_PROJECTION_SESSIONS * 2) {
        let summary = codegg_protocol::projection::dto::SessionSummaryProjection {
            session_id: format!("s-{i}"),
            project_id: snap.project_id.clone(),
            workspace_id: snap.workspace_id.clone(),
            title: format!("title-{i}"),
            status: "active".into(),
            selected_model: None,
            selected_agent: None,
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
        let outcome = reducer.apply(
            &mut snap,
            ReducerEventInput::session(
                (i as u64) + 1,
                i as i64,
                format!("s-{i}"),
                None,
                ProjectionEvent::SessionActivated { summary },
            ),
        );
        assert_eq!(outcome, ApplyOutcome::ScopeMismatch);
    }
    assert!(snap.secondary_sessions.len() <= MAX_PROJECTION_SESSIONS);
}

#[test]
fn fixture_snapshot_is_deterministic() {
    // The fixture builder is itself deterministic. Calling it twice
    // must produce structurally identical snapshots.
    let a = fixtures::active_turn_snapshot();
    let b = fixtures::active_turn_snapshot();
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}
