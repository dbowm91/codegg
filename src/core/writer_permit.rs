//! The daemon-side adapter for the M005 checkout writer permit (corrective C002).
//!
//! # What this layer is for
//!
//! [`codegg_core::writer_permit`] owns the permit's *authority*: the state
//! machine, the compare-and-swap, the quarantine transitions, and the durable
//! row. This module owns everything the store deliberately does not:
//!
//! - **authorization** — resolving the requester's identity against the
//!   worktree's authoritative record, so the scope a permit is bound to is
//!   never model-supplied;
//! - **waiting** — a bounded, per-checkout waiter that does not become a
//!   second admission queue;
//! - **lifecycle** — the scoped guard that makes "release with proven
//!   quiescence" the only way a permit is ever freed.
//!
//! # What this layer does not do
//!
//! It does not route any writer. C002 builds the authority; C003 (native file
//! and Git writers) and C004 (non-broker sinks) are what make writers consult
//! it. Nothing here enables shared mutation, and no user-visible
//! shared-mutation selector exists.
//!
//! # The holder contract
//!
//! A permit is held through a [`WriterPermitGuard`]. The guard is released
//! **explicitly**, with evidence:
//!
//! - [`WriterPermitGuard::release`] frees the permit when the caller can prove
//!   its effects settled.
//! - [`WriterPermitGuard::quarantine`] parks it when it cannot.
//! - **Dropping** the guard without settling it quarantines the permit. It
//!   never frees it. A forgotten or abandoned holder therefore leaves the
//!   checkout parked for an operator, rather than silently handing the
//!   checkout to the next writer while the previous writer's effects may still
//!   be landing.
//!
//! That is the whole point of the guard: a permit that is dropped mid-flight is
//! a checkout whose state is unknown, and "unknown" is not "free".

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use codegg_core::identity::{AgentRunGroupId, AgentRunId, PrincipalId, WorktreeId};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::writer_permit::{
    AcquireWriterPermit, BeginReleaseWriterPermit, CompleteReleaseWriterPermit,
    OperatorReleaseWriterPermit, QuarantineWriterPermit, QuiescenceEvidence, WriterPermitError,
    WriterPermitFilter, WriterPermitOperation, WriterPermitQuarantineReason, WriterPermitRecord,
    WriterPermitScope, WriterPermitStore, WriterPermitView,
};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

/// The bound on how many callers may wait for one checkout's permit.
///
/// The queue is per checkout, so a busy checkout cannot consume another's
/// capacity. The bound exists so a stalled checkout's waiters cannot grow
/// without limit.
pub const MAX_WRITER_PERMIT_WAITERS_PER_WORKTREE: usize = 64;

/// The default wait bound when a caller asks to wait without naming a timeout.
pub const DEFAULT_WRITER_PERMIT_WAIT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Authority
// ---------------------------------------------------------------------------

/// Why a permit request was refused before it reached the store.
#[derive(Debug, thiserror::Error)]
pub enum WriterPermitAuthorityError {
    #[error("worktree not found")]
    WorktreeNotFound,
    #[error("worktree is not in a writable state")]
    WorktreeUnavailable,
    #[error("worktree has no active lease")]
    NoActiveLease,
    #[error("the requesting run does not hold the checkout lease")]
    NotLeaseOwner,
    #[error("principal is not authorized to write in this project")]
    Unauthorized,
    #[error("agent run group reservation is not active")]
    GroupReservationInactive,
    #[error("authority lookup failed: {0}")]
    Lookup(String),
}

/// A request to resolve the authoritative scope of a permit.
#[derive(Debug, Clone)]
pub struct WriterPermitScopeRequest {
    pub worktree_id: WorktreeId,
    pub principal: AuthenticatedPrincipal,
    pub agent_run_id: Option<AgentRunId>,
    pub group_id: Option<AgentRunGroupId>,
    pub attempt_id: Option<String>,
}

