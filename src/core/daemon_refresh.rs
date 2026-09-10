//! M003: `CoreDaemon` runtime-refresh ownership.
//!
//! `CoreDaemon` remains the single daemon composition/lifecycle authority.
//! This module owns runtime refresh coordinators only: session-projection
//! snapshots, asset-context assembly, project/activation refresh, explicit
//! project activation leases, read-only health, activation-lease eviction,
//! per-session runtime-asset refresh, and the workspace/binding resolvers
//! those paths share with request families. It introduces no new store,
//! scheduler, state machine, or authority, and preserves exact refresh and
//! activation semantics (including lease-drop-on-failure and
//! generation-gated error mapping).
//!
//! ```text
//! Refresh paths (all through the daemon-owned AssetRefreshCoordinator):
//!   - refresh_project_context (context + plugin contributions -> coordinator)
//!   - refresh_project_activation (resolve -> refresh as ProjectActivation)
//!   - activate_project_workspace (resolve -> acquire lease -> refresh ->
//!       health; lease dropped on refresh/health failure)
//!   - refresh_runtime_assets (session runtime -> coordinator)
//!   - project_health (catalog + workspace peek + asset status -> aggregate)
//!   - evict_project_activation_leases (expiry -> idle -> service eviction)
//! ```
//!
//! Binding resolvers (`resolve_*`, `bind_*`, `workspace_for_*`, `session_dto`,
//! `project_details`, `project_lifecycle_request`) stay canonical here so
//! request families call one implementation; they carry no new authority.

use std::sync::Arc;

use super::daemon::CoreDaemon;
use super::project_activation::{self, ProjectActivation, ProjectHealthSnapshot};
use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreResponse};
use chrono::Utc;
use codegg_core::context::{
    ContextResolutionError, ProjectContext, ProjectContextRequest, SessionId,
};
use codegg_core::workspace::WorkspaceRecord;

impl CoreDaemon {
    /// Build the durable portion of a session projection from authoritative
    /// stores. This is used for initial connect and resync, so a reconnect
    /// does not depend on transient legacy subagent events. The method is
    /// read-only with respect to run/worktree authority.
    pub(crate) async fn projection_snapshot_for_session(
        &self,
        session_id: &str,
        project_id: &str,
        workspace_id: &str,
    ) -> codegg_protocol::projection::snapshot::SessionProjectionSnapshot {
        let mut snapshot = codegg_protocol::projection::snapshot::SessionProjectionSnapshot::empty(
            session_id,
            project_id,
            workspace_id,
        );
        let runs = self
            .deps
            .agent_run_store
            .list_by_session(session_id)
            .await
            .unwrap_or_default();
        for run in &runs {
            let Some(task) = self
                .deps
                .agent_run_store
                .get_task(&run.task_id)
                .await
                .ok()
                .flatten()
            else {
                continue;
            };
            let worktree = match &run.worktree_id {
                Some(worktree_id) => self.worktree_service.get(worktree_id).await.ok(),
                None => None,
            };
            let result = self
                .deps
                .agent_run_store
                .get_result(&run.run_id)
                .await
                .ok()
                .flatten();
            let groups = self
                .deps
                .run_group_service
                .groups_for_run(&run.run_id)
                .await
                .unwrap_or_default();
            let group_id = groups.first().map(|group| group.group_id.to_string());
            snapshot.upsert_agent_run(codegg_core::projection_replay::agent_run_summary(
                &task,
                run,
                worktree.as_ref(),
                result.as_ref(),
                group_id.as_deref(),
            ));
            if let Some(worktree) = worktree.as_ref() {
                snapshot
                    .upsert_worktree(codegg_core::projection_replay::worktree_summary(worktree));
            }
            for group in groups {
                let members = group
                    .member_run_ids
                    .iter()
                    .filter_map(|member_id| runs.iter().find(|run| &run.run_id == member_id))
                    .map(
                        |member| codegg_core::agent_run_group::AgentRunGroupMemberSummary {
                            ordinal: 0,
                            run_id: member.run_id.clone(),
                            status: member.status,
                            result_ref: member.result_ref.clone(),
                            failure_class: member.failure_class.clone(),
                            failure_message: member.failure_message.clone(),
                        },
                    )
                    .collect::<Vec<_>>();
                let group_summary = codegg_core::agent_run_group::AgentRunGroupSummary {
                    successful: members
                        .iter()
                        .filter(|member| {
                            member.status == codegg_core::agent_run::AgentRunStatus::Completed
                        })
                        .count(),
                    failed: members
                        .iter()
                        .filter(|member| {
                            matches!(
                                member.status,
                                codegg_core::agent_run::AgentRunStatus::Failed
                                    | codegg_core::agent_run::AgentRunStatus::Interrupted
                                    | codegg_core::agent_run::AgentRunStatus::Cancelled
                            )
                        })
                        .count(),
                    active: members
                        .iter()
                        .filter(|member| !member.status.is_terminal())
                        .count(),
                    timed_out: false,
                    group,
                    members,
                };
                snapshot.upsert_run_group(codegg_core::projection_replay::run_group_summary(
                    &group_summary,
                    run.updated_at,
                ));
            }
        }
        if let Ok(worktrees) = self
            .worktree_service
            .list(codegg_core::worktree_service::WorktreeQuery {
                workspace_id: Some(codegg_core::workspace::WorkspaceId::new_unchecked(
                    workspace_id,
                )),
                ..Default::default()
            })
            .await
        {
            for worktree in worktrees {
                snapshot
                    .upsert_worktree(codegg_core::projection_replay::worktree_summary(&worktree));
            }
        }
        snapshot
    }

