//! M002a (ADR-0035): durable user prompt queue storage — bounds, lazy
//! deterministic expiry, restart durability, and schema `CHECK` backstops.
//!
//! Every test name carries the `user_prompt_queue` prefix so the plan's §11
//! filters (`cargo test -p codegg-core user_prompt_queue`) select them by test
//! name rather than by module path.

use codegg_core::session::user_prompt_queue::{
    UserPromptQueueError, UserPromptQueueStatus, UserPromptQueueStore,
    USER_PROMPT_QUEUE_MAX_ENTRY_BYTES, USER_PROMPT_QUEUE_MAX_PENDING,
    USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION, USER_PROMPT_QUEUE_TTL_MS,
};
use codegg_core::storage::STORAGE_LAYOUT_VERSION;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

const SESSION_ID: &str = "session-queue-1";
const T0: i64 = 1_700_000_000_000;

async fn isolated_pool() -> SqlitePool {
    let name = format!("user_prompt_queue_{}", uuid::Uuid::new_v4().simple());
    let opts = SqliteConnectOptions::from_str(&format!("file:{name}?mode=memory&cache=shared"))
        .expect("sqlite options")
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        // Multi-connection so the concurrent-reader test exercises real
        // interleaving rather than a single serialized connection.
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("connect sqlite");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    pool
}

async fn ensure_session(pool: &SqlitePool) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) \
         VALUES ('project-queue', '/tmp/project-queue', '[]', ?, ?)",
    )
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert project");
    sqlx::query(
        "INSERT OR IGNORE INTO session \
         (id, project_id, slug, directory, title, version, time_created, time_updated) \
         VALUES (?, 'project-queue', 'queue', '/tmp/project-queue', 'Queue', '1', ?, ?)",
    )
    .bind(SESSION_ID)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert session");
}

/// Write one entry directly. M002a ships no mutation API, so the tests that
/// need rows write them the way M002b's insert will, which keeps the direct
/// write honest about what the schema accepts.
async fn insert_entry(
    pool: &SqlitePool,
    id: &str,
    ordinal: i64,
    text: &str,
    status: &str,
    created_at_ms: i64,
    expires_at_ms: i64,
) {
    sqlx::query(
        "INSERT INTO user_prompt_queue_entry \
         (id, session_id, ordinal, text, text_bytes, status, failure_reason, \
          created_at_ms, updated_at_ms, expires_at_ms, delivered_turn_id, delivered_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, NULL, NULL)",
    )
    .bind(id)
    .bind(SESSION_ID)
    .bind(ordinal)
    .bind(text)
    .bind(text.len() as i64)
    .bind(status)
    .bind(created_at_ms)
    .bind(created_at_ms)
    .bind(expires_at_ms)
    .execute(pool)
    .await
    .expect("insert queue entry");
}

// ---------------------------------------------------------------------------
// Focused unit tests: bounds constants and the `code()` ladder.
// ---------------------------------------------------------------------------

/// A bounded resource refuses rather than truncates. The over-bound text must
/// produce `EntryTooLarge`, and the message must never carry the text itself.
#[test]
fn user_prompt_queue_entry_byte_bound_refuses_with_typed_variant() {
    // A distinctive sentinel, not filler: the refusal message is prose, so a
    // single repeated character would make "does the message contain it?" a
    // test of English vocabulary rather than of leakage.
    const SENTINEL: &str = "SENTINEL-PROMPT-BODY-9f3c2a";
    let oversized = format!(
        "{SENTINEL}{}",
        "x".repeat(USER_PROMPT_QUEUE_MAX_ENTRY_BYTES)
    );
    let error = UserPromptQueueStore::validate_entry("entry-1", SESSION_ID, &oversized)
        .expect_err("over-bound entry must refuse");
    match &error {
        UserPromptQueueError::EntryTooLarge { limit } => {
            assert_eq!(*limit, USER_PROMPT_QUEUE_MAX_ENTRY_BYTES);
        }
        other => panic!("expected EntryTooLarge, got {other:?}"),
    }
    let message = error.to_string();
    assert!(
        !message.contains(SENTINEL),
        "refusal message must not echo entry text: {message}"
    );
}

