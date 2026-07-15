//! Workspace identity, registry, path policy, and immutable execution context.
//!
//! Phase 2 of the single-daemon multi-project orchestration roadmap makes
//! workspace identity a first-class concept. A `WorkspaceId` is a stable,
//! opaque identifier for a canonical project root. Every persisted session is
//! bound to exactly one workspace. The daemon's [`WorkspaceRegistry`]
//! deduplicates canonical roots (rejecting symlink/relative aliases) and
//! keeps active workspace state. An [`ExecutionContext`] is passed by `Arc`
//! through every daemon-owned execution path so that commands, tools, and
//! subagents never infer their working directory from process-global cwd.
//!
//! The module is UI-, server-, and plugin-free: it is the lowest level at
//! which the daemon reasons about "which project is the agent working on".
//! All path checks happen through [`ExecutionContext::resolve_relative_cwd`],
//! [`ExecutionContext::resolve_read_path`], and
//! [`ExecutionContext::resolve_write_path`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::error::StorageError;

/// Opaque, stable identifier for a registered workspace.
///
/// The string content is a UUIDv4 produced by the registry at registration
/// time. It is intentionally not derived from the canonical path so that
/// display paths can change (renames, host moves) without renaming
/// identifiers. Equality is structural (string compare) and the type is
/// `Hash` so it can be used as a map key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    /// Wrap an already-validated identifier. Prefer
    /// [`WorkspaceRegistry::register`] for new workspaces.
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Persisted registry entry for a single workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub id: WorkspaceId,
    pub canonical_root: PathBuf,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    pub last_opened_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

impl WorkspaceRecord {
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

/// Errors produced by the path policy helpers on [`ExecutionContext`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathPolicyError {
    #[error("path '{0}' escapes workspace root '{1}'")]
    OutsideWorkspace(PathBuf, PathBuf),

    #[error("absolute path '{0}' is not under any allowed root")]
    OutsideAllowedRoots(PathBuf),

    #[error("path '{0}' traverses through an unsupported alias (symlink/hardlink)")]
    UnsupportedAlias(PathBuf),

    #[error("path '{0}' could not be canonicalized: {1}")]
    CanonicalizationFailed(PathBuf, String),

    #[error("empty path is not permitted")]
    EmptyPath,

    #[error("path contains NUL byte")]
    ContainsNul,
}

/// Errors produced by [`WorkspaceRegistry`] operations.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace root '{0}' does not exist or is not a directory")]
    NotADirectory(PathBuf),

    #[error("workspace root '{0}' could not be canonicalized: {1}")]
    CanonicalizationFailed(PathBuf, String),

    #[error("workspace '{0}' not found")]
    NotFound(String),

    #[error("storage failure: {0}")]
    Storage(#[from] StorageError),
}

/// Storage backend trait for `WorkspaceRegistry`.
///
/// The default implementation [`SqliteWorkspaceStore`] persists into the
/// same database as sessions. Tests can substitute an in-memory store
/// without touching SQLite.
#[async_trait::async_trait]
pub trait WorkspaceStore: Send + Sync {
    async fn upsert(&self, record: &WorkspaceRecord) -> Result<(), WorkspaceError>;
    async fn load_by_id(&self, id: &str) -> Result<Option<WorkspaceRecord>, WorkspaceError>;
    async fn load_by_canonical_root(
        &self,
        canonical_root: &Path,
    ) -> Result<Option<WorkspaceRecord>, WorkspaceError>;
    async fn list(&self, include_archived: bool) -> Result<Vec<WorkspaceRecord>, WorkspaceError>;
    async fn archive(&self, id: &str) -> Result<(), WorkspaceError>;
    async fn touch_last_opened(&self, id: &str) -> Result<(), WorkspaceError>;
}

/// SQLite-backed implementation of [`WorkspaceStore`].
///
/// The table layout is created by schema migration v22. The store uses the
/// existing session database (`SqlitePool`) so workspace and session rows
/// share a single canonical database file per workspace root. Phase 3 will
/// migrate to a user-scoped catalog; this trait abstraction lets us swap
/// the implementation without touching the registry.
pub struct SqliteWorkspaceStore {
    pool: sqlx::SqlitePool,
}

