use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::super::scope::{context_error, resolve_context, ScopeQuery};
use super::super::state::ServerState;
use crate::error::{AppError, AxumAppError, StorageError};
use crate::session::{CreateSession, Session, SessionStore};
use codegg_core::project_storage::ProjectStorage;

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
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub directory: String,
    pub title: Option<String>,
    pub parent_id: Option<String>,
    pub workspace_id: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub tags: Option<Vec<String>>,
}

async fn create_context(
    state: &ServerState,
    req: &CreateSessionRequest,
) -> Result<codegg_core::context::ProjectContext, AxumAppError> {
    match (req.project_id.as_deref(), req.workspace_id.as_deref()) {
        (Some(project_id), Some(workspace_id)) => {
            let scope = ScopeQuery {
                project_id: Some(project_id.to_string()),
                workspace_id: Some(workspace_id.to_string()),
                directory: None,
            };
            resolve_context(&state.pool, &scope, None).await
        }
        (None, None) => {
            if req.directory.is_empty() {
                return Err(context_error(
                    "project_context_required",
                    "provide project_id and workspace_id, or a unique directory locator",
                ));
            }
            let scope = ScopeQuery {
                directory: Some(req.directory.clone()),
                ..ScopeQuery::default()
            };
            resolve_context(&state.pool, &scope, None).await
        }
        _ => Err(context_error(
            "project_context_required",
            "project_id and workspace_id must be provided together",
        )),
    }
}

pub async fn list_sessions(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
) -> Result<Json<SessionListResponse>, AxumAppError> {
    let context = resolve_context(&state.pool, &scope, None).await?;
    let store = SessionStore::new(state.pool.clone());
    let sessions = store
        .list_by_canonical_project(context.project_id.as_str(), Some(50))
        .await
        .map_err(|e| {
            tracing::error!("list_sessions failed: {e}");
            AppError::Storage(e)
        })?;
    Ok(Json(SessionListResponse { sessions }))
}

pub async fn get_session(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let session = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    resolve_context(&state.pool, &scope, Some(&id)).await?;
    Ok(Json(session))
}

pub async fn create_session(
    State(state): State<ServerState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<Session>), AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let context = create_context(&state, &req).await?;
    let directory = context.workspace_root.to_string_lossy().into_owned();
    let input = CreateSession {
        project_id: context.project_id.as_str().to_string(),
        directory,
        title: req.title,
        parent_id: req.parent_id,
        workspace_id: Some(context.workspace_id.as_str().to_string()),
        agent: req.agent,
        model: req.model,
        tags: req.tags,
        provider_connection_id: None,
        provider_connection_revision: None,
        model_catalog_revision: None,
        selected_model_id: None,
    };
    let session = store
        .create_with_binding(
            input,
            &context.project_id,
            &context.workspace_id,
            "server_session_create",
        )
        .await
        .map_err(|e| context_error("session_binding_failed", e.to_string()))?;
    Ok((StatusCode::CREATED, Json(session)))
}

pub async fn archive_session(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
    Path(id): Path<String>,
) -> Result<StatusCode, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let session = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    resolve_context(&state.pool, &scope, Some(&session.id)).await?;
    store.archive(&session.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn fork_session(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Session>), AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    let context = resolve_context(&state.pool, &scope, Some(&existing.id)).await?;
    let forked = store.fork(&id).await?;
    if let Err(error) = ProjectStorage::new(state.pool.clone())
        .bind_session(
            &forked.id,
            &context.project_id,
            &context.workspace_id,
            "server_session_fork",
        )
        .await
    {
        let _ = store.delete(&forked.id).await;
        return Err(context_error("session_binding_failed", error.to_string()));
    }
    Ok((StatusCode::CREATED, Json(forked)))
}

pub async fn share_session(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    resolve_context(&state.pool, &scope, Some(&existing.id)).await?;
    let session = store.share_session(&id).await?;
    Ok(Json(session))
}

pub async fn unshare_session(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    resolve_context(&state.pool, &scope, Some(&existing.id)).await?;
    let session = store.unshare_session(&id).await?;
    Ok(Json(session))
}

pub async fn revert_session(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
    Path(id): Path<String>,
    Json(req): Json<RevertToMessageRequest>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    resolve_context(&state.pool, &scope, Some(&existing.id)).await?;
    let session = store.revert_to_message(&id, &req.message_id).await?;
    Ok(Json(session))
}

pub async fn unrevert_session(
    State(state): State<ServerState>,
    Query(scope): Query<ScopeQuery>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    resolve_context(&state.pool, &scope, Some(&existing.id)).await?;
    let session = store.unrevert_session(&id).await?;
    Ok(Json(session))
}
