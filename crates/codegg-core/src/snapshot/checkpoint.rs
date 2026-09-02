use serde::{Deserialize, Serialize};
use sha2::Digest;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::SnapshotOptions;

/// Present/absent file state for checkpoint pre/post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileState {
    Absent,
    Present { hash: String, content: String },
}

impl FileState {
    pub fn is_absent(&self) -> bool {
        matches!(self, FileState::Absent)
    }

    pub fn hash(&self) -> Option<&str> {
        match self {
            FileState::Present { hash, .. } => Some(hash),
            FileState::Absent => None,
        }
    }
}

/// One file's pre/post state within a checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFileState {
    pub path: String,
    pub pre: FileState,
    pub post: FileState,
}

/// Durable edit checkpoint scoped to workspace/session/turn/batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditCheckpoint {
    pub id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub batch_seq: i64,
    pub created_at: i64,
    pub files: Vec<EditFileState>,
}

/// Manager for durable edit checkpoints. Reuses snapshot bounds.
pub struct EditCheckpointManager {
    pool: SqlitePool,
    project_root: PathBuf,
    options: SnapshotOptions,
}

impl EditCheckpointManager {
    pub fn new(pool: SqlitePool, project_root: PathBuf) -> Self {
        Self {
            pool,
            project_root,
            options: SnapshotOptions::default(),
        }
    }

