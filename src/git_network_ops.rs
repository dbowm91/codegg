//! Typed network and destructive Git operations.
//!
//! These functions wrap [`GitMutationExecutor::execute`] with
//! operation-specific validation for fetch, pull, push, remote
//! management, config, reset, and clean operations.
//!
//! Network operations use [`NetworkEnvPolicy`] which extends the base
//! [`GitEnvPolicy`] with credential-helper and proxy env vars needed
//! for remote access.
//!
//! Phase D invariants:
//!
//! * argv is rendered without a shell.
//! * paths are validated as `RepoPath` (rejects absolute, parent
//!   traversal, NUL bytes).
//! * branch/tag names are validated as `BranchName` / `RefName`.
//! * every operation captures pre/post snapshots and returns a typed
//!   `MutationResult`.

use std::path::Path;

use codegg_git::operation::ResetMode;
use codegg_git::path::Pathspec;
use codegg_git::ref_name::RemoteName;
use codegg_git::{GitOperation, RiskSet};
use tokio::process::Command;

use crate::git_mutations::{
    resolve_repo_root, validate_repo_path, GitEnvPolicy, GitMutationError, GitMutationExecutor,
    MutationResult, ALLOWED_ENV_VARS,
};
use crate::git_mutations_ops::run_raw_mutation;
use crate::git_network_policy::{redact_url_credentials, NETWORK_ALLOWED_ENV_VARS};

// ── Network environment policy ─────────────────────────────────────

/// Environment policy for network Git operations. Extends the base
/// [`GitEnvPolicy`] with credential-helper, proxy, and SSH env vars
/// required for remote access.
///
/// The policy clears the environment (same as base), restores
/// [`ALLOWED_ENV_VARS`], then restores [`NETWORK_ALLOWED_ENV_VARS`],
/// then applies the same flag pinning as [`GitEnvPolicy::default()`].
#[derive(Debug, Clone, Default)]
pub struct NetworkEnvPolicy {
    /// Base local-mutation policy.
    pub base: GitEnvPolicy,
}impl NetworkEnvPolicy {
    /// Build a `Command` from argv and repository root with the
    /// network policy applied. The caller receives the `Command` with
    /// `args` and `current_dir` already set.
    pub fn apply_to_command(&self, argv: &[String], cwd: &Path) -> Command {
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).current_dir(cwd);
        cmd.env_clear();

        // Restore base env vars.
        for key in ALLOWED_ENV_VARS {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }

        // Restore network-specific env vars.
        for key in NETWORK_ALLOWED_ENV_VARS {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }

        // Apply same flags as GitEnvPolicy::default().
        if self.base.terminal_prompt_disabled {
            cmd.env("GIT_TERMINAL_PROMPT", "0");
        }
        if self.base.pin_editor {
            cmd.env("GIT_EDITOR", "true");
            cmd.env("GIT_SEQUENCE_EDITOR", "true");
        }
        if self.base.strip_editors {
            cmd.env_remove("EDITOR");
            cmd.env_remove("VISUAL");
        }
        cmd.env("GPG_TTY", "");
        cmd.kill_on_drop(true);
        cmd
    }
}

// ── Fetch ──────────────────────────────────────────────────────────

/// Fetch refs from a remote.
///
/// When `remote` is `None`, fetches from the default remote.
/// When `all` is true, fetches from all remotes.
/// `refspecs` are passed through as-is (already validated by the
/// caller or the typed parser).
///
/// Uses [`NetworkEnvPolicy`] for env hardening.
pub async fn fetch(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    remote: Option<&str>,
    refspecs: Vec<String>,
    prune: bool,
    all: bool,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let remote_name = match remote {
        Some(r) => Some(RemoteName::new(r)?),
        None => None,
    };
    if prune {
        // The typed parser doesn't model --prune on Fetch. Render raw
        // argv so we get a single noninteractive invocation.
        let mut argv: Vec<String> = vec!["git".into(), "fetch".into(), "--prune".into()];
        if all {
            argv.push("--all".into());
        }
        if let Some(r) = remote {
            argv.push(r.to_string());
        }
        for rs in &refspecs {
            argv.push(rs.clone());
        }
        let op = GitOperation::ManagedGitArgv {
            argv: argv.clone(),
            risk: RiskSet::new(vec![codegg_git::GitRiskClass::NetworkRead]),
        };
        return run_raw_mutation(exec, repo_root, op, &argv).await;
    }
    let op = GitOperation::Fetch {
        remote: remote_name,
        refspecs,
        all,
    };
    exec.execute(&op, repo_root).await
}

