//! Durable, revisioned session-relative default working directory.
//!
//! This stores only a relative locator. The daemon resolves it beneath the
//! session's immutable workspace root before installing it in a turn.

use sqlx::SqlitePool;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWorkingDirectory {
    pub relative_path: String,
    pub revision: u64,
}

#[derive(Debug, Error)]
pub enum SessionWorkingDirectoryError {
    #[error("working-directory revision is stale")]
    StaleRevision,
    #[error("working-directory revision is out of range")]
    RevisionOverflow,
    #[error("session working-directory storage failed: {0}")]
    Storage(String),
}

#[derive(Clone)]
pub struct SessionWorkingDirectoryStore {
    pool: SqlitePool,
}

impl SessionWorkingDirectoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get(
        &self,
        session_id: &str,
    ) -> Result<SessionWorkingDirectory, SessionWorkingDirectoryError> {
        let row = sqlx::query_as::<_, (String, i64)>(
            "SELECT relative_path, revision FROM session_working_directory WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(match row {
            Some((relative_path, revision)) => SessionWorkingDirectory {
                relative_path,
                revision: u64::try_from(revision)
                    .map_err(|_| SessionWorkingDirectoryError::RevisionOverflow)?,
            },
            None => SessionWorkingDirectory {
                relative_path: ".".to_owned(),
                revision: 0,
            },
        })
    }

    pub async fn compare_set(
        &self,
        session_id: &str,
        expected_revision: u64,
        relative_path: &str,
    ) -> Result<SessionWorkingDirectory, SessionWorkingDirectoryError> {
        let next = expected_revision
            .checked_add(1)
            .and_then(|v| i64::try_from(v).ok())
            .ok_or(SessionWorkingDirectoryError::RevisionOverflow)?;
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        let changed = if expected_revision == 0 {
            sqlx::query(
                "INSERT INTO session_working_directory(session_id, relative_path, revision, updated_at_ms) \
                 VALUES (?, ?, 1, ?) ON CONFLICT(session_id) DO NOTHING",
            )
            .bind(session_id)
            .bind(relative_path)
            .bind(now_ms())
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?
            .rows_affected()
        } else {
            sqlx::query(
                "UPDATE session_working_directory SET relative_path = ?, revision = ?, updated_at_ms = ? \
                 WHERE session_id = ? AND revision = ?",
            )
            .bind(relative_path)
            .bind(next)
            .bind(now_ms())
            .bind(session_id)
            .bind(i64::try_from(expected_revision).map_err(|_| SessionWorkingDirectoryError::RevisionOverflow)?)
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?
            .rows_affected()
        };
        if changed != 1 {
            return Err(SessionWorkingDirectoryError::StaleRevision);
        }
        tx.commit().await.map_err(storage_error)?;
        Ok(SessionWorkingDirectory {
            relative_path: relative_path.to_owned(),
            revision: next as u64,
        })
    }
}

fn storage_error(error: sqlx::Error) -> SessionWorkingDirectoryError {
    SessionWorkingDirectoryError::Storage(error.to_string())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
