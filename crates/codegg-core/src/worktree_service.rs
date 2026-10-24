//! Durable daemon-owned worktree records and leases.
//!
//! This module owns the lifecycle metadata for CodeGG-created worktrees.  It
//! deliberately does not become a second Git implementation: discovery and
//! status come from `egggit`, while create/remove use the hardened helpers in
//! [`crate::worktree`].  The service is safe to use from daemon code because
//! blocking Git mutations are isolated in `spawn_blocking` and repository
//! contention is serialized by a workspace lock.

use async_trait::async_trait;
use chrono::Utc;
use codegg_git::{BranchName, ObjectId};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::identity::{AgentRunId, NodeId, ProjectId, RepositoryId, WorktreeId};
use crate::workspace::WorkspaceId;
use crate::workspace_services::WorkspaceLockTable;

/// Scheduler resource key required by worktree/repository metadata mutations.
/// The scheduler's Git mutation profile already reserves this key.
pub const WORKTREE_MUTATION_EXCLUSIVITY_KEY: &str = "exclusive:worktree-mutation";
pub const DEFAULT_WORKTREE_DIRECTORY: &str = "worktrees";
/// The bound on a **display** list of managed worktrees.
///
/// M005 corrective C008: this is a presentation bound, not an authority
/// bound. The checkout reservation authority must be able to find a managed
/// checkout whatever the size of the user's catalog, so it never reads
/// through this surface — it uses
/// [`WorktreeStore::list_authority_candidates`], which has no `LIMIT`.
pub const MAX_WORKTREE_LIST: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedWorktreeState {
    Reserved,
    Preparing,
    Ready,
    InUse,
    Releasing,
    Archived,
    Orphaned,
    Removed,
}

