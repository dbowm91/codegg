use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use axum::extract::FromRef;
use sqlx::SqlitePool;
use tokio::sync::{Mutex, RwLock};

use crate::config::schema::Config;
use crate::core::transport::projection::ProjectionLifecycleSeam;
use crate::mcp::McpService;
use crate::server::ws::{ConnectionTaskProbe, ProjectionTransportTestConfig};

/// Returns a fresh per-connection [`Arc<ConnectionTaskProbe>`]. Each upgraded
/// WebSocket connection consults the factory during `upgrade_core_ws` /
/// `upgrade_tui` so that probe counters are connection-local. The factory
/// returns the probe for the connection being upgraded and (optionally)
/// registers it with the calling test for later retrieval.
pub type ConnectionProbeFactory = Arc<dyn Fn(&str) -> Arc<ConnectionTaskProbe> + Send + Sync>;

/// Test-side registry that allocates a fresh [`Arc<ConnectionTaskProbe>`] per
/// upgrade and records the produced probes by the actual connection identity.
/// Tests pass the returned factory to `ServerState::probe_factory` and call
/// [`ConnectionProbeRegistry::take`] to drain finalized records, or
/// [`ConnectionProbeRegistry::for_connection`] to retrieve one by its actual
/// connection identity.
#[derive(Clone, Default)]
pub struct ConnectionProbeRegistry {
    inner: Arc<StdMutex<ProbeRegistryInner>>,
}

const MAX_RETAINED_CONNECTION_PROBES: usize = 256;

#[derive(Default)]
struct ProbeRegistryInner {
    entries: VecDeque<(String, Arc<ConnectionTaskProbe>)>,
}

impl ConnectionProbeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a factory closure that allocates a fresh probe and records it
    /// under the actual connection identity. Registration is synchronous and
    /// infallible; a poisoned mutex is recovered rather than dropping the
    /// correlation record.
    /// into this registry. The factory may be installed in
    /// [`ServerState::probe_factory`].
    pub fn factory(&self) -> ConnectionProbeFactory {
        let inner = Arc::clone(&self.inner);
        Arc::new(move |connection_id| {
            let probe = Arc::new(ConnectionTaskProbe::new());
            let mut guard = match inner.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if guard.entries.len() == MAX_RETAINED_CONNECTION_PROBES {
                guard.entries.pop_front();
            }
            guard
                .entries
                .push_back((connection_id.to_string(), Arc::clone(&probe)));
            probe
        })
    }

    /// Drain and return every retained probe currently registered.
    pub async fn take(&self) -> Vec<Arc<ConnectionTaskProbe>> {
        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.entries.drain(..).map(|(_, probe)| probe).collect()
    }

    /// Return the finalized probe for an exact connection identity, when it
    /// is still retained by the bounded registry.
    pub fn for_connection(&self, connection_id: &str) -> Option<Arc<ConnectionTaskProbe>> {
        let guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard
            .entries
            .iter()
            .find(|(id, _)| id == connection_id)
            .map(|(_, probe)| Arc::clone(probe))
    }

    /// Read the current probes without draining.
    pub async fn snapshot(&self) -> Vec<Arc<ConnectionTaskProbe>> {
        let guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard
            .entries
            .iter()
            .map(|(_, probe)| Arc::clone(probe))
            .collect()
    }
}

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
    /// Aggregate (server-wide) connection probe retained by older tests that
    /// need to count exactly how many send/receive/raw_event completions
    /// happened across all connections during a multi-connection scenario
    /// (e.g. the 100-cycle churn fixture). Each upgraded connection receives
    /// a clone of this Arc, so probe counters accumulate across connections.
    /// New per-connection fixtures SHOULD prefer [`ServerState::probe_factory`]
    /// for exact per-connection assertions.
    pub connection_task_probe: Option<Arc<ConnectionTaskProbe>>,
    /// Per-connection probe factory. When set, every upgraded WebSocket
    /// connection invokes the factory to obtain a fresh
    /// [`Arc<ConnectionTaskProbe>`] that is local to that connection. This
    /// is the preferred wiring for evidence correctness: per-connection
    /// counters never bleed across connections, and a test can capture the
    /// factory's output to assert against a specific connection only.
    pub probe_factory: Option<ConnectionProbeFactory>,
    /// Connection-local test configuration. Production callers leave this
    /// at `None`; tests may opt into a smaller outbound queue capacity,
    /// a writer gate, a raw-source cancellation token, and a lifecycle
    /// observer. Each field is connection-scoped and removed with the
    /// fixture.
    pub transport_test_config: Option<ProjectionTransportTestConfig>,
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
