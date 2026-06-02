//! SQLite database storage layer.
//!
//! This module provides the Database wrapper around SQLite for persistent storage
//! of sessions, messages, and analytics. It uses sqlx for async database operations.

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::error::StorageError;

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

fn get_db_path(project_dir: &str) -> PathBuf {
    let dir = if !project_dir.is_empty() {
        PathBuf::from(project_dir).join(".codegg")
    } else {
        dirs::config_dir()
            .map(|d| d.join("codegg"))
            .unwrap_or_else(|| PathBuf::from(".codegg"))
    };

    dir.join("sessions.db")
}

async fn connect_and_configure(path: &str) -> Result<SqlitePool, StorageError> {
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(30))
        .connect(path)
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

pub async fn init(project_dir: &str) -> Result<SqlitePool, StorageError> {
    let db_path = get_db_path(project_dir);
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

    let dir_metadata = tokio::fs::metadata(dir).await
        .map_err(|e| {
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
