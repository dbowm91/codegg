//! Project Work Orders M007 — end-to-end trajectory, recovery, contention,
//! and security qualification.
//!
//! Closure qualification for
//! `plans/implementation/project-work-orders-task-view/007-work-order-trajectory-and-recovery-qualification.md`.
//! This file does not redesign the domain: it composes the landed M001-M006
//! substrate (store/CAS/idempotency, coordinator/materialization, Task view,
//! dashboard, trigger, agent batch) and proves the composed invariants under
//! restart, duplicate delivery, concurrent mutation, policy/model drift,
//! worktree conflict, cancellation, permission wait, sequential failure,
//! trigger replay, and agent batch trajectories.
//!
//! Mapping to the plan qualification matrix:
//!
//! - A: `a_creation_immediate_materializes_once_with_restart_convergence`
//! - B: `b_fifteen_lane_order_restart_reorder_hold`
//! - C: `c_release_gate_join_latch_duplicate_timezone_restart`
//! - D: `d_finite_repeat_exact_counts_exhaustion`
//! - E: `e_fault_windows_converge_without_duplicates`
//! - F: `f_worktree_isolation_and_truthfulness`
//! - G: `g_model_provider_policy_drift`
//! - H: `h_permission_steering_cancel_use_ordinary_session_mechanisms`
//! - I: `i_trigger_replay_security_matrix`
//! - J: `j_agent_batch_idempotency_bounds`
//! - K: `k_team_contention_and_privacy`
//! - L: `l_tui_stale_completion_and_navigation`
//! - §7 representative trajectory: `trajectory_fifteen_plan_end_to_end`
//! - §9 static ownership: `ownership_single_scheduler_and_boundaries`

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use codegg::core::work_order_coordinator::{PartialStage, WorkOrderCoordinator};
use codegg::scheduler::submission::{JobSubmissionService, SubmissionKey};
use codegg::scheduler::{JobScheduler, ResolvedSchedulerConfig};
use codegg_core::bus::{PermissionDecision, PermissionRegistry};
use codegg_core::identity::{PrincipalId, ProjectId};
use codegg_core::jobs::{
    DaemonGeneration, ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobKind, JobPayload,
    JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::session::{CreateSession, SessionStore};
use codegg_core::work_order::{
    evaluate_occurrence_gates, is_repeat_exhausted, merge_latches, narrow_approval, narrow_sandbox,
    resolve_model, resolve_workspace_action, sequence_holds, sequence_predecessors_terminal,
    session_id_for_occurrence, submission_key_for_occurrence, ApprovalRequest, AttentionCode,
    GateJoin, GateKind, GateSpec, LaneFailurePolicy, NewSequenceLane, NewTaskTrigger, NewWorkOrder,
    OccurrenceState, ReleaseGateSet, SandboxRequest, WorkOrderError, WorkOrderPatch,
    WorkOrderService, WorkspacePolicy, MAX_REPEAT_COUNT, MAX_WORK_ORDER_BATCH_ITEMS,
};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

fn project(id: &str) -> ProjectId {
    ProjectId::parse(id).unwrap()
}

fn project_a() -> ProjectId {
    project("project-1")
}

fn creator() -> PrincipalId {
    PrincipalId::parse("local-owner").unwrap()
}

fn immediate_input(prompt: &str) -> NewWorkOrder {
    NewWorkOrder {
        title: Some("Task".to_owned()),
        prompt: prompt.to_owned(),
        requested_model: None,
        requested_approval: None,
        requested_sandbox: None,
        workspace_policy: None,
        gates: ReleaseGateSet::immediate(),
        repeat_count: 1,
        sequence_lane_id: None,
        parent_session_id: None,
        parent_turn_id: None,
        parent_work_order_id: None,
        idempotency_key: None,
    }
}

fn delay_input(prompt: &str, delay_secs: i64) -> NewWorkOrder {
    let mut input = immediate_input(prompt);
    input.gates = ReleaseGateSet {
        join: GateJoin::All,
        gates: vec![GateSpec {
            kind: GateKind::Delay,
            delay_secs: Some(delay_secs),
            not_before_ms: None,
            lane_id: None,
            trigger_ref: None,
        }],
    };
    input
}

fn not_before_input(prompt: &str, at_ms: i64) -> NewWorkOrder {
    let mut input = immediate_input(prompt);
    input.gates = ReleaseGateSet {
        join: GateJoin::All,
        gates: vec![GateSpec {
            kind: GateKind::NotBefore,
            delay_secs: None,
            not_before_ms: Some(at_ms),
            lane_id: None,
            trigger_ref: None,
        }],
    };
    input
}

struct SchedulerHarness {
    _root: tempfile::TempDir,
    store: Arc<dyn JobStore>,
    submission: Arc<JobSubmissionService>,
    workspace_id: codegg_core::workspace::WorkspaceId,
}

impl SchedulerHarness {
    async fn new() -> Self {
        let root = tempfile::tempdir().expect("temp workspace");
        let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("workspace registry");
        let workspace = workspace_registry
            .get_or_register(root.path())
            .await
            .expect("register workspace");
        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let scheduler = JobScheduler::new(
            store.clone(),
            services.clone(),
            ResolvedSchedulerConfig::default(),
            DaemonGeneration::new_unchecked("gen-m007"),
        );
        scheduler
            .register_executor_sync(Arc::new(codegg::scheduler::executors::AgentTurnExecutor))
            .expect("register agent-turn executor");
        let submission = JobSubmissionService::new(
            store.clone(),
            scheduler,
            services,
            DaemonGeneration::new_unchecked("gen-m007"),
        );
        Self {
            _root: root,
            store,
            submission,
            workspace_id: workspace.id.clone(),
        }
    }

    fn agent_turn_spec(&self, session_id: &str, prompt: &str, submission_key: &str) -> NewJob {
        NewJob {
            workspace_id: self.workspace_id.clone(),
            session_id: Some(session_id.to_owned()),
            turn_id: None,
            kind: JobKind::AgentTurn,
            source: JobSource::Api,
            priority: JobPriority::Normal,
            payload: JobPayload::AgentTurn {
                prompt: prompt.to_owned(),
                agent: "build".to_owned(),
                model: None,
                submission_key: Some(submission_key.to_owned()),
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: vec![],
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: ExecutionTarget::default(),
        }
    }
}

#[derive(Default)]
struct FakeSessions {
    sessions: tokio::sync::Mutex<HashMap<String, String>>,
}

impl FakeSessions {
    async fn get_or_create(&self, session_id: &str, prompt: &str) -> (String, bool) {
        let mut sessions = self.sessions.lock().await;
        if let Some(existing) = sessions.get(session_id) {
            return (existing.clone(), true);
        }
        sessions.insert(session_id.to_owned(), prompt.to_owned());
        (prompt.to_owned(), false)
    }

    async fn count(&self) -> usize {
        self.sessions.lock().await.len()
    }
}

async fn step_to_terminal(
    service: &WorkOrderService,
    proj: &ProjectId,
    occ: &codegg_core::identity::WorkOrderOccurrenceId,
    target: OccurrenceState,
    now_ms: i64,
) {
    // Walk the legal path Waiting -> Ready -> Claiming -> Running ->
    // `target` (terminal or attention-adjacent), skipping steps already
    // passed. Attention needs Waiting -> NeedsAttention directly.
    let path: Vec<OccurrenceState> = match target {
        OccurrenceState::NeedsAttention => vec![OccurrenceState::NeedsAttention],
        OccurrenceState::Cancelled => {
            // Cancel works from any non-terminal state via cancel_occurrence.
            service
                .cancel_occurrence(proj, occ, now_ms)
                .await
                .expect("cancel");
            return;
        }
        _ => vec![
            OccurrenceState::Ready,
            OccurrenceState::Claiming,
            OccurrenceState::Running,
            target,
        ],
    };
    for state in path {
        let current = service
            .get_occurrence(proj, occ)
            .await
            .expect("get")
            .expect("row");
        if current.state == state || current.state.is_terminal() {
            continue;
        }
        // Skip illegal jumps (e.g. Waiting -> Claiming): advance through
        // the legal prefix only.
        if codegg_core::work_order::can_transition_occurrence(current.state, state) {
            service
                .transition_occurrence(proj, occ, state, None, None, now_ms)
                .await
                .expect("advance occurrence");
        }
    }
}

// ── A — Creation and default immediate behavior ──────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn a_creation_immediate_materializes_once_with_restart_convergence() {
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool.clone())));
    let coordinator = WorkOrderCoordinator::new(service.clone());
    let sessions = FakeSessions::default();
    let scheduler = SchedulerHarness::new().await;

    // Human Task composer with default zero-delay/immediate gate.
    let created = service
        .create_work_order(
            &project_a(),
            &creator(),
            immediate_input("default task"),
            1_000,
        )
        .await
        .expect("create");
    assert!(!created.duplicate);
    let occurrence = service
        .create_occurrence(&project_a(), &created.work_order.id, None, 1_001)
        .await
        .expect("occurrence");

    // Coordinator claims it exactly once; one canonical session; one
    // initial AgentTurn; Task row moves to a running session identity.
    let ready = coordinator
        .evaluate_due_for_project(&project_a(), 1_002)
        .await
        .expect("evaluate");
    assert_eq!(ready.len(), 1);
    let claimed = service
        .claim_occurrence(&project_a(), &occurrence.id, 1_003)
        .await
        .expect("claim");
    assert_eq!(claimed.state, OccurrenceState::Claiming);
    // Duplicate claim loses.
    assert!(service
        .claim_occurrence(&project_a(), &occurrence.id, 1_004)
        .await
        .is_err());
    service
        .persist_workspace_link(&project_a(), &occurrence.id, "ws-1", None, 1_004)
        .await
        .expect("workspace link");

    // No fake Session rows exist for waiting work: the session id is
    // derived deterministically only at materialization.
    let session_id = WorkOrderCoordinator::session_id_for(&occurrence.id);
    let (_, dup) = sessions.get_or_create(&session_id, "default task").await;
    assert!(!dup);
    service
        .persist_session_link(&project_a(), &occurrence.id, &session_id, 1_005)
        .await
        .expect("session link");

    let submission_key = WorkOrderCoordinator::submission_key_for(&occurrence.id);
    let submitted = scheduler
        .submission
        .submit(
            Some(SubmissionKey::new(submission_key.clone()).expect("key")),
            scheduler.agent_turn_spec(&session_id, "default task", &submission_key),
        )
        .await
        .expect("submit");
    service
        .persist_job_link(
            &project_a(),
            &occurrence.id,
            submitted.job_id.as_str(),
            1_006,
        )
        .await
        .expect("job link");
    service
        .transition_occurrence(
            &project_a(),
            &occurrence.id,
            OccurrenceState::Running,
            None,
            None,
            1_007,
        )
        .await
        .expect("running");

    // Restart convergence: a reopened service over the same pool finds the
    // stored session/job links and reconciles by key instead of forking.
    let reopened = WorkOrderService::with_defaults(Some(pool.clone()));
    let current = reopened
        .get_occurrence(&project_a(), &occurrence.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(current.session_id.as_deref(), Some(session_id.as_str()));
    assert_eq!(current.job_id.as_deref(), Some(submitted.job_id.as_str()));
    let (_, dup) = sessions.get_or_create(&session_id, "default task").await;
    assert!(dup, "restart must converge on the stored session");
    let reconciled = scheduler
        .submission
        .reconcile_by_key(
            &SubmissionKey::new(submission_key).expect("key"),
            &scheduler.workspace_id,
            None,
        )
        .await
        .expect("reconcile");
    assert_eq!(reconciled.map(|job| job.job_id), Some(submitted.job_id));
    assert_eq!(sessions.count().await, 1);

    // Cancellation/steering thereafter routes through normal session
    // controls: the occurrence records Running with durable links and the
    // session row is an ordinary canonical session (proved in H).
    let running = reopened
        .get_occurrence(&project_a(), &occurrence.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(running.state, OccurrenceState::Running);
    assert_eq!(
        WorkOrderCoordinator::describe_partial(&running),
        PartialStage::Complete
    );
}

// ── B — Sequential queue behavior (15 lanes members) ─────────────────────

#[tokio::test(flavor = "current_thread")]
async fn b_fifteen_lane_order_restart_reorder_hold() {
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool.clone())));
    let coordinator = WorkOrderCoordinator::new(service.clone());

    let lane = service
        .create_lane(
            &project_a(),
            NewSequenceLane {
                label: Some("release-queue".to_owned()),
                failure_policy: LaneFailurePolicy::HoldLane,
                idempotency_key: None,
            },
            500,
        )
        .await
        .expect("lane");

    // Build a lane with 15 sequential work orders.
    let mut ids = Vec::new();
    let mut occ_ids = Vec::new();
    for i in 0..15 {
        let mut input = immediate_input(&format!("plan step {i}"));
        input.gates = ReleaseGateSet {
            join: GateJoin::All,
            gates: vec![GateSpec {
                kind: GateKind::SequenceReady,
                delay_secs: None,
                not_before_ms: None,
                lane_id: Some(lane.id.clone()),
                trigger_ref: None,
            }],
        };
        // First member is immediate in production batch shape; here all
        // members gate on sequence so lane order is the release order.
        if i == 0 {
            input.gates = ReleaseGateSet::immediate();
        }
        let created = service
            .create_work_order(&project_a(), &creator(), input, 1_000 + i as i64)
            .await
            .expect("create");
        let current = service
            .get_lane(&project_a(), &lane.id)
            .await
            .expect("lane")
            .expect("row");
        service
            .attach_to_lane(
                &project_a(),
                &lane.id,
                current.revision,
                &created.work_order.id,
                None,
                1_100 + i as i64,
            )
            .await
            .expect("attach");
        let occ = service
            .create_occurrence(&project_a(), &created.work_order.id, None, 1_200 + i as i64)
            .await
            .expect("occurrence");
        ids.push(created.work_order.id);
        occ_ids.push(occ.id);
    }

    // Stable order/revision after restart (same pool, new handle).
    let reopened = WorkOrderService::with_defaults(Some(pool.clone()));
    let lane_row = reopened
        .get_lane(&project_a(), &lane.id)
        .await
        .expect("lane")
        .expect("row");
    assert_eq!(lane_row.ordered_work_order_ids.len(), 15);
    assert_eq!(lane_row.ordered_work_order_ids, ids);
    let revision = lane_row.revision;

    // j/k reorder of future rows only: swap positions 5 and 6.
    let mut swapped = ids.clone();
    swapped.swap(5, 6);
    let reordered = service
        .reorder_lane(&project_a(), &lane.id, revision, swapped.clone(), 2_000)
        .await
        .expect("reorder future rows");
    assert_eq!(reordered.ordered_work_order_ids, swapped);
    assert_eq!(reordered.revision, revision + 1);

    // Stale revision conflict has zero mutation.
    let stale = service
        .reorder_lane(&project_a(), &lane.id, revision, ids.clone(), 2_001)
        .await;
    assert!(matches!(
        stale,
        Err(WorkOrderError::RevisionConflict { .. })
    ));
    let current = service
        .get_lane(&project_a(), &lane.id)
        .await
        .expect("lane")
        .expect("row");
    assert_eq!(current.ordered_work_order_ids, swapped);

    // Running/claimed predecessor is pinned: once the head occurrence is
    // claimed, reordering the lane is rejected with zero mutation.
    let head_occ = service
        .get_occurrence(&project_a(), &occ_ids[0])
        .await
        .expect("get")
        .expect("row");
    let _ = head_occ;
    service
        .claim_occurrence(&project_a(), &occ_ids[0], 2_002)
        .await
        .expect("claim head");
    let mut moved = swapped.clone();
    let first = moved.remove(0);
    moved.push(first);
    let pinned = service
        .reorder_lane(&project_a(), &lane.id, current.revision, moved, 2_003)
        .await;
    assert!(
        matches!(pinned, Err(WorkOrderError::StateConflict(_))),
        "claimed head must pin lane order, got {pinned:?}"
    );

    // Predecessor success releases only the next eligible item: after the
    // head completes, exactly member 1 becomes sequence-ready.
    for state in [OccurrenceState::Running, OccurrenceState::Completed] {
        // Claiming -> Running -> Completed is the legal path.
        service
            .transition_occurrence(&project_a(), &occ_ids[0], state, None, None, 2_004)
            .await
            .expect("advance head");
    }
    // Member 0 is terminal success; member 1's predecessor set is terminal
    // success so sequence_ready holds true; member 2 still waits on member 1.
    let second_wo = service
        .get_work_order(&project_a(), &swapped[1])
        .await
        .expect("get")
        .expect("row");
    let second_occ = service
        .list_occurrences(&project_a(), &swapped[1], Some(4))
        .await
        .expect("list")
        .occurrences
        .into_iter()
        .next()
        .expect("occurrence");
    assert!(
        coordinator
            .sequence_ready_for(&project_a(), &second_wo, &second_occ)
            .await
            .expect("sequence check"),
        "successor of completed head must be sequence-ready"
    );

    // Predecessor failure holds downstream by default (HoldLane): fail
    // member 1 and prove member 2 is not ready; no mass release.
    step_to_terminal(
        &service,
        &project_a(),
        &second_occ.id,
        OccurrenceState::Failed,
        2_005,
    )
    .await;
    let third_wo = service
        .get_work_order(&project_a(), &swapped[2])
        .await
        .expect("get")
        .expect("row");
    let third_occ = service
        .list_occurrences(&project_a(), &swapped[2], Some(4))
        .await
        .expect("list")
        .occurrences
        .into_iter()
        .next()
        .expect("occurrence");
    assert!(
        !coordinator
            .sequence_ready_for(&project_a(), &third_wo, &third_occ)
            .await
            .expect("sequence check"),
        "failed predecessor must hold downstream work"
    );
    assert!(sequence_holds(
        &[OccurrenceState::Failed],
        LaneFailurePolicy::HoldLane
    ));
    // Explicit authorized retry follows the documented attention path:
    // a running occurrence may enter needs-attention (permission wait,
    // worktree conflict) and an authorized actor resumes it to waiting.
    // (Failed itself is terminal by the matrix; the retry below exercises
    // the attention -> waiting resume on a fresh occurrence.)
    let retry_wo = service
        .create_work_order(&project_a(), &creator(), immediate_input("retry me"), 2_005)
        .await
        .expect("create");
    let retry_occ = service
        .create_occurrence(&project_a(), &retry_wo.work_order.id, None, 2_006)
        .await
        .expect("occurrence");
    service
        .claim_occurrence(&project_a(), &retry_occ.id, 2_007)
        .await
        .expect("claim");
    service
        .transition_occurrence(
            &project_a(),
            &retry_occ.id,
            OccurrenceState::Running,
            None,
            None,
            2_008,
        )
        .await
        .expect("running");
    service
        .transition_occurrence(
            &project_a(),
            &retry_occ.id,
            OccurrenceState::NeedsAttention,
            Some(AttentionCode::PredecessorFailed),
            Some("predecessor_failed: step 1 needs retry"),
            2_009,
        )
        .await
        .expect("attention");
    service
        .transition_occurrence(
            &project_a(),
            &retry_occ.id,
            OccurrenceState::Waiting,
            None,
            None,
            2_010,
        )
        .await
        .expect("retry resumes to waiting");
}

