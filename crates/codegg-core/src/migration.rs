//! Migration tooling for importing legacy project-local session
//! databases into the user-scoped daemon catalog.
//!
//! Phase 3 introduces a deliberate ownership split: the daemon catalog
//! lives in the user-scoped data directory while workspace-local
//! artifacts (RunStore, test runs, etc.) continue to live under
//! `<workspace>/.codegg/`. Legacy project-local session databases
//! (`<workspace>/.codegg/sessions.db`) must be imported into the
//! catalog explicitly rather than silently relocated.
//!
//! ## Idempotency
//!
//! Each migration records a provenance marker (`migration_marker` table
//! keyed by source path) so repeated calls do not duplicate imports.
//! The source database is NEVER modified or deleted by this module.
//! Removal is left to the caller via an explicit cleanup command.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use tracing::info;

use crate::error::StorageError;
use crate::project_storage::ProjectStorage;
use crate::session::{MessageStore, SessionStore};
use crate::workspace::{WorkspaceRecord, WorkspaceRegistry};

/// Outcome of a single migration run. Always idempotent: repeated
/// invocations against the same source return `AlreadyMigrated`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// Sessions and messages were imported into the catalog.
    Imported { sessions: usize, messages: usize },
    /// The source path was already imported previously.
    AlreadyMigrated,
    /// The source database does not exist; nothing to do.
    SourceMissing,
    /// The source path exists but is not a Codegg session database.
    /// Migration is refused to avoid clobbering unrelated SQLite files.
    InvalidSchema(String),
}

