//! C002 qualification: the durable checkout writer permit.
//!
//! These tests exercise the **store**, which is the permit's authority: the
//! state machine, the compare-and-swap, the fence, the quarantine transitions,
//! and the restart reconciliation.
//!
//! Every test runs against a real SQLite database. An in-memory fake would
//! prove nothing about the properties that matter here — the properties are
//! properties of the *transaction*, not of the data structure.

use std::str::FromStr;

use codegg_core::identity::{
    AgentRunGroupId, AgentRunId, PrincipalId, ProjectId, RepositoryId, WorkspaceId, WorktreeId,
};
use codegg_core::storage::immediate::ImmediateWrite;
use codegg_core::writer_permit::{
    AcquireWriterPermit, BeginReleaseWriterPermit, CompleteReleaseWriterPermit,
    OperatorReleaseWriterPermit, QuarantineWriterPermit, QuiescenceEvidence, WriterPermitError,
    WriterPermitOperation, WriterPermitQuarantineReason, WriterPermitScope, WriterPermitState,
    WriterPermitStore,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

/// An in-memory database, migrated once. `migrate` runs inside the helper, so
/// no test adds its own.
async fn isolated_pool() -> SqlitePool {
    let name = format!("writer_permit_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect in-memory sqlite");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    pool
}

/// A file-backed database, so a test can close it and open it again.
async fn file_pool(path: &std::path::Path) -> SqlitePool {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .expect("connect file sqlite");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    pool
}

fn scope(worktree_id: &WorktreeId, generation: u64) -> WriterPermitScope {
    WriterPermitScope {
        project_id: ProjectId::new(),
        repository_id: Some(RepositoryId::new()),
        workspace_id: WorkspaceId::new(),
        worktree_id: worktree_id.clone(),
        lease_generation: generation,
        group_id: None,
        principal_id: PrincipalId::new(),
        agent_run_id: Some(AgentRunId::new()),
        attempt_id: Some("attempt-1".to_owned()),
    }
}

fn acquire_request(scope: WriterPermitScope, key: &str) -> AcquireWriterPermit {
    AcquireWriterPermit {
        scope,
        operation: WriterPermitOperation::NativeFileWrite,
        idempotency_key: key.to_owned(),
        lease_expires_at_ms: None,
    }
}

// ---------------------------------------------------------------------------
// Contention
// ---------------------------------------------------------------------------

/// N=2: two acquirers, one holder.
#[tokio::test]
async fn two_acquirers_produce_exactly_one_holder() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();

    let first = store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("first acquire");
    assert_eq!(first.state, WriterPermitState::Held);

    let second = store
        .acquire(acquire_request(scope(&worktree, 1), "key-2"))
        .await;
    assert!(matches!(second, Err(WriterPermitError::Held)));
}

/// N=16: sixteen concurrent acquirers, one holder, no lost update.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sixteen_concurrent_acquirers_produce_exactly_one_holder() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool.clone());
    let worktree = WorktreeId::new();

    let mut tasks = Vec::new();
    for index in 0..16 {
        let store = store.clone();
        let worktree = worktree.clone();
        tasks.push(tokio::spawn(async move {
            store
                .acquire(acquire_request(
                    scope(&worktree, 1),
                    &format!("key-{index}"),
                ))
                .await
        }));
    }

    let mut held = 0usize;
    let mut refused = 0usize;
    for task in tasks {
        match task.await.expect("task joins") {
            Ok(record) => {
                assert_eq!(record.state, WriterPermitState::Held);
                held += 1;
            }
            Err(WriterPermitError::Held) => refused += 1,
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
    assert_eq!(held, 1, "exactly one acquirer may hold the permit");
    assert_eq!(refused, 15);

    let record = store.load(&worktree).await.expect("load").expect("row");
    assert_eq!(record.state, WriterPermitState::Held);
    assert_eq!(
        record.revision, 1,
        "a refused acquirer must not bump the revision"
    );
}

/// Two different checkouts are independent: a held permit on one does not
/// block the other.
#[tokio::test]
async fn distinct_worktrees_acquire_in_parallel() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let first = WorktreeId::new();
    let second = WorktreeId::new();

    store
        .acquire(acquire_request(scope(&first, 1), "key-1"))
        .await
        .expect("first acquire");
    let second_record = store
        .acquire(acquire_request(scope(&second, 1), "key-2"))
        .await
        .expect("a different checkout is not blocked");
    assert_eq!(second_record.state, WriterPermitState::Held);
    assert_ne!(second_record.worktree_id, first);
}

