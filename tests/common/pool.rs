//! Shared test database helpers.
//!
//! Two flavours of pool are exposed:
//!
//! - [`shared_pool`]: a process-wide `SqlitePool` backed by a named
//!   in-memory SQLite database (`?cache=shared`). Migrations run **once**
//!   per test binary, not once per test. Tests that opt into the shared
//!   pool **must clean up any data they insert** (or use unique IDs).
//!
//! - [`isolated_pool`]: a fresh named in-memory database per call. Each
//!   call runs the full migration set. Use this when a test asserts on
//!   the absence of pre-existing data or otherwise needs a clean slate.
//!
//! Both helpers avoid the on-disk `tempdir` + `Box::leak` pattern that
//! previously leaked one `TempDir` per test in some binaries.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;
use std::sync::OnceLock;
use uuid::Uuid;

const SHARED_DB_NAME: &str = "codegg_test_shared";

static SHARED_POOL: OnceLock<SqlitePool> = OnceLock::new();

/// Returns a process-wide `SqlitePool`. The underlying database lives in
/// memory and is shared across all callers in this process. Migrations
/// run exactly once, on the first call.
///
/// Tests that mutate state through this pool **must clean up after
/// themselves** — typically by deleting rows they inserted, or by using
/// unique session/project identifiers.
pub async fn shared_pool() -> &'static SqlitePool {
    if let Some(pool) = SHARED_POOL.get() {
        return pool;
    }
    let pool = build_pool(SHARED_DB_NAME).await;
    SHARED_POOL
        .set(pool)
        .expect("shared pool initialized exactly once");
    SHARED_POOL.get().expect("just initialized")
}

/// Returns a fresh in-memory `SqlitePool` with its own schema. Each
/// call re-runs the migration set, so this is more expensive than
/// [`shared_pool`] — use it only when you need true isolation.
pub async fn isolated_pool() -> SqlitePool {
    let name = format!("codegg_test_iso_{}", Uuid::new_v4().simple());
    build_pool(&name).await
}

async fn build_pool(db_name: &str) -> SqlitePool {
    let url = format!("file:{}?mode=memory&cache=shared", db_name);
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
    codegg::session::schema::migrate(&pool)
        .await
        .expect("run migrations");
    pool
}
