use sqlx::sqlite::SqlitePoolOptions;
use sqlx::Row;

#[tokio::test]
async fn migration_rerun_resumes_after_mid_migration_failure() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("failed to connect test sqlite pool");

    // Seed migration table so we can inject a deterministic failure when
    // migration step 6 tries to record its completion.
    sqlx::query(
        r#"
        CREATE TABLE migration_version (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            version INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("failed to create migration_version table");

    sqlx::query(
        r#"
        CREATE TRIGGER fail_migration_v6
        BEFORE INSERT ON migration_version
        WHEN NEW.id = 1 AND NEW.version = 6
        BEGIN
            SELECT RAISE(ABORT, 'forced migration failure at v6');
        END;
        "#,
    )
    .execute(&pool)
    .await
    .expect("failed to create migration failure trigger");

    let err = codegg::session::schema::migrate(&pool)
        .await
        .expect_err("migration should fail at v6");
    let err_text = err.to_string();
    assert!(
        err_text.contains("forced migration failure at v6"),
        "unexpected migration error: {err_text}"
    );

    let recorded_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(&pool)
    .await
    .expect("failed to read recorded migration version");
    assert_eq!(
        recorded_version, 5,
        "expected completed versions to persist before injected failure"
    );

    sqlx::query("DROP TRIGGER fail_migration_v6")
        .execute(&pool)
        .await
        .expect("failed to drop migration failure trigger");

    codegg::session::schema::migrate(&pool)
        .await
        .expect("migration rerun should resume and finish");

    let final_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(&pool)
    .await
    .expect("failed to read final migration version");
    assert_eq!(
        final_version,
        codegg::storage::STORAGE_LAYOUT_VERSION as i64,
        "expected latest migration version"
    );

    let allowed_paths_exists: i64 = sqlx::query(
        "SELECT COUNT(*) AS cnt FROM pragma_table_info('task') WHERE name = 'allowed_paths'",
    )
    .fetch_one(&pool)
    .await
    .expect("failed to inspect task schema")
    .get("cnt");
    assert_eq!(
        allowed_paths_exists, 1,
        "expected v14 schema change to be present"
    );
}

#[tokio::test]
async fn domain_identity_v25_is_additive_and_indexed() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("failed to connect test sqlite pool");
    codegg::session::schema::migrate(&pool)
        .await
        .expect("migration should create v25");

    sqlx::query(
        "INSERT INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES ('legacy-project', '/legacy/root', '[]', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('legacy-session', 'legacy-project', 'legacy', '/legacy/root', 'Legacy', '1', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    for table in [
        "logical_project",
        "repository",
        "project_repository",
        "workspace_project_binding",
        "session_project_binding",
        "identity_diagnostic",
    ] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(exists, 1, "missing canonical table {table}");
    }
    let legacy: (String, String) =
        sqlx::query_as("SELECT project_id, directory FROM session WHERE id = 'legacy-session'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        legacy,
        ("legacy-project".to_string(), "/legacy/root".to_string())
    );
    let index_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%project%binding%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(index_count >= 3, "expected binding lookup indexes");
}

#[tokio::test]
async fn provider_connections_v26_is_additive_and_secret_free() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("failed to connect test sqlite pool");
    codegg::session::schema::migrate(&pool)
        .await
        .expect("migration should create v26");

    for table in [
        "provider_connections",
        "provider_provisioning",
        "provider_connection_health",
        "provider_connection_models",
    ] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(exists, 1, "missing provider table {table}");
    }

    let provisioning_columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('provider_provisioning')")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!provisioning_columns.iter().any(|column| {
        matches!(
            column.as_str(),
            "api_key" | "credential" | "ciphertext" | "plaintext"
        )
    }));

    let health_columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('provider_connection_health')")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(health_columns
        .iter()
        .any(|column| column == "catalog_revision"));
}

