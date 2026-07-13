//! Unified Git execution service.
//!
//! This module provides the [`GitExecutionService`] which executes typed
//! [`GitOperation`]s and returns structured results. It delegates to `egggit`
//! for read-only operations and falls back to subprocess execution for mutations
//! and unsupported operations.

use std::path::Path;
use std::time::Duration;

use codegg_git::{render_argv, GitOperation};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// The result of executing a git operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitExecutionResult {
    /// The operation that was executed (display label).
    pub operation: String,
    /// Structured payload (when available for read operations).
    pub payload: Option<GitPayload>,
    /// Raw stdout from the git command.
    pub stdout: String,
    /// Raw stderr from the git command.
    pub stderr: String,
    /// Exit code (0 = success).
    pub exit_code: i32,
    /// Repository root where the command was executed.
    pub repository_root: String,
    /// Whether the operation completed successfully.
    pub success: bool,
    /// Projection hints for the TUI/model.
    pub projection_hints: ProjectionHints,
}

/// Structured payload variants for different operation types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GitPayload {
    /// High-level repo status.
    Status(RepoStatusPayload),
    /// Diff summary with file changes.
    DiffSummary(DiffSummaryPayload),
    /// Raw diff text.
    DiffText(String),
    /// Changed files list.
    ChangedFiles(Vec<ChangedFilePayload>),
    /// Log entries.
    Log(Vec<CommitInfoPayload>),
    /// Branch list.
    Branches(Vec<BranchPayload>),
    /// Tag list.
    Tags(Vec<String>),
    /// Remote list.
    Remotes(Vec<RemotePayload>),
    /// Worktree list.
    Worktrees(Vec<WorktreePayload>),
    /// Stash list.
    Stashes(Vec<StashPayload>),
    /// No structured payload (raw output only).
    None,
}

/// High-level repository status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoStatusPayload {
    /// Current branch name.
    pub branch: String,
    /// Whether the working tree has uncommitted changes.
    pub is_dirty: bool,
    /// HEAD commit hash (if available).
    pub commit_hash: Option<String>,
    /// Number of stash entries.
    pub stash_count: usize,
}

/// Diff summary with file-level details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSummaryPayload {
    /// Number of files changed.
    pub files_changed: usize,
    /// Number of insertions.
    pub insertions: usize,
    /// Number of deletions.
    pub deletions: usize,
    /// Per-file change info.
    pub files: Vec<ChangedFilePayload>,
}

/// A single changed file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFilePayload {
    /// File path relative to repo root.
    pub path: String,
    /// Kind of change.
    pub kind: String,
}

/// A single log commit entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfoPayload {
    /// Commit hash.
    pub hash: String,
    /// Short commit message.
    pub message: String,
}

/// A branch entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchPayload {
    /// Branch name.
    pub name: String,
    /// Whether this is the current branch.
    pub current: bool,
}

/// A remote entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePayload {
    /// Remote name.
    pub name: String,
    /// Remote URL (if fetched).
    pub url: Option<String>,
}

/// A worktree entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreePayload {
    /// Worktree path.
    pub path: String,
    /// Branch name.
    pub branch: String,
    /// Whether this is the current worktree.
    pub is_current: bool,
    /// Whether HEAD is detached.
    pub is_detached: bool,
}

/// A stash entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StashPayload {
    /// Stash ref (e.g. `stash@{0}`).
    pub ref_name: String,
    /// Branch the stash was created on.
    pub branch: Option<String>,
    /// Commit message.
    pub message: Option<String>,
}

/// Hints for projection formatting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectionHints {
    /// Suggested projector route (e.g. "git-status", "git-diff", "git-log").
    pub projector: Option<String>,
    /// Whether this result is suitable for truncation.
    pub truncatable: bool,
    /// Estimated output size in bytes.
    pub estimated_size: usize,
}

/// The unified git execution service.
pub struct GitExecutionService {
    timeout: Duration,
}

