use chrono::Utc;
use codegg_core::session::{schema, SessionWorkingDirectoryError, SessionWorkingDirectoryStore};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

async fn isolated_pool() -> SqlitePool {
    let name = format!("session_cwd_{}", uuid::Uuid::new_v4().simple());
    let opts = SqliteConnectOptions::from_str(&format!("file:{name}?mode=memory&cache=shared"))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    schema::migrate(&pool).await.unwrap();
    let now = Utc::now().timestamp_millis();
    sqlx::query("INSERT INTO project(id, worktree, sandboxes, time_created, time_updated) VALUES ('p', '/tmp', '[]', ?, ?)")
        .bind(now).bind(now).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO session(id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('s', 'p', 's', '/tmp', 's', '1', ?, ?)")
        .bind(now).bind(now).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO session(id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('s2', 'p', 's2', '/tmp', 's2', '1', ?, ?)")
        .bind(now).bind(now).execute(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn session_working_directory_compare_set_persists_and_rejects_stale_revision() {
    let pool = isolated_pool().await;
    let store = SessionWorkingDirectoryStore::new(pool.clone());
    let initial = store.get("s").await.unwrap();
    assert_eq!(initial.relative_path, ".");
    assert_eq!(initial.revision, 0);
    let changed = store.compare_set("s", 0, "crates").await.unwrap();
    assert_eq!(changed.revision, 1);
    assert_eq!(store.get("s").await.unwrap(), changed);
    assert!(matches!(
        store.compare_set("s", 0, "src").await,
        Err(SessionWorkingDirectoryError::StaleRevision)
    ));
    let reloaded = SessionWorkingDirectoryStore::new(pool.clone());
    assert_eq!(reloaded.get("s").await.unwrap(), changed);
    assert_eq!(store.get("s2").await.unwrap().relative_path, ".");
}

#[tokio::test]
async fn concurrent_session_working_directory_updates_have_one_cas_winner() {
    let pool = isolated_pool().await;
    let first = SessionWorkingDirectoryStore::new(pool.clone());
    let second = SessionWorkingDirectoryStore::new(pool);
    let (left, right) = tokio::join!(
        first.compare_set("s", 0, "src"),
        second.compare_set("s", 0, "crates")
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let loser = if left.is_err() { left } else { right };
    assert!(matches!(
        loser,
        Err(SessionWorkingDirectoryError::StaleRevision)
    ));
}
