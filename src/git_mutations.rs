//! Typed Git mutation operations with state-delta semantics.
//!
//! This module extends `git_service` with high-level mutation operations
//! that share a single execution model:
//!
//! 1. Resolve and policy-check the repository root.
//! 2. Capture a pre-operation snapshot (HEAD, branch, index, worktree state).
//! 3. Validate operation-specific preconditions.
//! 4. Render argv without a shell via `codegg_git::render_argv`.
//! 5. Execute with timeout and noninteractive controls.
//! 6. Capture raw stdout/stderr and exit status.
//! 7. Capture a post-operation snapshot even on nonzero exit where safe.
//! 8. Classify the result and return a typed state delta.
//!
//! These operations are the canonical entry points for native-tool
//! mutations. They do not own message generation or shell fallback; those
//! concerns belong to the tools and the routing layer respectively.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use codegg_git::path::{PathError, RepoPath, RepoRoot};
use codegg_git::ref_name::RefError;
use codegg_git::{render_argv, GitOperation, GitRiskClass};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::git_service::{GitExecutionService, GitServiceError, RawGitOutput};

// ── Process environment policy ───────────────────────────────────────

/// Environment variables that are always restored for noninteractive
/// local git operations. PATH is restored from the parent; the rest
/// pin git to a deterministic state so local operations cannot hang
/// waiting for a credential prompt, editor, or signing pinentry.
pub const ALLOWED_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "LANG",
    "LC_ALL",
    "LC_MESSAGES",
    "TZ",
    "TMPDIR",
    "USER",
    "LOGNAME",
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "LANGUAGE",
];

/// Process-environment policy applied to every mutation subprocess.
#[derive(Debug, Clone)]
pub struct GitEnvPolicy {
    /// Hard-pin `GIT_TERMINAL_PROMPT=0` so credential helpers never block.
    pub terminal_prompt_disabled: bool,
    /// When true, pin `GIT_EDITOR=true` and `GIT_SEQUENCE_EDITOR=true` to
    /// prevent git from launching a user `$EDITOR`.
    pub pin_editor: bool,
    /// When true, strip `EDITOR`/`VISUAL` from the parent environment
    /// before launching git.
    pub strip_editors: bool,
}

impl Default for GitEnvPolicy {
    fn default() -> Self {
        Self {
            terminal_prompt_disabled: true,
            pin_editor: true,
            strip_editors: true,
        }
    }
}

impl GitEnvPolicy {
    /// Build a `Command` from argv and repository root with the policy
    /// applied. The caller receives the `Command` with `args` and
    /// `current_dir` already set; the helper is the single source of
    /// truth for env hardening.
    pub fn apply(&self, argv: &[String], cwd: &Path) -> Command {
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).current_dir(cwd);
        cmd.env_clear();

        for key in ALLOWED_ENV_VARS {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }

        if self.terminal_prompt_disabled {
            cmd.env("GIT_TERMINAL_PROMPT", "0");
        }
        if self.pin_editor {
            cmd.env("GIT_EDITOR", "true");
            cmd.env("GIT_SEQUENCE_EDITOR", "true");
        }
        if self.strip_editors {
            cmd.env_remove("EDITOR");
            cmd.env_remove("VISUAL");
        }
        cmd.env("GPG_TTY", "");
        cmd.kill_on_drop(true);
        cmd
    }
}

// ── Snapshots ────────────────────────────────────────────────────────

/// A snapshot of repository state captured before or after a mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSnapshot {
    /// HEAD commit hash (or branch name when not detached; the
    /// porcelain v2 header sets this consistently).
    pub head: String,
    /// Current branch name (or detached hash if HEAD is detached).
    pub branch: String,
    /// Whether HEAD is detached.
    pub detached: bool,
    /// Number of staged entries (from porcelain v2 line 1).
    pub staged_count: usize,
    /// Number of unstaged entries (from porcelain v2 line 2).
    pub unstaged_count: usize,
    /// Number of untracked entries.
    pub untracked_count: usize,
    /// Number of conflicted entries.
    pub conflicted_count: usize,
    /// Wall-clock time of capture.
    pub captured_at: DateTime<Utc>,
    /// Optional full output of `git status --porcelain=v2 -z --branch`
    /// (populated for projection/conflict extraction).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_status: Option<String>,
}

impl RepoSnapshot {
    /// Capture a snapshot by running `git status --porcelain=v2 -z --branch`.
    pub async fn capture(repo_root: &Path) -> Result<Self, GitMutationError> {
        capture_snapshot(repo_root).await
    }
}

