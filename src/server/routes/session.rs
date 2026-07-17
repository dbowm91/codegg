use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::super::state::ServerState;
use crate::error::{AppError, AxumAppError, StorageError};
use crate::session::{CreateSession, Session, SessionStore};

#[derive(Deserialize)]
pub struct RevertToMessageRequest {
    pub message_id: String,
}

#[derive(Serialize)]
pub struct SessionListResponse {
    pub sessions: Vec<Session>,
}

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub project_id: String,
    pub directory: String,
    pub title: Option<String>,
    pub parent_id: Option<String>,
    pub workspace_id: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub tags: Option<Vec<String>>,
}

pub async fn list_sessions(
    State(state): State<ServerState>,
) -> Result<Json<SessionListResponse>, AxumAppError> {
    let store = SessionStore::new(state.pool);
    let sessions = store.list(&state.project_dir, 50).await.map_err(|e| {
        tracing::error!("list_sessions failed: {e}");
        AppError::Storage(e)
    })?;
    Ok(Json(SessionListResponse { sessions }))
}

pub async fn get_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool);
    let session = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if session.project_id != state.project_dir {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    Ok(Json(session))
}

pub async fn create_session(
    State(state): State<ServerState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<Session>), AxumAppError> {
    let store = SessionStore::new(state.pool);
    let input = CreateSession {
        project_id: state.project_dir.clone(),
        directory: req.directory,
        title: req.title,
        parent_id: req.parent_id,
        workspace_id: req.workspace_id,
        agent: req.agent,
        model: req.model,
        tags: req.tags,
        provider_connection_id: None,
        provider_connection_revision: None,
        model_catalog_revision: None,
        selected_model_id: None,
    };
    let session = store.create(input).await?;
    Ok((StatusCode::CREATED, Json(session)))
}

pub async fn archive_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AxumAppError> {
    let store = SessionStore::new(state.pool);
    let session = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if session.project_id != state.project_dir {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    store.archive(&session.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn fork_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Session>), AxumAppError> {
    let store = SessionStore::new(state.pool);
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if existing.project_id != state.project_dir {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    let forked = store.fork(&id).await?;
    Ok((StatusCode::CREATED, Json(forked)))
}

pub async fn share_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool);
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if existing.project_id != state.project_dir {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    let session = store.share_session(&id).await?;
    Ok(Json(session))
}

pub async fn unshare_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool);
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if existing.project_id != state.project_dir {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    let session = store.unshare_session(&id).await?;
    Ok(Json(session))
}

pub async fn revert_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<RevertToMessageRequest>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool);
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if existing.project_id != state.project_dir {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    let session = store.revert_to_message(&id, &req.message_id).await?;
    Ok(Json(session))
}

pub async fn unrevert_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool);
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if existing.project_id != state.project_dir {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    let session = store.unrevert_session(&id).await?;
    Ok(Json(session))
}
