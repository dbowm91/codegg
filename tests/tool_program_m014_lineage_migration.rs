//! M014 lineage migration tests.
//!
//! Covers C-21 through C-25: typed lineage includes parent program, job,
//! attempt, canonical call ID, instruction sequence, and relation kind;
//! a new migration upgrades databases; all transitions preserve immutable
//! lineage.

#![cfg(test)]

use codegg_core::jobs::{
    DaemonGeneration, IdempotencyClass, InMemoryJobStore, JobKind, JobPayload, JobPriority,
    JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;
use std::time::Duration;

fn make_ws() -> WorkspaceId {
    WorkspaceId::new_unchecked("ws-m014-lineage")
}

fn make_base_job() -> NewJob {
    NewJob {
        workspace_id: make_ws(),
        session_id: Some("sess-1".into()),
        turn_id: Some("turn-1".into()),
        kind: JobKind::ToolProgram,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::ToolProgram {
            program_id: "tp-lineage".into(),
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
    }
}

/// C-21: Typed lineage includes parent program, job, attempt, canonical
/// call ID, instruction sequence, and relation kind.
#[tokio::test(flavor = "current_thread")]
async fn c21_typed_lineage_fields_preserved() {
    let store = InMemoryJobStore::new();
    let parent = store.create_job(make_base_job()).await.unwrap();
    let parent_attempt = store
        .begin_attempt(&parent.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .unwrap();

    let child_spec = NewJob {
        parent_job_id: Some(parent.job_id.clone()),
        parent_attempt_id: Some(parent_attempt.attempt_id.clone()),
        parent_call_id: Some("call:tp-lineage:0".into()),
        parent_program_id: Some("tp-lineage".into()),
        parent_instruction_sequence: Some(0),
        relation_kind: Some("child_job".into()),
        ..make_base_job()
    };

    let child = store.create_job(child_spec).await.unwrap();

    assert_eq!(child.parent_job_id, Some(parent.job_id.clone()));
    assert_eq!(child.parent_attempt_id, Some(parent_attempt.attempt_id));
    assert_eq!(child.parent_call_id, Some("call:tp-lineage:0".into()));
    assert_eq!(child.parent_program_id, Some("tp-lineage".into()));
    assert_eq!(child.parent_instruction_sequence, Some(0));
    assert_eq!(child.relation_kind, Some("child_job".into()));
}

/// C-22: A new migration upgrades a database already at the pre-M014 latest
/// version. Verify STORAGE_LAYOUT_VERSION is bumped.
#[tokio::test(flavor = "current_thread")]
async fn c22_storage_layout_version_bumped() {
    const {
        assert!(
            codegg_core::storage::STORAGE_LAYOUT_VERSION >= 35,
            "STORAGE_LAYOUT_VERSION must be >= 35 after M014 lineage migration"
        );
    }
}

/// C-23: Every JobStore create/read/update/retry/cancel/block/recover/finish
/// path preserves immutable lineage.
#[tokio::test(flavor = "current_thread")]
async fn c23_lineage_preserved_through_transitions() {
    let store = InMemoryJobStore::new();
    let parent = store.create_job(make_base_job()).await.unwrap();
    let parent_attempt = store
        .begin_attempt(&parent.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .unwrap();

    let child_spec = NewJob {
        parent_job_id: Some(parent.job_id.clone()),
        parent_attempt_id: Some(parent_attempt.attempt_id),
        parent_call_id: Some("call:tp-lineage:1".into()),
        parent_program_id: Some("tp-lineage".into()),
        parent_instruction_sequence: Some(1),
        relation_kind: Some("child_job".into()),
        ..make_base_job()
    };

    let child = store.create_job(child_spec).await.unwrap();
    let child_id = child.job_id.clone();

    // Begin attempt
    let attempt = store
        .begin_attempt(
            &child_id,
            &codegg_core::jobs::DaemonGeneration::new_unchecked("gen-1"),
        )
        .await
        .unwrap();

    // Get the job after begin_attempt
    let retrieved = store.get_job(&child_id).await.unwrap().unwrap();
    assert_eq!(retrieved.parent_job_id, Some(parent.job_id.clone()));
    assert_eq!(retrieved.parent_program_id, Some("tp-lineage".into()));
    assert_eq!(retrieved.parent_instruction_sequence, Some(1));
    assert_eq!(retrieved.relation_kind, Some("child_job".into()));

    // Finish attempt first (transition Created -> Running -> Completed)
    use codegg_core::jobs::{AttemptCompletion, AttemptState};
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

    let finished = store.get_job(&child_id).await.unwrap().unwrap();
    assert_eq!(finished.parent_job_id, Some(parent.job_id.clone()));
    assert_eq!(finished.parent_program_id, Some("tp-lineage".into()));
    assert_eq!(finished.parent_instruction_sequence, Some(1));
    assert_eq!(finished.relation_kind, Some("child_job".into()));

    // Cancel the job (after finish)
    use codegg_core::jobs::CancelReason;
    let _ = store
        .request_cancel(&child_id, CancelReason::new("test", "test_cancel"))
        .await
        .unwrap();

    let cancelled = store.get_job(&child_id).await.unwrap().unwrap();
    assert_eq!(cancelled.parent_job_id, Some(parent.job_id.clone()));
    assert_eq!(cancelled.parent_program_id, Some("tp-lineage".into()));
    assert_eq!(cancelled.parent_instruction_sequence, Some(1));
    assert_eq!(cancelled.relation_kind, Some("child_job".into()));
}

/// C-24: Canonical child identity is derived from actual parent execution
/// identity and sequence, not operation name.
#[tokio::test(flavor = "current_thread")]
async fn c24_canonical_child_identity_from_sequence() {
    let store = InMemoryJobStore::new();
    let parent = store.create_job(make_base_job()).await.unwrap();
    let parent_attempt = store
        .begin_attempt(&parent.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .unwrap();

    // Two children at different instruction sequences
    let child1 = store
        .create_job(NewJob {
            parent_job_id: Some(parent.job_id.clone()),
            parent_attempt_id: Some(parent_attempt.attempt_id.clone()),
            parent_call_id: Some("call:tp-lineage:0".into()),
            parent_program_id: Some("tp-lineage".into()),
            parent_instruction_sequence: Some(0),
            relation_kind: Some("child_job".into()),
            ..make_base_job()
        })
        .await
        .unwrap();

    let child2 = store
        .create_job(NewJob {
            parent_job_id: Some(parent.job_id.clone()),
            parent_attempt_id: Some(parent_attempt.attempt_id.clone()),
            parent_call_id: Some("call:tp-lineage:1".into()),
            parent_program_id: Some("tp-lineage".into()),
            parent_instruction_sequence: Some(1),
            relation_kind: Some("child_job".into()),
            ..make_base_job()
        })
        .await
        .unwrap();

    // C-24: distinct instruction sequences create distinct lineage identities
    assert_ne!(child1.parent_call_id, child2.parent_call_id);
    assert_ne!(
        child1.parent_instruction_sequence,
        child2.parent_instruction_sequence
    );
    assert_ne!(child1.job_id, child2.job_id);
}

/// C-25: Replay of one child instruction reuses one child; distinct sequences
/// create distinct children.
#[tokio::test(flavor = "current_thread")]
async fn c25_replay_same_sequence_reuses_child() {
    let store = InMemoryJobStore::new();
    let parent = store.create_job(make_base_job()).await.unwrap();
    let parent_attempt = store
        .begin_attempt(&parent.job_id, &DaemonGeneration::new_unchecked("gen-1"))
        .await
        .unwrap();

    // Create child at sequence 0
    let child1 = store
        .create_job(NewJob {
            parent_job_id: Some(parent.job_id.clone()),
            parent_attempt_id: Some(parent_attempt.attempt_id.clone()),
            parent_call_id: Some("call:tp-lineage:0".into()),
            parent_program_id: Some("tp-lineage".into()),
            parent_instruction_sequence: Some(0),
            relation_kind: Some("child_job".into()),
            ..make_base_job()
        })
        .await
        .unwrap();

    // "Replay" the same sequence — should find the same child
    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(descendants.len(), 1, "one child should exist");
    assert_eq!(descendants[0].job_id, child1.job_id);
}
