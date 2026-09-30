//! M002 engine selection, git-backed facade, and completion-race tests.
//!
//! - Engine selection is deterministic from structured host state:
//!   terminal history, missing workspace context, unsupported evidence,
//!   non-Git workspace, and the EggplanGit path.
//! - The git-backed facade captures a governed current subject, assesses
//!   through the bridge, and enforces S1/S2 revalidation before the
//!   completion CAS (§13 race test, both directions).
//! - Git capture failure on a Git-backed root fails closed (error, never a
//!   legacy route).

mod common;

use codegg::work_plan_arbiter::{assess_active_plan, maybe_complete_plan_on_turn_end};
use codegg::work_plan_eggplan::{assess_work_plan_with_eggplan, AssessmentEngine};
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, DaemonGeneration, ExecutionSubjectDisposition,
    ExecutionSubjectKind, ExecutionSubjectProvenance, ExecutionSubjectRevision,
    ExecutionSubjectSealKind, ExecutionSubjectState, IdempotencyClass, JobKind, JobPayload,
    JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy, SqliteJobStore,
};
use codegg_core::session::models::CreateSession;
use codegg_core::session::store::SessionStore;
use codegg_core::work_plan::{
    NewWorkItem, NewWorkPlan, WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceKind,
    WorkEvidenceRef, WorkItemStatus, WorkPlanCompletionAssessment, WorkPlanStatus, WorkPlanStore,
};
use codegg_core::workspace::WorkspaceId;
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceRecord, WorkspaceStore};
use sqlx::SqlitePool;
use std::path::Path;
use std::process::Command;

const WS: &str = "ws-engine";

fn init_repo(dir: &Path, commit: bool) {
    Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::fs::write(dir.join("a.txt"), "hello").unwrap();
    if commit {
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
    }
}

