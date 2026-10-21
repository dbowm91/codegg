//! M006 — the daemon-side session launch service.
//!
//! The service drives a launch from an accepted request to a bound
//! session, and it drives it **recoverably**: every step is written to
//! the durable launch saga *before* the next step begins, so a launch
//! that dies half-way leaves a record of how far it got.
//!
//! # What the service owns
//!
//! * The **checkout**: a managed worktree, either freshly reserved and
//!   created, or an already-authorized managed checkout the caller names.
//! * The **workspace**: the checkout's own path, registered and bound to
//!   the project. A session in a checkout runs in the *checkout's*
//!   workspace, not in the project's.
//! * The **session**: created, or forked from a parent, and bound to that
//!   workspace.
//!
//! # What it deliberately does not own
//!
//! * **Authorization.** The caller authorizes the request; the service
//!   executes it. The service never decides whether a principal may
//!   launch.
//! * **Shared checkout execution.** M006 is about *isolated* checkouts.
//!   Nothing here enables a shared checkout.
//! * **Cleanup.** A launch that fails leaves its artefacts and a durable
//!   `needs_attention` record. Silently deleting a checkout or a
//!   session would destroy the evidence an operator needs.

use std::path::PathBuf;
use std::sync::Arc;

use codegg_core::identity::{AgentRunId, ProjectId, RepositoryId, WorktreeId};
use codegg_core::project_storage::ProjectStorage;
use codegg_core::session::SessionStore;
use codegg_core::session_launch::{
    SessionLaunchError, SessionLaunchIntent, SessionLaunchPlacement, SessionLaunchProgress,
    SessionLaunchRecord, SessionLaunchRequest, SessionLaunchStage, SqliteSessionLaunchStore,
};
use codegg_core::workspace::{WorkspaceId, WorkspaceRegistry};
use codegg_core::worktree_service::{
    CreateWorktreeRequest, ManagedWorktreeState, WorktreeRecord, WorktreeService,
};

/// The source of a launch, resolved by the daemon.
///
/// The client never supplies the repository root or the repository
/// identity: those are resolved from the project's own catalog, so a
/// caller cannot point a launch at an unrelated tree.
#[derive(Debug, Clone)]
pub struct SessionLaunchContext {
    pub project_id: ProjectId,
    pub repository_id: RepositoryId,
    /// The repository the checkout is created from.
    pub repository_root: PathBuf,
    /// The workspace the launch starts from.
    pub source_workspace_id: WorkspaceId,
}

/// What a launch produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLaunchOutcome {
    pub launch_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub worktree_id: String,
    pub branch: Option<String>,
    /// The checkout's path on disk.
    pub checkout_path: String,
    pub stage: SessionLaunchStage,
    /// True when this call converged on an already-recorded launch
    /// rather than doing the work again.
    pub converged: bool,
}

/// The daemon's session launch service.
pub struct SessionLaunchService {
    store: Arc<SqliteSessionLaunchStore>,
    worktrees: Arc<WorktreeService>,
    sessions: Arc<SessionStore>,
    storage: Arc<ProjectStorage>,
    workspaces: Arc<WorkspaceRegistry>,
    /// Serializes the launch work.
    ///
    /// The durable saga is what survives a *crash*; this is what
    /// survives two *callers*. Two simultaneous requests for the same
    /// idempotency key would otherwise both reserve a checkout —
    /// `WorktreeService::reserve` mints a fresh id per call — and the
    /// second one would fail its forward-only transition only *after* it
    /// had already created an orphan reservation.
    ///
    /// One lock for the whole service, not one per launch: the daemon is
    /// a user-scoped singleton and a launch is a user-initiated action
    /// that takes seconds, so serializing them costs nothing that
    /// matters and buys a guarantee with no per-launch bookkeeping to
    /// leak. The record is re-read under the lock, so the second caller
    /// converges on the first one's result rather than repeating it.
    launch_lock: tokio::sync::Mutex<()>,
}

