//! M005 C003 — the daemon's checkout writer fence.
//!
//! # What this adapter is
//!
//! The tool layer (`crate::tool::writer_fence`) owns the *contract*: a tool
//! asks the fence whether it may perform a native effect, and the fence
//! answers with either "no scope, do as you did before" or "here is the
//! checkout's permit, hold it until your effect settles".
//!
//! This module owns the *authority* behind that contract. It resolves the
//! run's checkout scope from daemon-owned stores, contains the effect inside
//! that scope, fails closed on effect classes this milestone has no profile
//! for, and — when the checkout is genuinely shared — takes the durable
//! writer permit that serializes the writers.
//!
//! # The two rules, and where they come from
//!
//! **Containment.** The effect's target must resolve inside the run's own
//! checkout. This is what closes the hole where the `git` tool accepted a
//! model-supplied `workdir` and could then stage, commit or reset a
//! repository the run does not own. The comparison is made on canonical
//! paths, so a symlink out of the checkout buys nothing.
//!
//! **Mutual exclusion.** A checkout shared by more than one run admits one
//! writer at a time. The share is not asserted by a caller: it is derived
//! from the durable group record. Two runs share a checkout when they are
//! members of the same *live* group and the lease belongs to one of them.
//! The group is the bounded reservation; the lease still fences the
//! generation, and the permit still records the individual run.
//!
//! # What this adapter deliberately does not do
//!
//! It does not widen anything. A refusal here is *additional* to the
//! permission, sandbox and Git-authorization layers, never a substitute for
//! them. It does not merge branches, does not resolve conflicts, and does
//! not decide that two writers may run at once because their writes look
//! disjoint — overlap is decided by the permit, not by inspection.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use codegg_core::agent_run::AgentRunStore;
use codegg_core::agent_run_group::AgentRunGroupService;
use codegg_core::identity::{AgentRunGroupId, AgentRunId, WorktreeId};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::worktree_service::WorktreeService;
use codegg_core::writer_permit::{
    QuiescenceEvidence, WriterPermitOperation, WriterPermitQuarantineReason,
};

use crate::core::writer_permit::{WriterPermitRequest, WriterPermitService};
use crate::tool::writer_fence::{
    CheckoutWriterFence, CheckoutWriterScope, NativeEffectClass, WriterFenceError,
    WriterFenceOutcome, WriterFencePermit, WriterFencePermitSettler, WriterFenceRequest,
};

/// How long a writer waits for a busy checkout before it is told no.
const WRITER_PERMIT_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// The daemon's authority over one run's checkout.
pub struct DaemonWriterFence {
    scope: CheckoutWriterScope,
    permit: Arc<WriterPermitService>,
    worktrees: Arc<WorktreeService>,
    runs: Arc<dyn AgentRunStore>,
    groups: Option<Arc<AgentRunGroupService>>,
    run_id: Option<AgentRunId>,
    attempt_id: Option<String>,
    cancellation: Option<tokio_util::sync::CancellationToken>,
    /// The principal used when the call carries no bound actor.
    ///
    /// A single-user daemon has no transport-bound principal for a
    /// scheduler-owned child, but it still owns the checkout it is writing.
    /// The fallback is the daemon's own local principal and is only ever
    /// installed by a daemon that has no team policy; a daemon with one
    /// leaves this empty, so an unattributed writer is refused rather than
    /// written anonymously.
    principal_fallback: Option<AuthenticatedPrincipal>,
    /// How long a writer waits for a busy checkout before it is told no.
    wait_timeout: std::time::Duration,
}

impl std::fmt::Debug for DaemonWriterFence {
    /// Redacted on purpose: the fence holds a principal and a permit handle,
    /// and neither belongs in a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonWriterFence")
            .field("scope", &self.scope)
            .field("run_id", &self.run_id)
            .field("attempt_id", &self.attempt_id)
            .finish_non_exhaustive()
    }
}