impl SqliteWorkspaceStore {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl WorkspaceStore for SqliteWorkspaceStore {
    async fn upsert(&self, record: &WorkspaceRecord) -> Result<(), WorkspaceError> {
        sqlx::query(
            r#"
            INSERT INTO workspace (
                id, canonical_root, display_name,
                time_created, time_last_opened, time_archived
            ) VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                canonical_root = excluded.canonical_root,
                display_name = excluded.display_name,
                time_last_opened = excluded.time_last_opened,
                time_archived = excluded.time_archived
            "#,
        )
        .bind(record.id.as_str())
        .bind(record.canonical_root.as_os_str().to_string_lossy().as_ref())
        .bind(&record.display_name)
        .bind(record.created_at.timestamp_millis())
        .bind(record.last_opened_at.timestamp_millis())
        .bind(record.archived_at.map(|d| d.timestamp_millis()))
        .execute(&self.pool)
        .await
        .map_err(|e| WorkspaceError::Storage(StorageError::Database(e.to_string())))?;
        Ok(())
    }

    async fn load_by_id(&self, id: &str) -> Result<Option<WorkspaceRecord>, WorkspaceError> {
        let row: Option<(String, String, String, i64, i64, Option<i64>)> = sqlx::query_as(
            "SELECT id, canonical_root, display_name, time_created, time_last_opened, time_archived FROM workspace WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| WorkspaceError::Storage(StorageError::Database(e.to_string())))?;

        Ok(row.and_then(|r| {
            Some(WorkspaceRecord {
                id: WorkspaceId::new_unchecked(r.0),
                canonical_root: PathBuf::from(r.1),
                display_name: r.2,
                created_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.3)?,
                last_opened_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.4)?,
                archived_at: r
                    .5
                    .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis),
            })
        }))
    }

    async fn load_by_canonical_root(
        &self,
        canonical_root: &Path,
    ) -> Result<Option<WorkspaceRecord>, WorkspaceError> {
        let row: Option<(String, String, String, i64, i64, Option<i64>)> = sqlx::query_as(
            "SELECT id, canonical_root, display_name, time_created, time_last_opened, time_archived FROM workspace WHERE canonical_root = ?",
        )
        .bind(canonical_root.as_os_str().to_string_lossy().as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| WorkspaceError::Storage(StorageError::Database(e.to_string())))?;

        Ok(row.and_then(|r| {
            Some(WorkspaceRecord {
                id: WorkspaceId::new_unchecked(r.0),
                canonical_root: PathBuf::from(r.1),
                display_name: r.2,
                created_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.3)?,
                last_opened_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.4)?,
                archived_at: r
                    .5
                    .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis),
            })
        }))
    }

    async fn list(&self, include_archived: bool) -> Result<Vec<WorkspaceRecord>, WorkspaceError> {
        let rows: Vec<(String, String, String, i64, i64, Option<i64>)> = if include_archived {
            sqlx::query_as(
                "SELECT id, canonical_root, display_name, time_created, time_last_opened, time_archived FROM workspace ORDER BY time_last_opened DESC",
            )
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as(
                "SELECT id, canonical_root, display_name, time_created, time_last_opened, time_archived FROM workspace WHERE time_archived IS NULL ORDER BY time_last_opened DESC",
            )
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| WorkspaceError::Storage(StorageError::Database(e.to_string())))?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(WorkspaceRecord {
                    id: WorkspaceId::new_unchecked(r.0),
                    canonical_root: PathBuf::from(r.1),
                    display_name: r.2,
                    created_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.3)?,
                    last_opened_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.4)?,
                    archived_at: r
                        .5
                        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis),
                })
            })
            .collect())
    }

    async fn archive(&self, id: &str) -> Result<(), WorkspaceError> {
        sqlx::query(
            "UPDATE workspace SET time_archived = ? WHERE id = ? AND time_archived IS NULL",
        )
        .bind(Utc::now().timestamp_millis())
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| WorkspaceError::Storage(StorageError::Database(e.to_string())))?;
        Ok(())
    }

    async fn touch_last_opened(&self, id: &str) -> Result<(), WorkspaceError> {
        sqlx::query("UPDATE workspace SET time_last_opened = ? WHERE id = ?")
            .bind(Utc::now().timestamp_millis())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| WorkspaceError::Storage(StorageError::Database(e.to_string())))?;
        Ok(())
    }
}

