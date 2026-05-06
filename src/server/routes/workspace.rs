use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::super::state::ServerState;
use super::file::sanitize_path;
use crate::error::AppError;

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
}

pub async fn get_workspace(
    State(state): State<ServerState>,
) -> Result<Json<WorkspaceInfo>, AppError> {
    let name = std::path::Path::new(&state.project_dir)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".to_string());

    let is_wt = is_git_worktree(&state.project_dir).await;

    Ok(Json(WorkspaceInfo {
        id: state.project_dir.clone(),
        name,
        path: state.project_dir.clone(),
        is_worktree: is_wt,
    }))
}

pub async fn list_workspaces(
    State(state): State<ServerState>,
) -> Result<Json<WorkspaceListResponse>, AppError> {
    let current = WorkspaceInfo {
        id: state.project_dir.clone(),
        name: std::path::Path::new(&state.project_dir)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string()),
        path: state.project_dir.clone(),
        is_worktree: is_git_worktree(&state.project_dir).await,
    };

    let mut workspaces = vec![current];

    if let Ok(entries) = std::fs::read_dir(&state.project_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && is_git_worktree(path.to_str().unwrap_or("")).await {
                workspaces.push(WorkspaceInfo {
                    id: path.to_string_lossy().to_string(),
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    path: path.to_string_lossy().to_string(),
                    is_worktree: true,
                });
            }
        }
    }

    Ok(Json(WorkspaceListResponse { workspaces }))
}

pub async fn create_workspace(
    State(state): State<ServerState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<WorkspaceInfo>), AppError> {
    let validated = sanitize_path(&state.project_dir, &req.path)?;

    if !validated.exists() {
        tokio::fs::create_dir_all(&validated)
            .await
            .map_err(AppError::Io)?;
    }

    let is_wt = is_git_worktree(&req.path).await;

    Ok((
        StatusCode::CREATED,
        Json(WorkspaceInfo {
            id: req.path.clone(),
            name: req.name,
            path: req.path,
            is_worktree: is_wt,
        }),
    ))
}

async fn is_git_worktree(dir: &str) -> bool {
    let git = PathBuf::from(dir).join(".git");
    if !git.exists() {
        return false;
    }
    if let Ok(content) = tokio::fs::read_to_string(&git).await {
        content.starts_with("gitdir:")
    } else {
        false
    }
}
