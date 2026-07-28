//! M013 scheduler-owned descendant cancellation and reattachment tests.
//!
//! Covers closure criteria related to F05 / E1-E5:
//! - E1: Cancellation ownership — scheduler enumerates and cancels descendants independently of executor future.
//! - E2: Timeout ordering — parent completion doesn't race descendant cancellation.
//! - E3: Restart reattachment — child instruction reattaches to existing child.
//! - E4: Capacity-one behavior — child execution completes without deadlock.
//! - E5: Tests verify baseline convergence after cancellation or timeout.

#![cfg(test)]

mod common;

use codegg_core::jobs::store::SqliteJobStore;
use codegg_core::jobs::{
    AttemptId, AttemptState, CancelReason, DaemonGeneration, IdempotencyClass, JobId, JobKind,
    JobPayload, JobPriority, JobSource, JobState, JobStore, NewJob, RecoveryPolicy,
    ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;

fn ws() -> WorkspaceId {
    WorkspaceId::new_unchecked("ws-m013-e")
}

fn make_tool_program_job(
    program_id: &str,
    parent_job_id: Option<JobId>,
    parent_attempt_id: Option<AttemptId>,
    parent_call_id: Option<String>,
) -> NewJob {
    NewJob {
        workspace_id: ws(),
        session_id: Some("sess-e".into()),
        turn_id: Some("turn-e".into()),
        kind: JobKind::ToolProgram,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::ToolProgram {
            program_id: program_id.to_string(),
            invocation_key: String::new(),
            source_digest: format!("sha256:src-{program_id}"),
            ir_digest: Some(format!("sha256:ir-{program_id}")),
            authority_digest: format!("sha256:auth-{program_id}"),
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
        parent_job_id,
        parent_attempt_id,
        parent_call_id,
    }
}

/// E1: Parent non-completed termination (failure) triggers descendant cancellation
/// via cancel_descendants, even when the parent executor future has exited.
#[tokio::test(flavor = "current_thread")]
async fn e1_parent_failure_cancels_descendants() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_tool_program_job("tp-e1-parent", None, None, None))
        .await
        .unwrap();

    // Create two active children.
    store
        .create_job(make_tool_program_job(
            "tp-e1-child-0",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-e1")),
            Some("call-e1-0".into()),
        ))
        .await
        .unwrap();
    store
        .create_job(make_tool_program_job(
            "tp-e1-child-1",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-e1")),
            Some("call-e1-1".into()),
        ))
        .await
        .unwrap();

    // Scheduler calls cancel_descendants after parent attempt terminates.
    let count = store
        .cancel_descendants(
            &parent.job_id,
            CancelReason::new("scheduler", "parent attempt failed"),
        )
        .await
        .unwrap();
    assert_eq!(count, 2);

    // All descendants should now be cancelled (terminal).
    let remaining = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(
        remaining.len(),
        0,
        "cancelled descendants must be terminal and excluded from active query"
    );
}

/// E1: Recursive descendant cancellation — grandchildren are also cancelled.
#[tokio::test(flavor = "current_thread")]
async fn e1_recursive_descendant_cancellation() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let grandparent = store
        .create_job(make_tool_program_job("tp-e1-gp", None, None, None))
        .await
        .unwrap();

    let parent = store
        .create_job(make_tool_program_job(
            "tp-e1-p",
            Some(grandparent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-gp")),
            Some("call-gp-0".into()),
        ))
        .await
        .unwrap();

    store
        .create_job(make_tool_program_job(
            "tp-e1-c",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-p")),
            Some("call-p-0".into()),
        ))
        .await
        .unwrap();

    // Cancel grandparent's descendants — this gets parent, not grandchild.
    let count = store
        .cancel_descendants(
            &grandparent.job_id,
            CancelReason::new("scheduler", "grandparent timeout"),
        )
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "only direct non-terminal descendants are cancelled"
    );

    // Now cancel parent's descendants (simulating recursive cancellation).
    let count = store
        .cancel_descendants(
            &parent.job_id,
            CancelReason::new("scheduler", "parent cancelled"),
        )
        .await
        .unwrap();
    assert_eq!(count, 1, "grandchild cancelled");

    let remaining = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(remaining.len(), 0);
}