impl SessionLaunchService {
    pub fn new(
        store: Arc<SqliteSessionLaunchStore>,
        worktrees: Arc<WorktreeService>,
        sessions: Arc<SessionStore>,
        storage: Arc<ProjectStorage>,
        workspaces: Arc<WorkspaceRegistry>,
    ) -> Self {
        Self {
            store,
            worktrees,
            sessions,
            storage,
            workspaces,
            launch_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// The durable launch store, for the operator surface.
    pub fn store(&self) -> &Arc<SqliteSessionLaunchStore> {
        &self.store
    }

    /// Launch a session into an isolated checkout.
    ///
    /// The request is accepted durably first, so a retry converges on the
    /// original launch rather than allocating a second checkout and a
    /// second session.
    ///
    /// A launch that fails leaves a **visible** record: the stage it
    /// reached, and the reason, in the durable saga. Nothing is cleaned
    /// up. A dirty checkout and a half-made session are the evidence an
    /// operator needs, and deleting them on the way out would destroy it.
    pub async fn launch(
        &self,
        request: SessionLaunchRequest,
        context: &SessionLaunchContext,
    ) -> Result<SessionLaunchOutcome, SessionLaunchError> {
        let record = self.store.accept(&request, now_ms()).await?;
        // The lock is taken *after* the accept, so the record exists for
        // both callers and the second one can converge on it.
        let _guard = self.launch_lock.lock().await;
        // Re-read under the lock: the other caller may have finished
        // this launch while this one waited for it.
        let record = match self.store.load(&record.launch_id).await? {
            Some(latest) => latest,
            None => record,
        };
        if record.stage == SessionLaunchStage::Completed {
            // The launch already finished. Converge on what it produced.
            return self.outcome(&record, true).await;
        }
        if record.stage == SessionLaunchStage::NeedsAttention {
            return Err(SessionLaunchError::InvalidRequest(format!(
                "launch {} needs attention and will not resume automatically",
                record.launch_id
            )));
        }

        let mut current = record;
        match self.run(&mut current, context).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                // Record the failure for the operator surface. The record
                // is best-effort: the original error is the one that
                // matters, and a failure to write the attention marker
                // must not replace it.
                let _ = self.mark_attention(&current, &error).await;
                Err(error)
            }
        }
    }

    /// Move a launch to `needs_attention`, with the failure's category.
    async fn mark_attention(
        &self,
        current: &SessionLaunchRecord,
        error: &SessionLaunchError,
    ) -> Result<(), SessionLaunchError> {
        self.store
            .advance(
                &current.launch_id,
                SessionLaunchStage::NeedsAttention,
                SessionLaunchProgress {
                    attention_code: Some(error.attention_code().to_string()),
                    ..Default::default()
                },
                now_ms(),
            )
            .await?;
        Ok(())
    }

    /// Advance the record, skipping a stage it has already passed.
    ///
    /// The store's transitions are forward-only, so a repeated or
    /// backwards advance is an error. A resumed launch is already at or
    /// past some stages, and re-recording one of them is not a contract
    /// violation — it is the resume working.
    async fn advance_to(
        &self,
        current: &mut SessionLaunchRecord,
        stage: SessionLaunchStage,
        progress: SessionLaunchProgress,
    ) -> Result<(), SessionLaunchError> {
        match (current.stage.rank(), stage.rank()) {
            (Some(have), Some(want)) if have >= want => return Ok(()),
            _ => {}
        }
        *current = self
            .store
            .advance(&current.launch_id, stage, progress, now_ms())
            .await?;
        Ok(())
    }