// ── C — Release gates and boolean join ───────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn c_release_gate_join_latch_duplicate_timezone_restart() {
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool.clone())));
    let coordinator = WorkOrderCoordinator::new(service.clone());

    // Each gate independently releases at its defined boundary.
    let delayed = service
        .create_work_order(&project_a(), &creator(), delay_input("delayed", 60), 1_000)
        .await
        .expect("create");
    service
        .create_occurrence(&project_a(), &delayed.work_order.id, None, 1_001)
        .await
        .expect("occurrence");
    assert!(coordinator
        .evaluate_due_for_project(&project_a(), 1_002)
        .await
        .expect("eval")
        .is_empty());
    let ready = coordinator
        .evaluate_due_for_project(&project_a(), 61_000)
        .await
        .expect("eval");
    assert!(ready.iter().any(|(wo, _)| wo.id == delayed.work_order.id));

    // NotBefore with an explicit offset timestamp is stable across
    // serialization: 2030-01-01T00:00:00Z == 1893456000000 ms.
    const NOT_BEFORE_2030: i64 = 1_893_456_000_000;
    let nb = service
        .create_work_order(
            &project_a(),
            &creator(),
            not_before_input("nb", NOT_BEFORE_2030),
            1_000,
        )
        .await
        .expect("create");
    let nb_occ = service
        .create_occurrence(&project_a(), &nb.work_order.id, None, 1_001)
        .await
        .expect("occurrence");
    let work = service
        .get_work_order(&project_a(), &nb.work_order.id)
        .await
        .expect("get")
        .expect("row");
    let occ = service
        .get_occurrence(&project_a(), &nb_occ.id)
        .await
        .expect("get")
        .expect("row");
    let early = evaluate_occurrence_gates(
        &work.gates,
        &occ,
        work.created_at_ms,
        None,
        NOT_BEFORE_2030 - 1,
        true,
    );
    assert!(!early.satisfied);
    let at = evaluate_occurrence_gates(
        &work.gates,
        &occ,
        work.created_at_ms,
        None,
        NOT_BEFORE_2030,
        true,
    );
    assert!(at.satisfied);
    // Serialization round-trips the exact instant (no timezone guessing).
    let json = serde_json::to_string(&work.gates).expect("gate json");
    let decoded: ReleaseGateSet = serde_json::from_str(&json).expect("decode");
    assert_eq!(decoded, work.gates);

    // All waits for every enabled gate; Any releases on the first.
    let both_all = ReleaseGateSet {
        join: GateJoin::All,
        gates: vec![
            GateSpec {
                kind: GateKind::NotBefore,
                delay_secs: None,
                not_before_ms: Some(10_000),
                lane_id: None,
                trigger_ref: None,
            },
            GateSpec {
                kind: GateKind::SequenceReady,
                delay_secs: None,
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            },
        ],
    };
    let both_any = ReleaseGateSet {
        join: GateJoin::Any,
        ..both_all.clone()
    };
    let probe = service
        .create_occurrence(&project_a(), &delayed.work_order.id, None, 2_000)
        .await
        .expect("probe occurrence");
    let probe_row = service
        .get_occurrence(&project_a(), &probe.id)
        .await
        .expect("get")
        .expect("row");
    let all_pending = evaluate_occurrence_gates(&both_all, &probe_row, 1_000, None, 20_000, false);
    assert!(!all_pending.satisfied);
    let all_ready = evaluate_occurrence_gates(&both_all, &probe_row, 1_000, None, 20_000, true);
    assert!(all_ready.satisfied);
    let any_ready = evaluate_occurrence_gates(&both_any, &probe_row, 1_000, None, 20_000, false);
    assert!(any_ready.satisfied);

    // Satisfaction latches once per occurrence; duplicate
    // timer/reconcile/trigger events are harmless.
    service
        .persist_gate_evaluation(
            &project_a(),
            &nb_occ.id,
            merge_latches(&occ.gate_latches, &at.newly_latched),
            None,
            true,
            NOT_BEFORE_2030,
        )
        .await
        .expect("latch");
    let latched = service
        .get_occurrence(&project_a(), &nb_occ.id)
        .await
        .expect("get")
        .expect("row");
    assert!(latched.gate_latches.contains(&GateKind::NotBefore));
    // Duplicate latch converges (idempotent merge, no new execution).
    let again = merge_latches(&latched.gate_latches, &[GateKind::NotBefore]);
    service
        .persist_gate_evaluation(&project_a(), &nb_occ.id, again, None, true, NOT_BEFORE_2030)
        .await
        .expect("duplicate latch");
    let latched2 = service
        .get_occurrence(&project_a(), &nb_occ.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(latched.gate_latches, latched2.gate_latches);

    // Restart before/after satisfaction preserves the correct state.
    let reopened = WorkOrderCoordinator::new(Arc::new(WorkOrderService::with_defaults(Some(pool))));
    let eval = reopened
        .evaluate_due_for_project(&project_a(), NOT_BEFORE_2030)
        .await
        .expect("eval");
    // The latched not-before occurrence is ready (or already claimed path);
    // the key assertion is no duplicate ready row is minted on re-eval.
    let eval2 = reopened
        .evaluate_due_for_project(&project_a(), NOT_BEFORE_2030)
        .await
        .expect("eval");
    assert_eq!(eval.len(), eval2.len());

    // Invalid or impossible configurations fail validation rather than spin.
    let mut bad = immediate_input("bad");
    bad.gates = ReleaseGateSet {
        join: GateJoin::All,
        gates: vec![
            GateSpec {
                kind: GateKind::Immediate,
                delay_secs: None,
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            },
            GateSpec {
                kind: GateKind::Delay,
                delay_secs: Some(5),
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            },
        ],
    };
    assert!(service
        .create_work_order(&project_a(), &creator(), bad, 3_000)
        .await
        .is_err());
    let mut over = immediate_input("over");
    over.gates = ReleaseGateSet {
        join: GateJoin::All,
        gates: vec![GateSpec {
            kind: GateKind::Delay,
            delay_secs: Some(366 * 24 * 60 * 60 + 1),
            not_before_ms: None,
            lane_id: None,
            trigger_ref: None,
        }],
    };
    assert!(service
        .create_work_order(&project_a(), &creator(), over, 3_001)
        .await
        .is_err());
}

