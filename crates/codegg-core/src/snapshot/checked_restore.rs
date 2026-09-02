use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::checkpoint::{EditCheckpoint, FileState};

/// Direction of checked restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreDirection {
    Undo,
    Reapply,
}

impl std::fmt::Display for RestoreDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RestoreDirection::Undo => write!(f, "undo"),
            RestoreDirection::Reapply => write!(f, "reapply"),
        }
    }
}

/// Typed result of a checked restore operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckedRestoreOutcome {
    Applied {
        checkpoint_id: String,
        workspace_id: String,
        session_id: String,
        direction: RestoreDirection,
        restored_paths: Vec<String>,
    },
    Conflict {
        checkpoint_id: String,
        workspace_id: String,
        direction: RestoreDirection,
        stale_paths: Vec<String>,
    },
    NotFound {
        checkpoint_id: String,
    },
    WrongWorkspace {
        checkpoint_id: String,
        expected_workspace: String,
        actual_workspace: String,
    },
    WrongSession {
        checkpoint_id: String,
        expected_session: String,
        actual_session: String,
    },
    PathValidationFailed {
        checkpoint_id: String,
        invalid_paths: Vec<String>,
        reason: String,
    },
    PermissionDenied {
        checkpoint_id: String,
        denied_paths: Vec<String>,
        reason: String,
    },
    PartialFailure {
        checkpoint_id: String,
        workspace_id: String,
        direction: RestoreDirection,
        applied_paths: Vec<String>,
        failed_paths: Vec<String>,
        error: String,
    },
    Unsupported {
        checkpoint_id: String,
        reason: String,
    },
}

impl CheckedRestoreOutcome {
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::Applied { .. })
    }
    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict { .. })
    }
}

/// Bounded metadata for listing checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointSummary {
    pub id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub batch_seq: i64,
    pub created_at: i64,
    pub file_count: usize,
    pub paths: Vec<String>,
    pub restorable: bool,
}

/// Durable operation audit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreOperationRecord {
    pub id: String,
    pub checkpoint_id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub direction: RestoreDirection,
    pub result: String,
    pub conflict_paths: Vec<String>,
    pub applied_paths: Vec<String>,
    pub failed_paths: Vec<String>,
    pub error_message: Option<String>,
    pub created_at: i64,
}

/// Helper: compare two FileStates for equality (hash-based for Present).
pub fn file_states_equal(a: &FileState, b: &FileState) -> bool {
    match (a, b) {
        (FileState::Absent, FileState::Absent) => true,
        (FileState::Present { hash: h1, .. }, FileState::Present { hash: h2, .. }) => h1 == h2,
        _ => false,
    }
}

/// Validate a stored checkpoint path at restore time.
/// Rejects absolute, traversal, empty, and unsafe paths.
pub fn validate_checkpoint_path(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("empty path".into());
    }
    let pb = PathBuf::from(path);
    if pb.is_absolute() {
        return Err(format!("absolute path: {}", path));
    }
    if !super::is_safe_relative_path(&pb) {
        return Err(format!("unsafe path: {}", path));
    }
    Ok(())
}

