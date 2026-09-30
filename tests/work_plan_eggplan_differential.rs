//! Eggplan M002 differential qualification: legacy vs Eggplan engines.
//!
//! Runs both engines over the same structured cases. The hard-stop rule:
//! for any `EggplanGit` case, `allows_completion == true` while legacy
//! says false (or Complete while legacy requires continuation/block/wait)
//! fails the suite. Stricter Eggplan outcomes are allowed only when
//! attributable to documented subject/verification authority; every such
//! delta is asserted explicitly below.

mod common;

use codegg::work_plan_eggplan::{
    assess_with_engine, complete_plan_with_subject_revalidation, AssessmentEngine,
    EggplanBackedAssessment,
};
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, AgentRunTerminalOutcome, NewAgentRun, NewAgentTask,
    SqliteAgentRunStore,
};
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, DaemonGeneration, ExecutionSubjectDisposition,
    ExecutionSubjectKind, ExecutionSubjectProvenance, ExecutionSubjectRevision,
    ExecutionSubjectSealKind, ExecutionSubjectState, IdempotencyClass, JobId, JobKind, JobPayload,
    JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy, SqliteJobStore,
};
use codegg_core::work_plan::{
    assess_work_plan, WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceKind, WorkEvidenceRef,
    WorkItem, WorkItemId, WorkItemStatus, WorkPlan, WorkPlanId, WorkPlanStatus, WorkPlanStore,
};
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceId, WorkspaceRegistry};
use sqlx::SqlitePool;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

// ── Fixtures ─────────────────────────────────────────────────────────────

fn git_repo() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?} failed");
    };
    run(&["init"]);
    run(&["config", "user.name", "Differential Test"]);
    run(&["config", "user.email", "diff@test.example.com"]);
    std::fs::write(dir.path().join("base.txt"), b"base\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "base"]);
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    (dir, head.trim().to_string())
}

async fn registered_workspace_id(pool: &SqlitePool, root: &Path) -> String {
    let store = Arc::new(SqliteWorkspaceStore::new(pool.clone()));
    let registry = WorkspaceRegistry::load(store).await.unwrap();
    let record = registry.get_or_register(root).await.unwrap();
    format!("codegg-workspace:{}", record.id.as_str())
}

fn clean_revision(identity: &str, oid: &str) -> ExecutionSubjectRevision {
    ExecutionSubjectRevision {
        schema_version: ExecutionSubjectRevision::SCHEMA_VERSION,
        subject_kind: ExecutionSubjectKind::Git,
        repository_identity: identity.to_string(),
        revision: oid.to_string(),
        state: ExecutionSubjectState::Clean,
        dirty_digest: None,
    }
}

fn stable_provenance(identity: &str, oid: &str) -> ExecutionSubjectProvenance {
    let revision = clean_revision(identity, oid);
    ExecutionSubjectProvenance {
        schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
        captured: Some(revision.clone()),
        sealed: Some(revision),
        disposition: ExecutionSubjectDisposition::Stable,
        seal_kind: ExecutionSubjectSealKind::LiveExecutionEnd,
        unavailable_reason: None,
        materialization: None,
    }
}

fn started_provenance(identity: &str, oid: &str) -> ExecutionSubjectProvenance {
    ExecutionSubjectProvenance {
        schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
        captured: Some(clean_revision(identity, oid)),
        sealed: None,
        disposition: ExecutionSubjectDisposition::Started,
        seal_kind: ExecutionSubjectSealKind::LiveExecutionEnd,
        unavailable_reason: None,
        materialization: None,
    }
}

fn drifted_provenance(identity: &str, s1: &str, s2: &str) -> ExecutionSubjectProvenance {
    ExecutionSubjectProvenance {
        schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
        captured: Some(clean_revision(identity, s1)),
        sealed: Some(clean_revision(identity, s2)),
        disposition: ExecutionSubjectDisposition::Drifted,
        seal_kind: ExecutionSubjectSealKind::LiveExecutionEnd,
        unavailable_reason: None,
        materialization: None,
    }
}

fn test_payload() -> JobPayload {
    JobPayload::Test {
        command: "cargo test".into(),
        argv: vec!["cargo".into(), "test".into()],
        cwd: None,
        scope: None,
        parent_run_id: None,
    }
}

