//! The M005 C003 checkout writer fence — the tool-facing contract.
//!
//! # What this layer is for
//!
//! A native writer tool (`write`, `edit`, `replace`, `apply_patch`, the typed
//! `git` mutations) reaches its effect through a plain `std::fs` call. This
//! module defines the small contract those tools consult *immediately before*
//! that call, so the daemon can decide — from authoritative records, never
//! from model text — whether the effect is allowed, and can hold a
//! checkout-scoped writer permit across the effect's whole lifetime
//! (write, rename and rollback).
//!
//! # What this layer does not do
//!
//! It holds no permit state, resolves no identity, and reads no store. The
//! daemon-side adapter (`crate::core::writer_fence`) owns all of that and
//! implements [`CheckoutWriterFence`]. This module deliberately depends on
//! `codegg_core` types only; it never imports `crate::core`, so the tool
//! layer stays independent of the daemon adapter.
//!
//! # The two rules this contract encodes
//!
//! 1. **Scope containment.** Every effect target must resolve inside the
//!    run's own checkout. A model-supplied path that resolves outside it is
//!    refused *before* the effect. This is what stops a `git` call with a
//!    model-chosen `workdir` from mutating a checkout the run does not own.
//! 2. **Mutual exclusion.** When the checkout is shared by more than one
//!    run, exactly one writer may hold it at a time. The permit is acquired
//!    after approval and released only with positive evidence that the
//!    effect settled.
//!
//! Both are advisory in the sense that they *narrow* what a tool may do; they
//! never widen the permission, sandbox, or Git authorization layers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use crate::tool::backend::ToolExecutionContext;

// ---------------------------------------------------------------------------
// Effect classes
// ---------------------------------------------------------------------------

/// The class of native effect a tool is about to perform.
///
/// The class is what lets the daemon fail closed: an effect class it has no
/// qualified profile for is refused before the effect rather than allowed
/// through an untyped fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeEffectClass {
    /// A checked native file transaction (write / edit / replace / patch).
    FileWrite,
    /// A typed Git index, worktree, ref or config mutation.
    GitMutation,
    /// A process spawned against the checkout.
    ProcessSpawn,
    /// An effect this milestone cannot classify.
    ///
    /// Unknown is not "probably fine". It is refused under a shared profile.
    Unknown,
}

impl NativeEffectClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FileWrite => "file_write",
            Self::GitMutation => "git_mutation",
            Self::ProcessSpawn => "process_spawn",
            Self::Unknown => "unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

/// The daemon-resolved scope of the checkout one tool call may write to.
///
/// Every field is resolved by the daemon from authoritative records. Nothing
/// here is model-supplied: `worktree_id` names the managed checkout the run
/// holds, and `checkout_root` is its canonical path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutWriterScope {
    /// The managed checkout's id. Empty for a workspace-scoped turn that
    /// holds no managed worktree; such a turn can be contained but can never
    /// mint a permit.
    pub worktree_id: String,
    /// The canonical root of the checkout the run may write.
    pub checkout_root: PathBuf,
    /// The run that owns the checkout, when the run is scheduler-owned.
    pub agent_run_id: Option<String>,
    /// The group that shares the checkout, when one does.
    pub group_id: Option<String>,
}

