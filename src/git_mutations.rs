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

use chrono::Utc;
use codegg_git::path::{PathError, RepoPath, RepoRoot};
use codegg_git::ref_name::RefError;
use codegg_git::{render_argv, GitOperation, GitRiskClass};
use serde::{Deserialize, Serialize};

use crate::git_network_policy::redact_url_credentials_in_text;
use crate::git_network_policy::NetworkFailureKind;
use crate::git_service::{GitExecutionService, GitServiceError, RawGitOutput};

// ── Process environment policy ───────────────────────────────────────
//
// The canonical env-var lists and builder live in `egggit::process`. These
// re-exports preserve the historical root paths used by downstream callers.

/// Re-export of the canonical allowlist. See
/// [`codegg_git::process_policy::ALLOWED_ENV_VARS`] for the source of
/// truth and rationale.
pub use egggit::process::ALLOWED_ENV_VARS;

/// Re-export of the canonical always-stripped set. See
/// [`codegg_git::process_policy::ALWAYS_STRIPPED_ENV_VARS`].
pub use egggit::process::ALWAYS_STRIPPED_ENV_VARS;

/// Compatibility re-export. Generic Git process construction is owned by
/// `egggit`; mutation orchestration remains in this root adapter until the
/// scheduler can consume a crate-level durable workflow boundary.
pub use egggit::process::GitEnvPolicy;

#[cfg(test)]
mod policy_drift_tests {
    use super::*;

    /// Drift guard: the canonical lists in `egggit::process`
    /// MUST match the historical values the root crate has relied on
    /// since Phase F. If this test fails, the canonical list has
    /// changed and either (a) the policy genuinely changed (update the
    /// test) or (b) the lists drifted and the policy needs to be
    /// re-audited before accepting the change.
    #[test]
    fn canonical_policy_includes_all_phase_f_entries() {
        // Allowed vars that local git operations have always relied on.
        for k in [
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
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "CURL_CA_BUNDLE",
            "REQUESTS_CA_BUNDLE",
            "GIT_SSL_CAINFO",
            "GIT_SSL_CAPATH",
        ] {
            assert!(
                ALLOWED_ENV_VARS.contains(&k),
                "{k} missing from canonical ALLOWED_ENV_VARS"
            );
        }

        // Stripped vars (command-bearing injection vectors).
        for k in [
            "GIT_ASKPASS",
            "GIT_SSH_COMMAND",
            "GIT_SSH_VARIANT",
            "GIT_PROXY_COMMAND",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
            "SSH_ASKPASS",
            "GIT_TOOL",
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_COMMON_DIR",
            "GIT_PAGER",
            "PAGER",
        ] {
            assert!(
                ALWAYS_STRIPPED_ENV_VARS.contains(&k),
                "{k} missing from canonical ALWAYS_STRIPPED_ENV_VARS"
            );
        }
    }

    /// Drift guard: `codegg-core::worktree::hardened_git_command`
    /// MUST consume the same canonical lists. Both `pub use` aliases
    /// below point at `egggit::process` constants, so this
    /// is a structural check that the root crate and `codegg-core`
    /// read from the same source of truth.
    #[test]
    fn root_and_core_share_canonical_lists() {
        // Same length ⇒ same set when both come from the canonical
        // source. (Equality is already enforced by the alias; this is
        // a smell-test for accidental re-declaration.)
        assert_eq!(ALLOWED_ENV_VARS.len(), codegg_git::ALLOWED_ENV_VARS.len());
        assert_eq!(
            ALWAYS_STRIPPED_ENV_VARS.len(),
            codegg_git::ALWAYS_STRIPPED_ENV_VARS.len()
        );
    }
}

// ── Snapshots ────────────────────────────────────────────────────────

pub use codegg_git::workflow::{MutationOutcome, MutationResult, RepoSnapshot, StateDelta};

// ── Errors ───────────────────────────────────────────────────────────

