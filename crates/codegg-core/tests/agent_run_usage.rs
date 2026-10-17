use codegg_core::session::{UsageRecord, UsageStore};
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn provider_usage_is_aggregated_by_durable_agent_run() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create usage fixture database");
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("enable usage foreign keys");
    sqlx::query("CREATE TABLE session (id TEXT PRIMARY KEY)")
        .execute(&pool)
        .await
        .expect("create session table");
    sqlx::query("CREATE TABLE agent_run (run_id TEXT PRIMARY KEY)")
        .execute(&pool)
        .await
        .expect("create agent run table");
    sqlx::query(
        "CREATE TABLE usage (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE, agent_run_id TEXT REFERENCES agent_run(run_id) ON DELETE SET NULL, provider TEXT NOT NULL, model TEXT NOT NULL, input_tokens INTEGER NOT NULL, output_tokens INTEGER NOT NULL, cached_tokens INTEGER NOT NULL, cost_usd REAL NOT NULL, timestamp INTEGER NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("create usage table");
    sqlx::query("INSERT INTO session (id) VALUES ('parent-session')")
        .execute(&pool)
        .await
        .expect("seed durable parent session");
    for run_id in ["run-a", "run-b"] {
        sqlx::query("INSERT INTO agent_run (run_id) VALUES (?)")
            .bind(run_id)
            .execute(&pool)
            .await
            .expect("seed durable child run");
    }
    let store = UsageStore::new(pool.clone());

    for (id, run_id, input, output, cached, cost) in [
        ("usage-a1", "run-a", 100, 20, 30, 0.01),
        ("usage-a2", "run-a", 50, 10, 0, 0.02),
        ("usage-b1", "run-b", 900, 90, 0, 0.50),
    ] {
        store
            .insert(UsageRecord {
                id: id.to_string(),
                session_id: "parent-session".to_string(),
                agent_run_id: Some(run_id.to_string()),
                provider: "fixture".to_string(),
                model: "fixture-model".to_string(),
                input_tokens: input,
                output_tokens: output,
                cached_tokens: cached,
                cost_usd: cost,
                timestamp: 1,
            })
            .await
            .expect("insert usage row");
    }

    let run_a = store.get_agent_run_cost_summary("run-a").await.unwrap();
    assert_eq!((run_a.0, run_a.1, run_a.2), (150, 30, 30));
    assert!((run_a.3 - 0.03).abs() < 1e-12);

    let run_b = store.get_agent_run_cost_summary("run-b").await.unwrap();
    assert_eq!((run_b.0, run_b.1, run_b.2), (900, 90, 0));
    assert!((run_b.3 - 0.50).abs() < 1e-12);
    assert_eq!(
        store
            .get_agent_run_cost_summary("missing-run")
            .await
            .unwrap(),
        (0, 0, 0, 0.0)
    );
    pool.close().await;
}
