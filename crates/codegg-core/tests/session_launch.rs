//! M006 qualification: the durable session-launch saga.
//!
//! These tests exercise the **store**, which is the saga's authority: the
//! idempotency contract, the stage machine, and the restart
//! reconciliation.
//!
//! Every test runs against a real SQLite database. An in-memory fake would
//! prove nothing about the properties that matter here — the properties are
//! properties of the *transaction*, not of the data structure.

use std::str::FromStr;

use codegg_core::session_launch::{
    SessionLaunchError, SessionLaunchIntent, SessionLaunchPlacement, SessionLaunchProgress,
    SessionLaunchRequest, SessionLaunchStage, SqliteSessionLaunchStore,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

/// An in-memory database, migrated once. `migrate` runs inside the helper,
/// so no test adds its own.
async fn isolated_pool() -> SqlitePool {
    let name = format!("session_launch_{}", uuid::Uuid::new_v4().simple());
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
        .expect("migrations run inside the helper");
    pool
}

fn new_request(key: &str) -> SessionLaunchRequest {
    SessionLaunchRequest {
        project_id: "project-1".to_string(),
        intent: SessionLaunchIntent::New,
        placement: SessionLaunchPlacement::NewManagedCheckout {
            base_ref: Some("main".to_string()),
            target_branch: Some("feature/m006".to_string()),
        },
        parent_session_id: None,
        idempotency_key: key.to_string(),
    }
}

fn fork_request(key: &str) -> SessionLaunchRequest {
    SessionLaunchRequest {
        project_id: "project-1".to_string(),
        intent: SessionLaunchIntent::Fork,
        placement: SessionLaunchPlacement::NewManagedCheckout {
            base_ref: None,
            target_branch: None,
        },
        parent_session_id: Some("session-parent".to_string()),
        idempotency_key: key.to_string(),
    }
}

#[tokio::test]
async fn an_accepted_launch_starts_at_the_accepted_stage() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    let record = store
        .accept(&new_request("k1"), 1_000)
        .await
        .expect("accept");
    assert_eq!(record.stage, SessionLaunchStage::Accepted);
    assert_eq!(record.project_id, "project-1");
    assert!(record.worktree_id.is_none());
    assert!(record.session_id.is_none());
    assert!(record.workspace_id.is_none());
}

#[tokio::test]
async fn a_retry_with_the_same_key_and_fingerprint_converges() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    let first = store
        .accept(&new_request("k1"), 1_000)
        .await
        .expect("accept");
    let second = store
        .accept(&new_request("k1"), 2_000)
        .await
        .expect("retry");
    assert_eq!(
        first.launch_id, second.launch_id,
        "a retry must converge on the original launch, not allocate a second one"
    );
    assert_eq!(second.stage, SessionLaunchStage::Accepted);
    let listed = store.list("project-1", 10).await.expect("list");
    assert_eq!(listed.len(), 1, "a retry must not create a second row");
}

#[tokio::test]
async fn a_retry_with_a_different_fingerprint_is_refused() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    store
        .accept(&new_request("k1"), 1_000)
        .await
        .expect("accept");
    let mut conflicting = new_request("k1");
    conflicting.placement = SessionLaunchPlacement::NewManagedCheckout {
        base_ref: Some("other".to_string()),
        target_branch: None,
    };
    let error = store
        .accept(&conflicting, 2_000)
        .await
        .expect_err("a different request under an old key is a conflict");
    assert!(matches!(
        error,
        SessionLaunchError::IdempotencyConflict { .. }
    ));
}

#[tokio::test]
async fn a_fork_without_a_parent_is_refused() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    let mut request = fork_request("k1");
    request.parent_session_id = None;
    let error = store
        .accept(&request, 1_000)
        .await
        .expect_err("a fork needs a parent");
    assert!(matches!(error, SessionLaunchError::InvalidRequest(_)));
}

#[tokio::test]
async fn an_empty_idempotency_key_is_refused() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    let error = store
        .accept(&new_request(""), 1_000)
        .await
        .expect_err("an empty key is not a key");
    assert!(matches!(error, SessionLaunchError::InvalidRequest(_)));
}

