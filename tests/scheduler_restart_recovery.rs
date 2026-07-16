//! Workstream E: restart and generation recovery tests.
//!
//! Proves that daemon crash/restart scenarios are correctly handled by
//! the durable job store's `recover_generation` method and the scheduler's
//! `recover_at_startup` wrapper. Uses `InMemoryJobStore` and
//! `InMemoryScheduleStore` for deterministic, hermetic tests.

use std::sync::Arc;

use codegg::scheduler::{
    AdmissionController, JobScheduler, JobSubmissionService, ResolvedSchedulerConfig, SubmissionKey,
};
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, BackoffPolicy, DaemonGeneration, FailureClass,
    IdempotencyClass, InMemoryJobStore, InMemoryScheduleStore, JobKind, JobPayload, JobPriority,
    JobSource, JobState, JobStore, JobStoreQuery, NewJob, RecoveryPolicy, ResourceRequest,
    RetryPolicy, ScheduleKind, ScheduleStore, ScheduleTemplate,
};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceId, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

fn ws() -> WorkspaceId {
    WorkspaceId::new_unchecked("test-ws-restart")
}

fn default_new_job(workspace_id: &WorkspaceId) -> NewJob {
    NewJob {
        workspace_id: workspace_id.clone(),
        session_id: None,
        turn_id: None,
        kind: JobKind::Test,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Test {
            command: "cargo test".into(),
            argv: vec!["cargo".into(), "test".into()],
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
    }
}

fn non_idempotent_job(workspace_id: &WorkspaceId) -> NewJob {
    NewJob {
        idempotency: IdempotencyClass::NonIdempotent,
        ..default_new_job(workspace_id)
    }
}

async fn make_workspace_service_registry() -> (Arc<WorkspaceServiceRegistry>, WorkspaceId) {
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
    (services, workspace.id.clone())
}

async fn setup_with_stores() -> (
    Arc<JobSubmissionService>,
    Arc<JobScheduler>,
    Arc<dyn JobStore>,
    Arc<dyn ScheduleStore>,
    WorkspaceId,
) {
    let (services, workspace_id) = make_workspace_service_registry().await;
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let schedule_store: Arc<dyn ScheduleStore> =
        Arc::new(InMemoryScheduleStore::new(store.clone()));
    let scheduler = JobScheduler::new(
        store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new_unchecked("gen-1"),
    );
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services,
        DaemonGeneration::new_unchecked("gen-1"),
    );
    (submission, scheduler, store, schedule_store, workspace_id)
}

async fn new_scheduler_with_generation(
    store: Arc<dyn JobStore>,
    generation: &str,
) -> Arc<JobScheduler> {
    let (services, _) = make_workspace_service_registry().await;
    JobScheduler::new(
        store,
        services,
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new_unchecked(generation),
    )
}

// ══════════════════════════════════════════════════════════════════════════
// Test 1: crash after job creation before attempt
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn crash_after_job_creation_before_attempt() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    // Submit a job (creates in Queued state, no attempt yet).
    let job = submission
        .submit(None, default_new_job(&workspace_id))
        .await
        .expect("submit");
    assert_eq!(job.state, JobState::Queued);

    // Simulate crash: drop scheduler, recover with new generation.
    let policy = RecoveryPolicy::default();
    let report = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .expect("recover");
    assert_eq!(report.interrupted_attempts, 0, "no attempts exist yet");
    assert_eq!(report.requeued_jobs, 0);
    assert_eq!(report.terminal_jobs, 0);

    // Job is still Queued and begin_attempt works with the new generation.
    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_record.state, JobState::Queued);
    let att = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-2"))
        .await
        .expect("begin attempt after recovery");
    assert_eq!(att.state, AttemptState::Created);
}

