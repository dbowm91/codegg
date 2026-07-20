use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::super::scope::{context_error, require_explicit_project, resolve_context, ScopeQuery};
use super::super::state::ServerState;
use super::file::sanitize_path_from_root;
use crate::error::{AppError, AxumAppError};
use codegg_core::project_catalog::ProjectCatalog;

#[derive(Serialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_worktree: bool,
}

#[derive(Serialize)]
pub struct WorkspaceListResponse {
    pub workspaces: Vec<WorkspaceInfo>,
}

#[derive(Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub path: String,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
}

pub async fn get_workspace(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
) -> Result<Json<WorkspaceInfo>, AxumAppError> {
    let context = resolve_context(&state.pool, &scope, None).await?;
    let name = context.workspace_display_name.clone();
    let path = context.workspace_root.to_string_lossy().into_owned();
    let is_wt = crate::worktree::is_git_file(&context.workspace_root.join(".git"));

    Ok(Json(WorkspaceInfo {
        id: context.workspace_id.to_string(),
        name,
        path,
        is_worktree: is_wt,
    }))
}

pub async fn list_workspaces(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
) -> Result<Json<WorkspaceListResponse>, AxumAppError> {
    let project_id = if scope.project_id.is_some() {
        require_explicit_project(&scope)?.to_string()
    } else {
        resolve_context(&state.pool, &scope, None)
            .await?
            .project_id
            .to_string()
    };
    let summaries = ProjectCatalog::new(state.pool.clone())
        .list_workspaces_for_project(
            &codegg_core::identity::ProjectId::parse(&project_id)
                .map_err(|e| context_error("invalid_project_context", e.to_string()))?,
        )
        .await
        .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
    let workspaces = summaries
        .into_iter()
        .map(|workspace| WorkspaceInfo {
            id: workspace.workspace_id.to_string(),
            name: workspace.display_name,
            is_worktree: crate::worktree::is_git_file(&workspace.canonical_root.join(".git")),
            path: workspace.canonical_root.to_string_lossy().into_owned(),
        })
        .collect();
    Ok(Json(WorkspaceListResponse { workspaces }))
}

pub async fn create_workspace(
    State(state): State<ServerState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<WorkspaceInfo>), AxumAppError> {
    let scope = ScopeQuery {
        project_id: req.project_id.clone(),
        workspace_id: req.workspace_id.clone(),
        directory: None,
    };
    let context = resolve_context(&state.pool, &scope, None).await?;
    let validated = sanitize_path_from_root(&context.workspace_root, &req.path)?;

    if !validated.exists() {
        tokio::fs::create_dir_all(&validated)
            .await
            .map_err(AppError::Io)?;
    }

    let is_wt = crate::worktree::is_git_worktree(&validated);

    Ok((
        StatusCode::CREATED,
        Json(WorkspaceInfo {
            id: validated.to_string_lossy().into_owned(),
            name: req.name,
            path: req.path,
            is_worktree: is_wt,
        }),
    ))
}