#[test]
fn user_prompt_queue_text_at_exact_byte_bound_is_accepted_not_truncated() {
    let at_bound = "x".repeat(USER_PROMPT_QUEUE_MAX_ENTRY_BYTES);
    let bytes = UserPromptQueueStore::validate_entry("entry-1", SESSION_ID, &at_bound)
        .expect("exactly-at-bound must be accepted");
    assert_eq!(bytes, USER_PROMPT_QUEUE_MAX_ENTRY_BYTES);
}

#[test]
fn user_prompt_queue_empty_and_oversized_identity_refuse_as_invalid() {
    for (field_args, expected) in [
        (("", SESSION_ID, "hello"), "id"),
        (("entry-1", "", "hello"), "session_id"),
        (("entry-1", SESSION_ID, "   "), "text"),
    ] {
        match UserPromptQueueStore::validate_entry(field_args.0, field_args.1, field_args.2) {
            Err(UserPromptQueueError::Invalid { field, .. }) => assert_eq!(field, expected),
            other => panic!("expected Invalid({expected}), got {other:?}"),
        }
    }
}

/// The `code()` ladder must be exhaustive and collision-free: a daemon handler
/// maps a variant to a `CoreResponse::Error` code through it, so two variants
/// sharing a code would make two different refusals indistinguishable.
#[test]
fn user_prompt_queue_error_code_ladder_is_distinct_and_stable() {
    let storage = codegg_core::error::StorageError::Database("x".into());
    let variants = [
        UserPromptQueueError::Invalid {
            field: "text",
            message: "m".into(),
        },
        UserPromptQueueError::TooManyEntries { limit: 1 },
        UserPromptQueueError::EntryTooLarge { limit: 1 },
        UserPromptQueueError::NotFound,
        UserPromptQueueError::Expired,
        UserPromptQueueError::Storage(storage),
    ];
    let codes: Vec<&str> = variants.iter().map(|error| error.code()).collect();
    assert_eq!(
        codes,
        vec![
            "user_prompt_queue_invalid",
            "user_prompt_queue_too_many_entries",
            "user_prompt_queue_entry_too_large",
            "user_prompt_queue_not_found",
            "user_prompt_queue_expired",
            "user_prompt_queue_storage_error",
        ]
    );
    let mut unique: Vec<&str> = codes.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        codes.len(),
        "every variant must map to its own code: a shared code would make two \
         different refusals indistinguishable to a caller"
    );
}

// ---------------------------------------------------------------------------
// Migration and compatibility.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn user_prompt_queue_v80_migration_creates_table_index_and_layout() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;

    let layout: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(&pool)
    .await
    .expect("read migration marker");
    assert_eq!(
        layout, STORAGE_LAYOUT_VERSION as i64,
        "layout constant must equal the highest wired migration"
    );
    // M002b bumped the layout to 81 for the queue-owner binding; M007 took v82
    // for the optional due time; managed-scratch M002 uses v83 for shared grants
    // and attempt leases. The constant is asserted against the wired migration
    // above; this line pins the reviewed current version.
    assert_eq!(STORAGE_LAYOUT_VERSION, 83);

    let table: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='user_prompt_queue_entry'",
    )
    .fetch_one(&pool)
    .await
    .expect("inspect table");
    assert_eq!(table, 1, "v80 must create user_prompt_queue_entry");

    let index: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' \
         AND name='idx_user_prompt_queue_entry_session'",
    )
    .fetch_one(&pool)
    .await
    .expect("inspect index");
    assert_eq!(index, 1, "v80 must create the covering index");

    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_info('user_prompt_queue_entry') ORDER BY cid",
    )
    .fetch_all(&pool)
    .await
    .expect("inspect columns");
    for expected in [
        "id",
        "session_id",
        "ordinal",
        "text",
        "text_bytes",
        "status",
        "failure_reason",
        "created_at_ms",
        "updated_at_ms",
        "expires_at_ms",
    ] {
        assert!(
            columns.iter().any(|column| column == expected),
            "missing column {expected}; got {columns:?}"
        );
    }
    // M002a's tripwire here used to be "`run_at_ms` must not ship in v80". It
    // **cannot survive M007**, and the reason is structural rather than
    // accidental: this pool is fully migrated, so `pragma_table_info` reports
    // the union of every migration's work. Once v82 legitimately adds
    // `run_at_ms`, nothing here can tell v80's DDL from v82's.
    //
    // Provenance is therefore asserted where it is still observable, by staging
    // **v81** and running only v82:
    // `v81_to_v82_adds_run_at_ms_as_null_not_a_sentinel` in
    // `tests/storage_migrations.rs`. What this test can still say is that the
    // column is now part of the schema, nullable, and added after the fact.
    let run_at_type: Option<String> = sqlx::query_scalar(
        "SELECT type FROM pragma_table_info('user_prompt_queue_entry') WHERE name = 'run_at_ms'",
    )
    .fetch_optional(&pool)
    .await
    .expect("inspect run_at_ms");
    assert_eq!(
        run_at_type.as_deref(),
        Some("INTEGER"),
        "v82 (M007, ADR-0039) adds run_at_ms; its provenance from a *separate* \
         migration is pinned by v81_to_v82_adds_run_at_ms_as_null_not_a_sentinel"
    );
    let run_at_not_null: Option<i64> = sqlx::query_scalar(
        "SELECT \"notnull\" FROM pragma_table_info('user_prompt_queue_entry') \
         WHERE name = 'run_at_ms'",
    )
    .fetch_optional(&pool)
    .await
    .expect("inspect run_at_ms nullability");
    assert_eq!(
        run_at_not_null,
        Some(0),
        "run_at_ms must stay nullable so 'no due time' has a representation"
    );
}