/// In-memory implementation of [`WorkspaceStore`] for unit tests.
#[derive(Default)]
pub struct InMemoryWorkspaceStore {
    inner: Mutex<Vec<WorkspaceRecord>>,
}

impl InMemoryWorkspaceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl WorkspaceStore for InMemoryWorkspaceStore {
    async fn upsert(&self, record: &WorkspaceRecord) -> Result<(), WorkspaceError> {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.iter_mut().find(|r| r.id == record.id) {
            *existing = record.clone();
        } else {
            guard.push(record.clone());
        }
        Ok(())
    }

    async fn load_by_id(&self, id: &str) -> Result<Option<WorkspaceRecord>, WorkspaceError> {
        let guard = self.inner.lock().await;
        Ok(guard.iter().find(|r| r.id.as_str() == id).cloned())
    }

    async fn load_by_canonical_root(
        &self,
        canonical_root: &Path,
    ) -> Result<Option<WorkspaceRecord>, WorkspaceError> {
        let guard = self.inner.lock().await;
        Ok(guard
            .iter()
            .find(|r| r.canonical_root == canonical_root)
            .cloned())
    }

    async fn list(&self, include_archived: bool) -> Result<Vec<WorkspaceRecord>, WorkspaceError> {
        let guard = self.inner.lock().await;
        Ok(guard
            .iter()
            .filter(|r| include_archived || !r.is_archived())
            .cloned()
            .collect())
    }

    async fn archive(&self, id: &str) -> Result<(), WorkspaceError> {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.iter_mut().find(|r| r.id.as_str() == id) {
            if existing.archived_at.is_none() {
                existing.archived_at = Some(Utc::now());
            }
        }
        Ok(())
    }

    async fn touch_last_opened(&self, id: &str) -> Result<(), WorkspaceError> {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.iter_mut().find(|r| r.id.as_str() == id) {
            existing.last_opened_at = Utc::now();
        }
        Ok(())
    }
}

/// Daemon-owned registry: dedupes canonical roots, owns active workspace
/// state, and persists via the supplied [`WorkspaceStore`].
///
/// `register` canonicalizes the input path, rejects nonexistent /
/// non-directory / symlink-escaping inputs, and atomically returns an
/// existing record if one already points at the canonical root. This is
/// the single producer of [`WorkspaceId`] values. Once a workspace is
/// registered, all subsequent reads come from the registry's in-memory
/// index, not the store.
pub struct WorkspaceRegistry {
    store: Arc<dyn WorkspaceStore>,
    by_id: DashMap<String, Arc<WorkspaceRecord>>,
    by_root: DashMap<PathBuf, WorkspaceId>,
}

impl WorkspaceRegistry {
    /// Build a registry backed by the given store. Hydrates the in-memory
    /// index from the store so that subsequent lookups are O(1).
    pub async fn load(store: Arc<dyn WorkspaceStore>) -> Result<Arc<Self>, WorkspaceError> {
        let records = store.list(true).await?;
        let registry = Arc::new(Self {
            store,
            by_id: DashMap::new(),
            by_root: DashMap::new(),
        });
        for record in records {
            registry
                .by_id
                .insert(record.id.as_str().to_string(), Arc::new(record.clone()));
            registry
                .by_root
                .insert(record.canonical_root.clone(), record.id.clone());
        }
        Ok(registry)
    }

    /// Construct a registry with the given store but without hydrating
    /// the in-memory index. The caller must call
    /// [`WorkspaceRegistry::hydrate_from_store`] to load existing rows.
    /// Construction is infallible (no async work), which is convenient
    /// inside constructors that can't easily `await`.
    pub fn new_for_tests(store: Arc<dyn WorkspaceStore>) -> Arc<Self> {
        Arc::new(Self {
            store,
            by_id: DashMap::new(),
            by_root: DashMap::new(),
        })
    }

    /// Hydrate the in-memory index from the backing store.
    pub async fn hydrate_from_store(&self) -> Result<(), WorkspaceError> {
        let records = self.store.list(true).await?;
        for record in records {
            let id_str = record.id.as_str().to_string();
            let canon = record.canonical_root.clone();
            if !self.by_id.contains_key(&id_str) {
                self.by_id.insert(id_str, Arc::new(record.clone()));
            }
            if !self.by_root.contains_key(&canon) {
                self.by_root.insert(canon, record.id.clone());
            }
        }
        Ok(())
    }

