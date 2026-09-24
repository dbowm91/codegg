//! Identity / audit corrective M003 — scheduler terminal `job_complete`.
//!
//! Live audit at the durable terminal attempt transition for success,
//! failure, cancellation/interruption, and other bounded outcomes:
//!
//! - success / failure / cancelled / interrupted terminals emit one
//!   structural `job_complete` with truthful outcome;
//! - retry keeps the failed prior attempt auditable and scopes the next
//!   attempt separately (no collapse, no duplicate);
//! - replayed completions and restart reconciliation of an already-terminal
//!   attempt emit no duplicate;
//! - legacy rows without durable attribution fall back to explicit
//!   `legacy-local` rather than a fabricated principal;
//! - audit store failure never corrupts scheduler terminalization and
//!   surfaces the existing failure counters.
//!
//! Metadata carries ids, bounded outcome/state labels, and the decision
//! outcome only: no job payload, tool output, command text, or secrets.

use std::sync::Arc;
use std::time::Duration;

use codegg::scheduler::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, JobExecutionContext, JobExecutor,
    JobScheduler, ResolvedSchedulerConfig,
};
use codegg_core::audit::{AuditAction, AuditQueryFilter, AuditStore};
use codegg_core::audit_instrumentation::{
    self as instr, emit_counters_snapshot, ExecutionAuditEmitter,
};
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, DaemonGeneration, ExecutionTarget, IdempotencyClass,
    InMemoryJobStore, JobKind, JobPayload, JobPriority, JobSource, JobStore, NewJob,
    ResourceRequest, RetryPolicy,
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

