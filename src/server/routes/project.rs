use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use super::super::state::ServerState;
use crate::error::{AppError, AxumAppError, StorageError};
use codegg_core::context::{ProjectContext, ProjectContextResolver};
use codegg_core::identity::{ProjectId, WorkspaceId};
use codegg_core::project_catalog::{ProjectCatalog, ProjectCatalogRecord, RegisterLocalProject};
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceRegistry, WorkspaceStore};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Serialize)]
pub struct ProjectInfo {
    /// Stable logical project identity. This is never a filesystem path.
    pub id: String,
    pub name: String,
    /// Compatibility locator retained for the single-project HTTP surface.
    pub path: String,
    pub git_root: Option<String>,
    pub session_count: usize,
}

#[derive(Serialize)]
pub struct ProjectListResponse {
    pub projects: Vec<ProjectInfo>,
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub path: String,
}

fn context_error(code: &str, message: impl Into<String>) -> AxumAppError {
    AppError::Storage(StorageError::NotFound(format!(
        "{code}: {}",
        message.into()
    )))
    .into()
}

pub(crate) async fn context_for_locator(
    pool: &sqlx::SqlitePool,
    locator: &Path,
) -> Result<ProjectContext, AxumAppError> {
    let canonical = locator.canonicalize().map_err(AppError::Io)?;
    let rows = sqlx::query(
        "SELECT wpb.project_id, wpb.workspace_id FROM workspace_project_binding wpb INNER JOIN workspace w ON w.id = wpb.workspace_id WHERE w.canonical_root = ? AND wpb.status = 'resolved' LIMIT 2",
    )
    .bind(canonical.to_string_lossy().as_ref())
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Storage(StorageError::Database(e.to_string())))?;

    let (project_id, workspace_id) = match rows.as_slice() {
        [] => {
            return Err(context_error(
                "project_context_required",
                "no canonical project is registered for this workspace",
            ))
        }
        [row] => {
            let raw: String = sqlx::Row::try_get(row, "project_id")
                .map_err(|e| AppError::Storage(StorageError::Database(e.to_string())))?;
            let project_id = ProjectId::parse(&raw)
                .map_err(|e| context_error("invalid_project_context", e.to_string()))?;
            let workspace_raw: String = sqlx::Row::try_get(row, "workspace_id")
                .map_err(|e| AppError::Storage(StorageError::Database(e.to_string())))?;
            let workspace_id = WorkspaceId::parse(&workspace_raw)
                .map_err(|e| context_error("invalid_project_context", e.to_string()))?;
            (project_id, workspace_id)
        }
        _ => {
            return Err(context_error(
                "ambiguous_project_context",
                "the workspace locator maps to more than one project",
            ))
        }
    };

    let store: Arc<dyn WorkspaceStore> = Arc::new(SqliteWorkspaceStore::new(pool.clone()));
    ProjectContextResolver::new(
        codegg_core::project_storage::ProjectStorage::new(pool.clone()),
        ProjectCatalog::new(pool.clone()),
        store,
    )
    .resolve_raw(project_id.as_str(), workspace_id.as_str(), None)
    .await
    .map_err(|e| context_error("project_context_unavailable", e.to_string()))
}

async fn project_for_locator(
    pool: &sqlx::SqlitePool,
    locator: &Path,
) -> Result<(ProjectCatalogRecord, PathBuf), AxumAppError> {
    let context = context_for_locator(pool, locator).await?;
    let project = ProjectCatalog::new(pool.clone())
        .get_project(&context.project_id)
        .await
        .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
    Ok((project, context.workspace_root))
}

fn project_info(project: ProjectCatalogRecord, path: PathBuf, session_count: usize) -> ProjectInfo {
    let git_root =
        crate::worktree::find_git_root(&path).map(|root| root.to_string_lossy().into_owned());
    ProjectInfo {
        id: project.project_id.to_string(),
        name: project.display_name,
        path: path.to_string_lossy().into_owned(),
        git_root,
        session_count,
    }
}

pub async fn get_project(
    State(state): State<ServerState>,
) -> Result<Json<ProjectInfo>, AxumAppError> {
    let (project, path) = project_for_locator(&state.pool, Path::new(&state.project_dir)).await?;
    let session_count = ProjectCatalog::new(state.pool.clone())
        .list_sessions_for_project(&project.project_id)
        .await
        .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
    Ok(Json(project_info(project, path, session_count)))
}

pub async fn list_projects(
    State(state): State<ServerState>,
) -> Result<Json<ProjectListResponse>, AxumAppError> {
    let catalog = ProjectCatalog::new(state.pool.clone());
    let projects = catalog
        .list_projects(false)
        .await
        .map_err(|e| context_error("project_catalog_unavailable", e.to_string()))?;
    let mut result = Vec::with_capacity(projects.len());
    for project in projects {
        let workspaces = catalog
            .list_workspaces_for_project(&project.project_id)
            .await
            .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
        let path = workspaces
            .first()
            .map(|workspace| workspace.canonical_root.clone())
            .unwrap_or_default();
        let session_count = catalog
            .list_sessions_for_project(&project.project_id)
            .await
            .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
        result.push(project_info(project, path, session_count));
    }
    Ok(Json(ProjectListResponse { projects: result }))
}

pub async fn create_project(
    State(state): State<ServerState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectInfo>), AxumAppError> {
    let project_root = Path::new(&state.project_dir)
        .canonicalize()
        .map_err(AppError::Io)?;
    let requested = Path::new(&req.path);
    let full = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        project_root.join(requested)
    };
    if !full.exists() {
        tokio::fs::create_dir_all(&full)
            .await
            .map_err(AppError::Io)?;
    }
    let canonical = full.canonicalize().map_err(AppError::Io)?;
    if !canonical.starts_with(&project_root) {
        return Err(context_error(
            "path_not_allowed",
            "project locator is outside the server workspace",
        ));
    }

    let workspace = if let Some(daemon) = &state.daemon {
        daemon
            .workspaces
            .get_or_register(&canonical)
            .await
            .map_err(|e| context_error("workspace_registration_failed", e.to_string()))?
    } else {
        let registry = WorkspaceRegistry::new_for_tests(Arc::new(SqliteWorkspaceStore::new(
            state.pool.clone(),
        )));
        registry
            .get_or_register(&canonical)
            .await
            .map_err(|e| context_error("workspace_registration_failed", e.to_string()))?
    };

    let catalog = ProjectCatalog::new(state.pool.clone());
    let project = catalog
        .register_local_project(
            RegisterLocalProject {
                display_name: req.name,
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &workspace.id,
            "server",
        )
        .await
        .map_err(|e| context_error("project_registration_failed", e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(project_info(project, canonical, 0)),
    ))
}
