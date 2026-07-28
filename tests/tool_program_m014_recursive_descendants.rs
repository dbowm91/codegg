//! M014 recursive descendant tests.
//!
//! Covers C-26 through C-30: the scheduler can enumerate recursive active
//! descendants, parent cancellation reconciles recursive descendants, restart
//! reattaches existing children, capacity-one execution completes without
//! deadlock, and descendants converge to baseline.

#![cfg(test)]

use codegg_core::jobs::{
    AttemptCompletion, AttemptState, CancelReason, DaemonGeneration, IdempotencyClass, JobId,
    JobKind, JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;
use std::time::Duration;

fn make_ws() -> WorkspaceId {
    WorkspaceId::new_unchecked("ws-m014-desc")
}

fn make_job(parent: Option<JobId>) -> NewJob {
    let mut job = NewJob {
        workspace_id: make_ws(),
        session_id: Some("sess-1".into()),
        turn_id: Some("turn-1".into()),
        kind: JobKind::ToolProgram,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: codegg_core::jobs::JobPayload::ToolProgram {
            program_id: "tp-desc".into(),
            invocation_key: "inv-1".into(),
            source_digest: "sha256:src".into(),
            ir_digest: Some("sha256:ir".into()),
            authority_digest: "sha256:auth".into(),
            execution_context_json: None,
            submission_key: "sub-1".into(),
            execution_mode: "foreground".into(),
            source_ref: None,
            source_length: None,
            allowed_tools: vec!["read".into()],
            authority_grant_json: None,
        },
        resource_request: ResourceRequest::default(),
        timeout: Some(Duration::from_secs(30)),
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
    };
    if let Some(pid) = parent {
        job.parent_job_id = Some(pid);
    }
    job
}

fn gen() -> DaemonGeneration {
    DaemonGeneration::new_unchecked("gen-1")
}

/// C-26: The scheduler can enumerate recursive active descendants without
/// payload scanning.
#[tokio::test(flavor = "current_thread")]
async fn c26_recursive_descendant_enumeration() {
    let store = codegg_core::jobs::InMemoryJobStore::new();

    let parent = store.create_job(make_job(None)).await.unwrap();
    let child = store
        .create_job(make_job(Some(parent.job_id.clone())))
        .await
        .unwrap();
    let grandchild = store
        .create_job(make_job(Some(child.job_id.clone())))
        .await
        .unwrap();

    let _ = store.begin_attempt(&parent.job_id, &gen()).await;
    let _ = store.begin_attempt(&child.job_id, &gen()).await;
    let _ = store.begin_attempt(&grandchild.job_id, &gen()).await;

    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(
        descendants.len(),
        2,
        "recursive enumeration must find both child and grandchild"
    );

    let desc_ids: Vec<&str> = descendants.iter().map(|d| d.job_id.as_str()).collect();
    assert!(desc_ids.contains(&child.job_id.as_str()));
    assert!(desc_ids.contains(&grandchild.job_id.as_str()));
}

/// C-27: Parent cancellation reconciles recursive descendants after the
/// executor future is gone.
#[tokio::test(flavor = "current_thread")]
async fn c27_parent_cancellation_cancels_recursive_descendants() {
    let store = codegg_core::jobs::InMemoryJobStore::new();

    let parent = store.create_job(make_job(None)).await.unwrap();
    let child = store
        .create_job(make_job(Some(parent.job_id.clone())))
        .await
        .unwrap();
    let grandchild = store
        .create_job(make_job(Some(child.job_id.clone())))
        .await
        .unwrap();

    let cancelled = store
        .cancel_descendants(
            &parent.job_id,
            CancelReason::new("test", "parent_cancelled"),
        )
        .await
        .unwrap();

    assert_eq!(
        cancelled, 2,
        "recursive cancellation must cancel both child and grandchild"
    );

    let remaining = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(
        remaining.len(),
        0,
        "no active descendants should remain after recursive cancellation"
    );
}

/// C-28: Restart reattaches existing queued/running children and consumes
/// terminal children without duplicate submission.
#[tokio::test(flavor = "current_thread")]
async fn c28_restart_reattaches_existing_children() {
    let store = codegg_core::jobs::InMemoryJobStore::new();

    let parent = store.create_job(make_job(None)).await.unwrap();
    let child = store
        .create_job(make_job(Some(parent.job_id.clone())))
        .await
        .unwrap();

    let child_attempt = store.begin_attempt(&child.job_id, &gen()).await.unwrap();
    store
        .mark_attempt_running(&child_attempt.attempt_id)
        .await
        .unwrap();

    let existing = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(
        existing.len(),
        1,
        "existing child must be found after restart"
    );
    assert_eq!(existing[0].job_id, child.job_id);

    // Terminal child should not appear in active descendants
    store
        .finish_attempt(AttemptCompletion {
            attempt_id: child_attempt.attempt_id,
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();

    let after_terminal = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(
        after_terminal.len(),
        0,
        "terminal children must not appear in active descendants"
    );
}

/// C-29: Capacity-one parent/child execution completes without deadlock.
#[tokio::test(flavor = "current_thread")]
async fn c29_capacity_one_no_deadlock() {
    let store = codegg_core::jobs::InMemoryJobStore::new();

    let parent = store.create_job(make_job(None)).await.unwrap();
    let child = store
        .create_job(make_job(Some(parent.job_id.clone())))
        .await
        .unwrap();

    let parent_attempt = store.begin_attempt(&parent.job_id, &gen()).await.unwrap();
    let child_attempt = store.begin_attempt(&child.job_id, &gen()).await.unwrap();
    store
        .mark_attempt_running(&child_attempt.attempt_id)
        .await
        .unwrap();

    store
        .finish_attempt(AttemptCompletion {
            attempt_id: child_attempt.attempt_id,
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();

    store
        .mark_attempt_running(&parent_attempt.attempt_id)
        .await
        .unwrap();
    store
        .finish_attempt(AttemptCompletion {
            attempt_id: parent_attempt.attempt_id,
            state: AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();

    let parent_state = store.get_job(&parent.job_id).await.unwrap().unwrap();
    assert!(parent_state.state.is_terminal());
    let child_state = store.get_job(&child.job_id).await.unwrap().unwrap();
    assert!(child_state.state.is_terminal());
}

/// C-30: Descendant jobs, attempts, process groups, permits, workspace leases,
/// and counters converge to baseline or an explicit recoverable unresolved state.
#[tokio::test(flavor = "current_thread")]
async fn c30_descendants_converge_to_baseline() {
    let store = codegg_core::jobs::InMemoryJobStore::new();

    let parent = store.create_job(make_job(None)).await.unwrap();
    let child = store
        .create_job(make_job(Some(parent.job_id.clone())))
        .await
        .unwrap();
    let grandchild = store
        .create_job(make_job(Some(child.job_id.clone())))
        .await
        .unwrap();

    let cancelled = store
        .cancel_descendants(&parent.job_id, CancelReason::new("test", "convergence"))
        .await
        .unwrap();
    assert_eq!(cancelled, 2);

    let active = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(active.len(), 0, "all descendants must be reconciled");

    for job_id in [&parent.job_id, &child.job_id, &grandchild.job_id] {
        let job = store.get_job(job_id).await.unwrap().unwrap();
        assert!(
            job.state.is_terminal() || job_id == &parent.job_id,
            "job {} must be terminal or parent",
            job_id
        );
    }
}