// ---------------------------------------------------------------------------
// The fence
// ---------------------------------------------------------------------------

/// A holder whose token does not match cannot release.
#[tokio::test]
async fn a_stale_owner_token_cannot_release() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    let record = store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");

    let outcome = store
        .begin_release(BeginReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: "not-the-token".to_owned(),
            expected_revision: record.revision,
            expected_lease_generation: record.lease_generation,
        })
        .await;
    assert!(matches!(outcome, Err(WriterPermitError::NotOwner)));
}

/// A holder acting on a stale revision cannot release.
#[tokio::test]
async fn a_stale_revision_cannot_release() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    let record = store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");
    let owner_token = record.owner.expect("owner").token;

    let outcome = store
        .begin_release(BeginReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token,
            expected_revision: record.revision + 7,
            expected_lease_generation: record.lease_generation,
        })
        .await;
    assert!(matches!(outcome, Err(WriterPermitError::StaleRevision)));
}

/// A holder acting on a stale lease generation cannot release.
#[tokio::test]
async fn a_stale_lease_generation_cannot_release() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    let record = store
        .acquire(acquire_request(scope(&worktree, 4), "key-1"))
        .await
        .expect("acquire");
    let owner_token = record.owner.expect("owner").token;

    let outcome = store
        .begin_release(BeginReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token,
            expected_revision: record.revision,
            expected_lease_generation: record.lease_generation - 1,
        })
        .await;
    assert!(matches!(outcome, Err(WriterPermitError::StaleRevision)));
}

/// ABA: a permit that was released and re-acquired cannot be released by the
/// first holder.
#[tokio::test]
async fn a_reacquired_permit_invalidates_the_previous_holder() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();

    let first = store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");
    let first_token = first.owner.expect("owner").token;

    let begun = store
        .begin_release(BeginReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: first_token.clone(),
            expected_revision: first.revision,
            expected_lease_generation: first.lease_generation,
        })
        .await
        .expect("begin release");
    store
        .complete_release(CompleteReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: first_token.clone(),
            expected_revision: begun.revision,
            expected_lease_generation: begun.lease_generation,
            evidence: QuiescenceEvidence::Settled,
            transfer: None,
        })
        .await
        .expect("complete release");

    let second = store
        .acquire(acquire_request(scope(&worktree, 1), "key-2"))
        .await
        .expect("reacquire");
    let second_token = second.owner.expect("owner").token;
    assert_ne!(first_token, second_token, "a new holder gets a new token");

    // The first holder now presents its old token against the new revision.
    let outcome = store
        .begin_release(BeginReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: first_token,
            expected_revision: second.revision,
            expected_lease_generation: second.lease_generation,
        })
        .await;
    assert!(matches!(outcome, Err(WriterPermitError::NotOwner)));
}

/// A permit granted under an old lease generation cannot be taken over by a
/// holder of the new generation while it is still held.
#[tokio::test]
async fn a_permit_held_under_an_old_generation_blocks_a_new_generation() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();

    store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");

    let outcome = store
        .acquire(acquire_request(scope(&worktree, 2), "key-2"))
        .await;
    assert!(
        matches!(outcome, Err(WriterPermitError::StaleLeaseGeneration)),
        "a checkout re-leased under a new generation must not silently hand the permit over: {outcome:?}"
    );
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