    /// Drive the launch from wherever it stopped to the end.
    ///
    /// Every step is written to the durable saga *before* the next step
    /// begins, so a launch that dies half-way leaves a record of how far
    /// it got — and a resume picks up from there.
    async fn run(
        &self,
        current: &mut SessionLaunchRecord,
        context: &SessionLaunchContext,
    ) -> Result<SessionLaunchOutcome, SessionLaunchError> {
        // ── The checkout ────────────────────────────────────────────────
        let worktree = match &current.placement {
            SessionLaunchPlacement::NewManagedCheckout { .. } => {
                let reserved =
                    match current.worktree_id.as_deref() {
                        // A resumed launch: the checkout was already
                        // reserved. Whether it was also *created* is a fact
                        // about the worktree record, not about the launch
                        // record — the two can disagree when a launch died
                        // between the two writes.
                        Some(worktree_id) => {
                            let id = WorktreeId::parse(worktree_id).map_err(|error| {
                                SessionLaunchError::InvalidRequest(format!(
                                    "invalid worktree id: {error}"
                                ))
                            })?;
                            let record =
                                self.worktrees.get(&id).await.map_err(|error| {
                                    SessionLaunchError::Checkout(error.to_string())
                                })?;
                            match record.state {
                                ManagedWorktreeState::Ready => record,
                                ManagedWorktreeState::Reserved => {
                                    self.worktrees
                                        .create_reserved(&record.worktree_id)
                                        .await
                                        .map_err(|error| {
                                            SessionLaunchError::Checkout(error.to_string())
                                        })?;
                                    self.worktrees.get(&record.worktree_id).await.map_err(
                                        |error| SessionLaunchError::Checkout(error.to_string()),
                                    )?
                                }
                                ref other => {
                                    return Err(SessionLaunchError::Checkout(format!(
                                        "the reserved checkout is {}",
                                        other.as_str()
                                    )))
                                }
                            }
                        }
                        None => {
                            let reserved = self.reserve_checkout(current, context).await?;
                            self.advance_to(
                                current,
                                SessionLaunchStage::CheckoutReserved,
                                SessionLaunchProgress {
                                    worktree_id: Some(reserved.worktree_id.as_str().to_string()),
                                    ..Default::default()
                                },
                            )
                            .await?;
                            self.worktrees
                                .create_reserved(&reserved.worktree_id)
                                .await
                                .map_err(|error| SessionLaunchError::Checkout(error.to_string()))?;
                            self.worktrees
                                .get(&reserved.worktree_id)
                                .await
                                .map_err(|error| SessionLaunchError::Checkout(error.to_string()))?
                        }
                    };
                self.advance_to(
                    current,
                    SessionLaunchStage::CheckoutReady,
                    SessionLaunchProgress {
                        worktree_id: Some(reserved.worktree_id.as_str().to_string()),
                        ..Default::default()
                    },
                )
                .await?;
                reserved
            }
            SessionLaunchPlacement::ExistingManagedWorktree { worktree_id } => {
                let id = WorktreeId::parse(worktree_id).map_err(|error| {
                    SessionLaunchError::InvalidRequest(format!("invalid worktree id: {error}"))
                })?;
                let existing = self
                    .worktrees
                    .get(&id)
                    .await
                    .map_err(|error| SessionLaunchError::Checkout(error.to_string()))?;
                if !existing.managed {
                    return Err(SessionLaunchError::InvalidRequest(
                        "an external checkout has no durable identity to bind a session to"
                            .to_string(),
                    ));
                }
                if existing.project_id != context.project_id {
                    return Err(SessionLaunchError::InvalidRequest(
                        "the named checkout belongs to another project".to_string(),
                    ));
                }
                if existing.repository_id != context.repository_id {
                    return Err(SessionLaunchError::InvalidRequest(
                        "the named checkout belongs to another repository".to_string(),
                    ));
                }
                // A checkout another run currently holds is a *shared*
                // working tree. M006 places a session in an **isolated**
                // checkout; letting a second session into a checkout that
                // is in use would be shared-checkout execution, which this
                // milestone does not enable.
                //
                // This check comes before the readiness check, because a
                // held checkout is in `in_use` and would otherwise be
                // refused for the wrong reason.
                if existing.owner_run_id.is_some() {
                    return Err(SessionLaunchError::InvalidRequest(format!(
                        "the named checkout is in use by another run ({})",
                        existing.worktree_id
                    )));
                }
                // A checkout that is not `ready` is not a place a session
                // can go: `reserved` means the identity was taken but the
                // directory was never created, and `orphaned` means
                // something went wrong. Both are for an operator, not
                // for a session.
                if existing.state != ManagedWorktreeState::Ready {
                    return Err(SessionLaunchError::InvalidRequest(format!(
                        "the named checkout is {} and is not ready",
                        existing.state.as_str()
                    )));
                }
                self.advance_to(
                    current,
                    SessionLaunchStage::CheckoutReserved,
                    SessionLaunchProgress {
                        worktree_id: Some(existing.worktree_id.as_str().to_string()),
                        ..Default::default()
                    },
                )
                .await?;
                self.advance_to(
                    current,
                    SessionLaunchStage::CheckoutReady,
                    SessionLaunchProgress {
                        worktree_id: Some(existing.worktree_id.as_str().to_string()),
                        ..Default::default()
                    },
                )
                .await?;
                existing
            }
        };

        // ── The workspace ───────────────────────────────────────────────
        // The session runs in the *checkout's* workspace, not the
        // project's. It is registered here and bound to the launch's
        // project, so the session's canonical binding resolves against
        // the project the caller named.
        //
        // The binding is explicit rather than auto-provisioned: the
        // catalog gives every previously unseen root its own logical
        // project, and a checkout is not a new project — it is a second
        // working directory of the project the launch names.
        let workspace = self
            .workspaces
            .get_or_register(&worktree.path)
            .await
            .map_err(|error| SessionLaunchError::Workspace(error.to_string()))?;
        self.storage
            .bind_checkout_workspace(
                &workspace,
                &context.project_id,
                &context.repository_id,
                &worktree.worktree_id,
                "session_launch",
            )
            .await
            .map_err(|error| SessionLaunchError::Workspace(error.to_string()))?;

        // ── The session ─────────────────────────────────────────────────
        let session = match current.session_id.as_deref() {
            // A resumed launch: the session already exists. It is loaded,
            // not created, so a resume does not produce a second session.
            Some(session_id) => self
                .sessions
                .get(session_id)
                .await
                .map_err(|error| SessionLaunchError::Session(error.to_string()))?
                .ok_or_else(|| {
                    SessionLaunchError::Session(format!(
                        "the recorded session {session_id} is gone"
                    ))
                })?,
            None => match current.intent {
                SessionLaunchIntent::New => self
                    .sessions
                    .create_with_binding(
                        codegg_core::session::CreateSession {
                            project_id: context.project_id.as_str().to_string(),
                            directory: worktree.path.to_string_lossy().into_owned(),
                            title: None,
                            parent_id: None,
                            workspace_id: Some(workspace.id.as_str().to_string()),
                            ..Default::default()
                        },
                        &context.project_id,
                        &workspace.id,
                        "session_launch",
                    )
                    .await
                    .map_err(|error| SessionLaunchError::Session(error.to_string()))?,
                SessionLaunchIntent::Fork => {
                    let parent = current.parent_session_id.as_deref().ok_or_else(|| {
                        SessionLaunchError::InvalidRequest(
                            "a fork launch requires a parent session".to_string(),
                        )
                    })?;
                    // The child's id is *derived* from the launch, so a
                    // resume converges on the child the first attempt
                    // made instead of forking a second one.
                    let child_id = format!("{}-fork", current.launch_id);
                    let child = self
                        .sessions
                        .fork_as(parent, &child_id)
                        .await
                        .map_err(|error| SessionLaunchError::Session(error.to_string()))?;
                    // The fork inherits the parent's workspace and
                    // project. Move both into the checkout before binding
                    // it, so the session row and the canonical binding
                    // agree.
                    self.sessions
                        .rebind_workspace(
                            &child.id,
                            &context.project_id,
                            &workspace.id,
                            &worktree.path.to_string_lossy(),
                        )
                        .await
                        .map_err(|error| SessionLaunchError::Session(error.to_string()))?;
                    child
                }
            },
        };
        self.advance_to(
            current,
            SessionLaunchStage::SessionCreated,
            SessionLaunchProgress {
                session_id: Some(session.id.clone()),
                workspace_id: Some(workspace.id.as_str().to_string()),
                worktree_id: Some(worktree.worktree_id.as_str().to_string()),
                ..Default::default()
            },
        )
        .await?;

        // ── The binding ────────────────────────────────────────────────
        self.storage
            .bind_session(
                &session.id,
                &context.project_id,
                &workspace.id,
                "session_launch",
            )
            .await
            .map_err(|error| SessionLaunchError::Workspace(error.to_string()))?;
        self.advance_to(
            current,
            SessionLaunchStage::Bound,
            SessionLaunchProgress::default(),
        )
        .await?;
        self.advance_to(
            current,
            SessionLaunchStage::Completed,
            SessionLaunchProgress::default(),
        )
        .await?;

        self.outcome(current, false).await
    }