fn managed_payload() -> JobPayload {
    JobPayload::ManagedArgv {
        argv: vec!["echo".into(), "hi".into()],
        cwd: None,
    }
}

fn subagent_payload() -> JobPayload {
    JobPayload::SubagentRun {
        prompt: "do the delegated thing".into(),
        agent: "worker".into(),
        model: Some("model".into()),
        parent_id: None,
        denied_tools: vec!["b".into(), "a".into()],
        allowed_paths: vec!["src".into()],
        max_tool_calls: Some(8),
        task_id: codegg_core::identity::AgentTaskId::new(),
        run_id: codegg_core::identity::AgentRunId::new(),
        delegation_key: "dk-1".into(),
        base_commit: None,
    }
}

fn job_spec(kind: JobKind, payload: JobPayload) -> NewJob {
    NewJob {
        workspace_id: WorkspaceId::new_unchecked("ws-diff"),
        session_id: None,
        turn_id: None,
        kind,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload,
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
        target: Default::default(),
    }
}

/// Create a job with one attempt: optionally sealed with `provenance`,
/// finished in `finish` (or left running when `finish` is `None`).
async fn evidence_job(
    pool: &SqlitePool,
    kind: JobKind,
    payload: JobPayload,
    provenance: Option<ExecutionSubjectProvenance>,
    finish: Option<AttemptState>,
) -> String {
    let store = SqliteJobStore::new(pool.clone());
    let job = store.create_job(job_spec(kind, payload)).await.unwrap();
    let attempt = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new())
        .await
        .unwrap();
    store
        .mark_attempt_running(&attempt.attempt_id)
        .await
        .unwrap();
    if let Some(provenance) = provenance {
        if provenance.disposition == ExecutionSubjectDisposition::Started {
            store
                .set_attempt_source_subject_started(&attempt.attempt_id, &provenance)
                .await
                .unwrap();
        } else {
            store
                .set_attempt_source_subject_started(
                    &attempt.attempt_id,
                    &started_provenance(
                        &provenance
                            .captured
                            .as_ref()
                            .map(|captured| captured.repository_identity.clone())
                            .unwrap_or_default(),
                        &provenance
                            .captured
                            .as_ref()
                            .map(|captured| captured.revision.clone())
                            .unwrap_or_default(),
                    ),
                )
                .await
                .unwrap();
            store
                .seal_attempt_source_subject(&attempt.attempt_id, &provenance)
                .await
                .unwrap();
        }
    }
    if let Some(state) = finish {
        store
            .finish_attempt(AttemptCompletion {
                attempt_id: attempt.attempt_id,
                state,
                error: None,
                run_id: None,
            })
            .await
            .unwrap();
    }
    job.job_id.to_string()
}

async fn linked_agent_run(pool: &SqlitePool, job_id: &str, attempt_done: bool) -> String {
    let store = SqliteAgentRunStore::new(pool.clone());
    let submission = store
        .create_or_get(
            NewAgentTask {
                task_id: codegg_core::identity::AgentTaskId::new(),
                parent_task_id: None,
                originating_session_id: "session-1".to_string(),
                originating_turn_id: None,
                project_id: codegg_core::identity::ProjectId::parse("project-1").unwrap(),
                repository_id: None,
                workspace_id: WorkspaceId::new_unchecked("ws-diff"),
                requested_agent: "agent".to_string(),
                delegation_key: format!("delegation-{}", uuid::Uuid::new_v4().simple()),
                request_fingerprint: "fingerprint".to_string(),
                description: "linked run".to_string(),
            },
            NewAgentRun {
                run_id: codegg_core::identity::AgentRunId::new(),
                parent_run_id: None,
                depth: 1,
                workspace_id: WorkspaceId::new_unchecked("ws-diff"),
                agent_name: "agent".to_string(),
                agent_digest: None,
                provider: "provider".to_string(),
                model: "model".to_string(),
                authority_digest: "authority".to_string(),
                budget: AgentRunBudget::default(),
            },
        )
        .await
        .unwrap();
    let run_id = submission.run.run_id;
    // Link to the latest attempt of the job.
    let job_store = SqliteJobStore::new(pool.clone());
    let job = JobId::new_unchecked(job_id.to_string());
    let attempt = job_store
        .list_attempts(&job)
        .await
        .unwrap()
        .into_iter()
        .max_by_key(|attempt| attempt.sequence);
    if let Some(attempt) = attempt {
        store.attach_job(&run_id, job.clone()).await.unwrap();
        store
            .attach_attempt(&run_id, attempt.attempt_id)
            .await
            .unwrap();
    }
    if attempt_done {
        for next in [
            codegg_core::agent_run::AgentRunStatus::Queued,
            codegg_core::agent_run::AgentRunStatus::Preparing,
            codegg_core::agent_run::AgentRunStatus::Running,
        ] {
            store.transition(&run_id, next).await.unwrap();
        }
        store
            .finish(
                &run_id,
                AgentRunTerminalOutcome::Completed,
                None,
                None,
                None,
            )
            .await
            .unwrap();
    }
    run_id.to_string()
}

