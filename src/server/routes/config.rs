use axum::{
    extract::{Extension, Path, State},
    Json,
};
use serde::Serialize;
use tracing::warn;

use super::super::authz;
use super::super::scope::{resolve_context, ScopeQuery};
use super::super::state::ServerState;
use crate::config::schema::Config;
use crate::error::{AppError, AxumAppError};
use crate::session::{message::MessageData, redact_for_export, MessageStore, SessionStore};
use codegg_core::transport_auth::AuthenticatedPrincipal;

fn jsonify_message(data: &MessageData) -> serde_json::Value {
    match serde_json::to_value(data) {
        Ok(v) => v,
        Err(e) => {
            warn!("failed to serialize config data: {}", e);
            serde_json::Value::Null
        }
    }
}

fn redact_api_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(mut obj) => {
            let sensitive = [
                "key",
                "secret",
                "password",
                "token",
                "client_id",
                "client_secret",
                "bearer",
                "jwt",
                "oauth",
                "credential",
                "private_key",
                "auth",
                "authorization",
                "apikey",
                "api_key",
                "access_token",
                "refresh_token",
                "session_token",
            ];
            let keys_to_redact: Vec<String> = obj
                .keys()
                .filter(|k| {
                    let lower = k.to_lowercase();
                    sensitive.iter().any(|s| lower.contains(s))
                })
                .cloned()
                .collect();

            for k in keys_to_redact {
                if let Some(serde_json::Value::String(_)) = obj.get(&k) {
                    obj.insert(k, serde_json::json!("[REDACTED]"));
                }
            }
            for (_, v) in obj.iter_mut() {
                *v = redact_api_keys(std::mem::take(v));
            }
            serde_json::Value::Object(obj)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(redact_api_keys).collect())
        }
        other => other,
    }
}

#[derive(Serialize)]
pub struct ConfigResponse {
    pub config: serde_json::Value,
}

/// Daemon-wide config compatibility surface. There is no safe project
/// scope (it exposes daemon-global configuration), so it is
/// LocalOwner-only. Team principals fail closed with a privacy-safe 404.
pub async fn get_config(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
) -> Result<Json<ConfigResponse>, AxumAppError> {
    authz::require_local_owner(&state.pool, &principal, "config_read").await?;
    let config = Config::load().map_err(|e| {
        tracing::error!("get_config failed: {e}");
        AppError::Config(e.into())
    })?;
    let value = serde_json::to_value(&config).map_err(|e| {
        tracing::error!("get_config serialize failed: {e}");
        AppError::Json(e)
    })?;
    let redacted_value = redact_api_keys(value);
    Ok(Json(ConfigResponse {
        config: redacted_value,
    }))
}

#[derive(Serialize)]
pub struct MessageListResponse {
    pub messages: Vec<serde_json::Value>,
    pub total: usize,
}

pub async fn list_messages(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    axum::extract::Query(scope): axum::extract::Query<ScopeQuery>,
    Path(id): Path<String>,
) -> Result<Json<MessageListResponse>, AxumAppError> {
    let store = SessionStore::new(state.pool.clone());
    let session = store.get(&id).await?.ok_or_else(authz::denial_not_found)?;
    resolve_context(&state.pool, &scope, Some(&session.id))
        .await
        .map_err(|_| authz::denial_not_found())?;
    authz::authorize_session(
        &state.pool,
        &principal,
        &session.id,
        codegg_core::authorization::Capability::SessionRead,
        "session_messages_load",
    )
    .await?;

    let msg_store = MessageStore::new(state.pool);
    let messages = msg_store.list(&id).await?;

    let total = messages.len();
    let messages: Vec<serde_json::Value> = messages
        .into_iter()
        .map(|m| redact_for_export(jsonify_message(&m.data)))
        .collect();

    Ok(Json(MessageListResponse { messages, total }))
}
