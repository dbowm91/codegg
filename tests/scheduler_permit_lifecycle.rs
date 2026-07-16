use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use codegg::scheduler::admission::UnschedulableReason;
use codegg::scheduler::permit::permit_from_request;
use codegg::scheduler::submission::JobSubmissionService;
use codegg::scheduler::{
    AdmissionController, AdmissionDecision, ExecutorCompletion, ExecutorKind, ExecutorMetrics,
    JobExecutionContext, JobExecutor, ResolvedSchedulerConfig,
};

use codegg_core::jobs::{
    AttemptState, DaemonGeneration, IdempotencyClass, InMemoryJobStore, JobKind, JobPayload,
    JobPriority, JobSource, JobState, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceId, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

fn build_test_spec(workspace_id: WorkspaceId) -> NewJob {
    NewJob {
        workspace_id,
        session_id: None,
        turn_id: None,
        kind: JobKind::Test,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Test {
            command: "echo ok".into(),
            argv: vec!["echo".into(), "ok".into()],
            cwd: Some("/tmp".into()),
            scope: None,
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

fn build_build_spec(workspace_id: WorkspaceId) -> NewJob {
    NewJob {
        workspace_id,
        session_id: None,
        turn_id: None,
        kind: JobKind::Build,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::ManagedArgv {
            argv: vec!["echo".into(), "build".into()],
            cwd: Some("/tmp".into()),
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

async fn make_scheduler_env(
    max_process_slots: u32,
) -> (
    Arc<codegg::scheduler::JobScheduler>,
    Arc<JobSubmissionService>,
    WorkspaceId,
) {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register ws");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let mut config = ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = max_process_slots;
    let scheduler = codegg::scheduler::JobScheduler::new(
        store.clone(),
        services.clone(),
        config,
        DaemonGeneration::new(),
    );
    scheduler
        .register_executor(Arc::new(ImmediateTestExecutor))
        .await
        .expect("register executor");
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services,
        DaemonGeneration::new(),
    );
    (scheduler, submission, workspace.id.clone())
}

// ── Immediate executor for roundtrip tests ──────────────────────────────────

struct ImmediateTestExecutor;

#[async_trait::async_trait]
impl JobExecutor for ImmediateTestExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Test
    }
    fn supports(&self, kind: JobKind) -> bool {
        matches!(kind, JobKind::Test)
    }
    fn validate(
        &self,
        _job: &codegg_core::jobs::JobRecord,
    ) -> Result<(), codegg::scheduler::executor::ExecutorValidationError> {
        Ok(())
    }
    async fn execute(&self, _ctx: JobExecutionContext) -> ExecutorCompletion {
        ExecutorCompletion {
            status: codegg::scheduler::executor::ExecutorStatus::Completed,
            summary: "immediate".into(),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}

// ── C1: Conservation ───────────────────────────────────────────────────────

#[test]
fn permit_counters_conserved_under_concurrent_admissions() {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    for round in 0..3 {
        let mut guards = vec![];
        for _ in 0..4 {
            let dims = permit_from_request(1, 0, 1, 1, 0, vec![]);
            match controller.try_admit_arc(&dims) {
                AdmissionDecision::Admitted(g) => guards.push(g),
                other => panic!("round {round}: expected admit, got {other:?}"),
            }
        }
        let snap = controller.snapshot();
        assert_eq!(snap.used_process, 4, "round {round}");
        assert_eq!(snap.used_cpu, 4, "round {round}");
        drop(guards);
        let snap = controller.snapshot();
        assert_eq!(snap.used_process, 0, "round {round} after drop");
        assert_eq!(snap.used_cpu, 0, "round {round} after drop");
    }
}

#[test]
fn counters_never_exceed_capacity() {
    let mut cfg = ResolvedSchedulerConfig::default();
    cfg.resources.max_process_slots = 2;
    cfg.resources.max_cpu_weight = 4;
    let controller = Arc::new(AdmissionController::new(cfg));
    let dims = permit_from_request(2, 0, 1, 0, 0, vec![]);
    let g1 = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected admit, got {other:?}"),
    };
    let g2 = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected admit, got {other:?}"),
    };
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 2);
    assert_eq!(snap.used_cpu, 4);
    let blocked = controller.try_admit_arc(&dims);
    assert!(matches!(blocked, AdmissionDecision::TemporarilyBlocked(_)));
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 2);
    assert_eq!(snap.used_cpu, 4);
    drop(g1);
    drop(g2);
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 0);
    assert_eq!(snap.used_cpu, 0);
}

#[test]
fn saturating_release_does_not_underflow() {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let dims = permit_from_request(1, 0, 1, 0, 0, vec![]);
    let g = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected admit, got {other:?}"),
    };
    drop(g);
    assert_eq!(controller.used_process_slots(), 0);
    controller.release(permit_from_request(1, 0, 1, 0, 0, vec![]));
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 0, "saturating_sub must not underflow");
}

