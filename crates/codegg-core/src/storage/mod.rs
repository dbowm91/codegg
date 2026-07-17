//! SQLite database storage layer.
//!
//! This module provides the Database wrapper around SQLite for persistent storage
//! of sessions, messages, and analytics. It uses sqlx for async database operations.
//!
//! ## Storage ownership (Phase 3)
//!
//! The daemon's SQLite database is split into two distinct concerns:
//!
//! 1. **User-scoped daemon catalog** -- owns workspace records, session
//!    catalog and messages, notification history, durable jobs (Phase 4+),
//!    and daemon-global metadata. Created via [`init_daemon_catalog`].
//! 2. **Workspace-local legacy project store** -- a project-rooted
//!    `<workspace>/.codegg/sessions.db` retained for backward compat
//!    with existing sessions. Initialized via
//!    [`init_legacy_project_store`].
//!
//! The legacy [`init`] entry point remains as a deprecated wrapper so
//! callers that have not yet been migrated continue to work.

pub mod paths;
pub mod preferences;
pub use paths::DaemonPaths;
pub use preferences::{UserPreferences, KEY_MODEL_LAST_USED, KEY_THEME_ACTIVE};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::error::StorageError;

/// Storage layout marker. Bumped whenever the on-disk schema or
/// directory layout changes in a way that requires a deliberate
/// migration path. Existing project-local session databases can be
/// discovered and imported into the daemon catalog using this marker.
pub const STORAGE_LAYOUT_VERSION: u32 = 26;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(path: &str) -> Result<Self, StorageError> {
        let pool = connect_and_configure(path).await?;
        crate::session::schema::migrate(&pool).await?;

        let db = Self { pool };

        db.try_checkpoint_wal().await;

        db.spawn_background_integrity_check().await;

        Ok(db)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), StorageError> {
        crate::session::schema::migrate(&self.pool).await
    }

    pub async fn health_check(&self) -> Result<(), StorageError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn close(self) {
        if let Err(e) = self.checkpoint_wal().await {
            warn!("WAL checkpoint on close failed: {}", e);
        }
        self.pool.close().await;
    }

    async fn checkpoint_wal(&self) -> Result<(), StorageError> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(format!("WAL checkpoint failed: {}", e)))?;
        Ok(())
    }

    async fn try_checkpoint_wal(&self) {
        if let Err(e) = self.checkpoint_wal().await {
            debug!("WAL checkpoint (non-fatal): {}", e);
        }
    }

    async fn spawn_background_integrity_check(&self) {
        let pool = self.pool.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if let Err(e) = run_quick_integrity_check(&pool).await {
                warn!("database integrity check warning: {}", e);
            }
        });
    }
}

/// Initialize the user-scoped daemon catalog database. The catalog owns
/// workspace records, session catalog and messages, notification
/// history, and Phase 4+ durable job metadata. It does NOT own
/// workspace-local artifacts such as RunStore data.
///
/// Use the supplied [`DaemonPaths`] to resolve platform-appropriate
/// locations. The default paths follow the operating system's
/// user-data conventions:
/// - macOS: `~/Library/Application Support/codegg/codegg.db`
/// - Linux: `$XDG_DATA_HOME/codegg/codegg.db` (or `~/.local/share/`)
pub async fn init_daemon_catalog(paths: &DaemonPaths) -> Result<SqlitePool, StorageError> {
    let db_path = paths.catalog_db_path();
    init_pool_at(&db_path).await
}

/// Initialize a legacy project-local SQLite database at
/// `<project_root>/.codegg/sessions.db`. Retained for backward compat
/// and for the migration tooling that imports legacy project databases
/// into the daemon catalog.
///
/// New code MUST NOT use this for production storage; production
/// daemons use [`init_daemon_catalog`] instead.
pub async fn init_legacy_project_store(project_root: &Path) -> Result<SqlitePool, StorageError> {
    let db_path = project_root.join(".codegg").join("sessions.db");
    init_pool_at(&db_path).await
}