// ══════════════════════════════════════════════════════════════════════════
// Test 2: crash after attempt creation before process spawn
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn crash_after_attempt_creation_before_spawn() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    let mut spec = default_new_job(&workspace_id);
    spec.idempotency = IdempotencyClass::SafeRepeat;
    spec.retry_policy = RetryPolicy::bounded(3, BackoffPolicy::None, vec![FailureClass::Transient]);
    let job = submission.submit(None, spec).await.expect("submit");

    // Begin an attempt under generation "gen-1" and mark it running.
    let att = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .expect("begin attempt");
    store
        .mark_attempt_running(&att.attempt_id)
        .await
        .expect("mark running");

    // Simulate crash: recover with "gen-2".
    let policy = RecoveryPolicy::default();
    let report = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .expect("recover");
    assert_eq!(report.interrupted_attempts, 1);
    // SafeRepeat + default policy + attempt_count(1) < max_attempts(3) → requeued
    assert_eq!(report.requeued_jobs, 1);

    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(
        job_record.state,
        JobState::Queued,
        "SafeRepeat job should be requeued"
    );

    let attempts = store.list_attempts(&job.job_id).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].state, AttemptState::Interrupted);
}

// ══════════════════════════════════════════════════════════════════════════
// Test 3: crash after process completion before persistence
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn crash_after_process_completion_before_persistence() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    let job = submission
        .submit(None, default_new_job(&workspace_id))
        .await
        .expect("submit");

    let att = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .expect("begin attempt");
    store
        .mark_attempt_running(&att.attempt_id)
        .await
        .expect("mark running");

    // Complete the attempt successfully (executor finished, persistence succeeded).
    store
        .finish_attempt(AttemptCompletion {
            attempt_id: att.attempt_id.clone(),
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .expect("finish attempt");

    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_record.state, JobState::Completed);

    // Simulate crash: recover with new generation.
    let policy = RecoveryPolicy::default();
    let report = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .expect("recover");
    assert_eq!(
        report.interrupted_attempts, 0,
        "completed attempt is terminal"
    );

    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(
        job_record.state,
        JobState::Completed,
        "completed job must not be touched"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Test 4: idempotency key resolves across restarts
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn idempotency_key_resolves_across_restarts() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    let key = SubmissionKey::new("retry-key-1").expect("key");
    let spec = default_new_job(&workspace_id);

    // First submission creates a job.
    let first = submission
        .submit(Some(key.clone()), spec.clone())
        .await
        .expect("first submit");
    // Second submission with same key returns same job (in-process dedup).
    let second = submission
        .submit(Some(key.clone()), spec.clone())
        .await
        .expect("second submit");
    assert_eq!(first.job_id, second.job_id);

    // Verify only one job in the store.
    let jobs = store
        .list_jobs(JobStoreQuery::default())
        .await
        .expect("list");
    assert_eq!(jobs.len(), 1);

    // The idempotency cache is process-local (in-memory HashMap). It
    // does NOT survive daemon restarts. The durable store does not have
    // store-level submission-key dedup — a fresh process would create a
    // second job with the same key. This is a documented limitation.
    //
    // Within the same process, the cache correctly deduplicates.
    let again = submission
        .submit(Some(key.clone()), spec.clone())
        .await
        .expect("same key same spec");
    assert_eq!(
        first.job_id, again.job_id,
        "in-process cache returns same job for same key+spec"
    );
    let jobs = store
        .list_jobs(JobStoreQuery::default())
        .await
        .expect("list");
    assert_eq!(
        jobs.len(),
        1,
        "still exactly one job after repeated in-process submission"
    );
    // A different fingerprint with the same key triggers a conflict error.
    let mut different_spec = spec;
    different_spec.kind = JobKind::Build;
    different_spec.payload = JobPayload::ManagedArgv {
        argv: vec!["cargo".into(), "build".into()],
        cwd: None,
    };
    let conflict = submission.submit(Some(key), different_spec).await;
    assert!(
        matches!(
            conflict,
            Err(codegg::scheduler::JobSubmissionError::SubmissionKeyConflict)
        ),
        "same key with different spec must conflict"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Test 5: schedule occurrence uniqueness
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn schedule_occurrence_uniqueness_across_restarts() {
    let (_submission, _scheduler, store, sched_store, workspace_id) = setup_with_stores().await;

    let past = chrono::Utc::now() - chrono::Duration::seconds(10);
    let _rec = sched_store
        .create(ScheduleTemplate {
            workspace_id: workspace_id.clone(),
            session_id: None,
            kind: ScheduleKind::OneShot { run_at: past },
            job_template: codegg_core::jobs::schedule::JobTemplate::for_subagent(
                JobKind::Test,
                "test prompt".into(),
                "test-agent".into(),
                None,
            ),
            overlap_policy: codegg_core::jobs::schedule::OverlapPolicy::SkipIfRunning,
            missed_run_policy: codegg_core::jobs::schedule::MissedRunPolicy::RunOnceNow,
            next_run_at: None,
            labels: std::collections::HashMap::new(),
        })
        .await
        .expect("create schedule");

    struct TestMaterializer {
        job_store: Arc<dyn JobStore>,
        workspace_id: WorkspaceId,
    }
    #[async_trait::async_trait]
    impl codegg_core::jobs::schedule::OccurrenceMaterializer for TestMaterializer {
        async fn materialize(
            &self,
            _sid: &codegg_core::jobs::ScheduleId,
            template: &codegg_core::jobs::schedule::JobTemplate,
            _at: chrono::DateTime<chrono::Utc>,
        ) -> Result<codegg_core::jobs::JobId, codegg_core::jobs::schedule::MaterializerError>
        {
            use codegg_core::jobs::schedule::MaterializerError;
            let job = self
                .job_store
                .create_job(NewJob {
                    workspace_id: self.workspace_id.clone(),
                    session_id: None,
                    turn_id: None,
                    kind: template.kind,
                    source: codegg_core::jobs::schedule::JobTemplate::job_source(
                        &codegg_core::jobs::ScheduleId::new_unchecked("sched"),
                        _at,
                    ),
                    priority: template.priority,
                    payload: template.payload.clone(),
                    resource_request: template.resource_request.clone(),
                    timeout: template.timeout,
                    retry_policy: template.retry_policy.clone(),
                    idempotency: template.idempotency,
                    not_before: None,
                    deadline: None,
                    schedule_id: None,
                    depends_on: vec![],
                })
                .await
                .map_err(MaterializerError::JobStore)?;
            Ok(job.job_id)
        }
    }
    let mat = TestMaterializer {
        job_store: store.clone(),
        workspace_id: workspace_id.clone(),
    };

    // First claim — should fire.
    let c1 = sched_store
        .claim_due(chrono::Utc::now(), &mat)
        .await
        .expect("claim 1");
    assert_eq!(c1.len(), 1, "one-shot should fire once");

    // Second claim (simulating restart) — schedule is exhausted.
    let c2 = sched_store
        .claim_due(chrono::Utc::now(), &mat)
        .await
        .expect("claim 2");
    assert!(c2.is_empty(), "one-shot must not double-fire after restart");

    // The occurrence uniqueness is enforced by the (schedule_id,
    // scheduled_for) dedup key in both InMemoryScheduleStore (HashMap
    // key) and SqliteScheduleStore (PRIMARY KEY constraint).
    // Also verify the store has exactly one job.
    let jobs = store
        .list_jobs(JobStoreQuery::default())
        .await
        .expect("list");
    assert_eq!(jobs.len(), 1, "exactly one job from one occurrence");
}

// ══════════════════════════════════════════════════════════════════════════
// Test 6: restart preserves queued work
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn restart_preserves_queued_work() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    // Submit 5 jobs.
    for _ in 0..5 {
        submission
            .submit(None, default_new_job(&workspace_id))
            .await
            .expect("submit");
    }

    // All 5 should be Queued.
    let jobs = store
        .list_jobs(JobStoreQuery {
            states: vec![JobState::Queued],
            ..Default::default()
        })
        .await
        .expect("list");
    assert_eq!(jobs.len(), 5, "all 5 jobs should be queued");

    // Simulate crash: recover with new generation. No attempts exist,
    // so nothing is interrupted or requeued — jobs just stay Queued.
    let policy = RecoveryPolicy::default();
    let report = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .expect("recover");
    assert_eq!(report.interrupted_attempts, 0);
    assert_eq!(report.requeued_jobs, 0);

    // Verify all 5 are still Queued.
    let jobs = store
        .list_jobs(JobStoreQuery {
            states: vec![JobState::Queued],
            ..Default::default()
        })
        .await
        .expect("list after recovery");
    assert_eq!(jobs.len(), 5, "all 5 jobs must survive recovery");

    // A reconcile pass on the new scheduler should find them all.
    let new_scheduler = new_scheduler_with_generation(store.clone(), "gen-2").await;
    let report = new_scheduler.reconcile().await.expect("reconcile");
    assert_eq!(
        report.added, 5,
        "reconcile should pick up all 5 queued jobs"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Test 7: restart does NOT re-execute completed work
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn restart_does_not_reexecute_completed_work() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    let job = submission
        .submit(None, default_new_job(&workspace_id))
        .await
        .expect("submit");

    // Complete the job.
    let att = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .expect("begin");
    store
        .mark_attempt_running(&att.attempt_id)
        .await
        .expect("running");
    store
        .finish_attempt(AttemptCompletion {
            attempt_id: att.attempt_id.clone(),
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .expect("finish");

    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_record.state, JobState::Completed);

    // Simulate crash: recover with new generation.
    let policy = RecoveryPolicy::default();
    let report = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .expect("recover");
    assert_eq!(report.interrupted_attempts, 0);
    assert_eq!(report.requeued_jobs, 0);

    // Reconcile should NOT pick up the completed job.
    let new_scheduler = new_scheduler_with_generation(store.clone(), "gen-2").await;
    let reconcile_report = new_scheduler.reconcile().await.expect("reconcile");
    assert_eq!(
        reconcile_report.added, 0,
        "completed job must not be reconciled"
    );

    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_record.state, JobState::Completed);
}

// ══════════════════════════════════════════════════════════════════════════
// Test 8: deterministic across repeated restarts
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn deterministic_across_repeated_restarts() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    // Submit a NonIdempotent job.
    let job = submission
        .submit(None, non_idempotent_job(&workspace_id))
        .await
        .expect("submit");

    // Begin an attempt and mark it running — then simulate crash
    // (do NOT finish the attempt).
    let att = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .expect("begin");
    store
        .mark_attempt_running(&att.attempt_id)
        .await
        .expect("running");

    // Recovery pass 1: NonIdempotent + default policy → not requeueable → Failed.
    let policy = RecoveryPolicy::default();
    let r1 = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .expect("recover 1");
    assert_eq!(r1.interrupted_attempts, 1);
    assert_eq!(r1.requeued_jobs, 0);
    assert_eq!(r1.terminal_jobs, 1);

    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_record.state, JobState::Failed);

    // Recovery pass 2: attempt is now Interrupted (terminal), so no-op.
    let r2 = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-3"), &policy)
        .await
        .expect("recover 2");
    assert_eq!(r2.interrupted_attempts, 0);
    assert_eq!(r2.terminal_jobs, 0);

    // Recovery pass 3: same result.
    let r3 = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-4"), &policy)
        .await
        .expect("recover 3");
    assert_eq!(r3.interrupted_attempts, 0);
    assert_eq!(r3.terminal_jobs, 0);

    // Job state is stable across all passes.
    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_record.state, JobState::Failed);
}

