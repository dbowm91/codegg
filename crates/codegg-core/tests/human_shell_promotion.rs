use codegg_core::identity::PrincipalId;
use codegg_core::session::{
    schema, HumanShellPromotionError, HumanShellPromotionStatus, HumanShellPromotionStore,
    NewHumanShellPromotion, StageHumanShellPromotionOutcome, HUMAN_SHELL_PROMOTION_MAX_PENDING,
};
use codegg_core::session_control::SessionControllerStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

async fn isolated_pool() -> SqlitePool {
    let name = format!("human_shell_promotion_{}", uuid::Uuid::new_v4().simple());
    let opts = SqliteConnectOptions::from_str(&format!("file:{name}?mode=memory&cache=shared"))
        .expect("sqlite options")
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect sqlite");
    schema::migrate(&pool).await.expect("migrate");
    pool
}

async fn ensure_session(pool: &SqlitePool, session_id: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO project (id, worktree, sandboxes, time_created, time_updated) \
         VALUES ('project-1', '/tmp/project-1', '[]', ?, ?)",
    )
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert project");
    sqlx::query(
        "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) \
         VALUES (?, 'project-1', 'test', '/tmp/project-1', 'Test', '1', ?, ?)",
    )
    .bind(session_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert session");
}

fn promotion(id: &str, projection: &str, now_ms: i64) -> NewHumanShellPromotion {
    NewHumanShellPromotion {
        id: id.to_owned(),
        source_id: "42".to_owned(),
        session_id: "session-1".to_owned(),
        principal_id: "principal-1".to_owned(),
        project_id: "project-1".to_owned(),
        workspace_id: "workspace-1".to_owned(),
        projection: projection.to_owned(),
        created_at_ms: now_ms,
    }
}

#[tokio::test]
async fn staging_is_bounded_idempotent_and_principal_scoped() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-1").await;
    let store = HumanShellPromotionStore::new(pool.clone());
    let now = chrono::Utc::now().timestamp_millis();

    let first = store
        .stage(promotion("promotion-1", "redacted evidence", now))
        .await
        .expect("stage");
    let StageHumanShellPromotionOutcome::Staged(first) = first else {
        panic!("first stage was unexpectedly treated as duplicate")
    };
    assert_eq!(first.status, HumanShellPromotionStatus::Pending);
    assert_eq!(first.projection_bytes, "redacted evidence".len());

    let duplicate = store
        .stage(promotion("promotion-1", "redacted evidence", now + 1))
        .await
        .expect("idempotent stage");
    assert!(matches!(
        duplicate,
        StageHumanShellPromotionOutcome::Duplicate(_)
    ));

    let mut conflicting = promotion("promotion-1", "different content", now + 2);
    assert!(matches!(
        store.stage(conflicting.clone()).await,
        Err(HumanShellPromotionError::IdentityConflict)
    ));
    conflicting.id = "promotion-2".to_owned();
    conflicting.projection = "x".repeat(32 * 1024 + 1);
    assert!(matches!(
        store.stage(conflicting).await,
        Err(HumanShellPromotionError::Invalid {
            field: "projection",
            ..
        })
    ));

    let owned = store
        .list_pending(
            "session-1",
            "principal-1",
            "project-1",
            "workspace-1",
            now + 3,
        )
        .await
        .expect("list owner records");
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].id, "promotion-1");
    let reopened = HumanShellPromotionStore::new(pool);
    let after_reconnect = reopened
        .list_pending(
            "session-1",
            "principal-1",
            "project-1",
            "workspace-1",
            now + 3,
        )
        .await
        .expect("recovered after store recreation");
    assert_eq!(after_reconnect, owned);
    let other = store
        .list_pending(
            "session-1",
            "principal-2",
            "project-1",
            "workspace-1",
            now + 3,
        )
        .await
        .expect("list other principal");
    assert!(other.is_empty());
    let stale_workspace = store
        .list_pending(
            "session-1",
            "principal-1",
            "project-1",
            "workspace-2",
            now + 3,
        )
        .await
        .expect("list stale workspace");
    assert!(stale_workspace.is_empty());
}

