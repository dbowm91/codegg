//! M003: `CoreDaemon` construction ownership.
//!
//! `CoreDaemon` remains the single daemon composition/lifecycle authority.
//! This module owns dependency construction only: it builds every daemon-owned
//! handle (event log + projection seam, notification router, workspace
//! registry, workspace services, worktree service, eggpool provisioner,
//! selection service, asset-refresh coordinator, project activation,
//! scheduler + submission, interactive protocol) in one deterministic order
//! and assembles the struct. It introduces no new store, scheduler, state
//! machine, service bus, actor/DI framework, or authority.
//!
//! ```text
//! Construction order (preserved verbatim from `daemon.rs`):
//!   1. daemon_id / generation identity
//!   2. event log (+ projection seam + maintenance task when pooled)
//!   3. notification router (+ audio arbiter when TTS enabled)
//!   4. workspace registry + context resolver
//!   5. workspace services registry (+ worktree service)
//!   6. eggpool provisioner (+ background refresh) + selection service
//!   7. daemon generation sync into deps
//!   8. asset-refresh coordinator + project activation registry
//!   9. scheduler (+ default executors + tool-program executor) + submission
//!  10. worktree reconcile task (pooled, when a runtime is present)
//!  11. struct assembly (single `Self { .. }`, no partial publish)
//! ```
//!
//! Construction failures unwind without publishing a partially ready daemon:
//! every step builds a local before the final assembly, so a panic or early
//! return never leaves a half-wired `Arc<CoreDaemon>` observable.

use std::sync::Arc;
use std::time::Instant;

use super::daemon::CoreDaemon;
use super::project_activation::{ProjectActivationPolicy, ProjectActivationRegistry};
use super::runtime_deps::CoreRuntimeDeps;
use crate::protocol::core::CoreEvent;
use codegg_core::context::ProjectContextResolver;
use codegg_core::workspace::WorkspaceRegistry;
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServiceRegistry,
};

/// Adapter bridging `EventLog`'s `ProjectionSink` trait to the
/// centralized `ProjectionPublicationSeam`. Spawned by the daemon
/// construction path when a SQLite pool is available.
struct SeamProjectionSink {
    inner: Arc<codegg_core::projection_replay::seam::ProjectionPublicationSeam>,
}

impl super::event_log::ProjectionSink for SeamProjectionSink {
    fn publish(
        &self,
        envelope: crate::protocol::core::EventEnvelope<CoreEvent>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>> {
        let seam = self.inner.clone();
        Box::pin(async move {
            let ctx = codegg_core::projection_replay::seam::ProjectionPublicationContext::default();
            if let Err(error) = seam.publish(&envelope, ctx).await {
                tracing::warn!(error = %error, "projection publication failed");
            }
        })
    }
}

impl CoreDaemon {
    /// Construct a `CoreDaemon` from a bundled [`CoreRuntimeDeps`].
    pub fn with_deps(deps: CoreRuntimeDeps) -> Self {
        let daemon_id = format!("codegg-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        Self::with_deps_and_identity(deps, daemon_id, uuid::Uuid::new_v4().to_string())
    }

    /// Construct a `CoreDaemon` with caller-provided `daemon_id` and
    /// `generation` strings. Used by the singleton-lifecycle path so the
    /// metadata on disk matches the daemon in memory.
    pub fn with_deps_and_identity(
        deps: CoreRuntimeDeps,
        daemon_id: String,
        generation: String,
    ) -> Self {
        let config = super::load_config_or_default();
        let capacity = config
            .daemon
            .as_ref()
            .and_then(|d| d.event_log_capacity)
            .unwrap_or(4096);
        let mut event_log = match deps.pool {
            Some(ref p) => super::event_log::EventLog::new_with_pool(capacity, p.clone()),
            None => super::event_log::EventLog::new(capacity),
        };

        // Install the projection replay publication seam when a SQLite pool
        // is available. The seam owns the replay store/service and routes
        // every published envelope into durable projection storage exactly once.
        let (projection_seam, projection_maintenance_handle) = if let Some(ref pool) = deps.pool {
            use codegg_core::project_storage::ProjectStorage;
            use codegg_core::projection_replay::seam::ProjectionPublicationSeam;
            use codegg_core::projection_replay::service::ProjectionReplayService;
            use codegg_core::projection_replay::store::ProjectionReplayStore;

            let replay_store = Arc::new(ProjectionReplayStore::new(pool.clone()));
            let replay_service = Arc::new(ProjectionReplayService::new(replay_store));
            let project_storage = Arc::new(ProjectStorage::new(pool.clone()));
            let seam = Arc::new(ProjectionPublicationSeam::with_project_storage(
                replay_service,
                project_storage,
            ));
            let sink = Arc::new(SeamProjectionSink {
                inner: seam.clone(),
            });
            event_log.install_projection_sink(sink);

            // Spawn a background maintenance task for retention/checkpointing
            let maintenance_seam = Arc::clone(&seam);
            let handle = tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                loop {
                    interval.tick().await;
                    let now = chrono::Utc::now().timestamp_millis();
                    if let Err(error) = maintenance_seam.service().maintenance_tick(now).await {
                        tracing::warn!(error = %error, "projection replay maintenance tick failed");
                    }
                }
            });

            (Some(seam), Some(handle))
        } else {
            (None, None)
        };

