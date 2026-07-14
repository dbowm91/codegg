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

use crate::git_mutations::GitEnvPolicy;
use crate::git_network_policy::redact_url_credentials_in_text;

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
    /// Structured diff result with parsed files and hunks.
    DiffResult(DiffResultPayload),
    /// Structured show result.
    Show(ShowPayload),
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

/// A structured diff result with parsed files and hunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResultPayload {
    /// Files changed in the diff.
    pub files: Vec<DiffFilePayload>,
    /// Total insertions across all files.
    pub total_insertions: usize,
    /// Total deletions across all files.
    pub total_deletions: usize,
}

/// A single file within a structured diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFilePayload {
    /// File path.
    pub path: String,
    /// Old file path (for renames/copies).
    pub old_path: Option<String>,
    /// Kind of change (added, modified, deleted, renamed, copied).
    pub kind: String,
    /// Whether this is a binary file.
    pub is_binary: bool,
    /// Number of lines added.
    pub additions: usize,
    /// Number of lines removed.
    pub deletions: usize,
    /// Parsed hunks (empty for binary files or stat-only diffs).
    pub hunks: Vec<DiffHunkPayload>,
}

/// A single hunk within a diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunkPayload {
    /// Old file start line.
    pub old_start: u32,
    /// Old file line count.
    pub old_lines: u32,
    /// New file start line.
    pub new_start: u32,
    /// New file line count.
    pub new_lines: u32,
    /// Hunk content lines (with +/-/ prefix).
    pub lines: Vec<String>,
}