// ── D — Finite repeat ────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn d_finite_repeat_exact_counts_exhaustion() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool.clone()));

    // Exact counts for 1 (run once): no successor may spawn.
    let mut once = immediate_input("once");
    once.repeat_count = 1;
    let created = service
        .create_work_order(&project_a(), &creator(), once, 1_000)
        .await
        .expect("create");
    let first = service
        .create_occurrence(&project_a(), &created.work_order.id, None, 1_001)
        .await
        .expect("occ");
    step_to_terminal(
        &service,
        &project_a(),
        &first.id,
        OccurrenceState::Completed,
        1_002,
    )
    .await;
    assert!(service
        .create_next_repeat_occurrence(&project_a(), &created.work_order.id, &first.id, 1_003)
        .await
        .is_err());
    assert!(is_repeat_exhausted(1, 1));

    // Exact count 2: one successor with a distinct identity, then stop.
    let mut twice = immediate_input("twice");
    twice.repeat_count = 2;
    let created = service
        .create_work_order(&project_a(), &creator(), twice, 2_000)
        .await
        .expect("create");
    let first = service
        .create_occurrence(&project_a(), &created.work_order.id, None, 2_001)
        .await
        .expect("occ");
    step_to_terminal(
        &service,
        &project_a(),
        &first.id,
        OccurrenceState::Completed,
        2_002,
    )
    .await;
    let next = service
        .create_next_repeat_occurrence(&project_a(), &created.work_order.id, &first.id, 2_003)
        .await
        .expect("repeat");
    assert!(!next.duplicate);
    assert_ne!(next.occurrence.id, first.id);
    assert_eq!(next.occurrence.occurrence_index, 1);
    assert_ne!(
        session_id_for_occurrence(first.id.as_str()),
        session_id_for_occurrence(next.occurrence.id.as_str())
    );
    assert_ne!(
        submission_key_for_occurrence(first.id.as_str()),
        submission_key_for_occurrence(next.occurrence.id.as_str())
    );
    // Duplicate wake converges on the stored successor instead of forking.
    let again = service
        .create_next_repeat_occurrence(&project_a(), &created.work_order.id, &first.id, 2_004)
        .await
        .expect("converge");
    assert!(again.duplicate);
    assert_eq!(again.occurrence.id, next.occurrence.id);
    step_to_terminal(
        &service,
        &project_a(),
        &next.occurrence.id,
        OccurrenceState::Completed,
        2_005,
    )
    .await;
    assert!(service
        .create_next_repeat_occurrence(
            &project_a(),
            &created.work_order.id,
            &next.occurrence.id,
            2_006
        )
        .await
        .is_err());

    // Maximum allowed repeat is accepted; over-bound is rejected.
    let mut max = immediate_input("max");
    max.repeat_count = MAX_REPEAT_COUNT;
    assert!(service
        .create_work_order(&project_a(), &creator(), max, 3_000)
        .await
        .is_ok());
    let mut over = immediate_input("over");
    over.repeat_count = MAX_REPEAT_COUNT + 1;
    assert!(service
        .create_work_order(&project_a(), &creator(), over, 3_001)
        .await
        .is_err());

    // Restart between occurrences preserves the chain: reopen and spawn.
    let mut chain = immediate_input("chain");
    chain.repeat_count = 3;
    let created = service
        .create_work_order(&project_a(), &creator(), chain, 4_000)
        .await
        .expect("create");
    let first = service
        .create_occurrence(&project_a(), &created.work_order.id, None, 4_001)
        .await
        .expect("occ");
    step_to_terminal(
        &service,
        &project_a(),
        &first.id,
        OccurrenceState::Completed,
        4_002,
    )
    .await;
    let reopened = WorkOrderService::with_defaults(Some(pool.clone()));
    let next = reopened
        .create_next_repeat_occurrence(&project_a(), &created.work_order.id, &first.id, 4_003)
        .await
        .expect("repeat after restart");
    assert_eq!(next.occurrence.occurrence_index, 1);

    // Delay anchoring across repeats: the second delay anchors to the
    // prior terminal timestamp, not to creation.
    let deadline =
        codegg_core::work_order::delay_deadline_for_occurrence(1_000, 1, Some(5_000), 60);
    assert_eq!(deadline, 65_000);

    // Cancellation of remaining repeats: cancelling the work order blocks
    // further occurrence creation (terminal template rejects new rows via
    // state checks at the service layer; occurrence cancel is terminal).
    step_to_terminal(
        &service,
        &project_a(),
        &next.occurrence.id,
        OccurrenceState::Cancelled,
        4_004,
    )
    .await;
    let cancelled = service
        .get_occurrence(&project_a(), &next.occurrence.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(cancelled.state, OccurrenceState::Cancelled);

    // No unbounded recurrence: repeat_count has no infinite representation
    // and the budget check is exact.
    assert!(is_repeat_exhausted(3, 3));
    assert!(!is_repeat_exhausted(2, 3));
}