/// Resolves the authoritative scope a permit is bound to.
///
/// Implementations MUST read the scope from daemon-owned stores. A scope
/// assembled from request payloads is not a scope, it is a suggestion.
#[async_trait::async_trait]
// Stable >=1.99 flags the `#[must_use]` injected by the async-trait expansion
// (the desugared future is already `must_use`). Suppressed at the macro site;
// no behavior change. Same convention as `crate::core::CoreClient`.
#[allow(clippy::double_must_use)]
pub trait WriterPermitAuthority: Send + Sync {
    async fn resolve_scope(
        &self,
        request: &WriterPermitScopeRequest,
    ) -> Result<WriterPermitScope, WriterPermitAuthorityError>;
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// A request to acquire a checkout's writer permit.
#[derive(Debug, Clone)]
pub struct WriterPermitRequest {
    pub worktree_id: WorktreeId,
    pub principal: AuthenticatedPrincipal,
    pub agent_run_id: Option<AgentRunId>,
    pub group_id: Option<AgentRunGroupId>,
    pub attempt_id: Option<String>,
    pub operation: WriterPermitOperation,
    /// The idempotency key of this acquisition attempt. A retry with the same
    /// key and the same payload returns the same permit; a retry with the same
    /// key and a different payload is refused.
    pub idempotency_key: String,
    /// How long to wait for a busy permit, and what cancels the wait.
    ///
    /// `None` means "do not wait": a busy permit is an immediate refusal.
    pub wait: Option<WriterPermitWait>,
}

/// The bound on one permit wait.
#[derive(Debug, Clone)]
pub struct WriterPermitWait {
    pub timeout: Duration,
    /// Cancelled when the caller goes away — a cancelled job, a closed
    /// session, a shut-down daemon.
    pub cancellation: CancellationToken,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// The daemon's writer permit service.
pub struct WriterPermitService {
    store: Arc<WriterPermitStore>,
    authority: Arc<dyn WriterPermitAuthority>,
    queues: Mutex<HashMap<String, WaitQueue>>,
}

/// The FIFO wait queue for one checkout.
#[derive(Default)]
struct WaitQueue {
    next_ticket: u64,
    waiters: VecDeque<(u64, Arc<Notify>)>,
}

impl WaitQueue {
    fn push(&mut self, notify: Arc<Notify>) -> u64 {
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.waiters.push_back((ticket, notify));
        ticket
    }

    fn is_head(&self, ticket: u64) -> bool {
        self.waiters
            .front()
            .map(|(head, _)| *head == ticket)
            .unwrap_or(false)
    }

    fn remove(&mut self, ticket: u64) -> Option<Arc<Notify>> {
        let index = self.waiters.iter().position(|(head, _)| *head == ticket)?;
        self.waiters.remove(index).map(|(_, notify)| notify)
    }

    /// The waiter that should be told the checkout's state changed.
    fn head_notify(&self) -> Option<Arc<Notify>> {
        self.waiters.front().map(|(_, notify)| notify.clone())
    }
}

impl WriterPermitService {
    pub fn new(
        store: Arc<WriterPermitStore>,
        authority: Arc<dyn WriterPermitAuthority>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            authority,
            queues: Mutex::new(HashMap::new()),
        })
    }

    pub fn store(&self) -> &Arc<WriterPermitStore> {
        &self.store
    }

    /// Acquire the checkout's writer permit.
    ///
    /// The returned guard holds the permit. It must be released explicitly with
    /// evidence; see the module docs.
    pub async fn acquire(
        self: &Arc<Self>,
        request: WriterPermitRequest,
    ) -> Result<WriterPermitGuard, WriterPermitError> {
        let scope = self
            .authority
            .resolve_scope(&WriterPermitScopeRequest {
                worktree_id: request.worktree_id.clone(),
                principal: request.principal.clone(),
                agent_run_id: request.agent_run_id.clone(),
                group_id: request.group_id.clone(),
                attempt_id: request.attempt_id.clone(),
            })
            .await
            .map_err(map_authority_error)?;

        // A caller whose wait is already cancelled gets no permit at all. The
        // check is before the store, so a cancelled caller never even takes
        // the token it would immediately have to hand back.
        if let Some(wait) = request.wait.as_ref() {
            if wait.cancellation.is_cancelled() {
                return Err(WriterPermitError::Cancelled);
            }
        }

        let acquire = AcquireWriterPermit {
            scope: scope.clone(),
            operation: request.operation,
            idempotency_key: request.idempotency_key.clone(),
            lease_expires_at_ms: None,
        };

        let record = match self.store.acquire(acquire.clone()).await {
            Ok(record) => record,
            Err(WriterPermitError::Held) => {
                let Some(wait) = request.wait.as_ref() else {
                    return Err(WriterPermitError::Held);
                };
                self.wait_for_permit(&request, &acquire, wait).await?
            }
            Err(error) => return Err(error),
        };

        // A wait that was cancelled while the permit was being taken must not
        // leave a token behind. The permit was taken but nothing was written,
        // so the honest evidence is "no effect".
        if let Some(wait) = request.wait.as_ref() {
            if wait.cancellation.is_cancelled() {
                let guard = WriterPermitGuard::new(self.clone(), record);
                let _ = guard.release(QuiescenceEvidence::NoEffect).await;
                return Err(WriterPermitError::Cancelled);
            }
        }

        Ok(WriterPermitGuard::new(self.clone(), record))
    }

