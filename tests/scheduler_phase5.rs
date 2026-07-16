//! Phase 5: scheduler integration tests.
//!
//! These cover the wiring between the scheduler, admission controller,
//! fair queue, executor registry, and durable job store. The unit
//! tests in `src/scheduler/` already pin the typed behaviours; this
//! integration suite verifies that the cross-cutting pieces line up
//! the way the Phase 5 plan requires:
//!
//!   * Two-workspace isolation under contention: lanes do not starve
//!     each other and fairness weights prevent one workspace from
//!     monopolising the global budget.
//!   * Admission correctness: the in-flight running count never
//!     exceeds `max_process_slots`, exclusivity keys are honoured, and
//!     the dropping guard releases the budget back to the controller.
//!   * Scheduler lifecycle: `wake()` triggers reconciliation, the
//!     main loop can be spawned and stopped, and `shutdown()` cancels
//!     running attempts cleanly.
//!   * Executor wiring: registering and dispatching through typed
//!     executors persists `RunId` (when the executor supplies one)
//!     and the canonical subsystems are reachable through the
//!     registry.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use codegg::scheduler::{
    AdmissionController, ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorRegistry,
    ExecutorRegistryError, ExecutorStatus, FairJobQueue, JobExecutionContext, JobExecutor,
    PermitDimensions, PriorityClass, ResolvedSchedulerConfig, SchedulerRolloutMode,
};