/// Initialize a SQLite pool at the given path, ensuring the parent
/// directory exists and applying the standard pragmas.
pub async fn init_pool_at(db_path: &Path) -> Result<SqlitePool, StorageError> {
    let dir = db_path.parent().ok_or_else(|| {
        StorageError::Database(format!("invalid database path: {}", db_path.display()))
    })?;

    debug!("initializing database at: {}", db_path.display());

    if !dir.exists() {
        info!("creating database directory: {}", dir.display());
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            StorageError::Database(format!(
                "failed to create database directory {}: {}",
                dir.display(),
                e
            ))
        })?;
    }

    let dir_metadata = tokio::fs::metadata(dir).await.map_err(|e| {
        StorageError::Database(format!(
            "cannot access database directory {}: {}",
            dir.display(),
            e
        ))
    })?;

    if dir_metadata.permissions().readonly() {
        return Err(StorageError::Database(format!(
            "database directory {} is read-only",
            dir.display()
        )));
    }

    let db_path_str = db_path.to_string_lossy().to_string();

    let pool = connect_and_configure(&db_path_str).await?;

    info!(
        "database initialized successfully at: {}",
        db_path.display()
    );

    Ok(pool)
}

async fn connect_and_configure(path: &str) -> Result<SqlitePool, StorageError> {
    let options = SqliteConnectOptions::from_str(path)
        .map_err(|e| StorageError::Database(format!("invalid database path {}: {}", path, e)))?
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(options)
        .await
        .map_err(|e| StorageError::Database(format!("failed to open database {}: {}", path, e)))?;

    sqlx::query(
        r#"
        PRAGMA journal_mode=WAL;
        PRAGMA wal_autocheckpoint = 1000;
        PRAGMA busy_timeout=5000;
        PRAGMA synchronous = NORMAL;
        PRAGMA mmap_size = 268435456;
        PRAGMA cache_size = -2000;
        PRAGMA temp_store = MEMORY;
        PRAGMA foreign_keys = ON;
        "#,
    )
    .execute(&pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?;

    Ok(pool)
}

/// Legacy, ambiguous entry point retained for tests and standalone
/// invocations. Production daemons use [`init_daemon_catalog`]. The
/// caller-supplied `project_dir` controls where the database file is
/// created; an empty string falls back to the user config directory.
///
/// This wrapper intentionally retains the same ambiguous
/// `<dir>/.codegg/sessions.db` layout so legacy callers do not need to
/// change at the call site. New code MUST NOT use this.
#[deprecated(
    since = "0.1.0",
    note = "Use init_daemon_catalog or init_legacy_project_store instead"
)]
pub async fn init(project_dir: &str) -> Result<SqlitePool, StorageError> {
    if project_dir.is_empty() {
        // Empty project_dir resolves to the user config directory,
        // matching the historical behavior.
        let dir = dirs::config_dir()
            .map(|d| d.join("codegg"))
            .unwrap_or_else(|| PathBuf::from(".codegg"));
        let db_path = dir.join("sessions.db");
        return init_pool_at(&db_path).await;
    }
    let path = std::path::PathBuf::from(project_dir);
    init_legacy_project_store(&path).await
}

async fn run_quick_integrity_check(pool: &SqlitePool) -> Result<(), StorageError> {
    let result: (String,) = sqlx::query_as("PRAGMA quick_check")
        .fetch_one(pool)
        .await
        .map_err(|e| StorageError::Database(format!("integrity check failed: {}", e)))?;

    if result.0 != "ok" {
        return Err(StorageError::Database(format!(
            "integrity check found issues: {}",
            result.0
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn init_daemon_catalog_creates_user_scoped_db() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DaemonPaths::with_overrides(Some(tmp.path().to_path_buf()), None);
        let pool = init_daemon_catalog(&paths).await.unwrap();
        // The catalog path is constructed from the override data root.
        let expected = tmp.path().join("codegg.db");
        assert!(
            expected.exists(),
            "catalog db should exist at {:?}",
            expected
        );
        let _: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn init_legacy_project_store_anchors_at_codegg_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = init_legacy_project_store(tmp.path()).await.unwrap();
        let expected = tmp.path().join(".codegg").join("sessions.db");
        assert!(
            expected.exists(),
            "legacy project db should exist at {:?}",
            expected
        );
        let _: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
    }
}
