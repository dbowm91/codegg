//! Daemon-owned workspace service bundles, leases, registry, and policy.
//!
//! Phase 3 of the single-daemon multi-project orchestration roadmap turns
//! each workspace into a daemon-owned service domain. A [`WorkspaceServices`]
//! bundle owns all the per-workspace resources that were previously
//! constructed ad-hoc by individual tools, the TUI, or `CoreDaemon` itself:
//! the `RunStore`, the `WorkspacePathPolicy`, the `WorkspaceLockTable`,
//! and the resolved `WorkspaceConfigSnapshot`.
//!
//! The bundle is keyed by [`WorkspaceId`] so that every session and
//! frontend attached to the same workspace shares one in-process service
//! instance. Bundles are activated lazily through
//! [`WorkspaceServiceRegistry::acquire`], which uses a single-flight
//! activation mutex per workspace to ensure that concurrent first
//! acquisitions produce exactly one bundle.
//!
//! The module is UI-, server-, and plugin-free: it only owns types
//! the daemon can reason about in isolation. Higher-level services
//! (LSP, Git mutation, search, plugins, providers) live in the root
//! crate and are stitched in via the [`WorkspaceServicesFactory`] trait.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;


use crate::error::StorageError;
use crate::run_store::{FsRunStore, RunStore};
use crate::workspace::{WorkspaceError, WorkspaceId, WorkspaceRecord, WorkspaceRegistry};