    /// Register a new workspace rooted at `root`, or return the existing
    /// record if one already targets the canonical path. Refreshes
    /// `last_opened_at` on existing records so the UI lists recently-used
    /// workspaces first.
    pub async fn get_or_register(
        &self,
        root: &Path,
    ) -> Result<Arc<WorkspaceRecord>, WorkspaceError> {
        let (canonical_root, display_name) = canonicalize_workspace_root(root)?;

        if let Some(existing) = self.by_root.get(&canonical_root) {
            let existing_id = existing.clone();
            drop(existing);
            // Refresh last-opened in the store (best-effort, non-fatal).
            if let Err(e) = self.store.touch_last_opened(existing_id.as_str()).await {
                tracing::warn!(error = %e, workspace_id = %existing_id, "workspace last-opened touch failed");
            }
            if let Some(cached) = self.by_id.get(existing_id.as_str()) {
                return Ok(cached.clone());
            }
        }

        let now = Utc::now();
        let record = WorkspaceRecord {
            id: WorkspaceId::new_unchecked(uuid::Uuid::new_v4().to_string()),
            canonical_root: canonical_root.clone(),
            display_name,
            created_at: now,
            last_opened_at: now,
            archived_at: None,
        };

        self.store.upsert(&record).await?;
        let id_str = record.id.as_str().to_string();
        let arc = Arc::new(record);
        self.by_id.insert(id_str.clone(), arc.clone());
        self.by_root.insert(canonical_root, arc.id.clone());
        Ok(arc)
    }

    pub async fn resolve(&self, id: &WorkspaceId) -> Option<Arc<WorkspaceRecord>> {
        if let Some(cached) = self.by_id.get(id.as_str()) {
            return Some(cached.clone());
        }
        // Hydrate from the store on miss.
        if let Ok(Some(record)) = self.store.load_by_id(id.as_str()).await {
            let arc = Arc::new(record);
            self.by_id.insert(id.as_str().to_string(), arc.clone());
            return Some(arc);
        }
        None
    }

    pub async fn resolve_root(&self, root: &Path) -> Option<Arc<WorkspaceRecord>> {
        let Ok((canonical, _)) = canonicalize_workspace_root(root) else {
            return None;
        };
        if let Some(id) = self.by_root.get(&canonical) {
            return self.resolve(&id).await;
        }
        if let Ok(Some(record)) = self.store.load_by_canonical_root(&canonical).await {
            let id = record.id.clone();
            let arc = Arc::new(record);
            self.by_id.insert(id.as_str().to_string(), arc.clone());
            self.by_root.insert(canonical, id);
            return Some(arc);
        }
        None
    }

    pub async fn archive(&self, id: &WorkspaceId) -> Result<(), WorkspaceError> {
        self.store.archive(id.as_str()).await?;
        if let Some(mut entry) = self.by_id.get_mut(id.as_str()) {
            let mut rec = (**entry).clone();
            rec.archived_at = Some(Utc::now());
            *entry = Arc::new(rec);
        }
        Ok(())
    }

    pub async fn list(
        &self,
        include_archived: bool,
    ) -> Result<Vec<Arc<WorkspaceRecord>>, WorkspaceError> {
        let records = self.store.list(include_archived).await?;
        Ok(records.into_iter().map(Arc::new).collect())
    }

    /// Test-only helper to insert a synthetic record without going through
    /// the canonical `get_or_register` path. Used by workspace services
    /// tests that need a workspace record but don't need a real on-disk
    /// project root (or are working against an in-memory store).
    #[doc(hidden)]
    pub async fn upsert_test_record(&self, record: Arc<WorkspaceRecord>) {
        let id_str = record.id.as_str().to_string();
        let canon = record.canonical_root.clone();
        let owned: WorkspaceRecord = (*record).clone();
        let _ = self.store.upsert(&owned).await;
        self.by_id.insert(id_str, record);
        self.by_root.insert(canon, owned.id);
    }
}