impl ManagedWorktreeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Preparing => "preparing",
            Self::Ready => "ready",
            Self::InUse => "in_use",
            Self::Releasing => "releasing",
            Self::Archived => "archived",
            Self::Orphaned => "orphaned",
            Self::Removed => "removed",
        }
    }

    pub fn parse_state(value: &str) -> Result<Self, WorktreeServiceError> {
        match value {
            "reserved" => Ok(Self::Reserved),
            "preparing" => Ok(Self::Preparing),
            "ready" => Ok(Self::Ready),
            "in_use" => Ok(Self::InUse),
            "releasing" => Ok(Self::Releasing),
            "archived" => Ok(Self::Archived),
            "orphaned" => Ok(Self::Orphaned),
            "removed" => Ok(Self::Removed),
            other => Err(WorktreeServiceError::Storage(format!(
                "unknown managed worktree state: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeHealth {
    Clean,
    Dirty,
    Conflicted,
    Missing,
    GitError,
    Unknown,
}

impl WorktreeHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::Conflicted => "conflicted",
            Self::Missing => "missing",
            Self::GitError => "git_error",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse_health(value: &str) -> Result<Self, WorktreeServiceError> {
        match value {
            "clean" => Ok(Self::Clean),
            "dirty" => Ok(Self::Dirty),
            "conflicted" => Ok(Self::Conflicted),
            "missing" => Ok(Self::Missing),
            "git_error" => Ok(Self::GitError),
            "unknown" => Ok(Self::Unknown),
            other => Err(WorktreeServiceError::Storage(format!(
                "unknown worktree health: {other}"
            ))),
        }
    }

    pub fn cleanup_safe(self) -> bool {
        matches!(self, Self::Clean)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeRecord {
    pub worktree_id: WorktreeId,
    pub project_id: ProjectId,
    pub repository_id: RepositoryId,
    pub workspace_id: WorkspaceId,
    pub node_id: Option<NodeId>,
    pub repository_root: PathBuf,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub base_commit: String,
    pub managed: bool,
    pub state: ManagedWorktreeState,
    pub health: WorktreeHealth,
    pub lease_generation: u64,
    pub owner_run_id: Option<AgentRunId>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeLease {
    pub worktree_id: WorktreeId,
    pub owner_run_id: AgentRunId,
    pub generation: u64,
    pub acquired_at: i64,
    pub renewed_at: i64,
    pub released_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CreateWorktreeRequest {
    pub project_id: ProjectId,
    pub repository_id: RepositoryId,
    pub workspace_id: WorkspaceId,
    pub node_id: Option<NodeId>,
    pub repository_root: PathBuf,
    /// Optional explicit base. When omitted, the current HEAD of the
    /// supplied worktree is used, which is the nested-run continuation rule.
    pub base_commit: Option<String>,
    /// M006: an optional base **ref** — a branch or tag name — resolved to
    /// a commit when the checkout is reserved.
    ///
    /// A ref, not a revision expression: `..`, `~`, `^`, `:` and `@{` are
    /// refused. The resolved commit is what the record stores, so the
    /// base is an immutable object id, never a moving name.
    pub base_ref: Option<String>,
    /// M006: an optional target branch for the new checkout.
    ///
    /// When omitted the worktree service mints `codegg/worktree/<id>`.
    /// The name is validated as a Git branch name before it is used, and
    /// it is rejected when the repository already has that branch.
    pub target_branch: Option<String>,
    /// Optional checkout whose effective HEAD should seed the new worktree.
    pub base_path: Option<PathBuf>,
    pub owner_run_id: AgentRunId,
}

#[derive(Debug, Clone, Default)]
pub struct WorktreeQuery {
    pub workspace_id: Option<WorkspaceId>,
    pub repository_id: Option<RepositoryId>,
    pub owner_run_id: Option<AgentRunId>,
    pub include_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub managed: bool,
    pub worktree_id: Option<WorktreeId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub inspected: usize,
    pub ready: Vec<WorktreeId>,
    pub retained_active: Vec<WorktreeId>,
    pub orphaned: Vec<WorktreeId>,
    pub released_terminal: Vec<WorktreeId>,
    pub external: Vec<PathBuf>,
}

#[derive(Debug, Error)]
pub enum WorktreeServiceError {
    #[error("worktree '{0}' not found")]
    NotFound(String),
    #[error("worktree lease conflict: {0}")]
    LeaseConflict(String),
    #[error("stale worktree lease generation: expected {expected}, actual {actual}")]
    StaleGeneration { expected: u64, actual: u64 },
    #[error("worktree collision: {0}")]
    Collision(String),
    #[error("unsafe worktree cleanup: {0}")]
    UnsafeCleanup(String),
    #[error("invalid worktree input: {0}")]
    InvalidInput(String),
    #[error("invalid worktree state transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: ManagedWorktreeState,
        to: ManagedWorktreeState,
    },
    #[error("git worktree operation failed: {0}")]
    Git(String),
    #[error("storage failure: {0}")]
    Storage(String),
}

fn transition_allowed(from: ManagedWorktreeState, to: ManagedWorktreeState) -> bool {
    matches!(
        (from, to),
        (
            ManagedWorktreeState::Reserved,
            ManagedWorktreeState::Preparing | ManagedWorktreeState::Orphaned
        ) | (
            ManagedWorktreeState::Preparing,
            ManagedWorktreeState::Ready | ManagedWorktreeState::Orphaned
        ) | (
            ManagedWorktreeState::Ready,
            ManagedWorktreeState::InUse
                | ManagedWorktreeState::Releasing
                | ManagedWorktreeState::Orphaned
        ) | (
            ManagedWorktreeState::InUse,
            ManagedWorktreeState::Ready
                | ManagedWorktreeState::Releasing
                | ManagedWorktreeState::Orphaned
        ) | (
            ManagedWorktreeState::Releasing,
            ManagedWorktreeState::Archived
                | ManagedWorktreeState::Removed
                | ManagedWorktreeState::Orphaned
        ) | (
            ManagedWorktreeState::Archived,
            ManagedWorktreeState::Removed | ManagedWorktreeState::Orphaned
        ) | (
            ManagedWorktreeState::Orphaned,
            ManagedWorktreeState::Ready
                | ManagedWorktreeState::Releasing
                | ManagedWorktreeState::Removed
        )
    )
}

fn transition(
    record: &mut WorktreeRecord,
    to: ManagedWorktreeState,
) -> Result<(), WorktreeServiceError> {
    if record.state == to {
        return Ok(());
    }
    if !transition_allowed(record.state, to) {
        return Err(WorktreeServiceError::InvalidTransition {
            from: record.state,
            to,
        });
    }
    record.state = to;
    record.updated_at = Utc::now().timestamp_millis();
    Ok(())
}

#[async_trait]
// Stable >=1.99 flags the #[must_use] injected by the async-trait expansion
// (the desugared future is already must_use). Suppressed at the macro site;
// no behavior change.
#[allow(clippy::double_must_use)]
pub trait WorktreeStore: Send + Sync {
    async fn insert_reserved(&self, record: WorktreeRecord) -> Result<(), WorktreeServiceError>;
    async fn get(&self, id: &WorktreeId) -> Result<Option<WorktreeRecord>, WorktreeServiceError>;
    async fn list(
        &self,
        query: &WorktreeQuery,
    ) -> Result<Vec<WorktreeRecord>, WorktreeServiceError>;
    /// M005 corrective C008: the **unbounded authority read**.
    ///
    /// The checkout reservation authority resolves the managed checkout that
    /// contains a path by canonical containment, and it must find that
    /// checkout whatever the size of the user's catalog. The `list` surface
    /// above is capped (`MAX_WORKTREE_LIST`, oldest-first), so a catalog with
    /// more non-removed rows than the cap hides the *newest* rows — and a
    /// newly created, currently reserved checkout is exactly one of them. A
    /// cap must never be able to hide a reserved checkout, so the authority
    /// reads through this method instead: every non-removed row, no
    /// `LIMIT`.
    ///
    /// It exists for that one caller. A caller that wants a bounded list for
    /// display uses `list`.
    async fn list_authority_candidates(&self) -> Result<Vec<WorktreeRecord>, WorktreeServiceError>;
    async fn save(&self, record: WorktreeRecord) -> Result<(), WorktreeServiceError>;
    async fn acquire_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
    ) -> Result<WorktreeLease, WorktreeServiceError>;
    async fn renew_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
        generation: u64,
    ) -> Result<WorktreeLease, WorktreeServiceError>;
    async fn release_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
        generation: u64,
    ) -> Result<WorktreeRecord, WorktreeServiceError>;
}

/// Narrow restart-reconciliation seam. The core worktree domain does not
/// depend on the agent runtime; the daemon supplies this adapter from its
/// durable run store.
#[async_trait]
// Stable >=1.99 flags the #[must_use] injected by the async-trait expansion
// (the desugared future is already must_use). Suppressed at the macro site;
// no behavior change.
#[allow(clippy::double_must_use)]
pub trait WorktreeOwnerResolver: Send + Sync {
    async fn is_terminal(&self, owner: &AgentRunId) -> Result<bool, String>;
}

/// Default bound for one contained worktree creation.
///
/// A contained child launches a sandbox, a namespace setup and a Git checkout.
/// The bound is generous enough for a large checkout on a cold cache and
/// short enough that a stuck child cannot hold the repository lock open
/// indefinitely.
pub const DEFAULT_CONTAINED_WORKTREE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(120);

/// What one contained worktree creation is asked to produce.
///
/// ADR-0048 (`plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md`)
/// grants exactly one operation shape: `git worktree add <path>
/// <existing-branch>` inside an OS-contained child. The **branch belongs to
/// the daemon** — the creator must create it before the child runs, so the
/// child attaches to a branch that already exists and writes no ref. The
/// grant is what enforces that, not an intention: the child's writable set
/// excludes the repository's refs, so a child that tried to create a branch
/// would fail closed.
#[derive(Debug, Clone)]
pub struct ContainedWorktreeRequest {
    /// The source repository's working tree — the directory the daemon's own
    /// typed Git path operates in.
    pub repository_root: PathBuf,
    /// The repository's **Git directory** — where the objects, refs, index and
    /// the `worktrees/` administration directory live. The source working
    /// tree is deliberately not named in the child's grant: a contained child
    /// that creates a linked worktree does not need it, and not granting it
    /// keeps the grant as narrow as the operation admits.
    pub repository_gitdir: PathBuf,
    /// The checkout to create. It must not exist yet.
    pub worktree_path: PathBuf,
    /// The branch the checkout must be attached to. The creator creates it
    /// before the contained child runs.
    pub branch: String,
    /// The immutable base commit the branch is created from.
    pub base_commit: String,
    /// The bound for the contained child.
    pub timeout: std::time::Duration,
}

/// What one contained worktree creation produced.
///
/// The outcome reports the containment the launch **obtained**, not the
/// containment it requested: a request is a hope, and only the launch's own
/// fact is evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainedWorktreeOutcome {
    /// The administration directory the child wrote, inside the repository's
    /// `worktrees/` directory.
    pub administration_dir: PathBuf,
    /// The sandbox backend that reported obtained enforcement.
    pub backend: String,
    /// The backend's reported ABI or version, when it has one.
    pub abi: Option<u32>,
    /// Whether the launch obtained the offline-network boundary.
    pub network_enforced: bool,
}

/// The creation seam for a managed worktree.
///
/// `codegg-core` owns the worktree record, its states and its lease, but it
/// must not own process spawning or the sandbox. The daemon supplies this
/// adapter; the core service prefers it over the in-process Git path when it
/// is configured.
///
/// A configured creator is **authoritative**: when it fails, the failure is
/// final. There is no fallback to an uncontained creation, because an
/// uncontained creation would be a weaker guarantee presented as a stronger
/// one.
#[async_trait]
#[allow(clippy::double_must_use)]
pub trait ContainedWorktreeCreator: Send + Sync {
    /// Create the checkout and return what the launch obtained.
    ///
    /// Implementations must fail — not degrade — when the containment cannot
    /// be obtained or the grant cannot be expressed.
    async fn create(
        &self,
        request: &ContainedWorktreeRequest,
    ) -> Result<ContainedWorktreeOutcome, WorktreeServiceError>;
}

#[derive(Default)]
struct MemoryWorktreeState {
    records: HashMap<WorktreeId, WorktreeRecord>,
    leases: Vec<WorktreeLease>,
}

#[derive(Default)]
pub struct InMemoryWorktreeStore {
    state: Mutex<MemoryWorktreeState>,
}

impl InMemoryWorktreeStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WorktreeStore for InMemoryWorktreeStore {
    async fn insert_reserved(&self, record: WorktreeRecord) -> Result<(), WorktreeServiceError> {
        let mut state = self.state.lock().await;
        if state.records.values().any(|existing| {
            existing.repository_id == record.repository_id && existing.path == record.path
        }) {
            return Err(WorktreeServiceError::Collision(
                record.path.display().to_string(),
            ));
        }
        if state.records.contains_key(&record.worktree_id) {
            return Err(WorktreeServiceError::Collision(
                record.worktree_id.to_string(),
            ));
        }
        state.records.insert(record.worktree_id.clone(), record);
        Ok(())
    }

    async fn get(&self, id: &WorktreeId) -> Result<Option<WorktreeRecord>, WorktreeServiceError> {
        Ok(self.state.lock().await.records.get(id).cloned())
    }

    async fn list(
        &self,
        query: &WorktreeQuery,
    ) -> Result<Vec<WorktreeRecord>, WorktreeServiceError> {
        let state = self.state.lock().await;
        let mut values: Vec<_> = state
            .records
            .values()
            .filter(|record| {
                query
                    .workspace_id
                    .as_ref()
                    .is_none_or(|id| &record.workspace_id == id)
                    && query
                        .repository_id
                        .as_ref()
                        .is_none_or(|id| &record.repository_id == id)
                    && query
                        .owner_run_id
                        .as_ref()
                        .is_none_or(|id| record.owner_run_id.as_ref() == Some(id))
                    && (query.include_removed || record.state != ManagedWorktreeState::Removed)
            })
            .cloned()
            .collect();
        values.sort_by_key(|record| record.created_at);
        Ok(values)
    }

    /// M005 corrective C008: every non-removed row, no cap.
    ///
    /// The in-memory store was already unbounded, so it keeps the durable
    /// store's semantics: the authority sees the whole catalog.
    ///
    /// M005 corrective C008 (implementation review): the ordering is a
    /// **total** order, matching the durable store's. Ordering by
    /// `created_at` alone leaves records that share a timestamp in an
    /// unspecified order — the in-memory store kept its insertion order and
    /// the durable store returned the database's — so the two stores could
    /// answer the same lookup differently. `worktree_id` breaks the tie.
    async fn list_authority_candidates(&self) -> Result<Vec<WorktreeRecord>, WorktreeServiceError> {
        let state = self.state.lock().await;
        let mut values: Vec<_> = state
            .records
            .values()
            .filter(|record| record.state != ManagedWorktreeState::Removed)
            .cloned()
            .collect();
        values.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.worktree_id.cmp(&right.worktree_id))
        });
        Ok(values)
    }

    async fn save(&self, record: WorktreeRecord) -> Result<(), WorktreeServiceError> {
        let mut state = self.state.lock().await;
        if !state.records.contains_key(&record.worktree_id) {
            return Err(WorktreeServiceError::NotFound(
                record.worktree_id.to_string(),
            ));
        }
        state.records.insert(record.worktree_id.clone(), record);
        Ok(())
    }

    async fn acquire_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
    ) -> Result<WorktreeLease, WorktreeServiceError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(id)
            .ok_or_else(|| WorktreeServiceError::NotFound(id.to_string()))?;
        if record.owner_run_id.is_some() {
            return Err(WorktreeServiceError::LeaseConflict(id.to_string()));
        }
        if !matches!(
            record.state,
            ManagedWorktreeState::Ready | ManagedWorktreeState::Reserved
        ) {
            return Err(WorktreeServiceError::LeaseConflict(format!(
                "{} is {:?}",
                id, record.state
            )));
        }
        let now = Utc::now().timestamp_millis();
        record.lease_generation = record.lease_generation.saturating_add(1);
        record.owner_run_id = Some(owner.clone());
        transition(record, ManagedWorktreeState::InUse)?;
        let lease = WorktreeLease {
            worktree_id: id.clone(),
            owner_run_id: owner.clone(),
            generation: record.lease_generation,
            acquired_at: now,
            renewed_at: now,
            released_at: None,
        };
        state.leases.push(lease.clone());
        Ok(lease)
    }

    async fn renew_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
        generation: u64,
    ) -> Result<WorktreeLease, WorktreeServiceError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get(id)
            .ok_or_else(|| WorktreeServiceError::NotFound(id.to_string()))?;
        if record.lease_generation != generation {
            return Err(WorktreeServiceError::StaleGeneration {
                expected: generation,
                actual: record.lease_generation,
            });
        }
        if record.owner_run_id.as_ref() != Some(owner) {
            return Err(WorktreeServiceError::LeaseConflict(id.to_string()));
        }
        let lease = state
            .leases
            .iter_mut()
            .rev()
            .find(|lease| {
                lease.worktree_id == *id
                    && lease.generation == generation
                    && lease.released_at.is_none()
            })
            .ok_or_else(|| WorktreeServiceError::LeaseConflict(id.to_string()))?;
        lease.renewed_at = Utc::now().timestamp_millis();
        Ok(lease.clone())
    }

    async fn release_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
        generation: u64,
    ) -> Result<WorktreeRecord, WorktreeServiceError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .get_mut(id)
            .ok_or_else(|| WorktreeServiceError::NotFound(id.to_string()))?;
        if record.lease_generation != generation {
            return Err(WorktreeServiceError::StaleGeneration {
                expected: generation,
                actual: record.lease_generation,
            });
        }
        if record.owner_run_id.as_ref() != Some(owner) {
            return Err(WorktreeServiceError::LeaseConflict(id.to_string()));
        }
        let now = Utc::now().timestamp_millis();
        record.owner_run_id = None;
        transition(record, ManagedWorktreeState::Ready)?;
        let released_record = record.clone();
        if let Some(lease) = state.leases.iter_mut().rev().find(|lease| {
            lease.worktree_id == *id
                && lease.generation == generation
                && lease.released_at.is_none()
        }) {
            lease.released_at = Some(now);
            lease.renewed_at = now;
        }
        Ok(released_record)
    }
}