/// A retry with the same key and the same payload returns the same permit.
#[tokio::test]
async fn an_idempotent_retry_returns_the_same_permit() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    let request = acquire_request(scope(&worktree, 1), "key-1");

    let first = store.acquire(request.clone()).await.expect("acquire");
    let second = store.acquire(request).await.expect("idempotent retry");
    assert_eq!(first.revision, second.revision);
    assert_eq!(first.owner, second.owner);
    assert_eq!(second.state, WriterPermitState::Held);
}

/// A retry with the same key and an altered payload is refused.
#[tokio::test]
async fn an_idempotent_retry_with_an_altered_payload_is_refused() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();

    let mut original = scope(&worktree, 1);
    store
        .acquire(acquire_request(original.clone(), "key-1"))
        .await
        .expect("acquire");

    // Same key, different scope: a different request wearing an old key.
    original.attempt_id = Some("attempt-2".to_owned());
    let outcome = store.acquire(acquire_request(original, "key-1")).await;
    assert!(matches!(
        outcome,
        Err(WriterPermitError::IdempotencyConflict)
    ));
}

// ---------------------------------------------------------------------------
// Quarantine
// ---------------------------------------------------------------------------

/// Unproven quiescence parks the permit instead of freeing it.
#[tokio::test]
async fn unproven_quiescence_quarantines_instead_of_releasing() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    let record = store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");
    let owner_token = record.owner.expect("owner").token;

    let begun = store
        .begin_release(BeginReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: owner_token.clone(),
            expected_revision: record.revision,
            expected_lease_generation: record.lease_generation,
        })
        .await
        .expect("begin release");
    let completed = store
        .complete_release(CompleteReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token,
            expected_revision: begun.revision,
            expected_lease_generation: begun.lease_generation,
            evidence: QuiescenceEvidence::Unproven(
                WriterPermitQuarantineReason::ProcessTreeUncertain,
            ),
            transfer: None,
        })
        .await
        .expect("complete release parks the permit");

    assert_eq!(completed.state, WriterPermitState::Quarantined);
    assert_eq!(
        completed.quarantine.expect("quarantine").reason,
        WriterPermitQuarantineReason::ProcessTreeUncertain
    );

    // A quarantined permit is not acquirable.
    let outcome = store
        .acquire(acquire_request(scope(&worktree, 1), "key-2"))
        .await;
    assert!(matches!(outcome, Err(WriterPermitError::Quarantined)));
}

/// A quarantined permit stays parked until an operator releases it, and the
/// operator's release needs proven quiescence.
#[tokio::test]
async fn a_quarantined_permit_needs_an_operator_release() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    let record = store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");
    assert_eq!(record.state, WriterPermitState::Held);

    let quarantined = store
        .quarantine(QuarantineWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: None,
            expected_revision: None,
            reason: WriterPermitQuarantineReason::OperatorHold,
            detail: "operator parked this checkout".to_owned(),
        })
        .await
        .expect("quarantine");
    assert_eq!(quarantined.state, WriterPermitState::Quarantined);

    // An operator release without proven quiescence is refused.
    let refused = store
        .operator_release(OperatorReleaseWriterPermit {
            worktree_id: worktree.clone(),
            expected_revision: quarantined.revision,
            reason: "looked at it".to_owned(),
            evidence: QuiescenceEvidence::Unproven(WriterPermitQuarantineReason::IncompleteEffects),
        })
        .await;
    assert!(matches!(
        refused,
        Err(WriterPermitError::UnprovenQuiescence(_))
    ));

    // With proven quiescence it is freed.
    let released = store
        .operator_release(OperatorReleaseWriterPermit {
            worktree_id: worktree.clone(),
            expected_revision: quarantined.revision,
            reason: "looked at it".to_owned(),
            evidence: QuiescenceEvidence::Settled,
        })
        .await
        .expect("operator release");
    assert_eq!(released.state, WriterPermitState::Available);
    assert!(released.quarantine.is_none());
}