#[tokio::test]
async fn the_stages_advance_in_order() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    let record = store
        .accept(&new_request("k1"), 1_000)
        .await
        .expect("accept");

    let record = store
        .advance(
            &record.launch_id,
            SessionLaunchStage::CheckoutReserved,
            SessionLaunchProgress {
                worktree_id: Some("wt-1".to_string()),
                ..Default::default()
            },
            2_000,
        )
        .await
        .expect("reserve");
    assert_eq!(record.stage, SessionLaunchStage::CheckoutReserved);
    assert_eq!(record.worktree_id.as_deref(), Some("wt-1"));

    let record = store
        .advance(
            &record.launch_id,
            SessionLaunchStage::CheckoutReady,
            SessionLaunchProgress {
                workspace_id: Some("ws-1".to_string()),
                ..Default::default()
            },
            3_000,
        )
        .await
        .expect("ready");
    assert_eq!(record.stage, SessionLaunchStage::CheckoutReady);
    assert_eq!(record.workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(
        record.worktree_id.as_deref(),
        Some("wt-1"),
        "an earlier stage's product must survive a later one"
    );

    let record = store
        .advance(
            &record.launch_id,
            SessionLaunchStage::SessionCreated,
            SessionLaunchProgress {
                session_id: Some("s-1".to_string()),
                ..Default::default()
            },
            4_000,
        )
        .await
        .expect("session");
    assert_eq!(record.stage, SessionLaunchStage::SessionCreated);

    let record = store
        .advance(
            &record.launch_id,
            SessionLaunchStage::Bound,
            SessionLaunchProgress::default(),
            5_000,
        )
        .await
        .expect("bind");
    assert_eq!(record.stage, SessionLaunchStage::Bound);

    let record = store
        .advance(
            &record.launch_id,
            SessionLaunchStage::Completed,
            SessionLaunchProgress::default(),
            6_000,
        )
        .await
        .expect("complete");
    assert_eq!(record.stage, SessionLaunchStage::Completed);
    assert!(record.stage.is_terminal());
}

#[tokio::test]
async fn a_backward_transition_is_refused() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    let record = store
        .accept(&new_request("k1"), 1_000)
        .await
        .expect("accept");
    let record = store
        .advance(
            &record.launch_id,
            SessionLaunchStage::CheckoutReserved,
            SessionLaunchProgress::default(),
            2_000,
        )
        .await
        .expect("reserve");
    let error = store
        .advance(
            &record.launch_id,
            SessionLaunchStage::Accepted,
            SessionLaunchProgress::default(),
            3_000,
        )
        .await
        .expect_err("a launch never moves backwards");
    assert!(matches!(
        error,
        SessionLaunchError::InvalidTransition { .. }
    ));
}

#[tokio::test]
async fn a_terminal_launch_never_moves_again() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    let record = store
        .accept(&new_request("k1"), 1_000)
        .await
        .expect("accept");
    let record = store
        .advance(
            &record.launch_id,
            SessionLaunchStage::NeedsAttention,
            SessionLaunchProgress {
                attention_code: Some("checkout_failed".to_string()),
                ..Default::default()
            },
            2_000,
        )
        .await
        .expect("attention");
    assert_eq!(record.stage, SessionLaunchStage::NeedsAttention);
    assert_eq!(record.attention_code.as_deref(), Some("checkout_failed"));
    let error = store
        .advance(
            &record.launch_id,
            SessionLaunchStage::Completed,
            SessionLaunchProgress::default(),
            3_000,
        )
        .await
        .expect_err("a launch that needs attention is terminal until an operator acts");
    assert!(matches!(
        error,
        SessionLaunchError::InvalidTransition { .. }
    ));
}

#[tokio::test]
async fn reconciliation_reports_only_the_unfinished_launches() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    let finished = store
        .accept(&new_request("done"), 1_000)
        .await
        .expect("accept");
    store
        .advance(
            &finished.launch_id,
            SessionLaunchStage::NeedsAttention,
            SessionLaunchProgress {
                attention_code: Some("checkout_failed".to_string()),
                ..Default::default()
            },
            1_100,
        )
        .await
        .expect("attention");
    let unfinished = store
        .accept(&new_request("open"), 2_000)
        .await
        .expect("accept");

    let pending = store.reconcile().await.expect("reconcile");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].launch_id, unfinished.launch_id);
    assert_eq!(pending[0].stage, SessionLaunchStage::Accepted);
}

#[tokio::test]
async fn the_fingerprint_distinguishes_the_request_from_its_key() {
    let base = new_request("k1");
    let mut other_placement = base.clone();
    other_placement.placement = SessionLaunchPlacement::ExistingManagedWorktree {
        worktree_id: "wt-9".to_string(),
    };
    assert_ne!(
        base.fingerprint(),
        other_placement.fingerprint(),
        "a different placement is a different request"
    );
    let mut other_parent = base.clone();
    other_parent.parent_session_id = Some("session-parent".to_string());
    assert_ne!(
        base.fingerprint(),
        other_parent.fingerprint(),
        "a different parent is a different request"
    );
    assert_eq!(
        base.fingerprint(),
        base.clone().fingerprint(),
        "the same request has the same fingerprint"
    );
}

#[tokio::test]
async fn an_existing_checkout_placement_is_recorded_verbatim() {
    let pool = isolated_pool().await;
    let store = SqliteSessionLaunchStore::new(pool);
    let mut request = new_request("k1");
    request.placement = SessionLaunchPlacement::ExistingManagedWorktree {
        worktree_id: "wt-existing".to_string(),
    };
    let record = store.accept(&request, 1_000).await.expect("accept");
    assert_eq!(
        record.placement,
        SessionLaunchPlacement::ExistingManagedWorktree {
            worktree_id: "wt-existing".to_string()
        }
    );
    let reloaded = store
        .load(&record.launch_id)
        .await
        .expect("load")
        .expect("present");
    assert_eq!(reloaded.placement, record.placement);
}
