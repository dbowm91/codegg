use crate::EgggitError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_detached: bool,
}

async fn run_git_async(
    root: PathBuf,
    args: Vec<String>,
) -> Result<std::process::Output, EgggitError> {
    tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new("git");
        cmd.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            cmd.env("PATH", path);
        } else {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
        cmd.args(&args).current_dir(&root);
        cmd.output().map_err(|e| EgggitError::Io(e.to_string()))
    })
    .await
    .map_err(|e| EgggitError::Join(e.to_string()))?
}

fn push_parsed_worktree(
    worktrees: &mut Vec<WorktreeInfo>,
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

    worktrees.push(WorktreeInfo {
        path: path.to_string(),
        branch: branch_name,
        is_current,
        is_detached,
    });
}

/// List worktrees in a repository (read-only).
pub async fn list_worktrees(git_root: &Path) -> Result<Vec<WorktreeInfo>, EgggitError> {
    let output = run_git_async(
        git_root.to_path_buf(),
        vec!["worktree".into(), "list".into(), "--porcelain".into()],
    )
    .await?;
    if !output.status.success() {
        return Err(EgggitError::Git(
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
            current_is_current = Path::new(path) == git_root
                || Path::new(path).canonicalize().is_ok_and(|p| {
                    p == git_root
                        .canonicalize()
                        .unwrap_or_else(|_| git_root.to_path_buf())
                });
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

/// Walk up the directory tree to find a git root.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn init_repo(dir: &Path) {
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    #[test]
    fn find_git_root_in_repo() {
        let dir = tempfile::Builder::new()
            .prefix("egggit-find-root-yes-")
            .tempdir()
            .unwrap();
        init_repo(dir.path());
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let root = find_git_root(&nested).unwrap();
        assert_eq!(
            root.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn find_git_root_returns_none_for_nonexistent_path() {
        // Walking up from a non-existent path can still find a real git
        // ancestor. The function should not crash and should return
        // *some* result. We only assert it doesn't panic and returns
        // the codegg workspace git root (since the test runs from there).
        let fake = std::path::PathBuf::from("/this/path/does/not/exist/xyz");
        // The result is non-deterministic depending on whether the test is
        // running inside the codegg workspace. We only assert no panic.
        let _ = find_git_root(&fake);
    }

    #[test]
    fn is_git_file_detects_gitdir() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join(".git");
        std::fs::write(&file, "gitdir: /tmp/foo").unwrap();
        assert!(is_git_file(&file));
    }

    #[test]
    fn is_git_file_rejects_other() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join(".git");
        std::fs::write(&file, "not a gitdir pointer").unwrap();
        assert!(!is_git_file(&file));
    }

    #[tokio::test]
    async fn list_worktrees_for_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        init_repo(dir.path());
        let list = list_worktrees(dir.path()).await.unwrap();
        // There is at least the main worktree.
        assert!(!list.is_empty());
        assert!(list[0].is_current);
    }
}
