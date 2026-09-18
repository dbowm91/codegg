//! Server-side authorization adapter (team-collaboration M001).
//!
//! Every authenticated HTTP compatibility route converges here before
//! touching stores, the filesystem, or the global event bus. The adapter
//! accepts the transport-bound [`AuthenticatedPrincipal`] (inserted by
//! `auth_middleware`, never from a body/query field) plus canonical
//! operation/scope information and delegates to the existing
//! [`AuthorizationService`] / canonical scope resolvers. No handler
//! hand-rolls role expansion.
//!
//! Disposition model (see [`route_disposition_table`]):
//!
//! - `CoreAdapter` — the route is a thin client of daemon/Core authority
//!   (`/core`, `/tui`); the daemon gate remains authoritative.
//! - `SharedAuthz` — the compatibility handler calls this adapter with the
//!   same capability as its Core equivalent.
//! - `LocalOwnerOnly` — no safe project scope exists; team principals fail
//!   closed with a privacy-safe 404. LocalOwner broad policy still passes
//!   through [`AuthorizationService`].
//! - `TriggerCapability` — the narrow `cggtr_...` fire endpoint, deliberately
//!   outside principal auth (see `routes/task_trigger.rs`).
//!
//! Team denials use the privacy-safe `project_not_found` shape
//! ([`denial_as_not_found`]) so unauthorized callers cannot distinguish
//! denial from absence. Authorization always precedes side effects;
//! revocation applies on the next request because every request
//! re-evaluates against current `TeamStore` state.

use codegg_core::authorization::{
    denial_as_not_found, is_local_owner_broad, visible_projects, AuthorizationRequest,
    AuthorizationService, Capability, OperationDescriptor, ScopeKind,
};
use codegg_core::identity::ProjectId;
use codegg_core::team::TeamStore;
use codegg_core::transport_auth::AuthenticatedPrincipal;

use super::scope::context_error;
use crate::error::AxumAppError;

/// How one authenticated route reaches a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDisposition {
    /// Authoritative Core/daemon path; the daemon gate decides.
    CoreAdapter,
    /// Compatibility handler authorizes here with the Core-equivalent
    /// capability.
    SharedAuthz,
    /// No safe project scope; team principals fail closed.
    LocalOwnerOnly,
    /// Narrow non-principal capability (task-trigger fire only).
    TriggerCapability,
}

impl RouteDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CoreAdapter => "core_adapter",
            Self::SharedAuthz => "shared_authz",
            Self::LocalOwnerOnly => "local_owner_only",
            Self::TriggerCapability => "trigger_capability",
        }
    }
}

/// One row of the executable route-disposition matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteDispositionEntry {
    pub method: &'static str,
    pub path: &'static str,
    pub disposition: RouteDisposition,
    /// Canonical operation name (Core equivalent or adapter operation).
    pub operation: &'static str,
    /// Required capability wire name, or `"none"`.
    pub capability: &'static str,
}

