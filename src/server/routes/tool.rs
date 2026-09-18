use axum::{
    extract::{Extension, State},
    Json,
};
use serde::Serialize;

use super::super::authz;
use super::super::state::ServerState;
use crate::error::AxumAppError;
use crate::tool::ToolRegistry;
use codegg_core::transport_auth::AuthenticatedPrincipal;

#[derive(Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct ToolListResponse {
    pub tools: Vec<ToolInfo>,
}

/// Tool catalog compatibility surface. The tool list is daemon-global with
/// no project scope, so it remains LocalOwner-only. Team principals fail
/// closed; authorized tool use flows through `/core` session capabilities.
pub async fn list_tools(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
) -> Result<Json<ToolListResponse>, AxumAppError> {
    authz::require_local_owner(&state.pool, &principal, "tool_list").await?;
    let registry = ToolRegistry::default();
    let tools = registry
        .list()
        .into_iter()
        .map(|t| ToolInfo {
            name: t.name().to_string(),
            description: t.description().to_string(),
        })
        .collect();

    Ok(Json(ToolListResponse { tools }))
}