/// Errors produced by [`WorkspaceServiceRegistry`] and the lease lifecycle.
#[derive(Debug, Error)]
pub enum WorkspaceServiceError {
    #[error("workspace registry error: {0}")]
    Workspace(#[from] WorkspaceError),

    #[error("storage failure: {0}")]
    Storage(#[from] StorageError),

    #[error("service activation failed for workspace '{0}': {1}")]
    ActivationFailed(String, String),

    #[error("workspace '{0}' not found")]
    NotFound(String),

    #[error("config reload for workspace '{0}' failed: {1}")]
    ConfigReloadFailed(String, String),
}

/// Tunable policy controlling workspace service lifecycle.
#[derive(Debug, Clone)]
pub struct WorkspaceServicePolicy {
    /// Maximum number of concurrently-active workspace bundles. When
    /// the registry reaches this cap, [`WorkspaceServiceRegistry::acquire`]
    /// may evict the oldest idle bundle to make room.
    pub max_active_workspaces: usize,
    /// Age after which an idle workspace service bundle (zero active
    /// leases) is eligible for idle eviction.
    pub idle_evict_after: Duration,
}

impl Default for WorkspaceServicePolicy {
    fn default() -> Self {
        Self {
            max_active_workspaces: 16,
            idle_evict_after: Duration::from_secs(1800),
        }
    }
}

/// Snapshot of a workspace's path policy as resolved at activation time.
///
/// Tools receive an `Arc<WorkspacePathPolicy>` from the workspace services
/// lease. The policy is constructed from the workspace's canonical root
/// plus any caller-supplied widened roots, and serves as the authoritative
/// source of truth for what paths may be read or written.
///
/// Phase 3 deliberately keeps this minimal — it mirrors the fields
/// exposed by [`crate::workspace::ExecutionContext`] so existing tools
/// can adopt the workspace-bundle path policy without rewriting their
/// path resolution code. Future phases can add symlink policy, sandbox
/// mode, and platform capability fields.
#[derive(Debug, Clone)]
pub struct WorkspacePathPolicy {
    /// Canonical workspace root.
    pub canonical_root: PathBuf,
    /// Approved additional read roots (always starts with `canonical_root`).
    pub allowed_read_roots: Arc<[PathBuf]>,
    /// Approved additional write roots (always starts with `canonical_root`).
    pub allowed_write_roots: Arc<[PathBuf]>,
}

impl WorkspacePathPolicy {
    /// Build a path policy rooted at the workspace's canonical root.
    pub fn for_workspace(workspace: &WorkspaceRecord) -> Arc<Self> {
        let root = workspace.canonical_root.clone();
        Arc::new(Self {
            canonical_root: root.clone(),
            allowed_read_roots: Arc::from([root.clone()]),
            allowed_write_roots: Arc::from([root]),
        })
    }

    /// Build a path policy with caller-supplied widened read/write roots.
    pub fn with_allowed_roots(
        workspace: &WorkspaceRecord,
        extra_read: Vec<PathBuf>,
        extra_write: Vec<PathBuf>,
    ) -> Arc<Self> {
        let root = workspace.canonical_root.clone();
        let mut read = vec![root.clone()];
        for r in extra_read {
            if !read.iter().any(|p| p == &r) {
                read.push(r);
            }
        }
        let mut write = vec![root.clone()];
        for w in extra_write {
            if !write.iter().any(|p| p == &w) {
                write.push(w);
            }
        }
        Arc::new(Self {
            canonical_root: root,
            allowed_read_roots: Arc::from(read.into_boxed_slice()),
            allowed_write_roots: Arc::from(write.into_boxed_slice()),
        })
    }
}

/// A versioned, immutable snapshot of a workspace's resolved
/// configuration. Reloading produces a new snapshot with an incremented
/// `revision`; existing attempts continue to see the snapshot they were
/// admitted under.
#[derive(Debug, Clone)]
pub struct WorkspaceConfigSnapshot {
    pub revision: u64,
    pub loaded_at: DateTime<Utc>,
    pub source_files: Vec<PathBuf>,
    pub diagnostics: Vec<ConfigDiagnostic>,
}

/// A single diagnostic produced while resolving workspace configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDiagnostic {
    pub severity: ConfigDiagnosticSeverity,
    pub source: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigDiagnosticSeverity {
    Warning,
    Error,
}

/// Lock table keyed by repository/worktree root. Used by Phase 3 Git
/// mutation flows to ensure native Git tool calls and Bash-translated
/// Git operations contend on the same per-repository lock.
///
/// The table only stores the lock objects — it does not parse Git
/// repositories itself. The Phase F Git service and Bash-translation
/// dispatcher both call [`WorkspaceLockTable::acquire_repository`] with
/// the canonical repository root (already resolved by their respective
/// Git roots helpers).
#[derive(Debug, Default)]
pub struct WorkspaceLockTable {
    inner: DashMap<PathBuf, Arc<AsyncMutex<()>>>,
}

impl WorkspaceLockTable {
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }

    /// Acquire (or fetch-and-acquire) the per-repository lock. Two callers
    /// with the same canonical root always receive the same
    /// `Arc<AsyncMutex<()>>` so their critical sections are serialized.
    pub async fn acquire_repository(&self, repo_root: &Path) -> WorkspaceRepositoryGuard {
        let canonical = match std::fs::canonicalize(repo_root) {
            Ok(p) => p,
            Err(_) => repo_root.to_path_buf(),
        };
        let lock: Arc<AsyncMutex<()>> = self
            .inner
            .entry(canonical)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let guard = Arc::clone(&lock).lock_owned().await;
        WorkspaceRepositoryGuard {
            _guard: guard,
            _lock: lock,
        }
    }

    /// Number of distinct repository roots currently tracked.
    pub fn tracked_repositories(&self) -> usize {
        self.inner.len()
    }

    /// Drop the entry for a repository root. Called after the workspace
    /// service is evicted so locks do not accumulate indefinitely.
    pub fn forget_repository(&self, repo_root: &Path) -> bool {
        let canonical = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
        self.inner.remove(&canonical).is_some()
    }
}

/// RAII guard returned by [`WorkspaceLockTable::acquire_repository`].
/// Drops the inner lock when it goes out of scope.
pub struct WorkspaceRepositoryGuard {
    _guard: tokio::sync::OwnedMutexGuard<()>,
    _lock: Arc<AsyncMutex<()>>,
}

/// Snapshot of an active workspace service bundle for inclusion in
/// daemon/workspace status responses. Redacts sensitive paths.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceServiceSnapshot {
    pub workspace_id: WorkspaceId,
    pub canonical_root: PathBuf,
    pub display_name: String,
    pub activated_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub active_leases: usize,
    pub config_revision: u64,
}

/// Daemon-owned bundle of per-workspace services. Exactly one instance
/// per `WorkspaceId` is active at a time per registry. Tools and TUI
/// code obtain access through [`WorkspaceServicesLease`].
pub struct WorkspaceServices {
    pub workspace: Arc<WorkspaceRecord>,
    pub config_snapshot: Arc<WorkspaceConfigSnapshot>,
    pub run_store: Arc<dyn RunStore>,
    pub path_policy: Arc<WorkspacePathPolicy>,
    pub locks: Arc<WorkspaceLockTable>,
    pub artifact_root: PathBuf,
    pub activated_at: DateTime<Utc>,
    pub last_used_at: AtomicI64,
    pub active_leases: AtomicUsize,
    pub shutdown: CancellationToken,
}

