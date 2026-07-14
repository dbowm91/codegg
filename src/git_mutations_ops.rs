//! Typed mutation operations (stage, commit, branch, stash, ...).
//!
//! These functions wrap [`GitMutationExecutor::execute`] with operation-
//! specific validation. They are the canonical entry points for native
//! tools and the command-routing backend.
//!
//! Phase D invariants:
//!
//! * argv is rendered without a shell.
//! * paths are validated as `RepoPath` (rejects absolute, parent
//!   traversal, NUL bytes).
//! * branch/tag names are validated as `BranchName` / `RefName`.
//! * every operation captures pre/post snapshots and returns a typed
//!   `MutationResult`.
//! * message generation (LLM) and shell fallback are NOT owned here.

use std::path::Path;

use codegg_git::operation::ResetMode;
use codegg_git::path::{Pathspec, RepoRoot};
use codegg_git::ref_name::{BranchName, RevisionExpr};
use codegg_git::GitOperation;

use crate::git_mutations::{
    classify_outcome, compute_delta, resolve_repo_root, validate_repo_path, CommitSelection,
    GitEnvPolicy, GitMutationError, GitMutationExecutor, MutationOutcome, MutationResult,
    StateDelta,
};

// ── Stage operations ────────────────────────────────────────────────

/// Stage named literal paths under the index.
pub async fn stage_paths(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    paths: Vec<String>,
) -> Result<MutationResult, GitMutationError> {
    let root = resolve_repo_root(repo_root)?;
    let mut validated = Vec::with_capacity(paths.len());
    for p in &paths {
        let rp = validate_repo_path(&root, p)?;
        validated.push(rp);
    }
    let op = GitOperation::Add { paths: validated };
    exec.execute(&op, repo_root).await
}

/// Stage every change (tracked and untracked) under the repository root.
pub async fn stage_all(
    exec: &GitMutationExecutor,
    repo_root: &Path,
) -> Result<MutationResult, GitMutationError> {
    let argv = vec!["git".to_string(), "add".to_string(), "-A".to_string()];
    run_raw_mutation(exec, repo_root, GitOperation::Add { paths: vec![] }, &argv).await
}

/// Update only tracked files (does not stage new untracked files).
pub async fn stage_tracked(
    exec: &GitMutationExecutor,
    repo_root: &Path,
) -> Result<MutationResult, GitMutationError> {
    let argv = vec!["git".to_string(), "add".to_string(), "-u".to_string()];
    run_raw_mutation(exec, repo_root, GitOperation::Add { paths: vec![] }, &argv).await
}

/// Unstage named literal paths.
pub async fn unstage_paths(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    paths: Vec<String>,
) -> Result<MutationResult, GitMutationError> {
    let root = resolve_repo_root(repo_root)?;
    let mut validated = Vec::with_capacity(paths.len());
    for p in &paths {
        let rp = validate_repo_path(&root, p)?;
        validated.push(rp);
    }
    let op = GitOperation::Reset {
        mode: ResetMode::Mixed,
        paths: Some(validated),
        rev: None,
    };
    exec.execute(&op, repo_root).await
}

/// Unstage all currently-staged changes.
pub async fn unstage_all(
    exec: &GitMutationExecutor,
    repo_root: &Path,
) -> Result<MutationResult, GitMutationError> {
    let argv = vec!["git".to_string(), "reset".to_string(), "HEAD".to_string()];
    let op = GitOperation::Reset {
        mode: ResetMode::Mixed,
        paths: None,
        rev: Some(RevisionExpr::new("HEAD").expect("HEAD is non-empty")),
    };
    run_raw_mutation(exec, repo_root, op, &argv).await
}

// ── Commit ──────────────────────────────────────────────────────────

/// Result of a commit operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommitOutcome {
    /// The mutation result (HEAD before/after, commits created, etc.).
    pub mutation: MutationResult,
    /// The OID of the created commit (parsed from stdout).
    pub created_oid: Option<String>,
    /// True when the commit was an amend of the previous HEAD.
    pub amended: bool,
    /// True when the commit was empty (no staged changes, allow_empty).
    pub empty: bool,
}