/// Discover a legacy project-local session database rooted at
/// `<project_root>/.codegg/sessions.db`. Returns `None` if no such
/// file exists.
pub fn find_legacy_project_db(project_root: &Path) -> Option<PathBuf> {
    let path = project_root.join(".codegg").join("sessions.db");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Verify that a SQLite database has the minimum Codegg session schema
/// (presence of the `session` table).
async fn verify_session_schema(pool: &SqlitePool) -> Result<bool, StorageError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'session'")
            .fetch_optional(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
    Ok(row.is_some())
}

/// Ensure the `migration_marker` table exists in the destination
/// catalog. Idempotent.
pub async fn ensure_migration_marker_table(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS migration_marker (\
            source_path TEXT PRIMARY KEY, \
            imported_at INTEGER NOT NULL, \
            sessions_count INTEGER NOT NULL, \
            messages_count INTEGER NOT NULL, \
            storage_layout_version INTEGER NOT NULL\
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?;
    Ok(())
}

/// Look up a previously-recorded migration for `source_path`. Returns
/// `Some(record)` if a row exists.
pub async fn fetch_marker(
    pool: &SqlitePool,
    source_path: &str,
) -> Result<Option<MigrationMarker>, StorageError> {
    let row: Option<(String, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT source_path, imported_at, sessions_count, messages_count, storage_layout_version \
         FROM migration_marker WHERE source_path = ?",
    )
    .bind(source_path)
    .fetch_optional(pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?;
    Ok(row.map(
        |(path, imported_at, sessions, messages, version)| MigrationMarker {
            source_path: path,
            imported_at: DateTime::<Utc>::from_timestamp_millis(imported_at)
                .unwrap_or_else(Utc::now),
            sessions_count: sessions as usize,
            messages_count: messages as usize,
            storage_layout_version: version as u32,
        },
    ))
}

/// Provenance marker written to the catalog after a successful import.
#[derive(Debug, Clone)]
pub struct MigrationMarker {
    pub source_path: String,
    pub imported_at: DateTime<Utc>,
    pub sessions_count: usize,
    pub messages_count: usize,
    pub storage_layout_version: u32,
}

/// Import a legacy project-local session database into the daemon
/// catalog. Idempotent: repeated invocations against the same source
/// return [`MigrationOutcome::AlreadyMigrated`].
///
/// The source database is NEVER modified. The workspace registry is
/// updated to include (or reuse) the project root's workspace record.
pub async fn migrate_legacy_project_database(
    catalog_pool: SqlitePool,
    workspace_registry: Arc<WorkspaceRegistry>,
    project_root: &Path,
) -> Result<MigrationOutcome, StorageError> {
    let Some(source_path) = find_legacy_project_db(project_root) else {
        return Ok(MigrationOutcome::SourceMissing);
    };

    let source_str = source_path.to_string_lossy().into_owned();
    ensure_migration_marker_table(&catalog_pool).await?;
    if fetch_marker(&catalog_pool, &source_str).await?.is_some() {
        return Ok(MigrationOutcome::AlreadyMigrated);
    }

    // Open a separate connection to the source.
    let source_pool = crate::storage::init_legacy_project_store(project_root).await?;
    if !verify_session_schema(&source_pool).await? {
        return Ok(MigrationOutcome::InvalidSchema(source_str));
    }

    // Resolve or create the workspace record for this project root.
    let workspace = workspace_registry
        .get_or_register(project_root)
        .await
        .map_err(|e| StorageError::Database(format!("workspace resolution: {}", e)))?;

    // Gather bounded local repository evidence and establish canonical
    // project/workspace authority before importing sessions. The legacy
    // `project_id` and `directory` values below remain compatibility writes;
    // they are never used as canonical identity input.
    let project_storage = ProjectStorage::new(catalog_pool.clone());
    let canonical_workspace = project_storage
        .reconcile_workspace_path(&workspace, "legacy_project_import")
        .await
        .map_err(|e| StorageError::Database(format!("canonical workspace binding: {}", e)))?;

    let source_session_store = SessionStore::new(source_pool.clone());
    let source_msg_store = MessageStore::new(source_pool.clone());

    let sessions = source_session_store
        .list_all_sessions(None)
        .await
        .map_err(|e| StorageError::Database(format!("list source sessions: {}", e)))?;

    let mut imported_sessions = 0usize;
    let mut imported_messages = 0usize;

    let dest_session_store = SessionStore::new(catalog_pool.clone());
    let dest_msg_store = MessageStore::new(catalog_pool.clone());

    for session in sessions {
        // Skip if a session with the same id already exists in the
        // catalog.
        let exists = dest_session_store
            .get(&session.id)
            .await
            .map_err(|e| StorageError::Database(format!("check existing session: {}", e)))?
            .is_some();
        if exists {
            project_storage
                .bind_session(
                    &session.id,
                    &canonical_workspace.binding.project_id,
                    &canonical_workspace.binding.workspace_id,
                    "legacy_project_import",
                )
                .await
                .map_err(|e| StorageError::Database(format!("bind existing session: {}", e)))?;
            continue;
        }

        let messages = source_msg_store
            .list(&session.id)
            .await
            .map_err(|e| StorageError::Database(format!("list source messages: {}", e)))?;
        imported_messages += messages.len();

        // Insert the session into the catalog. The session id is
        // preserved so `SessionLoad` semantics remain identical.
        dest_session_store
            .create_with_id(
                &session.id,
                crate::session::CreateSession {
                    // Preserve the source compatibility projections. These
                    // fields are never used as canonical identity authority;
                    // the binding record written below is authoritative.
                    project_id: session.project_id.clone(),
                    directory: session.directory.clone(),
                    title: Some(session.title.clone()),
                    parent_id: session.parent_id.clone(),
                    workspace_id: Some(workspace.id.as_str().to_string()),
                    agent: None,
                    model: None,
                    tags: if session.tags.is_empty() {
                        None
                    } else {
                        Some(session.tags.clone())
                    },
                },
            )
            .await
            .map_err(|e| StorageError::Database(format!("create session: {}", e)))?;
        project_storage
            .bind_session(
                &session.id,
                &canonical_workspace.binding.project_id,
                &canonical_workspace.binding.workspace_id,
                "legacy_project_import",
            )
            .await
            .map_err(|e| StorageError::Database(format!("bind session: {}", e)))?;
        imported_sessions += 1;

        for msg in &messages {
            let data = serde_json::to_value(&msg.data)
                .map_err(|e| StorageError::Database(format!("encode message data: {}", e)))?;
            dest_msg_store
                .create_with_id(&msg.id, &session.id, data)
                .await
                .map_err(|e| StorageError::Database(format!("insert message: {}", e)))?;
        }
    }

    sqlx::query(
        "INSERT INTO migration_marker (source_path, imported_at, sessions_count, messages_count, storage_layout_version) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&source_str)
    .bind(Utc::now().timestamp_millis())
    .bind(imported_sessions as i64)
    .bind(imported_messages as i64)
    .bind(crate::storage::STORAGE_LAYOUT_VERSION as i64)
    .execute(&catalog_pool)
    .await
    .map_err(|e| StorageError::Database(format!("record migration marker: {}", e)))?;

    info!(
        "migrated {} sessions / {} messages from {} into the daemon catalog",
        imported_sessions, imported_messages, source_str
    );

    Ok(MigrationOutcome::Imported {
        sessions: imported_sessions,
        messages: imported_messages,
    })
}

/// Discover all migration markers previously recorded in the catalog.
pub async fn list_migration_markers(
    catalog_pool: &SqlitePool,
) -> Result<Vec<MigrationMarker>, StorageError> {
    ensure_migration_marker_table(catalog_pool).await?;
    let rows: Vec<(String, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT source_path, imported_at, sessions_count, messages_count, storage_layout_version \
         FROM migration_marker ORDER BY imported_at DESC",
    )
    .fetch_all(catalog_pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?;
    Ok(rows
        .into_iter()
        .map(
            |(path, imported_at, sessions, messages, version)| MigrationMarker {
                source_path: path,
                imported_at: DateTime::<Utc>::from_timestamp_millis(imported_at)
                    .unwrap_or_else(Utc::now),
                sessions_count: sessions as usize,
                messages_count: messages as usize,
                storage_layout_version: version as u32,
            },
        )
        .collect())
}

/// A workspace record re-exported through a public helper so callers
/// that do not yet depend on the registry API can still consume
/// migration output.
pub fn describe_workspace(record: &WorkspaceRecord) -> WorkspaceDescription {
    WorkspaceDescription {
        workspace_id: record.id.as_str().to_string(),
        canonical_root: record.canonical_root.clone(),
        display_name: record.display_name.clone(),
        created_at: record.created_at,
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceDescription {
    pub workspace_id: String,
    pub canonical_root: PathBuf,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::schema::migrate;
    use crate::workspace::InMemoryWorkspaceStore;
    use std::str::FromStr;
    use std::time::Duration;

    async fn empty_pool() -> SqlitePool {
        // Use a fresh in-memory pool per test. With max_connections=1
        // and no shared cache, the pool is fully isolated from other
        // tests in the same process.
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = "file::memory:?cache=private";
        let opts = SqliteConnectOptions::from_str(url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect in-memory sqlite");
        migrate(&pool).await.expect("migrate");
        pool
    }

    #[tokio::test(flavor = "current_thread")]
    async fn source_missing_is_reported() {
        let catalog = empty_pool().await;
        let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let outcome = migrate_legacy_project_database(catalog, registry, tmp.path())
            .await
            .unwrap();
        assert_eq!(outcome, MigrationOutcome::SourceMissing);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalid_schema_is_rejected() {
        let catalog = empty_pool().await;
        let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let codegg_dir = tmp.path().join(".codegg");
        std::fs::create_dir_all(&codegg_dir).unwrap();
        // Write an unrelated SQLite file.
        let pool = crate::storage::init_pool_at(&codegg_dir.join("sessions.db"))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE unrelated (id INTEGER)")
            .execute(&pool)
            .await
            .unwrap();
        drop(pool);
        let outcome = migrate_legacy_project_database(catalog, registry, tmp.path())
            .await
            .unwrap();
        match outcome {
            MigrationOutcome::InvalidSchema(_) => {}
            other => panic!("expected InvalidSchema, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn idempotent_marker_is_recorded() {
        let catalog = empty_pool().await;
        let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let codegg_dir = tmp.path().join(".codegg");
        std::fs::create_dir_all(&codegg_dir).unwrap();
        let source_path = codegg_dir.join("sessions.db");
        // Use a single-connection pool so the BEGIN IMMEDIATE in the
        // session schema migration does not race with a second
        // connection from the same pool.
        {
            use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
            use std::str::FromStr;
            let opts =
                SqliteConnectOptions::from_str(&format!("sqlite://{}", source_path.display()))
                    .expect("sqlite options")
                    .create_if_missing(true);
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(5))
                .connect_with(opts)
                .await
                .expect("connect");
            sqlx::query("PRAGMA journal_mode=DELETE; PRAGMA busy_timeout=5000;")
                .execute(&pool)
                .await
                .unwrap();
            migrate(&pool).await.unwrap();
            sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(&pool)
                .await
                .ok();
            pool.close().await;
        }
        let outcome = migrate_legacy_project_database(catalog.clone(), registry, tmp.path())
            .await
            .unwrap();
        match outcome {
            MigrationOutcome::Imported { sessions, .. } => assert_eq!(sessions, 0),
            other => panic!("expected Imported (0 sessions), got {:?}", other),
        }
        // A second invocation against the SAME catalog and the same
        // source database must report AlreadyMigrated because the
        // marker row was written by the previous call.
        let registry2 = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .unwrap();
        let outcome2 = migrate_legacy_project_database(catalog, registry2, tmp.path())
            .await
            .unwrap();
        assert_eq!(outcome2, MigrationOutcome::AlreadyMigrated);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn import_preserves_session_and_message_ids_and_adds_canonical_binding() {
        let catalog = empty_pool().await;
        let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let source_path = tmp.path().join(".codegg").join("sessions.db");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        let source_options = sqlx::sqlite::SqliteConnectOptions::from_str(&format!(
            "sqlite://{}",
            source_path.display()
        ))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);
        let source = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(source_options)
            .await
            .unwrap();
        migrate(&source).await.unwrap();
        let source_session = SessionStore::new(source.clone())
            .create(crate::session::CreateSession {
                project_id: "legacy-project".to_string(),
                directory: tmp.path().display().to_string(),
                title: Some("Imported".to_string()),
                parent_id: None,
                workspace_id: None,
                agent: None,
                model: None,
                tags: None,
            })
            .await
            .unwrap();
        let source_message = MessageStore::new(source.clone())
            .create(
                &source_session.id,
                serde_json::json!({
                    "id": "source-message",
                    "sessionID": source_session.id,
                    "messageID": "source-message",
                    "parts": []
                }),
            )
            .await
            .unwrap();
        source.close().await;

        let outcome = migrate_legacy_project_database(catalog.clone(), registry, tmp.path())
            .await
            .unwrap();
        assert_eq!(
            outcome,
            MigrationOutcome::Imported {
                sessions: 1,
                messages: 1
            }
        );

        let imported_session: (String, String, Option<String>) =
            sqlx::query_as("SELECT id, project_id, workspace_id FROM session WHERE id = ?")
                .bind(&source_session.id)
                .fetch_one(&catalog)
                .await
                .unwrap();
        assert_eq!(imported_session.0, source_session.id);
        assert_eq!(imported_session.1, "legacy-project");
        assert!(imported_session.2.is_some());

        let imported_message: (String,) =
            sqlx::query_as("SELECT id FROM message WHERE session_id = ?")
                .bind(&source_session.id)
                .fetch_one(&catalog)
                .await
                .unwrap();
        assert_eq!(imported_message.0, source_message.id);

        let canonical: (String, String, String) = sqlx::query_as(
            "SELECT b.project_id, b.workspace_id, b.status FROM session_project_binding b WHERE b.session_id = ?",
        )
        .bind(&source_session.id)
        .fetch_one(&catalog)
        .await
        .unwrap();
        assert_ne!(canonical.0, imported_session.1);
        assert_eq!(canonical.1, imported_session.2.unwrap());
        assert_eq!(canonical.2, "resolved");
    }
}
