pub mod diff;

use serde::{Deserialize, Serialize};
use sha2::Digest;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SnapshotOptions {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            max_files: 5_000,
            max_file_bytes: 1_000_000,
            max_total_bytes: 20_000_000,
        }
    }
}

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
    options: SnapshotOptions,
}

impl SnapshotManager {
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

    pub async fn capture(
        &mut self,
        session_id: &str,
        label: Option<String>,
    ) -> Result<SnapshotView, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let project_root = self.project_root.clone();
        let options = self.options.clone();
        let files =
            tokio::task::spawn_blocking(move || collect_files_sync(&project_root, &options))
                .await
                .map_err(|e| format!("snapshot collection join error: {e}"))?;

        let data = serde_json::to_string(&files).map_err(|e| e.to_string())?;

        sqlx::query(
            "INSERT INTO snapshot (id, session_id, created_at, label, data) VALUES (?, ?, ?, ?, ?)",
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

    pub async fn capture_incremental(
        &self,
        session_id: &str,
        label: Option<String>,
        file_changes: Vec<(String, Option<String>)>,
    ) -> Result<Option<SnapshotView>, String> {
        let mut files = HashMap::new();
        let now = chrono::Utc::now().timestamp_millis();

        for (path, old_content) in file_changes {
            let Some(content) = old_content else {
                continue;
            };
            let path_buf = PathBuf::from(&path);
            // Reject absolute paths and any path whose components
            // include `..` so we never store a traversal key that
            // could later escape the project root during restore.
            if path_buf.is_absolute() || !is_safe_relative_path(&path_buf) {
                tracing::warn!("capture_incremental: rejecting unsafe path '{}'", path);
                continue;
            }
            let abs_path = self.project_root.join(&path_buf);
            if !abs_path.starts_with(&self.project_root) {
                continue;
            }
            let rel_path = self.to_relative_path(&path);
            let hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
            files.insert(
                rel_path.clone(),
                FileSnapshot {
                    path: rel_path,
                    content,
                    hash,
                    timestamp: now,
                },
            );
        }

        if files.is_empty() {
            return Ok(None);
        }

        let id = uuid::Uuid::new_v4().to_string();
        let data = serde_json::to_string(&files).map_err(|e| e.to_string())?;
        sqlx::query(
            "INSERT INTO snapshot (id, session_id, created_at, label, data) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(now)
        .bind(&label)
        .bind(&data)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(Some(SnapshotView {
            id,
            session_id: session_id.to_string(),
            files,
            created_at: now,
            label,
        }))
    }

    pub async fn get(&self, id: &str) -> Result<Option<SnapshotView>, String> {
        let snapshot = sqlx::query_as::<_, Snapshot>(
            "SELECT id, session_id, created_at, label, data FROM snapshot WHERE id = ?",
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

    fn to_relative_path(&self, path: &str) -> String {
        let path_buf = PathBuf::from(path);
        match path_buf.strip_prefix(&self.project_root) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => {
                tracing::warn!(
                    "to_relative_path: path '{}' is not relative to project_root '{}', returning absolute path",
                    path,
                    self.project_root.display()
                );
                path_buf.to_string_lossy().to_string()
            }
        }
    }

    pub async fn restore(&self, snapshot: &SnapshotView) -> Result<(), String> {
        let project_root = self.project_root.clone();
        let files: Vec<(String, FileSnapshot)> = snapshot
            .files
            .iter()
            .filter(|(k, _)| {
                let safe = is_safe_relative_path(Path::new(k));
                if !safe {
                    tracing::warn!(
                        "restore: rejecting snapshot path '{}' that contains traversal components",
                        k
                    );
                }
                safe
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let (tx, rx) = tokio::sync::oneshot::channel();
        let canonical_project_root = project_root.canonicalize().map_err(|e| {
            format!(
                "failed to canonicalize project root {}: {}",
                project_root.display(),
                e
            )
        })?;
        tokio::task::spawn_blocking(move || {
            let result: Result<(), String> = (|| {
                for (rel_path, file_snapshot) in files {
                    restore_file(&canonical_project_root, &rel_path, &file_snapshot.content)?;
                }
                Ok(())
            })();
            let _ = tx.send(result);
        })
        .await
        .map_err(|e| e.to_string())?;
        rx.await.map_err(|e| e.to_string())?
    }

    pub async fn restore_to_path(
        &self,
        snapshot: &SnapshotView,
        target_path: &Path,
    ) -> Result<(), String> {
        let files: Vec<(String, FileSnapshot)> = snapshot
            .files
            .iter()
            .filter(|(k, _)| {
                let safe = is_safe_relative_path(Path::new(k));
                if !safe {
                    tracing::warn!(
                        "restore_to_path: rejecting snapshot path '{}' that contains traversal components",
                        k
                    );
                }
                safe
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let target = target_path.to_path_buf();

        let (tx, rx) = tokio::sync::oneshot::channel();
        let canonical_target = target
            .canonicalize()
            .map_err(|e| format!("failed to canonicalize target {}: {}", target.display(), e))?;
        tokio::task::spawn_blocking(move || {
            let result: Result<(), String> = (|| {
                for (rel_path, file_snapshot) in files {
                    restore_file(&canonical_target, &rel_path, &file_snapshot.content)?;
                }
                Ok(())
            })();
            let _ = tx.send(result);
        })
        .await
        .map_err(|e| e.to_string())?;
        rx.await.map_err(|e| e.to_string())?
    }

    pub async fn delete_snapshot(&self, id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM snapshot WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn delete_all_for_session(&self, session_id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM snapshot WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Write file contents durably: create, write, and fsync before any
/// rename so the rename cannot land while data blocks are not on
/// stable storage (mirrors `run_store::write_file_durable`).
#[cfg(not(unix))]
fn write_durable(path: &Path, data: impl AsRef<[u8]>) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("failed to create {}: {}", path.display(), e))?;
    file.write_all(data.as_ref())
        .map_err(|e| format!("failed to write {}: {}", path.display(), e))?;
    file.sync_all()
        .map_err(|e| format!("failed to sync {}: {}", path.display(), e))
}

/// Restore one file with a unique temporary name, cleanup on failure, and a
/// best-effort directory sync after the atomic rename. The temporary file is
/// created with `O_NOFOLLOW` on Unix, while the parent is checked immediately
/// before the write to keep the path-validation window narrow.
fn restore_file(root: &Path, relative_path: &str, content: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        restore_file_unix(root, relative_path, content)
    }

    #[cfg(not(unix))]
    {
        let full_path = root.join(relative_path);
        let parent = full_path
            .parent()
            .ok_or_else(|| format!("invalid restore path: {}", full_path.display()))?;
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory {}: {}", parent.display(), e))?;
        }
        ensure_contained_parent(parent, root)?;
        let file_name = full_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("invalid restore path: {}", full_path.display()))?;
        let temp_path = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
        if let Err(error) = write_durable(&temp_path, content) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error);
        }
        if let Err(error) = std::fs::rename(&temp_path, &full_path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!(
                "failed to rename {}: {}",
                temp_path.display(),
                error
            ));
        }
        sync_parent_dir(&full_path);
        Ok(())
    }
}

#[cfg(unix)]
fn restore_file_unix(root: &Path, relative_path: &str, content: &str) -> Result<(), String> {
    use rustix::fs::{fsync, mkdirat, openat, renameat, AtFlags, Mode, OFlags, CWD};
    use rustix::io::write;

    let components: Vec<_> = Path::new(relative_path)
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_os_string()),
            std::path::Component::CurDir => None,
            _ => None,
        })
        .collect();
    let (file_name, parent_components) = components
        .split_last()
        .ok_or_else(|| format!("invalid restore path: {relative_path}"))?;

    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let mut directory = openat(CWD, root, directory_flags, Mode::empty())
        .map_err(|e| format!("failed to open restore root {}: {e}", root.display()))?;

    for component in parent_components {
        let next = match openat(&directory, component, directory_flags, Mode::empty()) {
            Ok(next) => next,
            Err(error) if error == rustix::io::Errno::NOENT => {
                match mkdirat(&directory, component, Mode::RWXU) {
                    Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                    Err(error) => {
                        return Err(format!("failed to create restore directory: {error}"))
                    }
                }
                openat(&directory, component, directory_flags, Mode::empty())
                    .map_err(|error| format!("failed to open restore directory: {error}"))?
            }
            Err(error) => return Err(format!("failed to open restore directory: {error}")),
        };
        directory = next;
    }

    let temp_name = format!(
        ".{}.{}.tmp",
        file_name.to_string_lossy(),
        uuid::Uuid::new_v4()
    );
    let temp_flags =
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let temp = openat(&directory, &temp_name, temp_flags, Mode::RUSR | Mode::WUSR)
        .map_err(|error| format!("failed to create temporary restore file: {error}"))?;
    let mut written = 0;
    while written < content.len() {
        match write(&temp, &content.as_bytes()[written..]) {
            Ok(0) => {
                let _ = rustix::fs::unlinkat(&directory, &temp_name, AtFlags::empty());
                return Err("failed to write temporary restore file: wrote zero bytes".to_string());
            }
            Ok(count) => written += count,
            Err(error) => {
                let _ = rustix::fs::unlinkat(&directory, &temp_name, AtFlags::empty());
                return Err(format!("failed to write temporary restore file: {error}"));
            }
        }
    }
    if let Err(error) = fsync(&temp) {
        let _ = rustix::fs::unlinkat(&directory, &temp_name, AtFlags::empty());
        return Err(format!("failed to sync temporary restore file: {error}"));
    }
    drop(temp);

    if let Err(error) = renameat(&directory, &temp_name, &directory, file_name) {
        let _ = rustix::fs::unlinkat(&directory, &temp_name, AtFlags::empty());
        return Err(format!("failed to rename restored file: {error}"));
    }
    let _ = fsync(&directory);
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
}