    /// The launches that are not finished, for startup reconciliation.
    ///
    /// A launch that failed and moved to `needs_attention` is **terminal**,
    /// so it is not in this list: it is waiting for an operator, not for a
    /// resume. [`Self::list`] is the surface that finds it.
    pub async fn unfinished(&self) -> Result<Vec<SessionLaunchRecord>, SessionLaunchError> {
        self.store.reconcile().await
    }

    /// The launches recorded for a project, newest first.
    ///
    /// The operator surface: it includes the launches that failed and are
    /// waiting for attention, which `reconcile` deliberately leaves out.
    pub async fn list(
        &self,
        project_id: &str,
        limit: usize,
    ) -> Result<Vec<SessionLaunchRecord>, SessionLaunchError> {
        self.store.list(project_id, limit).await
    }

    /// Load one launch.
    pub async fn load(
        &self,
        launch_id: &str,
    ) -> Result<Option<SessionLaunchRecord>, SessionLaunchError> {
        self.store.load(launch_id).await
    }

    /// Reserve a managed checkout for the launch.
    ///
    /// The request carries a synthetic owner identity, derived from the
    /// launch id so a retry converges on the same one. The launch does
    /// **not** take the lease with it: the lease is what fences writers
    /// (ADR-0042), and the writer that will actually work in this
    /// checkout is the session's own agent run. A lease held by a
    /// synthetic owner would fence that run out — `acquire_lease` refuses
    /// a checkout that already has an owner — and nothing would ever
    /// release it, because a synthetic owner has no lifetime.
    ///
    /// The checkout therefore stays `ready` and unleased, and the run that
    /// works in it takes the lease when it needs one.
    async fn reserve_checkout(
        &self,
        record: &SessionLaunchRecord,
        context: &SessionLaunchContext,
    ) -> Result<WorktreeRecord, SessionLaunchError> {
        let (base_ref, target_branch) = match &record.placement {
            SessionLaunchPlacement::NewManagedCheckout {
                base_ref,
                target_branch,
            } => (base_ref.clone(), target_branch.clone()),
            SessionLaunchPlacement::ExistingManagedWorktree { .. } => (None, None),
        };
        let owner_run_id = synthetic_owner_run_id(&record.launch_id);
        let request = CreateWorktreeRequest {
            project_id: context.project_id.clone(),
            repository_id: context.repository_id.clone(),
            workspace_id: context.source_workspace_id.clone(),
            node_id: None,
            repository_root: context.repository_root.clone(),
            base_commit: None,
            base_ref,
            target_branch,
            base_path: None,
            owner_run_id,
        };
        self.worktrees
            .reserve(&request)
            .await
            .map_err(|error| SessionLaunchError::Checkout(error.to_string()))
    }