pub struct SqliteWorktreeStore {
    pool: SqlitePool,
}

impl SqliteWorktreeStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn row_to_record(row: &sqlx::sqlite::SqliteRow) -> Result<WorktreeRecord, WorktreeServiceError> {
    Ok(WorktreeRecord {
        worktree_id: WorktreeId::parse(row.get::<String, _>("worktree_id").as_str())
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?,
        project_id: ProjectId::parse(row.get::<String, _>("project_id").as_str())
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?,
        repository_id: RepositoryId::parse(row.get::<String, _>("repository_id").as_str())
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?,
        workspace_id: WorkspaceId::new_unchecked(row.get::<String, _>("workspace_id")),
        node_id: row
            .get::<Option<String>, _>("node_id")
            .map(|v| NodeId::parse(&v))
            .transpose()
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?,
        repository_root: PathBuf::from(row.get::<String, _>("repository_root")),
        path: PathBuf::from(row.get::<String, _>("path")),
        branch: row.get("branch"),
        base_commit: row.get("base_commit"),
        managed: row.get::<i64, _>("managed") != 0,
        state: ManagedWorktreeState::parse_state(row.get::<String, _>("state").as_str())?,
        health: WorktreeHealth::parse_health(row.get::<String, _>("health").as_str())?,
        lease_generation: row.get::<i64, _>("lease_generation") as u64,
        owner_run_id: row
            .get::<Option<String>, _>("owner_run_id")
            .map(|v| AgentRunId::parse(&v))
            .transpose()
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

const RECORD_COLUMNS: &str = "worktree_id, project_id, repository_id, workspace_id, node_id, repository_root, path, branch, base_commit, managed, state, health, lease_generation, owner_run_id, created_at, updated_at";

async fn load_sqlite(
    pool: &SqlitePool,
    id: &WorktreeId,
) -> Result<Option<WorktreeRecord>, WorktreeServiceError> {
    sqlx::query(&format!(
        "SELECT {RECORD_COLUMNS} FROM managed_worktree WHERE worktree_id = ?"
    ))
    .bind(id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?
    .map(|row| row_to_record(&row))
    .transpose()
}

#[async_trait]
impl WorktreeStore for SqliteWorktreeStore {
    async fn insert_reserved(&self, record: WorktreeRecord) -> Result<(), WorktreeServiceError> {
        let result = sqlx::query("INSERT INTO managed_worktree (worktree_id, project_id, repository_id, workspace_id, node_id, repository_root, path, branch, base_commit, managed, state, health, lease_generation, owner_run_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(record.worktree_id.as_str()).bind(record.project_id.as_str()).bind(record.repository_id.as_str()).bind(record.workspace_id.as_str()).bind(record.node_id.as_ref().map(NodeId::as_str)).bind(record.repository_root.to_string_lossy().as_ref()).bind(record.path.to_string_lossy().as_ref()).bind(record.branch.as_deref()).bind(&record.base_commit).bind(if record.managed { 1_i64 } else { 0_i64 }).bind(record.state.as_str()).bind(record.health.as_str()).bind(record.lease_generation as i64).bind(record.owner_run_id.as_ref().map(AgentRunId::as_str)).bind(record.created_at).bind(record.updated_at).execute(&self.pool).await;
        result.map(|_| ()).map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                WorktreeServiceError::Collision(record.path.display().to_string())
            } else {
                WorktreeServiceError::Storage(e.to_string())
            }
        })
    }

    async fn get(&self, id: &WorktreeId) -> Result<Option<WorktreeRecord>, WorktreeServiceError> {
        load_sqlite(&self.pool, id).await
    }

    async fn list(
        &self,
        query: &WorktreeQuery,
    ) -> Result<Vec<WorktreeRecord>, WorktreeServiceError> {
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {RECORD_COLUMNS} FROM managed_worktree WHERE 1=1"
        ));
        if let Some(id) = &query.workspace_id {
            builder.push(" AND workspace_id = ").push_bind(id.as_str());
        }
        if let Some(id) = &query.repository_id {
            builder.push(" AND repository_id = ").push_bind(id.as_str());
        }
        if let Some(id) = &query.owner_run_id {
            builder.push(" AND owner_run_id = ").push_bind(id.as_str());
        }
        if !query.include_removed {
            builder.push(" AND state <> 'removed'");
        }
        builder
            .push(" ORDER BY created_at LIMIT ")
            .push_bind(MAX_WORKTREE_LIST as i64);
        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        rows.iter().map(row_to_record).collect()
    }

