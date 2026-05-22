//! SQLite database storage layer.
//!
//! This module provides the Database wrapper around SQLite for persistent storage
//! of sessions, messages, and analytics. It uses sqlx for async database operations.

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tracing::{debug, info};

use crate::error::StorageError;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(path: &str) -> Result<Self, StorageError> {
        let pool = connect_and_configure(path).await?;
        crate::session::schema::migrate(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), StorageError> {
        crate::session::schema::migrate(&self.pool).await
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

    if dir
        .metadata()
        .map_err(|e| {
            StorageError::Database(format!(
                "cannot access database directory {}: {}",
                dir.display(),
                e
            ))
        })?
        .permissions()
        .readonly()
    {
        return Err(StorageError::Database(format!(
            "database directory {} is read-only",
            dir.display()
        )));
    }

    let db_path_str = db_path.to_string_lossy().to_string();

    let pool = connect_and_configure(&db_path_str).await?;

    crate::session::schema::migrate(&pool).await?;

    info!(
        "database initialized successfully at: {}",
        db_path.display()
    );

    Ok(pool)
}
