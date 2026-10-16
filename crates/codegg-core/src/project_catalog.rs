//! Daemon-owned project catalog service on top of `ProjectStorage`.
//!
//! This module delivers the durable catalog of logical projects and
//! repositories with archive/restore, lifecycle/health/locator placeholders,
//! explicit local registration from existing workspace/repository,
//! conservative legacy association, and restart hydration.
//!
//! It does NOT perform filesystem scanning, remote execution, or
//! expensive service activation. The catalog is read/list/manage from
//! internal/diagnostic surfaces only.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use thiserror::Error;

use crate::identity::{NodeId, ProjectId};
use crate::project_storage::{bounded_text, ProjectLifecycle, MAX_DISPLAY_NAME_LENGTH};
use crate::workspace::{WorkspaceId, WorkspaceRecord};

// ---------------------------------------------------------------------------
// Length bounds
// ---------------------------------------------------------------------------

/// Maximum byte length for user-supplied text fields that map to TEXT columns
/// (description, notes, source, etc.).
pub const MAX_CATALOG_TEXT_LENGTH: usize = 1_024;

/// Maximum byte length for a locator field (SSH host, path, label, etc.).
pub const MAX_LOCATOR_FIELD_LENGTH: usize = 512;

/// Maximum byte length for the tags JSON array string.
pub const MAX_TAGS_JSON_LENGTH: usize = 1_024;

/// Maximum byte length for the registration source field.
pub const MAX_REGISTRATION_SOURCE_LENGTH: usize = 256;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn catalog_db_error(error: sqlx::Error) -> CatalogError {
    CatalogError::Database(error.to_string())
}

fn catalog_identity_error(error: crate::identity::IdentityParseError) -> CatalogError {
    CatalogError::InvalidValue(error.to_string())
}

fn catalog_timestamp(value: i64, field: &str) -> Result<DateTime<Utc>, CatalogError> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .ok_or_else(|| CatalogError::InvalidValue(format!("invalid timestamp in {field}")))
}

// ---------------------------------------------------------------------------
// Locator
// ---------------------------------------------------------------------------

/// Typed locator for a project. Locators are inert data — they never trigger
/// filesystem probing or remote execution.
///
/// - `Local`: a workspace-scoped local path reference.
/// - `Ssh`: an SSH placeholder (inert, no local path accessor).
/// - `LinkedNode`: a linked-node placeholder (inert, no local path accessor).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Locator {
    Local {
        workspace_id: WorkspaceId,
        canonical_root: PathBuf,
    },
    Ssh {
        host: String,
        port: Option<u16>,
        user: Option<String>,
        path: String,
        label: Option<String>,
    },
    LinkedNode {
        node_id: NodeId,
        alias: Option<String>,
        path_hint: Option<String>,
    },
}

/// Stored representation of a locator in the `project_locator` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogLocatorRecord {
    pub id: String,
    pub project_id: ProjectId,
    pub locator: Locator,
    pub display_summary: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Locator {
    /// True only for the `Local` variant.
    pub fn is_local(&self) -> bool {
        matches!(self, Locator::Local { .. })
    }

    /// Human-readable summary suitable for protocol DTOs. Never exposes
    /// a full filesystem path for SSH/LinkedNode locators.
    pub fn summary(&self) -> String {
        match self {
            Locator::Local { canonical_root, .. } => {
                format!("local:{}", canonical_root.display())
            }
            Locator::Ssh {
                host,
                port,
                user,
                path: _,
                label,
            } => {
                let user_prefix = user.as_ref().map(|u| format!("{}@", u)).unwrap_or_default();
                let port_suffix = port.map(|p| format!(":{}", p)).unwrap_or_default();
                let label_suffix = label
                    .as_ref()
                    .map(|l| format!(" ({})", l))
                    .unwrap_or_default();
                // NOTE: the `path` field is intentionally NOT included in
                // the summary. Remote paths are not local filesystem
                // references and must never be exposed as such.
                format!("ssh:{}{}{}{}", user_prefix, host, port_suffix, label_suffix)
            }
            Locator::LinkedNode {
                node_id,
                alias,
                path_hint,
            } => {
                let alias_suffix = alias
                    .as_ref()
                    .map(|a| format!(" ({})", a))
                    .unwrap_or_default();
                let hint_suffix = path_hint
                    .as_ref()
                    .map(|h| format!(" [{}]", h))
                    .unwrap_or_default();
                format!("node:{}{}{}", node_id, alias_suffix, hint_suffix)
            }
        }
    }

    /// Validate that all fields are present, bounded, and safe.
    pub fn validate(&self) -> Result<(), CatalogError> {
        match self {
            Locator::Local {
                workspace_id: _,
                canonical_root,
            } => {
                if canonical_root.as_os_str().is_empty() {
                    return Err(CatalogError::InvalidValue(
                        "local locator canonical_root is empty".to_string(),
                    ));
                }
            }
            Locator::Ssh {
                host,
                port,
                user,
                path,
                label,
            } => {
                validate_locator_text(host, "ssh host")?;
                validate_locator_text(path, "ssh path")?;
                if let Some(user) = user {
                    validate_locator_text(user, "ssh user")?;
                }
                if let Some(label) = label {
                    validate_locator_text(label, "ssh label")?;
                }
                if let Some(port) = port {
                    if *port == 0 {
                        return Err(CatalogError::InvalidValue(
                            "ssh port must be non-zero".to_string(),
                        ));
                    }
                }
                // Reject embedded secrets: user@host patterns in path
                if path.contains("://") && path.contains('@') {
                    return Err(CatalogError::InvalidValue(
                        "ssh path must not contain a URL with embedded credentials".to_string(),
                    ));
                }
            }
            Locator::LinkedNode {
                node_id: _,
                alias,
                path_hint,
            } => {
                if let Some(alias) = alias {
                    validate_locator_text(alias, "linked node alias")?;
                }
                if let Some(hint) = path_hint {
                    validate_locator_text(hint, "linked node path_hint")?;
                }
            }
        }
        Ok(())
    }

    /// Display summary for this locator.
    pub fn display_summary(&self) -> String {
        self.summary()
    }
}

fn validate_locator_text(value: &str, field: &str) -> Result<(), CatalogError> {
    if value.is_empty() {
        return Err(CatalogError::InvalidValue(format!("{field} is empty")));
    }
    if value.len() > MAX_LOCATOR_FIELD_LENGTH {
        return Err(CatalogError::InvalidValue(format!(
            "{field} exceeds maximum length of {}",
            MAX_LOCATOR_FIELD_LENGTH
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(CatalogError::InvalidValue(format!(
            "{field} contains control characters"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Health status for a project. Each variant is inert data — no probing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Unknown,
    Available,
    Unavailable,
    Unsupported,
    Stale,
    Error,
}

impl HealthStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::Stale => "stale",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Result<Self, CatalogError> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "available" => Ok(Self::Available),
            "unavailable" => Ok(Self::Unavailable),
            "unsupported" => Ok(Self::Unsupported),
            "stale" => Ok(Self::Stale),
            "error" => Ok(Self::Error),
            other => Err(CatalogError::InvalidValue(format!(
                "unknown health status {other:?}"
            ))),
        }
    }
}

/// Health record for a project. Status never reflects filesystem probing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectHealthRecord {
    pub project_id: ProjectId,
    pub status: HealthStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub source: String,
    pub evaluated_at: DateTime<Utc>,
    pub notes: Option<String>,
}

// ---------------------------------------------------------------------------
// Catalog record
// ---------------------------------------------------------------------------

/// Extended project record with catalog-specific fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCatalogRecord {
    pub project_id: ProjectId,
    pub display_name: String,
    pub lifecycle: ProjectLifecycle,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub time_last_opened_at: Option<DateTime<Utc>>,
    pub registration_source: String,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProjectCatalogRecord {
    /// True if this project is archived.
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

// ---------------------------------------------------------------------------
// Workspace summary
// ---------------------------------------------------------------------------

/// Compact workspace summary for catalog listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub workspace_id: WorkspaceId,
    pub display_name: String,
    pub canonical_root: PathBuf,
}

// ---------------------------------------------------------------------------
// Lifecycle counts
// ---------------------------------------------------------------------------

/// Aggregate counts by lifecycle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleCounts {
    pub active: usize,
    pub archived: usize,
    pub total: usize,
}

// ---------------------------------------------------------------------------
// Hydration report
// ---------------------------------------------------------------------------

/// Report from daemon restart hydration. Lists non-archived project IDs and
/// locator counts. Performs no probing, no fs access, no git scan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HydrationReport {
    pub active_project_count: usize,
    pub total_project_count: usize,
    pub locator_count: usize,
    pub health_count: usize,
}

// ---------------------------------------------------------------------------
// Legacy association
// ---------------------------------------------------------------------------

/// Report from conservative legacy association.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyAssociationReport {
    pub projects_associated: usize,
    pub diagnostics_recorded: usize,
    pub already_migrated: bool,
}

