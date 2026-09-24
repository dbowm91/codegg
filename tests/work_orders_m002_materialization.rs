//! Project Work Orders M002 — release coordinator and session
//! materialization.
//!
//! Exactly-once claim/materialization through the existing global scheduler
//! boundary: one occurrence creates at most one canonical session and one
//! canonical initial `AgentTurn` submission; restart/duplicate wakes cannot
//! duplicate side effects; managed worktrees isolate concurrent Git mutation;
//! model/policy resolution never silently falls back or widens.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use codegg::core::work_order_coordinator::WorkOrderCoordinator;
use codegg::scheduler::submission::{JobSubmissionService, SubmissionKey};
use codegg::scheduler::{JobScheduler, ResolvedSchedulerConfig};
use codegg_core::identity::{PrincipalId, ProjectId};
use codegg_core::jobs::{
    DaemonGeneration, ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobKind, JobPayload,
    JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::session::{CreateSession, SessionStore};
use codegg_core::work_order::{
    evaluate_occurrence_gates, merge_latches, narrow_approval, narrow_sandbox, resolve_model,
    resolve_workspace_action, sequence_holds, session_id_for_occurrence,
    submission_key_for_occurrence, ApprovalRequest, AttentionCode, GateJoin, GateKind, GateSpec,
    LaneFailurePolicy, NewWorkOrder, OccurrenceState, ReleaseGateSet, WorkOrderService,
    WorkspaceAction, WorkspacePolicy,
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

fn project() -> ProjectId {
    ProjectId::parse("project-1").unwrap()
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
            DaemonGeneration::new_unchecked("gen-m002"),
        );
        scheduler
            .register_executor_sync(Arc::new(codegg::scheduler::executors::AgentTurnExecutor))
            .expect("register agent-turn executor");
        let submission = JobSubmissionService::new(
            store.clone(),
            scheduler,
            services,
            DaemonGeneration::new_unchecked("gen-m002"),
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

/// In-memory session creator with query-before-create recovery semantics
/// (mirrors `SessionStore::create_with_id` deterministic recovery).
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

#[tokio::test(flavor = "current_thread")]
async fn one_occurrence_creates_one_session_and_one_agent_turn_job() {
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool.clone())));
    let coordinator = WorkOrderCoordinator::new(service.clone());
    let sessions = FakeSessions::default();
    let scheduler = SchedulerHarness::new().await;

    let created = service
        .create_work_order(&project(), &creator(), immediate_input("ship it"), 1_000)
        .await
        .expect("create");
    let occurrence = service
        .create_occurrence(&project(), &created.work_order.id, None, 1_001)
        .await
        .expect("occurrence");

    // Release evaluation marks the immediate occurrence ready.
    let ready = coordinator
        .evaluate_due_for_project(&project(), 1_002)
        .await
        .expect("evaluate");
    assert_eq!(ready.len(), 1);

    // Exactly-once claim.
    let claimed = service
        .claim_occurrence(&project(), &occurrence.id, 1_003)
        .await
        .expect("claim");
    assert_eq!(claimed.state, OccurrenceState::Claiming);

    // Workspace resolution: default AutoIsolated Git mutation takes a
    // managed worktree (lazy at claim, never at authoring).
    let action = resolve_workspace_action(None, true, true);
    assert_eq!(action, WorkspaceAction::UseManagedWorktree);
    service
        .persist_workspace_link(&project(), &occurrence.id, "ws-1", Some("wt-1"), 1_004)
        .await
        .expect("workspace link");

    // Canonical session creation with a deterministic occurrence-derived
    // idempotency key; the link persists before any job submission so
    // crash recovery finds the session instead of duplicating it.
    let session_id = WorkOrderCoordinator::session_id_for(&occurrence.id);
    let (_, duplicate) = sessions.get_or_create(&session_id, "ship it").await;
    assert!(!duplicate);
    service
        .persist_session_link(&project(), &occurrence.id, &session_id, 1_005)
        .await
        .expect("session link");
    // Duplicate recovery converges on the stored session.
    let (_, duplicate) = sessions.get_or_create(&session_id, "ship it").await;
    assert!(duplicate);

    // One initial AgentTurn enters JobSubmissionService + the scheduler with
    // a deterministic submission key.
    let submission_key = WorkOrderCoordinator::submission_key_for(&occurrence.id);
    let key = SubmissionKey::new(submission_key.clone()).expect("key");
    let submitted = scheduler
        .submission
        .submit(
            Some(key),
            scheduler.agent_turn_spec(&session_id, "ship it", &submission_key),
        )
        .await
        .expect("submit initial turn");
    service
        .persist_job_link(&project(), &occurrence.id, submitted.job_id.as_str(), 1_006)
        .await
        .expect("job link");

    // Duplicate coordinator wake reconciles by key instead of duplicating.
    let reconciled = scheduler
        .submission
        .reconcile_by_key(
            &SubmissionKey::new(submission_key).expect("key"),
            &scheduler.workspace_id,
            None,
        )
        .await
        .expect("reconcile");
    assert_eq!(
        reconciled.map(|job| job.job_id),
        Some(submitted.job_id.clone())
    );

    let running = service
        .transition_occurrence(
            &project(),
            &occurrence.id,
            OccurrenceState::Running,
            None,
            None,
            1_007,
        )
        .await
        .expect("running");
    assert_eq!(running.state, OccurrenceState::Running);
    assert_eq!(running.session_id.as_deref(), Some(session_id.as_str()));
    assert_eq!(running.job_id.as_deref(), Some(submitted.job_id.as_str()));
    assert_eq!(sessions.count().await, 1);
    let jobs = scheduler
        .store
        .list_jobs(codegg_core::jobs::JobStoreQuery::default())
        .await
        .expect("list jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, JobKind::AgentTurn);
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_claims_admit_exactly_one_winner() {
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool)));
    let created = service
        .create_work_order(&project(), &creator(), immediate_input("race"), 1_000)
        .await
        .expect("create");
    let occurrence = service
        .create_occurrence(&project(), &created.work_order.id, None, 1_001)
        .await
        .expect("occurrence");
    let proj = project();
    let (first, second) = tokio::join!(
        service.claim_occurrence(&proj, &occurrence.id, 1_002),
        service.claim_occurrence(&proj, &occurrence.id, 1_003),
    );
    assert_eq!(
        [first.is_ok(), second.is_ok()]
            .iter()
            .filter(|ok| **ok)
            .count(),
        1,
        "exactly one coordinator claim must win"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn crash_recovery_at_each_boundary_finds_existing_side_effects() {
    // Fault-injection around the materialization transaction boundary:
    // crash after claim, after workspace link, after session link, and
    // after job submit. Each restart reconciles by querying canonical
    // stores before creating anything new.
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool.clone())));
    let sessions = FakeSessions::default();
    let scheduler = SchedulerHarness::new().await;

    let created = service
        .create_work_order(&project(), &creator(), immediate_input("recover me"), 1_000)
        .await
        .expect("create");
    let occurrence = service
        .create_occurrence(&project(), &created.work_order.id, None, 1_001)
        .await
        .expect("occurrence");

    // Crash after claim: the claiming row survives reopen; a second claim
    // conflicts and the coordinator resumes from the stored row.
    service
        .claim_occurrence(&project(), &occurrence.id, 1_002)
        .await
        .expect("claim");
    let reopened = WorkOrderService::with_defaults(Some(pool.clone()));
    let current = reopened
        .get_occurrence(&project(), &occurrence.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(current.state, OccurrenceState::Claiming);
    assert!(reopened
        .claim_occurrence(&project(), &occurrence.id, 1_003)
        .await
        .is_err());

    // Crash after workspace link: recovery reuses the stored canonical ID.
    service
        .persist_workspace_link(&project(), &occurrence.id, "ws-1", Some("wt-1"), 1_004)
        .await
        .expect("workspace link");
    let current = reopened
        .get_occurrence(&project(), &occurrence.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(current.workspace_id.as_deref(), Some("ws-1"));

    // Crash after session link: recovery finds the existing session.
    let session_id = session_id_for_occurrence(occurrence.id.as_str());
    sessions.get_or_create(&session_id, "recover me").await;
    service
        .persist_session_link(&project(), &occurrence.id, &session_id, 1_005)
        .await
        .expect("session link");
    let (_, duplicate) = sessions.get_or_create(&session_id, "recover me").await;
    assert!(duplicate, "session recovery must converge");

    // Crash after job submit: recovery reconciles by submission key.
    let submission_key = submission_key_for_occurrence(occurrence.id.as_str());
    let submitted = scheduler
        .submission
        .submit(
            Some(SubmissionKey::new(submission_key.clone()).expect("key")),
            scheduler.agent_turn_spec(&session_id, "recover me", &submission_key),
        )
        .await
        .expect("submit");
    service
        .persist_job_link(&project(), &occurrence.id, submitted.job_id.as_str(), 1_006)
        .await
        .expect("job link");
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

    // Incomplete-occurrence scan surfaces the partial row for startup
    // reconciliation.
    let incomplete = service
        .list_incomplete_occurrences(&project(), 16)
        .await
        .expect("incomplete");
    assert_eq!(incomplete.len(), 1);
    service
        .transition_occurrence(
            &project(),
            &occurrence.id,
            OccurrenceState::Running,
            None,
            None,
            1_007,
        )
        .await
        .expect("running");
}

#[tokio::test(flavor = "current_thread")]
async fn canonical_session_creation_uses_the_deterministic_key() {
    let pool = test_pool().await;
    let sessions = SessionStore::new(pool.clone());
    let service = WorkOrderService::with_defaults(Some(pool));
    let created = service
        .create_work_order(&project(), &creator(), immediate_input("canonical"), 1_000)
        .await
        .expect("create");
    let occurrence = service
        .create_occurrence(&project(), &created.work_order.id, None, 1_001)
        .await
        .expect("occurrence");
    let session_id = session_id_for_occurrence(occurrence.id.as_str());
    let session = sessions
        .create_with_id(
            &session_id,
            CreateSession {
                project_id: project().as_str().to_owned(),
                directory: "/tmp".to_owned(),
                title: Some("canonical task session".to_owned()),
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
        .expect("create canonical session");
    assert_eq!(session.id, session_id);
    // Occurrence links the canonical session (never encoded in the title).
    service
        .persist_session_link(&project(), &occurrence.id, &session.id, 1_002)
        .await
        .expect("session link");
    assert_ne!(session.title, session.id);
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_git_mutation_gets_distinct_managed_worktrees() {
    let repo_dir = tempfile::tempdir().expect("repo dir");
    for args in [
        vec!["init"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
        vec!["config", "commit.gpgsign", "false"],
    ] {
        let status = std::process::Command::new("git")
            .args(&args)
            .current_dir(repo_dir.path())
            .status()
            .expect("git");
        assert!(status.success());
    }
    std::fs::write(repo_dir.path().join("README.md"), "hello\n").expect("write");
    for args in [vec!["add", "."], vec!["commit", "-m", "init"]] {
        let status = std::process::Command::new("git")
            .args(&args)
            .current_dir(repo_dir.path())
            .status()
            .expect("git");
        assert!(status.success());
    }
    let managed_root = tempfile::tempdir().expect("managed root");
    let service = codegg_core::worktree_service::WorktreeService::memory(
        managed_root.path().to_path_buf(),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    let project_id = project();
    let repository_id = codegg_core::identity::RepositoryId::new();
    let workspace_a = codegg_core::workspace::WorkspaceId::new_unchecked("ws-a");
    let workspace_b = codegg_core::workspace::WorkspaceId::new_unchecked("ws-b");
    let request_a = codegg_core::worktree_service::CreateWorktreeRequest {
        project_id: project_id.clone(),
        repository_id: repository_id.clone(),
        workspace_id: workspace_a,
        node_id: None,
        repository_root: repo_dir.path().to_path_buf(),
        base_commit: None,
        base_path: None,
        owner_run_id: codegg_core::identity::AgentRunId::new(),
    };
    let request_b = codegg_core::worktree_service::CreateWorktreeRequest {
        project_id: project_id.clone(),
        repository_id: repository_id.clone(),
        workspace_id: workspace_b,
        node_id: None,
        repository_root: repo_dir.path().to_path_buf(),
        base_commit: None,
        base_path: None,
        owner_run_id: codegg_core::identity::AgentRunId::new(),
    };
    let (record_a, _) = service.create(&request_a).await.expect("worktree a");
    let (record_b, _) = service.create(&request_b).await.expect("worktree b");
    assert_ne!(record_a.worktree_id, record_b.worktree_id);
    assert_ne!(record_a.path, record_b.path);
    let canonical_managed = managed_root
        .path()
        .canonicalize()
        .unwrap_or_else(|_| managed_root.path().to_path_buf());
    assert!(record_a.path.starts_with(&canonical_managed));
    assert!(record_b.path.starts_with(&canonical_managed));
}

#[tokio::test(flavor = "current_thread")]
async fn non_git_mutation_and_sharing_follow_the_truthful_policy() {
    // Non-Git mutation never claims worktree isolation.
    match resolve_workspace_action(None, false, true) {
        WorkspaceAction::NeedsAttention { diagnostic, .. } => {
            assert!(diagnostic.contains("isolation_unavailable"));
        }
        other => panic!("non-Git mutation must need attention, got {other:?}"),
    }
    // Read-only work shares safely; serialized mutation shares with
    // scheduler contention ensuring one writer.
    assert_eq!(
        resolve_workspace_action(None, true, false),
        WorkspaceAction::ShareReadOnly
    );
    assert_eq!(
        resolve_workspace_action(Some(WorkspacePolicy::Serialized), true, true),
        WorkspaceAction::ShareSerialized
    );
    // Dirty/conflicted worktrees are never silently cleaned: only clean
    // trees are cleanup-safe (managed-worktree service owns retention).
    assert!(!codegg_core::worktree_service::WorktreeHealth::Dirty.cleanup_safe());
    assert!(codegg_core::worktree_service::WorktreeHealth::Clean.cleanup_safe());
}

#[tokio::test(flavor = "current_thread")]
async fn removed_model_and_policy_tightening_never_silently_repair() {
    // Removed model → needs attention, no silent fallback.
    let available: HashSet<String> = ["openai/gpt-5".to_owned()].into_iter().collect();
    assert_eq!(
        resolve_model(Some("removed/model"), &|model| available.contains(model)).unwrap_err(),
        AttentionCode::ModelUnavailable
    );
    // Approval/sandbox snapshots narrow against ceilings but never widen.
    let (effective, narrowed) = narrow_approval(Some(ApprovalRequest::Yolo), Some(0));
    assert_eq!(effective, Some(ApprovalRequest::Interactive));
    assert!(narrowed);
    let (effective, narrowed) = narrow_sandbox(
        Some(codegg_core::work_order::SandboxRequest::FullHost),
        Some(1),
    );
    assert_eq!(
        effective,
        Some(codegg_core::work_order::SandboxRequest::WorkspaceWrite)
    );
    assert!(narrowed);
    // A preference recorded after scheduling cannot widen the work order:
    // re-narrowing the same snapshot against the same ceiling is stable.
    let (again, _) = narrow_approval(Some(ApprovalRequest::Yolo), Some(0));
    assert_eq!(again, effective.map(|_| ApprovalRequest::Interactive));
    let _ = narrowed;
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_covers_every_boundary_with_deterministic_precedence() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool));
    // Before claim: no workspace/session/job exists.
    let before = service
        .create_work_order(
            &project(),
            &creator(),
            immediate_input("cancel early"),
            1_000,
        )
        .await
        .expect("create");
    let early = service
        .create_occurrence(&project(), &before.work_order.id, None, 1_001)
        .await
        .expect("occurrence");
    let cancelled = service
        .cancel_occurrence(&project(), &early.id, 1_002)
        .await
        .expect("cancel");
    assert_eq!(cancelled.state, OccurrenceState::Cancelled);
    assert!(cancelled.session_id.is_none());
    assert!(cancelled.job_id.is_none());

    // After session link but before job submit: reconcile without
    // submitting new work.
    let mid = service
        .create_work_order(&project(), &creator(), immediate_input("cancel mid"), 2_000)
        .await
        .expect("create");
    let mid_occ = service
        .create_occurrence(&project(), &mid.work_order.id, None, 2_001)
        .await
        .expect("occurrence");
    service
        .claim_occurrence(&project(), &mid_occ.id, 2_002)
        .await
        .expect("claim");
    service
        .persist_session_link(&project(), &mid_occ.id, "session-mid", 2_003)
        .await
        .expect("session link");
    let cancelled = service
        .cancel_occurrence(&project(), &mid_occ.id, 2_004)
        .await
        .expect("cancel");
    assert_eq!(cancelled.state, OccurrenceState::Cancelled);
    assert_eq!(cancelled.session_id.as_deref(), Some("session-mid"));
    assert!(cancelled.job_id.is_none());

    // Completion-vs-cancel race: terminal wins deterministically; a cancel
    // arriving after completion is an idempotent already-terminal result.
    let late = service
        .create_work_order(&project(), &creator(), immediate_input("race"), 3_000)
        .await
        .expect("create");
    let late_occ = service
        .create_occurrence(&project(), &late.work_order.id, None, 3_001)
        .await
        .expect("occurrence");
    for state in [
        OccurrenceState::Ready,
        OccurrenceState::Claiming,
        OccurrenceState::Running,
        OccurrenceState::Completed,
    ] {
        service
            .transition_occurrence(&project(), &late_occ.id, state, None, None, 3_002)
            .await
            .expect("advance");
    }
    let again = service
        .cancel_occurrence(&project(), &late_occ.id, 3_003)
        .await
        .expect("cancel after terminal");
    assert_eq!(again.state, OccurrenceState::Completed);
}

#[tokio::test(flavor = "current_thread")]
async fn delay_not_before_sequence_and_repeat_survive_restart() {
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool.clone())));
    let coordinator = WorkOrderCoordinator::new(service.clone());

    // Delay: deadline persists across reopen (same pool, new handle).
    let mut delayed = immediate_input("delayed");
    delayed.gates = ReleaseGateSet {
        join: GateJoin::All,
        gates: vec![GateSpec {
            kind: GateKind::Delay,
            delay_secs: Some(60),
            not_before_ms: None,
            lane_id: None,
            trigger_ref: None,
        }],
    };
    let created = service
        .create_work_order(&project(), &creator(), delayed, 1_000)
        .await
        .expect("create");
    service
        .create_occurrence(&project(), &created.work_order.id, None, 1_001)
        .await
        .expect("occurrence");
    let early = coordinator
        .evaluate_due_for_project(&project(), 1_002)
        .await
        .expect("evaluate");
    assert!(early.is_empty(), "delay must hold before its deadline");
    // Simulate restart: a new service over the same durable pool reuses the
    // persisted deadline rather than recalculating a new clock origin.
    let reopened = WorkOrderCoordinator::new(Arc::new(WorkOrderService::with_defaults(Some(
        pool.clone(),
    ))));
    let early = reopened
        .evaluate_due_for_project(&project(), 30_000)
        .await
        .expect("evaluate");
    assert!(early.is_empty());
    let ready = reopened
        .evaluate_due_for_project(&project(), 61_000)
        .await
        .expect("evaluate");
    assert_eq!(ready.len(), 1);

    // NotBefore latches; backward clock never un-satisfies.
    let mut nb = immediate_input("not-before");
    nb.gates = ReleaseGateSet {
        join: GateJoin::All,
        gates: vec![GateSpec {
            kind: GateKind::NotBefore,
            delay_secs: None,
            not_before_ms: Some(10_000),
            lane_id: None,
            trigger_ref: None,
        }],
    };
    let created = service
        .create_work_order(&project(), &creator(), nb, 1_000)
        .await
        .expect("create");
    let nb_occ = service
        .create_occurrence(&project(), &created.work_order.id, None, 1_001)
        .await
        .expect("occurrence");
    let work = service
        .get_work_order(&project(), &created.work_order.id)
        .await
        .expect("get")
        .expect("row");
    let occ = service
        .get_occurrence(&project(), &nb_occ.id)
        .await
        .expect("get")
        .expect("row");
    let eval = evaluate_occurrence_gates(&work.gates, &occ, work.created_at_ms, None, 5_000, true);
    assert!(!eval.satisfied);
    service
        .persist_gate_evaluation(
            &project(),
            &nb_occ.id,
            merge_latches(
                &occ.gate_latches,
                &evaluate_occurrence_gates(
                    &work.gates,
                    &occ,
                    work.created_at_ms,
                    None,
                    10_000,
                    true,
                )
                .newly_latched,
            ),
            None,
            true,
            10_000,
        )
        .await
        .expect("latch");
    let latched = service
        .get_occurrence(&project(), &nb_occ.id)
        .await
        .expect("get")
        .expect("row");
    let eval =
        evaluate_occurrence_gates(&work.gates, &latched, work.created_at_ms, None, 1_000, true);
    assert!(eval.satisfied, "latched gate survives backward clock");

    // Finite repeat creates distinct occurrence IDs and exhausts exactly.
    let mut repeatable = immediate_input("repeatable");
    repeatable.repeat_count = 2;
    let created = service
        .create_work_order(&project(), &creator(), repeatable, 1_000)
        .await
        .expect("create");
    let first = service
        .create_occurrence(&project(), &created.work_order.id, None, 1_001)
        .await
        .expect("first");
    for state in [
        OccurrenceState::Ready,
        OccurrenceState::Claiming,
        OccurrenceState::Running,
        OccurrenceState::Completed,
    ] {
        service
            .transition_occurrence(&project(), &first.id, state, None, None, 1_002)
            .await
            .expect("advance");
    }
    let next = service
        .create_next_repeat_occurrence(&project(), &created.work_order.id, &first.id, 1_003)
        .await
        .expect("repeat");
    assert!(!next.duplicate);
    assert_ne!(next.occurrence.id, first.id);
    assert_eq!(next.occurrence.occurrence_index, 1);
    // Sequence hold: failed predecessors hold downstream work by default.
    assert!(sequence_holds(
        &[OccurrenceState::Failed],
        LaneFailurePolicy::HoldLane
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_wakes_never_duplicate_sessions_or_jobs() {
    let pool = test_pool().await;
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool)));
    let sessions = FakeSessions::default();
    let scheduler = SchedulerHarness::new().await;
    let created = service
        .create_work_order(&project(), &creator(), immediate_input("idempotent"), 1_000)
        .await
        .expect("create");
    let occurrence = service
        .create_occurrence(&project(), &created.work_order.id, None, 1_001)
        .await
        .expect("occurrence");
    service
        .claim_occurrence(&project(), &occurrence.id, 1_002)
        .await
        .expect("claim");
    let session_id = session_id_for_occurrence(occurrence.id.as_str());
    // Two concurrent session creations converge on one canonical session.
    let (first, second) = tokio::join!(
        sessions.get_or_create(&session_id, "idempotent"),
        sessions.get_or_create(&session_id, "idempotent"),
    );
    assert_eq!(first.0, second.0);
    assert_eq!(sessions.count().await, 1);
    // Two concurrent job submissions with the same deterministic key
    // converge on one durable job.
    let submission_key = submission_key_for_occurrence(occurrence.id.as_str());
    let (first, second) = tokio::join!(
        scheduler.submission.submit(
            Some(SubmissionKey::new(submission_key.clone()).expect("key")),
            scheduler.agent_turn_spec(&session_id, "idempotent", &submission_key),
        ),
        scheduler.submission.submit(
            Some(SubmissionKey::new(submission_key.clone()).expect("key")),
            scheduler.agent_turn_spec(&session_id, "idempotent", &submission_key),
        ),
    );
    let (first, second) = (first.expect("submit"), second.expect("submit"));
    assert_eq!(first.job_id, second.job_id);
}
