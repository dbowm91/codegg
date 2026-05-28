use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::AppError;

/// Compare two paths, resolving symlinks if necessary.
/// Returns true if both paths refer to the same filesystem location.
fn paths_match(a: &Path, b: &Path) -> bool {
    // Fast path: direct comparison works for non-symlinked paths
    if a == b {
        return true;
    }

    // Try canonicalizing both paths to resolve symlinks
    let a_canonical = fs::canonicalize(a);
    let b_canonical = fs::canonicalize(b);

    match (a_canonical, b_canonical) {
        (Ok(a), Ok(b)) => a == b,
        // If canonicalization fails (e.g., path doesn't exist), fall back to string comparison
        _ => a == b,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_detached: bool,
}

fn push_parsed_worktree(
    worktrees: &mut Vec<Worktree>,
    path: &str,
    branch: &str,
    head: Option<&str>,
    is_current: bool,
    is_detached: bool,
) {
    if path.is_empty() {
        return;
    }

    let branch_name = if !branch.is_empty() {
        branch
            .strip_prefix("refs/heads/")
            .unwrap_or(branch)
            .to_string()
    } else if is_detached {
        match head {
            Some(sha) if !sha.is_empty() => format!("detached@{sha}"),
            _ => format!("detached@{path}"),
        }
    } else {
        String::new()
    };

    worktrees.push(Worktree {
        path: path.to_string(),
        branch: branch_name,
        is_current,
        is_detached,
    });
}

pub fn list_worktrees(git_root: &Path) -> Result<Vec<Worktree>, AppError> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(git_root)
        .output()
        .map_err(|e| AppError::Worktree(format!("failed to run git worktree list: {e}")))?;

    if !output.status.success() {
        return Err(AppError::Worktree(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();
    let mut current_path = String::new();
    let mut current_branch = String::new();
    let mut current_head: Option<String> = None;
    let mut current_is_detached = false;
    let mut current_is_current = false;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            push_parsed_worktree(
                &mut worktrees,
                &current_path,
                &current_branch,
                current_head.as_deref(),
                current_is_current,
                current_is_detached,
            );
            current_path = path.to_string();
            current_branch.clear();
            current_head = None;
            current_is_detached = false;
            current_is_current = paths_match(Path::new(path), git_root);
        } else if let Some(branch) = line.strip_prefix("branch ") {
            current_branch = branch.to_string();
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            current_head = Some(head.to_string());
        } else if line == "detached" {
            current_is_detached = true;
        }
    }

    push_parsed_worktree(
        &mut worktrees,
        &current_path,
        &current_branch,
        current_head.as_deref(),
        current_is_current,
        current_is_detached,
    );

    Ok(worktrees)
}

pub fn create_worktree(
    git_root: &Path,
    path: &Path,
    branch: &str,
    create_branch: bool,
) -> Result<(), AppError> {
    let mut args = vec!["worktree", "add"];
    let path_str = path.to_string_lossy().to_string();
    args.push(&path_str);

    if create_branch {
        args.push("-b");
    }
    args.push(branch);

    let output = Command::new("git")
        .args(&args)
        .current_dir(git_root)
        .output()
        .map_err(|e| AppError::Worktree(format!("failed to create worktree: {e}")))?;

    if !output.status.success() {
        return Err(AppError::Worktree(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

pub fn remove_worktree(git_root: &Path, path: &Path, force: bool) -> Result<(), AppError> {
    let path_str = path.to_string_lossy().to_string();
    let mut args = vec!["worktree", "remove", &path_str];
    if force {
        args.push("--force");
    }
    let output = Command::new("git")
        .args(&args)
        .current_dir(git_root)
        .output()
        .map_err(|e| AppError::Worktree(format!("failed to remove worktree: {e}")))?;

    if !output.status.success() {
        return Err(AppError::Worktree(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let git_path = current.join(".git");
        if git_path.exists() || is_git_file(&git_path) {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

pub fn is_git_file(git_path: &Path) -> bool {
    if let Ok(content) = std::fs::read_to_string(git_path) {
        content.starts_with("gitdir:")
    } else {
        false
    }
}

pub fn is_git_worktree(dir: &Path) -> bool {
    let git_path = dir.join(".git");
    git_path.exists() && is_git_file(&git_path)
}