fn plan_fixture(status: WorkPlanStatus) -> WorkPlan {
    let now = chrono::Utc::now();
    WorkPlan {
        id: WorkPlanId::generate(),
        revision: 0,
        session_id: "sess-diff".to_string(),
        project_id: "proj-diff".to_string(),
        origin_turn_id: None,
        goal_id: None,
        objective: "differential objective".to_string(),
        objective_digest: "sha256:x".to_string(),
        origin_provenance: "turn:t".to_string(),
        status,
        current_phase: None,
        current_item_id: None,
        created_at: now,
        updated_at: now,
        completed_at: None,
    }
}

fn item_fixture(
    plan: &WorkPlan,
    position: i64,
    status: WorkItemStatus,
    acceptance: Vec<WorkAcceptance>,
    evidence: Vec<WorkEvidenceRef>,
) -> WorkItem {
    let now = chrono::Utc::now();
    WorkItem {
        id: WorkItemId::generate(),
        plan_id: plan.id.clone(),
        revision: 0,
        position,
        parent_item_id: None,
        dependencies: vec![],
        status,
        description: format!("differential work {position}"),
        acceptance,
        evidence,
        owner_run_id: None,
        owner_job_id: None,
        attempts: 0,
        blocker: None,
        next_action: Some("next step".to_string()),
        created_at: now,
        updated_at: now,
    }
}

fn unmet(description: &str) -> WorkAcceptance {
    WorkAcceptance {
        description: description.to_string(),
        disposition: WorkAcceptanceDisposition::Unmet,
        note: None,
    }
}

fn satisfied(description: &str) -> WorkAcceptance {
    WorkAcceptance {
        description: description.to_string(),
        disposition: WorkAcceptanceDisposition::Satisfied,
        note: None,
    }
}

fn judgment(description: &str) -> WorkAcceptance {
    WorkAcceptance {
        description: description.to_string(),
        disposition: WorkAcceptanceDisposition::RequiresUserJudgment,
        note: None,
    }
}

fn evidence_ref(kind: WorkEvidenceKind, ref_id: &str) -> WorkEvidenceRef {
    WorkEvidenceRef {
        kind,
        ref_id: ref_id.to_string(),
        detail: None,
    }
}

struct DualOutcome {
    legacy_allows: bool,
    legacy_code: &'static str,
    eggplan: Result<EggplanBackedAssessment, String>,
}

/// Run both engines; enforce the hard-stop rule on the caller side via
/// `assert_no_permissive_delta`.
async fn assess_dual(
    pool: &SqlitePool,
    root: &Path,
    plan: &WorkPlan,
    items: &[WorkItem],
) -> DualOutcome {
    let evidence = codegg::work_plan_evidence::assemble(pool, items)
        .await
        .unwrap_or_default();
    let legacy = assess_work_plan(plan, items, &evidence);
    let legacy_allows = legacy.allows_completion();
    let legacy_code = legacy.reason_code();
    let eggplan = assess_with_engine(pool, Some(root), plan, items)
        .await
        .map_err(|error| error.to_string());
    DualOutcome {
        legacy_allows,
        legacy_code,
        eggplan,
    }
}