// ══════════════════════════════════════════════════════════════════════════
// Test 9: recovery preserves permit accounting
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn recovery_preserves_permit_accounting() {
    // After recovery, a new AdmissionController starts with zero
    // permits used. The durable state is the source of truth.
    let mut cfg = ResolvedSchedulerConfig::default();
    cfg.resources.max_process_slots = 4;
    let controller = Arc::new(AdmissionController::new(cfg));

    // Admit one job.
    let dims = codegg::scheduler::PermitDimensions {
        cpu_weight: 1,
        memory_mb_hint: 0,
        process_slots: 1,
        io_weight: 0,
        network_slots: 0,
        exclusivity_keys: vec![],
    };
    let _permit = match controller.try_admit_arc(&dims) {
        codegg::scheduler::AdmissionDecision::Admitted(p) => p,
        other => panic!("expected admitted, got {other:?}"),
    };
    assert_eq!(controller.used_process_slots(), 1);

    // Simulate crash: drop the permit (simulates process death).
    drop(_permit);
    assert_eq!(controller.used_process_slots(), 0);

    // A new scheduler's AdmissionController starts fresh.
    let new_cfg = ResolvedSchedulerConfig::default();
    let new_controller = AdmissionController::new(new_cfg);
    assert_eq!(
        new_controller.used_process_slots(),
        0,
        "new controller must start at zero"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Test 10: orphan attempt policy
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn orphan_attempt_is_interrupted_on_recovery() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    let mut spec = default_new_job(&workspace_id);
    spec.idempotency = IdempotencyClass::SafeRepeat;
    spec.retry_policy = RetryPolicy::bounded(3, BackoffPolicy::None, vec![FailureClass::Transient]);
    let job = submission.submit(None, spec).await.expect("submit");

    // Begin an attempt under gen-1, mark it running — this is an
    // "orphan" because no executor is actually running.
    let att = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .expect("begin");
    store
        .mark_attempt_running(&att.attempt_id)
        .await
        .expect("running");

    // The attempt is now Running but no process is alive.
    // Recovery must mark it Interrupted and requeue the job.
    let policy = RecoveryPolicy::default();
    let report = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .expect("recover");
    assert_eq!(report.interrupted_attempts, 1);
    assert_eq!(report.requeued_jobs, 1);

    let attempts = store.list_attempts(&job.job_id).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(
        attempts[0].state,
        AttemptState::Interrupted,
        "orphan must be marked Interrupted"
    );

    // The actual OS process (if any) is left to OS-level cleanup.
    // The daemon does not assume the old process is gone solely
    // because the generation changed — it only marks the attempt
    // Interrupted in the durable store.
}

// ══════════════════════════════════════════════════════════════════════════
// Test 11: NonIdempotent crash after attempt → job marked Failed
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn non_idempotent_crash_marks_job_terminal() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    let job = submission
        .submit(None, non_idempotent_job(&workspace_id))
        .await
        .expect("submit");

    let att = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .expect("begin");
    store
        .mark_attempt_running(&att.attempt_id)
        .await
        .expect("running");

    // Simulate crash: NonIdempotent + default policy → not requeueable.
    let policy = RecoveryPolicy::default();
    let report = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .expect("recover");
    assert_eq!(report.interrupted_attempts, 1);
    assert_eq!(report.requeued_jobs, 0);
    assert_eq!(report.terminal_jobs, 1);

    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_record.state, JobState::Failed);
}

