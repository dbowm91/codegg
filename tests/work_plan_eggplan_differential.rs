//! M002 differential qualification: legacy assessor vs Eggplan adapter.
//!
//! Every case runs the same structured plan/items through both engines over
//! the same durable stores and asserts the outcome pair. The universal
//! invariant (the §12 permissiveness rule) is: Eggplan allowing completion
//! implies legacy allows completion. Stricter Eggplan outcomes are asserted
//! explicitly per case with their documented authority; a more-permissive
//! Eggplan outcome fails the suite unconditionally.
//!
//! Recorded stricter deltas (all Eggplan-non-complete vs legacy-Complete,
//! attributable to subject/verification authority legacy did not model):
//!
//! - forged serialized `Satisfied` without bound evidence (verification
//!   authority: disposition text is not bound proof);
//! - stale/drifted/incomplete-materialization/legacy-missing subjects
//!   (subject authority: terminal status alone is not exact-subject proof);
//! - unbindable execution specs (no durable reconstruction, e.g. legacy
//!   `Subagent` or non-executable payloads);
//! - unlinked `AgentRun` rows (status without a durable spec);
//! - `Cancelled` items (no bound completion evidence by design);
//! - multi-ref items where legacy any-passed ignored failed co-evidence
//!   (every cited requirement must be satisfied).
//!
//! Recorded variant-ordering deltas (both engines refuse completion, only
//! the family label differs):
//!
//! - plans mixing actionable and blocked items: legacy reports the first
//!   actionable item, Eggplan reports `Blocked` (highest-severity wins);
//! - owner-provenance-only in-flight items: legacy surfaces an `InFlight`
//!   owner handle, Eggplan reports actionable (no observation, no handle).

mod common;

use codegg::work_plan_eggplan::{assess_snapshot_with_subject, project_bridge_assessment};
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, AgentRunTerminalOutcome, NewAgentRun, NewAgentTask,
    SqliteAgentRunStore,
};
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, DaemonGeneration, ExecutionSubjectDisposition,
    ExecutionSubjectKind, ExecutionSubjectProvenance, ExecutionSubjectRevision,
    ExecutionSubjectSealKind, ExecutionSubjectState, ExecutionSubjectUnavailableReason,
    IdempotencyClass, JobId, JobKind, JobPayload, JobPriority, JobSource, JobStore, NewJob,
    ResourceRequest, RetryPolicy, SqliteJobStore,
};
use codegg_core::work_plan::{
    assess_work_plan, item_is_satisfied, HostEvidenceStatus, NewWorkItem, NewWorkPlan,
    WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceKind, WorkEvidenceRef, WorkItem,
    WorkItemId, WorkItemStatus, WorkPlan, WorkPlanCompletionAssessment, WorkPlanId, WorkPlanStatus,
    WorkPlanStore,
};
use codegg_core::workspace::WorkspaceId;
use eggplan_core::{SubjectRevision, SubjectState as EggplanSubjectState};
use sqlx::SqlitePool;

const WS: &str = "ws-evidence";
const OID_S1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OID_STALE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// ── fixtures ────────────────────────────────────────────────────────────────