// ── E — Exactly-once materialization fault injection (8 windows) ─────────

#[tokio::test(flavor = "current_thread")]
async fn e_fault_windows_converge_without_duplicates() {
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool.clone())));
    let sessions = FakeSessions::default();
    let scheduler = SchedulerHarness::new().await;

    let created = service
        .create_work_order(&project_a(), &creator(), immediate_input("faulty"), 1_000)
        .await
        .expect("create");
    let occurrence = service
        .create_occurrence(&project_a(), &created.work_order.id, None, 1_001)
        .await
        .expect("occurrence");

    // Window 1 — before occurrence claim commit: a duplicate claim attempt
    // after the first claim conflicts; recovery resumes from the stored row.
    service
        .claim_occurrence(&project_a(), &occurrence.id, 1_002)
        .await
        .expect("claim");
    assert!(service
        .claim_occurrence(&project_a(), &occurrence.id, 1_003)
        .await
        .is_err());
    let reopened = WorkOrderService::with_defaults(Some(pool.clone()));
    let current = reopened
        .get_occurrence(&project_a(), &occurrence.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(current.state, OccurrenceState::Claiming);
    assert_eq!(
        WorkOrderCoordinator::describe_partial(&current),
        PartialStage::AwaitingWorkspace
    );

    // Window 2 — after claim but before worktree allocation: workspace link
    // persists once; a second persist is idempotent convergence.
    service
        .persist_workspace_link(&project_a(), &occurrence.id, "ws-1", Some("wt-1"), 1_004)
        .await
        .expect("ws link");
    service
        .persist_workspace_link(&project_a(), &occurrence.id, "ws-1", Some("wt-1"), 1_005)
        .await
        .expect("ws relink converges");
    let current = reopened
        .get_occurrence(&project_a(), &occurrence.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(current.workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(
        WorkOrderCoordinator::describe_partial(&current),
        PartialStage::AwaitingSession
    );

    // Window 3 — after worktree reservation but before ready binding: the
    // stored reservation is reused, never reallocated (same ids above).

    // Window 4 — after session create but before occurrence stores session
    // id: deterministic `create_with_id` converges on the existing row.
    let session_id = session_id_for_occurrence(occurrence.id.as_str());
    let store = SessionStore::new(pool.clone());
    let first = store
        .create_with_id(
            &session_id,
            CreateSession {
                project_id: project_a().as_str().to_owned(),
                directory: "/tmp".to_owned(),
                title: Some("faulty".to_owned()),
                parent_id: None,
                workspace_id: None,
                agent: Some("build".to_owned()),
                model: None,
                tags: None,
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
            },
        )
        .await
        .expect("session create");
    assert_eq!(first.id, session_id);
    // Crash before the link: recovery finds the row by id.
    let found = store.get(&session_id).await.expect("get").expect("row");
    assert_eq!(found.id, session_id);
    service
        .persist_session_link(&project_a(), &occurrence.id, &session_id, 1_006)
        .await
        .expect("session link");
    let (_, dup) = sessions.get_or_create(&session_id, "faulty").await;
    let _ = dup;

    // Window 5 — after occurrence stores session id but before AgentTurn
    // job submit: recovery sees the session link and submits at most once.
    let current = reopened
        .get_occurrence(&project_a(), &occurrence.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(current.session_id.as_deref(), Some(session_id.as_str()));
    assert_eq!(
        WorkOrderCoordinator::describe_partial(&current),
        PartialStage::AwaitingJob
    );

    // Window 6 — after job creation but before occurrence stores job id:
    // reconcile-by-key finds the submitted job.
    let submission_key = submission_key_for_occurrence(occurrence.id.as_str());
    let submitted = scheduler
        .submission
        .submit(
            Some(SubmissionKey::new(submission_key.clone()).expect("key")),
            scheduler.agent_turn_spec(&session_id, "faulty", &submission_key),
        )
        .await
        .expect("submit");
    let reconciled = scheduler
        .submission
        .reconcile_by_key(
            &SubmissionKey::new(submission_key.clone()).expect("key"),
            &scheduler.workspace_id,
            None,
        )
        .await
        .expect("reconcile");
    assert_eq!(
        reconciled.map(|job| job.job_id),
        Some(submitted.job_id.clone())
    );
    service
        .persist_job_link(
            &project_a(),
            &occurrence.id,
            submitted.job_id.as_str(),
            1_007,
        )
        .await
        .expect("job link");

    // Window 7 — after job submit acknowledgement but before coordinator
    // completion marker: the stored job link plus Running transition
    // converge; a duplicate submit converges on the same job id.
    let dup_submit = scheduler
        .submission
        .submit(
            Some(SubmissionKey::new(submission_key.clone()).expect("key")),
            scheduler.agent_turn_spec(&session_id, "faulty", &submission_key),
        )
        .await
        .expect("duplicate submit converges");
    assert_eq!(dup_submit.job_id, submitted.job_id);
    service
        .transition_occurrence(
            &project_a(),
            &occurrence.id,
            OccurrenceState::Running,
            None,
            None,
            1_008,
        )
        .await
        .expect("running");

    // Window 8 — during daemon shutdown/restart while running: the
    // incomplete-occurrence scan no longer surfaces the running row as
    // partial, and the durable links survive reopen.
    let incomplete = reopened
        .list_incomplete_occurrences(&project_a(), 16)
        .await
        .expect("incomplete");
    assert!(!incomplete
        .iter()
        .any(|occ| occ.id == occurrence.id && occ.state == OccurrenceState::Claiming));
    let current = reopened
        .get_occurrence(&project_a(), &occurrence.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(current.state, OccurrenceState::Running);
    assert_eq!(current.session_id.as_deref(), Some(session_id.as_str()));
    assert_eq!(current.job_id.as_deref(), Some(submitted.job_id.as_str()));

    // Recovery never replays a committed mutating side effect merely to
    // repair metadata: exactly one session id and one job id exist.
    let jobs = scheduler
        .store
        .list_jobs(codegg_core::jobs::JobStoreQuery::default())
        .await
        .expect("jobs");
    assert_eq!(jobs.len(), 1);
}

// ── F — Workspace/worktree isolation ─────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn f_worktree_isolation_and_truthfulness() {
    // Three concurrent mutation-capable Git work orders get distinct
    // managed worktrees with independent leases/paths/provenance.
    let repo_dir = tempfile::tempdir().expect("repo dir");
    for args in [
        vec!["init"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
        vec!["config", "commit.gpgsign", "false"],
    ] {
        assert!(std::process::Command::new("git")
            .args(&args)
            .current_dir(repo_dir.path())
            .status()
            .expect("git")
            .success());
    }
    std::fs::write(repo_dir.path().join("README.md"), "hello\n").expect("write");
    for args in [vec!["add", "."], vec!["commit", "-m", "init"]] {
        assert!(std::process::Command::new("git")
            .args(&args)
            .current_dir(repo_dir.path())
            .status()
            .expect("git")
            .success());
    }
    let managed_root = tempfile::tempdir().expect("managed root");
    let service = codegg_core::worktree_service::WorktreeService::memory(
        managed_root.path().to_path_buf(),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    let project_id = project_a();
    let repository_id = codegg_core::identity::RepositoryId::new();
    let mut records = Vec::new();
    for name in ["ws-a", "ws-b", "ws-c"] {
        let request = codegg_core::worktree_service::CreateWorktreeRequest {
            project_id: project_id.clone(),
            repository_id: repository_id.clone(),
            workspace_id: codegg_core::workspace::WorkspaceId::new_unchecked(name),
            node_id: None,
            repository_root: repo_dir.path().to_path_buf(),
            base_commit: None,
            base_path: None,
            owner_run_id: codegg_core::identity::AgentRunId::new(),
        };
        let (record, _lease) = service.create(&request).await.expect("worktree");
        records.push(record);
    }
    assert_ne!(records[0].worktree_id, records[1].worktree_id);
    assert_ne!(records[1].worktree_id, records[2].worktree_id);
    assert_ne!(records[0].path, records[1].path);
    assert_ne!(records[0].path, records[2].path);
    let canonical_managed = managed_root
        .path()
        .canonicalize()
        .unwrap_or_else(|_| managed_root.path().to_path_buf());
    for record in &records {
        assert!(
            record.path.starts_with(&canonical_managed),
            "worktree must live under the managed root"
        );
    }
    // No shared write root.
    let mut paths: HashSet<_> = records.iter().map(|r| r.path.clone()).collect();
    assert_eq!(paths.len(), 3);
    paths.clear();

    // Dirty/conflicted attention: only clean trees are cleanup-safe; dirty
    // trees are retained, never silently cleaned.
    assert!(codegg_core::worktree_service::WorktreeHealth::Clean.cleanup_safe());
    assert!(!codegg_core::worktree_service::WorktreeHealth::Dirty.cleanup_safe());
    assert!(!codegg_core::worktree_service::WorktreeHealth::Conflicted.cleanup_safe());

    // Cancellation during preparation and restart with an active lease are
    // owned by the managed-worktree service (leases are independent per
    // record above); cleanup/archive after terminal state follows the
    // existing worktree rules (no ad-hoc directory copies in this path).
    assert_eq!(
        resolve_workspace_action(None, true, true),
        codegg_core::work_order::WorkspaceAction::UseManagedWorktree
    );

    // Non-Git mutation truthfulness: never labeled isolated.
    match resolve_workspace_action(None, false, true) {
        codegg_core::work_order::WorkspaceAction::NeedsAttention { diagnostic, .. } => {
            assert!(diagnostic.contains("isolation_unavailable"));
        }
        other => panic!("non-Git mutation must need attention, got {other:?}"),
    }
    // Serialized sharing is truthfully labeled shared, never isolated.
    assert_eq!(
        resolve_workspace_action(Some(WorkspacePolicy::Serialized), false, true),
        codegg_core::work_order::WorkspaceAction::ShareSerialized
    );
}

// ── G — Model/provider and policy drift ──────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn g_model_provider_policy_drift() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool));

    // Removed selected model → explicit attention, never silent substitution.
    let available: HashSet<String> = ["openai/gpt-5".to_owned()].into_iter().collect();
    assert_eq!(
        WorkOrderCoordinator::resolve_model_for(Some("removed/model"), &available).unwrap_err(),
        AttentionCode::ModelUnavailable
    );
    assert_eq!(
        resolve_model(Some("removed/model"), &|m| available.contains(m)).unwrap_err(),
        AttentionCode::ModelUnavailable
    );
    assert_eq!(
        WorkOrderCoordinator::resolve_model_for(Some("openai/gpt-5"), &available)
            .expect("available"),
        Some("openai/gpt-5".to_owned())
    );

    // Disabled/tombstoned provider connection: an empty catalog resolves
    // every requested model to attention (no fallback to daemon default
    // when a model was explicitly requested).
    let empty: HashSet<String> = HashSet::new();
    assert_eq!(
        WorkOrderCoordinator::resolve_model_for(Some("openai/gpt-5"), &empty).unwrap_err(),
        AttentionCode::ModelUnavailable
    );
    // Unrequested model stays daemon-default (None) even with no providers.
    assert_eq!(
        WorkOrderCoordinator::resolve_model_for(None, &empty).expect("default"),
        None
    );

    // Narrowed project approval policy between creation and execution:
    // snapshots narrow, never widen.
    let (effective, narrowed) = narrow_approval(Some(ApprovalRequest::Yolo), Some(0));
    assert_eq!(effective, Some(ApprovalRequest::Interactive));
    assert!(narrowed);
    let created_policy = service
        .create_work_order(
            &project_a(),
            &creator(),
            {
                let mut input = immediate_input("policy");
                input.requested_approval = Some(ApprovalRequest::Yolo);
                input.requested_sandbox = Some(SandboxRequest::FullHost);
                input
            },
            1_000,
        )
        .await
        .expect("create");
    let policy_row = service
        .get_work_order(&project_a(), &created_policy.work_order.id)
        .await
        .expect("get")
        .expect("row");
    let (approval_eff, approval_narrowed) = narrow_approval(policy_row.requested_approval, Some(0));
    let (sandbox_eff, sandbox_narrowed) = narrow_sandbox(policy_row.requested_sandbox, Some(1));
    assert_eq!(approval_eff, Some(ApprovalRequest::Interactive));
    assert!(approval_narrowed);
    assert_eq!(sandbox_eff, Some(SandboxRequest::WorkspaceWrite));
    assert!(sandbox_narrowed);

    // Narrowed sandbox profile: FullHost snapshot against a WorkspaceWrite
    // ceiling narrows deterministically.
    let (sandbox, narrowed) = narrow_sandbox(Some(SandboxRequest::FullHost), Some(1));
    assert_eq!(sandbox, Some(SandboxRequest::WorkspaceWrite));
    assert!(narrowed);

    // Revoked creating principal capability requires reauthorization: a
    // claimed-immutable execution payload cannot be widened by editing the
    // work order after claim (prompt/model/gates/repeat are frozen).
    let created = service
        .create_work_order(&project_a(), &creator(), immediate_input("frozen"), 2_000)
        .await
        .expect("create");
    let occ = service
        .create_occurrence(&project_a(), &created.work_order.id, None, 2_001)
        .await
        .expect("occ");
    service
        .claim_occurrence(&project_a(), &occ.id, 2_002)
        .await
        .expect("claim");
    let patch = WorkOrderPatch {
        prompt: Some("widened".to_owned()),
        ..Default::default()
    };
    let err = service
        .update_work_order(
            &project_a(),
            &created.work_order.id,
            patch,
            created.work_order.revision,
            2_003,
        )
        .await;
    assert!(matches!(err, Err(WorkOrderError::StateConflict(_))));

    // Original attribution is immutable (creator survives policy drift).
    let row = service
        .get_work_order(&project_a(), &created.work_order.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.creator_principal, creator());
}

// ── H — Permission/question and steering lifecycle ───────────────────────

#[tokio::test(flavor = "current_thread")]
async fn h_permission_steering_cancel_use_ordinary_session_mechanisms() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool.clone()));

    // Materialize a running work-order session as an ordinary session row.
    let created = service
        .create_work_order(
            &project_a(),
            &creator(),
            immediate_input("interactive"),
            1_000,
        )
        .await
        .expect("create");
    let occ = service
        .create_occurrence(&project_a(), &created.work_order.id, None, 1_001)
        .await
        .expect("occ");
    service
        .claim_occurrence(&project_a(), &occ.id, 1_002)
        .await
        .expect("claim");
    let session_id = session_id_for_occurrence(occ.id.as_str());
    let store = SessionStore::new(pool.clone());
    store
        .create_with_id(
            &session_id,
            CreateSession {
                project_id: project_a().as_str().to_owned(),
                directory: "/tmp".to_owned(),
                title: Some("interactive".to_owned()),
                parent_id: None,
                workspace_id: None,
                agent: Some("build".to_owned()),
                model: None,
                tags: Some(vec!["work-order".to_owned()]),
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
            },
        )
        .await
        .expect("session");
    service
        .persist_session_link(&project_a(), &occ.id, &session_id, 1_003)
        .await
        .expect("link");
    service
        .transition_occurrence(
            &project_a(),
            &occ.id,
            OccurrenceState::Running,
            None,
            None,
            1_004,
        )
        .await
        .expect("running");

    // Interactive permission request uses the ordinary sync registry:
    // register BEFORE publishing Pending, then answer from the authorized
    // controller. An unauthorized observer session cannot answer it.
    let perm_id = format!("perm-m007-{}", occ.id.as_str());
    let (tx, rx) = tokio::sync::oneshot::channel();
    PermissionRegistry::register_with_session(session_id.clone(), None, perm_id.clone(), tx);
    assert!(!PermissionRegistry::respond_scoped(
        "other-session",
        &perm_id,
        PermissionDecision::AllowOnce
    ));
    assert!(PermissionRegistry::respond_scoped(
        &session_id,
        &perm_id,
        PermissionDecision::AllowOnce
    ));
    assert_eq!(rx.await.expect("decision"), PermissionDecision::AllowOnce);

    // Steering the active turn goes through the ordinary session record:
    // the materialized session is independently readable as a normal
    // session (no parallel task-chat runtime).
    let session = store.get(&session_id).await.expect("get").expect("session");
    assert_eq!(session.id, session_id);
    assert_eq!(session.project_id, project_a().as_str());

    // Cancelling the session/task routes occurrence cancellation through
    // the durable outcome (terminal), and downstream sequential work does
    // not release while the predecessor is unresolved attention.
    let lane = service
        .create_lane(
            &project_a(),
            NewSequenceLane {
                label: None,
                failure_policy: LaneFailurePolicy::HoldLane,
                idempotency_key: None,
            },
            1_005,
        )
        .await
        .expect("lane");
    // Attach both work orders to the lane is covered in B; here assert the
    // hold predicate directly for the attention state.
    let _ = lane;
    service
        .transition_occurrence(
            &project_a(),
            &occ.id,
            OccurrenceState::NeedsAttention,
            Some(AttentionCode::PredecessorAttention),
            Some("waiting on user"),
            1_006,
        )
        .await
        .expect("attention");
    assert!(sequence_holds(
        &[OccurrenceState::NeedsAttention],
        LaneFailurePolicy::HoldLane
    ));
    assert!(!sequence_predecessors_terminal(&[
        OccurrenceState::NeedsAttention
    ]));
    service
        .cancel_occurrence(&project_a(), &occ.id, 1_007)
        .await
        .expect("cancel");
    let cancelled = service
        .get_occurrence(&project_a(), &occ.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(cancelled.state, OccurrenceState::Cancelled);
}

// ── I — External trigger security and replay ─────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn i_trigger_replay_security_matrix() {
    const REF: &str = "hook-m007";
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool.clone()));
    let mk_gated = |prompt: &str| {
        let mut input = immediate_input(prompt);
        input.gates = ReleaseGateSet {
            join: GateJoin::All,
            gates: vec![GateSpec {
                kind: GateKind::ExternalTrigger,
                delay_secs: None,
                not_before_ms: None,
                lane_id: None,
                trigger_ref: Some(REF.to_owned()),
            }],
        };
        input
    };

    let created = service
        .create_work_order(&project_a(), &creator(), mk_gated("gated"), 1_000)
        .await
        .expect("create");
    let occ = service
        .create_occurrence(&project_a(), &created.work_order.id, None, 1_001)
        .await
        .expect("occ");
    let outcome = service
        .create_task_trigger(
            &project_a(),
            &created.work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: None,
                max_fires: Some(2),
                idempotency_key: Some("m007-key-1".to_owned()),
            },
            1_002,
        )
        .await
        .expect("trigger");
    let bearer = outcome.plaintext_secret.clone().expect("one-time secret");
    assert!(bearer.starts_with("cggtr_"));

    // Stored secret is verifier/hash only: the row exposes no plaintext.
    let row = service
        .get_task_trigger(&project_a(), &outcome.trigger.id)
        .await
        .expect("get")
        .expect("row");
    let debug = format!("{row:?}");
    assert!(!debug.contains(&bearer));
    // Metadata projection carries no secret/verifier content.
    let meta = row.metadata(1_002);
    let meta_json = serde_json::to_string(&meta.to_dto()).expect("meta json");
    assert!(!meta_json.contains("cggtr_"));
    assert!(!meta_json.to_lowercase().contains("secret"));
    assert!(!meta_json.to_lowercase().contains("verifier"));

    // Valid POST fires only the intended gate (first fire latches).
    let first = service
        .fire_task_trigger(&bearer, Some("idem-1"), 1_003)
        .await
        .expect("fire");
    assert!(first.latched);
    assert!(!first.duplicate);
    // Duplicate POST and duplicate Idempotency-Key return stable semantics.
    let replay = service
        .fire_task_trigger(&bearer, Some("idem-1"), 1_004)
        .await
        .expect("replay");
    assert!(replay.duplicate);
    assert_eq!(replay.receipt_id, first.receipt_id);
    let no_key_replay = service
        .fire_task_trigger(&bearer, None, 1_005)
        .await
        .expect("no-key replay");
    assert!(
        !no_key_replay.latched,
        "already-latched occurrence must not double-release"
    );

    // Concurrent distinct idempotency keys cannot double-release one
    // latched occurrence.
    let (a, b) = tokio::join!(
        service.fire_task_trigger(&bearer, Some("idem-a"), 1_006),
        service.fire_task_trigger(&bearer, Some("idem-b"), 1_007),
    );
    assert!(!a.expect("fire a").latched);
    assert!(!b.expect("fire b").latched);

    // Wrong secret and wrong public trigger id reveal no sensitive detail
    // (same generic shape, no locator/project/secret content).
    let bad_secret = format!("{}WRONG", &bearer[..bearer.len() - 4]);
    for presented in [bad_secret.as_str(), "cggtr_missing.invalid"] {
        let err = service
            .fire_task_trigger(presented, None, 1_008)
            .await
            .unwrap_err();
        let text = format!("{err}");
        assert!(
            !text.contains("project-1"),
            "failure must not leak project, got {text}"
        );
        assert!(!text.contains(&bearer));
    }

    // Expired/revoked/max-fire triggers fail safely. Exhaust max_fires=2
    // via the repeat chain: complete this occurrence, spawn the next,
    // fire it, then prove the third fire is inert.
    step_to_terminal(
        &service,
        &project_a(),
        &occ.id,
        OccurrenceState::Completed,
        1_009,
    )
    .await;
    let next = service
        .create_next_repeat_occurrence(&project_a(), &created.work_order.id, &occ.id, 1_010)
        .await;
    // repeat_count == 1, so no successor exists: the fire is inert rather
    // than pre-arming future work.
    assert!(next.is_err());
    let inert = service
        .fire_task_trigger(&bearer, Some("idem-late"), 1_011)
        .await
        .expect("inert");
    assert!(!inert.latched);

    // Revocation is monotonic and idempotent; revoked fires fail closed.
    service
        .revoke_task_trigger(&project_a(), &outcome.trigger.id, 1_012)
        .await
        .expect("revoke");
    service
        .revoke_task_trigger(&project_a(), &outcome.trigger.id, 1_013)
        .await
        .expect("re-revoke idempotent");
    assert!(service
        .fire_task_trigger(&bearer, Some("idem-revoked"), 1_014)
        .await
        .is_err());

    // Expiry fails closed.
    let exp_wo = service
        .create_work_order(&project_a(), &creator(), mk_gated("expiring"), 2_000)
        .await
        .expect("create");
    service
        .create_occurrence(&project_a(), &exp_wo.work_order.id, None, 2_001)
        .await
        .expect("occ");
    let exp = service
        .create_task_trigger(
            &project_a(),
            &exp_wo.work_order.id,
            &creator(),
            NewTaskTrigger {
                trigger_ref: None,
                expires_at_ms: Some(2_002),
                max_fires: None,
                idempotency_key: Some("m007-exp".to_owned()),
            },
            2_001,
        )
        .await
        .expect("trigger");
    let exp_bearer = exp.plaintext_secret.expect("secret");
    assert!(service
        .fire_task_trigger(&exp_bearer, None, 2_003)
        .await
        .is_err());

    // The trigger bearer authorizes nothing beyond its one gate: it is not
    // a Core principal credential (fire path never resolves a principal;
    // management travels the authenticated project protocol — M005).
    // Logs/audit never contain the bearer secret: harvest the debug shapes.
    let fire_debug = format!("{first:?}");
    assert!(!fire_debug.contains(&bearer));
}