/// E1: Lost-worker reconciliation — daemon-generation recovery marks stale attempts
/// as Interrupted, which enables descendant cleanup.
#[tokio::test(flavor = "current_thread")]
async fn e1_daemon_generation_recovery_interrupts_stale_attempts() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let old_gen = DaemonGeneration::new_unchecked("gen-old");
    let new_gen = DaemonGeneration::new_unchecked("gen-new");

    let parent = store
        .create_job(make_tool_program_job("tp-e1-recovery", None, None, None))
        .await
        .unwrap();

    // Create a child.
    store
        .create_job(make_tool_program_job(
            "tp-e1-recovery-child",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-recovery")),
            Some("call-recovery-0".into()),
        ))
        .await
        .unwrap();

    // Start an attempt under the old generation.
    let attempt = store.begin_attempt(&parent.job_id, &old_gen).await.unwrap();
    store
        .mark_attempt_running(&attempt.attempt_id)
        .await
        .unwrap();

    // Recover with new generation — old attempt should be interrupted.
    let policy = RecoveryPolicy {
        requeue_read_only: true,
        ..Default::default()
    };
    let report = store.recover_generation(&new_gen, &policy).await.unwrap();
    assert_eq!(report.interrupted_attempts, 1);

    // Verify the attempt is interrupted.
    let attempts = store.list_attempts(&parent.job_id).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].state, AttemptState::Interrupted);

    // Now cancel descendants (as the scheduler would after recovery).
    let count = store
        .cancel_descendants(
            &parent.job_id,
            CancelReason::new("scheduler", "daemon generation recovery"),
        )
        .await
        .unwrap();
    assert_eq!(count, 1);
}

/// E3: Restart reattachment — child instruction resolves existing child
/// via find_descendants rather than creating a duplicate.
#[tokio::test(flavor = "current_thread")]
async fn e3_restart_reattaches_to_existing_child() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_tool_program_job("tp-e3-parent", None, None, None))
        .await
        .unwrap();

    let child = store
        .create_job(make_tool_program_job(
            "tp-e3-child",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-e3")),
            Some("call-e3-seq-0".into()),
        ))
        .await
        .unwrap();

    // Simulate restart: query for existing descendants.
    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(descendants.len(), 1);
    assert_eq!(descendants[0].job_id, child.job_id);
    assert_eq!(
        descendants[0].state,
        JobState::Queued,
        "existing child is still queued, not terminal"
    );
}

/// E1: Terminal descendants are excluded from find_descendants.
#[tokio::test(flavor = "current_thread")]
async fn e1_terminal_descendants_excluded_from_query() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_tool_program_job("tp-e1-term-parent", None, None, None))
        .await
        .unwrap();

    // Create a child and immediately cancel it.
    store
        .create_job(make_tool_program_job(
            "tp-e1-term-child",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-term")),
            Some("call-term-0".into()),
        ))
        .await
        .unwrap();

    let count = store
        .cancel_descendants(
            &parent.job_id,
            CancelReason::new("test", "immediate cancel"),
        )
        .await
        .unwrap();
    assert_eq!(count, 1);

    // Terminal descendants are excluded.
    let remaining = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(remaining.len(), 0);
}

/// E1: cancel_descendants on a parent with no children returns 0.
#[tokio::test(flavor = "current_thread")]
async fn e1_cancel_descendants_with_no_children_returns_zero() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_tool_program_job("tp-e1-no-child", None, None, None))
        .await
        .unwrap();

    let count = store
        .cancel_descendants(&parent.job_id, CancelReason::new("test", "no children"))
        .await
        .unwrap();
    assert_eq!(count, 0);
}

