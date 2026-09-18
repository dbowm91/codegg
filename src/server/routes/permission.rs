use axum::{
    extract::{Extension, Path, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use super::super::authz;
use super::super::state::ServerState;
use crate::error::{AppError, AxumAppError, ToolError};
use crate::server::perm_ids::parse_scoped_pending_id;
use codegg_core::transport_auth::AuthenticatedPrincipal;

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
    /// Explicit pending-item id. Required when the path carries the owning
    /// `session_id` (the canonical `/api/permission/{session_id}/submit`
    /// shape). Legacy callers that place the simple perm id in the path
    /// may omit it; the path is then treated as the perm id.
    #[serde(default)]
    pub perm_id: Option<String>,
}

/// Serializable view of a pending permission for remote clients.
/// Wire shape: `{ perm_id, session_id, turn_id, age_ms }`.
///
/// The pending item is resolved to its canonical owning session before
/// any exposure or mutation. Listing requires `session.read` on the
/// owning session; responding requires mutation authority
/// (`session.create`) via [`authz::authorize_control_response`] so M004
/// can narrow to the controller lease without another bypass. Denials
/// are privacy-safe and reveal no pending IDs.
pub async fn submit_permission(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    Path(path_value): Path<String>,
    Json(req): Json<SubmitPermissionRequest>,
) -> Result<impl IntoResponse, AxumAppError> {
    // Resolve (owning session, simple perm id) from the canonical shape
    // first: path == session, body carries `perm_id` (or a single pending
    // item is unambiguous). Legacy shape (path == simple perm id, or a
    // prefixed `perm:<session>:<turn>:<id>` path) is also accepted so
    // existing clients keep working; mismatched ownership fails closed.
    let (owning_session, simple_perm_id) = if req.session_id == path_value {
        match req.perm_id.clone() {
            Some(perm_id) => {
                // A prefixed protocol id in the body is split; otherwise the
                // body value is the simple id.
                match parse_scoped_pending_id(&perm_id) {
                    Some((session_id, simple)) => {
                        if session_id != req.session_id {
                            return Err(authz::denial_not_found());
                        }
                        (session_id, simple)
                    }
                    None => (req.session_id.clone(), perm_id),
                }
            }
            None => {
                let pending =
                    crate::bus::PermissionRegistry::get_pending_for_session(&req.session_id);
                if pending.len() != 1 {
                    return Err(authz::denial_not_found());
                }
                (req.session_id.clone(), pending[0].perm_id.clone())
            }
        }
    } else {
        match parse_scoped_pending_id(&path_value) {
            Some((session_id, simple)) => {
                if session_id != req.session_id {
                    return Err(authz::denial_not_found());
                }
                (session_id, simple)
            }
            None => (req.session_id.clone(), path_value.clone()),
        }
    };
    if !crate::bus::PermissionRegistry::is_registered_scoped(&owning_session, &simple_perm_id) {
        return Err(authz::denial_not_found());
    }
    authz::authorize_control_response(
        &state.pool,
        &principal,
        &owning_session,
        "permission_respond",
    )
    .await?;
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

    let responded =
        crate::bus::PermissionRegistry::respond_scoped(&owning_session, &simple_perm_id, choice);
    if !responded {
        tracing::warn!("permission response failed for session: {}", owning_session);
        return Err(authz::denial_not_found());
    }

    Ok(Json(PermissionResponse {
        session_id: req.session_id,
        tool: req.tool,
        decision: req.decision,
        persist: req.persist,
    }))
}

pub async fn get_pending_permissions(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, AxumAppError> {
    authz::authorize_session(
        &state.pool,
        &principal,
        &session_id,
        codegg_core::authorization::Capability::SessionRead,
        "permission_list",
    )
    .await?;
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