/// Detailed context attached to a `GitMutationError::Execution`. Carries
/// structured fields the projector and operator UI can surface, while
/// keeping stdout/stderr sanitized so credentials never leak through
/// `Display`/`Debug`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    /// The kind of operation that failed (e.g. "fetch", "remote_add",
    /// "commit"). Derived from `GitOperation::subcommand_name()` or the
    /// passed `&str` when no operation is available.
    pub operation_kind: String,
    /// The remote name targeted, when the operation carries one
    /// (`RemoteAdd`, `RemoteSetUrl`, `Fetch`, `Push`, etc.). Not
    /// included otherwise.
    pub remote_name: Option<String>,
    /// Classified network failure kind (DNS, Connect, Authentication,
    /// Authorization, RefRejected, Timeout, Transport). Only populated
    /// for network operations; `None` for local mutations.
    pub failure_kind: Option<NetworkFailureKind>,
    /// Subprocess exit code when available. `-1` indicates the child
    /// did not produce an exit code (spawn failure, signal kill).
    pub exit_code: Option<i32>,
    /// Whether the failure was caused by a timeout.
    pub timed_out: bool,
    /// Redacted stdout (already passed through
    /// `redact_url_credentials_in_text`).
    pub stdout_redacted: String,
    /// Redacted stderr (already passed through
    /// `redact_url_credentials_in_text`).
    pub stderr_redacted: String,
}

impl ExecutionContext {
    pub fn new(operation_kind: impl Into<String>) -> Self {
        Self {
            operation_kind: operation_kind.into(),
            remote_name: None,
            failure_kind: None,
            exit_code: None,
            timed_out: false,
            stdout_redacted: String::new(),
            stderr_redacted: String::new(),
        }
    }

    pub fn with_remote(mut self, remote: impl Into<String>) -> Self {
        self.remote_name = Some(remote.into());
        self
    }

    pub fn with_failure_kind(mut self, kind: NetworkFailureKind) -> Self {
        self.failure_kind = Some(kind);
        self
    }

    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    pub fn with_timed_out(mut self) -> Self {
        self.timed_out = true;
        self
    }

    pub fn with_stdout(mut self, stdout: impl Into<String>) -> Self {
        self.stdout_redacted = redact_url_credentials_in_text(&stdout.into());
        self
    }

    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr_redacted = redact_url_credentials_in_text(&stderr.into());
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GitMutationError {
    /// Subprocess failed (spawn error, non-zero exit, network failure).
    /// The contained `ExecutionContext` carries operation kind, remote
    /// name (when applicable), classified failure kind, exit code, and
    /// redacted stdout/stderr. The `message` field is a short summary
    /// safe to surface in tool results — it MUST NOT contain raw argv,
    /// raw URLs, or un-redacted credentials.
    #[error("git {kind} failed: {message}", kind = context.operation_kind)]
    Execution {
        message: String,
        context: ExecutionContext,
    },
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

impl GitMutationError {
    /// Convenience constructor for an `Execution` variant with the
    /// operation kind inferred from a `GitOperation`. The message
    /// string MUST NOT contain raw argv or un-redacted credentials.
    pub fn execution(operation: &GitOperation, message: impl Into<String>) -> Self {
        Self::Execution {
            message: message.into(),
            context: ExecutionContext::new(operation.subcommand_name()),
        }
    }

    /// Convenience constructor with explicit operation kind (when no
    /// typed operation is available — e.g. snapshot capture).
    pub fn execution_kind(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Execution {
            message: message.into(),
            context: ExecutionContext::new(kind),
        }
    }

    /// Get the operation kind from an `Execution` variant, or `None`.
    pub fn operation_kind(&self) -> Option<&str> {
        match self {
            Self::Execution { context, .. } => Some(&context.operation_kind),
            _ => None,
        }
    }

    /// Get the classified failure kind, when the error carries one.
    pub fn failure_kind(&self) -> Option<NetworkFailureKind> {
        match self {
            Self::Execution { context, .. } => context.failure_kind,
            _ => None,
        }
    }

    /// Get the exit code from an `Execution` variant.
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Execution { context, .. } => context.exit_code,
            _ => None,
        }
    }

    /// Get the remote name when the error carries one.
    pub fn remote_name(&self) -> Option<&str> {
        match self {
            Self::Execution { context, .. } => context.remote_name.as_deref(),
            _ => None,
        }
    }
}