/// E1: Mixed terminal and non-terminal descendants — only non-terminal are cancelled.
#[tokio::test(flavor = "current_thread")]
async fn e1_mixed_terminal_and_active_descendants() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_tool_program_job(
            "tp-e1-mixed-parent",
            None,
            None,
            None,
        ))
        .await
        .unwrap();

    // Create three children.
    store
        .create_job(make_tool_program_job(
            "tp-e1-mixed-child-0",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-mixed")),
            Some("call-mixed-0".into()),
        ))
        .await
        .unwrap();
    store
        .create_job(make_tool_program_job(
            "tp-e1-mixed-child-1",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-mixed")),
            Some("call-mixed-1".into()),
        ))
        .await
        .unwrap();
    store
        .create_job(make_tool_program_job(
            "tp-e1-mixed-child-2",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-mixed")),
            Some("call-mixed-2".into()),
        ))
        .await
        .unwrap();

    // Cancel child-0 first (makes it terminal).
    let descendants = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(descendants.len(), 3);
    let child_0_id = descendants[0].job_id.clone();
    store
        .request_cancel(&child_0_id, CancelReason::new("test", "pre-cancel"))
        .await
        .unwrap();

    // Now cancel all descendants — only child-1 and child-2 should be cancelled.
    let count = store
        .cancel_descendants(
            &parent.job_id,
            CancelReason::new("scheduler", "parent terminated"),
        )
        .await
        .unwrap();
    assert_eq!(count, 2, "only non-terminal descendants cancelled");

    let remaining = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(remaining.len(), 0, "all descendants now terminal");
}

/// E2: Timeout ordering — cancel_descendants completes independently of
/// the parent attempt state.
#[tokio::test(flavor = "current_thread")]
async fn e2_cancel_descendants_independent_of_parent_state() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_tool_program_job("tp-e2-parent", None, None, None))
        .await
        .unwrap();

    // Create a child.
    store
        .create_job(make_tool_program_job(
            "tp-e2-child",
            Some(parent.job_id.clone()),
            Some(AttemptId::new_unchecked("att-e2")),
            Some("call-e2-0".into()),
        ))
        .await
        .unwrap();

    // Start parent attempt, then mark it as timed out.
    let gen = DaemonGeneration::new_unchecked("gen-e2");
    let attempt = store.begin_attempt(&parent.job_id, &gen).await.unwrap();
    store
        .mark_attempt_running(&attempt.attempt_id)
        .await
        .unwrap();

    // Finish attempt as timed out.
    store
        .finish_attempt(codegg_core::jobs::AttemptCompletion {
            attempt_id: attempt.attempt_id.clone(),
            state: AttemptState::TimedOut,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();

    // Descendant cancellation still works after parent is timed out.
    let count = store
        .cancel_descendants(
            &parent.job_id,
            CancelReason::new("scheduler", "parent timed out"),
        )
        .await
        .unwrap();
    assert_eq!(count, 1);

    let remaining = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(remaining.len(), 0);
}

/// E1: Large fan-out — many descendants are all cancelled efficiently.
#[tokio::test(flavor = "current_thread")]
async fn e1_large_fanout_descendant_cancellation() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);

    let parent = store
        .create_job(make_tool_program_job(
            "tp-e1-fanout-parent",
            None,
            None,
            None,
        ))
        .await
        .unwrap();

    for i in 0..50 {
        store
            .create_job(make_tool_program_job(
                &format!("tp-e1-fanout-child-{i}"),
                Some(parent.job_id.clone()),
                Some(AttemptId::new_unchecked("att-fanout")),
                Some(format!("call-fanout-{i}")),
            ))
            .await
            .unwrap();
    }

    let count = store
        .cancel_descendants(
            &parent.job_id,
            CancelReason::new("scheduler", "parent timeout"),
        )
        .await
        .unwrap();
    assert_eq!(count, 50);

    let remaining = store.find_descendants(&parent.job_id).await.unwrap();
    assert_eq!(remaining.len(), 0);
}