// ── C2: Terminal paths ─────────────────────────────────────────────────────

#[test]
fn impossible_request_does_not_reserve() {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let dims = permit_from_request(0, 0, 100, 0, 0, vec![]);
    match controller.try_admit_arc(&dims) {
        AdmissionDecision::Impossible(UnschedulableReason::ProcessSlotsExceedBudget { .. }) => {}
        other => panic!("expected Impossible, got {other:?}"),
    }
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 0);
    assert_eq!(snap.impossible, 1);
}

#[test]
fn blocked_request_does_not_reserve() {
    let mut cfg = ResolvedSchedulerConfig::default();
    cfg.resources.max_process_slots = 1;
    let controller = Arc::new(AdmissionController::new(cfg));
    let dims = permit_from_request(0, 0, 1, 0, 0, vec![]);
    let _g = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected first admit, got {other:?}"),
    };
    let dims2 = permit_from_request(0, 0, 1, 0, 0, vec![]);
    match controller.try_admit_arc(&dims2) {
        AdmissionDecision::TemporarilyBlocked(_) => {}
        other => panic!("expected blocked, got {other:?}"),
    }
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 1);
    assert_eq!(snap.rejected, 1);
}

#[test]
fn excluded_key_blocked_does_not_reserve() {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let dims = permit_from_request(0, 0, 1, 0, 0, vec!["exclusive:workspace-mutation".into()]);
    let _g = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected admit, got {other:?}"),
    };
    let dims2 = permit_from_request(0, 0, 1, 0, 0, vec!["exclusive:workspace-mutation".into()]);
    match controller.try_admit_arc(&dims2) {
        AdmissionDecision::TemporarilyBlocked(_) => {}
        other => panic!("expected key blocked, got {other:?}"),
    }
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 1);
    assert_eq!(snap.rejected, 1);
}

#[test]
fn impossible_multiple_dimensions_does_not_reserve() {
    let mut cfg = ResolvedSchedulerConfig::default();
    cfg.resources.max_cpu_weight = 4;
    let controller = Arc::new(AdmissionController::new(cfg));
    let dims = permit_from_request(5, 0, 1, 0, 0, vec![]);
    match controller.try_admit_arc(&dims) {
        AdmissionDecision::Impossible(UnschedulableReason::CpuWeightExceedsBudget { .. }) => {}
        other => panic!("expected Impossible for CPU, got {other:?}"),
    }
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 0);
    assert_eq!(snap.used_cpu, 0);
}

// ── C3: Guard ownership ────────────────────────────────────────────────────

#[test]
fn exclusivity_key_released_after_drop() {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let dims = permit_from_request(1, 0, 1, 1, 0, vec!["exclusive:workspace-mutation".into()]);
    let g = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected admit, got {other:?}"),
    };
    let snap = controller.snapshot();
    assert_eq!(snap.held_keys.get("workspace-mutation").copied(), Some(1));
    drop(g);
    let snap = controller.snapshot();
    assert!(
        !snap.held_keys.contains_key("workspace-mutation"),
        "key must be released after drop"
    );
}

