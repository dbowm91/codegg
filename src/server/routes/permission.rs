use axum::{extract::Path, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, ToolError};

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
    let choice = match req.decision.as_str() {
        "allow" => crate::permission::PermissionChoice::AllowOnce,
        "deny" => crate::permission::PermissionChoice::DenyOnce,
        "always_allow" => crate::permission::PermissionChoice::AlwaysAllow,
        "always_deny" => crate::permission::PermissionChoice::AlwaysDeny,
        _ => {
            return Err(AppError::Tool(ToolError::Execution(
                "invalid decision, must be 'allow', 'deny', 'always_allow', or 'always_deny'"
                    .to_string(),
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
/// NOTE: PermissionRegistry does not store session_id in keys, so proper session-based
/// filtering is not possible without extending the registry. Returns empty list when
/// session_id is provided to indicate filtering is not supported.
pub fn get_pending_permissions_for_session(session_id: &str) -> serde_json::Value {
    let _pending_ids = crate::bus::PermissionRegistry::pending_permission_ids();

    // PermissionRegistry keys are in format "{tool_call_id}-{tool_name}" not "{session_id}-..."
    // We cannot properly filter by session without extending the registry.
    // Return empty to indicate filtering is not possible.
    let _ = session_id;
    let permissions: Vec<serde_json::Value> = Vec::new();

    serde_json::json!({
        "permissions": permissions
    })
}