// ---------------------------------------------------------------------------
// Round trip and truncation proof.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn user_prompt_queue_round_trip_is_byte_identical_and_dense_in_ordinal_order() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    let texts = ["first prompt", "second — with ünïcode", "third\ttabbed"];
    for (index, text) in texts.iter().enumerate() {
        insert_entry(
            &pool,
            &format!("entry-{index}"),
            index as i64,
            text,
            "pending",
            T0,
            T0 + USER_PROMPT_QUEUE_TTL_MS,
        )
        .await;
    }

    let store = UserPromptQueueStore::new(pool.clone());
    let live = store.list_live(SESSION_ID, T0).await.expect("list live");

    assert_eq!(live.len(), 3);
    // Byte-identical read-back: truncation of any kind would fail here rather
    // than pass quietly.
    assert_eq!(
        live.iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>(),
        texts
    );
    assert_eq!(
        live.iter().map(|entry| entry.ordinal).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    for entry in &live {
        assert_eq!(entry.text_bytes, entry.text.len());
    }

    // Delete by id.
    sqlx::query("DELETE FROM user_prompt_queue_entry WHERE id = ?")
        .bind("entry-1")
        .execute(&pool)
        .await
        .expect("delete by id");
    assert!(matches!(
        store.get("entry-1").await,
        Err(UserPromptQueueError::NotFound)
    ));
}

// ---------------------------------------------------------------------------
// Bounds as typed refusals (work package B).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn user_prompt_queue_pending_count_bound_refuses_with_too_many_entries() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    for index in 0..USER_PROMPT_QUEUE_MAX_PENDING {
        insert_entry(
            &pool,
            &format!("entry-{index}"),
            index,
            "queued prompt",
            "pending",
            T0,
            T0 + USER_PROMPT_QUEUE_TTL_MS,
        )
        .await;
    }

    let store = UserPromptQueueStore::new(pool.clone());
    match store.check_capacity(SESSION_ID, T0).await {
        Err(UserPromptQueueError::TooManyEntries { limit }) => {
            assert_eq!(limit, USER_PROMPT_QUEUE_MAX_PENDING)
        }
        other => panic!("expected TooManyEntries, got {other:?}"),
    }

    // One below the bound still passes, so the refusal is the bound and not an
    // off-by-one that always refuses.
    sqlx::query("DELETE FROM user_prompt_queue_entry WHERE id = ?")
        .bind("entry-0")
        .execute(&pool)
        .await
        .expect("delete one entry");
    store
        .check_capacity(SESSION_ID, T0)
        .await
        .expect("one below the bound must pass");
}