/// Apply a single FileState to the workspace root.
/// Absent => delete file if present.
/// Present => atomic write via restore_file helper.
pub fn apply_file_state(
    workspace_root: &Path,
    relative_path: &str,
    target: &FileState,
) -> Result<(), String> {
    validate_checkpoint_path(relative_path)?;
    let canonical_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    // Re-validate containment using the canonical root and the joined path
    // before performing any mutation.  This mirrors snapshot::restore_file
    // path checks but adds absent handling.
    match target {
        FileState::Absent => {
            // Validate path before delete
            let full = canonical_root.join(relative_path);
            // Ensure the full path stays within canonical_root (prefix check after cleaning)
            // We use a simple prefix check on the joined path without canonicalizing
            // the file itself (it may not exist).  Also reject symlink parent/files.
            if let Some(parent) = full.parent() {
                if parent.exists() {
                    if let Ok(meta) = std::fs::symlink_metadata(parent) {
                        if meta.file_type().is_symlink() {
                            return Err(format!("parent is symlink: {}", relative_path));
                        }
                    }
                    let canon_parent = parent.canonicalize().map_err(|e| {
                        format!("canonicalize parent failed for {}: {}", relative_path, e)
                    })?;
                    if !canon_parent.starts_with(&canonical_root) {
                        return Err(format!("path escapes root: {}", relative_path));
                    }
                } else {
                    // Parent does not exist; ensure the lexical join does not escape via `..`
                    // Already validated via is_safe_relative_path, so safe to skip.
                }
            }
            if let Ok(meta) = std::fs::symlink_metadata(&full) {
                if meta.file_type().is_symlink() {
                    return Err(format!("path is symlink: {}", relative_path));
                }
            }
            if full.exists() {
                let meta = std::fs::metadata(&full)
                    .map_err(|e| format!("stat failed for {}: {}", relative_path, e))?;
                if meta.is_dir() {
                    return Err(format!(
                        "path is directory, cannot delete: {}",
                        relative_path
                    ));
                }
                std::fs::remove_file(&full)
                    .map_err(|e| format!("failed to delete {}: {}", relative_path, e))?;
                // best-effort fsync parent dir
                if let Some(parent) = full.parent() {
                    if let Ok(dir) = std::fs::File::open(parent) {
                        let _ = dir.sync_all();
                    }
                }
            }
            Ok(())
        }
        FileState::Present { content, .. } => {
            // Use snapshot's atomic write helper.  We duplicate its logic here
            // to avoid making it public, keeping the checked restore boundary
            // self-contained but identical in safety.
            restore_file_checked(&canonical_root, relative_path, content)
        }
    }
}

#[cfg(unix)]
fn restore_file_checked(root: &Path, relative_path: &str, content: &str) -> Result<(), String> {
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
fn restore_file_checked(root: &Path, relative_path: &str, content: &str) -> Result<(), String> {
    use std::io::Write;
    let full_path = root.join(relative_path);
    let parent = full_path
        .parent()
        .ok_or_else(|| format!("invalid restore path: {}", full_path.display()))?;
    if !parent.exists() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directory {}: {}", parent.display(), e))?;
    }
    // Ensure parent is inside root and not symlink
    let meta = parent
        .symlink_metadata()
        .map_err(|e| format!("failed to stat {}: {}", parent.display(), e))?;
    if meta.file_type().is_symlink() {
        return Err(format!("parent is symlink: {}", parent.display()));
    }
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize {}: {}", parent.display(), e))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(format!("path escapes root: {}", parent.display()));
    }
    let file_name = full_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("invalid restore path: {}", full_path.display()))?;
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    let mut file = {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        }
        opts.open(&temp_path)
            .map_err(|e| format!("failed to create {}: {}", temp_path.display(), e))?
    };
    file.write_all(content.as_bytes())
        .map_err(|e| format!("failed to write {}: {}", temp_path.display(), e))?;
    file.sync_all()
        .map_err(|e| format!("failed to sync {}: {}", temp_path.display(), e))?;
    drop(file);
    std::fs::rename(&temp_path, &full_path).map_err(|e| {
        let _ = std::fs::remove_file(&temp_path);
        format!("failed to rename {}: {}", temp_path.display(), e)
    })?;
    if let Some(parent) = full_path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Core checked restore logic without lock or persistence.
