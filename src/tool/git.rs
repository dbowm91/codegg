use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

use crate::error::ToolError;
use crate::git_service::{GitExecutionService, GitPayload};
use crate::tool::{Tool, ToolCategory};
use codegg_git::parse_git_argv;

pub struct GitTool {
    timeout: Duration,
    workdir: PathBuf,
}

impl GitTool {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            workdir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn with_workdir(mut self, dir: PathBuf) -> Self {
        self.workdir = dir;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Determine if the given subcommand is a read-only operation.
    fn is_read_only(subcommand: &str) -> bool {
        matches!(
            subcommand,
            "status"
                | "diff"
                | "log"
                | "show"
                | "blame"
                | "branch"
                | "tag"
                | "remote"
                | "worktree"
                | "stash"
                | "rev-parse"
                | "for-each-ref"
        )
    }

    /// Try to execute a structured read via GitExecutionService.
    async fn try_structured_read(
        &self,
        subcommand: &str,
        args: &[String],
    ) -> Option<Result<String, ToolError>> {
        let full_argv = {
            let mut v = vec!["git".to_string(), subcommand.to_string()];
            v.extend_from_slice(args);
            v
        };

        let operation = parse_git_argv(&full_argv).ok()?;
        let service = GitExecutionService::new().with_timeout(self.timeout);
        let root = self.workdir.as_path();

        match service.execute(&operation, root).await {
            Ok(result) => Some(Ok(format_structured_result(subcommand, &result))),
            Err(_) => None, // Fall back to raw execution
        }
    }
}

impl Default for GitTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GitTool {
    fn name(&self) -> &str {
        "git"
    }