/// Typed state delta returned by every mutation operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDelta {
    /// Pre-operation snapshot.
    pub before: RepoSnapshot,
    /// Post-operation snapshot.
    pub after: RepoSnapshot,
    /// Commits created (commit, cherry-pick, revert).
    #[serde(default)]
    pub commits_created: Vec<String>,
    /// Refs created (branch, tag, etc.).
    #[serde(default)]
    pub refs_created: Vec<String>,
    /// Refs deleted (branch/tag delete).
    #[serde(default)]
    pub refs_deleted: Vec<String>,
    /// Paths added to the index (stage operations).
    #[serde(default)]
    pub paths_staged: Vec<String>,
    /// Paths removed from the index (restore --staged, reset).
    #[serde(default)]
    pub paths_unstaged: Vec<String>,
    /// Files with detected conflicts (merge/rebase/cherry-pick/revert
    /// left the index in a conflict state).
    #[serde(default)]
    pub conflicts: Vec<String>,
}

impl StateDelta {
    /// True if the operation actually changed the repository state.
    pub fn is_noop(&self) -> bool {
        self.commits_created.is_empty()
            && self.refs_created.is_empty()
            && self.refs_deleted.is_empty()
            && self.paths_staged.is_empty()
            && self.paths_unstaged.is_empty()
            && self.conflicts.is_empty()
            && self.before.head == self.after.head
            && self.before.branch == self.after.branch
            && self.before.staged_count == self.after.staged_count
            && self.before.unstaged_count == self.after.unstaged_count
    }
}

// ── Mutation result ─────────────────────────────────────────────────

/// High-level outcome of a mutation operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationOutcome {
    /// Operation completed successfully and changed repository state.
    Completed,
    /// Operation completed but produced no state change.
    NoOp,
    /// Operation completed and fast-forwarded HEAD.
    FastForward { from: String, to: String },
    /// Operation produced conflicts. The repository is in a recoverable
    /// Git-native state (MERGE_HEAD, REVERT_HEAD, etc. are set).
    Conflict,
    /// Operation was rejected by preconditions or git itself.
    Rejected { reason: String },
}

impl MutationOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NoOp => "no-op",
            Self::FastForward { .. } => "fast-forward",
            Self::Conflict => "conflict",
            Self::Rejected { .. } => "rejected",
        }
    }
}

/// The result of a single mutation operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationResult {
    /// The original typed operation that was executed.
    pub operation: GitOperation,
    /// Subcommand name for display.
    pub subcommand: String,
    /// State delta (before/after + paths/refs/commits touched).
    pub delta: StateDelta,
    /// High-level outcome classification.
    pub outcome: MutationOutcome,
    /// Raw stdout from the git subprocess (truncated to 64 KiB).
    pub stdout: String,
    /// Raw stderr from the git subprocess (truncated to 64 KiB).
    pub stderr: String,
    /// Exit code from the subprocess.
    pub exit_code: i32,
    /// Whether the operation completed successfully (exit 0).
    pub success: bool,
    /// Wall-clock duration of the subprocess.
    pub duration_ms: u64,
}

// ── Errors ───────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum GitMutationError {
    #[error("git mutation failed: {0}")]
    Execution(String),
    #[error("repository error: {0}")]
    Repository(String),
    #[error("precondition violated: {0}")]
    Precondition(String),
    #[error("path validation failed: {0}")]
    Path(String),
    #[error("ref validation failed: {0}")]
    Ref(String),
    #[error("operation timed out after {0}s")]
    Timeout(u64),
    #[error("state mismatch: expected operation '{expected}' but found '{actual}' on disk")]
    StateMismatch { expected: String, actual: String },
}

impl From<GitServiceError> for GitMutationError {
    fn from(err: GitServiceError) -> Self {
        match err {
            GitServiceError::Execution(s) => Self::Execution(s),
            GitServiceError::Repository(s) => Self::Repository(s),
            GitServiceError::Timeout(s) => {
                let secs = s
                    .split("timed out after")
                    .nth(1)
                    .and_then(|s| s.trim().trim_end_matches('s').parse().ok());
                Self::Timeout(secs.unwrap_or(30))
            }
        }
    }
}

impl From<PathError> for GitMutationError {
    fn from(err: PathError) -> Self {
        Self::Path(err.to_string())
    }
}

impl From<RefError> for GitMutationError {
    fn from(err: RefError) -> Self {
        Self::Ref(err.to_string())
    }
}

// ── Path validation helpers ──────────────────────────────────────────

