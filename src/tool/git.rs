use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

use crate::error::ToolError;
use crate::git_mutation_projector::project_mutation;
use crate::git_mutations::{GitEnvPolicy, GitMutationError, GitMutationExecutor};
use crate::git_mutations_ops as gm_ops;
use crate::git_service::{GitExecutionService, GitPayload};
use crate::tool::{Tool, ToolCategory};
use codegg_git::parse_git_argv;

pub struct GitTool {
    timeout: Duration,
    workdir: PathBuf,
    run_store: Option<std::sync::Arc<dyn codegg_core::run_store::RunStore>>,
}

impl GitTool {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            workdir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            run_store: None,
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

    pub fn with_run_store(
        mut self,
        store: std::sync::Arc<dyn codegg_core::run_store::RunStore>,
    ) -> Self {
        self.run_store = Some(store);
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
        "Execute git commands and operations. Read-only operations (status, diff, log, show, blame, branch, tag, remote, worktree) return structured results. Mutations may use either the curated `mutation` action or pass a `subcommand` + `args` for raw execution; mutations are routed through a typed executor with snapshot, state-delta, and env hardening."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "subcommand": {
                    "type": "string",
                    "description": "Git subcommand (e.g., status, log, diff, branch, add, commit, checkout). May be empty when using the typed mutation API."
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Arguments to pass to the git subcommand"
                },
                "mutation": {
                    "type": "string",
                    "enum": [
                        "stage_paths", "stage_all", "stage_tracked",
                        "unstage_paths", "unstage_all",
                        "commit", "commit_amend",
                        "branch_create", "branch_switch", "branch_create_and_switch", "branch_delete",
                        "detach",
                        "restore_worktree", "restore_staged", "restore_both",
                        "stash_push", "stash_apply", "stash_pop", "stash_drop",
                        "merge", "rebase", "cherry_pick", "revert", "abort"
                    ],
                    "description": "Typed mutation action. Preferred over raw subcommand for local mutations."
                },
                "paths": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Repo-relative paths (used by stage_paths, unstage_paths, restore_*)"
                },
                "name": {
                    "type": "string",
                    "description": "Branch, tag, or stash name (used by branch_*, tag_*, stash_*)"
                },
                "target": {
                    "type": "string",
                    "description": "Revision (HEAD~1, branch, sha, etc.) for switch / detach / cherry-pick / revert / rebase from / merge"
                },
                "rev": {
                    "type": "string",
                    "description": "Revision expression for cherry-pick or revert"
                },
                "from": {
                    "type": "string",
                    "description": "Revision expression for merge (default current)"
                },
                "message": {
                    "type": "string",
                    "description": "Commit/merge message"
                },
                "no_edit": {
                    "type": "boolean",
                    "description": "Skip editor / keep conflict state for revert"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force flag for branch_delete (lowercase -d → -D)"
                },
                "include_untracked": {
                    "type": "boolean",
                    "description": "Pass --include-untracked to stash push"
                },
                "keep_index": {
                    "type": "boolean",
                    "description": "Pass --keep-index to stash push"
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
            "required": []
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
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

        // Typed mutation API — preferred over raw argv for local mutations.
        if let Some(mutation) = input["mutation"].as_str() {
            return self
                .dispatch_mutation(mutation, &input, &workdir, timeout_secs)
                .await;
        }

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

        // Try structured read for read-only operations
        if Self::is_read_only(subcommand) {
            let tool = GitTool {
                timeout,
                workdir: workdir.clone(),
                run_store: self.run_store.clone(),
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

impl GitTool {
    /// Dispatch a typed mutation action. All branches run through the
    /// shared `GitMutationExecutor`, which captures snapshots, computes
    /// state deltas, and applies env hardening.
    async fn dispatch_mutation(
        &self,
        mutation: &str,
        input: &serde_json::Value,
        workdir: &std::path::Path,
        timeout_secs: u64,
    ) -> Result<String, ToolError> {
        let exec = GitMutationExecutor::new()
            .with_env_policy(GitEnvPolicy::default())
            .with_timeout(Duration::from_secs(timeout_secs.max(1)));
        let paths = || -> Vec<String> {
            input["paths"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let name = input["name"].as_str();
        let target = input["target"].as_str();
        let rev = input["rev"].as_str();
        let from = input["from"].as_str();
        let _ = from; // reserved for future merge --from support
        let message = input["message"].as_str();
        let no_edit = input["no_edit"].as_bool().unwrap_or(false);
        let force = input["force"].as_bool().unwrap_or(false);
        let include_untracked = input["include_untracked"].as_bool().unwrap_or(false);
        let keep_index = input["keep_index"].as_bool().unwrap_or(false);

        let result = match mutation {
            "stage_paths" => gm_ops::stage_paths(&exec, workdir, paths()).await,
            "stage_all" => gm_ops::stage_all(&exec, workdir).await,
            "stage_tracked" => gm_ops::stage_tracked(&exec, workdir).await,
            "unstage_paths" => gm_ops::unstage_paths(&exec, workdir, paths()).await,
            "unstage_all" => gm_ops::unstage_all(&exec, workdir).await,
            "commit" => {
                let sel = if !paths().is_empty() {
                    crate::git_mutations::CommitSelection::StagePaths(paths())
                } else {
                    crate::git_mutations::CommitSelection::StageAll
                };
                let msg = message
                    .ok_or_else(|| {
                        ToolError::Execution("commit requires 'message' parameter".to_string())
                    })?
                    .to_string();
                gm_ops::commit_with_selection(
                    &exec,
                    workdir,
                    sel,
                    &msg,
                    false,
                    input["allow_empty"].as_bool().unwrap_or(false),
                )
                .await
                .map(|o| o.mutation)
            }
            "commit_amend" => {
                let msg = message.unwrap_or("HEAD");
                gm_ops::commit_with_selection(
                    &exec,
                    workdir,
                    crate::git_mutations::CommitSelection::AlreadyStaged,
                    msg,
                    true,
                    input["allow_empty"].as_bool().unwrap_or(false),
                )
                .await
                .map(|o| o.mutation)
            }
            "branch_create" => {
                let n = name.ok_or_else(|| branch_param("name"))?;
                gm_ops::branch_create(&exec, workdir, n, target, force).await
            }
            "branch_switch" => {
                let n = name.ok_or_else(|| branch_param("name"))?;
                gm_ops::switch_branch(&exec, workdir, n, force).await
            }
            "branch_create_and_switch" => {
                let n = name.ok_or_else(|| branch_param("name"))?;
                gm_ops::create_and_switch(&exec, workdir, n, target).await
            }
            "branch_delete" => {
                let n = name.ok_or_else(|| branch_param("name"))?;
                gm_ops::branch_delete(&exec, workdir, n, force).await
            }
            "detach" => {
                let t = target.ok_or_else(|| branch_param("target"))?;
                gm_ops::detach_at(&exec, workdir, t).await
            }
            "restore_worktree" => gm_ops::restore_worktree(&exec, workdir, paths(), target).await,
            "restore_staged" => gm_ops::restore_staged(&exec, workdir, paths()).await,
            "restore_both" => gm_ops::restore_both(&exec, workdir, paths(), target).await,
            "stash_push" => {
                gm_ops::stash_push(&exec, workdir, message, include_untracked, Vec::new()).await
            }
            "stash_apply" => {
                let n = name.unwrap_or("stash@{0}");
                gm_ops::stash_apply(&exec, workdir, Some(n), keep_index).await
            }
            "stash_pop" => {
                let n = name.unwrap_or("stash@{0}");
                gm_ops::stash_pop(&exec, workdir, Some(n), keep_index).await
            }
            "stash_drop" => {
                let n = name.ok_or_else(|| branch_param("name"))?;
                gm_ops::stash_drop(&exec, workdir, n).await
            }
            "merge" => {
                let revs = match target {
                    Some(t) => vec![t.to_string()],
                    None => vec!["HEAD".to_string()],
                };
                gm_ops::merge(
                    &exec,
                    workdir,
                    revs,
                    input["no_ff"].as_bool().unwrap_or(false),
                    None,
                )
                .await
            }
            "rebase" => {
                let upstream = target.map(String::from);
                gm_ops::rebase(&exec, workdir, upstream.as_deref(), upstream.as_deref()).await
            }
            "cherry_pick" => {
                let revs = rev
                    .map(|r| vec![r.to_string()])
                    .or_else(|| target.map(|t| vec![t.to_string()]))
                    .ok_or_else(|| branch_param("rev"))?;
                gm_ops::cherry_pick(&exec, workdir, revs).await
            }
            "revert" => {
                let revs = rev
                    .map(|r| vec![r.to_string()])
                    .or_else(|| target.map(|t| vec![t.to_string()]))
                    .ok_or_else(|| branch_param("rev"))?;
                gm_ops::revert(&exec, workdir, revs, no_edit).await
            }
            "abort" => gm_ops::abort_in_progress(&exec, workdir).await,
            other => {
                return Err(ToolError::Execution(format!(
                    "unknown mutation action: {other}"
                )))
            }
        }
        .map_err(map_mutation_err)?;
        // Best-effort persistence to RunStore.
        let repo_root = crate::git_mutations::resolve_repo_root(workdir)
            .map(|r| r.as_path().to_path_buf())
            .unwrap_or_else(|_| workdir.to_path_buf());
        let _ = crate::git_run_store::persist_mutation(
            &self.run_store,
            &result,
            workdir,
            &repo_root,
            "git_native",
            Some(mutation.to_string()),
        )
        .await;
        Ok(project_mutation(&result))
    }
}

fn branch_param(label: &str) -> ToolError {
    ToolError::Execution(format!("mutation requires '{label}' parameter"))
}

fn map_mutation_err(e: GitMutationError) -> ToolError {
    match e {
        GitMutationError::Precondition(s) => ToolError::Execution(format!("precondition: {s}")),
        GitMutationError::Path(s) => ToolError::Execution(format!("path: {s}")),
        GitMutationError::Ref(s) => ToolError::Execution(format!("ref: {s}")),
        GitMutationError::Repository(s) => ToolError::Execution(format!("repository: {s}")),
        GitMutationError::Execution(s) => ToolError::Execution(s),
        GitMutationError::Timeout(s) => ToolError::Execution(format!("timed out after {s}s")),
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