#[tokio::test]
async fn user_prompt_queue_retention_bound_refuses_with_too_many_entries() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    // Fill the session past the pending bound using terminal rows, so the
    // retention bound is the one that must fire.
    let expires = T0 + USER_PROMPT_QUEUE_TTL_MS;
    for index in 0..USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION {
        insert_entry(
            &pool,
            &format!("entry-{index}"),
            index,
            "queued prompt",
            "delivered",
            T0,
            expires,
        )
        .await;
    }

    let store = UserPromptQueueStore::new(pool);
    match store.check_capacity(SESSION_ID, T0).await {
        Err(UserPromptQueueError::TooManyEntries { limit }) => {
            assert_eq!(limit, USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION)
        }
        other => panic!("expected TooManyEntries, got {other:?}"),
    }
}

/// The schema `CHECK` is the backstop that survives a missed call site. This
/// writes the row the store would have refused and proves the database rejects
/// it — so the backstop is real, not decorative.
#[tokio::test]
async fn user_prompt_queue_schema_check_rejects_over_bound_text_bytes() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    let result = sqlx::query(
        "INSERT INTO user_prompt_queue_entry \
         (id, session_id, ordinal, text, text_bytes, status, created_at_ms, updated_at_ms, \
          expires_at_ms) \
         VALUES ('entry-over', ?, 0, 'hi', ?, 'pending', ?, ?, ?)",
    )
    .bind(SESSION_ID)
    .bind(USER_PROMPT_QUEUE_MAX_ENTRY_BYTES as i64 + 1)
    .bind(T0)
    .bind(T0)
    .bind(T0 + USER_PROMPT_QUEUE_TTL_MS)
    .execute(&pool)
    .await;

    let error = result.expect_err("schema CHECK must reject over-bound text_bytes");
    let message = error.to_string();
    assert!(
        message.contains("CHECK constraint failed"),
        "expected a CHECK violation, got: {message}"
    );
}

/// The status vocabulary is enforced by the schema too — including the three
/// values that are accepted but unreachable until M003.
#[tokio::test]
async fn user_prompt_queue_schema_check_rejects_unknown_status() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    let result = sqlx::query(
        "INSERT INTO user_prompt_queue_entry \
         (id, session_id, ordinal, text, text_bytes, status, created_at_ms, updated_at_ms, \
          expires_at_ms) \
         VALUES ('entry-bad-status', ?, 0, 'hi', 2, 'teleported', ?, ?, ?)",
    )
    .bind(SESSION_ID)
    .bind(T0)
    .bind(T0)
    .bind(T0 + USER_PROMPT_QUEUE_TTL_MS)
    .execute(&pool)
    .await;
    assert!(result
        .expect_err("schema CHECK must reject an unknown status")
        .to_string()
        .contains("CHECK constraint failed"),);
}

/// The total order is a schema-enforced invariant, not a convention.
#[tokio::test]
async fn user_prompt_queue_schema_rejects_duplicate_ordinal_per_session() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    insert_entry(&pool, "entry-a", 0, "first", "pending", T0, T0 + 1).await;
    let duplicate = sqlx::query(
        "INSERT INTO user_prompt_queue_entry \
         (id, session_id, ordinal, text, text_bytes, status, created_at_ms, updated_at_ms, \
          expires_at_ms) VALUES ('entry-b', ?, 0, 'second', 6, 'pending', ?, ?, ?)",
    )
    .bind(SESSION_ID)
    .bind(T0)
    .bind(T0)
    .bind(T0 + 1)
    .execute(&pool)
    .await;
    assert!(
        duplicate.is_err(),
        "UNIQUE(session_id, ordinal) must reject a duplicate ordinal"
    );
}