    /// Wait for a busy permit, then take it.
    ///
    /// The wait is FIFO **per checkout**: the head of one checkout's queue is
    /// the only waiter allowed to attempt the compare-and-swap, so a waiter
    /// cannot be starved by later arrivals, and two checkouts never wait on
    /// each other.
    async fn wait_for_permit(
        self: &Arc<Self>,
        request: &WriterPermitRequest,
        acquire: &AcquireWriterPermit,
        wait: &WriterPermitWait,
    ) -> Result<WriterPermitRecord, WriterPermitError> {
        let key = request.worktree_id.to_string();
        let notify = Arc::new(Notify::new());
        let ticket = {
            let mut queues = self.queues.lock().await;
            let queue = queues.entry(key.clone()).or_default();
            if queue.waiters.len() >= MAX_WRITER_PERMIT_WAITERS_PER_WORKTREE {
                return Err(WriterPermitError::Capacity);
            }
            queue.push(notify.clone())
        };

        let deadline = tokio::time::Instant::now() + wait.timeout;
        let result = loop {
            let is_head = {
                let queues = self.queues.lock().await;
                queues
                    .get(&key)
                    .map(|queue| queue.is_head(ticket))
                    .unwrap_or(false)
            };

            if is_head {
                match self.store.acquire(acquire.clone()).await {
                    Ok(record) => break Ok(record),
                    Err(WriterPermitError::Held) => {}
                    Err(error) => break Err(error),
                }
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break Err(WriterPermitError::WaitTimeout);
            }
            tokio::select! {
                _ = notify.notified() => {}
                _ = tokio::time::sleep(remaining) => {
                    break Err(WriterPermitError::WaitTimeout);
                }
                _ = wait.cancellation.cancelled() => {
                    break Err(WriterPermitError::Cancelled);
                }
            }
        };

        // Leave the queue, and tell the next head that the checkout's state
        // may have changed.
        {
            let mut queues = self.queues.lock().await;
            if let Some(queue) = queues.get_mut(&key) {
                queue.remove(ticket);
                if let Some(next) = queue.head_notify() {
                    next.notify_one();
                }
            }
            if queues
                .get(&key)
                .map(|queue| queue.waiters.is_empty())
                .unwrap_or(false)
            {
                queues.remove(&key);
            }
        }

        result
    }

    /// Tell the checkout's head waiter that its state changed.
    async fn notify_waiters(&self, worktree_id: &WorktreeId) {
        let queues = self.queues.lock().await;
        if let Some(queue) = queues.get(&worktree_id.to_string()) {
            if let Some(notify) = queue.head_notify() {
                notify.notify_one();
            }
        }
    }

    /// The redacted operator view of one checkout's permit.
    pub async fn inspect(
        &self,
        worktree_id: &WorktreeId,
    ) -> Result<Option<WriterPermitView>, WriterPermitError> {
        Ok(self
            .store
            .load(worktree_id)
            .await?
            .map(|record| record.to_view()))
    }

    /// The redacted operator view of every permit matching a filter.
    pub async fn list(
        &self,
        filter: &WriterPermitFilter,
    ) -> Result<Vec<WriterPermitView>, WriterPermitError> {
        Ok(self
            .store
            .list(filter)
            .await?
            .iter()
            .map(WriterPermitRecord::to_view)
            .collect())
    }

    /// Free a quarantined permit.
    ///
    /// This never touches the checkout. Releasing a quarantined permit says
    /// "an operator looked at this checkout and decided it is safe to write
    /// again"; the files are exactly as the previous writer left them.
    pub async fn operator_release(
        &self,
        worktree_id: &WorktreeId,
        expected_revision: u64,
        reason: String,
        evidence: QuiescenceEvidence,
    ) -> Result<WriterPermitView, WriterPermitError> {
        let record = self
            .store
            .operator_release(OperatorReleaseWriterPermit {
                worktree_id: worktree_id.clone(),
                expected_revision,
                reason,
                evidence,
            })
            .await?;
        self.notify_waiters(worktree_id).await;
        Ok(record.to_view())
    }