#[test]
fn exclusivity_key_released_on_panic() {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let dims = permit_from_request(1, 0, 1, 1, 0, vec!["exclusive:mut".into()]);
    let g = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected admit, got {other:?}"),
    };
    let snap = controller.snapshot();
    assert_eq!(snap.held_keys.get("mut").copied(), Some(1));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(g);
    }));
    assert!(result.is_ok());
    let snap = controller.snapshot();
    assert!(
        snap.held_keys.is_empty(),
        "key must be released after panic-drop"
    );
}

#[test]
fn exclusivity_key_released_on_early_return() {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let dims = permit_from_request(1, 0, 1, 1, 0, vec!["exclusive:early".into()]);
    let g = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected admit, got {other:?}"),
    };
    let snap = controller.snapshot();
    assert_eq!(snap.held_keys.get("early").copied(), Some(1));
    let error_occurred = true;
    if error_occurred {
        drop(g);
        let snap = controller.snapshot();
        assert!(snap.held_keys.is_empty(), "early return must release key");
        return;
    }
    unreachable!("should have returned early");
}

#[test]
fn detach_does_not_release() {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let dims = permit_from_request(1, 0, 1, 1, 0, vec![]);
    let g = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected admit, got {other:?}"),
    };
    let detached_dims = g.detach();
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 1, "detach must not release");
    controller.release(detached_dims);
    let snap = controller.snapshot();
    assert_eq!(snap.used_process, 0);
}

#[test]
fn drop_without_controller_is_noop() {
    let dims = permit_from_request(1, 0, 1, 0, 0, vec![]);
    let g = codegg::scheduler::ResourcePermitGuard::new_orphan(dims);
    assert!(!g.is_controller_bound());
    drop(g);
}

#[test]
fn multiple_keys_released_independently() {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let dims = permit_from_request(
        0,
        0,
        2,
        0,
        0,
        vec!["exclusive:a".into(), "exclusive:b".into()],
    );
    let g = match controller.try_admit_arc(&dims) {
        AdmissionDecision::Admitted(g) => g,
        other => panic!("expected admit, got {other:?}"),
    };
    let snap = controller.snapshot();
    assert_eq!(snap.held_keys.len(), 2);
    drop(g);
    let snap = controller.snapshot();
    assert!(snap.held_keys.is_empty());
}

