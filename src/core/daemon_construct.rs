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

fn install_job_lifecycle_projection(
    scheduler: &Arc<crate::scheduler::JobScheduler>,
    submission: &Arc<crate::scheduler::JobSubmissionService>,
    job_store: Arc<dyn codegg_core::jobs::JobStore>,
    event_log: Arc<super::event_log::EventLog>,
) {
    let (created_tx, mut created_rx) = tokio::sync::mpsc::channel(256);
    submission.set_created_sink(created_tx);
    let created_log = event_log.clone();
    tokio::spawn(async move {
        while let Some(job) = created_rx.recv().await {
            created_log
                .publish(
                    job.session_id.clone(),
                    job.turn_id.clone(),
                    CoreEvent::JobCreated {
                        job_id: job.job_id.as_str().to_string(),
                        workspace_id: job.workspace_id.to_string(),
                        kind: job.kind.as_str().to_string(),
                        session_id: job.session_id.clone(),
                        turn_id: job.turn_id.clone(),
                    },
                )
                .await;
        }
    });

    let (scheduler_tx, mut scheduler_rx) = tokio::sync::mpsc::channel(256);
    if !scheduler.set_event_sink_blocking(scheduler_tx) {
        tracing::error!("failed to attach scheduler lifecycle projection event sink");
        return;
    }
    tokio::spawn(async move {
        while let Some(event) = scheduler_rx.recv().await {
            use crate::scheduler::events::SchedulerEvent;
            let (job_id, attempt_id, started, terminal_state) = match event {
                SchedulerEvent::JobAdmitted {
                    job_id, attempt_id, ..
                } => (job_id, attempt_id.to_string(), true, None),
                SchedulerEvent::JobResourceReleased { job_id, attempt_id } => {
                    (job_id, attempt_id.to_string(), false, None)
                }
                SchedulerEvent::JobTerminal { job_id, state } => {
                    (job_id, String::new(), false, Some(state))
                }
                _ => continue,
            };
            let id = codegg_core::jobs::JobId::new_unchecked(&job_id);
            let record = match job_store.get_job(&id).await {
                Ok(Some(record)) => record,
                Ok(None) => {
                    tracing::warn!(%job_id, "scheduler lifecycle event referenced a missing job");
                    continue;
                }
                Err(error) => {
                    tracing::warn!(%job_id, %error, "failed to resolve owner for scheduler lifecycle event");
                    continue;
                }
            };
            let payload = if started {
                CoreEvent::JobStarted {
                    job_id: job_id.clone(),
                    attempt_id,
                }
            } else {
                match terminal_state.unwrap_or(record.state) {
                    codegg_core::jobs::JobState::Completed => CoreEvent::JobCompleted {
                        job_id: job_id.clone(),
                        attempt_id,
                    },
                    codegg_core::jobs::JobState::Failed => CoreEvent::JobFailed {
                        job_id: job_id.clone(),
                        attempt_id,
                        error_class: "failed".into(),
                        message: String::new(),
                    },
                    codegg_core::jobs::JobState::Cancelled => CoreEvent::JobCancelled {
                        job_id: job_id.clone(),
                        attempt_id,
                    },
                    codegg_core::jobs::JobState::TimedOut => CoreEvent::JobTimedOut {
                        job_id: job_id.clone(),
                        attempt_id,
                    },
                    codegg_core::jobs::JobState::Interrupted => {
                        let attempt_id = codegg_core::jobs::AttemptId::new_unchecked(&attempt_id);
                        let recovery_generation = match job_store.list_attempts(&id).await {
                            Ok(attempts) => attempts
                                .into_iter()
                                .find(|attempt| attempt.attempt_id == attempt_id)
                                .map(|attempt| attempt.daemon_generation.to_string()),
                            Err(error) => {
                                tracing::warn!(%job_id, %error, "failed to resolve interrupted job generation");
                                None
                            }
                        };
                        let Some(recovery_generation) = recovery_generation else {
                            tracing::warn!(%job_id, attempt_id = %attempt_id, "interrupted job event has no durable attempt generation");
                            continue;
                        };
                        CoreEvent::JobInterrupted {
                            job_id: job_id.clone(),
                            attempt_id: attempt_id.to_string(),
                            recovery_generation,
                        }
                    }
                    _ => continue,
                }
            };
            event_log
                .publish(record.session_id, record.turn_id, payload)
                .await;
        }
    });
}

