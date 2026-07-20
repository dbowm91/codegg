//! Request-scoped project/workspace resolution for server adapters.
//!
//! A server process deliberately has no default project.  The only legacy
//! compatibility supported here is resolving a directory that already maps
//! to exactly one active project/workspace binding.

use serde::Deserialize;
use sqlx::SqlitePool;
use std::sync::Arc;

use crate::error::{AppError, AxumAppError, StorageError};
use codegg_core::context::{ProjectContext, ProjectContextRequest, ProjectContextResolver};
use codegg_core::project_catalog::ProjectCatalog;
use codegg_core::project_storage::ProjectStorage;
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceStore};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScopeQuery {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    /// Legacy compatibility locator. It is never an identity and must map
    /// uniquely to an existing active binding.
    pub directory: Option<String>,
}

impl ScopeQuery {
    pub(crate) fn from_json(params: &serde_json::Value) -> Self {
        params
            .as_object()
            .map(|params| Self {
                project_id: params
                    .get("project_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                workspace_id: params
                    .get("workspace_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                directory: params
                    .get("directory")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            })
            .unwrap_or_default()
    }
}

pub(crate) fn context_error(code: &str, message: impl Into<String>) -> AxumAppError {
    AppError::Storage(StorageError::NotFound(format!(
        "{code}: {}",
        message.into()
    )))
    .into()
}

pub(crate) fn resolver(pool: SqlitePool) -> ProjectContextResolver {
    let store: Arc<dyn WorkspaceStore> = Arc::new(SqliteWorkspaceStore::new(pool.clone()));
    ProjectContextResolver::new(
        ProjectStorage::new(pool.clone()),
        ProjectCatalog::new(pool),
        store,
    )
}

pub(crate) async fn resolve_context(
    pool: &SqlitePool,
    scope: &ScopeQuery,
    session_id: Option<&str>,
) -> Result<ProjectContext, AxumAppError> {
    match (
        scope.project_id.as_deref(),
        scope.workspace_id.as_deref(),
        scope.directory.as_deref(),
    ) {
        (Some(project_id), Some(workspace_id), _) => resolver(pool.clone())
            .resolve(
                ProjectContextRequest::from_raw(project_id, workspace_id, session_id)
                    .map_err(|e| context_error("invalid_project_context", e.to_string()))?,
            )
            .await
            .map_err(|e| context_error(context_error_code(&e), e.to_string())),
        (None, None, Some(directory)) => {
            let context = resolver(pool.clone())
                .resolve_directory(directory)
                .await
                .map_err(|e| context_error(context_error_code(&e), e.to_string()))?;
            if let Some(session_id) = session_id {
                resolver(pool.clone())
                    .resolve(
                        ProjectContextRequest::new(
                            context.project_id.clone(),
                            context.workspace_id.clone(),
                        )
                        .with_session_id(
                            codegg_core::context::SessionId::parse(session_id).map_err(|e| {
                                context_error("invalid_session_context", e.to_string())
                            })?,
                        ),
                    )
                    .await
                    .map_err(|e| context_error(context_error_code(&e), e.to_string()))
            } else {
                Ok(context)
            }
        }
        (Some(_), None, _) | (None, Some(_), _) => Err(context_error(
            "project_context_required",
            "project_id and workspace_id must be provided together",
        )),
        (None, None, None) => Err(context_error(
            "project_context_required",
            "provide project_id and workspace_id, or a unique directory locator",
        )),
    }
}

fn context_error_code(error: &codegg_core::context::ContextResolutionError) -> &'static str {
    use codegg_core::context::ContextResolutionError;
    match error {
        ContextResolutionError::DirectoryNotFound => "project_context_required",
        ContextResolutionError::DirectoryAmbiguous(_) => "ambiguous_project_context",
        ContextResolutionError::ProjectNotFound | ContextResolutionError::WorkspaceNotFound => {
            "project_context_not_found"
        }
        ContextResolutionError::ProjectArchived | ContextResolutionError::WorkspaceArchived => {
            "project_context_archived"
        }
        ContextResolutionError::BindingProjectMismatch
        | ContextResolutionError::SessionBindingMismatch => "project_context_mismatch",
        ContextResolutionError::BindingMissing
        | ContextResolutionError::BindingNotResolved { .. }
        | ContextResolutionError::SessionBindingMissing
        | ContextResolutionError::SessionBindingNotResolved { .. } => "project_context_unresolved",
        ContextResolutionError::InvalidInput(_) => "invalid_project_context",
        ContextResolutionError::CatalogFailure(_)
        | ContextResolutionError::StorageFailure(_)
        | ContextResolutionError::WorkspaceStoreFailure(_) => "project_context_unavailable",
    }
}

pub(crate) fn require_explicit_project(scope: &ScopeQuery) -> Result<&str, AxumAppError> {
    if scope.workspace_id.is_some() {
        return Err(context_error(
            "project_context_required",
            "workspace_id requires its project_id",
        ));
    }
    scope.project_id.as_deref().ok_or_else(|| {
        context_error(
            "project_context_required",
            "project_id is required for this project-scoped operation",
        )
    })
}