    /// M005 corrective C008: every non-removed row, no `LIMIT`.
    ///
    /// The `list` surface above is capped at `MAX_WORKTREE_LIST` rows,
    /// oldest-first. The reservation authority cannot use it: past the cap
    /// the newest rows — including a freshly created, currently reserved
    /// checkout — are absent from the result, so the authority would report
    /// a reserved checkout as unreserved. This read has no bound, so a cap
    /// can never hide a reserved checkout.
    /// M005 corrective C008: every non-removed row, no cap.
    ///
    /// The durable store's ordering is a **total** order. Ordering by
    /// `created_at` alone leaves records that share a timestamp in the
    /// database's unspecified order, so the in-memory and durable stores
    /// could answer the same lookup differently. `worktree_id` breaks the
    /// tie, and the in-memory store breaks it the same way.
    async fn list_authority_candidates(&self) -> Result<Vec<WorktreeRecord>, WorktreeServiceError> {
        let rows = sqlx::query(&format!(
            "SELECT {RECORD_COLUMNS} FROM managed_worktree WHERE state <> 'removed' ORDER BY created_at, worktree_id"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        rows.iter().map(row_to_record).collect()
    }

    async fn save(&self, record: WorktreeRecord) -> Result<(), WorktreeServiceError> {
        let result = sqlx::query("UPDATE managed_worktree SET node_id = ?, repository_root = ?, path = ?, branch = ?, base_commit = ?, managed = ?, state = ?, health = ?, lease_generation = ?, owner_run_id = ?, updated_at = ? WHERE worktree_id = ?")
            .bind(record.node_id.as_ref().map(NodeId::as_str)).bind(record.repository_root.to_string_lossy().as_ref()).bind(record.path.to_string_lossy().as_ref()).bind(record.branch.as_deref()).bind(&record.base_commit).bind(if record.managed { 1_i64 } else { 0_i64 }).bind(record.state.as_str()).bind(record.health.as_str()).bind(record.lease_generation as i64).bind(record.owner_run_id.as_ref().map(AgentRunId::as_str)).bind(record.updated_at).bind(record.worktree_id.as_str()).execute(&self.pool).await.map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(WorktreeServiceError::NotFound(
                record.worktree_id.to_string(),
            ));
        }
        Ok(())
    }

    async fn acquire_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
    ) -> Result<WorktreeLease, WorktreeServiceError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        let row = sqlx::query("SELECT lease_generation, owner_run_id, state FROM managed_worktree WHERE worktree_id = ?").bind(id.as_str()).fetch_optional(&mut *tx).await.map_err(|e| WorktreeServiceError::Storage(e.to_string()))?.ok_or_else(|| WorktreeServiceError::NotFound(id.to_string()))?;
        if row.get::<Option<String>, _>("owner_run_id").is_some() {
            return Err(WorktreeServiceError::LeaseConflict(id.to_string()));
        }
        let state = ManagedWorktreeState::parse_state(row.get::<String, _>("state").as_str())?;
        if !matches!(
            state,
            ManagedWorktreeState::Ready | ManagedWorktreeState::Reserved
        ) {
            return Err(WorktreeServiceError::LeaseConflict(format!(
                "{} is {:?}",
                id, state
            )));
        }
        let generation = row.get::<i64, _>("lease_generation") as u64 + 1;
        let now = Utc::now().timestamp_millis();
        let changed = sqlx::query("UPDATE managed_worktree SET lease_generation = ?, owner_run_id = ?, state = 'in_use', updated_at = ? WHERE worktree_id = ? AND owner_run_id IS NULL").bind(generation as i64).bind(owner.as_str()).bind(now).bind(id.as_str()).execute(&mut *tx).await.map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        if changed.rows_affected() != 1 {
            return Err(WorktreeServiceError::LeaseConflict(id.to_string()));
        }
        sqlx::query("INSERT INTO worktree_lease (worktree_id, owner_run_id, generation, acquired_at, renewed_at, released_at) VALUES (?, ?, ?, ?, ?, NULL)").bind(id.as_str()).bind(owner.as_str()).bind(generation as i64).bind(now).bind(now).execute(&mut *tx).await.map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        Ok(WorktreeLease {
            worktree_id: id.clone(),
            owner_run_id: owner.clone(),
            generation,
            acquired_at: now,
            renewed_at: now,
            released_at: None,
        })
    }

    async fn renew_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
        generation: u64,
    ) -> Result<WorktreeLease, WorktreeServiceError> {
        let now = Utc::now().timestamp_millis();
        let result = sqlx::query("UPDATE worktree_lease SET renewed_at = ? WHERE worktree_id = ? AND owner_run_id = ? AND generation = ? AND released_at IS NULL").bind(now).bind(id.as_str()).bind(owner.as_str()).bind(generation as i64).execute(&self.pool).await.map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            let record = load_sqlite(&self.pool, id)
                .await?
                .ok_or_else(|| WorktreeServiceError::NotFound(id.to_string()))?;
            if record.lease_generation != generation {
                return Err(WorktreeServiceError::StaleGeneration {
                    expected: generation,
                    actual: record.lease_generation,
                });
            }
            return Err(WorktreeServiceError::LeaseConflict(id.to_string()));
        }
        let row = sqlx::query("SELECT acquired_at FROM worktree_lease WHERE worktree_id = ? AND owner_run_id = ? AND generation = ?").bind(id.as_str()).bind(owner.as_str()).bind(generation as i64).fetch_one(&self.pool).await.map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        Ok(WorktreeLease {
            worktree_id: id.clone(),
            owner_run_id: owner.clone(),
            generation,
            acquired_at: row.get("acquired_at"),
            renewed_at: now,
            released_at: None,
        })
    }

    async fn release_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
        generation: u64,
    ) -> Result<WorktreeRecord, WorktreeServiceError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        let record = sqlx::query(
            "SELECT lease_generation, owner_run_id FROM managed_worktree WHERE worktree_id = ?",
        )
        .bind(id.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?
        .ok_or_else(|| WorktreeServiceError::NotFound(id.to_string()))?;
        let actual = record.get::<i64, _>("lease_generation") as u64;
        if actual != generation {
            return Err(WorktreeServiceError::StaleGeneration {
                expected: generation,
                actual,
            });
        }
        if record.get::<Option<String>, _>("owner_run_id").as_deref() != Some(owner.as_str()) {
            return Err(WorktreeServiceError::LeaseConflict(id.to_string()));
        }
        let now = Utc::now().timestamp_millis();
        sqlx::query("UPDATE managed_worktree SET owner_run_id = NULL, state = 'ready', updated_at = ? WHERE worktree_id = ? AND owner_run_id = ? AND lease_generation = ?").bind(now).bind(id.as_str()).bind(owner.as_str()).bind(generation as i64).execute(&mut *tx).await.map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        sqlx::query("UPDATE worktree_lease SET released_at = ?, renewed_at = ? WHERE worktree_id = ? AND owner_run_id = ? AND generation = ? AND released_at IS NULL").bind(now).bind(now).bind(id.as_str()).bind(owner.as_str()).bind(generation as i64).execute(&mut *tx).await.map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| WorktreeServiceError::Storage(e.to_string()))?;
        load_sqlite(&self.pool, id)
            .await?
            .ok_or_else(|| WorktreeServiceError::NotFound(id.to_string()))
    }
}

pub struct WorktreeService {
    store: Arc<dyn WorktreeStore>,
    locks: Arc<WorkspaceLockTable>,
    managed_root: PathBuf,
    owner_resolver: Option<Arc<dyn WorktreeOwnerResolver>>,
    contained_creator: Option<Arc<dyn ContainedWorktreeCreator>>,
}

