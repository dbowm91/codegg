use std::path::PathBuf;
use std::process::Command;

use crate::worktree;

#[derive(Debug, Clone)]
pub struct GitSession {
    pub session_id: String,
    pub worktree_path: Option<PathBuf>,
    pub git_root: PathBuf,
    pub status: GitStatus,
    pub auto_worktree: bool,
}

#[derive(Debug, Clone)]
pub struct GitStatus {
    pub branch: String,
    pub is_dirty: bool,
    pub commit_hash: Option<String>,
    pub stash_count: usize,
}

impl GitSession {
    pub fn new(session_id: String, git_root: PathBuf, auto_worktree: bool) -> Result<Self, String> {
        let status = Self::run_git_status(&git_root)?;
        let worktree_path = if auto_worktree {
            Some(git_root.join(".git").join("worktrees").join(&session_id))
        } else {
            None
        };

        Ok(Self {
            session_id,
            worktree_path,
            git_root,
            status,
            auto_worktree,
        })
    }

    fn run_git_status(git_root: &PathBuf) -> Result<GitStatus, String> {
        let branch = Self::get_branch(git_root);
        let is_dirty = Self::check_dirty(git_root);
        let commit_hash = Self::get_commit_hash(git_root);
        let stash_count = Self::get_stash_count(git_root);

        Ok(GitStatus {
            branch,
            is_dirty,
            commit_hash,
            stash_count,
        })
    }

    fn get_branch(git_root: &PathBuf) -> String {
        let output = Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .args(["branch", "--show-current"])
            .current_dir(git_root)
            .output()
            .ok();
        output
            .and_then(|o| {
                if o.status.success() {
                    let branch = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if branch.is_empty() {
                        Some("detached".to_string())
                    } else {
                        Some(branch)
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn check_dirty(git_root: &PathBuf) -> bool {
        let output = Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .args(["status", "--porcelain"])
            .current_dir(git_root)
            .output()
            .ok();
        output.is_some_and(|o| !o.stdout.is_empty())
    }

    fn get_commit_hash(git_root: &PathBuf) -> Option<String> {
        let output = Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .args(["rev-parse", "HEAD"])
            .current_dir(git_root)
            .output()
            .ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    fn get_stash_count(git_root: &PathBuf) -> usize {
        let output = Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .args(["stash", "list"])
            .current_dir(git_root)
            .output()
            .ok();
        output.map_or(0, |o| {
            if o.status.success() {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|line| line.starts_with("stash@"))
                    .count()
            } else {
                0
            }
        })
    }

    pub fn refresh_status(&mut self) -> Result<(), String> {
        self.status = Self::run_git_status(&self.git_root)?;
        Ok(())
    }

    pub fn worktree_path(&self) -> Option<&PathBuf> {
        self.worktree_path.as_ref()
    }

    pub fn create_worktree(&self, branch: &str) -> Result<(), String> {
        let Some(worktree_path) = &self.worktree_path else {
            return Err("Worktree disabled for this session".to_string());
        };

        worktree::create_worktree(&self.git_root, worktree_path, branch, true)
            .map_err(|e| e.to_string())
    }

    pub fn remove_worktree(&self) -> Result<(), String> {
        let Some(worktree_path) = &self.worktree_path else {
            return Ok(());
        };

        if worktree_path.exists() {
            worktree::remove_worktree(&self.git_root, worktree_path, true)
                .map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }

    pub fn format_for_prompt(&self) -> String {
        let status_str = if self.status.is_dirty {
            "dirty (uncommitted changes)"
        } else {
            "clean"
        };

        let mut info = format!(
            "[Git Info]\nBranch: {}\nStatus: {}",
            self.status.branch, status_str
        );

        if let Some(ref hash) = self.status.commit_hash {
            info.push_str(&format!("\nCommit: {}", hash));
        }

        if self.status.stash_count > 0 {
            info.push_str(&format!("\nStash: {} entries", self.status.stash_count));
        }

        if let Some(ref wt) = self.worktree_path {
            info.push_str(&format!("\nWorktree: {}/", wt.display()));
        }

        info
    }
}

pub fn get_git_root(start: &std::path::Path) -> Option<PathBuf> {
    worktree::find_git_root(start)
}