impl CheckoutWriterScope {
    /// A scope rooted at a plain workspace, with no managed checkout.
    pub fn workspace(root: PathBuf) -> Self {
        Self {
            worktree_id: String::new(),
            checkout_root: root,
            agent_run_id: None,
            group_id: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why the fence refused an effect.
///
/// Every variant is a refusal that happens **before** the filesystem, index
/// or ref effect. None of them is a partial effect.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriterFenceError {
    #[error("no checkout writer scope was resolved for this call")]
    ScopeMissing,
    #[error("refusing to write '{path}': it resolves outside this run's checkout '{root}'")]
    OutsideScope { path: String, root: String },
    #[error("refusing a {class} effect: {reason}")]
    EffectClassDenied { class: &'static str, reason: String },
    #[error("this run is not a writer of checkout '{root}'")]
    NotAuthorized { root: String },
    #[error("this checkout is being written by another writer")]
    Held,
    #[error("the write was cancelled before it took effect")]
    Cancelled,
    #[error("this checkout's writer permit is quarantined and needs operator recovery")]
    Quarantined,
    #[error("the checkout writer fence is unavailable: {0}")]
    Unavailable(String),
}

impl WriterFenceError {
    /// Whether the refusal is a durable, operator-visible condition.
    ///
    /// A held permit and a quarantined permit both need attention; a
    /// cancelled write does not.
    pub fn needs_attention(&self) -> bool {
        matches!(self, Self::Held | Self::Quarantined)
    }
}

// ---------------------------------------------------------------------------
// Requests and outcomes
// ---------------------------------------------------------------------------

/// One effect's request to the fence.
#[derive(Debug, Clone)]
pub struct WriterFenceRequest {
    /// The class of effect about to happen.
    pub effect_class: NativeEffectClass,
    /// Every path the effect will touch. All of them must resolve inside
    /// the run's checkout.
    pub targets: Vec<PathBuf>,
    /// A stable key for this acquisition attempt, so a retried call does
    /// not mint a second permit for the same effect.
    pub idempotency_key: String,
    /// Roots the daemon granted this execution *in addition* to the
    /// checkout — the run's private scratch root and the shared scratch
    /// roots it was granted.
    ///
    /// These are daemon-owned, not model-chosen: the model never names them,
    /// it only asks to write under one. They are part of the scope because a
    /// scratch write is a legitimate write that is not a checkout write.
    pub granted_roots: Vec<PathBuf>,
    /// The actor the effect runs on behalf of, when the execution carries a
    /// bound principal.
    ///
    /// This is the transport-bound principal the daemon attached to the
    /// trusted execution-audit context. It is never synthesized from a
    /// model-supplied string. A shared checkout with no bound actor is
    /// refused: mutual exclusion without an attributed writer is not a
    /// writer, it is an anonymous writer.
    pub principal: Option<codegg_core::transport_auth::AuthenticatedPrincipal>,
}

/// How an effect finished, from the writer's point of view.
///
/// This is the permit's release evidence. `Unproven` does **not** free the
/// permit; it parks the checkout, because "I could not prove the write
/// landed" is not the same as "nothing happened".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriterFenceOutcome {
    /// The effect completed and its result was observed.
    Settled,
    /// The effect produced no change at all.
    NoEffect,
    /// The writer cannot prove the effect settled.
    Unproven(String),
}

// ---------------------------------------------------------------------------
// The fence
// ---------------------------------------------------------------------------

/// A held checkout writer permit.
///
/// The permit is held across the effect's whole lifetime. It is settled
/// explicitly; dropping it without settling leaves the checkout parked
/// rather than handing it to the next writer while the previous writer's
/// effects may still be landing.
pub struct WriterFencePermit {
    settler: Box<dyn WriterFencePermitSettler>,
}

impl WriterFencePermit {
    pub(crate) fn new(settler: Box<dyn WriterFencePermitSettler>) -> Self {
        Self { settler }
    }

    /// Settle the permit with positive evidence.
    ///
    /// `Unproven` parks the checkout instead of freeing it.
    pub async fn settle(self, outcome: WriterFenceOutcome) -> Result<(), WriterFenceError> {
        self.settler.settle(outcome).await
    }
}

impl std::fmt::Debug for WriterFencePermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriterFencePermit").finish_non_exhaustive()
    }
}

/// The daemon-side behaviour behind a [`WriterFencePermit`].
#[async_trait]
// Stable >=1.99 flags the `#[must_use]` injected by the async-trait expansion
// (the desugared future is already `must_use`). Suppressed at the macro site;
// no behavior change. Same convention as `crate::core::CoreClient`.
#[allow(clippy::double_must_use)]
pub trait WriterFencePermitSettler: Send {
    /// Release or park the permit according to the evidence.
    async fn settle(self: Box<Self>, outcome: WriterFenceOutcome) -> Result<(), WriterFenceError>;
}

/// The daemon's authority over one checkout's writers.
///
/// `Debug` is required because the execution context that carries the fence
/// is itself debuggable, and a redacted fence view is better than a context
/// that cannot be printed at all.
#[async_trait]
// Stable >=1.99 flags the `#[must_use]` injected by the async-trait expansion
// (the desugared future is already `must_use`). Suppressed at the macro site;
// no behavior change. Same convention as `crate::core::CoreClient`.
#[allow(clippy::double_must_use)]
pub trait CheckoutWriterFence: Send + Sync + std::fmt::Debug {
    /// The scope this fence governs.
    fn scope(&self) -> &CheckoutWriterScope;

    /// Authorize one effect, and take the checkout's permit when the
    /// checkout is shared.
    ///
    /// Returns `Ok(None)` when the effect is in scope and the checkout is
    /// not shared: the caller then performs the effect exactly as it did
    /// before, with no permit and no durable write.
    ///
    /// Returns a permit when the checkout is shared. The caller must hold it
    /// until the effect settles and then settle it with evidence.
    async fn begin(
        &self,
        request: WriterFenceRequest,
    ) -> Result<Option<WriterFencePermit>, WriterFenceError>;
}

// ---------------------------------------------------------------------------
// Tool-side helpers
// ---------------------------------------------------------------------------