    fn description(&self) -> &str {
        "Execute git commands and operations. Read-only operations (status, diff, log, show, blame, branch, tag, remote, worktree) return structured results."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "subcommand": {
                    "type": "string",
                    "description": "Git subcommand (e.g., status, log, diff, branch, add, commit, checkout)"
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Arguments to pass to the git subcommand"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for git command (default: current directory)"
                },
                "timeout": {
                    "type": "number",
                    "description": "Timeout in seconds (default: 30)"
                }
            },
            "required": ["subcommand"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let subcommand = input["subcommand"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'subcommand' parameter".to_string()))?;

        let args: Vec<String> = input["args"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let workdir = input["workdir"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workdir.clone());

        let timeout_secs = input["timeout"].as_u64().unwrap_or(30);
        let timeout = Duration::from_secs(timeout_secs);

        if !workdir.exists() {
            return Err(ToolError::Execution(format!(
                "working directory does not exist: {}",
                workdir.display()
            )));
        }

        // Try structured read for read-only operations
        if Self::is_read_only(subcommand) {
            let tool = GitTool {
                timeout,
                workdir: workdir.clone(),
            };
            if let Some(result) = tool.try_structured_read(subcommand, &args).await {
                return result;
            }
        }

        // Fall back to raw execution
        let full_args = {
            let mut full = vec![subcommand.to_string()];
            full.extend(args);
            full
        };

        tracing::info!(
            "Running git command in {}: git {:?}",
            workdir.display(),
            full_args
        );

        let output = tokio::time::timeout(timeout, async {
            let mut cmd = Command::new("git");
            cmd.env_clear();
            if let Some(path) = std::env::var_os("PATH") {
                cmd.env("PATH", path);
            } else {
                cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
            }
            cmd.args(&full_args)
                .current_dir(&workdir)
                .kill_on_drop(true)
                .output()
                .await
        })
        .await
        .map_err(|_| ToolError::Timeout(format!("git command timed out after {}s", timeout_secs)))?
        .map_err(|e| ToolError::Execution(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n--- stderr ---\n");
            }
            result.push_str(&stderr);
        }

        result.push_str(&format!(
            "\n\n[exit code: {}]",
            output.status.code().unwrap_or(-1)
        ));

        Ok(result)
    }
}

/// Format a structured git execution result into a human-readable string.
fn format_structured_result(
    _subcommand: &str,
    result: &crate::git_service::GitExecutionResult,
) -> String {
    let mut output = String::new();

    match &result.payload {
        Some(GitPayload::Status(status)) => {
            output.push_str(&format!("Branch: {}\n", &status.branch));
            output.push_str(&format!(
                "HEAD: {}\n",
                status.commit_hash.as_deref().unwrap_or("unknown")
            ));
            output.push_str(&format!(
                "Dirty: {}\n",
                if status.is_dirty { "yes" } else { "no" }
            ));
            if status.stash_count > 0 {
                output.push_str(&format!("Stashes: {}\n", status.stash_count));
            }
        }
        Some(GitPayload::DiffSummary(summary)) => {
            output.push_str(&format!(
                "{} file(s) changed, +{} -{}\n",
                summary.files_changed, summary.insertions, summary.deletions
            ));
            for file in &summary.files {
                output.push_str(&format!("  {} ({})\n", file.path, file.kind));
            }
        }
        Some(GitPayload::DiffText(text)) => {
            output.push_str(text);
        }
        Some(GitPayload::DiffResult(result)) => {
            output.push_str(&format!(
                "{} file(s) changed, +{} -{}\n",
                result.files.len(),
                result.total_insertions,
                result.total_deletions
            ));
            for file in &result.files {
                let binary = if file.is_binary { " [binary]" } else { "" };
                let rename = file
                    .old_path
                    .as_deref()
                    .map(|old| format!(" (was {})", old))
                    .unwrap_or_default();
                output.push_str(&format!(
                    "  {} ({}){}{}\n",
                    file.path, file.kind, rename, binary
                ));
            }
        }
        Some(GitPayload::Show(show)) => {
            let short_hash = if show.hash.len() > 7 {
                &show.hash[..7]
            } else {
                &show.hash
            };
            output.push_str(&format!("commit {}\n", short_hash));
            output.push_str(&format!("Author: {}\n", show.author));
            output.push_str(&format!("Date: {}\n\n", show.date));
            output.push_str(&format!("{}\n\n", show.message));
            if !show.files.is_empty() {
                output.push_str(&format!("{} file(s) changed\n", show.files.len()));
            }
            if !show.patch.is_empty() {
                output.push_str(&show.patch);
            }
        }
        Some(GitPayload::Log(commits)) => {
            for commit in commits {
                output.push_str(&format!(
                    "{} {}\n",
                    &commit.hash[..7.min(commit.hash.len())],
                    commit.message
                ));
            }
        }
        Some(GitPayload::Branches(branches)) => {
            for branch in branches {
                let marker = if branch.current { "* " } else { "  " };
                output.push_str(&format!("{}{}\n", marker, branch.name));
            }
        }
        Some(GitPayload::Tags(tags)) => {
            for tag in tags {
                output.push_str(&format!("{}\n", tag));
            }
        }
        Some(GitPayload::Remotes(remotes)) => {
            for remote in remotes {
                if let Some(url) = &remote.url {
                    output.push_str(&format!("{}\t{}\n", remote.name, url));
                } else {
                    output.push_str(&format!("{}\n", remote.name));
                }
            }
        }
        Some(GitPayload::Worktrees(worktrees)) => {
            for wt in worktrees {
                let marker = if wt.is_current { "* " } else { "  " };
                output.push_str(&format!("{}{} ({})\n", marker, wt.path, wt.branch));
            }
        }
        Some(GitPayload::Stashes(stashes)) => {
            for stash in stashes {
                let msg = stash.message.as_deref().unwrap_or("");
                output.push_str(&format!("{}: {}\n", stash.ref_name, msg));
            }
        }
        Some(GitPayload::ChangedFiles(files)) => {
            for file in files {
                output.push_str(&format!("{} ({})\n", file.path, file.kind));
            }
        }
        _ => {
            // Fall back to raw output
            output.push_str(&result.stdout);
        }
    }

    if !result.stderr.is_empty() {
        output.push_str("\n--- stderr ---\n");
        output.push_str(&result.stderr);
    }

    output.push_str(&format!("\n\n[exit code: {}]", result.exit_code));
    output
}
