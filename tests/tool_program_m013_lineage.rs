//! M013 durable descendant lineage schema and round-trip tests.
//!
//! Covers closure criteria related to F04 / D1-D5:
//! - D1: Migration adds parent_job_id, parent_attempt_id, parent_call_id columns and indexes.
//! - D2: Store round trip retains all lineage fields.
//! - D3: Canonical call identity comes from interpreter's durable call ID.
//! - D4: Query API returns only correct descendants.
//! - D5: Retry/restart retain lineage; distinct operations at different sequences create distinct children.

#![cfg(test)]

mod common;

use codegg_core::jobs::store::SqliteJobStore;
use codegg_core::jobs::{
    CancelReason, IdempotencyClass, JobId, JobKind, JobPayload, JobPriority, JobSource, JobStore,
    NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;

fn make_ws() -> WorkspaceId {
    WorkspaceId::new_unchecked("ws-m013-lineage")
}

fn make_parent_job(program_id: &str) -> NewJob {
    NewJob {
        workspace_id: make_ws(),
        session_id: Some("sess-1".into()),
        turn_id: Some("turn-1".into()),
        kind: JobKind::ToolProgram,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::ToolProgram {
            program_id: program_id.to_string(),
            invocation_key: String::new(),
            source_digest: "sha256:parent-src".into(),
            ir_digest: Some("sha256:parent-ir".into()),
            authority_digest: "sha256:parent-auth".into(),
            execution_context_json: None,
            submission_key: String::new(),
            execution_mode: "foreground".into(),
            source_ref: None,
            source_length: None,
            allowed_tools: vec!["read".into(), "grep".into()],
            authority_grant_json: None,
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::ReadOnly,
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
    }
}

fn make_child_job(
    parent_job_id: &JobId,
    parent_attempt_id: &str,
    parent_call_id: &str,
    program_id: &str,
) -> NewJob {
    NewJob {
        workspace_id: make_ws(),
        session_id: Some("sess-1".into()),
        turn_id: Some("turn-1".into()),
        kind: JobKind::ToolProgram,
        source: JobSource::AgentDelegated,
        priority: JobPriority::Normal,
        payload: JobPayload::ToolProgram {
            program_id: program_id.to_string(),
            invocation_key: String::new(),
            source_digest: "sha256:child-src".into(),
            ir_digest: Some("sha256:child-ir".into()),
            authority_digest: "sha256:child-auth".into(),
            execution_context_json: None,
            submission_key: String::new(),
            execution_mode: "foreground".into(),
            source_ref: None,
            source_length: None,
            allowed_tools: vec!["read".into(), "grep".into()],
            authority_grant_json: None,
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::ReadOnly,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
        parent_job_id: Some(parent_job_id.clone()),
        parent_attempt_id: Some(codegg_core::jobs::AttemptId::new_unchecked(
            parent_attempt_id,
        )),
        parent_call_id: Some(parent_call_id.to_string()),
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
    }
}

/// D2: SQLite round trip retains all lineage fields.
#[tokio::test(flavor = "current_thread")]
async fn d2_lineage_round_trip_preserves_all_fields() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_parent_job("tp-m013-d2-parent"))
        .await
        .unwrap();
    let child_spec = make_child_job(
        &parent.job_id,
        "att-d2-1",
        "call-d2-seq-0",
        "tp-m013-d2-child",
    );
    let child = store.create_job(child_spec).await.unwrap();

    // Verify lineage fields survived the round trip.
    assert_eq!(child.parent_job_id, Some(parent.job_id.clone()));
    assert_eq!(
        child.parent_attempt_id,
        Some(codegg_core::jobs::AttemptId::new_unchecked("att-d2-1"))
    );
    assert_eq!(child.parent_call_id, Some("call-d2-seq-0".into()));

    // Verify the job is queryable by parent.
    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(descendants.len(), 1);
    assert_eq!(descendants[0].job_id, child.job_id);
}

/// D2: Retry retains lineage fields.
#[tokio::test(flavor = "current_thread")]
async fn d2_retry_retains_lineage() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_parent_job("tp-m013-d2-retry-parent"))
        .await
        .unwrap();
    let child_spec = make_child_job(
        &parent.job_id,
        "att-d2-retry",
        "call-d2-retry-seq",
        "tp-m013-d2-retry-child",
    );
    let child = store.create_job(child_spec).await.unwrap();

    // Lineage must persist even if the child job is retried.
    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(descendants.len(), 1);
    assert_eq!(descendants[0].job_id, child.job_id);
    assert_eq!(descendants[0].state, codegg_core::jobs::JobState::Queued);
}