impl DaemonWriterFence {
    /// Build the fence for a run that holds a managed checkout.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scope: CheckoutWriterScope,
        permit: Arc<WriterPermitService>,
        worktrees: Arc<WorktreeService>,
        runs: Arc<dyn AgentRunStore>,
        groups: Option<Arc<AgentRunGroupService>>,
        run_id: Option<AgentRunId>,
        attempt_id: Option<String>,
        cancellation: Option<tokio_util::sync::CancellationToken>,
    ) -> Self {
        Self {
            scope,
            permit,
            worktrees,
            runs,
            groups,
            run_id,
            attempt_id,
            cancellation,
            principal_fallback: None,
            wait_timeout: WRITER_PERMIT_WAIT,
        }
    }

    /// Install the principal used when the call carries no bound actor.
    ///
    /// Only a daemon without a team policy may install one. A daemon with a
    /// team policy must bind the transport principal, and an unattributed
    /// write is refused rather than attributed to the daemon.
    pub fn with_principal_fallback(mut self, principal: AuthenticatedPrincipal) -> Self {
        self.principal_fallback = Some(principal);
        self
    }

    /// Bound the wait for a busy checkout.
    ///
    /// The wait is bounded so a writer cannot be parked forever behind
    /// another writer; when the bound expires the writer is refused with the
    /// same typed refusal a busy checkout produces.
    pub fn with_wait_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.wait_timeout = timeout;
        self
    }

    /// The checkout this fence governs, re-resolved from the run's own
    /// durable record.
    ///
    /// The run's record wins over the scope: a scope that names a different
    /// checkout than the run actually holds must not be able to bind the
    /// permit. A run with no durable record — a daemon turn — falls back to
    /// the scope the daemon resolved for it.
    async fn worktree_id(&self) -> Option<WorktreeId> {
        if let Some(run) = self.run_id.as_ref() {
            if let Ok(Some(record)) = self.runs.get_run(run).await {
                return record.worktree_id.clone();
            }
        }
        if self.scope.worktree_id.is_empty() {
            return None;
        }
        WorktreeId::parse(&self.scope.worktree_id).ok()
    }

    /// The live group that shares this checkout, if any.
    ///
    /// A checkout is shared when the run and the run that holds the checkout's
    /// lease are members of the same live group. The group is the bounded
    /// reservation that authorizes the second writer; without it, the lease
    /// owner is the only writer and the checkout is private.
    async fn shared_group(&self) -> Option<AgentRunGroupId> {
        let groups = self.groups.as_ref()?;
        let run = self.run_id.clone()?;
        let worktree_id = self.worktree_id().await?;
        let record = self.worktrees.get(&worktree_id).await.ok()?;
        let owner = record.owner_run_id.clone()?;

        if owner == run {
            // The lease owner is a writer in its own right. It still needs a
            // group to *share* the checkout, but the group is looked up
            // below through the membership overlap.
        }

        let member_groups = groups.groups_for_member(&run).await.ok()?;
        let owner_groups = groups.groups_for_member(&owner).await.ok()?;

        member_groups
            .into_iter()
            .find(|member_group| {
                !codegg_core::agent_run_group::RunGroupStatus::is_terminal(member_group.status)
                    && owner_groups
                        .iter()
                        .any(|owner_group| owner_group.group_id == member_group.group_id)
            })
            .map(|member_group| member_group.group_id)
    }

    /// Record a refusal in the daemon's log.
    ///
    /// The event carries the scope's identity and the reason, never a path,
    /// a file body or a model string: the refusal is about *which* checkout
    /// and *which* class, and anything more would be private data.
    fn record_denial(&self, request: &WriterFenceRequest, reason: &str) {
        tracing::warn!(
            worktree_id = %self.scope.worktree_id,
            agent_run_id = self.run_id.as_ref().map(|run| run.as_str()).unwrap_or("none"),
            effect_class = request.effect_class.as_str(),
            reason = reason,
            "the checkout writer fence refused an effect",
        );
    }

    /// Whether two runs share a live group, which is what makes a checkout
    /// shared rather than private to its lease owner.
    async fn shares_live_group(
        &self,
        requesting: &AgentRunId,
        owner: &AgentRunId,
    ) -> Result<bool, WriterFenceError> {
        let Some(groups) = self.groups.as_ref() else {
            return Ok(false);
        };
        let member_groups = groups
            .groups_for_member(requesting)
            .await
            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?;
        let owner_groups = groups
            .groups_for_member(owner)
            .await
            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?;
        Ok(member_groups.iter().any(|member_group| {
            !codegg_core::agent_run_group::RunGroupStatus::is_terminal(member_group.status)
                && owner_groups
                    .iter()
                    .any(|owner_group| owner_group.group_id == member_group.group_id)
        }))
    }

    /// The permit operation an effect class maps to.
    ///
    /// A class this milestone cannot type is `None`, and the caller refuses
    /// it. There is no "best effort" mapping and no raw fallback.
    fn operation_for(effect_class: NativeEffectClass) -> Option<WriterPermitOperation> {
        match effect_class {
            NativeEffectClass::FileWrite => Some(WriterPermitOperation::NativeFileWrite),
            NativeEffectClass::GitMutation => Some(WriterPermitOperation::NativeGitMutation),
            NativeEffectClass::ProcessSpawn => Some(WriterPermitOperation::ProcessSpawn),
            NativeEffectClass::Unknown => None,
        }
    }
}