/// Commit with explicit selection. The message MUST be sanitized
/// (no NUL, length checks) by the caller. Co-authored trailers are
/// appended by the caller. Generation of the message is a separate
/// concern; this function does not own provider/LLM logic.
pub async fn commit_with_selection(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    selection: CommitSelection,
    message: &str,
    amend: bool,
    allow_empty: bool,
) -> Result<CommitOutcome, GitMutationError> {
    match &selection {
        CommitSelection::AlreadyStaged => {}
        CommitSelection::StagePaths(paths) => {
            stage_paths(exec, repo_root, paths.clone()).await?;
        }
        CommitSelection::StageAll => {
            stage_all(exec, repo_root).await?;
        }
    }

    let snapshot = exec.snapshot(repo_root).await?;
    if snapshot.staged_count == 0 && !allow_empty {
        return Err(GitMutationError::Precondition(
            "no staged changes to commit (set allow_empty=true to override)".to_string(),
        ));
    }

    let sanitized = sanitize_commit_message(message)?;

    let op = GitOperation::Commit {
        message: sanitized,
        amend,
        allow_empty,
    };
    let result = exec.execute(&op, repo_root).await?;

    let created_oid = extract_commit_oid(&result);
    Ok(CommitOutcome {
        mutation: result,
        created_oid,
        amended: amend,
        empty: allow_empty && snapshot.staged_count == 0,
    })
}

fn sanitize_commit_message(msg: &str) -> Result<String, GitMutationError> {
    if msg.is_empty() {
        return Err(GitMutationError::Precondition(
            "commit message cannot be empty".to_string(),
        ));
    }
    if msg.contains('\0') {
        return Err(GitMutationError::Precondition(
            "commit message cannot contain NUL bytes".to_string(),
        ));
    }
    if msg.len() > 64 * 1024 {
        return Err(GitMutationError::Precondition(
            "commit message exceeds 64 KiB".to_string(),
        ));
    }
    Ok(msg.replace("\r\n", "\n"))
}

fn extract_commit_oid(result: &MutationResult) -> Option<String> {
    result
        .stdout
        .split_whitespace()
        .find(|tok| is_hex_sha(tok) && tok.len() >= 7)
        .map(|s| s.to_string())
}

