use axum::{extract::Path, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, StorageError, ToolError};

#[derive(Deserialize, Serialize)]
pub struct PermissionResponse {
    pub session_id: String,
    pub tool: String,
    pub decision: String,
    pub persist: bool,
}

#[derive(Deserialize)]
pub struct SubmitPermissionRequest {
    pub session_id: String,
    pub tool: String,
    pub decision: String,
    #[serde(default)]
    pub persist: bool,
}

pub async fn submit_permission(
    Path(perm_id): Path<String>,
    Json(req): Json<SubmitPermissionRequest>,
) -> Result<impl IntoResponse, AppError> {
    if req.session_id != perm_id.splitn(2, '-').next().unwrap_or(&req.session_id) {
        return Err(AppError::Storage(StorageError::NotFound(
            "session id mismatch".to_string(),
        )));
    }

    let choice = match req.decision.as_str() {
        "allow" => crate::permission::PermissionChoice::AllowOnce,
        "deny" => crate::permission::PermissionChoice::DenyOnce,
        "always_allow" => crate::permission::PermissionChoice::AlwaysAllow,
        "always_deny" => crate::permission::PermissionChoice::AlwaysDeny,
        _ => {
            return Err(AppError::Tool(ToolError::Execution(
                "invalid decision, must be 'allow', 'deny', 'always_allow', or 'always_deny'".to_string(),
            )));
        }
    };

    if !crate::bus::PermissionRegistry::respond(perm_id.clone(), choice) {
        tracing::warn!("permission response failed for perm_id: {}", perm_id);
    }

    Ok(Json(PermissionResponse {
        session_id: req.session_id,
        tool: req.tool,
        decision: req.decision,
        persist: req.persist,
    }))
}

pub async fn get_pending_permissions(
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(get_pending_permissions_for_session(&session_id)))
}

/// Helper function that returns pending permissions for a session.
/// This can be called directly in tests without Axum extractors.
pub fn get_pending_permissions_for_session(session_id: &str) -> serde_json::Value {
    let pending_ids = crate::bus::PermissionRegistry::pending_permission_ids();

    // Return all pending permission IDs
    // Note: PermissionRegistry keys are in format "{tool_call_id}-{tool_name}"
    // To filter by session, we would need to extend the registry to store session_id
    let permissions: Vec<serde_json::Value> = pending_ids
        .iter()
        .map(|id| {
            // Parse the perm_id to extract tool name if possible
            // Format is typically "{tool_call_id}-{tool_name}"
            let parts: Vec<&str> = id.splitn(2, '-').collect();
            let tool_name = parts.get(1).unwrap_or(&"unknown");

            serde_json::json!({
                "perm_id": id,
                "tool": tool_name,
                "session_id": session_id,
            })
        })
        .collect();

    serde_json::json!({
        "permissions": permissions
    })
}