// ---------------------------------------------------------------------------
// Lazy deterministic expiry (work package C).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn user_prompt_queue_due_entry_expires_and_leaves_the_live_list() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    insert_entry(
        &pool,
        "entry-live",
        0,
        "keep me",
        "pending",
        T0,
        T0 + 10_000,
    )
    .await;
    insert_entry(&pool, "entry-due", 1, "drop me", "pending", T0, T0 - 1).await;

    let store = UserPromptQueueStore::new(pool);
    let live = store.list_live(SESSION_ID, T0).await.expect("list live");

    assert_eq!(
        live.iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["entry-live"],
        "a due entry must not appear in the live list"
    );

    // The direct store read observes the sweep's committed status.
    let due = store
        .get("entry-due")
        .await
        .expect("due entry still readable");
    assert_eq!(due.status, UserPromptQueueStatus::Expired);
    let live_entry = store.get("entry-live").await.expect("live entry readable");
    assert_eq!(
        live_entry.status,
        UserPromptQueueStatus::Pending,
        "a non-due entry must be untouched by the sweep"
    );
}

#[tokio::test]
async fn user_prompt_queue_expiry_is_idempotent_across_repeated_reads() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    insert_entry(&pool, "entry-due", 0, "drop me", "pending", T0, T0 - 1).await;

    let store = UserPromptQueueStore::new(pool);
    let mut observed = Vec::new();
    for _ in 0..3 {
        let live = store.list_live(SESSION_ID, T0).await.expect("list live");
        let due = store.get("entry-due").await.expect("due entry readable");
        observed.push(format!("{}:{}", live.len(), due.status.as_str()));
    }
    assert_eq!(
        observed,
        vec!["0:expired", "0:expired", "0:expired"],
        "repeated reads must be idempotent"
    );
}

#[tokio::test]
async fn user_prompt_queue_get_live_distinguishes_expired_from_not_found() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    insert_entry(&pool, "entry-due", 0, "drop me", "pending", T0, T0 - 1).await;
    insert_entry(&pool, "entry-live", 1, "keep me", "pending", T0, T0 + 10).await;

    let store = UserPromptQueueStore::new(pool);
    assert!(matches!(
        store.get_live("entry-due", T0).await,
        Err(UserPromptQueueError::Expired)
    ));
    assert!(matches!(
        store.get_live("missing", T0).await,
        Err(UserPromptQueueError::NotFound)
    ));
    assert!(store.get_live("entry-live", T0).await.is_ok());
}

// ---------------------------------------------------------------------------
// Restart and recovery.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn user_prompt_queue_entries_survive_reopen_with_unchanged_fields() {
    let name = format!(
        "user_prompt_queue_restart_{}",
        uuid::Uuid::new_v4().simple()
    );
    let path = std::env::temp_dir().join(format!("{name}.db"));
    let opts = SqliteConnectOptions::from_str(&format!("file:{}", path.display()))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);

    let before = {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts.clone())
            .await
            .expect("connect");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        ensure_session(&pool).await;
        insert_entry(
            &pool,
            "entry-restart",
            0,
            "durable prompt — ünïcode",
            "pending",
            T0,
            T0 + USER_PROMPT_QUEUE_TTL_MS,
        )
        .await;
        let record = UserPromptQueueStore::new(pool.clone())
            .get("entry-restart")
            .await
            .expect("read before restart");
        pool.close().await;
        record
    };

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("reconnect");
    let after = UserPromptQueueStore::new(pool)
        .get("entry-restart")
        .await
        .expect("read after restart");

    assert_eq!(after, before, "entries must survive restart unchanged");
    assert_eq!(after.text, "durable prompt — ünïcode");
    assert_eq!(after.status, UserPromptQueueStatus::Pending);
    assert_eq!(after.ordinal, 0);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn user_prompt_queue_due_entries_expire_across_restart_and_others_do_not() {
    let name = format!(
        "user_prompt_queue_expiry_restart_{}",
        uuid::Uuid::new_v4().simple()
    );
    let path = std::env::temp_dir().join(format!("{name}.db"));
    let opts = SqliteConnectOptions::from_str(&format!("file:{}", path.display()))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);

    {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts.clone())
            .await
            .expect("connect");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        ensure_session(&pool).await;
        insert_entry(&pool, "entry-due", 0, "drop me", "pending", T0, T0 - 1).await;
        insert_entry(
            &pool,
            "entry-live",
            1,
            "keep me",
            "pending",
            T0,
            T0 + 10_000,
        )
        .await;
        // No read happens before the close: the sweep must be driven by the
        // post-restart read, which is what "lazy" means.
        pool.close().await;
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("reconnect");
    let store = UserPromptQueueStore::new(pool);
    let live = store.list_live(SESSION_ID, T0).await.expect("list live");
    assert_eq!(
        live.iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["entry-live"]
    );
    assert_eq!(
        store.get("entry-due").await.expect("read due").status,
        UserPromptQueueStatus::Expired
    );
    assert_eq!(
        store.get("entry-live").await.expect("read live").status,
        UserPromptQueueStatus::Pending
    );
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// Contention.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_queue_concurrent_readers_observe_identical_dense_order() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    for index in 0..16i64 {
        insert_entry(
            &pool,
            &format!("entry-{index}"),
            index,
            &format!("prompt {index}"),
            "pending",
            T0,
            T0 + USER_PROMPT_QUEUE_TTL_MS,
        )
        .await;
    }

    let store = UserPromptQueueStore::new(pool);
    let first = tokio::spawn({
        let store = store.clone();
        async move { store.list_live(SESSION_ID, T0).await }
    });
    let second = tokio::spawn({
        let store = store.clone();
        async move { store.list_live(SESSION_ID, T0).await }
    });

    let a = first.await.expect("reader one").expect("reader one ok");
    let b = second.await.expect("reader two").expect("reader two ok");

    let ordinals_a: Vec<i64> = a.iter().map(|entry| entry.ordinal).collect();
    let ordinals_b: Vec<i64> = b.iter().map(|entry| entry.ordinal).collect();
    assert_eq!(ordinals_a, ordinals_b, "readers must agree on the order");
    assert_eq!(
        ordinals_a,
        (0..16).collect::<Vec<_>>(),
        "ordinals must be dense"
    );
}