impl WorktreeService {
    pub fn with_store(
        store: Arc<dyn WorktreeStore>,
        managed_root: PathBuf,
        locks: Arc<WorkspaceLockTable>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            locks,
            managed_root,
            owner_resolver: None,
            contained_creator: None,
        })
    }

    pub fn with_owner_resolver(
        self: Arc<Self>,
        owner_resolver: Arc<dyn WorktreeOwnerResolver>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store: self.store.clone(),
            locks: self.locks.clone(),
            managed_root: self.managed_root.clone(),
            owner_resolver: Some(owner_resolver),
            contained_creator: self.contained_creator.clone(),
        })
    }

    /// Install the contained creation path.
    ///
    /// A configured creator is authoritative: `create_reserved` uses it, and
    /// its failure is final. There is no fallback to the in-process Git path,
    /// because the in-process path cannot obtain the containment a linked
    /// worktree's checkout requires.
    pub fn with_contained_creator(
        self: Arc<Self>,
        contained_creator: Arc<dyn ContainedWorktreeCreator>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store: self.store.clone(),
            locks: self.locks.clone(),
            managed_root: self.managed_root.clone(),
            owner_resolver: self.owner_resolver.clone(),
            contained_creator: Some(contained_creator),
        })
    }

    pub fn sqlite(
        pool: SqlitePool,
        managed_root: PathBuf,
        locks: Arc<WorkspaceLockTable>,
    ) -> Arc<Self> {
        Self::with_store(
            Arc::new(SqliteWorktreeStore::new(pool)),
            managed_root,
            locks,
        )
    }

    pub fn memory(managed_root: PathBuf, locks: Arc<WorkspaceLockTable>) -> Arc<Self> {
        Self::with_store(Arc::new(InMemoryWorktreeStore::new()), managed_root, locks)
    }

    pub fn managed_root(&self) -> &Path {
        &self.managed_root
    }

    pub async fn get(&self, id: &WorktreeId) -> Result<WorktreeRecord, WorktreeServiceError> {
        self.store
            .get(id)
            .await?
            .ok_or_else(|| WorktreeServiceError::NotFound(id.to_string()))
    }

    pub async fn list(
        &self,
        query: WorktreeQuery,
    ) -> Result<Vec<WorktreeRecord>, WorktreeServiceError> {
        self.store.list(&query).await
    }

    /// M005 corrective C008: the unbounded candidate set the checkout
    /// reservation authority reads.
    ///
    /// `list` is capped and oldest-first, so it cannot answer "which managed
    /// checkout contains this path" once the user's catalog grows past
    /// [`MAX_WORKTREE_LIST`] rows: the newest rows are missing from the
    /// result. This reads every non-removed row, so a reserved checkout is
    /// never hidden by the size of the catalog.
    pub async fn list_authority_candidates(
        &self,
    ) -> Result<Vec<WorktreeRecord>, WorktreeServiceError> {
        self.store.list_authority_candidates().await
    }

    /// Reserve a deterministic identity and locator. No Git mutation occurs.
    pub async fn reserve(
        &self,
        request: &CreateWorktreeRequest,
    ) -> Result<WorktreeRecord, WorktreeServiceError> {
        let root = canonical_repo_root(&request.repository_root)?;
        let base_path = request
            .base_path
            .as_deref()
            .unwrap_or(&request.repository_root);
        let base_path = canonical_or_self(base_path);
        let status = egggit::status_v2::rich_repo_status(&base_path)
            .await
            .map_err(|e| WorktreeServiceError::Git(e.to_string()))?
            .into_value();
        let base_commit = request.base_commit.clone().or(status.head).ok_or_else(|| {
            WorktreeServiceError::Git("repository has no commit to use as a base".into())
        })?;
        ObjectId::new(&base_commit).map_err(|e| {
            WorktreeServiceError::InvalidInput(format!("invalid base commit from Git: {e}"))
        })?;
        // M006: a base *ref* is resolved to an immutable commit here, so
        // the record never stores a moving name.
        let base_commit = match request.base_ref.as_deref() {
            Some(base_ref) => {
                let resolved = egggit::refs::resolve_base_ref(&base_path, base_ref)
                    .await
                    .map_err(|e| {
                        WorktreeServiceError::InvalidInput(format!(
                            "cannot resolve base ref {base_ref}: {e}"
                        ))
                    })?
                    .into_value();
                ObjectId::new(&resolved).map_err(|e| {
                    WorktreeServiceError::InvalidInput(format!("invalid base commit from Git: {e}"))
                })?;
                resolved
            }
            None => base_commit,
        };
        let worktree_id = WorktreeId::new();
        let id_short = worktree_id.as_str().replace('-', "");
        let id_short = &id_short[..id_short.len().min(12)];
        // M006: an explicit target branch wins over the minted name.
        let branch = match request.target_branch.as_deref() {
            Some(target) => {
                if target.trim().is_empty() || target.len() > 255 {
                    return Err(WorktreeServiceError::InvalidInput(
                        "target branch must be 1..=255 bytes".into(),
                    ));
                }
                target.to_string()
            }
            None => format!("codegg/worktree/{id_short}"),
        };
        BranchName::new(&branch).map_err(|e| WorktreeServiceError::InvalidInput(e.to_string()))?;
        let repo_root = self.managed_root.join(request.repository_id.as_str());
        let path = repo_root.join(format!("wt-{id_short}"));
        let managed_root = canonical_or_self(&repo_root);
        if path.exists() || path.is_symlink() {
            return Err(WorktreeServiceError::Collision(path.display().to_string()));
        }
        if managed_root.starts_with(&root) || root.starts_with(&managed_root) {
            return Err(WorktreeServiceError::InvalidInput(
                "managed worktree root must be outside the repository root".into(),
            ));
        }
        tokio::fs::create_dir_all(&managed_root)
            .await
            .map_err(|e| {
                WorktreeServiceError::Storage(format!("create managed worktree root: {e}"))
            })?;
        let managed_root = tokio::fs::canonicalize(&managed_root).await.map_err(|e| {
            WorktreeServiceError::Storage(format!("canonicalize managed root: {e}"))
        })?;
        let path = managed_root.join(format!("wt-{id_short}"));
        let existing = crate::worktree::list_worktrees(&root)
            .await
            .map_err(|e| WorktreeServiceError::Git(e.to_string()))?;
        if existing
            .iter()
            .any(|tree| tree.path == path.to_string_lossy() || tree.branch == branch)
        {
            return Err(WorktreeServiceError::Collision(path.display().to_string()));
        }
        let now = Utc::now().timestamp_millis();
        let record = WorktreeRecord {
            worktree_id,
            project_id: request.project_id.clone(),
            repository_id: request.repository_id.clone(),
            workspace_id: request.workspace_id.clone(),
            node_id: request.node_id.clone(),
            repository_root: root,

            path,
            branch: Some(branch),
            base_commit,
            managed: true,
            state: ManagedWorktreeState::Reserved,
            health: WorktreeHealth::Unknown,
            lease_generation: 0,
            owner_run_id: None,
            created_at: now,
            updated_at: now,
        };
        self.store.insert_reserved(record.clone()).await?;
        Ok(record)
    }

    /// Reserve and create a worktree, then atomically issue its first lease.
    pub async fn create(
        &self,
        request: &CreateWorktreeRequest,
    ) -> Result<(WorktreeRecord, WorktreeLease), WorktreeServiceError> {
        let record = self.reserve(request).await?;
        let record = self.create_reserved(&record.worktree_id).await?;
        let lease = self
            .acquire_lease(&record.worktree_id, &request.owner_run_id)
            .await?;
        Ok((self.get(&record.worktree_id).await?, lease))
    }

    pub async fn create_reserved(
        &self,
        id: &WorktreeId,
    ) -> Result<WorktreeRecord, WorktreeServiceError> {
        let mut record = self.get(id).await?;
        transition(&mut record, ManagedWorktreeState::Preparing)?;
        self.store.save(record.clone()).await?;
        let _guard = self.locks.acquire_repository(&record.repository_root).await;
        if let Some(creator) = self.contained_creator.clone() {
            return self.create_reserved_contained(creator, record).await;
        }
        let result = tokio::task::spawn_blocking({
            let root = record.repository_root.clone();
            let path = record.path.clone();
            let branch = record.branch.clone();
            let base = record.base_commit.clone();
            move || {
                crate::worktree::create_worktree_at(
                    &root,
                    &path,
                    branch.as_deref().unwrap_or_default(),
                    true,
                    Some(&base),
                )
            }
        })
        .await
        .map_err(|e| WorktreeServiceError::Git(format!("worktree create task: {e}")))?;
        if let Err(error) = result {
            transition(&mut record, ManagedWorktreeState::Orphaned)?;
            record.health = WorktreeHealth::GitError;
            self.store.save(record).await?;
            return Err(WorktreeServiceError::Git(error.to_string()));
        }
        let listed = crate::worktree::list_worktrees(&record.repository_root)
            .await
            .map_err(|e| WorktreeServiceError::Git(e.to_string()))?;
        let registered = listed.iter().any(|tree| {
            canonical_or_self(Path::new(&tree.path)) == canonical_or_self(&record.path)
                && tree.branch == record.branch.clone().unwrap_or_default()
        });
        if !registered || !crate::worktree::is_git_worktree(&record.path) {
            transition(&mut record, ManagedWorktreeState::Orphaned)?;
            record.health = WorktreeHealth::GitError;
            self.store.save(record).await?;
            return Err(WorktreeServiceError::Git(
                "created worktree failed post-create verification".into(),
            ));
        }
        transition(&mut record, ManagedWorktreeState::Ready)?;
        record.health = WorktreeHealth::Clean;
        self.store.save(record.clone()).await?;
        Ok(record)
    }

    /// Create the checkout through the contained seam.
    ///
    /// The record's branch is the **daemon's**: the creator creates it before
    /// the contained child runs, and the child attaches to a branch that
    /// already exists. The child's grant excludes the repository's refs, so
    /// the child cannot create a branch even by accident — a creation attempt
    /// fails closed rather than writing a ref.
    ///
    /// The registration check below is the same one the in-process path
    /// applies: Git must report the checkout at the recorded path **on the
    /// recorded branch**, and it must be a real linked worktree. A detached
    /// checkout reports `(detached HEAD)`, which is not the recorded branch,
    /// so it is not accepted.
    async fn create_reserved_contained(
        &self,
        creator: Arc<dyn ContainedWorktreeCreator>,
        mut record: WorktreeRecord,
    ) -> Result<WorktreeRecord, WorktreeServiceError> {
        let branch = record.branch.clone().unwrap_or_default();
        if branch.is_empty() {
            transition(&mut record, ManagedWorktreeState::Orphaned)?;
            record.health = WorktreeHealth::GitError;
            self.store.save(record).await?;
            return Err(WorktreeServiceError::InvalidInput(
                "the reserved worktree has no branch to attach to".into(),
            ));
        }
        let repository_root = record.repository_root.clone();
        let repository_gitdir = match crate::worktree::git_dir_for(&repository_root) {
            Some(gitdir) => gitdir,
            None => {
                transition(&mut record, ManagedWorktreeState::Orphaned)?;
                record.health = WorktreeHealth::GitError;
                self.store.save(record).await?;
                return Err(WorktreeServiceError::Git(format!(
                    "the repository at {} has no Git directory",
                    repository_root.display()
                )));
            }
        };
        let request = ContainedWorktreeRequest {
            repository_root: repository_root.clone(),
            repository_gitdir,
            worktree_path: record.path.clone(),
            branch,
            base_commit: record.base_commit.clone(),
            timeout: DEFAULT_CONTAINED_WORKTREE_TIMEOUT,
        };
        if let Err(error) = creator.create(&request).await {
            transition(&mut record, ManagedWorktreeState::Orphaned)?;
            record.health = WorktreeHealth::GitError;
            self.store.save(record).await?;
            return Err(error);
        }
        let listed = crate::worktree::list_worktrees(&record.repository_root)
            .await
            .map_err(|e| WorktreeServiceError::Git(e.to_string()))?;
        let registered = listed.iter().any(|tree| {
            canonical_or_self(Path::new(&tree.path)) == canonical_or_self(&record.path)
                && tree.branch == record.branch.clone().unwrap_or_default()
        });
        if !registered || !crate::worktree::is_git_worktree(&record.path) {
            transition(&mut record, ManagedWorktreeState::Orphaned)?;
            record.health = WorktreeHealth::GitError;
            self.store.save(record).await?;
            return Err(WorktreeServiceError::Git(
                "created worktree failed post-create verification".into(),
            ));
        }
        transition(&mut record, ManagedWorktreeState::Ready)?;
        record.health = WorktreeHealth::Clean;
        self.store.save(record.clone()).await?;
        Ok(record)
    }

    pub async fn acquire_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
    ) -> Result<WorktreeLease, WorktreeServiceError> {
        let record = self.get(id).await?;
        let _guard = self.locks.acquire_repository(&record.repository_root).await;
        self.store.acquire_lease(id, owner).await
    }

    pub async fn renew_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
        generation: u64,
    ) -> Result<WorktreeLease, WorktreeServiceError> {
        let record = self.get(id).await?;
        if record.lease_generation != generation {
            return Err(WorktreeServiceError::StaleGeneration {
                expected: generation,
                actual: record.lease_generation,
            });
        }
        self.store.renew_lease(id, owner, generation).await
    }

    pub async fn release_lease(
        &self,
        id: &WorktreeId,
        owner: &AgentRunId,
        generation: u64,
    ) -> Result<WorktreeRecord, WorktreeServiceError> {
        let record = self.get(id).await?;
        let _guard = self.locks.acquire_repository(&record.repository_root).await;
        self.store.release_lease(id, owner, generation).await
    }

    pub async fn refresh(&self, id: &WorktreeId) -> Result<WorktreeRecord, WorktreeServiceError> {
        let mut record = self.get(id).await?;
        let (health, registered) = inspect_health(&record).await;
        record.health = health;
        if (!registered || health != WorktreeHealth::Clean)
            && record.state != ManagedWorktreeState::Removed
            && record.owner_run_id.is_none()
        {
            if let Err(error) = transition(&mut record, ManagedWorktreeState::Orphaned) {
                tracing::warn!(worktree_id = %record.worktree_id, %error, "failed to mark worktree orphaned");
            }
        }
        self.store.save(record.clone()).await?;
        Ok(record)
    }

    pub async fn mark_health(
        &self,
        id: &WorktreeId,
        health: WorktreeHealth,
    ) -> Result<WorktreeRecord, WorktreeServiceError> {
        let mut record = self.get(id).await?;
        record.health = health;
        if !health.cleanup_safe()
            && record.owner_run_id.is_none()
            && record.state == ManagedWorktreeState::Ready
        {
            transition(&mut record, ManagedWorktreeState::Orphaned)?;
        }
        self.store.save(record.clone()).await?;
        Ok(record)
    }

    /// Retain a released worktree for operator inspection without removing
    /// it. Archived records can later be removed through generation-checked
    /// cleanup once they remain clean.
    pub async fn archive(
        &self,
        id: &WorktreeId,
        expected_generation: u64,
    ) -> Result<WorktreeRecord, WorktreeServiceError> {
        let mut record = self.get(id).await?;
        if record.lease_generation != expected_generation {
            return Err(WorktreeServiceError::StaleGeneration {
                expected: expected_generation,
                actual: record.lease_generation,
            });
        }
        if record.owner_run_id.is_some() {
            return Err(WorktreeServiceError::UnsafeCleanup(
                "worktree is still leased".into(),
            ));
        }
        record = self.refresh(id).await?;
        if record.owner_run_id.is_some() || !record.health.cleanup_safe() {
            return Err(WorktreeServiceError::UnsafeCleanup(format!(
                "worktree health is {:?}",
                record.health
            )));
        }
        transition(&mut record, ManagedWorktreeState::Archived)?;
        self.store.save(record.clone()).await?;
        Ok(record)
    }

    /// Remove only a released, clean, still-managed worktree. The owner and
    /// generation are checked before and after the final status refresh.
    pub async fn cleanup(
        &self,
        id: &WorktreeId,
        expected_generation: u64,
    ) -> Result<WorktreeRecord, WorktreeServiceError> {
        let mut record = self.get(id).await?;
        if !record.managed {
            return Err(WorktreeServiceError::UnsafeCleanup(
                "external worktrees are never automatically removed".into(),
            ));
        }
        if record.lease_generation != expected_generation {
            return Err(WorktreeServiceError::StaleGeneration {
                expected: expected_generation,
                actual: record.lease_generation,
            });
        }
        if record.owner_run_id.is_some() {
            return Err(WorktreeServiceError::UnsafeCleanup(
                "worktree is still leased".into(),
            ));
        }
        let _guard = self.locks.acquire_repository(&record.repository_root).await;
        record = self.refresh(id).await?;
        if record.owner_run_id.is_some() || record.lease_generation != expected_generation {
            return Err(WorktreeServiceError::StaleGeneration {
                expected: expected_generation,
                actual: record.lease_generation,
            });
        }
        if !record.health.cleanup_safe() {
            return Err(WorktreeServiceError::UnsafeCleanup(format!(
                "health is {:?}",
                record.health
            )));
        }
        ensure_safe_managed_path(&self.managed_root, &record.path)?;
        if !record.path.is_dir() || !crate::worktree::is_git_worktree(&record.path) {
            return Err(WorktreeServiceError::UnsafeCleanup(
                "worktree path is missing or not a CodeGG worktree".into(),
            ));
        }
        if record.state != ManagedWorktreeState::Archived {
            transition(&mut record, ManagedWorktreeState::Releasing)?;
        }
        self.store.save(record.clone()).await?;
        let root = record.repository_root.clone();
        let path = record.path.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::worktree::remove_worktree(&root, &path, false)
        })
        .await
        .map_err(|e| WorktreeServiceError::Git(format!("worktree remove task: {e}")))?;
        if let Err(error) = result {
            record.health = WorktreeHealth::Unknown;
            transition(&mut record, ManagedWorktreeState::Orphaned)?;
            self.store.save(record).await?;
            return Err(WorktreeServiceError::Git(error.to_string()));
        }
        if record.path.exists() {
            return Err(WorktreeServiceError::UnsafeCleanup(
                "Git reported removal but path remains".into(),
            ));
        }
        transition(&mut record, ManagedWorktreeState::Removed)?;
        record.health = WorktreeHealth::Missing;
        self.store.save(record.clone()).await?;
        Ok(record)
    }

    pub async fn discover(
        &self,
        repository_root: &Path,
        repository_id: &RepositoryId,
    ) -> Result<Vec<DiscoveredWorktree>, WorktreeServiceError> {
        let root = canonical_repo_root(repository_root)?;
        let actual = crate::worktree::list_worktrees(&root)
            .await
            .map_err(|e| WorktreeServiceError::Git(e.to_string()))?;
        let records = self
            .store
            .list(&WorktreeQuery {
                repository_id: Some(repository_id.clone()),
                include_removed: false,
                ..Default::default()
            })
            .await?;
        Ok(actual
            .into_iter()
            .map(|tree| {
                let path = canonical_or_self(Path::new(&tree.path));
                let found = records
                    .iter()
                    .find(|record| canonical_or_self(&record.path) == canonical_or_self(&path));
                DiscoveredWorktree {
                    path,
                    branch: (!tree.branch.is_empty()).then_some(tree.branch),
                    managed: found.is_some_and(|record| record.managed),
                    worktree_id: found.map(|record| record.worktree_id.clone()),
                }
            })
            .collect())
    }

    pub async fn reconcile_repository(
        &self,
        repository_root: &Path,
        repository_id: &RepositoryId,
    ) -> Result<ReconcileReport, WorktreeServiceError> {
        let discovered = self.discover(repository_root, repository_id).await?;
        let actual: HashSet<PathBuf> = discovered
            .iter()
            .map(|item| canonical_or_self(&item.path))
            .collect();
        let records = self
            .store
            .list(&WorktreeQuery {
                repository_id: Some(repository_id.clone()),
                include_removed: false,
                ..Default::default()
            })
            .await?;
        let mut report = ReconcileReport {
            inspected: records.len(),
            ..Default::default()
        };
        for mut record in records {
            let present = actual.contains(&canonical_or_self(&record.path));
            let (health, registered) = if present {
                inspect_health(&record).await
            } else {
                (WorktreeHealth::Missing, false)
            };
            record.health = health;
            if let Some(owner) = record.owner_run_id.clone() {
                let terminal = match &self.owner_resolver {
                    Some(resolver) => resolver
                        .is_terminal(&owner)
                        .await
                        .map_err(WorktreeServiceError::Storage)?,
                    None => false,
                };
                if terminal {
                    if self
                        .store
                        .release_lease(&record.worktree_id, &owner, record.lease_generation)
                        .await
                        .is_ok()
                    {
                        report.released_terminal.push(record.worktree_id.clone());
                    }
                } else {
                    let _ = transition(&mut record, ManagedWorktreeState::InUse);
                    self.store.save(record.clone()).await?;
                    report.retained_active.push(record.worktree_id.clone());
                }
            } else if present
                && registered
                && health == WorktreeHealth::Clean
                && matches!(
                    record.state,
                    ManagedWorktreeState::Reserved
                        | ManagedWorktreeState::Preparing
                        | ManagedWorktreeState::Orphaned
                )
            {
                transition(&mut record, ManagedWorktreeState::Ready)?;
                self.store.save(record.clone()).await?;
                report.ready.push(record.worktree_id.clone());
            } else if !present || !registered || !health.cleanup_safe() {
                let _ = transition(&mut record, ManagedWorktreeState::Orphaned);
                self.store.save(record.clone()).await?;
                report.orphaned.push(record.worktree_id.clone());
            } else {
                self.store.save(record.clone()).await?;
            }
        }
        report.external = discovered
            .into_iter()
            .filter(|item| !item.managed)
            .map(|item| item.path)
            .collect();
        Ok(report)
    }

    pub async fn reconcile_all(&self) -> Result<ReconcileReport, WorktreeServiceError> {
        let records = self.store.list(&WorktreeQuery::default()).await?;
        let mut report = ReconcileReport::default();
        let mut repositories = HashSet::new();
        for record in records {
            repositories.insert((record.repository_id, record.repository_root));
        }
        for (repository_id, root) in repositories {
            let current = self.reconcile_repository(&root, &repository_id).await?;
            report.inspected += current.inspected;
            report.ready.extend(current.ready);
            report.retained_active.extend(current.retained_active);
            report.orphaned.extend(current.orphaned);
            report.released_terminal.extend(current.released_terminal);
            report.external.extend(current.external);
        }
        Ok(report)
    }
}