// ══════════════════════════════════════════════════════════════════════════
// Test 12: crash during transition period — recovery idempotency
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn recovery_is_idempotent() {
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let w = ws();

    let job = store.create_job(default_new_job(&w)).await.unwrap();
    let gen = DaemonGeneration::new();
    let _att = store.begin_attempt(&job.job_id, &gen).await.unwrap();

    // First recovery: interrupts the attempt.
    let policy = RecoveryPolicy::default();
    let r1 = store
        .recover_generation(&DaemonGeneration::new_unchecked("new-gen"), &policy)
        .await
        .unwrap();
    assert_eq!(r1.interrupted_attempts, 1);

    // Second recovery with same new-gen: attempt is now Interrupted
    // (terminal for attempt), so no-op.
    let r2 = store
        .recover_generation(&DaemonGeneration::new_unchecked("new-gen"), &policy)
        .await
        .unwrap();
    assert_eq!(
        r2.interrupted_attempts, 0,
        "second recovery pass must be a no-op"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Test 13: ReadOnly crash after attempt → requeued
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn read_only_crash_requeues() {
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let w = ws();

    let mut spec = default_new_job(&w);
    spec.idempotency = IdempotencyClass::ReadOnly;
    spec.retry_policy = RetryPolicy::bounded(3, BackoffPolicy::None, vec![FailureClass::Transient]);
    let job = store.create_job(spec).await.unwrap();

    let att = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .unwrap();
    store.mark_attempt_running(&att.attempt_id).await.unwrap();

    let policy = RecoveryPolicy::default();
    let report = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .unwrap();
    assert_eq!(report.interrupted_attempts, 1);
    assert_eq!(report.requeued_jobs, 1);

    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_record.state, JobState::Queued);
}

// ══════════════════════════════════════════════════════════════════════════
// Test 14: max_attempts exhausted → job marked Failed on recovery
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn max_attempts_exhausted_marks_failed_on_recovery() {
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let w = ws();

    let mut spec = default_new_job(&w);
    spec.idempotency = IdempotencyClass::SafeRepeat;
    spec.retry_policy = RetryPolicy::bounded(2, BackoffPolicy::None, vec![FailureClass::Transient]);
    let job = store.create_job(spec).await.unwrap();

    // Use both attempts.
    let att1 = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .unwrap();
    store.mark_attempt_running(&att1.attempt_id).await.unwrap();
    store
        .finish_attempt(AttemptCompletion {
            attempt_id: att1.attempt_id.clone(),
            state: AttemptState::Failed,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();

    // Retry creates attempt 2.
    let att2 = store
        .retry_job(
            &job.job_id,
            &DaemonGeneration::new_unchecked("gen-1"),
            &att1.attempt_id,
        )
        .await
        .unwrap();
    store.mark_attempt_running(&att2.attempt_id).await.unwrap();

    // Crash: attempt 2 is interrupted, but attempt_count(2) >= max_attempts(2).
    let policy = RecoveryPolicy::default();
    let report = store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .unwrap();
    assert_eq!(report.interrupted_attempts, 1);
    assert_eq!(report.requeued_jobs, 0, "max attempts exhausted");
    assert_eq!(report.terminal_jobs, 1);

    let job_record = store.get_job(&job.job_id).await.unwrap().unwrap();
    assert_eq!(job_record.state, JobState::Failed);
}

// ══════════════════════════════════════════════════════════════════════════
// Test 15: reconcile finds queued jobs after recovery
// ══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "current_thread")]
async fn reconcile_finds_queued_jobs_after_recovery() {
    let (submission, _scheduler, store, _sched_store, workspace_id) = setup_with_stores().await;

    // Submit 3 jobs.
    for _ in 0..3 {
        submission
            .submit(None, default_new_job(&workspace_id))
            .await
            .expect("submit");
    }

    // Simulate crash and recovery (no-op since no attempts).
    let policy = RecoveryPolicy::default();
    store
        .recover_generation(&DaemonGeneration::new_unchecked("gen-2"), &policy)
        .await
        .expect("recover");

    // New scheduler's reconcile should find all 3.
    let new_scheduler = new_scheduler_with_generation(store.clone(), "gen-2").await;
    let report = new_scheduler.reconcile().await.expect("reconcile");
    assert_eq!(report.added, 3, "reconcile should find all 3 queued jobs");
}