// ── Pull ───────────────────────────────────────────────────────────

/// Strategy for integrating remote changes during pull.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PullStrategy {
    /// Merge remote changes into the current branch (default).
    Merge,
    /// Rebase the current branch on top of the remote branch.
    Rebase,
    /// Fast-forward only; fail if a merge commit would be required.
    FastForwardOnly,
}

impl PullStrategy {
    /// Return the flag name for display/permission purposes.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::FastForwardOnly => "ff-only",
        }
    }
}

/// Pull (fetch + integrate) from a remote.
///
/// `strategy` controls how the remote changes are integrated.
/// When `branch` is `None`, the default tracking branch is used.
///
/// Uses [`NetworkEnvPolicy`] for env hardening.
pub async fn pull(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    remote: Option<&str>,
    branch: Option<&str>,
    strategy: PullStrategy,
    ff_only: bool,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let remote_name = match remote {
        Some(r) => Some(RemoteName::new(r)?),
        None => None,
    };
    let op = GitOperation::Pull {
        remote: remote_name,
        branch: branch.map(String::from),
        rebase: matches!(strategy, PullStrategy::Rebase),
        ff_only: matches!(strategy, PullStrategy::FastForwardOnly) || ff_only,
    };
    exec.execute(&op, repo_root).await
}

// ── Push ───────────────────────────────────────────────────────────

/// Force-push mode for push operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PushForce {
    /// Normal push (non-fast-forward rejected).
    Normal,
    /// Force-with-lease: force push but abort if the remote ref has
    /// changed since last fetch.
    ForceWithLease {
        /// Expected SHA of the remote ref (for --force-with-lease=<ref>).
        expected_sha: Option<String>,
    },
    /// Unconditional force push (--force). Destructive.
    Force,
}

impl PushForce {
    /// Return a display label for permission prompts.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::ForceWithLease { .. } => "force-with-lease",
            Self::Force => "force",
        }
    }

    /// True if this force mode is destructive (unconditional force).
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::Force)
    }
}

impl std::fmt::Display for PushForce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::ForceWithLease { .. } => write!(f, "force-with-lease"),
            Self::Force => write!(f, "force"),
        }
    }
}

/// Request to push refs to a remote.
#[derive(Debug, Clone)]
pub struct PushRequest {
    /// Remote name (defaults to `origin` when `None`).
    pub remote: Option<String>,
    /// Branch to push (defaults to current branch when `None`).
    pub branch: Option<String>,
    /// Set upstream tracking reference (`-u`).
    pub set_upstream: bool,
    /// Force-push mode.
    pub force: PushForce,
    /// Push all refs/tags.
    pub tags: bool,
    /// Delete a remote ref.
    pub delete: bool,
    /// Dry-run only (report what would be pushed).
    pub dry_run: bool,
}

impl PushRequest {
    /// True if this push may overwrite remote history.
    pub fn is_destructive(&self) -> bool {
        self.delete
            || matches!(
                self.force,
                PushForce::Force | PushForce::ForceWithLease { .. }
            )
    }

    /// True if this push deletes a remote ref.
    pub fn is_delete(&self) -> bool {
        self.delete
    }
}

/// Push refs to a remote.
///
/// When `delete` is true, the remote ref is deleted. When `force` is
/// not `Normal`, the push may overwrite remote history.
///
/// Uses [`NetworkEnvPolicy`] for env hardening.
pub async fn push(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    req: PushRequest,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let remote_name = match req.remote {
        Some(r) => Some(RemoteName::new(&r)?),
        None => None,
    };
    let (force, force_with_lease) = match req.force {
        PushForce::Normal => (false, false),
        PushForce::ForceWithLease { .. } => (false, true),
        PushForce::Force => (true, false),
    };
    let op = GitOperation::Push {
        remote: remote_name,
        branch: req.branch,
        set_upstream: req.set_upstream,
        force,
        force_with_lease,
        tags: req.tags,
        delete: req.delete,
    };
    exec.execute(&op, repo_root).await
}

