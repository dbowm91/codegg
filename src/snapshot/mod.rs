pub mod diff;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: String,
    pub content: String,
    pub hash: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Snapshot {
    pub id: String,
    pub session_id: String,
    pub created_at: i64,
    pub label: Option<String>,
    pub data: String, // JSON serialized HashMap<String, FileSnapshot>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotView {
    pub id: String,
    pub session_id: String,
    pub files: HashMap<String, FileSnapshot>,
    pub created_at: i64,
    pub label: Option<String>,
}

pub struct SnapshotManager {
    pool: SqlitePool,
    project_root: PathBuf,
}

impl SnapshotManager {
    pub fn new(pool: SqlitePool, project_root: PathBuf) -> Self {
        Self {
            pool,
            project_root,
        }
    }

    pub async fn capture(
        &mut self,
        session_id: &str,
        label: Option<String>,
    ) -> Result<SnapshotView, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let mut files = HashMap::new();

        self.collect_files(&self.project_root, &mut files);

        let data = serde_json::to_string(&files).map_err(|e| e.to_string())?;

        sqlx::query(
            "INSERT INTO snapshot (id, session_id, created_at, label, data) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(session_id)
        .bind(now)
        .bind(&label)
        .bind(&data)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(SnapshotView {
            id,
            session_id: session_id.to_string(),
            files,
            created_at: now,
            label,
        })
    }

    pub async fn get(&self, id: &str) -> Result<Option<SnapshotView>, String> {
        let snapshot = sqlx::query_as::<_, Snapshot>(
            "SELECT id, session_id, created_at, label, data FROM snapshot WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        match snapshot {
            Some(s) => {
                let files = serde_json::from_str(&s.data).map_err(|e| e.to_string())?;
                Ok(Some(SnapshotView {
                    id: s.id,
                    session_id: s.session_id,
                    files,
                    created_at: s.created_at,
                    label: s.label,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn list_for_session(&self, session_id: &str) -> Result<Vec<SnapshotView>, String> {
        let snapshots = sqlx::query_as::<_, Snapshot>(
            "SELECT id, session_id, created_at, label, data FROM snapshot WHERE session_id = ? ORDER BY created_at DESC"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let mut views = Vec::new();
        for s in snapshots {
            let files = serde_json::from_str(&s.data).map_err(|e| e.to_string())?;
            views.push(SnapshotView {
                id: s.id,
                session_id: s.session_id,
                files,
                created_at: s.created_at,
                label: s.label,
            });
        }
        Ok(views)
    }

    pub async fn latest(&self, session_id: &str) -> Result<Option<SnapshotView>, String> {
        let snapshot = sqlx::query_as::<_, Snapshot>(
            "SELECT id, session_id, created_at, label, data FROM snapshot WHERE session_id = ? ORDER BY created_at DESC LIMIT 1"
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        match snapshot {
            Some(s) => {
                let files = serde_json::from_str(&s.data).map_err(|e| e.to_string())?;
                Ok(Some(SnapshotView {
                    id: s.id,
                    session_id: s.session_id,
                    files,
                    created_at: s.created_at,
                    label: s.label,
                }))
            }
            None => Ok(None),
        }
    }

    fn collect_files(&self, dir: &Path, files: &mut HashMap<String, FileSnapshot>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default();
                    if name == ".git" || name == "node_modules" || name == "target" || name == ".codegg" {
                        continue;
                    }
                    self.collect_files(&path, files);
                } else if path.is_file() {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let hash = format!("{:x}", md5::compute(content.as_bytes()));
                        let rel_path = path
                            .strip_prefix(&self.project_root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string();

                        let snapshot = FileSnapshot {
                            path: rel_path.clone(),
                            content,
                            hash,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        };

                        files.insert(rel_path, snapshot);
                    }
                }
            }
        }
    }
}
