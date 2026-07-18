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
    assert_eq!(final_version, 29, "expected latest migration version");

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
    // is the authoritative gate so the version stays at 29.
    codegg::session::schema::migrate(&pool)
        .await
        .expect("rerun migration should be a no-op past v29");
    let final_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(&pool)
    .await
    .expect("failed to read final migration version");
    assert_eq!(final_version, 29, "expected version to remain at 29");
}