/// Exhaustive disposition for every route mounted inside the authenticated
/// API router plus the narrow trigger exception.
///
/// The static guard `scripts/check_http_route_disposition.py` asserts that
/// every `.route(...)` in `src/server/http.rs` (and the trigger router)
/// appears here and vice versa, so new routes cannot be mounted without
/// an explicit disposition.
pub fn route_disposition_table() -> Vec<RouteDispositionEntry> {
    use RouteDisposition as D;
    vec![
        // Sessions — same capabilities as the Core equivalents.
        RouteDispositionEntry {
            method: "GET",
            path: "/api/sessions",
            disposition: D::SharedAuthz,
            operation: "session_list",
            capability: "session.read",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/sessions",
            disposition: D::SharedAuthz,
            operation: "session_create",
            capability: "session.create",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/sessions/{id}",
            disposition: D::SharedAuthz,
            operation: "session_load",
            capability: "session.read",
        },
        RouteDispositionEntry {
            method: "DELETE",
            path: "/api/sessions/{id}/archive",
            disposition: D::SharedAuthz,
            operation: "session_archive",
            capability: "session.create",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/sessions/{id}/fork",
            disposition: D::SharedAuthz,
            operation: "session_fork",
            capability: "session.read",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/sessions/{id}/share",
            disposition: D::SharedAuthz,
            operation: "session_share",
            capability: "project.configure",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/sessions/{id}/unshare",
            disposition: D::SharedAuthz,
            operation: "session_unshare",
            capability: "project.configure",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/sessions/{id}/revert",
            disposition: D::SharedAuthz,
            operation: "session_revert",
            capability: "session.create",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/sessions/{id}/unrevert",
            disposition: D::SharedAuthz,
            operation: "session_unrevert",
            capability: "session.create",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/sessions/{id}/messages",
            disposition: D::SharedAuthz,
            operation: "session_messages_load",
            capability: "session.read",
        },
        // Projects.
        RouteDispositionEntry {
            method: "GET",
            path: "/api/project",
            disposition: D::SharedAuthz,
            operation: "project_get",
            capability: "project.read",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/project",
            disposition: D::SharedAuthz,
            operation: "project_register",
            capability: "none",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/project/list",
            disposition: D::SharedAuthz,
            operation: "project_list",
            capability: "project.read",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/projects",
            disposition: D::SharedAuthz,
            operation: "project_list",
            capability: "project.read",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/projects/{id}",
            disposition: D::SharedAuthz,
            operation: "project_get",
            capability: "project.read",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/projects/{id}/archive",
            disposition: D::SharedAuthz,
            operation: "project_archive",
            capability: "project.configure",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/projects/{id}/restore",
            disposition: D::SharedAuthz,
            operation: "project_restore",
            capability: "project.configure",
        },
        // Workspaces.
        RouteDispositionEntry {
            method: "GET",
            path: "/api/workspace",
            disposition: D::SharedAuthz,
            operation: "workspace_snapshot_request",
            capability: "project.read",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/workspace",
            disposition: D::SharedAuthz,
            operation: "workspace_register",
            capability: "project.configure",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/workspace/list",
            disposition: D::SharedAuthz,
            operation: "workspace_list",
            capability: "project.read",
        },
        // Files — explicit canonical project/workspace context required.
        RouteDispositionEntry {
            method: "GET",
            path: "/api/file/read",
            disposition: D::SharedAuthz,
            operation: "file_read",
            capability: "file.read",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/file/list",
            disposition: D::SharedAuthz,
            operation: "file_read",
            capability: "file.read",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/file/write",
            disposition: D::SharedAuthz,
            operation: "file_modify",
            capability: "file.modify",
        },
        RouteDispositionEntry {
            method: "DELETE",
            path: "/api/file/delete",
            disposition: D::SharedAuthz,
            operation: "file_modify",
            capability: "file.modify",
        },
        // Permission/question — session-scoped; responses need mutation
        // authority now so M004 can narrow to the controller lease without
        // another bypass (see `authorize_control_response`).
        RouteDispositionEntry {
            method: "GET",
            path: "/api/permission/{session_id}",
            disposition: D::SharedAuthz,
            operation: "permission_list",
            capability: "session.read",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/permission/{session_id}/submit",
            disposition: D::SharedAuthz,
            operation: "permission_respond",
            capability: "session.create",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/question/{session_id}",
            disposition: D::SharedAuthz,
            operation: "question_list",
            capability: "session.read",
        },
        RouteDispositionEntry {
            method: "POST",
            path: "/api/question/{session_id}",
            disposition: D::SharedAuthz,
            operation: "question_respond",
            capability: "session.create",
        },
        // No safe project scope — LocalOwner-only compatibility.
        RouteDispositionEntry {
            method: "GET",
            path: "/api/config",
            disposition: D::LocalOwnerOnly,
            operation: "config_read",
            capability: "none",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/mcp",
            disposition: D::LocalOwnerOnly,
            operation: "mcp_list",
            capability: "none",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/providers",
            disposition: D::LocalOwnerOnly,
            operation: "provider_list",
            capability: "none",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/api/tools",
            disposition: D::LocalOwnerOnly,
            operation: "tool_list",
            capability: "none",
        },
        // Global SSE is not available as an unfiltered team stream.
        RouteDispositionEntry {
            method: "GET",
            path: "/api/event",
            disposition: D::LocalOwnerOnly,
            operation: "event_subscribe",
            capability: "none",
        },
        // Legacy compatibility WebSocket: bounded, authenticated, no
        // projection authority. Team principals fail closed here and must
        // use `/core` (CoreAdapter) for authorized project work.
        RouteDispositionEntry {
            method: "GET",
            path: "/ws",
            disposition: D::LocalOwnerOnly,
            operation: "ws_legacy_rpc",
            capability: "none",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/tui",
            disposition: D::CoreAdapter,
            operation: "tui_transport",
            capability: "none",
        },
        RouteDispositionEntry {
            method: "GET",
            path: "/core",
            disposition: D::CoreAdapter,
            operation: "core_transport",
            capability: "none",
        },
        // Narrow trigger capability — outside principal auth by design.
        RouteDispositionEntry {
            method: "POST",
            path: "/api/v1/task-triggers/{trigger_id}/fire",
            disposition: D::TriggerCapability,
            operation: "work_order_trigger_fire",
            capability: "none",
        },
    ]
}

