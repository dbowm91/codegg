use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::FromRef;
use sqlx::SqlitePool;
use tokio::sync::{Mutex, RwLock};

use crate::config::schema::Config;
use crate::core::transport::projection::ProjectionLifecycleSeam;
use crate::mcp::McpService;
use crate::server::ws::ConnectionTaskProbe;

#[derive(Clone)]
pub struct ServerState {
    pub pool: SqlitePool,
    pub mcp_service: Arc<RwLock<McpService>>,
    pub config: Config,
    pub ws_rate_limiter: Arc<WsRateLimiter>,
    pub daemon: Option<Arc<crate::core::daemon::CoreDaemon>>,
    /// Connection-adapter lifecycle seam. The default is a no-op; tests may
    /// supply a connection-local pause/fault policy without global state.
    pub projection_lifecycle_seam: ProjectionLifecycleSeam,
    /// Connection-local task completion probe. Each WebSocket connection
    /// receives its own clone of this Arc; tests read the counters after
    /// handler exit to verify lifecycle invariants.
    pub connection_task_probe: Option<Arc<ConnectionTaskProbe>>,
}

#[derive(Clone)]
pub struct WsRateLimiter {
    cache: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}

impl WsRateLimiter {
    pub fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    pub async fn check_rate_limit(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut cache = self.cache.lock().await;

        let requests = cache.entry(key.to_string()).or_insert_with(Vec::new);
        requests.retain(|&t| now.duration_since(t) < self.window);

        if requests.len() >= self.max_requests {
            return false;
        }

        requests.push(now);
        true
    }
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

impl FromRef<ServerState> for Config {
    fn from_ref(state: &ServerState) -> Config {
        state.config.clone()
    }
}