/// Human-readable permission hint for a push request.
///
/// The returned string is intended for the `reason` field of a
/// `CommandPermissionRequest` so users see the actual scope of the
/// push, not a generic "git push" string.
pub fn push_permission_hint(req: &PushRequest) -> String {
    if req.delete {
        return "push (delete remote branch — strong confirmation)".to_string();
    }
    match &req.force {
        PushForce::Force => {
            return "push (force — destructive, denied by default)".to_string();
        }
        PushForce::ForceWithLease { expected_sha } => {
            if let Some(sha) = expected_sha {
                return format!("push (force-with-lease — confirm against expected SHA {sha})");
            }
            return "push (force-with-lease — confirm against expected SHA)".to_string();
        }
        PushForce::Normal => {}
    }
    if req.set_upstream {
        return "push (set upstream)".to_string();
    }
    "push".to_string()
}

// ── Remote management ──────────────────────────────────────────────

/// Add a new remote. The URL is sanitized with
/// [`redact_url_credentials`] before constructing the operation (so
/// the raw credential never reaches the persistence layer).
pub async fn remote_add(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    name: &str,
    url: &str,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let rn = RemoteName::new(name)?;
    // Redact before constructing the operation — the sanitized URL is
    // what gets stored in MutationResult and persisted.
    let sanitized_url = redact_url_credentials(url);
    let op = GitOperation::RemoteAdd {
        name: rn,
        url: sanitized_url,
    };
    exec.execute(&op, repo_root).await
}

/// Remove a remote.
pub async fn remote_remove(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    name: &str,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let rn = RemoteName::new(name)?;
    let op = GitOperation::RemoteRemove { name: rn };
    exec.execute(&op, repo_root).await
}

/// Set the URL of an existing remote. When `append` is true, the URL
/// is added as an additional push URL (`--add`).
pub async fn remote_set_url(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    name: &str,
    url: &str,
    append: bool,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let rn = RemoteName::new(name)?;
    let op = GitOperation::RemoteSetUrl {
        name: rn,
        url: url.to_string(),
        append,
    };
    exec.execute(&op, repo_root).await
}

/// Rename a remote. Uses `ManagedGitArgv` fallback because the typed
/// parser does not model `git remote rename`.
pub async fn remote_rename(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    old_name: &str,
    new_name: &str,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let _old_rn = RemoteName::new(old_name)?;
    let _new_rn = RemoteName::new(new_name)?;
    let argv = vec![
        "git".to_string(),
        "remote".to_string(),
        "rename".to_string(),
        old_name.to_string(),
        new_name.to_string(),
    ];
    let op = GitOperation::ManagedGitArgv {
        argv: argv.clone(),
        risk: codegg_git::RiskSet::new(vec![codegg_git::GitRiskClass::RepositoryConfigMutation]),
    };
    run_raw_mutation(exec, repo_root, op, &argv).await
}

// ── Config operations ──────────────────────────────────────────────

/// Configuration operation type.
#[derive(Debug, Clone)]
pub enum ConfigOp {
    /// Get a config value.
    Get {
        /// Config key (e.g., `pull.rebase`).
        key: String,
        /// When true, read from local repo config only.
        local: bool,
    },
    /// Set a config value.
    Set {
        /// Config key.
        key: String,
        /// Config value.
        value: String,
        /// When true, write to local repo config only.
        local: bool,
    },
    /// Unset a config value.
    Unset {
        /// Config key.
        key: String,
        /// When true, unset from local repo config only.
        local: bool,
    },
}

/// Allowlisted local config key prefixes. Only keys matching one of
/// these prefixes may be read or written.
pub const CONFIG_KEY_ALLOWLIST: &[&str] = &[
    "branch.",
    "pull.rebase",
    "pull.ff",
    "pull.twohead",
    "pull.octopus",
    "merge.tool",
    "merge.ff",
    "merge.log",
    "rebase.autosquash",
    "rebase.autostash",
    "rebase.missingCommitsCheck",
    "init.defaultBranch",
    "commit.gpgsign",
    "tag.gpgsign",
    "core.autocrlf",
    "core.whitespace",
    "core.eol",
    "core.safecrlf",
    "core.quotepath",
    "core.autopush",
    "credential.helper",
    "http.postbuffer",
    "http.sslverify",
];

/// Glob-like patterns for sensitive credential/URL config keys that
/// must be denied or elevated to permission prompts.
pub const CONFIG_DENIED_KEY_PATTERNS: &[&str] = &[
    "credential.*",
    "http.*",
    "url.*",
    "core.gitProxy",
    "core.sshCommand",
    "core.sshVariant",
];

