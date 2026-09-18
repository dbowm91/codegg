use axum::{
    extract::{Extension, State},
    Json,
};
use serde::Serialize;

use super::super::authz;
use super::super::state::ServerState;
use crate::error::AxumAppError;
use crate::provider::ProviderRegistry;
use codegg_core::transport_auth::AuthenticatedPrincipal;

#[derive(Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct ProviderListResponse {
    pub providers: Vec<ProviderInfo>,
}

/// Provider catalog compatibility surface. Connection details are
/// credential-adjacent with no safe project scope, so this remains
/// LocalOwner-only (Core connection operations are opaque and fail closed
/// for team principals). Team principals fail closed with a 404.
pub async fn list_providers(
    Extension(principal): Extension<AuthenticatedPrincipal>,
    State(state): State<ServerState>,
) -> Result<Json<ProviderListResponse>, AxumAppError> {
    authz::require_local_owner(&state.pool, &principal, "provider_list").await?;
    let mut registry = ProviderRegistry::new();
    crate::provider::register_builtin(&mut registry);

    let providers = registry
        .list()
        .into_iter()
        .map(|p| ProviderInfo {
            id: p.id().to_string(),
            name: p.name().to_string(),
        })
        .collect();

    Ok(Json(ProviderListResponse { providers }))
}
