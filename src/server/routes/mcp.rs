use axum::{
    extract::{Extension, State},
    Json,
};
use serde::Serialize;

use super::super::authz;
use crate::error::AxumAppError;
use codegg_core::transport_auth::AuthenticatedPrincipal;

#[derive(Serialize)]
pub struct McpServerStatusResponse {
    pub name: String,
    pub status: String,
    pub status_error: Option<String>,
    pub tool_count: usize,
}

/// MCP status compatibility surface. Server status is daemon-global with
/// no project scope, so it remains LocalOwner-only. Team principals fail
/// closed with a privacy-safe 404.
pub async fn list_mcp_servers(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<crate::server::state::ServerState>,
) -> Result<Json<Vec<McpServerStatusResponse>>, AxumAppError> {
    authz::require_local_owner(&state.pool, &principal, "mcp_list").await?;
    let service = state.mcp_service.read().await;
    let statuses: Vec<McpServerStatusResponse> = service
        .server_status()
        .iter()
        .map(|(name, status)| {
            let (status_str, status_error) = match status {
                crate::mcp::McpServerStatus::Disconnected => ("disconnected".to_string(), None),
                crate::mcp::McpServerStatus::Connecting => ("connecting".to_string(), None),
                crate::mcp::McpServerStatus::Connected => ("connected".to_string(), None),
                crate::mcp::McpServerStatus::Error(e) => ("error".to_string(), Some(e.clone())),
            };
            let tool_count = service
                .server_tools()
                .get(name)
                .map(|t| t.len())
                .unwrap_or(0);
            McpServerStatusResponse {
                name: name.to_string(),
                status: status_str,
                status_error,
                tool_count,
            }
        })
        .collect();

    Ok(Json(statuses))
}
