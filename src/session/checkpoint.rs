use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tracing::warn;

use crate::error::StorageError;
use crate::session::message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub timestamp: i64,
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub messages: Vec<message::Message>,
    pub completed_steps: Vec<String>,
    pub working_files: Vec<WorkingFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingFile {
    pub path: String,
    pub checksum: String,
    pub pre_state: Option<String>,
}

#[derive(sqlx::FromRow, Debug)]
#[allow(dead_code)]
struct CheckpointRow {
    id: String,
    session_id: String,
    timestamp: i64,
    state: String,
}

impl TryFrom<CheckpointRow> for Checkpoint {
    type Error = StorageError;
    fn try_from(row: CheckpointRow) -> Result<Self, Self::Error> {
        serde_json::from_str(&row.state).map_err(|e| StorageError::Database(e.to_string()))
    }
}

pub struct CheckpointStore {
    pool: SqlitePool,
}

impl CheckpointStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn save(&self, checkpoint: &Checkpoint) -> Result<(), StorageError> {
        let state =
            serde_json::to_string(checkpoint).map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO checkpoints (id, session_id, timestamp, state)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(&checkpoint.id)
        .bind(&checkpoint.session_id)
        .bind(checkpoint.timestamp)
        .bind(&state)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    pub async fn load(&self, id: &str) -> Result<Option<Checkpoint>, StorageError> {
        sqlx::query_as::<_, CheckpointRow>(
            r#"
            SELECT id, session_id, timestamp, state
            FROM checkpoints WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?
        .map(|r| r.try_into())
        .transpose()
        .map_err(|e: StorageError| e)
    }

    pub async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>, StorageError> {
        sqlx::query_as::<_, CheckpointRow>(
            r#"
            SELECT id, session_id, timestamp, state
            FROM checkpoints WHERE session_id = ?
            ORDER BY timestamp DESC
            LIMIT 1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?
        .map(|r| r.try_into())
        .transpose()
        .map_err(|e: StorageError| e)
    }

    pub async fn list(&self, session_id: &str) -> Result<Vec<Checkpoint>, StorageError> {
        sqlx::query_as::<_, CheckpointRow>(
            r#"
            SELECT id, session_id, timestamp, state
            FROM checkpoints WHERE session_id = ?
            ORDER BY timestamp DESC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?
        .into_iter()
        .map(|r| r.try_into())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e: StorageError| e)
    }

    pub async fn delete(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM checkpoints WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn delete_all(&self, session_id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM checkpoints WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn has_checkpoint(&self, session_id: &str) -> Result<bool, StorageError> {
        let latest = self.load_latest(session_id).await?;
        Ok(latest.is_some())
    }
}

pub fn compute_checksum(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn create_working_file(path: &str, pre_state: Option<String>) -> Option<WorkingFile> {
    let path = path.to_string();
    let current = std::fs::read_to_string(&path).ok()?;

    let checksum = compute_checksum(&current);
    Some(WorkingFile {
        path,
        checksum,
        pre_state,
    })
}

pub fn verify_file(path: &str, expected_checksum: &str) -> bool {
    let current = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!("failed to read file for verification {}: {}", path, e);
            return false;
        }
    };
    compute_checksum(&current) == expected_checksum
}