/// Marker record in the `legacy_catalog_association_marker` table.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LegacyMarker {
    source: String,
    completed_at: i64,
    projects_associated: i64,
    diagnostics_recorded: i64,
}

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Input for registering a new local project.
#[derive(Debug, Clone)]
pub struct RegisterLocalProject {
    pub display_name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub primary_repository_id: Option<crate::identity::RepositoryId>,
}

// ---------------------------------------------------------------------------
// DTO support
// ---------------------------------------------------------------------------

/// Project summary for closure-record evidence (NOT used in production).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub name: String,
    pub lifecycle: ProjectLifecycle,
    pub archived_at: Option<DateTime<Utc>>,
    pub active_count: usize,
    pub archived_count: usize,
}

// ---------------------------------------------------------------------------
// Catalog error
// ---------------------------------------------------------------------------

/// Error type for project catalog operations.
#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("catalog database error: {0}")]
    Database(String),
    #[error("catalog record not found: {0}")]
    NotFound(String),
    #[error("invalid catalog value: {0}")]
    InvalidValue(String),
    #[error("catalog relation conflict: {0}")]
    Conflict(String),
    #[error("catalog already exists: {0}")]
    AlreadyExists(String),
}

// Safety: CatalogError is Send + Sync
const _: () = {
    fn _assert_send_sync<T: Send + Sync>() {}
    fn _assert_catalog_error() {
        _assert_send_sync::<CatalogError>();
    }
};

// ---------------------------------------------------------------------------
// ProjectCatalog service
// ---------------------------------------------------------------------------

/// Daemon-owned project catalog service wrapping a SQLite pool.
pub struct ProjectCatalog {
    pool: SqlitePool,
}

struct RepositoryLineage {
    lineage_key: String,
    remote_identity: Option<String>,
}