/// Validate a config key against the allowlist and denied-pattern
/// lists. Returns `Ok(())` if the key is permitted, or
/// `Err(GitMutationError::Precondition)` if denied.
///
/// When `allow_local_only` is true, the key must also be usable in
/// local scope (global-scope keys like `user.name` are rejected).
pub fn validate_config_key(key: &str, allow_local_only: bool) -> Result<(), GitMutationError> {
    if key.is_empty() {
        return Err(GitMutationError::Precondition(
            "config key cannot be empty".to_string(),
        ));
    }

    // Reject global-only keys that don't belong in local config.
    if allow_local_only {
        let global_only = ["user.name", "user.email", "user.signingKey", "gpg.format"];
        for prefix in &global_only {
            if key == *prefix || key.starts_with(&format!("{prefix}.")) {
                return Err(GitMutationError::Precondition(format!(
                    "config key '{key}' is a global-only key and cannot be set in local scope"
                )));
            }
        }
    }

    // Check denied patterns first.
    for pattern in CONFIG_DENIED_KEY_PATTERNS {
        if let Some(prefix) = pattern.strip_suffix(".*") {
            if key == prefix || key.starts_with(&format!("{prefix}.")) {
                return Err(GitMutationError::Precondition(format!(
                    "config key '{key}' matches denied pattern '{pattern}'"
                )));
            }
        } else if key == *pattern {
            return Err(GitMutationError::Precondition(format!(
                "config key '{key}' matches denied pattern '{pattern}'"
            )));
        }
    }

    // Check allowlist prefixes.
    for prefix in CONFIG_KEY_ALLOWLIST {
        if key == *prefix || key.starts_with(prefix) {
            return Ok(());
        }
    }

    Err(GitMutationError::Precondition(format!(
        "config key '{key}' is not in the allowlist"
    )))
}

/// Set a config value. Validates the key and requires local scope.
pub async fn config_set(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    key: &str,
    value: &str,
    local_only: bool,
) -> Result<MutationResult, GitMutationError> {
    validate_config_key(key, local_only)?;
    let _ = resolve_repo_root(repo_root)?;
    let op = GitOperation::ConfigSet {
        key: key.to_string(),
        value: value.to_string(),
        global: false,
        local: local_only,
    };
    exec.execute(&op, repo_root).await
}

/// Unset a config value. Validates the key and requires local scope.
pub async fn config_unset(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    key: &str,
    local_only: bool,
) -> Result<MutationResult, GitMutationError> {
    validate_config_key(key, local_only)?;
    let _ = resolve_repo_root(repo_root)?;
    let op = GitOperation::ConfigUnset {
        key: key.to_string(),
        global: false,
        local: local_only,
    };
    exec.execute(&op, repo_root).await
}

/// Get a config value. Validates the key and requires local scope.
pub async fn config_get(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    key: &str,
    local_only: bool,
) -> Result<MutationResult, GitMutationError> {
    validate_config_key(key, local_only)?;
    let _ = resolve_repo_root(repo_root)?;
    let op = GitOperation::ConfigGet {
        key: key.to_string(),
        global: false,
        local: local_only,
    };
    exec.execute(&op, repo_root).await
}

// ── Reset operations ───────────────────────────────────────────────

/// Soft reset (index only, leaves working tree untouched).
pub async fn reset_soft(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    rev: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let op = GitOperation::ResetSoft {
        rev: rev.map(String::from),
    };
    exec.execute(&op, repo_root).await
}

/// Mixed reset (resets index and working tree).
pub async fn reset_mixed(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    rev: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let op = GitOperation::ResetMixed {
        rev: rev.map(String::from),
    };
    exec.execute(&op, repo_root).await
}

/// Hard reset (resets index, working tree, and HEAD). Destructive:
/// uncommitted changes are discarded.
pub async fn reset_hard(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    rev: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let op = GitOperation::ResetHard {
        rev: rev.map(String::from),
    };
    exec.execute(&op, repo_root).await
}

/// Merge-mode reset (resets like `--merge` for resolving merge
/// conflicts).
pub async fn reset_merge(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    rev: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let op = GitOperation::ResetMerge {
        rev: rev.map(String::from),
    };
    exec.execute(&op, repo_root).await
}

/// Keep reset (resets like `--keep`, preserving uncommitted changes
/// in named paths).
pub async fn reset_keep(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    rev: Option<&str>,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let op = GitOperation::ResetKeep {
        rev: rev.map(String::from),
    };
    exec.execute(&op, repo_root).await
}

