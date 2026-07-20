//! Golden fixtures and a test builder for the projection contract.
//!
//! Fixtures are used by:
//!
//! * the reducer unit tests in [`crate::projection::reducer`],
//! * the equivalence tests in
//!   [`crate::projection::fixtures::tests`], and
//! * downstream consumers (TUI, web, observer) that need a stable
//!   starting snapshot.

use crate::projection::caps::PROJECTION_PROTOCOL_VERSION;
use crate::projection::dto::{
    AgentTreeNodeProjection, AgentTreeStatus, FileChangeProjection, JobProjection,
    MessageProjection, MessageRole, PermissionProjection, PermissionStatus,
    ProjectSummaryProjection, QuestionProjection, RunProjection, SessionSummaryProjection,
    ToolArgumentProjection, ToolOutputProjection, ToolProjection, ToolStatus, TurnProjection,
    TurnStatus, VisibilityClass, WorkspaceHealthProjection, WorkspaceSummaryProjection,
};
use crate::projection::event::{ProjectionEnvelope, ProjectionEvent};
use crate::projection::reducer::{ReducerConfig, ReducerEventInput};
use crate::projection::snapshot::SessionProjectionSnapshot;

/// Identifier used by every fixture below. Centralising it keeps
/// fixtures comparable across reducer versions.
pub const FIXTURE_SESSION_ID: &str = "session-fixture";
pub const FIXTURE_PROJECT_ID: &str = "project-fixture";
pub const FIXTURE_WORKSPACE_ID: &str = "workspace-fixture";

/// Build a deterministic idle snapshot. The returned snapshot is
/// normalised; applying more events on top of it produces a
/// deterministic result.
pub fn idle_snapshot() -> SessionProjectionSnapshot {
    SessionProjectionSnapshot {
        protocol_version: PROJECTION_PROTOCOL_VERSION,
        event_seq: 0,
        generated_at_ms: 0,
        primary_session_id: FIXTURE_SESSION_ID.into(),
        project_id: FIXTURE_PROJECT_ID.into(),
        workspace_id: FIXTURE_WORKSPACE_ID.into(),
        primary_session: SessionSummaryProjection {
            session_id: FIXTURE_SESSION_ID.into(),
            project_id: FIXTURE_PROJECT_ID.into(),
            workspace_id: FIXTURE_WORKSPACE_ID.into(),
            title: "fixture".into(),
            status: "idle".into(),
            selected_model: Some("test/model".into()),
            selected_agent: Some("fixture-agent".into()),
            has_active_turn: false,
            pending_permission_count: 0,
            pending_question_count: 0,
            input_tokens: Some(0),
            output_tokens: Some(0),
            active_subagents: 0,
            time_created_at: Some(0),
            time_updated_at: Some(0),
            recent_summary: None,
        },
        secondary_sessions: Vec::new(),
        workspace: WorkspaceSummaryProjection {
            workspace_id: FIXTURE_WORKSPACE_ID.into(),
            canonical_root: "/fixture".into(),
            display_name: "fixture".into(),
            created_at: 0,
            last_opened_at: 0,
            archived_at: None,
            active_sessions: 1,
            services_active: true,
            active_leases: 0,
            config_revision: 1,
            health: WorkspaceHealthProjection::default(),
        },
        active_turn: None,
        recent_turns: Vec::new(),
        runs: Vec::new(),
        jobs: Vec::new(),
        diagnostics: Vec::new(),
    }
}

/// Build a deterministic snapshot with one active turn and a single
/// tool call. This is the "active turn" fixture used by the schema
/// examples in the plan.
pub fn active_turn_snapshot() -> SessionProjectionSnapshot {
    let mut snap = idle_snapshot();
    snap.active_turn = Some(TurnProjection {
        turn_id: "turn-1".into(),
        status: TurnStatus::Active,
        started_at: 1,
        updated_at: 5,
        stop_reason: None,
        error: None,
        messages: vec![MessageProjection {
            message_id: "msg-1".into(),
            parent_turn_id: "turn-1".into(),
            role: MessageRole::Assistant,
            text: "hello".into(),
            tool_call_id: None,
            visibility: VisibilityClass::Public,
            created_at: 1,
            truncated: false,
        }],
        tools: vec![ToolProjection {
            tool_id: "tool-1".into(),
            tool_name: "Bash".into(),
            status: ToolStatus::Started,
            arguments: ToolArgumentProjection::Summary {
                summary: "ls".into(),
            },
            output: ToolOutputProjection::Pending,
            visibility: VisibilityClass::Public,
            started_at: Some(1),
            completed_at: None,
            duration_ms: None,
        }],
        pending_permissions: Vec::new(),
        pending_questions: Vec::new(),
        agent_tree: Vec::new(),
        subagent_count: 0,
        input_tokens: Some(0),
        output_tokens: Some(0),
    });
    snap.primary_session.has_active_turn = true;
    snap
}