    /// Park a permit on the operator's instruction.
    pub async fn operator_quarantine(
        &self,
        worktree_id: &WorktreeId,
        reason: WriterPermitQuarantineReason,
        detail: String,
    ) -> Result<WriterPermitView, WriterPermitError> {
        let record = self
            .store
            .quarantine(QuarantineWriterPermit {
                worktree_id: worktree_id.clone(),
                owner_token: None,
                expected_revision: None,
                reason,
                detail,
            })
            .await?;
        self.notify_waiters(worktree_id).await;
        Ok(record.to_view())
    }

    /// Reconcile permits after a daemon restart.
    ///
    /// Every permit that was live when the daemon went down is an orphan, and
    /// is quarantined. The holder's operation is never replayed: repairing
    /// metadata by re-running a mutation would be a second effect, not a
    /// repair.
    pub async fn reconcile_after_restart(
        &self,
    ) -> Result<Vec<WriterPermitView>, WriterPermitError> {
        let records = self.store.reconcile_after_restart().await?;
        for record in &records {
            self.notify_waiters(&record.worktree_id).await;
        }
        Ok(records.iter().map(WriterPermitRecord::to_view).collect())
    }
}

fn map_authority_error(error: WriterPermitAuthorityError) -> WriterPermitError {
    match error {
        WriterPermitAuthorityError::WorktreeNotFound => WriterPermitError::NotFound,
        WriterPermitAuthorityError::Unauthorized => WriterPermitError::ScopeMismatch,
        WriterPermitAuthorityError::NotLeaseOwner => WriterPermitError::NotOwner,
        WriterPermitAuthorityError::GroupReservationInactive => WriterPermitError::ScopeMismatch,
        WriterPermitAuthorityError::NoActiveLease
        | WriterPermitAuthorityError::WorktreeUnavailable
        | WriterPermitAuthorityError::Lookup(_) => WriterPermitError::InvalidScope,
    }
}

// ---------------------------------------------------------------------------
// Guard
// ---------------------------------------------------------------------------

/// A held checkout writer permit.
///
/// See the module docs for the release contract. The short version: release
/// it explicitly with evidence, and let dropping it quarantine rather than
/// free.
pub struct WriterPermitGuard {
    service: Arc<WriterPermitService>,
    record: WriterPermitRecord,
    settled: bool,
}

impl WriterPermitGuard {
    fn new(service: Arc<WriterPermitService>, record: WriterPermitRecord) -> Self {
        Self {
            service,
            record,
            settled: false,
        }
    }

    /// The permit's current record.
    pub fn record(&self) -> &WriterPermitRecord {
        &self.record
    }

    pub fn worktree_id(&self) -> &WorktreeId {
        &self.record.worktree_id
    }

    /// The non-duplicable possession token.
    pub fn owner_token(&self) -> &str {
        self.record
            .owner
            .as_ref()
            .map(|owner| owner.token.as_str())
            .unwrap_or_default()
    }

    /// Free the permit.
    ///
    /// `evidence` must be **positive** proof that the caller's effects
    /// settled. Unproven evidence does not free the permit — it quarantines
    /// it, and the returned record says so.
    pub async fn release(
        mut self,
        evidence: QuiescenceEvidence,
    ) -> Result<WriterPermitRecord, WriterPermitError> {
        let begun = self
            .service
            .store
            .begin_release(BeginReleaseWriterPermit {
                worktree_id: self.record.worktree_id.clone(),
                owner_token: self.owner_token().to_owned(),
                expected_revision: self.record.revision,
                expected_lease_generation: self.record.lease_generation,
            })
            .await?;
        let completed = self
            .service
            .store
            .complete_release(CompleteReleaseWriterPermit {
                worktree_id: begun.worktree_id.clone(),
                owner_token: self.owner_token().to_owned(),
                expected_revision: begun.revision,
                expected_lease_generation: begun.lease_generation,
                evidence,
                transfer: None,
            })
            .await;
        self.service.notify_waiters(&begun.worktree_id).await;
        // A failure on either leg leaves the permit unsettled, and an unsettled
        // guard is quarantined when it drops. That is the fail-safe direction:
        // a release that did not complete leaves the checkout's state unknown,
        // and unknown is not free.
        let record = completed?;
        self.settled = true;
        Ok(record)
    }

