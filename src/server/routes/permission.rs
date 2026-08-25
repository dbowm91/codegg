use axum::{extract::Path, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AxumAppError, ToolError};
use crate::server::perm_ids::parse_scoped_pending_id;

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

/// Serializable view of a pending permission for remote clients.
/// Wire shape: `{ perm_id, session_id, turn_id, age_ms }`.
pub async fn submit_permission(
    Path(perm_id): Path<String>,
    Json(req): Json<SubmitPermissionRequest>,
) -> Result<impl IntoResponse, AxumAppError> {
    let choice = match req.decision.as_str() {
        "allow" => crate::bus::PermissionDecision::AllowOnce,
        "deny" => crate::bus::PermissionDecision::DenyOnce,
        "always_allow" => crate::bus::PermissionDecision::AlwaysAllow,
        "always_deny" => crate::bus::PermissionDecision::AlwaysDeny,
        _ => {
            return Err(AppError::Tool(ToolError::Execution(
                "invalid decision, must be 'allow', 'deny', 'always_allow', or 'always_deny'"
                    .to_string(),
            ))
            .into());
        }
    };

    // The registry is session-scoped: resolve (session, simple perm id)
    // from a prefixed protocol ID when present, otherwise trust the
    // body's session_id. Unscoped legacy `respond` would silently fail
    // against any real registration (which always carries a session).
    let responded = match parse_scoped_pending_id(&perm_id) {
        Some((session_id, simple_perm_id)) => {
            crate::bus::PermissionRegistry::respond_scoped(&session_id, &simple_perm_id, choice)
        }
        None => crate::bus::PermissionRegistry::respond_scoped(&req.session_id, &perm_id, choice),
    };
    if !responded {
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
) -> Result<Json<serde_json::Value>, AxumAppError> {
    Ok(Json(get_pending_permissions_for_session(&session_id)))
}

/// Helper function that returns pending permissions owned by
/// `session_id`. This can be called directly in tests without Axum
/// extractors.
pub fn get_pending_permissions_for_session(session_id: &str) -> serde_json::Value {
    let permissions: Vec<serde_json::Value> =
        crate::bus::PermissionRegistry::get_pending_for_session(session_id)
            .into_iter()
            .map(|p| {
                serde_json::json!({
                    "perm_id": p.perm_id,
                    "session_id": p.session_id,
                    "turn_id": p.turn_id,
                    "age_ms": p.created_at.elapsed().as_millis() as u64,
                })
            })
            .collect();

    serde_json::json!({
        "permissions": permissions
    })
}