/// Consult the fence carried by an execution context, if any.
///
/// A context with no fence is a legacy or test path with no resolved scope;
/// it performs no containment and mints no permit, which is the pre-M005
/// behavior.
pub async fn acquire_writer_permit(
    ctx: Option<&ToolExecutionContext>,
    effect_class: NativeEffectClass,
    targets: Vec<PathBuf>,
    idempotency_key: impl Into<String>,
) -> Result<Option<WriterFencePermit>, crate::error::ToolError> {
    let Some(ctx) = ctx else {
        return Ok(None);
    };
    let Some(fence) = ctx.writer_fence.as_ref() else {
        return Ok(None);
    };
    let request = WriterFenceRequest {
        effect_class,
        targets,
        idempotency_key: idempotency_key.into(),
        granted_roots: granted_roots(ctx),
        // The actor comes from the daemon-bound trusted audit context. A
        // legacy or local path without one carries no actor, and the fence
        // decides what that means.
        principal: ctx
            .execution_audit
            .as_ref()
            .map(|audit| audit.principal().clone()),
    };
    fence.begin(request).await.map_err(|error| {
        // A fence refusal is a permission outcome, not an execution
        // failure: it is not retryable, and it must read as a refusal.
        crate::error::ToolError::Permission(error.to_string())
    })
}

/// A stable key for one fence acquisition.
///
/// The tool name and the target paths say *what* is written; the invocation
/// key separates two calls that write the same path, so a retried call does
/// not collide with the attempt it retries.
pub fn writer_fence_key(
    tool: &str,
    targets: &[PathBuf],
    ctx: Option<&ToolExecutionContext>,
) -> String {
    let paths = targets
        .iter()
        .map(|target| target.display().to_string())
        .collect::<Vec<_>>()
        .join(",");
    match ctx.and_then(|ctx| ctx.invocation_key.as_deref()) {
        Some(invocation) => format!("{tool}:{paths}:{invocation}"),
        None => format!("{tool}:{paths}"),
    }
}