/// Canonicalize a workspace root path for registration.
///
/// Rules:
/// 1. The path must exist.
/// 2. The path must be a directory (not a file or symlink to a file).
/// 3. The path is canonicalized against its filesystem to dedupe
///    symlink/relative aliases.
///
/// We deliberately do **not** follow symlinks during traversal here --
/// [`std::fs::canonicalize`] resolves every path component with
/// `realpath`, which is what we want for root identity. A symlink that
/// resolves into the workspace is still inside the workspace; a symlink
/// at the root that escapes would have already been resolved.
fn canonicalize_workspace_root(root: &Path) -> Result<(PathBuf, String), WorkspaceError> {
    let metadata = std::fs::metadata(root).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            WorkspaceError::NotADirectory(root.to_path_buf())
        } else {
            WorkspaceError::CanonicalizationFailed(root.to_path_buf(), e.to_string())
        }
    })?;
    if !metadata.is_dir() {
        return Err(WorkspaceError::NotADirectory(root.to_path_buf()));
    }
    let canonical = std::fs::canonicalize(root)
        .map_err(|e| WorkspaceError::CanonicalizationFailed(root.to_path_buf(), e.to_string()))?;
    let display_name = canonical
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| canonical.to_string_lossy().into_owned());
    Ok((canonical, display_name))
}

/// Immutable, daemon-resolved execution context that flows through every
/// execution path. Holders must treat the context as read-only: every
/// field is reached by shared reference, and path resolution is the
/// only correct way to materialize a working directory or path.
#[derive(Debug)]
pub struct ExecutionContext {
    pub workspace_id: WorkspaceId,
    pub workspace_root: PathBuf,
    pub session_id: Option<String>,
    /// Additional roots that may be read by tools/cwd requests. Always
    /// starts with `workspace_root`; may be widened by configuration.
    pub allowed_read_roots: Arc<[PathBuf]>,
    /// Additional roots that may be written. Always starts with
    /// `workspace_root`; may be widened by configuration.
    pub allowed_write_roots: Arc<[PathBuf]>,
    /// Cancellation signal for the entire execution attempt. Every
    /// subsystem that owns long-running work should select on this.
    pub cancellation: tokio_util::sync::CancellationToken,
}