impl GitExecutionService {
    /// Create a new service with default 30-second timeout.
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }

    /// Create a service with a custom timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Execute a typed [`GitOperation`] and return structured results.
    ///
    /// For read operations, delegates to `egggit` functions for structured
    /// payloads. For mutations and unsupported operations, falls back to
    /// raw subprocess execution via [`codegg_git::render_argv`].
    pub async fn execute(
        &self,
        operation: &GitOperation,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        if !repository_root.exists() {
            return Err(GitServiceError::Repository(format!(
                "repository root does not exist: {}",
                repository_root.display()
            )));
        }

        match operation {
            // Read operations → delegate to egggit for structured results
            GitOperation::Status { short } => self.execute_status(*short, repository_root).await,
            GitOperation::Diff { .. } => self.execute_diff(operation, repository_root).await,
            GitOperation::DiffStaged { .. } => self.execute_diff(operation, repository_root).await,
            GitOperation::Log { .. } => self.execute_log(operation, repository_root).await,
            GitOperation::Blame { path } => {
                self.execute_blame(path.as_str(), repository_root).await
            }
            GitOperation::ChangedFiles { .. } => {
                self.execute_changed_files(operation, repository_root).await
            }
            GitOperation::BranchList { .. } => {
                self.execute_branches(operation, repository_root).await
            }
            GitOperation::TagList => self.execute_tags(repository_root).await,
            GitOperation::RemoteList | GitOperation::RemoteGetUrl { .. } => {
                self.execute_remotes(operation, repository_root).await
            }
            GitOperation::WorktreeList => self.execute_worktrees(repository_root).await,
            GitOperation::StashList => self.execute_stash_list(repository_root).await,
            // Fallback: raw subprocess execution for mutations and unsupported
            _ => self.execute_raw(operation, repository_root).await,
        }
    }

    // ── Read operations via egggit ──────────────────────────────────

    async fn execute_status(
        &self,
        short: bool,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(&GitOperation::Status { short });
        let op_label = "status".to_string();

        let repo_status = egggit::status::repo_status(repository_root)
            .await
            .ok()
            .map(|s| RepoStatusPayload {
                branch: s.branch,
                is_dirty: s.is_dirty,
                commit_hash: s.commit_hash,
                stash_count: s.stash_count,
            });

        let raw = self.run_git_raw(&argv, repository_root).await?;
        let payload = repo_status
            .map(GitPayload::Status)
            .unwrap_or(GitPayload::None);

        Ok(GitExecutionResult {
            operation: op_label,
            payload: Some(payload),
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: Some("git-status".to_string()),
                truncatable: true,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    async fn execute_diff(
        &self,
        operation: &GitOperation,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(operation);
        let op_label = operation.subcommand_name().to_string();

        // Try egggit for structured summary
        let (base_ref, is_stat, is_name_only) = match operation {
            GitOperation::Diff {
                base_ref,
                stat,
                name_only,
                ..
            } => (base_ref.as_ref().map(|r| r.as_str()), *stat, *name_only),
            GitOperation::DiffStaged {
                stat, name_only, ..
            } => (None, *stat, *name_only),
            _ => (None, false, false),
        };

        let diff_summary = if !is_stat && !is_name_only {
            egggit::diff::diff_summary(repository_root, base_ref)
                .await
                .ok()
                .map(|s| DiffSummaryPayload {
                    files_changed: s.files_changed,
                    insertions: s.insertions,
                    deletions: s.deletions,
                    files: s
                        .files
                        .iter()
                        .map(|f| ChangedFilePayload {
                            path: f.path.clone(),
                            kind: format!("{:?}", f.kind).to_lowercase(),
                        })
                        .collect(),
                })
        } else {
            None
        };

        let raw = self.run_git_raw(&argv, repository_root).await?;

        let payload = if let Some(summary) = diff_summary {
            Some(GitPayload::DiffSummary(summary))
        } else {
            Some(GitPayload::DiffText(raw.stdout.clone()))
        };

        Ok(GitExecutionResult {
            operation: op_label,
            payload,
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: Some("git-diff".to_string()),
                truncatable: true,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    async fn execute_log(
        &self,
        operation: &GitOperation,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(operation);
        let op_label = "log".to_string();

        let raw = self.run_git_raw(&argv, repository_root).await?;

        // Parse log output into structured entries
        let commits = parse_log_output(&raw.stdout);

        Ok(GitExecutionResult {
            operation: op_label,
            payload: Some(GitPayload::Log(commits)),
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: Some("git-log".to_string()),
                truncatable: true,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    async fn execute_blame(
        &self,
        path: &str,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(&GitOperation::Blame {
            path: codegg_git::path::RepoPath::new(
                &codegg_git::path::RepoRoot::new(repository_root.to_str().unwrap_or("."))
                    .map_err(|e| GitServiceError::Repository(e.to_string()))?,
                path,
            )
            .map_err(|e| GitServiceError::Repository(e.to_string()))?,
        });
        let op_label = "blame".to_string();

        let raw = self.run_git_raw(&argv, repository_root).await?;

        Ok(GitExecutionResult {
            operation: op_label,
            payload: None,
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: None,
                truncatable: true,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    async fn execute_changed_files(
        &self,
        operation: &GitOperation,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(operation);
        let op_label = "changed-files".to_string();

        let changed = egggit::diff::changed_files(repository_root, None)
            .await
            .ok()
            .map(|files| {
                files
                    .iter()
                    .map(|f| ChangedFilePayload {
                        path: f.path.clone(),
                        kind: format!("{:?}", f.kind).to_lowercase(),
                    })
                    .collect::<Vec<_>>()
            });

        let raw = self.run_git_raw(&argv, repository_root).await?;

        let payload = changed
            .map(GitPayload::ChangedFiles)
            .unwrap_or(GitPayload::None);

        Ok(GitExecutionResult {
            operation: op_label,
            payload: Some(payload),
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: Some("git-status".to_string()),
                truncatable: true,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    async fn execute_branches(
        &self,
        operation: &GitOperation,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(operation);
        let op_label = "branch".to_string();

        let raw = self.run_git_raw(&argv, repository_root).await?;

        // Parse branch output: lines starting with "* " are current
        let branches = raw
            .stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|line| {
                let current = line.starts_with("* ");
                let name = if current {
                    line[2..].to_string()
                } else {
                    line.to_string()
                };
                BranchPayload { name, current }
            })
            .collect();

        Ok(GitExecutionResult {
            operation: op_label,
            payload: Some(GitPayload::Branches(branches)),
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: Some("git-status".to_string()),
                truncatable: false,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    async fn execute_tags(
        &self,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(&GitOperation::TagList);
        let op_label = "tag".to_string();

        let raw = self.run_git_raw(&argv, repository_root).await?;

        let tags: Vec<String> = raw
            .stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();

        Ok(GitExecutionResult {
            operation: op_label,
            payload: Some(GitPayload::Tags(tags)),
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: Some("git-status".to_string()),
                truncatable: true,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    async fn execute_remotes(
        &self,
        operation: &GitOperation,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(operation);
        let op_label = "remote".to_string();

        let raw = self.run_git_raw(&argv, repository_root).await?;

        // For RemoteList, parse names; for RemoteGetUrl, it's a single URL
        let payload = match operation {
            GitOperation::RemoteList => {
                let remotes: Vec<RemotePayload> = raw
                    .stdout
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|name| RemotePayload {
                        name: name.to_string(),
                        url: None,
                    })
                    .collect();
                GitPayload::Remotes(remotes)
            }
            GitOperation::RemoteGetUrl { remote } => {
                let url = raw.stdout.trim().to_string();
                GitPayload::Remotes(vec![RemotePayload {
                    name: remote.as_str().to_string(),
                    url: Some(url),
                }])
            }
            _ => GitPayload::None,
        };

        Ok(GitExecutionResult {
            operation: op_label,
            payload: Some(payload),
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: Some("git-status".to_string()),
                truncatable: false,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    async fn execute_worktrees(
        &self,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(&GitOperation::WorktreeList);
        let op_label = "worktree".to_string();

        let worktrees = egggit::worktree::list_worktrees(repository_root)
            .await
            .ok()
            .map(|list| {
                list.iter()
                    .map(|w| WorktreePayload {
                        path: w.path.clone(),
                        branch: w.branch.clone(),
                        is_current: w.is_current,
                        is_detached: w.is_detached,
                    })
                    .collect::<Vec<_>>()
            });

        let raw = self.run_git_raw(&argv, repository_root).await?;

        let payload = worktrees
            .map(GitPayload::Worktrees)
            .unwrap_or(GitPayload::None);

        Ok(GitExecutionResult {
            operation: op_label,
            payload: Some(payload),
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: Some("git-status".to_string()),
                truncatable: true,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    async fn execute_stash_list(
        &self,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(&GitOperation::StashList);
        let op_label = "stash".to_string();

        let raw = self.run_git_raw(&argv, repository_root).await?;

        // Parse stash output: "stash@{0}: On branch: message"
        let stashes: Vec<StashPayload> = raw
            .stdout
            .lines()
            .filter(|l| l.starts_with("stash@"))
            .enumerate()
            .map(|(i, line)| {
                let ref_name = format!("stash@{{{i}}}");
                // Try to extract the branch and message after ": "
                let rest = line.split_once(": ").map(|(_, r)| r).unwrap_or("");
                let branch = rest
                    .split_once(": ")
                    .map(|(before, _)| before)
                    .unwrap_or(rest)
                    .strip_prefix("On ")
                    .map(|b| b.to_string());
                let message = rest.split_once(": ").map(|(_, r)| r.to_string());
                StashPayload {
                    ref_name,
                    branch,
                    message,
                }
            })
            .collect();

        Ok(GitExecutionResult {
            operation: op_label,
            payload: Some(GitPayload::Stashes(stashes)),
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: Some("git-status".to_string()),
                truncatable: true,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    // ── Raw subprocess fallback ─────────────────────────────────────

    async fn execute_raw(
        &self,
        operation: &GitOperation,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(operation);
        let op_label = operation.subcommand_name().to_string();

        let raw = self.run_git_raw(&argv, repository_root).await?;

        Ok(GitExecutionResult {
            operation: op_label,
            payload: None,
            stdout: raw.stdout.clone(),
            stderr: raw.stderr,
            exit_code: raw.exit_code,
            repository_root: repository_root.display().to_string(),
            success: raw.exit_code == 0,
            projection_hints: ProjectionHints {
                projector: None,
                truncatable: true,
                estimated_size: raw.stdout.len(),
            },
        })
    }

    // ── Internal helpers ────────────────────────────────────────────

    /// Run a git command and capture stdout/stderr as raw strings.
    async fn run_git_raw(
        &self,
        argv: &[String],
        repository_root: &Path,
    ) -> Result<RawGitOutput, GitServiceError> {
        if argv.is_empty() {
            return Err(GitServiceError::Execution("empty argv".to_string()));
        }

        let root = repository_root.to_path_buf();
        let argv_owned = argv.to_vec();
        let timeout = self.timeout;

        let output = tokio::time::timeout(timeout, async {
            let mut cmd = Command::new(&argv_owned[0]);
            cmd.args(&argv_owned[1..]);
            cmd.current_dir(&root);
            // Harden environment: clear and restore only PATH.
            cmd.env_clear();
            if let Some(path) = std::env::var_os("PATH") {
                cmd.env("PATH", path);
            } else {
                cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
            }
            // Prevent git from prompting for credentials.
            cmd.env("GIT_TERMINAL_PROMPT", "0");
            cmd.kill_on_drop(true);
            cmd.output().await
        })
        .await
        .map_err(|_| {
            GitServiceError::Timeout(format!(
                "git command timed out after {}s: {}",
                timeout.as_secs(),
                argv.join(" ")
            ))
        })?
        .map_err(|e| GitServiceError::Execution(format!("failed to spawn git: {e}")))?;

        Ok(RawGitOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

impl Default for GitExecutionService {
    fn default() -> Self {
        Self::new()
    }
}

/// Raw captured output from a git subprocess.
struct RawGitOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

// ── Output parsing helpers ───────────────────────────────────────

/// Parse `git log` output (one-line format) into structured commit entries.
fn parse_log_output(stdout: &str) -> Vec<CommitInfoPayload> {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            // Format: "abc1234 commit message here"
            let mut parts = line.splitn(2, ' ');
            let hash = parts.next()?.to_string();
            let message = parts.next().unwrap_or("").to_string();
            Some(CommitInfoPayload { hash, message })
        })
        .collect()
}

// ── Error type ───────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum GitServiceError {
    #[error("git service error: {0}")]
    Execution(String),
    #[error("repository error: {0}")]
    Repository(String),
    #[error("timeout: {0}")]
    Timeout(String),
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_git::path::RepoRoot;
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    fn init_repo(dir: &Path) {
        StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    fn commit_file(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        StdCommand::new("git")
            .args(["add", name])
            .current_dir(dir)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["commit", "-m", &format!("add {name}")])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_status_returns_structured_payload() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "hello");

        let svc = GitExecutionService::new();
        let result = svc
            .execute(&GitOperation::Status { short: false }, dir.path())
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.exit_code, 0);
        assert!(result.payload.is_some());
        match result.payload.unwrap() {
            GitPayload::Status(status) => {
                assert_eq!(status.branch, "main");
                assert!(!status.is_dirty);
            }
            other => panic!("expected Status payload, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_diff_returns_summary() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "hello");
        std::fs::write(dir.path().join("a.txt"), "hello world").unwrap();

        let svc = GitExecutionService::new();
        let result = svc
            .execute(
                &GitOperation::Diff {
                    staged: false,
                    stat: false,
                    name_only: false,
                    base_ref: None,
                    paths: vec![],
                },
                dir.path(),
            )
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::DiffSummary(summary) => {
                assert_eq!(summary.files_changed, 1);
            }
            other => panic!("expected DiffSummary, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_log_returns_commits() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "first");
        commit_file(dir.path(), "b.txt", "second");

        let svc = GitExecutionService::new();
        let result = svc
            .execute(
                &GitOperation::Log {
                    oneline: true,
                    max_count: Some(2),
                    paths: vec![],
                },
                dir.path(),
            )
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::Log(commits) => {
                assert_eq!(commits.len(), 2);
                assert!(!commits[0].hash.is_empty());
            }
            other => panic!("expected Log payload, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_branches_lists_branches() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "content");

        let svc = GitExecutionService::new();
        let result = svc
            .execute(
                &GitOperation::BranchList {
                    remotes: false,
                    all: false,
                },
                dir.path(),
            )
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::Branches(branches) => {
                assert!(!branches.is_empty());
                assert!(branches.iter().any(|b| b.current && b.name == "main"));
            }
            other => panic!("expected Branches, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_tags_lists_tags() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "content");
        StdCommand::new("git")
            .args(["tag", "v1.0"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let svc = GitExecutionService::new();
        let result = svc
            .execute(&GitOperation::TagList, dir.path())
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::Tags(tags) => {
                assert!(tags.contains(&"v1.0".to_string()));
            }
            other => panic!("expected Tags, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_worktrees_lists_worktrees() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "content");

        let svc = GitExecutionService::new();
        let result = svc
            .execute(&GitOperation::WorktreeList, dir.path())
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::Worktrees(worktrees) => {
                assert!(!worktrees.is_empty());
                assert!(worktrees.iter().any(|w| w.is_current));
            }
            other => panic!("expected Worktrees, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_stash_list_parses_stashes() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "first");
        std::fs::write(dir.path().join("a.txt"), "modified").unwrap();
        StdCommand::new("git")
            .args(["stash", "push", "-m", "wip"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let svc = GitExecutionService::new();
        let result = svc
            .execute(&GitOperation::StashList, dir.path())
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::Stashes(stashes) => {
                assert!(!stashes.is_empty());
                assert_eq!(stashes[0].ref_name, "stash@{0}");
            }
            other => panic!("expected Stashes, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_add_returns_raw_output() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("new.txt"), "new file").unwrap();

        let root = RepoRoot::new(dir.path().to_str().unwrap()).unwrap();
        let path = codegg_git::path::RepoPath::new(&root, "new.txt").unwrap();

        let svc = GitExecutionService::new();
        let result = svc
            .execute(&GitOperation::Add { paths: vec![path] }, dir.path())
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.payload.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_nonexistent_repo_errors() {
        let dir = TempDir::new().unwrap();
        let fake = dir.path().join("nonexistent");

        let svc = GitExecutionService::new();
        let result = svc
            .execute(&GitOperation::Status { short: false }, &fake)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_git_failure_returns_nonzero_exit() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());

        // git log on empty repo with --all might fail or succeed depending on version.
        // Use a guaranteed failure: git show with nonexistent ref.
        let svc = GitExecutionService::new();
        let result = svc
            .execute(
                &GitOperation::Show {
                    rev: codegg_git::ref_name::RevisionExpr::new("nonexistent-ref-xyz").unwrap(),
                },
                dir.path(),
            )
            .await
            .unwrap();

        assert!(!result.success);
        assert_ne!(result.exit_code, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn with_timeout_configures_timeout() {
        let svc = GitExecutionService::new().with_timeout(Duration::from_secs(5));
        assert_eq!(svc.timeout, Duration::from_secs(5));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn changed_files_returns_payload() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "first");
        std::fs::write(dir.path().join("a.txt"), "modified").unwrap();
        std::fs::write(dir.path().join("b.txt"), "new").unwrap();

        let svc = GitExecutionService::new();
        let result = svc
            .execute(&GitOperation::ChangedFiles { base_ref: None }, dir.path())
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::ChangedFiles(files) => {
                assert!(!files.is_empty());
            }
            other => panic!("expected ChangedFiles, got {:?}", other),
        }
    }

    #[test]
    fn projection_hints_default() {
        let hints = ProjectionHints::default();
        assert!(hints.projector.is_none());
        assert!(!hints.truncatable);
        assert_eq!(hints.estimated_size, 0);
    }

    #[test]
    fn parse_log_output_extracts_commits() {
        let output = "abc1234 first commit\ndef5678 second commit\n";
        let commits = parse_log_output(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc1234");
        assert_eq!(commits[0].message, "first commit");
        assert_eq!(commits[1].hash, "def5678");
    }

    #[test]
    fn parse_log_output_handles_empty() {
        let commits = parse_log_output("");
        assert!(commits.is_empty());
    }

    #[test]
    fn service_error_display() {
        let err = GitServiceError::Execution("test".into());
        assert!(err.to_string().contains("test"));

        let err = GitServiceError::Repository("path".into());
        assert!(err.to_string().contains("path"));

        let err = GitServiceError::Timeout("30s".into());
        assert!(err.to_string().contains("30s"));
    }
}