#[tokio::test]
async fn pending_queue_cap_rejects_without_adding_an_extra_row() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-1").await;
    let store = HumanShellPromotionStore::new(pool);
    let now = chrono::Utc::now().timestamp_millis();
    for index in 0..codegg_core::session::HUMAN_SHELL_PROMOTION_MAX_PENDING {
        store
            .stage(promotion(
                &format!("promotion-{index}"),
                "bounded evidence",
                now + index,
            ))
            .await
            .expect("within pending queue bound");
    }
    assert!(matches!(
        store
            .stage(promotion(
                "promotion-overflow",
                "bounded evidence",
                now + 100
            ))
            .await,
        Err(HumanShellPromotionError::QueueFull)
    ));
    let pending = store
        .list_pending(
            "session-1",
            "principal-1",
            "project-1",
            "workspace-1",
            now + 101,
        )
        .await
        .expect("read bounded queue");
    assert_eq!(pending.len(), HUMAN_SHELL_PROMOTION_MAX_PENDING as usize);
}

#[tokio::test]
async fn turn_admission_consumes_atomically_and_busy_admission_preserves_pending() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-1").await;
    let promotions = HumanShellPromotionStore::new(pool.clone());
    let admission = SessionControllerStore::new(pool.clone());
    let principal = PrincipalId::parse("principal-1").expect("principal");
    let now = chrono::Utc::now().timestamp_millis();
    promotions
        .stage(promotion("promotion-1", "redacted evidence", now))
        .await
        .expect("stage first");

    let (controller, claimed) = admission
        .acquire_with_promotions(
            "session-1",
            "turn-1",
            &principal,
            Some("client-1"),
            "project-1",
            "workspace-1",
            &["promotion-1".to_owned()],
            now + 1,
        )
        .await
        .expect("admit first turn");
    assert_eq!(controller.turn_id, "turn-1");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].status, HumanShellPromotionStatus::Consumed);
    assert_eq!(claimed[0].consumed_turn_id.as_deref(), Some("turn-1"));

    assert!(matches!(
        admission
            .acquire_with_promotions(
                "session-1",
                "turn-2",
                &principal,
                Some("client-1"),
                "project-1",
                "workspace-1",
                &["promotion-1".to_owned()],
                now + 2,
            )
            .await,
        Err(codegg_core::session_control::SessionTurnAdmissionError::Controller(_))
    ));

    promotions
        .stage(promotion("promotion-2", "another projection", now + 3))
        .await
        .expect("stage second");
    admission
        .release("session-1", "turn-1")
        .await
        .expect("release first turn");
    let (controller, claimed) = admission
        .acquire_with_promotions(
            "session-1",
            "turn-3",
            &principal,
            Some("client-1"),
            "project-1",
            "workspace-1",
            &["promotion-2".to_owned()],
            now + 4,
        )
        .await
        .expect("admit second promotion");
    assert_eq!(controller.turn_id, "turn-3");
    assert_eq!(claimed[0].consumed_turn_id.as_deref(), Some("turn-3"));

    // A mismatched scope rolls back the controller insert as well as the
    // promotion claim, so another client can still use the pending row.
    admission
        .release("session-1", "turn-3")
        .await
        .expect("release second turn");
    promotions
        .stage(promotion("promotion-3", "third projection", now + 5))
        .await
        .expect("stage third");
    let error = admission
        .acquire_with_promotions(
            "session-1",
            "turn-4",
            &principal,
            Some("client-1"),
            "project-1",
            "workspace-elsewhere",
            &["promotion-3".to_owned()],
            now + 6,
        )
        .await
        .expect_err("stale workspace must fail closed");
    assert!(matches!(
        error,
        codegg_core::session_control::SessionTurnAdmissionError::Promotion(
            HumanShellPromotionError::StaleBinding
        )
    ));
    assert!(admission
        .get("session-1")
        .await
        .expect("controller read")
        .is_none());
    let pending = promotions
        .list_pending(
            "session-1",
            "principal-1",
            "project-1",
            "workspace-1",
            now + 7,
        )
        .await
        .expect("pending read");
    assert!(pending.iter().any(|item| item.id == "promotion-3"));
}