#[tokio::test]
async fn project_catalog_v28_and_discovery_v29_are_additive_and_idempotent() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("failed to connect test sqlite pool");
    codegg::session::schema::migrate(&pool)
        .await
        .expect("migration should create v29");

    for table in [
        "project_locator",
        "project_health",
        "legacy_catalog_association_marker",
        "discovery_root",
        "discovery_scan",
        "discovery_observation",
    ] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(exists, 1, "missing catalog table {table}");
    }

    let logical_project_columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('logical_project')")
            .fetch_all(&pool)
            .await
            .unwrap();
    for column in [
        "archived_at",
        "description",
        "tags",
        "registration_source",
        "time_last_opened",
    ] {
        assert!(
            logical_project_columns.iter().any(|c| c == column),
            "missing logical_project column {column}"
        );
    }

    let locator_columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('project_locator')")
            .fetch_all(&pool)
            .await
            .unwrap();
    for column in [
        "locator_kind",
        "workspace_id",
        "canonical_root",
        "ssh_host",
        "linked_node_id",
        "display_summary",
    ] {
        assert!(
            locator_columns.iter().any(|c| c == column),
            "missing project_locator column {column}"
        );
    }

    // Re-running the migration is idempotent: the migration_version table
    // is the authoritative gate so the version stays at the latest layout.
    codegg::session::schema::migrate(&pool)
        .await
        .expect("rerun migration should be a no-op past the latest version");
    let final_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(&pool)
    .await
    .expect("failed to read final migration version");
    assert_eq!(
        final_version,
        codegg::storage::STORAGE_LAYOUT_VERSION as i64,
        "expected version to remain current"
    );
}

