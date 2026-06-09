use std::sync::Arc;

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
}

impl CoreRuntimeDeps {
    pub fn new(
        pool: Option<sqlx::SqlitePool>,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    ) -> Self {
        Self { pool, memory_store, subagent_pool, bg_scheduler }
    }
}