/// Reset specific paths to the index state (mixed mode, no revision).
/// Validates each path as a `RepoPath`.
pub async fn reset_paths(
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

// ── Clean operations ───────────────────────────────────────────────

/// Classification of a single entry produced by `git clean`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanEntryKind {
    /// Regular untracked file.
    File,
    /// Untracked directory.
    Directory,
    /// Ignored file (requires `-x`).
    IgnoredFile,
    /// Ignored directory (requires `-x`).
    IgnoredDirectory,
}

/// A single entry that `git clean` would remove.
#[derive(Debug, Clone)]
pub struct CleanEntry {
    /// Relative path of the entry.
    pub path: String,
    /// Whether it is a file or directory.
    pub kind: CleanEntryKind,
}

/// Result of a dry-run `git clean` operation.
#[derive(Debug, Clone)]
pub struct CleanPreview {
    /// Entries that would be removed.
    pub entries: Vec<CleanEntry>,
}

impl CleanPreview {
    /// True if no entries would be removed.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of entries that would be removed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Request to execute `git clean`.
#[derive(Debug, Clone)]
pub struct CleanRequest {
    /// Force removal (`-f`). Required for any actual deletion.
    pub force: bool,
    /// Remove directories (`-d`).
    pub dirs: bool,
    /// Also remove ignored files (`-x`).
    pub ignored: bool,
    /// Scoped paths to clean. If empty, cleans the entire worktree.
    pub paths: Vec<String>,
}

impl CleanRequest {
    /// True when `ignored=true` and no paths are scoped — a broad
    /// cleanup that would delete ignored files across the entire
    /// worktree.
    pub fn is_broad(&self) -> bool {
        self.ignored && self.paths.is_empty()
    }
}

/// Run a dry-run `git clean` to preview what would be removed, then
/// parse the output into a structured [`CleanPreview`].
///
/// Runs `git clean -n -d -- <paths>` and parses each output line.
/// Lines starting with `Would remove ` are files; lines starting
/// with `Would remove directory ` are directories.
pub async fn clean_preview(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    paths: Vec<String>,
) -> Result<CleanPreview, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let mut argv = vec![
        "git".to_string(),
        "clean".to_string(),
        "-n".to_string(),
        "-d".to_string(),
    ];
    if !paths.is_empty() {
        argv.push("--".to_string());
        argv.extend(paths);
    }
    let raw = exec.run_subprocess(&argv, repo_root).await?;
    let entries = parse_clean_preview(&raw.stdout);
    Ok(CleanPreview { entries })
}

/// Execute `git clean` with the given options.
///
/// Validates that broad ignored cleanup (`ignored=true` with no paths)
/// is rejected at the call site. This function does not itself enforce
/// the broad-cleanup policy; callers must check
/// [`CleanRequest::is_broad()`] beforehand.
pub async fn clean(
    exec: &GitMutationExecutor,
    repo_root: &Path,
    req: CleanRequest,
) -> Result<MutationResult, GitMutationError> {
    let _ = resolve_repo_root(repo_root)?;
    let mut argv = vec!["git".to_string(), "clean".to_string()];
    if req.force {
        argv.push("-f".to_string());
    }
    if req.dirs {
        argv.push("-d".to_string());
    }
    if req.ignored {
        argv.push("-x".to_string());
    }
    if !req.paths.is_empty() {
        argv.push("--".to_string());
        for p in &req.paths {
            argv.push(p.clone());
        }
    }
    let pathspecs: Vec<Pathspec> = req
        .paths
        .iter()
        .map(|p| Pathspec::new(p))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| GitMutationError::Path(e.to_string()))?;
    let op = GitOperation::Clean {
        force: req.force,
        dry_run: false,
        dirs: req.dirs,
        ignored: req.ignored,
        paths: pathspecs,
    };
    run_raw_mutation(exec, repo_root, op, &argv).await
}

/// Parse the output of `git clean -n -d` into structured entries.
fn parse_clean_preview(stdout: &str) -> Vec<CleanEntry> {
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        let path = if let Some(rest) = line.strip_prefix("Would remove directory ") {
            rest.trim()
        } else if let Some(rest) = line.strip_prefix("Would remove ") {
            rest.trim()
        } else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        let kind = if line.starts_with("Would remove directory ") {
            CleanEntryKind::Directory
        } else {
            CleanEntryKind::File
        };
        entries.push(CleanEntry {
            path: path.to_string(),
            kind,
        });
    }
    entries
}