/// Snapshot of a session with a single completed turn, a run record,
/// and a job. Used by schema examples for the "completed session"
/// case.
pub fn completed_snapshot() -> SessionProjectionSnapshot {
    let mut snap = idle_snapshot();
    let turn = TurnProjection {
        turn_id: "turn-1".into(),
        status: TurnStatus::Completed,
        started_at: 1,
        updated_at: 5,
        stop_reason: Some("ok".into()),
        error: None,
        messages: vec![MessageProjection {
            message_id: "msg-1".into(),
            parent_turn_id: "turn-1".into(),
            role: MessageRole::Assistant,
            text: "done".into(),
            tool_call_id: None,
            visibility: VisibilityClass::Public,
            created_at: 1,
            truncated: false,
        }],
        tools: Vec::new(),
        pending_permissions: Vec::new(),
        pending_questions: Vec::new(),
        agent_tree: Vec::new(),
        subagent_count: 0,
        input_tokens: Some(10),
        output_tokens: Some(20),
    };
    snap.recent_turns.push(turn);
    snap.runs.push(RunProjection {
        run_id: "run-1".into(),
        kind: "test".into(),
        command: "cargo test".into(),
        status: "completed".into(),
        summary: "ok".into(),
        job_id: Some("job-1".into()),
        log_dir: None,
        started_at: 1,
        completed_at: Some(5),
        artifact_count: 0,
        pinned: false,
    });
    snap.jobs.push(JobProjection {
        job_id: "job-1".into(),
        workspace_id: FIXTURE_WORKSPACE_ID.into(),
        kind: "test".into(),
        state: "completed".into(),
        summary: "ok".into(),
        session_id: Some(FIXTURE_SESSION_ID.into()),
        turn_id: Some("turn-1".into()),
        active_attempt_id: None,
        error_class: None,
        updated_at: 5,
    });
    snap
}

/// Pending permission fixture used by the schema example for the
/// "permission" case.
pub fn permission_pending_snapshot() -> SessionProjectionSnapshot {
    let mut snap = idle_snapshot();
    snap.active_turn = Some(TurnProjection {
        turn_id: "turn-1".into(),
        status: TurnStatus::AwaitingPermission,
        started_at: 1,
        updated_at: 5,
        stop_reason: None,
        error: None,
        messages: Vec::new(),
        tools: Vec::new(),
        pending_permissions: vec![PermissionProjection {
            permission_id: "perm-1".into(),
            tool: "Bash".into(),
            path: None,
            status: PermissionStatus::Pending,
            created_at: 5,
            resolved_at: None,
        }],
        pending_questions: Vec::new(),
        agent_tree: Vec::new(),
        subagent_count: 0,
        input_tokens: Some(0),
        output_tokens: Some(0),
    });
    snap.primary_session.has_active_turn = true;
    snap.primary_session.pending_permission_count = 1;
    snap
}

/// Project summary fixture used by the schema example.
pub fn project_summary_fixture() -> ProjectSummaryProjection {
    ProjectSummaryProjection {
        project_id: FIXTURE_PROJECT_ID.into(),
        display_name: "Fixture Project".into(),
        lifecycle: "active".into(),
        description: Some("a fixture project".into()),
        tags: vec!["fixture".into(), "test".into()],
        time_last_opened_at: Some(1),
        registration_source: "fixture".into(),
        archived_at: None,
        created_at: 0,
        updated_at: 1,
    }
}

/// Build a list of canonical fixture events that drives an idle
/// snapshot to the active-turn state. Independent consumers are
/// expected to apply the same events and observe equivalent logical
/// state.
pub fn active_turn_event_script() -> Vec<ReducerEventInput> {
    vec![
        ReducerEventInput::session(
            1,
            1,
            FIXTURE_SESSION_ID,
            Some("turn-1".into()),
            ProjectionEvent::TurnStarted {
                turn: TurnProjection {
                    turn_id: "turn-1".into(),
                    status: TurnStatus::Active,
                    started_at: 1,
                    updated_at: 1,
                    stop_reason: None,
                    error: None,
                    messages: Vec::new(),
                    tools: Vec::new(),
                    pending_permissions: Vec::new(),
                    pending_questions: Vec::new(),
                    agent_tree: Vec::new(),
                    subagent_count: 0,
                    input_tokens: None,
                    output_tokens: None,
                },
            },
        ),
        ReducerEventInput::session(
            2,
            2,
            FIXTURE_SESSION_ID,
            Some("turn-1".into()),
            ProjectionEvent::MessageAppended {
                message: MessageProjection {
                    message_id: "msg-1".into(),
                    parent_turn_id: "turn-1".into(),
                    role: MessageRole::Assistant,
                    text: "hello".into(),
                    tool_call_id: None,
                    visibility: VisibilityClass::Public,
                    created_at: 2,
                    truncated: false,
                },
            },
        ),
        ReducerEventInput::session(
            3,
            3,
            FIXTURE_SESSION_ID,
            Some("turn-1".into()),
            ProjectionEvent::tool_started(
                "tool-1",
                "Bash",
                ToolArgumentProjection::Summary {
                    summary: "ls".into(),
                },
                3,
            ),
        ),
    ]
}

