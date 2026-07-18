use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::super::state::ServerState;
use crate::error::{AppError, AxumAppError, StorageError};
use crate::session::{CreateSession, Session, SessionStore};
use codegg_core::context::{ProjectContextRequest, ProjectContextResolver};
use codegg_core::project_catalog::ProjectCatalog;
use codegg_core::project_storage::{BindingStatus, ProjectStorage};
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceStore};
use std::sync::Arc;

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
    pub directory: String,
    pub title: Option<String>,
    pub parent_id: Option<String>,
    pub workspace_id: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub tags: Option<Vec<String>>,
}

fn context_error(code: &str, message: impl Into<String>) -> AxumAppError {
    AppError::Storage(StorageError::NotFound(format!(
        "{code}: {}",
        message.into()
    )))
    .into()
}

async fn session_belongs_to_default_context(
    state: &ServerState,
    session: &Session,
) -> Result<bool, AxumAppError> {
    let context = match super::project::context_for_locator(
        &state.pool,
        std::path::Path::new(&state.project_dir),
    )
    .await
    {
        Ok(context) => context,
        Err(_) => return Ok(false),
    };
    let binding = ProjectStorage::new(state.pool.clone())
        .session_binding(&session.id)
        .await
        .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
    Ok(binding.as_ref().is_some_and(|binding| {
        binding.status == BindingStatus::Resolved
            && binding.project_id.as_ref() == Some(&context.project_id)
            && binding.workspace_id.as_ref() == Some(&context.workspace_id)
    }))
}

async fn create_context(
    state: &ServerState,
    req: &CreateSessionRequest,
) -> Result<codegg_core::context::ProjectContext, AxumAppError> {
    match (req.project_id.as_deref(), req.workspace_id.as_deref()) {
        (Some(project_id), Some(workspace_id)) => {
            let store: Arc<dyn WorkspaceStore> =
                Arc::new(SqliteWorkspaceStore::new(state.pool.clone()));
            ProjectContextResolver::new(
                ProjectStorage::new(state.pool.clone()),
                ProjectCatalog::new(state.pool.clone()),
                store,
            )
            .resolve(
                ProjectContextRequest::from_raw(project_id, workspace_id, None)
                    .map_err(|e| context_error("invalid_project_context", e.to_string()))?,
            )
            .await
            .map_err(|e| context_error("project_context_required", e.to_string()))
        }
        (None, None) => {
            super::project::context_for_locator(&state.pool, std::path::Path::new(&req.directory))
                .await
        }
        _ => Err(context_error(
            "project_context_required",
            "project_id and workspace_id must be provided together",
        )),
    }
}

pub async fn list_sessions(
    State(state): State<ServerState>,
) -> Result<Json<SessionListResponse>, AxumAppError> {
    let pool = state.pool.clone();
    let store = SessionStore::new(pool.clone());
    let context =
        super::project::context_for_locator(&pool, std::path::Path::new(&state.project_dir))
            .await?;
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
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let session = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if !session_belongs_to_default_context(&state, &session).await? {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
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
    Path(id): Path<String>,
) -> Result<StatusCode, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let session = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if !session_belongs_to_default_context(&state, &session).await? {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    store.archive(&session.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn fork_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Session>), AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if !session_belongs_to_default_context(&state, &existing).await? {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    let context =
        super::project::context_for_locator(&state.pool, std::path::Path::new(&state.project_dir))
            .await?;
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
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if !session_belongs_to_default_context(&state, &existing).await? {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    let session = store.share_session(&id).await?;
    Ok(Json(session))
}

pub async fn unshare_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if !session_belongs_to_default_context(&state, &existing).await? {
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
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if !session_belongs_to_default_context(&state, &existing).await? {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    let session = store.revert_to_message(&id, &req.message_id).await?;
    Ok(Json(session))
}

pub async fn unrevert_session(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<Session>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound("session not found".into())))?;
    if !session_belongs_to_default_context(&state, &existing).await? {
        return Err(AppError::Storage(StorageError::NotFound("session not found".into())).into());
    }
    let session = store.unrevert_session(&id).await?;
    Ok(Json(session))
}
