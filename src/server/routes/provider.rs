use axum::Json;
use serde::Serialize;

use crate::error::AxumAppError;
use crate::provider::ProviderRegistry;

#[derive(Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct ProviderListResponse {
    pub providers: Vec<ProviderInfo>,
}

pub async fn list_providers() -> Result<Json<ProviderListResponse>, AxumAppError> {
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