fn assert_no_permissive_delta(outcome: &DualOutcome, case: &str) {
    if let Ok(backed) = &outcome.eggplan {
        assert_eq!(
            backed.engine,
            AssessmentEngine::EggplanGit,
            "{case}: expected the EggplanGit engine"
        );
        let eggplan_allows = backed.assessment.allows_completion();
        assert!(
            !eggplan_allows || outcome.legacy_allows,
            "{case}: HARD STOP — Eggplan allows completion while legacy forbids \
             (legacy={}, eggplan={:?})",
            outcome.legacy_code,
            backed.assessment.reason_code(),
        );
    }
}

// ── Matrix ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn differential_no_evidence_stays_actionable() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Pending,
        vec![unmet("c")],
        vec![],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert!(!outcome.legacy_allows);
    assert_no_permissive_delta(&outcome, "no-evidence");
    let backed = outcome.eggplan.unwrap();
    assert!(backed.assessment.requires_continuation());
}

#[tokio::test(flavor = "current_thread")]
async fn differential_passing_test_job_completes_both() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    let job_id = evidence_job(
        &pool,
        JobKind::Test,
        test_payload(),
        Some(stable_provenance(&identity, &head)),
        Some(AttemptState::Completed),
    )
    .await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let mut item = item_fixture(
        &plan,
        0,
        WorkItemStatus::Completed,
        vec![satisfied("tests pass")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    );
    item.next_action = None;
    let outcome = assess_dual(&pool, dir.path(), &plan, &[item]).await;
    assert!(outcome.legacy_allows, "legacy must complete");
    assert_no_permissive_delta(&outcome, "passing-test");
    let backed = outcome.eggplan.unwrap();
    assert!(
        backed.assessment.allows_completion(),
        "Eggplan must complete on bound passing evidence, got {:?}",
        backed.assessment.reason_code(),
    );
    assert!(backed.mapping_digest.is_some());
    assert_eq!(backed.subject_revision.as_deref(), Some(head.as_str()));
}

#[tokio::test(flavor = "current_thread")]
async fn differential_failed_test_job_stays_actionable() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    let job_id = evidence_job(
        &pool,
        JobKind::Test,
        test_payload(),
        Some(stable_provenance(&identity, &head)),
        Some(AttemptState::Failed),
    )
    .await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::InProgress,
        vec![unmet("tests pass")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert_no_permissive_delta(&outcome, "failed-test");
    assert!(!outcome.eggplan.unwrap().assessment.allows_completion());
}

#[tokio::test(flavor = "current_thread")]
async fn differential_missing_test_job_is_fail_closed_error() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::InProgress,
        vec![unmet("tests pass")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, "job-missing")],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    // Legacy stays actionable on the dangling ref; Eggplan fails the
    // whole assessment closed (never completion). Documented stricter delta.
    assert!(!outcome.legacy_allows);
    assert!(
        outcome.eggplan.is_err(),
        "dangling refs must error, not complete"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn differential_in_flight_test_job_waits() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    let job_id = evidence_job(
        &pool,
        JobKind::Test,
        test_payload(),
        Some(stable_provenance(&identity, &head)),
        None,
    )
    .await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    // The running item carries its acceptance criterion: without criteria
    // the bridge has nothing to assess and reports missing evidence.
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::InProgress,
        vec![unmet("tests pass")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert_no_permissive_delta(&outcome, "in-flight");
    let backed = outcome.eggplan.unwrap();
    assert_eq!(backed.assessment.reason_code(), "in_flight");
}

#[tokio::test(flavor = "current_thread")]
async fn differential_blocked_item_blocks_both() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let mut item = item_fixture(&plan, 0, WorkItemStatus::Blocked, vec![], vec![]);
    item.blocker = Some("waiting on review".to_string());
    let outcome = assess_dual(&pool, dir.path(), &plan, &[item]).await;
    assert_no_permissive_delta(&outcome, "blocked");
    assert_eq!(outcome.eggplan.unwrap().assessment.reason_code(), "blocked");
}

#[tokio::test(flavor = "current_thread")]
async fn differential_dependency_gate_blocks_both() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let first = item_fixture(&plan, 0, WorkItemStatus::Pending, vec![unmet("a")], vec![]);
    let mut second = item_fixture(&plan, 1, WorkItemStatus::Pending, vec![unmet("b")], vec![]);
    second.dependencies = vec![first.id.clone()];
    let outcome = assess_dual(&pool, dir.path(), &plan, &[first, second]).await;
    assert_no_permissive_delta(&outcome, "dependency-gate");
    assert!(!outcome.eggplan.unwrap().assessment.allows_completion());
}