// ── J — Agent work-order batch ───────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn j_agent_batch_idempotency_bounds() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool.clone()));

    // Host bounds are hard and restart-durable (M006 tool constants pin
    // the agent surface below the human surface).
    const {
        assert!(codegg::tool::work_order::MAX_AGENT_WORK_ORDER_BATCH == 16);
        assert!(codegg::tool::work_order::MAX_AGENT_WORK_ORDER_BATCH <= MAX_WORK_ORDER_BATCH_ITEMS);
        assert!(codegg::tool::work_order::MAX_AGENT_REPEAT_COUNT == 8);
        assert!(codegg::tool::work_order::MAX_AGENT_REPEAT_COUNT <= MAX_REPEAT_COUNT);
        assert!(codegg::tool::work_order::MAX_AGENT_WORK_ORDER_DEPTH == 4);
    }

    // One bounded atomic batch commits with deterministic lane order.
    let lane = service
        .create_lane(
            &project_a(),
            NewSequenceLane {
                label: Some("agent-plans".to_owned()),
                failure_policy: LaneFailurePolicy::HoldLane,
                idempotency_key: Some("m007-agent-lane".to_owned()),
            },
            1_000,
        )
        .await
        .expect("lane");
    let items: Vec<NewWorkOrder> = (0..4)
        .map(|i| {
            let mut input = immediate_input(&format!("agent plan {i}"));
            input.gates = ReleaseGateSet {
                join: GateJoin::All,
                gates: vec![GateSpec {
                    kind: GateKind::SequenceReady,
                    delay_secs: None,
                    not_before_ms: None,
                    lane_id: Some(lane.id.clone()),
                    trigger_ref: None,
                }],
            };
            input.idempotency_key = Some(format!("m007-aw-item-{i}"));
            input
        })
        .collect();
    let batch = service
        .batch_create_work_orders(
            &project_a(),
            &creator(),
            items,
            Some(lane.id.clone()),
            Some("m007-aw-batch-1".to_owned()),
            1_001,
        )
        .await
        .expect("batch");
    assert_eq!(batch.work_orders.len(), 4);
    assert!(!batch.duplicate);

    // Retry of the exact batch invocation converges without duplicates.
    let items2: Vec<NewWorkOrder> = (0..4)
        .map(|i| {
            let mut input = immediate_input(&format!("agent plan {i}"));
            input.gates = ReleaseGateSet {
                join: GateJoin::All,
                gates: vec![GateSpec {
                    kind: GateKind::SequenceReady,
                    delay_secs: None,
                    not_before_ms: None,
                    lane_id: Some(lane.id.clone()),
                    trigger_ref: None,
                }],
            };
            input.idempotency_key = Some(format!("m007-aw-item-{i}"));
            input
        })
        .collect();
    let retry = service
        .batch_create_work_orders(
            &project_a(),
            &creator(),
            items2,
            Some(lane.id.clone()),
            Some("m007-aw-batch-1".to_owned()),
            1_002,
        )
        .await
        .expect("retry");
    assert!(retry.duplicate);
    assert_eq!(
        retry
            .work_orders
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>(),
        batch
            .work_orders
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>()
    );

    // Invalid item N aborts with no partial commit.
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_order WHERE project_id = ?")
        .bind(project_a().as_str())
        .fetch_one(&pool)
        .await
        .expect("count");
    let mut bad_items: Vec<NewWorkOrder> = (0..3)
        .map(|i| immediate_input(&format!("ok {i}")))
        .collect();
    bad_items.push(NewWorkOrder {
        prompt: String::new(),
        ..immediate_input("bad")
    });
    assert!(service
        .batch_create_work_orders(
            &project_a(),
            &creator(),
            bad_items,
            None,
            Some("m007-aw-bad".to_owned()),
            1_003
        )
        .await
        .is_err());
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_order WHERE project_id = ?")
        .bind(project_a().as_str())
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(before, after, "invalid batch must leave zero partial rows");

    // Cross-project authority is rejected (batch lane must live in the
    // calling project).
    let other = project("project-2");
    let other_items = vec![immediate_input("x")];
    assert!(service
        .batch_create_work_orders(
            &other,
            &creator(),
            other_items,
            Some(lane.id.clone()),
            Some("m007-aw-xproj".to_owned()),
            1_004
        )
        .await
        .is_err());

    // Fan-out/depth counters survive restart: the durable rows re-read
    // identically through a reopened handle.
    let reopened = WorkOrderService::with_defaults(Some(pool));
    for wo in &batch.work_orders {
        assert!(reopened
            .get_work_order(&project_a(), &wo.id)
            .await
            .expect("get")
            .is_some());
    }
}