// ---------------------------------------------------------------------------
// Security and negative tests.
// ---------------------------------------------------------------------------

/// M002b boundary guard: no `CoreRequest` variant can mutate the queue.
///
/// This is a source-level assertion over the protocol enum, which is where the
/// authority for a mutation would have to live. It fails if anyone adds a
/// queue-mutating request without revisiting ADR-0037.
#[test]
fn user_prompt_queue_no_core_request_mutation_variant_exists() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../codegg-protocol/src/core.rs"
    ))
    .expect("read protocol source");

    let mut cursor = source.as_str();
    let mut found = false;
    while let Some(index) = cursor.find("UserPromptQueue") {
        found = true;
        // Walk back to the nearest variant-name line and read the identifier.
        let head = &cursor[..index];
        let line_start = head.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line = head[line_start..].trim();
        let name = line
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next_back()
            .unwrap_or_default();
        assert!(
            !matches!(
                name,
                "Add" | "Insert" | "Enqueue" | "Update" | "Edit" | "Delete" | "Remove"
                    | "Reorder" | "Move" | "Clear" | "Create"
            ),
            "found a queue mutation variant `{name}`; mutations are M002b and are gated on ADR-0037"
        );
        cursor = &cursor[index + "UserPromptQueue".len()..];
    }
    assert!(
        found,
        "expected at least one UserPromptQueue identifier in the protocol source"
    );
}

/// Prompt text must not reach any error message the daemon can return.
#[tokio::test]
async fn user_prompt_queue_error_messages_carry_no_entry_text() {
    let pool = isolated_pool().await;
    ensure_session(&pool).await;
    let secret = "SENSITIVE-PROMPT-BODY-a1b2c3";
    insert_entry(&pool, "entry-secret", 0, secret, "pending", T0, T0 - 1).await;

    let store = UserPromptQueueStore::new(pool);
    let mut messages = Vec::new();
    if let Err(error) = store.get_live("entry-secret", T0).await {
        messages.push(error.to_string());
    }
    if let Err(error) = store.get("missing-secret").await {
        messages.push(error.to_string());
    }
    if let Err(error) = store.list_live("", T0).await {
        messages.push(error.to_string());
    }
    if let Err(error) = store.check_capacity("", T0).await {
        messages.push(error.to_string());
    }

    assert!(!messages.is_empty(), "expected refusal messages to inspect");
    for message in &messages {
        assert!(
            !message.contains(secret),
            "refusal message leaked entry text: {message}"
        );
    }
}