/// Settle the writer-fence permit for an effect that has already run.
///
/// The tool's own failure is what the caller sees: a settlement failure on a
/// failed effect is logged, not surfaced, because the effect's own failure is
/// the more important fact. A settlement failure on a *successful* effect is
/// surfaced, because the caller must know the checkout was not released.
pub async fn settle_writer_fence(
    permit: Option<WriterFencePermit>,
    outcome: WriterFenceOutcome,
    effect_failed: bool,
) -> Result<(), crate::error::ToolError> {
    match settle_writer_permit(permit, outcome).await {
        Ok(()) => Ok(()),
        Err(error) if effect_failed => {
            tracing::warn!(error = %error, "writer fence settlement failed after a failed effect");
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Settle a permit, mapping a settlement failure to a tool error.
///
/// A failure to settle is reported, never swallowed: the permit stays
/// parked, and the caller is told.
pub async fn settle_writer_permit(
    permit: Option<WriterFencePermit>,
    outcome: WriterFenceOutcome,
) -> Result<(), crate::error::ToolError> {
    let Some(permit) = permit else {
        return Ok(());
    };
    permit
        .settle(outcome)
        .await
        .map_err(|error| crate::error::ToolError::Execution(error.to_string()))
}

/// Resolve a target path against the scope's checkout root.
///
/// A path that does not exist yet is resolved through its nearest existing
/// ancestor, so a *new* file cannot escape the checkout by pointing at a
/// directory that is not there. The comparison is on canonical paths, so a
/// symlink that points out of the checkout does not buy an escape.
pub fn resolve_within_scope(
    scope: &CheckoutWriterScope,
    target: &Path,
    granted_roots: &[PathBuf],
) -> Result<PathBuf, WriterFenceError> {
    let root = canonical_root(&scope.checkout_root);

    let absolute = if target.is_absolute() {
        target.to_path_buf()
    } else {
        root.join(target)
    };

    let candidate = canonicalize_existing_prefix(&absolute);

    if is_under(&candidate, &root) {
        return Ok(candidate);
    }

    for granted in granted_roots {
        let granted_root = canonical_root(granted);
        if is_under(&candidate, &granted_root) {
            return Ok(candidate);
        }
    }

    Err(WriterFenceError::OutsideScope {
        path: absolute.to_string_lossy().into_owned(),
        root: root.to_string_lossy().into_owned(),
    })
}

/// The canonical form of a root, falling back to the lexical path when the
/// root cannot be read.
fn canonical_root(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

/// Whether a resolved path is the root itself or lives under it.
fn is_under(candidate: &Path, root: &Path) -> bool {
    candidate == root || candidate.starts_with(root)
}

/// Canonicalize the longest existing prefix of a path, keeping the
/// non-existent tail intact.
fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    let mut prefix = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match prefix.canonicalize() {
            Ok(canonical) => {
                let mut resolved = canonical;
                for part in tail.iter().rev() {
                    resolved.push(part);
                }
                return resolved;
            }
            Err(_) => {
                let Some(name) = prefix.file_name().map(|name| name.to_os_string()) else {
                    return path.to_path_buf();
                };
                tail.push(name);
                let Some(parent) = prefix.parent().map(Path::to_path_buf) else {
                    return path.to_path_buf();
                };
                if parent == prefix {
                    return path.to_path_buf();
                }
                prefix = parent;
            }
        }
    }
}

/// The shared handle type the execution context carries.
pub type SharedWriterFence = Arc<dyn CheckoutWriterFence>;

// ---------------------------------------------------------------------------
// The workspace-scoped fence
// ---------------------------------------------------------------------------

/// A fence over a plain workspace that holds no managed checkout.
///
/// A daemon turn that was not given a managed worktree still has a scope:
/// the session's workspace root. Such a turn can be *contained* — an effect
/// that resolves outside the workspace is refused — but it can never mint a
/// permit, because it holds no checkout lease. That is the whole of C003's
/// behavior for the default profile: no new acquisition, no durable write,
/// and the model can no longer reach a checkout the run does not own.
#[derive(Debug)]
pub struct WorkspaceWriterFence {
    scope: CheckoutWriterScope,
}

impl WorkspaceWriterFence {
    pub fn new(root: PathBuf) -> Self {
        Self {
            scope: CheckoutWriterScope::workspace(root),
        }
    }
}

#[async_trait]
impl CheckoutWriterFence for WorkspaceWriterFence {
    fn scope(&self) -> &CheckoutWriterScope {
        &self.scope
    }

    async fn begin(
        &self,
        request: WriterFenceRequest,
    ) -> Result<Option<WriterFencePermit>, WriterFenceError> {
        contain_targets(&self.scope, &request.targets, &request.granted_roots)?;
        // No managed checkout means no lease, so there is no permit to take
        // and nothing to settle.
        Ok(None)
    }
}

/// Refuse any target that resolves outside the scope's checkout and the
/// roots the daemon granted alongside it.
pub fn contain_targets(
    scope: &CheckoutWriterScope,
    targets: &[PathBuf],
    granted_roots: &[PathBuf],
) -> Result<(), WriterFenceError> {
    for target in targets {
        resolve_within_scope(scope, target, granted_roots)?;
    }
    Ok(())
}

/// The roots the daemon granted this execution beyond the checkout.
///
/// The private scratch root and the shared scratch roots come from the
/// execution's authorization context, which only the daemon builds. The
/// session scratch directory is the fallback for a local execution with no
/// scratch scope; it is derived from the session id, never from model text.
pub fn granted_roots(ctx: &ToolExecutionContext) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(scope) = ctx
        .execution_authorization
        .as_ref()
        .and_then(|authorization| authorization.scratch_scope.as_ref())
    {
        roots.push(scope.private_root.clone());
        roots.extend(scope.shared.iter().map(|root| root.path.clone()));
    } else if let Some(session_id) = ctx.session_id.as_deref().filter(|id| !id.is_empty()) {
        if let Ok(dir) = crate::scratch::session_scratch_dir(session_id) {
            roots.push(dir);
        }
    }
    roots
}

// ---------------------------------------------------------------------------
// The factory
// ---------------------------------------------------------------------------

/// What the daemon needs in order to resolve a run's checkout scope.
#[derive(Debug, Clone)]
pub struct WriterScopeRequest {
    /// The scheduler-owned run, when the call comes from a delegated run.
    pub agent_run_id: Option<String>,
    /// The run's workspace root, used when the run holds no managed checkout.
    pub workspace_root: PathBuf,
    /// The execution attempt within the run.
    ///
    /// The permit records it, so a permit can be attributed to one attempt
    /// rather than to a run that has been retried.
    pub attempt_id: Option<String>,
    /// The run's cancellation, so a cancelled writer is refused instead of
    /// parking the checkout's permit.
    pub cancellation: Option<tokio_util::sync::CancellationToken>,
}

/// Builds the fence for one run's checkout.
///
/// The daemon implements this; the agent and scheduler layers only carry
/// the trait object. The factory is where the authoritative scope is
/// resolved — from the run record and the worktree lease, never from a
/// model-supplied path.
#[async_trait]
// Stable >=1.99 flags the `#[must_use]` injected by the async-trait expansion
// (the desugared future is already `must_use`). Suppressed at the macro site;
// no behavior change. Same convention as `crate::core::CoreClient`.
#[allow(clippy::double_must_use)]
pub trait WriterFenceFactory: Send + Sync {
    /// Resolve the fence for one run.
    ///
    /// Returns `None` when the run holds no managed checkout and no
    /// workspace root was captured, in which case the call has no scope and
    /// performs no containment.
    async fn fence_for(
        &self,
        request: WriterScopeRequest,
    ) -> Result<Option<Arc<dyn CheckoutWriterFence>>, WriterFenceError>;
}
