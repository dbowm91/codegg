use axum::extract::FromRef;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::mcp::McpService;
use crate::server::routes::GlobalEventBus;

#[derive(Clone)]
pub struct ServerState {
    pub project_dir: String,
    pub pool: SqlitePool,
    pub mcp_service: Arc<RwLock<McpService>>,
    pub event_bus: GlobalEventBus,
}

impl FromRef<ServerState> for SqlitePool {
    fn from_ref(state: &ServerState) -> SqlitePool {
        state.pool.clone()
    }
}

impl FromRef<ServerState> for Arc<RwLock<McpService>> {
    fn from_ref(state: &ServerState) -> Arc<RwLock<McpService>> {
        state.mcp_service.clone()
    }
}

impl FromRef<ServerState> for GlobalEventBus {
    fn from_ref(state: &ServerState) -> GlobalEventBus {
        state.event_bus.clone()
    }
}