fn head_oid(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

async fn session_with_workspace(pool: &SqlitePool, workspace_root: &Path) -> String {
    let now = chrono::Utc::now();
    let workspaces = SqliteWorkspaceStore::new(pool.clone());
    workspaces
        .upsert(&WorkspaceRecord {
            id: WorkspaceId::new_unchecked(WS),
            canonical_root: workspace_root.to_path_buf(),
            display_name: "engine test".to_string(),
            created_at: now,
            last_opened_at: now,
            archived_at: None,
        })
        .await
        .unwrap();
    let sessions = SessionStore::new(pool.clone());
    let session = sessions
        .create(CreateSession {
            project_id: "proj-engine".to_string(),
            directory: workspace_root.to_string_lossy().to_string(),
            workspace_id: Some(WS.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    session.id
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

fn job_spec(payload: JobPayload) -> NewJob {
    NewJob {
        workspace_id: WorkspaceId::new_unchecked(WS),
        session_id: None,
        turn_id: None,
        kind: JobKind::Test,
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

/// Build a passing exact-subject Test job sealed at `oid`, plus an active
/// plan with one completed item citing it. Returns `(plan_id, job_id)`.
async fn passing_setup(pool: &SqlitePool, session_id: &str, oid: &str) -> String {
    let store = SqliteJobStore::new(pool.clone());
    let job = store
        .create_job(job_spec(JobPayload::Test {
            command: "cargo test".into(),
            argv: vec!["cargo".into(), "test".into()],
            cwd: None,
            scope: None,
            parent_run_id: None,
        }))
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
        .set_attempt_source_subject_started(&attempt.attempt_id, &started(oid))
        .await
        .unwrap();
    store
        .seal_attempt_source_subject(&attempt.attempt_id, &stable(oid))
        .await
        .unwrap();
    store
        .finish_attempt(AttemptCompletion {
            attempt_id: attempt.attempt_id.clone(),
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();
    let plans = WorkPlanStore::new(pool.clone());
    let plan = plans
        .create_active(NewWorkPlan {
            session_id: session_id.to_string(),
            project_id: "proj-engine".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "engine objective".to_string(),
            origin_provenance: "turn:engine".to_string(),
            current_phase: None,
        })
        .await
        .unwrap();
    plans
        .add_item(
            &plan.id,
            NewWorkItem {
                parent_item_id: None,
                dependencies: vec![],
                status: WorkItemStatus::Completed,
                description: "engine item".to_string(),
                acceptance: vec![WorkAcceptance {
                    description: "tests pass".to_string(),
                    disposition: WorkAcceptanceDisposition::Unmet,
                    note: None,
                }],
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::TestJob,
                    ref_id: job.job_id.to_string(),
                    detail: None,
                }],
                owner_run_id: None,
                owner_job_id: None,
                blocker: None,
                next_action: None,
            },
        )
        .await
        .unwrap();
    plan.id.as_str().to_string()
}

// ── engine selection ────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn engine_terminal_history_passthrough() {
    let pool = common::pool::isolated_pool().await;
    let dir = tempfile::tempdir().unwrap();
    let session_id = session_with_workspace(&pool, dir.path()).await;
    let plans = WorkPlanStore::new(pool.clone());
    let plan = plans
        .create_active(NewWorkPlan {
            session_id: session_id.clone(),
            project_id: "proj-engine".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "terminal objective".to_string(),
            origin_provenance: "turn:engine".to_string(),
            current_phase: None,
        })
        .await
        .unwrap();
    plans
        .transition_plan(&plan.id, plan.revision, WorkPlanStatus::Completed)
        .await
        .unwrap();
    let plan = plans.get(&plan.id).await.unwrap().unwrap();
    let items = plans.list_items(&plan.id).await.unwrap();
    let backed = assess_work_plan_with_eggplan(&pool, &plan, &items)
        .await
        .unwrap();
    assert_eq!(backed.engine, AssessmentEngine::TerminalHistory);
    assert!(matches!(
        backed.assessment,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(backed.subject.is_none());
    assert!(backed.mapping_digest.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn engine_no_workspace_context() {
    // A session-less plan (legacy sessions, unit-test pools) stays on the
    // legacy engine with a byte-identical legacy result.
    let pool = common::pool::isolated_pool().await;
    let plans = WorkPlanStore::new(pool.clone());
    let plan = plans
        .create_active(NewWorkPlan {
            session_id: "sess-without-row".to_string(),
            project_id: "proj-engine".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "legacy objective".to_string(),
            origin_provenance: "turn:engine".to_string(),
            current_phase: None,
        })
        .await
        .unwrap();
    plans
        .add_item(
            &plan.id,
            NewWorkItem {
                parent_item_id: None,
                dependencies: vec![],
                status: WorkItemStatus::Actionable,
                description: "legacy item".to_string(),
                acceptance: vec![WorkAcceptance {
                    description: "c".to_string(),
                    disposition: WorkAcceptanceDisposition::Unmet,
                    note: None,
                }],
                evidence: vec![],
                owner_run_id: None,
                owner_job_id: None,
                blocker: None,
                next_action: None,
            },
        )
        .await
        .unwrap();
    let plan = plans.get(&plan.id).await.unwrap().unwrap();
    let items = plans.list_items(&plan.id).await.unwrap();
    let backed = assess_work_plan_with_eggplan(&pool, &plan, &items)
        .await
        .unwrap();
    assert_eq!(backed.engine, AssessmentEngine::LegacyNoWorkspaceContext);
    assert!(matches!(
        backed.assessment,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn engine_unsupported_evidence_kind() {
    // Artifact refs select the explicit legacy engine for the whole
    // assessment with the legacy result intact.
    let pool = common::pool::isolated_pool().await;
    let dir = tempfile::tempdir().unwrap();
    let session_id = session_with_workspace(&pool, dir.path()).await;
    let plans = WorkPlanStore::new(pool.clone());
    let plan = plans
        .create_active(NewWorkPlan {
            session_id: session_id.clone(),
            project_id: "proj-engine".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "artifact objective".to_string(),
            origin_provenance: "turn:engine".to_string(),
            current_phase: None,
        })
        .await
        .unwrap();
    plans
        .add_item(
            &plan.id,
            NewWorkItem {
                parent_item_id: None,
                dependencies: vec![],
                status: WorkItemStatus::Completed,
                description: "artifact item".to_string(),
                acceptance: vec![WorkAcceptance {
                    description: "artifact exists".to_string(),
                    disposition: WorkAcceptanceDisposition::Unmet,
                    note: None,
                }],
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::Artifact,
                    ref_id: "artifact-1".to_string(),
                    detail: None,
                }],
                owner_run_id: None,
                owner_job_id: None,
                blocker: None,
                next_action: None,
            },
        )
        .await
        .unwrap();
    let plan = plans.get(&plan.id).await.unwrap().unwrap();
    let items = plans.list_items(&plan.id).await.unwrap();
    let backed = assess_work_plan_with_eggplan(&pool, &plan, &items)
        .await
        .unwrap();
    assert_eq!(backed.engine, AssessmentEngine::LegacyUnsupportedEvidence);
    assert!(backed.engine_detail.contains("unsupported_evidence_kind"));
    // Legacy result: completed item with unavailable artifact evidence
    // stays actionable.
    assert!(matches!(
        backed.assessment,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn engine_non_git_workspace() {
    // A resolved workspace root that is positively not a git repository
    // selects LegacyNonGit. A git capture error would never select it
    // (covered by the capture-failure test below).
    let pool = common::pool::isolated_pool().await;
    let dir = tempfile::tempdir().unwrap();
    let session_id = session_with_workspace(&pool, dir.path()).await;
    let plans = WorkPlanStore::new(pool.clone());
    let plan = plans
        .create_active(NewWorkPlan {
            session_id: session_id.clone(),
            project_id: "proj-engine".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "non-git objective".to_string(),
            origin_provenance: "turn:engine".to_string(),
            current_phase: None,
        })
        .await
        .unwrap();
    plans
        .add_item(
            &plan.id,
            NewWorkItem {
                parent_item_id: None,
                dependencies: vec![],
                status: WorkItemStatus::Actionable,
                description: "non-git item".to_string(),
                acceptance: vec![WorkAcceptance {
                    description: "c".to_string(),
                    disposition: WorkAcceptanceDisposition::Unmet,
                    note: None,
                }],
                evidence: vec![],
                owner_run_id: None,
                owner_job_id: None,
                blocker: None,
                next_action: None,
            },
        )
        .await
        .unwrap();
    let plan = plans.get(&plan.id).await.unwrap().unwrap();
    let items = plans.list_items(&plan.id).await.unwrap();
    let backed = assess_work_plan_with_eggplan(&pool, &plan, &items)
        .await
        .unwrap();
    assert_eq!(backed.engine, AssessmentEngine::LegacyNonGit);
    assert!(matches!(
        backed.assessment,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
}

// ── git-backed facade + §13 race ────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn facade_git_backed_complete_and_stable_close() {
    let pool = common::pool::isolated_pool().await;
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), true);
    let oid = head_oid(dir.path());
    let session_id = session_with_workspace(&pool, dir.path()).await;
    let _ = passing_setup(&pool, &session_id, &oid).await;

    let Some((plan, _, backed)) = assess_active_plan(&pool, &session_id).await.unwrap() else {
        panic!("expected an active plan");
    };
    assert_eq!(backed.engine, AssessmentEngine::EggplanGit);
    assert_eq!(
        backed.eggplan_completion_family.as_deref(),
        Some("complete")
    );
    assert!(backed.mapping_digest.is_some());
    let subject = backed.subject.clone().expect("S1 present");
    assert_eq!(subject.revision, oid);
    assert!(matches!(
        backed.assessment,
        WorkPlanCompletionAssessment::Complete { .. }
    ));

    // Stable S1/S2: the completion CAS proceeds.
    assert!(
        maybe_complete_plan_on_turn_end(&pool, &plan, &backed, false)
            .await
            .unwrap()
    );
    let plans = WorkPlanStore::new(pool.clone());
    // The plan is completed, so no active plan remains.
    assert!(plans
        .active_for_session(&session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn facade_source_drift_blocks_completion_cas() {
    // §13: assess at S1 (completion-eligible), mutate the source, then the
    // status transition must refuse: no completion, no evidence rewrite,
    // and re-assessment reports stale applicability.
    let pool = common::pool::isolated_pool().await;
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), true);
    let oid = head_oid(dir.path());
    let session_id = session_with_workspace(&pool, dir.path()).await;
    let _ = passing_setup(&pool, &session_id, &oid).await;

    let Some((plan, items, backed)) = assess_active_plan(&pool, &session_id).await.unwrap() else {
        panic!("expected an active plan");
    };
    assert!(matches!(
        backed.assessment,
        WorkPlanCompletionAssessment::Complete { .. }
    ));

    // Mutate the source after the assessment and before the transition.
    std::fs::write(dir.path().join("uncommitted.txt"), "drift").unwrap();

    assert!(
        !maybe_complete_plan_on_turn_end(&pool, &plan, &backed, false)
            .await
            .unwrap(),
        "drifted S2 must block the completion CAS"
    );
    let plans = WorkPlanStore::new(pool.clone());
    let reloaded = plans
        .active_for_session(&session_id)
        .await
        .unwrap()
        .expect("plan stays active");
    assert_eq!(reloaded.status, WorkPlanStatus::Active);

    // The historical evidence rows are untouched by the refused close.
    let resolved = codegg::work_plan_evidence::assemble_resolved(&pool, &items)
        .await
        .unwrap();
    assert_eq!(resolved.entries.len(), 1);
    assert_eq!(
        resolved.entries[0].status,
        codegg_core::work_plan::HostEvidenceStatus::Passed
    );

    // Re-assessment against the drifted subject reports stale applicability:
    // terminal status with no exact subject, never completion.
    let Some((_, _, stale)) = assess_active_plan(&pool, &session_id).await.unwrap() else {
        panic!("expected an active plan");
    };
    assert_eq!(stale.engine, AssessmentEngine::EggplanGit);
    assert!(
        !stale.assessment.allows_completion(),
        "stale evidence must not allow completion"
    );
    assert!(matches!(
        stale.assessment,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn facade_capture_failure_fails_closed() {
    // A git-backed root whose capture fails (empty repo: no HEAD) returns
    // a fail-closed error, never a legacy route.
    let pool = common::pool::isolated_pool().await;
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), false);
    let session_id = session_with_workspace(&pool, dir.path()).await;
    let plans = WorkPlanStore::new(pool.clone());
    let plan = plans
        .create_active(NewWorkPlan {
            session_id: session_id.clone(),
            project_id: "proj-engine".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "capture failure objective".to_string(),
            origin_provenance: "turn:engine".to_string(),
            current_phase: None,
        })
        .await
        .unwrap();
    plans
        .add_item(
            &plan.id,
            NewWorkItem {
                parent_item_id: None,
                dependencies: vec![],
                status: WorkItemStatus::Actionable,
                description: "capture failure item".to_string(),
                acceptance: vec![WorkAcceptance {
                    description: "c".to_string(),
                    disposition: WorkAcceptanceDisposition::Unmet,
                    note: None,
                }],
                evidence: vec![],
                owner_run_id: None,
                owner_job_id: None,
                blocker: None,
                next_action: None,
            },
        )
        .await
        .unwrap();
    let plan = plans.get(&plan.id).await.unwrap().unwrap();
    let items = plans.list_items(&plan.id).await.unwrap();
    let error = assess_work_plan_with_eggplan(&pool, &plan, &items)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            codegg::work_plan_eggplan::AssessmentAdapterError::CurrentCaptureFailed(_)
        ),
        "capture failure must fail closed, got {error}"
    );
}
