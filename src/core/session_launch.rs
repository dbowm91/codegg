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
use codegg_core::worktree_service::{CreateWorktreeRequest, WorktreeRecord, WorktreeService};

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
    pub async fn launch(
        &self,
        request: SessionLaunchRequest,
        context: &SessionLaunchContext,
    ) -> Result<SessionLaunchOutcome, SessionLaunchError> {
        let record = self.store.accept(&request, now_ms()).await?;
        if record.stage != SessionLaunchStage::Accepted {
            // The launch already started. Converge on what it produced.
            return self.outcome(&record, true).await;
        }

        let mut current = record;
        // ── The checkout ────────────────────────────────────────────────
        let worktree = match &current.placement {
            SessionLaunchPlacement::NewManagedCheckout { .. } => {
                let reserved = self.reserve_checkout(&current, context).await?;
                current = self
                    .store
                    .advance(
                        &current.launch_id,
                        SessionLaunchStage::CheckoutReserved,
                        SessionLaunchProgress {
                            worktree_id: Some(reserved.worktree_id.as_str().to_string()),
                            ..Default::default()
                        },
                        now_ms(),
                    )
                    .await?;
                let created = self
                    .worktrees
                    .create_reserved(&reserved.worktree_id)
                    .await
                    .map_err(|error| SessionLaunchError::Checkout(error.to_string()))?;
                current = self
                    .store
                    .advance(
                        &current.launch_id,
                        SessionLaunchStage::CheckoutReady,
                        SessionLaunchProgress {
                            worktree_id: Some(created.worktree_id.as_str().to_string()),
                            ..Default::default()
                        },
                        now_ms(),
                    )
                    .await?;
                created
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
                current = self
                    .store
                    .advance(
                        &current.launch_id,
                        SessionLaunchStage::CheckoutReserved,
                        SessionLaunchProgress {
                            worktree_id: Some(existing.worktree_id.as_str().to_string()),
                            ..Default::default()
                        },
                        now_ms(),
                    )
                    .await?;
                current = self
                    .store
                    .advance(
                        &current.launch_id,
                        SessionLaunchStage::CheckoutReady,
                        SessionLaunchProgress {
                            worktree_id: Some(existing.worktree_id.as_str().to_string()),
                            ..Default::default()
                        },
                        now_ms(),
                    )
                    .await?;
                existing
            }
        };

        // ── The workspace ───────────────────────────────────────────────
        // The session runs in the *checkout's* workspace, not the
        // project's. It is registered here and bound to the project so
        // the session's canonical binding resolves.
        let workspace = self
            .workspaces
            .get_or_register(&worktree.path)
            .await
            .map_err(|error| SessionLaunchError::Workspace(error.to_string()))?;
        self.storage
            .reconcile_workspace_path(&workspace, "session_launch")
            .await
            .map_err(|error| SessionLaunchError::Workspace(error.to_string()))?;
        self.storage
            .record_workspace_worktree(&workspace.id, &worktree.worktree_id)
            .await
            .map_err(|error| SessionLaunchError::Workspace(error.to_string()))?;

        // ── The session ─────────────────────────────────────────────────
        let session = match current.intent {
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
                let child = self
                    .sessions
                    .fork(parent)
                    .await
                    .map_err(|error| SessionLaunchError::Session(error.to_string()))?;
                // The fork inherits the parent's workspace. Move it into
                // the checkout before binding it, so the two agree.
                self.sessions
                    .rebind_workspace(&child.id, &workspace.id, &worktree.path.to_string_lossy())
                    .await
                    .map_err(|error| SessionLaunchError::Session(error.to_string()))?;
                child
            }
        };
        current = self
            .store
            .advance(
                &current.launch_id,
                SessionLaunchStage::SessionCreated,
                SessionLaunchProgress {
                    session_id: Some(session.id.clone()),
                    workspace_id: Some(workspace.id.as_str().to_string()),
                    worktree_id: Some(worktree.worktree_id.as_str().to_string()),
                    ..Default::default()
                },
                now_ms(),
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
        current = self
            .store
            .advance(
                &current.launch_id,
                SessionLaunchStage::Bound,
                SessionLaunchProgress::default(),
                now_ms(),
            )
            .await?;
        current = self
            .store
            .advance(
                &current.launch_id,
                SessionLaunchStage::Completed,
                SessionLaunchProgress::default(),
                now_ms(),
            )
            .await?;

        self.outcome(&current, false).await
    }

    /// The launches that are not finished, for startup reconciliation.
    pub async fn unfinished(&self) -> Result<Vec<SessionLaunchRecord>, SessionLaunchError> {
        self.store.reconcile().await
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
    /// The lease owner is a synthetic run identity: a user-launched
    /// session has no agent run behind it, but the worktree lease model
    /// is owned by one. The identity is derived from the launch id, so a
    /// retry converges on the same owner.
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

/// The synthetic lease owner for a user-launched checkout.
///
/// The worktree lease is owned by an agent run. A session the user
/// launched has no run behind it, so the launch mints one. It is derived
/// from the launch id, so a retry converges on the same owner rather
/// than orphaning the first lease.
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
