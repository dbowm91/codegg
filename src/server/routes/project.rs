use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use super::super::state::ServerState;
use crate::error::{AppError, StorageError};

#[derive(Serialize)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
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

pub async fn get_project(State(state): State<ServerState>) -> Result<Json<ProjectInfo>, AppError> {
    let store = crate::session::SessionStore::new(state.pool);
    let session_count = store
        .list(&state.project_dir, 1)
        .await
        .map(|sessions| sessions.len())
        .unwrap_or(0);

    let git_root = crate::worktree::find_git_root(std::path::Path::new(&state.project_dir))
        .map(|p| p.to_string_lossy().to_string());
    let name = std::path::Path::new(&state.project_dir)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(Json(ProjectInfo {
        id: state.project_dir.clone(),
        name,
        path: state.project_dir.clone(),
        git_root,
        session_count,
    }))
}

pub async fn list_projects(
    State(state): State<ServerState>,
) -> Result<Json<ProjectListResponse>, AppError> {
    let store = crate::session::SessionStore::new(state.pool);
    let all = store.list(&state.project_dir, 1000).await?;

    let mut map = std::collections::HashMap::new();
    for s in &all {
        map.entry(s.project_id.clone())
            .or_insert_with(Vec::new)
            .push(s);
    }

    let projects: Vec<ProjectInfo> = map
        .iter()
        .map(|(id, sessions)| {
            let name = std::path::Path::new(id)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            ProjectInfo {
                id: id.clone(),
                name,
                path: id.clone(),
                git_root: None,
                session_count: sessions.len(),
            }
        })
        .collect();

    Ok(Json(ProjectListResponse { projects }))
}

pub async fn create_project(
    State(state): State<ServerState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectInfo>), AppError> {
    let requested_path = std::path::Path::new(&req.path);
    let project_root = std::path::Path::new(&state.project_dir);

    let canonical_requested = requested_path.canonicalize().map_err(AppError::Io)?;
    let canonical_root = project_root.canonicalize().map_err(AppError::Io)?;

    if !canonical_requested.starts_with(&canonical_root) {
        return Err(AppError::Storage(StorageError::NotFound(
            "path not allowed".into(),
        )));
    }

    let full = std::path::Path::new(&req.path);
    if !full.exists() {
        tokio::fs::create_dir_all(full)
            .await
            .map_err(AppError::Io)?;
    }

    let git_root = crate::worktree::find_git_root(&full)
        .map(|p| p.to_string_lossy().to_string());
    let abs = full
        .canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(req.path.clone());

    let info = ProjectInfo {
        id: abs.clone(),
        name: req.name,
        path: abs,
        git_root,
        session_count: 0,
    };

    Ok((StatusCode::CREATED, Json(info)))
}