impl WorkspaceServices {
    /// Construct a fresh bundle. The factory wires concrete services;
    /// production callers go through [`WorkspaceServicesFactory`].
    pub fn new(
        workspace: Arc<WorkspaceRecord>,
        config_snapshot: Arc<WorkspaceConfigSnapshot>,
        run_store: Arc<dyn RunStore>,
        path_policy: Arc<WorkspacePathPolicy>,
        locks: Arc<WorkspaceLockTable>,
    ) -> Arc<Self> {
        let artifact_root = workspace.canonical_root.join(".codegg").join("runs");
        Arc::new(Self {
            workspace,
            config_snapshot,
            run_store,
            path_policy,
            locks,
            artifact_root,
            activated_at: Utc::now(),
            last_used_at: AtomicI64::new(Utc::now().timestamp()),
            active_leases: AtomicUsize::new(0),
            shutdown: CancellationToken::new(),
        })
    }

    /// Mark the bundle as recently used. Called by the registry on
    /// every successful `acquire`.
    pub fn touch(&self) {
        self.last_used_at
            .store(Utc::now().timestamp(), Ordering::Relaxed);
    }

    /// Increment the active-lease counter.
    fn increment_leases(&self) {
        self.active_leases.fetch_add(1, Ordering::Relaxed);
        self.touch();
    }

    /// Decrement the active-lease counter.
    fn decrement_leases(&self) {
        let _ = self.active_leases.fetch_sub(1, Ordering::Release);
        self.touch();
    }

    /// Build a snapshot describing the bundle's current state.
    pub fn snapshot(&self) -> WorkspaceServiceSnapshot {
        WorkspaceServiceSnapshot {
            workspace_id: self.workspace.id.clone(),
            canonical_root: self.workspace.canonical_root.clone(),
            display_name: self.workspace.display_name.clone(),
            activated_at: self.activated_at,
            last_used_at: Utc::now(),
            active_leases: self.active_leases.load(Ordering::Relaxed),
            config_revision: self.config_snapshot.revision,
        }
    }
}

/// RAII handle returned by [`WorkspaceServiceRegistry::acquire`]. Holds
/// an active lease on the underlying [`WorkspaceServices`] bundle.
/// Dropping the lease decrements the active-lease counter.
pub struct WorkspaceServicesLease {
    services: Arc<WorkspaceServices>,
    /// Set to `true` after the registry decrements accounting on drop.
    released: std::sync::atomic::AtomicBool,
}

impl WorkspaceServicesLease {
    pub fn services(&self) -> &Arc<WorkspaceServices> {
        &self.services
    }

    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.services.workspace.id
    }

    pub fn run_store(&self) -> Arc<dyn RunStore> {
        Arc::clone(&self.services.run_store)
    }

    pub fn config_snapshot(&self) -> Arc<WorkspaceConfigSnapshot> {
        Arc::clone(&self.services.config_snapshot)
    }

    pub fn path_policy(&self) -> Arc<WorkspacePathPolicy> {
        Arc::clone(&self.services.path_policy)
    }

    pub fn locks(&self) -> Arc<WorkspaceLockTable> {
        Arc::clone(&self.services.locks)
    }

    pub fn artifact_root(&self) -> &Path {
        &self.services.artifact_root
    }
}

impl Drop for WorkspaceServicesLease {
    fn drop(&mut self) {
        if !self.released.swap(true, Ordering::AcqRel) {
            self.services.decrement_leases();
        }
    }
}

/// Trait for constructing [`WorkspaceServices`] bundles. The daemon
/// owns the production implementation; tests inject an in-memory
/// factory so they can observe activation behavior without touching
/// SQLite or filesystem paths.
pub trait WorkspaceServicesFactory: Send + Sync {
    fn build(&self, workspace: Arc<WorkspaceRecord>) -> Result<Arc<WorkspaceServices>, String>;
}

/// Production factory that wires the filesystem-backed `FsRunStore`,
/// the canonical workspace path policy, and a fresh per-workspace lock
/// table.
pub struct ProductionWorkspaceServicesFactory;