/// Reject paths whose components include `..`, Windows drive
/// prefixes, the root directory, or any other non-normal segment.
/// Empty paths are also rejected. This is the first line of
/// defence against snapshot paths escaping the project root
/// during restore.
fn is_safe_relative_path(path: &Path) -> bool {
    if path.as_os_str().is_empty() {
        return false;
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return false,
        }
    }
    true
}

/// Validate that `parent` is safe to write into: it must be a real
/// (non-symlink) directory whose canonical path stays inside
/// `canonical_root`. Fails closed on stat or canonicalize errors,
/// shrinking the create-then-check TOCTOU window around restore.
#[cfg(not(unix))]
fn ensure_contained_parent(parent: &Path, canonical_root: &Path) -> Result<(), String> {
    let meta = match parent.symlink_metadata() {
        Ok(meta) => meta,
        Err(e) => return Err(format!("failed to stat {}: {}", parent.display(), e)),
    };
    if meta.file_type().is_symlink() {
        return Err(format!(
            "path traversal attempt detected (symlinked directory): {}",
            parent.display()
        ));
    }
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize {}: {}", parent.display(), e))?;
    if !canonical_parent.starts_with(canonical_root) {
        return Err(format!(
            "path traversal attempt detected: {}",
            parent.display()
        ));
    }
    Ok(())
}

