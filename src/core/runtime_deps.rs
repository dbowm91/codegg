use std::sync::Arc;

use crate::agent::turn_runtime::TurnRuntime;

use codegg_core::jobs::{
    DaemonGeneration, InMemoryJobStore, InMemoryScheduleStore, JobStore, RecoveryPolicy,
    ScheduleStore, SqliteJobStore, SqliteScheduleStore,
};
use codegg_core::workspace_services::{WorkspaceServicePolicy, WorkspaceServiceRegistry};

/// Transitional container for concrete agent runtime dependencies.
///
/// These fields are still needed for task scheduling and subagent spawning,
/// but will eventually be replaced by the turn runtime abstraction.
/// Grouped here to make their legacy status explicit.
#[derive(Clone, Default)]
pub struct LegacyAgentRuntimeDeps {
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    /// Compatibility switch for legacy `TaskSchedule` / `TaskDelete` /
    /// `TaskList` requests. It is enabled only by the legacy convenience
    /// constructor; production SQLite daemons use the durable schedule API.
    pub bg_scheduler_compat_enabled: bool,
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
    /// Phase 3: workspace services registry. The daemon owns exactly
    /// one of these; it is created during `CoreDaemon` construction
    /// using the daemon's workspace registry and a factory appropriate
    /// for the runtime mode. `None` only in legacy/test daemons that
    /// have not yet been migrated; production daemons always populate
    /// this.
    pub workspace_services: Option<Arc<WorkspaceServiceRegistry>>,
    /// Phase 3: workspace service lifecycle policy. Tunables for
    /// `max_active_workspaces` and `idle_evict_after`. The daemon
    /// constructs a default policy if the caller does not supply one.
    pub workspace_service_policy: WorkspaceServicePolicy,
    /// Phase 4: durable job control plane store.
    pub job_store: Arc<dyn JobStore>,
    /// Phase 4: durable schedule store.
    pub schedule_store: Arc<dyn ScheduleStore>,
    /// Phase 4: recovery policy applied at daemon startup.
    pub recovery_policy: RecoveryPolicy,
    /// Phase 4: daemon generation for attempt lease tracking.
    pub daemon_generation: DaemonGeneration,
    /// Phase 5: global admission control scheduler. The daemon owns
    /// exactly one of these when enabled; it is created during
    /// `CoreDaemon` construction using the configured budgets and a
    /// default executor set. `None` in legacy daemons that have not
    /// yet been migrated; production daemons populate this when
    /// `[scheduler].enabled = true`.
    pub scheduler: Option<Arc<crate::scheduler::JobScheduler>>,
    /// Daemon-owned create/enqueue facade. Production tools use this
    /// instead of writing to `JobStore` and dispatching separately.
    pub submission: Option<Arc<crate::scheduler::JobSubmissionService>>,
    /// Phase 5: scheduler configuration applied at construction time.
    /// Even when `scheduler` is `None`, the resolved config is held
    /// here so it can be queried for snapshots and settings pages.
    pub scheduler_config: crate::scheduler::config::ResolvedSchedulerConfig,
}