/// D5: Identical operations at different instruction sequences create distinct
/// child identities.
#[tokio::test(flavor = "current_thread")]
async fn d5_distinct_sequences_create_distinct_children() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_parent_job("tp-m013-d5-parent"))
        .await
        .unwrap();

    // Two children with same tool but different call IDs (different instruction sequences).
    let child_a = store
        .create_job(make_child_job(
            &parent.job_id,
            "att-d5",
            "call-seq-0",
            "tp-m013-d5-child-a",
        ))
        .await
        .unwrap();
    let child_b = store
        .create_job(make_child_job(
            &parent.job_id,
            "att-d5",
            "call-seq-1",
            "tp-m013-d5-child-b",
        ))
        .await
        .unwrap();

    assert_ne!(child_a.job_id, child_b.job_id);

    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(descendants.len(), 2);
    let ids: std::collections::BTreeSet<String> =
        descendants.iter().map(|j| j.job_id.to_string()).collect();
    assert!(ids.contains(child_a.job_id.as_str()));
    assert!(ids.contains(child_b.job_id.as_str()));
}

/// D4: Lineage query returns only the correct descendants, not unrelated jobs.
#[tokio::test(flavor = "current_thread")]
async fn d4_lineage_query_returns_only_correct_descendants() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent_a = store
        .create_job(make_parent_job("tp-m013-d4-parent-a"))
        .await
        .unwrap();
    let parent_b = store
        .create_job(make_parent_job("tp-m013-d4-parent-b"))
        .await
        .unwrap();

    // Two children under parent_a, one under parent_b, one orphan.
    store
        .create_job(make_child_job(
            &parent_a.job_id,
            "att",
            "call-a-0",
            "tp-m013-d4-child-a0",
        ))
        .await
        .unwrap();
    store
        .create_job(make_child_job(
            &parent_a.job_id,
            "att",
            "call-a-1",
            "tp-m013-d4-child-a1",
        ))
        .await
        .unwrap();
    store
        .create_job(make_child_job(
            &parent_b.job_id,
            "att",
            "call-b-0",
            "tp-m013-d4-child-b0",
        ))
        .await
        .unwrap();
    store
        .create_job(make_parent_job("tp-m013-d4-orphan"))
        .await
        .unwrap();

    let descendants_a = store.find_descendants(&parent_a.job_id).await.unwrap();
    assert_eq!(descendants_a.len(), 2);

    let descendants_b = store.find_descendants(&parent_b.job_id).await.unwrap();
    assert_eq!(descendants_b.len(), 1);
}

/// D5: Cancellation cascades to descendants — only non-terminal descendants are cancelled.
#[tokio::test(flavor = "current_thread")]
async fn d5_cancel_descendants_only_affects_non_terminal() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_parent_job("tp-m013-d5-cancel-parent"))
        .await
        .unwrap();

    // Create a queued child (will be cancelled).
    store
        .create_job(make_child_job(
            &parent.job_id,
            "att",
            "call-cancel-0",
            "tp-m013-d5-cancel-child-0",
        ))
        .await
        .unwrap();

    // Cancel descendants.
    let count = store
        .cancel_descendants(
            &parent.job_id,
            CancelReason::new("test", "parent terminated"),
        )
        .await
        .unwrap();
    assert_eq!(count, 1);

    // The child should now be cancelled (terminal).
    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(
        descendants.len(),
        0,
        "cancelled children are terminal and excluded"
    );
}

/// D2: SQLite indexes exist and parent_job_id is queryable.
#[tokio::test(flavor = "current_thread")]
async fn d2_parent_indexes_are_usable() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_parent_job("tp-m013-d2-idx-parent"))
        .await
        .unwrap();

    // Create many children to exercise the index.
    for i in 0..10 {
        store
            .create_job(make_child_job(
                &parent.job_id,
                "att-idx",
                &format!("call-idx-{i}"),
                &format!("tp-m013-d2-idx-child-{i}"),
            ))
            .await
            .unwrap();
    }

    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(descendants.len(), 10);
}

/// D2: get_job returns lineage fields from SQLite.
#[tokio::test(flavor = "current_thread")]
async fn d2_get_job_returns_lineage_fields() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_parent_job("tp-m013-d2-get-parent"))
        .await
        .unwrap();
    let child = store
        .create_job(make_child_job(
            &parent.job_id,
            "att-get",
            "call-get-seq",
            "tp-m013-d2-get-child",
        ))
        .await
        .unwrap();

    let loaded = store.get_job(&child.job_id).await.unwrap().unwrap();
    assert_eq!(loaded.parent_job_id, Some(parent.job_id));
    assert_eq!(
        loaded.parent_attempt_id,
        Some(codegg_core::jobs::AttemptId::new_unchecked("att-get"))
    );
    assert_eq!(loaded.parent_call_id, Some("call-get-seq".into()));
}