impl ProjectCatalog {
    /// Create a new catalog service from an existing pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Access the underlying pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// List projects, optionally including archived ones.
    pub async fn list_projects(
        &self,
        include_archived: bool,
    ) -> Result<Vec<ProjectCatalogRecord>, CatalogError> {
        let rows = if include_archived {
            sqlx::query(
                r#"SELECT id, display_name, lifecycle, description, tags,
                   time_last_opened, registration_source, archived_at,
                   time_created, time_updated
                   FROM logical_project ORDER BY time_updated DESC, id"#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(catalog_db_error)?
        } else {
            sqlx::query(
                r#"SELECT id, display_name, lifecycle, description, tags,
                   time_last_opened, registration_source, archived_at,
                   time_created, time_updated
                   FROM logical_project
                   WHERE archived_at IS NULL
                   ORDER BY time_updated DESC, id"#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(catalog_db_error)?
        };
        rows.into_iter().map(catalog_record_from_row).collect()
    }

    /// Get a single project by ID.
    pub async fn get_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<ProjectCatalogRecord, CatalogError> {
        let row = sqlx::query(
            r#"SELECT id, display_name, lifecycle, description, tags,
               time_last_opened, registration_source, archived_at,
               time_created, time_updated
               FROM logical_project WHERE id = ?"#,
        )
        .bind(project_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(catalog_db_error)?
        .ok_or_else(|| CatalogError::NotFound(format!("project {}", project_id)))?;
        catalog_record_from_row(row)
    }

    /// Get a project together with its health record.
    pub async fn get_project_with_health(
        &self,
        project_id: &ProjectId,
    ) -> Result<(ProjectCatalogRecord, Option<ProjectHealthRecord>), CatalogError> {
        let project = self.get_project(project_id).await?;
        let health = self.get_health(project_id).await?;
        Ok((project, health))
    }

    /// Register a new local project with an existing workspace.
    ///
    /// The workspace must already be registered in the `workspace` table.
    /// If the canonical_root resolves to an existing project via the
    /// workspace_project_binding, returns the existing project.
    pub async fn register_local_project(
        &self,
        input: RegisterLocalProject,
        workspace_id: &WorkspaceId,
        source: &str,
    ) -> Result<ProjectCatalogRecord, CatalogError> {
        let display_name =
            bounded_text(&input.display_name, MAX_DISPLAY_NAME_LENGTH, "display name")
                .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;

        let description = input
            .description
            .as_ref()
            .map(|d| bounded_text(d, MAX_CATALOG_TEXT_LENGTH, "description"))
            .transpose()
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;

        let tags_json = if input.tags.is_empty() {
            None
        } else {
            let json = serde_json::to_string(&input.tags)
                .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;
            let validated = bounded_text(&json, MAX_TAGS_JSON_LENGTH, "tags")
                .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;
            Some(validated)
        };

        let source = bounded_text(
            source,
            MAX_REGISTRATION_SOURCE_LENGTH,
            "registration source",
        )
        .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;

        // Validate workspace exists
        let workspace_row = sqlx::query(
            "SELECT id, canonical_root, display_name, time_created, time_last_opened, time_archived FROM workspace WHERE id = ?",
        )
        .bind(workspace_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(catalog_db_error)?
        .ok_or_else(|| {
            CatalogError::InvalidValue(
                "local registration requires an existing registered workspace".to_string(),
            )
        })?;

        let canonical_root: String = workspace_row.get("canonical_root");

        // Probe lineage before deciding anything, but DO NOT create the
        // `repository` row yet.
        //
        // Registration used to record `status = 'resolved'` with a NULL
        // repository and never probe Git at all, so the `repository` table
        // stayed empty for every TUI-launched project. Because the
        // existing-binding branch returned early, and
        // `ProjectStorage::reconcile_workspace_once` treats a `resolved`
        // binding as authoritative and never re-probes, that empty identity
        // was permanent: a delegated run needing managed isolation then failed
        // with `isolation_unavailable` in a repository with perfectly good
        // lineage evidence.
        let lineage = Self::probe_repository_lineage(std::path::Path::new(&canonical_root));

        // Check if workspace is already bound to a project
        let existing_binding = sqlx::query(
            "SELECT project_id, repository_id FROM workspace_project_binding WHERE workspace_id = ?",
        )
        .bind(workspace_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(catalog_db_error)?;

        if let Some(row) = existing_binding {
            let existing_project_id: String = row.get("project_id");
            let project_id =
                ProjectId::parse(&existing_project_id).map_err(catalog_identity_error)?;
            // A binding that already carries a repository is authoritative:
            // return it unchanged and create nothing at all.
            let bound_repository: Option<String> = row.get("repository_id");
            if bound_repository.is_none() {
                let healed = match input.primary_repository_id.clone() {
                    Some(explicit) => Some(explicit),
                    None => match &lineage {
                        Some(lineage) => Some(self.ensure_repository_row(lineage, &source).await?),
                        None => None,
                    },
                };
                if let Some(repository_id) = healed {
                    self.heal_repository_binding(
                        workspace_id,
                        &project_id,
                        &repository_id,
                        &source,
                    )
                    .await?;
                }
            }
            return self.get_project(&project_id).await;
        }

        // No binding yet: resolve the repository identity for real. An explicit
        // operator-supplied repository wins over probed lineage.
        let repository_id = match input.primary_repository_id.clone() {
            Some(explicit) => Some(explicit),
            None => match &lineage {
                Some(lineage) => Some(self.ensure_repository_row(lineage, &source).await?),
                None => None,
            },
        };

        // Check if the primary repository already links to an existing project.
        // Identity follows lineage, not path: a rename, a move, or a second
        // worktree of the same remote lands on the same logical project.
        if let Some(repo_id) = &repository_id {
            let existing = sqlx::query(
                "SELECT project_id FROM project_repository WHERE repository_id = ? AND relation_kind = 'primary' ORDER BY project_id LIMIT 2",
            )
            .bind(repo_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(catalog_db_error)?;

            if existing.len() == 1 {
                let project_id =
                    ProjectId::parse(existing[0].get::<String, _>("project_id").as_str())
                        .map_err(catalog_identity_error)?;
                // Bind workspace to existing project
                self.bind_workspace_to_project(workspace_id, &project_id, Some(repo_id), &source)
                    .await?;
                return self.get_project(&project_id).await;
            }
        }

        // Create new project
        let mut tx = self.pool.begin().await.map_err(catalog_db_error)?;
        let now = Utc::now();
        let project_id = ProjectId::new();

        sqlx::query(
            r#"INSERT INTO logical_project
               (id, display_name, lifecycle, description, tags,
                time_last_opened, registration_source, archived_at,
                time_created, time_updated)
               VALUES (?, ?, 'active', ?, ?, ?, ?, NULL, ?, ?)"#,
        )
        .bind(project_id.as_str())
        .bind(&display_name)
        .bind(description.clone())
        .bind(tags_json.clone())
        .bind(now.timestamp_millis())
        .bind(&source)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&mut *tx)
        .await
        .map_err(catalog_db_error)?;

        // Create project_repository if repository is provided
        if let Some(repo_id) = &repository_id {
            sqlx::query(
                "INSERT INTO project_repository (project_id, repository_id, relation_kind, time_created, revision) VALUES (?, ?, 'primary', ?, 1) ON CONFLICT(project_id, repository_id) DO NOTHING",
            )
            .bind(project_id.as_str())
            .bind(repo_id.as_str())
            .bind(now.timestamp_millis())
            .execute(&mut *tx)
            .await
            .map_err(catalog_db_error)?;
        }

        // Bind workspace
        let locator_root = canonical_root;
        sqlx::query(
            r#"INSERT INTO workspace_project_binding
               (workspace_id, project_id, repository_id, worktree_id, node_id,
                locator, status, source, revision, time_created, time_updated)
               VALUES (?, ?, ?, NULL, NULL, ?, 'resolved', ?, 1, ?, ?)
               ON CONFLICT(workspace_id) DO UPDATE SET
                project_id = excluded.project_id,
                repository_id = excluded.repository_id,
                locator = excluded.locator,
                status = excluded.status,
                source = excluded.source,
                revision = workspace_project_binding.revision + 1,
                time_updated = excluded.time_updated"#,
        )
        .bind(workspace_id.as_str())
        .bind(project_id.as_str())
        .bind(repository_id.as_ref().map(|r| r.as_str()))
        .bind(&locator_root)
        .bind(&source)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&mut *tx)
        .await
        .map_err(catalog_db_error)?;

        tx.commit().await.map_err(catalog_db_error)?;
        self.get_project(&project_id).await
    }

    /// Usable repository lineage evidence for one workspace root.
    ///
    /// The local Git probe runs BEFORE any write transaction is opened — the same
    /// discipline `ProjectStorage::reconcile_workspace_path` uses — so a slow or
    /// failing probe never holds the SQLite write lock. Only unique, usable remote
    /// evidence produces a key: a root that is not a repository, carries no remote,
    /// or presents ambiguous/unsafe remote evidence yields `None` and keeps a
    /// project-only binding. Lineage is never inferred from the path, and a probe
    /// that cannot run at all is not treated as evidence about the repository.
    fn probe_repository_lineage(root: &std::path::Path) -> Option<RepositoryLineage> {
        let evidence = crate::repository_lineage::inspect_repository_lineage(root).ok()?;
        let lineage_key = evidence.equality_key()?;
        let remote_identity = match &evidence {
            crate::repository_lineage::RepositoryLineageEvidence::Unique { remote } => {
                Some(remote.equality_key())
            }
            _ => None,
        };
        Some(RepositoryLineage {
            lineage_key,
            remote_identity,
        })
    }

    /// Fetch the durable repository id for a lineage, creating the
    /// `repository` row the first time that lineage is seen.
    ///
    /// Called only once the caller knows it will actually bind the identity.
    /// Probing eagerly would leave an orphan `repository` row behind whenever
    /// an existing binding already carried an identity and was returned
    /// unchanged.
    async fn ensure_repository_row(
        &self,
        lineage: &RepositoryLineage,
        source: &str,
    ) -> Result<crate::identity::RepositoryId, CatalogError> {
        let existing =
            sqlx::query("SELECT id FROM repository WHERE vcs_kind = 'git' AND lineage_key = ?")
                .bind(&lineage.lineage_key)
                .fetch_optional(&self.pool)
                .await
                .map_err(catalog_db_error)?;
        if let Some(row) = existing {
            return crate::identity::RepositoryId::parse(row.get::<String, _>("id").as_str())
                .map_err(catalog_identity_error);
        }

        let repository_id = crate::identity::RepositoryId::new();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO repository (id, vcs_kind, lineage_key, remote_identity, common_directory, default_branch, head, provenance, status, time_created, time_updated) VALUES (?, 'git', ?, ?, NULL, NULL, NULL, ?, 'resolved', ?, ?)",
        )
        .bind(repository_id.as_str())
        .bind(&lineage.lineage_key)
        .bind(lineage.remote_identity.as_deref())
        .bind(source)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&self.pool)
        .await
        .map_err(catalog_db_error)?;
        Ok(repository_id)
    }

    /// Attach repository identity to an existing binding that was recorded
    /// without one.
    ///
    /// Only strictly more informative evidence is applied. A binding that
    /// already carries a repository is never touched, and a repository already
    /// primary-bound to a *different* project is recorded as an
    /// `ambiguous` diagnostic instead of merging two logical projects — the
    /// identity layer never merges records on a guess. The UPDATE is guarded
    /// on `repository_id IS NULL` so a concurrent writer that already bound an
    /// identity wins rather than being overwritten.
    async fn heal_repository_binding(
        &self,
        workspace_id: &WorkspaceId,
        project_id: &ProjectId,
        repository_id: &crate::identity::RepositoryId,
        source: &str,
    ) -> Result<(), CatalogError> {
        let conflicting = sqlx::query(
            "SELECT project_id FROM project_repository WHERE repository_id = ? AND relation_kind = 'primary' AND project_id <> ? LIMIT 1",
        )
        .bind(repository_id.as_str())
        .bind(project_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(catalog_db_error)?;

        if let Some(row) = conflicting {
            let other_project: String = row.get("project_id");
            self.record_identity_diagnostic(
                Some(workspace_id.as_str()),
                Some(project_id.as_str()),
                "repository_project_conflict",
                crate::project_storage::BindingStatus::Ambiguous,
                &format!(
                    "repository {repository_id} is already primary-bound to project {other_project}; workspace {workspace_id} keeps its unbound repository rather than merging projects"
                ),
                source,
            )
            .await?;
            return Ok(());
        }

        let now = Utc::now();
        let mut tx = self.pool.begin().await.map_err(catalog_db_error)?;
        sqlx::query(
            "INSERT INTO project_repository (project_id, repository_id, relation_kind, time_created, revision) VALUES (?, ?, 'primary', ?, 1) ON CONFLICT(project_id, repository_id) DO NOTHING",
        )
        .bind(project_id.as_str())
        .bind(repository_id.as_str())
        .bind(now.timestamp_millis())
        .execute(&mut *tx)
        .await
        .map_err(catalog_db_error)?;

        sqlx::query(
            "UPDATE workspace_project_binding SET repository_id = ?, source = ?, revision = revision + 1, time_updated = ? WHERE workspace_id = ? AND repository_id IS NULL",
        )
        .bind(repository_id.as_str())
        .bind(source)
        .bind(now.timestamp_millis())
        .bind(workspace_id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(catalog_db_error)?;

        tx.commit().await.map_err(catalog_db_error)?;
        Ok(())
    }

    async fn record_identity_diagnostic(
        &self,
        workspace_id: Option<&str>,
        project_id: Option<&str>,
        code: &str,
        status: crate::project_storage::BindingStatus,
        message: &str,
        source: &str,
    ) -> Result<(), CatalogError> {
        let id = uuid::Uuid::new_v4().to_string();
        let message = bounded_text(message, MAX_CATALOG_TEXT_LENGTH, "diagnostic message")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;
        let code = bounded_text(code, MAX_REGISTRATION_SOURCE_LENGTH, "diagnostic code")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;
        let source = bounded_text(source, MAX_REGISTRATION_SOURCE_LENGTH, "diagnostic source")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO identity_diagnostic (id, workspace_id, session_id, project_id, code, status, message, source, time_created, time_updated) VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(workspace_id)
        .bind(project_id)
        .bind(&code)
        .bind(status.as_str())
        .bind(&message)
        .bind(&source)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&self.pool)
        .await
        .map_err(catalog_db_error)?;
        Ok(())
    }

    /// Archive a project (logical, non-destructive).
    pub async fn archive_project(
        &self,
        project_id: &ProjectId,
        source: &str,
    ) -> Result<ProjectCatalogRecord, CatalogError> {
        let source = bounded_text(source, MAX_REGISTRATION_SOURCE_LENGTH, "archive source")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;

        for attempt in 0..3 {
            match self.archive_project_once(project_id, &source).await {
                Err(CatalogError::Database(msg))
                    if attempt < 2 && msg.contains("database is locked") =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                result => return result,
            }
        }
        unreachable!("archive retry loop always returns")
    }

    async fn archive_project_once(
        &self,
        project_id: &ProjectId,
        source: &str,
    ) -> Result<ProjectCatalogRecord, CatalogError> {
        let now = Utc::now();
        let result = sqlx::query(
            r#"UPDATE logical_project
               SET lifecycle = 'archived', archived_at = ?, time_updated = ?, registration_source = ?
               WHERE id = ? AND archived_at IS NULL"#,
        )
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .bind(source)
        .bind(project_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(catalog_db_error)?;

        if result.rows_affected() == 0 {
            // Check if project exists at all
            let exists =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM logical_project WHERE id = ?")
                    .bind(project_id.as_str())
                    .fetch_one(&self.pool)
                    .await
                    .map_err(catalog_db_error)?;
            if exists == 0 {
                return Err(CatalogError::NotFound(format!("project {}", project_id)));
            }
            // Already archived — return current state
        }
        self.get_project(project_id).await
    }

    /// Restore an archived project.
    pub async fn restore_project(
        &self,
        project_id: &ProjectId,
        source: &str,
    ) -> Result<ProjectCatalogRecord, CatalogError> {
        let source = bounded_text(source, MAX_REGISTRATION_SOURCE_LENGTH, "restore source")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;

        let now = Utc::now();
        let result = sqlx::query(
            r#"UPDATE logical_project
               SET lifecycle = 'active', archived_at = NULL, time_updated = ?, registration_source = ?
               WHERE id = ? AND archived_at IS NOT NULL"#,
        )
        .bind(now.timestamp_millis())
        .bind(source)
        .bind(project_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(catalog_db_error)?;

        if result.rows_affected() == 0 {
            let exists =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM logical_project WHERE id = ?")
                    .bind(project_id.as_str())
                    .fetch_one(&self.pool)
                    .await
                    .map_err(catalog_db_error)?;
            if exists == 0 {
                return Err(CatalogError::NotFound(format!("project {}", project_id)));
            }
            // Already active — return current state
        }
        self.get_project(project_id).await
    }

    /// List workspaces bound to a project.
    pub async fn list_workspaces_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<WorkspaceSummary>, CatalogError> {
        let rows = sqlx::query(
            r#"SELECT w.id, w.display_name, w.canonical_root
               FROM workspace w
               INNER JOIN workspace_project_binding wpb ON w.id = wpb.workspace_id
               WHERE wpb.project_id = ?
               ORDER BY w.display_name, w.id"#,
        )
        .bind(project_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(catalog_db_error)?;

        rows.into_iter()
            .map(|row| {
                Ok(WorkspaceSummary {
                    workspace_id: WorkspaceId::parse(row.get::<String, _>("id").as_str())
                        .map_err(catalog_identity_error)?,
                    display_name: row.get("display_name"),
                    canonical_root: PathBuf::from(row.get::<String, _>("canonical_root")),
                })
            })
            .collect()
    }

    /// Count sessions bound to a project.
    pub async fn list_sessions_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<usize, CatalogError> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM session_project_binding WHERE project_id = ?")
                .bind(project_id.as_str())
                .fetch_one(&self.pool)
                .await
                .map_err(catalog_db_error)?;
        Ok(count as usize)
    }

    /// List locators for a project.
    pub async fn list_locators(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<CatalogLocatorRecord>, CatalogError> {
        let rows = sqlx::query(
            r#"SELECT id, project_id, locator_kind, workspace_id, canonical_root,
               ssh_host, ssh_port, ssh_user, ssh_path, ssh_label,
               linked_node_id, linked_node_alias, linked_node_path_hint,
               display_summary, source, time_created, time_updated
               FROM project_locator
               WHERE project_id = ?
               ORDER BY time_created, id"#,
        )
        .bind(project_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(catalog_db_error)?;

        rows.into_iter().map(locator_from_row).collect()
    }

    /// Attach a locator to a project. For `Local`, the workspace must already
    /// be bound to the project. For `Ssh` and `LinkedNode`, only placeholder
    /// data is stored.
    pub async fn attach_locator(
        &self,
        project_id: &ProjectId,
        locator: Locator,
        source: &str,
    ) -> Result<CatalogLocatorRecord, CatalogError> {
        locator.validate()?;

        let source = bounded_text(source, MAX_REGISTRATION_SOURCE_LENGTH, "locator source")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;

        let display_summary = locator.display_summary();
        let display_summary = bounded_text(
            &display_summary,
            MAX_CATALOG_TEXT_LENGTH,
            "locator display summary",
        )
        .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;

        // Validate project exists
        if self.get_project(project_id).await.is_err() {
            return Err(CatalogError::NotFound(format!("project {}", project_id)));
        }

        // For Local variant, validate workspace binding
        if let Locator::Local { workspace_id, .. } = &locator {
            let binding = sqlx::query(
                "SELECT project_id FROM workspace_project_binding WHERE workspace_id = ? AND project_id = ?",
            )
            .bind(workspace_id.as_str())
            .bind(project_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(catalog_db_error)?;
            if binding.is_none() {
                return Err(CatalogError::InvalidValue(
                    "local locator requires workspace to be bound to this project".to_string(),
                ));
            }
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        // Extract values before match to avoid temporary lifetime issues
        let local_root_str;
        let (
            kind,
            ws_id,
            canonical_root,
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_path,
            ssh_label,
            node_id,
            node_alias,
            node_hint,
        ) = match &locator {
            Locator::Local {
                workspace_id,
                canonical_root,
            } => {
                local_root_str = canonical_root.to_string_lossy().to_string();
                (
                    "local",
                    Some(workspace_id.as_str()),
                    Some(local_root_str.as_str()),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            }
            Locator::Ssh {
                host,
                port,
                user,
                path,
                label,
            } => (
                "ssh",
                None,
                None,
                Some(host.as_str()),
                port.map(|p| p as i64),
                user.as_deref(),
                Some(path.as_str()),
                label.as_deref(),
                None,
                None,
                None,
            ),
            Locator::LinkedNode {
                node_id,
                alias,
                path_hint,
            } => (
                "linked_node",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(node_id.as_str()),
                alias.as_deref(),
                path_hint.as_deref(),
            ),
        };

        sqlx::query(
            r#"INSERT INTO project_locator
               (id, project_id, locator_kind, workspace_id, canonical_root,
                ssh_host, ssh_port, ssh_user, ssh_path, ssh_label,
                linked_node_id, linked_node_alias, linked_node_path_hint,
                display_summary, source, time_created, time_updated)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&id)
        .bind(project_id.as_str())
        .bind(kind)
        .bind(ws_id)
        .bind(canonical_root)
        .bind(ssh_host)
        .bind(ssh_port)
        .bind(ssh_user)
        .bind(ssh_path)
        .bind(ssh_label)
        .bind(node_id)
        .bind(node_alias)
        .bind(node_hint)
        .bind(&display_summary)
        .bind(&source)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&self.pool)
        .await
        .map_err(catalog_db_error)?;

        Ok(CatalogLocatorRecord {
            id,
            project_id: project_id.clone(),
            locator,
            display_summary,
            source: source.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Detach a locator by ID.
    pub async fn detach_locator(&self, locator_id: &str) -> Result<(), CatalogError> {
        let result = sqlx::query("DELETE FROM project_locator WHERE id = ?")
            .bind(locator_id)
            .execute(&self.pool)
            .await
            .map_err(catalog_db_error)?;
        if result.rows_affected() == 0 {
            return Err(CatalogError::NotFound(format!("locator {}", locator_id)));
        }
        Ok(())
    }

    /// Set health status for a project (placeholder model, no probing).
    pub async fn set_health(
        &self,
        project_id: &ProjectId,
        status: HealthStatus,
        source: &str,
    ) -> Result<ProjectHealthRecord, CatalogError> {
        let source = bounded_text(source, MAX_REGISTRATION_SOURCE_LENGTH, "health source")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;

        // Validate project exists
        if self.get_project(project_id).await.is_err() {
            return Err(CatalogError::NotFound(format!("project {}", project_id)));
        }

        let now = Utc::now();
        sqlx::query(
            r#"INSERT INTO project_health
               (project_id, status, error_code, error_message, source, time_evaluated, notes)
               VALUES (?, ?, NULL, NULL, ?, ?, NULL)
               ON CONFLICT(project_id) DO UPDATE SET
                status = excluded.status,
                error_code = NULL,
                error_message = NULL,
                source = excluded.source,
                time_evaluated = excluded.time_evaluated,
                notes = NULL"#,
        )
        .bind(project_id.as_str())
        .bind(status.as_str())
        .bind(&source)
        .bind(now.timestamp_millis())
        .execute(&self.pool)
        .await
        .map_err(catalog_db_error)?;

        Ok(ProjectHealthRecord {
            project_id: project_id.clone(),
            status,
            error_code: None,
            error_message: None,
            source: source.to_string(),
            evaluated_at: now,
            notes: None,
        })
    }

    /// Get the health record for a project.
    pub async fn get_health(
        &self,
        project_id: &ProjectId,
    ) -> Result<Option<ProjectHealthRecord>, CatalogError> {
        let row = sqlx::query(
            r#"SELECT project_id, status, error_code, error_message, source, time_evaluated, notes
               FROM project_health WHERE project_id = ?"#,
        )
        .bind(project_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(catalog_db_error)?;

        row.map(health_from_row).transpose()
    }

    /// Mark a project as recently opened.
    pub async fn mark_opened(&self, project_id: &ProjectId) -> Result<(), CatalogError> {
        let now = Utc::now();
        let result = sqlx::query(
            "UPDATE logical_project SET time_last_opened = ?, time_updated = ? WHERE id = ?",
        )
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .bind(project_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(catalog_db_error)?;
        if result.rows_affected() == 0 {
            return Err(CatalogError::NotFound(format!("project {}", project_id)));
        }
        Ok(())
    }

    /// Count projects by lifecycle.
    pub async fn count_by_lifecycle(&self) -> Result<LifecycleCounts, CatalogError> {
        let active: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM logical_project WHERE archived_at IS NULL")
                .fetch_one(&self.pool)
                .await
                .map_err(catalog_db_error)?;
        let archived: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM logical_project WHERE archived_at IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(catalog_db_error)?;
        Ok(LifecycleCounts {
            active: active as usize,
            archived: archived as usize,
            total: (active + archived) as usize,
        })
    }

    /// Restart hydration: list non-archived project IDs and locator counts.
    /// Performs no probing, no fs access, no git scan.
    pub async fn restart_hydration(&self) -> Result<HydrationReport, CatalogError> {
        let active: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM logical_project WHERE archived_at IS NULL")
                .fetch_one(&self.pool)
                .await
                .map_err(catalog_db_error)?;
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logical_project")
            .fetch_one(&self.pool)
            .await
            .map_err(catalog_db_error)?;
        let locators: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_locator")
            .fetch_one(&self.pool)
            .await
            .map_err(catalog_db_error)?;
        let health: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_health")
            .fetch_one(&self.pool)
            .await
            .map_err(catalog_db_error)?;

        Ok(HydrationReport {
            active_project_count: active as usize,
            total_project_count: total as usize,
            locator_count: locators as usize,
            health_count: health as usize,
        })
    }

    /// Bind an existing workspace to a project (internal helper).
    async fn bind_workspace_to_project(
        &self,
        workspace_id: &WorkspaceId,
        project_id: &ProjectId,
        repository_id: Option<&crate::identity::RepositoryId>,
        source: &str,
    ) -> Result<(), CatalogError> {
        let now = Utc::now();
        let workspace_row = sqlx::query("SELECT canonical_root FROM workspace WHERE id = ?")
            .bind(workspace_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(catalog_db_error)?
            .ok_or_else(|| {
                CatalogError::InvalidValue("workspace not found for binding".to_string())
            })?;
        let locator_root: String = workspace_row.get("canonical_root");

        sqlx::query(
            r#"INSERT INTO workspace_project_binding
               (workspace_id, project_id, repository_id, worktree_id, node_id,
                locator, status, source, revision, time_created, time_updated)
               VALUES (?, ?, ?, NULL, NULL, ?, 'resolved', ?, 1, ?, ?)
               ON CONFLICT(workspace_id) DO UPDATE SET
                project_id = excluded.project_id,
                repository_id = excluded.repository_id,
                locator = excluded.locator,
                status = excluded.status,
                source = excluded.source,
                revision = workspace_project_binding.revision + 1,
                time_updated = excluded.time_updated"#,
        )
        .bind(workspace_id.as_str())
        .bind(project_id.as_str())
        .bind(repository_id.map(|r| r.as_str()))
        .bind(&locator_root)
        .bind(source)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&self.pool)
        .await
        .map_err(catalog_db_error)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Conservative legacy association
// ---------------------------------------------------------------------------

/// Perform conservative legacy association. Checks the marker table; if
/// already migrated for this `source`, returns `AlreadyMigrated`. Otherwise
/// walks workspace rows and creates logical_project + workspace_project_binding
/// for each unique canonical_root that has no current project_id, ONLY if
/// the project lineage from repository_lineage is unambiguous.
pub async fn conservative_legacy_association(
    pool: &SqlitePool,
    workspaces: &[WorkspaceRecord],
    source: &str,
) -> Result<LegacyAssociationReport, CatalogError> {
    let source_bounded = bounded_text(
        source,
        MAX_REGISTRATION_SOURCE_LENGTH,
        "legacy association source",
    )
    .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;

    // Check marker
    let marker = sqlx::query(
        "SELECT source, completed_at, projects_associated, diagnostics_recorded FROM legacy_catalog_association_marker WHERE source = ?",
    )
    .bind(&source_bounded)
    .fetch_optional(pool)
    .await
    .map_err(catalog_db_error)?;

    if marker.is_some() {
        return Ok(LegacyAssociationReport {
            projects_associated: 0,
            diagnostics_recorded: 0,
            already_migrated: true,
        });
    }

    let mut projects_associated: usize = 0;
    let mut diagnostics_recorded: usize = 0;

    let mut tx = pool.begin().await.map_err(catalog_db_error)?;

    for workspace in workspaces {
        // Check if workspace already has a binding
        let existing =
            sqlx::query("SELECT project_id FROM workspace_project_binding WHERE workspace_id = ?")
                .bind(workspace.id.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(catalog_db_error)?;

        if existing.is_some() {
            continue;
        }

        // Inspect repository lineage
        let evidence =
            crate::repository_lineage::inspect_repository_lineage(&workspace.canonical_root);

        match evidence {
            Ok(evidence) => {
                // Check if unambiguous
                if let Some(lineage_key) = evidence.equality_key() {
                    // Look for existing project with this repository
                    let repo_row = sqlx::query(
                        "SELECT id FROM repository WHERE vcs_kind = 'git' AND lineage_key = ?",
                    )
                    .bind(&lineage_key)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(catalog_db_error)?;

                    let project_id = if let Some(row) = repo_row {
                        let repo_id: String = row.get("id");
                        // Find existing project for this repo
                        let proj_row = sqlx::query(
                            "SELECT project_id FROM project_repository WHERE repository_id = ? AND relation_kind = 'primary' LIMIT 1",
                        )
                        .bind(&repo_id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(catalog_db_error)?;

                        if let Some(row) = proj_row {
                            ProjectId::parse(row.get::<String, _>("project_id").as_str())
                                .map_err(catalog_identity_error)?
                        } else {
                            // Create new project
                            let new_project = ProjectId::new();
                            let now = Utc::now();
                            let display_name = bounded_text(
                                &workspace.display_name,
                                MAX_DISPLAY_NAME_LENGTH,
                                "display name",
                            )
                            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;
                            sqlx::query(
                                r#"INSERT INTO logical_project
                                   (id, display_name, lifecycle, registration_source,
                                    time_created, time_updated)
                                   VALUES (?, ?, 'active', ?, ?, ?)"#,
                            )
                            .bind(new_project.as_str())
                            .bind(&display_name)
                            .bind(&source_bounded)
                            .bind(now.timestamp_millis())
                            .bind(now.timestamp_millis())
                            .execute(&mut *tx)
                            .await
                            .map_err(catalog_db_error)?;

                            // Create project_repository relation
                            sqlx::query(
                                "INSERT INTO project_repository (project_id, repository_id, relation_kind, time_created, revision) VALUES (?, ?, 'primary', ?, 1) ON CONFLICT(project_id, repository_id) DO NOTHING",
                            )
                            .bind(new_project.as_str())
                            .bind(&repo_id)
                            .bind(now.timestamp_millis())
                            .execute(&mut *tx)
                            .await
                            .map_err(catalog_db_error)?;

                            new_project
                        }
                    } else {
                        // No existing repo, create new project
                        let new_project = ProjectId::new();
                        let now = Utc::now();
                        let display_name = bounded_text(
                            &workspace.display_name,
                            MAX_DISPLAY_NAME_LENGTH,
                            "display name",
                        )
                        .map_err(|e| CatalogError::InvalidValue(e.to_string()))?;
                        sqlx::query(
                            r#"INSERT INTO logical_project
                               (id, display_name, lifecycle, registration_source,
                                time_created, time_updated)
                               VALUES (?, ?, 'active', ?, ?, ?)"#,
                        )
                        .bind(new_project.as_str())
                        .bind(&display_name)
                        .bind(&source_bounded)
                        .bind(now.timestamp_millis())
                        .bind(now.timestamp_millis())
                        .execute(&mut *tx)
                        .await
                        .map_err(catalog_db_error)?;

                        // Create repository
                        let repo_id = crate::identity::RepositoryId::new();
                        let _remote_identity =
                            crate::project_storage::ProjectStorage::new(pool.clone());
                        // We can't call project_storage methods in a tx context easily,
                        // so we insert directly
                        sqlx::query(
                            "INSERT INTO repository (id, vcs_kind, lineage_key, remote_identity, provenance, status, time_created, time_updated) VALUES (?, 'git', ?, NULL, ?, 'resolved', ?, ?)",
                        )
                        .bind(repo_id.as_str())
                        .bind(&lineage_key)
                        .bind(&source_bounded)
                        .bind(now.timestamp_millis())
                        .bind(now.timestamp_millis())
                        .execute(&mut *tx)
                        .await
                        .map_err(catalog_db_error)?;

                        sqlx::query(
                            "INSERT INTO project_repository (project_id, repository_id, relation_kind, time_created, revision) VALUES (?, ?, 'primary', ?, 1) ON CONFLICT(project_id, repository_id) DO NOTHING",
                        )
                        .bind(new_project.as_str())
                        .bind(repo_id.as_str())
                        .bind(now.timestamp_millis())
                        .execute(&mut *tx)
                        .await
                        .map_err(catalog_db_error)?;

                        new_project
                    };

                    // Bind workspace
                    let now = Utc::now();
                    let locator_root = workspace.canonical_root.to_string_lossy().to_string();
                    sqlx::query(
                        r#"INSERT INTO workspace_project_binding
                           (workspace_id, project_id, repository_id, worktree_id, node_id,
                            locator, status, source, revision, time_created, time_updated)
                           VALUES (?, ?, NULL, NULL, NULL, ?, 'resolved', ?, 1, ?, ?)
                           ON CONFLICT(workspace_id) DO NOTHING"#,
                    )
                    .bind(workspace.id.as_str())
                    .bind(project_id.as_str())
                    .bind(&locator_root)
                    .bind(&source_bounded)
                    .bind(now.timestamp_millis())
                    .bind(now.timestamp_millis())
                    .execute(&mut *tx)
                    .await
                    .map_err(catalog_db_error)?;

                    projects_associated += 1;
                } else {
                    // Ambiguous — record diagnostic
                    let now = Utc::now();
                    let diag_id = uuid::Uuid::new_v4().to_string();
                    sqlx::query(
                        r#"INSERT INTO identity_diagnostic
                           (id, workspace_id, session_id, project_id, code, status, message, source, time_created, time_updated)
                           VALUES (?, ?, NULL, NULL, 'legacy_catalog_ambiguous', 'ambiguous',
                           'workspace has ambiguous repository lineage; cannot auto-associate', ?, ?, ?)"#,
                    )
                    .bind(&diag_id)
                    .bind(workspace.id.as_str())
                    .bind(&source_bounded)
                    .bind(now.timestamp_millis())
                    .bind(now.timestamp_millis())
                    .execute(&mut *tx)
                    .await
                    .map_err(catalog_db_error)?;

                    diagnostics_recorded += 1;
                }
            }
            Err(_) => {
                // No lineage evidence — record diagnostic
                let now = Utc::now();
                let diag_id = uuid::Uuid::new_v4().to_string();
                sqlx::query(
                    r#"INSERT INTO identity_diagnostic
                       (id, workspace_id, session_id, project_id, code, status, message, source, time_created, time_updated)
                       VALUES (?, ?, NULL, NULL, 'legacy_catalog_no_lineage', 'rebind_required',
                       'workspace has no repository lineage; cannot auto-associate', ?, ?, ?)"#,
                )
                .bind(&diag_id)
                .bind(workspace.id.as_str())
                .bind(&source_bounded)
                .bind(now.timestamp_millis())
                .bind(now.timestamp_millis())
                .execute(&mut *tx)
                .await
                .map_err(catalog_db_error)?;

                diagnostics_recorded += 1;
            }
        }
    }

    // Write marker
    let now = Utc::now();
    sqlx::query(
        r#"INSERT INTO legacy_catalog_association_marker
           (source, completed_at, projects_associated, diagnostics_recorded)
           VALUES (?, ?, ?, ?)"#,
    )
    .bind(&source_bounded)
    .bind(now.timestamp_millis())
    .bind(projects_associated as i64)
    .bind(diagnostics_recorded as i64)
    .execute(&mut *tx)
    .await
    .map_err(catalog_db_error)?;

    tx.commit().await.map_err(catalog_db_error)?;

    Ok(LegacyAssociationReport {
        projects_associated,
        diagnostics_recorded,
        already_migrated: false,
    })
}

// ---------------------------------------------------------------------------
// Row parsing helpers
// ---------------------------------------------------------------------------

fn catalog_record_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ProjectCatalogRecord, CatalogError> {
    let tags_json: Option<String> = row.get("tags");
    let tags: Vec<String> = tags_json
        .map(|json| {
            serde_json::from_str::<Vec<String>>(&json)
                .map_err(|e| CatalogError::InvalidValue(format!("invalid tags: {e}")))
        })
        .transpose()?
        .unwrap_or_default();

    Ok(ProjectCatalogRecord {
        project_id: ProjectId::parse(row.get::<String, _>("id").as_str())
            .map_err(catalog_identity_error)?,
        display_name: bounded_text(
            row.get::<String, _>("display_name").as_str(),
            MAX_DISPLAY_NAME_LENGTH,
            "display name",
        )
        .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        lifecycle: crate::project_storage::ProjectLifecycle::parse(
            row.get::<String, _>("lifecycle").as_str(),
        )
        .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        description: row.get("description"),
        tags,
        time_last_opened_at: row
            .get::<Option<i64>, _>("time_last_opened")
            .map(|v| catalog_timestamp(v, "time_last_opened"))
            .transpose()
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        registration_source: bounded_text(
            row.get::<String, _>("registration_source").as_str(),
            MAX_REGISTRATION_SOURCE_LENGTH,
            "registration source",
        )
        .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        archived_at: row
            .get::<Option<i64>, _>("archived_at")
            .map(|v| catalog_timestamp(v, "archived_at"))
            .transpose()
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        created_at: catalog_timestamp(row.get("time_created"), "project.time_created")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        updated_at: catalog_timestamp(row.get("time_updated"), "project.time_updated")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
    })
}

fn health_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ProjectHealthRecord, CatalogError> {
    Ok(ProjectHealthRecord {
        project_id: ProjectId::parse(row.get::<String, _>("project_id").as_str())
            .map_err(catalog_identity_error)?,
        status: HealthStatus::parse(row.get::<String, _>("status").as_str())
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        error_code: row.get("error_code"),
        error_message: row.get("error_message"),
        source: bounded_text(
            row.get::<String, _>("source").as_str(),
            MAX_REGISTRATION_SOURCE_LENGTH,
            "health source",
        )
        .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        evaluated_at: catalog_timestamp(row.get("time_evaluated"), "health.time_evaluated")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        notes: row.get("notes"),
    })
}

fn locator_from_row(row: sqlx::sqlite::SqliteRow) -> Result<CatalogLocatorRecord, CatalogError> {
    let kind: String = row.get("locator_kind");
    let locator = match kind.as_str() {
        "local" => {
            let ws_id: Option<String> = row.get("workspace_id");
            let root: Option<String> = row.get("canonical_root");
            Locator::Local {
                workspace_id: ws_id
                    .map(|s| WorkspaceId::parse(&s))
                    .transpose()
                    .map_err(catalog_identity_error)?
                    .unwrap_or_default(),
                canonical_root: root.map(PathBuf::from).unwrap_or_default(),
            }
        }
        "ssh" => Locator::Ssh {
            host: row.get::<String, _>("ssh_host"),
            port: row.get::<Option<i64>, _>("ssh_port").map(|p| p as u16),
            user: row.get("ssh_user"),
            path: row.get::<String, _>("ssh_path"),
            label: row.get("ssh_label"),
        },
        "linked_node" => Locator::LinkedNode {
            node_id: row
                .get::<Option<String>, _>("linked_node_id")
                .map(|s| NodeId::parse(&s))
                .transpose()
                .map_err(catalog_identity_error)?
                .unwrap_or_default(),
            alias: row.get("linked_node_alias"),
            path_hint: row.get("linked_node_path_hint"),
        },
        other => {
            return Err(CatalogError::InvalidValue(format!(
                "unknown locator kind {other:?}"
            )))
        }
    };

    Ok(CatalogLocatorRecord {
        id: row.get("id"),
        project_id: ProjectId::parse(row.get::<String, _>("project_id").as_str())
            .map_err(catalog_identity_error)?,
        locator,
        display_summary: bounded_text(
            row.get::<String, _>("display_summary").as_str(),
            MAX_CATALOG_TEXT_LENGTH,
            "display summary",
        )
        .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        source: bounded_text(
            row.get::<String, _>("source").as_str(),
            MAX_REGISTRATION_SOURCE_LENGTH,
            "locator source",
        )
        .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        created_at: catalog_timestamp(row.get("time_created"), "locator.time_created")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
        updated_at: catalog_timestamp(row.get("time_updated"), "locator.time_updated")
            .map_err(|e| CatalogError::InvalidValue(e.to_string()))?,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::schema;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::process::Command;

    /// In-memory pool with the canonical schema applied. Mirrors the storage
    /// test harness: migrations run inside, never added a second time.
    async fn catalog_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        schema::migrate(&pool).await.unwrap();
        pool
    }

    fn workspace_record(root: &std::path::Path) -> WorkspaceRecord {
        let now = Utc::now();
        WorkspaceRecord {
            id: WorkspaceId::new(),
            canonical_root: root.to_path_buf(),
            display_name: root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.to_string_lossy().into_owned()),
            created_at: now,
            last_opened_at: now,
            archived_at: None,
        }
    }

    async fn register_workspace(pool: &SqlitePool, workspace: &WorkspaceRecord) {
        sqlx::query(
            "INSERT INTO workspace (id, canonical_root, display_name, time_created, time_last_opened, time_archived) VALUES (?, ?, ?, ?, ?, NULL)",
        )
        .bind(workspace.id.as_str())
        .bind(workspace.canonical_root.to_string_lossy().as_ref())
        .bind(&workspace.display_name)
        .bind(workspace.created_at.timestamp_millis())
        .bind(workspace.last_opened_at.timestamp_millis())
        .execute(pool)
        .await
        .unwrap();
    }

    fn input(name: &str) -> RegisterLocalProject {
        RegisterLocalProject {
            display_name: name.to_string(),
            description: None,
            tags: Vec::new(),
            primary_repository_id: None,
        }
    }

    /// Create a real repository with exactly one remote so the local lineage
    /// probe has usable evidence. Uses the same direct-git approach as the
    /// `repository_lineage` tests.
    fn init_repo_with_remote(root: &std::path::Path, remote: &str) {
        let init = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root)
            .output()
            .expect("git is installed");
        assert!(
            init.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        let add_remote = Command::new("git")
            .args(["remote", "add", "origin", remote])
            .current_dir(root)
            .output()
            .expect("git remote add");
        assert!(
            add_remote.status.success(),
            "git remote add failed: {}",
            String::from_utf8_lossy(&add_remote.stderr)
        );
    }

    fn init_repo_without_remote(root: &std::path::Path) {
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root)
            .status()
            .expect("git is installed");
    }

    async fn bound_repository(pool: &SqlitePool, workspace_id: &WorkspaceId) -> Option<String> {
        sqlx::query("SELECT repository_id FROM workspace_project_binding WHERE workspace_id = ?")
            .bind(workspace_id.as_str())
            .fetch_one(pool)
            .await
            .unwrap()
            .get::<Option<String>, _>("repository_id")
    }

    /// Regression: local registration used to record `status = 'resolved'`
    /// with a NULL repository and never probe Git, so the `repository` table
    /// stayed empty for every TUI-launched project. A delegated run that needs
    /// managed isolation then failed `isolation_unavailable` even though the
    /// root had unique remote evidence.
    #[tokio::test(flavor = "current_thread")]
    async fn registration_binds_repository_identity_from_lineage() {
        let root = tempfile::tempdir().unwrap();
        init_repo_with_remote(root.path(), "https://example.com/team/alpha.git");
        let workspace = workspace_record(root.path());
        let pool = catalog_pool().await;
        register_workspace(&pool, &workspace).await;
        let catalog = ProjectCatalog::new(pool.clone());

        let project = catalog
            .register_local_project(input("alpha"), &workspace.id, "tui-launch")
            .await
            .unwrap();

        let repository_id = bound_repository(&pool, &workspace.id).await;
        assert!(
            repository_id.is_some(),
            "a unique remote must produce a repository identity"
        );
        let repository_id = repository_id.unwrap();
        let linked: i64 =
            sqlx::query_scalar("SELECT count(*) FROM project_repository WHERE project_id = ? AND repository_id = ? AND relation_kind = 'primary'")
                .bind(project.project_id.as_str())
                .bind(&repository_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(linked, 1, "project must claim the repository as primary");
    }

    /// A root that is not a repository still registers a project — it simply
    /// has no repository identity. This is what lets a mutating delegated run
    /// execute in place instead of failing for want of a worktree.
    #[tokio::test(flavor = "current_thread")]
    async fn non_repository_root_registers_without_repository_identity() {
        let root = tempfile::tempdir().unwrap();
        let workspace = workspace_record(root.path());
        let pool = catalog_pool().await;
        register_workspace(&pool, &workspace).await;
        let catalog = ProjectCatalog::new(pool.clone());

        let project = catalog
            .register_local_project(input("plain"), &workspace.id, "tui-launch")
            .await
            .unwrap();

        assert_eq!(
            project.project_id.to_string(),
            project.project_id.to_string()
        );
        assert_eq!(bound_repository(&pool, &workspace.id).await, None);
    }

    /// A repository with no remote is not lineage evidence. It must not be
    /// invented from the path.
    #[tokio::test(flavor = "current_thread")]
    async fn repository_without_remote_is_not_given_an_identity() {
        let root = tempfile::tempdir().unwrap();
        init_repo_without_remote(root.path());
        let workspace = workspace_record(root.path());
        let pool = catalog_pool().await;
        register_workspace(&pool, &workspace).await;
        let catalog = ProjectCatalog::new(pool.clone());

        catalog
            .register_local_project(input("no-remote"), &workspace.id, "tui-launch")
            .await
            .unwrap();

        assert_eq!(bound_repository(&pool, &workspace.id).await, None);
        let repositories: i64 = sqlx::query_scalar("SELECT count(*) FROM repository")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            repositories, 0,
            "no lineage evidence means no repository row"
        );
    }

    /// The binding that started this bug is permanently `resolved` with a NULL
    /// repository, and `ProjectStorage::reconcile_workspace_once` treats a
    /// resolved binding as authoritative and never re-probes. A later launch
    /// must therefore heal it, or existing workspaces stay broken forever.
    #[tokio::test(flavor = "current_thread")]
    async fn existing_binding_without_repository_is_healed() {
        let root = tempfile::tempdir().unwrap();
        init_repo_with_remote(root.path(), "https://example.com/team/heal.git");
        let workspace = workspace_record(root.path());
        let pool = catalog_pool().await;
        register_workspace(&pool, &workspace).await;

        // Reproduce the poisoned binding the launch bootstrap used to write.
        let seeded = ProjectId::new();
        sqlx::query(
            "INSERT INTO logical_project (id, display_name, lifecycle, registration_source, time_created, time_updated) VALUES (?, 'seeded', 'active', 'tui-launch', 0, 0)",
        )
        .bind(seeded.as_str())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO workspace_project_binding (workspace_id, project_id, repository_id, worktree_id, node_id, locator, status, source, revision, time_created, time_updated) VALUES (?, ?, NULL, NULL, NULL, ?, 'resolved', 'tui-launch', 1, 0, 0)",
        )
        .bind(workspace.id.as_str())
        .bind(seeded.as_str())
        .bind(workspace.canonical_root.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();

        let catalog = ProjectCatalog::new(pool.clone());
        let project = catalog
            .register_local_project(input("heal"), &workspace.id, "tui-launch")
            .await
            .unwrap();

        assert_eq!(
            project.project_id, seeded,
            "healing must attach identity to the existing project, not churn it"
        );
        assert!(
            bound_repository(&pool, &workspace.id).await.is_some(),
            "a resolved binding with no repository must gain one"
        );
    }

    /// Identity follows lineage, not path: a second workspace with the same
    /// remote (a worktree, or a renamed directory) lands on the same project.
    #[tokio::test(flavor = "current_thread")]
    async fn same_lineage_reuses_one_project_across_workspaces() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        init_repo_with_remote(first_root.path(), "https://example.com/team/shared.git");
        init_repo_with_remote(second_root.path(), "https://example.com/team/shared.git");
        let first = workspace_record(first_root.path());
        let second = workspace_record(second_root.path());
        let pool = catalog_pool().await;
        register_workspace(&pool, &first).await;
        register_workspace(&pool, &second).await;
        let catalog = ProjectCatalog::new(pool.clone());

        let a = catalog
            .register_local_project(input("shared"), &first.id, "tui-launch")
            .await
            .unwrap();
        let b = catalog
            .register_local_project(input("shared"), &second.id, "tui-launch")
            .await
            .unwrap();

        assert_eq!(a.project_id, b.project_id);
    }

    /// A binding that already carries a repository is authoritative; healing
    /// must not overwrite it with a different one.
    #[tokio::test(flavor = "current_thread")]
    async fn existing_repository_identity_is_never_overwritten() {
        let root = tempfile::tempdir().unwrap();
        init_repo_with_remote(root.path(), "https://example.com/team/keep.git");
        let workspace = workspace_record(root.path());
        let pool = catalog_pool().await;
        register_workspace(&pool, &workspace).await;

        let seeded_project = ProjectId::new();
        let seeded_repository = crate::identity::RepositoryId::new();
        let now = Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO logical_project (id, display_name, lifecycle, registration_source, time_created, time_updated) VALUES (?, 'seeded', 'active', 'tui-launch', ?, ?)",
        )
        .bind(seeded_project.as_str())
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO repository (id, vcs_kind, lineage_key, remote_identity, common_directory, default_branch, head, provenance, status, time_created, time_updated) VALUES (?, 'git', 'git:example.com/pinned', 'git:example.com/pinned', NULL, NULL, NULL, 'test', 'resolved', ?, ?)",
        )
        .bind(seeded_repository.as_str())
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO workspace_project_binding (workspace_id, project_id, repository_id, worktree_id, node_id, locator, status, source, revision, time_created, time_updated) VALUES (?, ?, ?, NULL, NULL, ?, 'resolved', 'tui-launch', 1, ?, ?)",
        )
        .bind(workspace.id.as_str())
        .bind(seeded_project.as_str())
        .bind(seeded_repository.as_str())
        .bind(workspace.canonical_root.to_string_lossy().as_ref())
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

        let catalog = ProjectCatalog::new(pool.clone());
        catalog
            .register_local_project(input("keep"), &workspace.id, "tui-launch")
            .await
            .unwrap();

        assert_eq!(
            bound_repository(&pool, &workspace.id).await,
            Some(seeded_repository.to_string()),
            "an existing repository identity must be retained"
        );
        let repositories: i64 = sqlx::query_scalar("SELECT count(*) FROM repository")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            repositories, 1,
            "re-probing must not create a second repository for the same root"
        );
    }

    #[test]
    fn locator_is_local_only_for_local_variant() {
        let local = Locator::Local {
            workspace_id: WorkspaceId::new(),
            canonical_root: PathBuf::from("/tmp/test"),
        };
        assert!(local.is_local());

        let ssh = Locator::Ssh {
            host: "example.com".to_string(),
            port: Some(22),
            user: None,
            path: "/repo".to_string(),
            label: None,
        };
        assert!(!ssh.is_local());

        let node = Locator::LinkedNode {
            node_id: NodeId::new(),
            alias: None,
            path_hint: None,
        };
        assert!(!node.is_local());
    }

    #[test]
    fn locator_summary_does_not_leak_paths_for_non_local() {
        let ssh = Locator::Ssh {
            host: "example.com".to_string(),
            port: Some(22),
            user: Some("dev".to_string()),
            path: "/home/dev/repo".to_string(),
            label: Some("staging".to_string()),
        };
        let summary = ssh.summary();
        assert!(summary.starts_with("ssh:"));
        assert!(!summary.contains("/home/dev/repo"));

        let node = Locator::LinkedNode {
            node_id: NodeId::parse("node-abc").unwrap(),
            alias: Some("office".to_string()),
            path_hint: None,
        };
        let summary = node.summary();
        assert!(summary.starts_with("node:"));
    }

    #[test]
    fn catalog_error_is_send_sync_static() {
        fn _assert<T: Send + Sync + 'static>() {}
        _assert::<CatalogError>();
    }

    #[test]
    fn health_status_serde_round_trip() {
        let statuses = [
            HealthStatus::Unknown,
            HealthStatus::Available,
            HealthStatus::Unavailable,
            HealthStatus::Unsupported,
            HealthStatus::Stale,
            HealthStatus::Error,
        ];
        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let decoded: HealthStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(*status, decoded);
        }
    }

    #[test]
    fn project_catalog_record_json_round_trip() {
        let record = ProjectCatalogRecord {
            project_id: ProjectId::new(),
            display_name: "Test Project".to_string(),
            lifecycle: ProjectLifecycle::Active,
            description: Some("A test".to_string()),
            tags: vec!["rust".to_string()],
            time_last_opened_at: None,
            registration_source: "test".to_string(),
            archived_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&record).unwrap();
        let decoded: ProjectCatalogRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record.project_id, decoded.project_id);
        assert_eq!(record.display_name, decoded.display_name);
        assert_eq!(record.tags, decoded.tags);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalid_tags_are_reported_in_catalog_rows() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let row = sqlx::query(
            "SELECT 'project-1' AS id, 'Project' AS display_name, 'active' AS lifecycle, \
             CAST(NULL AS TEXT) AS description, 'not-json' AS tags, \
             CAST(NULL AS INTEGER) AS time_last_opened, 'test' AS registration_source, \
             CAST(NULL AS INTEGER) AS archived_at, 1 AS time_created, 1 AS time_updated",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let error = catalog_record_from_row(row).unwrap_err();
        assert!(
            matches!(error, CatalogError::InvalidValue(message) if message.contains("invalid tags"))
        );
    }

    #[test]
    fn locator_validation_rejects_empty_fields() {
        let ssh = Locator::Ssh {
            host: "".to_string(),
            port: None,
            user: None,
            path: "/repo".to_string(),
            label: None,
        };
        assert!(ssh.validate().is_err());

        let ssh2 = Locator::Ssh {
            host: "example.com".to_string(),
            port: None,
            user: None,
            path: "".to_string(),
            label: None,
        };
        assert!(ssh2.validate().is_err());
    }

    #[test]
    fn locator_validation_rejects_oversized_fields() {
        let oversized = "x".repeat(MAX_LOCATOR_FIELD_LENGTH + 1);
        let ssh = Locator::Ssh {
            host: oversized.clone(),
            port: None,
            user: None,
            path: "/repo".to_string(),
            label: None,
        };
        assert!(ssh.validate().is_err());

        let ssh2 = Locator::Ssh {
            host: "example.com".to_string(),
            port: None,
            user: None,
            path: oversized,
            label: None,
        };
        assert!(ssh2.validate().is_err());
    }

    #[test]
    fn locator_validation_rejects_embedded_credentials() {
        let ssh = Locator::Ssh {
            host: "example.com".to_string(),
            port: None,
            user: None,
            path: "ssh://user:secret@example.com/repo".to_string(),
            label: None,
        };
        assert!(ssh.validate().is_err());
    }

    #[test]
    fn locator_validation_rejects_control_characters() {
        let ssh = Locator::Ssh {
            host: "example.com".to_string(),
            port: None,
            user: None,
            path: "/repo\x00".to_string(),
            label: None,
        };
        assert!(ssh.validate().is_err());
    }

    #[test]
    fn lifecycle_counts_default() {
        let counts = LifecycleCounts::default();
        assert_eq!(counts.active, 0);
        assert_eq!(counts.archived, 0);
        assert_eq!(counts.total, 0);
    }

    #[test]
    fn hydration_report_default() {
        let report = HydrationReport::default();
        assert_eq!(report.active_project_count, 0);
        assert_eq!(report.total_project_count, 0);
        assert_eq!(report.locator_count, 0);
        assert_eq!(report.health_count, 0);
    }
}