/// An expired lease does not free a live writer.
#[tokio::test]
async fn an_expired_lease_does_not_free_the_permit() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();

    let mut request = acquire_request(scope(&worktree, 1), "key-1");
    // A lease that expired an hour ago.
    request.lease_expires_at_ms = Some(chrono::Utc::now().timestamp_millis() - 3_600_000);
    let acquired = store.acquire(request).await.expect("acquire");
    assert_eq!(acquired.state, WriterPermitState::Held);

    // The permit is still held: expiry is a liveness hint, not a release.
    let outcome = store
        .acquire(acquire_request(scope(&worktree, 1), "key-2"))
        .await;
    assert!(matches!(outcome, Err(WriterPermitError::Held)));
}

// ---------------------------------------------------------------------------
// Transfer
// ---------------------------------------------------------------------------

/// A transfer mints a new token and invalidates the old one in the same
/// transition.
#[tokio::test]
async fn a_transfer_invalidates_the_previous_holder() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    let record = store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");
    let first_token = record.owner.expect("owner").token;

    let begun = store
        .begin_release(BeginReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: first_token.clone(),
            expected_revision: record.revision,
            expected_lease_generation: record.lease_generation,
        })
        .await
        .expect("begin release");
    let transferred = store
        .complete_release(CompleteReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: first_token.clone(),
            expected_revision: begun.revision,
            expected_lease_generation: begun.lease_generation,
            evidence: QuiescenceEvidence::Settled,
            transfer: Some(codegg_core::writer_permit::WriterPermitTransfer {
                principal_id: PrincipalId::new(),
                agent_run_id: Some(AgentRunId::new()),
                group_id: Some(AgentRunGroupId::new()),
                attempt_id: Some("attempt-2".to_owned()),
                operation: WriterPermitOperation::NativeGitMutation,
                idempotency_key: "key-2".to_owned(),
                lease_expires_at_ms: None,
            }),
        })
        .await
        .expect("complete release with a transfer");

    assert_eq!(transferred.state, WriterPermitState::Held);
    let second_token = transferred.owner.expect("owner").token;
    assert_ne!(first_token, second_token);
    assert_eq!(
        transferred.operation,
        Some(WriterPermitOperation::NativeGitMutation)
    );

    // The old token is dead.
    let outcome = store
        .begin_release(BeginReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: first_token,
            expected_revision: transferred.revision,
            expected_lease_generation: transferred.lease_generation,
        })
        .await;
    assert!(matches!(outcome, Err(WriterPermitError::NotOwner)));
}

// ---------------------------------------------------------------------------
// Restart reconciliation
// ---------------------------------------------------------------------------

/// A permit that was live when the daemon died is quarantined, not freed.
#[tokio::test]
async fn restart_reconciliation_quarantines_a_live_permit() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");

    let reconciled = store.reconcile_after_restart().await.expect("reconcile");
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, WriterPermitState::Quarantined);
    assert_eq!(
        reconciled[0].quarantine.clone().expect("quarantine").reason,
        WriterPermitQuarantineReason::DaemonRestart
    );

    // It is parked, not freed: a restart never hands the checkout to the next
    // writer on its own.
    let outcome = store
        .acquire(acquire_request(scope(&worktree, 1), "key-2"))
        .await;
    assert!(matches!(outcome, Err(WriterPermitError::Quarantined)));
}

/// A permit that was mid-release when the daemon died is also quarantined.
#[tokio::test]
async fn restart_reconciliation_quarantines_an_interrupted_release() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    let record = store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");
    store
        .begin_release(BeginReleaseWriterPermit {
            worktree_id: worktree.clone(),
            owner_token: record.owner.expect("owner").token,
            expected_revision: record.revision,
            expected_lease_generation: record.lease_generation,
        })
        .await
        .expect("begin release");

    let reconciled = store.reconcile_after_restart().await.expect("reconcile");
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, WriterPermitState::Quarantined);
}