    /// Park the permit.
    pub async fn quarantine(
        mut self,
        reason: WriterPermitQuarantineReason,
        detail: String,
    ) -> Result<WriterPermitRecord, WriterPermitError> {
        let outcome = self
            .service
            .store
            .quarantine(QuarantineWriterPermit {
                worktree_id: self.record.worktree_id.clone(),
                owner_token: Some(self.owner_token().to_owned()),
                expected_revision: Some(self.record.revision),
                reason,
                detail,
            })
            .await;
        self.settled = true;
        self.service.notify_waiters(&self.record.worktree_id).await;
        outcome
    }
}

impl std::fmt::Debug for WriterPermitGuard {
    /// Redacted on purpose: the possession token is never printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriterPermitGuard")
            .field("worktree_id", &self.record.worktree_id.to_string())
            .field("state", &self.record.state.as_str())
            .field("revision", &self.record.revision)
            .field("settled", &self.settled)
            .finish()
    }
}

impl Drop for WriterPermitGuard {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // The guard was dropped without being settled. The checkout's state is
        // unknown, so the permit is parked rather than freed. `Drop` cannot
        // await, so the quarantine is spawned on the current runtime; if there
        // is no runtime, the permit stays held and the next daemon restart
        // reconciles it into a quarantine.
        let service = self.service.clone();
        let worktree_id = self.record.worktree_id.clone();
        let revision = self.record.revision;
        let owner_token = self.owner_token().to_owned();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            let _ = service
                .store
                .quarantine(QuarantineWriterPermit {
                    worktree_id: worktree_id.clone(),
                    owner_token: Some(owner_token),
                    expected_revision: Some(revision),
                    reason: WriterPermitQuarantineReason::IncompleteEffects,
                    detail: "the permit guard was dropped without an explicit release".to_owned(),
                })
                .await;
            service.notify_waiters(&worktree_id).await;
        });
    }
}

// ---------------------------------------------------------------------------
// Worktree-backed authority
// ---------------------------------------------------------------------------

/// The daemon's authority implementation, backed by the managed worktree
/// service and the team store.
///
/// The rules it enforces:
///
/// 1. The worktree must exist and be in a state that can be written.
/// 2. The worktree must have an active lease — a checkout nobody has leased
///    has no writer authority to hand out.
/// 3. The requesting run must be the run that **holds the lease**. A run that
///    does not own the checkout cannot write to it, and a run in another
///    project cannot own this checkout's lease, so a cross-project request
///    fails here rather than at the store.
/// 4. The principal must be authorized for the worktree's project. This is
///    checked through the daemon's authorization service when one is
///    configured; a daemon with no team store has no team policy to apply.
pub struct WorktreePermitAuthority {
    worktrees: Arc<codegg_core::worktree_service::WorktreeService>,
    groups: Option<Arc<codegg_core::agent_run_group::AgentRunGroupService>>,
    authorization: Option<Arc<codegg_core::authorization::AuthorizationService>>,
}

impl WorktreePermitAuthority {
    pub fn new(worktrees: Arc<codegg_core::worktree_service::WorktreeService>) -> Arc<Self> {
        Arc::new(Self {
            worktrees,
            groups: None,
            authorization: None,
        })
    }

    /// Whether two runs share a live group, which is what makes a checkout
    /// shared rather than private to its lease owner.
    async fn shares_live_group(
        &self,
        requesting: &codegg_core::identity::AgentRunId,
        owner: Option<&codegg_core::identity::AgentRunId>,
    ) -> Result<bool, WriterPermitAuthorityError> {
        let (Some(groups), Some(owner)) = (self.groups.as_ref(), owner) else {
            return Ok(false);
        };
        let member_groups = groups
            .groups_for_member(requesting)
            .await
            .map_err(|error| WriterPermitAuthorityError::Lookup(error.to_string()))?;
        let owner_groups = groups
            .groups_for_member(owner)
            .await
            .map_err(|error| WriterPermitAuthorityError::Lookup(error.to_string()))?;
        Ok(member_groups.iter().any(|member_group| {
            !codegg_core::agent_run_group::RunGroupStatus::is_terminal(member_group.status)
                && owner_groups
                    .iter()
                    .any(|owner_group| owner_group.group_id == member_group.group_id)
        }))
    }

