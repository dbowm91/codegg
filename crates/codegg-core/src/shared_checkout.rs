//! The managed checkout a shared agent-run group uses.
//!
//! M005 C006. A group of bounded agents may be placed on **one** managed
//! checkout instead of one checkout each. The group is the bounded
//! reservation: it names the runs that may write, and the checkout's writer
//! permit turns "may write" into "may write *now*".
//!
//! This module answers one question — *which checkout does this group share?*
//! — and creates that checkout when the group does not have one yet. It never
//! grants authority on its own: the run must already be a member of the
//! durable group record, and the group's reservation is what authorizes it.

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::OwnedMutexGuard;

use super::agent_run::AgentRunStore;
use super::agent_run_group::{AgentRunGroupRecord, AgentRunGroupService};
use crate::identity::{AgentRunGroupId, AgentRunId, WorktreeId};
use crate::process_admission::ProcessAdmissionService;
use crate::worktree_service::{
    CreateWorktreeRequest, ManagedWorktreeState, WorktreeLease, WorktreeRecord, WorktreeService,
};

/// The managed checkout a live group shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedCheckout {
    /// The managed checkout's identity.
    pub worktree_id: WorktreeId,
    /// The checkout's root path.
    pub path: PathBuf,
    /// The lease generation the checkout is currently held at.
    pub lease_generation: u64,
    /// The run that holds the checkout's lease.
    ///
    /// Every other member of the group writes under this run's lease, bound
    /// to this generation. Re-leasing the checkout to a different owner
    /// bumps the generation and invalidates every permit bound to the old
    /// one.
    pub owner_run_id: AgentRunId,
    /// The group that shares the checkout.
    pub group_id: AgentRunGroupId,
}

/// Why the shared checkout could not be resolved or created.
#[derive(Debug, thiserror::Error)]
pub enum SharedCheckoutError {
    /// The shared-checkout profile is not available on this platform.
    #[error("the shared-checkout profile is not available: {0}")]
    Unavailable(String),
    /// A durable store the resolution needs is not configured.
    #[error("the shared-checkout profile needs a durable {0} store")]
    MissingStore(&'static str),
    /// The named group does not exist.
    #[error("the agent-run group does not exist")]
    UnknownGroup,
    /// The named group has already finished.
    #[error("the agent-run group has already finished")]
    GroupNotLive,
    /// The run is not a member of the named group.
    #[error("the run is not a member of the agent-run group")]
    NotAMember,
    /// The group's checkout exists but cannot be used.
    #[error("the group's managed checkout is not usable: {0}")]
    CheckoutUnusable(String),
    /// The checkout could not be allocated.
    #[error("the shared checkout could not be allocated: {0}")]
    Allocation(String),
    /// A persistent process is live in the checkout, so the group cannot
    /// take it over.
    #[error("a persistent process is live in the shared checkout: {0}")]
    PersistentProcessPresent(String),
}

/// How the run came to hold the group's checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedCheckoutOutcome {
    /// The group already held a checkout and the run joined it.
    Joined(SharedCheckout),
    /// The run allocated the group's checkout and holds its lease.
    Created {
        /// The checkout the group now uses.
        checkout: SharedCheckout,
        /// The durable record of the checkout.
        ///
        /// Boxed: the record is far larger than the other variant, and the
        /// outcome is returned by value.
        record: Box<WorktreeRecord>,
        /// The lease the creating run holds, and must release.
        lease: Box<WorktreeLease>,
    },
}

impl SharedCheckoutOutcome {
    /// The checkout either way.
    pub fn checkout(&self) -> &SharedCheckout {
        match self {
            SharedCheckoutOutcome::Joined(checkout) => checkout,
            SharedCheckoutOutcome::Created { checkout, .. } => checkout,
        }
    }

    /// Whether the run allocated the checkout (and therefore holds its lease).
    pub fn is_creator(&self) -> bool {
        matches!(self, SharedCheckoutOutcome::Created { .. })
    }
}

/// What the coordinator needs to place a run on the group's checkout.
pub struct SharedCheckoutRequest {
    /// The group whose checkout is shared.
    pub group_id: AgentRunGroupId,
    /// The run that is joining the group.
    pub member_run_id: AgentRunId,
    /// How to allocate the checkout when the group does not hold one yet.
    pub allocation: CreateWorktreeRequest,
}