/// M007 (ADR-0039): v81 -> v82 adds `run_at_ms` to the prompt queue entry.
///
/// This test stages the **v81** shape by hand and lets the real `migrate()` run
/// only v82, so what is asserted is the actual upgrade rather than a fresh
/// database that happens to have the column.
///
/// The load-bearing assertion is that pre-existing rows read back as SQL `NULL`
/// — "no due time" — and not as epoch `0`. A migration that backfilled `0`
/// would be silently correct for the eligibility predicate (`0 <= now` is true,
/// so such a row is immediately due) and wrong for the user, who would see every
/// pre-existing entry reported as scheduled. The two are indistinguishable in
/// the delivery query and only distinguishable here.
#[tokio::test]
async fn v81_to_latest_preserves_queue_and_adds_network_intent() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("failed to connect test sqlite pool");

    // Stage v81: the migration version marker, a session row for the foreign
    // key, and v80's queue table verbatim. `run_at_ms` is absent.
    sqlx::query(
        r#"
        CREATE TABLE migration_version (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            version INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("failed to create migration_version table");
    sqlx::query("INSERT INTO migration_version (id, version) VALUES (1, 81)")
        .execute(&pool)
        .await
        .expect("failed to seed v81 marker");

    sqlx::query("CREATE TABLE session (id TEXT PRIMARY KEY)")
        .execute(&pool)
        .await
        .expect("failed to create session table");
    sqlx::query("INSERT INTO session (id) VALUES ('session-v81')")
        .execute(&pool)
        .await
        .expect("failed to seed session row");

    // This v81 fixture predates the Linux M005 preference column but already
    // contains the preference table. Keep its schema minimal so the upgrade
    // proves the additive v83 migration runs after the queue v82 migration.
    sqlx::query(
        "CREATE TABLE runtime_preferences (principal_id TEXT PRIMARY KEY, revision INTEGER NOT NULL, updated_at INTEGER NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("failed to create pre-network-intent preferences table");

    sqlx::query(
        r#"
        CREATE TABLE user_prompt_queue_entry (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
            text TEXT NOT NULL,
            text_bytes INTEGER NOT NULL CHECK (text_bytes >= 0 AND text_bytes <= 65536),
            status TEXT NOT NULL CHECK (
                status IN ('pending', 'delivering', 'delivered', 'failed', 'expired')
            ),
            failure_reason TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            expires_at_ms INTEGER NOT NULL,
            delivered_turn_id TEXT,
            delivered_at_ms INTEGER,
            UNIQUE (session_id, ordinal),
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("failed to create v80 queue table");
    sqlx::query(
        r#"
        INSERT INTO user_prompt_queue_entry
            (id, session_id, ordinal, text, text_bytes, status, failure_reason,
             created_at_ms, updated_at_ms, expires_at_ms, delivered_turn_id, delivered_at_ms)
        VALUES
            ('pre-v82', 'session-v81', 0, 'queued before timers existed', 32,
             'pending', NULL, 1000, 1000, 900000, NULL, NULL)
        "#,
    )
    .execute(&pool)
    .await
    .expect("failed to seed pre-v82 queue entry");

    codegg::session::schema::migrate(&pool)
        .await
        .expect("latest migrations should apply over a v81 database");

    let final_version: i64 =
        sqlx::query_scalar("SELECT version FROM migration_version WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("failed to read final migration version");
    assert_eq!(
        final_version,
        codegg::storage::STORAGE_LAYOUT_VERSION as i64,
        "latest migration must land exactly at the declared storage layout version"
    );
    assert_eq!(final_version, 83, "the network-intent migration follows v82");

    let network_intent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('runtime_preferences') WHERE name = 'network_intent'",
    )
    .fetch_one(&pool)
    .await
    .expect("failed to inspect network-intent migration");
    assert_eq!(network_intent, 1, "v83 adds the nullable network intent");

    // The column exists and is nullable.
    let run_at_type: String = sqlx::query_scalar(
        "SELECT type FROM pragma_table_info('user_prompt_queue_entry') WHERE name = 'run_at_ms'",
    )
    .fetch_one(&pool)
    .await
    .expect("run_at_ms column missing after v82");
    assert_eq!(
        run_at_type, "INTEGER",
        "run_at_ms must be an INTEGER column"
    );
    let not_null: i64 = sqlx::query_scalar(
        "SELECT \"notnull\" FROM pragma_table_info('user_prompt_queue_entry') \
         WHERE name = 'run_at_ms'",
    )
    .fetch_one(&pool)
    .await
    .expect("failed to read run_at_ms notnull flag");
    assert_eq!(
        not_null, 0,
        "run_at_ms must stay nullable so 'no due time' has a representation"
    );

    // The load-bearing assertion: NULL, not a sentinel.
    let run_at: Option<i64> =
        sqlx::query_scalar("SELECT run_at_ms FROM user_prompt_queue_entry WHERE id = 'pre-v82'")
            .fetch_one(&pool)
            .await
            .expect("pre-v82 entry disappeared during upgrade");
    assert_eq!(
        run_at, None,
        "a pre-v82 entry must read as 'no due time', not as a sentinel such as epoch 0"
    );

    // The rest of the row survived untouched.
    let (status, ordinal, text): (String, i64, String) = sqlx::query_as(
        "SELECT status, ordinal, text FROM user_prompt_queue_entry WHERE id = 'pre-v82'",
    )
    .fetch_one(&pool)
    .await
    .expect("failed to re-read pre-v82 entry");
    assert_eq!(status, "pending");
    assert_eq!(ordinal, 0);
    assert_eq!(text, "queued before timers existed");

    // The sweep index exists, and is partial over scheduled pending rows only.
    let index_sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'index' \
         AND name = 'idx_user_prompt_queue_entry_due'",
    )
    .fetch_optional(&pool)
    .await
    .expect("failed to look up the due-time index")
    .expect("v82 must create idx_user_prompt_queue_entry_due");
    assert!(
        index_sql.contains("WHERE status = 'pending' AND run_at_ms IS NOT NULL"),
        "the due index must stay partial over scheduled pending rows, got: {index_sql}"
    );
}