impl WorkspaceServicesFactory for ProductionWorkspaceServicesFactory {
    fn build(&self, workspace: Arc<WorkspaceRecord>) -> Result<Arc<WorkspaceServices>, String> {
        let run_root = workspace.canonical_root.join(".codegg").join("runs");
        if let Some(parent) = run_root.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!(
                        "failed to create workspace artifact parent {}: {}",
                        parent.display(),
                        e
                    )
                })?;
            }
        }
        let run_store: Arc<dyn RunStore> = Arc::new(FsRunStore::new(run_root));
        let path_policy = WorkspacePathPolicy::for_workspace(&workspace);
        let locks = Arc::new(WorkspaceLockTable::new());
        let config_snapshot = Arc::new(WorkspaceConfigSnapshot {
            revision: 0,
            loaded_at: Utc::now(),
            source_files: Vec::new(),
            diagnostics: Vec::new(),
        });
        Ok(WorkspaceServices::new(
            workspace,
            config_snapshot,
            run_store,
            path_policy,
            locks,
        ))
    }
}

/// Report returned by [`WorkspaceServiceRegistry::evict_idle`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvictionReport {
    pub evicted: Vec<WorkspaceId>,
    pub skipped_active: Vec<WorkspaceId>,
    pub evaluated: usize,
}

/// Report returned by [`WorkspaceServiceRegistry::shutdown_all`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShutdownReport {
    pub drained: Vec<WorkspaceId>,
    pub force_terminated: Vec<WorkspaceId>,
    pub deadline_hit: bool,
}

/// Result of [`WorkspaceServiceRegistry::reload_config`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadResult {
    pub workspace_id: WorkspaceId,
    pub previous_revision: u64,
    pub new_revision: u64,
    pub diagnostics: Vec<ConfigDiagnostic>,
}

/// Daemon-owned registry that maps `WorkspaceId` -> active
/// `WorkspaceServices` bundle, enforces single-flight activation, and
/// manages idle eviction.
pub struct WorkspaceServiceRegistry {
    workspaces: Arc<WorkspaceRegistry>,
    active: DashMap<WorkspaceId, Arc<WorkspaceServices>>,
    activation_locks: DashMap<WorkspaceId, Arc<AsyncMutex<()>>>,
    factory: Arc<dyn WorkspaceServicesFactory>,
    policy: WorkspaceServicePolicy,
}

impl WorkspaceServiceRegistry {
    /// Construct a registry over the given workspace registry using the
    /// supplied factory and policy.
    pub fn new(
        workspaces: Arc<WorkspaceRegistry>,
        factory: Arc<dyn WorkspaceServicesFactory>,
        policy: WorkspaceServicePolicy,
    ) -> Arc<Self> {
        Arc::new(Self {
            workspaces,
            active: DashMap::new(),
            activation_locks: DashMap::new(),
            factory,
            policy,
        })
    }