async fn test_services() -> (
    Arc<WorkspaceServiceRegistry>,
    codegg_core::workspace::WorkspaceId,
) {
    let root = tempfile::tempdir().expect("temp workspace");
    // Leak the tempdir so the workspace root survives for the scheduler run.
    let root = Box::leak(Box::new(root));
    let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("registry");
    let record = registry
        .get_or_register(root.path())
        .await
        .expect("register");
    let services = WorkspaceServiceRegistry::new(
        registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    (services, record.id.clone())
}

fn new_job(workspace: codegg_core::workspace::WorkspaceId, session: Option<String>) -> NewJob {
    NewJob {
        workspace_id: workspace,
        session_id: session,
        turn_id: Some("turn-m003".to_owned()),
        kind: JobKind::Test,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Test {
            command: "echo m003".into(),
            argv: vec!["echo".into(), "m003".into()],
            cwd: Some("/tmp".into()),
            scope: None,
            parent_run_id: None,
        },
        resource_request: ResourceRequest::for_kind(JobKind::Test),
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

struct FixedExecutor(codegg::scheduler::ExecutorStatus);

#[async_trait::async_trait]
impl JobExecutor for FixedExecutor {
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
            status: self.0,
            summary: format!("m003-terminal-{:?}", self.0),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}

async fn events_for_job(
    pool: &sqlx::SqlitePool,
    job_id: &str,
) -> Vec<codegg_core::audit::AuditEvent> {
    let store = AuditStore::new(pool.clone());
    let page = store
        .query(&AuditQueryFilter {
            project_id: None,
            action: Some(AuditAction::JobComplete.as_str().to_owned()),
            principal: None,
            from_seq: None,
            limit: 200,
        })
        .await
        .expect("query audit");
    page.events
        .into_iter()
        .filter(|event| {
            event.job_id.as_deref() == Some(job_id)
                || event.metadata.get("job.id").map(String::as_str) == Some(job_id)
        })
        .collect()
}

fn assert_structural_only(event: &codegg_core::audit::AuditEvent) {
    let blob = serde_json::to_string(&event.metadata).unwrap_or_default();
    assert!(
        !blob.contains("echo m003"),
        "job payload leaked into audit: {blob}"
    );
    // Bounded metadata keys only: job locators, outcomes, correlation.
    for key in event.metadata.keys() {
        assert!(
            matches!(
                key.as_str(),
                "job.id" | "job.outcome" | "decision.outcome" | "run.id" | "session.id"
            ) || key.starts_with("session.")
                || key.starts_with("turn.")
                || key.starts_with("run.")
                || key.starts_with("job.")
                || key.starts_with("decision."),
            "unexpected audit metadata key {key}"
        );
    }
    assert!(
        event.body_ref.is_none(),
        "terminal event must not carry a body"
    );
    assert!(
        event.content_digest.is_none(),
        "terminal event must not carry content"
    );
}

async fn record_attribution(
    pool: &sqlx::SqlitePool,
    job_id: &str,
    correlation: &str,
) -> codegg_core::authorization::OriginAttribution {
    let principal = codegg_core::transport_auth::AuthenticatedPrincipal::local_owner("client-m003");
    let decision = codegg_core::authorization::AuthorizationDecision {
        principal_id: principal.principal_id().clone(),
        operation: "job_submit".to_owned(),
        capability: None,
        project_id: None,
        membership_revision: None,
        policy: codegg_core::authorization::PolicyKind::LocalOwnerBroad,
        decision_id: format!("decision-{correlation}"),
        correlation_id: correlation.to_owned(),
        reason: "test".to_owned(),
        decided_at_ms: 0,
    };
    let attribution =
        codegg_core::authorization::OriginAttribution::from_authority(&principal, &decision);
    codegg_core::authorization::OriginAttributionStore::new(pool.clone())
        .record("job", job_id, &attribution)
        .await
        .expect("record attribution")
}

async fn run_terminal(
    pool: sqlx::SqlitePool,
    status: codegg::scheduler::ExecutorStatus,
    with_attribution: bool,
) -> (String, Vec<codegg_core::audit::AuditEvent>) {
    let (services, workspace) = test_services().await;
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store.clone(),
        services,
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new(),
    );
    scheduler
        .set_audit_emitter(ExecutionAuditEmitter::new(Some(pool.clone())))
        .await;
    scheduler
        .register_executor(Arc::new(FixedExecutor(status)))
        .await
        .expect("register executor");
    let job = store
        .create_job(new_job(workspace, Some("session-m003".to_owned())))
        .await
        .expect("create job");
    let job_id = job.job_id.as_str().to_owned();
    let correlation = format!("corr-m003-{job_id}");
    if with_attribution {
        record_attribution(&pool, &job_id, &correlation).await;
    }
    scheduler.enqueue_existing(job).await.expect("enqueue job");
    let handle = scheduler.spawn_run();
    let completed = scheduler
        .wait_for_completion(
            &codegg_core::jobs::JobId::new_unchecked(job_id.clone()),
            Duration::from_secs(10),
        )
        .await
        .expect("terminal completion");
    assert_eq!(completed.status, status);
    // Allow the spawned terminal task to persist + emit before shutdown.
    for _ in 0..50 {
        let rows = events_for_job(&pool, &job_id).await;
        if !rows.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = handle.await;
    let rows = events_for_job(&pool, &job_id).await;
    (job_id, rows)
}

#[tokio::test(flavor = "current_thread")]
async fn success_terminal_emits_truthful_event() {
    let pool = test_pool().await;
    let (job_id, rows) = run_terminal(
        pool.clone(),
        codegg::scheduler::ExecutorStatus::Completed,
        true,
    )
    .await;
    assert_eq!(rows.len(), 1, "one success terminal → one event");
    let event = &rows[0];
    assert_eq!(event.action, "job_complete");
    assert_eq!(
        event.metadata.get("job.id").map(String::as_str),
        Some(job_id.as_str())
    );
    assert_eq!(
        event.metadata.get("job.outcome").map(String::as_str),
        Some("success")
    );
    assert_eq!(
        event.metadata.get("decision.outcome").map(String::as_str),
        Some("allow")
    );
    assert_eq!(event.session_id.as_deref(), Some("session-m003"));
    assert_eq!(event.turn_id.as_deref(), Some("turn-m003"));
    assert_structural_only(event);
}

#[tokio::test(flavor = "current_thread")]
async fn failure_terminal_emits_truthful_event() {
    let pool = test_pool().await;
    let (job_id, rows) = run_terminal(
        pool.clone(),
        codegg::scheduler::ExecutorStatus::Failed,
        true,
    )
    .await;
    assert_eq!(rows.len(), 1, "one failure terminal → one event");
    assert_eq!(
        rows[0].metadata.get("job.outcome").map(String::as_str),
        Some("failure")
    );
    assert_eq!(
        rows[0].metadata.get("job.id").map(String::as_str),
        Some(job_id.as_str())
    );
    assert_structural_only(&rows[0]);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_and_interrupted_terminals_stay_distinguishable() {
    for (status, outcome) in [
        (codegg::scheduler::ExecutorStatus::Cancelled, "cancelled"),
        (
            codegg::scheduler::ExecutorStatus::Interrupted,
            "interrupted",
        ),
        (codegg::scheduler::ExecutorStatus::TimedOut, "timed_out"),
    ] {
        let pool = test_pool().await;
        let (_, rows) = run_terminal(pool, status, true).await;
        assert_eq!(rows.len(), 1, "one {outcome} terminal → one event");
        assert_eq!(
            rows[0].metadata.get("job.outcome").map(String::as_str),
            Some(outcome)
        );
        assert_structural_only(&rows[0]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn retry_keeps_prior_attempt_and_scopes_next_attempt() {
    let pool = test_pool().await;
    let (services, workspace) = test_services().await;
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store.clone(),
        services,
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new(),
    );
    let emitter = ExecutionAuditEmitter::new(Some(pool.clone()));
    scheduler.set_audit_emitter(emitter.clone()).await;
    scheduler
        .register_executor(Arc::new(FixedExecutor(
            codegg::scheduler::ExecutorStatus::Failed,
        )))
        .await
        .expect("register executor");

    let mut spec = new_job(workspace, Some("session-m003-retry".to_owned()));
    spec.retry_policy = RetryPolicy::bounded(2, codegg_core::jobs::BackoffPolicy::None, vec![]);
    let job = store.create_job(spec).await.expect("create job");
    let job_id = job.job_id.as_str().to_owned();
    record_attribution(&pool, &job_id, &format!("corr-retry-{job_id}")).await;
    scheduler.enqueue_existing(job).await.expect("enqueue");
    let handle = scheduler.spawn_run();
    let completed = scheduler
        .wait_for_completion(
            &codegg_core::jobs::JobId::new_unchecked(job_id.clone()),
            Duration::from_secs(10),
        )
        .await
        .expect("first terminal");
    assert_eq!(completed.status, codegg::scheduler::ExecutorStatus::Failed);
    for _ in 0..50 {
        if !events_for_job(&pool, &job_id).await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = handle.await;

    let first_rows = events_for_job(&pool, &job_id).await;
    assert_eq!(first_rows.len(), 1);
    assert_eq!(
        first_rows[0]
            .metadata
            .get("job.outcome")
            .map(String::as_str),
        Some("failure")
    );
    let first_event_id = first_rows[0].event_id.clone();

    // Retry creates the next attempt; the failed prior attempt remains
    // terminal and auditable. Drive the next terminal directly through the
    // store + terminal emit path (same owner semantics as the executor
    // task): finish the retried attempt as success and emit once.
    let attempts = store
        .list_attempts(&codegg_core::jobs::JobId::new_unchecked(job_id.clone()))
        .await
        .expect("list attempts");
    let prior = attempts.last().expect("prior attempt").attempt_id.clone();
    let generation = DaemonGeneration::new();
    let next = store
        .retry_job(
            &codegg_core::jobs::JobId::new_unchecked(job_id.clone()),
            &generation,
            &prior,
        )
        .await
        .expect("retry creates next attempt");
    assert_ne!(next.attempt_id, prior);
    store
        .mark_attempt_running(&next.attempt_id)
        .await
        .expect("mark running");
    let terminal = store
        .finish_attempt(AttemptCompletion {
            attempt_id: next.attempt_id.clone(),
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .expect("finish next attempt");
    let ctx = codegg::scheduler::job_complete_audit::trusted_context_for_terminal_job(
        &emitter, &terminal, None,
    )
    .await;
    let scope = codegg::scheduler::job_complete_audit::terminal_scope(&next.attempt_id, "success");
    codegg::scheduler::job_complete_audit::emit_job_complete(
        &emitter,
        &ctx,
        &terminal.job_id,
        "success",
        &scope,
    )
    .await;

    let rows = events_for_job(&pool, &job_id).await;
    assert_eq!(rows.len(), 2, "prior failure + retry success stay distinct");
    let outcomes: Vec<_> = rows
        .iter()
        .map(|event| {
            event
                .metadata
                .get("job.outcome")
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    assert!(outcomes.contains(&"failure".to_owned()));
    assert!(outcomes.contains(&"success".to_owned()));
    assert!(
        !rows.iter().any(|event| event.event_id == first_event_id
            && event.metadata.get("job.outcome").map(String::as_str) == Some("success")),
        "retry successor must scope a distinct event id"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn replayed_completion_and_restart_emit_no_duplicate() {
    let pool = test_pool().await;
    let (services, workspace) = test_services().await;
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store.clone(),
        services,
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new(),
    );
    let emitter = ExecutionAuditEmitter::new(Some(pool.clone()));
    scheduler.set_audit_emitter(emitter.clone()).await;
    scheduler
        .register_executor(Arc::new(FixedExecutor(
            codegg::scheduler::ExecutorStatus::Completed,
        )))
        .await
        .expect("register executor");
    let job = store
        .create_job(new_job(workspace, Some("session-m003-replay".to_owned())))
        .await
        .expect("create job");
    let job_id = job.job_id.as_str().to_owned();
    record_attribution(&pool, &job_id, &format!("corr-replay-{job_id}")).await;
    scheduler.enqueue_existing(job).await.expect("enqueue");
    let handle = scheduler.spawn_run();
    scheduler
        .wait_for_completion(
            &codegg_core::jobs::JobId::new_unchecked(job_id.clone()),
            Duration::from_secs(10),
        )
        .await
        .expect("terminal");
    for _ in 0..50 {
        if !events_for_job(&pool, &job_id).await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = handle.await;
    let rows = events_for_job(&pool, &job_id).await;
    assert_eq!(rows.len(), 1);

    // Replayed completion of the already-terminal attempt fails at the
    // store before any emit: no duplicate terminal row.
    let attempts = store
        .list_attempts(&codegg_core::jobs::JobId::new_unchecked(job_id.clone()))
        .await
        .expect("list attempts");
    let attempt_id = attempts.last().expect("attempt").attempt_id.clone();
    let replay = store
        .finish_attempt(AttemptCompletion {
            attempt_id: attempt_id.clone(),
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await;
    assert!(replay.is_err(), "replay of terminal attempt must fail");
    let rows = events_for_job(&pool, &job_id).await;
    assert_eq!(
        rows.len(),
        1,
        "replay must not duplicate the terminal event"
    );

    // Restart reconciliation over an already-terminal job emits nothing.
    let report = store
        .recover_generation(
            &DaemonGeneration::new(),
            &codegg_core::jobs::RecoveryPolicy::default(),
        )
        .await
        .expect("recover");
    let _ = report;
    let rows = events_for_job(&pool, &job_id).await;
    assert_eq!(rows.len(), 1, "recovery of terminal job must not duplicate");
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_row_without_attribution_uses_explicit_fallback() {
    let pool = test_pool().await;
    let (job_id, rows) = run_terminal(
        pool.clone(),
        codegg::scheduler::ExecutorStatus::Completed,
        false,
    )
    .await;
    assert_eq!(rows.len(), 1, "legacy terminal still emits once");
    let event = &rows[0];
    assert_eq!(
        event.metadata.get("job.id").map(String::as_str),
        Some(job_id.as_str())
    );
    assert_eq!(
        event.metadata.get("job.outcome").map(String::as_str),
        Some("success")
    );
    // Explicit legacy-local attribution: local-owner binding, never a
    // fabricated team principal.
    assert_eq!(event.decision_id, "legacy-local");
    assert_eq!(
        event.actor_principal.as_str(),
        "local-owner",
        "legacy fallback must not invent a human principal"
    );
    assert_structural_only(event);
}

#[tokio::test(flavor = "current_thread")]
async fn audit_store_failure_keeps_terminalization_and_counts_failure() {
    let pool = test_pool().await;
    let (services, workspace) = test_services().await;
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store.clone(),
        services,
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new(),
    );
    // Closed pool: every audit append times out/fails with the shared
    // bounded policy. Scheduler terminalization (InMemory store) must
    // still succeed.
    pool.close().await;
    let before = emit_counters_snapshot();
    scheduler
        .set_audit_emitter(
            ExecutionAuditEmitter::new(Some(pool.clone())).with_timeout(Duration::from_millis(20)),
        )
        .await;
    scheduler
        .register_executor(Arc::new(FixedExecutor(
            codegg::scheduler::ExecutorStatus::Completed,
        )))
        .await
        .expect("register executor");
    let job = store
        .create_job(new_job(workspace, Some("session-m003-failure".to_owned())))
        .await
        .expect("create job");
    let job_id = job.job_id.clone();
    scheduler.enqueue_existing(job).await.expect("enqueue");
    let handle = scheduler.spawn_run();
    let completed = scheduler
        .wait_for_completion(&job_id, Duration::from_secs(10))
        .await
        .expect("terminal despite audit failure");
    assert_eq!(
        completed.status,
        codegg::scheduler::ExecutorStatus::Completed
    );
    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = handle.await;
    // Durable terminal state is accepted even though audit failed.
    let record = store
        .get_job(&job_id)
        .await
        .expect("get job")
        .expect("job exists");
    assert!(
        record.state.is_terminal(),
        "audit failure must not corrupt terminalization"
    );
    // Allow the failed emit to record its counter.
    for _ in 0..20 {
        let after = emit_counters_snapshot();
        if after.failed > before.failed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let after = emit_counters_snapshot();
    assert!(
        after.failed > before.failed,
        "audit failure must increment the existing failure counter"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn queued_cancel_terminal_emits_once_and_replay_stays_silent() {
    let pool = test_pool().await;
    let (services, workspace) = test_services().await;
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store.clone(),
        services,
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new(),
    );
    scheduler
        .set_audit_emitter(ExecutionAuditEmitter::new(Some(pool.clone())))
        .await;
    let job = store
        .create_job(new_job(
            workspace,
            Some("session-m003-queued-cancel".to_owned()),
        ))
        .await
        .expect("create job");
    let job_id = job.job_id.as_str().to_owned();
    record_attribution(&pool, &job_id, &format!("corr-queued-cancel-{job_id}")).await;
    // Queued cancel terminalizes without an attempt: one `cancelled` event.
    let result = scheduler
        .request_cancel(
            &codegg_core::jobs::JobId::new_unchecked(job_id.clone()),
            "test queued cancel",
        )
        .await
        .expect("request cancel");
    assert_eq!(result.state, codegg_core::jobs::CancelOutcome::Cancelled);
    for _ in 0..50 {
        if !events_for_job(&pool, &job_id).await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let rows = events_for_job(&pool, &job_id).await;
    assert_eq!(rows.len(), 1, "queued cancel → one terminal event");
    assert_eq!(
        rows[0].metadata.get("job.outcome").map(String::as_str),
        Some("cancelled")
    );
    assert_structural_only(&rows[0]);
    // Replay of an already-terminal cancel emits nothing.
    let replay = scheduler
        .request_cancel(
            &codegg_core::jobs::JobId::new_unchecked(job_id.clone()),
            "test queued cancel replay",
        )
        .await
        .expect("replay cancel");
    assert_eq!(
        replay.state,
        codegg_core::jobs::CancelOutcome::AlreadyTerminal
    );
    let rows = events_for_job(&pool, &job_id).await;
    assert_eq!(rows.len(), 1, "AlreadyTerminal replay must not duplicate");
}

#[test]
fn job_complete_audit_helpers_stay_deterministic_and_bounded() {
    let attempt = codegg_core::jobs::AttemptId::new_unchecked("attempt-scope");
    let scope = codegg::scheduler::job_complete_audit::terminal_scope(&attempt, "success");
    assert_eq!(
        scope,
        codegg::scheduler::job_complete_audit::terminal_scope(&attempt, "success")
    );
    // Deterministic event ids reuse the stored row instead of duplicating.
    let first = instr::deterministic_event_id(
        "decision-m003",
        &AuditAction::JobComplete,
        "corr-m003",
        &scope,
    );
    let replay = instr::deterministic_event_id(
        "decision-m003",
        &AuditAction::JobComplete,
        "corr-m003",
        &scope,
    );
    assert_eq!(first, replay);
}
