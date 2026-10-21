//! M005 C005 — the tool-layer contract for refusing a process launch.
//!
//! # What this module is
//!
//! The C003 fence answers *"may this execution write, and under which
//! permit?"*. It cannot answer *"may this execution start a process?"*,
//! because a process's write set is not enumerable at the call site: the
//! program decides, at runtime, what to touch.
//!
//! So the C005 contract is a **refusal**, not a permit. There is no
//! "process permit" and no "settled / unproven" outcome, because there is no
//! effect to settle: the refusal happens *before* a process exists, and a
//! refused launch leaves nothing behind to account for.
//!
//! The contract is deliberately small:
//!
//! * [`ProcessScope`] is the **daemon-resolved** checkout scope of the
//!   execution. It is never a model-supplied working directory. A model that
//!   points a process at `/tmp` does not thereby leave the reserved
//!   checkout: the process's write set is unconstrained either way, so the
//!   decision is made about the *execution*, not about the *command*.
//! * [`ProcessAdmissionPolicy`] is the authority. The root crate's daemon
//!   adapter implements it against the durable reservation state; this
//!   module owns the shape, not the answer.
//! * [`admit_process`] is the single call a tool makes before it spawns
//!   anything.
//!
//! # What this module is not
//!
//! It is not a sandbox, a supervisor, or a containment mechanism. A refusal
//! stops CodeGG from *starting* a process in a reserved checkout. It says
//! nothing about a process that was already running, and it says nothing
//! about an unrelated program the same OS user starts outside CodeGG —
//! those are out of contract by design (ADR-0044 clause 1).
//!
//! The tool layer never imports `crate::core`: the daemon adapter supplies
//! the authority, the tool layer supplies the question.

use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;

use codegg_core::process_admission::{ProcessDenial, ProcessKind};

/// The daemon-resolved checkout scope of an execution.
///
/// The scope is resolved once, by the daemon, from durable records. A tool
/// never derives it from its own arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessScope {
    /// The canonical checkout root the execution is bound to.
    pub checkout_root: PathBuf,
    /// The managed checkout identity, when the execution holds one.
    pub worktree_id: Option<String>,
}

impl ProcessScope {
    /// A scope for an ordinary, unmanaged checkout.
    pub fn unmanaged(checkout_root: PathBuf) -> Self {
        Self {
            checkout_root,
            worktree_id: None,
        }
    }
}

/// A request to start a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessAdmissionRequest {
    /// The kind of process that would be created.
    pub kind: ProcessKind,
    /// The daemon-resolved scope the launch would run in.
    pub scope: ProcessScope,
}

/// The answer to a [`ProcessAdmissionRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessAdmissionOutcome {
    /// The launch may proceed.
    Admitted,
    /// The launch is refused, before any process exists.
    Denied(ProcessDenial),
}

impl ProcessAdmissionOutcome {
    /// Whether the launch may proceed.
    pub fn is_admitted(&self) -> bool {
        matches!(self, Self::Admitted)
    }

    /// The denial, when the launch was refused.
    pub fn denial(&self) -> Option<&ProcessDenial> {
        match self {
            Self::Admitted => None,
            Self::Denied(denial) => Some(denial),
        }
    }
}

/// Why the admission authority could not answer.
#[derive(Debug, thiserror::Error)]
#[error("process admission lookup failed: {0}")]
pub struct ProcessAdmissionError(pub String);

/// The authority that answers whether a process may be started.
///
/// Implementations resolve the answer from **durable daemon records**. A
/// model argument, a tool description, a plugin's own metadata and a
/// prompt are all outside the authority: none of them can widen the
/// profile.
#[async_trait::async_trait]
#[allow(clippy::double_must_use)]
pub trait ProcessAdmissionPolicy: Send + Sync + Debug {
    /// The scope this policy governs.
    fn scope(&self) -> &ProcessScope;

    /// The answer for a launch request.
    async fn admit(
        &self,
        request: &ProcessAdmissionRequest,
    ) -> Result<ProcessAdmissionOutcome, ProcessAdmissionError>;
}

/// The concrete policy: the durable authority, asked about a scope.
///
/// The tool layer owns the *shape* of the contract, and this is the one
/// implementation of it: it answers from the durable reservation state
/// that `codegg-core` owns. Keeping the adapter beside the trait means a
/// lower layer — the scheduler, for instance — can build a policy without
/// reaching into the daemon's `core` module.
pub struct ServiceProcessAdmission {
    scope: ProcessScope,
    service: Arc<codegg_core::process_admission::ProcessAdmissionService>,
}

