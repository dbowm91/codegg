pub mod diff;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: String,
    pub content: String,
    pub hash: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub session_id: String,
    pub files: HashMap<String, FileSnapshot>,
    pub created_at: i64,
    pub label: Option<String>,
}

pub struct SnapshotManager {
    snapshots: Vec<Snapshot>,
    project_root: PathBuf,
}

impl SnapshotManager {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            snapshots: Vec::new(),
            project_root,
        }
    }

    pub async fn capture(
        &mut self,
        session_id: &str,
        label: Option<String>,
    ) -> Result<Snapshot, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let mut files = HashMap::new();

        self.collect_files(&self.project_root, &mut files);

        let snapshot = Snapshot {
            id,
            session_id: session_id.to_string(),
            files,
            created_at: now,
            label,
        };

        self.snapshots.push(snapshot.clone());
        Ok(snapshot)
    }

    pub fn get(&self, id: &str) -> Option<&Snapshot> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    pub fn list_for_session(&self, session_id: &str) -> Vec<&Snapshot> {
        self.snapshots
            .iter()
            .filter(|s| s.session_id == session_id)
            .collect()
    }

    pub fn latest(&self, session_id: &str) -> Option<&Snapshot> {
        self.snapshots
            .iter()
            .filter(|s| s.session_id == session_id)
            .max_by_key(|s| s.created_at)
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
                    if name == ".git" || name == "node_modules" || name == "target" {
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