/// Structured payload for `git show`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowPayload {
    /// Commit hash.
    pub hash: String,
    /// Author name.
    pub author: String,
    /// Author date.
    pub date: String,
    /// Commit message.
    pub message: String,
    /// Files changed (if --stat or patch included).
    pub files: Vec<ChangedFilePayload>,
    /// Raw patch text.
    pub patch: String,
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
#[derive(Clone)]
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
            GitOperation::Show { rev } => self.execute_show(rev.as_str(), repository_root).await,
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

        // For non-stat, non-name-only diffs, try to parse unified diff output
        let payload = if !is_stat && !is_name_only {
            if let Some(summary) = diff_summary {
                // Try structured parsing of unified diff for richer results
                if raw.stdout.contains("diff --git ") {
                    match parse_diff_output(&raw.stdout) {
                        Ok(diff_result) => Some(GitPayload::DiffResult(diff_result)),
                        Err(_) => Some(GitPayload::DiffSummary(summary)),
                    }
                } else {
                    Some(GitPayload::DiffSummary(summary))
                }
            } else {
                Some(GitPayload::DiffText(raw.stdout.clone()))
            }
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

    async fn execute_show(
        &self,
        rev: &str,
        repository_root: &Path,
    ) -> Result<GitExecutionResult, GitServiceError> {
        let argv = render_argv(&GitOperation::Show {
            rev: codegg_git::ref_name::RevisionExpr::new(rev)
                .map_err(|e| GitServiceError::Repository(e.to_string()))?,
        });
        let op_label = "show".to_string();

        let raw = self.run_git_raw(&argv, repository_root).await?;

        let payload = if raw.exit_code == 0 {
            Some(GitPayload::Show(parse_show_output(rev, &raw.stdout)))
        } else {
            None
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
                // Defense-in-depth: the raw URL was already passed
                // through `redact_url_credentials_in_text` in
                // `run_git_raw`, but the trim() here is a final safety
                // pass before the value reaches `format_structured_result`
                // and the model context.
                let url = raw.stdout.trim().to_string();
                let safe_url = crate::git_network_policy::redact_url_credentials(&url);
                GitPayload::Remotes(vec![RemotePayload {
                    name: remote.as_str().to_string(),
                    url: Some(safe_url),
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
        let env = GitEnvPolicy::default();

        let output = tokio::time::timeout(timeout, async {
            let mut cmd = env.apply(&argv_owned, &root);
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
            // Defense-in-depth: redact any URL-embedded credentials from
            // git-emitted bytes before they reach Tool output, payload
            // parsers, projection, or RunStore. The corresponding argv
            // boundary in `codegg-git::render_argv` keeps the raw URL only
            // long enough to invoke the child process.
            stdout: redact_url_credentials_in_text(&String::from_utf8_lossy(&output.stdout)),
            stderr: redact_url_credentials_in_text(&String::from_utf8_lossy(&output.stderr)),
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
pub(crate) struct RawGitOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
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

/// Parse unified diff output into a structured [`DiffResultPayload`].
///
/// Handles `diff --git` headers, rename/copy detection, binary files,
/// `@@` hunk headers, and +/- line counting.
fn parse_diff_output(stdout: &str) -> Result<DiffResultPayload, DiffParseError> {
    let mut files: Vec<DiffFilePayload> = Vec::new();
    let mut total_insertions: usize = 0;
    let mut total_deletions: usize = 0;

    let lines: Vec<&str> = stdout.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if line.starts_with("diff --git ") {
            // Parse "diff --git a/path b/path"
            let path = parse_diff_header_path(line);
            let mut file = DiffFilePayload {
                path: path.clone(),
                old_path: None,
                kind: "modified".to_string(),
                is_binary: false,
                additions: 0,
                deletions: 0,
                hunks: Vec::new(),
            };

            i += 1;
            let mut is_binary = false;

            // Scan ahead for file metadata before hunks
            while i < lines.len() {
                let l = lines[i];
                if l.starts_with("@@ ") {
                    break;
                } else if let Some(rest) = l.strip_prefix("new file mode ") {
                    file.kind = "added".to_string();
                    let _ = rest;
                } else if let Some(rest) = l.strip_prefix("deleted file mode ") {
                    file.kind = "deleted".to_string();
                    let _ = rest;
                } else if let Some(rest) = l.strip_prefix("rename from ") {
                    file.old_path = Some(rest.to_string());
                    file.kind = "renamed".to_string();
                } else if let Some(rest) = l.strip_prefix("rename to ") {
                    file.path = rest.to_string();
                } else if let Some(rest) = l.strip_prefix("copy from ") {
                    file.old_path = Some(rest.to_string());
                    file.kind = "copied".to_string();
                } else if let Some(rest) = l.strip_prefix("copy to ") {
                    file.path = rest.to_string();
                } else if l.starts_with("Binary files ") && l.ends_with(" differ") {
                    file.is_binary = true;
                    is_binary = true;
                }
                i += 1;
            }

            if is_binary {
                files.push(file);
            } else {
                // Parse hunks
                while i < lines.len() {
                    let l = lines[i];
                    if l.starts_with("diff --git ") {
                        break;
                    }
                    if let Some(hunk) = parse_hunk_header(l) {
                        i += 1;
                        let mut hunk_lines = Vec::new();
                        while i < lines.len() {
                            let cl = lines[i];
                            if cl.starts_with("@@ ") || cl.starts_with("diff --git ") {
                                break;
                            }
                            if let Some(first) = cl.chars().next() {
                                match first {
                                    '+' => file.additions += 1,
                                    '-' => file.deletions += 1,
                                    _ => {}
                                }
                            }
                            hunk_lines.push(cl.to_string());
                            i += 1;
                        }
                        file.hunks.push(DiffHunkPayload {
                            old_start: hunk.0,
                            old_lines: hunk.1,
                            new_start: hunk.2,
                            new_lines: hunk.3,
                            lines: hunk_lines,
                        });
                        continue;
                    }
                    i += 1;
                }

                total_insertions += file.additions;
                total_deletions += file.deletions;
                files.push(file);
            }
        } else {
            i += 1;
        }
    }

    if files.is_empty() {
        return Err(DiffParseError::NoFilesFound);
    }

    Ok(DiffResultPayload {
        files,
        total_insertions,
        total_deletions,
    })
}

/// Parse the file path from a `diff --git a/path b/path` header.
fn parse_diff_header_path(line: &str) -> String {
    // "diff --git a/path b/path" → "path"
    let rest = &line["diff --git a/".len()..];
    if let Some(end) = rest.find(" b/") {
        rest[..end].to_string()
    } else {
        rest.to_string()
    }
}

/// Parse a `@@` hunk header line, returning (old_start, old_lines, new_start, new_lines).
fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    if !line.starts_with("@@ ") {
        return None;
    }
    let rest = &line[3..];
    let at_idx = rest.find(" @@")?;
    let range_part = &rest[..at_idx];
    let (old_range, new_range) = range_part.split_once(' ')?;

    let parse_range = |s: &str| -> Option<(u32, u32)> {
        let s = s
            .strip_prefix('-')
            .or_else(|| s.strip_prefix('+'))
            .unwrap_or(s);
        match s.split_once(',') {
            Some((start_str, count_str)) => {
                let start: u32 = start_str.parse().ok()?;
                let count: u32 = count_str.parse().ok()?;
                Some((start, count))
            }
            None => {
                let start: u32 = s.parse().ok()?;
                Some((start, 1))
            }
        }
    };

    let (old_start, old_lines) = parse_range(old_range)?;
    let (new_start, new_lines) = parse_range(new_range)?;
    Some((old_start, old_lines, new_start, new_lines))
}

/// Parse `git show` output into a structured [`ShowPayload`].
fn parse_show_output(_rev: &str, stdout: &str) -> ShowPayload {
    let mut hash = String::new();
    let mut author = String::new();
    let mut date = String::new();
    let mut message = String::new();
    let mut files: Vec<ChangedFilePayload> = Vec::new();
    let mut patch = String::new();
    let mut in_patch = false;

    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("commit ") {
            if hash.is_empty() {
                hash = rest.trim().to_string();
            }
        } else if let Some(rest) = line.strip_prefix("Author: ") {
            author = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("AuthorDate: ") {
            date = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("CommitDate: ") {
            if date.is_empty() {
                date = rest.trim().to_string();
            }
        } else if line.starts_with("    ") && message.is_empty() {
            message = line.trim().to_string();
        } else if line.starts_with("diff --git ") {
            in_patch = true;
            patch.push_str(line);
            patch.push('\n');
        } else if in_patch {
            patch.push_str(line);
            patch.push('\n');
        } else if let Some(rest) = line.strip_prefix(" delete mode ") {
            let path = rest.trim().to_string();
            if !path.is_empty() {
                files.push(ChangedFilePayload {
                    path,
                    kind: "deleted".to_string(),
                });
            }
        } else if let Some(rest) = line.strip_prefix(" create mode ") {
            let path = rest.trim().to_string();
            if !path.is_empty() {
                files.push(ChangedFilePayload {
                    path,
                    kind: "added".to_string(),
                });
            }
        }
    }

    ShowPayload {
        hash,
        author,
        date,
        message,
        files,
        patch,
    }
}

/// Error type for diff parsing.
#[derive(Debug, thiserror::Error)]
pub enum DiffParseError {
    #[error("no files found in diff output")]
    NoFilesFound,
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
    async fn execute_diff_returns_structured_result() {
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
            GitPayload::DiffResult(diff) => {
                assert_eq!(diff.files.len(), 1);
                assert_eq!(diff.files[0].path, "a.txt");
                assert_eq!(diff.files[0].kind, "modified");
                assert!(diff.files[0].additions > 0 || diff.files[0].deletions > 0);
                assert!(diff.total_insertions > 0 || diff.total_deletions > 0);
            }
            GitPayload::DiffSummary(summary) => {
                // Fallback: egggit summary still works
                assert_eq!(summary.files_changed, 1);
            }
            other => panic!("expected DiffResult or DiffSummary, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_show_returns_metadata() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "hello world");

        let svc = GitExecutionService::new();
        let result = svc
            .execute(
                &GitOperation::Show {
                    rev: codegg_git::ref_name::RevisionExpr::new("HEAD").unwrap(),
                },
                dir.path(),
            )
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.exit_code, 0);
        match result.payload.unwrap() {
            GitPayload::Show(show) => {
                assert!(!show.hash.is_empty());
                assert!(show.author.contains("Test") || show.author.contains("test"));
                assert!(!show.message.is_empty());
            }
            other => panic!("expected Show, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn diff_rename_detection_populates_old_path() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "old.txt", "content");
        StdCommand::new("git")
            .args(["mv", "old.txt", "new.txt"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["commit", "-m", "rename file"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let svc = GitExecutionService::new();
        let result = svc
            .execute(
                &GitOperation::Diff {
                    staged: false,
                    stat: false,
                    name_only: false,
                    base_ref: Some(codegg_git::ref_name::RevisionExpr::new("HEAD~1").unwrap()),
                    paths: vec![],
                },
                dir.path(),
            )
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::DiffResult(diff) => {
                assert_eq!(diff.files.len(), 1);
                let file = &diff.files[0];
                assert_eq!(file.kind, "renamed");
                assert!(
                    file.old_path.is_some(),
                    "old_path should be set for renames"
                );
            }
            other => panic!("expected DiffResult, got {:?}", other),
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

    // ── Edge-case fixtures ────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn detached_head_status() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "first");
        commit_file(dir.path(), "b.txt", "second");

        // Get the first commit hash and detach HEAD
        let output = StdCommand::new("git")
            .args(["rev-parse", "HEAD~1"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

        StdCommand::new("git")
            .args(["checkout", &hash])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let svc = GitExecutionService::new();
        let result = svc
            .execute(&GitOperation::Status { short: false }, dir.path())
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::Status(status) => {
                // Detached HEAD shows as hash, not branch name
                assert!(!status.branch.is_empty());
                assert!(!status.is_dirty);
            }
            other => panic!("expected Status, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn diff_binary_file() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());

        // Create a binary file (with null bytes)
        let binary_content: Vec<u8> = vec![0x00, 0x01, 0x02, 0xFF, 0xFE, 0xFD];
        std::fs::write(dir.path().join("binary.bin"), &binary_content).unwrap();
        StdCommand::new("git")
            .args(["add", "binary.bin"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["commit", "-m", "add binary"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Modify the binary file
        let new_content: Vec<u8> = vec![0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE, 0xFD, 0xFC];
        std::fs::write(dir.path().join("binary.bin"), &new_content).unwrap();

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
        // Binary diffs produce DiffResult or DiffText depending on git version
        match result.payload.unwrap() {
            GitPayload::DiffResult(diff_result) => {
                assert_eq!(diff_result.files.len(), 1);
                assert!(diff_result.files[0].is_binary);
            }
            GitPayload::DiffText(text) => {
                // Some git versions output raw text for binary diffs
                assert!(text.contains("Binary") || text.contains("bin"));
            }
            other => panic!("expected DiffResult or DiffText, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn status_unborn_repo() {
        let dir = TempDir::new().unwrap();
        // Init but don't commit anything
        StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let svc = GitExecutionService::new();
        let result = svc
            .execute(&GitOperation::Status { short: false }, dir.path())
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::Status(status) => {
                assert_eq!(status.branch, "main");
                assert!(!status.is_dirty);
            }
            other => panic!("expected Status, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn diff_renamed_file() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "old_name.txt", "content");

        // Rename the file
        StdCommand::new("git")
            .args(["mv", "old_name.txt", "new_name.txt"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let svc = GitExecutionService::new();
        let result = svc
            .execute(
                &GitOperation::Diff {
                    staged: true,
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
            GitPayload::DiffResult(diff_result) => {
                assert_eq!(diff_result.files.len(), 1);
                let file = &diff_result.files[0];
                assert_eq!(file.kind, "renamed");
                assert!(file.old_path.is_some());
            }
            GitPayload::DiffText(text) => {
                // Raw diff should contain rename indicators
                assert!(text.contains("rename") || text.contains("old_name"));
            }
            other => panic!("expected DiffResult or DiffText, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn show_commit_metadata() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "hello world");

        let svc = GitExecutionService::new();
        let result = svc
            .execute(
                &GitOperation::Show {
                    rev: codegg_git::ref_name::RevisionExpr::new("HEAD").unwrap(),
                },
                dir.path(),
            )
            .await
            .unwrap();

        assert!(result.success);
        match result.payload.unwrap() {
            GitPayload::Show(show) => {
                assert!(!show.hash.is_empty());
                assert!(show.author.contains("Test") || show.author.contains("test"));
                assert!(show.message.contains("add a.txt"));
            }
            other => panic!("expected Show, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn log_oneline_format() {
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
                // Each commit should have a hash and message
                for c in &commits {
                    assert!(!c.hash.is_empty());
                    assert!(!c.message.is_empty());
                }
            }
            other => panic!("expected Log, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn branch_list_with_detached_head() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_file(dir.path(), "a.txt", "first");
        commit_file(dir.path(), "b.txt", "second");

        // Detach HEAD
        let output = StdCommand::new("git")
            .args(["rev-parse", "HEAD~1"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        StdCommand::new("git")
            .args(["checkout", &hash])
            .current_dir(dir.path())
            .output()
            .unwrap();

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
                // Should still list branches, but none marked as current
                // (detached HEAD means no branch is checked out)
                assert!(!branches.is_empty());
            }
            other => panic!("expected Branches, got {:?}", other),
        }
    }
}
