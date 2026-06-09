use std::sync::Arc;

use crate::agent::turn_runtime::TurnRuntime;

/// Bundles optional runtime dependencies for [`CoreDaemon`].
///
/// This localizes concrete agent/tool types so `CoreDaemon` does not
/// need to import `SubAgentPool`, `BackgroundScheduler`, etc. directly.
#[derive(Clone, Default)]
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    /// The turn runtime that owns tool registry, permission checker,
    /// agent loop construction, and turn execution.
    ///
    /// When `Some`, the daemon delegates turn execution to this runtime
    /// instead of building tools/permissions/agent_loop inline.
    pub agent_runtime: Option<Arc<dyn TurnRuntime>>,
}

impl CoreRuntimeDeps {
    pub fn new(
        pool: Option<sqlx::SqlitePool>,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    ) -> Self {
        Self { pool, memory_store, subagent_pool, bg_scheduler, agent_runtime: None }
    }

    /// Builder-style setter for the agent runtime.
    pub fn with_agent_runtime(mut self, runtime: Arc<dyn TurnRuntime>) -> Self {
        self.agent_runtime = Some(runtime);
        self
    }
}