    /// Turn a durable record into the launch's outcome.
    async fn outcome(
        &self,
        record: &SessionLaunchRecord,
        converged: bool,
    ) -> Result<SessionLaunchOutcome, SessionLaunchError> {
        let session_id = record.session_id.clone().ok_or_else(|| {
            SessionLaunchError::InvalidRequest(format!(
                "launch {} has not created a session yet",
                record.launch_id
            ))
        })?;
        let workspace_id = record.workspace_id.clone().ok_or_else(|| {
            SessionLaunchError::InvalidRequest(format!(
                "launch {} has not registered a workspace yet",
                record.launch_id
            ))
        })?;
        let worktree_id = record.worktree_id.clone().ok_or_else(|| {
            SessionLaunchError::InvalidRequest(format!(
                "launch {} has not reserved a checkout yet",
                record.launch_id
            ))
        })?;
        let worktree = self
            .worktrees
            .get(&WorktreeId::parse(&worktree_id).map_err(|error| {
                SessionLaunchError::InvalidRequest(format!("invalid worktree id: {error}"))
            })?)
            .await
            .map_err(|error| SessionLaunchError::Checkout(error.to_string()))?;
        Ok(SessionLaunchOutcome {
            launch_id: record.launch_id.clone(),
            session_id,
            workspace_id,
            worktree_id,
            branch: worktree.branch.clone(),
            checkout_path: worktree.path.to_string_lossy().into_owned(),
            stage: record.stage,
            converged,
        })
    }
}

/// The synthetic owner identity a user-launched checkout is reserved
/// under.
///
/// The worktree lease is owned by an agent run, and a session the user
/// launched has no run behind it. The identity is derived from the
/// launch id, so a retry reserves under the same owner rather than
/// stranding the first reservation.
///
/// The launch does **not** take the lease with this identity. The lease
/// is what fences writers, and the writer that works in this checkout is
/// the session's own agent run — a lease held by a synthetic owner would
/// fence that run out (`acquire_lease` refuses a checkout that already
/// has an owner) and nothing would ever release it, because a synthetic
/// owner has no lifetime. The checkout stays `ready` and unleased until
/// a run takes it.
fn synthetic_owner_run_id(launch_id: &str) -> AgentRunId {
    let short: String = launch_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect();
    AgentRunId::parse(&format!("launch-run-{short}")).unwrap_or_else(|_| AgentRunId::new())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