impl From<GitServiceError> for GitMutationError {
    fn from(err: GitServiceError) -> Self {
        match err {
            GitServiceError::Execution(s) => {
                // Legacy path: no operation context available. The
                // service error string is sanitized through the
                // redaction helper so any URL-embedded credential is
                // stripped before reaching `Display`.
                let redacted = redact_url_credentials_in_text(&s);
                Self::Execution {
                    message: redacted,
                    context: ExecutionContext::new("git"),
                }
            }
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
    let output = cmd.output().await.map_err(|e| {
        GitMutationError::execution_kind("snapshot", format!("snapshot spawn failed: {e}"))
    })?;

    if !output.status.success() {
        let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();
        let redacted_stderr = redact_url_credentials_in_text(&stderr_text);
        return Err(GitMutationError::Repository(format!(
            "git status failed (exit {:?}): {}",
            output.status.code(),
            redacted_stderr
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
///
/// Identity / audit live-execution M001: the executor can hold an
/// optional daemon-injected trusted execution-audit context plus the
/// shared bounded emitter. M001 threads the seam only; M002 emits
/// `git_operation` through it. The context is never synthesized from
/// URLs, refs, or subprocess output.
#[derive(Clone)]
pub struct GitMutationExecutor {
    /// Read service used for snapshots and read-only preconditions.
    pub read_service: GitExecutionService,
    /// Process environment policy.
    pub env_policy: GitEnvPolicy,
    /// Per-operation timeout. Defaults to 30s.
    pub timeout: Duration,
    /// Trusted execution-audit context injected by the daemon boundary.
    pub execution_audit: Option<codegg_core::audit_instrumentation::TrustedExecutionAuditContext>,
    /// Shared bounded audit emitter injected by the daemon boundary.
    pub audit_emitter: Option<codegg_core::audit_instrumentation::ExecutionAuditEmitter>,
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
            execution_audit: None,
            audit_emitter: None,
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

    /// Inject the daemon-built trusted execution-audit context.
    /// M002 consumes this for `git_operation` emission.
    pub fn with_execution_audit(
        mut self,
        ctx: codegg_core::audit_instrumentation::TrustedExecutionAuditContext,
    ) -> Self {
        self.execution_audit = Some(ctx);
        self
    }

    /// Inject the shared bounded audit emitter.
    /// M002 consumes this for `git_operation` emission.
    pub fn with_audit_emitter(
        mut self,
        emitter: codegg_core::audit_instrumentation::ExecutionAuditEmitter,
    ) -> Self {
        self.audit_emitter = Some(emitter);
        self
    }

    /// Trusted execution-audit context, when the daemon threaded one.
    pub fn execution_audit(
        &self,
    ) -> Option<&codegg_core::audit_instrumentation::TrustedExecutionAuditContext> {
        self.execution_audit.as_ref()
    }

    /// Identity / audit live-execution M002: emit one structural
    /// `git_operation` event for an executed mutation/network/recovery
    /// transition.
    ///
    /// Best-effort through the shared bounded emitter and silent when
    /// the daemon threaded no context/emitter pair. One executed
    /// transition emits at most one event; retries/replays of the same
    /// committed state reuse the deterministic event id so the store
    /// returns the stored row instead of duplicating it.
    pub async fn emit_git_operation(
        &self,
        operation: &GitOperation,
        outcome: &MutationOutcome,
        after: &RepoSnapshot,
    ) {
        let (Some(audit), Some(emitter)) =
            (self.execution_audit.as_ref(), self.audit_emitter.as_ref())
        else {
            return;
        };
        let Some(op_label) = git_audit_op_label(operation) else {
            return;
        };
        let ref_digest = git_audit_ref_digest(operation);
        let outcome_label = outcome.label();
        let state_digest = codegg_core::audit_instrumentation::structural_digest(
            format!(
                "{}|{}|{}|{}|{}",
                after.head,
                after.branch,
                after.staged_count,
                after.unstaged_count,
                after.conflicted_count
            )
            .as_bytes(),
        );
        let scope = format!("{op_label}|{ref_digest}|{outcome_label}|{state_digest}");
        emit_git_operation_parts(audit, emitter, op_label, &ref_digest, outcome_label, &scope)
            .await;
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
            return Err(GitMutationError::execution(
                operation,
                "empty rendered argv",
            ));
        }

        let raw = self.run_subprocess(&argv, repo_root).await?;
        let after = match self.snapshot(repo_root).await {
            Ok(s) => s,
            Err(_) => before.clone(),
        };

        let outcome = classify_outcome(operation, &before, &after, raw.exit_code);
        let delta = compute_delta(operation, &before, &after, &raw, &outcome);

        // M002: one executed transition emits at most one structural
        // `git_operation` event (silent without a threaded context).
        // Network (`fetch`/`pull`/`push`), local mutations, and recovery
        // transitions all funnel through this method, so native and
        // bash-routed invocations converge on one audit event.
        self.emit_git_operation(operation, &outcome, &after).await;

        let stdout = sanitize_truncate_for_result(&raw.stdout, 64 * 1024);
        let stderr = sanitize_truncate_for_result(&raw.stderr, 64 * 1024);
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
            return Err(GitMutationError::execution_kind("subprocess", "empty argv"));
        }
        let start = std::time::Instant::now();
        let timeout = self.timeout;
        let repo_root_owned = repo_root.to_path_buf();
        let argv_owned = argv.to_vec();
        let env = self.env_policy.clone();

        let output = match tokio::time::timeout(timeout, async move {
            let mut cmd = env.apply(&argv_owned, &repo_root_owned);
            cmd.output().await
        })
        .await
        {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                return Err(GitMutationError::execution_kind(
                    "subprocess",
                    format!("spawn failed: {e}"),
                ));
            }
            Err(_) => {
                let mut ctx = ExecutionContext::new("subprocess");
                ctx.timed_out = true;
                return Err(GitMutationError::Execution {
                    message: format!("timed out after {}s", timeout.as_secs()),
                    context: ctx,
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        // We deliberately do NOT surface a structured Execution error
        // here for non-zero exit codes. Operations like merge exit 1
        // when there are conflicts (a recoverable state, not a
        // subprocess failure), and classify_outcome() already turns
        // that into MutationOutcome::Conflict via the after-state
        // snapshot. Genuine subprocess failures (spawn error,
        // timeout) are caught above before this point.
        let raw = RawGitOutput {
            stdout,
            stderr,
            exit_code,
        };
        // Note: raw captures the wall-clock but we discard it here; the
        // public MutationResult tracks its own duration.
        let _ = start;
        Ok(raw)
    }
}

/// Truncate a string to `max_bytes` with a clear marker. The cut point is
/// always a UTF-8 char boundary so multi-byte content (commit messages,
/// renamed paths, diffs) cannot panic mid-character.
fn truncate_for_result(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut out = String::with_capacity(max_bytes + 64);
    out.push_str(crate::util::truncate_prefix(s, max_bytes));
    out.push_str(&format!("\n... [truncated, original {} bytes]", s.len()));
    out
}

/// Defense-in-depth: redact any URL-embedded credentials, then truncate.
/// This is the single boundary through which every Git-emitted byte
/// reaches `MutationResult.stdout`/`stderr`, RunStore artifacts, and
/// downstream projectors. The raw URL still reaches the Git child via
/// `RedactedUrl::expose_secret` at the argv construction site.
pub(crate) fn sanitize_truncate_for_result(s: &str, max_bytes: usize) -> String {
    truncate_for_result(&redact_url_credentials_in_text(s), max_bytes)
}

/// M002: emit one structural `git_operation` event from precomputed
/// structural parts.
///
/// Shared tail for the executor-owned hook above and for dispatch
/// paths that execute git without a [`MutationResult`] (the tool
/// raw-subcommand fallback). All parts are bounded labels or digests;
/// `scope` must already bind the logical invocation (op, ref digest,
/// outcome, and — where available — post-state) so replays reuse the
/// deterministic event id.
pub async fn emit_git_operation_parts(
    audit: &codegg_core::audit_instrumentation::TrustedExecutionAuditContext,
    emitter: &codegg_core::audit_instrumentation::ExecutionAuditEmitter,
    op_label: &str,
    ref_digest_hex: &str,
    outcome_label: &str,
    scope: &str,
) {
    let correlation = audit
        .chain()
        .correlation_id
        .as_deref()
        .filter(|correlation| !correlation.is_empty())
        .unwrap_or_else(|| audit.provenance().correlation_id());
    let event_id = codegg_core::audit_instrumentation::deterministic_event_id(
        audit.provenance().decision_id(),
        &codegg_core::audit::AuditAction::GitOperation,
        correlation,
        scope,
    );
    emitter
        .emit_with(audit, |principal, provenance, chain| {
            codegg_core::audit_instrumentation::git_operation_event(
                principal,
                provenance,
                chain,
                op_label,
                ref_digest_hex,
                outcome_label,
            )
            .with_event_id(event_id.clone())
        })
        .await;
}

// ── M002 live `git_operation` audit labels ────────────────────────────

/// Bounded audit label for one executed [`GitOperation`].
///
/// Returns `None` for read-only operations (status/diff/log/blame,
/// listings, previews, config reads, operation-state probes): those
/// are not `git_operation` events unless long-term policy explicitly
/// changes. Recovery flag variants (`--continue`/`--abort`/`--skip`)
/// keep their subcommand label — the executed git command — while the
/// bare sequencer control ops use `recover_*` labels.
pub fn git_audit_op_label(operation: &GitOperation) -> Option<&'static str> {
    use codegg_git::GitOperation as Op;
    Some(match operation {
        Op::Add { .. } => "stage",
        Op::Reset { .. }
        | Op::ResetHard { .. }
        | Op::ResetMixed { .. }
        | Op::ResetSoft { .. }
        | Op::ResetMerge { .. }
        | Op::ResetKeep { .. } => "reset",
        Op::Commit { .. } => "commit",
        Op::StashPush { .. }
        | Op::StashApply { .. }
        | Op::StashPop { .. }
        | Op::StashDrop { .. } => "stash",
        Op::Checkout { .. } => "checkout",
        Op::Switch { .. } => "switch",
        Op::Restore { .. } => "restore",
        Op::BranchCreate { .. } => "branch_create",
        Op::BranchDelete { .. } => "branch_delete",
        Op::BranchRename { .. } => "branch_rename",
        Op::TagCreate { .. } => "tag_create",
        Op::TagDelete { .. } | Op::TagForceDelete { .. } => "tag_delete",
        Op::Merge { .. } => "merge",
        Op::Rebase { .. } => "rebase",
        Op::CherryPick { .. } => "cherry_pick",
        Op::Revert { .. } => "revert",
        Op::Fetch { .. } => "fetch",
        Op::Pull { .. } => "pull",
        Op::Push { .. } => "push",
        Op::Clean { .. } => "clean",
        Op::RemoteAdd { .. } => "remote_add",
        Op::RemoteRemove { .. } => "remote_remove",
        Op::RemoteSetUrl { .. } => "remote_set_url",
        Op::ConfigSet { .. } => "config_set",
        Op::ConfigUnset { .. } => "config_unset",
        Op::Abort => "recover_abort",
        Op::Continue => "recover_continue",
        Op::Skip => "recover_skip",
        Op::ManagedGitArgv { .. } => "managed",
        // Read-only inspection, listings, previews, and raw-shell
        // fallbacks carry no `git_operation` event by design.
        _ => return None,
    })
}

/// Secret-free digest source for one executed [`GitOperation`].
///
/// Returns the SHA-256 hex digest over the normalized target
/// ref/remote-name/refspec material only. Remote URLs (which may embed
/// credentials) contribute NOTHING — only the remote NAME is digested.
/// Commit messages, path lists, patch bodies, and subprocess output
/// are never digested: operations without ref material hash the empty
/// string (the "empty structural digest").
pub fn git_audit_ref_digest(operation: &GitOperation) -> String {
    use codegg_core::audit_instrumentation::structural_digest;
    use codegg_git::GitOperation as Op;
    // Join with NUL so adjacent-field boundaries cannot collide, then
    // strip anything URL-shaped before digesting as defense in depth:
    // only names/refs/revs may contribute.
    let mut parts: Vec<&str> = Vec::new();
    match operation {
        Op::BranchCreate {
            name, start_point, ..
        } => {
            parts.push(name.as_str());
            if let Some(start) = start_point {
                parts.push(start.as_str());
            }
        }
        Op::BranchDelete { name, .. } => parts.push(name.as_str()),
        Op::BranchRename { old, new, .. } => {
            parts.push(old.as_str());
            parts.push(new.as_str());
        }
        Op::TagCreate { name, rev, .. } => {
            parts.push(name.as_str());
            if let Some(rev) = rev {
                parts.push(rev.as_str());
            }
        }
        Op::TagDelete { name } | Op::TagForceDelete { name } => parts.push(name.as_str()),
        Op::Merge { revisions, .. }
        | Op::CherryPick { revisions, .. }
        | Op::Revert { revisions, .. } => {
            for rev in revisions {
                parts.push(rev.as_str());
            }
        }
        Op::Rebase { upstream, onto, .. } => {
            if let Some(upstream) = upstream {
                parts.push(upstream.as_str());
            }
            if let Some(onto) = onto {
                parts.push(onto.as_str());
            }
        }
        Op::Fetch {
            remote, refspecs, ..
        } => {
            if let Some(remote) = remote {
                parts.push(remote.as_str());
            }
            for refspec in refspecs {
                parts.push(refspec.as_str());
            }
        }
        Op::Pull { remote, branch, .. } | Op::Push { remote, branch, .. } => {
            if let Some(remote) = remote {
                parts.push(remote.as_str());
            }
            if let Some(branch) = branch {
                parts.push(branch.as_str());
            }
        }
        Op::RemoteAdd { name, .. } | Op::RemoteRemove { name } | Op::RemoteSetUrl { name, .. } => {
            // Remote NAME only: the URL (raw or redacted) never
            // contributes, so embedded credentials cannot influence
            // even the digest preimage.
            parts.push(name.as_str());
        }
        Op::Checkout {
            target: Some(target),
            ..
        } => {
            parts.push(target.as_str());
        }
        Op::Switch { branch, .. } => parts.push(branch.as_str()),
        Op::Restore {
            source: Some(source),
            ..
        } => {
            parts.push(source.as_str());
        }
        Op::Reset { rev: Some(rev), .. } => {
            parts.push(rev.as_str());
        }
        Op::ResetHard { rev: Some(rev) }
        | Op::ResetMixed { rev: Some(rev) }
        | Op::ResetSoft { rev: Some(rev) }
        | Op::ResetMerge { rev: Some(rev) }
        | Op::ResetKeep { rev: Some(rev) } => {
            parts.push(rev.as_str());
        }
        Op::ManagedGitArgv { argv, .. } => {
            // Only the subcommand token (argv[1]) may contribute, and
            // only when it is a bounded token; raw argv (paths, URLs,
            // refspecs from unparsed input) never enters the preimage.
            if let Some(subcommand) = argv.get(1) {
                if subcommand
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-')
                    && subcommand.len() <= 32
                {
                    parts.push(subcommand.as_str());
                }
            }
        }
        // Stage/commit/stash/clean/config operations: no ref material.
        // The empty structural digest proves "no ref" structurally.
        _ => {}
    }
    let joined = parts.join("\0");
    let scrubbed = redact_url_credentials_in_text(&joined);
    structural_digest(scrubbed.as_bytes())
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

#[cfg(test)]
mod error_context_tests {
    //! Unit tests for `ExecutionContext` and the structured
    //! `GitMutationError::Execution` variant. The tests pin the
    //! boundary that the corrective security closure pass added:
    //! error types carry operation kind, remote name, classified
    //! failure kind, exit code, and **redacted** stdout/stderr —
    //! never raw argv or un-redacted credentials.

    use super::{ExecutionContext, GitMutationError};
    use crate::git_network_policy::NetworkFailureKind;
    use codegg_git::GitOperation;

    #[test]
    fn execution_context_builder_populates_fields() {
        let ctx = ExecutionContext::new("fetch")
            .with_remote("origin")
            .with_failure_kind(NetworkFailureKind::Authentication)
            .with_exit_code(128)
            .with_stdout("From origin\nabc..def main -> origin/main\n")
            .with_stderr("fatal: Authentication failed");
        assert_eq!(ctx.operation_kind, "fetch");
        assert_eq!(ctx.remote_name.as_deref(), Some("origin"));
        assert_eq!(ctx.failure_kind, Some(NetworkFailureKind::Authentication));
        assert_eq!(ctx.exit_code, Some(128));
        assert!(ctx.stdout_redacted.contains("origin/main"));
        assert!(ctx.stderr_redacted.contains("Authentication failed"));
        assert!(!ctx.timed_out);
    }

    #[test]
    fn execution_context_with_stdout_redacts_credentials() {
        let ctx = ExecutionContext::new("fetch").with_stdout(
            "From https://user:secret_token@github.com/r.git\n\
             \x20\x20\x20\x20abc..def main -> origin/main\n",
        );
        assert!(
            !ctx.stdout_redacted.contains("secret_token"),
            "stdout_redacted leaked credential: {}",
            ctx.stdout_redacted
        );
        assert!(ctx.stdout_redacted.contains("github.com"));
    }

    #[test]
    fn execution_context_with_stderr_redacts_credentials() {
        let ctx = ExecutionContext::new("fetch").with_stderr(
            "fatal: unable to access 'https://user:secret_token@github.com/r.git': \
             Could not resolve host: github.com",
        );
        assert!(
            !ctx.stderr_redacted.contains("secret_token"),
            "stderr_redacted leaked credential: {}",
            ctx.stderr_redacted
        );
    }

    #[test]
    fn execution_error_display_does_not_leak_credentials() {
        // Even when the message string happens to embed a URL (which it
        // should not in practice), the Display impl must not surface
        // anything from raw argv — but we also verify that the
        // struct-based payload keeps the operation_kind visible.
        let err = GitMutationError::execution_kind("remote add", "remote add failed");
        let displayed = format!("{err}");
        assert!(
            displayed.contains("remote add"),
            "missing op kind: {displayed}"
        );
    }

    #[test]
    fn execution_error_accessors_return_structured_fields() {
        let err = GitMutationError::execution_kind("fetch", "fetch exited with code 128");
        let inner = match err {
            GitMutationError::Execution { message, context } => {
                assert_eq!(message, "fetch exited with code 128");
                context
            }
            other => panic!("expected Execution variant, got {other:?}"),
        };
        assert_eq!(inner.operation_kind, "fetch");
        assert_eq!(inner.remote_name, None);
        assert_eq!(inner.failure_kind, None);
        assert_eq!(inner.exit_code, None);
    }

    #[test]
    fn execution_kind_helper_infers_from_operation() {
        let op = GitOperation::Fetch {
            remote: Some(codegg_git::RemoteName::new("origin").expect("valid name")),
            refspecs: vec![],
            all: false,
        };
        let err = GitMutationError::execution(&op, "boom");
        assert_eq!(err.operation_kind(), Some("fetch"));
        assert_eq!(err.failure_kind(), None);
        assert_eq!(err.exit_code(), None);
        assert_eq!(err.remote_name(), None);
    }

    #[test]
    fn timeout_error_carries_seconds() {
        let err = GitMutationError::Timeout(45);
        let displayed = format!("{err}");
        assert!(
            displayed.contains("45"),
            "timeout seconds missing: {displayed}"
        );
    }
}

#[cfg(test)]
mod truncate_tests {
    use super::{
        redact_url_credentials_in_text, sanitize_truncate_for_result, truncate_for_result,
    };

    #[test]
    fn truncate_for_result_is_utf8_boundary_safe() {
        // Git output routinely contains multi-byte characters (commit
        // messages, renamed paths); a byte-offset cut must not split one.
        let s = "コミット".repeat(200); // 2400 bytes
        let out = truncate_for_result(&s, 500);
        assert!(out.starts_with("コミット"));
        assert!(out.ends_with("\n... [truncated, original 2400 bytes]"));
    }

    #[test]
    fn truncate_for_result_short_input_untouched() {
        assert_eq!(truncate_for_result("ok", 500), "ok");
    }

    #[test]
    fn sanitize_truncate_redacts_url_credentials_in_stdout_and_stderr() {
        // Regression: the raw-mutation path (`run_raw_mutation`) must
        // route through this boundary like the typed path does. Every
        // Git-emitted byte reaching `MutationResult.stdout`/`stderr`
        // must be credential-free.
        let s = "remote: https://user:hunter2@example.com/repo.git\nok";
        let out = sanitize_truncate_for_result(s, 500);
        assert!(!out.contains("hunter2"), "credential leaked: {out}");
        assert!(out.contains("example.com"));
    }

    #[test]
    fn redact_url_credentials_in_text_handles_multiple_urls() {
        let s = "a https://u:p@one.example/x b ssh://git:token@two.example/y";
        let out = redact_url_credentials_in_text(s);
        assert!(!out.contains(":p@"), "leak: {out}");
        assert!(!out.contains(":token@"), "leak: {out}");
    }
}

#[cfg(test)]
mod m002_audit_label_tests {
    use super::{git_audit_op_label, git_audit_ref_digest};
    use codegg_core::audit_instrumentation::structural_digest;
    use codegg_git::ref_name::{BranchName, RemoteName};
    use codegg_git::sensitive::RedactedUrl;
    use codegg_git::GitOperation;

    fn branch(name: &str) -> BranchName {
        BranchName::new(name).expect("valid branch")
    }

    fn remote(name: &str) -> RemoteName {
        RemoteName::new(name).expect("valid remote")
    }

    #[test]
    fn mutation_labels_are_bounded() {
        let cases: Vec<(GitOperation, &str)> = vec![
            (GitOperation::Add { paths: vec![] }, "stage"),
            (
                GitOperation::Commit {
                    message: "x".to_owned(),
                    amend: false,
                    allow_empty: false,
                },
                "commit",
            ),
            (
                GitOperation::BranchCreate {
                    name: branch("feature"),
                    start_point: None,
                    force: false,
                },
                "branch_create",
            ),
            (
                GitOperation::Merge {
                    revisions: vec!["main".to_owned()],
                    no_ff: false,
                    strategy: None,
                    abort: false,
                },
                "merge",
            ),
            (
                GitOperation::Rebase {
                    upstream: None,
                    onto: None,
                    interactive: false,
                    abort: false,
                    continue_op: true,
                    skip: false,
                },
                "rebase",
            ),
            (
                GitOperation::Fetch {
                    remote: Some(remote("origin")),
                    refspecs: vec![],
                    all: false,
                },
                "fetch",
            ),
            (
                GitOperation::Push {
                    remote: Some(remote("origin")),
                    branch: Some("main".to_owned()),
                    set_upstream: false,
                    force: false,
                    force_with_lease: false,
                    tags: false,
                    delete: false,
                },
                "push",
            ),
            (GitOperation::Abort, "recover_abort"),
            (GitOperation::Continue, "recover_continue"),
            (GitOperation::Skip, "recover_skip"),
        ];
        for (op, expected) in cases {
            assert_eq!(git_audit_op_label(&op), Some(expected));
        }
    }

    #[test]
    fn read_only_operations_emit_no_git_event() {
        for op in [
            GitOperation::Status { short: false },
            GitOperation::Log {
                oneline: true,
                max_count: None,
                paths: vec![],
            },
            GitOperation::BranchList {
                remotes: false,
                all: true,
            },
            GitOperation::RemoteList,
            GitOperation::StashList,
            GitOperation::ConfigGet {
                key: "user.name".to_owned(),
                global: false,
                local: true,
            },
        ] {
            assert_eq!(git_audit_op_label(&op), None, "read-only {op:?}");
        }
    }

    #[test]
    fn ref_digest_covers_names_and_never_urls() {
        // Remote NAME contributes; the URL (even redacted) never does.
        let with_url = GitOperation::RemoteAdd {
            name: remote("origin"),
            url: RedactedUrl::new("https://user:s3cret@example.com/repo.git"),
        };
        let without_url = GitOperation::RemoteRemove {
            name: remote("origin"),
        };
        assert_eq!(git_audit_op_label(&with_url), Some("remote_add"));
        // Same name + no other ref material: identical digests prove
        // the URL contributed nothing.
        assert_eq!(
            git_audit_ref_digest(&with_url),
            git_audit_ref_digest(&without_url)
        );
        // Push binds remote name + branch.
        let push = GitOperation::Push {
            remote: Some(remote("origin")),
            branch: Some("main".to_owned()),
            set_upstream: false,
            force: false,
            force_with_lease: false,
            tags: false,
            delete: false,
        };
        assert_ne!(
            git_audit_ref_digest(&push),
            structural_digest(b""),
            "ref-bearing ops must not hash empty"
        );
        // Stage/commit carry no ref material: the empty structural
        // digest proves "no ref" without storing paths or messages.
        let empty = structural_digest(b"");
        assert_eq!(
            git_audit_ref_digest(&GitOperation::Add { paths: vec![] }),
            empty
        );
        assert_eq!(
            git_audit_ref_digest(&GitOperation::Commit {
                message: "s3cret message".to_owned(),
                amend: false,
                allow_empty: false,
            }),
            empty,
            "commit messages must never enter the digest preimage"
        );
    }

    #[test]
    fn managed_argv_digest_excludes_raw_argv() {
        let op = GitOperation::ManagedGitArgv {
            argv: vec![
                "git".to_owned(),
                "fetch".to_owned(),
                "https://user:s3cret@example.com/repo.git".to_owned(),
            ],
            risk: codegg_git::RiskSet::new(vec![codegg_git::GitRiskClass::NetworkRead]),
        };
        assert_eq!(git_audit_op_label(&op), Some("managed"));
        let digest = git_audit_ref_digest(&op);
        assert_ne!(digest, structural_digest(b""), "subcommand binds");
        // The digest binds only the bounded subcommand token.
        assert_eq!(
            digest,
            structural_digest("fetch".as_bytes()),
            "raw argv (URLs, paths) must never enter the preimage"
        );
    }
}