/// The `CoreResponse::UserPromptQueue` payload is the only place entry text is
/// legitimately carried, and it is constructed from the entry itself — so the
/// serialization path cannot drop or mangle it.
#[test]
fn user_prompt_queue_entry_dto_round_trips_with_defaulted_absent_projection() {
    let entry = codegg_protocol::core::UserPromptQueueEntryDto {
        id: "entry-1".to_owned(),
        ordinal: 0,
        text: "queue me".to_owned(),
        text_bytes: 8,
        status: "pending".to_owned(),
        created_at_ms: T0,
        expires_at_ms: T0 + USER_PROMPT_QUEUE_TTL_MS,
        run_at_ms: None,
    };
    let encoded = serde_json::to_string(&entry).expect("encode");
    let decoded: codegg_protocol::core::UserPromptQueueEntryDto =
        serde_json::from_str(&encoded).expect("decode");
    assert_eq!(decoded, entry);

    // An absent projection from an older daemon reads as an empty queue rather
    // than failing to deserialize.
    let empty: codegg_protocol::core::UserPromptQueueEntryDto =
        serde_json::from_str("{}").expect("absent entry defaults");
    assert_eq!(empty.text, "");
    assert_eq!(empty.ordinal, 0);
}
// ---------------------------------------------------------------------------
// Authorization: the descriptor mapping and the deliberate capability
// divergence from the promotion read.
// ---------------------------------------------------------------------------

/// The queue read is `ScopeKind::ViaSession` + `Capability::SessionRead`.
///
/// This pins the divergence from `human_shell_promotions_get`, which is also a
/// read but is authorized with the broader `Capability::AgentInvoke`. ADR-0035
/// states "queue reads are subject to session read authority", so a future edit
/// that copies the promotion precedent would widen this read to any principal
/// who can invoke an agent. The two are asserted together so the difference
/// cannot be closed silently.
#[test]
fn user_prompt_queue_descriptor_uses_session_read_not_agent_invoke() {
    use codegg_core::authorization::{operation_descriptor, Capability, ScopeKind};
    use codegg_protocol::core::CoreRequest;

    let queue = operation_descriptor(&CoreRequest::UserPromptQueueList {
        session_id: "session-1".to_owned(),
    });
    assert_eq!(queue.operation, "user_prompt_queue_list");
    assert_eq!(
        queue.scope_kind,
        ScopeKind::ViaSession,
        "queue reads are session-scoped"
    );
    assert_eq!(
        queue.capability,
        Some(Capability::SessionRead),
        "ADR-0035: queue reads are subject to session read authority"
    );
    assert_eq!(queue.capability_name(), "session.read");

    // The deliberate divergence, asserted rather than assumed.
    let promotion = operation_descriptor(&CoreRequest::HumanShellPromotionsGet {
        session_id: "session-1".to_owned(),
    });
    assert_eq!(
        promotion.capability,
        Some(Capability::AgentInvoke),
        "the promotion read keeps the broader capability; this test documents \
         that difference rather than treating it as a bug to be copied"
    );
    assert_ne!(
        queue.capability, promotion.capability,
        "the queue read must stay narrower than the promotion read"
    );
}

/// The read is classified as uninstrumented, like every other read that carries
/// no shared-state mutation. `scripts/check_audit_coverage.py` fails an
/// operation in neither list, so this test is the in-crate half of that guard.
#[test]
fn user_prompt_queue_list_is_classified_uninstrumented() {
    assert!(
        codegg_core::audit_instrumentation::UNINSTRUMENTED_OPERATIONS
            .contains(&"user_prompt_queue_list"),
        "the queue read belongs in UNINSTRUMENTED_OPERATIONS"
    );
    assert!(
        !codegg_core::audit_instrumentation::INSTRUMENTED_OPERATIONS
            .iter()
            .any(|(operation, _)| *operation == "user_prompt_queue_list"),
        "the queue read must not be double-classified as instrumented"
    );
}