#[async_trait]
impl CheckoutWriterFence for DaemonWriterFence {
    fn scope(&self) -> &CheckoutWriterScope {
        &self.scope
    }

    async fn begin(
        &self,
        request: WriterFenceRequest,
    ) -> Result<Option<WriterFencePermit>, WriterFenceError> {
        // 1. Containment, before anything else. A target that resolves
        //    outside the run's own checkout is refused here, so no permit is
        //    taken and no effect is attempted.
        crate::tool::writer_fence::contain_targets(
            &self.scope,
            &request.targets,
            &request.granted_roots,
        )?;

        // 2. A run that holds no managed checkout has no lease, so there is
        //    no permit to take. Containment above is the whole of the fence
        //    for such a run, and it costs no durable write.
        let Some(worktree_id) = self.worktree_id().await else {
            return Ok(None);
        };

        // 3. The run must be a writer of this checkout: the run that holds
        //    the lease, or a run that shares a live group with it. A run
        //    that is neither is refused before any permit is taken.
        let record = self
            .worktrees
            .get(&worktree_id)
            .await
            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?;
        let owner = record.owner_run_id.clone();
        let authorized = match (self.run_id.as_ref(), owner.as_ref()) {
            (Some(run), Some(owner)) => run == owner || self.shares_live_group(run, owner).await?,
            _ => false,
        };
        if !authorized {
            return Err(WriterFenceError::NotAuthorized {
                root: self.scope.checkout_root.to_string_lossy().into_owned(),
            });
        }

        // 4. The checkout's share is derived from durable state, never
        //    asserted by the caller.
        let shared_group = self.shared_group().await;

        // 5. Fail closed on effect classes this milestone has no profile
        //    for, but only where a shared profile is in force: an isolated
        //    checkout keeps its pre-M005 behaviour exactly.
        let Some(group_id) = shared_group.clone() else {
            return Ok(None);
        };

        let operation = match Self::operation_for(request.effect_class) {
            Some(operation) => operation,
            None => {
                self.record_denial(
                    &request,
                    "this effect class has no qualified profile for a shared checkout",
                );
                return Err(WriterFenceError::EffectClassDenied {
                    class: request.effect_class.as_str(),
                    reason: "this effect class has no qualified profile for a shared checkout"
                        .to_owned(),
                });
            }
        };

        if request.effect_class == NativeEffectClass::ProcessSpawn {
            self.record_denial(
                &request,
                "process writers are refused on a shared checkout; the process deny \
                 contract refuses every unqualified process surface",
            );
            return Err(WriterFenceError::EffectClassDenied {
                class: request.effect_class.as_str(),
                reason: "process writers are refused on a shared checkout; the process deny \
                         contract refuses every unqualified process surface"
                    .to_owned(),
            });
        }

        // 6. A shared checkout with no bound actor is refused. Mutual
        //    exclusion without an attributed writer is an anonymous writer.
        //    A single-user daemon owns its own checkout, so it names its own
        //    principal; a daemon with a team policy installs no fallback and
        //    refuses the unattributed write instead.
        let principal = match request
            .principal
            .clone()
            .or_else(|| self.principal_fallback.clone())
        {
            Some(principal) => principal,
            None => {
                self.record_denial(&request, "this shared checkout has no bound actor");
                return Err(WriterFenceError::EffectClassDenied {
                    class: request.effect_class.as_str(),
                    reason: "this shared checkout has no bound actor for the write".to_owned(),
                });
            }
        };

        let run_id = self
            .run_id
            .clone()
            .ok_or_else(|| WriterFenceError::EffectClassDenied {
                class: request.effect_class.as_str(),
                reason: "a shared checkout write must name the run that holds it".to_owned(),
            })?;

        // 7. Take the checkout's permit. The permit is bound to the run and
        //    the lease generation, so a checkout that was re-leased under the
        //    writer's feet cannot be written through a stale scope.
        let permit_request = WriterPermitRequest {
            worktree_id,
            principal,
            agent_run_id: Some(run_id.clone()),
            group_id: Some(group_id.clone()),
            attempt_id: self.attempt_id.clone(),
            operation,
            idempotency_key: request.idempotency_key.clone(),
            wait: Some(crate::core::writer_permit::WriterPermitWait {
                timeout: self.wait_timeout,
                cancellation: self.cancellation.clone().unwrap_or_default(),
            }),
        };

        let guard = self.permit.acquire(permit_request).await.map_err(|error| {
            use codegg_core::writer_permit::WriterPermitError;
            match error {
                WriterPermitError::Held | WriterPermitError::WaitTimeout => WriterFenceError::Held,
                WriterPermitError::Cancelled => WriterFenceError::Cancelled,
                WriterPermitError::Quarantined => WriterFenceError::Quarantined,
                other => WriterFenceError::Unavailable(other.to_string()),
            }
        })?;

        tracing::info!(
            worktree_id = %self.scope.worktree_id,
            agent_run_id = run_id.as_str(),
            group_id = %group_id.as_str(),
            effect_class = request.effect_class.as_str(),
            "the checkout writer fence took the checkout's writer permit",
        );

        Ok(Some(WriterFencePermit::new(Box::new(PermitSettler {
            guard,
            worktree_id: self.scope.worktree_id.clone(),
            run_id: run_id.as_str().to_owned(),
        }))))
    }
}