// ── C4: Attempt/executor consistency ───────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn full_scheduler_roundtrip_releases_permits() {
    let (scheduler, submission, workspace_id) = make_scheduler_env(1).await;
    let handle = scheduler.spawn_run();
    let _submitted = submission
        .submit(None, build_test_spec(workspace_id))
        .await
        .expect("submit");
    scheduler
        .wait_for_completion(&_submitted.job_id, Duration::from_secs(5))
        .await
        .expect("completion");
    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = handle.await;
    let snap = scheduler.admission().snapshot();
    assert_eq!(
        snap.used_process, 0,
        "permit must be released after completion"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_submissions_do_not_double_reserve() {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register ws");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let mut config = ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = 1;
    config.resources.max_cpu_weight = 2;
    let scheduler = codegg::scheduler::JobScheduler::new(
        store.clone(),
        services.clone(),
        config,
        DaemonGeneration::new(),
    );
    scheduler
        .register_executor(Arc::new(ImmediateTestExecutor))
        .await
        .expect("register");
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services,
        DaemonGeneration::new(),
    );

    let max_seen = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];
    for _ in 0..4 {
        let sub = submission.clone();
        let ws_id = workspace.id.clone();
        let m = max_seen.clone();
        let a = active.clone();
        handles.push(tokio::spawn(async move {
            let spec = build_build_spec(ws_id);
            sub.submit(None, spec).await.expect("submit");
            let active_now = a.fetch_add(1, Ordering::SeqCst) + 1;
            m.fetch_max(active_now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            a.fetch_sub(1, Ordering::SeqCst);
        }));
    }

    for h in handles {
        h.await.expect("task join");
    }
    let snap = scheduler.admission().snapshot();
    assert_eq!(snap.used_process, 0, "all permits released");
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_before_admission_does_not_leak() {
    let (scheduler, submission, workspace_id) = make_scheduler_env(1).await;
    let handle = scheduler.spawn_run();
    let submitted = submission
        .submit(None, build_test_spec(workspace_id))
        .await
        .expect("submit");
    scheduler
        .request_cancel(&submitted.job_id, "test-cancel")
        .await
        .expect("cancel");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let job = store_get_job(scheduler.store(), &submitted.job_id).await;
    assert!(
        job.state == JobState::Cancelled || job.state.is_terminal(),
        "job must be cancelled or terminal"
    );
    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = handle.await;
    let snap = scheduler.admission().snapshot();
    assert_eq!(snap.used_process, 0, "no leak after cancel");
}

#[tokio::test(flavor = "current_thread")]
async fn attempt_executor_consistency() {
    let (scheduler, submission, workspace_id) = make_scheduler_env(1).await;
    let handle = scheduler.spawn_run();
    let submitted = submission
        .submit(None, build_test_spec(workspace_id))
        .await
        .expect("submit");
    scheduler
        .wait_for_completion(&submitted.job_id, Duration::from_secs(5))
        .await
        .expect("completion");
    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = handle.await;
    let attempts = scheduler
        .store()
        .list_attempts(&submitted.job_id)
        .await
        .expect("list attempts");
    assert_eq!(attempts.len(), 1, "exactly one attempt");
    let attempt = &attempts[0];
    assert!(
        attempt.executor.is_some(),
        "executor must be set before execution"
    );
    assert!(
        matches!(
            attempt.state,
            AttemptState::Completed
                | AttemptState::Failed
                | AttemptState::Cancelled
                | AttemptState::TimedOut
                | AttemptState::Interrupted
        ),
        "attempt must be terminal"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_releases_running_permits() {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register ws");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let mut config = ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = 1;
    let scheduler = codegg::scheduler::JobScheduler::new(
        store.clone(),
        services.clone(),
        config,
        DaemonGeneration::new(),
    );
    scheduler
        .register_executor(Arc::new(SlowTestExecutor))
        .await
        .expect("register");
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services,
        DaemonGeneration::new(),
    );

    let _handle = scheduler.spawn_run();
    let submitted = submission
        .submit(None, build_test_spec(workspace.id.clone()))
        .await
        .expect("submit");
    tokio::time::sleep(Duration::from_millis(200)).await;
    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::StopAcceptingAndCancelQueued)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(Some(job)) = scheduler.store().get_job(&submitted.job_id).await {
                if job.state.is_terminal() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    let snap = scheduler.admission().snapshot();
    assert_eq!(snap.used_process, 0, "shutdown must release all permits");
}

async fn store_get_job(
    store: &Arc<dyn JobStore>,
    job_id: &codegg_core::jobs::JobId,
) -> codegg_core::jobs::JobRecord {
    store
        .get_job(job_id)
        .await
        .expect("get job")
        .expect("job exists")
}

// ── Slow executor for shutdown tests ───────────────────────────────────────

struct SlowTestExecutor;

#[async_trait::async_trait]
impl JobExecutor for SlowTestExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Test
    }
    fn supports(&self, kind: JobKind) -> bool {
        matches!(kind, JobKind::Test)
    }
    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => {}
            _ = ctx.cancellation.cancelled() => {
                return ExecutorCompletion {
                    status: codegg::scheduler::executor::ExecutorStatus::Cancelled,
                    summary: "cancelled".into(),
                    run_id: None,
                    metrics: ExecutorMetrics::default(),
                };
            }
        }
        ExecutorCompletion {
            status: codegg::scheduler::executor::ExecutorStatus::Completed,
            summary: "slow".into(),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}
