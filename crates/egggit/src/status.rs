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
    crate::process::run(&args, &root).await
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    /// End-to-end proof that a Git command which never exits is now bounded.
    ///
    /// Regression: `git_query` on a new repo could hang indefinitely because
    /// every structured read went through an unbounded `spawn_blocking` wait.
    /// The reported symptom was a single call stuck for 531s.
    ///
    /// A `core.fsmonitor` hook is used because it runs inside an otherwise
    /// ordinary `git status` — the exact shape of a slow/hung repository
    /// read, rather than a synthetic `sleep`.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_hanging_git_command_is_killed_at_the_deadline() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .expect("git init");
        };
        run(&["init", "--initial-branch=main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);

        // `core.fsmonitor` is consulted by `git status`; pointing it at a
        // sleeping program makes the read hang exactly like a wedged repo.
        let hook = root.join("hang.sh");
        std::fs::write(&hook, "#!/bin/sh\nsleep 120\n").expect("write hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
                .expect("chmod hook");
        }
        run(&["config", "core.fsmonitor", hook.to_str().unwrap()]);

        let started = Instant::now();
        // Short bound so the test stays fast; production uses
        // `DEFAULT_READ_TIMEOUT` (30s).
        let result =
            crate::process::run_with_timeout(&["status".to_string()], root, Duration::from_secs(2))
                .await;
        let elapsed = started.elapsed();

        assert!(
            result.is_err(),
            "a hung read must surface as an error, not hang forever"
        );
        assert!(
            matches!(result, Err(crate::EgggitError::Timeout(_))),
            "the failure must be a timeout, got {:?}",
            result.err()
        );
        assert!(
            elapsed < Duration::from_secs(20),
            "the deadline must be enforced promptly, took {elapsed:?}"
        );
    }
}
