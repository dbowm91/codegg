use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::{Path as StdPath, PathBuf};

use super::super::state::ServerState;
use crate::error::{AppError, StorageError};
use crate::tool::util::check_path_for_symlinks;

pub fn sanitize_path(root: &str, requested: &str) -> Result<PathBuf, AppError> {
    let root = StdPath::new(root);
    let joined = root.join(requested);

    let mut root_clone = root.to_path_buf();
    if root_clone.is_relative() {
        root_clone = std::env::current_dir()
            .map_err(AppError::Io)?
            .join(root_clone);
    }
    let root_canonicalized = root_clone
        .canonicalize()
        .map_err(|_| AppError::Storage(StorageError::NotFound("root path not found".into())))?;

    check_path_for_symlinks(&joined).map_err(|e| {
        AppError::Storage(StorageError::NotFound(format!(
            "path validation failed: {}",
            e
        )))
    })?;

    let resolved = match joined.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let mut test_path = root_canonicalized.clone();
            for component in requested.split('/') {
                if component == ".." {
                    test_path.pop();
                } else if component != "." && !component.is_empty() {
                    test_path.push(component);
                }
            }

            if !test_path.starts_with(&root_canonicalized) {
                return Err(AppError::Storage(StorageError::NotFound(
                    "path outside allowed directory".into(),
                )));
            }

            return Ok(test_path);
        }
    };

    if resolved.starts_with(&root_canonicalized) {
        Ok(resolved)
    } else {
        Err(AppError::Storage(StorageError::NotFound(
            "path outside allowed directory".into(),
        )))
    }
}

#[derive(Deserialize)]
pub struct ReadFileQuery {
    pub path: String,
}

#[derive(Serialize)]
pub struct FileReadResponse {
    pub path: String,
    pub content: String,
    pub size: u64,
}

#[derive(Serialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_file: bool,
}

#[derive(Serialize)]
pub struct FileListResponse {
    pub entries: Vec<FileInfo>,
}

#[derive(Deserialize)]
pub struct WriteFileRequest {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct DeleteFileRequest {
    pub path: String,
}

pub async fn read_file(
    State(state): State<ServerState>,
    Query(query): Query<ReadFileQuery>,
) -> Result<Json<FileReadResponse>, AppError> {
    let full = sanitize_path(&state.project_dir, &query.path)?;
    let content = tokio::fs::read_to_string(&full)
        .await
        .map_err(AppError::Io)?;
    let meta = tokio::fs::metadata(&full).await.map_err(AppError::Io)?;
    Ok(Json(FileReadResponse {
        path: query.path,
        content,
        size: meta.len(),
    }))
}

pub async fn list_files(
    State(state): State<ServerState>,
    Query(query): Query<ReadFileQuery>,
) -> Result<Json<FileListResponse>, AppError> {
    let dir = sanitize_path(&state.project_dir, &query.path)?;
    let mut entries = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir).await.map_err(AppError::Io)?;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata().await.map_err(AppError::Io)?;
        entries.push(FileInfo {
            path: path.to_string_lossy().to_string(),
            name,
            size: meta.len(),
            is_dir: meta.is_dir(),
            is_file: meta.is_file(),
        });
    }
    Ok(Json(FileListResponse { entries }))
}

pub async fn write_file(
    State(state): State<ServerState>,
    Json(req): Json<WriteFileRequest>,
) -> Result<Json<FileInfo>, AppError> {
    let full = sanitize_path(&state.project_dir, &req.path)?;
    if let Some(parent) = full.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(AppError::Io)?;
    }
    tokio::fs::write(&full, &req.content)
        .await
        .map_err(AppError::Io)?;
    let meta = tokio::fs::metadata(&full).await.map_err(AppError::Io)?;
    Ok(Json(FileInfo {
        path: req.path,
        name: full
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        size: meta.len(),
        is_dir: false,
        is_file: true,
    }))
}

pub async fn delete_file(
    State(state): State<ServerState>,
    Json(req): Json<DeleteFileRequest>,
) -> Result<StatusCode, AppError> {
    let full = sanitize_path(&state.project_dir, &req.path)?;
    tokio::fs::remove_file(&full).await.map_err(AppError::Io)?;
    Ok(StatusCode::NO_CONTENT)
}