impl std::fmt::Debug for ServiceProcessAdmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceProcessAdmission")
            .field("checkout_root", &self.scope.checkout_root)
            .field("worktree_id", &self.scope.worktree_id)
            .finish()
    }
}

impl ServiceProcessAdmission {
    /// Build the policy for a checkout scope.
    pub fn for_scope(
        service: Arc<codegg_core::process_admission::ProcessAdmissionService>,
        checkout_root: &std::path::Path,
        worktree_id: Option<String>,
    ) -> SharedProcessAdmission {
        Arc::new(Self {
            scope: ProcessScope {
                checkout_root: checkout_root.to_path_buf(),
                worktree_id,
            },
            service,
        })
    }
}

#[async_trait::async_trait]
impl ProcessAdmissionPolicy for ServiceProcessAdmission {
    fn scope(&self) -> &ProcessScope {
        &self.scope
    }

    async fn admit(
        &self,
        request: &ProcessAdmissionRequest,
    ) -> Result<ProcessAdmissionOutcome, ProcessAdmissionError> {
        let outcome = self
            .service
            .admit_scope(&self.scope.checkout_root, request.kind)
            .await
            .map_err(|error| ProcessAdmissionError(error.to_string()))?;
        Ok(match outcome {
            codegg_core::process_admission::ProcessAdmission::Admitted => {
                ProcessAdmissionOutcome::Admitted
            }
            codegg_core::process_admission::ProcessAdmission::Denied(denial) => {
                ProcessAdmissionOutcome::Denied(denial)
            }
        })
    }
}

/// Build the policy handle for a checkout scope.
///
/// `None` service means the daemon has no durable reservation authority:
/// a local-only daemon can never share a checkout, so it has nothing to
/// refuse and every process keeps working.
pub fn build_process_admission(
    service: Option<Arc<codegg_core::process_admission::ProcessAdmissionService>>,
    checkout_root: &std::path::Path,
    worktree_id: Option<String>,
) -> Option<SharedProcessAdmission> {
    let service = service?;
    Some(ServiceProcessAdmission::for_scope(
        service,
        checkout_root,
        worktree_id,
    ))
}

/// A shared process admission policy.
pub type SharedProcessAdmission = Arc<dyn ProcessAdmissionPolicy>;

/// Emit the durable refusal event for a refused process launch.
///
/// This is the live owner of the `process_denial` audit action. The event
/// is attributed to the transport-bound principal carried by the trusted
/// execution-audit context, and it names the **capability** that was
/// refused, the reason and the group. It never names a path, a command
/// line, an argv, or a file body: a process refusal is a concurrency
/// answer, and the command that was refused is the caller's own business.
pub async fn emit_process_denial(
    emitter: &codegg_core::audit_instrumentation::ExecutionAuditEmitter,
    audit: &codegg_core::audit_instrumentation::TrustedExecutionAuditContext,
    denial: &ProcessDenial,
) {
    let builder = codegg_core::audit_instrumentation::process_denial_event(
        audit.principal(),
        audit.provenance(),
        audit.chain(),
        denial.kind.as_str(),
        denial.reason.as_str(),
        denial.group_id.as_deref().unwrap_or(""),
        denial.worktree_id.as_deref().unwrap_or(""),
    );
    emitter.emit(builder).await;
}

/// Consult the gate for a tool, by tool name.
///
/// This is the tool-boundary check: it classifies the tool by its
/// **effective runtime effect** (see
/// [`crate::tool::process_capability`]) and refuses the call when the
/// restricted shared-checkout profile denies that capability.
///
/// A tool whose capability is *not* a direct spawn is not refused here.
/// `job` submits work whose process class the scheduler decides, where the
/// job kind is known; `task` creates a child execution that inherits the
/// narrow profile. Refusing either here would refuse more than the
/// contract does.
pub async fn admit_tool_process(
    tool_name: &str,
    ctx: Option<&crate::tool::backend::ToolExecutionContext>,
) -> Result<(), codegg_core::error::ToolError> {
    let capability = crate::tool::process_capability::process_capability(tool_name);
    if !capability.spawns_itself() && !capability.uses_persistent_process() {
        return Ok(());
    }
    let Some(kind) = capability.process_kind() else {
        return Ok(());
    };
    admit_process_for_context(ctx, kind).await
}