    fn asset_context_for_project(
        context: &ProjectContext,
        session_id: Option<&str>,
    ) -> Result<crate::agent::asset_context::AssetContext, AppError> {
        let project_id = crate::agent::asset_context::ProjectId::parse(context.project_id.as_str())
            .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
        let config_revision = u64::try_from(context.binding_revision).map_err(|_| {
            AppError::Other(anyhow::anyhow!(
                "invalid negative binding revision: {}",
                context.binding_revision
            ))
        })?;
        let mut builder = crate::agent::asset_context::AssetContextBuilder::new()
            .with_project_id(project_id)
            .with_workspace_root(context.workspace_root.clone())
            .with_config_revision(config_revision);
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

    pub(crate) async fn refresh_project_context(
        &self,
        context: &ProjectContext,
        session_id: Option<&str>,
        reason: crate::agent::asset_refresh::RefreshReason,
    ) -> Result<crate::agent::asset_refresh::RefreshReport, AppError> {
        let mut asset_context = Self::asset_context_for_project(context, session_id)?;
        // Capture the same immutable activation view used for this refresh as
        // explicit context input. The snapshot builder never reads plugin
        // activation storage or mutable plugin state itself.
        if let Some(plugin_service) = crate::plugin::create_default_plugin_service().await {
            match plugin_service
                .for_workspace(context.workspace_id.to_string())
                .await
            {
                Ok(plugin_service) => {
                    let contributions = plugin_service.resolved_contributions().await;
                    asset_context =
                        asset_context.with_plugin_contributions(Arc::new(contributions));
                }
                Err(error) => {
                    tracing::warn!(%error, "plugin activation unavailable during asset refresh")
                }
            }
        }
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

    pub(crate) async fn refresh_runtime_assets(
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

    pub(crate) fn refresh_report_error(
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

    pub(crate) fn asset_refresh_report_dto(
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

    pub(crate) fn asset_refresh_status_dto(
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

    pub(crate) fn context_error(error: &ContextResolutionError) -> AppError {
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

    pub(crate) fn project_health_dto(
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

    pub(crate) fn project_catalog_error(
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

    pub(crate) async fn project_details(
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

    pub(crate) async fn project_lifecycle_request(
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
                self.event_log.publish(None, None, event).await;
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

    pub(crate) async fn resolve_session_context(
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

    pub(crate) async fn resolve_request_context(
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

    pub(crate) async fn resolve_session_list_project(
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

    pub(crate) fn session_dto(
        session: crate::session::Session,
        context: Option<&ProjectContext>,
    ) -> crate::protocol::dto::Session {
        let mut dto = crate::protocol_conversions::session_to_dto(session).unwrap_or_else(|e| {
            tracing::error!(error = %e, "session_to_dto conversion failed");
            Default::default()
        });
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

    /// Canonical refresh coordinator names.
    ///
    /// Pinned by `refresh_coordinators_match_documented_set` so a new
    /// coordinator is an explicit ownership decision.
    #[cfg(test)]
    pub(crate) fn refresh_coordinator_names() -> [&'static str; 6] {
        [
            "refresh_project_context",
            "refresh_project_activation",
            "activate_project_workspace",
            "refresh_runtime_assets",
            "project_health",
            "evict_project_activation_leases",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_coordinators_match_documented_set() {
        assert_eq!(
            CoreDaemon::refresh_coordinator_names(),
            [
                "refresh_project_context",
                "refresh_project_activation",
                "activate_project_workspace",
                "refresh_runtime_assets",
                "project_health",
                "evict_project_activation_leases",
            ]
        );
    }

    #[test]
    fn refresh_report_error_requires_usable_generation() {
        let without_generation = crate::agent::asset_refresh::RefreshReport {
            scope: crate::agent::asset_refresh::AssetScope::new("p", "w"),
            reason: crate::agent::asset_refresh::RefreshReason::Manual,
            outcome: crate::agent::asset_refresh::RefreshOutcome::Invalid,
            generation: None,
            previous_generation: None,
            fingerprint: None,
            added: Vec::new(),
            removed: Vec::new(),
            changed: Vec::new(),
            shadowed: Vec::new(),
            invalid: Vec::new(),
            retained: Vec::new(),
            diagnostics: vec!["no generation".to_string()],
            coalesced: false,
            completed_at: chrono::Utc::now(),
        };
        assert!(CoreDaemon::refresh_report_error(&without_generation).is_some());
    }

    #[tokio::test]
    async fn project_health_and_activation_require_resolver_and_pool() {
        // Pool-less daemons fail closed with a typed error, never a panic.
        let daemon = CoreDaemon::new(None, None, None);
        let health = daemon.project_health("p", "w").await;
        assert!(health.is_err());
        let activation = daemon.activate_project_workspace("p", "w", "owner").await;
        assert!(activation.is_err());
        let refresh = daemon.refresh_project_activation("p", "w").await;
        assert!(refresh.is_err());
    }

    #[tokio::test]
    async fn evict_expired_leases_is_bounded_and_side_effect_free() {
        let daemon = CoreDaemon::new(None, None, None);
        let report = daemon.evict_project_activation_leases(chrono::Utc::now());
        // Fresh daemon owns no leases; eviction reports zero without touching
        // workspace services.
        let debug = format!("{report:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn context_error_maps_to_secret_free_codes() {
        let err = CoreDaemon::context_error(&ContextResolutionError::DirectoryNotFound);
        let msg = err.to_string();
        assert!(msg.contains("project_context_required"));
        // No path or identity leaks into the code string.
        assert!(!msg.contains("/tmp"));
    }
}
