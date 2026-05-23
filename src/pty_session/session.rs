use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::StorageError;
use crate::pty_session::{CreatePtySession, PtyResize, PtySession};

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

    pub async fn update_cwd(&self, id: &str, cwd: &str) -> Result<PtySession, StorageError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| StorageError::NotFound(format!("pty session {id}")))?;

        session.cwd = cwd.to_string();
        Ok(session.clone())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_manager() -> PtyManager {
        PtyManager::new()
    }

    fn create_test_session_input() -> CreatePtySession {
        CreatePtySession {
            project_id: "test-project".to_string(),
            cwd: Some("/tmp".to_string()),
            shell: Some("bash".to_string()),
            cols: Some(80),
            rows: Some(24),
        }
    }

    #[tokio::test]
    async fn test_create_session() {
        let manager = create_test_manager();
        let input = create_test_session_input();

        let session = manager.create(input).await.unwrap();

        assert!(!session.id.is_empty());
        assert_eq!(session.project_id, "test-project");
        assert_eq!(session.cwd, "/tmp");
        assert_eq!(session.shell, "bash");
        assert_eq!(session.cols, 80);
        assert_eq!(session.rows, 24);
        assert!(session.created_at > 0);
    }

    #[tokio::test]
    async fn test_create_session_defaults() {
        let manager = create_test_manager();
        let input = CreatePtySession {
            project_id: "test-project".to_string(),
            cwd: None,
            shell: None,
            cols: None,
            rows: None,
        };

        let session = manager.create(input).await.unwrap();

        assert_eq!(session.cwd, ".");
        assert_eq!(session.shell, "bash");
        assert_eq!(session.cols, 80);
        assert_eq!(session.rows, 24);
    }

    #[tokio::test]
    async fn test_get_session() {
        let manager = create_test_manager();
        let input = create_test_session_input();

        let created = manager.create(input).await.unwrap();
        let retrieved = manager.get(&created.id).await;

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, created.id);
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let manager = create_test_manager();
        let result = manager.get("nonexistent-id").await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_update_cwd() {
        let manager = create_test_manager();
        let input = create_test_session_input();

        let created = manager.create(input).await.unwrap();
        let updated = manager.update_cwd(&created.id, "/home/user").await.unwrap();

        assert_eq!(updated.cwd, "/home/user");
    }

    #[tokio::test]
    async fn test_update_cwd_not_found() {
        let manager = create_test_manager();
        let result = manager.update_cwd("nonexistent-id", "/tmp").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let manager = create_test_manager();

        manager
            .create(CreatePtySession {
                project_id: "project-a".to_string(),
                cwd: None,
                shell: None,
                cols: None,
                rows: None,
            })
            .await
            .unwrap();

        manager
            .create(CreatePtySession {
                project_id: "project-a".to_string(),
                cwd: None,
                shell: None,
                cols: None,
                rows: None,
            })
            .await
            .unwrap();

        manager
            .create(CreatePtySession {
                project_id: "project-b".to_string(),
                cwd: None,
                shell: None,
                cols: None,
                rows: None,
            })
            .await
            .unwrap();

        let sessions = manager.list("project-a").await;
        assert_eq!(sessions.len(), 2);

        let sessions = manager.list("project-b").await;
        assert_eq!(sessions.len(), 1);

        let sessions = manager.list("nonexistent").await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_resize() {
        let manager = create_test_manager();
        let input = create_test_session_input();

        let created = manager.create(input).await.unwrap();
        let result = manager
            .resize(&created.id, PtyResize { cols: 120, rows: 40 })
            .await;

        assert!(result.is_ok());
        let resized = manager.get(&created.id).await.unwrap();
        assert_eq!(resized.cols, 120);
        assert_eq!(resized.rows, 40);
    }

    #[tokio::test]
    async fn test_resize_not_found() {
        let manager = create_test_manager();
        let result = manager
            .resize("nonexistent-id", PtyResize { cols: 120, rows: 40 })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete() {
        let manager = create_test_manager();
        let input = create_test_session_input();

        let created = manager.create(input).await.unwrap();
        let result = manager.delete(&created.id).await;

        assert!(result.is_ok());
        assert!(manager.get(&created.id).await.is_none());
    }

    #[tokio::test]
    async fn test_delete_not_found() {
        let manager = create_test_manager();
        let result = manager.delete("nonexistent-id").await;

        assert!(result.is_err());
    }
}