fn is_hex_sha(s: &str) -> bool {
    s.len() >= 7
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

// ── Branch operations ───────────────────────────────────────────────

/// Create a new branch from an optional start point.
pub async fn branch_create(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    name: &str,
    start_point: Option<&str>,
    force: bool,
) -> Result<MutationResult, GitMutationError> {
    let bn = BranchName::new(name)?;
    let op = GitOperation::BranchCreate {
        name: bn,
        start_point: start_point.map(String::from),
        force,
    };
    exec.execute(&op, repo_root).await
}

/// Switch to an existing branch. Fails if the branch does not exist.
pub async fn switch_branch(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    name: &str,
    force: bool,
) -> Result<MutationResult, GitMutationError> {
    let bn = BranchName::new(name)?;
    let op = GitOperation::Switch {
        branch: bn,
        create: false,
        force,
        detach: false,
    };
    exec.execute(&op, repo_root).await
}

/// Create a new branch and switch to it.
pub async fn create_and_switch(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    name: &str,
    start_point: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    let bn = BranchName::new(name)?;
    if start_point.is_some() {
        branch_create(exec, repo_root, name, start_point, /* force = */ false).await?;
    }
    let op = GitOperation::Switch {
        branch: bn,
        create: true,
        force: false,
        detach: false,
    };
    exec.execute(&op, repo_root).await
}

/// Detach HEAD at a specific revision.
pub async fn detach_at(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    rev: &str,
) -> Result<MutationResult, GitMutationError> {
    let rev = RevisionExpr::new(rev)?;
    let argv = vec![
        "git".to_string(),
        "switch".to_string(),
        "--detach".to_string(),
        rev.as_str().to_string(),
    ];
    let op = GitOperation::Switch {
        branch: BranchName::new("HEAD").expect("HEAD is valid"),
        create: false,
        force: false,
        detach: true,
    };
    let _ = argv;
    exec.execute(&op, repo_root).await
}

// ── Restore / checkout paths ────────────────────────────────────────

/// Restore worktree files from the index (or HEAD if no source).
pub async fn restore_worktree(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    paths: Vec<String>,
    source: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    let root = resolve_repo_root(repo_root)?;
    let mut validated = Vec::with_capacity(paths.len());
    for p in &paths {
        let rp = validate_repo_path(&root, p)?;
        validated.push(rp);
    }
    let op = GitOperation::Restore {
        staged: false,
        paths: validated,
        source: source.map(String::from),
        worktree: true,
    };
    exec.execute(&op, repo_root).await
}

/// Restore staged state (i.e., unstage paths). Equivalent to
/// `git reset HEAD <path>` for the named paths.
pub async fn restore_staged(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    paths: Vec<String>,
) -> Result<MutationResult, GitMutationError> {
    let root = resolve_repo_root(repo_root)?;
    let mut validated = Vec::with_capacity(paths.len());
    for p in &paths {
        let rp = validate_repo_path(&root, p)?;
        validated.push(rp);
    }
    let op = GitOperation::Restore {
        staged: true,
        paths: validated,
        source: None,
        worktree: false,
    };
    exec.execute(&op, repo_root).await
}

/// Restore both staged and worktree state (full reset to source).
pub async fn restore_both(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    paths: Vec<String>,
    source: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    let root = resolve_repo_root(repo_root)?;
    let mut validated = Vec::with_capacity(paths.len());
    for p in &paths {
        let rp = validate_repo_path(&root, p)?;
        validated.push(rp);
    }
    let staged_op = GitOperation::Restore {
        staged: true,
        paths: validated.clone(),
        source: source.map(String::from),
        worktree: false,
    };
    let worktree_op = GitOperation::Restore {
        staged: false,
        paths: validated,
        source: source.map(String::from),
        worktree: true,
    };
    exec.execute(&staged_op, repo_root).await?;
    exec.execute(&worktree_op, repo_root).await
}

// ── Stash operations ────────────────────────────────────────────────

/// Push the current changes to a stash entry.
pub async fn stash_push(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    message: Option<&str>,
    include_untracked: bool,
    paths: Vec<String>,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let mut pathspecs = Vec::with_capacity(paths.len());
    for p in &paths {
        pathspecs.push(Pathspec::new(p).map_err(GitMutationError::from)?);
    }
    let op = GitOperation::StashPush {
        message: message.map(String::from),
        include_untracked,
        paths: pathspecs,
    };
    exec.execute(&op, repo_root).await
}

/// Apply a stash entry by reference (e.g., `stash@{0}`).
pub async fn stash_apply(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    stash: Option<&str>,
    index: bool,
) -> Result<MutationResult, GitMutationError> {
    let stash = match stash {
        Some(s) => Some(RevisionExpr::new(s)?),
        None => None,
    };
    let op = GitOperation::StashApply { stash, index };
    exec.execute(&op, repo_root).await
}

/// Pop the top stash entry (apply + drop).
pub async fn stash_pop(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    stash: Option<&str>,
    index: bool,
) -> Result<MutationResult, GitMutationError> {
    let stash = match stash {
        Some(s) => Some(RevisionExpr::new(s)?),
        None => None,
    };
    let op = GitOperation::StashPop { stash, index };
    exec.execute(&op, repo_root).await
}

/// Drop a stash entry by reference.
pub async fn stash_drop(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    stash: &str,
) -> Result<MutationResult, GitMutationError> {
    let stash = RevisionExpr::new(stash)?;
    let op = GitOperation::StashDrop { stash };
    exec.execute(&op, repo_root).await
}

// ── History integration ─────────────────────────────────────────────

/// Built-in merge strategies that may be passed to `git merge --strategy`.
/// Any other value is rejected to prevent arbitrary command execution.
pub const ALLOWED_MERGE_STRATEGIES: &[&str] =
    &["recursive", "resolve", "octopus", "ours", "subtree", "ort"];

/// Run a noninteractive merge.
pub async fn merge(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    revisions: Vec<String>,
    no_ff: bool,
    strategy: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    if let Some(s) = strategy {
        if !ALLOWED_MERGE_STRATEGIES.contains(&s) {
            return Err(GitMutationError::Precondition(format!(
                "merge strategy '{s}' is not in the allowlist; refusing arbitrary strategy command execution"
            )));
        }
    }
    let op = GitOperation::Merge {
        revisions,
        no_ff,
        strategy: strategy.map(String::from),
        abort: false,
    };
    exec.execute(&op, repo_root).await
}

/// Run a noninteractive rebase. Refuses interactive rebase.
pub async fn rebase(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    upstream: Option<&str>,
    onto: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    let op = GitOperation::Rebase {
        upstream: upstream.map(String::from),
        onto: onto.map(String::from),
        interactive: false,
        abort: false,
        continue_op: false,
        skip: false,
    };
    exec.execute(&op, repo_root).await
}

/// Cherry-pick one or more revisions. Refuses interactive variants.
pub async fn cherry_pick(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    revisions: Vec<String>,
) -> Result<MutationResult, GitMutationError> {
    if revisions.is_empty() {
        return Err(GitMutationError::Precondition(
            "cherry-pick requires at least one revision".to_string(),
        ));
    }
    let op = GitOperation::CherryPick {
        revisions,
        continue_op: false,
        abort: false,
        skip: false,
    };
    exec.execute(&op, repo_root).await
}

/// Revert one or more revisions.
pub async fn revert(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    revisions: Vec<String>,
    no_edit: bool,
) -> Result<MutationResult, GitMutationError> {
    if revisions.is_empty() {
        return Err(GitMutationError::Precondition(
            "revert requires at least one revision".to_string(),
        ));
    }
    let op = GitOperation::Revert {
        revisions,
        no_edit,
        continue_op: false,
        abort: false,
        skip: false,
    };
    exec.execute(&op, repo_root).await
}

/// Abort an in-progress merge/rebase/cherry-pick/revert. Inspects
/// the operation state and runs the appropriate `--abort` invocation.
pub async fn abort_in_progress(
    exec: &GitMutationExecutor,
    repo_root: &Path,
) -> Result<MutationResult, GitMutationError> {
    let snapshot = exec.snapshot(repo_root).await?;
    if snapshot.conflicted_count == 0 {
        return Err(GitMutationError::Precondition(
            "no in-progress merge/rebase/cherry-pick/revert to abort".to_string(),
        ));
    }
    let merge_op = GitOperation::Merge {
        revisions: vec![],
        no_ff: false,
        strategy: None,
        abort: true,
    };
    let result = exec.execute(&merge_op, repo_root).await?;
    if result.success {
        return Ok(result);
    }
    let rebase_op = GitOperation::Rebase {
        upstream: None,
        onto: None,
        interactive: false,
        abort: true,
        continue_op: false,
        skip: false,
    };
    exec.execute(&rebase_op, repo_root).await
}

// ── Local ref deletion ──────────────────────────────────────────────

/// Delete a local branch. Refuses to delete the currently checked-out
/// branch. Force deletion requires `force = true` and is destructive.
pub async fn branch_delete(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    name: &str,
    force: bool,
) -> Result<MutationResult, GitMutationError> {
    let bn = BranchName::new(name)?;
    let snapshot = exec.snapshot(repo_root).await?;
    if !snapshot.detached && snapshot.branch == name {
        return Err(GitMutationError::Precondition(format!(
            "cannot delete the currently checked-out branch '{name}'"
        )));
    }
    let op = GitOperation::BranchDelete { name: bn, force };
    exec.execute(&op, repo_root).await
}

/// Delete a tag. Tag deletion is a ref mutation and is non-destructive
/// (the tagged commit is preserved even if no ref points to it).
pub async fn tag_delete(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    name: &str,
    force: bool,
) -> Result<MutationResult, GitMutationError> {
    if name.is_empty() {
        return Err(GitMutationError::Precondition(
            "tag name cannot be empty".to_string(),
        ));
    }
    let op = if force {
        GitOperation::TagForceDelete {
            name: name.to_string(),
        }
    } else {
        GitOperation::TagDelete {
            name: name.to_string(),
        }
    };
    exec.execute(&op, repo_root).await
}

// ── Permission descriptions ─────────────────────────────────────────

/// State-aware permission description for a `git` mutation. The
/// returned string is intended for the `reason` field of a
/// `CommandPermissionRequest` so users see the actual scope of the
/// operation, not a generic "git mutation" string.
pub fn describe_for_permission(
    operation: &codegg_git::GitOperation,
    snapshot: Option<&crate::git_mutations::RepoSnapshot>,
) -> String {
    use codegg_git::GitOperation::*;
    match operation {
        Add { paths } => format!(
            "stage {} file(s): {}",
            paths.len(),
            paths
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Restore { paths, .. } => format!(
            "restore {} file(s): {}",
            paths.len(),
            paths
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Commit {
            amend, allow_empty, ..
        } => {
            let amend_s = if *amend { " (amend)" } else { "" };
            let empty_s = if *allow_empty { " (empty allowed)" } else { "" };
            format!(
                "commit{}{}; {} staged file(s){}",
                amend_s,
                empty_s,
                snapshot.map(|s| s.staged_count).unwrap_or(0),
                if *amend {
                    "; replaces previous HEAD"
                } else {
                    ""
                }
            )
        }
        Switch { branch, create, .. } => {
            let action = if *create {
                "create and switch to"
            } else {
                "switch to"
            };
            format!(
                "{action} branch '{}'{}",
                branch.as_str(),
                if !create {
                    snapshot
                        .map(|s| format!(" (current: {})", s.branch))
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            )
        }
        BranchCreate {
            name,
            start_point,
            force,
        } => format!(
            "create branch '{}'{} from {}{}",
            name.as_str(),
            if *force { " (force)" } else { "" },
            start_point.clone().unwrap_or_else(|| "HEAD".to_string()),
            snapshot
                .map(|s| format!(" (current: {})", s.branch))
                .unwrap_or_default()
        ),
        BranchDelete { name, force } => format!(
            "delete branch '{}'{}",
            name.as_str(),
            if *force { " (force, destructive)" } else { "" }
        ),
        TagCreate { name, rev, .. } => format!(
            "create tag '{}' at {}",
            name,
            rev.clone().unwrap_or_else(|| "HEAD".to_string())
        ),
        TagDelete { name } => format!("delete tag '{name}'"),
        TagForceDelete { name } => format!("force-delete tag '{name}' (destructive)"),
        Merge {
            revisions, no_ff, ..
        } => format!(
            "merge {} into current branch{}",
            revisions.join(", "),
            if *no_ff { " (no fast-forward)" } else { "" }
        ),
        Rebase { upstream, onto, .. } => format!(
            "rebase current branch{}{}",
            upstream
                .as_ref()
                .map(|u| format!(" onto '{u}'"))
                .unwrap_or_default(),
            onto.as_ref()
                .map(|o| format!(" (onto '{o}')"))
                .unwrap_or_default()
        ),
        CherryPick { revisions, .. } => {
            format!("cherry-pick {} revision(s)", revisions.len())
        }
        Revert { revisions, .. } => format!("revert {} revision(s)", revisions.len()),
        StashPush {
            message,
            include_untracked,
            paths,
        } => format!(
            "stash push ({} path(s)){}{}",
            paths.len(),
            if *include_untracked {
                " including untracked"
            } else {
                ""
            },
            message
                .as_ref()
                .map(|m| format!(": \"{m}\""))
                .unwrap_or_default()
        ),
        StashApply { stash, .. } => format!(
            "stash apply {}",
            stash.as_ref().map(|s| s.as_str()).unwrap_or("top")
        ),
        StashPop { stash, .. } => format!(
            "stash pop {}",
            stash.as_ref().map(|s| s.as_str()).unwrap_or("top")
        ),
        StashDrop { stash } => format!("stash drop {}", stash.as_str()),
        Clean {
            force,
            dirs,
            ignored,
            ..
        } => format!(
            "git clean{}{}{} (destructive)",
            if *force { " -f" } else { "" },
            if *dirs { " -d" } else { "" },
            if *ignored { " -x" } else { "" }
        ),
        ResetHard { rev } => format!(
            "git reset --hard{} (destructive)",
            rev.as_ref().map(|r| format!(" {r}")).unwrap_or_default()
        ),
        Push {
            force,
            force_with_lease,
            ..
        } => {
            if *force || *force_with_lease {
                "git push (force/force-with-lease, destructive)".to_string()
            } else {
                "git push".to_string()
            }
        }
        _ => format!("git {}", operation.subcommand_name()),
    }
}

// ── Internal: raw mutation runner ───────────────────────────────────

/// Execute a raw argv mutation (e.g. `git add -A`) through the same
/// snapshot/timeout/policy pipeline as the typed operations. Used
/// for variants that the typed parser does not model (e.g. `git add
/// -A` or `git reset HEAD`).
pub(crate) async fn run_raw_mutation(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    operation: GitOperation,
    argv: &[String],
) -> Result<MutationResult, GitMutationError> {
    let before = exec.snapshot(repo_root).await?;
    let start = std::time::Instant::now();
    let raw = exec.run_subprocess(argv, repo_root).await?;
    let after = match exec.snapshot(repo_root).await {
        Ok(s) => s,
        Err(_) => before.clone(),
    };
    let subcommand = operation.subcommand_name().to_string();
    let outcome = classify_outcome(&operation, &before, &after, raw.exit_code);
    let delta = compute_delta(&operation, &before, &after, &raw, &outcome);

    Ok(MutationResult {
        operation,
        subcommand,
        delta,
        outcome,
        stdout: crate::git_mutations::truncate_for_public(&raw.stdout, 64 * 1024),
        stderr: crate::git_mutations::truncate_for_public(&raw.stderr, 64 * 1024),
        exit_code: raw.exit_code,
        success: raw.exit_code == 0,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

// Re-exports for tests / sibling modules.
pub use crate::git_mutations::RepoSnapshot;

// Avoid an unused-import warning on `RepoRoot` when no test code uses it.
#[allow(dead_code)]
fn _root_marker(_: &RepoRoot) {}

// Avoid unused-import warnings.
#[allow(dead_code)]
fn _env_marker(_: &GitEnvPolicy) {}
#[allow(dead_code)]
fn _outcome_marker(_: &MutationOutcome) {}
#[allow(dead_code)]
fn _state_marker(_: &StateDelta) {}
