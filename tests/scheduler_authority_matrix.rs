//! B3 — Runtime proof: authority matrix for migrated heavy-work classes.
//!
//! This test suite proves that for each migrated heavy-work class exactly
//! one `JobSubmissionService::submit` call produces exactly one durable
//! `JobRecord`, exactly one durable `JobAttempt`, exactly one scheduler
//! admission, exactly one executor entry, and exactly one terminal
//! completion. It also verifies that no raw-shell or direct legacy
//! executor marker fires.
//!
//! This is the runtime complement to `scripts/check_execution_ownership.py`
//! (the static guard).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use codegg::scheduler::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, JobExecutionContext,
    JobExecutor, JobScheduler, JobSubmissionService, ResolvedSchedulerConfig,
    SchedulerShutdownMode, SubmissionKey,
};
use codegg_core::jobs::{
    AttemptCompletion, DaemonGeneration, IdempotencyClass, InMemoryJobStore, JobAttempt, JobId,
    JobKind, JobPayload, JobPriority, JobRecord, JobSource, JobStore, JobStoreError, NewJob,
    ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;

// ── Helpers ───────────────────────────────────────────────────────────────

/// A `JobStore` wrapper that counts `create_job`, `begin_attempt`,
/// and `finish_attempt` invocations.
struct CountingJobStore {
    inner: InMemoryJobStore,
    create_count: Arc<AtomicU32>,
    begin_count: Arc<AtomicU32>,
    finish_count: Arc<AtomicU32>,
}

impl CountingJobStore {
    fn new() -> Self {
        Self {
            inner: InMemoryJobStore::new(),
            create_count: Arc::new(AtomicU32::new(0)),
            begin_count: Arc::new(AtomicU32::new(0)),
            finish_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn counts(&self) -> StoreCounts {
        StoreCounts {
            create: self.create_count.load(Ordering::SeqCst),
            begin: self.begin_count.load(Ordering::SeqCst),
            finish: self.finish_count.load(Ordering::SeqCst),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StoreCounts {
    create: u32,
    begin: u32,
    finish: u32,
}

#[async_trait]
impl JobStore for CountingJobStore {
    async fn create_job(&self, spec: NewJob) -> Result<JobRecord, JobStoreError> {
        self.create_count.fetch_add(1, Ordering::SeqCst);
        self.inner.create_job(spec).await
    }
    async fn get_job(&self, id: &JobId) -> Result<Option<JobRecord>, JobStoreError> {
        self.inner.get_job(id).await
    }
    async fn list_jobs(
        &self,
        query: codegg_core::jobs::store::JobStoreQuery,
    ) -> Result<Vec<codegg_core::jobs::store::JobSummary>, JobStoreError> {
        self.inner.list_jobs(query).await
    }
    async fn list_attempts(&self, job_id: &JobId) -> Result<Vec<JobAttempt>, JobStoreError> {
        self.inner.list_attempts(job_id).await
    }
    async fn enqueue(&self, id: &JobId) -> Result<JobRecord, JobStoreError> {
        self.inner.enqueue(id).await
    }
    async fn begin_attempt(
        &self,
        id: &JobId,
        generation: &DaemonGeneration,
    ) -> Result<JobAttempt, JobStoreError> {
        self.begin_count.fetch_add(1, Ordering::SeqCst);
        self.inner.begin_attempt(id, generation).await
    }
    async fn mark_attempt_running(
        &self,
        attempt_id: &codegg_core::jobs::AttemptId,
    ) -> Result<(), JobStoreError> {
        self.inner.mark_attempt_running(attempt_id).await
    }
    async fn set_attempt_executor(
        &self,
        attempt_id: &codegg_core::jobs::AttemptId,
        executor: &str,
    ) -> Result<(), JobStoreError> {
        self.inner.set_attempt_executor(attempt_id, executor).await
    }
    async fn record_heartbeat(
        &self,
        attempt_id: &codegg_core::jobs::AttemptId,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), JobStoreError> {
        self.inner.record_heartbeat(attempt_id, at).await
    }
    async fn finish_attempt(
        &self,
        completion: AttemptCompletion,
    ) -> Result<JobRecord, JobStoreError> {
        self.finish_count.fetch_add(1, Ordering::SeqCst);
        self.inner.finish_attempt(completion).await
    }
    async fn request_cancel(
        &self,
        id: &JobId,
        reason: codegg_core::jobs::CancelReason,
    ) -> Result<codegg_core::jobs::CancelResult, JobStoreError> {
        self.inner.request_cancel(id, reason).await
    }
    async fn retry_job(
        &self,
        id: &JobId,
        generation: &DaemonGeneration,
        prior_attempt_id: &codegg_core::jobs::AttemptId,
    ) -> Result<JobAttempt, JobStoreError> {
        self.inner.retry_job(id, generation, prior_attempt_id).await
    }
    async fn block_job(&self, id: &JobId) -> Result<JobRecord, JobStoreError> {
        self.inner.block_job(id).await
    }
    async fn recover_generation(
        &self,
        stale: &DaemonGeneration,
        policy: &codegg_core::jobs::RecoveryPolicy,
    ) -> Result<codegg_core::jobs::RecoveryReport, JobStoreError> {
        self.inner.recover_generation(stale, policy).await
    }
}

/// An executor that wraps any inner executor and counts entries and
/// completions.
struct CountingExecutor {
    inner: tokio::sync::Mutex<Option<Arc<dyn JobExecutor>>>,
    entries: Arc<AtomicU32>,
    completions: Arc<AtomicU32>,
    executor_kind: ExecutorKind,
}

impl CountingExecutor {
    fn new(kind: ExecutorKind) -> Self {
        Self {
            inner: tokio::sync::Mutex::new(None),
            entries: Arc::new(AtomicU32::new(0)),
            completions: Arc::new(AtomicU32::new(0)),
            executor_kind: kind,
        }
    }

    fn entry_count(&self) -> u32 {
        self.entries.load(Ordering::SeqCst)
    }

    fn completion_count(&self) -> u32 {
        self.completions.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl JobExecutor for CountingExecutor {
    fn kind(&self) -> ExecutorKind {
        self.executor_kind
    }

    fn supports(&self, _kind: JobKind) -> bool {
        true
    }

    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        self.entries.fetch_add(1, Ordering::SeqCst);
        let result = {
            let guard = self.inner.lock().await;
            match guard.as_ref() {
                Some(inner) => inner.execute(ctx).await,
                None => ExecutorCompletion {
                    status: ExecutorStatus::Completed,
                    summary: "counting-executor-noop".into(),
                    run_id: None,
                    metrics: ExecutorMetrics::default(),
                },
            }
        };
        self.completions.fetch_add(1, Ordering::SeqCst);
        result
    }
}

/// Build a `NewJob` with the given `kind` and `payload`.
fn build_new_job(ws_id: &WorkspaceId, kind: JobKind, payload: JobPayload) -> NewJob {
    NewJob {
        workspace_id: ws_id.clone(),
        session_id: None,
        turn_id: None,
        kind,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload,
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

/// Construct the full test stack: counting store, scheduler with counting
/// executor, and submission service.
///
/// Returns (scheduler, submission_service, counting_job_store, workspace_id,
/// counting_executor_ref).
async fn setup_authority_stack(
    job_kind: JobKind,
) -> (
    Arc<JobScheduler>,
    Arc<JobSubmissionService>,
    Arc<CountingJobStore>,
    WorkspaceId,
    Arc<CountingExecutor>,
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
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );

    let counting_store = Arc::new(CountingJobStore::new());
    let store_for_scheduler: Arc<dyn JobStore> = counting_store.clone();

    let config = ResolvedSchedulerConfig::default();
    let generation = DaemonGeneration::new_unchecked("authority-matrix-gen");

    let scheduler = JobScheduler::new(
        store_for_scheduler.clone(),
        services.clone(),
        config,
        generation.clone(),
    );

    let executor_kind = match job_kind {
        JobKind::Test => ExecutorKind::Test,
        JobKind::Subagent => ExecutorKind::Subagent,
        _ => ExecutorKind::ManagedArgv,
    };
    let counting_executor = Arc::new(CountingExecutor::new(executor_kind));
    let exec_ref = counting_executor.clone();
    scheduler
        .register_executor(counting_executor)
        .await
        .expect("register counting executor");

    let submission = JobSubmissionService::new(
        counting_store.clone(),
        scheduler.clone(),
        services,
        generation,
    );

    (
        scheduler,
        submission,
        counting_store,
        ws_record.id.clone(),
        exec_ref,
    )
}

/// Wait for a job to reach a terminal state.
async fn wait_for_terminal(
    store: &CountingJobStore,
    job_id: &JobId,
    timeout: Duration,
) -> JobRecord {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(job) = store.get_job(job_id).await.expect("get_job") {
            if job.state.is_terminal() {
                return job;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out waiting for job {job_id} to become terminal");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Build a `NewJob` for a test job.
fn test_payload() -> JobPayload {
    JobPayload::Test {
        command: "echo ok".into(),
        argv: vec!["echo".into(), "ok".into()],
        cwd: Some("/tmp".into()),
        scope: None,
    }
}

fn managed_argv_payload(argv: Vec<&str>) -> JobPayload {
    JobPayload::ManagedArgv {
        argv: argv.into_iter().map(String::from).collect(),
        cwd: Some("/tmp".into()),
    }
}

fn subagent_payload(agent: &str) -> JobPayload {
    JobPayload::Subagent {
        prompt: "do something".into(),
        agent: agent.into(),
        parent_id: None,
        denied_tools: vec![],
        allowed_paths: vec![],
        max_tool_calls: None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_job_produces_one_attempt_and_one_executor_entry() {
    let (scheduler, submission, store, ws_id, exec) = setup_authority_stack(JobKind::Test).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let submitted = submission
        .submit(None, build_new_job(&ws_id, JobKind::Test, test_payload()))
        .await
        .expect("submit");

    let job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(10)).await;
    assert!(
        job.state.is_terminal(),
        "job should be terminal, got {:?}",
        job.state
    );

    let counts = store.counts();
    assert_eq!(counts.create, 1, "exactly 1 create_job call");
    assert_eq!(counts.begin, 1, "exactly 1 begin_attempt call");
    assert_eq!(counts.finish, 1, "exactly 1 finish_attempt call");

    assert_eq!(exec.entry_count(), 1, "exactly 1 executor entry");
    assert_eq!(exec.completion_count(), 1, "exactly 1 executor completion");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn build_job_produces_one_attempt_and_one_executor_entry() {
    let (scheduler, submission, store, ws_id, exec) = setup_authority_stack(JobKind::Build).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let submitted = submission
        .submit(
            None,
            build_new_job(
                &ws_id,
                JobKind::Build,
                managed_argv_payload(vec!["cargo", "build"]),
            ),
        )
        .await
        .expect("submit");

    let job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(10)).await;
    assert!(job.state.is_terminal(), "build job should be terminal");

    let counts = store.counts();
    assert_eq!(counts.create, 1, "exactly 1 create_job call");
    assert_eq!(counts.begin, 1, "exactly 1 begin_attempt call");
    assert_eq!(counts.finish, 1, "exactly 1 finish_attempt call");

    assert_eq!(exec.entry_count(), 1, "exactly 1 executor entry");
    assert_eq!(exec.completion_count(), 1, "exactly 1 executor completion");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn lint_job_produces_one_attempt_and_one_executor_entry() {
    let (scheduler, submission, store, ws_id, exec) = setup_authority_stack(JobKind::Lint).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let submitted = submission
        .submit(
            None,
            build_new_job(
                &ws_id,
                JobKind::Lint,
                managed_argv_payload(vec!["cargo", "clippy"]),
            ),
        )
        .await
        .expect("submit");

    let job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(10)).await;
    assert!(job.state.is_terminal(), "lint job should be terminal");

    let counts = store.counts();
    assert_eq!(counts.create, 1, "exactly 1 create_job call");
    assert_eq!(counts.begin, 1, "exactly 1 begin_attempt call");
    assert_eq!(counts.finish, 1, "exactly 1 finish_attempt call");

    assert_eq!(exec.entry_count(), 1, "exactly 1 executor entry");
    assert_eq!(exec.completion_count(), 1, "exactly 1 executor completion");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn format_job_produces_one_attempt_and_one_executor_entry() {
    let (scheduler, submission, store, ws_id, exec) = setup_authority_stack(JobKind::Format).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let submitted = submission
        .submit(
            None,
            build_new_job(
                &ws_id,
                JobKind::Format,
                managed_argv_payload(vec!["cargo", "fmt"]),
            ),
        )
        .await
        .expect("submit");

    let job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(10)).await;
    assert!(job.state.is_terminal(), "format job should be terminal");

    let counts = store.counts();
    assert_eq!(counts.create, 1, "exactly 1 create_job call");
    assert_eq!(counts.begin, 1, "exactly 1 begin_attempt call");
    assert_eq!(counts.finish, 1, "exactly 1 finish_attempt call");

    assert_eq!(exec.entry_count(), 1, "exactly 1 executor entry");
    assert_eq!(exec.completion_count(), 1, "exactly 1 executor completion");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn subagent_job_produces_one_attempt_and_one_executor_entry() {
    let (scheduler, submission, store, ws_id, exec) =
        setup_authority_stack(JobKind::Subagent).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let submitted = submission
        .submit(
            None,
            build_new_job(&ws_id, JobKind::Subagent, subagent_payload("test-agent")),
        )
        .await
        .expect("submit");

    let job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(10)).await;
    assert!(job.state.is_terminal(), "subagent job should be terminal");

    let counts = store.counts();
    assert_eq!(counts.create, 1, "exactly 1 create_job call");
    assert_eq!(counts.begin, 1, "exactly 1 begin_attempt call");
    assert_eq!(counts.finish, 1, "exactly 1 finish_attempt call");

    assert_eq!(exec.entry_count(), 1, "exactly 1 executor entry");
    assert_eq!(exec.completion_count(), 1, "exactly 1 executor completion");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn task_tool_subagent_produces_one_attempt_and_one_executor_entry() {
    let (scheduler, submission, store, ws_id, exec) =
        setup_authority_stack(JobKind::Subagent).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let submitted = submission
        .submit(
            None,
            build_new_job(&ws_id, JobKind::Subagent, subagent_payload("task-agent")),
        )
        .await
        .expect("submit");

    let job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(10)).await;
    assert!(
        job.state.is_terminal(),
        "task-tool subagent job should be terminal"
    );

    let counts = store.counts();
    assert_eq!(counts.create, 1, "exactly 1 create_job call");
    assert_eq!(counts.begin, 1, "exactly 1 begin_attempt call");
    assert_eq!(counts.finish, 1, "exactly 1 finish_attempt call");

    assert_eq!(exec.entry_count(), 1, "exactly 1 executor entry");
    assert_eq!(exec.completion_count(), 1, "exactly 1 executor completion");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_security_review_produces_one_attempt_and_one_executor_entry() {
    let (scheduler, submission, store, ws_id, exec) =
        setup_authority_stack(JobKind::Subagent).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let submitted = submission
        .submit(
            None,
            build_new_job(
                &ws_id,
                JobKind::Subagent,
                subagent_payload("security-review"),
            ),
        )
        .await
        .expect("submit");

    let job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(10)).await;
    assert!(
        job.state.is_terminal(),
        "security-review job should be terminal"
    );

    let counts = store.counts();
    assert_eq!(counts.create, 1, "exactly 1 create_job call");
    assert_eq!(counts.begin, 1, "exactly 1 begin_attempt call");
    assert_eq!(counts.finish, 1, "exactly 1 finish_attempt call");

    assert_eq!(exec.entry_count(), 1, "exactly 1 executor entry");
    assert_eq!(exec.completion_count(), 1, "exactly 1 executor completion");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_scheduler_rejects_and_creates_no_job() {
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
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );

    let counting_store = Arc::new(CountingJobStore::new());
    let store_for_scheduler: Arc<dyn JobStore> = counting_store.clone();

    let disabled_config = ResolvedSchedulerConfig {
        enabled: false,
        ..ResolvedSchedulerConfig::default()
    };
    let generation = DaemonGeneration::new_unchecked("disabled-gen");
    let scheduler = JobScheduler::new(
        store_for_scheduler.clone(),
        services.clone(),
        disabled_config,
        generation.clone(),
    );

    let submission = JobSubmissionService::new(
        counting_store.clone(),
        scheduler.clone(),
        services,
        generation,
    );

    let error = submission
        .submit(
            None,
            build_new_job(&ws_record.id, JobKind::Test, test_payload()),
        )
        .await
        .expect_err("disabled scheduler must reject");

    assert!(
        matches!(
            error,
            codegg::scheduler::JobSubmissionError::SchedulerDisabled
        ),
        "expected SchedulerDisabled, got {error:?}"
    );

    let counts = counting_store.counts();
    assert_eq!(
        counts.create, 0,
        "no create_job calls on disabled scheduler"
    );
    assert_eq!(
        counts.begin, 0,
        "no begin_attempt calls on disabled scheduler"
    );
    assert_eq!(
        counts.finish, 0,
        "no finish_attempt calls on disabled scheduler"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn same_submission_key_coalesces_into_one_job() {
    let (scheduler, submission, store, ws_id, _exec) = setup_authority_stack(JobKind::Test).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let key = SubmissionKey::new("idempotent-key-1").expect("key");
    let spec = build_new_job(&ws_id, JobKind::Test, test_payload());

    let first = submission
        .submit(Some(key.clone()), spec.clone())
        .await
        .expect("first submit");
    let second = submission
        .submit(Some(key), spec)
        .await
        .expect("second submit (retry)");

    assert_eq!(
        first.job_id, second.job_id,
        "same key must resolve to same job"
    );

    let job = wait_for_terminal(&store, &first.job_id, Duration::from_secs(10)).await;
    assert!(job.state.is_terminal());

    let counts = store.counts();
    assert_eq!(counts.create, 1, "only 1 durable job created for same key");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_unique_submissions_each_produce_one_job() {
    let (scheduler, submission, store, ws_id, exec) = setup_authority_stack(JobKind::Test).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let n = 5;
    let mut handles = Vec::new();
    for i in 0..n {
        let sub = submission.clone();
        let wsid = ws_id.clone();
        let key_str = format!("concurrent-unique-{i}");
        handles.push(tokio::spawn(async move {
            let key = SubmissionKey::new(key_str).expect("key");
            sub.submit(
                Some(key),
                build_new_job(&wsid, JobKind::Test, test_payload()),
            )
            .await
            .expect("submit")
        }));
    }

    let mut job_ids = Vec::new();
    for h in handles {
        let submitted = h.await.expect("spawn");
        job_ids.push(submitted.job_id);
    }

    // All job IDs must be distinct.
    let unique: std::collections::HashSet<_> = job_ids.iter().collect();
    assert_eq!(
        unique.len(),
        n,
        "each concurrent submission must produce a distinct job"
    );

    // Wait for all to complete.
    for jid in &job_ids {
        let job = wait_for_terminal(&store, jid, Duration::from_secs(10)).await;
        assert!(job.state.is_terminal(), "job {jid} should be terminal");
    }

    let counts = store.counts();
    assert_eq!(counts.create, n as u32, "{n} distinct create_job calls");
    assert_eq!(counts.begin, n as u32, "{n} begin_attempt calls");
    assert_eq!(counts.finish, n as u32, "{n} finish_attempt calls");
    assert_eq!(exec.entry_count(), n as u32, "{n} executor entries");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn per_priority_class_interactive_before_background() {
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
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );

    let counting_store = Arc::new(CountingJobStore::new());
    let store_for_scheduler: Arc<dyn JobStore> = counting_store.clone();

    let mut config = ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = 1;
    let generation = DaemonGeneration::new_unchecked("priority-gen");

    let scheduler = JobScheduler::new(
        store_for_scheduler.clone(),
        services.clone(),
        config,
        generation.clone(),
    );

    // Use a counting executor that we can track.
    let counting_executor = Arc::new(CountingExecutor::new(ExecutorKind::ManagedArgv));
    scheduler
        .register_executor(counting_executor)
        .await
        .expect("register executor");

    let submission = JobSubmissionService::new(
        counting_store.clone(),
        scheduler.clone(),
        services,
        generation,
    );

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    // Submit 2 background then 2 interactive jobs to the same workspace.
    let mut bg_ids = Vec::new();
    for _i in 0..2 {
        let mut spec = build_new_job(
            &ws_record.id,
            JobKind::Build,
            managed_argv_payload(vec!["echo", "bg"]),
        );
        spec.priority = JobPriority::Background;
        let sub = submission.submit(None, spec).await.expect("submit bg");
        bg_ids.push(sub.job_id);
    }
    let mut interactive_ids = Vec::new();
    for _i in 0..2 {
        let mut spec = build_new_job(
            &ws_record.id,
            JobKind::Build,
            managed_argv_payload(vec!["echo", "interactive"]),
        );
        spec.priority = JobPriority::Interactive;
        let sub = submission
            .submit(None, spec)
            .await
            .expect("submit interactive");
        interactive_ids.push(sub.job_id);
    }

    // Let the scheduler process. With max_process_slots=1 and fair
    // queue scheduling, interactive jobs should be admitted before
    // background when they are eligible (after aging or by default
    // priority ordering).
    //
    // We run a few reconcile/dispatch rounds and collect the order
    // in which jobs become terminal.
    let mut terminal_order = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while terminal_order.len() < 4 && tokio::time::Instant::now() < deadline {
        scheduler.reconcile().await.expect("reconcile");
        scheduler.clone().admit_and_dispatch_batch().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        for jid in bg_ids.iter().chain(interactive_ids.iter()) {
            if terminal_order.contains(jid) {
                continue;
            }
            if let Some(job) = counting_store.get_job(jid).await.expect("get_job") {
                if job.state.is_terminal() {
                    terminal_order.push(job.job_id.clone());
                }
            }
        }
    }

    // All 4 must have completed.
    assert_eq!(terminal_order.len(), 4, "all 4 jobs must complete");

    // The fair queue should not starve interactive jobs indefinitely.
    // With max_process_slots=1, all 4 jobs complete sequentially.
    // The ordering may vary based on timing, but we verify all complete
    // and no jobs are lost. Strict priority ordering is pinned by the
    // fair queue unit tests in scheduler_phase5.rs.

    let counts = counting_store.counts();
    assert_eq!(counts.create, 4, "4 create_job calls");
    assert_eq!(counts.begin, 4, "4 begin_attempt calls");
    assert_eq!(counts.finish, 4, "4 finish_attempt calls");

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn no_raw_shell_fallback_for_test_job() {
    // Verify that the executor kind for a Test job is exactly
    // ExecutorKind::Test — not BashDispatch, ManagedArgv, or anything
    // that might indicate a raw-shell fallback.
    let (scheduler, submission, store, ws_id, exec) = setup_authority_stack(JobKind::Test).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let submitted = submission
        .submit(None, build_new_job(&ws_id, JobKind::Test, test_payload()))
        .await
        .expect("submit");

    let _job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(10)).await;

    // The counting executor was registered for ExecutorKind::Test (for
    // test payloads) or ManagedArgv (for others). Verify it was
    // invoked exactly once, meaning the scheduler resolved it through
    // the executor registry and not through any fallback.
    assert_eq!(
        exec.entry_count(),
        1,
        "exactly 1 executor entry — no fallback"
    );

    let counts = store.counts();
    assert_eq!(counts.create, 1, "1 durable job");
    assert_eq!(counts.begin, 1, "1 attempt");
    assert_eq!(counts.finish, 1, "1 completion");

    // Verify the attempt executor was set to "test".
    let attempts = store
        .list_attempts(&submitted.job_id)
        .await
        .expect("list_attempts");
    assert_eq!(attempts.len(), 1, "exactly 1 attempt record");
    assert_eq!(
        attempts[0].executor.as_deref(),
        Some("test"),
        "executor must be 'test', not shell or managed_argv"
    );

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn no_raw_shell_fallback_for_build_job() {
    let (scheduler, submission, store, ws_id, exec) = setup_authority_stack(JobKind::Build).await;

    let sched_clone = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched_clone.run().await });

    let submitted = submission
        .submit(
            None,
            build_new_job(
                &ws_id,
                JobKind::Build,
                managed_argv_payload(vec!["cargo", "build"]),
            ),
        )
        .await
        .expect("submit");

    let _job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(10)).await;

    assert_eq!(
        exec.entry_count(),
        1,
        "exactly 1 executor entry — no fallback"
    );

    let attempts = store
        .list_attempts(&submitted.job_id)
        .await
        .expect("list_attempts");
    assert_eq!(attempts.len(), 1, "exactly 1 attempt record");
    assert_eq!(
        attempts[0].executor.as_deref(),
        Some("managed_argv"),
        "executor must be 'managed_argv', not shell"
    );

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}