fn service_for(pool: &sqlx::SqlitePool) -> AuthorizationService {
    AuthorizationService::new(TeamStore::new(pool.clone()))
}

/// Privacy-safe denial: indistinguishable from absence.
pub fn denial_not_found() -> AxumAppError {
    let (code, message) = denial_as_not_found();
    context_error(code, message)
}

/// LocalOwner-only denial for scope-less compatibility routes. Team
/// principals observe a plain 404 with no project/session signal.
pub fn local_owner_denial() -> AxumAppError {
    context_error("not_found", "not found")
}

/// Enforce LocalOwner-only compatibility. The LocalOwner broad policy
/// passes through the canonical authorization service (no bespoke
/// superuser bypass); team principals fail closed.
pub async fn require_local_owner(
    pool: &sqlx::SqlitePool,
    principal: &AuthenticatedPrincipal,
    operation: &'static str,
) -> Result<(), AxumAppError> {
    if is_local_owner_broad(principal) {
        // Still construct a decision through the canonical service so the
        // broad policy carries a decision id for attribution.
        let service = service_for(pool);
        let descriptor = OperationDescriptor::new(operation, ScopeKind::Global, None);
        let request = AuthorizationRequest::new(
            principal.clone(),
            descriptor,
            None,
            format!("http:{operation}"),
        );
        service
            .authorize(&request)
            .await
            .map_err(|_| local_owner_denial())?;
        return Ok(());
    }
    Err(local_owner_denial())
}

/// Authorize `principal` for `capability` on `project`. Denials map to the
/// privacy-safe `project_not_found` shape.
pub async fn authorize_project(
    pool: &sqlx::SqlitePool,
    principal: &AuthenticatedPrincipal,
    project: &ProjectId,
    capability: Capability,
    operation: &'static str,
) -> Result<(), AxumAppError> {
    let service = service_for(pool);
    let descriptor =
        OperationDescriptor::new(operation, ScopeKind::DirectProject, Some(capability));
    let request = AuthorizationRequest::new(
        principal.clone(),
        descriptor,
        Some(project.clone()),
        format!("http:{operation}"),
    );
    service.authorize(&request).await.map(|_| ()).map_err(|e| {
        if e.is_denial() {
            denial_not_found()
        } else {
            context_error("authorization_unavailable", "authorization unavailable")
        }
    })
}