/// Releases the checkout's permit once the effect has settled.
struct PermitSettler {
    guard: crate::core::writer_permit::WriterPermitGuard,
    worktree_id: String,
    run_id: String,
}

#[async_trait]
impl WriterFencePermitSettler for PermitSettler {
    async fn settle(self: Box<Self>, outcome: WriterFenceOutcome) -> Result<(), WriterFenceError> {
        let evidence = match outcome {
            WriterFenceOutcome::Settled => QuiescenceEvidence::Settled,
            WriterFenceOutcome::NoEffect => QuiescenceEvidence::NoEffect,
            // Unproven does not free the checkout. It parks it, because
            // "I could not show that the write landed" is not the same as
            // "nothing happened".
            WriterFenceOutcome::Unproven(_) => {
                QuiescenceEvidence::Unproven(WriterPermitQuarantineReason::IncompleteEffects)
            }
        };
        let settled = self.guard.release(evidence).await;
        match &settled {
            Ok(_) => tracing::info!(
                worktree_id = %self.worktree_id,
                agent_run_id = %self.run_id,
                "the checkout writer fence released the checkout's writer permit",
            ),
            Err(error) => tracing::warn!(
                worktree_id = %self.worktree_id,
                agent_run_id = %self.run_id,
                error = %error,
                "the checkout writer fence could not release the checkout's writer permit",
            ),
        }
        settled
            .map(|_| ())
            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))
    }
}

