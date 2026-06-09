use crate::error::AppError;
use egggit::worktree::WorktreeInfo;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_detached: bool,
}

fn into_legacy(info: WorktreeInfo) -> Worktree {
    Worktree {
        path: info.path,
        branch: info.branch,
        is_current: info.is_current,
        is_detached: info.is_detached,
    }
}

pub async fn list_worktrees(git_root: &Path) -> Result<Vec<Worktree>, AppError> {
    let infos = egggit::worktree::list_worktrees(git_root)
        .await
        .map_err(|e| AppError::Worktree(format!("failed to list worktrees: {e}")))?;
    Ok(infos.into_iter().map(into_legacy).collect())
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

    let output = std::process::Command::new("git")
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
    let output = std::process::Command::new("git")
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

pub fn find_git_root(start: &Path) -> Option<std::path::PathBuf> {
    egggit::worktree::find_git_root(start)
}

pub fn is_git_file(git_path: &Path) -> bool {
    egggit::worktree::is_git_file(git_path)
}

pub fn is_git_worktree(dir: &Path) -> bool {
    egggit::worktree::is_git_worktree(dir)
}