    pub fn with_groups(
        self: Arc<Self>,
        groups: Arc<codegg_core::agent_run_group::AgentRunGroupService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            worktrees: self.worktrees.clone(),
            groups: Some(groups),
            authorization: self.authorization.clone(),
        })
    }

    pub fn with_authorization(
        self: Arc<Self>,
        authorization: Arc<codegg_core::authorization::AuthorizationService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            worktrees: self.worktrees.clone(),
            groups: self.groups.clone(),
            authorization: Some(authorization),
        })
    }
}

#[async_trait::async_trait]
impl WriterPermitAuthority for WorktreePermitAuthority {
    async fn resolve_scope(
        &self,
        request: &WriterPermitScopeRequest,
    ) -> Result<WriterPermitScope, WriterPermitAuthorityError> {
        let record =
            self.worktrees
                .get(&request.worktree_id)
                .await
                .map_err(|error| match error {
                    codegg_core::worktree_service::WorktreeServiceError::NotFound(_) => {
                        WriterPermitAuthorityError::WorktreeNotFound
                    }
                    other => WriterPermitAuthorityError::Lookup(other.to_string()),
                })?;

        if !matches!(
            record.state,
            codegg_core::worktree_service::ManagedWorktreeState::Ready
                | codegg_core::worktree_service::ManagedWorktreeState::Reserved
                | codegg_core::worktree_service::ManagedWorktreeState::InUse
        ) {
            return Err(WriterPermitAuthorityError::WorktreeUnavailable);
        }
        if record.lease_generation == 0 {
            return Err(WriterPermitAuthorityError::NoActiveLease);
        }

        // The run that holds the lease is the run that may write. A request
        // without a run has no lease to claim, and a request naming a
        // different run is not the owner.
        let Some(requesting_run) = request.agent_run_id.as_ref() else {
            return Err(WriterPermitAuthorityError::NotLeaseOwner);
        };
        if record.owner_run_id.as_ref() != Some(requesting_run) {
            // A checkout may be shared by a group of runs under a bounded
            // reservation: the lease owner is one writer, and a run that
            // shares a *live* group with the owner is the other. The permit
            // still records the individual run and the lease generation, so
            // this widens *who* may write a shared checkout, never *what* a
            // writer may do. Without the live group there is no
            // reservation, and the lease owner is the only writer.
            if !self
                .shares_live_group(requesting_run, record.owner_run_id.as_ref())
                .await?
            {
                return Err(WriterPermitAuthorityError::NotLeaseOwner);
            }
        }

        // The group reservation must still be live. A group that has finished
        // no longer covers this checkout, so its reservation is not a
        // justification to write.
        if let (Some(groups), Some(group_id)) = (self.groups.as_ref(), request.group_id.as_ref()) {
            let member_groups = groups
                .groups_for_member(requesting_run)
                .await
                .map_err(|error| WriterPermitAuthorityError::Lookup(error.to_string()))?;
            let covered = member_groups.iter().any(|group| {
                &group.group_id == group_id
                    && !codegg_core::agent_run_group::RunGroupStatus::is_terminal(group.status)
            });
            if !covered {
                return Err(WriterPermitAuthorityError::GroupReservationInactive);
            }
        }

        if let Some(authorization) = self.authorization.as_ref() {
            let descriptor = codegg_core::authorization::OperationDescriptor::new(
                "writer_permit.acquire",
                codegg_core::authorization::ScopeKind::DirectProject,
                Some(codegg_core::authorization::Capability::FileModify),
            );
            let authorization_request = codegg_core::authorization::AuthorizationRequest::new(
                request.principal.clone(),
                descriptor,
                Some(record.project_id.clone()),
                format!("writer-permit:{}", record.worktree_id),
            );
            authorization
                .authorize(&authorization_request)
                .await
                .map_err(|_| WriterPermitAuthorityError::Unauthorized)?;
        }

        Ok(WriterPermitScope {
            project_id: record.project_id.clone(),
            repository_id: Some(record.repository_id.clone()),
            workspace_id: record.workspace_id.clone(),
            worktree_id: record.worktree_id.clone(),
            lease_generation: record.lease_generation,
            group_id: request.group_id.clone(),
            principal_id: request.principal.principal_id().clone(),
            agent_run_id: request.agent_run_id.clone(),
            attempt_id: request.attempt_id.clone(),
        })
    }
}

/// The principal a request is made on behalf of, for daemons that resolve it
/// from a transport binding.
pub fn principal_id_of(principal: &AuthenticatedPrincipal) -> PrincipalId {
    principal.principal_id().clone()
}