async fn inspect_health(record: &WorktreeRecord) -> (WorktreeHealth, bool) {
    if !record.path.is_dir() {
        return (WorktreeHealth::Missing, false);
    }
    let listed = match crate::worktree::list_worktrees(&record.repository_root).await {
        Ok(value) => value,
        Err(_) => return (WorktreeHealth::GitError, false),
    };
    let registered = listed
        .iter()
        .any(|tree| canonical_or_self(Path::new(&tree.path)) == canonical_or_self(&record.path));
    if !registered || !crate::worktree::is_git_worktree(&record.path) {
        return (WorktreeHealth::GitError, registered);
    }
    match egggit::status_v2::rich_repo_status(&record.path)
        .await
        .map(|fact| fact.into_value())
    {
        Ok(status)
            if !status.conflicted.is_empty()
                || status.operation_state.is_some()
                || status
                    .repository_operation_state
                    .as_ref()
                    .is_some_and(|state| {
                        !matches!(
                            state,
                            egggit::operation_state::RepositoryOperationState::None
                        )
                    }) =>
        {
            (WorktreeHealth::Conflicted, true)
        }
        Ok(status) if status.is_clean => (WorktreeHealth::Clean, true),
        Ok(_) => (WorktreeHealth::Dirty, true),
        Err(_) => (WorktreeHealth::GitError, true),
    }
}

