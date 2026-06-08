use crate::EgggitError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoStatus {
    pub branch: String,
    pub is_dirty: bool,
    pub commit_hash: Option<String>,
    pub stash_count: usize,
}

impl Default for RepoStatus {
    fn default() -> Self {
        Self {
            branch: "unknown".to_string(),
            is_dirty: false,
            commit_hash: None,
            stash_count: 0,
        }
    }
}

async fn run_git_async(
    root: std::path::PathBuf,
    args: Vec<String>,
) -> Result<std::process::Output, EgggitError> {
    tokio::task::spawn_blocking(move || {
        if !root.exists() {
            return Err(EgggitError::NotARepository(root.display().to_string()));
        }
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

async fn capture_stdout(root: &Path, args: &[&str]) -> Result<String, EgggitError> {
    let root = root.to_path_buf();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let output = run_git_async(root, args).await?;
    if !output.status.success() {
        return Err(EgggitError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Read the high-level status of a repository: branch, dirtiness, head hash,
/// and stash count. Returns `Ok` even when fields are not parseable (in which
/// case defaults are used) but errors if the directory does not exist or git
/// is not installed.
pub async fn repo_status(root: &Path) -> Result<RepoStatus, EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }

    let branch = {
        let out = capture_stdout(root, &["branch", "--show-current"]).await;
        match out {
            Ok(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    "detached".to_string()
                } else {
                    trimmed.to_string()
                }
            }
            Err(_) => "unknown".to_string(),
        }
    };

    let is_dirty = {
        let out = capture_stdout(root, &["status", "--porcelain"]).await;
        out.map(|s| !s.is_empty()).unwrap_or(false)
    };

    let commit_hash = capture_stdout(root, &["rev-parse", "HEAD"])
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let stash_count = {
        let out = capture_stdout(root, &["stash", "list"]).await;
        out.map(|s| s.lines().filter(|line| line.starts_with("stash@")).count())
            .unwrap_or(0)
    };

    Ok(RepoStatus {
        branch,
        is_dirty,
        commit_hash,
        stash_count,
    })
}