    pub fn new_with_options(
        pool: SqlitePool,
        project_root: PathBuf,
        options: SnapshotOptions,
    ) -> Self {
        let mut options = options;
        if options.max_files == 0 {
            tracing::warn!("SnapshotOptions: max_files is 0, clamping to 1");
            options.max_files = 1;
        }
        if options.max_file_bytes == 0 {
            tracing::warn!("SnapshotOptions: max_file_bytes is 0, clamping to 1");
            options.max_file_bytes = 1;
        }
        if options.max_total_bytes == 0 {
            tracing::warn!("SnapshotOptions: max_total_bytes is 0, clamping to 1");
            options.max_total_bytes = 1;
        }
        Self {
            pool,
            project_root,
            options,
        }
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn pool(&self) -> SqlitePool {
        self.pool.clone()
    }

    /// Capture current FileState for a single relative path.
    /// Returns Absent if file does not exist, Present if it exists and is within bounds.
    /// Errors on unsafe path, symlink, oversize, or binary content.
    pub fn capture_file_state_sync(&self, relative_path: &str) -> Result<FileState, String> {
        let path_buf = PathBuf::from(relative_path);
        if path_buf.is_absolute() || !super::is_safe_relative_path(&path_buf) {
            return Err(format!("unsafe checkpoint path: {}", relative_path));
        }
        let abs_path = self.project_root.join(&path_buf);
        // Containment check via prefix without canonicalize of non-existent files:
        // Ensure abs_path starts with project_root via simple path prefix after join.
        // For existing files we also check symlink via symlink_metadata.
        if !abs_path.starts_with(&self.project_root) {
            return Err(format!("checkpoint path escapes root: {}", relative_path));
        }

        // Check symlink at every component (best effort via symlink_metadata on full path parent)
        if let Some(parent) = abs_path.parent() {
            if parent.exists() {
                if let Ok(meta) = std::fs::symlink_metadata(parent) {
                    if meta.file_type().is_symlink() {
                        return Err(format!(
                            "checkpoint path traverses symlink: {}",
                            relative_path
                        ));
                    }
                }
            }
        }
        if let Ok(meta) = std::fs::symlink_metadata(&abs_path) {
            if meta.file_type().is_symlink() {
                return Err(format!("checkpoint path is symlink: {}", relative_path));
            }
        }

        if !abs_path.exists() {
            return Ok(FileState::Absent);
        }
        let metadata = std::fs::metadata(&abs_path)
            .map_err(|e| format!("stat failed for {}: {}", relative_path, e))?;
        if !metadata.is_file() {
            return Ok(FileState::Absent);
        }
        if metadata.len() > self.options.max_file_bytes {
            return Err(format!(
                "file {} exceeds max_file_bytes {}",
                relative_path, self.options.max_file_bytes
            ));
        }
        let bytes = std::fs::read(&abs_path)
            .map_err(|e| format!("read failed for {}: {}", relative_path, e))?;
        if bytes.len() as u64 > self.options.max_file_bytes {
            return Err(format!(
                "file {} exceeds max_file_bytes after read",
                relative_path
            ));
        }
        let content = String::from_utf8(bytes)
            .map_err(|_| format!("file {} is not valid UTF-8", relative_path))?;
        let hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
        Ok(FileState::Present { hash, content })
    }

    /// Capture states for a set of relative paths. Enforces total bytes bound.
    pub fn capture_file_states_sync(
        &self,
        paths: &[String],
    ) -> Result<HashMap<String, FileState>, String> {
        if paths.len() > self.options.max_files {
            return Err(format!(
                "checkpoint file count {} exceeds max_files {}",
                paths.len(),
                self.options.max_files
            ));
        }
        let mut out = HashMap::new();
        let mut total_bytes: u64 = 0;
        for p in paths {
            let state = self.capture_file_state_sync(p)?;
            if let FileState::Present { content, .. } = &state {
                total_bytes = total_bytes.saturating_add(content.len() as u64);
                if total_bytes > self.options.max_total_bytes {
                    return Err(format!(
                        "checkpoint total bytes {} exceeds max_total_bytes {}",
                        total_bytes, self.options.max_total_bytes
                    ));
                }
            }
            out.insert(p.clone(), state);
        }
        Ok(out)
    }

    /// Async wrapper that captures states without blocking the executor.
    pub async fn capture_states(
        &self,
        paths: &[String],
    ) -> Result<HashMap<String, FileState>, String> {
        let manager_root = self.project_root.clone();
        let options = self.options.clone();
        let paths_owned = paths.to_owned();
        let mgr = EditCheckpointManager {
            pool: self.pool.clone(),
            project_root: manager_root,
            options,
        };
        tokio::task::spawn_blocking(move || mgr.capture_file_states_sync(&paths_owned))
            .await
            .map_err(|e| format!("capture join error: {}", e))?
    }

    pub async fn persist_checkpoint(
        &self,
        checkpoint: EditCheckpoint,
    ) -> Result<EditCheckpoint, String> {
        // Validate files before persisting
        if checkpoint.files.is_empty() {
            return Err("checkpoint has no files".to_string());
        }
        if checkpoint.files.len() > self.options.max_files {
            return Err(format!(
                "checkpoint file count {} exceeds max_files",
                checkpoint.files.len()
            ));
        }
        // Validate paths and size
        let data = serde_json::to_string(&checkpoint.files).map_err(|e| e.to_string())?;
        // Check total bytes approx
        if data.len() as u64 > self.options.max_total_bytes {
            return Err("checkpoint serialized data exceeds max_total_bytes".to_string());
        }
        for f in &checkpoint.files {
            let pb = PathBuf::from(&f.path);
            if pb.is_absolute() || !super::is_safe_relative_path(&pb) {
                return Err(format!("unsafe path in checkpoint: {}", f.path));
            }
            if f.path.is_empty() {
                return Err("empty path in checkpoint".to_string());
            }
            // Check content size per file if present
            for state in [&f.pre, &f.post] {
                if let FileState::Present { content, .. } = state {
                    if content.len() as u64 > self.options.max_file_bytes {
                        return Err(format!("file {} content exceeds max_file_bytes", f.path));
                    }
                }
            }
        }

        sqlx::query(
            "INSERT INTO edit_checkpoint (id, workspace_id, session_id, turn_id, batch_seq, created_at, data) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&checkpoint.id)
        .bind(&checkpoint.workspace_id)
        .bind(&checkpoint.session_id)
        .bind(&checkpoint.turn_id)
        .bind(checkpoint.batch_seq)
        .bind(checkpoint.created_at)
        .bind(&data)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(checkpoint)
    }

    pub async fn get(&self, id: &str) -> Result<Option<EditCheckpoint>, String> {
        let row: Option<(String, String, String, Option<String>, i64, i64, String)> = sqlx::query_as(
            "SELECT id, workspace_id, session_id, turn_id, batch_seq, created_at, data FROM edit_checkpoint WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        match row {
            Some((id, workspace_id, session_id, turn_id, batch_seq, created_at, data)) => {
                let files = serde_json::from_str(&data).map_err(|e| e.to_string())?;
                Ok(Some(EditCheckpoint {
                    id,
                    workspace_id,
                    session_id,
                    turn_id,
                    batch_seq,
                    created_at,
                    files,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn list_for_session(&self, session_id: &str) -> Result<Vec<EditCheckpoint>, String> {
        let rows: Vec<(String, String, String, Option<String>, i64, i64, String)> = sqlx::query_as(
            "SELECT id, workspace_id, session_id, turn_id, batch_seq, created_at, data FROM edit_checkpoint WHERE session_id = ? ORDER BY created_at DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for (id, workspace_id, sid, turn_id, batch_seq, created_at, data) in rows {
            let files = serde_json::from_str(&data).map_err(|e| e.to_string())?;
            out.push(EditCheckpoint {
                id,
                workspace_id,
                session_id: sid,
                turn_id,
                batch_seq,
                created_at,
                files,
            });
        }
        Ok(out)
    }

    pub async fn list_for_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<EditCheckpoint>, String> {
        let rows: Vec<(String, String, String, Option<String>, i64, i64, String)> = sqlx::query_as(
            "SELECT id, workspace_id, session_id, turn_id, batch_seq, created_at, data FROM edit_checkpoint WHERE workspace_id = ? ORDER BY created_at DESC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for (id, wid, session_id, turn_id, batch_seq, created_at, data) in rows {
            let files = serde_json::from_str(&data).map_err(|e| e.to_string())?;
            out.push(EditCheckpoint {
                id,
                workspace_id: wid,
                session_id,
                turn_id,
                batch_seq,
                created_at,
                files,
            });
        }
        Ok(out)
    }

    pub async fn latest_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<EditCheckpoint>, String> {
        let row: Option<(String, String, String, Option<String>, i64, i64, String)> = sqlx::query_as(
            "SELECT id, workspace_id, session_id, turn_id, batch_seq, created_at, data FROM edit_checkpoint WHERE session_id = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        match row {
            Some((id, workspace_id, sid, turn_id, batch_seq, created_at, data)) => {
                let files = serde_json::from_str(&data).map_err(|e| e.to_string())?;
                Ok(Some(EditCheckpoint {
                    id,
                    workspace_id,
                    session_id: sid,
                    turn_id,
                    batch_seq,
                    created_at,
                    files,
                }))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            r#"
            CREATE TABLE edit_checkpoint (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                turn_id TEXT,
                batch_seq INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                data TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[test]
    fn file_state_absent_present_serialization_round_trip() {
        let absent = FileState::Absent;
        let present = FileState::Present {
            hash: "abc".into(),
            content: "hello".into(),
        };
        let json_absent = serde_json::to_string(&absent).unwrap();
        let json_present = serde_json::to_string(&present).unwrap();
        let de_absent: FileState = serde_json::from_str(&json_absent).unwrap();
        let de_present: FileState = serde_json::from_str(&json_present).unwrap();
        assert_eq!(absent, de_absent);
        assert_eq!(present, de_present);
    }

    #[tokio::test]
    async fn persistence_round_trip_create_update_delete_move() {
        let pool = test_pool().await;
        let tmp = tempfile::tempdir().unwrap();
        let mgr = EditCheckpointManager::new(pool, tmp.path().to_path_buf());

        // create: absent -> present
        let cp_create = EditCheckpoint {
            id: "cp1".into(),
            workspace_id: "ws1".into(),
            session_id: "sess1".into(),
            turn_id: Some("turn1".into()),
            batch_seq: 1,
            created_at: 1000,
            files: vec![EditFileState {
                path: "new.txt".into(),
                pre: FileState::Absent,
                post: FileState::Present {
                    hash: format!("{:x}", sha2::Sha256::digest(b"hello")),
                    content: "hello".into(),
                },
            }],
        };
        mgr.persist_checkpoint(cp_create.clone()).await.unwrap();
        let got = mgr.get("cp1").await.unwrap().unwrap();
        assert_eq!(got.files.len(), 1);
        assert!(got.files[0].pre.is_absent());

        // update: present -> present
        let cp_update = EditCheckpoint {
            id: "cp2".into(),
            workspace_id: "ws1".into(),
            session_id: "sess1".into(),
            turn_id: Some("turn1".into()),
            batch_seq: 2,
            created_at: 2000,
            files: vec![EditFileState {
                path: "existing.txt".into(),
                pre: FileState::Present {
                    hash: "h1".into(),
                    content: "old".into(),
                },
                post: FileState::Present {
                    hash: "h2".into(),
                    content: "new".into(),
                },
            }],
        };
        mgr.persist_checkpoint(cp_update).await.unwrap();

        // delete: present -> absent
        let cp_delete = EditCheckpoint {
            id: "cp3".into(),
            workspace_id: "ws1".into(),
            session_id: "sess1".into(),
            turn_id: Some("turn1".into()),
            batch_seq: 3,
            created_at: 3000,
            files: vec![EditFileState {
                path: "del.txt".into(),
                pre: FileState::Present {
                    hash: "h".into(),
                    content: "bye".into(),
                },
                post: FileState::Absent,
            }],
        };
        mgr.persist_checkpoint(cp_delete).await.unwrap();

        // move: two files
        let cp_move = EditCheckpoint {
            id: "cp4".into(),
            workspace_id: "ws1".into(),
            session_id: "sess1".into(),
            turn_id: Some("turn1".into()),
            batch_seq: 4,
            created_at: 4000,
            files: vec![
                EditFileState {
                    path: "old.txt".into(),
                    pre: FileState::Present {
                        hash: "h1".into(),
                        content: "move_me".into(),
                    },
                    post: FileState::Absent,
                },
                EditFileState {
                    path: "new.txt".into(),
                    pre: FileState::Absent,
                    post: FileState::Present {
                        hash: "h1".into(),
                        content: "move_me".into(),
                    },
                },
            ],
        };
        mgr.persist_checkpoint(cp_move.clone()).await.unwrap();
        let got_move = mgr.get("cp4").await.unwrap().unwrap();
        assert_eq!(got_move.files.len(), 2);
    }

    #[tokio::test]
    async fn unsafe_path_rejected() {
        let pool = test_pool().await;
        let tmp = tempfile::tempdir().unwrap();
        let mgr = EditCheckpointManager::new(pool, tmp.path().to_path_buf());
        let cp = EditCheckpoint {
            id: "bad".into(),
            workspace_id: "ws1".into(),
            session_id: "sess1".into(),
            turn_id: None,
            batch_seq: 1,
            created_at: 1,
            files: vec![EditFileState {
                path: "../evil.txt".into(),
                pre: FileState::Absent,
                post: FileState::Present {
                    hash: "h".into(),
                    content: "x".into(),
                },
            }],
        };
        assert!(mgr.persist_checkpoint(cp).await.is_err());
    }

    #[tokio::test]
    async fn oversized_content_rejected() {
        let pool = test_pool().await;
        let tmp = tempfile::tempdir().unwrap();
        let mgr = EditCheckpointManager::new_with_options(
            pool,
            tmp.path().to_path_buf(),
            SnapshotOptions {
                max_files: 10,
                max_file_bytes: 10,
                max_total_bytes: 1000,
            },
        );
        let cp = EditCheckpoint {
            id: "big".into(),
            workspace_id: "ws1".into(),
            session_id: "sess1".into(),
            turn_id: None,
            batch_seq: 1,
            created_at: 1,
            files: vec![EditFileState {
                path: "big.txt".into(),
                pre: FileState::Absent,
                post: FileState::Present {
                    hash: "h".into(),
                    content: "01234567890".into(), // 11 >10
                },
            }],
        };
        assert!(mgr.persist_checkpoint(cp).await.is_err());
    }
}