        let event_log = Arc::new(event_log);
        let notification_router = Arc::new(super::notification::NotificationRouter::new(
            super::notification::NotificationPolicy::from_config(&config),
        ));
        let audio_arbiter = if notification_router.is_tts_enabled() {
            let arbiter = Arc::new(super::notification::AudioArbiter::new(Arc::clone(
                &notification_router,
            )));
            arbiter.start();
            Some(arbiter)
        } else {
            None
        };
        // Workspace registry: prefer the on-disk SQLite store when a
        // session DB pool is available; fall back to an in-memory store
        // for standalone / in-process test daemons.
        let workspace_store: Arc<dyn codegg_core::workspace::WorkspaceStore> = match deps.pool {
            Some(ref p) => Arc::new(codegg_core::workspace::SqliteWorkspaceStore::new(p.clone())),
            None => Arc::new(codegg_core::workspace::InMemoryWorkspaceStore::new()),
        };
        let workspaces = WorkspaceRegistry::new_for_tests(workspace_store.clone());
        let context_resolver = deps.pool.clone().map(|pool| {
            Arc::new(ProjectContextResolver::new(
                codegg_core::project_storage::ProjectStorage::new(pool.clone()),
                codegg_core::project_catalog::ProjectCatalog::new(pool),
                workspace_store.clone(),
            ))
        });

        // Phase 3: workspace services registry. Use the one supplied
        // via `with_workspace_services` if present; otherwise create
        // one with the production factory and the configured policy.
        let workspace_services = deps.workspace_services.clone().unwrap_or_else(|| {
            WorkspaceServiceRegistry::new(
                workspaces.clone(),
                Arc::new(ProductionWorkspaceServicesFactory),
                deps.workspace_service_policy.clone(),
            )
        });
        let worktree_service = deps.worktree_service.clone().unwrap_or_else(|| {
            codegg_core::worktree_service::WorktreeService::memory(
                std::path::PathBuf::from(".codegg/worktrees"),
                Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
            )
        });
        // Keep `deps.workspace_services` in sync so callers reading the
        // field observe the active registry.
        let mut deps = deps;
        let eggpool_provisioner = deps
            .pool
            .clone()
            .map(crate::core::eggpool::EggpoolProvisioner::new)
            .map(Arc::new);
        if let Some(provisioner) = eggpool_provisioner.as_ref() {
            provisioner.start_background_refresh();
        }
        let selection_service = match deps.pool.clone() {
            Some(pool) => {
                let session_store = Arc::new(codegg_core::session::SessionStore::new(pool.clone()));
                let connection_store =
                    Arc::new(codegg_core::provider_connections::ProviderConnectionStore::new(pool));
                Some(Arc::new(
                    crate::core::session_selection::SelectionService::new(
                        session_store,
                        connection_store,
                        eggpool_provisioner.clone(),
                    ),
                ))
            }
            None => None,
        };
        // The durable attempt generation must be the same identity that
        // owns this daemon. Otherwise restart recovery cannot distinguish
        // work from the current process from work left by a prior one.
        deps.daemon_generation =
            codegg_core::jobs::DaemonGeneration::new_unchecked(generation.clone());
        deps.workspace_services = Some(workspace_services.clone());

        let asset_builder = Arc::new(
            crate::agent::asset_snapshot_builder::ProjectAssetSnapshotBuilder::new(
                crate::agent::asset_snapshot_builder::SnapshotBuilderConfig::default(),
                Arc::new(config.clone()),
            ),
        );
        let asset_refresh = Arc::new(crate::agent::asset_refresh::AssetRefreshCoordinator::new(
            asset_builder,
        ));
        let project_activation = ProjectActivationRegistry::new(
            workspace_services.clone(),
            ProjectActivationPolicy::default(),
        );

