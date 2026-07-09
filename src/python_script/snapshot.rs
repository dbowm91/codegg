use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Snapshot of workspace file states before script execution.
#[derive(Debug, Clone)]
pub struct WorkspaceSnapshot {
    /// Map from relative path to metadata hash (size + mtime).
    files: HashMap<PathBuf, FileMetadata>,
    #[allow(dead_code)]
    root: PathBuf,
}

#[derive(Debug, Clone)]
struct FileMetadata {
    size: u64,
    mtime_secs: i64,
    #[allow(dead_code)]
    mtime_nanos: u32,
}

impl WorkspaceSnapshot {
    /// Capture current state of workspace files.
    /// Only files under `root` are included.
    pub fn capture(root: &Path) -> Self {
        let mut files = HashMap::new();
        if let Ok(entries) = walk_workspace(root) {
            for entry in entries {
                if let Ok(meta) = std::fs::metadata(&entry) {
                    let rel = entry.strip_prefix(root).unwrap_or(&entry).to_path_buf();
                    files.insert(
                        rel,
                        FileMetadata {
                            size: meta.len(),
                            mtime_secs: meta
                                .modified()
                                .map(|t| {
                                    t.duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs() as i64
                                })
                                .unwrap_or(0),
                            mtime_nanos: 0,
                        },
                    );
                }
            }
        }
        Self {
            files,
            root: root.to_path_buf(),
        }
    }

    /// Compare with another snapshot to find changed files.
    pub fn diff(&self, after: &WorkspaceSnapshot) -> Vec<PathBuf> {
        let mut changed = Vec::new();
        for (path, meta_after) in &after.files {
            match self.files.get(path) {
                Some(meta_before) => {
                    if meta_before.size != meta_after.size
                        || meta_before.mtime_secs != meta_after.mtime_secs
                    {
                        changed.push(path.clone());
                    }
                }
                None => {
                    // New file
                    changed.push(path.clone());
                }
            }
        }
        // Check for deleted files
        for path in self.files.keys() {
            if !after.files.contains_key(path) {
                changed.push(path.clone());
            }
        }
        changed.sort();
        changed
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

/// Walk workspace directory, collecting .py and other relevant files.
fn walk_workspace(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut results = Vec::new();
    if !root.exists() {
        return Ok(results);
    }
    walk_dir(root, &mut results, 10)?; // limit depth
    Ok(results)
}

fn walk_dir(dir: &Path, results: &mut Vec<PathBuf>, depth: usize) -> Result<(), std::io::Error> {
    if depth == 0 {
        return Ok(());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default();
                // Skip hidden dirs, build artifacts, and codegg internals
                let name_str = name.to_string_lossy();
                if name_str == "."
                    || name_str == ".."
                    || name_str.starts_with('.')
                    || name_str == "target"
                    || name_str == "node_modules"
                    || name_str == ".codegg"
                {
                    continue;
                }
                walk_dir(&path, results, depth - 1)?;
            } else {
                let name = path.file_name().unwrap_or_default();
                let name_str = name.to_string_lossy();
                // Skip OS metadata files
                if name_str.starts_with('.') || name_str == "Thumbs.db" {
                    continue;
                }
                results.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn snapshot_capture_empty_dir() {
        let dir = std::env::temp_dir().join("python_snap_test_empty");
        let _ = fs::create_dir_all(&dir);
        let snap = WorkspaceSnapshot::capture(&dir);
        assert_eq!(snap.file_count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_detects_new_file() {
        let dir = std::env::temp_dir().join("python_snap_test_new");
        let _ = fs::create_dir_all(&dir);
        let before = WorkspaceSnapshot::capture(&dir);
        fs::write(dir.join("test.txt"), "hello").unwrap();
        let after = WorkspaceSnapshot::capture(&dir);
        let changed = before.diff(&after);
        assert!(changed.iter().any(|p| p.ends_with("test.txt")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_detects_modified_file() {
        let dir = std::env::temp_dir().join("python_snap_test_mod");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("test.txt"), "v1").unwrap();
        let before = WorkspaceSnapshot::capture(&dir);
        // Small delay to ensure mtime differs
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(dir.join("test.txt"), "v2 is longer").unwrap();
        let after = WorkspaceSnapshot::capture(&dir);
        let changed = before.diff(&after);
        assert!(changed.iter().any(|p| p.ends_with("test.txt")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_detects_deleted_file() {
        let dir = std::env::temp_dir().join("python_snap_test_del");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("test.txt"), "hello").unwrap();
        let before = WorkspaceSnapshot::capture(&dir);
        fs::remove_file(dir.join("test.txt")).unwrap();
        let after = WorkspaceSnapshot::capture(&dir);
        let changed = before.diff(&after);
        assert!(changed.iter().any(|p| p.ends_with("test.txt")));
        let _ = fs::remove_dir_all(&dir);
    }
}
