//! Reference second-client equivalence tests for projection M4.
//!
//! These tests drive the [`ProjectionClientController`] as an
//! independent client (separate from any TUI code) and compare the
//! controller's snapshot to a second consumer built directly on top
//! of the canonical reducer. Both clients must produce equivalent
//! logical state from the same ordered event stream.
//!
//! The two consumers are:
//!
//! 1. The controller (`ProjectionClientController`), which composes the
//!    canonical reducer with subscription/cursor/ack lifecycle.
//! 2. A direct reducer-only consumer that mirrors the controller's
//!    apply semantics and is structurally independent of the
//!    controller's internal state.
//!
//! The two implementations are written from different sources of
//! truth: the controller uses its `apply_envelope` path; the direct
//! consumer uses `ProjectionReducer::apply_all`. Drift between the two
//! surfaces contract regressions.

use std::collections::BTreeMap;

use codegg_protocol::projection::caps::ProjectionCapabilities;
use codegg_protocol::projection::controller::{
    ControllerApplyOutcome, ProjectionClientController, ProjectionMode,
};
use codegg_protocol::projection::dto::{
    AgentTreeStatus, MessageRole, PermissionStatus, ToolStatus, TurnStatus,
};
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
use codegg_protocol::projection::reducer::{ProjectionReducer, ReducerEventInput};
use codegg_protocol::projection::replay::{
    ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind, ProjectionSubscriptionId,
};
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;
use codegg_protocol::projection::{
    active_turn_event_script, completed_event_script, fixture_reducer_config,
    permission_event_script, subagent_event_script, MAX_PROJECTION_MESSAGES,
    MAX_PROJECTION_PENDING_PERMISSIONS, MAX_PROJECTION_PENDING_QUESTIONS,
    MAX_PROJECTION_RECENT_TOOLS, MAX_PROJECTION_RUNS, MAX_PROJECTION_SUBAGENTS,
};

/// A digest shape used to compare logical state across consumers.
///
/// Fields are derived from a `SessionProjectionSnapshot`. The digest
/// is intentionally small and stable so it can be serialized, logged,
/// and asserted in tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProjectionDigest {
    active_turn_id: Option<String>,
    active_turn_status: Option<TurnStatus>,
    message_count: usize,
    tool_count: usize,
    pending_permissions: usize,
    pending_questions: usize,
    recent_turn_count: usize,
    active_subagents: usize,
    agent_tree: Vec<(u64, AgentTreeStatus)>,
    runs: Vec<(String, String)>,
    jobs: Vec<(String, String)>,
    diagnostics_count: usize,
    event_seq: u64,
}

impl ProjectionDigest {
    fn from_snapshot(snapshot: &SessionProjectionSnapshot) -> Self {
        let active_turn = snapshot.active_turn.as_ref();
        Self {
            active_turn_id: active_turn.map(|t| t.turn_id.clone()),
            active_turn_status: active_turn.map(|t| t.status),
            message_count: active_turn.map(|t| t.messages.len()).unwrap_or(0),
            tool_count: active_turn.map(|t| t.tools.len()).unwrap_or(0),
            pending_permissions: snapshot.primary_session.pending_permission_count as usize,
            pending_questions: snapshot.primary_session.pending_question_count as usize,
            recent_turn_count: snapshot.recent_turns.len(),
            active_subagents: snapshot.primary_session.active_subagents as usize,
            agent_tree: active_turn
                .map(|t| t.agent_tree.iter().map(|n| (n.task_id, n.status)).collect())
                .unwrap_or_default(),
            runs: snapshot
                .runs
                .iter()
                .map(|r| (r.run_id.clone(), r.status.clone()))
                .collect(),
            jobs: snapshot
                .jobs
                .iter()
                .map(|j| (j.job_id.clone(), j.state.clone()))
                .collect(),
            diagnostics_count: snapshot.diagnostics.len(),
            event_seq: snapshot.event_seq,
        }
    }

    fn from_controller(controller: &ProjectionClientController) -> Self {
        // The controller holds a snapshot per stream. For these tests we
        // install a single subscription, so we take whichever snapshot
        // exists.
        let mut digest = ProjectionDigest::default();
        let Some((_id, snap)) = controller.snapshots().iter().next() else {
            return digest;
        };
        digest.clone_from(&Self::from_snapshot(snap));
        digest
    }
}

