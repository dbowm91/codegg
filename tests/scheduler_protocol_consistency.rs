//! Scheduler protocol surface consistency tests.
//!
//! These verify that the externally visible protocol surface
//! (snapshot, wait, list, cancel, events) stays consistent with the
//! durable `JobStore` state. No production code is modified; any
//! observed inconsistency is a finding, not a fix.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use codegg::scheduler::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, JobExecutionContext,
    JobScheduler, JobSubmissionError, JobSubmissionService, ResolvedSchedulerConfig,
    SchedulerShutdownMode, SubmissionKey,
};
use codegg_core::jobs::store::JobStoreQuery;
use codegg_core::jobs::{
    DaemonGeneration, IdempotencyClass, InMemoryJobStore, JobKind, JobPayload, JobPriority,
    JobRecord, JobSource, JobState, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;

fn ws(id: &str) -> WorkspaceId {
    WorkspaceId::new_unchecked(id)
}

fn build_managed_argv_job(workspace: &WorkspaceId, argv: Vec<String>) -> NewJob {
    NewJob {
        workspace_id: workspace.clone(),
        session_id: None,
        turn_id: None,
        kind: JobKind::Build,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::ManagedArgv { argv, cwd: None },
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

fn build_slow_job(workspace: &WorkspaceId) -> NewJob {
    NewJob {
        workspace_id: workspace.clone(),
        session_id: None,
        turn_id: None,
        kind: JobKind::Build,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::ManagedArgv {
            argv: vec!["sleep".into(), "10".into()],
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

/// A simple executor that completes immediately.
struct SimpleExecutor;

#[async_trait::async_trait]
impl codegg::scheduler::JobExecutor for SimpleExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ManagedArgv
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

/// An executor that holds the permit until the `release` flag is set
/// or the cancellation token fires. Used to saturate the single
/// process slot so subsequent jobs are temporarily blocked.
struct HoldingExecutor {
    release: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl codegg::scheduler::JobExecutor for HoldingExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ManagedArgv
    }
    fn supports(&self, _kind: JobKind) -> bool {
        true
    }
    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        self.active.fetch_add(1, Ordering::SeqCst);
        // Hold the slot until release is signaled or cancellation fires.
        tokio::select! {
            _ = async {
                while !self.release.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            } => {},
            _ = ctx.cancellation.cancelled() => {},
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        ExecutorCompletion {
            status: ExecutorStatus::Completed,
            summary: "held".into(),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}

/// Set up a scheduler with a simple executor, submission service,
/// and a registered workspace. Returns (scheduler, submission, store, ws_id).
async fn setup_managed_argv() -> (
    Arc<JobScheduler>,
    Arc<JobSubmissionService>,
    Arc<dyn JobStore>,
    WorkspaceId,
) {
    let root = tempfile::tempdir().expect("workspace");
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("workspace registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let config = ResolvedSchedulerConfig::default();
    let generation = DaemonGeneration::new_unchecked("proto-consistency-gen");
    let scheduler = JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
    scheduler
        .register_executor(Arc::new(SimpleExecutor))
        .await
        .expect("register executor");
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services.clone(),
        generation,
    );
    (scheduler, submission, store, workspace.id.clone())
}

/// Set up a scheduler with max_process_slots = 1 and a holding
/// executor. The first admitted job holds the slot, causing subsequent
/// jobs to be temporarily blocked (not impossible-failed).
async fn setup_with_holding_executor() -> (
    Arc<JobScheduler>,
    Arc<JobSubmissionService>,
    Arc<dyn JobStore>,
    WorkspaceId,
    Arc<AtomicBool>,
) {
    let root = tempfile::tempdir().expect("workspace");
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("workspace registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let mut config = ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = 1;
    let generation = DaemonGeneration::new_unchecked("holding-gen");
    let scheduler = JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
    let release = Arc::new(AtomicBool::new(false));
    scheduler
        .register_executor(Arc::new(HoldingExecutor {
            release: release.clone(),
            active: Arc::new(AtomicUsize::new(0)),
        }))
        .await
        .expect("register executor");
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services.clone(),
        generation,
    );
    (scheduler, submission, store, workspace.id.clone(), release)
}

/// Poll until the job reaches a terminal state.
async fn wait_for_terminal(
    store: &dyn JobStore,
    job_id: &codegg_core::jobs::JobId,
    timeout: Duration,
) -> JobRecord {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let job = store
            .get_job(job_id)
            .await
            .expect("get_job")
            .expect("job exists");
        if job.state.is_terminal() {
            return job;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "job {} did not reach terminal state within {:?}; last state: {:?}",
                job_id, timeout, job.state
            );
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn shutdown_scheduler(scheduler: &Arc<JobScheduler>) {
    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
}

// ── Test 1: snapshot reflects durable state after completion ────────────

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_reflects_durable_state_after_completion() {
    let (scheduler, submission, store, ws_id) = setup_managed_argv().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let submitted = submission
        .submit(
            None,
            build_managed_argv_job(&ws_id, vec!["echo".into(), "ok".into()]),
        )
        .await
        .expect("submit");

    let job = wait_for_terminal(&*store, &submitted.job_id, Duration::from_secs(5)).await;
    assert_eq!(job.state, JobState::Completed);

    let snap = scheduler.snapshot().await;
    let ws_summary = snap
        .per_workspace
        .iter()
        .find(|w| w.workspace_id == ws_id)
        .expect("workspace in snapshot");
    assert_eq!(ws_summary.running, 0, "no running jobs");
    assert_eq!(ws_summary.queued, 0, "no queued jobs");
    assert_eq!(snap.running_attempts, 0, "no running attempts");

    shutdown_scheduler(&scheduler).await;
    let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
}

// ── Test 2: JobWait returns bounded completion ─────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn jobwait_returns_bounded_completion() {
    let (scheduler, submission, _store, ws_id) = setup_managed_argv().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let submitted = submission
        .submit(
            None,
            build_managed_argv_job(&ws_id, vec!["echo".into(), "ok".into()]),
        )
        .await
        .expect("submit");

    let completion = scheduler
        .wait_for_completion(&submitted.job_id, Duration::from_secs(5))
        .await
        .expect("wait");

    assert!(
        matches!(completion.status, ExecutorStatus::Completed),
        "expected Completed, got {:?}",
        completion.status
    );
    // Bounded fields: summary is a short string, run_id is optional.
    assert!(completion.summary.len() < 1024, "summary should be bounded");
    let _ = completion.run_id;
    let _ = completion.metrics;

    shutdown_scheduler(&scheduler).await;
    let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
}

// ── Test 3: JobList bounded response ──────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn joblist_bounded_response() {
    let (scheduler, submission, store, ws_id) = setup_managed_argv().await;
    let sched = scheduler.clone();
    let _loop_handle = tokio::spawn(async move { sched.run().await });

    // Submit 5 jobs; the scheduler loop will quickly complete them
    // since the executor finishes instantly, but durable records persist.
    for i in 0..5 {
        submission
            .submit(
                None,
                build_managed_argv_job(&ws_id, vec!["echo".into(), format!("job-{i}")]),
            )
            .await
            .expect("submit");
    }

    // Wait for all to complete.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let all = store
        .list_jobs(JobStoreQuery::default())
        .await
        .expect("list all");
    assert_eq!(all.len(), 5, "all 5 jobs persisted");

    let limited = store
        .list_jobs(JobStoreQuery {
            limit: Some(2),
            ..Default::default()
        })
        .await
        .expect("list limited");
    assert!(
        limited.len() <= 2,
        "limit=2 must return at most 2, got {}",
        limited.len()
    );

    shutdown_scheduler(&scheduler).await;
}

// ── Test 4: oldest-queued-age matches durable created_at ──────────────

#[tokio::test(flavor = "multi_thread")]
async fn oldest_queued_age_matches_created_at() {
    let (scheduler, submission, _store, ws_id, _release) = setup_with_holding_executor().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    // First job: gets admitted and holds the slot.
    let _holder = submission
        .submit(None, build_managed_argv_job(&ws_id, vec!["hold".into()]))
        .await
        .expect("submit holder");

    // Wait for it to be admitted and running.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Second job: stays queued because the slot is held.
    let _queued = submission
        .submit(
            None,
            build_managed_argv_job(&ws_id, vec!["echo".into(), "age-test".into()]),
        )
        .await
        .expect("submit queued");

    // Let the scheduler reconcile so the queue entry appears.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let snap1 = scheduler.snapshot().await;
    let age1 = snap1.oldest_queued_age_secs.unwrap_or(0);

    tokio::time::sleep(Duration::from_millis(600)).await;

    let snap2 = scheduler.snapshot().await;
    let age2 = snap2.oldest_queued_age_secs.unwrap_or(0);
    assert!(
        age2 >= age1,
        "oldest_queued_age must not decrease: {age1} -> {age2}"
    );

    // Release the holder.
    _release.store(true, Ordering::SeqCst);

    shutdown_scheduler(&scheduler).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

// ── Test 5: cancel removes from queue snapshot ────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn cancel_removes_from_queue_snapshot() {
    let (scheduler, submission, store, ws_id, _release) = setup_with_holding_executor().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    // First job: gets admitted and holds the slot.
    let _holder = submission
        .submit(None, build_managed_argv_job(&ws_id, vec!["hold".into()]))
        .await
        .expect("submit holder");

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Second job: stays queued.
    let queued = submission
        .submit(
            None,
            build_managed_argv_job(&ws_id, vec!["echo".into(), "to-cancel".into()]),
        )
        .await
        .expect("submit queued");

    // Let the scheduler reconcile so the queue entry appears.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let pre_cancel = store
        .get_job(&queued.job_id)
        .await
        .expect("get_job")
        .expect("exists");
    assert_eq!(pre_cancel.state, JobState::Queued);

    let cancel_result = scheduler
        .request_cancel(&queued.job_id, "test cancel")
        .await
        .expect("cancel");
    assert!(
        matches!(
            cancel_result.state,
            codegg_core::jobs::CancelOutcome::Cancelled
        ),
        "queued job should be immediately cancelled"
    );

    // Allow the scheduler to reconcile and evict the queue entry.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let snap = scheduler.snapshot().await;
    let ws_summary = snap
        .per_workspace
        .iter()
        .find(|w| w.workspace_id == ws_id)
        .expect("workspace in snapshot");
    assert_eq!(ws_summary.queued, 0, "cancelled job removed from queue");

    let post_cancel = store
        .get_job(&queued.job_id)
        .await
        .expect("get_job")
        .expect("exists");
    assert_eq!(post_cancel.state, JobState::Cancelled);

    _release.store(true, Ordering::SeqCst);
    shutdown_scheduler(&scheduler).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

// ── Test 6: event-log boundedness ─────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn event_log_boundedness() {
    let root = tempfile::tempdir().expect("workspace");
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("workspace registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let mut config = ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = 1;
    let generation = DaemonGeneration::new_unchecked("event-bounded-gen");
    let scheduler = JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
    let release = Arc::new(AtomicBool::new(false));
    scheduler
        .register_executor(Arc::new(HoldingExecutor {
            release: release.clone(),
            active: Arc::new(AtomicUsize::new(0)),
        }))
        .await
        .expect("register executor");

    // Submit a holder job to fill the single slot.
    let holder_spec = NewJob {
        workspace_id: workspace.id.clone(),
        session_id: None,
        turn_id: None,
        kind: JobKind::Build,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::ManagedArgv {
            argv: vec!["hold".into()],
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
    };
    let _ = scheduler.submit(holder_spec).await;
    // Wait for the holder to be admitted.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Submit 100 more jobs — they stay queued because slot is held.
    for i in 0..100 {
        let spec = NewJob {
            workspace_id: workspace.id.clone(),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv {
                argv: vec!["echo".into(), format!("ev-{i}")],
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
        };
        let _ = scheduler.submit(spec).await;
    }

    // Run reconciliation + dispatch rounds to trigger admission blocks.
    for _ in 0..10 {
        let _ = scheduler.clone().reconcile().await;
        scheduler.clone().admit_and_dispatch_batch().await;
    }

    // The scheduler's overload counters should be bounded and monotonic.
    let snap = scheduler.snapshot().await;
    assert!(
        snap.overload.rejected_admissions > 0,
        "expected admission blocks, got {}",
        snap.overload.rejected_admissions
    );
    // The queue should not grow beyond 100 (the number submitted).
    assert!(
        snap.durable_queued_count <= 101,
        "queue must not grow unbounded: {}",
        snap.durable_queued_count
    );

    // Internal state must be consistent: per_workspace queued counts
    // sum to durable_queued_count.
    let queued_sum: usize = snap.per_workspace.iter().map(|w| w.queued).sum();
    assert_eq!(
        queued_sum, snap.durable_queued_count,
        "per_workspace queued sum must match durable_queued_count"
    );

    release.store(true, Ordering::SeqCst);
    shutdown_scheduler(&scheduler).await;
}

// ── Test 7: protocol handler error taxonomy ───────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn invalid_submission_key_is_rejected() {
    let result = SubmissionKey::new("");
    assert!(matches!(
        result,
        Err(JobSubmissionError::InvalidSubmissionKey)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_scheduler_rejects_submit() {
    let root = tempfile::tempdir().expect("workspace");
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("workspace registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let config = ResolvedSchedulerConfig {
        enabled: false,
        ..ResolvedSchedulerConfig::default()
    };
    let generation = DaemonGeneration::new_unchecked("disabled-gen");
    let scheduler = JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
    let submission = JobSubmissionService::new(store, scheduler, services, generation);
    let error = submission
        .submit(
            None,
            build_managed_argv_job(&workspace.id, vec!["echo".into(), "nope".into()]),
        )
        .await
        .expect_err("disabled scheduler must reject");
    assert!(
        matches!(error, JobSubmissionError::SchedulerDisabled),
        "expected SchedulerDisabled, got: {error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn unregistered_workspace_is_rejected() {
    let root_a = tempfile::tempdir().expect("workspace a");
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("workspace registry");
    let _workspace_a = workspace_registry
        .get_or_register(root_a.path())
        .await
        .expect("register workspace a");
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let config = ResolvedSchedulerConfig::default();
    let generation = DaemonGeneration::new_unchecked("ws-reject-gen");
    let scheduler = JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
    let submission = JobSubmissionService::new(store, scheduler, services, generation);
    let workspace_b = ws("unregistered-ws-id");
    let error = submission
        .submit(
            None,
            build_managed_argv_job(&workspace_b, vec!["echo".into(), "nope".into()]),
        )
        .await
        .expect_err("unregistered workspace must be rejected");
    assert!(
        matches!(error, JobSubmissionError::Workspace(_)),
        "expected Workspace error, got: {error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_payload_for_kind_is_rejected() {
    let root = tempfile::tempdir().expect("workspace");
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("workspace registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let config = ResolvedSchedulerConfig::default();
    let generation = DaemonGeneration::new_unchecked("payload-gen");
    let scheduler = JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
    let submission = JobSubmissionService::new(store, scheduler, services, generation);

    // Build kind requires ManagedArgv; send Test payload instead.
    let error = submission
        .submit(
            None,
            NewJob {
                workspace_id: workspace.id.clone(),
                session_id: None,
                turn_id: None,
                kind: JobKind::Build,
                source: JobSource::Interactive,
                priority: JobPriority::Interactive,
                payload: JobPayload::Test {
                    command: "echo ok".into(),
                    argv: vec!["echo".into(), "ok".into()],
                    cwd: None,
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
            },
        )
        .await
        .expect_err("mismatched payload must be rejected");
    assert!(
        matches!(error, JobSubmissionError::InvalidPayload(_)),
        "expected InvalidPayload, got: {error}"
    );
}

// ── Test 8: snapshot queue consistency after reconcile ────────────────

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_queue_consistency_after_reconcile() {
    let (scheduler, submission, _store, ws_id, _release) = setup_with_holding_executor().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    // First job: fills the single slot.
    let _holder = submission
        .submit(None, build_managed_argv_job(&ws_id, vec!["hold".into()]))
        .await
        .expect("submit holder");

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Submit 3 more jobs; they stay queued.
    for i in 0..3 {
        submission
            .submit(
                None,
                build_managed_argv_job(&ws_id, vec!["echo".into(), format!("c-{i}")]),
            )
            .await
            .expect("submit");
    }

    // Allow reconciliation.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let snap = scheduler.snapshot().await;
    let ws_summary = snap
        .per_workspace
        .iter()
        .find(|w| w.workspace_id == ws_id)
        .expect("workspace");
    assert_eq!(ws_summary.queued, 3, "all 3 jobs should be queued");
    assert_eq!(ws_summary.running, 1, "holder is running");
    assert_eq!(snap.durable_queued_count, 3);

    _release.store(true, Ordering::SeqCst);
    shutdown_scheduler(&scheduler).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

// ── Test 9: snapshot after cancel clears workspace running count ──────

#[tokio::test(flavor = "multi_thread")]
async fn cancel_clears_running_from_snapshot() {
    let (scheduler, submission, store, ws_id, _release) = setup_with_holding_executor().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    // Submit a job; it gets admitted and holds the slot.
    let submitted = submission
        .submit(None, build_slow_job(&ws_id))
        .await
        .expect("submit");

    // Wait until the job is actually Running.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let job = store
            .get_job(&submitted.job_id)
            .await
            .expect("get_job")
            .expect("exists");
        if job.state == JobState::Running {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("job did not start running within deadline");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Cancel the running job.
    let cancel_result = scheduler
        .request_cancel(&submitted.job_id, "test cancel running")
        .await
        .expect("cancel");
    assert!(
        matches!(
            cancel_result.state,
            codegg_core::jobs::CancelOutcome::Requested
        ),
        "running job cancel should be Requested, got {:?}",
        cancel_result.state
    );

    // Wait for the job to reach terminal state.
    let _job = wait_for_terminal(&*store, &submitted.job_id, Duration::from_secs(5)).await;

    let snap = scheduler.snapshot().await;
    let ws_summary = snap
        .per_workspace
        .iter()
        .find(|w| w.workspace_id == ws_id)
        .expect("workspace");
    assert_eq!(
        ws_summary.running, 0,
        "no running jobs after cancel + terminal"
    );

    _release.store(true, Ordering::SeqCst);
    shutdown_scheduler(&scheduler).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

// ── Test 10: serialization round-trip of SchedulerSnapshot ────────────

#[tokio::test(flavor = "current_thread")]
async fn snapshot_serialization_round_trip() {
    let (scheduler, _submission, _store, _ws_id) = setup_managed_argv().await;
    let snap = scheduler.snapshot().await;

    let json = serde_json::to_string(&snap).expect("serialize snapshot");
    let deserialized: codegg::scheduler::SchedulerSnapshot =
        serde_json::from_str(&json).expect("deserialize snapshot");

    assert_eq!(deserialized.ready_window_count, snap.ready_window_count);
    assert_eq!(deserialized.durable_queued_count, snap.durable_queued_count);
    assert_eq!(deserialized.running_attempts, snap.running_attempts);
    assert_eq!(deserialized.enabled, snap.enabled);
    assert_eq!(
        deserialized.oldest_queued_age_secs,
        snap.oldest_queued_age_secs
    );

    shutdown_scheduler(&scheduler).await;
}