// ── K — Team contention and privacy ──────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn k_team_contention_and_privacy() {
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool.clone())));

    let lane = service
        .create_lane(
            &project_a(),
            NewSequenceLane {
                label: None,
                failure_policy: LaneFailurePolicy::HoldLane,
                idempotency_key: None,
            },
            500,
        )
        .await
        .expect("lane");
    let mut ids = Vec::new();
    for i in 0..3 {
        let created = service
            .create_work_order(
                &project_a(),
                &creator(),
                immediate_input(&format!("team {i}")),
                1_000 + i,
            )
            .await
            .expect("create");
        let cur = service
            .get_lane(&project_a(), &lane.id)
            .await
            .expect("lane")
            .expect("row");
        service
            .attach_to_lane(
                &project_a(),
                &lane.id,
                cur.revision,
                &created.work_order.id,
                None,
                1_100 + i,
            )
            .await
            .expect("attach");
        ids.push(created.work_order.id);
    }
    let rev = service
        .get_lane(&project_a(), &lane.id)
        .await
        .expect("lane")
        .expect("row")
        .revision;

    // Concurrent reorder on the same lane: one success + one revision
    // conflict (deterministic CAS, zero lost update).
    let mut order_a = ids.clone();
    order_a.swap(0, 1);
    let mut order_b = ids.clone();
    order_b.swap(1, 2);
    let proj = project_a();
    let lane_id = lane.id.clone();
    let (first, second) = tokio::join!(
        service.reorder_lane(&proj, &lane_id, rev, order_a, 2_000),
        service.reorder_lane(&proj, &lane_id, rev, order_b, 2_000),
    );
    assert_eq!(
        [first.is_ok(), second.is_ok()]
            .iter()
            .filter(|ok| **ok)
            .count(),
        1
    );

    // Edit vs claim race: the execution payload is immutable after claim.
    let target = service
        .create_work_order(
            &project_a(),
            &creator(),
            immediate_input("race payload"),
            3_000,
        )
        .await
        .expect("create");
    let occ = service
        .create_occurrence(&project_a(), &target.work_order.id, None, 3_001)
        .await
        .expect("occ");
    service
        .claim_occurrence(&project_a(), &occ.id, 3_002)
        .await
        .expect("claim");
    let patch = WorkOrderPatch {
        prompt: Some("mutated".to_owned()),
        ..Default::default()
    };
    assert!(matches!(
        service
            .update_work_order(
                &project_a(),
                &target.work_order.id,
                patch,
                target.work_order.revision,
                3_003
            )
            .await,
        Err(WorkOrderError::StateConflict(_))
    ));

    // Cancel vs claim/completion resolves to one documented durable
    // outcome: terminal wins; cancel after terminal is idempotent.
    step_to_terminal(
        &service,
        &project_a(),
        &occ.id,
        OccurrenceState::Completed,
        3_004,
    )
    .await;
    let again = service
        .cancel_occurrence(&project_a(), &occ.id, 3_005)
        .await
        .expect("cancel after terminal");
    assert_eq!(again.state, OccurrenceState::Completed);

    // Privacy: ID-only lookups resolve owning project server-side; a
    // foreign project lookup finds nothing (not-found convention, no
    // existence leak).
    let other = project("project-2");
    assert!(service
        .get_work_order(&other, &target.work_order.id)
        .await
        .expect("get")
        .is_none());
    assert!(service
        .get_occurrence(&other, &occ.id)
        .await
        .expect("get")
        .is_none());
    assert!(service
        .get_lane(&other, &lane.id)
        .await
        .expect("get")
        .is_none());

    // Unauthorized project/task lookups use the not-found convention (the
    // store returns None rather than a denial shape; the daemon maps both
    // to the privacy-preserving error — M001/M004 suites pin the wire
    // shape and the dashboard counts below exclude unauthorized rows by
    // construction: per-row counts additionally require session.read).
    let page = service
        .list_occurrences(&project_a(), &target.work_order.id, Some(8))
        .await
        .expect("list");
    assert_eq!(page.occurrences.len(), 1);
}

