use std::sync::Arc;

use crate::agent::turn_runtime::TurnRuntime;

use codegg_core::jobs::{
    DaemonGeneration, InMemoryJobStore, InMemoryScheduleStore, JobStore, RecoveryPolicy,
    ScheduleStore, SqliteJobStore, SqliteScheduleStore,
};
use codegg_core::workspace_services::{WorkspaceServicePolicy, WorkspaceServiceRegistry};

struct DurableWorktreeOwnerResolver {
    runs: Arc<dyn codegg_core::agent_run::AgentRunStore>,
}

#[async_trait::async_trait]
impl codegg_core::worktree_service::WorktreeOwnerResolver for DurableWorktreeOwnerResolver {
    async fn is_terminal(&self, owner: &codegg_core::identity::AgentRunId) -> Result<bool, String> {
        Ok(self
            .runs
            .get_run(owner)
            .await
            .map_err(|error| error.to_string())?
            .is_some_and(|run| run.status.is_terminal()))
    }
}

fn default_connection_manager(
    pool: &sqlx::SqlitePool,
) -> Option<Arc<crate::core::provider_connections::ConnectionManager>> {
    let credential_store = codegg_providers::CredentialStore::at_default_location().ok()?;
    let metadata_store =
        Arc::new(codegg_core::provider_connections::ProviderConnectionStore::new(pool.clone()));
    let factory = Arc::new(codegg_providers::ProviderConnectionFactory::from_store(
        Arc::new(credential_store),
    ));
    Some(Arc::new(
        crate::core::provider_connections::ConnectionManager::new(metadata_store, factory),
    ))
}

/// Transitional container for concrete agent runtime dependencies.
///
/// Subagent spawning remains here for legacy construction paths; scheduled
/// work is owned exclusively by the durable scheduler stores.
#[derive(Clone, Default)]
pub struct LegacyAgentRuntimeDeps {
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
}

/// Bundles optional runtime dependencies for [`CoreDaemon`].
///
/// This localizes concrete agent/tool types so `CoreDaemon` does not
/// need to import `SubAgentPool` directly.
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub legacy_agent: LegacyAgentRuntimeDeps,
    /// Canonical durable ownership store for delegated agent tasks/runs.
    pub agent_run_store: Arc<dyn codegg_core::agent_run::AgentRunStore>,
    /// Durable mailbox/journal bridge for delegated-run controls.
    pub run_control: Arc<crate::agent::run_control::RunControlService>,
    /// Durable bounded run-group coordination over the canonical run store.
    pub run_group_service: Arc<codegg_core::agent_run_group::AgentRunGroupService>,
    /// Durable convergence records and bounded cycle state.
    pub convergence_store: Arc<dyn codegg_core::agent_convergence::ConvergenceStore>,
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
    /// M003: durable daemon-owned managed worktree service.
    pub worktree_service: Option<Arc<codegg_core::worktree_service::WorktreeService>>,
    /// M005 corrective C002: durable, generation-fenced checkout writer permit
    /// service.
    ///
    /// The daemon owns exactly one. It is the *authority* for who may write
    /// to a managed checkout, and it is deliberately **not** wired to any
    /// writer: C002 builds the permit, and C003/C004 are what route writers
    /// through it. No shared mutation is enabled by its presence.
    pub writer_permit_service: Option<Arc<crate::core::writer_permit::WriterPermitService>>,
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
    /// C001 daemon-scoped singleton job-terminal notifier. Created once per
    /// daemon lifetime and injected into turn runtime + watch service; per-turn
    /// construction is the pre-C001 fallback only. `None` in legacy/test
    /// daemons that have not yet asked for the shared handle.
    pub job_notification_service:
        Option<Arc<crate::scheduler::job_notifications::JobNotificationService>>,
    /// C001 daemon-scoped singleton CI-watch service. Created once per
    /// daemon lifetime alongside the notifier above; shared by every turn
    /// and by the daemon-owned idle driver. `None` until initialized.
    pub ci_watch_service: Option<Arc<crate::scheduler::ci_watch::CiWatchService>>,
    /// C004 daemon-scoped singleton idle-continuation grant/intent store.
    /// Created once per daemon lifetime alongside the notifier above.
    /// Production idle provider dispatch remains blocked (fail-closed):
    /// no daemon path calls `maybe_dispatch` with a real executor until
    /// an authenticated host-grant workflow exists. The store is owned
    /// here so future grant issuance can bind server-resolved principal,
    /// session, and workspace instead of caller-supplied strings.
    pub continuation_service: Option<Arc<crate::scheduler::idle_continuation::ContinuationService>>,
    /// C001 daemon-owned idle CI-watch coordinator (earliest-due wake loop).
    /// `None` when idle watch support is unavailable (pool-less) or not yet
    /// started; exactly one loop per daemon when present.
    pub ci_watch_driver: Option<Arc<crate::scheduler::ci_watch_idle::CiWatchIdleDriver>>,
    /// Phase 5: scheduler configuration applied at construction time.
    /// Even when `scheduler` is `None`, the resolved config is held
    /// here so it can be queried for snapshots and settings pages.
    pub scheduler_config: crate::scheduler::config::ResolvedSchedulerConfig,
    /// Daemon-owned lazy provider-connection runtime manager. `None` keeps
    /// legacy/test constructors source-compatible; production wiring can
    /// install it with [`Self::with_connection_manager`].
    pub connection_manager: Option<Arc<crate::core::provider_connections::ConnectionManager>>,
}

