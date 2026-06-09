use std::sync::Arc;

use crate::agent::turn_runtime::TurnRuntime;

/// Bundles optional runtime dependencies for [`CoreDaemon`].
///
/// This localizes concrete agent/tool types so `CoreDaemon` does not
/// need to import `SubAgentPool`, `BackgroundScheduler`, etc. directly.
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    /// The turn runtime that owns tool registry, permission checker,
    /// agent loop construction, and turn execution.
    ///
    /// Always present: defaults to [`crate::agent::turn_runtime::DefaultTurnRuntime`].
    pub turn_runtime: Arc<dyn TurnRuntime>,
}

impl Clone for CoreRuntimeDeps {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            memory_store: self.memory_store.clone(),
            subagent_pool: self.subagent_pool.clone(),
            bg_scheduler: self.bg_scheduler.clone(),
            turn_runtime: Arc::clone(&self.turn_runtime),
        }
    }
}

impl CoreRuntimeDeps {
    pub fn new(
        pool: Option<sqlx::SqlitePool>,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    ) -> Self {
        Self {
            pool,
            memory_store,
            subagent_pool,
            bg_scheduler,
            turn_runtime: Arc::new(crate::agent::turn_runtime::DefaultTurnRuntime),
        }
    }

    /// Builder-style setter for the turn runtime.
    pub fn with_turn_runtime(mut self, runtime: Arc<dyn TurnRuntime>) -> Self {
        self.turn_runtime = runtime;
        self
    }
}