/// Consult the gate carried by an execution context.
///
/// This is the shape a tool uses: it hands over the execution context it
/// was given and the capability it is about to create.
pub async fn admit_process_for_context(
    ctx: Option<&crate::tool::backend::ToolExecutionContext>,
    kind: ProcessKind,
) -> Result<(), codegg_core::error::ToolError> {
    admit_process(ctx.and_then(|ctx| ctx.process_admission.as_ref()), kind).await
}

/// Consult the gate.
///
/// A `None` policy is an execution that is not under the restricted shared
/// profile — an ordinary single-user session, a CLI run, a test harness.
/// There is no reservation to violate, so the launch is allowed, exactly as
/// it was before this milestone.
///
/// The refusal is typed and carries the recovery path. It is deliberately
/// *not* a [`codegg_core::error::ToolError::Permission`]: a permission
/// failure says "you may not do this", while this says "this checkout is
/// shared, and a shared checkout admits no unqualified process". The
/// second is a profile fact, and a caller that retries it with different
/// arguments is still refused.
pub async fn admit_process(
    policy: Option<&SharedProcessAdmission>,
    kind: ProcessKind,
) -> Result<(), codegg_core::error::ToolError> {
    let Some(policy) = policy else {
        return Ok(());
    };
    let request = ProcessAdmissionRequest {
        kind,
        scope: policy.scope().clone(),
    };
    let outcome = policy.admit(&request).await.map_err(|error| {
        codegg_core::error::ToolError::ProcessDenied {
            kind,
            reason: codegg_core::process_admission::ProcessDenialReason::AuthorityUnavailable,
            recovery: error.0,
        }
    })?;
    match outcome {
        ProcessAdmissionOutcome::Admitted => Ok(()),
        ProcessAdmissionOutcome::Denied(denial) => {
            Err(codegg_core::error::ToolError::ProcessDenied {
                kind: denial.kind,
                reason: denial.reason,
                recovery: denial.recovery().to_owned(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::process_admission::ProcessDenialReason;

    #[derive(Debug)]
    struct Denying {
        scope: ProcessScope,
    }

    #[async_trait::async_trait]
    impl ProcessAdmissionPolicy for Denying {
        fn scope(&self) -> &ProcessScope {
            &self.scope
        }

        async fn admit(
            &self,
            _request: &ProcessAdmissionRequest,
        ) -> Result<ProcessAdmissionOutcome, ProcessAdmissionError> {
            Ok(ProcessAdmissionOutcome::Denied(ProcessDenial {
                reason: ProcessDenialReason::SharedCheckout,
                kind: ProcessKind::Command,
                worktree_id: Some("wt-1".to_owned()),
                group_id: Some("grp-1".to_owned()),
            }))
        }
    }

    #[derive(Debug)]
    struct Admitting {
        scope: ProcessScope,
    }

    #[async_trait::async_trait]
    impl ProcessAdmissionPolicy for Admitting {
        fn scope(&self) -> &ProcessScope {
            &self.scope
        }

        async fn admit(
            &self,
            _request: &ProcessAdmissionRequest,
        ) -> Result<ProcessAdmissionOutcome, ProcessAdmissionError> {
            Ok(ProcessAdmissionOutcome::Admitted)
        }
    }

    fn scope() -> ProcessScope {
        ProcessScope::unmanaged(std::path::PathBuf::from("/tmp/example"))
    }

    #[tokio::test]
    async fn a_refused_launch_is_typed_and_carries_the_recovery() {
        let policy: SharedProcessAdmission = Arc::new(Denying { scope: scope() });
        let error = admit_process(Some(&policy), ProcessKind::Command)
            .await
            .expect_err("the launch must be refused");
        match error {
            codegg_core::error::ToolError::ProcessDenied {
                kind,
                reason,
                recovery,
            } => {
                assert_eq!(kind, ProcessKind::Command);
                assert_eq!(reason, ProcessDenialReason::SharedCheckout);
                assert!(recovery.contains("isolated checkout"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_admitted_launch_proceeds() {
        let policy: SharedProcessAdmission = Arc::new(Admitting { scope: scope() });
        admit_process(Some(&policy), ProcessKind::Command)
            .await
            .expect("the launch must be admitted");
    }

    #[tokio::test]
    async fn an_execution_without_a_policy_is_unrestricted() {
        admit_process(None, ProcessKind::Command)
            .await
            .expect("an ordinary execution is not restricted");
    }
}