impl Clone for CoreRuntimeDeps {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            memory_store: self.memory_store.clone(),
            legacy_agent: self.legacy_agent.clone(),
            agent_run_store: Arc::clone(&self.agent_run_store),
            run_control: Arc::clone(&self.run_control),
            run_group_service: Arc::clone(&self.run_group_service),
            convergence_store: Arc::clone(&self.convergence_store),
            turn_runtime: Arc::clone(&self.turn_runtime),
            lsp_service: self.lsp_service.clone(),
            workspace_services: self.workspace_services.clone(),
            worktree_service: self.worktree_service.clone(),
            writer_permit_service: self.writer_permit_service.clone(),
            workspace_service_policy: self.workspace_service_policy.clone(),
            job_store: Arc::clone(&self.job_store),
            schedule_store: Arc::clone(&self.schedule_store),
            recovery_policy: self.recovery_policy.clone(),
            daemon_generation: self.daemon_generation.clone(),
            scheduler: self.scheduler.clone(),
            submission: self.submission.clone(),
            job_notification_service: self.job_notification_service.clone(),
            ci_watch_service: self.ci_watch_service.clone(),
            continuation_service: self.continuation_service.clone(),
            ci_watch_driver: self.ci_watch_driver.clone(),
            scheduler_config: self.scheduler_config.clone(),
            connection_manager: self.connection_manager.clone(),
        }
    }
}

impl CoreRuntimeDeps {
    /// C001 shared daemon-scoped notifier, creating it once on first use.
    /// The same `Arc` is returned on every call (pointer identity is the
    /// singleton proof); per-turn construction must not replace it.
    pub fn shared_job_notification_service(
        &mut self,
    ) -> Arc<crate::scheduler::job_notifications::JobNotificationService> {
        if let Some(existing) = self.job_notification_service.clone() {
            return existing;
        }
        let created = Arc::new(match self.pool.clone() {
            Some(pool) => {
                crate::scheduler::job_notifications::JobNotificationService::with_pool(pool)
            }
            None => crate::scheduler::job_notifications::JobNotificationService::new(),
        });
        self.job_notification_service = Some(created.clone());
        created
    }

    /// C001 shared daemon-scoped watch service, creating it once on first use
    /// over the shared notifier above. Pointer identity is the singleton proof.
    pub fn shared_ci_watch_service(&mut self) -> Arc<crate::scheduler::ci_watch::CiWatchService> {
        if let Some(existing) = self.ci_watch_service.clone() {
            return existing;
        }
        // Avoid double-borrow: build the notifier first, then the watcher.
        let notifier = self.shared_job_notification_service();
        let created = Arc::new(match self.pool.clone() {
            Some(pool) => crate::scheduler::ci_watch::CiWatchService::with_pool(
                pool,
                notifier,
                crate::scheduler::ci_watch::CiWatchConfig::from_env(),
            ),
            None => crate::scheduler::ci_watch::CiWatchService::new(
                notifier,
                crate::scheduler::ci_watch::CiWatchConfig::from_env(),
            ),
        });
        self.ci_watch_service = Some(created.clone());
        created
    }

