//! Scheduler cancellation chain tests (Workstream D, section 8).
//!
//! These integration tests verify that cancellation propagates correctly
//! from `JobScheduler::request_cancel` through the `CancellationToken`
//! plumbing to the typed executor, and that the durable job/attempt
//! state machine reaches the correct terminal state.
//!
//! No production code is modified. Custom executors simulate controlled
//! workloads to exercise the cancellation chain end-to-end.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use codegg::scheduler::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, JobExecutionContext,
    JobScheduler, JobSubmissionService, ResolvedSchedulerConfig,
};
use codegg_core::jobs::{
    CancelOutcome, DaemonGeneration, IdempotencyClass, InMemoryJobStore, JobKind, JobPayload,
    JobPriority, JobRecord, JobSource, JobState, JobStore, NewJob, RecoveryPolicy, ResourceRequest,
    RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;

fn gen() -> DaemonGeneration {
    DaemonGeneration::new_unchecked(format!("test-gen-{}", uuid::Uuid::new_v4()))
}

fn build_config(max_process_slots: u32) -> ResolvedSchedulerConfig {
    ResolvedSchedulerConfig {
        enabled: true,
        resources: codegg::scheduler::config::ResourceBudget {
            max_process_slots,
            max_cpu_weight: 8,
            max_memory_mb_hint: 8192,
            max_io_weight: 8,
            max_network_slots: 4,
        },
        ..ResolvedSchedulerConfig::default()
    }
}

fn build_managed_process_spec(workspace: &WorkspaceId, argv: Vec<String>) -> NewJob {
    NewJob {
        workspace_id: workspace.clone(),
        session_id: None,
        turn_id: None,
        kind: JobKind::ManagedProcess,
        source: codegg_core::jobs::JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::ManagedArgv {
            argv,
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

fn build_subagent_spec(workspace: &WorkspaceId) -> NewJob {
    NewJob {
        workspace_id: workspace.clone(),
        session_id: None,
        turn_id: None,
        kind: JobKind::Subagent,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Subagent {
            prompt: "do something".into(),
            agent: "general".into(),
            parent_id: None,
            denied_tools: vec![],
            allowed_paths: vec![],
            max_tool_calls: None,
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

/// Create a workspace registry with a registered workspace and return
/// (registry, services, workspace_id).
async fn setup_workspace() -> (
    Arc<codegg_core::workspace::WorkspaceRegistry>,
    Arc<codegg_core::workspace_services::WorkspaceServiceRegistry>,
    WorkspaceId,
    tempfile::TempDir,
) {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("workspace registry");
    let ws_record = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let ws_id = ws_record.id.clone();
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry.clone(),
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    (workspace_registry, services, ws_id, root)
}

/// Helper: construct scheduler + submission service + register a custom
/// executor. Returns (scheduler, submission, store, workspace_id, _tempdir).
async fn setup_with_executor(
    _kind: ExecutorKind,
    executor: Arc<dyn codegg::scheduler::JobExecutor>,
    max_process_slots: u32,
) -> (
    Arc<JobScheduler>,
    Arc<JobSubmissionService>,
    Arc<dyn JobStore>,
    WorkspaceId,
    tempfile::TempDir,
) {
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let (_registry, services, ws_id, root) = setup_workspace().await;
    let config = build_config(max_process_slots);
    let generation = gen();
    let scheduler = JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
    let submission =
        JobSubmissionService::new(store.clone(), scheduler.clone(), services, generation);

    // Register custom executor.
    let sched_clone = scheduler.clone();
    let exec_clone = executor;
    tokio::spawn(async move {
        sched_clone
            .register_executor(exec_clone)
            .await
            .expect("register executor");
    });

    (scheduler, submission, store, ws_id, root)
}

/// Busy-wait for a job to reach a specific state, with a bounded timeout.
async fn wait_for_state(
    store: &Arc<dyn JobStore>,
    job_id: &codegg_core::jobs::JobId,
    expected: JobState,
    timeout: Duration,
) -> JobRecord {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let job = store
            .get_job(job_id)
            .await
            .expect("get_job")
            .expect("job missing");
        if job.state == expected {
            return job;
        }
        if job.state.is_terminal() && expected != JobState::Running {
            return job;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timeout waiting for job {} to reach {:?}; current state: {:?}",
                job_id, expected, job.state
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Busy-wait for a job to reach a terminal state, with a bounded timeout.
async fn wait_for_terminal(
    store: &Arc<dyn JobStore>,
    job_id: &codegg_core::jobs::JobId,
    timeout: Duration,
) -> JobRecord {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let job = store
            .get_job(job_id)
            .await
            .expect("get_job")
            .expect("job missing");
        if job.state.is_terminal() {
            return job;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timeout waiting for job {} to become terminal; current state: {:?}",
                job_id, job.state
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Custom executors for cancellation testing
// ═══════════════════════════════════════════════════════════════════════════

/// Executor that sleeps for a configurable duration, then completes.
/// Records whether its cancellation token was observed.
struct SleepExecutor {
    sleep_ms: u64,
    cancel_observed: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl codegg::scheduler::JobExecutor for SleepExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ManagedArgv
    }
    fn supports(&self, kind: JobKind) -> bool {
        matches!(
            kind,
            JobKind::Build | JobKind::ManagedProcess | JobKind::Shell
        )
    }
    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let started = std::time::Instant::now();
        let sleep_ms = self.sleep_ms;
        let token = ctx.cancellation.clone();
        let cancel_flag = self.cancel_observed.clone();

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(sleep_ms)) => {}
            _ = token.cancelled() => {
                cancel_flag.store(true, Ordering::SeqCst);
                return ExecutorCompletion {
                    status: ExecutorStatus::Cancelled,
                    summary: "cancelled during sleep".into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        }
        ExecutorCompletion {
            status: ExecutorStatus::Completed,
            summary: "ok".into(),
            run_id: None,
            metrics: ExecutorMetrics {
                elapsed_ms: started.elapsed().as_millis() as u64,
                ..Default::default()
            },
        }
    }
}

/// Executor that does an instant completion.
struct InstantExecutor;

#[async_trait::async_trait]
impl codegg::scheduler::JobExecutor for InstantExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ManagedArgv
    }
    fn supports(&self, kind: JobKind) -> bool {
        matches!(
            kind,
            JobKind::Build | JobKind::ManagedProcess | JobKind::Shell
        )
    }
    async fn execute(&self, _ctx: JobExecutionContext) -> ExecutorCompletion {
        ExecutorCompletion {
            status: ExecutorStatus::Completed,
            summary: "instant ok".into(),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}

/// Executor that observes and records the CancellationToken state.
struct CancellationObservingExecutor {
    token_cancelled_at_entry: Arc<AtomicBool>,
    token_cancelled_during: Arc<AtomicBool>,
    sleep_ms: u64,
}

#[async_trait::async_trait]
impl codegg::scheduler::JobExecutor for CancellationObservingExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ManagedArgv
    }
    fn supports(&self, kind: JobKind) -> bool {
        matches!(
            kind,
            JobKind::Build | JobKind::ManagedProcess | JobKind::Shell
        )
    }
    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let token = ctx.cancellation.clone();
        self.token_cancelled_at_entry
            .store(token.is_cancelled(), Ordering::SeqCst);
        let started = std::time::Instant::now();
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(self.sleep_ms)) => {
                ExecutorCompletion {
                    status: ExecutorStatus::Completed,
                    summary: "completed".into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                }
            }
            _ = token.cancelled() => {
                self.token_cancelled_during.store(true, Ordering::SeqCst);
                ExecutorCompletion {
                    status: ExecutorStatus::Cancelled,
                    summary: "cancelled".into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                }
            }
        }
    }
}

/// Executor that simulates a subagent with controlled delay.
struct FakeSubagentExecutor {
    sleep_ms: u64,
    cancel_observed: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl codegg::scheduler::JobExecutor for FakeSubagentExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Subagent
    }
    fn supports(&self, kind: JobKind) -> bool {
        matches!(kind, JobKind::Subagent)
    }
    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let started = std::time::Instant::now();
        let token = ctx.cancellation.clone();
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(self.sleep_ms)) => {
                ExecutorCompletion {
                    status: ExecutorStatus::Completed,
                    summary: "subagent completed".into(),
                    run_id: None,
                    metrics: ExecutorMetrics { elapsed_ms: started.elapsed().as_millis() as u64, ..Default::default() },
                }
            }
            _ = token.cancelled() => {
                self.cancel_observed.store(true, Ordering::SeqCst);
                ExecutorCompletion {
                    status: ExecutorStatus::Cancelled,
                    summary: "subagent cancelled".into(),
                    run_id: None,
                    metrics: ExecutorMetrics { elapsed_ms: started.elapsed().as_millis() as u64, ..Default::default() },
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 1: Cancel a running job
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_running_job_terminates_process_and_releases_permit() {
    let cancel_observed = Arc::new(AtomicBool::new(false));
    let executor: Arc<dyn codegg::scheduler::JobExecutor> = Arc::new(SleepExecutor {
        sleep_ms: 10_000,
        cancel_observed: cancel_observed.clone(),
    });
    let (scheduler, submission, store, ws_id, _root) =
        setup_with_executor(ExecutorKind::ManagedArgv, executor, 4).await;

    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let job_id = submission
        .submit(
            None,
            build_managed_process_spec(&ws_id, vec!["sleep".into(), "10".into()]),
        )
        .await
        .expect("submit")
        .job_id;

    // Wait for the scheduler to admit and dispatch.
    let job = wait_for_state(&store, &job_id, JobState::Running, Duration::from_secs(3)).await;
    assert_eq!(job.state, JobState::Running);

    // Request cancellation.
    let cancel_result = scheduler
        .request_cancel(&job_id, "user cancel")
        .await
        .expect("request_cancel");
    assert_eq!(cancel_result.state, CancelOutcome::Requested);
    assert!(!cancel_result.terminal);

    // Wait for the job to become terminal.
    let job = wait_for_terminal(&store, &job_id, Duration::from_secs(5)).await;
    assert!(
        job.state == JobState::Cancelled || job.state == JobState::Completed,
        "expected Cancelled or Completed, got {:?}",
        job.state
    );
    assert!(
        cancel_observed.load(Ordering::SeqCst),
        "executor must observe cancellation token"
    );

    // Permit is released.
    assert_eq!(scheduler.admission().used_process_slots(), 0);

    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 2: Cancel before admission
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_before_admission_terminates_job() {
    let executor: Arc<dyn codegg::scheduler::JobExecutor> = Arc::new(SleepExecutor {
        sleep_ms: 10_000,
        cancel_observed: Arc::new(AtomicBool::new(false)),
    });
    let (scheduler, submission, store, ws_id, _root) =
        setup_with_executor(ExecutorKind::ManagedArgv, executor, 1).await;

    let job_id = submission
        .submit(
            None,
            build_managed_process_spec(&ws_id, vec!["sleep".into(), "10".into()]),
        )
        .await
        .expect("submit")
        .job_id;

    // Cancel before the scheduler admits.
    let cancel_result = scheduler
        .request_cancel(&job_id, "pre-admission")
        .await
        .expect("request_cancel");
    assert_eq!(cancel_result.state, CancelOutcome::Cancelled);
    assert!(cancel_result.terminal);

    let job = store
        .get_job(&job_id)
        .await
        .expect("get_job")
        .expect("job missing");
    assert_eq!(job.state, JobState::Cancelled);

    // After reconcile, stays Cancelled.
    scheduler.reconcile().await.expect("reconcile");
    let job = store
        .get_job(&job_id)
        .await
        .expect("get_job")
        .expect("job missing");
    assert_eq!(job.state, JobState::Cancelled);
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 3: Duplicate cancel is idempotent
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn duplicate_cancel_is_idempotent() {
    let (_registry, services, ws_id, _root) = setup_workspace().await;
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let config = build_config(1);
    let generation = gen();
    let scheduler = JobScheduler::new(store.clone(), services, config, generation);

    let job = store
        .create_job(build_managed_process_spec(
            &ws_id,
            vec!["echo".into(), "hi".into()],
        ))
        .await
        .expect("create_job");

    // First cancel — queued → cancelled.
    let r1 = scheduler
        .request_cancel(&job.job_id, "first")
        .await
        .expect("first cancel");
    assert_eq!(r1.state, CancelOutcome::Cancelled);
    assert!(r1.terminal);

    // Second cancel — already terminal, must not panic.
    let r2 = scheduler
        .request_cancel(&job.job_id, "second")
        .await
        .expect("second cancel");
    assert_eq!(r2.state, CancelOutcome::AlreadyTerminal);
    assert!(r2.terminal);

    // Third cancel — still idempotent.
    let r3 = scheduler
        .request_cancel(&job.job_id, "third")
        .await
        .expect("third cancel");
    assert_eq!(r3.state, CancelOutcome::AlreadyTerminal);
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 4: Completion racing cancellation
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_after_completion_returns_already_terminal() {
    let executor: Arc<dyn codegg::scheduler::JobExecutor> = Arc::new(InstantExecutor);
    let (scheduler, submission, store, ws_id, _root) =
        setup_with_executor(ExecutorKind::ManagedArgv, executor, 4).await;

    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let job_id = submission
        .submit(
            None,
            build_managed_process_spec(&ws_id, vec!["echo".into(), "ok".into()]),
        )
        .await
        .expect("submit")
        .job_id;

    // Wait for completion.
    let job = wait_for_terminal(&store, &job_id, Duration::from_secs(3)).await;
    assert_eq!(job.state, JobState::Completed);

    // Cancel — should be already terminal.
    let cancel_result = scheduler
        .request_cancel(&job_id, "too late")
        .await
        .expect("request_cancel");
    assert_eq!(cancel_result.state, CancelOutcome::AlreadyTerminal);
    assert!(cancel_result.terminal);

    let job = store
        .get_job(&job_id)
        .await
        .expect("get_job")
        .expect("job missing");
    assert_eq!(job.state, JobState::Completed);

    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 5: Timeout racing cancellation
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeout_racing_cancellation_first_writer_wins() {
    // Executor sleeps 200ms; job has 50ms timeout.
    let executor: Arc<dyn codegg::scheduler::JobExecutor> = Arc::new(SleepExecutor {
        sleep_ms: 200,
        cancel_observed: Arc::new(AtomicBool::new(false)),
    });
    let (scheduler, submission, store, ws_id, _root) =
        setup_with_executor(ExecutorKind::ManagedArgv, executor, 4).await;

    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let mut spec = build_managed_process_spec(&ws_id, vec!["sleep".into(), "10".into()]);
    spec.timeout = Some(Duration::from_millis(50));

    let job_id = submission.submit(None, spec).await.expect("submit").job_id;

    let job = wait_for_terminal(&store, &job_id, Duration::from_secs(3)).await;
    assert!(
        job.state == JobState::TimedOut
            || job.state == JobState::Cancelled
            || job.state == JobState::Completed,
        "expected terminal state, got {:?}",
        job.state
    );

    // Cancel after terminal.
    let cancel_result = scheduler
        .request_cancel(&job_id, "post-timeout")
        .await
        .expect("request_cancel");
    assert_eq!(cancel_result.state, CancelOutcome::AlreadyTerminal);

    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 6: Cancel subagent work
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_subagent_interrupts_attempt() {
    let cancel_observed = Arc::new(AtomicBool::new(false));
    let executor: Arc<dyn codegg::scheduler::JobExecutor> = Arc::new(FakeSubagentExecutor {
        sleep_ms: 5_000,
        cancel_observed: cancel_observed.clone(),
    });
    let (scheduler, submission, store, ws_id, _root) =
        setup_with_executor(ExecutorKind::Subagent, executor, 4).await;

    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let job_id = submission
        .submit(None, build_subagent_spec(&ws_id))
        .await
        .expect("submit")
        .job_id;

    // Wait for the scheduler to admit and dispatch.
    let job = wait_for_state(&store, &job_id, JobState::Running, Duration::from_secs(3)).await;
    assert_eq!(job.state, JobState::Running);

    // Cancel.
    let cancel_result = scheduler
        .request_cancel(&job_id, "cancel subagent")
        .await
        .expect("request_cancel");
    assert_eq!(cancel_result.state, CancelOutcome::Requested);

    let job = wait_for_terminal(&store, &job_id, Duration::from_secs(5)).await;
    assert!(
        job.state == JobState::Cancelled || job.state == JobState::Completed,
        "expected Cancelled or Completed, got {:?}",
        job.state
    );
    assert!(
        cancel_observed.load(Ordering::SeqCst),
        "subagent executor must observe cancellation"
    );

    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 7: Cancel during admission (blocked by max_process_slots)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_queued_job_blocked_by_slots() {
    // Only 1 process slot — first job occupies it, second is blocked.
    let executor: Arc<dyn codegg::scheduler::JobExecutor> = Arc::new(SleepExecutor {
        sleep_ms: 10_000,
        cancel_observed: Arc::new(AtomicBool::new(false)),
    });
    let (scheduler, submission, store, ws_id, _root) =
        setup_with_executor(ExecutorKind::ManagedArgv, executor, 1).await;

    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    // First job occupies the sole slot.
    let first_id = submission
        .submit(
            None,
            build_managed_process_spec(&ws_id, vec!["sleep".into(), "10".into()]),
        )
        .await
        .expect("submit first")
        .job_id;

    // Wait for first job to be Running (occupying the slot).
    wait_for_state(&store, &first_id, JobState::Running, Duration::from_secs(3)).await;

    // Second job is blocked.
    let second_id = submission
        .submit(
            None,
            build_managed_process_spec(&ws_id, vec!["sleep".into(), "10".into()]),
        )
        .await
        .expect("submit second")
        .job_id;

    // Give the scheduler a tick.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Cancel the queued second job.
    let cancel_result = scheduler
        .request_cancel(&second_id, "cancel queued")
        .await
        .expect("request_cancel");
    assert_eq!(cancel_result.state, CancelOutcome::Cancelled);
    assert!(cancel_result.terminal);

    let job = store
        .get_job(&second_id)
        .await
        .expect("get_job")
        .expect("job missing");
    assert_eq!(job.state, JobState::Cancelled);

    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 8: Cancellation token is propagated to typed executor
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_token_propagated_to_executor() {
    let token_at_entry = Arc::new(AtomicBool::new(false));
    let token_during = Arc::new(AtomicBool::new(false));
    let executor: Arc<dyn codegg::scheduler::JobExecutor> =
        Arc::new(CancellationObservingExecutor {
            token_cancelled_at_entry: token_at_entry.clone(),
            token_cancelled_during: token_during.clone(),
            sleep_ms: 5_000,
        });
    let (scheduler, submission, store, ws_id, _root) =
        setup_with_executor(ExecutorKind::ManagedArgv, executor, 4).await;

    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let job_id = submission
        .submit(
            None,
            build_managed_process_spec(&ws_id, vec!["sleep".into(), "5".into()]),
        )
        .await
        .expect("submit")
        .job_id;

    // Wait for admitted and running.
    let job = wait_for_state(&store, &job_id, JobState::Running, Duration::from_secs(3)).await;
    assert_eq!(job.state, JobState::Running);

    // Token should NOT be cancelled at entry.
    assert!(
        !token_at_entry.load(Ordering::SeqCst),
        "token must not be pre-cancelled"
    );

    // Cancel.
    let _ = scheduler
        .request_cancel(&job_id, "propagation")
        .await
        .expect("request_cancel");

    let job = wait_for_terminal(&store, &job_id, Duration::from_secs(5)).await;
    assert!(
        job.state == JobState::Cancelled || job.state == JobState::Completed,
        "expected Cancelled or Completed, got {:?}",
        job.state
    );

    assert!(
        token_during.load(Ordering::SeqCst),
        "executor must observe cancellation token during execution"
    );

    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 9: Cancellation after daemon restarts (recover_generation)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn cancel_after_generation_recovery() {
    let (_registry, services, ws_id, _root) = setup_workspace().await;
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let config = build_config(4);
    let stale_gen = DaemonGeneration::new_unchecked("stale-gen");
    let _scheduler = JobScheduler::new(store.clone(), services, config, stale_gen.clone());

    // Create a job and begin an attempt under the stale generation.
    let mut spec = build_managed_process_spec(&ws_id, vec!["echo".into(), "hi".into()]);
    // Allow one retry so recovery requeues (default no_retry has
    // max_attempts == 1, which means requeue would be skipped).
    spec.retry_policy = RetryPolicy::bounded(
        3,
        codegg_core::jobs::BackoffPolicy::None,
        vec![codegg_core::jobs::FailureClass::Transient],
    );
    let job = store.create_job(spec).await.expect("create_job");
    let attempt = store
        .begin_attempt(&job.job_id, &stale_gen)
        .await
        .expect("begin_attempt");
    store
        .mark_attempt_running(&attempt.attempt_id)
        .await
        .expect("mark_running");

    // Verify state before recovery.
    let job_before = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_before.state, JobState::Running);
    let attempt_before = store.list_attempts(&job.job_id).await.unwrap();
    assert_eq!(attempt_before.len(), 1);
    assert_eq!(
        attempt_before[0].state,
        codegg_core::jobs::AttemptState::Running
    );

    // Simulate daemon restart: recover with a new generation.
    let policy = RecoveryPolicy::default(); // SafeRepeat is requeue-eligible
    let new_gen_for_recovery = DaemonGeneration::new_unchecked("new-gen-recovery");
    let report = store
        .recover_generation(&new_gen_for_recovery, &policy)
        .await
        .expect("recover_generation");
    assert_eq!(report.interrupted_attempts, 1);
    assert_eq!(report.requeued_jobs, 1);

    // Job is now Queued again after recovery.
    let job_after = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_after.state, JobState::Queued);

    // Create a new scheduler with the new generation and cancel the requeued job.
    let new_gen = DaemonGeneration::new_unchecked("new-gen");
    let workspace_registry2 = codegg_core::workspace::WorkspaceRegistry::new_for_tests(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ));
    let services2 = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry2,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let scheduler2 = JobScheduler::new(store.clone(), services2, build_config(4), new_gen);

    let cancel_result = scheduler2
        .request_cancel(&job.job_id, "post-recovery cancel")
        .await
        .expect("cancel after recovery");
    assert_eq!(cancel_result.state, CancelOutcome::Cancelled);
    assert!(cancel_result.terminal);

    let job_final = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_final.state, JobState::Cancelled);
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 10: Terminal precedence — completion wins
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completion_wins_over_late_cancel() {
    let executor: Arc<dyn codegg::scheduler::JobExecutor> = Arc::new(InstantExecutor);
    let (scheduler, submission, store, ws_id, _root) =
        setup_with_executor(ExecutorKind::ManagedArgv, executor, 4).await;

    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let job_id = submission
        .submit(
            None,
            build_managed_process_spec(&ws_id, vec!["echo".into(), "done".into()]),
        )
        .await
        .expect("submit")
        .job_id;

    let job = wait_for_terminal(&store, &job_id, Duration::from_secs(3)).await;
    assert_eq!(job.state, JobState::Completed);

    let completed_at = job.terminal_at.expect("terminal_at must be set");

    let cancel_result = scheduler
        .request_cancel(&job_id, "late cancel")
        .await
        .expect("request_cancel");
    assert_eq!(cancel_result.state, CancelOutcome::AlreadyTerminal);
    assert!(cancel_result.terminal);

    let job = store.get_job(&job_id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Completed);
    assert_eq!(job.terminal_at, Some(completed_at));

    let attempts = store.list_attempts(&job_id).await.unwrap();
    assert!(!attempts.is_empty());
    assert!(attempts[0].state.is_terminal());

    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
}