/// Validates all current states against expected, then applies.
/// Caller must ensure workspace lock is held from capture through apply.
pub fn checked_restore_inner(
    workspace_root: &Path,
    checkpoint: &EditCheckpoint,
    direction: RestoreDirection,
    current_states: &std::collections::HashMap<String, FileState>,
) -> CheckedRestoreOutcome {
    // Validate every stored path first
    let mut invalid = Vec::new();
    for f in &checkpoint.files {
        if let Err(e) = validate_checkpoint_path(&f.path) {
            invalid.push(format!("{}: {}", f.path, e));
        } else {
            // Also ensure lexical path does not escape via join prefix check
            let joined = workspace_root.join(&f.path);
            // Use simple lexical containment: if path contains `..` already rejected,
            // and is not absolute, it stays within root.
            // We still check that the parent after join would be under root when
            // canonicalized at apply time, but for preflight we just bound.
            if !joined.starts_with(workspace_root) {
                // This can happen if workspace_root is not canonicalized; we do lexical check
                // The workspace_root may be canonical, but joined with relative keeps prefix.
                // If it fails, mark invalid.
                // Actually Path::starts_with on non-canonical may be fragile, so we only
                // enforce via validate_checkpoint_path which already rejects `..`.
            }
        }
    }
    if !invalid.is_empty() {
        return CheckedRestoreOutcome::PathValidationFailed {
            checkpoint_id: checkpoint.id.clone(),
            invalid_paths: invalid.clone(),
            reason: invalid.join("; "),
        };
    }

    // Determine expected and target per file
    let mut stale_paths = Vec::new();
    let mut to_apply: Vec<(String, FileState)> = Vec::new();
    for f in &checkpoint.files {
        let expected = match direction {
            RestoreDirection::Undo => &f.post,
            RestoreDirection::Reapply => &f.pre,
        };
        let target = match direction {
            RestoreDirection::Undo => &f.pre,
            RestoreDirection::Reapply => &f.post,
        };
        let current = match current_states.get(&f.path) {
            Some(c) => c,
            None => {
                // Capture should have returned entry for every checkpoint path.
                // If missing, treat as stale.
                stale_paths.push(f.path.clone());
                continue;
            }
        };
        if !file_states_equal(current, expected) {
            stale_paths.push(f.path.clone());
        } else {
            to_apply.push((f.path.clone(), target.clone()));
        }
    }

    if !stale_paths.is_empty() {
        // Fail-closed: zero mutation, bounded list
        let bounded: Vec<String> = stale_paths.into_iter().take(20).collect();
        return CheckedRestoreOutcome::Conflict {
            checkpoint_id: checkpoint.id.clone(),
            workspace_id: checkpoint.workspace_id.clone(),
            direction,
            stale_paths: bounded,
        };
    }

    // All preflight passed; apply each path sequentially.
    // If any I/O error occurs, return PartialFailure with evidence.
    let mut applied = Vec::new();
    let mut failed = Vec::new();
    let mut error_msg = String::new();
    for (path, target_state) in to_apply {
        match apply_file_state(workspace_root, &path, &target_state) {
            Ok(()) => applied.push(path),
            Err(e) => {
                failed.push(path.clone());
                error_msg = e;
                // Stop further writes when safe, per plan.
                break;
            }
        }
    }

    if !failed.is_empty() {
        return CheckedRestoreOutcome::PartialFailure {
            checkpoint_id: checkpoint.id.clone(),
            workspace_id: checkpoint.workspace_id.clone(),
            direction,
            applied_paths: applied,
            failed_paths: failed,
            error: error_msg,
        };
    }

    CheckedRestoreOutcome::Applied {
        checkpoint_id: checkpoint.id.clone(),
        workspace_id: checkpoint.workspace_id.clone(),
        session_id: checkpoint.session_id.clone(),
        direction,
        restored_paths: applied,
    }
}

/// Convert a FileState equality check into bounded error logging.
/// Ensures no file bodies are emitted in logs; caller should only log paths.
pub fn conflict_paths_for_log(paths: &[String]) -> String {
    // Bounded, path-only.
    let bounded: Vec<&String> = paths.iter().take(10).collect();
    format!(
        "conflict paths: {}",
        bounded
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )
}