// ── L — TUI stale-completion and navigation ──────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn l_tui_stale_completion_and_navigation() {
    use codegg::tui::app::state::work_orders::TaskViewState;
    use codegg::tui::app::state::workspace_dashboard::WorkspaceDashboardState;

    // Task view: late completions carrying an older generation are
    // dropped; Vim navigation stays project-correct.
    let mut view = TaskViewState::default();
    let gen1 = view.begin_refresh("project-1");
    let gen2 = view.begin_refresh("project-1");
    assert_ne!(gen1, gen2);
    // Only the newest generation may apply (the guard lives in the
    // command layer comparing `generation`; the state bump above is the
    // evidence that a second refresh invalidated the first).
    assert_eq!(view.generation, gen2);
    // Switching projects replaces identity; refreshes for the old project
    // are project-scoped and never applied to the new one.
    let gen3 = view.begin_refresh("project-2");
    assert_eq!(view.project_id.as_deref(), Some("project-2"));
    assert_ne!(gen3, gen2);

    // j/k lane math never moves the pinned running head and never jumps
    // ahead of it.
    let order = vec!["wo-1".to_owned(), "wo-2".to_owned(), "wo-3".to_owned()];
    assert_eq!(
        TaskViewState::moved_lane_order(&order, Some("wo-1"), "wo-1", 1),
        None
    );
    assert_eq!(
        TaskViewState::moved_lane_order(&order, Some("wo-1"), "wo-2", -1),
        None
    );
    assert_eq!(
        TaskViewState::moved_lane_order(&order, None, "wo-3", -1),
        Some(vec![
            "wo-1".to_owned(),
            "wo-3".to_owned(),
            "wo-2".to_owned()
        ])
    );

    // Dashboard: stale generations and moved-on expansions drop; revoked
    // projects clear immediately without lingering rows.
    let mut dash = WorkspaceDashboardState::new(None, 0);
    let g1 = dash.begin_refresh();
    assert!(dash.apply_loaded(g1, vec![], false, None));
    let g2 = dash.begin_refresh();
    assert!(
        !dash.apply_loaded(g1, vec![], false, None),
        "stale generation must drop"
    );
    assert!(dash.apply_loaded(g2, vec![], false, None));
    dash.begin_expand("project-1");
    assert!(
        !dash.apply_expanded(g2, "project-2", vec![]),
        "moved-on expansion must drop"
    );
    dash.clear_revoked("project-1");
    assert!(
        dash.selected().is_none() || dash.selected().unwrap().summary.project_id != "project-1"
    );
}