fn canonical_repo_root(path: &Path) -> Result<PathBuf, WorktreeServiceError> {
    if !path.is_dir() {
        return Err(WorktreeServiceError::InvalidInput(format!(
            "repository root is not a directory: {}",
            path.display()
        )));
    }
    let root = crate::worktree::find_git_root(path)
        .and_then(|root| root.canonicalize().ok())
        .ok_or_else(|| {
            WorktreeServiceError::InvalidInput(format!("not a Git repository: {}", path.display()))
        })?;
    // Linked worktrees have a `.git` file pointing below the canonical
    // repository's common `.git` directory. Keep repository identity and
    // worktree allocation anchored at that common root.
    let git_entry = root.join(".git");
    if git_entry.is_file() {
        let contents = std::fs::read_to_string(&git_entry)
            .map_err(|e| WorktreeServiceError::Git(e.to_string()))?;
        if let Some(raw) = contents.strip_prefix("gitdir:") {
            let git_dir = PathBuf::from(raw.trim());
            let git_dir = if git_dir.is_absolute() {
                git_dir
            } else {
                root.join(git_dir)
            };
            if let Some(worktrees) = git_dir.parent() {
                if worktrees
                    .file_name()
                    .is_some_and(|name| name == "worktrees")
                {
                    if let Some(common_git) = worktrees.parent() {
                        if common_git.file_name().is_some_and(|name| name == ".git") {
                            if let Some(repository_root) = common_git.parent() {
                                return Ok(repository_root.to_path_buf());
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(root)
}

fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn ensure_safe_managed_path(root: &Path, path: &Path) -> Result<(), WorktreeServiceError> {
    let root = canonical_or_self(root);
    if !path.starts_with(&root) {
        return Err(WorktreeServiceError::UnsafeCleanup(
            "cleanup target is outside the managed root".into(),
        ));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::RootDir | Component::Prefix(_)) {
            continue;
        }
        if current.exists()
            && std::fs::symlink_metadata(&current)
                .map_err(|e| WorktreeServiceError::UnsafeCleanup(e.to_string()))?
                .file_type()
                .is_symlink()
        {
            return Err(WorktreeServiceError::UnsafeCleanup(format!(
                "symlink in cleanup path: {}",
                current.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_repo(dir: &Path) {
        // execution-ownership: test_only
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir)
            .output()
            .unwrap();
        // execution-ownership: test_only
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        // execution-ownership: test_only
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::fs::write(dir.join("README"), "base\n").unwrap();
        // execution-ownership: test_only
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        // execution-ownership: test_only
        Command::new("git")
            .args(["commit", "-m", "base"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    fn ids() -> (ProjectId, RepositoryId, WorkspaceId, AgentRunId) {
        (
            ProjectId::new(),
            RepositoryId::new(),
            WorkspaceId::new(),
            AgentRunId::new(),
        )
    }

    #[test]
    fn lifecycle_transition_matrix_rejects_reopening_removed() {
        assert!(!transition_allowed(
            ManagedWorktreeState::Removed,
            ManagedWorktreeState::Ready
        ));
        assert!(transition_allowed(
            ManagedWorktreeState::Ready,
            ManagedWorktreeState::InUse
        ));
    }

    #[test]
    fn generated_branch_uses_git_safe_identity_shape() {
        let branch = BranchName::new("codegg/worktree/abc123").unwrap();
        assert_eq!(branch.as_str(), "codegg/worktree/abc123");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn worktree_creation_fails_closed_without_containment_backend() {
        let repo = TempDir::new().unwrap();
        init_repo(repo.path());
        let managed = TempDir::new().unwrap();
        let (project, repository, workspace, _run) = ids();
        let service = WorktreeService::memory(
            managed.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
        );
        let error = service
            .create(&CreateWorktreeRequest {
                project_id: project,
                repository_id: repository,
                workspace_id: workspace,
                node_id: None,
                repository_root: repo.path().to_path_buf(),
                base_commit: None,
                base_ref: None,
                target_branch: None,
                base_path: None,
                owner_run_id: _run,
            })
            .await
            .expect_err("worktree add needs a contained Git backend");
        assert!(error.to_string().contains("Git backend unavailable"));
        assert!(error.to_string().contains("CleanSmudgeFilter"));
        let records = service.list(WorktreeQuery::default()).await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state, ManagedWorktreeState::Orphaned);
        assert_eq!(records[0].health, WorktreeHealth::GitError);
        assert!(!records[0].path.exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_generation_cannot_release_new_owner() {
        let store = Arc::new(InMemoryWorktreeStore::new());
        let now = Utc::now().timestamp_millis();
        let (project, repository, workspace, run_a) = ids();
        let run_b = AgentRunId::new();
        let id = WorktreeId::new();
        store
            .insert_reserved(WorktreeRecord {
                worktree_id: id.clone(),
                project_id: project,
                repository_id: repository.clone(),
                workspace_id: workspace.clone(),
                node_id: None,
                repository_root: PathBuf::from("/repo"),
                path: PathBuf::from("/managed/wt"),
                branch: Some("codegg/worktree/x".into()),
                base_commit: "a".repeat(40),
                managed: true,
                state: ManagedWorktreeState::Ready,
                health: WorktreeHealth::Clean,
                lease_generation: 0,
                owner_run_id: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
        let first = store.acquire_lease(&id, &run_a).await.unwrap();
        store
            .release_lease(&id, &run_a, first.generation)
            .await
            .unwrap();
        let second = store.acquire_lease(&id, &run_b).await.unwrap();
        assert!(matches!(
            store.release_lease(&id, &run_a, first.generation).await,
            Err(WorktreeServiceError::StaleGeneration { .. })
        ));
        assert_eq!(second.generation, 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dirty_worktree_is_not_cleanup_safe() {
        let repo = TempDir::new().unwrap();
        init_repo(repo.path());
        let managed = TempDir::new().unwrap();
        let (project, repository, workspace, run) = ids();
        let service = WorktreeService::memory(
            managed.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
        );
        let mut record = service
            .reserve(&CreateWorktreeRequest {
                project_id: project,
                repository_id: repository,
                workspace_id: workspace,
                node_id: None,
                repository_root: repo.path().to_path_buf(),
                base_commit: None,
                base_ref: None,
                target_branch: None,
                base_path: None,
                owner_run_id: run,
            })
            .await
            .unwrap();
        record.state = ManagedWorktreeState::Ready;
        record.health = WorktreeHealth::Clean;
        service.store.save(record.clone()).await.unwrap();
        let dirty = service
            .mark_health(&record.worktree_id, WorktreeHealth::Dirty)
            .await
            .unwrap();
        assert_eq!(dirty.state, ManagedWorktreeState::Orphaned);
        assert_eq!(dirty.health, WorktreeHealth::Dirty);
        assert!(!dirty.health.cleanup_safe());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn external_worktree_is_discovered_but_not_managed() {
        let repo = TempDir::new().unwrap();
        init_repo(repo.path());
        let external = TempDir::new().unwrap();
        let managed = TempDir::new().unwrap();
        let (_, repository, _, _) = ids();
        // execution-ownership: test_only
        Command::new("git")
            .args([
                "worktree",
                "add",
                external.path().to_str().unwrap(),
                "-b",
                "manual",
            ])
            .current_dir(repo.path())
            .output()
            .unwrap();
        let service = WorktreeService::memory(
            managed.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
        );
        let found = service.discover(repo.path(), &repository).await.unwrap();
        assert!(found
            .iter()
            .any(|item| item.path == canonical_or_self(repo.path()) && !item.managed));
        assert!(found
            .iter()
            .any(|item| item.path == canonical_or_self(external.path()) && !item.managed));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_worktree_creation_is_denied_without_containment_backend() {
        let repo = TempDir::new().unwrap();
        init_repo(repo.path());
        let managed = TempDir::new().unwrap();
        let (project, repository, workspace, run_a) = ids();
        let run_b = AgentRunId::new();
        let service = WorktreeService::memory(
            managed.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
        );
        let request = |owner_run_id| CreateWorktreeRequest {
            project_id: project.clone(),
            repository_id: repository.clone(),
            workspace_id: workspace.clone(),
            node_id: None,
            repository_root: repo.path().to_path_buf(),
            base_commit: None,
            base_ref: None,
            target_branch: None,
            base_path: None,
            owner_run_id,
        };
        let request_a = request(run_a);
        let request_b = request(run_b);
        let (first, second) = tokio::join!(service.create(&request_a), service.create(&request_b));
        for result in [first, second] {
            let error = result.expect_err("worktree add needs a contained Git backend");
            assert!(error.to_string().contains("Git backend unavailable"));
            assert!(error.to_string().contains("CleanSmudgeFilter"));
        }
        let records = service.list(WorktreeQuery::default()).await.unwrap();
        assert_eq!(records.len(), 2);
        assert_ne!(records[0].worktree_id, records[1].worktree_id);
        assert!(records
            .iter()
            .all(|record| record.state == ManagedWorktreeState::Orphaned));
        assert!(records.iter().all(|record| !record.path.exists()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn nested_worktree_creation_waits_for_contained_git_backend() {
        let repo = TempDir::new().unwrap();
        init_repo(repo.path());
        let managed = TempDir::new().unwrap();
        let (project, repository, workspace, parent_run) = ids();
        let service = WorktreeService::memory(
            managed.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
        );
        let error = service
            .create(&CreateWorktreeRequest {
                project_id: project,
                repository_id: repository,
                workspace_id: workspace,
                node_id: None,
                repository_root: repo.path().to_path_buf(),
                base_commit: None,
                base_ref: None,
                target_branch: None,
                base_path: None,
                owner_run_id: parent_run,
            })
            .await
            .expect_err("parent worktree requires a contained Git backend");
        assert!(error.to_string().contains("Git backend unavailable"));
        assert!(error.to_string().contains("CleanSmudgeFilter"));
        assert_eq!(
            service.list(WorktreeQuery::default()).await.unwrap().len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sqlite_store_round_trips_after_migration() {
        let root = TempDir::new().unwrap();
        let db_path = root.path().join("catalog.db");
        let pool = crate::storage::init_pool_at_for_migration(&db_path)
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();
        let managed = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        init_repo(repo.path());
        let (project, repository, workspace, run) = ids();
        let service = WorktreeService::with_store(
            Arc::new(SqliteWorktreeStore::new(pool)),
            managed.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
        );
        let record = service
            .reserve(&CreateWorktreeRequest {
                project_id: project,
                repository_id: repository.clone(),
                workspace_id: workspace.clone(),
                node_id: None,
                repository_root: repo.path().to_path_buf(),
                base_commit: None,
                base_ref: None,
                target_branch: None,
                base_path: None,
                owner_run_id: run,
            })
            .await
            .unwrap();
        let loaded = service.get(&record.worktree_id).await.unwrap();
        assert_eq!(loaded.owner_run_id, None);
        assert_eq!(loaded.lease_generation, 0);
        assert_eq!(loaded.state, ManagedWorktreeState::Reserved);
        assert_eq!(loaded.workspace_id, workspace);
        assert_eq!(loaded.repository_id, repository);
    }

    /// M005 corrective C008: the capped `list` surface hides the newest rows
    /// of a user catalog that holds more than `MAX_WORKTREE_LIST` non-removed
    /// rows, and the reservation authority must never read through it — a
    /// cap that hides a reserved checkout admits the writer.
    #[tokio::test(flavor = "current_thread")]
    async fn the_authority_read_finds_a_checkout_the_capped_list_hides() {
        let root = TempDir::new().unwrap();
        let db_path = root.path().join("catalog.db");
        let pool = crate::storage::init_pool_at_for_migration(&db_path)
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();
        let managed = TempDir::new().unwrap();
        let store = Arc::new(SqliteWorktreeStore::new(pool));
        let service = WorktreeService::with_store(
            store.clone(),
            managed.path().to_path_buf(),
            Arc::new(WorkspaceLockTable::new()),
        );
        let (project, repository, workspace, run) = ids();
        let now = Utc::now().timestamp_millis();
        // One row past the cap, so the newest checkout is the one the
        // oldest-first capped list drops.
        let total = MAX_WORKTREE_LIST + 1;
        let newest = WorktreeId::new();
        for index in 0..total {
            let worktree_id = if index + 1 == total {
                newest.clone()
            } else {
                WorktreeId::new()
            };
            store
                .insert_reserved(WorktreeRecord {
                    worktree_id,
                    project_id: project.clone(),
                    repository_id: repository.clone(),
                    workspace_id: workspace.clone(),
                    node_id: None,
                    repository_root: PathBuf::from("/repo"),
                    path: PathBuf::from(format!("/managed/wt-{index:05}")),
                    branch: Some("codegg/worktree/x".into()),
                    base_commit: "a".repeat(40),
                    managed: true,
                    state: ManagedWorktreeState::InUse,
                    health: WorktreeHealth::Clean,
                    lease_generation: 0,
                    owner_run_id: Some(run.clone()),
                    created_at: now + index as i64,
                    updated_at: now + index as i64,
                })
                .await
                .unwrap();
        }

        let capped = service.list(WorktreeQuery::default()).await.unwrap();
        assert_eq!(capped.len(), MAX_WORKTREE_LIST);
        assert!(
            !capped.iter().any(|record| record.worktree_id == newest),
            "the capped list is the defect this milestone closes: it drops the newest row"
        );

        let candidates = service.list_authority_candidates().await.unwrap();
        assert_eq!(candidates.len(), total);
        assert!(
            candidates.iter().any(|record| record.worktree_id == newest),
            "the authority read must see the newest checkout"
        );
    }
}