#[tokio::test(flavor = "current_thread")]
async fn differential_human_judgment_only_awaits_both() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Pending,
        vec![judgment("owner signs off")],
        vec![],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert_no_permissive_delta(&outcome, "judgment");
    assert_eq!(
        outcome.eggplan.unwrap().assessment.reason_code(),
        "awaiting_user_judgment"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn differential_forged_satisfied_is_not_completion() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    // Serialized Satisfied with no host evidence: legacy trusts it.
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Completed,
        vec![satisfied("claimed done")],
        vec![],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert!(outcome.legacy_allows, "legacy trusts serialized Satisfied");
    // Eggplan records the disposition as a loss, never as evidence.
    let backed = outcome.eggplan.unwrap();
    assert!(
        !backed.assessment.allows_completion(),
        "forged Satisfied must not complete under Eggplan"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn differential_stale_historical_subject_errors() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    // Evidence sealed at an older revision than current HEAD.
    let job_id = evidence_job(
        &pool,
        JobKind::Test,
        test_payload(),
        Some(stable_provenance(&identity, &"0".repeat(40))),
        Some(AttemptState::Completed),
    )
    .await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Completed,
        vec![satisfied("tests pass")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert!(outcome.legacy_allows, "legacy is status-only: {head}");
    assert!(
        outcome.eggplan.is_err(),
        "stale subjects must fail closed, never complete"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn differential_drifted_subject_errors() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    let job_id = evidence_job(
        &pool,
        JobKind::Test,
        test_payload(),
        Some(drifted_provenance(&identity, &head, &"1".repeat(40))),
        Some(AttemptState::Completed),
    )
    .await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Completed,
        vec![satisfied("tests pass")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert!(
        outcome.eggplan.is_err(),
        "drifted subjects must fail closed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn differential_verification_unavailable_errors() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    // Shell without canonical argv cannot prove execution semantics.
    let payload = JobPayload::Shell {
        command: "echo hi".to_string(),
        argv: None,
        cwd: None,
    };
    let job_id = evidence_job(
        &pool,
        JobKind::Shell,
        payload,
        Some(stable_provenance(&identity, &head)),
        Some(AttemptState::Completed),
    )
    .await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Completed,
        vec![satisfied("shell ran")],
        vec![evidence_ref(WorkEvidenceKind::SchedulerJob, &job_id)],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert!(
        outcome.eggplan.is_err(),
        "missing verification must fail closed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn differential_completed_without_proof_stays_actionable() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Completed,
        vec![],
        vec![],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert_no_permissive_delta(&outcome, "no-proof");
    assert!(!outcome.eggplan.unwrap().assessment.allows_completion());
}

#[tokio::test(flavor = "current_thread")]
async fn differential_delegated_run_completes_both() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    let job_id = evidence_job(
        &pool,
        JobKind::Build,
        subagent_payload(),
        Some(stable_provenance(&identity, &head)),
        Some(AttemptState::Completed),
    )
    .await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Completed,
        vec![satisfied("delegated work done")],
        vec![evidence_ref(WorkEvidenceKind::DelegatedRun, &job_id)],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert!(outcome.legacy_allows, "legacy must complete");
    assert_no_permissive_delta(&outcome, "delegated");
    assert!(outcome.eggplan.unwrap().assessment.allows_completion());
}

#[tokio::test(flavor = "current_thread")]
async fn differential_linked_agent_run_completes_both() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    let job_id = evidence_job(
        &pool,
        JobKind::Build,
        subagent_payload(),
        Some(stable_provenance(&identity, &head)),
        Some(AttemptState::Completed),
    )
    .await;
    let run_id = linked_agent_run(&pool, &job_id, true).await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Completed,
        vec![satisfied("agent work done")],
        vec![evidence_ref(WorkEvidenceKind::AgentRun, &run_id)],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert!(outcome.legacy_allows, "legacy must complete");
    assert_no_permissive_delta(&outcome, "agent-run");
    assert!(outcome.eggplan.unwrap().assessment.allows_completion());
}

#[tokio::test(flavor = "current_thread")]
async fn differential_dangling_agent_run_is_fail_closed_error() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::InProgress,
        vec![unmet("agent work")],
        vec![evidence_ref(WorkEvidenceKind::AgentRun, "run-missing")],
    )];
    let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
    assert!(!outcome.legacy_allows);
    assert!(outcome.eggplan.is_err(), "dangling AgentRun must error");
}

#[tokio::test(flavor = "current_thread")]
async fn differential_unsupported_evidence_selects_legacy_engine() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    for kind in [WorkEvidenceKind::Artifact, WorkEvidenceKind::Commit] {
        let plan = plan_fixture(WorkPlanStatus::Active);
        let items = vec![item_fixture(
            &plan,
            0,
            WorkItemStatus::Pending,
            vec![unmet("c")],
            vec![evidence_ref(kind, "ref-1")],
        )];
        let backed = assess_with_engine(&pool, Some(dir.path()), &plan, &items)
            .await
            .unwrap();
        assert_eq!(backed.engine, AssessmentEngine::LegacyUnsupportedEvidence);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn differential_non_git_workspace_selects_legacy_engine() {
    let pool = common::pool::isolated_pool().await;
    let dir = tempfile::tempdir().unwrap();
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Pending,
        vec![unmet("c")],
        vec![],
    )];
    let backed = assess_with_engine(&pool, Some(dir.path()), &plan, &items)
        .await
        .unwrap();
    assert_eq!(backed.engine, AssessmentEngine::LegacyNonGit);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_git_capture_failure_is_not_legacy() {
    let pool = common::pool::isolated_pool().await;
    // A `.git` FILE (not a directory) claims Git-backed, but capture
    // fails: this must error fail-closed, never route to LegacyNonGit.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".git"), b"not a repo").unwrap();
    let plan = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        0,
        WorkItemStatus::Pending,
        vec![unmet("c")],
        vec![],
    )];
    let error = assess_with_engine(&pool, Some(dir.path()), &plan, &items)
        .await
        .unwrap_err();
    assert_ne!(error.code, "legacy");
}