impl ExecutionContext {
    /// Construct a fresh execution context for a session/turn.
    pub fn new(
        workspace: Arc<WorkspaceRecord>,
        session_id: Option<String>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Arc<Self> {
        let root = workspace.canonical_root.clone();
        Arc::new(Self {
            workspace_id: workspace.id.clone(),
            workspace_root: root.clone(),
            session_id,
            allowed_read_roots: Arc::from([root.clone()]),
            allowed_write_roots: Arc::from([root]),
            cancellation,
        })
    }

    /// Construct an execution context with caller-supplied allowed roots
    /// (for example, when widening access to test fixtures or
    /// pre-baked integration-test sandboxes).
    pub fn with_allowed_roots(
        workspace: Arc<WorkspaceRecord>,
        session_id: Option<String>,
        read_roots: Vec<PathBuf>,
        write_roots: Vec<PathBuf>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Arc<Self> {
        let root = workspace.canonical_root.clone();
        let mut read = vec![root.clone()];
        for r in read_roots {
            if !read.iter().any(|p| p == &r) {
                read.push(r);
            }
        }
        let mut write = vec![root.clone()];
        for w in write_roots {
            if !write.iter().any(|p| p == &w) {
                write.push(w);
            }
        }
        Arc::new(Self {
            workspace_id: workspace.id.clone(),
            workspace_root: root,
            session_id,
            allowed_read_roots: Arc::from(read.into_boxed_slice()),
            allowed_write_roots: Arc::from(write.into_boxed_slice()),
            cancellation,
        })
    }

    /// Pick the directory a command should run in. `None` means "use
    /// the workspace root"; a relative path is resolved under the
    /// workspace root; an absolute path must fall under an explicitly
    /// allowed root (workspace plus extras).
    pub fn resolve_relative_cwd(
        &self,
        requested: Option<&Path>,
    ) -> Result<PathBuf, PathPolicyError> {
        match requested {
            None => Ok(self.workspace_root.clone()),
            Some(p) if p.as_os_str().is_empty() => Ok(self.workspace_root.clone()),
            Some(p) if p.is_absolute() => self.pick_allowed_root(p, &self.allowed_read_roots),
            Some(p) => resolve_under_root(&self.workspace_root, p),
        }
    }

    /// Verify and canonicalize a *read* access target. Relative paths
    /// resolve under the workspace root. Absolute paths must fall under
    /// an explicitly allowed read root. Existing files are canonicalized
    /// directly; paths that don't yet exist canonicalize the nearest
    /// existing ancestor and append the missing suffix.
    pub fn resolve_read_path(&self, requested: &Path) -> Result<PathBuf, PathPolicyError> {
        self.resolve_path_with_allowed(requested, &self.allowed_read_roots)
    }

    /// Verify and canonicalize a *write* access target. Same rules as
    /// read, plus the destination must also lie under
    /// `allowed_write_roots`.
    pub fn resolve_write_path(&self, requested: &Path) -> Result<PathBuf, PathPolicyError> {
        self.resolve_path_with_allowed(requested, &self.allowed_write_roots)
    }

    fn resolve_path_with_allowed(
        &self,
        requested: &Path,
        allowed_roots: &[PathBuf],
    ) -> Result<PathBuf, PathPolicyError> {
        if requested.as_os_str().is_empty() {
            return Err(PathPolicyError::EmptyPath);
        }
        let candidate: PathBuf = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.workspace_root.join(requested)
        };
        // Canonicalize when possible. For paths that don't exist yet,
        // canonicalize the nearest existing ancestor and append the
        // missing suffix. This avoids rejecting newly-introduced files.
        let canonical = canonicalize_existing(&candidate)?;
        if path_within_any(&canonical, allowed_roots) {
            Ok(canonical)
        } else if requested.is_absolute() {
            Err(PathPolicyError::OutsideAllowedRoots(
                requested.to_path_buf(),
            ))
        } else {
            Err(PathPolicyError::OutsideWorkspace(
                canonical,
                self.workspace_root.clone(),
            ))
        }
    }

    fn pick_allowed_root(
        &self,
        requested: &Path,
        allowed_roots: &[PathBuf],
    ) -> Result<PathBuf, PathPolicyError> {
        let canonical = canonicalize_existing(requested)?;
        if !path_within_any(&canonical, allowed_roots) {
            return Err(PathPolicyError::OutsideAllowedRoots(
                requested.to_path_buf(),
            ));
        }
        Ok(canonical)
    }
}

/// Canonicalize a path that may not yet exist. Walks up until a
/// directory exists, canonicalizes that ancestor, then reattaches the
/// trailing segments. Returns an `OutsideWorkspace` error when the
/// nearest existing ancestor is *not* under the workspace root (which
/// catches attempted escapes that target a not-yet-existing `..`).
fn canonicalize_existing(path: &Path) -> Result<PathBuf, PathPolicyError> {
    if path.as_os_str().is_empty() {
        return Err(PathPolicyError::EmptyPath);
    }
    if let Ok(existing) = std::fs::canonicalize(path) {
        return Ok(existing);
    }
    let mut current: PathBuf = path.to_path_buf();
    let mut suffix_segments: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if current.exists() {
            let canon = std::fs::canonicalize(&current).map_err(|e| {
                PathPolicyError::CanonicalizationFailed(path.to_path_buf(), e.to_string())
            })?;
            let mut joined = canon;
            for seg in suffix_segments.iter().rev() {
                joined.push(seg);
            }
            return Ok(joined);
        }
        match current.file_name() {
            Some(name) => {
                suffix_segments.push(name.to_os_string());
                current.pop();
            }
            None => {
                return Err(PathPolicyError::CanonicalizationFailed(
                    path.to_path_buf(),
                    "no existing ancestor".to_string(),
                ));
            }
        }
    }
}

fn resolve_under_root(root: &Path, requested: &Path) -> Result<PathBuf, PathPolicyError> {
    let joined = root.join(requested);
    let canonical = canonicalize_existing(&joined)?;
    if !canonical.starts_with(root) {
        return Err(PathPolicyError::OutsideWorkspace(
            canonical,
            root.to_path_buf(),
        ));
    }
    Ok(canonical)
}