fn collect_files_sync(
    project_root: &Path,
    options: &SnapshotOptions,
) -> HashMap<String, FileSnapshot> {
    let mut files = HashMap::new();
    let mut stack = vec![project_root.to_path_buf()];
    let mut total_bytes = 0_u64;
    let now = chrono::Utc::now().timestamp_millis();

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            if files.len() >= options.max_files || total_bytes >= options.max_total_bytes {
                return files;
            }

            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                if name == ".git" || name == "node_modules" || name == "target" || name == ".codegg"
                {
                    continue;
                }
                stack.push(path);
                continue;
            }

            if !path.is_file() {
                continue;
            }

            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.len() > options.max_file_bytes {
                continue;
            }

            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if bytes.is_empty() {
                let rel_path = path
                    .strip_prefix(project_root)
                    .unwrap_or(path.as_path())
                    .to_string_lossy()
                    .to_string();
                files.insert(
                    rel_path.clone(),
                    FileSnapshot {
                        path: rel_path,
                        content: String::new(),
                        hash: format!("{:x}", sha2::Sha256::digest([])),
                        timestamp: now,
                    },
                );
                continue;
            }
            if total_bytes.saturating_add(bytes.len() as u64) > options.max_total_bytes {
                return files;
            }
            let Ok(content) = String::from_utf8(bytes) else {
                continue;
            };

            total_bytes = total_bytes.saturating_add(content.len() as u64);
            let hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
            let rel_path = path
                .strip_prefix(project_root)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .to_string();
            files.insert(
                rel_path.clone(),
                FileSnapshot {
                    path: rel_path,
                    content,
                    hash,
                    timestamp: now,
                },
            );
        }
    }

    files
}
