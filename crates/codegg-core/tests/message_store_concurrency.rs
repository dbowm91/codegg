use chrono::Utc;
use codegg_core::session::{schema, MessageStore};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;
use std::time::Duration;

async fn isolated_pool() -> SqlitePool {
    let name = format!("message_store_{}", uuid::Uuid::new_v4().simple());
    let options = SqliteConnectOptions::from_str(&format!("file:{name}?mode=memory&cache=shared"))
        .expect("sqlite options")
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .expect("connect sqlite");
    schema::migrate(&pool).await.expect("migrate");
    let now = Utc::now().timestamp_millis();
    sqlx::query("INSERT INTO project(id, worktree, sandboxes, time_created, time_updated) VALUES ('p', '/tmp', '[]', ?, ?)")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert project");
    sqlx::query("INSERT INTO session(id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('s', 'p', 's', '/tmp', 's', '1', ?, ?)")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert session");
    pool
}

fn batch(writer: &str, round: usize) -> Vec<(String, serde_json::Value)> {
    (0..2)
        .map(|ordinal| {
            let id = format!("{}-{}-{}", writer, round, ordinal);
            let data = serde_json::json!({
                "id": id.clone(),
                "session_id": "s",
                "message_id": id.clone(),
                "parts": []
            });
            (id, data)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_transcript_appends_persist_both_writers() {
    const ROUNDS: usize = 64;

    let pool = isolated_pool().await;
    let first = MessageStore::new(pool.clone());
    let second = MessageStore::new(pool.clone());

    for round in 0..ROUNDS {
        let first_batch = batch("first", round);
        let second_batch = batch("second", round);
        let (first_result, second_result) = tokio::join!(
            first.append_batch("s", &first_batch),
            second.append_batch("s", &second_batch),
        );
        assert_eq!(first_result.expect("first append"), Some(2));
        assert_eq!(second_result.expect("second append"), Some(2));
    }

    assert_eq!(
        first.count("s").await.expect("count transcript"),
        ROUNDS * 4
    );
}
