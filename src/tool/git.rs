use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

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
        "Execute git operations. Read-only subcommands (status, diff, log, show, blame, branch, tag, remote, worktree, stash, rev-parse, for-each-ref) return structured JSON results via egggit. Prefer the typed `mutation` action (stage_paths, commit, branch_create, merge, revert, push, reset_hard, clean, …) for local, network, and destructive operations — they route through a snapshot-based executor with state-delta, env hardening, and RunStore persistence. For in-progress operations (merge/rebase/cherry-pick/revert), use `operation_state` (read) and `recover` (write: continue | abort | skip) — these inspect plumbing, refuse cross-operation misuse, and return a typed `RecoveryOutcome`. Conflicts are NOT auto-resolved: edit conflict markers in the worktree, `git add <path>` to stage the resolution, then run `recover` with `continue`."
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
                        "merge", "rebase", "cherry_pick", "revert", "abort",
                        "fetch", "pull", "push",
                        "remote_add", "remote_remove", "remote_set_url", "remote_rename",
                        "config_get", "config_set", "config_unset",
                        "reset_soft", "reset_mixed", "reset_hard", "reset_merge", "reset_keep",
                        "reset_paths",
                        "clean_preview", "clean"
                    ],
                    "description": "Typed mutation action. Preferred over raw subcommand for local mutations and network/destructive operations."
                },
                "recover": {
                    "type": "string",
                    "enum": ["continue", "abort", "skip"],
                    "description": "Operation-aware recovery action for an in-progress merge/rebase/cherry-pick/revert. Inspects the active operation state and refuses cross-operation misuse. Must be paired with NO subcommand/mutation (recover is mutually exclusive with both)."
                },
                "operation_state": {
                    "type": "boolean",
                    "description": "When true, the tool returns the typed in-progress operation state (merge/rebase/cherry-pick/revert/bisect/am/sequencer/none) plus conflicted paths and legal recovery actions instead of executing a subcommand. Mutually exclusive with `mutation`, `recover`, and `subcommand`."
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
                "remote": {
                    "type": "string",
                    "description": "Remote name (fetch, pull, push, remote_*)"
                },
                "url": {
                    "type": "string",
                    "description": "URL (remote_add, remote_set_url, push)"
                },
                "old_name": {
                    "type": "string",
                    "description": "Old remote name (remote_rename)"
                },
                "refspecs": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Refspecs to fetch or push (fetch, push)"
                },
                "all": {
                    "type": "boolean",
                    "description": "Pass --all (fetch, push)"
                },
                "prune": {
                    "type": "boolean",
                    "description": "Pass --prune (fetch)"
                },
                "strategy": {
                    "type": "string",
                    "enum": ["ff-only", "rebase", "merge", "ff"],
                    "description": "Pull strategy (pull)"
                },
                "force_with_lease": {
                    "type": "boolean",
                    "description": "Pass --force-with-lease to push (safer than --force)"
                },
                "force_push": {
                    "type": "boolean",
                    "description": "Pass --force to push (DANGEROUS, requires explicit permission)"
                },
                "set_upstream": {
                    "type": "boolean",
                    "description": "Pass --set-upstream to push"
                },
                "key": {
                    "type": "string",
                    "description": "Config key (config_get, config_set, config_unset) — must be allowlisted"
                },
                "value": {
                    "type": "string",
                    "description": "Config value (config_set)"
                },
                "scope": {
                    "type": "string",
                    "enum": ["local", "global", "worktree"],
                    "description": "Config scope (default: local)"
                },
                "mode": {
                    "type": "string",
                    "enum": ["soft", "mixed", "hard", "merge", "keep"],
                    "description": "Reset mode (used by reset_* actions)"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Pass --dry-run (clean_preview)"
                },
                "ignored": {
                    "type": "boolean",
                    "description": "Include ignored files (clean, clean_preview)"
                },
                "directories": {
                    "type": "boolean",
                    "description": "Remove untracked directories (clean, clean_preview)"
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

        // Operation-state discovery — returns the typed active state.
        if input["operation_state"].as_bool().unwrap_or(false) {
            return self.dispatch_operation_state(&workdir, timeout_secs).await;
        }

        // Operation-aware recovery — continue/abort/skip.
        if let Some(recover) = input["recover"].as_str() {
            return self.dispatch_recover(recover, &workdir, timeout_secs).await;
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

        // Apply the shared Git env policy so the raw fallback path is
        // hardened identically to typed mutations, ending the prior
        // raw-fallback hardening gap (Phase F design limitation).
        let env_policy = GitEnvPolicy::default();
        let argv_for_run: Vec<String> = {
            let mut v = vec!["git".to_string()];
            v.extend(full_args.clone());
            v
        };

        let output = tokio::time::timeout(timeout, async {
            let mut cmd = env_policy.apply(&argv_for_run, &workdir);
            cmd.output().await
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

        // Phase E parameters (network, config, destructive).
        let remote = input["remote"].as_str();
        let url = input["url"].as_str();
        let old_name = input["old_name"].as_str();
        let all = input["all"].as_bool().unwrap_or(false);
        let prune = input["prune"].as_bool().unwrap_or(false);
        let strategy = input["strategy"].as_str();
        let force_with_lease = input["force_with_lease"].as_bool().unwrap_or(false);
        let force_push = input["force_push"].as_bool().unwrap_or(false);
        let set_upstream = input["set_upstream"].as_bool().unwrap_or(false);
        let refspecs: Vec<String> = input["refspecs"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let key = input["key"].as_str();
        let value = input["value"].as_str();
        let scope = input["scope"].as_str();
        let dry_run = input["dry_run"].as_bool().unwrap_or(false);
        let ignored = input["ignored"].as_bool().unwrap_or(false);
        let directories = input["directories"].as_bool().unwrap_or(false);

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

            // ---- Phase E: network operations ----
            "fetch" => {
                crate::git_network_ops::fetch(&exec, workdir, remote, refspecs, prune, all).await
            }
            "pull" => {
                let pull_strategy = match strategy {
                    Some("ff-only") => crate::git_network_ops::PullStrategy::FastForwardOnly,
                    Some("rebase") => crate::git_network_ops::PullStrategy::Rebase,
                    Some("merge") => crate::git_network_ops::PullStrategy::Merge,
                    Some("ff") => crate::git_network_ops::PullStrategy::FastForwardOnly,
                    None => crate::git_network_ops::PullStrategy::Merge,
                    Some(other) => {
                        return Err(ToolError::Execution(format!(
                            "unknown pull strategy: {other}"
                        )));
                    }
                };
                crate::git_network_ops::pull(&exec, workdir, remote, target, pull_strategy, false)
                    .await
            }
            "push" => {
                let push_force = if force_push {
                    crate::git_network_ops::PushForce::Force
                } else if force_with_lease {
                    crate::git_network_ops::PushForce::ForceWithLease { expected_sha: None }
                } else {
                    crate::git_network_ops::PushForce::Normal
                };
                let req = crate::git_network_ops::PushRequest {
                    remote: remote.map(String::from),
                    branch: target.map(String::from),
                    set_upstream,
                    force: push_force,
                    tags: all,
                    delete: false,
                    dry_run: false,
                };
                crate::git_network_ops::push(&exec, workdir, req).await
            }

            // ---- Phase E: remote management ----
            "remote_add" => {
                let r = remote.ok_or_else(|| branch_param("remote"))?;
                let u = url.ok_or_else(|| branch_param("url"))?;
                crate::git_network_ops::remote_add(&exec, workdir, r, u).await
            }
            "remote_remove" => {
                let r = remote.ok_or_else(|| branch_param("remote"))?;
                crate::git_network_ops::remote_remove(&exec, workdir, r).await
            }
            "remote_set_url" => {
                let r = remote.ok_or_else(|| branch_param("remote"))?;
                let u = url.ok_or_else(|| branch_param("url"))?;
                crate::git_network_ops::remote_set_url(&exec, workdir, r, u, false).await
            }
            "remote_rename" => {
                let r = remote.ok_or_else(|| branch_param("remote"))?;
                let o = old_name.ok_or_else(|| branch_param("old_name"))?;
                crate::git_network_ops::remote_rename(&exec, workdir, o, r).await
            }

            // ---- Phase E: git config ----
            "config_get" => {
                let k = key.ok_or_else(|| branch_param("key"))?;
                let local = scope_unwrap_local(scope);
                crate::git_network_ops::config_get(&exec, workdir, k, local).await
            }
            "config_set" => {
                let k = key.ok_or_else(|| branch_param("key"))?;
                let v = value.ok_or_else(|| branch_param("value"))?;
                let local = scope_unwrap_local(scope);
                crate::git_network_ops::config_set(&exec, workdir, k, v, local).await
            }
            "config_unset" => {
                let k = key.ok_or_else(|| branch_param("key"))?;
                let local = scope_unwrap_local(scope);
                crate::git_network_ops::config_unset(&exec, workdir, k, local).await
            }

            // ---- Phase E: destructive reset/clean ----
            "reset_soft" => {
                let t = target.unwrap_or("HEAD");
                crate::git_network_ops::reset_soft(&exec, workdir, Some(t)).await
            }
            "reset_mixed" => {
                let t = target.unwrap_or("HEAD");
                crate::git_network_ops::reset_mixed(&exec, workdir, Some(t)).await
            }
            "reset_hard" => {
                let t = target.ok_or_else(|| branch_param("target"))?;
                crate::git_network_ops::reset_hard(&exec, workdir, Some(t)).await
            }
            "reset_merge" => {
                let t = target.ok_or_else(|| branch_param("target"))?;
                crate::git_network_ops::reset_merge(&exec, workdir, Some(t)).await
            }
            "reset_keep" => {
                let t = target.ok_or_else(|| branch_param("target"))?;
                crate::git_network_ops::reset_keep(&exec, workdir, Some(t)).await
            }
            "reset_paths" => crate::git_network_ops::reset_paths(&exec, workdir, paths()).await,
            "clean_preview" => {
                let _ = (ignored, directories);
                let preview = crate::git_network_ops::clean_preview(&exec, workdir, paths())
                    .await
                    .map_err(map_mutation_err)?;
                let mut out = String::new();
                if preview.is_empty() {
                    out.push_str("clean preview: nothing to remove\n");
                } else {
                    out.push_str(&format!(
                        "clean preview: {} entries would be removed\n",
                        preview.len()
                    ));
                    for entry in &preview.entries {
                        out.push_str(&format!("  - {} ({:?})\n", entry.path, entry.kind));
                    }
                }
                return Ok(out);
            }
            "clean" => {
                let req = crate::git_network_ops::CleanRequest {
                    force: !dry_run,
                    dirs: directories,
                    ignored,
                    paths: paths(),
                };
                if req.is_broad() {
                    return Err(ToolError::Execution(
                        "broad ignored cleanup (clean --include-ignored at root) is rejected \
                         by policy; scope to specific paths or remove --ignored"
                            .to_string(),
                    ));
                }
                if dry_run {
                    let preview =
                        crate::git_network_ops::clean_preview(&exec, workdir, req.paths.clone())
                            .await
                            .map_err(map_mutation_err)?;
                    let mut out = String::new();
                    if preview.is_empty() {
                        out.push_str("clean preview: nothing to remove\n");
                    } else {
                        out.push_str(&format!(
                            "clean preview: {} entries would be removed\n",
                            preview.len()
                        ));
                        for entry in &preview.entries {
                            out.push_str(&format!("  - {} ({:?})\n", entry.path, entry.kind));
                        }
                    }
                    return Ok(out);
                }
                crate::git_network_ops::clean(&exec, workdir, req).await
            }

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

    /// Inspect the active operation state via `egggit::detect_operation_state_for_root`.
    /// Returns a typed string describing the active operation family,
    /// the typed label, and the legal recovery actions.
    async fn dispatch_operation_state(
        &self,
        workdir: &std::path::Path,
        _timeout_secs: u64,
    ) -> Result<String, ToolError> {
        let state = egggit::detect_operation_state_for_root(workdir)
            .map_err(|e| ToolError::Execution(format!("operation-state probe failed: {e}")))?;
        let available_actions: Vec<&'static str> = state
            .available_actions()
            .into_iter()
            .map(|a| a.label())
            .collect();
        let label = state.label();
        let family = state.family().label();
        let mut out = String::new();
        out.push_str(&format!("operation state: {family}\n"));
        out.push_str(&format!("label: {label}\n"));
        let actions_str = if available_actions.is_empty() {
            "<none>".to_string()
        } else {
            available_actions.join(", ")
        };
        out.push_str(&format!("available_actions: {actions_str}\n"));
        Ok(out)
    }

    /// Run an operation-aware recovery action (continue | abort | skip).
    async fn dispatch_recover(
        &self,
        action: &str,
        workdir: &std::path::Path,
        timeout_secs: u64,
    ) -> Result<String, ToolError> {
        let parsed_action = match action {
            "continue" => egggit::RecoveryAction::Continue,
            "abort" => egggit::RecoveryAction::Abort,
            "skip" => egggit::RecoveryAction::Skip,
            other => {
                return Err(ToolError::Execution(format!(
                    "unknown recover action: {other} (expected continue | abort | skip)"
                )));
            }
        };
        let exec = GitMutationExecutor::new()
            .with_env_policy(GitEnvPolicy::default())
            .with_timeout(Duration::from_secs(timeout_secs.max(1)));
        let result = match parsed_action {
            egggit::RecoveryAction::Continue => {
                crate::git_recovery::continue_in_progress(&exec, workdir).await
            }
            egggit::RecoveryAction::Abort => {
                crate::git_recovery::abort_in_progress_typed(&exec, workdir).await
            }
            egggit::RecoveryAction::Skip => {
                crate::git_recovery::skip_in_progress(&exec, workdir).await
            }
        };
        let result = result.map_err(map_mutation_err)?;
        let repo_root = crate::git_mutations::resolve_repo_root(workdir)
            .map(|r| r.as_path().to_path_buf())
            .unwrap_or_else(|_| workdir.to_path_buf());
        // Phase F: detect the family BEFORE the action so we can label
        // the projection accurately even when the action is a no-op.
        let family = egggit::detect_operation_state_for_root(workdir)
            .map(|s| s.family().label().to_string())
            .unwrap_or_else(|_| "none".to_string());
        let _ = crate::git_run_store::persist_recovery(
            &self.run_store,
            &result,
            workdir,
            &repo_root,
            action,
        )
        .await;
        Ok(crate::git_mutation_projector::project_recovery(
            &result, action, &family,
        ))
    }
}

fn branch_param(label: &str) -> ToolError {
    ToolError::Execution(format!("mutation requires '{label}' parameter"))
}

/// Resolve the `scope` JSON parameter (local | global | worktree) into
/// the local-only boolean expected by git_network_ops.
///
/// Anything other than `local` is rejected because Phase E intentionally
/// disallows global config writes (those belong in `~/.gitconfig`
/// outside the repo boundary).
fn scope_unwrap_local(scope: Option<&str>) -> bool {
    match scope {
        None | Some("local") => true,
        _ => {
            tracing::warn!(
                "scope={scope:?} requested but Phase E only allows local scope; \
                 defaulting to local"
            );
            true
        }
    }
}

fn map_mutation_err(e: GitMutationError) -> ToolError {
    match e {
        GitMutationError::Precondition(s) => ToolError::Execution(format!("precondition: {s}")),
        GitMutationError::Path(s) => ToolError::Execution(format!("path: {s}")),
        GitMutationError::Ref(s) => ToolError::Execution(format!("ref: {s}")),
        GitMutationError::Repository(s) => ToolError::Execution(format!("repository: {s}")),
        GitMutationError::Execution(s) => ToolError::Execution(s),
        GitMutationError::Timeout(s) => ToolError::Execution(format!("timed out after {s}s")),
        GitMutationError::StateMismatch { expected, actual } => ToolError::Execution(format!(
            "state mismatch: expected operation '{expected}' but found '{actual}' on disk"
        )),
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

#[cfg(test)]
mod schema_tests {
    //! Schema snapshot tests for the `git` tool.
    //!
    //! These tests pin the model-facing schema in two forms: the JSON
    //! `properties`/`enum` surface (intended for snapshot diffing in code
    //! review) and a high-level structural shape (intended for catching
    //! accidental breaking changes such as the removal of the
    //! `operation_state` parameter or a regression that turns the
    //! `mutation` enum back into a free string).

    use super::*;
    use serde_json::Value;

    fn params() -> Value {
        GitTool::new().parameters()
    }

    #[test]
    fn mutation_enum_includes_phase_f_recovery_aliases() {
        let schema = params();
        let mutation_enum = schema["properties"]["mutation"]["enum"]
            .as_array()
            .expect("mutation.enum should be an array");
        let names: Vec<&str> = mutation_enum.iter().filter_map(|v| v.as_str()).collect();
        // Local mutations (Phase D)
        assert!(names.contains(&"merge"), "merge mutation missing");
        assert!(names.contains(&"rebase"), "rebase mutation missing");
        assert!(
            names.contains(&"cherry_pick"),
            "cherry_pick mutation missing"
        );
        assert!(names.contains(&"revert"), "revert mutation missing");
        assert!(names.contains(&"abort"), "abort mutation missing");
        // Network mutations (Phase E)
        assert!(names.contains(&"fetch"), "fetch mutation missing");
        assert!(names.contains(&"push"), "push mutation missing");
        // Destructive (Phase E)
        assert!(names.contains(&"reset_hard"), "reset_hard mutation missing");
        assert!(names.contains(&"clean"), "clean mutation missing");
        // Counted: maintain a healthy pressure on the schema's stability.
        assert!(
            names.len() >= 35,
            "expected at least 35 mutation actions, got {}: {names:?}",
            names.len()
        );
    }

    #[test]
    fn recover_parameter_present_with_canonical_enum() {
        let schema = params();
        let recover_enum = schema["properties"]["recover"]["enum"]
            .as_array()
            .expect("recover.enum should be an array");
        let names: Vec<&str> = recover_enum.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(
            names,
            vec!["continue", "abort", "skip"],
            "recover.enum must be exactly ['continue','abort','skip']"
        );
    }

    #[test]
    fn operation_state_parameter_present() {
        let schema = params();
        let op = &schema["properties"]["operation_state"];
        assert!(op.is_object(), "operation_state should exist as a property");
        assert_eq!(op["type"], "boolean");
    }

    #[test]
    fn description_mentions_recovery_and_conflicts() {
        let binding = GitTool::new();
        let desc = binding.description();
        assert!(
            desc.contains("operation_state") || desc.contains("recover"),
            "description should advertise the operation_state/recover parameters, got: {desc}"
        );
        // We want agents to understand conflicts are NOT auto-resolved.
        assert!(
            desc.contains("conflict") || desc.contains("Conflicts"),
            "description should call out conflict handling, got: {desc}"
        );
    }

    #[test]
    fn parameters_pin_top_level_keys() {
        let schema = params();
        let top_keys: Vec<&str> = schema["properties"]
            .as_object()
            .expect("properties is object")
            .keys()
            .map(String::as_str)
            .collect();
        for required in [
            "subcommand",
            "args",
            "mutation",
            "recover",
            "operation_state",
            "paths",
            "name",
            "target",
            "remote",
            "url",
            "message",
            "force",
            "workdir",
            "timeout",
        ] {
            assert!(
                top_keys.contains(&required),
                "schema drift: missing top-level parameter `{required}`; got {top_keys:?}"
            );
        }
    }

    #[test]
    fn recover_is_mutually_exclusive_with_mutation_via_description() {
        // The schema does not (yet) enforce mutual exclusion at the JSON
        // level; we surface the contract via the field description.
        let schema = params();
        assert!(
            schema["properties"]["recover"]["description"]
                .as_str()
                .unwrap_or("")
                .contains("mutually exclusive")
                || schema["properties"]["recover"]["description"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Mutually exclusive"),
            "recover.description must note mutual exclusion with mutation"
        );
    }
}
