use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;
use uuid::Uuid;

use codegg_core::projection_replay::service::ProjectionReplayService;
use codegg_core::projection_replay::store::ProjectionReplayStore;

#[allow(dead_code)]
pub async fn test_pool() -> SqlitePool {
    let name = format!("proj_replay_test_{}", Uuid::new_v4().simple());
    let url = format!("file:{}?mode=memory&cache=shared", name);
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite connect options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect to in-memory sqlite");
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("run migrations");
    pool
}

#[allow(dead_code)]
pub async fn test_service() -> ProjectionReplayService {
    let pool = test_pool().await;
    let store = std::sync::Arc::new(ProjectionReplayStore::new(pool));
    ProjectionReplayService::new(store)
}
