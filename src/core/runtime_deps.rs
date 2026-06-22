use std::sync::Arc;

use crate::agent::turn_runtime::TurnRuntime;

/// Transitional container for concrete agent runtime dependencies.
///
/// These fields are still needed for task scheduling and subagent spawning,
/// but will eventually be replaced by the turn runtime abstraction.
/// Grouped here to make their legacy status explicit.
#[derive(Clone, Default)]
pub struct LegacyAgentRuntimeDeps {
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
}

/// Bundles optional runtime dependencies for [`CoreDaemon`].
///
/// This localizes concrete agent/tool types so `CoreDaemon` does not
/// need to import `SubAgentPool`, `BackgroundScheduler`, etc. directly.
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub legacy_agent: LegacyAgentRuntimeDeps,
    /// The turn runtime that owns tool registry, permission checker,
    /// agent loop construction, and turn execution.
    ///
    /// Always present: defaults to [`crate::agent::turn_runtime::DefaultTurnRuntime`].
    pub turn_runtime: Arc<dyn TurnRuntime>,
    /// Shared LSP service for context assembly in agent prompts.
    /// `None` in socket/remote mode; `Some` in local mode.
    pub lsp_service: Option<Arc<crate::lsp::service::LspService>>,
}

impl Clone for CoreRuntimeDeps {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            memory_store: self.memory_store.clone(),
            legacy_agent: self.legacy_agent.clone(),
            turn_runtime: Arc::clone(&self.turn_runtime),
            lsp_service: self.lsp_service.clone(),
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
            legacy_agent: LegacyAgentRuntimeDeps {
                subagent_pool,
                bg_scheduler,
            },
            turn_runtime: Arc::new(crate::agent::turn_runtime::DefaultTurnRuntime),
            lsp_service: None,
        }
    }

    pub fn from_parts(
        pool: Option<sqlx::SqlitePool>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        legacy_agent: LegacyAgentRuntimeDeps,
        turn_runtime: Arc<dyn TurnRuntime>,
    ) -> Self {
        Self {
            pool,
            memory_store,
            legacy_agent,
            turn_runtime,
            lsp_service: None,
        }
    }

    /// Builder-style setter for the turn runtime.
    pub fn with_turn_runtime(mut self, runtime: Arc<dyn TurnRuntime>) -> Self {
        self.turn_runtime = runtime;
        self
    }

    /// Builder-style setter for the shared LSP service.
    pub fn with_lsp_service(mut self, service: Arc<crate::lsp::service::LspService>) -> Self {
        self.lsp_service = Some(service);
        self
    }
}