/// Build the canonical event script that drives the snapshot through
/// the "permission" example.
pub fn permission_event_script() -> Vec<ReducerEventInput> {
    let mut events = active_turn_event_script();
    events.push(ReducerEventInput::session(
        4,
        4,
        FIXTURE_SESSION_ID,
        Some("turn-1".into()),
        ProjectionEvent::PermissionPending {
            permission: PermissionProjection {
                permission_id: "perm-1".into(),
                tool: "Bash".into(),
                path: None,
                status: PermissionStatus::Pending,
                created_at: 4,
                resolved_at: None,
            },
        },
    ));
    events
}

/// Build the canonical event script for the "completed session"
/// example.
pub fn completed_event_script() -> Vec<ReducerEventInput> {
    let mut events = active_turn_event_script();
    events.push(ReducerEventInput::session(
        4,
        4,
        FIXTURE_SESSION_ID,
        Some("turn-1".into()),
        ProjectionEvent::RunStarted {
            run: RunProjection {
                run_id: "run-1".into(),
                kind: "test".into(),
                command: "cargo test".into(),
                status: "running".into(),
                summary: String::new(),
                job_id: Some("job-1".into()),
                log_dir: None,
                started_at: 4,
                completed_at: None,
                artifact_count: 0,
                pinned: false,
            },
        },
    ));
    events.push(ReducerEventInput::session(
        5,
        5,
        FIXTURE_SESSION_ID,
        Some("turn-1".into()),
        ProjectionEvent::RunCompleted {
            run_id: "run-1".into(),
            status: "completed".into(),
            summary: "ok".into(),
            completed_at: 5,
        },
    ));
    events.push(ReducerEventInput::session(
        6,
        6,
        FIXTURE_SESSION_ID,
        Some("turn-1".into()),
        ProjectionEvent::TurnCompleted {
            turn_id: "turn-1".into(),
            stop_reason: "ok".into(),
            completed_at: 6,
        },
    ));
    events
}

/// Build an event script that exercises the agent-tree placeholder
/// path. Used to prove the projection surfaces stable task ids.
pub fn subagent_event_script() -> Vec<ReducerEventInput> {
    let mut events = active_turn_event_script();
    events.push(ReducerEventInput::session(
        4,
        4,
        FIXTURE_SESSION_ID,
        Some("turn-1".into()),
        ProjectionEvent::SubagentStarted {
            node: AgentTreeNodeProjection {
                task_id: 7,
                agent: "explorer".into(),
                description: "exploring the tree".into(),
                status: AgentTreeStatus::Running,
                parent_task_id: None,
                created_at: 4,
                completed_at: None,
                result_summary: None,
            },
        },
    ));
    events.push(ReducerEventInput::session(
        5,
        5,
        FIXTURE_SESSION_ID,
        Some("turn-1".into()),
        ProjectionEvent::SubagentCompleted {
            task_id: 7,
            result_summary: "found 3 nodes".into(),
            completed_at: 5,
        },
    ));
    events
}

/// Build an event script that exercises the file-change summary
/// path. The reducer coalesces file-change events into the session
/// summary.
pub fn file_change_event_script() -> Vec<ReducerEventInput> {
    vec![ReducerEventInput::session(
        1,
        1,
        FIXTURE_SESSION_ID,
        None,
        ProjectionEvent::FileChanged {
            change: FileChangeProjection::Modified {
                path: "src/main.rs".into(),
            },
            at: 1,
        },
    )]
}

/// Build an event script that exercises the job upsert path.
pub fn job_event_script() -> Vec<ReducerEventInput> {
    vec![
        ReducerEventInput::session(
            1,
            1,
            FIXTURE_SESSION_ID,
            None,
            ProjectionEvent::JobUpserted {
                job: JobProjection {
                    job_id: "job-1".into(),
                    workspace_id: FIXTURE_WORKSPACE_ID.into(),
                    kind: "test".into(),
                    state: "created".into(),
                    summary: String::new(),
                    session_id: Some(FIXTURE_SESSION_ID.into()),
                    turn_id: None,
                    active_attempt_id: None,
                    error_class: None,
                    updated_at: 1,
                },
            },
        ),
        ReducerEventInput::session(
            2,
            2,
            FIXTURE_SESSION_ID,
            None,
            ProjectionEvent::JobUpserted {
                job: JobProjection {
                    job_id: "job-1".into(),
                    workspace_id: FIXTURE_WORKSPACE_ID.into(),
                    kind: "test".into(),
                    state: "completed".into(),
                    summary: "ok".into(),
                    session_id: Some(FIXTURE_SESSION_ID.into()),
                    turn_id: None,
                    active_attempt_id: Some("attempt-1".into()),
                    error_class: None,
                    updated_at: 2,
                },
            },
        ),
    ]
}