/// The quarantine survives a close and a reopen: the permit is durable.
#[tokio::test]
async fn a_quarantine_survives_a_close_and_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("permits.db");

    {
        let pool = file_pool(&path).await;
        let store = WriterPermitStore::new(pool.clone());
        let worktree = WorktreeId::new();
        store
            .acquire(acquire_request(scope(&worktree, 1), "key-1"))
            .await
            .expect("acquire");
        store.reconcile_after_restart().await.expect("reconcile");
        pool.close().await;
    }

    let pool = file_pool(&path).await;
    let store = WriterPermitStore::new(pool);
    let rows = store
        .list(&codegg_core::writer_permit::WriterPermitFilter::default())
        .await
        .expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, WriterPermitState::Quarantined);
}

// ---------------------------------------------------------------------------
// Transaction shape
// ---------------------------------------------------------------------------

/// An uncommitted transaction rolls back. This is the property that makes a
/// panicking holder safe: the row it was writing is never half-written.
#[tokio::test]
async fn an_uncommitted_transaction_rolls_back() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool.clone());
    let worktree = WorktreeId::new();
    store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");
    let before = store.load(&worktree).await.expect("load").expect("row");

    {
        // Take the write lock, change the row, and drop the transaction
        // without committing — exactly what a panic inside the critical
        // section does.
        let mut tx = ImmediateWrite::begin(&pool).await.expect("begin immediate");
        sqlx::query(
            "UPDATE writer_permit SET state = 'available', owner_principal_id = NULL, \
             owner_token = NULL WHERE worktree_id = ?",
        )
        .bind(worktree_id(&before))
        .execute(&mut **tx)
        .await
        .expect("update");
        // Dropped here, uncommitted.
    }

    let after = store.load(&worktree).await.expect("load").expect("row");
    assert_eq!(
        after.state,
        WriterPermitState::Held,
        "the update rolled back"
    );
    assert_eq!(after.revision, before.revision);
}

fn worktree_id(record: &codegg_core::writer_permit::WriterPermitRecord) -> &str {
    record.worktree_id.as_str()
}

/// A panicking holder leaves the permit **held**, not freed.
#[tokio::test]
async fn a_panicking_holder_leaves_the_permit_held() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool.clone());
    let worktree = WorktreeId::new();

    let store_for_task = store.clone();
    let worktree_for_task = worktree.clone();
    let task = tokio::spawn(async move {
        let _guard = store_for_task
            .acquire(acquire_request(scope(&worktree_for_task, 1), "key-1"))
            .await
            .expect("acquire");
        panic!("the holder crashed mid-flight");
    });
    let _ = task.await;

    let record = store.load(&worktree).await.expect("load").expect("row");
    assert_eq!(
        record.state,
        WriterPermitState::Held,
        "a crashed holder must not free the permit; the checkout's state is unknown"
    );
}

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

/// The operator view carries identity and state, never free text.
#[tokio::test]
async fn the_operator_view_carries_no_free_text() {
    let pool = isolated_pool().await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();
    let record = store
        .acquire(acquire_request(scope(&worktree, 1), "key-1"))
        .await
        .expect("acquire");

    let view = record.to_view();
    assert_eq!(view.state, "held");
    assert_eq!(view.operation.as_deref(), Some("native_file_write"));
    assert!(view.owner_principal_id.is_some());
    assert!(view.quarantine_reason.is_none());
    // The possession token is never projected.
    let serialized = serde_json::to_string(&view).expect("serialize");
    assert!(!serialized.contains(&record.owner.expect("owner").token));
}

/// The store is shareable across threads and connections.
#[tokio::test]
async fn the_store_is_usable_from_many_connections() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("permits.db");
    let pool = file_pool(&path).await;
    let store = WriterPermitStore::new(pool);
    let worktree = WorktreeId::new();

    let mut tasks = Vec::new();
    for index in 0..8 {
        let store = store.clone();
        let worktree = worktree.clone();
        tasks.push(tokio::spawn(async move {
            store
                .acquire(acquire_request(
                    scope(&worktree, 1),
                    &format!("key-{index}"),
                ))
                .await
        }));
    }
    let mut held = 0;
    for task in tasks {
        if task.await.expect("joins").is_ok() {
            held += 1;
        }
    }
    assert_eq!(held, 1, "competing transactions produce exactly one holder");
}