fn reducer_only_digest(script: &[ReducerEventInput]) -> ProjectionDigest {
    let mut snap =
        SessionProjectionSnapshot::empty("session-fixture", "project-fixture", "workspace-fixture");
    let reducer = ProjectionReducer::new(fixture_reducer_config());
    reducer.apply_all(&mut snap, script.iter().cloned());
    ProjectionDigest::from_snapshot(&snap)
}

fn controller_digest(script: &[ReducerEventInput]) -> ProjectionDigest {
    let mut controller = ProjectionClientController::new(ProjectionCapabilities::current());
    controller.negotiate(Some(&ProjectionCapabilities::default()));
    let stream_id = ProjectionStreamId::new("session-session-fixture").expect("stream id");
    let descriptor = ProjectionStreamDescriptor {
        stream_id: stream_id.clone(),
        kind: ProjectionStreamKind::Session,
        project_id: "project-fixture".into(),
        workspace_id: Some("workspace-fixture".into()),
        session_id: Some("session-fixture".into()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    };
    let initial =
        SessionProjectionSnapshot::empty("session-fixture", "project-fixture", "workspace-fixture");
    let _ = controller.install_subscription(
        ProjectionSubscriptionId::new("ref-sub"),
        descriptor,
        initial,
    );
    let sub_id = ProjectionSubscriptionId::new("ref-sub");
    for input in script {
        let envelope = ProjectionEnvelope {
            protocol_version: input.protocol_version,
            event_seq: input.event_seq,
            timestamp_ms: input.timestamp_ms,
            session_id: input.session_id.clone(),
            turn_id: input.turn_id.clone(),
            scope: codegg_protocol::projection::event::ProjectionStreamScope::Session,
            payload: input.payload.clone(),
        };
        let outcome = controller.apply_envelope(&sub_id, envelope);
        assert!(
            matches!(
                outcome,
                ControllerApplyOutcome::Applied { .. }
                    | ControllerApplyOutcome::Reconciled { .. }
                    | ControllerApplyOutcome::Duplicate { .. }
            ),
            "controller apply failed: {outcome:?}"
        );
    }
    ProjectionDigest::from_controller(&controller)
}

fn assert_digests_equal(label: &str, a: &ProjectionDigest, b: &ProjectionDigest) {
    assert_eq!(
        a, b,
        "{label}: digest mismatch\nleft = {a:#?}\nright = {b:#?}"
    );
}

#[test]
fn controller_and_reducer_agree_on_active_turn_script() {
    let script = active_turn_event_script();
    let a = controller_digest(&script);
    let b = reducer_only_digest(&script);
    assert_digests_equal("active_turn", &a, &b);
    assert_eq!(a.active_turn_status, Some(TurnStatus::Active));
    assert!(a.message_count <= MAX_PROJECTION_MESSAGES);
    assert!(a.tool_count <= MAX_PROJECTION_RECENT_TOOLS);
}

#[test]
fn controller_and_reducer_agree_on_permission_script() {
    let script = permission_event_script();
    let a = controller_digest(&script);
    let b = reducer_only_digest(&script);
    assert_digests_equal("permission", &a, &b);
    assert!(a.pending_permissions <= MAX_PROJECTION_PENDING_PERMISSIONS);
    assert!(a.pending_questions <= MAX_PROJECTION_PENDING_QUESTIONS);
}

#[test]
fn controller_and_reducer_agree_on_completed_script() {
    let script = completed_event_script();
    let a = controller_digest(&script);
    let b = reducer_only_digest(&script);
    assert_digests_equal("completed", &a, &b);
    assert!(a.active_turn_id.is_none());
    assert!(a.recent_turn_count >= 1);
    assert!(a.runs.len() <= MAX_PROJECTION_RUNS);
}

#[test]
fn controller_and_reducer_agree_on_subagent_script() {
    let script = subagent_event_script();
    let a = controller_digest(&script);
    let b = reducer_only_digest(&script);
    assert_digests_equal("subagent", &a, &b);
    assert!(a.agent_tree.len() <= MAX_PROJECTION_SUBAGENTS);
}

#[test]
fn controller_unsubscribe_drops_snapshot() {
    let mut controller = ProjectionClientController::new(ProjectionCapabilities::current());
    controller.negotiate(Some(&ProjectionCapabilities::default()));
    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("session-session-fixture").expect("stream id"),
        kind: ProjectionStreamKind::Session,
        project_id: "project-fixture".into(),
        workspace_id: Some("workspace-fixture".into()),
        session_id: Some("session-fixture".into()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    };
    let _ = controller.install_subscription(
        ProjectionSubscriptionId::new("ref-sub"),
        descriptor,
        SessionProjectionSnapshot::empty("session-fixture", "project-fixture", "workspace-fixture"),
    );
    let sub_id = ProjectionSubscriptionId::new("ref-sub");
    let env = ProjectionEnvelope {
        protocol_version: 1,
        event_seq: 1,
        timestamp_ms: 0,
        session_id: Some("session-fixture".into()),
        turn_id: None,
        scope: codegg_protocol::projection::event::ProjectionStreamScope::Session,
        payload: ProjectionEvent::Diagnostic {
            code: "x".into(),
            message: "y".into(),
        },
    };
    let _ = controller.apply_envelope(&sub_id, env);
    assert!(!controller.snapshots().is_empty());
    let _ = controller.unsubscribe(&sub_id);
    assert!(controller.snapshots().is_empty());
}

#[test]
fn controller_ack_cadence_is_respected() {
    let mut controller = ProjectionClientController::new(ProjectionCapabilities::current());
    controller.negotiate(Some(&ProjectionCapabilities::default()));
    controller.set_ack_cadence(2);
    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("session-session-fixture").expect("stream id"),
        kind: ProjectionStreamKind::Session,
        project_id: "project-fixture".into(),
        workspace_id: Some("workspace-fixture".into()),
        session_id: Some("session-fixture".into()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    };
    let _ = controller.install_subscription(
        ProjectionSubscriptionId::new("ref-sub"),
        descriptor,
        SessionProjectionSnapshot::empty("session-fixture", "project-fixture", "workspace-fixture"),
    );
    let sub_id = ProjectionSubscriptionId::new("ref-sub");
    for seq in 1..=2 {
        let env = ProjectionEnvelope {
            protocol_version: 1,
            event_seq: seq,
            timestamp_ms: 0,
            session_id: Some("session-fixture".into()),
            turn_id: None,
            scope: codegg_protocol::projection::event::ProjectionStreamScope::Session,
            payload: ProjectionEvent::TurnStarted {
                turn: codegg_protocol::projection::dto::TurnProjection {
                    turn_id: format!("turn-{seq}"),
                    status: TurnStatus::Active,
                    started_at: 0,
                    updated_at: 0,
                    stop_reason: None,
                    error: None,
                    messages: Vec::new(),
                    tools: Vec::new(),
                    pending_permissions: Vec::new(),
                    pending_questions: Vec::new(),
                    agent_tree: Vec::new(),
                    subagent_count: 0,
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                },
            },
        };
        let _ = controller.apply_envelope(&sub_id, env);
    }
    let ack = controller.try_ack(&sub_id).expect("ack");
    assert_eq!(ack.cursor.event_seq, 2);
}

#[test]
fn controller_handles_resync_request() {
    let mut controller = ProjectionClientController::new(ProjectionCapabilities::current());
    controller.negotiate(Some(&ProjectionCapabilities::default()));
    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("session-session-fixture").expect("stream id"),
        kind: ProjectionStreamKind::Session,
        project_id: "project-fixture".into(),
        workspace_id: Some("workspace-fixture".into()),
        session_id: Some("session-fixture".into()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    };
    let _ = controller.install_subscription(
        ProjectionSubscriptionId::new("ref-sub"),
        descriptor,
        SessionProjectionSnapshot::empty("session-fixture", "project-fixture", "workspace-fixture"),
    );
    controller.request_resync(
        &ProjectionSubscriptionId::new("ref-sub"),
        codegg_protocol::projection::replay::ProjectionResyncReason::HistoryExpired,
    );
    let status = controller
        .subscription(&ProjectionSubscriptionId::new("ref-sub"))
        .expect("status");
    assert_eq!(
        status.state,
        codegg_protocol::projection::replay::ProjectionSubscriptionState::ResyncRequired
    );
}

#[test]
fn controller_reconnect_resets_subscriptions() {
    let mut controller = ProjectionClientController::new(ProjectionCapabilities::current());
    controller.negotiate(Some(&ProjectionCapabilities::default()));
    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("session-session-fixture").expect("stream id"),
        kind: ProjectionStreamKind::Session,
        project_id: "project-fixture".into(),
        workspace_id: Some("workspace-fixture".into()),
        session_id: Some("session-fixture".into()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    };
    let _ = controller.install_subscription(
        ProjectionSubscriptionId::new("ref-sub"),
        descriptor,
        SessionProjectionSnapshot::empty("session-fixture", "project-fixture", "workspace-fixture"),
    );
    assert_eq!(controller.subscription_count(), 1);
    controller.on_reconnect();
    assert_eq!(controller.subscription_count(), 0);
    assert!(controller.mode().is_unsupported());
}

#[test]
fn controller_rejects_unknown_subscription() {
    let mut controller = ProjectionClientController::new(ProjectionCapabilities::current());
    controller.negotiate(Some(&ProjectionCapabilities::default()));
    let env = ProjectionEnvelope {
        protocol_version: 1,
        event_seq: 1,
        timestamp_ms: 0,
        session_id: Some("session-fixture".into()),
        turn_id: None,
        scope: codegg_protocol::projection::event::ProjectionStreamScope::Session,
        payload: ProjectionEvent::Diagnostic {
            code: "x".into(),
            message: "y".into(),
        },
    };
    let outcome = controller.apply_envelope(&ProjectionSubscriptionId::new("never-installed"), env);
    assert!(matches!(outcome, ControllerApplyOutcome::Error(_)));
}

#[test]
fn controller_refuses_subscription_in_raw_compatibility_mode() {
    let mut controller = ProjectionClientController::new(ProjectionCapabilities::current());
    controller.enter_raw_compatibility("client is too old");
    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("session-session-fixture").expect("stream id"),
        kind: ProjectionStreamKind::Session,
        project_id: "project-fixture".into(),
        workspace_id: Some("workspace-fixture".into()),
        session_id: Some("session-fixture".into()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    };
    let outcome = controller.install_subscription(
        ProjectionSubscriptionId::new("ref-sub"),
        descriptor,
        SessionProjectionSnapshot::empty("session-fixture", "project-fixture", "workspace-fixture"),
    );
    assert!(matches!(
        outcome,
        codegg_protocol::projection::controller::ControllerSubscribeOutcome::Failed {
            reason:
                codegg_protocol::projection::controller::ControllerSubscribeFailure::UnsupportedMode
        }
    ));
}

#[test]
fn digest_shape_is_comparable_via_btreemap() {
    let script = active_turn_event_script();
    let a = controller_digest(&script);
    let b = reducer_only_digest(&script);
    let mut map: BTreeMap<&str, &ProjectionDigest> = BTreeMap::new();
    map.insert("controller", &a);
    map.insert("reducer", &b);
    assert_eq!(map.len(), 2);
    assert_eq!(map["controller"], map["reducer"]);
}

/// Ensure the digests expose the canonical logical fields the plan
/// promises are equivalent across clients.
#[test]
fn digest_carries_required_fields() {
    let script = active_turn_event_script();
    let digest = controller_digest(&script);
    // active_turn_status set
    assert!(digest.active_turn_status.is_some());
    // event_seq monotonic
    assert!(digest.event_seq > 0);
}

#[test]
fn controller_negotiates_to_projection_primary() {
    let mut controller = ProjectionClientController::new(ProjectionCapabilities::current());
    controller.negotiate(Some(&ProjectionCapabilities::default()));
    assert_eq!(controller.mode(), ProjectionMode::ProjectionPrimary);
    assert_eq!(controller.negotiated_version(), Some(1));
}

#[test]
fn controller_negotiates_unsupported_when_no_daemon_caps() {
    let mut controller = ProjectionClientController::new(ProjectionCapabilities::current());
    controller.negotiate(None);
    assert_eq!(controller.mode(), ProjectionMode::Unsupported);
}

#[allow(dead_code)]
fn _shape_check() {
    // Touch types so rustc flags unused imports if drift ever breaks
    // this file.
    let _: Option<MessageRole> = None;
    let _: Option<PermissionStatus> = None;
    let _: Option<ToolStatus> = None;
}
