use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::AxumAppError;
use crate::mcp::McpService;

#[derive(Serialize)]
pub struct McpServerStatusResponse {
    pub name: String,
    pub status: String,
    pub status_error: Option<String>,
    pub tool_count: usize,
}

pub async fn list_mcp_servers(
    State(mcp_service): State<Arc<RwLock<McpService>>>,
) -> Result<Json<Vec<McpServerStatusResponse>>, AxumAppError> {
    let service = mcp_service.read().await;
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