impl super::event_log::ProjectionSink for SeamProjectionSink {
    fn publish(
        &self,
        envelope: crate::protocol::core::EventEnvelope<CoreEvent>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>> {
        let seam = self.inner.clone();
        Box::pin(async move {
            let ctx = codegg_core::projection_replay::seam::ProjectionPublicationContext::default();
            // Bounded retry on SQLite lock contention only. The daemon's
            // single-file pool runs several connections against one WAL
            // database, so a writer can collide with a reader holding a
            // snapshot. Losing that event silently is what left the
            // projection permanently short of job and subagent lifecycle
            // events — the frontend then had nothing to show even though the
            // work was real. Anything else is a genuine failure and is
            // reported immediately rather than retried into a stall.
            let mut attempt = 0;
            loop {
                match seam.publish(&envelope, ctx.clone()).await {
                    Ok(_) => return,
                    Err(error) => {
                        attempt += 1;
                        if attempt >= PROJECTION_PUBLISH_ATTEMPTS || !is_transient_busy(&error) {
                            tracing::warn!(
                                error = %error,
                                attempt,
                                "projection publication failed"
                            );
                            return;
                        }
                        tokio::time::sleep(PROJECTION_PUBLISH_BACKOFF).await;
                    }
                }
            }
        })
    }
}

/// Total publish attempts, including the first. Three is enough to ride out
/// a checkpoint-sized write without turning a stuck database into a stall.
const PROJECTION_PUBLISH_ATTEMPTS: u32 = 3;

/// Fixed pause between retries. Short enough that a subscriber notices no
/// meaningful delay, long enough not to spin against the same contention.
const PROJECTION_PUBLISH_BACKOFF: std::time::Duration = std::time::Duration::from_millis(50);

/// `true` when a storage error is transient SQLite lock contention that a
/// bounded retry may resolve. Mirrors the work-order store's classifier so
/// both subsystems agree on what "busy" means.
fn is_transient_busy(error: &codegg_core::error::StorageError) -> bool {
    match error {
        codegg_core::error::StorageError::Database(message) => {
            let lower = message.to_lowercase();
            // SQLITE_BUSY (5) surfaces as "database is locked"; the WAL
            // snapshot-conflict variant (517) surfaces as "database table is
            // locked". Neither is retried by busy_timeout itself, which is
            // why the pool's 30s timeout does not save these.
            lower.contains("database is locked")
                || lower.contains("database table is locked")
                || lower.contains("database is busy")
        }
        _ => false,
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

        // M006: the daemon-held scratch census. The walk is a bounded
        // filesystem measurement, so it runs on a background timer and never
        // on a request path or an agent hot path.
        let scratch_census = Arc::new(super::daemon_scratch::ScratchCensus::new());
        let scratch_census_handle =
            super::daemon_scratch::spawn_census_refresh(Arc::clone(&scratch_census));

        // M007: the scratch retention sweep. It reclaims the daemon's own
        // disposable trees, so it needs the durable session set, the run
        // store, and the job store. A daemon without a pool has no durable
        // state to sweep against, so it never starts one.
        let scratch_sweep_policy = super::scratch_sweep::ScratchRetentionPolicy::from(
            config
                .scratch
                .as_ref()
                .unwrap_or(&codegg_config::ScratchConfig::default()),
        );
        let scratch_sweep_handle = match (deps.pool.clone(), deps.agent_run_store.clone()) {
            (Some(pool), runs) => Some(super::scratch_sweep::spawn_scratch_sweep(
                pool,
                runs,
                deps.job_store.clone(),
                scratch_sweep_policy,
            )),
            (None, _) => None,
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
            crate::agent::asset_snapshot_builder::ProjectAssetSnapshotBuilder::new(Arc::new(
                config.clone(),
            )),
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
        // Per-node ceilings on concurrently admitted remote executions.
        //
        // A remote job does not consume the controller's local CPU, memory,
        // or process budget — those tokens describe the *node's* capacity.
        // It consumes a node-scoped reservation instead, bounded here.
        // A node with no configured ceiling falls back to the controller's
        // process-slot budget, which preserves the effective concurrency
        // bound remote work had before the budgets were split.
        let scheduler_config = scheduler_config.with_remote_node_capacity(
            config
                .eggwork
                .as_ref()
                .and_then(|eggwork| eggwork.nodes.as_ref())
                .map(|nodes| {
                    nodes
                        .iter()
                        .filter_map(|(table_key, profile)| {
                            profile
                                .max_concurrent_executions
                                .map(|limit| (table_key.clone(), limit))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        );
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
            // Registration succeeding is not the same as covering every
            // dispatchable job kind. A kind with no executor is marked
            // unschedulable at dispatch, and its submitting tool then blocks
            // until its wait bound expires and reports a bogus timeout — the
            // `python_script` stall this guards against. Surface the gap at
            // startup, where it is actionable, instead of mid-turn.
            {
                let registered = scheduler_arc.executor_kinds_sync();
                let missing: Vec<&'static str> =
                    crate::scheduler::executor::REQUIRED_EXECUTOR_KINDS
                        .iter()
                        .map(|kind| kind.as_str())
                        .filter(|name| {
                            !registered.is_empty()
                                && !registered.iter().any(|r| r.as_str() == *name)
                        })
                        .collect();
                if !missing.is_empty() {
                    tracing::error!(
                        missing = ?missing,
                        "USER ACTION REQUIRED: scheduler degraded - job kinds with no registered \
                         executor. Tool calls for these kinds will stall until their wait bound \
                         expires and then fail with a timeout. Restart the daemon."
                    );
                }
            }
            // The scheduler sink is installed with its daemon-owned lifecycle
            // projector below, after the shared submission boundary exists.
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
        // M001: resolve the daemon-owned remote-target admission policy.
        //
        // An explicitly injected policy is the caller's decision and is
        // never overridden. Otherwise the policy comes from the daemon
        // configuration, which approves no node until an administrator
        // names one — so a daemon that never opted in refuses every
        // remote target at the host boundary while local execution
        // keeps working unchanged.
        if deps.remote_target_policy.is_none() {
            let policy = match config.eggwork.as_ref() {
                Some(eggwork) => {
                    crate::core::remote_targets::RemoteTargetPolicy::from_config(eggwork)
                }
                None => crate::core::remote_targets::RemoteTargetPolicy::default(),
            };
            deps.remote_target_policy = Some(Arc::new(policy));
        }
        // Eggwork M002: resolve the daemon's node table exactly once.
        //
        // The same resolution feeds the read-only host-telemetry
        // coordinator and the fixed-target executor, so both agree on
        // which nodes exist and on their trust material. A resolution
        // failure leaves both surfaces empty rather than half-configured,
        // and it is logged once here instead of once per consumer.
        let mut eggwork_nodes = match config.eggwork.as_ref() {
            Some(eggwork) => {
                match crate::scheduler::eggwork::EggworkExecutorConfig::from_daemon_config(eggwork)
                {
                    Ok(exec_config) => Some(exec_config),
                    Err(error) => {
                        tracing::error!(
                            error = %error,
                            "USER ACTION REQUIRED: eggwork nodes not resolved - fix [eggwork.nodes] key material paths"
                        );
                        None
                    }
                }
            }
            None => None,
        };
        // The telemetry coordinator is daemon-owned and independent of job
        // dispatch, so it is installed whether or not the scheduler is
        // enabled.
        if let Some(exec_config) = eggwork_nodes.as_ref() {
            if !exec_config.nodes.is_empty() {
                deps.node_telemetry = Some(Arc::new(
                    crate::scheduler::eggwork_telemetry::NodeTelemetryService::new(
                        exec_config.nodes.clone(),
                        Arc::new(crate::scheduler::eggwork::NodeClientFactory),
                    ),
                ));
                tracing::info!(
                    node_count = exec_config.nodes.len(),
                    "eggwork node telemetry coordinator installed"
                );
            }
        }
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
        install_job_lifecycle_projection(
            &scheduler,
            &submission,
            deps.job_store.clone(),
            event_log.clone(),
        );
        // C001 daemon-scoped singletons: one notifier + one watcher for the
        // daemon lifetime, injected into every turn. Per-turn construction
        // remains only for legacy/test paths that never asked for the shared
        // handle. The scheduler's notification pointer is installed here (not
        // per turn) so turns cannot race it.
        let shared_notifier = deps.shared_job_notification_service();
        let shared_watches = deps.shared_ci_watch_service();
        // C004 daemon-scoped continuation store (fail-closed: owning the
        // store never starts turns; no coordinator dispatch is wired).
        let shared_continuation = deps.shared_continuation_service();
        scheduler.set_job_notification_service_sync(shared_notifier.clone());
        // C004 post-commit completion fanout: one unbounded channel per
        // daemon. The notifier sends each newly stored terminal here after
        // its durable commit; the forwarder below publishes the live
        // owner-scoped `BackgroundJobCompleted` hint. Duplicates never
        // send (see `record_notice`), so live delivery is exactly-once per
        // logical terminal. A full/closed channel never fails the terminal
        // path; durability is the notice row, not the hint.
        let (completion_tx, completion_rx) = tokio::sync::mpsc::unbounded_channel::<
            crate::scheduler::job_notifications::JobTerminalNotice,
        >();
        shared_notifier.set_completion_hook_sync(completion_tx);
        // C005 (ADR-0031) delegated-wake cell: published post-`Arc` via
        // `attach_idle_wake`. The forwarder below upgrades it per notice;
        // unset means notify-only (fail-closed).
        let idle_wake_cell: Arc<std::sync::OnceLock<std::sync::Weak<super::daemon::CoreDaemon>>> =
            Arc::new(std::sync::OnceLock::new());
        let idle_completion_handle = {
            let event_log_for_fanout = event_log.clone();
            let router_for_fanout = notification_router.clone();
            let wake_cell_for_fanout = idle_wake_cell.clone();
            tokio::runtime::Handle::try_current().ok().map(|handle| {
                handle.spawn(async move {
                    crate::core::daemon::CoreDaemon::forward_idle_completions(
                        completion_rx,
                        event_log_for_fanout,
                        router_for_fanout,
                        wake_cell_for_fanout,
                    )
                    .await;
                })
            })
        };
        // C001 idle driver: one earliest-due wake loop per pooled daemon.
        // Pool-less (in-memory/headless) daemons explicitly have no idle
        // persistence: `ci_watch_driver` stays `None` and idle advancement is
        // unavailable rather than falsely claimed.
        let (ci_watch_driver, ci_watch_driver_handle) = match deps.pool.clone() {
            Some(_) => {
                let driver = Arc::new(crate::scheduler::ci_watch_idle::CiWatchIdleDriver::new(
                    shared_watches.clone(),
                ));
                let handle = tokio::runtime::Handle::try_current()
                    .ok()
                    .map(|_| driver.spawn());
                deps.ci_watch_driver = Some(driver.clone());
                (Some(driver), handle)
            }
            None => (None, None),
        };
        // M004: the schedule materialization driver — one earliest-due
        // wake loop per pooled daemon with the scheduler enabled.
        //
        // The driver *creates* work; the scheduler *runs* it. It is gated
        // on the scheduler being enabled because the submission facade
        // refuses every submission when the scheduler is disabled, and on
        // a pool because a pool-less daemon has no cross-process
        // durability to claim. A schedule that never fires is worse than
        // an honest absence, so the driver is installed only where it
        // can actually do its job.
        let (schedule_driver, schedule_driver_handle) = match deps.pool.clone() {
            Some(_) if scheduler.is_enabled() => {
                let materializer =
                    crate::scheduler::schedule_materializer::ScheduleMaterializer::new(
                        submission.clone(),
                        deps.remote_target_policy.clone(),
                    );
                let driver = crate::scheduler::schedule_driver::ScheduleDriver::new(
                    deps.schedule_store.clone(),
                    materializer,
                );
                let handle = tokio::runtime::Handle::try_current()
                    .ok()
                    .map(|_| driver.spawn());
                tracing::info!("schedule materialization driver installed");
                (Some(driver), handle)
            }
            _ => (None, None),
        };
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
        //
        // `default()` builds no registry here on purpose: at daemon startup no
        // frontend session exists, so there is no workspace root to bind
        // against. `ToolProgramExecutor::job_tool_context` binds the dispatch
        // registry per job from the scheduler-owned lease root.
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
            // Fixed-target Eggwork remote execution (M001). The executor is
            // registered only when named nodes are configured; Eggwork jobs
            // without a matching profile fail closed at execution time.
            //
            // The node table was resolved once above, where the telemetry
            // coordinator took its share of the same resolution.
            if let Some(exec_config) = eggwork_nodes.take() {
                if exec_config.nodes.is_empty() {
                    tracing::info!("eggwork executor not registered: no nodes configured");
                } else {
                    let count = exec_config.nodes.len();
                    let executor = crate::scheduler::eggwork::EggworkExecutor::new(
                        exec_config.with_store(deps.job_store.clone()),
                    );
                    if let Err(error) = scheduler.register_executor_sync(Arc::new(executor)) {
                        tracing::warn!(?error, "eggwork executor registration failed");
                    } else {
                        tracing::info!(node_count = count, "eggwork executor registered");
                    }
                }
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
            documents: Arc::new(crate::document_service::DocumentService::new()),
            directory_listing_permits: Arc::new(tokio::sync::Semaphore::new(1)),
            directory_listing_probe: Arc::new(super::daemon_documents::DirectoryListingProbe::new()),
            project_init_drafts: Arc::new(super::project_init::ProjectInitDraftRegistry::new()),
            work_orders: Arc::new(codegg_core::work_order::WorkOrderService::with_defaults(
                deps.pool.clone(),
            )),
            deps,
            event_log,
            lsp_diagnostics: Arc::new(parking_lot::Mutex::new(
                super::lsp_diagnostics_store::LspDiagnosticsStore::new(),
            )),
            sessions: Arc::new(crate::core::session_runtime::SessionRuntimeRegistry::new()),
            queue_schedule_changed: Arc::new(tokio::sync::Notify::new()),
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
            ci_watch_driver,
            _ci_watch_driver_handle: ci_watch_driver_handle,
            schedule_driver,
            _schedule_driver_handle: schedule_driver_handle,
            continuation_service: Some(shared_continuation),
            _idle_completion_handle: idle_completion_handle,
            idle_wake_cell,
            dropped_event_bridge_events: std::sync::atomic::AtomicU64::new(0),
            // M002: share the scheduler's admission controller so
            // interactive spawns draw from the same process-slot
            // accounting as durable work (M001 permit contract).
            interactive_processes: Arc::new(
                crate::interactive_process_attach::InteractiveProcessProtocol::new(
                    scheduler.admission().clone(),
                ),
            ),
            human_shell_runs: Arc::new(crate::shell::daemon::HumanShellRunService::new(
                scheduler.admission().clone(),
            )),
            scratch_census,
            _scratch_census_handle: Some(scratch_census_handle),
            _scratch_sweep_handle: scratch_sweep_handle,
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