    /// C004 shared daemon-scoped continuation store, creating it once on
    /// first use. Pointer identity is the singleton proof. Production
    /// dispatch stays fail-closed: owning the store does not start turns.
    pub fn shared_continuation_service(
        &mut self,
    ) -> Arc<crate::scheduler::idle_continuation::ContinuationService> {
        if let Some(existing) = self.continuation_service.clone() {
            return existing;
        }
        let created = Arc::new(match self.pool.clone() {
            Some(pool) => crate::scheduler::idle_continuation::ContinuationService::with_pool(pool),
            None => crate::scheduler::idle_continuation::ContinuationService::new(),
        });
        self.continuation_service = Some(created.clone());
        created
    }
}

impl CoreRuntimeDeps {
    pub fn new(
        pool: Option<sqlx::SqlitePool>,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
    ) -> Self {
        // Use in-memory stores for the legacy convenience constructor.
        let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let schedule_store: Arc<dyn ScheduleStore> =
            Arc::new(InMemoryScheduleStore::new(Arc::clone(&job_store)));
        let agent_run_store: Arc<dyn codegg_core::agent_run::AgentRunStore> =
            Arc::new(codegg_core::agent_run::InMemoryAgentRunStore::new());
        let run_control =
            crate::agent::run_control::RunControlService::in_memory(agent_run_store.clone());
        let run_group_service =
            codegg_core::agent_run_group::AgentRunGroupService::in_memory(agent_run_store.clone());
        let convergence_store: Arc<dyn codegg_core::agent_convergence::ConvergenceStore> =
            Arc::new(codegg_core::agent_convergence::InMemoryConvergenceStore::new());
        run_control.set_group_service_sync(run_group_service.clone());
        Self {
            pool,
            memory_store,
            legacy_agent: LegacyAgentRuntimeDeps { subagent_pool },
            agent_run_store,
            run_control,
            run_group_service,
            convergence_store,
            turn_runtime: Arc::new(crate::agent::turn_runtime::DefaultTurnRuntime),
            lsp_service: None,
            workspace_services: None,
            worktree_service: Some(codegg_core::worktree_service::WorktreeService::memory(
                std::path::PathBuf::from(".codegg/worktrees"),
                Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
            )),
            writer_permit_service: None,
            workspace_service_policy: WorkspaceServicePolicy::default(),
            job_store,
            schedule_store,
            recovery_policy: RecoveryPolicy::default(),
            daemon_generation: DaemonGeneration::new(),
            scheduler: None,
            submission: None,
            job_notification_service: None,
            ci_watch_service: None,
            continuation_service: None,
            ci_watch_driver: None,
            scheduler_config: crate::scheduler::config::ResolvedSchedulerConfig::default(),
            connection_manager: None,
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
        let agent_run_store: Arc<dyn codegg_core::agent_run::AgentRunStore> =
            Arc::new(codegg_core::agent_run::InMemoryAgentRunStore::new());
        let run_control =
            crate::agent::run_control::RunControlService::in_memory(agent_run_store.clone());
        let run_group_service =
            codegg_core::agent_run_group::AgentRunGroupService::in_memory(agent_run_store.clone());
        let convergence_store: Arc<dyn codegg_core::agent_convergence::ConvergenceStore> =
            Arc::new(codegg_core::agent_convergence::InMemoryConvergenceStore::new());
        run_control.set_group_service_sync(run_group_service.clone());
        Self {
            pool,
            memory_store,
            legacy_agent,
            agent_run_store,
            run_control,
            run_group_service,
            convergence_store,
            turn_runtime,
            lsp_service: None,
            workspace_services: None,
            worktree_service: Some(codegg_core::worktree_service::WorktreeService::memory(
                std::path::PathBuf::from(".codegg/worktrees"),
                Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
            )),
            // No pool means no durable permit store. The permit is a durable
            // authority; an in-memory one would be a second, weaker answer to
            // "who may write", which is exactly what this milestone is trying
            // not to create.
            writer_permit_service: None,
            workspace_service_policy: WorkspaceServicePolicy::default(),
            job_store,
            schedule_store,
            recovery_policy: RecoveryPolicy::default(),
            daemon_generation: DaemonGeneration::new(),
            scheduler: None,
            submission: None,
            job_notification_service: None,
            ci_watch_service: None,
            continuation_service: None,
            ci_watch_driver: None,
            scheduler_config: crate::scheduler::config::ResolvedSchedulerConfig::default(),
            connection_manager: None,
        }
    }

    /// Construct `CoreRuntimeDeps` with production SQLite-backed stores.
    ///
    /// This is the preferred constructor for daemons that have a pool.
    pub fn with_jobs(
        pool: sqlx::SqlitePool,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
    ) -> Self {
        let job_store: Arc<dyn JobStore> = Arc::new(SqliteJobStore::new(pool.clone()));
        let schedule_store: Arc<dyn ScheduleStore> = Arc::new(SqliteScheduleStore::new(
            pool.clone(),
            Arc::clone(&job_store),
        ));
        let connection_manager = default_connection_manager(&pool);
        let agent_run_store: Arc<dyn codegg_core::agent_run::AgentRunStore> = Arc::new(
            codegg_core::agent_run::SqliteAgentRunStore::new(pool.clone()),
        );
        let run_control = crate::agent::run_control::RunControlService::with_pool(
            agent_run_store.clone(),
            pool.clone(),
        );
        let run_group_service = codegg_core::agent_run_group::AgentRunGroupService::with_pool(
            agent_run_store.clone(),
            pool.clone(),
        );
        let convergence_store: Arc<dyn codegg_core::agent_convergence::ConvergenceStore> = Arc::new(
            codegg_core::agent_convergence::SqliteConvergenceStore::new(pool.clone()),
        );
        run_control.set_group_service_sync(run_group_service.clone());
        let worktree_service = codegg_core::worktree_service::WorktreeService::sqlite(
            pool.clone(),
            codegg_core::storage::DaemonPaths::default()
                .data_root()
                .join(codegg_core::worktree_service::DEFAULT_WORKTREE_DIRECTORY),
            Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
        )
        .with_owner_resolver(Arc::new(DurableWorktreeOwnerResolver {
            runs: agent_run_store.clone(),
        }));
        // M005 corrective C002: the permit service is the daemon's authority
        // for who may write to a managed checkout. It shares the daemon's pool
        // and the daemon's worktree service, so the scope it resolves is read
        // from the same authoritative records the rest of the daemon uses.
        let writer_permit_service = crate::core::writer_permit::WriterPermitService::new(
            codegg_core::writer_permit::WriterPermitStore::new(pool.clone()),
            crate::core::writer_permit::WorktreePermitAuthority::new(worktree_service.clone())
                .with_groups(run_group_service.clone()),
        );
        Self {
            pool: Some(pool.clone()),
            memory_store,
            legacy_agent: LegacyAgentRuntimeDeps { subagent_pool },
            agent_run_store: agent_run_store.clone(),
            run_control,
            run_group_service,
            convergence_store,
            turn_runtime: Arc::new(crate::agent::turn_runtime::DefaultTurnRuntime),
            lsp_service: None,
            workspace_services: None,
            worktree_service: Some(worktree_service),
            writer_permit_service: Some(writer_permit_service),
            workspace_service_policy: WorkspaceServicePolicy::default(),
            job_store,
            schedule_store,
            recovery_policy: RecoveryPolicy::default(),
            daemon_generation: DaemonGeneration::new(),
            scheduler: None,
            submission: None,
            job_notification_service: None,
            ci_watch_service: None,
            continuation_service: None,
            ci_watch_driver: None,
            scheduler_config: crate::scheduler::config::ResolvedSchedulerConfig::default(),
            connection_manager,
        }
    }

    /// Builder-style setter for the checkout writer permit service.
    ///
    /// M005 corrective C002. The daemon owns exactly one of these; the
    /// pool-backed constructor installs one by default.
    pub fn with_writer_permit_service(
        mut self,
        service: Arc<crate::core::writer_permit::WriterPermitService>,
    ) -> Self {
        self.writer_permit_service = Some(service);
        self
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

    pub fn with_run_control(
        mut self,
        run_control: Arc<crate::agent::run_control::RunControlService>,
    ) -> Self {
        self.run_control = run_control;
        self
    }

    /// Install the daemon-owned lazy provider-connection manager.
    pub fn with_connection_manager(
        mut self,
        manager: Arc<crate::core::provider_connections::ConnectionManager>,
    ) -> Self {
        self.connection_manager = Some(manager);
        self
    }
}
