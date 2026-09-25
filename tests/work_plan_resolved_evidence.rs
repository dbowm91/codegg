//! C002 corrective: AgentRun-link resolution and Eggplan bridge fixtures.
//!
//! Regression coverage for the `agent_run` link predicate fix (`run_id`,
//! not `id`) and golden tests pinning the lossless Eggplan five-field
//! projection. The legacy status-only `assemble` snapshot is unchanged.

mod common;

use codegg::work_plan_evidence::{assemble, assemble_resolved};
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
    HostEvidenceStatus, WorkEvidenceKind, WorkEvidenceRef, WorkItem, WorkItemId, WorkItemStatus,
    WorkPlanId,
};
use codegg_core::workspace::WorkspaceId;
use sqlx::SqlitePool;

fn job_spec(kind: JobKind, payload: JobPayload) -> NewJob {
    NewJob {
        workspace_id: WorkspaceId::new_unchecked("ws-evidence"),
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

fn clean_revision(oid: &str) -> ExecutionSubjectRevision {
    ExecutionSubjectRevision {
        schema_version: ExecutionSubjectRevision::SCHEMA_VERSION,
        subject_kind: ExecutionSubjectKind::Git,
        repository_identity: "codegg-workspace:ws-evidence".to_string(),
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

fn item_with(kind: WorkEvidenceKind, ref_id: &str) -> WorkItem {
    let now = chrono::Utc::now();
    WorkItem {
        id: WorkItemId::generate(),
        plan_id: WorkPlanId::generate(),
        revision: 1,
        position: 0,
        parent_item_id: None,
        dependencies: vec![],
        status: WorkItemStatus::Actionable,
        description: "evidence item".to_string(),
        acceptance: vec![],
        evidence: vec![WorkEvidenceRef {
            kind,
            ref_id: ref_id.to_string(),
            detail: None,
        }],
        owner_run_id: None,
        owner_job_id: None,
        attempts: 0,
        blocker: None,
        next_action: None,
        created_at: now,
        updated_at: now,
    }
}

/// Create a job, run one attempt through S1/seal/finish, return ids.
async fn sealed_job(
    pool: &SqlitePool,
    oid_s1: &str,
    seal: Option<ExecutionSubjectProvenance>,
    finish: AttemptState,
) -> (String, String) {
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
        .set_attempt_source_subject_started(&attempt.attempt_id, &started(oid_s1))
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

async fn create_linked_agent_run(
    pool: &SqlitePool,
    completed: bool,
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
                workspace_id: WorkspaceId::new_unchecked("ws-evidence"),
                requested_agent: "agent".to_string(),
                delegation_key: format!("delegation-{}", uuid::Uuid::new_v4().simple()),
                request_fingerprint: "fingerprint".to_string(),
                description: "linked run".to_string(),
            },
            NewAgentRun {
                run_id: codegg_core::identity::AgentRunId::new(),
                parent_run_id: None,
                depth: 1,
                workspace_id: WorkspaceId::new_unchecked("ws-evidence"),
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
    if completed {
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
    }
    run_id.to_string()
}

// ── D001: AgentRun link predicate ──────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn linked_agent_run_resolves_passed_plus_exact_subject() {
    let pool = common::pool::isolated_pool().await;
    let oid = "f".repeat(40);
    let (job_id, attempt_id) =
        sealed_job(&pool, &oid, Some(stable(&oid)), AttemptState::Completed).await;
    let agent_run_id = create_linked_agent_run(
        &pool,
        true,
        Some(JobId::new_unchecked(job_id.clone())),
        Some(codegg_core::jobs::AttemptId::new_unchecked(
            attempt_id.clone(),
        )),
    )
    .await;

    // Legacy status-only path resolves through the agent_run table.
    let items = vec![item_with(WorkEvidenceKind::AgentRun, &agent_run_id)];
    let snapshot = assemble(&pool, &items).await.unwrap();
    assert_eq!(
        snapshot.lookup(WorkEvidenceKind::AgentRun, &agent_run_id),
        HostEvidenceStatus::Passed
    );

    // Enriched path resolves status plus the exact attempt subject.
    let resolved = assemble_resolved(&pool, &items).await.unwrap();
    assert_eq!(resolved.entries.len(), 1);
    let evidence = &resolved.entries[0];
    assert_eq!(evidence.status, HostEvidenceStatus::Passed);
    assert_eq!(
        evidence.source_subject_disposition,
        ExecutionSubjectDisposition::Stable
    );
    let subject = evidence
        .source_subject
        .as_ref()
        .expect("linked run must resolve its attempt subject");
    assert_eq!(subject.captured.as_ref(), Some(&clean_revision(&oid)));
    assert_eq!(subject.sealed.as_ref(), Some(&clean_revision(&oid)));
    assert_eq!(evidence.native_job_id.as_deref(), Some(job_id.as_str()));
    assert_eq!(
        evidence.native_attempt_id.as_deref(),
        Some(attempt_id.as_str())
    );
}

#[tokio::test(flavor = "current_thread")]
async fn agent_run_with_dangling_link_is_subject_unavailable() {
    let pool = common::pool::isolated_pool().await;
    let dangling = create_linked_agent_run(
        &pool,
        true,
        Some(JobId::new_unchecked("job-dangling")),
        Some(codegg_core::jobs::AttemptId::new_unchecked(
            "attempt-dangling",
        )),
    )
    .await;
    let items = vec![item_with(WorkEvidenceKind::AgentRun, &dangling)];
    let resolved = assemble_resolved(&pool, &items).await.unwrap();
    // Status still resolves from the durable link row; the subject does
    // not exist, so it stays unavailable without touching the worktree.
    assert_eq!(resolved.entries[0].status, HostEvidenceStatus::Passed);
    assert_eq!(
        resolved.entries[0].source_subject_disposition,
        ExecutionSubjectDisposition::Unavailable
    );
    assert_eq!(resolved.entries[0].source_subject, None);
}

#[tokio::test(flavor = "current_thread")]
async fn completed_job_with_stable_subject_returns_passed_plus_subject() {
    let pool = common::pool::isolated_pool().await;
    let oid = "a".repeat(40);
    let (job_id, _) = sealed_job(&pool, &oid, Some(stable(&oid)), AttemptState::Completed).await;
    let items = vec![item_with(WorkEvidenceKind::TestJob, &job_id)];
    let snapshot = assemble(&pool, &items).await.unwrap();
    assert_eq!(
        snapshot.lookup(WorkEvidenceKind::TestJob, &job_id),
        HostEvidenceStatus::Passed
    );
    let resolved = assemble_resolved(&pool, &items).await.unwrap();
    assert_eq!(resolved.entries[0].status, HostEvidenceStatus::Passed);
    assert_eq!(
        resolved.entries[0].source_subject_disposition,
        ExecutionSubjectDisposition::Stable
    );
    assert!(resolved.entries[0].source_subject.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_completed_job_returns_passed_with_unavailable_subject() {
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
        .finish_attempt(AttemptCompletion {
            attempt_id: attempt.attempt_id,
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();
    let job_id = job.job_id.to_string();
    let items = vec![item_with(WorkEvidenceKind::TestJob, &job_id)];
    let resolved = assemble_resolved(&pool, &items).await.unwrap();
    assert_eq!(resolved.entries[0].status, HostEvidenceStatus::Passed);
    assert_eq!(resolved.entries[0].source_subject, None);
    assert_eq!(
        resolved.entries[0].source_subject_disposition,
        ExecutionSubjectDisposition::Unavailable
    );
}

#[tokio::test(flavor = "current_thread")]
async fn running_job_returns_in_progress_without_stable_subject() {
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
    let oid = "c".repeat(40);
    store
        .set_attempt_source_subject_started(&attempt.attempt_id, &started(&oid))
        .await
        .unwrap();
    let job_id = job.job_id.to_string();
    let items = vec![item_with(WorkEvidenceKind::TestJob, &job_id)];
    let resolved = assemble_resolved(&pool, &items).await.unwrap();
    assert_eq!(resolved.entries[0].status, HostEvidenceStatus::InProgress);
    assert_ne!(
        resolved.entries[0].source_subject_disposition,
        ExecutionSubjectDisposition::Stable
    );
}

#[tokio::test(flavor = "current_thread")]
async fn drifted_completed_job_returns_terminal_status_with_drift() {
    let pool = common::pool::isolated_pool().await;
    let (job_id, _) = sealed_job(
        &pool,
        &"d".repeat(40),
        Some(drifted(&"d".repeat(40), &"e".repeat(40))),
        AttemptState::Completed,
    )
    .await;
    let items = vec![item_with(WorkEvidenceKind::TestJob, &job_id)];
    let resolved = assemble_resolved(&pool, &items).await.unwrap();
    assert_eq!(resolved.entries[0].status, HostEvidenceStatus::Passed);
    assert_eq!(
        resolved.entries[0].source_subject_disposition,
        ExecutionSubjectDisposition::Drifted
    );
}

#[tokio::test(flavor = "current_thread")]
async fn missing_ref_remains_unavailable() {
    let pool = common::pool::isolated_pool().await;
    let items = vec![item_with(WorkEvidenceKind::TestJob, "job-missing")];
    let resolved = assemble_resolved(&pool, &items).await.unwrap();
    assert_eq!(resolved.entries[0].status, HostEvidenceStatus::Unavailable);
    assert_eq!(resolved.entries[0].source_subject, None);
}

// ── D002: Eggplan bridge golden fixtures ───────────────────────────────────

#[test]
fn eggplan_bridge_golden_clean_subject() {
    let revision = clean_revision(&"a".repeat(40));
    assert!(revision.validate());
    let json = serde_json::to_value(revision.to_eggplan_fields()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "subject_kind": "git",
            "repository_id": "codegg-workspace:ws-evidence",
            "revision": "a".repeat(40),
            "state": "clean",
            "dirty_digest": null,
        })
    );
}

#[test]
fn eggplan_bridge_golden_dirty_subject() {
    let revision = ExecutionSubjectRevision {
        schema_version: ExecutionSubjectRevision::SCHEMA_VERSION,
        subject_kind: ExecutionSubjectKind::Git,
        repository_identity: "codegg-workspace:ws-evidence".to_string(),
        revision: "b".repeat(40),
        state: ExecutionSubjectState::Dirty,
        dirty_digest: Some("c".repeat(64)),
    };
    assert!(revision.validate());
    let json = serde_json::to_value(revision.to_eggplan_fields()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "subject_kind": "git",
            "repository_id": "codegg-workspace:ws-evidence",
            "revision": "b".repeat(40),
            "state": "dirty",
            "dirty_digest": "c".repeat(64),
        })
    );
}