fn job_spec(kind: JobKind, payload: JobPayload) -> NewJob {
    NewJob {
        workspace_id: WorkspaceId::new_unchecked(WS),
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

fn test_payload() -> JobPayload {
    JobPayload::Test {
        command: "cargo test".into(),
        argv: vec!["cargo".into(), "test".into()],
        cwd: None,
        scope: None,
        parent_run_id: None,
    }
}

fn subagent_run_payload() -> JobPayload {
    JobPayload::SubagentRun {
        prompt: "summarize the diff".into(),
        agent: "reviewer".into(),
        model: Some("model-x".into()),
        parent_id: None,
        denied_tools: vec![],
        allowed_paths: vec![],
        max_tool_calls: Some(10),
        task_id: codegg_core::identity::AgentTaskId::new(),
        run_id: codegg_core::identity::AgentRunId::new(),
        delegation_key: "delegation-1".into(),
        base_commit: None,
    }
}

fn clean_revision(oid: &str) -> ExecutionSubjectRevision {
    ExecutionSubjectRevision {
        schema_version: ExecutionSubjectRevision::SCHEMA_VERSION,
        subject_kind: ExecutionSubjectKind::Git,
        repository_identity: format!("codegg-workspace:{WS}"),
        revision: oid.to_string(),
        state: ExecutionSubjectState::Clean,
        dirty_digest: None,
    }
}

fn started(oid: &str) -> ExecutionSubjectProvenance {
    ExecutionSubjectProvenance {
        schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
        captured: Some(clean_revision(oid)),
        sealed: None,
        disposition: ExecutionSubjectDisposition::Started,
        seal_kind: ExecutionSubjectSealKind::LiveExecutionEnd,
        unavailable_reason: None,
        materialization: None,
    }
}

fn stable(oid: &str) -> ExecutionSubjectProvenance {
    let revision = clean_revision(oid);
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

fn drifted(s1: &str, s2: &str) -> ExecutionSubjectProvenance {
    ExecutionSubjectProvenance {
        schema_version: ExecutionSubjectProvenance::SCHEMA_VERSION,
        captured: Some(clean_revision(s1)),
        sealed: Some(clean_revision(s2)),
        disposition: ExecutionSubjectDisposition::Drifted,
        seal_kind: ExecutionSubjectSealKind::LiveExecutionEnd,
        unavailable_reason: None,
        materialization: None,
    }
}

fn current_subject() -> SubjectRevision {
    SubjectRevision {
        subject_kind: "git".into(),
        repository_id: format!("codegg-workspace:{WS}"),
        revision: OID_S1.into(),
        state: EggplanSubjectState::Clean,
        dirty_digest: None,
    }
}

/// Create a job, run one attempt through S1/seal/finish. Returns
/// `(job_id, attempt_id)`.
async fn sealed_job(
    pool: &SqlitePool,
    payload: JobPayload,
    kind: JobKind,
    seal: Option<ExecutionSubjectProvenance>,
    finish: AttemptState,
) -> (String, String) {
    sealed_job_from(pool, payload, kind, OID_S1, seal, finish).await
}

/// Create a job, run one attempt through start/seal/finish, where the
/// started subject uses `start_oid` (the seal must be continuous with the
/// start; use a different start oid to model historically exact-but-stale
/// evidence).
async fn sealed_job_from(
    pool: &SqlitePool,
    payload: JobPayload,
    kind: JobKind,
    start_oid: &str,
    seal: Option<ExecutionSubjectProvenance>,
    finish: AttemptState,
) -> (String, String) {
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
    store
        .set_attempt_source_subject_started(&attempt.attempt_id, &started(start_oid))
        .await
        .unwrap();
    if let Some(seal) = seal {
        store
            .seal_attempt_source_subject(&attempt.attempt_id, &seal)
            .await
            .unwrap();
    }
    store
        .finish_attempt(AttemptCompletion {
            attempt_id: attempt.attempt_id.clone(),
            state: finish,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();
    (job.job_id.to_string(), attempt.attempt_id.to_string())
}

fn plan_fixture(status: WorkPlanStatus) -> (WorkPlan, Vec<WorkItem>) {
    let now = chrono::Utc::now();
    let plan = WorkPlan {
        id: WorkPlanId::generate(),
        revision: 0,
        session_id: "sess-differential".to_string(),
        project_id: "proj-differential".to_string(),
        origin_turn_id: None,
        goal_id: None,
        objective: "differential objective".to_string(),
        objective_digest: "sha256:differential".to_string(),
        origin_provenance: "turn:differential".to_string(),
        status,
        current_phase: None,
        current_item_id: None,
        created_at: now,
        updated_at: now,
        completed_at: None,
    };
    (plan, Vec::new())
}

fn item_fixture(
    plan: &WorkPlan,
    status: WorkItemStatus,
    position: i64,
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
        description: format!("differential item {position}"),
        acceptance,
        evidence,
        owner_run_id: None,
        owner_job_id: None,
        attempts: 0,
        blocker: None,
        next_action: Some("do the next step".to_string()),
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

fn human(description: &str) -> WorkAcceptance {
    WorkAcceptance {
        description: description.to_string(),
        disposition: WorkAcceptanceDisposition::RequiresUserJudgment,
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

fn evidence_ref(kind: WorkEvidenceKind, ref_id: &str) -> WorkEvidenceRef {
    WorkEvidenceRef {
        kind,
        ref_id: ref_id.to_string(),
        detail: None,
    }
}

fn reason_code(assessment: &WorkPlanCompletionAssessment) -> &'static str {
    assessment.reason_code()
}

/// Run both engines over the same durable state. Returns
/// `(legacy, eggplan_dto, eggplan_family)`.
async fn run_both(
    pool: &SqlitePool,
    plan: &WorkPlan,
    items: &[WorkItem],
) -> (
    WorkPlanCompletionAssessment,
    WorkPlanCompletionAssessment,
    String,
) {
    let legacy_snapshot = codegg::work_plan_evidence::assemble(pool, items)
        .await
        .unwrap();
    let legacy = assess_work_plan(plan, items, &legacy_snapshot);
    let current = current_subject();
    let (bridge, resolved) = assess_snapshot_with_subject(pool, plan, items, &current)
        .await
        .unwrap();
    let family = bridge.completion_family.as_str().to_string();
    let dto = project_bridge_assessment(plan, items, &resolved, &bridge);
    (legacy, dto, family)
}

/// The §12 permissiveness rule: an Eggplan `allows_completion == true`
/// with legacy `false` is a hard stop.
fn assert_permissiveness(
    legacy: &WorkPlanCompletionAssessment,
    eggplan: &WorkPlanCompletionAssessment,
) {
    assert!(
        !eggplan.allows_completion() || legacy.allows_completion(),
        "HARD STOP: eggplan allows completion while legacy refuses: legacy={} eggplan={}",
        reason_code(legacy),
        reason_code(eggplan),
    );
}

// ── §12 matrix: parity cases ────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn differential_no_evidence() {
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Actionable,
        0,
        vec![unmet("c")],
        vec![],
    )];
    let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_eq!(family, "actionable_work_remaining");
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_passing_test_job() {
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert_eq!(family, "complete");
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_failed_test_job() {
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Failed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_missing_test_job() {
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, "job-missing")],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_inflight_test_job() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool.clone());
    let job = store
        .create_job(job_spec(JobKind::Test, test_payload()))
        .await
        .unwrap();
    let attempt = store
        .begin_attempt(&job.job_id, &DaemonGeneration::new())
        .await
        .unwrap();
    store
        .mark_attempt_running(&attempt.attempt_id)
        .await
        .unwrap();
    store
        .set_attempt_source_subject_started(&attempt.attempt_id, &started(OID_S1))
        .await
        .unwrap();
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::InProgress,
        0,
        vec![unmet("c")],
        vec![evidence_ref(
            WorkEvidenceKind::TestJob,
            &job.job_id.to_string(),
        )],
    )];
    let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::InFlight { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::InFlight { .. }
    ));
    assert_eq!(family, "in_flight");
    if let WorkPlanCompletionAssessment::InFlight {
        ref handle_kind,
        ref handle_id,
        ..
    } = eggplan
    {
        assert_eq!(handle_kind, "test_job");
        assert_eq!(handle_id, &job.job_id.to_string());
    } else {
        panic!("expected inflight");
    }
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_blocked_item() {
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let mut item = item_fixture(&plan, WorkItemStatus::Blocked, 0, vec![unmet("c")], vec![]);
    item.blocker = Some("waiting on review".to_string());
    let (legacy, eggplan, family) = run_both(&pool, &plan, &[item]).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Blocked { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::Blocked { .. }
    ));
    assert_eq!(family, "blocked");
    if let WorkPlanCompletionAssessment::Blocked { ref blocker, .. } = eggplan {
        assert!(blocker.contains("waiting on review"));
    }
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_dependency_gated_item() {
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let first = item_fixture(
        &plan,
        WorkItemStatus::Actionable,
        0,
        vec![unmet("a")],
        vec![],
    );
    let mut second = item_fixture(&plan, WorkItemStatus::Pending, 1, vec![unmet("b")], vec![]);
    second.dependencies = vec![first.id.clone()];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &[first, second]).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    // The first actionable item drives both engines here.
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_human_judgment_only_item() {
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Pending,
        0,
        vec![human("owner signs off")],
        vec![],
    )];
    let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::AwaitingUserJudgment { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::AwaitingUserJudgment { .. }
    ));
    assert_eq!(family, "awaiting_user_judgment");
    assert!(legacy.allows_completion() && eggplan.allows_completion());
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_completed_item_without_host_proof() {
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_scheduler_job_kinds() {
    for (name, payload) in [
        (
            "managed_argv",
            JobPayload::ManagedArgv {
                argv: vec!["cargo".into(), "build".into()],
                cwd: None,
            },
        ),
        (
            "shell",
            JobPayload::Shell {
                command: "cargo build".into(),
                argv: Some(vec!["cargo".into(), "build".into()]),
                cwd: None,
            },
        ),
        (
            "python",
            JobPayload::Python {
                script_path: "scripts/check.py".into(),
                args: vec![],
                mode: "verify".into(),
                source: Some("print('ok')".into()),
                source_hash: None,
                cwd: None,
                timeout_secs: None,
            },
        ),
        (
            "git",
            JobPayload::Git {
                argv: vec!["git".into(), "status".into()],
                cwd: None,
            },
        ),
    ] {
        let pool = common::pool::isolated_pool().await;
        let kind = match name {
            "managed_argv" | "shell" => JobKind::Shell,
            "python" => JobKind::Python,
            _ => JobKind::GitRead,
        };
        let (job_id, _) = sealed_job(
            &pool,
            payload,
            kind,
            Some(stable(OID_S1)),
            AttemptState::Completed,
        )
        .await;
        let (plan, _) = plan_fixture(WorkPlanStatus::Active);
        let items = vec![item_fixture(
            &plan,
            WorkItemStatus::Completed,
            0,
            vec![unmet("c")],
            vec![evidence_ref(WorkEvidenceKind::SchedulerJob, &job_id)],
        )];
        let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
        assert!(
            matches!(legacy, WorkPlanCompletionAssessment::Complete { .. }),
            "{name}: legacy must complete"
        );
        assert!(
            matches!(eggplan, WorkPlanCompletionAssessment::Complete { .. }),
            "{name}: eggplan must complete"
        );
        assert_eq!(family, "complete");
        assert_permissiveness(&legacy, &eggplan);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn differential_delegated_run() {
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        subagent_run_payload(),
        JobKind::Subagent,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::DelegatedRun, &job_id)],
    )];
    let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert_eq!(family, "complete");
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_linked_agent_run() {
    let pool = common::pool::isolated_pool().await;
    let (job_id, attempt_id) = sealed_job(
        &pool,
        subagent_run_payload(),
        JobKind::Subagent,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    let run_id = create_completed_agent_run(
        &pool,
        Some(JobId::new_unchecked(job_id)),
        Some(codegg_core::jobs::AttemptId::new_unchecked(attempt_id)),
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::AgentRun, &run_id)],
    )];
    let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert_eq!(family, "complete");
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_dangling_agent_run() {
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::AgentRun, "run-missing")],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_shared_ref_across_items() {
    // One passing job cited by two completed items: both engines satisfy
    // both items, and the resolver keeps deterministic unique observation
    // IDs (the bridge would reject reused IDs).
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![
        item_fixture(
            &plan,
            WorkItemStatus::Completed,
            0,
            vec![unmet("a")],
            vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
        ),
        item_fixture(
            &plan,
            WorkItemStatus::Completed,
            1,
            vec![unmet("b")],
            vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
        ),
    ];
    let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert_eq!(family, "complete");
    assert_permissiveness(&legacy, &eggplan);
}