/// The scope of a run that holds a managed checkout.
pub fn managed_scope(
    worktree_id: &WorktreeId,
    checkout_root: PathBuf,
    run_id: Option<AgentRunId>,
) -> CheckoutWriterScope {
    CheckoutWriterScope {
        worktree_id: worktree_id.as_str().to_owned(),
        checkout_root,
        agent_run_id: run_id.map(|run| run.as_str().to_owned()),
        group_id: None,
    }
}

/// The principal a fence uses when the daemon has no bound actor for a
/// checkout.
///
/// This is only ever the local daemon's own principal, and it exists so a
/// single-user daemon can still write its own checkout. A daemon with a
/// team policy never reaches this path: the bound principal is required.
pub fn local_principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal::local_owner("codegg-writer-fence")
}

/// Builds the fence for one run's checkout.
///
/// M005 C006. This is the daemon-side implementation of the tool layer's
/// `WriterFenceFactory` seam, and it is the piece that makes the shared
/// profile *real*: the authority behind the fence — the permit service, the
/// worktree service, the durable run store and the group service — lives
/// here, in the daemon, and the agent and scheduler layers only carry the
/// trait object.
///
/// The scope is resolved from the run's **own durable record**, never from a
/// model-supplied path: a run that holds no managed checkout gets no fence at
/// all, and a run that holds one gets the fence for exactly that checkout.
pub struct DaemonWriterFenceFactory {
    permit: Arc<WriterPermitService>,
    worktrees: Arc<WorktreeService>,
    runs: Arc<dyn AgentRunStore>,
    groups: Option<Arc<AgentRunGroupService>>,
    /// Whether this daemon may attribute an unattributed write to itself.
    ///
    /// A single-user daemon owns its own checkout and names its own
    /// principal. A daemon with a team policy leaves this off, so a write
    /// that arrives without a bound actor is refused rather than written
    /// anonymously.
    local_principal_fallback: bool,
}

impl DaemonWriterFenceFactory {
    /// Build the factory.
    pub fn new(
        permit: Arc<WriterPermitService>,
        worktrees: Arc<WorktreeService>,
        runs: Arc<dyn AgentRunStore>,
        groups: Option<Arc<AgentRunGroupService>>,
        local_principal_fallback: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            permit,
            worktrees,
            runs,
            groups,
            local_principal_fallback,
        })
    }
}

#[async_trait]
impl crate::tool::writer_fence::WriterFenceFactory for DaemonWriterFenceFactory {
    async fn fence_for(
        &self,
        request: crate::tool::writer_fence::WriterScopeRequest,
    ) -> Result<Option<Arc<dyn CheckoutWriterFence>>, WriterFenceError> {
        let run_id = request
            .agent_run_id
            .as_deref()
            .and_then(|raw| AgentRunId::parse(raw).ok());

        // The run's durable record is the authority for which checkout the
        // run holds. Without a record there is no lease, and a run with no
        // lease has nothing to fence.
        let worktree_id = match run_id.as_ref() {
            Some(run) => self
                .runs
                .get_run(run)
                .await
                .ok()
                .flatten()
                .and_then(|record| record.worktree_id),
            None => None,
        };
        let Some(worktree_id) = worktree_id else {
            return Ok(None);
        };
        let record = self
            .worktrees
            .get(&worktree_id)
            .await
            .map_err(|error| WriterFenceError::Unavailable(error.to_string()))?;

        let scope = managed_scope(&worktree_id, record.path.clone(), run_id.clone());
        let mut fence = DaemonWriterFence::new(
            scope,
            Arc::clone(&self.permit),
            Arc::clone(&self.worktrees),
            Arc::clone(&self.runs),
            self.groups.clone(),
            run_id,
            request.attempt_id.clone(),
            request.cancellation.clone(),
        );
        if self.local_principal_fallback {
            fence = fence.with_principal_fallback(local_principal());
        }
        Ok(Some(Arc::new(fence)))
    }
}
