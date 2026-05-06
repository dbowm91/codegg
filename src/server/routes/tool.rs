use axum::Json;
use serde::Serialize;

use crate::error::AppError;
use crate::tool::ToolRegistry;

#[derive(Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct ToolListResponse {
    pub tools: Vec<ToolInfo>,
}

pub async fn list_tools() -> Result<Json<ToolListResponse>, AppError> {
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
