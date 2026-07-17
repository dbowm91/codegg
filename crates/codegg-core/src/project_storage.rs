//! Durable logical-project, repository, workspace-binding, and session-binding
//! storage.
//!
//! This module is deliberately additive. The historical `project` table and
//! string-backed session fields remain compatibility projections; the tables
//! owned here are the canonical authority introduced by Domain Identity
//! Milestone 002.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use thiserror::Error;

use crate::identity::{ProjectId, RepositoryId, WorkspaceId};
use crate::repository_lineage::{inspect_repository_lineage, RepositoryLineageEvidence};
use crate::workspace::WorkspaceRecord;

pub const MAX_DISPLAY_NAME_LENGTH: usize = 256;
pub const MAX_LINEAGE_VALUE_LENGTH: usize = 512;
pub const MAX_DIAGNOSTIC_LENGTH: usize = 1_024;

#[derive(Debug, Error)]
pub enum ProjectStorageError {
    #[error("identity storage database error: {0}")]
    Database(String),
    #[error("identity storage record not found: {0}")]
    NotFound(String),
    #[error("invalid identity storage value: {0}")]
    InvalidValue(String),
    #[error("stale {kind} binding revision: expected {expected}, actual {actual}")]
    RevisionConflict {
        kind: &'static str,
        expected: i64,
        actual: i64,
    },
    #[error("identity storage relation conflict: {0}")]
    Conflict(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingStatus {
    Resolved,
    Unresolved,
    Ambiguous,
    StaleLocator,
    RebindRequired,
}

impl BindingStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
            Self::Ambiguous => "ambiguous",
            Self::StaleLocator => "stale_locator",
            Self::RebindRequired => "rebind_required",
        }
    }

    fn parse(value: &str) -> Result<Self, ProjectStorageError> {
        match value {
            "resolved" => Ok(Self::Resolved),
            "unresolved" => Ok(Self::Unresolved),
            "ambiguous" => Ok(Self::Ambiguous),
            "stale_locator" => Ok(Self::StaleLocator),
            "rebind_required" => Ok(Self::RebindRequired),
            other => Err(ProjectStorageError::InvalidValue(format!(
                "unknown binding status {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectLifecycle {
    Active,
    Archived,
}

impl ProjectLifecycle {
    pub fn parse(value: &str) -> Result<Self, ProjectStorageError> {
        match value {
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            other => Err(ProjectStorageError::InvalidValue(format!(
                "unknown project lifecycle {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub project_id: ProjectId,
    pub display_name: String,
    pub lifecycle: ProjectLifecycle,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRecord {
    pub repository_id: RepositoryId,
    pub vcs_kind: String,
    pub lineage_key: Option<String>,
    pub remote_identity: Option<String>,
    pub common_directory: Option<String>,
    pub default_branch: Option<String>,
    pub head: Option<String>,
    pub provenance: String,
    pub status: BindingStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRepositoryRecord {
    pub project_id: ProjectId,
    pub repository_id: RepositoryId,
    pub relation_kind: String,
    pub created_at: DateTime<Utc>,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceBindingRecord {
    pub workspace_id: WorkspaceId,
    pub project_id: ProjectId,
    pub repository_id: Option<RepositoryId>,
    pub worktree_id: Option<String>,
    pub node_id: Option<String>,
    pub locator: Option<PathBuf>,
    pub status: BindingStatus,
    pub source: String,
    pub revision: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBindingRecord {
    pub session_id: String,
    pub project_id: Option<ProjectId>,
    pub workspace_id: Option<WorkspaceId>,
    pub status: BindingStatus,
    pub source: String,
    pub revision: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityDiagnostic {
    pub id: String,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<String>,
    pub project_id: Option<ProjectId>,
    pub code: String,
    pub status: BindingStatus,
    pub message: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceInspection {
    pub binding: Option<WorkspaceBindingRecord>,
    pub workspace: Option<WorkspaceRecord>,
    pub session_counts: BindingCounts,
    pub diagnostics: Vec<IdentityDiagnostic>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingCounts {
    pub resolved: usize,
    pub unresolved: usize,
    pub ambiguous: usize,
    pub stale_locator: usize,
    pub rebind_required: usize,
}

impl BindingCounts {
    fn add(&mut self, status: BindingStatus) {
        match status {
            BindingStatus::Resolved => self.resolved += 1,
            BindingStatus::Unresolved => self.unresolved += 1,
            BindingStatus::Ambiguous => self.ambiguous += 1,
            BindingStatus::StaleLocator => self.stale_locator += 1,
            BindingStatus::RebindRequired => self.rebind_required += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReconciliation {
    pub binding: WorkspaceBindingRecord,
    pub sessions_bound: usize,
    pub diagnostics: Vec<IdentityDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRebind {
    pub session_id: String,
    pub project_id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub expected_revision: i64,
}

#[derive(Clone)]
pub struct ProjectStorage {
    pool: SqlitePool,
}

impl ProjectStorage {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> SqlitePool {
        self.pool.clone()
    }

    pub async fn create_project(
        &self,
        display_name: impl AsRef<str>,
    ) -> Result<ProjectRecord, ProjectStorageError> {
        let display_name = bounded_text(
            display_name.as_ref(),
            MAX_DISPLAY_NAME_LENGTH,
            "display name",
        )?;
        let project_id = ProjectId::new();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO logical_project (id, display_name, lifecycle, time_created, time_updated) VALUES (?, ?, 'active', ?, ?)",
        )
        .bind(project_id.as_str())
        .bind(&display_name)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(ProjectRecord {
            project_id,
            display_name,
            lifecycle: ProjectLifecycle::Active,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Option<ProjectRecord>, ProjectStorageError> {
        let row = sqlx::query(
            "SELECT id, display_name, lifecycle, time_created, time_updated FROM logical_project WHERE id = ?",
        )
        .bind(project_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        row.map(project_from_row).transpose()
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>, ProjectStorageError> {
        let rows = sqlx::query(
            "SELECT id, display_name, lifecycle, time_created, time_updated FROM logical_project ORDER BY time_updated DESC, id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        rows.into_iter().map(project_from_row).collect()
    }

    pub async fn get_repository(
        &self,
        repository_id: &RepositoryId,
    ) -> Result<Option<RepositoryRecord>, ProjectStorageError> {
        let row = sqlx::query(
            "SELECT id, vcs_kind, lineage_key, remote_identity, common_directory, default_branch, head, provenance, status, time_created, time_updated FROM repository WHERE id = ?",
        )
        .bind(repository_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        row.map(repository_from_row).transpose()
    }

    pub async fn list_repositories(&self) -> Result<Vec<RepositoryRecord>, ProjectStorageError> {
        let rows = sqlx::query(
            "SELECT id, vcs_kind, lineage_key, remote_identity, common_directory, default_branch, head, provenance, status, time_created, time_updated FROM repository ORDER BY time_updated DESC, id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        rows.into_iter().map(repository_from_row).collect()
    }

    pub async fn list_project_repositories(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<ProjectRepositoryRecord>, ProjectStorageError> {
        let rows = sqlx::query(
            "SELECT project_id, repository_id, relation_kind, time_created, revision FROM project_repository WHERE project_id = ? ORDER BY relation_kind, repository_id",
        )
        .bind(project_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        rows.into_iter()
            .map(|row| {
                Ok(ProjectRepositoryRecord {
                    project_id: ProjectId::parse(row.get::<String, _>("project_id").as_str())
                        .map_err(identity_error)?,
                    repository_id: RepositoryId::parse(
                        row.get::<String, _>("repository_id").as_str(),
                    )
                    .map_err(identity_error)?,
                    relation_kind: bounded_text(
                        row.get::<String, _>("relation_kind").as_str(),
                        64,
                        "relation kind",
                    )?,
                    created_at: timestamp(
                        row.get("time_created"),
                        "project relation.time_created",
                    )?,
                    revision: row.get("revision"),
                })
            })
            .collect()
    }

    pub async fn workspace_binding(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Option<WorkspaceBindingRecord>, ProjectStorageError> {
        let row = sqlx::query(
            "SELECT workspace_id, project_id, repository_id, worktree_id, node_id, locator, status, source, revision, time_created, time_updated FROM workspace_project_binding WHERE workspace_id = ?",
        )
        .bind(workspace_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        row.map(workspace_binding_from_row).transpose()
    }

    pub async fn session_binding(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionBindingRecord>, ProjectStorageError> {
        let row = sqlx::query(
            "SELECT session_id, project_id, workspace_id, status, source, revision, time_created, time_updated FROM session_project_binding WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        row.map(session_binding_from_row).transpose()
    }

    /// Persist the canonical binding for an already-created session. Legacy
    /// session projections are intentionally not modified by this method.
    pub async fn bind_session(
        &self,
        session_id: &str,
        project_id: &ProjectId,
        workspace_id: &WorkspaceId,
        source: &str,
    ) -> Result<SessionBindingRecord, ProjectStorageError> {
        let workspace = self
            .workspace_binding(workspace_id)
            .await?
            .ok_or_else(|| ProjectStorageError::NotFound(format!("workspace {}", workspace_id)))?;
        if workspace.status != BindingStatus::Resolved || workspace.project_id != *project_id {
            return Err(ProjectStorageError::Conflict(
                "session binding does not match a resolved workspace binding".to_string(),
            ));
        }
        let source = bounded_text(source, MAX_LINEAGE_VALUE_LENGTH, "binding source")?;
        let now = Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO session_project_binding (session_id, project_id, workspace_id, status, source, revision, time_created, time_updated) VALUES (?, ?, ?, 'resolved', ?, 1, ?, ?) ON CONFLICT(session_id) DO UPDATE SET project_id = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.project_id ELSE excluded.project_id END, workspace_id = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.workspace_id ELSE excluded.workspace_id END, status = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.status ELSE excluded.status END, source = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.source ELSE excluded.source END, revision = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.revision ELSE session_project_binding.revision + 1 END, time_updated = excluded.time_updated",
        )
        .bind(session_id)
        .bind(project_id.as_str())
        .bind(workspace_id.as_str())
        .bind(&source)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        self.session_binding(session_id)
            .await?
            .ok_or_else(|| ProjectStorageError::NotFound(format!("session {session_id}")))
    }

    /// Reconcile one explicitly registered workspace. Repository evidence must
    /// be gathered before this call so no SQLite write transaction is held
    /// while Git is probed.
    pub async fn reconcile_workspace(
        &self,
        workspace: &WorkspaceRecord,
        evidence: &RepositoryLineageEvidence,
        source: &str,
    ) -> Result<WorkspaceReconciliation, ProjectStorageError> {
        for attempt in 0..3 {
            match self
                .reconcile_workspace_once(workspace, evidence, source)
                .await
            {
                Err(ProjectStorageError::Database(message))
                    if attempt < 2 && message.contains("database is locked") =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                result => return result,
            }
        }
        unreachable!("reconciliation retry loop always returns")
    }

    async fn reconcile_workspace_once(
        &self,
        workspace: &WorkspaceRecord,
        evidence: &RepositoryLineageEvidence,
        source: &str,
    ) -> Result<WorkspaceReconciliation, ProjectStorageError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let now = Utc::now();
        let source = bounded_text(source, MAX_LINEAGE_VALUE_LENGTH, "binding source")?;

        sqlx::query(
            "INSERT INTO workspace (id, canonical_root, display_name, time_created, time_last_opened, time_archived) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO NOTHING",
        )
        .bind(workspace.id.as_str())
        .bind(workspace.canonical_root.to_string_lossy().as_ref())
        .bind(&workspace.display_name)
        .bind(workspace.created_at.timestamp_millis())
        .bind(workspace.last_opened_at.timestamp_millis())
        .bind(workspace.archived_at.map(|value| value.timestamp_millis()))
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;

        if let Some(existing) = select_workspace_binding(&mut tx, &workspace.id).await? {
            // A successful binding is authoritative even if a later probe is
            // less informative. Retain the same non-resolved classification
            // on rerun so interruption recovery cannot manufacture IDs.
            if existing.status == BindingStatus::Resolved
                || existing.status == evidence_binding_status(evidence)
            {
                let sessions_bound = if existing.status == BindingStatus::Resolved {
                    bind_sessions_for_workspace(&mut tx, &existing, &source, now).await?
                } else {
                    0
                };
                tx.commit().await.map_err(db_error)?;
                return Ok(WorkspaceReconciliation {
                    binding: existing,
                    sessions_bound,
                    diagnostics: Vec::new(),
                });
            }
        }

        let (project_id, repository_id, status, diagnostics) =
            resolve_project_and_repository(&mut tx, workspace, evidence, &source, now).await?;

        let previous_revision = select_workspace_revision(&mut tx, &workspace.id).await?;
        let revision = previous_revision.unwrap_or(0) + 1;
        sqlx::query(
            "INSERT INTO workspace_project_binding (workspace_id, project_id, repository_id, worktree_id, node_id, locator, status, source, revision, time_created, time_updated) VALUES (?, ?, ?, NULL, NULL, ?, ?, ?, ?, ?, ?) ON CONFLICT(workspace_id) DO UPDATE SET project_id = excluded.project_id, repository_id = excluded.repository_id, locator = excluded.locator, status = excluded.status, source = excluded.source, revision = excluded.revision, time_updated = excluded.time_updated",
        )
        .bind(workspace.id.as_str())
        .bind(project_id.as_str())
        .bind(repository_id.as_ref().map(RepositoryId::as_str))
        .bind(workspace.canonical_root.to_string_lossy().as_ref())
        .bind(status.as_str())
        .bind(&source)
        .bind(revision)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;

        let binding = WorkspaceBindingRecord {
            workspace_id: workspace.id.clone(),
            project_id,
            repository_id,
            worktree_id: None,
            node_id: None,
            locator: Some(workspace.canonical_root.clone()),
            status,
            source: source.clone(),
            revision,
            created_at: now,
            updated_at: now,
        };
        let sessions_bound = if status == BindingStatus::Resolved {
            bind_sessions_for_workspace(&mut tx, &binding, &source, now).await?
        } else {
            0
        };
        tx.commit().await.map_err(db_error)?;

        Ok(WorkspaceReconciliation {
            binding,
            sessions_bound,
            diagnostics,
        })
    }

    /// Probe and reconcile one registered workspace. The probe runs before
    /// the storage transaction and is bounded by the lineage module.
    pub async fn reconcile_workspace_path(
        &self,
        workspace: &WorkspaceRecord,
        source: &str,
    ) -> Result<WorkspaceReconciliation, ProjectStorageError> {
        let evidence = inspect_repository_lineage(&workspace.canonical_root)
            .map_err(|error| ProjectStorageError::InvalidValue(error.to_string()))?;
        self.reconcile_workspace(workspace, &evidence, source).await
    }

    /// Reconcile all currently registered workspaces in bounded batches, then
    /// mark sessions without a resolvable workspace as rebind-required.
    pub async fn reconcile_catalog(
        &self,
        workspaces: &[WorkspaceRecord],
        batch_size: usize,
        source: &str,
    ) -> Result<Vec<WorkspaceReconciliation>, ProjectStorageError> {
        let batch_size = batch_size.max(1);
        let mut output = Vec::new();
        for batch in workspaces.chunks(batch_size) {
            for workspace in batch {
                output.push(self.reconcile_workspace_path(workspace, source).await?);
            }
        }
        self.mark_unbound_sessions(source).await?;
        Ok(output)
    }

    pub async fn rebind_workspace(
        &self,
        workspace_id: &WorkspaceId,
        project_id: &ProjectId,
        repository_id: Option<&RepositoryId>,
        expected_revision: i64,
        source: &str,
    ) -> Result<WorkspaceBindingRecord, ProjectStorageError> {
        if self.get_project(project_id).await?.is_none() {
            return Err(ProjectStorageError::NotFound(format!(
                "project {}",
                project_id
            )));
        }
        if let Some(repository_id) = repository_id {
            if self.get_repository(repository_id).await?.is_none() {
                return Err(ProjectStorageError::NotFound(format!(
                    "repository {}",
                    repository_id
                )));
            }
        }
        let source = bounded_text(source, MAX_LINEAGE_VALUE_LENGTH, "binding source")?;
        let now = Utc::now();
        let result = sqlx::query(
            "UPDATE workspace_project_binding SET project_id = ?, repository_id = ?, status = 'resolved', source = ?, revision = revision + 1, time_updated = ? WHERE workspace_id = ? AND revision = ?",
        )
        .bind(project_id.as_str())
        .bind(repository_id.map(RepositoryId::as_str))
        .bind(&source)
        .bind(now.timestamp_millis())
        .bind(workspace_id.as_str())
        .bind(expected_revision)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        if result.rows_affected() == 0 {
            let actual = self
                .workspace_binding(workspace_id)
                .await?
                .map(|binding| binding.revision)
                .ok_or_else(|| {
                    ProjectStorageError::NotFound(format!("workspace {}", workspace_id))
                })?;
            return Err(ProjectStorageError::RevisionConflict {
                kind: "workspace",
                expected: expected_revision,
                actual,
            });
        }
        self.workspace_binding(workspace_id)
            .await?
            .ok_or_else(|| ProjectStorageError::NotFound(format!("workspace {}", workspace_id)))
    }

    pub async fn rebind_session(
        &self,
        request: &SessionRebind,
        source: &str,
    ) -> Result<SessionBindingRecord, ProjectStorageError> {
        let workspace = self
            .workspace_binding(&request.workspace_id)
            .await?
            .ok_or_else(|| {
                ProjectStorageError::NotFound(format!("workspace {}", request.workspace_id))
            })?;
        if workspace.project_id != request.project_id || workspace.status != BindingStatus::Resolved
        {
            return Err(ProjectStorageError::Conflict(
                "session rebind must use the resolved workspace project".to_string(),
            ));
        }
        let source = bounded_text(source, MAX_LINEAGE_VALUE_LENGTH, "binding source")?;
        let now = Utc::now();
        let result = sqlx::query(
            "UPDATE session_project_binding SET project_id = ?, workspace_id = ?, status = 'resolved', source = ?, revision = revision + 1, time_updated = ? WHERE session_id = ? AND revision = ?",
        )
        .bind(request.project_id.as_str())
        .bind(request.workspace_id.as_str())
        .bind(&source)
        .bind(now.timestamp_millis())
        .bind(&request.session_id)
        .bind(request.expected_revision)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        if result.rows_affected() == 0 {
            let actual = self
                .session_binding(&request.session_id)
                .await?
                .map(|binding| binding.revision)
                .ok_or_else(|| {
                    ProjectStorageError::NotFound(format!("session {}", request.session_id))
                })?;
            return Err(ProjectStorageError::RevisionConflict {
                kind: "session",
                expected: request.expected_revision,
                actual,
            });
        }
        self.session_binding(&request.session_id)
            .await?
            .ok_or_else(|| ProjectStorageError::NotFound(format!("session {}", request.session_id)))
    }

    pub async fn inspect_workspace(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<WorkspaceInspection, ProjectStorageError> {
        let binding = self.workspace_binding(workspace_id).await?;
        let workspace = sqlx::query(
            "SELECT id, canonical_root, display_name, time_created, time_last_opened, time_archived FROM workspace WHERE id = ?",
        )
        .bind(workspace_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?
        .map(workspace_from_row)
        .transpose()?;
        let rows = sqlx::query("SELECT status FROM session_project_binding WHERE workspace_id = ?")
            .bind(workspace_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        let mut session_counts = BindingCounts::default();
        for row in rows {
            session_counts.add(BindingStatus::parse(
                row.get::<String, _>("status").as_str(),
            )?);
        }
        let diagnostics = self.list_diagnostics_for_workspace(workspace_id).await?;
        Ok(WorkspaceInspection {
            binding,
            workspace,
            session_counts,
            diagnostics,
        })
    }

    pub async fn list_diagnostics_for_workspace(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Vec<IdentityDiagnostic>, ProjectStorageError> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, session_id, project_id, code, status, message, source, time_created, time_updated FROM identity_diagnostic WHERE workspace_id = ? ORDER BY time_updated DESC, id",
        )
        .bind(workspace_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        rows.into_iter().map(diagnostic_from_row).collect()
    }

    async fn mark_unbound_sessions(&self, source: &str) -> Result<(), ProjectStorageError> {
        let source = bounded_text(source, MAX_LINEAGE_VALUE_LENGTH, "binding source")?;
        let rows = sqlx::query("SELECT id FROM session WHERE workspace_id IS NULL")
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        for row in rows {
            let session_id: String = row.get("id");
            let now = Utc::now().timestamp_millis();
            let result = sqlx::query(
                "INSERT INTO session_project_binding (session_id, project_id, workspace_id, status, source, revision, time_created, time_updated) VALUES (?, NULL, NULL, 'rebind_required', ?, 1, ?, ?) ON CONFLICT(session_id) DO NOTHING",
            )
            .bind(&session_id)
            .bind(&source)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
            if result.rows_affected() > 0 {
                self.record_diagnostic(
                    None,
                    Some(&session_id),
                    None,
                    "session_workspace_missing",
                    BindingStatus::RebindRequired,
                    "session has no resolvable workspace binding",
                    &source,
                )
                .await?;
            }
        }
        Ok(())
    }

    pub async fn record_diagnostic(
        &self,
        workspace_id: Option<&WorkspaceId>,
        session_id: Option<&str>,
        project_id: Option<&ProjectId>,
        code: &str,
        status: BindingStatus,
        message: &str,
        source: &str,
    ) -> Result<IdentityDiagnostic, ProjectStorageError> {
        let id = uuid::Uuid::new_v4().to_string();
        let code = bounded_text(code, MAX_LINEAGE_VALUE_LENGTH, "diagnostic code")?;
        let message = bounded_text(message, MAX_DIAGNOSTIC_LENGTH, "diagnostic message")?;
        let source = bounded_text(source, MAX_LINEAGE_VALUE_LENGTH, "diagnostic source")?;
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO identity_diagnostic (id, workspace_id, session_id, project_id, code, status, message, source, time_created, time_updated) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(workspace_id.map(WorkspaceId::as_str))
        .bind(session_id)
        .bind(project_id.map(ProjectId::as_str))
        .bind(&code)
        .bind(status.as_str())
        .bind(&message)
        .bind(&source)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(IdentityDiagnostic {
            id,
            workspace_id: workspace_id.cloned(),
            session_id: session_id.map(str::to_owned),
            project_id: project_id.cloned(),
            code,
            status,
            message,
            source,
            created_at: now,
            updated_at: now,
        })
    }
}

async fn resolve_project_and_repository(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    workspace: &WorkspaceRecord,
    evidence: &RepositoryLineageEvidence,
    source: &str,
    now: DateTime<Utc>,
) -> Result<
    (
        ProjectId,
        Option<RepositoryId>,
        BindingStatus,
        Vec<IdentityDiagnostic>,
    ),
    ProjectStorageError,
> {
    let mut diagnostics = Vec::new();
    let display_name = bounded_text(
        &workspace.display_name,
        MAX_DISPLAY_NAME_LENGTH,
        "display name",
    )?;
    let mut status = BindingStatus::Resolved;
    if let Some(lineage_key) = evidence.equality_key() {
        let row = sqlx::query("SELECT id FROM repository WHERE vcs_kind = ? AND lineage_key = ?")
            .bind("git")
            .bind(&lineage_key)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_error)?;
        let repo = if let Some(row) = row {
            RepositoryId::parse(row.get::<String, _>("id").as_str()).map_err(identity_error)?
        } else {
            let repo = RepositoryId::new();
            let remote_identity = evidence_remote_identity(evidence);
            sqlx::query(
                "INSERT INTO repository (id, vcs_kind, lineage_key, remote_identity, common_directory, default_branch, head, provenance, status, time_created, time_updated) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'resolved', ?, ?)",
            )
            .bind(repo.as_str())
            .bind("git")
            .bind(&lineage_key)
            .bind(remote_identity.as_deref())
            .bind(None::<&str>)
            .bind(None::<&str>)
            .bind(None::<&str>)
            .bind(source)
            .bind(now.timestamp_millis())
            .bind(now.timestamp_millis())
            .execute(&mut **tx)
            .await
            .map_err(db_error)?;
            repo
        };
        let repository_id = Some(repo.clone());

        let project_row = sqlx::query(
            "SELECT project_id FROM project_repository WHERE repository_id = ? AND relation_kind = 'primary' ORDER BY project_id LIMIT 2",
        )
        .bind(repo.as_str())
        .fetch_all(&mut **tx)
        .await
        .map_err(db_error)?;
        let project_id = if project_row.len() == 1 {
            ProjectId::parse(project_row[0].get::<String, _>("project_id").as_str())
                .map_err(identity_error)?
        } else {
            let project = ProjectId::new();
            sqlx::query(
                "INSERT INTO logical_project (id, display_name, lifecycle, time_created, time_updated) VALUES (?, ?, 'active', ?, ?)",
            )
            .bind(project.as_str())
            .bind(&display_name)
            .bind(now.timestamp_millis())
            .bind(now.timestamp_millis())
            .execute(&mut **tx)
            .await
            .map_err(db_error)?;
            project
        };
        sqlx::query(
            "INSERT INTO project_repository (project_id, repository_id, relation_kind, time_created, revision) VALUES (?, ?, 'primary', ?, 1) ON CONFLICT(project_id, repository_id) DO NOTHING",
        )
        .bind(project_id.as_str())
        .bind(repo.as_str())
        .bind(now.timestamp_millis())
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
        Ok((project_id, repository_id, status, diagnostics))
    } else {
        let project_id = ProjectId::new();
        sqlx::query(
            "INSERT INTO logical_project (id, display_name, lifecycle, time_created, time_updated) VALUES (?, ?, 'active', ?, ?)",
        )
        .bind(project_id.as_str())
        .bind(&display_name)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
        status = evidence_binding_status(evidence);
        let (code, message) = evidence_diagnostic(evidence);
        if status != BindingStatus::Resolved {
            diagnostics.push(IdentityDiagnostic {
                id: uuid::Uuid::new_v4().to_string(),
                workspace_id: Some(workspace.id.clone()),
                session_id: None,
                project_id: Some(project_id.clone()),
                code: code.to_owned(),
                status,
                message: message.to_owned(),
                source: source.to_owned(),
                created_at: now,
                updated_at: now,
            });
            sqlx::query(
                "INSERT INTO identity_diagnostic (id, workspace_id, session_id, project_id, code, status, message, source, time_created, time_updated) VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&diagnostics[0].id)
            .bind(workspace.id.as_str())
            .bind(project_id.as_str())
            .bind(code)
            .bind(status.as_str())
            .bind(message)
            .bind(source)
            .bind(now.timestamp_millis())
            .bind(now.timestamp_millis())
            .execute(&mut **tx)
            .await
            .map_err(db_error)?;
        }
        Ok((project_id, None, status, diagnostics))
    }
}

fn evidence_remote_identity(evidence: &RepositoryLineageEvidence) -> Option<String> {
    match evidence {
        RepositoryLineageEvidence::Unique { remote } => Some(remote.equality_key()),
        _ => None,
    }
}

fn evidence_binding_status(evidence: &RepositoryLineageEvidence) -> BindingStatus {
    match evidence {
        RepositoryLineageEvidence::StaleLocator => BindingStatus::StaleLocator,
        RepositoryLineageEvidence::NotRepository | RepositoryLineageEvidence::NoRemote => {
            BindingStatus::Resolved
        }
        RepositoryLineageEvidence::Unique { .. } => BindingStatus::Resolved,
        RepositoryLineageEvidence::Ambiguous { .. } => BindingStatus::Ambiguous,
        RepositoryLineageEvidence::Insufficient { .. } => BindingStatus::RebindRequired,
    }
}

fn evidence_diagnostic(evidence: &RepositoryLineageEvidence) -> (&'static str, &'static str) {
    match evidence {
        RepositoryLineageEvidence::Ambiguous { .. } => (
            "repository_lineage_ambiguous",
            "multiple repository lineage identities were found; explicit rebinding is required",
        ),
        RepositoryLineageEvidence::Insufficient { .. } => (
            "repository_lineage_insufficient",
            "repository lineage evidence was unsafe or insufficient for automatic binding",
        ),
        RepositoryLineageEvidence::StaleLocator => (
            "workspace_locator_stale",
            "workspace locator is missing or is no longer a directory; explicit rebinding is required",
        ),
        RepositoryLineageEvidence::NotRepository | RepositoryLineageEvidence::NoRemote => (
            "no_repository_lineage",
            "workspace has no uniquely identifiable repository lineage",
        ),
        RepositoryLineageEvidence::Unique { .. } => (
            "repository_lineage_resolved",
            "repository lineage was uniquely identified",
        ),
    }
}

async fn bind_sessions_for_workspace(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    binding: &WorkspaceBindingRecord,
    source: &str,
    now: DateTime<Utc>,
) -> Result<usize, ProjectStorageError> {
    let rows = sqlx::query("SELECT id FROM session WHERE workspace_id = ?")
        .bind(binding.workspace_id.as_str())
        .fetch_all(&mut **tx)
        .await
        .map_err(db_error)?;
    let mut count = 0;
    for row in rows {
        let session_id: String = row.get("id");
        sqlx::query(
            "INSERT INTO session_project_binding (session_id, project_id, workspace_id, status, source, revision, time_created, time_updated) VALUES (?, ?, ?, 'resolved', ?, 1, ?, ?) ON CONFLICT(session_id) DO UPDATE SET project_id = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.project_id ELSE excluded.project_id END, workspace_id = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.workspace_id ELSE excluded.workspace_id END, status = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.status ELSE excluded.status END, source = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.source ELSE excluded.source END, revision = CASE WHEN session_project_binding.status = 'resolved' THEN session_project_binding.revision ELSE session_project_binding.revision + 1 END, time_updated = excluded.time_updated",
        )
        .bind(&session_id)
        .bind(binding.project_id.as_str())
        .bind(binding.workspace_id.as_str())
        .bind(source)
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
        count += 1;
    }
    Ok(count)
}

async fn select_workspace_binding(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    workspace_id: &WorkspaceId,
) -> Result<Option<WorkspaceBindingRecord>, ProjectStorageError> {
    let row = sqlx::query(
        "SELECT workspace_id, project_id, repository_id, worktree_id, node_id, locator, status, source, revision, time_created, time_updated FROM workspace_project_binding WHERE workspace_id = ?",
    )
    .bind(workspace_id.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?;
    row.map(workspace_binding_from_row).transpose()
}

async fn select_workspace_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    workspace_id: &WorkspaceId,
) -> Result<Option<i64>, ProjectStorageError> {
    sqlx::query("SELECT revision FROM workspace_project_binding WHERE workspace_id = ?")
        .bind(workspace_id.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)
        .map(|row| row.map(|value| value.get("revision")))
}

pub fn bounded_text(value: &str, max: usize, field: &str) -> Result<String, ProjectStorageError> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(ProjectStorageError::InvalidValue(format!(
            "{field} is empty, oversized, or contains control characters"
        )));
    }
    Ok(value.to_owned())
}

pub fn identity_error(error: crate::identity::IdentityParseError) -> ProjectStorageError {
    ProjectStorageError::InvalidValue(error.to_string())
}

pub fn db_error(error: sqlx::Error) -> ProjectStorageError {
    ProjectStorageError::Database(error.to_string())
}

pub fn timestamp(value: i64, field: &str) -> Result<DateTime<Utc>, ProjectStorageError> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .ok_or_else(|| ProjectStorageError::InvalidValue(format!("invalid timestamp in {field}")))
}

fn project_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ProjectRecord, ProjectStorageError> {
    Ok(ProjectRecord {
        project_id: ProjectId::parse(row.get::<String, _>("id").as_str())
            .map_err(identity_error)?,
        display_name: bounded_text(
            row.get::<String, _>("display_name").as_str(),
            MAX_DISPLAY_NAME_LENGTH,
            "display name",
        )?,
        lifecycle: ProjectLifecycle::parse(row.get::<String, _>("lifecycle").as_str())?,
        created_at: timestamp(row.get("time_created"), "project.time_created")?,
        updated_at: timestamp(row.get("time_updated"), "project.time_updated")?,
    })
}

fn repository_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<RepositoryRecord, ProjectStorageError> {
    Ok(RepositoryRecord {
        repository_id: RepositoryId::parse(row.get::<String, _>("id").as_str())
            .map_err(identity_error)?,
        vcs_kind: bounded_text(row.get::<String, _>("vcs_kind").as_str(), 64, "VCS kind")?,
        lineage_key: bounded_optional(
            row.get("lineage_key"),
            MAX_LINEAGE_VALUE_LENGTH,
            "lineage key",
        )?,
        remote_identity: bounded_optional(
            row.get("remote_identity"),
            MAX_LINEAGE_VALUE_LENGTH,
            "remote identity",
        )?,
        common_directory: bounded_optional(
            row.get("common_directory"),
            MAX_LINEAGE_VALUE_LENGTH,
            "common directory",
        )?,
        default_branch: bounded_optional(
            row.get("default_branch"),
            MAX_LINEAGE_VALUE_LENGTH,
            "default branch",
        )?,
        head: bounded_optional(row.get("head"), MAX_LINEAGE_VALUE_LENGTH, "head")?,
        provenance: bounded_text(
            row.get::<String, _>("provenance").as_str(),
            MAX_LINEAGE_VALUE_LENGTH,
            "provenance",
        )?,
        status: BindingStatus::parse(row.get::<String, _>("status").as_str())?,
        created_at: timestamp(row.get("time_created"), "repository.time_created")?,
        updated_at: timestamp(row.get("time_updated"), "repository.time_updated")?,
    })
}

fn workspace_binding_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkspaceBindingRecord, ProjectStorageError> {
    Ok(WorkspaceBindingRecord {
        workspace_id: WorkspaceId::parse(row.get::<String, _>("workspace_id").as_str())
            .map_err(identity_error)?,
        project_id: ProjectId::parse(row.get::<String, _>("project_id").as_str())
            .map_err(identity_error)?,
        repository_id: row
            .get::<Option<String>, _>("repository_id")
            .as_deref()
            .map(RepositoryId::parse)
            .transpose()
            .map_err(identity_error)?,
        worktree_id: row.get("worktree_id"),
        node_id: row.get("node_id"),
        locator: row.get::<Option<String>, _>("locator").map(PathBuf::from),
        status: BindingStatus::parse(row.get::<String, _>("status").as_str())?,
        source: bounded_text(
            row.get::<String, _>("source").as_str(),
            MAX_LINEAGE_VALUE_LENGTH,
            "binding source",
        )?,
        revision: row.get("revision"),
        created_at: timestamp(row.get("time_created"), "workspace binding.time_created")?,
        updated_at: timestamp(row.get("time_updated"), "workspace binding.time_updated")?,
    })
}

fn session_binding_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<SessionBindingRecord, ProjectStorageError> {
    Ok(SessionBindingRecord {
        session_id: row.get("session_id"),
        project_id: row
            .get::<Option<String>, _>("project_id")
            .as_deref()
            .map(ProjectId::parse)
            .transpose()
            .map_err(identity_error)?,
        workspace_id: row
            .get::<Option<String>, _>("workspace_id")
            .as_deref()
            .map(WorkspaceId::parse)
            .transpose()
            .map_err(identity_error)?,
        status: BindingStatus::parse(row.get::<String, _>("status").as_str())?,
        source: bounded_text(
            row.get::<String, _>("source").as_str(),
            MAX_LINEAGE_VALUE_LENGTH,
            "binding source",
        )?,
        revision: row.get("revision"),
        created_at: timestamp(row.get("time_created"), "session binding.time_created")?,
        updated_at: timestamp(row.get("time_updated"), "session binding.time_updated")?,
    })
}

fn diagnostic_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<IdentityDiagnostic, ProjectStorageError> {
    Ok(IdentityDiagnostic {
        id: row.get("id"),
        workspace_id: row
            .get::<Option<String>, _>("workspace_id")
            .as_deref()
            .map(WorkspaceId::parse)
            .transpose()
            .map_err(identity_error)?,
        session_id: row.get("session_id"),
        project_id: row
            .get::<Option<String>, _>("project_id")
            .as_deref()
            .map(ProjectId::parse)
            .transpose()
            .map_err(identity_error)?,
        code: bounded_text(
            row.get::<String, _>("code").as_str(),
            MAX_LINEAGE_VALUE_LENGTH,
            "diagnostic code",
        )?,
        status: BindingStatus::parse(row.get::<String, _>("status").as_str())?,
        message: bounded_text(
            row.get::<String, _>("message").as_str(),
            MAX_DIAGNOSTIC_LENGTH,
            "diagnostic message",
        )?,
        source: bounded_text(
            row.get::<String, _>("source").as_str(),
            MAX_LINEAGE_VALUE_LENGTH,
            "diagnostic source",
        )?,
        created_at: timestamp(row.get("time_created"), "diagnostic.time_created")?,
        updated_at: timestamp(row.get("time_updated"), "diagnostic.time_updated")?,
    })
}

fn workspace_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<WorkspaceRecord, ProjectStorageError> {
    Ok(WorkspaceRecord {
        id: WorkspaceId::parse(row.get::<String, _>("id").as_str()).map_err(identity_error)?,
        canonical_root: PathBuf::from(row.get::<String, _>("canonical_root")),
        display_name: row.get("display_name"),
        created_at: timestamp(row.get("time_created"), "workspace.time_created")?,
        last_opened_at: timestamp(row.get("time_last_opened"), "workspace.time_last_opened")?,
        archived_at: row
            .get::<Option<i64>, _>("time_archived")
            .map(|value| timestamp(value, "workspace.time_archived"))
            .transpose()?,
    })
}

fn bounded_optional(
    value: Option<String>,
    max: usize,
    field: &str,
) -> Result<Option<String>, ProjectStorageError> {
    value
        .map(|value| bounded_text(&value, max, field))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::schema;
    use crate::workspace::WorkspaceId;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::path::Path;
    use std::str::FromStr;
    use tempfile::tempdir;

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        schema::migrate(&pool).await.unwrap();
        pool
    }

    async fn concurrent_pool() -> (SqlitePool, tempfile::TempDir) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("catalog.db");
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
            .unwrap()
            .create_if_missing(true)
            .foreign_keys(true);
        let setup = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        schema::migrate(&setup).await.unwrap();
        setup.close().await;
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
            .unwrap()
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap();
        (pool, directory)
    }

    fn workspace(root: &Path, name: &str) -> WorkspaceRecord {
        let now = Utc::now();
        WorkspaceRecord {
            id: WorkspaceId::new(),
            canonical_root: root.to_path_buf(),
            display_name: name.to_string(),
            created_at: now,
            last_opened_at: now,
            archived_at: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn schema_and_binding_round_trip() {
        let storage = ProjectStorage::new(pool().await);
        let root = tempdir().unwrap();
        let workspace = workspace(root.path(), "Example");
        let evidence =
            crate::repository_lineage::classify_remote_urls(["https://example.test/org/repo"]);
        let result = storage
            .reconcile_workspace(&workspace, &evidence, "test")
            .await
            .unwrap();
        assert_eq!(result.binding.status, BindingStatus::Resolved);
        assert!(result.binding.repository_id.is_some());
        assert_eq!(storage.list_projects().await.unwrap().len(), 1);
        assert_eq!(storage.list_repositories().await.unwrap().len(), 1);
        let persisted = storage
            .workspace_binding(&workspace.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted.workspace_id, result.binding.workspace_id);
        assert_eq!(persisted.project_id, result.binding.project_id);
        assert_eq!(persisted.repository_id, result.binding.repository_id);
        assert_eq!(persisted.status, result.binding.status);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn same_lineage_reuses_project_and_repository() {
        let storage = ProjectStorage::new(pool().await);
        let first_root = tempdir().unwrap();
        let second_root = tempdir().unwrap();
        let first = workspace(first_root.path(), "First");
        let second = workspace(second_root.path(), "Second");
        let evidence =
            crate::repository_lineage::classify_remote_urls(["git@example.test:org/repo.git"]);
        let first_binding = storage
            .reconcile_workspace(&first, &evidence, "test")
            .await
            .unwrap()
            .binding;
        let second_binding = storage
            .reconcile_workspace(&second, &evidence, "test")
            .await
            .unwrap()
            .binding;
        assert_eq!(first_binding.project_id, second_binding.project_id);
        assert_eq!(first_binding.repository_id, second_binding.repository_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_registration_converges_on_one_project_and_repository() {
        let (pool, _directory) = concurrent_pool().await;
        let storage = ProjectStorage::new(pool);
        let first_root = tempdir().unwrap();
        let second_root = tempdir().unwrap();
        let first = workspace(first_root.path(), "First");
        let second = workspace(second_root.path(), "Second");
        let evidence =
            crate::repository_lineage::classify_remote_urls(["https://example.test/repo"]);
        let (first_result, second_result) = tokio::join!(
            storage.reconcile_workspace(&first, &evidence, "concurrent"),
            storage.reconcile_workspace(&second, &evidence, "concurrent")
        );
        let first_binding = first_result.unwrap().binding;
        let second_binding = second_result.unwrap().binding;
        assert_eq!(first_binding.project_id, second_binding.project_id);
        assert_eq!(first_binding.repository_id, second_binding.repository_id);
        assert_eq!(storage.list_projects().await.unwrap().len(), 1);
        assert_eq!(storage.list_repositories().await.unwrap().len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_rebind_is_a_typed_conflict() {
        let storage = ProjectStorage::new(pool().await);
        let root = tempdir().unwrap();
        let workspace = workspace(root.path(), "Example");
        let evidence =
            crate::repository_lineage::classify_remote_urls(["https://example.test/repo"]);
        let binding = storage
            .reconcile_workspace(&workspace, &evidence, "test")
            .await
            .unwrap()
            .binding;
        let project = storage.create_project("Other").await.unwrap();
        let error = storage
            .rebind_workspace(
                &workspace.id,
                &project.project_id,
                None,
                binding.revision - 1,
                "test",
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProjectStorageError::RevisionConflict {
                kind: "workspace",
                ..
            }
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_rebind_resolves_an_ambiguous_binding() {
        let storage = ProjectStorage::new(pool().await);
        let root = tempdir().unwrap();
        let workspace = workspace(root.path(), "Example");
        let evidence = crate::repository_lineage::classify_remote_urls([
            "https://one.example/repo",
            "https://two.example/repo",
        ]);
        let initial = storage
            .reconcile_workspace(&workspace, &evidence, "migration")
            .await
            .unwrap()
            .binding;
        let replacement = storage.create_project("Explicit project").await.unwrap();
        let rebound = storage
            .rebind_workspace(
                &workspace.id,
                &replacement.project_id,
                None,
                initial.revision,
                "operator",
            )
            .await
            .unwrap();
        assert_eq!(rebound.status, BindingStatus::Resolved);
        assert_eq!(rebound.project_id, replacement.project_id);
        assert_eq!(rebound.revision, initial.revision + 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ambiguous_evidence_is_preserved_as_diagnostic() {
        let storage = ProjectStorage::new(pool().await);
        let root = tempdir().unwrap();
        let workspace = workspace(root.path(), "Example");
        let evidence = crate::repository_lineage::classify_remote_urls([
            "https://one.example/repo",
            "https://two.example/repo",
        ]);
        let result = storage
            .reconcile_workspace(&workspace, &evidence, "migration")
            .await
            .unwrap();
        let rerun = storage
            .reconcile_workspace(&workspace, &evidence, "migration")
            .await
            .unwrap();
        assert_eq!(result.binding.status, BindingStatus::Ambiguous);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(rerun.binding.project_id, result.binding.project_id);
        assert_eq!(storage.list_projects().await.unwrap().len(), 1);
        assert!(storage.list_repositories().await.unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn secret_bearing_lineage_is_not_persisted() {
        let storage = ProjectStorage::new(pool().await);
        let root = tempdir().unwrap();
        let workspace = workspace(root.path(), "Example");
        let evidence = crate::repository_lineage::classify_remote_urls([
            "https://user:super-secret@example.test/org/repo.git",
        ]);
        let result = storage
            .reconcile_workspace(&workspace, &evidence, "migration")
            .await
            .unwrap();
        assert_eq!(result.binding.status, BindingStatus::RebindRequired);
        assert!(storage.list_repositories().await.unwrap().is_empty());
        let diagnostics = storage
            .list_diagnostics_for_workspace(&workspace.id)
            .await
            .unwrap();
        assert!(!diagnostics.is_empty());
        assert!(diagnostics
            .iter()
            .all(|item| !item.message.contains("secret")));
    }
}