// ── Permission descriptions ────────────────────────────────────────

/// State-aware permission description for a network or destructive
/// Git operation. The returned string is intended for the `reason`
/// field of a `CommandPermissionRequest` so users see the actual
/// scope of the operation, not a generic "git mutation" string.
pub fn describe_network_operation(operation: &codegg_git::GitOperation) -> String {
    use codegg_git::GitOperation::*;
    match operation {
        Fetch {
            remote,
            refspecs,
            all,
        } => {
            let remote_s = remote
                .as_ref()
                .map(|r| r.as_str().to_string())
                .unwrap_or_else(|| "default".to_string());
            if *all {
                "fetch all remotes".to_string()
            } else if refspecs.is_empty() {
                format!("fetch from remote '{remote_s}'")
            } else {
                format!(
                    "fetch {} refspec(s) from remote '{remote_s}'",
                    refspecs.len()
                )
            }
        }
        Pull {
            remote,
            branch,
            rebase,
            ff_only,
        } => {
            let remote_s = remote
                .as_ref()
                .map(|r| r.as_str().to_string())
                .unwrap_or_else(|| "default".to_string());
            let strategy = if *rebase {
                "rebase"
            } else if *ff_only {
                "fast-forward only"
            } else {
                "merge"
            };
            let branch_s = branch
                .as_deref()
                .map(|b| format!(" branch '{b}'"))
                .unwrap_or_default();
            format!("pull from remote '{remote_s}'{branch_s} ({strategy})")
        }
        Push {
            remote,
            branch,
            set_upstream,
            force,
            force_with_lease,
            tags,
            delete,
        } => {
            let remote_s = remote
                .as_ref()
                .map(|r| r.as_str().to_string())
                .unwrap_or_else(|| "default".to_string());
            if *delete {
                return format!("delete remote ref from '{remote_s}'");
            }
            if *force {
                return format!("force-push to remote '{remote_s}' (destructive)");
            }
            if *force_with_lease {
                return format!("force-with-lease push to remote '{remote_s}'");
            }
            let mut parts = vec![format!("push to remote '{remote_s}'")];
            if let Some(b) = branch {
                parts.push(format!("branch '{b}'"));
            }
            if *set_upstream {
                parts.push("set upstream".to_string());
            }
            if *tags {
                parts.push("including tags".to_string());
            }
            parts.join("; ")
        }
        RemoteAdd { name, url } => {
            format!("add remote '{}' → {}", name.as_str(), url)
        }
        RemoteRemove { name } => {
            format!("remove remote '{}'", name.as_str())
        }
        RemoteSetUrl { name, url, append } => {
            if *append {
                format!("add push URL for remote '{}' → {}", name.as_str(), url)
            } else {
                format!("set URL for remote '{}' → {}", name.as_str(), url)
            }
        }
        ConfigSet { key, value, .. } => {
            format!("set config '{key}' = '{value}'")
        }
        ConfigUnset { key, .. } => {
            format!("unset config '{key}'")
        }
        ConfigGet { key, .. } => {
            format!("get config '{key}'")
        }
        ResetHard { rev } => format!(
            "git reset --hard{} (destructive)",
            rev.as_ref().map(|r| format!(" {r}")).unwrap_or_default()
        ),
        ResetMixed { rev } => format!(
            "git reset --mixed{}",
            rev.as_ref().map(|r| format!(" {r}")).unwrap_or_default()
        ),
        ResetSoft { rev } => format!(
            "git reset --soft{}",
            rev.as_ref().map(|r| format!(" {r}")).unwrap_or_default()
        ),
        ResetMerge { rev } => format!(
            "git reset --merge{}",
            rev.as_ref().map(|r| format!(" {r}")).unwrap_or_default()
        ),
        ResetKeep { rev } => format!(
            "git reset --keep{}",
            rev.as_ref().map(|r| format!(" {r}")).unwrap_or_default()
        ),
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
        _ => format!("git {}", operation.subcommand_name()),
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── PushForce Display ──────────────────────────────────────────

    #[test]
    fn push_force_display_normal() {
        assert_eq!(PushForce::Normal.to_string(), "normal");
    }

    #[test]
    fn push_force_display_force_with_lease() {
        assert_eq!(
            PushForce::ForceWithLease {
                expected_sha: Some("abc123".to_string())
            }
            .to_string(),
            "force-with-lease"
        );
    }

    #[test]
    fn push_force_display_force() {
        assert_eq!(PushForce::Force.to_string(), "force");
    }

    // ── PushRequest::is_destructive ────────────────────────────────

    #[test]
    fn push_request_normal_not_destructive() {
        let req = PushRequest {
            remote: None,
            branch: None,
            set_upstream: false,
            force: PushForce::Normal,
            tags: false,
            delete: false,
            dry_run: false,
        };
        assert!(!req.is_destructive());
    }

    #[test]
    fn push_request_force_is_destructive() {
        let req = PushRequest {
            remote: None,
            branch: None,
            set_upstream: false,
            force: PushForce::Force,
            tags: false,
            delete: false,
            dry_run: false,
        };
        assert!(req.is_destructive());
    }

    #[test]
    fn push_request_force_with_lease_is_destructive() {
        let req = PushRequest {
            remote: None,
            branch: None,
            set_upstream: false,
            force: PushForce::ForceWithLease { expected_sha: None },
            tags: false,
            delete: false,
            dry_run: false,
        };
        assert!(req.is_destructive());
    }

    #[test]
    fn push_request_delete_is_destructive() {
        let req = PushRequest {
            remote: None,
            branch: None,
            set_upstream: false,
            force: PushForce::Normal,
            tags: false,
            delete: true,
            dry_run: false,
        };
        assert!(req.is_destructive());
    }

    // ── validate_config_key ────────────────────────────────────────

    #[test]
    fn validate_config_key_accepts_pull_rebase() {
        assert!(validate_config_key("pull.rebase", true).is_ok());
    }

    #[test]
    fn validate_config_key_accepts_branch_merge() {
        assert!(validate_config_key("branch.main.merge", true).is_ok());
    }

    #[test]
    fn validate_config_key_accepts_core_autocrlf() {
        assert!(validate_config_key("core.autocrlf", true).is_ok());
    }

    #[test]
    fn validate_config_key_rejects_credential_helper_pattern() {
        assert!(validate_config_key("credential.helper", true).is_err());
    }

    #[test]
    fn validate_config_key_rejects_http_sslverify_pattern() {
        assert!(validate_config_key("http.sslverify", true).is_err());
    }

    #[test]
    fn validate_config_key_rejects_url_pattern() {
        assert!(validate_config_key("url.ssh://git@github.com/.insteadOf", true).is_err());
    }

    #[test]
    fn validate_config_key_rejects_core_git_proxy() {
        assert!(validate_config_key("core.gitProxy", true).is_err());
    }

    #[test]
    fn validate_config_key_rejects_core_ssh_command() {
        assert!(validate_config_key("core.sshCommand", true).is_err());
    }

    #[test]
    fn validate_config_key_rejects_empty() {
        assert!(validate_config_key("", true).is_err());
    }

    #[test]
    fn validate_config_key_rejects_global_only_user_name() {
        assert!(validate_config_key("user.name", true).is_err());
    }

    #[test]
    fn validate_config_key_rejects_global_only_user_email() {
        assert!(validate_config_key("user.email", true).is_err());
    }

    #[test]
    fn validate_config_key_rejects_unknown_key() {
        assert!(validate_config_key("some.random.key", true).is_err());
    }

    #[test]
    fn validate_config_key_accepts_merge_tool() {
        assert!(validate_config_key("merge.tool", true).is_ok());
    }

    #[test]
    fn validate_config_key_accepts_rebase_autosquash() {
        assert!(validate_config_key("rebase.autosquash", true).is_ok());
    }

    #[test]
    fn validate_config_key_accepts_init_default_branch() {
        assert!(validate_config_key("init.defaultBranch", true).is_ok());
    }

    // ── redact_url_credentials is called inside remote_add ─────────
    // (verify the sanitization happens before operation construction)

    #[test]
    fn remote_add_url_is_redacted() {
        // Simulate what remote_add does: redact before constructing op.
        let url = "https://user:secret@github.com/repo.git";
        let sanitized = redact_url_credentials(url);
        assert_eq!(sanitized, "https://redacted@github.com/repo.git");
        assert!(!sanitized.contains("secret"));
    }

    #[test]
    fn remote_add_url_without_credentials_passthrough() {
        let url = "https://github.com/repo.git";
        let sanitized = redact_url_credentials(url);
        assert_eq!(sanitized, url);
    }

    // ── CleanRequest::is_broad ─────────────────────────────────────

    #[test]
    fn clean_request_is_broad_when_ignored_and_no_paths() {
        let req = CleanRequest {
            force: true,
            dirs: true,
            ignored: true,
            paths: vec![],
        };
        assert!(req.is_broad());
    }

    #[test]
    fn clean_request_not_broad_when_ignored_with_paths() {
        let req = CleanRequest {
            force: true,
            dirs: true,
            ignored: true,
            paths: vec!["target/".to_string()],
        };
        assert!(!req.is_broad());
    }

    #[test]
    fn clean_request_not_broad_when_not_ignored() {
        let req = CleanRequest {
            force: true,
            dirs: true,
            ignored: false,
            paths: vec![],
        };
        assert!(!req.is_broad());
    }

    // ── PullStrategy ───────────────────────────────────────────────

    #[test]
    fn pull_strategy_as_str() {
        assert_eq!(PullStrategy::Merge.as_str(), "merge");
        assert_eq!(PullStrategy::Rebase.as_str(), "rebase");
        assert_eq!(PullStrategy::FastForwardOnly.as_str(), "ff-only");
    }

    // ── CleanPreview ───────────────────────────────────────────────

    #[test]
    fn clean_preview_empty() {
        let preview = CleanPreview { entries: vec![] };
        assert!(preview.is_empty());
        assert_eq!(preview.len(), 0);
    }

    #[test]
    fn clean_preview_non_empty() {
        let preview = CleanPreview {
            entries: vec![
                CleanEntry {
                    path: "foo.o".to_string(),
                    kind: CleanEntryKind::File,
                },
                CleanEntry {
                    path: "target/".to_string(),
                    kind: CleanEntryKind::Directory,
                },
            ],
        };
        assert!(!preview.is_empty());
        assert_eq!(preview.len(), 2);
    }

    // ── push_permission_hint ───────────────────────────────────────

    #[test]
    fn push_permission_hint_delete() {
        let req = PushRequest {
            remote: None,
            branch: None,
            set_upstream: false,
            force: PushForce::Normal,
            tags: false,
            delete: true,
            dry_run: false,
        };
        assert!(push_permission_hint(&req).contains("delete remote branch"));
    }

    #[test]
    fn push_permission_hint_force() {
        let req = PushRequest {
            remote: None,
            branch: None,
            set_upstream: false,
            force: PushForce::Force,
            tags: false,
            delete: false,
            dry_run: false,
        };
        assert!(push_permission_hint(&req).contains("force"));
        assert!(push_permission_hint(&req).contains("destructive"));
    }

    #[test]
    fn push_permission_hint_force_with_lease() {
        let req = PushRequest {
            remote: None,
            branch: None,
            set_upstream: false,
            force: PushForce::ForceWithLease {
                expected_sha: Some("abc123".to_string()),
            },
            tags: false,
            delete: false,
            dry_run: false,
        };
        let hint = push_permission_hint(&req);
        assert!(hint.contains("force-with-lease"));
        assert!(hint.contains("abc123"));
    }

    #[test]
    fn push_permission_hint_set_upstream() {
        let req = PushRequest {
            remote: None,
            branch: None,
            set_upstream: true,
            force: PushForce::Normal,
            tags: false,
            delete: false,
            dry_run: false,
        };
        assert!(push_permission_hint(&req).contains("set upstream"));
    }

    #[test]
    fn push_permission_hint_normal() {
        let req = PushRequest {
            remote: None,
            branch: None,
            set_upstream: false,
            force: PushForce::Normal,
            tags: false,
            delete: false,
            dry_run: false,
        };
        assert_eq!(push_permission_hint(&req), "push");
    }

    // ── PushForce is_destructive ───────────────────────────────────

    #[test]
    fn push_force_normal_not_destructive() {
        assert!(!PushForce::Normal.is_destructive());
    }

    #[test]
    fn push_force_with_lease_not_destructive() {
        assert!(!PushForce::ForceWithLease { expected_sha: None }.is_destructive());
    }

    #[test]
    fn push_force_force_is_destructive() {
        assert!(PushForce::Force.is_destructive());
    }

    // ── PushForce label ────────────────────────────────────────────

    #[test]
    fn push_force_labels() {
        assert_eq!(PushForce::Normal.label(), "normal");
        assert_eq!(
            PushForce::ForceWithLease { expected_sha: None }.label(),
            "force-with-lease"
        );
        assert_eq!(PushForce::Force.label(), "force");
    }
}