impl Clone for CoreRuntimeDeps {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            memory_store: self.memory_store.clone(),
            legacy_agent: self.legacy_agent.clone(),
            turn_runtime: Arc::clone(&self.turn_runtime),
            lsp_service: self.lsp_service.clone(),
            workspace_services: self.workspace_services.clone(),
            workspace_service_policy: self.workspace_service_policy.clone(),
            job_store: Arc::clone(&self.job_store),
            schedule_store: Arc::clone(&self.schedule_store),
            recovery_policy: self.recovery_policy.clone(),
            daemon_generation: self.daemon_generation.clone(),
            scheduler: self.scheduler.clone(),
            submission: self.submission.clone(),
            scheduler_config: self.scheduler_config.clone(),
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
        // Use in-memory stores for the legacy convenience constructor.
        let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let schedule_store: Arc<dyn ScheduleStore> =
            Arc::new(InMemoryScheduleStore::new(Arc::clone(&job_store)));
        Self {
            pool,
            memory_store,
            legacy_agent: LegacyAgentRuntimeDeps {
                subagent_pool,
                bg_scheduler,
                bg_scheduler_compat_enabled: true,
            },
            turn_runtime: Arc::new(crate::agent::turn_runtime::DefaultTurnRuntime),
            lsp_service: None,
            workspace_services: None,
            workspace_service_policy: WorkspaceServicePolicy::default(),
            job_store,
            schedule_store,
            recovery_policy: RecoveryPolicy::default(),
            daemon_generation: DaemonGeneration::new(),
            scheduler: None,
            submission: None,
            scheduler_config: crate::scheduler::config::ResolvedSchedulerConfig::default(),
        }
    }

    pub fn from_parts(
        pool: Option<sqlx::SqlitePool>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        legacy_agent: LegacyAgentRuntimeDeps,
        turn_runtime: Arc<dyn TurnRuntime>,
    ) -> Self {
        let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let schedule_store: Arc<dyn ScheduleStore> =
            Arc::new(InMemoryScheduleStore::new(Arc::clone(&job_store)));
        Self {
            pool,
            memory_store,
            legacy_agent,
            turn_runtime,
            lsp_service: None,
            workspace_services: None,
            workspace_service_policy: WorkspaceServicePolicy::default(),
            job_store,
            schedule_store,
            recovery_policy: RecoveryPolicy::default(),
            daemon_generation: DaemonGeneration::new(),
            scheduler: None,
            submission: None,
            scheduler_config: crate::scheduler::config::ResolvedSchedulerConfig::default(),
        }
    }

    /// Construct `CoreRuntimeDeps` with production SQLite-backed stores.
    ///
    /// This is the preferred constructor for daemons that have a pool.
    pub fn with_jobs(
        pool: sqlx::SqlitePool,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    ) -> Self {
        let job_store: Arc<dyn JobStore> = Arc::new(SqliteJobStore::new(pool.clone()));
        let schedule_store: Arc<dyn ScheduleStore> = Arc::new(SqliteScheduleStore::new(
            pool.clone(),
            Arc::clone(&job_store),
        ));
        Self {
            pool: Some(pool),
            memory_store,
            legacy_agent: LegacyAgentRuntimeDeps {
                subagent_pool,
                bg_scheduler,
                bg_scheduler_compat_enabled: false,
            },
            turn_runtime: Arc::new(crate::agent::turn_runtime::DefaultTurnRuntime),
            lsp_service: None,
            workspace_services: None,
            workspace_service_policy: WorkspaceServicePolicy::default(),
            job_store,
            schedule_store,
            recovery_policy: RecoveryPolicy::default(),
            daemon_generation: DaemonGeneration::new(),
            scheduler: None,
            submission: None,
            scheduler_config: crate::scheduler::config::ResolvedSchedulerConfig::default(),
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

    /// Builder-style setter for the workspace services registry.
    pub fn with_workspace_services(mut self, services: Arc<WorkspaceServiceRegistry>) -> Self {
        self.workspace_services = Some(services);
        self
    }

    /// Builder-style setter for the workspace service policy.
    pub fn with_workspace_service_policy(mut self, policy: WorkspaceServicePolicy) -> Self {
        self.workspace_service_policy = policy;
        self
    }

    /// Builder-style setter for the global admission scheduler.
    pub fn with_scheduler(
        mut self,
        scheduler: Arc<crate::scheduler::JobScheduler>,
        config: crate::scheduler::config::ResolvedSchedulerConfig,
    ) -> Self {
        self.scheduler = Some(scheduler);
        self.scheduler_config = config;
        self
    }

    pub fn with_submission(
        mut self,
        submission: Arc<crate::scheduler::JobSubmissionService>,
    ) -> Self {
        self.submission = Some(submission);
        self
    }
}