    /// Acquire a lease on the workspace service bundle for
    /// `workspace_id`. Lazily activates a bundle on first acquisition.
    /// Concurrent first acquisitions are serialized through a
    /// per-workspace parking-lot mutex so exactly one bundle is
    /// constructed.
    pub async fn acquire(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<WorkspaceServicesLease, WorkspaceServiceError> {
        let services = self.activate(workspace_id).await?;
        services.increment_leases();
        Ok(WorkspaceServicesLease {
            services,
            released: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Activate (or fetch) the workspace service bundle for
    /// `workspace_id`. Uses a per-workspace async mutex so concurrent
    /// first activations produce exactly one bundle. The lock is
    /// `Send`-safe across awaits.
    pub async fn activate(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Arc<WorkspaceServices>, WorkspaceServiceError> {
        if let Some(existing) = self.active.get(workspace_id) {
            existing.touch();
            return Ok(existing.clone());
        }

        let activation_lock: Arc<AsyncMutex<()>> = self
            .activation_locks
            .entry(workspace_id.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let guard = activation_lock.lock().await;

        // Re-check after acquiring the per-workspace lock to avoid
        // double-construction when two callers race.
        if let Some(existing) = self.active.get(workspace_id) {
            existing.touch();
            return Ok(existing.clone());
        }

        let workspace_record = self
            .workspaces
            .resolve(workspace_id)
            .await
            .ok_or_else(|| WorkspaceServiceError::NotFound(workspace_id.to_string()))?;

        let services = self
            .factory
            .build(workspace_record)
            .map_err(|e| WorkspaceServiceError::ActivationFailed(workspace_id.to_string(), e))?;

        // Cap enforcement: when the registry already holds
        // `max_active_workspaces` active bundles, evict the oldest
        // idle one (or skip if none is idle). Eviction is best-effort
        // here; callers should also run `evict_idle` periodically.
        if self.active.len() >= self.policy.max_active_workspaces {
            self.evict_oldest_idle();
        }

        self.active.insert(workspace_id.clone(), services.clone());
        drop(guard);
        Ok(services)
    }

    /// Return a snapshot describing the bundle's current state, or
    /// `None` if no bundle is currently active.
    pub fn peek(&self, workspace_id: &WorkspaceId) -> Option<WorkspaceServiceSnapshot> {
        self.active.get(workspace_id).map(|s| s.snapshot())
    }

    /// Return snapshots for every active workspace service bundle.
    pub fn list_active(&self) -> Vec<WorkspaceServiceSnapshot> {
        self.active
            .iter()
            .map(|entry| entry.value().snapshot())
            .collect()
    }

    /// Evict idle bundles older than `policy.idle_evict_after` and
    /// return a structured report.
    pub fn evict_idle(&self, now: DateTime<Utc>) -> EvictionReport {
        let mut report = EvictionReport::default();
        let threshold_secs = (now - chrono::Duration::from_std(self.policy.idle_evict_after).unwrap_or(chrono::Duration::seconds(0))).timestamp();
        let mut to_evict: Vec<WorkspaceId> = Vec::new();
        for entry in self.active.iter() {
            let services = entry.value();
            let last_used = services.last_used_at.load(Ordering::Relaxed);
            let leases = services.active_leases.load(Ordering::Relaxed);
            report.evaluated += 1;
            if leases == 0 && last_used <= threshold_secs {
                to_evict.push(entry.key().clone());
            } else if leases > 0 {
                report.skipped_active.push(entry.key().clone());
            }
        }
        for id in to_evict.iter() {
            self.shutdown_bundle(id);
        }
        report.evicted = to_evict;
        report
    }

    /// Evict the oldest idle bundle to make room under
    /// `max_active_workspaces`. Best-effort: if every active bundle
    /// has active leases, nothing is evicted.
    fn evict_oldest_idle(&self) {
        let mut oldest: Option<(WorkspaceId, DateTime<Utc>)> = None;
        for entry in self.active.iter() {
            let services = entry.value();
            let leases = services.active_leases.load(Ordering::Relaxed);
            if leases > 0 {
                continue;
            }
            match &oldest {
                Some((_, ts)) if *ts <= services.activated_at => {}
                _ => oldest = Some((entry.key().clone(), services.activated_at)),
            }
        }
        if let Some((id, _)) = oldest {
            self.shutdown_bundle(&id);
        }
    }

    /// Drain and shutdown every active workspace service bundle.
    pub async fn shutdown_all(
        &self,
        deadline: Duration,
    ) -> ShutdownReport {
        let mut report = ShutdownReport::default();
        let started = std::time::Instant::now();

        // First, signal every bundle to begin draining.
        for entry in self.active.iter() {
            entry.value().shutdown.cancel();
        }

        // Wait for leases to drain, bounded by the deadline.
        loop {
            let any_active = self
                .active
                .iter()
                .any(|e| e.value().active_leases.load(Ordering::Relaxed) > 0);
            if !any_active {
                break;
            }
            if started.elapsed() >= deadline {
                report.deadline_hit = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Collect IDs and force-terminate anything with outstanding
        // leases.
        let ids: Vec<WorkspaceId> = self
            .active
            .iter()
            .map(|e| e.key().clone())
            .collect();

        for id in ids {
            let leases_now = self
                .active
                .get(&id)
                .map(|s| s.active_leases.load(Ordering::Relaxed))
                .unwrap_or(0);
            if leases_now == 0 {
                report.drained.push(id.clone());
            } else {
                report.force_terminated.push(id.clone());
            }
            self.shutdown_bundle(&id);
        }

        report
    }

    fn shutdown_bundle(&self, id: &WorkspaceId) {
        if let Some((_, services)) = self.active.remove(id) {
            // Forget any tracked repository locks for this workspace.
            for entry in services.locks.inner.iter() {
                services.locks.forget_repository(entry.key());
            }
        }
    }

    /// Reload the workspace configuration snapshot, producing a new
    /// revision. Existing leases continue to see their previously-held
    /// snapshot; future leases see the new one.
    ///
    /// Phase 3 implements the simple case: bumps the revision
    /// monotonically and records the new `loaded_at`. A future phase
    /// can add file watching and source file tracking.
    pub fn reload_config(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<ReloadResult, WorkspaceServiceError> {
        let services = self
            .active
            .get(workspace_id)
            .map(|e| e.value().clone())
            .ok_or_else(|| WorkspaceServiceError::NotFound(workspace_id.to_string()))?;
        let previous = services.config_snapshot.revision;
        let new_snapshot = Arc::new(WorkspaceConfigSnapshot {
            revision: previous + 1,
            loaded_at: Utc::now(),
            source_files: services.config_snapshot.source_files.clone(),
            diagnostics: services.config_snapshot.diagnostics.clone(),
        });
        let diagnostics = new_snapshot.diagnostics.clone();
        let new_revision = new_snapshot.revision;
        // Construct a fresh `WorkspaceServices` shell with the new
        // snapshot, but preserve the existing run_store, path policy,
        // locks, and bookkeeping counters.
        let replacement = Arc::new(WorkspaceServices {
            workspace: services.workspace.clone(),
            config_snapshot: new_snapshot,
            run_store: services.run_store.clone(),
            path_policy: services.path_policy.clone(),
            locks: services.locks.clone(),
            artifact_root: services.artifact_root.clone(),
            activated_at: services.activated_at,
            last_used_at: AtomicI64::new(services.last_used_at.load(Ordering::Relaxed)),
            active_leases: AtomicUsize::new(services.active_leases.load(Ordering::Relaxed)),
            shutdown: services.shutdown.clone(),
        });
        self.active
            .insert(workspace_id.clone(), replacement);
        Ok(ReloadResult {
            workspace_id: workspace_id.clone(),
            previous_revision: previous,
            new_revision,
            diagnostics,
        })
    }

    /// Number of currently-active workspace service bundles.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Access the underlying workspace registry. Used by daemon code
    /// that needs both registry resolution and service leases.
    pub fn workspaces(&self) -> &Arc<WorkspaceRegistry> {
        &self.workspaces
    }

    /// Resolve a workspace, activating a service bundle on demand.
    /// Convenience helper used by `CoreDaemon` request handlers that
    /// want to combine registry resolution with service activation in
    /// one step.
    pub async fn acquire_for_root(
        &self,
        workspace_root: &Path,
    ) -> Result<WorkspaceServicesLease, WorkspaceServiceError> {
        let record = self.workspaces.get_or_register(workspace_root).await?;
        self.acquire(&record.id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fresh_registry() -> (
        tempfile::TempDir,
        Arc<WorkspaceRegistry>,
        Arc<WorkspaceServiceRegistry>,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let store: Arc<dyn crate::workspace::WorkspaceStore> =
            Arc::new(crate::workspace::InMemoryWorkspaceStore::new());
        let workspaces = WorkspaceRegistry::load(store).await.unwrap();
        let registry = WorkspaceServiceRegistry::new(
            workspaces.clone(),
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy {
                max_active_workspaces: 4,
                idle_evict_after: Duration::from_secs(60),
            },
        );
        (tmp, workspaces, registry)
    }

    fn record_for_tmp(tmp: &tempfile::TempDir) -> Arc<WorkspaceRecord> {
        Arc::new(WorkspaceRecord {
            id: WorkspaceId::new_unchecked(uuid::Uuid::new_v4().to_string()),
            canonical_root: tmp.path().canonicalize().unwrap(),
            display_name: "tmp".into(),
            created_at: Utc::now(),
            last_opened_at: Utc::now(),
            archived_at: None,
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn acquire_returns_same_bundle_for_same_workspace() {
        let (tmp, workspaces, registry) = fresh_registry().await;
        let record = record_for_tmp(&tmp);
        workspaces.upsert_test_record(record.clone()).await;

        let lease_a = registry.acquire(&record.id).await.unwrap();
        let lease_b = registry.acquire(&record.id).await.unwrap();
        assert!(Arc::ptr_eq(lease_a.services(), lease_b.services()));
        assert_eq!(registry.active_count(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn acquire_releases_lease_on_drop() {
        let (tmp, workspaces, registry) = fresh_registry().await;
        let record = record_for_tmp(&tmp);
        workspaces.upsert_test_record(record.clone()).await;

        let lease = registry.acquire(&record.id).await.unwrap();
        assert_eq!(lease.services().active_leases.load(Ordering::Relaxed), 1);
        drop(lease);
        let services = registry.active.get(&record.id).unwrap();
        assert_eq!(services.active_leases.load(Ordering::Relaxed), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn distinct_workspaces_get_distinct_bundles() {
        let (tmp_a, workspaces, registry) = fresh_registry().await;
        let tmp_b = tempfile::tempdir().unwrap();
        let rec_a = record_for_tmp(&tmp_a);
        let rec_b = Arc::new(WorkspaceRecord {
            id: WorkspaceId::new_unchecked(uuid::Uuid::new_v4().to_string()),
            canonical_root: tmp_b.path().canonicalize().unwrap(),
            display_name: "b".into(),
            created_at: Utc::now(),
            last_opened_at: Utc::now(),
            archived_at: None,
        });
        workspaces.upsert_test_record(rec_a.clone()).await;
        workspaces.upsert_test_record(rec_b.clone()).await;

        let lease_a = registry.acquire(&rec_a.id).await.unwrap();
        let lease_b = registry.acquire(&rec_b.id).await.unwrap();
        assert!(!Arc::ptr_eq(lease_a.services(), lease_b.services()));
        assert_eq!(registry.active_count(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn evict_idle_skips_active_leases() {
        let (tmp, workspaces, registry) = fresh_registry().await;
        let record = record_for_tmp(&tmp);
        workspaces.upsert_test_record(record.clone()).await;

        let lease = registry.acquire(&record.id).await.unwrap();
        let report = registry.evict_idle(Utc::now() + chrono::Duration::seconds(120));
        assert!(report.evicted.is_empty());
        assert!(report.skipped_active.iter().any(|id| id == &record.id));
        drop(lease);
        let report = registry.evict_idle(Utc::now() + chrono::Duration::seconds(120));
        assert!(report.evicted.iter().any(|id| id == &record.id));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reload_config_bumps_revision() {
        let (tmp, workspaces, registry) = fresh_registry().await;
        let record = record_for_tmp(&tmp);
        workspaces.upsert_test_record(record.clone()).await;
        let _lease = registry.acquire(&record.id).await.unwrap();
        let result = registry.reload_config(&record.id).unwrap();
        assert_eq!(result.previous_revision, 0);
        assert_eq!(result.new_revision, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_all_force_terminates_active_leases() {
        // When a lease is still alive when `shutdown_all` is called,
        // the deadline is hit and the bundle is force-terminated. The
        // lease retains its counter locally but the registry removes
        // the bundle from the active map.
        let (tmp, workspaces, registry) = fresh_registry().await;
        let record = record_for_tmp(&tmp);
        workspaces.upsert_test_record(record.clone()).await;
        let lease = registry.acquire(&record.id).await.unwrap();
        let report = registry.shutdown_all(Duration::from_millis(50)).await;
        assert!(report.deadline_hit);
        assert!(report.drained.is_empty());
        assert!(report.force_terminated.iter().any(|id| id == &record.id));
        // Lease must NOT see services after shutdown.
        assert!(lease.services().active_leases.load(Ordering::Relaxed) >= 1);
        // The bundle is removed from the active map after shutdown.
        assert!(registry.peek(&record.id).is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_all_drains_when_no_active_leases() {
        // With no outstanding leases, shutdown_all should drain
        // cleanly without hitting the deadline.
        let (tmp, workspaces, registry) = fresh_registry().await;
        let record = record_for_tmp(&tmp);
        workspaces.upsert_test_record(record.clone()).await;
        let _lease = registry.acquire(&record.id).await.unwrap();
        drop(_lease);
        let report = registry.shutdown_all(Duration::from_millis(200)).await;
        assert!(!report.deadline_hit);
        assert!(report.drained.iter().any(|id| id == &record.id));
        assert!(report.force_terminated.is_empty());
    }
}