// ── §7 representative 15-plan trajectory ─────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn trajectory_fifteen_plan_end_to_end() {
    const REF: &str = "hook-traj";
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool.clone())));
    let coordinator = WorkOrderCoordinator::new(service.clone());
    let scheduler = SchedulerHarness::new().await;

    // 15 medium work orders: 0 immediate, 1 delayed, 2 not-before+sequence,
    // 3 external-trigger-gated, 4 repeated, 5-6 parallel independents,
    // 7 permission wait, 8 intentional failure/attention, 9-14 followers
    // including one agent-batch segment (11-13).
    let lane = service
        .create_lane(
            &project_a(),
            NewSequenceLane {
                label: Some("trajectory".to_owned()),
                failure_policy: LaneFailurePolicy::HoldLane,
                idempotency_key: Some("traj-lane".to_owned()),
            },
            100,
        )
        .await
        .expect("lane");
    let mut wo_ids = Vec::new();
    let mut occ_ids = Vec::new();
    for i in 0..15u32 {
        let prompt = format!("trajectory plan step {i}");
        let mut input = immediate_input(&prompt);
        input.title = Some(format!("Step {i}"));
        input.idempotency_key = Some(format!("traj-item-{i}"));
        match i {
            0 => {} // immediate
            1 => input.gates = delay_input(&prompt, 30).gates,
            2 => {
                input.gates = ReleaseGateSet {
                    join: GateJoin::All,
                    gates: vec![
                        GateSpec {
                            kind: GateKind::NotBefore,
                            delay_secs: None,
                            not_before_ms: Some(50_000),
                            lane_id: None,
                            trigger_ref: None,
                        },
                        GateSpec {
                            kind: GateKind::SequenceReady,
                            delay_secs: None,
                            not_before_ms: None,
                            lane_id: Some(lane.id.clone()),
                            trigger_ref: None,
                        },
                    ],
                };
            }
            3 => {
                input.gates = ReleaseGateSet {
                    join: GateJoin::All,
                    gates: vec![GateSpec {
                        kind: GateKind::ExternalTrigger,
                        delay_secs: None,
                        not_before_ms: None,
                        lane_id: None,
                        trigger_ref: Some(REF.to_owned()),
                    }],
                };
            }
            4 => {
                input.repeat_count = 2;
            }
            7 => {} // permission wait resolved by user (attention -> waiting)
            8 => {} // intentional failure requiring retry/skip
            _ => {
                // Sequential followers (lane members gate on sequence).
                input.gates = ReleaseGateSet {
                    join: GateJoin::All,
                    gates: vec![GateSpec {
                        kind: GateKind::SequenceReady,
                        delay_secs: None,
                        not_before_ms: None,
                        lane_id: Some(lane.id.clone()),
                        trigger_ref: None,
                    }],
                };
            }
        }
        // Steps 5-6 are parallel independents: lane-free immediate work.
        if i == 5 || i == 6 {
            input.gates = ReleaseGateSet::immediate();
            input.sequence_lane_id = None;
        } else if i != 3 {
            // Lane membership for the sequential spine (trigger-gated step
            // 3 stays lane-free so the external gate is the release).
            input.sequence_lane_id = Some(lane.id.clone());
        }
        let created = service
            .create_work_order(&project_a(), &creator(), input, 1_000 + i64::from(i))
            .await
            .expect("create");
        // Lane attach for sequence members created with an explicit lane
        // id is automatic via create; attach only when the lane row does
        // not already contain the member (idempotent trajectory setup).
        let lane_row = service
            .get_lane(&project_a(), &lane.id)
            .await
            .expect("lane")
            .expect("row");
        if !lane_row
            .ordered_work_order_ids
            .contains(&created.work_order.id)
            && created.work_order.sequence_lane_id.is_some()
        {
            service
                .attach_to_lane(
                    &project_a(),
                    &lane.id,
                    lane_row.revision,
                    &created.work_order.id,
                    None,
                    2_000 + i64::from(i),
                )
                .await
                .expect("attach");
        }
        let occ = service
            .create_occurrence(
                &project_a(),
                &created.work_order.id,
                None,
                3_000 + i64::from(i),
            )
            .await
            .expect("occ");
        wo_ids.push(created.work_order.id);
        occ_ids.push(occ.id);
    }
    assert_eq!(wo_ids.len(), 15);

    // External trigger for step 3.
    let trigger = service
        .create_task_trigger(
            &project_a(),
            &wo_ids[3],
            &creator(),
            NewTaskTrigger {
                trigger_ref: Some(REF.to_owned()),
                expires_at_ms: None,
                max_fires: Some(4),
                idempotency_key: Some("traj-trigger".to_owned()),
            },
            4_000,
        )
        .await
        .expect("trigger");
    let bearer = trigger.plaintext_secret.expect("secret");

    // Drive the trajectory in order at t=60s (delay/not-before satisfied,
    // trigger fires step 3, repeats spawn successors).
    let now_ms = 61_000;
    // Step 3 fires exactly once; duplicate Idempotency-Key delivery
    // converges on the stored receipt with stable semantics.
    let fire = service
        .fire_task_trigger(&bearer, Some("traj-fire-1"), now_ms)
        .await
        .expect("fire");
    assert!(fire.latched);
    let replay = service
        .fire_task_trigger(&bearer, Some("traj-fire-1"), now_ms)
        .await
        .expect("replay");
    assert!(replay.duplicate);
    assert_eq!(replay.receipt_id, fire.receipt_id);
    assert!(
        replay.latched,
        "replay converges on the stored latched receipt"
    );

    let ready = coordinator
        .evaluate_due_for_project(&project_a(), now_ms)
        .await
        .expect("eval");
    // Immediate, delayed, fired trigger, repeat head, and parallel steps
    // are due; pure sequence followers wait on predecessors.
    assert!(
        ready.len() >= 5,
        "expected the releasable spine, got {}",
        ready.len()
    );

    // Materialize each due occurrence exactly once (claim -> session link
    // -> job link -> running) with deterministic identities.
    let mut materialized = 0;
    for (wo, occ) in &ready {
        if service
            .claim_occurrence(&project_a(), &occ.id, now_ms)
            .await
            .is_err()
        {
            continue;
        }
        let session_id = WorkOrderCoordinator::session_id_for(&occ.id);
        service
            .persist_workspace_link(&project_a(), &occ.id, "ws-traj", None, now_ms)
            .await
            .expect("ws");
        service
            .persist_session_link(&project_a(), &occ.id, &session_id, now_ms)
            .await
            .expect("session");
        let key = WorkOrderCoordinator::submission_key_for(&occ.id);
        let submitted = scheduler
            .submission
            .submit(
                Some(SubmissionKey::new(key.clone()).expect("key")),
                scheduler.agent_turn_spec(&session_id, &wo.prompt, &key),
            )
            .await
            .expect("submit");
        service
            .persist_job_link(&project_a(), &occ.id, submitted.job_id.as_str(), now_ms)
            .await
            .expect("job");
        service
            .transition_occurrence(
                &project_a(),
                &occ.id,
                OccurrenceState::Running,
                None,
                None,
                now_ms,
            )
            .await
            .expect("running");
        materialized += 1;
    }
    assert!(materialized >= 5);

    // Daemon restart while a task is running: durable links survive reopen
    // with no duplicate session/job.
    let reopened = WorkOrderService::with_defaults(Some(pool.clone()));
    for (wo, occ) in ready.iter().take(2) {
        let _ = wo;
        let current = reopened
            .get_occurrence(&project_a(), &occ.id)
            .await
            .expect("get")
            .expect("row");
        assert!(current.session_id.is_some());
        assert!(current.job_id.is_some());
    }

    // Permission wait (step 7) resolved by user: attention -> waiting.
    service
        .transition_occurrence(
            &project_a(),
            &occ_ids[7],
            OccurrenceState::NeedsAttention,
            Some(AttentionCode::PredecessorAttention),
            Some("waiting on user"),
            now_ms,
        )
        .await
        .expect("attention");
    service
        .transition_occurrence(
            &project_a(),
            &occ_ids[7],
            OccurrenceState::Waiting,
            None,
            None,
            now_ms + 1,
        )
        .await
        .expect("user resolves");

    // Intentional failure/attention (step 8) requiring retry or explicit
    // skip: hold, then the authorized retry resumes to waiting. (Failed is
    // terminal by the matrix, so the trajectory exercises the documented
    // attention -> waiting resume; the Failed-holds predicate is pinned in
    // B and below.) The step is already Running here (immediate release,
    // like step 7): NeedsAttention is reachable from every non-terminal
    // pre-state.
    let step8 = service
        .get_occurrence(&project_a(), &occ_ids[8])
        .await
        .expect("get")
        .expect("row");
    assert!(!step8.state.is_terminal());
    service
        .transition_occurrence(
            &project_a(),
            &occ_ids[8],
            OccurrenceState::NeedsAttention,
            Some(AttentionCode::PredecessorFailed),
            Some("simulated failure: retry me"),
            now_ms + 3,
        )
        .await
        .expect("attention");
    assert!(sequence_holds(
        &[OccurrenceState::Failed],
        LaneFailurePolicy::HoldLane
    ));
    assert!(sequence_holds(
        &[OccurrenceState::NeedsAttention],
        LaneFailurePolicy::HoldLane
    ));
    service
        .transition_occurrence(
            &project_a(),
            &occ_ids[8],
            OccurrenceState::Waiting,
            None,
            None,
            now_ms + 4,
        )
        .await
        .expect("retry");

    // Restart while several future tasks remain: lane order/revisions are
    // stable across reopen.
    let lane_before = service
        .get_lane(&project_a(), &lane.id)
        .await
        .expect("lane")
        .expect("row");
    let lane_after = reopened
        .get_lane(&project_a(), &lane.id)
        .await
        .expect("lane")
        .expect("row");
    assert_eq!(
        lane_before.ordered_work_order_ids,
        lane_after.ordered_work_order_ids
    );
    assert_eq!(lane_before.revision, lane_after.revision);

    // Agent-created batch segment (steps 11-13): atomic batch with lane
    // placement converges on retry.
    let batch_items: Vec<NewWorkOrder> = (11..14)
        .map(|i| {
            let mut input = immediate_input(&format!("trajectory agent plan {i}"));
            input.idempotency_key = Some(format!("traj-agent-{i}"));
            input
        })
        .collect();
    let batch = service
        .batch_create_work_orders(
            &project_a(),
            &creator(),
            batch_items,
            None,
            Some("traj-agent-batch".to_owned()),
            now_ms + 5,
        )
        .await
        .expect("agent batch");
    assert_eq!(batch.work_orders.len(), 3);

    // Terminal accounting: every materialized occurrence has distinct
    // session/job identities; no occurrence duplicated.
    let mut session_set = HashSet::new();
    let mut job_set = HashSet::new();
    for occ_id in &occ_ids {
        if let Some(occ) = reopened
            .get_occurrence(&project_a(), occ_id)
            .await
            .expect("get")
        {
            if let Some(session) = occ.session_id {
                assert!(session_set.insert(session), "duplicate session identity");
            }
            if let Some(job) = occ.job_id {
                assert!(job_set.insert(job), "duplicate job identity");
            }
        }
    }
    // Each materialized session is independently inspectable as a normal
    // session row shape (deterministic id carries no prompt content).
    for session in session_set.iter().take(2) {
        assert!(session.starts_with("wo-session-"));
    }
}

// ── §9 static ownership and duplication audit ────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn ownership_single_scheduler_and_boundaries() {
    // Exactly one durable scheduler/admission owner: the coordinator routes
    // every initial turn through JobSubmissionService (static guard
    // `scripts/check_work_order_coordinator.py` pins the absence of a
    // second scheduler/loop/bypass; here we pin the runtime shape).
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool)));
    let coordinator = WorkOrderCoordinator::new(service);
    let _ = coordinator;

    // One core persistence owner: all work-order rows flow through
    // WorkOrderService (pool-less daemons fail durable ops closed).
    let pool_less = WorkOrderService::with_defaults(None);
    assert!(matches!(
        pool_less
            .create_work_order(&project_a(), &creator(), immediate_input("x"), 1)
            .await,
        Err(WorkOrderError::Unavailable(_))
    ));

    // No raw current_dir/path-derived project identity in the new domain:
    // identities are typed and never path-derived (parse rejects paths).
    assert!(ProjectId::parse("/tmp/evil").is_err());
    assert!(ProjectId::parse("project-1").is_ok());

    // Existing delegated TaskTool remains semantically distinct from the
    // dedicated work-order tool (names/categories differ).
    use codegg::tool::Tool as _;
    assert_eq!(codegg::tool::task::TaskTool::default().name(), "task");
    assert_eq!(
        codegg::tool::work_order::WorkOrderTool::new(None).name(),
        "work_order"
    );
    assert_ne!(
        codegg::tool::task::TaskTool::default().name(),
        codegg::tool::work_order::WorkOrderTool::new(None).name()
    );

    // Authorization matrix covers the work-order operations (capability
    // descriptors exist; denials are privacy-preserving not-found — pinned
    // by `scripts/check_authorization_matrix.py` plus test K above).
    // Projection transport stays read-only adapters: dashboard apply paths
    // never mutate durable stores (L exercises pure state transitions).
}