// ── §12 matrix: recorded stricter deltas ────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn differential_forged_satisfied_disposition() {
    // Serialized `Satisfied` without bound evidence satisfies legacy but
    // can never satisfy Eggplan: disposition text is not bound proof.
    // Recorded stricter delta (verification authority).
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![satisfied("model asserts done")],
        vec![],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_stale_historical_subject() {
    // Terminal Passed status with a sealed subject that no longer equals
    // the current one: legacy completes on status alone; Eggplan stays
    // fail-closed. Recorded stricter delta (subject authority).
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job_from(
        &pool,
        test_payload(),
        JobKind::Test,
        OID_STALE,
        Some(stable(OID_STALE)),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_drifted_subject() {
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(drifted(OID_S1, OID_STALE)),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_materialization_incomplete() {
    let pool = common::pool::isolated_pool().await;
    let mut incomplete = stable(OID_S1);
    incomplete.disposition = ExecutionSubjectDisposition::Unavailable;
    incomplete.unavailable_reason =
        Some(ExecutionSubjectUnavailableReason::MaterializationIncomplete);
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(incomplete),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_legacy_missing_provenance() {
    // Pre-M001 attempt: started S1 but never sealed. Legacy completes on
    // status; Eggplan has no Stable proof. Recorded stricter delta.
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        None,
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_unbindable_spec() {
    // `DelegatedRun` evidence over a legacy `Subagent` payload (no durable
    // task/run/delegation identity): legacy completes on status; Eggplan
    // cannot bind it. Recorded stricter delta (verification authority).
    let pool = common::pool::isolated_pool().await;
    let legacy_payload = JobPayload::Subagent {
        prompt: "old-style delegation".into(),
        agent: "agent".into(),
        model: None,
        parent_id: None,
        denied_tools: vec![],
        allowed_paths: vec![],
        max_tool_calls: None,
    };
    let (job_id, _) = sealed_job(
        &pool,
        legacy_payload,
        JobKind::Subagent,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::DelegatedRun, &job_id)],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_unlinked_agent_run_status() {
    // A completed `agent_run` row without a job link resolves Passed for
    // legacy, but Eggplan has no durable spec to bind. Recorded stricter
    // delta (verification authority).
    let pool = common::pool::isolated_pool().await;
    let run_id = create_completed_agent_run(&pool, None, None).await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::AgentRun, &run_id)],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_cancelled_item() {
    // A `Cancelled` item carries no bound completion evidence by design:
    // legacy excludes it as terminal scope, Eggplan keeps the plan open.
    // Recorded stricter delta.
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let mut done = item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("a")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    );
    done.next_action = None;
    let cancelled = item_fixture(
        &plan,
        WorkItemStatus::Cancelled,
        1,
        vec![unmet("b")],
        vec![],
    );
    let (legacy, eggplan, _) = run_both(&pool, &plan, &[done, cancelled]).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_failed_coevidence() {
    // Legacy any-passed satisfaction ignores a failed co-ref; Eggplan
    // requires every cited requirement to be satisfied. Recorded stricter
    // delta (requirement authority).
    let pool = common::pool::isolated_pool().await;
    let (passing, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    // The failing ref carries a distinct verification identity: same-spec
    // co-evidence shares Eggplan's any-passed requirement semantics with
    // legacy, but a distinct failed requirement blocks satisfaction.
    let mut failing_payload = test_payload();
    if let JobPayload::Test { argv, .. } = &mut failing_payload {
        argv.push("--ignored".to_string());
    }
    let (failing, _) = sealed_job(
        &pool,
        failing_payload,
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Failed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![
            evidence_ref(WorkEvidenceKind::TestJob, &passing),
            evidence_ref(WorkEvidenceKind::TestJob, &failing),
        ],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

// ── N1/N2 normalization parity ──────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn differential_mixed_human_and_unmet() {
    // N1: `Unmet` dominates judgment-only in CodeGG; the adapter projects
    // human acceptances as unmet so Eggplan cannot allow completion on the
    // human criterion alone.
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![human("owner signs off"), unmet("tests pass")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert_eq!(family, "complete");
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_human_with_failed_evidence() {
    // N2: judgment-only requires clean evidence. A human acceptance with
    // failed evidence stays open in both engines.
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Failed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![human("owner signs off")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_human_with_dangling_evidence() {
    // N2 with a dangling ref: legacy refuses judgment-only; the adapter
    // normalizes so Eggplan agrees.
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = vec![item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![human("owner signs off")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, "job-missing")],
    )];
    let (legacy, eggplan, _) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

// ── §12 matrix: recorded variant-ordering deltas ────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn differential_actionable_plus_blocked_items() {
    // Legacy reports the first actionable item; Eggplan reports Blocked
    // (highest severity wins). Both refuse completion.
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let actionable = item_fixture(
        &plan,
        WorkItemStatus::Actionable,
        0,
        vec![unmet("a")],
        vec![],
    );
    let mut blocked = item_fixture(&plan, WorkItemStatus::Blocked, 1, vec![unmet("b")], vec![]);
    blocked.blocker = Some("waiting on review".to_string());
    let (legacy, eggplan, family) = run_both(&pool, &plan, &[actionable, blocked]).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::Blocked { .. }
    ));
    assert_eq!(family, "blocked");
    assert_permissiveness(&legacy, &eggplan);
}

#[tokio::test(flavor = "current_thread")]
async fn differential_owner_provenance_only_inflight() {
    // An InProgress item with owner provenance but no live ref: legacy
    // surfaces an InFlight owner handle; Eggplan has no observation to
    // wait on and reports actionable. Both refuse completion.
    let pool = common::pool::isolated_pool().await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let mut item = item_fixture(
        &plan,
        WorkItemStatus::InProgress,
        0,
        vec![unmet("c")],
        vec![],
    );
    item.owner_run_id = Some("run-owner-1".to_string());
    let (legacy, eggplan, _) = run_both(&pool, &plan, &[item]).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::InFlight { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    assert_permissiveness(&legacy, &eggplan);
}

// ── bridge enforcement: verification binding ────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn differential_bridge_rejects_binding_mismatch() {
    // The bridge enforces expected==observed verification digests: a
    // resolver that binds the wrong digest fails the whole mapping instead
    // of satisfying the requirement. Proves mismatch can never satisfy.
    use eggplan_codegg_compat::{assess_codegg_snapshot, CodeggPlanSnapshot};
    use eggplan_core::{ProviderDescriptor, ProviderRegistry};
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    let (plan, _) = plan_fixture(WorkPlanStatus::Active);
    let items = [item_fixture(
        &plan,
        WorkItemStatus::Completed,
        0,
        vec![unmet("c")],
        vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
    )];
    let current = current_subject();
    let store = SqliteJobStore::new(pool.clone());
    let job = store
        .get_job(&JobId::new_unchecked(job_id.clone()))
        .await
        .unwrap()
        .unwrap();
    let correct = codegg::work_plan_eggplan::verification_digest_for_test(&job).unwrap();
    let wrong =
        eggplan_core::VerificationDigest::new(format!("sha256:{}", "0".repeat(64))).unwrap();
    assert_ne!(correct, wrong);
    let snapshot = CodeggPlanSnapshot {
        plan_id: plan.id.as_str().to_string(),
        revision: 0,
        status: eggplan_codegg_compat::CodeggPlanStatus::Active,
        objective: plan.objective.clone(),
        current_phase: None,
        current_item_id: None,
        items: vec![eggplan_codegg_compat::CodeggItemSnapshot {
            item_id: items[0].id.as_str().to_string(),
            position: 0,
            parent_item_id: None,
            dependencies: vec![],
            status: eggplan_codegg_compat::CodeggItemStatus::Completed,
            description: items[0].description.clone(),
            acceptance: vec![eggplan_codegg_compat::CodeggAcceptance {
                description: "c".to_string(),
                disposition: eggplan_codegg_compat::AcceptanceDisposition::Unmet,
                note: None,
            }],
            evidence: vec![eggplan_codegg_compat::CodeggEvidenceRef {
                kind: eggplan_codegg_compat::CodeggEvidenceKind::TestJob,
                ref_id: job_id.clone(),
                detail: None,
            }],
            owner_run_id: None,
            owner_job_id: None,
            blocker: None,
            next_action: None,
        }],
    };
    struct WrongBinder {
        digest: eggplan_core::VerificationDigest,
        job_status: HostEvidenceStatus,
    }
    impl eggplan_codegg_compat::EvidenceResolver for WrongBinder {
        fn resolve(
            &mut self,
            _reference: &eggplan_codegg_compat::CodeggEvidenceRef,
            subject: &SubjectRevision,
        ) -> Result<eggplan_codegg_compat::ResolvedEvidence, String> {
            use eggplan_core::{
                EvidenceKind, EvidenceObservation, EvidenceObservationId, EvidenceObservationInput,
                EvidenceProviderId, EvidenceStatus,
            };
            let observation = EvidenceObservation::finalize(EvidenceObservationInput {
                id: EvidenceObservationId::new("epe_wrong_binder".to_string()).unwrap(),
                provider_id: EvidenceProviderId::new("epp_codegg_host".to_string()).unwrap(),
                kind: EvidenceKind::Test,
                status: match self.job_status {
                    HostEvidenceStatus::Passed => EvidenceStatus::Passed,
                    _ => EvidenceStatus::Failed,
                },
                subject: subject.clone(),
                observed_at_unix_ms: 0,
                invocation_ref: None,
                verification_digest: Some(self.digest.clone()),
                result_metadata: Default::default(),
                artifacts: vec![],
            })
            .unwrap();
            Ok(eggplan_codegg_compat::ResolvedEvidence {
                observation,
                // Deliberately bind a different digest than the observation
                // carries: the bridge must reject, never satisfy.
                expected_verification: Some(
                    eggplan_core::VerificationDigest::new(format!("sha256:{}", "f".repeat(64)))
                        .unwrap(),
                ),
            })
        }
    }
    let mut providers = ProviderRegistry::default();
    providers
        .register_trusted(
            ProviderDescriptor::new(
                eggplan_core::EvidenceProviderId::new("epp_codegg_host".to_string()).unwrap(),
                "codegg-host",
                [eggplan_core::EvidenceKind::Test],
            )
            .unwrap(),
        )
        .unwrap();
    let mut resolver = WrongBinder {
        digest: wrong,
        job_status: HostEvidenceStatus::Passed,
    };
    let rejected = assess_codegg_snapshot(&snapshot, current, &mut resolver, &providers).is_err();
    assert!(rejected, "binding mismatch must fail the mapping");
}

// ── store-backed plan creation parity ───────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn differential_store_created_plan() {
    // Plans/items created through the durable store (not fixtures) assess
    // identically through both engines.
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        test_payload(),
        JobKind::Test,
        Some(stable(OID_S1)),
        AttemptState::Completed,
    )
    .await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(NewWorkPlan {
            session_id: "sess-differential".to_string(),
            project_id: "proj-differential".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "differential objective".to_string(),
            origin_provenance: "turn:differential".to_string(),
            current_phase: None,
        })
        .await
        .unwrap();
    let (_, item) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                parent_item_id: None,
                dependencies: vec![],
                status: WorkItemStatus::Completed,
                description: "differential item".to_string(),
                acceptance: vec![unmet("c")],
                evidence: vec![evidence_ref(WorkEvidenceKind::TestJob, &job_id)],
                owner_run_id: None,
                owner_job_id: None,
                blocker: None,
                next_action: None,
            },
        )
        .await
        .unwrap();
    assert!(item_is_satisfied(
        &item,
        &codegg::work_plan_evidence::assemble(&pool, std::slice::from_ref(&item))
            .await
            .unwrap()
    ));
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    let items = store.list_items(&plan.id).await.unwrap();
    let (legacy, eggplan, family) = run_both(&pool, &plan, &items).await;
    assert!(matches!(
        legacy,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(matches!(
        eggplan,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert_eq!(family, "complete");
    assert_permissiveness(&legacy, &eggplan);
}

// ── helpers ─────────────────────────────────────────────────────────────────

async fn create_completed_agent_run(
    pool: &SqlitePool,
    job_id: Option<JobId>,
    attempt_id: Option<codegg_core::jobs::AttemptId>,
) -> String {
    use codegg_core::agent_run::AgentRunStatus;
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
                workspace_id: WorkspaceId::new_unchecked(WS),
                requested_agent: "agent".to_string(),
                delegation_key: format!("delegation-{}", uuid::Uuid::new_v4().simple()),
                request_fingerprint: "fingerprint".to_string(),
                description: "linked run".to_string(),
            },
            NewAgentRun {
                run_id: codegg_core::identity::AgentRunId::new(),
                parent_run_id: None,
                depth: 1,
                workspace_id: WorkspaceId::new_unchecked(WS),
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
    if let Some(job_id) = job_id {
        store.attach_job(&run_id, job_id).await.unwrap();
    }
    if let Some(attempt_id) = attempt_id {
        store.attach_attempt(&run_id, attempt_id).await.unwrap();
    }
    for next in [
        AgentRunStatus::Queued,
        AgentRunStatus::Preparing,
        AgentRunStatus::Running,
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
    run_id.to_string()
}
