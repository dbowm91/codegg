//! Storage invariant for `cached_models`.
//!
//! `cached_models` is a **cross-provider** discovery cache: the same
//! model id is legitimately advertised by several providers at once
//! (`gpt-*` on OpenAI/Azure/OpenRouter, `claude-*` on Anthropic/
//! Bedrock, `gemini-*` on Google/Vertex). The table originally declared
//! `id TEXT PRIMARY KEY`, so a whole-batch insert aborted with
//! `UNIQUE constraint failed: cached_models.id`. The writer clears the
//! table before inserting, so the cache was left permanently empty and
//! every discovery refresh re-hit the network.
//!
//! The real identity of a discovered model is `(id, provider)`.

use codegg_core::session::schema;
use codegg_core::storage::STORAGE_LAYOUT_VERSION;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

async fn memory_pool(name: &str) -> SqlitePool {
    let url = format!("file:{name}?mode=memory&cache=shared");
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect in-memory sqlite")
}

async fn insert_row(pool: &SqlitePool, id: &str, provider: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR REPLACE INTO cached_models \
         (id, provider, name, context_window, max_output_tokens, \
          supports_tools, supports_vision, fetched_at) \
         VALUES (?, ?, ?, 0, NULL, 1, 0, 0)",
    )
    .bind(id)
    .bind(provider)
    .bind(format!("{provider}:{id}"))
    .execute(pool)
    .await
    .map(|_| ())
}

async fn row_count(pool: &SqlitePool, id: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM cached_models WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count rows")
}

#[tokio::test]
async fn the_same_model_id_is_storable_under_two_providers() {
    let pool = memory_pool("cached_models_composite_key").await;
    schema::migrate(&pool).await.expect("migrate");

    // This is the exact write that used to abort the entire cache batch.
    insert_row(&pool, "gpt-4o", "openai")
        .await
        .expect("openai advertises gpt-4o");
    insert_row(&pool, "gpt-4o", "azure")
        .await
        .expect("azure advertises gpt-4o");

    assert_eq!(
        row_count(&pool, "gpt-4o").await,
        2,
        "a cross-provider duplicate id must persist both rows"
    );
}

#[tokio::test]
async fn the_same_provider_id_stays_unique() {
    let pool = memory_pool("cached_models_same_provider").await;
    schema::migrate(&pool).await.expect("migrate");

    insert_row(&pool, "mimo-v2.6-flash", "opencode_go")
        .await
        .expect("first write");
    // Re-discovery of the same model is idempotent, not a duplicate.
    insert_row(&pool, "mimo-v2.6-flash", "opencode_go")
        .await
        .expect("re-discovery is idempotent");

    assert_eq!(row_count(&pool, "mimo-v2.6-flash").await, 1);
}

#[tokio::test]
async fn upgrading_from_the_single_key_shape_preserves_rows_and_lifts_the_constraint() {
    // Build a pre-v70 database: `id` is the sole primary key and one
    // row is already cached.
    let pool = memory_pool("cached_models_upgrade").await;
    sqlx::query(
        "CREATE TABLE migration_version (id INTEGER PRIMARY KEY, version INTEGER NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("create migration_version");
    sqlx::query("INSERT INTO migration_version (id, version) VALUES (1, 69)")
        .execute(&pool)
        .await
        .expect("seed version 69");
    sqlx::query(
        r#"
        CREATE TABLE cached_models (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            name TEXT NOT NULL,
            context_window INTEGER,
            max_output_tokens INTEGER,
            supports_tools INTEGER NOT NULL DEFAULT 1,
            supports_vision INTEGER NOT NULL DEFAULT 0,
            fetched_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy cached_models");
    sqlx::query(
        "INSERT INTO cached_models \
         (id, provider, name, context_window, max_output_tokens, \
          supports_tools, supports_vision, fetched_at) \
         VALUES ('muse-spark-1.2-contributor', 'opencode_go', 'Muse 1.2', 0, NULL, 1, 0, 7)",
    )
    .execute(&pool)
    .await
    .expect("seed a cached row");

    schema::migrate(&pool).await.expect("upgrade to current");

    let version: i64 = sqlx::query_scalar("SELECT version FROM migration_version WHERE id = 1")
        .fetch_one(&pool)
        .await
        .expect("read version");
    assert_eq!(version, STORAGE_LAYOUT_VERSION as i64);

    // The pre-existing row survived the table rebuild.
    let carried: String = sqlx::query_scalar(
        "SELECT name FROM cached_models WHERE id = 'muse-spark-1.2-contributor'",
    )
    .fetch_one(&pool)
    .await
    .expect("cached row carried forward");
    assert_eq!(carried, "Muse 1.2");

    // And the constraint that caused the cache to be permanently empty is gone.
    insert_row(&pool, "muse-spark-1.2-contributor", "other:opencode_go")
        .await
        .expect("a second provider may now advertise the same id");
    assert_eq!(row_count(&pool, "muse-spark-1.2-contributor").await, 2);
}

#[tokio::test]
async fn the_provider_lookup_index_survives_the_rebuild() {
    let pool = memory_pool("cached_models_index").await;
    schema::migrate(&pool).await.expect("migrate");

    let indexed: String = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' \
         AND name = 'cached_models_provider_idx'",
    )
    .fetch_one(&pool)
    .await
    .expect("provider index present after migration");
    assert_eq!(indexed, "cached_models_provider_idx");
}
