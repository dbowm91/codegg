use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::StorageError;
use crate::pty::{CreatePtySession, PtyResize, PtySession};

pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, PtySession>>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create(&self, input: CreatePtySession) -> Result<PtySession, StorageError> {
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();

        let session = PtySession {
            id: id.clone(),
            project_id: input.project_id,
            cwd: input.cwd.unwrap_or_else(|| ".".to_string()),
            shell: input.shell.unwrap_or_else(|| "bash".to_string()),
            cols: input.cols.unwrap_or(80),
            rows: input.rows.unwrap_or(24),
            created_at: now,
        };

        self.sessions.write().await.insert(id, session.clone());
        Ok(session)
    }

    pub async fn get(&self, id: &str) -> Option<PtySession> {
        self.sessions.read().await.get(id).cloned()
    }

    pub async fn list(&self, project_id: &str) -> Vec<PtySession> {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| s.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn resize(&self, id: &str, resize: PtyResize) -> Result<(), StorageError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| StorageError::NotFound(format!("pty session {id}")))?;

        session.cols = resize.cols;
        session.rows = resize.rows;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), StorageError> {
        self.sessions
            .write()
            .await
            .remove(id)
            .ok_or_else(|| StorageError::NotFound(format!("pty session {id}")))?;
        Ok(())
    }
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}
