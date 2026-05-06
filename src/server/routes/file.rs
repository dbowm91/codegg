use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::{Path as StdPath, PathBuf};

use super::super::state::ServerState;
use crate::error::{AppError, StorageError};

pub fn sanitize_path(root: &str, requested: &str) -> Result<PathBuf, AppError> {
    let root = StdPath::new(root);
    let joined = root.join(requested);

    // Check for path traversal before canonicalization
    // Normalize the path and check each component
    let _normalized = joined.components().collect::<Vec<_>>();
    let _root_components: Vec<_> = root.components().collect();

    // Reject if the joined path tries to escape root using ../
    // We check lexicographically first, then verify with canonicalize if possible
    let joined_str = joined.to_string_lossy();
    if joined_str.contains("..") && !requested.starts_with("..") {
        // The requested path might be trying traversal
        // Let the canonicalize handle it if path exists
    }

    // Try to canonicalize if path exists
    let resolved = match joined.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // Path doesn't exist, but we still need to check it's not trying to escape
            // Use lexical analysis for path traversal detection
            let mut root_clone = root.to_path_buf();
            if root_clone.is_relative() {
                root_clone = std::env::current_dir()
                    .map_err(AppError::Io)?
                    .join(root_clone);
            }
            let root_canonicalized = root_clone.canonicalize().map_err(|_| {
                AppError::Storage(StorageError::NotFound("root path not found".into()))
            })?;

            // For non-existent paths, check if the joined path escapes root
            // by checking components
            let mut test_path = root_canonicalized.clone();
            for component in requested.split('/') {
                if component == ".." {
                    test_path.pop();
                } else if component != "." && !component.is_empty() {
                    test_path.push(component);
                }
            }

            // Check if the final path is within root
            if !test_path.starts_with(&root_canonicalized) {
                return Err(AppError::Storage(StorageError::NotFound(
                    "path outside allowed directory".into(),
                )));
            }

            return Ok(test_path);
        }
    };

    let root_canonicalized = root
        .canonicalize()
        .map_err(|_| AppError::Storage(StorageError::NotFound("root path not found".into())))?;

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