        // Phase 5: global admission control scheduler. Daemon-owned work
        // is scheduler-authoritative by default; an explicitly disabled
        // scheduler produces a placeholder that rejects heavy submission.
        let scheduler_config = match crate::scheduler::config::ResolvedSchedulerConfig::from_input(
            config.scheduler.as_ref(),
        ) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(error = %error, "invalid scheduler config; using defaults");
                Default::default()
            }
        };
        let (scheduler, should_spawn_scheduler) = if let Some(existing) = deps.scheduler.clone() {
            (existing, false)
        } else if scheduler_config.enabled {
            let scheduler_arc = crate::scheduler::JobScheduler::new(
                deps.job_store.clone(),
                workspace_services.clone(),
                scheduler_config.clone(),
                deps.daemon_generation.clone(),
            );
            // Register the default executor set synchronously.
            if let Err(error) = scheduler_arc
                .register_default_executors_sync_with_agent_runs_and_control_and_worktree(
                    deps.legacy_agent.subagent_pool.clone(),
                    Some(deps.agent_run_store.clone()),
                    Some(deps.run_control.clone()),
                    Some(worktree_service.clone()),
                )
            {
                tracing::error!(
                    ?error,
                    "USER ACTION REQUIRED: scheduler degraded - default executor registration \
                     failed. Durable jobs and subagent dispatch will be unavailable. \
                     Check provider connectivity and restart the daemon."
                );
            }
            // SchedulerEvent has no corresponding CoreEvent protocol variant.
            // Leave the optional sink unset rather than attaching a sender whose
            // receiver would be dropped and falsely reporting a live event path.
            (scheduler_arc, true)
        } else {
            // Even when disabled, build a placeholder scheduler so
            // snapshots and introspection still work.
            (
                crate::scheduler::JobScheduler::new(
                    deps.job_store.clone(),
                    workspace_services.clone(),
                    scheduler_config.clone(),
                    deps.daemon_generation.clone(),
                ),
                false,
            )
        };
        deps.scheduler = Some(scheduler.clone());
        let run_control = deps.run_control.clone();
        // The scheduler is allowed to cancel the owned job after the durable
        // mailbox intent has been committed.
        run_control.set_scheduler_sync(scheduler.clone());
        deps.scheduler_config = scheduler_config.clone();
        if let Err(error) = scheduler.configure_agent_run_store_sync(deps.agent_run_store.clone()) {
            tracing::error!(
                ?error,
                "failed to wire durable agent-run store into scheduler"
            );
        }
        let submission = match deps.pool.clone() {
            Some(pool) => crate::scheduler::JobSubmissionService::new_with_goal_store(
                deps.job_store.clone(),
                scheduler.clone(),
                workspace_services.clone(),
                deps.daemon_generation.clone(),
                Arc::new(codegg_core::goal::GoalStore::new(pool)),
            ),
            None => crate::scheduler::JobSubmissionService::new(
                deps.job_store.clone(),
                scheduler.clone(),
                workspace_services.clone(),
                deps.daemon_generation.clone(),
            ),
        };
        deps.submission = Some(submission.clone());
        if let Some(pool) = deps.legacy_agent.subagent_pool.as_ref() {
            pool.configure_durable_delegation(
                submission.clone(),
                deps.agent_run_store.clone(),
                deps.run_control.clone(),
                deps.run_group_service.clone(),
            );
        }

        // The Tool Program executor needs the daemon-owned submission facade
        // for nested child jobs. Install it only after the scheduler exists,
        // but before the scheduler loop is spawned, so production execution
        // cannot fall back to an executor with no child-job authority.
        if scheduler_config.enabled {
            let notification_service = Arc::new(match deps.pool.clone() {
                Some(pool) => crate::scheduler::tool_program_notifications::ToolProgramNotificationService::with_pool(pool),
                None => crate::scheduler::tool_program_notifications::ToolProgramNotificationService::new(),
            });
            if let Err(error) = scheduler.register_executor_sync(Arc::new(
                crate::scheduler::tool_program_executor::ToolProgramExecutor::default()
                    .with_submission(submission)
                    .with_notification_service(notification_service),
            )) {
                tracing::debug!(
                    ?error,
                    "tool program executor already supplied by scheduler"
                );
            }
        }
        if should_spawn_scheduler {
            let _handle = scheduler.spawn_run();
        }

        let worktree_reconcile_handle = if deps.pool.is_some() {
            tokio::runtime::Handle::try_current().ok().map(|handle| {
                let service = worktree_service.clone();
                handle.spawn(async move {
                    if let Err(error) = service.reconcile_all().await {
                        tracing::warn!(?error, "managed worktree startup reconciliation failed");
                    }
                })
            })
        } else {
            None
        };

        Self {
            daemon_id,
            generation,
            pool: deps.pool.clone(),
            collaboration: Arc::new(
                codegg_core::collaboration::CollaborationService::with_defaults(deps.pool.clone()),
            ),
            deps,
            event_log,
            sessions: Arc::new(crate::core::session_runtime::SessionRuntimeRegistry::new()),
            clients: Arc::new(super::client_registry::ClientRegistry::new()),
            notification_router,
            audio_arbiter,
            started_at: Instant::now(),
            workspaces,
            context_resolver,
            workspace_services,
            worktree_service,
            _worktree_reconcile_handle: worktree_reconcile_handle,
            eggpool_provisioner,
            selection_service,
            asset_refresh,
            project_activation,
            presence: Arc::new(codegg_core::presence::PresenceService::default()),
            projection_seam,
            _projection_maintenance_handle: projection_maintenance_handle,
            dropped_event_bridge_events: std::sync::atomic::AtomicU64::new(0),
            // M002: share the scheduler's admission controller so
            // interactive spawns draw from the same process-slot
            // accounting as durable work (M001 permit contract).
            interactive_processes: Arc::new(
                crate::interactive_process_attach::InteractiveProcessProtocol::new(
                    scheduler.admission().clone(),
                ),
            ),
        }
    }

    /// Legacy constructor for backward compatibility. Prefer `with_deps`.
    pub fn new(
        pool: Option<sqlx::SqlitePool>,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
    ) -> Self {
        Self::with_deps(CoreRuntimeDeps::new(pool, subagent_pool, memory_store))
    }

    /// Canonical construction phase names in execution order.
    ///
    /// Pinned by `construction_phases_match_documented_order` so a future
    /// reorder is an explicit, reviewed change rather than silent drift.
    #[cfg(test)]
    pub(crate) fn construction_phase_names() -> [&'static str; 11] {
        [
            "identity",
            "event_log_and_projection_seam",
            "notification_router",
            "workspace_registry",
            "workspace_services",
            "eggpool_and_selection",
            "daemon_generation_sync",
            "asset_refresh_and_activation",
            "scheduler_and_submission",
            "worktree_reconcile",
            "assembly",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn in_memory_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:daemon_construct_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect in-memory sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    #[test]
    fn construction_phases_match_documented_order() {
        assert_eq!(
            CoreDaemon::construction_phase_names(),
            [
                "identity",
                "event_log_and_projection_seam",
                "notification_router",
                "workspace_registry",
                "workspace_services",
                "eggpool_and_selection",
                "daemon_generation_sync",
                "asset_refresh_and_activation",
                "scheduler_and_submission",
                "worktree_reconcile",
                "assembly",
            ]
        );
    }

    #[tokio::test]
    async fn construction_wires_scheduler_generation_and_services() {
        let pool = in_memory_pool().await;
        let generation = uuid::Uuid::new_v4().to_string();
        let daemon = CoreDaemon::with_deps_and_identity(
            crate::core::runtime_deps::CoreRuntimeDeps::new(Some(pool.clone()), None, None),
            "codegg-test".to_string(),
            generation.clone(),
        );
        assert_eq!(daemon.generation, generation);
        assert_eq!(daemon.deps.daemon_generation.as_str(), generation.as_str());
        assert!(daemon.deps.scheduler.is_some());
        assert!(daemon.deps.submission.is_some());
        assert!(daemon.deps.workspace_services.is_some());
        assert!(daemon.projection_seam.is_some());
    }

    #[tokio::test]
    async fn construction_pool_less_has_no_seam_or_handles() {
        let daemon = CoreDaemon::new(None, None, None);
        assert!(daemon.pool.is_none());
        assert!(daemon.projection_seam.is_none());
        assert!(daemon.context_resolver.is_none());
        assert!(daemon.selection_service.is_none());
        assert!(daemon.eggpool_provisioner.is_none());
        assert!(daemon.deps.scheduler.is_some());
        assert!(daemon.deps.submission.is_some());
    }

    #[tokio::test]
    async fn construction_preserves_identity_and_unique_ids() {
        let d1 = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::new(
            None, None, None,
        ));
        let d2 = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::new(
            None, None, None,
        ));
        assert_ne!(d1.daemon_id, d2.daemon_id);
        assert_ne!(d1.generation, d2.generation);
        let daemon = CoreDaemon::with_deps_and_identity(
            crate::core::runtime_deps::CoreRuntimeDeps::new(None, None, None),
            "codegg-fixed".to_string(),
            "gen-fixed".to_string(),
        );
        assert_eq!(daemon.daemon_id, "codegg-fixed");
        assert_eq!(daemon.generation, "gen-fixed");
        assert_eq!(daemon.deps.daemon_generation.as_str(), "gen-fixed");
    }

    #[tokio::test]
    async fn construction_reuses_supplied_workspace_services() {
        let pool = in_memory_pool().await;
        let first = CoreDaemon::new(Some(pool.clone()), None, None);
        let supplied = first.workspace_services.clone();
        let mut deps = crate::core::runtime_deps::CoreRuntimeDeps::new(Some(pool), None, None);
        deps.workspace_services = Some(supplied.clone());
        let second = CoreDaemon::with_deps(deps);
        assert!(Arc::ptr_eq(&second.workspace_services, &supplied));
    }
}