/// Build a `RepoRoot` from a path. Returns an error if the path is not
/// a directory, if canonicalization fails, or if `.git` is missing.
pub fn resolve_repo_root(path: &Path) -> Result<RepoRoot, GitMutationError> {
    if !path.exists() {
        return Err(GitMutationError::Repository(format!(
            "repository root does not exist: {}",
            path.display()
        )));
    }
    if !path.is_dir() {
        return Err(GitMutationError::Repository(format!(
            "repository root is not a directory: {}",
            path.display()
        )));
    }
    let root = RepoRoot::new(path).map_err(|e| GitMutationError::Repository(e.to_string()))?;
    if !root.as_path().join(".git").exists() {
        return Err(GitMutationError::Repository(format!(
            "not a git repository: {}",
            path.display()
        )));
    }
    Ok(root)
}

/// Build a `RepoPath` for a relative path under `repo_root`.
pub fn validate_repo_path(repo_root: &RepoRoot, path: &str) -> Result<RepoPath, GitMutationError> {
    RepoPath::new(repo_root, path).map_err(Into::into)
}

// ── Internal helpers ────────────────────────────────────────────────

/// Capture a `RepoSnapshot` for the given repository root.
async fn capture_snapshot(repo_root: &Path) -> Result<RepoSnapshot, GitMutationError> {
    let argv = vec![
        "git".to_string(),
        "status".to_string(),
        "--porcelain=v2".to_string(),
        "-z".to_string(),
        "--branch".to_string(),
    ];
    let env = GitEnvPolicy::default();
    let mut cmd = env.apply(&argv, repo_root);
    let output = cmd
        .output()
        .await
        .map_err(|e| GitMutationError::Execution(format!("snapshot spawn failed: {e}")))?;

    if !output.status.success() {
        return Err(GitMutationError::Repository(format!(
            "git status failed (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    parse_porcelain_v2_branch(&raw)
}

/// Parse the porcelain v2 `-z --branch` output into a snapshot.
fn parse_porcelain_v2_branch(raw: &str) -> Result<RepoSnapshot, GitMutationError> {
    let mut head = String::new();
    let mut detached = false;
    let mut staged = 0usize;
    let mut unstaged = 0usize;
    let mut untracked = 0usize;
    let mut conflicted = 0usize;

    for entry in raw.split('\0') {
        if entry.is_empty() {
            continue;
        }
        if let Some(rest) = entry.strip_prefix("# branch.head ") {
            head = rest.to_string();
        } else if let Some(rest) = entry.strip_prefix("# branch.oid ") {
            if !rest.is_empty() && rest != "(initial)" {
                head = rest.to_string();
            }
        } else if entry.starts_with("# branch.head (detached)") {
            detached = true;
        } else if entry.starts_with('#') {
            // Other header lines: ignore.
        } else if let Some(stripped) = entry.strip_prefix("1 ") {
            let xy = stripped.split(' ').next().unwrap_or("");
            update_xy_counts(xy, &mut staged, &mut unstaged, &mut conflicted);
        } else if let Some(stripped) = entry.strip_prefix("2 ") {
            let xy = stripped.split(' ').next().unwrap_or("");
            update_xy_counts(xy, &mut staged, &mut unstaged, &mut conflicted);
        } else if entry.starts_with("u ") {
            conflicted += 1;
        } else if entry.starts_with("? ") {
            untracked += 1;
        }
    }

    let branch = head.clone();
    Ok(RepoSnapshot {
        head,
        branch,
        detached,
        staged_count: staged,
        unstaged_count: unstaged,
        untracked_count: untracked,
        conflicted_count: conflicted,
        captured_at: Utc::now(),
        raw_status: Some(raw.to_string()),
    })
}

fn update_xy_counts(xy: &str, staged: &mut usize, unstaged: &mut usize, conflicted: &mut usize) {
    if xy.len() < 2 {
        return;
    }
    let x = xy.chars().next().unwrap_or(' ');
    let y = xy.chars().nth(1).unwrap_or(' ');
    if x != '.' {
        *staged += 1;
    }
    if y != '.' {
        *unstaged += 1;
    }
    if y == 'U' || x == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
        *conflicted += 1;
    }
}

// ── Mutation executor ───────────────────────────────────────────────

/// Reusable executor for local Git mutations. One executor instance
/// is shared by every typed mutation operation; cloning is cheap.
#[derive(Clone)]
pub struct GitMutationExecutor {
    /// Read service used for snapshots and read-only preconditions.
    pub read_service: GitExecutionService,
    /// Process environment policy.
    pub env_policy: GitEnvPolicy,
    /// Per-operation timeout. Defaults to 30s.
    pub timeout: Duration,
}

// Manual Debug impl because `GitExecutionService` does not derive Debug.
impl std::fmt::Debug for GitMutationExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitMutationExecutor")
            .field("env_policy", &self.env_policy)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl Default for GitMutationExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl GitMutationExecutor {
    pub fn new() -> Self {
        Self {
            read_service: GitExecutionService::new(),
            env_policy: GitEnvPolicy::default(),
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self.read_service = self.read_service.with_timeout(timeout);
        self
    }

    pub fn with_env_policy(mut self, env_policy: GitEnvPolicy) -> Self {
        self.env_policy = env_policy;
        self
    }

    /// Capture a `RepoSnapshot` for the given repository root.
    pub async fn snapshot(&self, repo_root: &Path) -> Result<RepoSnapshot, GitMutationError> {
        capture_snapshot(repo_root).await
    }

    /// Execute a single typed `GitOperation` mutation end-to-end.
    pub async fn execute(
        &self,
        operation: &GitOperation,
        repo_root: &Path,
    ) -> Result<MutationResult, GitMutationError> {
        let before = self.snapshot(repo_root).await?;
        let argv = render_argv(operation);

        if argv.is_empty() {
            return Err(GitMutationError::Execution(
                "empty rendered argv".to_string(),
            ));
        }

        let raw = self.run_subprocess(&argv, repo_root).await?;
        let after = match self.snapshot(repo_root).await {
            Ok(s) => s,
            Err(_) => before.clone(),
        };

        let outcome = classify_outcome(operation, &before, &after, raw.exit_code);
        let delta = compute_delta(operation, &before, &after, &raw, &outcome);

        let stdout = truncate_for_result(&raw.stdout, 64 * 1024);
        let stderr = truncate_for_result(&raw.stderr, 64 * 1024);
        let start = std::time::Instant::now();

        Ok(MutationResult {
            operation: operation.clone(),
            subcommand: operation.subcommand_name().to_string(),
            delta,
            outcome,
            stdout,
            stderr,
            exit_code: raw.exit_code,
            success: raw.exit_code == 0,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Run a git subprocess with policy and timeout. Returns raw output.
    pub(crate) async fn run_subprocess(
        &self,
        argv: &[String],
        repo_root: &Path,
    ) -> Result<RawGitOutput, GitMutationError> {
        if argv.is_empty() {
            return Err(GitMutationError::Execution("empty argv".to_string()));
        }
        let start = std::time::Instant::now();
        let timeout = self.timeout;
        let repo_root_owned = repo_root.to_path_buf();
        let argv_owned = argv.to_vec();
        let env = self.env_policy.clone();

        let output = tokio::time::timeout(timeout, async move {
            let mut cmd = env.apply(&argv_owned, &repo_root_owned);
            cmd.output().await
        })
        .await
        .map_err(|_| GitMutationError::Timeout(timeout.as_secs()))?
        .map_err(|e| GitMutationError::Execution(format!("spawn failed: {e}")))?;

        let raw = RawGitOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        };
        // Note: raw captures the wall-clock but we discard it here; the
        // public MutationResult tracks its own duration.
        let _ = start;
        Ok(raw)
    }
}

/// Truncate a string to `max_bytes` with a clear marker.
fn truncate_for_result(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut out = String::with_capacity(max_bytes + 64);
    out.push_str(&s[..max_bytes]);
    out.push_str(&format!("\n... [truncated, original {} bytes]", s.len()));
    out
}

/// Public alias used by sibling modules (e.g. raw-mutation helpers).
pub(crate) fn truncate_for_public(s: &str, max_bytes: usize) -> String {
    truncate_for_result(s, max_bytes)
}

/// Classify the outcome of a mutation given before/after snapshots.
pub(crate) fn classify_outcome(
    operation: &GitOperation,
    before: &RepoSnapshot,
    after: &RepoSnapshot,
    exit_code: i32,
) -> MutationOutcome {
    // Conflict takes priority over generic non-zero exit: a merge that
    // exited 1 because of unresolved conflicts is in `Conflict` state,
    // not a generic `Rejected`. The state is recoverable.
    if after.conflicted_count > 0 {
        return MutationOutcome::Conflict;
    }

    if exit_code != 0 {
        return MutationOutcome::Rejected {
            reason: format!("git exited with code {exit_code}"),
        };
    }

    let is_history_integration = operation
        .risk_classes()
        .contains(&GitRiskClass::HistoryIntegration);
    if is_history_integration && before.head != after.head && before.branch == after.branch {
        return MutationOutcome::FastForward {
            from: before.head.clone(),
            to: after.head.clone(),
        };
    }

    if before == after {
        return MutationOutcome::NoOp;
    }

    MutationOutcome::Completed
}

/// Compute the state delta from before/after snapshots and the operation.
pub(crate) fn compute_delta(
    operation: &GitOperation,
    before: &RepoSnapshot,
    after: &RepoSnapshot,
    raw: &RawGitOutput,
    outcome: &MutationOutcome,
) -> StateDelta {
    let mut delta = StateDelta {
        before: before.clone(),
        after: after.clone(),
        commits_created: Vec::new(),
        refs_created: Vec::new(),
        refs_deleted: Vec::new(),
        paths_staged: Vec::new(),
        paths_unstaged: Vec::new(),
        conflicts: Vec::new(),
    };

    if matches!(
        operation,
        GitOperation::Commit { .. } | GitOperation::CherryPick { .. } | GitOperation::Revert { .. }
    ) {
        for token in raw.stdout.split_whitespace() {
            if is_hex_sha(token) && token.len() >= 7 {
                delta.commits_created.push(token.to_string());
            }
        }
    }

    if matches!(
        operation,
        GitOperation::BranchCreate { .. }
            | GitOperation::TagCreate { .. }
            | GitOperation::Switch { create: true, .. }
            | GitOperation::Checkout { create: true, .. }
    ) {
        for token in raw.stdout.split_whitespace() {
            let cleaned: String = token
                .chars()
                .filter(|c| !matches!(c, ':' | ',' | '.' | '(' | ')'))
                .collect();
            if !cleaned.is_empty()
                && !cleaned.contains('/')
                && cleaned
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                delta.refs_created.push(cleaned);
            }
        }
    }

    if matches!(
        operation,
        GitOperation::BranchDelete { .. }
            | GitOperation::TagDelete { .. }
            | GitOperation::TagForceDelete { .. }
    ) {
        if let Some(name) = operation_ref_name(operation) {
            delta.refs_deleted.push(name.to_string());
        }
    }

    if matches!(operation, GitOperation::Add { .. }) {
        if let Some(paths) = operation_paths(operation) {
            delta.paths_staged = paths;
        }
    }
    if matches!(
        operation,
        GitOperation::Restore { staged: true, .. } | GitOperation::Reset { .. }
    ) {
        if let Some(paths) = operation_paths(operation) {
            delta.paths_unstaged = paths;
        }
    }

    if matches!(outcome, MutationOutcome::Conflict) {
        delta.conflicts = after
            .raw_status
            .as_deref()
            .map(extract_conflict_paths)
            .unwrap_or_default();
    }

    delta
}

/// Heuristic: extract conflict paths from porcelain v2 output.
fn extract_conflict_paths(raw: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for entry in raw.split('\0') {
        if let Some(rest) = entry.strip_prefix("u ") {
            let path = rest.split_whitespace().last().unwrap_or("").to_string();
            if !path.is_empty() {
                paths.push(path);
            }
        } else if entry.starts_with("AA ")
            || entry.starts_with("DD ")
            || entry.starts_with("AU ")
            || entry.starts_with("UA ")
            || entry.starts_with("DU ")
            || entry.starts_with("UD ")
            || entry.starts_with("UU ")
        {
            let path = entry
                .split_once(' ')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or_default();
            if !path.is_empty() {
                paths.push(path);
            }
        }
    }
    paths
}

/// Extract the literal path list from a `GitOperation`, when it carries one.
fn operation_paths(operation: &GitOperation) -> Option<Vec<String>> {
    match operation {
        GitOperation::Add { paths } => Some(paths.iter().map(|p| p.as_str().to_string()).collect()),
        GitOperation::Restore { paths, .. } => {
            Some(paths.iter().map(|p| p.as_str().to_string()).collect())
        }
        GitOperation::Reset { paths, .. } => paths
            .clone()
            .map(|ps| ps.iter().map(|p| p.as_str().to_string()).collect()),
        _ => None,
    }
}

/// Extract the literal ref name from a `GitOperation` that targets one.
fn operation_ref_name(operation: &GitOperation) -> Option<&str> {
    match operation {
        GitOperation::BranchDelete { name, .. } => Some(name.as_str()),
        GitOperation::TagDelete { name } | GitOperation::TagForceDelete { name } => Some(name),
        _ => None,
    }
}

/// Heuristic: is this token a hex sha (any length 7-64)?
fn is_hex_sha(s: &str) -> bool {
    s.len() >= 7
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

// ── Commit selection (Phase D) ──────────────────────────────────────

/// Explicit selection of what to commit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitSelection {
    /// Use whatever is currently in the index.
    #[default]
    AlreadyStaged,
    /// Stage these literal paths before committing.
    StagePaths(Vec<String>),
    /// Stage every change (tracked + untracked) before committing.
    StageAll,
}