/// Authorize enumeration (project listing). The principal must still be
/// active; rows are filtered post-read via [`visible_projects`].
pub async fn authorize_enumeration(
    pool: &sqlx::SqlitePool,
    principal: &AuthenticatedPrincipal,
    operation: &'static str,
) -> Result<(), AxumAppError> {
    let service = service_for(pool);
    service
        .authorize_enumeration(principal, operation, &format!("http:{operation}"))
        .await
        .map(|_| ())
        .map_err(|e| {
            if e.is_denial() {
                denial_not_found()
            } else {
                context_error("authorization_unavailable", "authorization unavailable")
            }
        })
}

/// Filter catalog project ids to the rows `principal` may observe.
pub async fn filter_visible_projects(
    pool: &sqlx::SqlitePool,
    principal: &AuthenticatedPrincipal,
    candidates: &[ProjectId],
) -> Vec<ProjectId> {
    let service = service_for(pool);
    visible_projects(&service, principal, candidates).await
}

/// Resolve `session_id` to its owning project through the canonical
/// session row, failing closed with the privacy-safe shape when the
/// session or its project cannot be resolved.
pub async fn session_project(
    pool: &sqlx::SqlitePool,
    session_id: &str,
) -> Result<ProjectId, AxumAppError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT project_id FROM session WHERE id = ?")
        .bind(session_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| context_error("project_context_unavailable", e.to_string()))?;
    match row {
        Some((raw,)) => ProjectId::parse(&raw)
            .map_err(|e| context_error("project_not_found", e.to_string()))
            .map_err(|_| denial_not_found()),
        None => Err(denial_not_found()),
    }
}

/// Authorize `principal` for `capability` on the project owning
/// `session_id`. The project is resolved server-side from the durable
/// session row; the caller never supplies it.
pub async fn authorize_session(
    pool: &sqlx::SqlitePool,
    principal: &AuthenticatedPrincipal,
    session_id: &str,
    capability: Capability,
    operation: &'static str,
) -> Result<ProjectId, AxumAppError> {
    let project = session_project(pool, session_id).await?;
    let service = service_for(pool);
    let descriptor = OperationDescriptor::new(operation, ScopeKind::ViaSession, Some(capability));
    let request = AuthorizationRequest::new(
        principal.clone(),
        descriptor,
        Some(project.clone()),
        format!("http:{operation}"),
    );
    service
        .authorize(&request)
        .await
        .map(|_| project)
        .map_err(|e| {
            if e.is_denial() {
                denial_not_found()
            } else {
                context_error("authorization_unavailable", "authorization unavailable")
            }
        })
}

/// Policy hook for permission/question responses.
///
/// M001 requires mutation authority (`session.create`) on the owning
/// session before exposing or mutating a pending item. M004 (controller
/// lease) will narrow this hook to the active-turn controller without
/// adding another registry-only route: add the controller check here and
/// keep the capability gate.
pub async fn authorize_control_response(
    pool: &sqlx::SqlitePool,
    principal: &AuthenticatedPrincipal,
    session_id: &str,
    operation: &'static str,
) -> Result<ProjectId, AxumAppError> {
    // M004 hook: controller-lease narrowing plugs in here.
    authorize_session(
        pool,
        principal,
        session_id,
        Capability::SessionCreate,
        operation,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_table_has_no_duplicate_routes() {
        let table = route_disposition_table();
        let mut keys: Vec<String> = table
            .iter()
            .map(|e| format!("{} {}", e.method, e.path))
            .collect();
        keys.sort();
        let len = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), len, "duplicate route disposition entries");
    }

    #[test]
    fn every_shared_authz_row_names_a_capability() {
        for entry in route_disposition_table() {
            if entry.disposition == RouteDisposition::SharedAuthz {
                assert_ne!(
                    entry.capability, "none",
                    "shared_authz route must name a capability: {} {}",
                    entry.method, entry.path
                );
            }
        }
    }
}