fn path_within_any(candidate: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| candidate.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("marker.txt"), label).unwrap();
        dir
    }

    #[tokio::test(flavor = "current_thread")]
    async fn registers_and_dedupes_canonical_root() {
        let dir = temp_root("a");
        let store = Arc::new(InMemoryWorkspaceStore::new());
        let registry = WorkspaceRegistry::load(store.clone()).await.unwrap();

        let first = registry.get_or_register(dir.path()).await.unwrap();
        let again = registry.get_or_register(dir.path()).await.unwrap();
        assert_eq!(first.id, again.id);
        // Symlink alias should also dedupe to the same canonical root.
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(dir.path(), &alias).unwrap();
        let alias_rec = registry.get_or_register(&alias).await.unwrap();
        assert_eq!(first.id, alias_rec.id);

        let all = registry.list(false).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_nonexistent_and_files() {
        let dir = temp_root("a");
        let store = Arc::new(InMemoryWorkspaceStore::new());
        let registry = WorkspaceRegistry::load(store).await.unwrap();

        let missing = dir.path().join("nope");
        assert!(matches!(
            registry.get_or_register(&missing).await,
            Err(WorkspaceError::NotADirectory(_))
        ));

        let file = dir.path().join("marker.txt");
        assert!(matches!(
            registry.get_or_register(&file).await,
            Err(WorkspaceError::NotADirectory(_))
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn archive_filter_and_resolve() {
        let dir = temp_root("a");
        let store = Arc::new(InMemoryWorkspaceStore::new());
        let registry = WorkspaceRegistry::load(store).await.unwrap();
        let rec = registry.get_or_register(dir.path()).await.unwrap();
        assert!(registry.list(false).await.unwrap().len() == 1);
        registry.archive(&rec.id).await.unwrap();
        assert!(registry.list(false).await.unwrap().is_empty());
        let archived = registry.list(true).await.unwrap();
        assert!(archived.iter().all(|r| r.is_archived()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execution_context_resolves_cwd() {
        let dir = temp_root("a");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let store = Arc::new(InMemoryWorkspaceStore::new());
        let registry = WorkspaceRegistry::load(store).await.unwrap();
        let rec = registry.get_or_register(dir.path()).await.unwrap();
        let ctx = ExecutionContext::new(rec.clone(), Some("s".into()), Default::default());

        // None => workspace root
        let cwd = ctx.resolve_relative_cwd(None).unwrap();
        assert_eq!(cwd, rec.canonical_root);

        // Relative subdir resolves under workspace root
        let sub_rel = Path::new("sub");
        let cwd_rel = ctx.resolve_relative_cwd(Some(sub_rel)).unwrap();
        assert!(cwd_rel.starts_with(&rec.canonical_root));
        assert!(cwd_rel.ends_with("sub"));

        // Relative escape via `..` rejected
        let escape = Path::new("../escape");
        match ctx.resolve_relative_cwd(Some(escape)) {
            Err(PathPolicyError::OutsideWorkspace(_, _)) => {}
            other => panic!("expected outside-workspace error, got {:?}", other),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_and_write_paths_check_allowed_roots() {
        let workspace_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(workspace_dir.path().join("src")).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let store = Arc::new(InMemoryWorkspaceStore::new());
        let registry = WorkspaceRegistry::load(store).await.unwrap();
        let rec = registry
            .get_or_register(workspace_dir.path())
            .await
            .unwrap();
        let ctx = ExecutionContext::new(rec.clone(), None, Default::default());

        // Inside workspace
        let inside = workspace_dir.path().join("src");
        assert!(ctx.resolve_read_path(&inside).is_ok());
        assert!(ctx.resolve_write_path(&inside).is_ok());

        // Relative escape via `..`
        let escape = Path::new("../escaped");
        assert!(matches!(
            ctx.resolve_read_path(escape),
            Err(PathPolicyError::OutsideWorkspace(_, _))
        ));

        // Absolute path outside any allowed root
        let outside = elsewhere.path();
        let canonical_outside = std::fs::canonicalize(outside).unwrap();
        assert!(matches!(
            ctx.resolve_read_path(&canonical_outside),
            Err(PathPolicyError::OutsideAllowedRoots(_))
        ));
    }

    #[test]
    fn path_resolution_rejects_empty_paths() {
        assert_eq!(
            canonicalize_existing(Path::new("")).unwrap_err(),
            PathPolicyError::EmptyPath,
        );
    }
}