/// Question event script used to prove the projection surfaces
/// pending questions.
pub fn question_event_script() -> Vec<ReducerEventInput> {
    let mut events = active_turn_event_script();
    events.push(ReducerEventInput::session(
        4,
        4,
        FIXTURE_SESSION_ID,
        Some("turn-1".into()),
        ProjectionEvent::QuestionPending {
            question: QuestionProjection {
                question_id: "q-1".into(),
                header: Some("Choose".into()),
                prompt: "Which path?".into(),
                status: PermissionStatus::Pending,
                created_at: 4,
                resolved_at: None,
            },
        },
    ));
    events
}

/// Default reducer configuration used by fixture-driven tests.
pub fn fixture_reducer_config() -> ReducerConfig {
    ReducerConfig::default()
}

/// Wrap a [`ProjectionEvent`] in a [`ProjectionEnvelope`] using the
/// fixture session identity. Convenient for tests that bypass the
/// reducer and inspect envelopes directly.
pub fn envelope_from_event(
    event_seq: u64,
    timestamp_ms: i64,
    payload: ProjectionEvent,
) -> ProjectionEnvelope {
    ProjectionEnvelope::session_event(event_seq, timestamp_ms, FIXTURE_SESSION_ID, None, payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::reducer::ProjectionReducer;

    fn run_script(script: &[ReducerEventInput]) -> SessionProjectionSnapshot {
        let mut snap = idle_snapshot();
        let reducer = ProjectionReducer::new(fixture_reducer_config());
        reducer.apply_all(&mut snap, script.iter().cloned());
        snap
    }

    #[test]
    fn active_turn_script_produces_active_turn_state() {
        let snap = run_script(&active_turn_event_script());
        assert!(snap.active_turn.is_some());
        assert!(snap.primary_session.has_active_turn);
        assert_eq!(snap.active_turn.as_ref().unwrap().messages.len(), 1);
        assert_eq!(snap.active_turn.as_ref().unwrap().tools.len(), 1);
    }

    #[test]
    fn permission_script_sets_pending_status() {
        let snap = run_script(&permission_event_script());
        let turn = snap.active_turn.expect("active turn");
        assert_eq!(turn.status, TurnStatus::AwaitingPermission);
        assert_eq!(turn.pending_permissions.len(), 1);
        assert_eq!(snap.primary_session.pending_permission_count, 1);
    }

    #[test]
    fn completed_script_closes_turn_and_persists_run() {
        let snap = run_script(&completed_event_script());
        assert!(snap.active_turn.is_none());
        assert_eq!(snap.recent_turns.len(), 1);
        assert_eq!(snap.runs.len(), 1);
        assert_eq!(snap.runs[0].status, "completed");
    }

    #[test]
    fn subagent_script_records_placeholder_node() {
        let snap = run_script(&subagent_event_script());
        let turn = snap.active_turn.expect("active turn");
        assert_eq!(turn.agent_tree.len(), 1);
        assert_eq!(turn.agent_tree[0].task_id, 7);
        assert_eq!(turn.agent_tree[0].status, AgentTreeStatus::Completed);
    }

    #[test]
    fn file_change_script_coalesces_into_recent_summary() {
        let snap = run_script(&file_change_event_script());
        let summary = snap.primary_session.recent_summary.expect("summary");
        assert!(summary.contains("src/main.rs"));
    }

    #[test]
    fn job_script_upserts_job_with_latest_state() {
        let snap = run_script(&job_event_script());
        assert_eq!(snap.jobs.len(), 1);
        assert_eq!(snap.jobs[0].state, "completed");
        assert_eq!(snap.jobs[0].active_attempt_id.as_deref(), Some("attempt-1"));
    }

    #[test]
    fn question_script_records_pending_question() {
        let snap = run_script(&question_event_script());
        let turn = snap.active_turn.expect("active turn");
        assert_eq!(turn.pending_questions.len(), 1);
        assert_eq!(turn.status, TurnStatus::AwaitingQuestion);
    }

    #[test]
    fn two_consumers_agree_on_active_turn_state() {
        let snap_a = run_script(&active_turn_event_script());
        let snap_b = run_script(&active_turn_event_script());
        assert_eq!(
            serde_json::to_string(&snap_a).unwrap(),
            serde_json::to_string(&snap_b).unwrap()
        );
    }
}