#[tokio::test(flavor = "current_thread")]
async fn differential_scheduler_job_variants_complete() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    for (kind, payload) in [
        (JobKind::Build, managed_payload()),
        (
            JobKind::Shell,
            JobPayload::Shell {
                command: "echo hi".to_string(),
                argv: Some(vec!["echo".to_string(), "hi".to_string()]),
                cwd: None,
            },
        ),
        (
            JobKind::Build,
            JobPayload::Git {
                argv: vec!["git".to_string(), "status".to_string()],
                cwd: None,
            },
        ),
    ] {
        let job_id = evidence_job(
            &pool,
            kind,
            payload,
            Some(stable_provenance(&identity, &head)),
            Some(AttemptState::Completed),
        )
        .await;
        let plan = plan_fixture(WorkPlanStatus::Active);
        let items = vec![item_fixture(
            &plan,
            0,
            WorkItemStatus::Completed,
            vec![satisfied("scheduler work done")],
            vec![evidence_ref(WorkEvidenceKind::SchedulerJob, &job_id)],
        )];
        let outcome = assess_dual(&pool, dir.path(), &plan, &items).await;
        assert_no_permissive_delta(&outcome, "scheduler-variant");
        assert!(
            outcome.eggplan.unwrap().assessment.allows_completion(),
            "scheduler variant must complete"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn differential_multi_item_dependencies_agree() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    let job_id = evidence_job(
        &pool,
        JobKind::Test,
        test_payload(),
        Some(stable_provenance(&identity, &head)),
        Some(AttemptState::Completed),
    )
    .await;
    let plan = plan_fixture(WorkPlanStatus::Active);
    let mut first = item_fixture(
        &plan,
        0,
        WorkItemStatus::Completed,
        vec![satisfied("first done")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    );
    first.next_action = None;
    let mut second = item_fixture(
        &plan,
        1,
        WorkItemStatus::Pending,
        vec![unmet("second")],
        vec![],
    );
    second.dependencies = vec![first.id.clone()];
    let outcome = assess_dual(&pool, dir.path(), &plan, &[first, second]).await;
    assert_no_permissive_delta(&outcome, "multi-item");
    // First item satisfied; the pending dependent keeps both actionable.
    assert!(!outcome.legacy_allows);
    assert!(!outcome.eggplan.unwrap().assessment.allows_completion());
}

// ── S1/S2 completion race ────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn completion_revalidation_refuses_changed_subject() {
    let pool = common::pool::isolated_pool().await;
    let (dir, head) = git_repo();
    let identity = registered_workspace_id(&pool, dir.path()).await;
    let job_id = evidence_job(
        &pool,
        JobKind::Test,
        test_payload(),
        Some(stable_provenance(&identity, &head)),
        Some(AttemptState::Completed),
    )
    .await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(codegg_core::work_plan::NewWorkPlan {
            session_id: "sess-diff".to_string(),
            project_id: "proj-diff".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "race objective".to_string(),
            origin_provenance: "turn:t".to_string(),
            current_phase: None,
        })
        .await
        .unwrap();
    let (_plan, item) = store
        .add_item(
            &plan.id,
            codegg_core::work_plan::NewWorkItem {
                parent_item_id: None,
                dependencies: vec![],
                status: WorkItemStatus::Completed,
                description: "done work".to_string(),
                acceptance: vec![satisfied("tests pass")],
                evidence: vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
                owner_run_id: None,
                owner_job_id: None,
                blocker: None,
                next_action: None,
            },
        )
        .await
        .unwrap();
    let items = vec![item];
    let backed = assess_with_engine(&pool, Some(dir.path()), &plan, &items)
        .await
        .unwrap();
    assert_eq!(backed.engine, AssessmentEngine::EggplanGit);
    assert!(backed.assessment.allows_completion());
    // Mutate source before the status transition: S2 differs, completion
    // is refused, plan state and evidence are untouched.
    std::fs::write(dir.path().join("base.txt"), b"changed\n").unwrap();
    let error = complete_plan_with_subject_revalidation(&pool, dir.path(), &plan, &backed)
        .await
        .unwrap_err();
    assert_eq!(error, "subject_changed_before_completion");
    let stored = store.get(&plan.id).await.unwrap().expect("plan row");
    assert_eq!(stored.status, WorkPlanStatus::Active);
}

#[tokio::test(flavor = "current_thread")]
async fn completion_revalidation_accepts_stable_subject() {
    let pool = common::pool::isolated_pool().await;
    let (dir, _head) = git_repo();
    // The facade resolves the repository identity read-only from the
    // workspace registry; register the fixture root first.
    registered_workspace_id(&pool, dir.path()).await;
    let identity = registered_workspace_id(&pool, dir.path()).await;
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let head = head.trim().to_string();
    let job_id = evidence_job(
        &pool,
        JobKind::Test,
        test_payload(),
        Some(stable_provenance(&identity, &head)),
        Some(AttemptState::Completed),
    )
    .await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(codegg_core::work_plan::NewWorkPlan {
            session_id: "sess-diff".to_string(),
            project_id: "proj-diff".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "stable objective".to_string(),
            origin_provenance: "turn:t".to_string(),
            current_phase: None,
        })
        .await
        .unwrap();
    let (_plan, item) = store
        .add_item(
            &plan.id,
            codegg_core::work_plan::NewWorkItem {
                parent_item_id: None,
                dependencies: vec![],
                status: WorkItemStatus::Completed,
                description: "done work".to_string(),
                acceptance: vec![satisfied("tests pass")],
                evidence: vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
                owner_run_id: None,
                owner_job_id: None,
                blocker: None,
                next_action: None,
            },
        )
        .await
        .unwrap();
    // add_item bumps the plan revision; re-read the current plan row so
    // the completion CAS carries a fresh revision.
    let plan = store.get(&plan.id).await.unwrap().expect("plan row");
    let backed = assess_with_engine(&pool, Some(dir.path()), &plan, &[item])
        .await
        .unwrap();
    assert!(backed.assessment.allows_completion());
    assert!(
        complete_plan_with_subject_revalidation(&pool, dir.path(), &plan, &backed)
            .await
            .unwrap(),
        "stable S1/S2 must complete"
    );
    let stored = store.get(&plan.id).await.unwrap().expect("plan row");
    assert_eq!(stored.status, WorkPlanStatus::Completed);
}