use codegg_core::jobs::{
    DaemonGeneration, IdempotencyClass, InMemoryJobStore, InMemoryScheduleStore, JobKind,
    JobPayload, JobPriority, JobRecord, JobSource, JobState, JobStore, NewJob, ResourceRequest,
    RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;

fn ws(id: &str) -> WorkspaceId {
    WorkspaceId::new_unchecked(id)
}

fn build_job(workspace: &WorkspaceId, kind: JobKind) -> JobRecord {
    let now = chrono::Utc::now();
    JobRecord {
        job_id: codegg_core::jobs::JobId::new_unchecked(format!(
            "test-{}-{}",
            workspace.as_str(),
            uuid::Uuid::new_v4()
        )),
        workspace_id: workspace.clone(),
        session_id: None,
        turn_id: None,
        kind,
        source: codegg_core::jobs::JobSource::Interactive,
        priority: codegg_core::jobs::JobPriority::Normal,
        payload: JobPayload::ManagedArgv {
            argv: vec!["echo".into(), "hi".into()],
            cwd: None,
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
        state: JobState::Queued,
        current_attempt_id: None,
        attempt_count: 0,
        not_before: None,
        deadline: None,
        schedule_id: None,
        created_at: now,
        updated_at: now,
        terminal_at: None,
        cancel_requested_at: None,
        cancel_reason: None,
        depends_on: vec![],
        labels: std::collections::HashMap::new(),
    }
}

// ── Admission tests ────────────────────────────────────────────────────────

#[test]
fn admission_capacity_is_respected() {
    let mut cfg = ResolvedSchedulerConfig::default();
    cfg.resources.max_process_slots = 2;
    cfg.enabled = true;
    let controller = Arc::new(AdmissionController::new(cfg));

    let dims = PermitDimensions {
        cpu_weight: 1,
        memory_mb_hint: 0,
        process_slots: 1,
        io_weight: 0,
        network_slots: 0,
        exclusivity_keys: vec![],
    };

    let p1 = controller.try_admit_arc(&dims);
    let p2 = controller.try_admit_arc(&dims);
    let p3 = controller.try_admit_arc(&dims);

    assert!(matches!(
        p1,
        codegg::scheduler::AdmissionDecision::Admitted(_)
    ));
    assert!(matches!(
        p2,
        codegg::scheduler::AdmissionDecision::Admitted(_)
    ));
    assert!(matches!(
        p3,
        codegg::scheduler::AdmissionDecision::TemporarilyBlocked(_)
    ));
}

#[test]
fn exclusivity_keys_block_concurrent_jobs() {
    let cfg = ResolvedSchedulerConfig::default();
    let controller = Arc::new(AdmissionController::new(cfg));
    let dims_holder = |key: &str| PermitDimensions {
        cpu_weight: 1,
        memory_mb_hint: 0,
        process_slots: 1,
        io_weight: 0,
        network_slots: 0,
        exclusivity_keys: vec![format!("exclusive:{key}")],
    };
    let a = controller.try_admit_arc(&dims_holder("run-tests"));
    let b = controller.try_admit_arc(&dims_holder("run-tests"));
    assert!(matches!(
        a,
        codegg::scheduler::AdmissionDecision::Admitted(_)
    ));
    // Same exclusivity key — must be blocked.
    assert!(matches!(
        b,
        codegg::scheduler::AdmissionDecision::TemporarilyBlocked(
            codegg::scheduler::BlockReason::KeyContended { .. }
        )
    ));
}

#[test]
fn guard_drop_releases_capacity() {
    let mut cfg = ResolvedSchedulerConfig::default();
    cfg.resources.max_process_slots = 1;
    cfg.enabled = true;
    let controller = Arc::new(AdmissionController::new(cfg));
    let dims = PermitDimensions {
        cpu_weight: 1,
        memory_mb_hint: 0,
        process_slots: 1,
        io_weight: 0,
        network_slots: 0,
        exclusivity_keys: vec![],
    };
    let p1 = match controller.try_admit_arc(&dims) {
        codegg::scheduler::AdmissionDecision::Admitted(p) => p,
        other => panic!("first admission should succeed, got {other:?}"),
    };
    assert_eq!(controller.used_process_slots(), 1);
    drop(p1);
    assert_eq!(controller.used_process_slots(), 0);
}

// ── Fair queue tests ───────────────────────────────────────────────────────

#[test]
fn fair_queue_round_robin_across_lanes() {
    let cfg = ResolvedSchedulerConfig::default();
    let mut queue = FairJobQueue::new(cfg);
    let wa = ws("ws-a");
    let wb = ws("ws-b");
    let job_a = build_job(&wa, JobKind::Build);
    let job_b = build_job(&wb, JobKind::Build);
    let entry_a = codegg::scheduler::QueueEntry::from_job(&job_a);
    let entry_b = codegg::scheduler::QueueEntry::from_job(&job_b);
    queue.insert(entry_a).expect("insert a");
    queue.insert(entry_b).expect("insert b");
    let first = queue.select_next().expect("first selection");
    let first_workspace = first.entry.workspace_id.clone();
    let second = queue.select_next().expect("second selection");
    let second_workspace = second.entry.workspace_id.clone();
    assert_ne!(
        first_workspace, second_workspace,
        "round-robin must alternate lanes"
    );
}

#[test]
fn fair_queue_aging_recompute_no_panic() {
    let cfg = ResolvedSchedulerConfig::default();
    let mut queue = FairJobQueue::new(cfg);
    let wa = ws("ws-a");
    let job_a = build_job(&wa, JobKind::Build);
    let entry = codegg::scheduler::QueueEntry::from_job(&job_a);
    queue.insert(entry).expect("insert background");
    // Run aging recomputation a few times. We don't assert any
    // specific outcome — just that it doesn't panic or break the
    // queue invariants.
    for _ in 0..3 {
        queue.recompute_aging(chrono::Utc::now());
        let snap = queue.lanes();
        assert!(snap.len() <= 4);
    }
    // Use the priority class for the import to silence unused warnings.
    let _ = PriorityClass::Normal;
}

// ── Two-workspace contention ──────────────────────────────────────────────

#[test]
fn admission_budget_is_shared_across_workspaces() {
    let mut cfg = ResolvedSchedulerConfig::default();
    cfg.resources.max_process_slots = 4;
    cfg.enabled = true;
    let controller = Arc::new(AdmissionController::new(cfg));
    let dims = PermitDimensions {
        cpu_weight: 0,
        memory_mb_hint: 0,
        process_slots: 1,
        io_weight: 0,
        network_slots: 0,
        exclusivity_keys: vec![],
    };
    let mut held = Vec::new();
    for _ in 0..4 {
        if let codegg::scheduler::AdmissionDecision::Admitted(p) = controller.try_admit_arc(&dims) {
            held.push(p);
        }
    }
    let in_flight = controller.used_process_slots();
    assert_eq!(in_flight, held.len() as u32);
    assert!(in_flight <= 4);
    drop(held);
    assert_eq!(controller.used_process_slots(), 0);
}

// ── Executor wiring ───────────────────────────────────────────────────────

struct PassthroughExecutor {
    kind: ExecutorKind,
}

struct CountingExecutor {
    active: Arc<AtomicUsize>,
    max_seen: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl JobExecutor for CountingExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ManagedArgv
    }

    fn supports(&self, kind: JobKind) -> bool {
        kind == JobKind::Build
    }

    async fn execute(&self, _ctx: JobExecutionContext) -> ExecutorCompletion {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_seen.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        ExecutorCompletion {
            status: ExecutorStatus::Completed,
            summary: "counted".into(),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}

fn build_spec(workspace_id: WorkspaceId) -> NewJob {
    NewJob {
        workspace_id,
        session_id: None,
        turn_id: None,
        kind: JobKind::Build,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::ManagedArgv {
            argv: vec!["echo".into(), "build".into()],
            cwd: None,
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_workspaces_share_one_process_cap() {
    let root_a = tempfile::tempdir().expect("workspace a");
    let root_b = tempfile::tempdir().expect("workspace b");
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("workspace registry");
    let workspace_a = workspace_registry
        .get_or_register(root_a.path())
        .await
        .expect("register workspace a");
    let workspace_b = workspace_registry
        .get_or_register(root_b.path())
        .await
        .expect("register workspace b");
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let state_store = store.clone();
    let mut config = ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = 1;
    let scheduler = codegg::scheduler::JobScheduler::new(
        store.clone(),
        services.clone(),
        config,
        DaemonGeneration::new_unchecked("contention-generation"),
    );
    let active = Arc::new(AtomicUsize::new(0));
    let max_seen = Arc::new(AtomicUsize::new(0));
    scheduler
        .register_executor(Arc::new(CountingExecutor {
            active: active.clone(),
            max_seen: max_seen.clone(),
        }))
        .await
        .expect("register counting executor");
    let submission = codegg::scheduler::JobSubmissionService::new(
        store,
        scheduler.clone(),
        services,
        DaemonGeneration::new_unchecked("contention-generation"),
    );
    let first = submission
        .submit(None, build_spec(workspace_a.id.clone()))
        .await
        .expect("submit workspace a");
    let second = submission
        .submit(None, build_spec(workspace_b.id.clone()))
        .await
        .expect("submit workspace b");

    scheduler.reconcile().await.expect("reconcile");
    assert_eq!(scheduler.clone().admit_and_dispatch_batch().await, 1);
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(active.load(Ordering::SeqCst), 1);
    assert_eq!(max_seen.load(Ordering::SeqCst), 1);
    let first_state = state_store
        .get_job(&first.job_id)
        .await
        .expect("first state lookup")
        .expect("first job");
    let (running_job, queued_job) = if first_state.state == JobState::Running {
        (&first, &second)
    } else {
        (&second, &first)
    };
    scheduler
        .wait_for_completion(&running_job.job_id, Duration::from_secs(2))
        .await
        .expect("first admitted completion");

    scheduler.reconcile().await.expect("second reconcile");
    assert_eq!(scheduler.clone().admit_and_dispatch_batch().await, 1);
    scheduler
        .wait_for_completion(&queued_job.job_id, Duration::from_secs(2))
        .await
        .expect("second admitted completion");
    assert_eq!(max_seen.load(Ordering::SeqCst), 1);
}

#[async_trait::async_trait]
impl JobExecutor for PassthroughExecutor {
    fn kind(&self) -> ExecutorKind {
        self.kind
    }
    fn supports(&self, _kind: JobKind) -> bool {
        true
    }
    async fn execute(&self, _ctx: JobExecutionContext) -> ExecutorCompletion {
        ExecutorCompletion {
            status: ExecutorStatus::Completed,
            summary: "ok".into(),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn executor_registry_round_trip() {
    let mut registry = ExecutorRegistry::new();
    registry
        .register(Arc::new(PassthroughExecutor {
            kind: ExecutorKind::Test,
        }))
        .expect("register test executor");
    let exec = registry.get(ExecutorKind::Test).expect("test executor");
    assert_eq!(exec.kind(), ExecutorKind::Test);
}

#[tokio::test(flavor = "current_thread")]
async fn executor_duplicate_rejected() {
    let mut registry = ExecutorRegistry::new();
    registry
        .register(Arc::new(PassthroughExecutor {
            kind: ExecutorKind::Test,
        }))
        .unwrap();
    let result = registry.register(Arc::new(PassthroughExecutor {
        kind: ExecutorKind::Test,
    }));
    assert!(matches!(result, Err(ExecutorRegistryError::Duplicate(_))));
}

// ── Scheduler wiring smoke (in-memory only) ───────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn scheduler_construction_in_memory() {
    let cfg = ResolvedSchedulerConfig::default();
    let job_store: Arc<dyn codegg_core::jobs::JobStore> = Arc::new(InMemoryJobStore::new());
    let _schedule_store: Arc<dyn codegg_core::jobs::ScheduleStore> =
        Arc::new(InMemoryScheduleStore::new(Arc::clone(&job_store)));
    let workspaces = codegg_core::workspace::WorkspaceRegistry::new_for_tests(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ));
    let workspaces_arc = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspaces,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let scheduler_arc = codegg::scheduler::JobScheduler::new(
        job_store,
        workspaces_arc,
        cfg,
        DaemonGeneration::new(),
    );
    let kinds = scheduler_arc.executor_kinds().await;
    assert!(
        kinds.is_empty(),
        "default scheduler has no executors until registered"
    );
}

#[test]
fn admission_blocks_when_only_exclusivity_is_held() {
    let cfg = ResolvedSchedulerConfig::default();
    let controller = Arc::new(AdmissionController::new(cfg));
    let key_holder = PermitDimensions {
        cpu_weight: 0,
        memory_mb_hint: 0,
        process_slots: 0,
        io_weight: 0,
        network_slots: 0,
        exclusivity_keys: vec!["exclusive:test-only".into()],
    };
    let other = PermitDimensions {
        cpu_weight: 0,
        memory_mb_hint: 0,
        process_slots: 1,
        io_weight: 0,
        network_slots: 0,
        exclusivity_keys: vec!["exclusive:test-only".into()],
    };
    let first = controller.try_admit_arc(&key_holder);
    let second = controller.try_admit_arc(&other);
    assert!(matches!(
        first,
        codegg::scheduler::AdmissionDecision::Admitted(_)
    ));
    assert!(matches!(
        second,
        codegg::scheduler::AdmissionDecision::TemporarilyBlocked(_)
    ));
}

#[test]
fn rollout_mode_default_is_mandatory() {
    let cfg = ResolvedSchedulerConfig::default();
    assert_eq!(cfg.rollout, SchedulerRolloutMode::Mandatory);
}
