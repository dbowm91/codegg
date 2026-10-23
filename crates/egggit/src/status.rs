use crate::consistency::{GitReadConsistency, GitReadFact};
use crate::process::GitCommandOutput;
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
) -> Result<GitCommandOutput, EgggitError> {
    crate::process::run(&args, &root).await
}

async fn capture_stdout(root: &Path, args: &[&str]) -> Result<GitReadFact<String>, EgggitError> {
    let root = root.to_path_buf();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let output = run_git_async(root, args).await?;
    if !output.status.success() {
        return Err(EgggitError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(GitReadFact::new(stdout, output.consistency))
}

/// Fold the classes of a multi-spawn read into the composite class.
///
/// The composite is the **weakest** class among the contributing reads: a
/// read assembled from several spawns is only as strong as its weakest
/// part. A contributing read that produced no result at all observed no
/// object generation, so it contributes
/// [`GitReadConsistency::NoObjectGeneration`].
fn composite_class(contributions: &[Option<GitReadConsistency>]) -> GitReadConsistency {
    let mut composite: Option<GitReadConsistency> = None;
    for contribution in contributions {
        let class = contribution.unwrap_or(GitReadConsistency::NoObjectGeneration);
        composite = Some(match composite {
            Some(accumulated) => accumulated.weaker(class),
            None => class,
        });
    }
    composite.unwrap_or(GitReadConsistency::NoObjectGeneration)
}

/// Read the high-level status of a repository: branch, dirtiness, head hash,
/// and stash count. Returns `Ok` even when fields are not parseable (in which
/// case defaults are used) but errors if the directory does not exist or git
/// is not installed.
///
/// The result carries the consistency class of the read: the composite class
/// of the four contributing spawns, i.e. the weakest of them.
pub async fn repo_status(root: &Path) -> Result<GitReadFact<RepoStatus>, EgggitError> {
    if !root.exists() {
        return Err(EgggitError::NotARepository(root.display().to_string()));
    }

    let branch_fact = capture_stdout(root, &["branch", "--show-current"])
        .await
        .ok();
    let status_fact = capture_stdout(root, &["status", "--porcelain"]).await.ok();
    let head_fact = capture_stdout(root, &["rev-parse", "HEAD"]).await.ok();
    let stash_fact = capture_stdout(root, &["stash", "list"]).await.ok();

    let branch = match &branch_fact {
        Some(fact) => {
            let trimmed = fact.value().trim();
            if trimmed.is_empty() {
                "detached".to_string()
            } else {
                trimmed.to_string()
            }
        }
        None => "unknown".to_string(),
    };

    let is_dirty = match &status_fact {
        Some(fact) => !fact.value().is_empty(),
        None => false,
    };

    let commit_hash = head_fact
        .as_ref()
        .map(|fact| fact.value().trim().to_string())
        .filter(|s| !s.is_empty());

    let stash_count = match &stash_fact {
        Some(fact) => fact
            .value()
            .lines()
            .filter(|line| line.starts_with("stash@"))
            .count(),
        None => 0,
    };

    let composite = composite_class(&[
        branch_fact.as_ref().map(GitReadFact::class),
        status_fact.as_ref().map(GitReadFact::class),
        head_fact.as_ref().map(GitReadFact::class),
        stash_fact.as_ref().map(GitReadFact::class),
    ]);

    Ok(GitReadFact::new(
        RepoStatus {
            branch,
            is_dirty,
            commit_hash,
            stash_count,
        },
        composite,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
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
    async fn trusted_git_status_disables_repository_fsmonitor() {
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
        // marker-writing sleeper proves the trusted route disables it.
        let marker = root.join("fsmonitor-ran");
        let hook = root.join("hang.sh");
        std::fs::write(
            &hook,
            format!("#!/bin/sh\ntouch '{}'\nsleep 120\n", marker.display()),
        )
        .expect("write hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
                .expect("chmod hook");
        }
        run(&["config", "core.fsmonitor", hook.to_str().unwrap()]);

        let started = Instant::now();
        // Short bound so a policy regression fails quickly; production uses
        // `DEFAULT_READ_TIMEOUT` (30s).
        let result =
            crate::process::run_with_timeout(&["status".to_string()], root, Duration::from_secs(2))
                .await;
        let elapsed = started.elapsed();

        assert!(
            result.is_ok_and(|output| output.status.success()),
            "trusted status should succeed with repository fsmonitor disabled"
        );
        assert!(
            elapsed < Duration::from_secs(20),
            "status must not wait for the hostile fsmonitor, took {elapsed:?}"
        );
        assert!(!marker.exists(), "repository fsmonitor must not execute");
    }

    /// The status read is assembled from four spawns, and the composite
    /// class is the weakest of them.
    ///
    /// `git branch --show-current` consults no object store, so the
    /// composite read observed no object generation — the class has to say
    /// so rather than borrow the stronger class of the other three spawns.
    #[tokio::test(flavor = "current_thread")]
    async fn repo_status_carries_the_weakest_contributing_class() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .expect("git");
        };
        run(&["init", "--initial-branch=main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);

        let fact = repo_status(root).await.expect("status read");
        assert_eq!(fact.value().branch, "main");
        assert_eq!(
            fact.class(),
            GitReadConsistency::NoObjectGeneration,
            "the composite class must be the weakest contributing class"
        );
    }
}
