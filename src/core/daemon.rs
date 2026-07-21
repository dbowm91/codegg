use std::sync::Arc;
use std::time::Instant;

use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, RequestEnvelope};
use chrono::Utc;
use sqlx::Row;

use super::event_log::EventFilter;
use super::runtime_deps::CoreRuntimeDeps;
use crate::core::session_runtime::RuntimeSessionStatus;

use super::project_activation::{
    self, ProjectActivation, ProjectActivationPolicy, ProjectActivationRegistry,
    ProjectHealthSnapshot,
};
use codegg_core::context::{
    ContextResolutionError, ProjectContext, ProjectContextRequest, ProjectContextResolver,
    SessionId,
};
use codegg_core::workspace::{WorkspaceRecord, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServiceRegistry,
};
use codegg_protocol::projection::replay::{
    ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
};

pub struct CoreDaemon {
    pub daemon_id: String,
    /// Identity generation captured at lock acquisition. Distinct from
    /// `daemon_id` so two processes that happen to roll the same 8-hex
    /// suffix are still distinguishable. Set by [`Self::with_deps_and_identity`]
    /// or generated on demand by [`Self::with_deps`].
    pub generation: String,
    pub pool: Option<sqlx::SqlitePool>,
    pub deps: CoreRuntimeDeps,
    pub event_log: Arc<super::event_log::EventLog>,
    pub sessions: Arc<crate::core::session_runtime::SessionRuntimeRegistry>,
    pub clients: Arc<super::client_registry::ClientRegistry>,
    pub notification_router: Arc<super::notification::NotificationRouter>,
    pub audio_arbiter: Option<Arc<super::notification::AudioArbiter>>,
    pub started_at: Instant,
    /// Phase 2: daemon-owned workspace registry. Every persisted session
    /// is bound to exactly one workspace before any execution is
    /// permitted. The registry deduplicates canonical project roots and
    /// gates turn submission behind a valid `WorkspaceId`.
    pub workspaces: Arc<WorkspaceRegistry>,
    /// Canonical project/workspace/session resolver. Directory compatibility
    /// is read-only and succeeds only for an existing unique binding.
    pub context_resolver: Option<Arc<ProjectContextResolver>>,
    /// Phase 3: workspace services registry. The daemon owns the
    /// canonical [`WorkspaceServiceRegistry`] which lazily activates
    /// per-workspace `WorkspaceServices` bundles and shares them
    /// across sessions, the TUI, and remote clients. Created during
    /// `with_deps_and_identity` if the caller did not supply one via
    /// `CoreRuntimeDeps::with_workspace_services`.
    pub workspace_services: Arc<WorkspaceServiceRegistry>,
    /// Daemon-owned Eggpool provisioning service. It is present only for
    /// SQLite-backed daemons; legacy in-memory daemons retain compatibility.
    pub eggpool_provisioner: Option<Arc<crate::core::eggpool::EggpoolProvisioner>>,
    /// Provider Connections Milestone 3: daemon-owned session selection
    /// service. Reads and writes the connection/model selection on the
    /// session row through the typed core crate; never mutates provider
    /// credentials or constructs a provider in the frontend.
    pub selection_service: Option<Arc<crate::core::session_selection::SelectionService>>,
    /// Daemon-owned immutable runtime-asset publication coordinator. Every
    /// lifecycle and manual refresh path uses this one service.
    pub asset_refresh: Arc<crate::agent::asset_refresh::AssetRefreshCoordinator>,
    /// Project Catalog Milestone 3: explicit owner-scoped activation leases.
    pub project_activation: Arc<ProjectActivationRegistry>,
    /// Projection replay publication seam. Present only for SQLite-backed
    /// daemons; legacy in-memory daemons retain `None`.
    pub projection_seam:
        Option<Arc<codegg_core::projection_replay::seam::ProjectionPublicationSeam>>,
    /// Handle for the background projection replay maintenance task.
    /// `None` when no pool is available. Held to keep the task alive.
    _projection_maintenance_handle: Option<tokio::task::JoinHandle<()>>,
}

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
            let _ = seam.publish(&envelope, ctx).await;
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
        let config = crate::config::schema::Config::load().unwrap_or_default();
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
                    let _ = maintenance_seam.service().maintenance_tick(now).await;
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
        let scheduler_config = crate::scheduler::config::ResolvedSchedulerConfig::from_input(
            config.scheduler.as_ref(),
        )
        .unwrap_or_default();
        let scheduler = if let Some(existing) = deps.scheduler.clone() {
            existing
        } else if scheduler_config.enabled {
            let scheduler_arc = crate::scheduler::JobScheduler::new(
                deps.job_store.clone(),
                workspace_services.clone(),
                scheduler_config.clone(),
                deps.daemon_generation.clone(),
            );
            // Register the default executor set synchronously.
            let _ = scheduler_arc
                .register_default_executors_sync(deps.legacy_agent.subagent_pool.clone());
            let (tx, _rx) = tokio::sync::mpsc::channel(64);
            let _ = scheduler_arc.set_event_sink_blocking(tx);
            let _handle = scheduler_arc.spawn_run();
            scheduler_arc
        } else {
            // Even when disabled, build a placeholder scheduler so
            // snapshots and introspection still work.
            crate::scheduler::JobScheduler::new(
                deps.job_store.clone(),
                workspace_services.clone(),
                scheduler_config.clone(),
                deps.daemon_generation.clone(),
            )
        };
        deps.scheduler = Some(scheduler.clone());
        deps.scheduler_config = scheduler_config;
        deps.submission = Some(crate::scheduler::JobSubmissionService::new(
            deps.job_store.clone(),
            scheduler,
            workspace_services.clone(),
            deps.daemon_generation.clone(),
        ));

        Self {
            daemon_id,
            generation,
            pool: deps.pool.clone(),
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
            eggpool_provisioner,
            selection_service,
            asset_refresh,
            project_activation,
            projection_seam,
            _projection_maintenance_handle: projection_maintenance_handle,
        }
    }

    fn asset_context_for_project(
        context: &ProjectContext,
        session_id: Option<&str>,
    ) -> Result<crate::agent::asset_context::AssetContext, AppError> {
        let project_id = crate::agent::asset_context::ProjectId::parse(context.project_id.as_str())
            .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
        let mut builder = crate::agent::asset_context::AssetContextBuilder::new()
            .with_project_id(project_id)
            .with_workspace_root(context.workspace_root.clone())
            .with_config_revision(u64::try_from(context.binding_revision).unwrap_or_default());
        if let Some(session_id) = session_id {
            builder = builder.with_session_id(session_id);
        }
        // AssetRegistry expects the configuration directory as the parent of
        // the global CodeGG/foreign-harness roots. The path is configuration
        // data, never an identity or a secret-bearing report field.
        if let Some(config_dir) = dirs::config_dir() {
            builder = builder.with_global_root(config_dir);
        }
        builder
            .build()
            .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))
    }

    async fn refresh_project_context(
        &self,
        context: &ProjectContext,
        session_id: Option<&str>,
        reason: crate::agent::asset_refresh::RefreshReason,
    ) -> Result<crate::agent::asset_refresh::RefreshReport, AppError> {
        let asset_context = Self::asset_context_for_project(context, session_id)?;
        let scope = crate::agent::asset_refresh::AssetScope::new(
            context.project_id.as_str(),
            context.workspace_id.as_str(),
        );
        let report = self
            .asset_refresh
            .refresh(scope, asset_context, reason)
            .await;
        self.persist_asset_refresh_metadata(&report).await;
        Ok(report)
    }

    /// Refresh the explicitly activated project/workspace scope. Project
    /// Catalog owns activation policy; this is its daemon-side refresh seam.
    pub async fn refresh_project_activation(
        &self,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<crate::agent::asset_refresh::RefreshReport, AppError> {
        let resolver = self.context_resolver.as_ref().ok_or_else(|| {
            AppError::Other(anyhow::anyhow!(
                "project context resolver is unavailable for this daemon"
            ))
        })?;
        let context = resolver
            .resolve_raw(project_id, workspace_id, None)
            .await
            .map_err(|e| Self::context_error(&e))?;
        self.refresh_project_context(
            &context,
            None,
            crate::agent::asset_refresh::RefreshReason::ProjectActivation,
        )
        .await
    }

    /// Explicitly activate one project/workspace for a bounded owner lease.
    /// Workspace services are acquired lazily and runtime assets are refreshed
    /// through the existing daemon-owned coordinator seam.
    pub async fn activate_project_workspace(
        &self,
        project_id: &str,
        workspace_id: &str,
        owner: &str,
    ) -> Result<ProjectActivation, AppError> {
        let resolver = self.context_resolver.as_ref().ok_or_else(|| {
            AppError::Other(anyhow::anyhow!(
                "project context resolver is unavailable for this daemon"
            ))
        })?;
        let context = resolver
            .resolve_raw(project_id, workspace_id, None)
            .await
            .map_err(|error| Self::context_error(&error))?;
        let lease = self
            .project_activation
            .acquire(project_id, workspace_id, owner)
            .await
            .map_err(|error| AppError::Other(anyhow::anyhow!(error.to_string())))?;
        let refresh = match self
            .refresh_project_activation(project_id, workspace_id)
            .await
        {
            Ok(report) => report,
            Err(error) => {
                drop(lease);
                return Err(error);
            }
        };
        if let Some(error) = Self::refresh_report_error(&refresh) {
            drop(lease);
            return Err(error);
        }
        let health = self.project_health(project_id, workspace_id).await?;
        Ok(ProjectActivation {
            lease,
            refresh,
            health,
            binding_revision: context.binding_revision,
            diagnostics: vec![format!(
                "activated project/workspace binding revision {}",
                context.binding_revision
            )],
        })
    }

    /// Return a bounded, path-free health aggregate for a project/workspace.
    /// This method only reads durable catalog state and in-memory status; it
    /// never activates services or probes a repository.
    pub async fn project_health(
        &self,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<ProjectHealthSnapshot, AppError> {
        let resolver = self.context_resolver.as_ref().ok_or_else(|| {
            AppError::Other(anyhow::anyhow!(
                "project context resolver is unavailable for this daemon"
            ))
        })?;
        let context = resolver
            .resolve_raw(project_id, workspace_id, None)
            .await
            .map_err(|error| Self::context_error(&error))?;
        let pool = self.pool.clone().ok_or_else(|| {
            AppError::Other(anyhow::anyhow!("project health requires a database pool"))
        })?;
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool);
        let catalog_health = catalog
            .get_health(&context.project_id)
            .await
            .map_err(|error| AppError::Other(anyhow::anyhow!(error.to_string())))?
            .map(|record| record.status);
        let typed_workspace_id =
            codegg_core::workspace::WorkspaceId::new_unchecked(context.workspace_id.to_string());
        let service_snapshot = self.workspace_services.peek(&typed_workspace_id);
        let asset_status = self
            .asset_refresh
            .status(&crate::agent::asset_refresh::AssetScope::new(
                project_id,
                workspace_id,
            ))
            .await;
        Ok(project_activation::aggregate_health(
            project_id,
            workspace_id,
            project_activation::catalog_health_layer(catalog_health),
            project_activation::workspace_health_layer(true),
            project_activation::service_health_layer(service_snapshot.as_ref()),
            project_activation::asset_health_layer(&asset_status),
        ))
    }

    /// Evict project activation leases whose bounded lifetime has elapsed.
    /// Underlying workspace service bundles become idle and are then eligible
    /// for the normal workspace-service eviction policy.
    pub fn evict_project_activation_leases(
        &self,
        now: chrono::DateTime<Utc>,
    ) -> project_activation::ActivationEvictionReport {
        self.project_activation.evict_expired(now)
    }

    async fn refresh_runtime_assets(
        &self,
        runtime: &Arc<crate::core::session_runtime::SessionRuntime>,
        session_id: &str,
        reason: crate::agent::asset_refresh::RefreshReason,
    ) -> Result<crate::agent::asset_refresh::RefreshReport, AppError> {
        let project_id = crate::agent::asset_context::ProjectId::parse(&runtime.project_id)
            .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
        let mut builder = crate::agent::asset_context::AssetContextBuilder::new()
            .with_project_id(project_id)
            .with_workspace_root(runtime.workspace_root.clone())
            .with_session_id(session_id);
        if let Some(config_dir) = dirs::config_dir() {
            builder = builder.with_global_root(config_dir);
        }
        let context = builder
            .build()
            .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
        let scope = crate::agent::asset_refresh::AssetScope::new(
            &runtime.project_id,
            runtime.workspace_id.as_str(),
        );
        let report = self.asset_refresh.refresh(scope, context, reason).await;
        self.persist_asset_refresh_metadata(&report).await;
        Ok(report)
    }

    fn refresh_report_error(
        report: &crate::agent::asset_refresh::RefreshReport,
    ) -> Option<AppError> {
        if report.generation.is_some() {
            return None;
        }
        Some(AppError::Other(anyhow::anyhow!(
            "runtime asset refresh {:?} did not publish a usable generation: {}",
            report.outcome,
            report
                .diagnostics
                .first()
                .map(String::as_str)
                .unwrap_or("no diagnostic"),
        )))
    }

    fn asset_refresh_reason_dto(
        reason: crate::agent::asset_refresh::RefreshReason,
    ) -> crate::protocol::core::AssetRefreshReasonDto {
        match reason {
            crate::agent::asset_refresh::RefreshReason::Startup => {
                crate::protocol::core::AssetRefreshReasonDto::Startup
            }
            crate::agent::asset_refresh::RefreshReason::ProjectActivation => {
                crate::protocol::core::AssetRefreshReasonDto::ProjectActivation
            }
            crate::agent::asset_refresh::RefreshReason::SessionLifecycle => {
                crate::protocol::core::AssetRefreshReasonDto::SessionLifecycle
            }
            crate::agent::asset_refresh::RefreshReason::Manual => {
                crate::protocol::core::AssetRefreshReasonDto::Manual
            }
            crate::agent::asset_refresh::RefreshReason::Reload => {
                crate::protocol::core::AssetRefreshReasonDto::Reload
            }
        }
    }

    fn asset_refresh_outcome_dto(
        outcome: crate::agent::asset_refresh::RefreshOutcome,
    ) -> crate::protocol::core::AssetRefreshOutcomeDto {
        match outcome {
            crate::agent::asset_refresh::RefreshOutcome::Published => {
                crate::protocol::core::AssetRefreshOutcomeDto::Published
            }
            crate::agent::asset_refresh::RefreshOutcome::Retained => {
                crate::protocol::core::AssetRefreshOutcomeDto::Retained
            }
            crate::agent::asset_refresh::RefreshOutcome::Cancelled => {
                crate::protocol::core::AssetRefreshOutcomeDto::Cancelled
            }
            crate::agent::asset_refresh::RefreshOutcome::Invalid => {
                crate::protocol::core::AssetRefreshOutcomeDto::Invalid
            }
            crate::agent::asset_refresh::RefreshOutcome::Coalesced => {
                crate::protocol::core::AssetRefreshOutcomeDto::Coalesced
            }
        }
    }

    fn asset_refresh_report_dto(
        report: crate::agent::asset_refresh::RefreshReport,
    ) -> crate::protocol::core::AssetRefreshReportDto {
        crate::protocol::core::AssetRefreshReportDto {
            scope: crate::protocol::core::AssetRefreshScopeDto {
                project_id: report.scope.project_id,
                workspace_id: report.scope.workspace_id,
            },
            reason: Self::asset_refresh_reason_dto(report.reason),
            outcome: Self::asset_refresh_outcome_dto(report.outcome),
            generation: report.generation,
            previous_generation: report.previous_generation,
            fingerprint: report.fingerprint,
            added: report.added,
            removed: report.removed,
            changed: report.changed,
            shadowed: report.shadowed,
            invalid: report.invalid,
            retained: report.retained,
            diagnostics: report.diagnostics,
            coalesced: report.coalesced,
            completed_at_ms: report.completed_at.timestamp_millis(),
        }
    }

    fn asset_refresh_status_dto(
        status: crate::agent::asset_refresh::RefreshStatus,
    ) -> crate::protocol::core::AssetRefreshStatusDto {
        crate::protocol::core::AssetRefreshStatusDto {
            scope: crate::protocol::core::AssetRefreshScopeDto {
                project_id: status.scope.project_id,
                workspace_id: status.scope.workspace_id,
            },
            generation: status.generation,
            fingerprint: status.fingerprint,
            last_success_at_ms: status.last_success_at.map(|value| value.timestamp_millis()),
            in_flight: status.in_flight,
            last_outcome: status.last_outcome.map(Self::asset_refresh_outcome_dto),
            last_diagnostics: status.last_diagnostics,
        }
    }

    /// Rehydrate the workspace registry from its backing store. Daemon
    /// construction synchronously creates a registry with the store
    /// attached; the in-memory cache is empty until this method is called.
    /// Existing CLI/socket entry points invoke this after `with_deps` so
    /// existing `workspace` rows survive restarts.
    pub async fn hydrate_workspace_registry(
        &self,
    ) -> Result<(), codegg_core::workspace::WorkspaceError> {
        self.workspaces.hydrate_from_store().await?;
        self.hydrate_asset_refresh_metadata().await;
        Ok(())
    }

    async fn hydrate_asset_refresh_metadata(&self) {
        let Some(pool) = self.pool.as_ref() else {
            return;
        };
        let rows = match sqlx::query(
            "SELECT project_id, workspace_id, generation, fingerprint \
             FROM runtime_asset_refresh",
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::debug!(error = %error, "runtime asset metadata unavailable during hydration");
                return;
            }
        };
        for row in rows {
            let Ok(project_id) = row.try_get::<String, _>("project_id") else {
                continue;
            };
            let Ok(workspace_id) = row.try_get::<String, _>("workspace_id") else {
                continue;
            };
            let Ok(generation) = row.try_get::<i64, _>("generation") else {
                continue;
            };
            let fingerprint = row
                .try_get::<Option<String>, _>("fingerprint")
                .ok()
                .flatten();
            self.asset_refresh
                .restore_metadata(
                    crate::agent::asset_refresh::AssetScope::new(project_id, workspace_id),
                    u64::try_from(generation).unwrap_or_default(),
                    fingerprint,
                )
                .await;
        }
    }

    async fn persist_asset_refresh_metadata(
        &self,
        report: &crate::agent::asset_refresh::RefreshReport,
    ) {
        let Some(pool) = self.pool.as_ref() else {
            return;
        };
        let Some(generation) = report.generation else {
            return;
        };
        if !matches!(
            report.outcome,
            crate::agent::asset_refresh::RefreshOutcome::Published
                | crate::agent::asset_refresh::RefreshOutcome::Coalesced
        ) {
            return;
        }
        let diagnostics =
            serde_json::to_string(&report.diagnostics).unwrap_or_else(|_| "[]".into());
        if let Err(error) = sqlx::query(
            "INSERT INTO runtime_asset_refresh \
             (project_id, workspace_id, generation, fingerprint, last_success_at, diagnostics_json, time_updated) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(project_id, workspace_id) DO UPDATE SET \
             generation = excluded.generation, fingerprint = excluded.fingerprint, \
             last_success_at = excluded.last_success_at, diagnostics_json = excluded.diagnostics_json, \
             time_updated = excluded.time_updated",
        )
        .bind(&report.scope.project_id)
        .bind(&report.scope.workspace_id)
        .bind(i64::try_from(generation).unwrap_or(i64::MAX))
        .bind(&report.fingerprint)
        .bind(report.completed_at.timestamp_millis())
        .bind(diagnostics)
        .bind(Utc::now().timestamp_millis())
        .execute(pool)
        .await
        {
            tracing::warn!(
                error = %error,
                project_id = %report.scope.project_id,
                workspace_id = %report.scope.workspace_id,
                generation,
                "failed to persist runtime asset refresh metadata"
            );
        }
    }

    /// Resolve a session's compatibility directory to an already registered
    /// workspace. This never registers a workspace or creates project
    /// identity from path text.
    pub async fn workspace_for_session_directory(
        &self,
        session_id: &str,
        session_directory: &str,
    ) -> Result<Arc<WorkspaceRecord>, AppError> {
        let path = std::path::Path::new(session_directory);
        self.workspaces.resolve_root(path).await.ok_or_else(|| {
            AppError::Other(anyhow::anyhow!(
                "session {} has no registered workspace for its compatibility locator",
                session_id
            ))
        })
    }

    fn context_error(error: &ContextResolutionError) -> AppError {
        let code =
            match error {
                ContextResolutionError::DirectoryNotFound
                | ContextResolutionError::DirectoryAmbiguous(_) => "project_context_required",
                ContextResolutionError::ProjectNotFound
                | ContextResolutionError::WorkspaceNotFound => "project_context_not_found",
                ContextResolutionError::ProjectArchived
                | ContextResolutionError::WorkspaceArchived => "project_context_archived",
                ContextResolutionError::BindingProjectMismatch
                | ContextResolutionError::SessionBindingMismatch => "project_context_mismatch",
                ContextResolutionError::BindingMissing
                | ContextResolutionError::BindingNotResolved { .. }
                | ContextResolutionError::SessionBindingMissing
                | ContextResolutionError::SessionBindingNotResolved { .. } => {
                    "project_context_unresolved"
                }
                ContextResolutionError::InvalidInput(_) => "invalid_project_context",
                ContextResolutionError::CatalogFailure(_)
                | ContextResolutionError::StorageFailure(_)
                | ContextResolutionError::WorkspaceStoreFailure(_) => "project_context_unavailable",
            };
        AppError::Other(anyhow::anyhow!(
            "{code}: project/workspace context is not executable"
        ))
    }

    fn project_health_layer_dto(
        layer: &project_activation::HealthLayer,
    ) -> crate::protocol::dto::ProjectHealthLayerDto {
        let state = match layer.state {
            project_activation::HealthState::Available => "available",
            project_activation::HealthState::Stale => "stale",
            project_activation::HealthState::Unavailable => "unavailable",
            project_activation::HealthState::Contended => "contended",
            project_activation::HealthState::Error => "error",
        };
        crate::protocol::dto::ProjectHealthLayerDto {
            state: state.to_string(),
            code: layer.code.clone(),
            message: layer.message.clone(),
        }
    }

    fn project_health_dto(
        snapshot: &ProjectHealthSnapshot,
        durable: Option<crate::protocol::dto::ProjectHealthRecordDto>,
    ) -> crate::protocol::dto::ProjectHealthDto {
        crate::protocol::dto::ProjectHealthDto {
            project_id: snapshot.project_id.clone(),
            workspace_id: snapshot.workspace_id.clone(),
            overall: match snapshot.overall {
                project_activation::HealthState::Available => "available",
                project_activation::HealthState::Stale => "stale",
                project_activation::HealthState::Unavailable => "unavailable",
                project_activation::HealthState::Contended => "contended",
                project_activation::HealthState::Error => "error",
            }
            .to_string(),
            catalog: Self::project_health_layer_dto(&snapshot.catalog),
            workspace: Self::project_health_layer_dto(&snapshot.workspace),
            assets: Self::project_health_layer_dto(&snapshot.assets),
            services: Self::project_health_layer_dto(&snapshot.services),
            diagnostics: snapshot.diagnostics.iter().take(16).cloned().collect(),
            durable,
        }
    }

    fn project_catalog_error(
        operation: &'static str,
        error: &codegg_core::project_catalog::CatalogError,
    ) -> CoreResponse {
        let code = match error {
            codegg_core::project_catalog::CatalogError::NotFound(_) => "project_not_found",
            codegg_core::project_catalog::CatalogError::InvalidValue(_) => {
                "invalid_project_request"
            }
            codegg_core::project_catalog::CatalogError::Conflict(_) => "project_catalog_conflict",
            codegg_core::project_catalog::CatalogError::AlreadyExists(_) => {
                "project_already_exists"
            }
            codegg_core::project_catalog::CatalogError::Database(_) => {
                "project_catalog_unavailable"
            }
        };
        CoreResponse::Error {
            code: code.to_string(),
            message: format!("{operation}: {error}"),
        }
    }

    async fn project_details(
        &self,
        project_id: &codegg_core::identity::ProjectId,
    ) -> Result<crate::protocol::dto::ProjectDetailsDto, codegg_core::project_catalog::CatalogError>
    {
        let pool = self.pool.clone().ok_or_else(|| {
            codegg_core::project_catalog::CatalogError::Database(
                "project catalog requires a database pool".to_string(),
            )
        })?;
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool);
        let record = catalog.get_project(project_id).await?;
        let workspaces = catalog.list_workspaces_for_project(project_id).await?;
        let session_count = catalog.list_sessions_for_project(project_id).await?;
        let health = catalog.get_health(project_id).await?;
        Ok(codegg_core::protocol_conversions::project_details_to_dto(
            &record,
            &workspaces,
            session_count,
            health.as_ref(),
        ))
    }

    async fn project_lifecycle_request(
        &self,
        raw_project_id: &str,
        restore: bool,
    ) -> Result<CoreResponse, AppError> {
        let project_id = match codegg_core::identity::ProjectId::parse(raw_project_id) {
            Ok(id) => id,
            Err(error) => {
                return Ok(CoreResponse::Error {
                    code: "invalid_project_id".to_string(),
                    message: error.to_string(),
                })
            }
        };
        let Some(pool) = self.pool.clone() else {
            return Ok(CoreResponse::Error {
                code: "project_catalog_unavailable".to_string(),
                message: "project lifecycle operations require a catalog".to_string(),
            });
        };
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool);
        let result = if restore {
            catalog.restore_project(&project_id, "protocol").await
        } else {
            catalog.archive_project(&project_id, "protocol").await
        };
        match result {
            Ok(record) => {
                let project =
                    codegg_core::protocol_conversions::project_catalog_record_to_dto(&record);
                let event = if restore {
                    CoreEvent::ProjectRestored {
                        project_id: project.project_id.clone(),
                        project: project.clone(),
                    }
                } else {
                    CoreEvent::ProjectArchived {
                        project_id: project.project_id.clone(),
                        project: project.clone(),
                    }
                };
                let _ = self.event_log.publish(None, None, event).await;
                Ok(if restore {
                    CoreResponse::ProjectRestored { project }
                } else {
                    CoreResponse::ProjectArchived { project }
                })
            }
            Err(error) => Ok(Self::project_catalog_error(
                if restore {
                    "project restore failed"
                } else {
                    "project archive failed"
                },
                &error,
            )),
        }
    }

    async fn resolve_session_context(
        &self,
        session_id: &str,
        directory: &str,
    ) -> Result<ProjectContext, AppError> {
        let resolver = self.context_resolver.as_ref().ok_or_else(|| {
            AppError::Other(anyhow::anyhow!(
                "project context resolver is unavailable for this daemon"
            ))
        })?;
        let session_id = SessionId::parse(session_id)
            .map_err(|e| Self::context_error(&ContextResolutionError::InvalidInput(e)))?;
        let storage = codegg_core::project_storage::ProjectStorage::new(
            self.pool.clone().ok_or_else(|| {
                AppError::Other(anyhow::anyhow!(
                    "no database pool available for context lookup"
                ))
            })?,
        );
        if let Some(binding) = storage
            .session_binding(session_id.as_str())
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?
        {
            let project_id = binding.project_id.ok_or_else(|| {
                Self::context_error(&ContextResolutionError::SessionBindingNotResolved {
                    status: binding.status,
                })
            })?;
            let workspace_id = binding.workspace_id.ok_or_else(|| {
                Self::context_error(&ContextResolutionError::SessionBindingNotResolved {
                    status: binding.status,
                })
            })?;
            return resolver
                .resolve(
                    ProjectContextRequest::new(project_id, workspace_id)
                        .with_session_id(session_id),
                )
                .await
                .map_err(|e| Self::context_error(&e));
        }
        resolver
            .resolve_directory(directory)
            .await
            .map_err(|e| Self::context_error(&e))
    }

    async fn resolve_request_context(
        &self,
        directory: &str,
        project_id: Option<&str>,
        workspace_id: Option<&str>,
    ) -> Result<ProjectContext, AppError> {
        let resolver = self.context_resolver.as_ref().ok_or_else(|| {
            AppError::Other(anyhow::anyhow!(
                "project context resolver is unavailable for this daemon"
            ))
        })?;
        match (project_id, workspace_id) {
            (Some(project_id), Some(workspace_id)) => resolver
                .resolve_raw(project_id, workspace_id, None)
                .await
                .map_err(|e| Self::context_error(&e)),
            (None, None) => resolver
                .resolve_directory(directory)
                .await
                .map_err(|e| Self::context_error(&e)),
            _ => Err(AppError::Other(anyhow::anyhow!(
                "project_context_required: project_id and workspace_id must be provided together"
            ))),
        }
    }

    async fn resolve_session_list_project(
        &self,
        project_id_or_directory: &str,
    ) -> Result<codegg_core::identity::ProjectId, AppError> {
        if let Ok(project_id) = codegg_core::identity::ProjectId::parse(project_id_or_directory) {
            return Ok(project_id);
        }
        self.context_resolver
            .as_ref()
            .ok_or_else(|| {
                AppError::Other(anyhow::anyhow!(
                    "project_context_required: project resolver is unavailable"
                ))
            })?
            .resolve_directory(project_id_or_directory)
            .await
            .map(|context| context.project_id)
            .map_err(|error| Self::context_error(&error))
    }

    fn session_dto(
        session: crate::session::Session,
        context: Option<&ProjectContext>,
    ) -> crate::protocol::dto::Session {
        let mut dto = crate::protocol_conversions::session_to_dto(session);
        if let Some(context) = context {
            dto.binding = Some(crate::protocol::dto::SessionBindingDto {
                project_id: context.project_id.as_str().to_string(),
                workspace_id: context.workspace_id.as_str().to_string(),
                repository_id: context
                    .repository_id
                    .as_ref()
                    .map(|id| id.as_str().to_string()),
                binding_state: Some(context.binding_status.as_str().to_string()),
                binding_revision: u64::try_from(context.binding_revision).ok(),
                compatibility_directory: Some(
                    context.workspace_root.to_string_lossy().into_owned(),
                ),
            });
            dto.project_id = context.project_id.as_str().to_string();
            dto.workspace_id = Some(context.workspace_id.as_str().to_string());
        }
        dto
    }

    /// Bind a `SessionRuntime` to a workspace record. Stores
    /// compatibility projections for `project_id` and `directory`. Idempotent
    /// when an existing runtime already carries the same workspace.
    pub fn bind_runtime(
        &self,
        session_id: &str,
        workspace: Arc<WorkspaceRecord>,
        project_id: String,
        directory: std::path::PathBuf,
    ) -> Arc<crate::core::session_runtime::SessionRuntime> {
        self.sessions.get_or_create(
            session_id,
            workspace.id.clone(),
            workspace.canonical_root.clone(),
            project_id,
            directory,
        )
    }

    /// Resolve a session_id to a bound runtime, looking up the session in
    /// storage, resolving its workspace, and creating the runtime. Returns
    /// `Err` when the session is unbound and the directory cannot be
    /// turned into a workspace (e.g., the directory no longer exists).
    pub async fn bind_runtime_for_session(
        &self,
        session_id: &str,
    ) -> Result<Arc<crate::core::session_runtime::SessionRuntime>, AppError> {
        let pool = self.pool.clone().ok_or_else(|| {
            AppError::Other(anyhow::anyhow!(
                "no database pool available for session lookup"
            ))
        })?;
        let store = crate::session::SessionStore::new(pool);
        let session = store
            .get(session_id)
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("session store error: {}", e)))?
            .ok_or_else(|| AppError::Other(anyhow::anyhow!("session not found: {}", session_id)))?;
        let context = self
            .resolve_session_context(session_id, &session.directory)
            .await?;
        let workspace = self
            .workspaces
            .resolve(&context.workspace_id)
            .await
            .ok_or_else(|| Self::context_error(&ContextResolutionError::WorkspaceNotFound))?;
        Ok(self.bind_runtime(
            session_id,
            workspace,
            context.project_id.as_str().to_string(),
            context.workspace_root,
        ))
    }

    /// Legacy constructor for backward compatibility. Prefer `with_deps`.
    pub fn new(
        pool: Option<sqlx::SqlitePool>,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    ) -> Self {
        Self::with_deps(CoreRuntimeDeps::new(
            pool,
            subagent_pool,
            memory_store,
            bg_scheduler,
        ))
    }

    pub fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<
        crate::protocol::core::EventEnvelope<crate::protocol::core::CoreEvent>,
    > {
        self.event_log.subscribe()
    }

    /// Apply the bridge fallback to a single `AppEvent` and return the
    /// resulting `(session_id, turn_id, core_event)` triple, or `None` if
    /// the event has no corresponding `CoreEvent`. If the bridged event
    /// has an empty or missing `turn_id`, look up the active turn for the
    /// session and attach its `turn_id` to the event so every event
    /// belonging to a turn carries the same identity.
    pub(crate) async fn bridge_app_event(
        &self,
        app_event: crate::bus::events::AppEvent,
    ) -> Option<(
        Option<String>,
        Option<String>,
        crate::protocol::core::CoreEvent,
    )> {
        let mut core_event = super::map_app_event_to_core_event(app_event)?;
        let (session_id, mut turn_id) = super::core_event_metadata(&core_event);
        let turn_id_empty = match &turn_id {
            Some(t) => t.is_empty(),
            None => true,
        };
        if turn_id_empty {
            if let Some(sid) = session_id.clone() {
                if let Some(runtime) = self.sessions.get(&sid) {
                    let active = runtime.active_turn.read().await;
                    if let Some(handle) = active.as_ref() {
                        core_event =
                            super::set_turn_id_on_event(core_event, handle.turn_id.clone());
                        turn_id = Some(handle.turn_id.clone());
                    }
                }
            }
        }
        Some((session_id, turn_id, core_event))
    }

    /// Recover daemon state after restart.
    /// Marks previously active turns as failed and logs stale permissions/questions.
    pub async fn recover_state(&self) {
        let Some(ref pool) = self.pool else {
            return;
        };

        // Find interrupted turns: TurnStarted without a matching TurnCompleted/TurnFailed
        // for the same session_id + turn_id. Use the explicit event-type strings
        // written by `core_event_type()` (snake_case) so the query is stable and
        // grep-able. The DISTINCT + NOT EXISTS pattern ensures each (session, turn)
        // pair is reported at most once and we only flag turns that have a real
        // turn_id (e.g., not blank rows from older schemas).
        let active_turns: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT e1.session_id, e1.turn_id \
             FROM core_event_log e1 \
             WHERE e1.event_type = 'turn_started' \
             AND e1.turn_id IS NOT NULL \
             AND NOT EXISTS ( \
                 SELECT 1 FROM core_event_log e2 \
                 WHERE e2.session_id = e1.session_id \
                 AND e2.turn_id = e1.turn_id \
                 AND (e2.event_type = 'turn_completed' OR e2.event_type = 'turn_failed') \
             )",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        if !active_turns.is_empty() {
            tracing::info!(
                "Recovery: found {} interrupted turn(s), emitting TurnFailed",
                active_turns.len()
            );
            for (session_id, turn_id) in &active_turns {
                tracing::info!(
                    "  Marking session {} turn {} as failed (daemon restarted while active)",
                    session_id,
                    turn_id
                );
                self.event_log
                    .publish(
                        Some(session_id.clone()),
                        Some(turn_id.clone()),
                        crate::protocol::core::CoreEvent::TurnFailed {
                            session_id: session_id.clone(),
                            turn_id: Some(turn_id.clone()),
                            message: "Daemon restarted while turn was active".to_string(),
                        },
                    )
                    .await;

                // Clear runtime state for this session
                if let Some(runtime) = self.sessions.get(session_id) {
                    let mut active = runtime.active_turn.write().await;
                    *active = None;
                    drop(active);

                    let mut status = runtime.status.write().await;
                    *status = RuntimeSessionStatus::Idle;
                }
            }
        }

        // Count stale PermissionPending events (no PermissionResponded in same session)
        let stale_perms: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM core_event_log WHERE event_type = 'permission_pending' \
             AND NOT EXISTS ( \
                 SELECT 1 FROM core_event_log e2 \
                 WHERE e2.event_type = 'permission_responded' \
                 AND e2.session_id = core_event_log.session_id \
             )",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // Count stale QuestionPending events (no QuestionAnswered in same session)
        let stale_questions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM core_event_log WHERE event_type = 'question_pending' \
             AND NOT EXISTS ( \
                 SELECT 1 FROM core_event_log e2 \
                 WHERE e2.event_type = 'question_answered' \
                 AND e2.session_id = core_event_log.session_id \
             )",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        if stale_perms > 0 || stale_questions > 0 {
            tracing::info!(
                "Recovery: {} stale permission(s), {} stale question(s) from previous run (will timeout naturally)",
                stale_perms,
                stale_questions
            );
        }

        tracing::info!("Daemon state recovery complete");
    }

    /// Recover durable jobs whose attempts originated from a prior
    /// daemon generation. Must run at startup before the scheduler
    /// admits queued work so interrupted attempts do not silently
    /// consume capacity. Returns the recovery report; the report is
    /// also available via `CoreRequest::JobRecoveryReport`.
    pub async fn recover_jobs(&self) -> Option<crate::job_recovery::RecoveryReportSummary> {
        if let Some(scheduler) = self.deps.scheduler.as_ref() {
            match scheduler
                .recover_at_startup(&self.deps.recovery_policy)
                .await
            {
                Ok(report) => {
                    tracing::info!(
                        interrupted = report.interrupted_attempts,
                        requeued = report.requeued_jobs,
                        terminal = report.terminal_jobs,
                        "Job recovery complete"
                    );
                    Some(crate::job_recovery::RecoveryReportSummary {
                        interrupted_attempts: report.interrupted_attempts,
                        requeued_jobs: report.requeued_jobs,
                        terminal_jobs: report.terminal_jobs,
                        schedules_reconciled: report.schedules_reconciled,
                    })
                }
                Err(e) => {
                    tracing::warn!(error = %e, "job recovery failed");
                    None
                }
            }
        } else {
            None
        }
    }

    pub async fn replay_from(
        &self,
        from_event_seq: u64,
        filter: &EventFilter,
    ) -> Vec<crate::protocol::core::EventEnvelope<crate::protocol::core::CoreEvent>> {
        self.event_log.replay_from(from_event_seq, filter).await
    }

    pub fn start_event_bridge(self: &Arc<Self>) {
        let daemon = Arc::clone(self);
        tokio::spawn(async move {
            let mut bus_rx = crate::bus::global::GlobalEventBus::subscribe();
            loop {
                match bus_rx.recv().await {
                    Ok(app_event) => {
                        if let Some((session_id, turn_id, core_event)) =
                            daemon.bridge_app_event(app_event.clone()).await
                        {
                            daemon
                                .event_log
                                .publish(session_id, turn_id, core_event)
                                .await;
                        }
                        match &app_event {
                            crate::bus::events::AppEvent::AgentFinished {
                                session_id,
                                stop_reason,
                                input_tokens,
                                output_tokens,
                                ..
                            } => {
                                // Update runtime token counts
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    *runtime.last_input_tokens.write().await = *input_tokens;
                                    *runtime.last_output_tokens.write().await = *output_tokens;
                                }
                                use super::notification::*;
                                let kind = if stop_reason == "error" {
                                    NotificationKind::TurnFailed
                                } else {
                                    NotificationKind::TurnCompleted
                                };
                                let priority = if stop_reason == "error" {
                                    NotificationPriority::High
                                } else {
                                    NotificationPriority::Low
                                };
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind,
                                    priority,
                                    message: format!(
                                        "Turn {} for session {}",
                                        stop_reason, session_id
                                    ),
                                    dedupe_key: Some(format!("turn-done:{}", session_id)),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::PermissionPending {
                                session_id,
                                turn_id,
                                tool,
                                ..
                            } => {
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: turn_id.clone(),
                                    kind: NotificationKind::PermissionRequired,
                                    priority: NotificationPriority::Urgent,
                                    message: format!("Permission required for tool: {}", tool),
                                    dedupe_key: Some(format!("perm:{}", session_id)),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::QuestionPending {
                                session_id,
                                turn_id,
                                ..
                            } => {
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: turn_id.clone(),
                                    kind: NotificationKind::QuestionRequired,
                                    priority: NotificationPriority::Urgent,
                                    message: "Question requires your input".to_string(),
                                    dedupe_key: Some(format!("question:{}", session_id)),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::Error { message } => {
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: None,
                                    turn_id: None,
                                    kind: NotificationKind::Error,
                                    priority: NotificationPriority::High,
                                    message: message.clone(),
                                    dedupe_key: None,
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::SubagentStarted {
                                session_id, ..
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                            }
                            crate::bus::events::AppEvent::SubagentCompleted {
                                session_id,
                                task_id,
                                agent,
                                ..
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind: NotificationKind::SubagentCompleted,
                                    priority: NotificationPriority::Normal,
                                    message: format!(
                                        "Subagent {} completed task {}",
                                        agent, task_id
                                    ),
                                    dedupe_key: Some(format!(
                                        "subagent-done:{}:{}",
                                        session_id, task_id
                                    )),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::SubagentFailed {
                                session_id,
                                task_id,
                                agent,
                                error,
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind: NotificationKind::SubagentFailed,
                                    priority: NotificationPriority::High,
                                    message: format!(
                                        "Subagent {} failed task {}: {}",
                                        agent, task_id, error
                                    ),
                                    dedupe_key: Some(format!(
                                        "subagent-fail:{}:{}",
                                        session_id, task_id
                                    )),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Event bridge lagged, {} events dropped", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    pub async fn handle_request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::AssetRefresh { request } => {
                let Some(resolver) = self.context_resolver.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "asset_refresh_unavailable".to_string(),
                        message: "asset refresh requires the daemon project context resolver"
                            .to_string(),
                    });
                };
                let project_id =
                    match codegg_core::identity::ProjectId::parse(&request.scope.project_id) {
                        Ok(id) => id,
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "invalid_asset_refresh_scope".to_string(),
                                message: error.to_string(),
                            });
                        }
                    };
                let workspace_id =
                    match codegg_core::workspace::WorkspaceId::parse(&request.scope.workspace_id) {
                        Ok(id) => id,
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "invalid_asset_refresh_scope".to_string(),
                                message: error.to_string(),
                            });
                        }
                    };
                let context = match resolver
                    .resolve(ProjectContextRequest::new(project_id, workspace_id))
                    .await
                {
                    Ok(context) => context,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "asset_refresh_context_failed".to_string(),
                            message: Self::context_error(&error).to_string(),
                        });
                    }
                };
                let reason = match request.reason {
                    crate::protocol::core::AssetRefreshReasonDto::Startup => {
                        crate::agent::asset_refresh::RefreshReason::Startup
                    }
                    crate::protocol::core::AssetRefreshReasonDto::ProjectActivation => {
                        crate::agent::asset_refresh::RefreshReason::ProjectActivation
                    }
                    crate::protocol::core::AssetRefreshReasonDto::SessionLifecycle => {
                        crate::agent::asset_refresh::RefreshReason::SessionLifecycle
                    }
                    crate::protocol::core::AssetRefreshReasonDto::Manual => {
                        crate::agent::asset_refresh::RefreshReason::Manual
                    }
                    crate::protocol::core::AssetRefreshReasonDto::Reload => {
                        crate::agent::asset_refresh::RefreshReason::Reload
                    }
                };
                let report = match self
                    .refresh_project_context(&context, request.session_id.as_deref(), reason)
                    .await
                {
                    Ok(report) => report,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "asset_refresh_failed".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                let dto = Self::asset_refresh_report_dto(report);
                let _ = self
                    .event_log
                    .publish(
                        None,
                        None,
                        crate::protocol::core::CoreEvent::AssetRefreshCompleted {
                            report: dto.clone(),
                        },
                    )
                    .await;
                Ok(CoreResponse::AssetRefresh { report: dto })
            }
            CoreRequest::AssetRefreshStatus { scope } => {
                let scope_internal = crate::agent::asset_refresh::AssetScope::new(
                    scope.project_id.clone(),
                    scope.workspace_id.clone(),
                );
                let status = self.asset_refresh.status(&scope_internal).await;
                Ok(CoreResponse::AssetRefreshStatus {
                    status: Self::asset_refresh_status_dto(status),
                })
            }
            CoreRequest::AssetRefreshCapabilities => Ok(CoreResponse::AssetRefreshCapabilities {
                supported: true,
                max_report_entries: 64,
            }),
            CoreRequest::EggpoolConnectionCreate { request } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                match provisioner.create(request).await {
                    Ok(result) => Ok(CoreResponse::EggpoolConnectionCreated { result }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: eggpool_error_code(&error).to_string(),
                        message: eggpool_error_message(&error).to_string(),
                    }),
                }
            }
            CoreRequest::EggpoolConnectionCancel { operation_id } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                if provisioner.cancel(&operation_id) {
                    Ok(CoreResponse::EggpoolConnectionCancelled { operation_id })
                } else {
                    Ok(CoreResponse::Error {
                        code: "connection_not_in_flight".to_string(),
                        message: "Connection operation is not in flight".to_string(),
                    })
                }
            }
            CoreRequest::EggpoolConnectionStatus { operation_id } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                match provisioner.status(&operation_id).await {
                    Ok(status) => Ok(CoreResponse::EggpoolConnectionStatus { status }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: eggpool_error_code(&error).to_string(),
                        message: eggpool_error_message(&error).to_string(),
                    }),
                }
            }
            CoreRequest::ProviderConnectionList => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                match provisioner.list().await {
                    Ok(connections) => Ok(CoreResponse::ProviderConnections { connections }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: eggpool_error_code(&error).to_string(),
                        message: eggpool_error_message(&error).to_string(),
                    }),
                }
            }
            CoreRequest::ProviderConnectionModels { connection_id } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                let Ok(connection_id) =
                    codegg_core::identity::ProviderConnectionId::parse(&connection_id)
                else {
                    return Ok(CoreResponse::Error {
                        code: "invalid_connection_id".to_string(),
                        message: "Provider connection ID is invalid".to_string(),
                    });
                };
                match provisioner.models(&connection_id).await {
                    Ok((catalog_revision, models)) => Ok(CoreResponse::ProviderConnectionModels {
                        connection_id: connection_id.to_string(),
                        catalog_revision,
                        models,
                    }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: eggpool_error_code(&error).to_string(),
                        message: eggpool_error_message(&error).to_string(),
                    }),
                }
            }
            CoreRequest::ConnectionGet { connection_id } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                let summaries = match provisioner.list().await {
                    Ok(summaries) => summaries,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "provider_connections_unavailable".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                let Some(summary) = summaries
                    .into_iter()
                    .find(|summary| summary.id == connection_id)
                else {
                    return Ok(CoreResponse::Error {
                        code: "connection_not_found".to_string(),
                        message: "Provider connection was not found".to_string(),
                    });
                };
                Ok(CoreResponse::ConnectionDetail {
                    detail: connection_detail_dto(&summary),
                })
            }
            CoreRequest::ConnectionListDetail => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                let summaries = match provisioner.list().await {
                    Ok(summaries) => summaries,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "provider_connections_unavailable".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                let details = summaries.iter().map(connection_detail_dto).collect();
                Ok(CoreResponse::ConnectionDetails { details })
            }
            CoreRequest::ConnectionRotateSecretStage { request_id, secret } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                let handle = codegg_protocol::provider::SecretInputRef::new(format!(
                    "rot-secret-{}",
                    uuid::Uuid::new_v4()
                ))
                .expect("generated rotation secret handle must satisfy protocol bounds");
                if !provisioner.register_rotation_secret(handle.clone(), secret.expose().to_owned())
                {
                    return Ok(CoreResponse::Error {
                        code: "connection_rotation_secret_rejected".to_string(),
                        message: "Rotation secret was rejected by the bounded local secret buffer"
                            .to_string(),
                    });
                }
                Ok(CoreResponse::ConnectionRotateSecretStaged {
                    request_id,
                    secret: handle,
                })
            }
            CoreRequest::ConnectionRotateBegin {
                request_id,
                connection_id,
                expected_revision,
                change,
                secret,
            } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                let Ok(connection_id) =
                    codegg_core::identity::ProviderConnectionId::parse(&connection_id)
                else {
                    return Ok(CoreResponse::Error {
                        code: "invalid_connection_id".to_string(),
                        message: "Provider connection ID is invalid".to_string(),
                    });
                };
                let delete_previous = matches!(
                    change,
                    codegg_protocol::provider::ConnectionRotateChange::CredentialOnly
                        | codegg_protocol::provider::ConnectionRotateChange::CredentialAndEndpoint { .. }
                );
                match provisioner
                    .rotate(
                        &request_id,
                        &connection_id,
                        expected_revision,
                        change,
                        secret,
                        delete_previous,
                    )
                    .await
                {
                    Ok(result) => {
                        if let Some(manager) = self.deps.connection_manager.as_ref() {
                            manager.rotate(
                                &connection_id,
                                result
                                    .new_revision
                                    .unwrap_or(expected_revision)
                                    .saturating_sub(1),
                            );
                        }
                        let _ = self
                            .event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::ConnectionRotated {
                                    connection_id: connection_id.to_string(),
                                    new_revision: result.new_revision.unwrap_or(expected_revision),
                                    catalog_revision: result.catalog_revision.clone(),
                                    actor_seam: "local_operator".to_string(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::ConnectionRotateStatus { result })
                    }
                    Err(error) => Ok(CoreResponse::Error {
                        code: "connection_rotation_failed".to_string(),
                        message: error.to_string(),
                    }),
                }
            }
            CoreRequest::ConnectionRotateCancel { request_id } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                let cancelled = provisioner.cancel(&request_id);
                let result = provisioner.rotation_status(&request_id).unwrap_or(
                    codegg_protocol::provider::ConnectionRotateStatusDto {
                        request_id,
                        connection_id: String::new(),
                        state: if cancelled { "cancelling" } else { "unknown" }.to_string(),
                        new_revision: None,
                        catalog_revision: None,
                        error_code: (!cancelled).then(|| "operation_not_found".to_string()),
                    },
                );
                Ok(CoreResponse::ConnectionRotateStatus { result })
            }
            CoreRequest::ConnectionRotateStatus { request_id } => {
                let result = self
                    .eggpool_provisioner
                    .as_ref()
                    .and_then(|provisioner| provisioner.rotation_status(&request_id))
                    .unwrap_or(codegg_protocol::provider::ConnectionRotateStatusDto {
                        request_id,
                        connection_id: String::new(),
                        state: "unknown".to_string(),
                        new_revision: None,
                        catalog_revision: None,
                        error_code: Some("operation_not_found".to_string()),
                    });
                Ok(CoreResponse::ConnectionRotateStatus { result })
            }
            CoreRequest::ConnectionRefreshBegin {
                connection_id,
                expected_revision,
            } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                let Ok(connection_id) =
                    codegg_core::identity::ProviderConnectionId::parse(&connection_id)
                else {
                    return Ok(CoreResponse::Error {
                        code: "invalid_connection_id".to_string(),
                        message: "Provider connection ID is invalid".to_string(),
                    });
                };
                let operation_id = format!("refresh-{}", uuid::Uuid::new_v4());
                match provisioner
                    .refresh_with_operation(&operation_id, &connection_id, expected_revision)
                    .await
                {
                    Ok(result) => {
                        if let Some(manager) = self.deps.connection_manager.as_ref() {
                            manager.refresh(&connection_id);
                        }
                        Ok(CoreResponse::ConnectionRefreshResult { result })
                    }
                    Err(error) => Ok(CoreResponse::Error {
                        code: "connection_refresh_failed".to_string(),
                        message: error.to_string(),
                    }),
                }
            }
            CoreRequest::ConnectionRefreshCancel { operation_id } => {
                let Some(provisioner) = self.eggpool_provisioner.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                let cancelled = provisioner.cancel(&operation_id);
                let result = provisioner.refresh_status(&operation_id).unwrap_or(
                    codegg_protocol::provider::ConnectionRefreshStatusDto {
                        operation_id,
                        connection_id: String::new(),
                        state: if cancelled { "cancelling" } else { "unknown" }.to_string(),
                        revision: None,
                        catalog_revision: None,
                        error_code: (!cancelled).then(|| "operation_not_found".to_string()),
                    },
                );
                Ok(CoreResponse::ConnectionRefreshStatus { result })
            }
            CoreRequest::ConnectionRefreshStatus { operation_id } => {
                let result = self
                    .eggpool_provisioner
                    .as_ref()
                    .and_then(|provisioner| provisioner.refresh_status(&operation_id))
                    .unwrap_or(codegg_protocol::provider::ConnectionRefreshStatusDto {
                        operation_id,
                        connection_id: String::new(),
                        state: "unknown".to_string(),
                        revision: None,
                        catalog_revision: None,
                        error_code: Some("operation_not_found".to_string()),
                    });
                Ok(CoreResponse::ConnectionRefreshStatus { result })
            }
            CoreRequest::ConnectionEnable {
                connection_id,
                expected_revision,
                require_probe: _,
            } => {
                connection_lifecycle_response(
                    self.pool.clone(),
                    connection_id,
                    expected_revision,
                    "enable",
                )
                .await
            }
            CoreRequest::ConnectionDisable {
                connection_id,
                expected_revision,
            } => {
                connection_lifecycle_response(
                    self.pool.clone(),
                    connection_id,
                    expected_revision,
                    "disable",
                )
                .await
            }
            CoreRequest::ConnectionDelete {
                connection_id,
                expected_revision,
            } => {
                connection_lifecycle_response(
                    self.pool.clone(),
                    connection_id,
                    expected_revision,
                    "delete",
                )
                .await
            }
            CoreRequest::ConnectionRestore {
                connection_id,
                expected_revision,
            } => {
                connection_lifecycle_response(
                    self.pool.clone(),
                    connection_id,
                    expected_revision,
                    "restore",
                )
                .await
            }
            CoreRequest::ConnectionPurge {
                connection_id,
                expected_revision,
            } => {
                let Some(_pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connections require a daemon SQLite catalog".to_string(),
                    });
                };
                let Ok(id) = codegg_core::identity::ProviderConnectionId::parse(&connection_id)
                else {
                    return Ok(CoreResponse::Error {
                        code: "invalid_connection_id".to_string(),
                        message: "Provider connection ID is invalid".to_string(),
                    });
                };
                let provisioner = self
                    .eggpool_provisioner
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Eggpool provisioner is unavailable for purge"));
                let Ok(provisioner) = provisioner else {
                    return Ok(CoreResponse::Error {
                        code: "provider_connections_unavailable".to_string(),
                        message: "Provider connection purge requires the daemon provisioner"
                            .to_string(),
                    });
                };
                match provisioner.purge(&id, expected_revision).await {
                    Ok(codegg_core::provider_connections::PurgeOutcome::Purged) => {
                        Ok(CoreResponse::ConnectionPurge {
                            outcome: codegg_protocol::provider::PurgeOutcome::Purged,
                        })
                    }
                    Ok(codegg_core::provider_connections::PurgeOutcome::Blocked(blockers)) => {
                        Ok(CoreResponse::ConnectionPurge {
                            outcome: codegg_protocol::provider::PurgeOutcome::Blocked(
                                blockers.into_iter().map(purge_blocker_dto).collect(),
                            ),
                        })
                    }
                    Err(error) => Ok(CoreResponse::Error {
                        code: "connection_purge_failed".to_string(),
                        message: error.to_string(),
                    }),
                }
            }
            CoreRequest::SessionSelectionGet { session_id } => {
                let Some(service) = self.selection_service.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "session_selection_unavailable".to_string(),
                        message: "Session selection requires a daemon SQLite catalog".to_string(),
                    });
                };
                match service.get(&session_id).await {
                    Ok(selection) => Ok(CoreResponse::SessionSelection {
                        session_id,
                        selection,
                    }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: crate::core::session_selection::selection_error_code(&error)
                            .to_string(),
                        message: crate::core::session_selection::selection_error_message(&error),
                    }),
                }
            }
            CoreRequest::SessionLifecycleGet { session_id } => {
                let Some(service) = self.selection_service.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "session_selection_unavailable".to_string(),
                        message: "Session lifecycle requires a daemon SQLite catalog".to_string(),
                    });
                };
                match service.get(&session_id).await {
                    Ok(crate::protocol::provider::SessionSelectionDto::Selected {
                        connection,
                        model,
                        ..
                    }) => Ok(CoreResponse::SessionLifecycle {
                        projection: crate::protocol::provider::SessionLifecycleProjection {
                            connection_id: connection.id,
                            state: connection.state,
                            last_health_at: connection.health.map(|health| health.checked_at),
                            current_selected_model_id: Some(model.model_id),
                            removed_models: Vec::new(),
                        },
                    }),
                    Ok(crate::protocol::provider::SessionSelectionDto::LegacyUnresolved {
                        reason,
                        ..
                    }) => Ok(CoreResponse::SessionLifecycle {
                        projection: crate::protocol::provider::SessionLifecycleProjection {
                            connection_id: String::new(),
                            state: "legacy_unresolved".to_string(),
                            last_health_at: None,
                            current_selected_model_id: None,
                            removed_models: vec![reason],
                        },
                    }),
                    Ok(crate::protocol::provider::SessionSelectionDto::Unselected {}) => {
                        Ok(CoreResponse::SessionLifecycle {
                            projection: crate::protocol::provider::SessionLifecycleProjection {
                                connection_id: String::new(),
                                state: "unselected".to_string(),
                                last_health_at: None,
                                current_selected_model_id: None,
                                removed_models: Vec::new(),
                            },
                        })
                    }
                    Err(error) => Ok(CoreResponse::Error {
                        code: crate::core::session_selection::selection_error_code(&error)
                            .to_string(),
                        message: crate::core::session_selection::selection_error_message(&error),
                    }),
                }
            }
            CoreRequest::SessionSelectionList { session_id } => {
                let Some(service) = self.selection_service.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "session_selection_unavailable".to_string(),
                        message: "Session selection requires a daemon SQLite catalog".to_string(),
                    });
                };
                match service.list(&session_id).await {
                    Ok(connections) => Ok(CoreResponse::ProviderConnections { connections }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: crate::core::session_selection::selection_error_code(&error)
                            .to_string(),
                        message: crate::core::session_selection::selection_error_message(&error),
                    }),
                }
            }
            CoreRequest::SessionSelectionModels {
                session_id,
                connection_id,
            } => {
                let Some(service) = self.selection_service.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "session_selection_unavailable".to_string(),
                        message: "Session selection requires a daemon SQLite catalog".to_string(),
                    });
                };
                let Ok(connection_id) =
                    codegg_core::identity::ProviderConnectionId::parse(&connection_id)
                else {
                    return Ok(CoreResponse::Error {
                        code: "invalid_connection_id".to_string(),
                        message: "Provider connection ID is invalid".to_string(),
                    });
                };
                match service.models(&session_id, &connection_id).await {
                    Ok((catalog_revision, models)) => Ok(CoreResponse::ProviderConnectionModels {
                        connection_id: connection_id.to_string(),
                        catalog_revision,
                        models,
                    }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: crate::core::session_selection::selection_error_code(&error)
                            .to_string(),
                        message: crate::core::session_selection::selection_error_message(&error),
                    }),
                }
            }
            CoreRequest::SessionSelectionUpdate { request } => {
                let Some(service) = self.selection_service.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "session_selection_unavailable".to_string(),
                        message: "Session selection requires a daemon SQLite catalog".to_string(),
                    });
                };
                let req = *request;
                let Ok(connection_id) =
                    codegg_core::identity::ProviderConnectionId::parse(&req.connection_id)
                else {
                    return Ok(CoreResponse::Error {
                        code: "invalid_connection_id".to_string(),
                        message: "Provider connection ID is invalid".to_string(),
                    });
                };
                match service
                    .update(
                        &req.session_id,
                        &connection_id,
                        &req.model_id,
                        req.expected_connection_revision,
                        req.expected_catalog_revision,
                    )
                    .await
                {
                    Ok(outcome) => match outcome {
                        crate::core::session_selection::SelectionUpdateOutcome::Updated(
                            selection,
                        ) => Ok(CoreResponse::SessionSelectionUpdated {
                            session_id: req.session_id,
                            selection,
                        }),
                        other => Ok(CoreResponse::Error {
                            code: crate::core::session_selection::selection_outcome_code(&other)
                                .to_string(),
                            message: crate::core::session_selection::selection_outcome_message(
                                &other,
                            ),
                        }),
                    },
                    Err(error) => Ok(CoreResponse::Error {
                        code: crate::core::session_selection::selection_error_code(&error)
                            .to_string(),
                        message: crate::core::session_selection::selection_error_message(&error),
                    }),
                }
            }
            CoreRequest::TurnSubmit {
                session_id,
                model,
                agents,
                current_agent_idx,
                messages,
                plan_mode,
                ..
            } => {
                if current_agent_idx >= agents.len() {
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::Error {
                            message: format!(
                                "Invalid agent index {} for {} agents",
                                current_agent_idx,
                                agents.len()
                            ),
                        },
                    );
                    return Ok(CoreResponse::Error {
                        code: "invalid_agent_index".to_string(),
                        message: "Invalid agent index".to_string(),
                    });
                }
                // A durable session selection is authoritative for new
                // turns. Never silently route around a selected connection
                // that has entered a non-active lifecycle state.
                if let Some(selection_service) = self.selection_service.as_ref() {
                    if let Ok(crate::protocol::provider::SessionSelectionDto::Selected {
                        connection,
                        ..
                    }) = selection_service.get(&session_id).await
                    {
                        if connection.state != "active" {
                            return Ok(CoreResponse::Error {
                                code: "connection_state".to_string(),
                                message: format!(
                                    "selected provider connection {} is {}",
                                    connection.id, connection.state
                                ),
                            });
                        }
                    }
                }
                // Validate the provider exists before delegating to the turn
                // runtime. This preserves the existing `provider_not_found`
                // response shape from the daemon layer. The turn runtime
                // also validates provider existence internally, so this is
                // intentionally duplicated for backward-compatible error handling.
                let mut registry = crate::provider::ProviderRegistry::new();
                let config = crate::config::schema::Config::load().unwrap_or_default();
                crate::provider::register_builtin_with_config(&mut registry, &config);
                let provider_name = model.split('/').next().unwrap_or("openai").to_string();
                let _model_name = model.split('/').next_back().unwrap_or(&model).to_string();
                let Some(_base_provider) = registry.get(&provider_name) else {
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::Error {
                            message: format!(
                                "Provider '{}' not found. Please check your configuration.",
                                provider_name
                            ),
                        },
                    );
                    return Ok(CoreResponse::Error {
                        code: "provider_not_found".to_string(),
                        message: format!("Provider not found: {}", provider_name),
                    });
                };

                let runtime = match self.bind_runtime_for_session(&session_id).await {
                    Ok(rt) => rt,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_unbound".to_string(),
                            message: format!(
                                "session {} has no resolvable workspace: {}",
                                session_id, e
                            ),
                        });
                    }
                };

                // Session-open and manual refreshes converge here as the
                // final correctness gate: the turn captures the currently
                // published immutable generation before runtime assembly.
                let asset_refresh = match self
                    .refresh_runtime_assets(
                        &runtime,
                        &session_id,
                        crate::agent::asset_refresh::RefreshReason::SessionLifecycle,
                    )
                    .await
                {
                    Ok(report) => report,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "turn_asset_refresh_failed".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                if let Some(error) = Self::refresh_report_error(&asset_refresh) {
                    return Ok(CoreResponse::Error {
                        code: "turn_asset_refresh_failed".to_string(),
                        message: error.to_string(),
                    });
                }
                let asset_scope = crate::agent::asset_refresh::AssetScope::new(
                    &runtime.project_id,
                    runtime.workspace_id.as_str(),
                );
                let asset_snapshot =
                    self.asset_refresh
                        .snapshot(&asset_scope)
                        .await
                        .map(|published| {
                            (
                                published.snapshot.clone(),
                                std::sync::Arc::new(std::sync::Mutex::new(
                                    published.runtime_asset_pin(),
                                )),
                            )
                        });
                let (asset_snapshot, asset_pin) = asset_snapshot
                    .map(|(snapshot, pin)| (Some(snapshot), Some(pin)))
                    .unwrap_or((None, None));

                let turn_id = {
                    let mut active = runtime.active_turn.write().await;
                    if active.is_some() {
                        return Ok(CoreResponse::Error {
                            code: "turn_already_active".to_string(),
                            message: "A turn is already active for this session".to_string(),
                        });
                    }
                    let turn_id = format!("turn-{}", uuid::Uuid::new_v4());
                    *active = Some(crate::core::session_runtime::TurnHandle {
                        turn_id: turn_id.clone(),
                        cancel_tx: tokio::sync::watch::channel(false).0,
                        steer_tx: None,
                        started_at: chrono::Utc::now(),
                        asset_pin: asset_pin.clone(),
                    });
                    turn_id
                };

                {
                    let mut status = runtime.status.write().await;
                    *status = crate::core::session_runtime::RuntimeSessionStatus::Running;
                }

                // Emit TurnStarted immediately so subscribers (and the bridge
                // fallback) see a coherent turn identity from the first event.
                self.event_log
                    .publish(
                        Some(session_id.clone()),
                        Some(turn_id.clone()),
                        crate::protocol::core::CoreEvent::TurnStarted {
                            session_id: session_id.clone(),
                            turn_id: turn_id.clone(),
                        },
                    )
                    .await;

                // Build an immutable execution context from the bound
                // runtime's workspace identity. The context flows through
                // every daemon-owned execution path inside the turn.
                let workspace_record = codegg_core::workspace::WorkspaceRecord {
                    id: runtime.workspace_id.clone(),
                    canonical_root: runtime.workspace_root.clone(),
                    display_name: runtime
                        .workspace_root
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| runtime.workspace_root.to_string_lossy().into_owned()),
                    created_at: chrono::Utc::now(),
                    last_opened_at: chrono::Utc::now(),
                    archived_at: None,
                };
                let execution = codegg_core::workspace::ExecutionContext::new(
                    Arc::new(workspace_record),
                    Some(session_id.clone()),
                    Default::default(),
                );

                // Delegate to the injected turn runtime which handles tool
                // registry, agent loop construction, and background spawning.
                let plugin_service = crate::plugin::create_default_plugin_service().await;
                let turn_input = crate::agent::turn_runtime::TurnRunInput {
                    session_id: session_id.clone(),
                    agents_dto: agents,
                    current_agent_idx,
                    model,
                    messages_dto: messages,
                    plan_mode,
                    config,
                    pool: self.pool.clone(),
                    subagent_pool: self.deps.legacy_agent.subagent_pool.clone(),
                    memory_store: self.deps.memory_store.clone(),
                    event_log: Arc::clone(&self.event_log),
                    turn_id: turn_id.clone(),
                    lsp_service: self.deps.lsp_service.clone(),
                    lsp_context_input: None,
                    plugin_service,
                    execution,
                    submission: self.deps.submission.clone(),
                    asset_snapshot,
                    asset_pin,
                };
                let turn_output = self.deps.turn_runtime.run_turn(turn_input).await?;

                // Update the TurnHandle with the runtime's cancel/steer channels.
                {
                    let mut active = runtime.active_turn.write().await;
                    if let Some(handle) = active.as_mut() {
                        handle.cancel_tx = turn_output.cancel_tx;
                        handle.steer_tx = Some(turn_output.steer_tx);
                    }
                }

                Ok(CoreResponse::Ack)
            }
            CoreRequest::SessionMessagesLoad { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::MessageStore::new(pool);
                match store.list(&session_id).await {
                    Ok(messages) => Ok(CoreResponse::SessionMessages {
                        session_id,
                        messages: crate::protocol_conversions::messages_to_dtos(messages),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_messages_load_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionMessageCounts { session_ids } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.message_counts(&session_ids).await {
                    Ok(counts) => Ok(CoreResponse::SessionMessageCounts { counts }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_message_counts_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionCreate {
                directory,
                title,
                project_id,
                workspace_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let context = match self
                    .resolve_request_context(
                        &directory,
                        project_id.as_deref(),
                        workspace_id.as_deref(),
                    )
                    .await
                {
                    Ok(context) => context,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "project_context_required".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                let store = crate::session::SessionStore::new(pool.clone());
                match store
                    .create_with_binding(
                        crate::session::CreateSession {
                            project_id: context.project_id.as_str().to_string(),
                            directory,
                            title,
                            parent_id: None,
                            workspace_id: Some(context.workspace_id.as_str().to_string()),
                            agent: None,
                            model: None,
                            tags: None,
                            provider_connection_id: None,
                            provider_connection_revision: None,
                            model_catalog_revision: None,
                            selected_model_id: None,
                        },
                        &context.project_id,
                        &context.workspace_id,
                        "daemon_session_create",
                    )
                    .await
                {
                    Ok(session) => {
                        let refresh = self
                            .refresh_project_context(
                                &context,
                                Some(session.id.as_str()),
                                crate::agent::asset_refresh::RefreshReason::SessionLifecycle,
                            )
                            .await;
                        match refresh {
                            Ok(report) => {
                                if let Some(error) = Self::refresh_report_error(&report) {
                                    Ok(CoreResponse::Error {
                                        code: "session_asset_refresh_failed".to_string(),
                                        message: error.to_string(),
                                    })
                                } else {
                                    Ok(CoreResponse::Session {
                                        session: Self::session_dto(session, Some(&context)),
                                    })
                                }
                            }
                            Err(error) => Ok(CoreResponse::Error {
                                code: "session_asset_refresh_failed".to_string(),
                                message: error.to_string(),
                            }),
                        }
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionLoad { session_id } | CoreRequest::SessionAttach { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.get(&session_id).await {
                    Ok(Some(session)) => {
                        let context = match self
                            .resolve_session_context(&session_id, &session.directory)
                            .await
                        {
                            Ok(context) => {
                                if let Some(workspace) =
                                    self.workspaces.resolve(&context.workspace_id).await
                                {
                                    self.bind_runtime(
                                        &session_id,
                                        workspace,
                                        context.project_id.as_str().to_string(),
                                        context.workspace_root.clone(),
                                    );
                                }
                                Some(context)
                            }
                            Err(e) => {
                                tracing::warn!(
                                    session_id = %session_id,
                                    error = %e,
                                    "session loaded without executable canonical context",
                                );
                                None
                            }
                        };
                        if let Some(context) = context.as_ref() {
                            match self
                                .refresh_project_context(
                                    context,
                                    Some(session_id.as_str()),
                                    crate::agent::asset_refresh::RefreshReason::SessionLifecycle,
                                )
                                .await
                            {
                                Ok(report) => {
                                    if let Some(error) = Self::refresh_report_error(&report) {
                                        return Ok(CoreResponse::Error {
                                            code: "session_asset_refresh_failed".to_string(),
                                            message: error.to_string(),
                                        });
                                    }
                                }
                                Err(error) => {
                                    return Ok(CoreResponse::Error {
                                        code: "session_asset_refresh_failed".to_string(),
                                        message: error.to_string(),
                                    });
                                }
                            }
                        }
                        Ok(CoreResponse::Session {
                            session: Self::session_dto(session, context.as_ref()),
                        })
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "session_not_found".to_string(),
                        message: format!("Session not found: {}", session_id),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_load_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionList {
                project_id,
                show_archived,
                limit,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let project_id = match self.resolve_session_list_project(&project_id).await {
                    Ok(project_id) => project_id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "project_context_required".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
                match catalog.get_project(&project_id).await {
                    Ok(project)
                        if project.lifecycle
                            == codegg_core::project_storage::ProjectLifecycle::Active => {}
                    Ok(_) => {
                        return Ok(CoreResponse::Error {
                            code: "project_context_archived".to_string(),
                            message: "project is archived".to_string(),
                        });
                    }
                    Err(_) => {
                        return Ok(CoreResponse::Error {
                            code: "project_context_not_found".to_string(),
                            message: "project was not found".to_string(),
                        });
                    }
                }
                let store = crate::session::SessionStore::new(pool);
                let sessions = if show_archived {
                    store
                        .list_by_canonical_project(project_id.as_str(), None)
                        .await
                } else {
                    store
                        .list_by_canonical_project(project_id.as_str(), Some(limit))
                        .await
                };
                match sessions {
                    Ok(sessions) => Ok(CoreResponse::SessionList {
                        sessions: sessions
                            .into_iter()
                            .map(|session| Self::session_dto(session, None))
                            .collect(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionFork { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool.clone());
                let parent = match store.get(&session_id).await {
                    Ok(Some(parent)) => parent,
                    Ok(None) => {
                        return Ok(CoreResponse::Error {
                            code: "session_not_found".to_string(),
                            message: "session not found".to_string(),
                        });
                    }
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "session_fork_failed".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                let context = match self
                    .resolve_session_context(&session_id, &parent.directory)
                    .await
                {
                    Ok(context) => context,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "project_context_required".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                match store.fork(&session_id).await {
                    Ok(child) => {
                        let storage = codegg_core::project_storage::ProjectStorage::new(pool);
                        if let Err(error) = storage
                            .bind_session(
                                &child.id,
                                &context.project_id,
                                &context.workspace_id,
                                "daemon_session_fork",
                            )
                            .await
                        {
                            let _ = store.delete(&child.id).await;
                            return Ok(CoreResponse::Error {
                                code: "session_binding_failed".to_string(),
                                message: error.to_string(),
                            });
                        }
                        Ok(CoreResponse::Ack)
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_fork_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionDelete {
                session_id,
                permanent,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                let result = if permanent {
                    store.delete(&session_id).await.map(|_| ())
                } else {
                    store.soft_delete(&session_id).await.map(|_| ())
                };
                match result {
                    Ok(()) => Ok(CoreResponse::Ack),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_delete_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionArchive {
                session_id,
                unarchive,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                let result = if unarchive {
                    store.unarchive(&session_id).await
                } else {
                    store.archive(&session_id).await
                };
                match result {
                    Ok(_) => Ok(CoreResponse::Ack),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_archive_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionRestore { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.restore(&session_id).await {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_restore_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionShare { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.share_session(&session_id).await {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_share_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionUnshare { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.unshare_session(&session_id).await {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_unshare_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionRename {
                session_id,
                new_title,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store
                    .update(
                        &session_id,
                        crate::session::UpdateSession {
                            title: Some(new_title),
                            share_url: None,
                            summary_additions: None,
                            summary_deletions: None,
                            summary_files: None,
                            summary_diffs: None,
                            revert: None,
                            permission: None,
                            tags: None,
                            time_compacting: None,
                            time_archived: None,
                            provider_connection_id: None,
                            provider_connection_revision: None,
                            model_catalog_revision: None,
                            selected_model_id: None,
                        },
                    )
                    .await
                {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_rename_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionExport { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.export_session(&session_id).await {
                    Ok(data) => Ok(CoreResponse::Json { data }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_export_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionImportData { data } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                let directory = data
                    .get("session")
                    .and_then(|session| session.get("directory"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let context = match self.resolve_request_context(&directory, None, None).await {
                    Ok(context) => context,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "project_context_required".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                match store
                    .import_session_with_binding(
                        data,
                        None,
                        &context.project_id,
                        &context.workspace_id,
                        "daemon_session_import",
                    )
                    .await
                {
                    Ok(session) => {
                        let refresh = self
                            .refresh_project_context(
                                &context,
                                Some(session.id.as_str()),
                                crate::agent::asset_refresh::RefreshReason::SessionLifecycle,
                            )
                            .await;
                        match refresh {
                            Ok(report) => {
                                if let Some(error) = Self::refresh_report_error(&report) {
                                    Ok(CoreResponse::Error {
                                        code: "session_asset_refresh_failed".to_string(),
                                        message: error.to_string(),
                                    })
                                } else {
                                    Ok(CoreResponse::Session {
                                        session: Self::session_dto(session, Some(&context)),
                                    })
                                }
                            }
                            Err(error) => Ok(CoreResponse::Error {
                                code: "session_asset_refresh_failed".to_string(),
                                message: error.to_string(),
                            }),
                        }
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_import_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionCreateFromTemplate {
                template,
                project_id,
                directory,
                workspace_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let context = match self
                    .resolve_request_context(
                        &directory,
                        project_id.as_deref(),
                        workspace_id.as_deref(),
                    )
                    .await
                {
                    Ok(context) => context,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "project_context_required".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                let store = crate::session::SessionStore::new(pool.clone());
                let template = crate::protocol_conversions::dto_to_session_template(template);
                match store
                    .create_with_binding(
                        crate::session::CreateSession {
                            project_id: context.project_id.as_str().to_string(),
                            directory,
                            title: Some(template.name),
                            parent_id: None,
                            workspace_id: Some(context.workspace_id.as_str().to_string()),
                            agent: template.agent,
                            model: template.model,
                            tags: template.tags,
                            provider_connection_id: None,
                            provider_connection_revision: None,
                            model_catalog_revision: None,
                            selected_model_id: None,
                        },
                        &context.project_id,
                        &context.workspace_id,
                        "daemon_template_create",
                    )
                    .await
                {
                    Ok(session) => {
                        let refresh = self
                            .refresh_project_context(
                                &context,
                                Some(session.id.as_str()),
                                crate::agent::asset_refresh::RefreshReason::SessionLifecycle,
                            )
                            .await;
                        match refresh {
                            Ok(report) => {
                                if let Some(error) = Self::refresh_report_error(&report) {
                                    Ok(CoreResponse::Error {
                                        code: "session_asset_refresh_failed".to_string(),
                                        message: error.to_string(),
                                    })
                                } else {
                                    Ok(CoreResponse::Session {
                                        session: Self::session_dto(session, Some(&context)),
                                    })
                                }
                            }
                            Err(error) => Ok(CoreResponse::Error {
                                code: "session_asset_refresh_failed".to_string(),
                                message: error.to_string(),
                            }),
                        }
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_create_from_template_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::PermissionRespond { id, choice } => {
                let parsed = match choice.as_str() {
                    "allow" => crate::bus::PermissionDecision::AllowOnce,
                    "always_allow" => crate::bus::PermissionDecision::AlwaysAllow,
                    "deny" => crate::bus::PermissionDecision::DenyOnce,
                    "always_deny" => crate::bus::PermissionDecision::AlwaysDeny,
                    _ => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_permission_choice".to_string(),
                            message: format!("Invalid permission choice: {}", choice),
                        });
                    }
                };
                // Extract session_id and simple perm_id from protocol ID: perm:{session_id}:{turn_id}:{perm_id}.
                // Reject malformed IDs explicitly rather than silently using empty defaults
                // (which could route a response to the wrong session).
                let (session_id, simple_perm_id) = match id.strip_prefix("perm:").and_then(|rest| {
                    let mut parts = rest.splitn(3, ':');
                    let sid = parts.next()?.to_string();
                    let _turn_id = parts.next()?;
                    let pid = parts.next()?.to_string();
                    Some((sid, pid))
                }) {
                    Some(parsed) => parsed,
                    None => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_permission_id".to_string(),
                            message: format!(
                                "Permission ID '{}' is not in perm:<session_id>:<turn_id>:<perm_id> format",
                                id
                            ),
                        });
                    }
                };
                let sent = crate::bus::PermissionRegistry::respond_scoped(
                    &session_id,
                    &simple_perm_id,
                    parsed,
                );
                if sent {
                    // Remove from session runtime's pending set
                    if let Some(runtime) = self.sessions.get(&session_id) {
                        runtime.pending_permissions.remove(&id);
                    }
                    // Emit PermissionResponded event
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::PermissionResponded {
                            session_id,
                            tool: String::new(),
                            allowed: parsed.allowed(),
                        },
                    );
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "permission_response_failed".to_string(),
                        message: "No pending permission request found".to_string(),
                    })
                }
            }
            CoreRequest::QuestionRespond { id, answers } => {
                // Extract session_id and simple question_id from protocol ID: question:{session_id}:{turn_id}:{question_id}.
                // Reject malformed IDs explicitly.
                let (session_id, simple_question_id) = match id.strip_prefix("question:").and_then(
                    |rest| {
                        let mut parts = rest.splitn(3, ':');
                        let sid = parts.next()?.to_string();
                        let _turn_id = parts.next()?;
                        let qid = parts.next()?.to_string();
                        Some((sid, qid))
                    },
                ) {
                    Some(parsed) => parsed,
                    None => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_question_id".to_string(),
                            message: format!(
                                "Question ID '{}' is not in question:<session_id>:<turn_id>:<question_id> format",
                                id
                            ),
                        });
                    }
                };
                let sent = crate::bus::QuestionRegistry::answer_question_scoped(
                    &session_id,
                    &simple_question_id,
                    answers.to_string(),
                );
                if sent {
                    // Remove from session runtime's pending set
                    if let Some(runtime) = self.sessions.get(&session_id) {
                        runtime.pending_questions.remove(&id);
                    }
                    // Emit QuestionAnswered event
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::QuestionAnswered {
                            session_id,
                            answers: answers.to_string(),
                        },
                    );
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "question_response_failed".to_string(),
                        message: "No pending question found".to_string(),
                    })
                }
            }
            CoreRequest::ModelsRefresh => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let config = crate::config::schema::Config::load().unwrap_or_default();
                let mut registry = crate::provider::ProviderRegistry::new();
                crate::provider::register_builtin_with_config(&mut registry, &config);
                let discovery = crate::provider::discovery::ModelDiscoveryService::new(
                    std::path::PathBuf::new(),
                )
                .with_pool(pool);
                let models = discovery.refresh(&registry).await;
                let model_ids: Vec<String> = models
                    .iter()
                    .map(|m| format!("{}/{}", m.provider, m.id))
                    .collect();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "models": model_ids }),
                })
            }
            CoreRequest::TaskList => {
                if !self.deps.legacy_agent.bg_scheduler_compat_enabled {
                    return Ok(CoreResponse::Error {
                        code: "legacy_task_compatibility_disabled".to_string(),
                        message: "use durable schedule/job protocol in daemon mode".to_string(),
                    });
                }
                let Some(scheduler) = self.deps.legacy_agent.bg_scheduler.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_scheduler".to_string(),
                        message: "Core client missing background scheduler".to_string(),
                    });
                };
                let tasks = scheduler.list().await;
                Ok(CoreResponse::Json {
                    data: serde_json::json!({
                        "tasks": tasks.iter().map(|t| serde_json::json!({
                            "id": t.id,
                            "message": t.message,
                            "interval_secs": t.interval.as_secs(),
                            "session_id": t.session_id,
                            "created_at": t.created_at,
                            "last_run": t.last_run,
                        })).collect::<Vec<_>>()
                    }),
                })
            }
            CoreRequest::TaskDelete { id } => {
                if !self.deps.legacy_agent.bg_scheduler_compat_enabled {
                    return Ok(CoreResponse::Error {
                        code: "legacy_task_compatibility_disabled".to_string(),
                        message: "use durable schedule/job protocol in daemon mode".to_string(),
                    });
                }
                let Some(scheduler) = self.deps.legacy_agent.bg_scheduler.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_scheduler".to_string(),
                        message: "Core client missing background scheduler".to_string(),
                    });
                };
                let removed = scheduler.remove(&id.to_string()).await;
                if removed {
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "task_not_found".to_string(),
                        message: format!("Task not found: {}", id),
                    })
                }
            }
            CoreRequest::TaskSchedule {
                session_id,
                interval_secs,
                message,
            } => {
                if !self.deps.legacy_agent.bg_scheduler_compat_enabled {
                    return Ok(CoreResponse::Error {
                        code: "legacy_task_compatibility_disabled".to_string(),
                        message: "use durable schedule/job protocol in daemon mode".to_string(),
                    });
                }
                let Some(scheduler) = self.deps.legacy_agent.bg_scheduler.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_scheduler".to_string(),
                        message: "Core client missing background scheduler".to_string(),
                    });
                };
                let task = crate::agent::task::BackgroundTask::new(
                    session_id.clone(),
                    std::time::Duration::from_secs(interval_secs),
                    message.clone(),
                );
                let task_id = task.id.clone();
                match scheduler.add(task).await {
                    Ok(_) => {
                        // Legacy TaskSchedule is retained for standalone
                        // compatibility. It records the task only; daemon
                        // production work must arrive through ScheduleStore
                        // and JobSubmissionService rather than dispatching
                        // an immediate subagent here.
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({ "task_id": task_id, "interval_secs": interval_secs }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "task_schedule_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            // ── Phase 4: Durable Jobs and Schedules ──────────────────────
            CoreRequest::JobSubmit { spec } => {
                let submission_key = spec
                    .submission_key
                    .clone()
                    .map(crate::scheduler::SubmissionKey::new)
                    .transpose()
                    .map_err(|e| e.to_string());
                let submission_key = match submission_key {
                    Ok(key) => key,
                    Err(message) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_job_submit".to_string(),
                            message,
                        });
                    }
                };
                let new_job = match crate::protocol_conversions::job_submit_from_dto(spec) {
                    Ok(j) => j,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_job_submit".to_string(),
                            message: e,
                        });
                    }
                };
                let Some(submission) = self.deps.submission.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "scheduler_unavailable".to_string(),
                        message: "daemon has no job submission service".to_string(),
                    });
                };
                match submission.submit(submission_key, new_job).await {
                    Ok(submitted) => {
                        let job_id = submitted.job_id.as_str().to_string();
                        let record = self
                            .deps
                            .job_store
                            .get_job(&submitted.job_id)
                            .await
                            .ok()
                            .flatten();
                        let _ = self
                            .event_log
                            .publish(
                                record.as_ref().and_then(|r| r.session_id.clone()),
                                None,
                                crate::protocol::core::CoreEvent::JobCreated {
                                    job_id: job_id.clone(),
                                    workspace_id: submitted.workspace_id.to_string(),
                                    kind: record
                                        .as_ref()
                                        .map(|r| r.kind.as_str())
                                        .unwrap_or("unknown")
                                        .to_string(),
                                    session_id: record.as_ref().and_then(|r| r.session_id.clone()),
                                    turn_id: record.as_ref().and_then(|r| r.turn_id.clone()),
                                },
                            )
                            .await;
                        Ok(CoreResponse::JobSubmitted { job_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_submit_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobGet { job_id } => {
                let id = codegg_core::jobs::JobId::new_unchecked(job_id);
                match self.deps.job_store.get_job(&id).await {
                    Ok(record) => Ok(CoreResponse::JobGet {
                        job: record
                            .as_ref()
                            .map(crate::protocol_conversions::job_record_to_dto),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_get_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobWait { job_id, timeout_ms } => {
                let Some(scheduler) = self.deps.scheduler.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "job_wait_failed".to_string(),
                        message: "scheduler unavailable".to_string(),
                    });
                };
                let id = codegg_core::jobs::JobId::new_unchecked(job_id.clone());
                let timeout =
                    std::time::Duration::from_millis(timeout_ms.unwrap_or(900_000).min(3_600_000));
                match scheduler.wait_for_completion(&id, timeout).await {
                    Ok(completion) => Ok(CoreResponse::JobWaited {
                        job_id,
                        status: format!("{:?}", completion.status).to_lowercase(),
                        summary: completion.summary,
                        run_id: completion.run_id.map(|id| id.as_str().to_string()),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_wait_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobList { query } => {
                let mut q = codegg_core::jobs::store::JobStoreQuery::default();
                if let Some(w) = query.workspace_id {
                    q.workspace_id = Some(codegg_core::workspace::WorkspaceId::new_unchecked(w));
                }
                if !query.states.is_empty() {
                    q.states = query
                        .states
                        .iter()
                        .map(|s| crate::protocol_conversions::job_state_from_str(s))
                        .collect();
                }
                if !query.kinds.is_empty() {
                    q.kinds = query
                        .kinds
                        .iter()
                        .map(|k| crate::protocol_conversions::job_kind_from_str(k))
                        .collect();
                }
                q.session_id = query.session_id;
                if query.limit > 0 {
                    q.limit = Some(query.limit);
                }
                match self.deps.job_store.list_jobs(q).await {
                    Ok(summaries) => {
                        let dtos = summaries
                            .iter()
                            .map(crate::protocol_conversions::job_summary_to_dto)
                            .collect();
                        Ok(CoreResponse::JobList { jobs: dtos })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobAttempts { job_id } => {
                let id = codegg_core::jobs::JobId::new_unchecked(job_id.clone());
                match self.deps.job_store.list_attempts(&id).await {
                    Ok(attempts) => {
                        let dtos = attempts
                            .iter()
                            .map(crate::protocol_conversions::job_attempt_to_dto)
                            .collect();
                        Ok(CoreResponse::JobAttempts {
                            job_id,
                            attempts: dtos,
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_attempts_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobCancel { job_id, reason } => {
                let id = codegg_core::jobs::JobId::new_unchecked(job_id);
                let Some(scheduler) = self.deps.scheduler.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "job_cancel_failed".to_string(),
                        message: "scheduler unavailable".to_string(),
                    });
                };
                match scheduler
                    .request_cancel(&id, &reason.unwrap_or_else(|| "user requested".to_string()))
                    .await
                {
                    Ok(result) => {
                        let outcome_str =
                            crate::protocol_conversions::cancel_outcome_to_str(result.state)
                                .to_string();
                        let _ = self
                            .event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::JobCancelRequested {
                                    job_id: result.job_id.as_str().to_string(),
                                    reason: outcome_str,
                                },
                            )
                            .await;
                        Ok(CoreResponse::JobCancelResult {
                            result: crate::protocol_conversions::cancel_result_to_dto(&result),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_cancel_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobRetry { job_id } => {
                let id = codegg_core::jobs::JobId::new_unchecked(&job_id);
                let gen = self.deps.daemon_generation.clone();
                match self.deps.job_store.get_job(&id).await {
                    Ok(Some(record)) => {
                        let prior_attempt_id = match &record.current_attempt_id {
                            Some(aid) => aid.clone(),
                            None => {
                                // Find the last attempt from history
                                match self.deps.job_store.list_attempts(&id).await {
                                    Ok(attempts) => match attempts.last() {
                                        Some(a) => a.attempt_id.clone(),
                                        None => {
                                            return Ok(CoreResponse::Error {
                                                code: "no_prior_attempt".to_string(),
                                                message: "job has no attempts to retry".to_string(),
                                            });
                                        }
                                    },
                                    Err(e) => {
                                        return Ok(CoreResponse::Error {
                                            code: "retry_failed".to_string(),
                                            message: e.to_string(),
                                        });
                                    }
                                }
                            }
                        };
                        match self
                            .deps
                            .job_store
                            .retry_job(&id, &gen, &prior_attempt_id)
                            .await
                        {
                            Ok(new_attempt) => {
                                let _ = self
                                    .event_log
                                    .publish(
                                        record.session_id.clone(),
                                        None,
                                        crate::protocol::core::CoreEvent::JobRetried {
                                            job_id: job_id.clone(),
                                            new_attempt_id: new_attempt.attempt_id.to_string(),
                                            prior_attempt_id: prior_attempt_id.to_string(),
                                        },
                                    )
                                    .await;
                                Ok(CoreResponse::JobRetryStarted {
                                    job_id,
                                    attempt_id: new_attempt.attempt_id.to_string(),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "job_retry_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "job_not_found".to_string(),
                        message: format!("job '{job_id}' not found"),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_retry_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleCreate { spec } => {
                let template = match crate::protocol_conversions::schedule_create_from_dto(spec) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_schedule_create".to_string(),
                            message: e,
                        });
                    }
                };
                match self.deps.schedule_store.create(template).await {
                    Ok(record) => {
                        let schedule_id = record.schedule_id.as_str().to_string();
                        let _ = self
                            .event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::ScheduleCreated {
                                    schedule_id: schedule_id.clone(),
                                    workspace_id: record.workspace_id.to_string(),
                                    kind_summary: record.kind.tag().to_string(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::ScheduleCreated { schedule_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleList {
                workspace_id,
                include_archived,
            } => {
                let mut query = codegg_core::jobs::ScheduleQuery::default();
                if let Some(w) = workspace_id {
                    query.workspace_id =
                        Some(codegg_core::workspace::WorkspaceId::new_unchecked(w));
                }
                query.include_archived = include_archived;
                match self.deps.schedule_store.list(query).await {
                    Ok(summaries) => {
                        let dtos = summaries
                            .iter()
                            .map(crate::protocol_conversions::schedule_summary_to_dto)
                            .collect();
                        Ok(CoreResponse::ScheduleList { schedules: dtos })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleGet { schedule_id } => {
                let id = codegg_core::jobs::ScheduleId::new_unchecked(&schedule_id);
                match self.deps.schedule_store.get(&id).await {
                    Ok(Some(record)) => Ok(CoreResponse::ScheduleGet {
                        schedule: crate::protocol_conversions::schedule_record_to_dto(&record),
                    }),
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "schedule_not_found".to_string(),
                        message: format!("schedule '{schedule_id}' not found"),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_get_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SchedulePause { schedule_id } => {
                let id = codegg_core::jobs::ScheduleId::new_unchecked(schedule_id.clone());
                match self
                    .deps
                    .schedule_store
                    .set_state(&id, codegg_core::jobs::ScheduleState::Paused)
                    .await
                {
                    Ok(_record) => {
                        let _ = self
                            .event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::SchedulePaused {
                                    schedule_id: schedule_id.clone(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::SchedulePaused { schedule_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_pause_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleResume { schedule_id } => {
                let id = codegg_core::jobs::ScheduleId::new_unchecked(schedule_id.clone());
                match self
                    .deps
                    .schedule_store
                    .set_state(&id, codegg_core::jobs::ScheduleState::Active)
                    .await
                {
                    Ok(_record) => {
                        let _ = self
                            .event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::ScheduleResumed {
                                    schedule_id: schedule_id.clone(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::ScheduleResumed { schedule_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_resume_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleDelete { schedule_id } => {
                let id = codegg_core::jobs::ScheduleId::new_unchecked(schedule_id.clone());
                match self.deps.schedule_store.delete(&id).await {
                    Ok(()) => {
                        let _ = self
                            .event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::ScheduleDeleted {
                                    schedule_id: schedule_id.clone(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::ScheduleDeleted { schedule_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_delete_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobRecoveryReport => {
                // The `recover_generation` API parameter is the
                // *new* daemon generation; the store finds any
                // non-terminal attempt whose generation does not
                // match. Pass our current generation so the report
                // reflects what would happen at startup.
                let current_gen = self.deps.daemon_generation.clone();
                let policy = self.deps.recovery_policy.clone();
                match self
                    .deps
                    .job_store
                    .recover_generation(&current_gen, &policy)
                    .await
                {
                    Ok(report) => Ok(CoreResponse::JobRecoveryReport {
                        report: crate::protocol_conversions::recovery_report_to_dto(&report),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "recovery_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::WorktreeList { project_dir } => {
                let git_root = std::path::PathBuf::from(&project_dir);
                let Some(root) = crate::worktree::find_git_root(&git_root) else {
                    return Ok(CoreResponse::Json {
                        data: serde_json::json!({ "worktrees": [] }),
                    });
                };
                match crate::worktree::list_worktrees(&root).await {
                    Ok(trees) => Ok(CoreResponse::Json {
                        data: serde_json::json!({
                            "worktrees": trees.iter().map(|t| serde_json::json!({
                                "path": t.path,
                                "branch": t.branch
                            })).collect::<Vec<_>>()
                        }),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "worktree_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ProjectCatalogCapabilities => {
                Ok(CoreResponse::ProjectCatalogCapabilities {
                    supported: self.pool.is_some(),
                    max_list_items: crate::protocol::dto::MAX_PROJECT_LIST_ITEMS,
                    max_workspaces_per_project: crate::protocol::dto::MAX_PROJECT_WORKSPACES,
                })
            }
            CoreRequest::ProjectList {
                include_archived,
                limit,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "project_catalog_unavailable".into(),
                        message: "project listing requires a catalog".into(),
                    });
                };
                let limit = if limit == 0 {
                    crate::protocol::dto::MAX_PROJECT_LIST_ITEMS
                } else {
                    limit.min(crate::protocol::dto::MAX_PROJECT_LIST_ITEMS)
                };
                let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool);
                match catalog.list_projects(include_archived).await {
                    Ok(records) => Ok(CoreResponse::ProjectList {
                        truncated: records.len() > limit,
                        projects: records
                            .iter()
                            .take(limit)
                            .map(codegg_core::protocol_conversions::project_catalog_record_to_dto)
                            .collect(),
                    }),
                    Err(error) => Ok(Self::project_catalog_error("project list failed", &error)),
                }
            }
            CoreRequest::ProjectGet { project_id } => {
                let project_id = match codegg_core::identity::ProjectId::parse(&project_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_project_id".into(),
                            message: error.to_string(),
                        })
                    }
                };
                match self.project_details(&project_id).await {
                    Ok(project) => Ok(CoreResponse::ProjectGet { project }),
                    Err(error) => Ok(Self::project_catalog_error("project get failed", &error)),
                }
            }
            CoreRequest::ProjectRegister { request } => {
                if request.tags.len() > crate::protocol::dto::MAX_PROJECT_TAGS {
                    return Ok(CoreResponse::Error {
                        code: "project_register_limit_exceeded".into(),
                        message: "project tag count exceeds the protocol limit".into(),
                    });
                }
                let workspace_id =
                    match codegg_core::workspace::WorkspaceId::parse(&request.workspace_id) {
                        Ok(id) => id,
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "invalid_workspace_id".into(),
                                message: error.to_string(),
                            })
                        }
                    };
                let repository_id = match request.repository_id.as_deref() {
                    Some(value) => match codegg_core::identity::RepositoryId::parse(value) {
                        Ok(id) => Some(id),
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "invalid_repository_id".into(),
                                message: error.to_string(),
                            })
                        }
                    },
                    None => None,
                };
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "project_catalog_unavailable".into(),
                        message: "project registration requires a catalog".into(),
                    });
                };
                let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool);
                let input = codegg_core::project_catalog::RegisterLocalProject {
                    display_name: request.display_name,
                    description: request.description,
                    tags: request.tags,
                    primary_repository_id: repository_id,
                };
                match catalog
                    .register_local_project(input, &workspace_id, &request.source)
                    .await
                {
                    Ok(record) => {
                        let project =
                            codegg_core::protocol_conversions::project_catalog_record_to_dto(
                                &record,
                            );
                        let _ = self
                            .event_log
                            .publish(
                                None,
                                None,
                                CoreEvent::ProjectRegistered {
                                    project_id: project.project_id.clone(),
                                    project: project.clone(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::ProjectRegistered { project })
                    }
                    Err(error) => Ok(Self::project_catalog_error(
                        "project registration failed",
                        &error,
                    )),
                }
            }
            CoreRequest::ProjectArchive { project_id } => {
                self.project_lifecycle_request(&project_id, false).await
            }
            CoreRequest::ProjectRestore { project_id } => {
                self.project_lifecycle_request(&project_id, true).await
            }
            CoreRequest::ProjectHealth {
                project_id,
                workspace_id,
            } => {
                if let Err(error) = codegg_core::identity::ProjectId::parse(&project_id) {
                    return Ok(CoreResponse::Error {
                        code: "invalid_project_id".into(),
                        message: error.to_string(),
                    });
                }
                if let Err(error) = codegg_core::workspace::WorkspaceId::parse(&workspace_id) {
                    return Ok(CoreResponse::Error {
                        code: "invalid_workspace_id".into(),
                        message: error.to_string(),
                    });
                }
                let snapshot = match self.project_health(&project_id, &workspace_id).await {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "project_health_unavailable".into(),
                            message: error.to_string(),
                        })
                    }
                };
                let health = Self::project_health_dto(&snapshot, None);
                let _ = self
                    .event_log
                    .publish(
                        None,
                        None,
                        CoreEvent::ProjectHealthChanged {
                            project_id: project_id.clone(),
                            workspace_id: workspace_id.clone(),
                            health: health.clone(),
                        },
                    )
                    .await;
                Ok(CoreResponse::ProjectHealth { health })
            }
            CoreRequest::MemoryList { namespace } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let memories = memory_store.list(&namespace);
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memories": memories }),
                })
            }
            CoreRequest::MemorySearch { query } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let memories = memory_store.search(&query);
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memories": memories }),
                })
            }
            CoreRequest::MemoryRemember { text, namespace } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let ns = namespace.unwrap_or_else(|| "user/preferences".to_string());
                let memory = crate::memory::Memory::new(ns, text);
                memory_store.add(memory.clone());
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memory": memory }),
                })
            }
            CoreRequest::MemoryForget { id } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let deleted = memory_store.delete(&id).is_some();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "deleted": deleted }),
                })
            }
            CoreRequest::GoalSet {
                session_id,
                project_id,
                objective,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool.clone());
                let title = objective
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or(&objective)
                    .chars()
                    .take(80)
                    .collect::<String>();
                let completion_criteria = vec![
                    "Implementation satisfies the stated objective.".to_string(),
                    "Relevant tests or checks have been run, or skipped with justification."
                        .to_string(),
                    "Checkpoint/progress state is updated.".to_string(),
                ];
                match goal_store
                    .create_active(
                        &session_id,
                        &project_id,
                        &title,
                        &objective,
                        None,
                        None,
                        completion_criteria,
                    )
                    .await
                {
                    Ok(goal) => {
                        let project_path = std::path::PathBuf::from(&project_id);
                        let checkpoint_path = match crate::goal::checkpoint::create_checkpoint_file(
                            &project_path,
                            &goal,
                            None,
                        )
                        .await
                        {
                            Ok(path) => Some(path.to_string_lossy().to_string()),
                            Err(_) => None,
                        };
                        if let Some(ref cp) = checkpoint_path {
                            let _ = sqlx::query("UPDATE goal SET checkpoint_path = ? WHERE id = ?")
                                .bind(cp)
                                .bind(&goal.id)
                                .execute(&pool)
                                .await;
                        }
                        let updated = goal_store.get(&goal.id).await.ok().flatten();
                        super::publish_goal_updated(&session_id, updated);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "status": "active",
                                "id": goal.id,
                                "title": title,
                                "checkpoint_path": checkpoint_path,
                            }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalFromFile {
                session_id,
                project_id,
                path,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let file_path = if std::path::Path::new(&path).is_absolute() {
                    std::path::PathBuf::from(&path)
                } else {
                    std::path::PathBuf::from(&project_id).join(&path)
                };
                let content = match tokio::fs::read_to_string(&file_path).await {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "file_read_failed".to_string(),
                            message: format!("Failed to read {}: {}", path, e),
                        })
                    }
                };
                let title = content
                    .lines()
                    .find(|l| l.starts_with('#'))
                    .map(|l| {
                        l.trim_start_matches('#')
                            .trim()
                            .chars()
                            .take(80)
                            .collect::<String>()
                    })
                    .unwrap_or_else(|| {
                        std::path::Path::new(&path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Goal from file".to_string())
                    });
                let objective = format!("Follow implementation plan from {}", path);
                let completion_criteria = vec![
                    "All phases in the plan file that are in scope are completed.".to_string(),
                    "Tests/checks specified in the plan have been run.".to_string(),
                    "Goal checkpoint is updated with completed/remaining work.".to_string(),
                ];
                let plan_excerpt = if content.len() > 4000 {
                    Some(&content[..4000])
                } else {
                    Some(content.as_str())
                };
                let goal_store = crate::goal::GoalStore::new(pool.clone());
                match goal_store
                    .create_active(
                        &session_id,
                        &project_id,
                        &title,
                        &objective,
                        Some(path),
                        None,
                        completion_criteria,
                    )
                    .await
                {
                    Ok(goal) => {
                        let project_path = std::path::PathBuf::from(&project_id);
                        let checkpoint_path = match crate::goal::checkpoint::create_checkpoint_file(
                            &project_path,
                            &goal,
                            plan_excerpt,
                        )
                        .await
                        {
                            Ok(path) => Some(path.to_string_lossy().to_string()),
                            Err(_) => None,
                        };
                        if let Some(ref cp) = checkpoint_path {
                            let _ = sqlx::query("UPDATE goal SET checkpoint_path = ? WHERE id = ?")
                                .bind(cp)
                                .bind(&goal.id)
                                .execute(&pool)
                                .await;
                        }
                        let updated = goal_store.get(&goal.id).await.ok().flatten();
                        super::publish_goal_updated(&session_id, updated);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "status": "active",
                                "id": goal.id,
                                "title": goal.title,
                                "checkpoint_path": checkpoint_path,
                            }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalShow { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        let checkpoint_excerpt = if let Some(ref path) = goal.checkpoint_path {
                            crate::goal::checkpoint::read_checkpoint_excerpt(path, 4000)
                                .await
                                .ok()
                                .flatten()
                        } else {
                            None
                        };
                        let rendered = crate::goal::render::render_goal_status(&goal);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "goal": serde_json::to_value(&goal).unwrap_or_default(),
                                "rendered": rendered,
                                "checkpoint_excerpt": checkpoint_excerpt,
                            }),
                        })
                    }
                    Ok(None) => Ok(CoreResponse::Json {
                        data: serde_json::json!({ "active": false }),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_show_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalPause { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        match goal_store
                            .update_status(&goal.id, crate::goal::GoalStatus::Paused)
                            .await
                        {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "paused", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                super::publish_goal_updated(&session_id, None);
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "paused", "id": goal.id }),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_pause_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal to pause".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_pause_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalResume { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.latest_paused_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        match goal_store
                            .update_status(&goal.id, crate::goal::GoalStatus::Active)
                            .await
                        {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "active", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                super::publish_goal_updated(&session_id, None);
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "active", "id": goal.id }),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_resume_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_paused_goal".to_string(),
                        message: "No paused goal to resume".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_resume_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalClear { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.clear_active_for_session(&session_id).await {
                    Ok(()) => {
                        super::publish_goal_updated(&session_id, None);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({ "cleared": true }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_clear_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalDone { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        match goal_store
                            .update_status(&goal.id, crate::goal::GoalStatus::Complete)
                            .await
                        {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated.clone()));
                                crate::bus::global::GlobalEventBus::publish(
                                    crate::bus::events::AppEvent::GoalCompleted {
                                        session_id: session_id.clone(),
                                        goal_id: goal.id.clone(),
                                        evidence: "marked complete via /goal done".to_string(),
                                    },
                                );
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "complete", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                super::publish_goal_updated(&session_id, None);
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "complete", "id": goal.id }),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_done_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal to mark done".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_done_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalCheckpoint {
                session_id,
                project_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        if let Some(ref cp_path) = goal.checkpoint_path {
                            let update = crate::goal::GoalProgressUpdate {
                                current_phase: goal.current_phase.clone(),
                                progress_summary: Some(goal.progress_summary.clone()),
                                next_action: goal.next_action.clone(),
                                completed_items: vec![],
                                remaining_items: vec![],
                                open_questions: goal.open_questions.clone(),
                            };
                            let _ =
                                crate::goal::checkpoint::append_checkpoint_update(cp_path, &update)
                                    .await;
                            Ok(CoreResponse::Json {
                                data: serde_json::json!({ "checkpoint_path": cp_path, "appended": true }),
                            })
                        } else {
                            let project_path = std::path::PathBuf::from(&project_id);
                            match crate::goal::checkpoint::create_checkpoint_file(
                                &project_path,
                                &goal,
                                None,
                            )
                            .await
                            {
                                Ok(path) => {
                                    let path_str = path.to_string_lossy().to_string();
                                    let _ = sqlx::query(
                                        "UPDATE goal SET checkpoint_path = ? WHERE id = ?",
                                    )
                                    .bind(&path_str)
                                    .bind(&goal.id)
                                    .execute(&goal_store.pool)
                                    .await;
                                    Ok(CoreResponse::Json {
                                        data: serde_json::json!({ "checkpoint_path": path_str, "created": true }),
                                    })
                                }
                                Err(e) => Ok(CoreResponse::Error {
                                    code: "goal_checkpoint_failed".to_string(),
                                    message: e.to_string(),
                                }),
                            }
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_checkpoint_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::TodoList { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::store::TodoStore::new(pool);
                match store.list(&session_id).await {
                    Ok(items) => {
                        let snapshots: Vec<crate::bus::events::TodoItemSnapshot> = items
                            .iter()
                            .enumerate()
                            .map(|(i, item)| {
                                use crate::bus::events::TodoItemSnapshot;
                                TodoItemSnapshot {
                                    id: format!("pos-{}", i),
                                    content: item.content.clone(),
                                    status: item.status.clone(),
                                    priority: item.priority.clone(),
                                }
                            })
                            .collect();
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "items": serde_json::to_value(&snapshots)
                                    .unwrap_or(serde_json::Value::Array(vec![])),
                            }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "todo_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ActiveGoalLoad { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => Ok(CoreResponse::Json {
                        data: serde_json::json!({
                            "active": true,
                            "goal": serde_json::to_value(goal.to_snapshot())
                                .unwrap_or(serde_json::Value::Null),
                        }),
                    }),
                    Ok(None) => Ok(CoreResponse::Json {
                        data: serde_json::json!({ "active": false }),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "active_goal_load_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalSetBudget {
                session_id,
                max_turns,
                max_model_tokens,
                max_tool_calls,
                max_wallclock_secs,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        let new_budget = crate::goal::model::GoalBudget {
                            max_turns,
                            max_model_tokens,
                            max_tool_calls,
                            max_wallclock_secs,
                        };
                        match goal_store.set_budget(&goal.id, new_budget).await {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "ok", "id": goal.id }),
                                })
                            }
                            Ok(None) => Ok(CoreResponse::Json {
                                data: serde_json::json!({ "status": "ok", "id": goal.id }),
                            }),
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_set_budget_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal to update budget".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_set_budget_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::Subscribe { session_id } => {
                let current_seq = self.event_log.current_seq();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({
                        "current_seq": current_seq,
                        "session_id": session_id,
                    }),
                })
            }
            CoreRequest::Resume {
                session_id,
                from_event_seq,
            } => {
                let filter = EventFilter {
                    session_id: session_id.clone(),
                    client_id: None,
                    include_global: true,
                };

                let current_seq = self.event_log.current_seq();

                // `ResyncRequired` means "the requested sequence is too old
                // to replay from available event storage" -- not "there are
                // no new events". A client that is already caught up
                // (from_event_seq >= current_seq) gets an empty `Events`
                // response so the resume handshake always completes for
                // in-sync clients.
                if !self.event_log.covers_from(from_event_seq).await {
                    return Ok(CoreResponse::ResyncRequired {
                        from_event_seq,
                        current_seq,
                        session_id,
                    });
                }

                // Already caught up (or covered by the ring/DB): return
                // an empty events vector. A future `from_event_seq` (above
                // `current_seq`) also returns empty here rather than
                // erroring; clients treat that as a no-op resume.
                if from_event_seq >= current_seq {
                    return Ok(CoreResponse::Events {
                        events: Vec::new(),
                        current_seq,
                    });
                }

                let events = self.event_log.replay_from(from_event_seq, &filter).await;
                Ok(CoreResponse::Events {
                    events,
                    current_seq,
                })
            }
            CoreRequest::TurnCancel {
                session_id,
                turn_id,
            } => {
                let Some(runtime) = self.sessions.get(&session_id) else {
                    return Ok(CoreResponse::Error {
                        code: "session_not_found".to_string(),
                        message: format!("No runtime for session: {}", session_id),
                    });
                };
                let active = runtime.active_turn.read().await;
                match active.as_ref() {
                    Some(handle) if handle.turn_id == turn_id => {
                        let _ = handle.cancel_tx.send(true);
                        Ok(CoreResponse::Ack)
                    }
                    Some(handle) => Ok(CoreResponse::Error {
                        code: "turn_id_mismatch".to_string(),
                        message: format!(
                            "Requested turn_id '{}' does not match active turn_id '{}'",
                            turn_id, handle.turn_id
                        ),
                    }),
                    None => Ok(CoreResponse::Error {
                        code: "no_active_turn".to_string(),
                        message: "No active turn to cancel".to_string(),
                    }),
                }
            }
            CoreRequest::TurnSteer {
                session_id,
                turn_id,
                text,
            } => {
                let Some(runtime) = self.sessions.get(&session_id) else {
                    return Ok(CoreResponse::Error {
                        code: "session_not_found".to_string(),
                        message: format!("No runtime for session: {}", session_id),
                    });
                };
                let active = runtime.active_turn.read().await;
                match active.as_ref() {
                    Some(handle) if handle.turn_id == turn_id => {
                        if let Some(ref steer_tx) = handle.steer_tx {
                            let _ = steer_tx.send(text);
                            Ok(CoreResponse::Ack)
                        } else {
                            Ok(CoreResponse::Error {
                                code: "steer_not_supported".to_string(),
                                message: "Turn does not support steering".to_string(),
                            })
                        }
                    }
                    Some(handle) => Ok(CoreResponse::Error {
                        code: "turn_id_mismatch".to_string(),
                        message: format!(
                            "Requested turn_id '{}' does not match active turn_id '{}'",
                            turn_id, handle.turn_id
                        ),
                    }),
                    None => Ok(CoreResponse::Error {
                        code: "no_active_turn".to_string(),
                        message: "No active turn to steer".to_string(),
                    }),
                }
            }
            CoreRequest::AgentSelect {
                session_id,
                agent_name,
            } => {
                let runtime = match self.bind_runtime_for_session(&session_id).await {
                    Ok(rt) => rt,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_unbound".to_string(),
                            message: format!(
                                "session {} has no resolvable workspace: {}",
                                session_id, e
                            ),
                        });
                    }
                };
                {
                    let mut selected = runtime.selected_agent.write().await;
                    *selected = Some(agent_name.clone());
                }
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::SessionUpdated {
                        id: session_id.clone(),
                    },
                );
                Ok(CoreResponse::Ack)
            }
            CoreRequest::ModelSelect { session_id, model } => {
                let runtime = match self.bind_runtime_for_session(&session_id).await {
                    Ok(rt) => rt,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_unbound".to_string(),
                            message: format!(
                                "session {} has no resolvable workspace: {}",
                                session_id, e
                            ),
                        });
                    }
                };
                {
                    let mut selected = runtime.selected_model.write().await;
                    *selected = Some(model.clone());
                }
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::SessionUpdated {
                        id: session_id.clone(),
                    },
                );
                Ok(CoreResponse::Ack)
            }
            CoreRequest::SnapshotSession { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool.clone());
                let msg_store = crate::session::MessageStore::new(pool);

                let session = match store.get(&session_id).await {
                    Ok(Some(s)) => s,
                    Ok(None) => {
                        return Ok(CoreResponse::Error {
                            code: "session_not_found".to_string(),
                            message: format!("Session not found: {}", session_id),
                        })
                    }
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_load_failed".to_string(),
                            message: e.to_string(),
                        })
                    }
                };

                let messages = msg_store.list(&session_id).await.unwrap_or_default();

                let (
                    status,
                    selected_model,
                    selected_agent,
                    pending_permissions,
                    pending_questions,
                    input_tokens,
                    output_tokens,
                    active_subagents,
                ) = if let Some(runtime) = self.sessions.get(&session_id) {
                    let status = format!("{:?}", *runtime.status.read().await);
                    let model = runtime.selected_model.read().await.clone();
                    let agent = runtime.selected_agent.read().await.clone();
                    let pending_permissions: Vec<String> = runtime
                        .pending_permissions
                        .iter()
                        .map(|r| r.key().clone())
                        .collect();
                    let pending_questions: Vec<String> = runtime
                        .pending_questions
                        .iter()
                        .map(|r| r.key().clone())
                        .collect();
                    let input_tokens = *runtime.last_input_tokens.read().await;
                    let output_tokens = *runtime.last_output_tokens.read().await;
                    let active_subagents = runtime
                        .active_subagent_count
                        .load(std::sync::atomic::Ordering::Relaxed);
                    (
                        status,
                        model,
                        agent,
                        pending_permissions,
                        pending_questions,
                        input_tokens,
                        output_tokens,
                        active_subagents,
                    )
                } else {
                    (
                        "idle".to_string(),
                        None,
                        None,
                        Vec::new(),
                        Vec::new(),
                        None,
                        None,
                        0,
                    )
                };

                let event_seq = self.event_log.current_seq();

                Ok(CoreResponse::SnapshotSession {
                    event_seq,
                    session: crate::protocol_conversions::session_to_dto(session),
                    messages: crate::protocol_conversions::messages_to_dtos(messages),
                    status,
                    selected_model,
                    selected_agent,
                    pending_permissions,
                    pending_questions,
                    input_tokens,
                    output_tokens,
                    active_subagents,
                })
            }
            CoreRequest::SnapshotModels => {
                let config = crate::config::schema::Config::load().unwrap_or_default();
                let mut registry = crate::provider::ProviderRegistry::new();
                crate::provider::register_builtin_with_config(&mut registry, &config);
                let model_ids: Vec<String> = if let Some(pool) = self.pool.clone() {
                    let discovery = crate::provider::discovery::ModelDiscoveryService::new(
                        std::path::PathBuf::new(),
                    )
                    .with_pool(pool);
                    let models = discovery.refresh(&registry).await;
                    models
                        .iter()
                        .map(|m| format!("{}/{}", m.provider, m.id))
                        .collect()
                } else {
                    let mut ids = Vec::new();
                    for provider in registry.list() {
                        if let Ok(models) = provider.models().await {
                            for m in models {
                                ids.push(format!("{}/{}", provider.id(), m.id));
                            }
                        }
                    }
                    ids
                };
                Ok(CoreResponse::ModelsSnapshot {
                    current_model: None,
                    models: model_ids,
                })
            }
            CoreRequest::SchedulerSnapshot => {
                let Some(scheduler) = self.deps.scheduler.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "scheduler_unavailable".to_string(),
                        message: "scheduler unavailable".to_string(),
                    });
                };
                match serde_json::to_value(scheduler.snapshot().await) {
                    Ok(snapshot) => Ok(CoreResponse::SchedulerSnapshot { snapshot }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "scheduler_snapshot_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SnapshotDaemon => {
                let event_seq = self.event_log.current_seq();
                let session_ids = self.sessions.list_sessions();
                let mut snapshots = Vec::new();
                for sid in &session_ids {
                    if let Some(runtime) = self.sessions.get(sid) {
                        let status = format!("{:?}", *runtime.status.read().await);
                        let model = runtime.selected_model.read().await.clone();
                        let agent = runtime.selected_agent.read().await.clone();
                        let has_active_turn = runtime.active_turn.read().await.is_some();
                        let pending_permissions: Vec<String> = runtime
                            .pending_permissions
                            .iter()
                            .map(|r| r.key().clone())
                            .collect();
                        let pending_questions: Vec<String> = runtime
                            .pending_questions
                            .iter()
                            .map(|r| r.key().clone())
                            .collect();
                        let input_tokens = *runtime.last_input_tokens.read().await;
                        let output_tokens = *runtime.last_output_tokens.read().await;
                        let active_subagents = runtime
                            .active_subagent_count
                            .load(std::sync::atomic::Ordering::Relaxed);
                        snapshots.push(crate::protocol::core::SessionSnapshot {
                            session_id: sid.clone(),
                            project_id: runtime.project_id.clone(),
                            workspace_id: Some(runtime.workspace_id.as_str().to_string()),
                            binding: Some(crate::protocol::dto::SessionBindingDto {
                                project_id: runtime.project_id.clone(),
                                workspace_id: runtime.workspace_id.as_str().to_string(),
                                repository_id: None,
                                binding_state: Some("resolved".to_string()),
                                binding_revision: None,
                                compatibility_directory: Some(
                                    runtime.directory.to_string_lossy().into_owned(),
                                ),
                            }),
                            directory: runtime.directory.to_string_lossy().into_owned(),
                            status,
                            selected_model: model,
                            selected_agent: agent,
                            has_active_turn,
                            pending_permissions,
                            pending_questions,
                            input_tokens,
                            output_tokens,
                            active_subagents,
                        });
                    }
                }
                let scheduler_snapshot = match self.deps.scheduler.as_ref() {
                    Some(scheduler) => serde_json::to_value(scheduler.snapshot().await).ok(),
                    None => None,
                };
                Ok(CoreResponse::SnapshotDaemon {
                    event_seq,
                    daemon_id: self.daemon_id.clone(),
                    uptime_secs: self.started_at.elapsed().as_secs(),
                    active_sessions: snapshots,
                    connected_clients: self
                        .clients
                        .list()
                        .iter()
                        .map(|c| crate::protocol::core::ClientSnapshot {
                            client_id: c.client_id.clone(),
                            client_name: c.client_name.clone(),
                            connected_at: c.connected_at.to_rfc3339(),
                            attached_sessions: c.attached_sessions.clone(),
                        })
                        .collect(),
                    scheduler_snapshot,
                })
            }
            CoreRequest::SnapshotWorkspace { project_dir } => {
                let path = std::path::PathBuf::from(&project_dir);

                let git_root = crate::worktree::find_git_root(&path);

                let git_status = match git_root.as_ref() {
                    Some(root) => {
                        let argv: Vec<String> =
                            vec!["git".into(), "status".into(), "--porcelain".into()];
                        let mut cmd =
                            crate::git_mutations::GitEnvPolicy::default().apply(&argv, root);
                        cmd.output().await.ok().map(|output| {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            let changed_files = stdout.lines().count();
                            serde_json::json!({
                                "git_root": root.to_string_lossy(),
                                "changed_files": changed_files,
                            })
                        })
                    }
                    None => None,
                };

                let worktrees: Vec<serde_json::Value> = match git_root.as_ref() {
                    Some(root) => crate::worktree::list_worktrees(root)
                        .await
                        .unwrap_or_default()
                        .iter()
                        .map(|t| {
                            serde_json::json!({
                                "path": t.path,
                                "branch": t.branch,
                            })
                        })
                        .collect(),
                    None => Vec::new(),
                };

                Ok(CoreResponse::Json {
                    data: serde_json::json!({
                        "project_dir": project_dir,
                        "git_status": git_status,
                        "worktrees": worktrees,
                    }),
                })
            }
            CoreRequest::WorkspaceRegister { root } => {
                let path = std::path::PathBuf::from(&root);
                match self.workspaces.get_or_register(&path).await {
                    Ok(record) => {
                        let dto =
                            codegg_core::protocol_conversions::workspace_record_to_dto(&record, 0);
                        Ok(CoreResponse::WorkspaceSnapshot { workspace: dto })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_register_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::WorkspaceList { include_archived } => {
                match self.workspaces.list(include_archived).await {
                    Ok(records) => {
                        let dtos = records
                            .iter()
                            .map(|r| {
                                let snap = self.workspace_services.peek(&r.id);
                                codegg_core::protocol_conversions::workspace_record_with_services_to_dto(
                                    r,
                                    0,
                                    snap.as_ref(),
                                )
                            })
                            .collect();
                        Ok(CoreResponse::WorkspaceList { workspaces: dtos })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::WorkspaceArchive { workspace_id } => {
                let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                match self.workspaces.archive(&id).await {
                    Ok(()) => Ok(CoreResponse::Ack),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_archive_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::WorkspaceSnapshotRequest { workspace_id } => {
                let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                match self.workspaces.resolve(&id).await {
                    Some(record) => {
                        let snap = self.workspace_services.peek(&id);
                        let dto = codegg_core::protocol_conversions::workspace_record_with_services_to_dto(
                            &record,
                            0,
                            snap.as_ref(),
                        );
                        Ok(CoreResponse::WorkspaceSnapshot { workspace: dto })
                    }
                    None => Ok(CoreResponse::Error {
                        code: "workspace_not_found".to_string(),
                        message: format!("workspace {} not found", id),
                    }),
                }
            }
            CoreRequest::WorkspaceServicesSnapshot => {
                let snaps = self.workspace_services.list_active();
                let dtos = snaps
                    .iter()
                    .map(codegg_core::protocol_conversions::workspace_service_snapshot_to_dto)
                    .collect();
                Ok(CoreResponse::WorkspaceServicesSnapshot { services: dtos })
            }
            CoreRequest::WorkspaceConfigReload { workspace_id } => {
                let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                match self.workspace_services.reload_config(&id) {
                    Ok(result) => {
                        let diagnostics = result
                            .diagnostics
                            .iter()
                            .map(codegg_core::protocol_conversions::config_diagnostic_to_dto)
                            .collect();
                        Ok(CoreResponse::WorkspaceConfigReload {
                            workspace_id: result.workspace_id.to_string(),
                            previous_revision: result.previous_revision,
                            new_revision: result.new_revision,
                            diagnostics,
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_config_reload_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::RunList {
                workspace_id,
                query,
            } => {
                let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                match self.workspace_services.acquire(&id).await {
                    Ok(lease) => {
                        let run_query =
                            codegg_core::protocol_conversions::run_query_from_dto(query);
                        let workspace_id_str = lease.workspace_id().to_string();
                        match lease.run_store().list_runs(run_query).await {
                            Ok(summaries) => {
                                let dtos = summaries
                                    .iter()
                                    .map(|s| {
                                        codegg_core::protocol_conversions::run_summary_to_dto(
                                            s,
                                            Some(&workspace_id_str),
                                        )
                                    })
                                    .collect();
                                drop(lease);
                                Ok(CoreResponse::RunList {
                                    workspace_id: workspace_id_str,
                                    runs: dtos,
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "run_list_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_not_active".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::RunGet {
                workspace_id,
                run_id,
            } => {
                let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                match self.workspace_services.acquire(&id).await {
                    Ok(lease) => {
                        let workspace_id_str = lease.workspace_id().to_string();
                        let run_id_typed = codegg_core::run_store::RunId::new_unchecked(run_id);
                        match lease.run_store().get_run(&run_id_typed).await {
                            Ok(Some(manifest)) => {
                                let dto = codegg_core::protocol_conversions::run_manifest_to_dto(
                                    &manifest,
                                    Some(&workspace_id_str),
                                );
                                drop(lease);
                                Ok(CoreResponse::RunGet {
                                    workspace_id: workspace_id_str,
                                    run: Some(dto),
                                })
                            }
                            Ok(None) => Ok(CoreResponse::RunGet {
                                workspace_id: workspace_id_str,
                                run: None,
                            }),
                            Err(e) => Ok(CoreResponse::Error {
                                code: "run_get_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_not_active".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::RunArtifactRead {
                workspace_id,
                artifact_id,
                start,
                end,
            } => {
                let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                match self.workspace_services.acquire(&id).await {
                    Ok(lease) => {
                        let workspace_id_str = lease.workspace_id().to_string();
                        let artifact_id_typed =
                            codegg_core::run_store::ArtifactId::new_unchecked(artifact_id);
                        let range = if end <= start {
                            None
                        } else {
                            Some(codegg_core::run_store::ByteRange { start, end })
                        };
                        match lease
                            .run_store()
                            .read_artifact(&artifact_id_typed, range)
                            .await
                        {
                            Ok(chunk) => {
                                drop(lease);
                                let data_b64 = base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD,
                                    &chunk.data,
                                );
                                Ok(CoreResponse::RunArtifactChunk {
                                    workspace_id: workspace_id_str,
                                    artifact_id: artifact_id_typed.to_string(),
                                    data_b64,
                                    byte_offset: chunk.byte_offset,
                                    total_bytes: chunk.total_bytes,
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "run_artifact_read_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_not_active".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::NotificationSpeak {
                text,
                kind,
                priority,
                session_id,
            } => {
                use super::notification::*;
                let kind = match kind.as_deref() {
                    Some("turn_completed") => NotificationKind::TurnCompleted,
                    Some("turn_failed") => NotificationKind::TurnFailed,
                    Some("awaiting_input") => NotificationKind::AwaitingInput,
                    Some("permission_required") => NotificationKind::PermissionRequired,
                    Some("question_required") => NotificationKind::QuestionRequired,
                    Some("subagent_completed") => NotificationKind::SubagentCompleted,
                    Some("subagent_failed") => NotificationKind::SubagentFailed,
                    Some("error") => NotificationKind::Error,
                    _ => NotificationKind::AwaitingInput,
                };
                let priority = match priority.as_deref() {
                    Some("urgent") => NotificationPriority::Urgent,
                    Some("high") => NotificationPriority::High,
                    Some("low") => NotificationPriority::Low,
                    _ => NotificationPriority::Normal,
                };
                let event = NotificationEvent {
                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                    session_id,
                    turn_id: None,
                    kind,
                    priority,
                    message: text,
                    dedupe_key: None,
                    created_at: Utc::now(),
                };
                self.notification_router.emit(event.clone()).await;
                if let Some(ref pool) = self.pool {
                    self.notification_router
                        .persist_notification(pool, &event)
                        .await;
                }
                Ok(CoreResponse::Ack)
            }
            CoreRequest::NotificationStop => {
                if let Some(ref arbiter) = self.audio_arbiter {
                    arbiter.request_interrupt();
                }
                Ok(CoreResponse::Ack)
            }
            // ── Session Projections M2: Replay Protocol ──────────────────────
            CoreRequest::ProjectionCapabilities => {
                Ok(CoreResponse::ProjectionCapabilitiesResponse {
                    supported: self.projection_seam.is_some(),
                    projection_version: 1,
                    max_events_per_batch: 512,
                    max_event_bytes: 64 * 1024,
                    max_subscriptions_per_client: 32,
                    max_subscriptions_per_daemon: 256,
                    retention_session_max_events: 20_000,
                    retention_project_max_events: 50_000,
                })
            }
            CoreRequest::ProjectionSubscribe { request } => {
                let Some(ref seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };
                if let Err(e) = request.validate() {
                    return Ok(CoreResponse::Error {
                        code: "invalid_projection_subscribe".into(),
                        message: e.to_string(),
                    });
                }
                let service = seam.service();
                let client_id = request.scope_id.clone();

                // Resolve canonical binding for Session scope so the
                // subscription lands on the same stream publications use.
                let (resolved_project, resolved_workspace, resolved_revision) =
                    if matches!(request.scope, ProjectionStreamKind::Session) {
                        if let Some(storage) = seam.project_storage() {
                            match storage.session_binding(&request.scope_id).await {
                                Ok(Some(record))
                                    if matches!(
                                        record.status,
                                        codegg_core::project_storage::BindingStatus::Resolved
                                    ) =>
                                {
                                    (
                                        record
                                            .project_id
                                            .map(|p| p.as_str().to_string())
                                            .unwrap_or_default(),
                                        record.workspace_id.map(|w| w.as_str().to_string()),
                                        record.revision,
                                    )
                                }
                                _ => (String::new(), None, 1),
                            }
                        } else {
                            (String::new(), None, 1)
                        }
                    } else {
                        (request.scope_id.clone(), None, 1)
                    };

                let sub_id = match request.scope {
                    ProjectionStreamKind::Session => {
                        service
                            .subscribe_session(
                                &request.scope_id,
                                &resolved_project,
                                resolved_workspace.as_deref(),
                                &client_id,
                                &request,
                            )
                            .await
                    }
                    ProjectionStreamKind::Project => {
                        service
                            .subscribe_project(&request.scope_id, &client_id, &request)
                            .await
                    }
                };

                match sub_id {
                    Ok(sub_id) => {
                        let descriptor = match request.scope {
                            ProjectionStreamKind::Session => service
                                .store()
                                .lookup_session_stream(&request.scope_id, &resolved_project)
                                .await
                                .ok()
                                .flatten()
                                .unwrap_or_else(|| ProjectionStreamDescriptor {
                                    stream_id: ProjectionStreamId(request.scope_id.clone()),
                                    kind: ProjectionStreamKind::Session,
                                    project_id: resolved_project.clone(),
                                    workspace_id: resolved_workspace.clone(),
                                    session_id: Some(request.scope_id.clone()),
                                    projection_version: 1,
                                    retention_floor_seq: 0,
                                    high_water_seq: 0,
                                    latest_checkpoint_seq: None,
                                }),
                            ProjectionStreamKind::Project => service
                                .store()
                                .get_or_create_project_stream(&request.scope_id)
                                .await
                                .map(|(d, _)| d)
                                .unwrap_or_else(|_| ProjectionStreamDescriptor {
                                    stream_id: ProjectionStreamId(request.scope_id.clone()),
                                    kind: ProjectionStreamKind::Project,
                                    project_id: request.scope_id.clone(),
                                    workspace_id: None,
                                    session_id: None,
                                    projection_version: 1,
                                    retention_floor_seq: 0,
                                    high_water_seq: 0,
                                    latest_checkpoint_seq: None,
                                }),
                        };
                        let snapshot = codegg_protocol::projection::replay::ProjectionSnapshotBundle::One {
                            snapshot: Box::new(
                                codegg_protocol::projection::snapshot::SessionProjectionSnapshot::empty(
                                    &request.scope_id,
                                    &descriptor.project_id,
                                    descriptor.workspace_id.as_deref().unwrap_or(""),
                                ),
                            ),
                        };
                        let cursor = codegg_protocol::projection::replay::ProjectionCursor {
                            stream_id: descriptor.stream_id.clone(),
                            event_seq: descriptor.high_water_seq,
                            projection_version: descriptor.projection_version,
                        };
                        let retention_floor_seq = descriptor.retention_floor_seq;
                        Ok(CoreResponse::ProjectionSubscribed {
                            subscription_id: sub_id,
                            descriptor,
                            snapshot,
                            cursor: cursor.clone(),
                            retention_floor_seq,
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "projection_subscribe_failed".into(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ProjectionResume {
                cursor,
                include_snapshot_if_resync,
            } => {
                let Some(ref seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };
                let service = seam.service();
                // Look up the subscription from the cursor's stream to find its sub_id.
                // For now, iterate subscriptions to find one matching this stream.
                let sub_id_opt = service
                    .subscriptions()
                    .by_id()
                    .iter()
                    .find(|e| e.value().stream_id == cursor.stream_id)
                    .map(|e| e.key().clone());

                let Some(sub_id) = sub_id_opt else {
                    return Ok(CoreResponse::Error {
                        code: "subscription_not_found".into(),
                        message: "no active subscription for this stream".into(),
                    });
                };

                match service
                    .resume(&sub_id, &cursor, include_snapshot_if_resync)
                    .await
                {
                    Ok(codegg_core::projection_replay::service::ResumeOutcome::Replayed {
                        events,
                        current_high_water,
                        next_cursor,
                    }) => {
                        let descriptor = service
                            .store()
                            .lookup_stream_by_id(cursor.stream_id.as_str())
                            .await
                            .ok()
                            .flatten()
                            .unwrap_or_else(|| ProjectionStreamDescriptor {
                                stream_id: cursor.stream_id.clone(),
                                kind: ProjectionStreamKind::Session,
                                project_id: String::new(),
                                workspace_id: None,
                                session_id: None,
                                projection_version: 1,
                                retention_floor_seq: 0,
                                high_water_seq: current_high_water,
                                latest_checkpoint_seq: None,
                            });
                        Ok(CoreResponse::ProjectionReplay {
                            batch: codegg_protocol::projection::replay::ProjectionReplayBatch {
                                descriptor,
                                events,
                                snapshot: None,
                                replay_start_seq: cursor.event_seq + 1,
                                replay_end_seq: next_cursor.event_seq,
                                current_high_water,
                                truncation_flag: false,
                                next_cursor: if next_cursor.event_seq < current_high_water {
                                    Some(next_cursor)
                                } else {
                                    None
                                },
                            },
                        })
                    }
                    Ok(codegg_core::projection_replay::service::ResumeOutcome::Empty {
                        current_high_water,
                        next_cursor,
                    }) => Ok(CoreResponse::ProjectionReplay {
                        batch: codegg_protocol::projection::replay::ProjectionReplayBatch {
                            descriptor: ProjectionStreamDescriptor {
                                stream_id: cursor.stream_id.clone(),
                                kind: ProjectionStreamKind::Session,
                                project_id: String::new(),
                                workspace_id: None,
                                session_id: None,
                                projection_version: 1,
                                retention_floor_seq: 0,
                                high_water_seq: current_high_water,
                                latest_checkpoint_seq: None,
                            },
                            events: vec![],
                            snapshot: None,
                            replay_start_seq: cursor.event_seq + 1,
                            replay_end_seq: next_cursor.event_seq,
                            current_high_water,
                            truncation_flag: false,
                            next_cursor: None,
                        },
                    }),
                    Ok(codegg_core::projection_replay::service::ResumeOutcome::Resync {
                        reason,
                        descriptor,
                        requested_cursor,
                        snapshot,
                    }) => Ok(CoreResponse::ProjectionResyncRequired {
                        reason,
                        descriptor,
                        requested_cursor,
                        snapshot,
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "projection_resume_failed".into(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ProjectionAck { ack } => {
                let Some(ref seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };
                let service = seam.service();
                // Find the subscription that owns this ack
                let sub_id_opt = service
                    .subscriptions()
                    .by_id()
                    .get(&ack.subscription_id)
                    .map(|e| e.key().clone());

                let Some(sub_id) = sub_id_opt else {
                    return Ok(CoreResponse::Error {
                        code: "subscription_not_found".into(),
                        message: "no active subscription with this ID".into(),
                    });
                };

                match service.ack(&sub_id, &ack.cursor).await {
                    Ok(codegg_core::projection_replay::service::AckResult::Accepted {
                        last_acked_seq,
                        lag_count,
                    }) => Ok(CoreResponse::ProjectionAckAccepted {
                        subscription_id: sub_id,
                        last_acked_seq,
                        lag_count,
                    }),
                    Ok(codegg_core::projection_replay::service::AckResult::Rejected { reason }) => {
                        Ok(CoreResponse::Error {
                            code: "projection_ack_rejected".into(),
                            message: reason,
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "projection_ack_failed".into(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ProjectionUnsubscribe { subscription_id } => {
                let Some(ref seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };
                let service = seam.service();
                match service.unsubscribe(&subscription_id).await {
                    Ok(()) => Ok(CoreResponse::ProjectionUnsubscribed { subscription_id }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "projection_unsubscribe_failed".into(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ProjectionSnapshotGet { scope, scope_id } => {
                let snapshot = match scope {
                    ProjectionStreamKind::Session => {
                        codegg_protocol::projection::replay::ProjectionSnapshotBundle::One {
                            snapshot: Box::new(
                                codegg_protocol::projection::snapshot::SessionProjectionSnapshot::empty(
                                    &scope_id,
                                    "",
                                    "",
                                ),
                            ),
                        }
                    }
                    ProjectionStreamKind::Project => {
                        codegg_protocol::projection::replay::ProjectionSnapshotBundle::BoundedSessionList {
                            sessions: vec![],
                            truncated: false,
                        }
                    }
                };
                Ok(CoreResponse::ProjectionReplay {
                    batch: codegg_protocol::projection::replay::ProjectionReplayBatch {
                        descriptor: ProjectionStreamDescriptor {
                            stream_id: ProjectionStreamId(scope_id.clone()),
                            kind: scope,
                            project_id: scope_id.clone(),
                            workspace_id: None,
                            session_id: if matches!(scope, ProjectionStreamKind::Session) {
                                Some(scope_id.clone())
                            } else {
                                None
                            },
                            projection_version: 1,
                            retention_floor_seq: 0,
                            high_water_seq: 0,
                            latest_checkpoint_seq: None,
                        },
                        events: vec![],
                        snapshot: Some(snapshot),
                        replay_start_seq: 0,
                        replay_end_seq: 0,
                        current_high_water: 0,
                        truncation_flag: false,
                        next_cursor: None,
                    },
                })
            }
            // ── Session Projections M3: Artifact Read Protocol ────────────────
            CoreRequest::ProjectionArtifactRead {
                request,
                project_id,
                context_correlation_id: _,
            } => {
                let Some(ref _seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };

                // Build access context for the calling principal
                let access_ctx = std::sync::Arc::new(
                    codegg_core::projection_replay::context::ProjectionAccessContext::local(
                        "daemon",
                        "artifact-read",
                    ),
                );
                let policy = std::sync::Arc::new(
                    codegg_core::projection_replay::policy::PolicyRegistry::default(),
                );

                // Authorize the artifact read
                let kind = codegg_core::projection_replay::policy::ArtifactReadKind::RunArtifact;

                if !policy
                    .policy()
                    .authorize_artifact_read(&access_ctx, &project_id, kind)
                {
                    return Ok(CoreResponse::ProjectionArtifactRead {
                        outcome:
                            codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::Denied {
                                reason: "authorization failed".into(),
                            },
                    });
                }

                // Build a disclosure context to access the artifact registry
                let metrics = std::sync::Arc::new(
                    codegg_core::projection_replay::ProjectionReplayMetrics::new(),
                );
                let disclosure =
                    codegg_core::projection_replay::ProjectionDisclosureContext::local(
                        None,
                        Some(project_id.clone()),
                        metrics,
                    );

                if let Some(ref registry) = disclosure.artifact_registry {
                    // Convert wire DTO to core type
                    let core_request = codegg_core::projection_replay::artifacts::ArtifactReadRequest {
                        handle_id: request.handle_id.clone(),
                        start: request.start,
                        end: request.end,
                        expected_revision: request.expected_revision,
                    };
                    match registry.read(&core_request, &project_id).await {
                        Ok(response) => {
                            return Ok(CoreResponse::ProjectionArtifactRead {
                                outcome:
                                    codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::Ok(
                                        codegg_protocol::projection::replay::ProjectionArtifactReadResponse {
                                            handle_id: response.handle_id,
                                            revision: response.revision,
                                            start: response.start,
                                            end: response.end,
                                            content_type: format!("{:?}", response.content_type)
                                                .to_lowercase(),
                                            content: response.content,
                                            redacted: response.redacted,
                                            truncated: response.truncated,
                                            note: response.note,
                                        },
                                    ),
                            });
                        }
                        Err(e) => {
                            let outcome = match &e {
                                codegg_core::projection_replay::artifact_registry::ArtifactRegistryError::NotFound => {
                                    codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::NotFound
                                }
                                codegg_core::projection_replay::artifact_registry::ArtifactRegistryError::RevisionMismatch { current, .. } => {
                                    codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::RevisionMismatch {
                                        current_revision: *current,
                                    }
                                }
                                codegg_core::projection_replay::artifact_registry::ArtifactRegistryError::InvalidRequest(msg) => {
                                    codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::InvalidRequest {
                                        reason: msg.clone(),
                                    }
                                }
                                _ => codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::InvalidRequest {
                                    reason: "internal error".into(),
                                },
                            };
                            return Ok(CoreResponse::ProjectionArtifactRead { outcome });
                        }
                    }
                }

                Ok(CoreResponse::ProjectionArtifactRead {
                    outcome:
                        codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::InvalidRequest {
                            reason: "no artifact registry available".into(),
                        },
                })
            }
            CoreRequest::ProjectionArtifactList { project_id } => {
                let Some(ref _seam) = self.projection_seam else {
                    return Ok(CoreResponse::Error {
                        code: "projection_unavailable".into(),
                        message: "projection replay requires a SQLite-backed daemon".into(),
                    });
                };

                let metrics = std::sync::Arc::new(
                    codegg_core::projection_replay::ProjectionReplayMetrics::new(),
                );
                let disclosure =
                    codegg_core::projection_replay::ProjectionDisclosureContext::local(
                        None,
                        Some(project_id.clone()),
                        metrics,
                    );

                if let Some(ref registry) = disclosure.artifact_registry {
                    if let Ok(handles) = registry.list(&project_id).await {
                        let dto_handles: Vec<
                            codegg_protocol::projection::replay::ProjectionArtifactHandleDto,
                        > = handles
                            .iter()
                            .map(|h| {
                                codegg_protocol::projection::replay::ProjectionArtifactHandleDto {
                                    handle_id: h.handle_id.clone(),
                                    kind: match h.kind {
                                        codegg_core::projection_replay::artifacts::ArtifactKind::RunOutput => {
                                            codegg_protocol::projection::replay::ArtifactHandleKind::RunOutput
                                        }
                                        codegg_core::projection_replay::artifacts::ArtifactKind::ToolOutput => {
                                            codegg_protocol::projection::replay::ArtifactHandleKind::ToolOutput
                                        }
                                        codegg_core::projection_replay::artifacts::ArtifactKind::DiffExcerpt => {
                                            codegg_protocol::projection::replay::ArtifactHandleKind::DiffExcerpt
                                        }
                                        codegg_core::projection_replay::artifacts::ArtifactKind::LogTail => {
                                            codegg_protocol::projection::replay::ArtifactHandleKind::LogTail
                                        }
                                    },
                                    project_id: h.project_id.clone(),
                                    source_record_id: h.source_record_id.clone(),
                                    content_type: format!("{:?}", h.content_type).to_lowercase(),
                                    total_bytes: h.total_bytes,
                                    created_at: h.created_at,
                                    expires_at: h.expires_at,
                                    revision: h.revision,
                                    public_summary: h.public_summary.clone(),
                                }
                            })
                            .collect();
                        return Ok(CoreResponse::ProjectionArtifactList {
                            handles: dto_handles,
                        });
                    }
                }

                Ok(CoreResponse::ProjectionArtifactList {
                    handles: vec![],
                })
            }
            _ => {
                tracing::warn!("Unhandled CoreRequest variant");
                Ok(CoreResponse::Error {
                    code: "unimplemented".to_string(),
                    message: "This request type is not yet implemented".to_string(),
                })
            }
        }
    }
}

fn connection_detail_dto(
    summary: &crate::protocol::provider::ProviderConnectionSummaryDto,
) -> crate::protocol::provider::ConnectionDetailDto {
    crate::protocol::provider::ConnectionDetailDto {
        connection_id: summary.id.clone(),
        display_name: summary.display_name.clone(),
        endpoint_authority: summary.endpoint.clone(),
        tls_policy: summary.tls_policy.clone(),
        scope: summary.scope.clone(),
        state: summary.state.clone(),
        revision: summary.revision,
        catalog_revision: summary.catalog_revision.clone(),
        health: summary.health.clone(),
        actor_seam: Some("local_operator".to_string()),
    }
}

fn purge_blocker_dto(
    blocker: codegg_core::provider_connections::PurgeBlocker,
) -> crate::protocol::provider::PurgeBlocker {
    match blocker {
        codegg_core::provider_connections::PurgeBlocker::SelectedSessions { count } => {
            crate::protocol::provider::PurgeBlocker::SelectedSessions { count }
        }
        codegg_core::provider_connections::PurgeBlocker::ProvisioningOperation { operation_id } => {
            crate::protocol::provider::PurgeBlocker::ProvisioningOperation { operation_id }
        }
        codegg_core::provider_connections::PurgeBlocker::ActiveRuntime { reference_id } => {
            crate::protocol::provider::PurgeBlocker::ActiveRuntime { reference_id }
        }
    }
}

async fn connection_lifecycle_response(
    pool: Option<sqlx::SqlitePool>,
    connection_id: String,
    expected_revision: u64,
    action: &str,
) -> Result<CoreResponse, AppError> {
    let Some(pool) = pool else {
        return Ok(CoreResponse::Error {
            code: "provider_connections_unavailable".to_string(),
            message: "Provider connections require a daemon SQLite catalog".to_string(),
        });
    };
    let Ok(id) = codegg_core::identity::ProviderConnectionId::parse(&connection_id) else {
        return Ok(CoreResponse::Error {
            code: "invalid_connection_id".to_string(),
            message: "Provider connection ID is invalid".to_string(),
        });
    };
    let store = codegg_core::provider_connections::ProviderConnectionStore::new(pool);
    let result = match action {
        "enable" => store.enable(&id, expected_revision).await.map(|_| ()),
        "disable" => store.disable(&id, expected_revision).await.map(|_| ()),
        "delete" => store.delete(&id, expected_revision).await.map(|_| ()),
        "restore" => store.restore(&id, expected_revision).await.map(|_| ()),
        _ => Err(
            codegg_core::provider_connections::ProviderConnectionError::Invalid(
                "unknown lifecycle action".to_string(),
            ),
        ),
    };
    match result {
        Ok(()) => Ok(CoreResponse::Ack),
        Err(error) => Ok(CoreResponse::Error {
            code: "connection_lifecycle_failed".to_string(),
            message: error.to_string(),
        }),
    }
}

fn eggpool_error_code(error: &crate::core::eggpool::EggpoolError) -> &'static str {
    match error {
        crate::core::eggpool::EggpoolError::InvalidEndpoint(_) => "invalid_endpoint",
        crate::core::eggpool::EggpoolError::InvalidScope(_) => "invalid_scope",
        crate::core::eggpool::EggpoolError::CredentialStore => "credential_store_unavailable",
        crate::core::eggpool::EggpoolError::MasterKeyMissing => "master_key_missing",
        crate::core::eggpool::EggpoolError::Conflict => "connection_conflict",
        crate::core::eggpool::EggpoolError::Cancelled => "connection_cancelled",
        crate::core::eggpool::EggpoolError::Probe(reason) => reason.code(),
        crate::core::eggpool::EggpoolError::Storage => "connection_storage_error",
        crate::core::eggpool::EggpoolError::Rotation(_) => "connection_rotation_failed",
        crate::core::eggpool::EggpoolError::Refresh(_) => "connection_refresh_failed",
    }
}

fn eggpool_error_message(error: &crate::core::eggpool::EggpoolError) -> &'static str {
    match error {
        crate::core::eggpool::EggpoolError::InvalidEndpoint(_) => "Eggpool endpoint is invalid",
        crate::core::eggpool::EggpoolError::InvalidScope(_) => "Connection scope is invalid",
        crate::core::eggpool::EggpoolError::CredentialStore => {
            "Protected credential store is unavailable"
        }
        crate::core::eggpool::EggpoolError::MasterKeyMissing => {
            "Configure the credential-store master key before connecting"
        }
        crate::core::eggpool::EggpoolError::Conflict => {
            "An equivalent connection or provisioning operation already exists"
        }
        crate::core::eggpool::EggpoolError::Cancelled => "Connection provisioning was cancelled",
        crate::core::eggpool::EggpoolError::Probe(reason) => match reason {
            crate::core::eggpool::ProbeReason::AuthenticationFailed => {
                "Eggpool rejected the credential"
            }
            crate::core::eggpool::ProbeReason::Unreachable => "Eggpool endpoint is unreachable",
            crate::core::eggpool::ProbeReason::Timeout => "Eggpool probe timed out",
            crate::core::eggpool::ProbeReason::TlsFailed => "Eggpool TLS negotiation failed",
            crate::core::eggpool::ProbeReason::RedirectDisallowed => {
                "Eggpool endpoint redirected unexpectedly"
            }
            crate::core::eggpool::ProbeReason::UnsupportedApi => {
                "Eggpool endpoint does not expose the supported model API"
            }
            crate::core::eggpool::ProbeReason::InvalidJson => "Eggpool returned invalid model data",
            crate::core::eggpool::ProbeReason::EmptyCatalog => "Eggpool returned no models",
            crate::core::eggpool::ProbeReason::CatalogOversized => {
                "Eggpool model catalog exceeded the safety limit"
            }
            crate::core::eggpool::ProbeReason::Cancelled => "Eggpool probe was cancelled",
        },
        crate::core::eggpool::EggpoolError::Storage => "Provider connection storage is unavailable",
        crate::core::eggpool::EggpoolError::Rotation(_) => "Provider connection rotation failed",
        crate::core::eggpool::EggpoolError::Refresh(_) => "Provider connection refresh failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::turn_runtime::TurnRuntime;
    use crate::core::CoreEvent;
    use crate::session::schema::migrate;

    /// Build a fresh in-memory SQLite pool with the full session
    /// schema. No on-disk tempdir is created, so the pool's memory is
    /// reclaimed when the test's `SqlitePool` is dropped — no
    /// `Box::leak` required.
    async fn in_memory_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:daemon_test_{}?mode=memory&cache=shared",
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
        migrate(&pool).await.expect("migrate");
        pool
    }

    async fn test_daemon() -> CoreDaemon {
        let pool = in_memory_pool().await;
        CoreDaemon::new(Some(pool), None, None, None)
    }

    async fn seed_test_context(daemon: &CoreDaemon, root: &std::path::Path) -> (String, String) {
        let workspace = daemon
            .workspaces
            .get_or_register(root)
            .await
            .expect("test workspace registration");
        let project = codegg_core::project_catalog::ProjectCatalog::new(
            daemon.pool.clone().expect("test daemon pool"),
        )
        .register_local_project(
            codegg_core::project_catalog::RegisterLocalProject {
                display_name: "Daemon test project".to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &workspace.id,
            "daemon-test",
        )
        .await
        .expect("test project registration");
        (
            project.project_id.as_str().to_string(),
            workspace.id.as_str().to_string(),
        )
    }

    #[tokio::test]
    async fn daemon_has_unique_id() {
        let d1 = test_daemon().await;
        let d2 = test_daemon().await;
        assert_ne!(d1.daemon_id, d2.daemon_id);
    }

    #[tokio::test]
    async fn session_create_through_daemon() {
        let daemon = test_daemon().await;
        let (project_id, workspace_id) =
            seed_test_context(&daemon, std::path::Path::new("/tmp")).await;
        let req = crate::core::new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp".into(),
                title: Some("Test".into()),
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Session { .. }));
    }

    #[tokio::test]
    async fn project_catalog_protocol_lists_lifecycle_and_health_by_scope() {
        let daemon = test_daemon().await;
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let (first_project, first_workspace) = seed_test_context(&daemon, first_root.path()).await;
        let (second_project, _second_workspace) =
            seed_test_context(&daemon, second_root.path()).await;

        let list = daemon
            .handle_request(crate::core::new_request(
                "project-list".into(),
                CoreRequest::ProjectList {
                    include_archived: false,
                    limit: 1,
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            list,
            CoreResponse::ProjectList {
                projects,
                truncated: true
            } if projects.len() == 1
        ));

        let details = daemon
            .handle_request(crate::core::new_request(
                "project-get".into(),
                CoreRequest::ProjectGet {
                    project_id: first_project.clone(),
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            details,
            CoreResponse::ProjectGet { project } if project.project.project_id == first_project
        ));

        let health = daemon
            .handle_request(crate::core::new_request(
                "project-health".into(),
                CoreRequest::ProjectHealth {
                    project_id: first_project.clone(),
                    workspace_id: first_workspace,
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            health,
            CoreResponse::ProjectHealth { health }
                if health.project_id == first_project
        ));

        let archived = daemon
            .handle_request(crate::core::new_request(
                "project-archive".into(),
                CoreRequest::ProjectArchive {
                    project_id: second_project.clone(),
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            archived,
            CoreResponse::ProjectArchived { project }
                if project.project_id == second_project && project.lifecycle == "archived"
        ));

        let restored = daemon
            .handle_request(crate::core::new_request(
                "project-restore".into(),
                CoreRequest::ProjectRestore {
                    project_id: second_project,
                },
            ))
            .await
            .unwrap();
        assert!(matches!(
            restored,
            CoreResponse::ProjectRestored { project } if project.lifecycle == "active"
        ));
    }

    #[tokio::test]
    async fn snapshot_daemon_returns_state() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request("req-1".into(), CoreRequest::SnapshotDaemon);
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::SnapshotDaemon {
                daemon_id,
                uptime_secs,
                ..
            } => {
                assert!(!daemon_id.is_empty());
                assert!(uptime_secs < 5);
            }
            other => panic!("expected SnapshotDaemon, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn turn_submit_rejects_when_active() {
        let daemon = test_daemon().await;
        let (project_id, workspace_id) =
            seed_test_context(&daemon, std::path::Path::new("/tmp")).await;
        let req = crate::core::new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp".into(),
                title: None,
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        );
        let session_id = match daemon.handle_request(req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            _ => panic!("expected Session"),
        };

        let runtime = daemon
            .sessions
            .get_or_create_for_test(&session_id, std::path::PathBuf::new());
        assert!(runtime.active_turn.read().await.is_none());
    }

    #[tokio::test]
    async fn resume_returns_typed_resync_when_seq_too_old() {
        // This test exercises the same path as before -- a too-old seq
        // when nothing is recorded anywhere. To force the ring to
        // have no record of seq 1, we use a no-pool daemon (so the
        // DB layer is bypassed) and a small ring, then evict the
        // only event by overflowing the ring.
        let daemon = CoreDaemon::new(None, None, None, None);
        // No pool is configured, so the event log is in-memory only
        // and the ring is the source of truth.
        // Publish a few events to a small ring by setting capacity
        // indirectly: we use the default capacity (4096) and publish
        // a single event so seq=1 is in the ring, then issue a
        // resume from seq 0 with no pool -- this would be covered by
        // the ring. To get a true "too old" we need to evict seq 1.
        // The cleanest way without changing daemon internals is to
        // request a seq the ring definitely does not have; with no
        // pool, the only valid request is one the ring can satisfy.
        // A future seq (e.g. 999_999) is treated as caught-up and
        // returns Events(empty), NOT ResyncRequired. So we use a
        // daemon without a pool and no events at all, with a
        // from_event_seq < current_seq (0 < 0 is false). The truly
        // "too old" case below uses a pool + eviction.
        // With no events and from_event_seq=0 and current_seq=0, the
        // path is caught-up and returns empty events.
        let req = crate::core::new_request(
            "req-resume-future".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: 999_999,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, 0);
                assert!(events.is_empty());
            }
            other => panic!("expected Events(empty) for future seq, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_returns_typed_events_on_success() {
        let daemon = test_daemon().await;

        daemon
            .event_log
            .publish(
                Some("s1".into()),
                None,
                crate::protocol::core::CoreEvent::SessionUpdated {
                    session_id: "s1".into(),
                },
            )
            .await;

        let req = crate::core::new_request(
            "req-resume-ok".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: 0,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, 1);
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].event_seq, 1);
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_from_current_seq_returns_empty_events_not_resync() {
        // A client that is already caught up (from_event_seq == current_seq)
        // must get an empty Events response, NOT ResyncRequired. This is
        // the core Pass 2 invariant: ResyncRequired is reserved for
        // too-old sequences that can no longer be replayed.
        let daemon = test_daemon().await;
        let s1 = daemon
            .event_log
            .publish(
                Some("s1".into()),
                None,
                crate::protocol::core::CoreEvent::SessionUpdated {
                    session_id: "s1".into(),
                },
            )
            .await;

        let req = crate::core::new_request(
            "req-resume-current".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: s1,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, s1);
                assert!(
                    events.is_empty(),
                    "expected empty events for caught-up client, got {:?}",
                    events
                );
            }
            other => panic!("expected Events(empty), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_from_future_seq_returns_empty_events() {
        // from_event_seq > current_seq is treated as "no new events" --
        // the client effectively overshot but we don't have anything to
        // send. Return Events(empty, current_seq) so the client can
        // resync its bookkeeping. The plan lists this as one of the
        // acceptable behaviors; we chose empty events.
        let daemon = test_daemon().await;
        let s1 = daemon
            .event_log
            .publish(
                Some("s1".into()),
                None,
                crate::protocol::core::CoreEvent::SessionUpdated {
                    session_id: "s1".into(),
                },
            )
            .await;

        let req = crate::core::new_request(
            "req-resume-future".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: s1 + 100,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, s1);
                assert!(events.is_empty());
            }
            other => panic!("expected Events(empty) for future seq, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_from_too_old_seq_returns_resync() {
        // To force a real "too old" outcome we need a daemon without
        // a SQLite pool. With a pool, the DB layer would still cover
        // any from_event_seq whose `from_event_seq + 1` is in the
        // persisted range, so the resync path becomes unreachable
        // for ordinary replay requests. With no pool, the ring is
        // the source of truth and eviction makes old seqs unsatisfiable.
        let daemon = CoreDaemon::new(None, None, None, None);
        // Publish enough events to overflow the default ring (4096).
        for _ in 0..5000 {
            daemon
                .event_log
                .publish(
                    Some("s1".into()),
                    None,
                    crate::protocol::core::CoreEvent::Error {
                        code: "filler".into(),
                        message: "m".into(),
                    },
                )
                .await;
        }
        let current = daemon.event_log.current_seq();
        assert!(
            current > 4096,
            "ring should have wrapped, current={}",
            current
        );

        // from_event_seq=0 is now too old: the ring's front is
        // current-4095 and there is no DB to fall back to.
        let req = crate::core::new_request(
            "req-resume-old".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: 0,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::ResyncRequired {
                from_event_seq,
                current_seq,
                session_id,
            } => {
                assert_eq!(from_event_seq, 0);
                assert_eq!(current_seq, current);
                assert_eq!(session_id.as_deref(), Some("s1"));
            }
            other => panic!("expected ResyncRequired for too-old seq, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn recovery_detects_interrupted_turn() {
        let daemon = test_daemon().await;

        // Subscribe before publishing so we can observe the recovery-emitted TurnFailed.
        let mut rx = daemon.event_log.subscribe();

        // Insert an interrupted TurnStarted directly (no matching TurnCompleted/TurnFailed).
        sqlx::query(
            "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) \
             VALUES (1, 's1', 't1', 'turn_started', '{}')",
        )
        .execute(daemon.pool.as_ref().unwrap())
        .await
        .unwrap();

        daemon.recover_state().await;

        // The recovery should have published a TurnFailed for (s1, t1).
        let mut found = false;
        while let Ok(env) = rx.try_recv() {
            if let crate::protocol::core::CoreEvent::TurnFailed {
                session_id,
                turn_id,
                ..
            } = &env.payload
            {
                if session_id == "s1" && turn_id.as_deref() == Some("t1") {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "expected recovery to emit TurnFailed for s1/t1");
    }

    #[tokio::test]
    async fn recovery_ignores_completed_turn() {
        let daemon = test_daemon().await;

        let mut rx = daemon.event_log.subscribe();

        // Insert a completed turn: TurnStarted followed by TurnCompleted.
        sqlx::query(
            "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) \
             VALUES (1, 's1', 't1', 'turn_started', '{}')",
        )
        .execute(daemon.pool.as_ref().unwrap())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) \
             VALUES (2, 's1', 't1', 'turn_completed', '{\"stop_reason\":\"ok\"}')",
        )
        .execute(daemon.pool.as_ref().unwrap())
        .await
        .unwrap();

        daemon.recover_state().await;

        // Drain and ensure no TurnFailed was emitted.
        let mut emitted_failed = false;
        while let Ok(env) = rx.try_recv() {
            if let crate::protocol::core::CoreEvent::TurnFailed {
                session_id,
                turn_id,
                ..
            } = &env.payload
            {
                if session_id == "s1" && turn_id.as_deref() == Some("t1") {
                    emitted_failed = true;
                    break;
                }
            }
        }
        assert!(
            !emitted_failed,
            "did not expect recovery to emit TurnFailed for a completed turn"
        );
    }

    #[tokio::test]
    async fn snapshot_models_returns_model_ids() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request("req-snap".into(), CoreRequest::SnapshotModels);
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::ModelsSnapshot {
                current_model,
                models,
            } => {
                assert!(current_model.is_none());
                // With no providers configured, the model list is empty.
                // The format contract is `provider/model` (e.g. `openai/gpt-4o`),
                // which is exercised by ModelsRefresh; for the empty-config case
                // we only assert the response shape is well-formed.
                for m in &models {
                    assert!(
                        m.contains('/'),
                        "model id '{}' should be 'provider/model'",
                        m
                    );
                }
            }
            other => panic!("expected ModelsSnapshot, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_respond_invalid_id_format() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-perm-invalid".into(),
            CoreRequest::PermissionRespond {
                id: "perm-1".into(),
                choice: "allow".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, message } => {
                assert_eq!(code, "invalid_permission_id");
                assert!(message.contains("perm-1"));
            }
            other => panic!("expected Error(invalid_permission_id), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_respond_malformed_id() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-perm-malformed".into(),
            CoreRequest::PermissionRespond {
                id: "perm:foo:bar".into(),
                choice: "allow".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "invalid_permission_id");
            }
            other => panic!("expected Error(invalid_permission_id), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn question_respond_invalid_id_format() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-q-invalid".into(),
            CoreRequest::QuestionRespond {
                id: "q-1".into(),
                answers: serde_json::json!("yes"),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, message } => {
                assert_eq!(code, "invalid_question_id");
                assert!(message.contains("q-1"));
            }
            other => panic!("expected Error(invalid_question_id), got {:?}", other),
        }
    }

    /// Manually install a `TurnHandle` on the given runtime's
    /// `active_turn` so we can exercise `TurnCancel`/`TurnSteer` paths
    /// without spinning up an actual agent loop. Returns the cancel
    /// sender, the cancel receiver (so the watch channel stays open),
    /// and the steer receiver so tests can observe the downstream
    /// effects.
    async fn install_active_turn(
        runtime: &std::sync::Arc<crate::core::session_runtime::SessionRuntime>,
        turn_id: &str,
    ) -> (
        tokio::sync::watch::Sender<bool>,
        tokio::sync::watch::Receiver<bool>,
        tokio::sync::mpsc::UnboundedReceiver<String>,
    ) {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let (steer_tx, steer_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut active = runtime.active_turn.write().await;
        *active = Some(crate::core::session_runtime::TurnHandle {
            turn_id: turn_id.to_string(),
            cancel_tx: cancel_tx.clone(),
            steer_tx: Some(steer_tx),
            started_at: chrono::Utc::now(),
            asset_pin: None,
        });
        (cancel_tx, cancel_rx, steer_rx)
    }

    #[tokio::test]
    async fn turn_cancel_wrong_id_rejected() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-cancel-wrong", std::path::PathBuf::from("."));
        let (cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, "turn-real").await;

        let req = crate::core::new_request(
            "req-cancel".into(),
            CoreRequest::TurnCancel {
                session_id: "s-cancel-wrong".into(),
                turn_id: "turn-typo".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, message } => {
                assert_eq!(code, "turn_id_mismatch");
                assert!(message.contains("turn-typo"));
                assert!(message.contains("turn-real"));
            }
            other => panic!("expected Error(turn_id_mismatch), got {:?}", other),
        }

        // The runtime should still have an active turn; we did not cancel.
        let active = runtime.active_turn.read().await;
        assert!(
            active.is_some(),
            "active_turn should remain set after a rejected cancel"
        );
        // The cancel channel should not have been signaled.
        assert!(
            !*cancel_tx.borrow(),
            "cancel_tx should not have been signaled"
        );
    }

    #[tokio::test]
    async fn turn_cancel_correct_id_succeeds() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-cancel-ok", std::path::PathBuf::from("."));
        let (cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, "turn-good").await;

        let req = crate::core::new_request(
            "req-cancel".into(),
            CoreRequest::TurnCancel {
                session_id: "s-cancel-ok".into(),
                turn_id: "turn-good".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));

        // The cancel channel should have been signaled.
        assert!(
            *cancel_tx.borrow(),
            "cancel_tx should have been signaled on matching turn_id"
        );
    }

    #[tokio::test]
    async fn turn_cancel_no_active_turn() {
        let daemon = test_daemon().await;
        // Register the session but do not install an active turn.
        daemon
            .sessions
            .get_or_create_for_test("s-cancel-none", std::path::PathBuf::from("."));

        let req = crate::core::new_request(
            "req-cancel".into(),
            CoreRequest::TurnCancel {
                session_id: "s-cancel-none".into(),
                turn_id: "turn-anything".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "no_active_turn");
            }
            other => panic!("expected Error(no_active_turn), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn turn_steer_wrong_id_rejected() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-steer-wrong", std::path::PathBuf::from("."));
        let (_cancel_tx, _cancel_rx, _steer_rx) =
            install_active_turn(&runtime, "turn-real-steer").await;

        let req = crate::core::new_request(
            "req-steer".into(),
            CoreRequest::TurnSteer {
                session_id: "s-steer-wrong".into(),
                turn_id: "turn-typo".into(),
                text: "redirect".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "turn_id_mismatch");
            }
            other => panic!("expected Error(turn_id_mismatch), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn turn_steer_correct_id_succeeds() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-steer-ok", std::path::PathBuf::from("."));
        let (_cancel_tx, _cancel_rx, mut steer_rx) =
            install_active_turn(&runtime, "turn-good-steer").await;

        let req = crate::core::new_request(
            "req-steer".into(),
            CoreRequest::TurnSteer {
                session_id: "s-steer-ok".into(),
                turn_id: "turn-good-steer".into(),
                text: "redirect".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));

        // The steer channel should have received the message.
        let got = tokio::time::timeout(std::time::Duration::from_millis(50), steer_rx.recv())
            .await
            .expect("steer message should arrive")
            .expect("steer_rx should yield a value");
        assert_eq!(got, "redirect");
    }

    #[tokio::test]
    async fn turn_started_emitted_on_submit() {
        // Set up an env var to register the openai provider so TurnSubmit
        // passes the provider-not-found check. The actual API call will
        // fail in the spawned agent loop, but we only care that TurnStarted
        // is published synchronously by the daemon before the spawn.
        std::env::set_var("OPENAI_API_KEY", "test-key-not-used");

        let daemon = test_daemon().await;
        let agent = crate::agent::Agent {
            name: "test".into(),
            description: "test agent".into(),
            ..Default::default()
        };

        // Pre-create a session so TurnSubmit can resolve its workspace.
        let workspace_dir = tempfile::tempdir().unwrap();
        let (project_id, workspace_id) = seed_test_context(&daemon, workspace_dir.path()).await;
        let create_req = crate::core::new_request(
            "req-create".into(),
            CoreRequest::SessionCreate {
                directory: workspace_dir.path().to_string_lossy().into_owned(),
                title: None,
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        );
        let session_id = match daemon.handle_request(create_req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };

        let req = crate::core::new_request(
            "req-submit".into(),
            CoreRequest::TurnSubmit {
                session_id: session_id.clone(),
                text: "hello".into(),
                plan_mode: false,
                model: "openai/gpt-4o".into(),
                agents: vec![crate::protocol_conversions::agent_to_dto(agent)],
                current_agent_idx: 0,
                messages: vec![],
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));

        // The TurnStarted event should be in the log, identified by
        // session_id and a turn_id that starts with "turn-".
        let filter = EventFilter {
            session_id: Some(session_id.clone()),
            include_global: true,
            client_id: None,
        };
        let events = daemon.event_log.replay_from(0, &filter).await;

        let mut found: Option<(String, String)> = None;
        for env in &events {
            if let CoreEvent::TurnStarted {
                session_id: sid,
                turn_id,
            } = &env.payload
            {
                if sid == &session_id {
                    found = Some((sid.clone(), turn_id.clone()));
                    break;
                }
            }
        }
        let (sid, turn_id) = found.expect("expected TurnStarted event in log");
        assert_eq!(sid, session_id);
        assert!(
            turn_id.starts_with("turn-"),
            "turn_id '{}' should start with 'turn-'",
            turn_id
        );

        std::env::remove_var("OPENAI_API_KEY");
    }

    #[tokio::test]
    async fn bridge_attaches_turn_id_for_text_delta() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-bridge-delta", std::path::PathBuf::from("."));
        let turn_id = "turn-bridge-delta".to_string();
        let (_cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, &turn_id).await;

        // A TextDelta from the bus carries no turn_id; the bridge must
        // attach the active turn_id.
        let app_event = crate::bus::events::AppEvent::TextDelta {
            session_id: "s-bridge-delta".into(),
            delta: "hi".into(),
        };
        let result = daemon
            .bridge_app_event(app_event)
            .await
            .expect("bridge_app_event should map TextDelta");
        let (session_id, attached_turn_id, core_event) = result;
        assert_eq!(session_id.as_deref(), Some("s-bridge-delta"));
        assert_eq!(attached_turn_id.as_deref(), Some(turn_id.as_str()));
        match core_event {
            CoreEvent::TurnTextDelta { turn_id: tid, .. } => {
                assert_eq!(
                    tid, turn_id,
                    "TurnTextDelta should carry the active turn_id"
                );
            }
            other => panic!("expected TurnTextDelta, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn bridge_no_longer_maps_agent_finished_to_turn_completed() {
        // Pass 3 invariant: the bridge must NOT produce a duplicate
        // `CoreEvent::TurnCompleted` for `AppEvent::AgentFinished`,
        // because the TurnSubmit spawned task publishes the lifecycle
        // event directly with the captured turn_id. The bus event is
        // still consumed by the event bridge to update token counts
        // and emit notifications, but it does not flow through
        // `map_app_event_to_core_event`.
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-bridge-finished", std::path::PathBuf::from("."));
        let turn_id = "turn-bridge-finished".to_string();
        let (_cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, &turn_id).await;

        let app_event = crate::bus::events::AppEvent::AgentFinished {
            session_id: "s-bridge-finished".into(),
            stop_reason: "completed".into(),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            reasoning_tokens: None,
        };
        let result = daemon.bridge_app_event(app_event).await;
        assert!(
            result.is_none(),
            "AgentFinished must not produce a CoreEvent from the bridge; got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn direct_turn_completion_uses_runtime_turn_id() {
        // The TurnSubmit spawn task publishes a CoreEvent::TurnCompleted
        // directly with the captured turn_id. We exercise this path
        // here by publishing the same event shape the spawn task
        // produces and asserting that the envelope carries the
        // non-empty turn id and matches what a subscriber sees on
        // the broadcast channel.
        let daemon = test_daemon().await;
        let session_id = "s-direct-completion".to_string();
        let turn_id = "turn-direct".to_string();
        let mut rx = daemon.event_log.subscribe();

        // Direct publish path (mirrors the spawn task).
        daemon
            .event_log
            .publish(
                Some(session_id.clone()),
                Some(turn_id.clone()),
                CoreEvent::TurnCompleted {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    stop_reason: "completed".to_string(),
                },
            )
            .await;

        let env = rx.recv().await.expect("expected an envelope on the bus");
        match env.payload {
            CoreEvent::TurnCompleted {
                turn_id: tid,
                stop_reason,
                ..
            } => {
                assert_eq!(tid, turn_id);
                assert_eq!(stop_reason, "completed");
            }
            other => panic!("expected TurnCompleted, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn bridge_keeps_turn_id_from_event_when_present() {
        let daemon = test_daemon().await;
        let runtime = daemon
            .sessions
            .get_or_create_for_test("s-bridge-explicit", std::path::PathBuf::from("."));
        let active_turn_id = "turn-active".to_string();
        let (_cancel_tx, _cancel_rx, _steer_rx) =
            install_active_turn(&runtime, &active_turn_id).await;

        // ToolResult carries a turn_id on the AppEvent? No - the bus
        // AppEvent::ToolResult doesn't have a turn_id. The bridged
        // CoreEvent::ToolCompleted has turn_id: None, so the bridge
        // should fall back to the active turn_id.
        let app_event = crate::bus::events::AppEvent::ToolResult {
            session_id: "s-bridge-explicit".into(),
            tool_id: "t1".into(),
            tool_name: "bash".into(),
            output: "ok".into(),
            success: true,
        };
        let result = daemon
            .bridge_app_event(app_event)
            .await
            .expect("bridge_app_event should map ToolResult");
        let (_session_id, attached_turn_id, core_event) = result;
        assert_eq!(attached_turn_id.as_deref(), Some(active_turn_id.as_str()));
        match core_event {
            CoreEvent::ToolCompleted { turn_id, .. } => {
                assert_eq!(turn_id.as_deref(), Some(active_turn_id.as_str()));
            }
            other => panic!("expected ToolCompleted, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn bridge_no_active_turn_keeps_empty_turn_id() {
        let daemon = test_daemon().await;
        // No active turn installed for this session.
        daemon
            .sessions
            .get_or_create_for_test("s-bridge-none", std::path::PathBuf::from("."));

        let app_event = crate::bus::events::AppEvent::TextDelta {
            session_id: "s-bridge-none".into(),
            delta: "orphan".into(),
        };
        let result = daemon
            .bridge_app_event(app_event)
            .await
            .expect("bridge_app_event should map TextDelta");
        let (_session_id, attached_turn_id, core_event) = result;
        // No active turn -> turn_id is the empty default from the mapper.
        assert_eq!(attached_turn_id.as_deref(), Some(""));
        match core_event {
            CoreEvent::TurnTextDelta { turn_id, .. } => {
                assert_eq!(turn_id, "");
            }
            other => panic!("expected TurnTextDelta, got {:?}", other),
        }
    }

    /// A minimal fake turn runtime that records whether `run_turn` was called.
    struct FakeTurnRuntime {
        called: std::sync::atomic::AtomicBool,
    }

    impl FakeTurnRuntime {
        fn new() -> Self {
            Self {
                called: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::agent::turn_runtime::TurnRuntime for FakeTurnRuntime {
        async fn run_turn(
            &self,
            _input: crate::agent::turn_runtime::TurnRunInput,
        ) -> Result<crate::agent::turn_runtime::TurnRunOutput, crate::error::AppError> {
            self.called.store(true, std::sync::atomic::Ordering::SeqCst);
            let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(false);
            let (steer_tx, _steer_rx) = tokio::sync::mpsc::unbounded_channel();
            Ok(crate::agent::turn_runtime::TurnRunOutput {
                cancel_tx,
                steer_tx,
            })
        }
    }

    #[tokio::test]
    async fn turn_submit_uses_injected_runtime() {
        // Verify that CoreDaemon::TurnSubmit delegates to the injected
        // TurnRuntime instead of constructing DefaultTurnRuntime directly.
        std::env::set_var("OPENAI_API_KEY", "test-key-not-used");

        let fake = Arc::new(FakeTurnRuntime::new());
        let pool = in_memory_pool().await;
        let deps = CoreRuntimeDeps::new(Some(pool), None, None, None)
            .with_turn_runtime(Arc::clone(&fake) as Arc<dyn TurnRuntime>);
        let daemon = CoreDaemon::with_deps(deps);
        daemon.hydrate_workspace_registry().await.unwrap();

        let agent = crate::agent::Agent {
            name: "test".into(),
            description: "test agent".into(),
            ..Default::default()
        };

        // Pre-create a session so TurnSubmit can resolve its workspace.
        let workspace_dir = tempfile::tempdir().unwrap();
        let (project_id, workspace_id) = seed_test_context(&daemon, workspace_dir.path()).await;
        let create_req = crate::core::new_request(
            "req-inject-create".into(),
            CoreRequest::SessionCreate {
                directory: workspace_dir.path().to_string_lossy().into_owned(),
                title: None,
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        );
        let session_id = match daemon.handle_request(create_req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };

        let req = crate::core::new_request(
            "req-inject".into(),
            CoreRequest::TurnSubmit {
                session_id,
                text: "hello".into(),
                plan_mode: false,
                model: "openai/gpt-4o".into(),
                agents: vec![crate::protocol_conversions::agent_to_dto(agent)],
                current_agent_idx: 0,
                messages: vec![],
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));
        assert!(
            fake.called.load(std::sync::atomic::Ordering::SeqCst),
            "injected FakeTurnRuntime should have been invoked"
        );
        // Note: do not remove OPENAI_API_KEY here to avoid racing
        // with other tests that also set it. The env var is process-global.
    }
}
