//! M015 descendant traversal through terminal intermediates and malformed cycles.

mod common;

use codegg_core::jobs::{
    CancelReason, IdempotencyClass, JobId, JobKind, JobPriority, JobSource, JobState, JobStore,
    NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;
use std::time::Duration;

fn job(parent: Option<JobId>) -> NewJob {
    NewJob {
        workspace_id: WorkspaceId::new_unchecked("ws-m015-descendants"),
        session_id: Some("session-m015".into()),
        turn_id: Some("turn-m015".into()),
        kind: JobKind::ToolProgram,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: codegg_core::jobs::JobPayload::ToolProgram {
            program_id: uuid::Uuid::new_v4().to_string(),
            invocation_key: uuid::Uuid::new_v4().to_string(),
            source_digest: "sha256:source".into(),
            ir_digest: Some("sha256:ir".into()),
            authority_digest: "sha256:authority".into(),
            execution_context_json: None,
            submission_key: uuid::Uuid::new_v4().to_string(),
            execution_mode: "background".into(),
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
        parent_job_id: parent,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn active_grandchild_beneath_terminal_child_is_discovered_and_cancelled() {
    let store = codegg_core::jobs::InMemoryJobStore::new();
    let parent = store.create_job(job(None)).await.unwrap();
    let child = store
        .create_job(job(Some(parent.job_id.clone())))
        .await
        .unwrap();
    let grandchild = store
        .create_job(job(Some(child.job_id.clone())))
        .await
        .unwrap();
    store
        .request_cancel(
            &child.job_id,
            CancelReason::new("test", "terminal-intermediate"),
        )
        .await
        .unwrap();
    assert_eq!(
        store.get_job(&child.job_id).await.unwrap().unwrap().state,
        JobState::Cancelled
    );

    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(
        descendants
            .iter()
            .map(|item| &item.job_id)
            .collect::<Vec<_>>(),
        vec![&grandchild.job_id]
    );
    assert_eq!(
        store
            .cancel_descendants(&parent.job_id, CancelReason::new("test", "converge"))
            .await
            .unwrap(),
        1
    );
    assert!(store
        .find_descendants(&parent.job_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn deeper_mixed_lineage_is_deterministic_and_converges() {
    let store = codegg_core::jobs::InMemoryJobStore::new();
    let root = store.create_job(job(None)).await.unwrap();
    let mut parent = root.job_id.clone();
    let mut active = Vec::new();
    for depth in 0..8 {
        let current = store.create_job(job(Some(parent))).await.unwrap();
        if depth % 2 == 0 {
            store
                .request_cancel(
                    &current.job_id,
                    CancelReason::new("test", "terminal-intermediate"),
                )
                .await
                .unwrap();
        } else {
            active.push(current.job_id.clone());
        }
        parent = current.job_id;
    }

    let first = store.find_descendants(&root.job_id).await.unwrap();
    let second = store.find_descendants(&root.job_id).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first
            .iter()
            .map(|item| item.job_id.clone())
            .collect::<Vec<_>>(),
        active
    );
    assert_eq!(
        store
            .cancel_descendants(&root.job_id, CancelReason::new("test", "converge"))
            .await
            .unwrap(),
        4
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_sqlite_cycle_is_bounded() {
    let pool = common::pool::isolated_pool().await;
    let store = codegg_core::jobs::SqliteJobStore::new(pool.clone());
    let first = store.create_job(job(None)).await.unwrap();
    let second = store
        .create_job(job(Some(first.job_id.clone())))
        .await
        .unwrap();
    sqlx::query("UPDATE job SET parent_job_id = ? WHERE id = ?")
        .bind(second.job_id.as_str())
        .bind(first.job_id.as_str())
        .execute(&pool)
        .await
        .unwrap();

    let descendants = tokio::time::timeout(
        Duration::from_secs(1),
        store.find_descendants(&first.job_id),
    )
    .await
    .expect("cycle traversal must be bounded")
    .unwrap();
    assert_eq!(descendants.len(), 1);
    assert_eq!(descendants[0].job_id, second.job_id);
}
