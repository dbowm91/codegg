use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;
use tracing::warn;

use super::super::state::ServerState;
use crate::config::schema::Config;
use crate::error::AppError;
use crate::session::{message::MessageData, redact_for_export, MessageStore, SessionStore};

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

pub async fn get_config(
    State(_state): State<ServerState>,
) -> Result<Json<ConfigResponse>, AppError> {
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
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<MessageListResponse>, AppError> {
    let store = SessionStore::new(state.pool.clone());
    let _session = store.get(&id).await?.ok_or_else(|| {
        AppError::Storage(crate::error::StorageError::NotFound(
            "session not found".to_string(),
        ))
    })?;

    let msg_store = MessageStore::new(state.pool);
    let messages = msg_store.list(&id).await?;

    let total = messages.len();
    let messages: Vec<serde_json::Value> = messages
        .into_iter()
        .map(|m| redact_for_export(jsonify_message(&m.data)))
        .collect();

    Ok(Json(MessageListResponse { messages, total }))
}