/// How long the coordinator waits for a group record to appear.
const GROUP_APPEAR_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// How often the coordinator re-reads the group record while waiting.
const GROUP_APPEAR_POLL: std::time::Duration = std::time::Duration::from_millis(25);

/// Resolves and creates the managed checkout a group shares.
///
/// The coordinator serializes on the group: two members of the same group
/// that reach it at the same time resolve the group's checkout under one
/// critical section, so exactly one of them allocates it. The lock is
/// process-local, which is sufficient because the daemon is a user-scoped
/// singleton; the durable anchor is the group's member records, which name
/// the checkout whoever holds it.
pub struct SharedCheckoutCoordinator {
    groups: Option<Arc<AgentRunGroupService>>,
    runs: Option<Arc<dyn AgentRunStore>>,
    worktrees: Option<Arc<WorktreeService>>,
    admission: Option<Arc<ProcessAdmissionService>>,
    group_locks: DashMap<AgentRunGroupId, Arc<AsyncMutex<()>>>,
}

impl SharedCheckoutCoordinator {
    /// Build the coordinator.
    ///
    /// Every store is optional because a local-only daemon has no durable
    /// group store. Without one there are no groups, so there is no shared
    /// checkout — which is the correct answer for a daemon that never shares
    /// a checkout.
    pub fn new(
        groups: Option<Arc<AgentRunGroupService>>,
        runs: Option<Arc<dyn AgentRunStore>>,
        worktrees: Option<Arc<WorktreeService>>,
        admission: Option<Arc<ProcessAdmissionService>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            groups,
            runs,
            worktrees,
            admission,
            group_locks: DashMap::new(),
        })
    }

    /// The group's checkout, when the group already holds one.
    ///
    /// Returns `Ok(None)` when the group is valid, live and holds the run as
    /// a member, but no member has a managed checkout yet. Every other
    /// outcome is an error: the caller must not treat an unknown group, a
    /// finished group or a run that is not a member as "no checkout".
    pub async fn resolve(
        &self,
        group_id: &AgentRunGroupId,
        member_run_id: &AgentRunId,
    ) -> Result<Option<SharedCheckout>, SharedCheckoutError> {
        let groups = self
            .groups
            .as_ref()
            .ok_or(SharedCheckoutError::MissingStore("agent-run group"))?;
        let group = groups
            .group_record(group_id)
            .await
            .map_err(|error| SharedCheckoutError::CheckoutUnusable(error.to_string()))?
            .ok_or(SharedCheckoutError::UnknownGroup)?;
        self.resolve_with_group(&group, member_run_id).await
    }

    /// The group's checkout, given the group record.
    async fn resolve_with_group(
        &self,
        group: &AgentRunGroupRecord,
        member_run_id: &AgentRunId,
    ) -> Result<Option<SharedCheckout>, SharedCheckoutError> {
        let runs = self
            .runs
            .as_ref()
            .ok_or(SharedCheckoutError::MissingStore("agent-run"))?;
        let worktrees = self
            .worktrees
            .as_ref()
            .ok_or(SharedCheckoutError::MissingStore("managed worktree"))?;

        if group.status.is_terminal() {
            return Err(SharedCheckoutError::GroupNotLive);
        }
        if !group.member_run_ids.contains(member_run_id) && &group.owner_run_id != member_run_id {
            return Err(SharedCheckoutError::NotAMember);
        }

        // The group's checkout is the managed checkout whose lease is held by
        // a member of the group. The lease owner is what makes the checkout
        // *this group's* checkout, so a checkout leased by an unrelated run
        // is never adopted.
        let mut candidates = group.member_run_ids.clone();
        if !candidates.contains(&group.owner_run_id) {
            candidates.push(group.owner_run_id.clone());
        }
        for member in candidates {
            let Ok(Some(run)) = runs.get_run(&member).await else {
                continue;
            };
            let Some(worktree_id) = run.worktree_id.clone() else {
                continue;
            };
            let Ok(record) = worktrees.get(&worktree_id).await else {
                continue;
            };
            let Some(owner) = record.owner_run_id.clone() else {
                continue;
            };
            if owner != group.owner_run_id && !group.member_run_ids.contains(&owner) {
                continue;
            }
            if !matches!(
                record.state,
                ManagedWorktreeState::Ready
                    | ManagedWorktreeState::Reserved
                    | ManagedWorktreeState::InUse
            ) {
                return Err(SharedCheckoutError::CheckoutUnusable(format!(
                    "the group's checkout is {}",
                    record.state.as_str()
                )));
            }
            if record.lease_generation == 0 {
                return Err(SharedCheckoutError::CheckoutUnusable(
                    "the group's checkout has no active lease".to_owned(),
                ));
            }
            return Ok(Some(SharedCheckout {
                worktree_id: record.worktree_id.clone(),
                path: record.path.clone(),
                lease_generation: record.lease_generation,
                owner_run_id: owner,
                group_id: group.group_id.clone(),
            }));
        }
        Ok(None)
    }

    /// Join the group's checkout, allocating it when the group has none.
    ///
    /// The whole decision is taken under the group's lock, so two members
    /// racing for the same group end up on one checkout: the first allocates
    /// it, the second joins it.
    pub async fn acquire(
        &self,
        request: SharedCheckoutRequest,
    ) -> Result<SharedCheckoutOutcome, SharedCheckoutError> {
        let lock = self
            .group_locks
            .entry(request.group_id.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let _guard: OwnedMutexGuard<()> = Arc::clone(&lock).lock_owned().await;

        // The task tool creates the group record *after* it has spawned the
        // children, so a child that executes immediately can arrive before
        // the group exists. The wait is bounded and the failure is a typed
        // refusal: a run never silently falls back to an isolated checkout
        // because the group was a moment late.
        let group = self.wait_for_group(&request.group_id).await?;

        if let Some(checkout) = self
            .resolve_with_group(&group, &request.member_run_id)
            .await?
        {
            self.preflight(&checkout).await?;
            return Ok(SharedCheckoutOutcome::Joined(checkout));
        }

        let worktrees = self
            .worktrees
            .as_ref()
            .ok_or(SharedCheckoutError::MissingStore("managed worktree"))?;
        let (record, lease) = worktrees
            .create(&request.allocation)
            .await
            .map_err(|error| SharedCheckoutError::Allocation(error.to_string()))?;
        let checkout = SharedCheckout {
            worktree_id: record.worktree_id.clone(),
            path: record.path.clone(),
            lease_generation: lease.generation,
            owner_run_id: lease.owner_run_id.clone(),
            group_id: request.group_id.clone(),
        };
        self.preflight(&checkout).await?;
        Ok(SharedCheckoutOutcome::Created {
            checkout,
            record: Box::new(record),
            lease: Box::new(lease),
        })
    }

    /// Wait, briefly, for the group's durable record to appear.
    ///
    /// The task tool creates the group record after it has spawned the
    /// children, because the group's membership is the children's durable run
    /// handles. A child that is scheduled immediately can therefore arrive
    /// just before the group exists. The wait is bounded and the failure is
    /// a typed refusal.
    async fn wait_for_group(
        &self,
        group_id: &AgentRunGroupId,
    ) -> Result<AgentRunGroupRecord, SharedCheckoutError> {
        let groups = self
            .groups
            .as_ref()
            .ok_or(SharedCheckoutError::MissingStore("agent-run group"))?;
        let deadline = std::time::Instant::now() + GROUP_APPEAR_WAIT;
        loop {
            match groups.group_record(group_id).await {
                Ok(Some(group)) => return Ok(group),
                Ok(None) if std::time::Instant::now() < deadline => {
                    tokio::time::sleep(GROUP_APPEAR_POLL).await;
                }
                Ok(None) => return Err(SharedCheckoutError::UnknownGroup),
                Err(error) => return Err(SharedCheckoutError::CheckoutUnusable(error.to_string())),
            }
        }
    }

    /// The reservation preflight a shared checkout must pass.
    ///
    /// A persistent process that was started before the checkout became
    /// shared keeps the authority it had, so the group cannot take the
    /// checkout over while one is live. The refusal never kills the process:
    /// its lifetime belongs to its own owner.
    async fn preflight(&self, checkout: &SharedCheckout) -> Result<(), SharedCheckoutError> {
        let Some(admission) = self.admission.as_ref() else {
            return Ok(());
        };
        let admission = admission
            .preflight_reservation(&checkout.worktree_id)
            .await
            .map_err(|error| SharedCheckoutError::CheckoutUnusable(error.to_string()))?;
        if admission.is_admitted() {
            return Ok(());
        }
        let denial = admission.denial().expect("a refusal carries its reason");
        Err(SharedCheckoutError::PersistentProcessPresent(
            denial.reason.as_str().to_owned(),
        ))
    }
}
