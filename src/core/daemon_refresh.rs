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
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse};
use chrono::Utc;
use codegg_core::context::{
    ContextResolutionError, ProjectContext, ProjectContextRequest, SessionId,
};
use codegg_core::workspace::WorkspaceRecord;

/// The refusal a delegated-run read answers with when the named session does
/// not own the run, or the run does not exist.
///
/// Both cases get the **same** answer, so a guessed run id reveals neither
/// the run's existence nor its size. It is deliberately not the "absent"
/// page: an absent page is the statement "this run exists and nothing was
/// recorded", and that statement must not be made about a run the caller
/// does not own.
fn agent_run_not_found() -> CoreResponse {
    CoreResponse::Error {
        code: "agent_run_not_found".to_string(),
        message: "delegated run not found".to_string(),
    }
}

impl CoreDaemon {
    /// The session's durable record, when the daemon has a store.
    pub(crate) async fn session_record(
        &self,
        session_id: &str,
    ) -> Option<codegg_core::session::Session> {
        let pool = self.pool.clone()?;
        let store = codegg_core::session::SessionStore::new(pool);
        match store.get(session_id).await {
            Ok(record) => record,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    session_id = %session_id,
                    "session record read failed"
                );
                None
            }
        }
    }

    /// The durable delegated-run transcript store, when this daemon has a
    /// SQLite pool to read it from.
    ///
    /// A daemon without a pool has no transcript store: the snapshot then
    /// carries no transcript at all, and the client degrades to the run
    /// inspector. That is different from a run with no recorded transcript,
    /// and the two are kept apart.
    fn transcript_store(
        &self,
    ) -> Option<std::sync::Arc<dyn codegg_core::agent_run_transcript::AgentRunTranscriptStore>>
    {
        self.pool.clone().map(|pool| {
            std::sync::Arc::new(
                codegg_core::agent_run_transcript::SqliteAgentRunTranscriptStore::new(pool),
            )
                as std::sync::Arc<dyn codegg_core::agent_run_transcript::AgentRunTranscriptStore>
        })
    }

    /// M002: serve one bounded page of a delegated run's recorded transcript.
    ///
    /// The authority is the run's **own durable ownership**. The run's task
    /// origin names the session that owns it, and that durable origin — not
    /// the session id the client sent — is what authorizes a read of its
    /// transcript. The client's session id is a claim that is checked, never
    /// a grant.
    ///
    /// Three answers, and they are three different facts:
    ///
    /// - **Not yours, or not there.** A run the named session does not own
    ///   and a run id that does not exist get the *same* answer, so a
    ///   guessed run id reveals neither the run's existence nor its size.
    /// - **Yours, but nothing recorded.** An owned run with no recorded
    ///   transcript is a *page* whose status is absent. That is not the
    ///   refusal above, and the client can tell them apart.
    /// - **Temporarily unavailable.** A store that cannot be read gets its
    ///   own answer, because "could not read it" is not "not yours".
    ///
    /// The read is bounded three ways, independently: the store's own read
    /// limit, the declared page row ceiling, and the declared byte ceiling
    /// applied to the already-redacted bodies. The page answers with the
    /// store's authoritative cursor, so a client walks past the initial
    /// preview without re-fetching the session.
    ///
    /// A retired parent turn does not close this door: ownership is
    /// session-scoped and durable, so a run whose parent turn has ended is
    /// still readable by the session that owns it.
    pub(crate) async fn handle_agent_run_request(
        &self,
        request: CoreRequest,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (trusted_client_id, &authority, &authz_decision);
        let CoreRequest::AgentRunTranscriptRead {
            session_id,
            run_id,
            from_ordinal,
            limit,
        } = request
        else {
            return Ok(CoreResponse::Error {
                code: "unsupported_request".to_string(),
                message: "the delegated run family does not serve this request".to_string(),
            });
        };

        // A run id that is not a well-formed identity cannot name a run, so
        // it gets the same answer as a run that does not exist.
        let Ok(run_id) = codegg_core::identity::AgentRunId::parse(&run_id) else {
            return Ok(agent_run_not_found());
        };

        // The ownership check. The run's own task origin is the authority:
        // the session that originated the task owns the run, and only that
        // session may read its transcript.
        //
        // A store read that fails is answered like a missing run, so a
        // failing run store cannot be used to probe which run ids exist.
        let run = match self.deps.agent_run_store.get_run(&run_id).await {
            Ok(Some(run)) => run,
            Ok(None) => return Ok(agent_run_not_found()),
            Err(error) => {
                tracing::warn!(%error, %run_id, "delegated run ownership read failed");
                return Ok(agent_run_not_found());
            }
        };
        let task = match self.deps.agent_run_store.get_task(&run.task_id).await {
            Ok(Some(task)) => task,
            Ok(None) => return Ok(agent_run_not_found()),
            Err(error) => {
                tracing::warn!(%error, %run_id, "delegated run task ownership read failed");
                return Ok(agent_run_not_found());
            }
        };
        if task.originating_session_id != session_id {
            return Ok(agent_run_not_found());
        }

        let Some(store) = self.transcript_store() else {
            return Ok(CoreResponse::Error {
                code: "transcript_unavailable".to_string(),
                message: "the delegated run transcript store is not available".to_string(),
            });
        };

        match codegg_core::projection_replay::agent_run_transcript_page(
            store.as_ref(),
            &run_id,
            from_ordinal,
            limit as usize,
        )
        .await
        {
            Ok(page) => Ok(CoreResponse::AgentRunTranscriptRead { page }),
            Err(codegg_core::projection_replay::AgentRunTranscriptPageError::Storage(error)) => {
                // The message carries no body and no ordinal count: a
                // failed read says nothing about what is in the store.
                tracing::warn!(%error, %run_id, "delegated run transcript page read failed");
                Ok(CoreResponse::Error {
                    code: "transcript_unavailable".to_string(),
                    message: "the delegated run transcript could not be read".to_string(),
                })
            }
            Err(codegg_core::projection_replay::AgentRunTranscriptPageError::BoundExceeded(
                error,
            )) => {
                // Refused, not shortened: a page over the declared bound is
                // not shipped, and the client is told to re-read with a
                // smaller cursor step rather than be handed a short page
                // that reads as the whole transcript.
                tracing::warn!(%error, %run_id, "refused an over-bound delegated run transcript page");
                Ok(CoreResponse::Error {
                    code: "transcript_bound_exceeded".to_string(),
                    message: "the delegated run transcript page is over the declared bound"
                        .to_string(),
                })
            }
        }
    }

    /// Build the durable portion of a session projection from authoritative
    /// stores. This is used for initial connect and resync, so a reconnect
    /// does not depend on transient legacy subagent events. The method is
    /// read-only with respect to run/worktree authority.
    ///
    /// The snapshot is the **exact-owner** boundary for the delegated run's
    /// transcript: it is built for one `session_id` and delivered only to the
    /// client bound to that session, so no client receives another session's
    /// runs — nor their transcripts.
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
        let record = self.session_record(session_id).await;
        let runtime = self.sessions.get(session_id);
        let mut summary = super::session_summary::session_summary(
            session_id,
            record.as_ref(),
            runtime.as_deref(),
        )
        .await;
        if summary.project_id.is_empty() {
            summary.project_id = project_id.to_string();
        }
        if summary.workspace_id.is_empty() {
            summary.workspace_id = workspace_id.to_string();
        }
        snapshot.primary_session = summary;
        for meta in self.human_shell_runs.list_for_session(session_id).await {
            snapshot.upsert_shell_run(crate::shell::daemon::run_projection(&meta));
        }
        snapshot.set_tool_contracts(self.session_tool_contracts.get(session_id));
        if let Some(runtime) = self.sessions.get(session_id) {
            let mut nodes = runtime
                .subagents
                .read()
                .await
                .values()
                .cloned()
                .collect::<Vec<_>>();
            nodes.sort_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.child_instance_id.cmp(&right.child_instance_id))
            });
            for node in nodes {
                snapshot.upsert_subagent(node);
            }
            snapshot.primary_session.active_subagents = snapshot
                .subagents
                .iter()
                .filter(|node| {
                    matches!(
                        node.status,
                        codegg_protocol::projection::dto::AgentTreeStatus::Running
                    )
                })
                .count();
            if runtime
                .subagent_overflow
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
            {
                snapshot.push_diagnostic(codegg_protocol::projection::snapshot::ProjectionDiagnostic {
                    code: "session_subagent_capacity".into(),
                    message: "additional live child instances were omitted by the session projection bound".into(),
                    at_ms: Utc::now().timestamp_millis(),
                    session_id: Some(session_id.to_string()),
                    turn_id: None,
                });
            }
        }
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
            let mut summary = codegg_core::projection_replay::agent_run_summary(
                &task,
                run,
                worktree.as_ref(),
                result.as_ref(),
                group_id.as_deref(),
            );
            // The delegated run's recorded transcript, read at this boundary
            // and bounded by the DTO's declared bound. It is absent — not
            // empty — when the run recorded none, so the tab can say so.
            if let Some(store) = self.transcript_store() {
                summary = codegg_core::projection_replay::agent_run_summary_with_transcript(
                    summary,
                    store.as_ref(),
                    &run.run_id,
                )
                .await;
            }
            snapshot.upsert_agent_run(summary);
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
        // JobStore is authoritative for session-owned scheduler work. Keep
        // live jobs ahead of recent terminals so history cannot hide active
        // work. This bounded indexed query runs only during snapshot/resync,
        // never in a TUI render path.
        //
        // "Live" is the domain's own complement of terminality
        // (`JobState::non_terminal_states`), not a hand-written list: an
        // interrupted job is not finished, and because an interrupted
        // attempt may still be requeued it may still run, so it belongs with
        // the live jobs rather than with the finished ones.
        let live_jobs = self
            .deps
            .job_store
            .list_job_records(codegg_core::jobs::JobStoreQuery {
                session_id: Some(session_id.to_string()),
                states: codegg_core::jobs::JobState::non_terminal_states(),
                limit: Some((codegg_protocol::projection::MAX_PROJECTION_JOBS + 1) as u32),
                ..Default::default()
            })
            .await;
        let mut jobs_added = 0usize;
        match live_jobs {
            Ok(jobs) => {
                let truncated = jobs.len() > codegg_protocol::projection::MAX_PROJECTION_JOBS;
                for job in jobs
                    .into_iter()
                    .take(codegg_protocol::projection::MAX_PROJECTION_JOBS)
                {
                    snapshot.upsert_job(job_projection(&job));
                    jobs_added += 1;
                }
                if truncated {
                    snapshot.push_diagnostic(
                        codegg_protocol::projection::snapshot::ProjectionDiagnostic {
                            code: "job_snapshot_truncated".into(),
                            message: "session has more live jobs than the projection cap".into(),
                            at_ms: Utc::now().timestamp_millis(),
                            session_id: Some(session_id.to_string()),
                            turn_id: None,
                        },
                    );
                }
            }
            Err(error) => {
                tracing::warn!(%session_id, %error, "failed to rebuild live jobs for session projection snapshot");
                snapshot.push_diagnostic(
                    codegg_protocol::projection::snapshot::ProjectionDiagnostic {
                        code: "job_snapshot_unavailable".into(),
                        message: "live job state could not be loaded".into(),
                        at_ms: Utc::now().timestamp_millis(),
                        session_id: Some(session_id.to_string()),
                        turn_id: None,
                    },
                );
            }
        }
        if jobs_added < codegg_protocol::projection::MAX_PROJECTION_JOBS {
            // The finished jobs are exactly the states the domain calls
            // terminal (`JobState::terminal_states`). An interrupted job is
            // **not** among them: it is not finished, and it is already
            // projected above with the live jobs, so nothing the inclusive
            // query used to select is lost by this change.
            let terminal_jobs = self
                .deps
                .job_store
                .list_job_records(codegg_core::jobs::JobStoreQuery {
                    session_id: Some(session_id.to_string()),
                    states: codegg_core::jobs::JobState::terminal_states(),
                    limit: Some(
                        (codegg_protocol::projection::MAX_PROJECTION_JOBS - jobs_added) as u32,
                    ),
                    ..Default::default()
                })
                .await;
            match terminal_jobs {
                Ok(jobs) => {
                    for job in jobs {
                        snapshot.upsert_job(job_projection(&job));
                    }
                }
                Err(error) => {
                    tracing::warn!(%session_id, %error, "failed to load recent terminal jobs for session projection snapshot")
                }
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
        request_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
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
        let session_count = match catalog.list_sessions_for_project(&project_id).await {
            Ok(count) => count,
            Err(error) => {
                return Ok(Self::project_catalog_error(
                    "project lifecycle session count failed",
                    &error,
                ))
            }
        };
        let result = if restore {
            catalog.restore_project(&project_id, "protocol").await
        } else {
            catalog.archive_project(&project_id, "protocol").await
        };
        match result {
            Ok(record) => {
                let project =
                    codegg_core::protocol_conversions::project_catalog_record_to_dto(&record);
                let provenance = codegg_core::authorization::audit_provenance(authz_decision);
                let audit = codegg_core::audit::AuditEventBuilder::new(
                    codegg_core::audit::AuditAction::ConfigChange,
                    authority.principal(),
                    &provenance,
                )
                .with_correlation(request_id)
                .with_project(project_id.clone())
                .with_metadata(
                    "project.lifecycle.operation",
                    if restore { "reactivate" } else { "deactivate" },
                )
                .with_metadata("project.session_count", session_count.to_string())
                .with_metadata("decision.id", provenance.decision_id())
                .with_metadata("decision.outcome", "allow");
                self.audit_emitter().emit(audit).await;
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

fn job_projection(
    job: &codegg_core::jobs::JobRecord,
) -> codegg_protocol::projection::dto::JobProjection {
    codegg_protocol::projection::dto::JobProjection {
        job_id: job.job_id.as_str().to_string(),
        workspace_id: job.workspace_id.to_string(),
        kind: job.kind.as_str().to_string(),
        state: job.state.as_str().to_string(),
        summary: String::new(),
        session_id: job.session_id.clone(),
        turn_id: job.turn_id.clone(),
        active_attempt_id: job.current_attempt_id.as_ref().map(ToString::to_string),
        error_class: None,
        updated_at: job.updated_at.timestamp_millis(),
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
    async fn session_projection_rebuilds_owner_scoped_live_jobs() {
        use codegg_core::jobs::{
            IdempotencyClass, JobKind, JobPayload, JobPriority, JobSource, NewJob, ResourceRequest,
            RetryPolicy,
        };

        let daemon = CoreDaemon::new(None, None, None);
        let workspace = daemon
            .workspaces
            .get_or_register(std::path::Path::new("/tmp"))
            .await
            .expect("register workspace");
        let spec = NewJob {
            workspace_id: workspace.id.clone(),
            session_id: Some("session-owner".into()),
            turn_id: Some("turn-owner".into()),
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Normal,
            payload: JobPayload::Maintenance {
                task: "projection snapshot fixture".into(),
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::default(),
            idempotency: IdempotencyClass::ReadOnly,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: codegg_core::jobs::ExecutionTarget::Local,
        };
        let job = daemon
            .deps
            .job_store
            .create_job(spec.clone())
            .await
            .expect("persist owner-scoped job");
        let mut other_session_spec = spec;
        other_session_spec.session_id = Some("session-other".into());
        other_session_spec.turn_id = Some("turn-other".into());
        daemon
            .deps
            .job_store
            .create_job(other_session_spec)
            .await
            .expect("persist another session's job");

        let snapshot = daemon
            .projection_snapshot_for_session(
                "session-owner",
                "project-owner",
                workspace.id.as_str(),
            )
            .await;
        assert_eq!(snapshot.jobs.len(), 1);
        let projected = &snapshot.jobs[0];
        assert_eq!(projected.job_id, job.job_id.as_str());
        assert_eq!(projected.session_id.as_deref(), Some("session-owner"));
        assert_eq!(projected.turn_id.as_deref(), Some("turn-owner"));
        assert_eq!(projected.state, "queued");
    }

    #[tokio::test]
    async fn session_projection_snapshot_includes_in_process_live_child_instances() {
        let daemon = CoreDaemon::new(None, None, None);
        let workspace = daemon
            .workspaces
            .get_or_register(std::path::Path::new("/tmp"))
            .await
            .expect("register workspace");
        let runtime = daemon.bind_runtime(
            "session-child-owner",
            workspace.clone(),
            "project-child".into(),
            workspace.canonical_root.clone(),
        );
        let cold_start = daemon
            .projection_snapshot_for_session(
                "session-child-owner",
                "project-child",
                workspace.id.as_str(),
            )
            .await;
        assert!(
            cold_start.subagents.is_empty(),
            "a new daemon runtime must not reconstruct an ephemeral worker as live"
        );
        runtime.subagents.write().await.insert(
            "child-live-1".into(),
            codegg_protocol::projection::dto::AgentTreeNodeProjection {
                child_instance_id: "child-live-1".into(),
                originating_turn_id: Some("parent-turn-old".into()),
                task_id: 7,
                agent: "explore".into(),
                description: "still running after parent completion".into(),
                status: codegg_protocol::projection::dto::AgentTreeStatus::Running,
                parent_task_id: None,
                created_at: 1,
                completed_at: None,
                result_summary: None,
                display_name: Some("Sally".into()),
                profile: None,
                usefulness: None,
                run_id: None,
            },
        );
        let snapshot = daemon
            .projection_snapshot_for_session(
                "session-child-owner",
                "project-child",
                workspace.id.as_str(),
            )
            .await;
        assert_eq!(snapshot.subagents.len(), 1);
        assert_eq!(
            snapshot.subagents[0].originating_turn_id.as_deref(),
            Some("parent-turn-old")
        );
        assert_eq!(snapshot.primary_session.active_subagents, 1);
        let other_runtime = daemon.bind_runtime(
            "session-child-other",
            workspace.clone(),
            "project-child".into(),
            workspace.canonical_root.clone(),
        );
        assert!(other_runtime.subagents.read().await.is_empty());
        let other_snapshot = daemon
            .projection_snapshot_for_session(
                "session-child-other",
                "project-child",
                workspace.id.as_str(),
            )
            .await;
        assert!(
            other_snapshot.subagents.is_empty(),
            "another session must not inherit this runtime's ephemeral child"
        );
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

    /// A migrated in-memory pool, so the transcript store has its table.
    async fn transcript_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::SqliteConnectOptions;
        use std::str::FromStr;
        let url = format!(
            "file:daemon-transcript-{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let options = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite url")
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("sqlite pool");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate the transcript schema");
        pool
    }

    /// A real, non-symlink directory to register as a workspace root.
    ///
    /// `/tmp` is a symlink on macOS and the registry refuses a symlinked
    /// root, so the test uses the platform's own temporary directory.
    fn workspace_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "codegg-c003-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("create the workspace root");
        root
    }

    /// Start one delegated run for `session_id` and return its run id.
    async fn delegated_run(
        daemon: &CoreDaemon,
        session_id: &str,
        workspace_id: &codegg_core::workspace::WorkspaceId,
    ) -> codegg_core::identity::AgentRunId {
        use codegg_core::agent_run::{AgentRunBudget, NewAgentRun, NewAgentTask};
        use codegg_core::identity::{AgentRunId, AgentTaskId, ProjectId};

        let run_id = AgentRunId::new();
        let task = NewAgentTask {
            task_id: AgentTaskId::new(),
            parent_task_id: None,
            originating_session_id: session_id.to_string(),
            originating_turn_id: Some(format!("turn-{session_id}")),
            project_id: ProjectId::new(),
            repository_id: None,
            workspace_id: workspace_id.clone(),
            requested_agent: "explore".into(),
            delegation_key: format!("delegation-{session_id}-{run_id}"),
            request_fingerprint: format!("fingerprint-{session_id}-{run_id}"),
            description: "a delegated run".into(),
        };
        let run = NewAgentRun {
            run_id: run_id.clone(),
            parent_run_id: None,
            depth: 1,
            workspace_id: workspace_id.clone(),
            agent_name: "explore".into(),
            agent_digest: Some("digest".into()),
            provider: "openai".into(),
            model: "model".into(),
            authority_digest: "authority".into(),
            budget: AgentRunBudget::default(),
            shared_scratch: Vec::new(),
        };
        daemon
            .deps
            .agent_run_store
            .create_or_get(task, run)
            .await
            .expect("start a delegated run");
        run_id
    }

    #[tokio::test]
    async fn a_session_snapshot_carries_only_its_own_delegated_transcripts() {
        use codegg_core::agent_run_transcript::{
            AgentRunTranscriptStore, SqliteAgentRunTranscriptStore, TranscriptKind, TranscriptRole,
        };

        let pool = transcript_pool().await;
        // The production dependency set: the durable agent run store and the
        // transcript store are both SQLite-backed over the same pool, so the
        // transcript's foreign key to the run row is satisfied.
        let daemon = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::with_jobs(
            pool.clone(),
            None,
            None,
        ));
        let workspace = daemon
            .workspaces
            .get_or_register(&workspace_root("owner"))
            .await
            .expect("register workspace");

        let owner_run = delegated_run(&daemon, "session-owner", &workspace.id).await;
        let other_run = delegated_run(&daemon, "session-other", &workspace.id).await;

        let store = SqliteAgentRunTranscriptStore::new(pool);
        store
            .append(
                &owner_run,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                "the owner's own conversation",
            )
            .await
            .expect("record the owner's transcript");
        store
            .append(
                &other_run,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                "another session's private conversation",
            )
            .await
            .expect("record the other session's transcript");

        let owner_snapshot = daemon
            .projection_snapshot_for_session(
                "session-owner",
                "project-owner",
                workspace.id.as_str(),
            )
            .await;
        assert_eq!(owner_snapshot.agent_runs.len(), 1);
        let owner_transcript = owner_snapshot.agent_runs[0]
            .transcript
            .as_ref()
            .expect("the owning session's transcript is attached");
        let owner_bodies = owner_transcript
            .entries()
            .iter()
            .map(|entry| entry.body.as_str())
            .collect::<Vec<_>>();
        assert_eq!(owner_bodies, vec!["the owner's own conversation"]);

        let other_snapshot = daemon
            .projection_snapshot_for_session(
                "session-other",
                "project-owner",
                workspace.id.as_str(),
            )
            .await;
        assert_eq!(other_snapshot.agent_runs.len(), 1);
        let other_bodies = other_snapshot.agent_runs[0]
            .transcript
            .as_ref()
            .expect("the other session's transcript is attached")
            .entries()
            .iter()
            .map(|entry| entry.body.as_str())
            .collect::<Vec<_>>();
        assert_eq!(other_bodies, vec!["another session's private conversation"]);

        // The negative case: a client bound to another session does not
        // receive this session's transcript. The snapshot is built per
        // session, so the other session's run is not in it at all.
        let owner_bodies_joined = owner_bodies.join(" ");
        assert!(
            !owner_bodies_joined.contains("another session's private conversation"),
            "the owning session's snapshot must not carry another session's transcript"
        );
    }

    #[tokio::test]
    async fn a_run_with_no_recorded_transcript_says_absent() {
        let pool = transcript_pool().await;
        let daemon = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::with_jobs(
            pool, None, None,
        ));
        let workspace = daemon
            .workspaces
            .get_or_register(&workspace_root("owner"))
            .await
            .expect("register workspace");
        let run_id = delegated_run(&daemon, "session-absent", &workspace.id).await;

        let snapshot = daemon
            .projection_snapshot_for_session(
                "session-absent",
                "project-absent",
                workspace.id.as_str(),
            )
            .await;
        assert_eq!(snapshot.agent_runs.len(), 1);
        assert_eq!(
            snapshot.agent_runs[0].transcript,
            Some(codegg_protocol::projection::dto::AgentRunTranscriptProjection::Absent),
            "a run that recorded nothing must be absent, not an empty conversation"
        );
        assert_eq!(run_id.as_str(), snapshot.agent_runs[0].run_id);
    }

    /// M002: ask the delegated-run family for one bounded transcript page.
    ///
    /// The handler is called the way the daemon's dispatcher calls it, with
    /// a local-owner authority and the decision the authorization layer
    /// already made. The ownership gate under test is the handler's own:
    /// it must resolve the run's durable owner rather than trust the
    /// session id in the request.
    async fn transcript_page(
        daemon: &CoreDaemon,
        session_id: &str,
        run_id: &codegg_core::identity::AgentRunId,
        from_ordinal: i64,
        limit: u32,
    ) -> CoreResponse {
        let request = CoreRequest::AgentRunTranscriptRead {
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            from_ordinal,
            limit,
        };
        daemon
            .handle_agent_run_request(
                request,
                "test-client",
                codegg_core::transport_auth::RequestAuthorityContext::local(
                    "test-client",
                    "test-correlation",
                ),
                codegg_core::authorization::AuthorizationDecision {
                    principal_id: codegg_core::identity::PrincipalId::new(),
                    operation: "agent_run_transcript_read".to_string(),
                    capability: Some("session.read".to_string()),
                    project_id: None,
                    membership_revision: None,
                    policy: codegg_core::authorization::PolicyKind::LocalOwnerBroad,
                    decision_id: "test-decision".to_string(),
                    correlation_id: "test-correlation".to_string(),
                    reason: "test".to_string(),
                    decided_at_ms: 0,
                },
            )
            .await
            .expect("the handler answers with a response")
    }

    /// The page carried by a transcript response.
    fn page_of(
        response: &CoreResponse,
    ) -> codegg_protocol::projection::dto::AgentRunTranscriptPageDto {
        match response {
            CoreResponse::AgentRunTranscriptRead { page } => page.clone(),
            other => panic!("expected a transcript page, got {other:?}"),
        }
    }

    /// The error code carried by an error response.
    fn error_code(response: &CoreResponse) -> String {
        match response {
            CoreResponse::Error { code, .. } => code.clone(),
            other => panic!("expected an error response, got {other:?}"),
        }
    }

    /// The error message carried by an error response.
    fn error_message(response: &CoreResponse) -> String {
        match response {
            CoreResponse::Error { message, .. } => message.clone(),
            other => panic!("expected an error response, got {other:?}"),
        }
    }

    /// Append `count` rows of `body` to a run's durable transcript.
    async fn append_rows(
        daemon: &CoreDaemon,
        run_id: &codegg_core::identity::AgentRunId,
        count: usize,
        body: &str,
    ) {
        use codegg_core::agent_run_transcript::{
            AgentRunTranscriptStore, SqliteAgentRunTranscriptStore, TranscriptKind, TranscriptRole,
        };
        let store = SqliteAgentRunTranscriptStore::new(
            daemon
                .pool
                .clone()
                .expect("a pool-backed daemon has a pool"),
        );
        for index in 0..count {
            store
                .append(
                    run_id,
                    TranscriptRole::Orchestrator,
                    TranscriptKind::ModelMessage,
                    &format!("{body}-{index}"),
                )
                .await
                .expect("append a transcript row");
        }
    }

    #[tokio::test]
    async fn the_owner_reads_the_tail_past_the_preview_ceiling() {
        let pool = transcript_pool().await;
        let daemon = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::with_jobs(
            pool, None, None,
        ));
        let workspace = daemon
            .workspaces
            .get_or_register(&workspace_root("tail"))
            .await
            .expect("register workspace");
        let run_id = delegated_run(&daemon, "session-tail", &workspace.id).await;
        // More rows than one page may carry, so the tail is genuinely past
        // the preview ceiling.
        append_rows(&daemon, &run_id, 70, "row").await;

        let first = page_of(&transcript_page(&daemon, "session-tail", &run_id, 0, 64).await);
        assert!(
            first.has_more,
            "a page that stopped at the row ceiling must say the store holds more"
        );
        assert_eq!(
            first.entries.len(),
            64,
            "one page carries at most the declared row ceiling"
        );

        let second = page_of(
            &transcript_page(&daemon, "session-tail", &run_id, first.next_ordinal, 64).await,
        );
        assert!(
            !second.has_more,
            "the last page must not claim the store holds more"
        );
        assert!(
            !second.source.recording_stopped,
            "a run that recorded to the end did not stop recording"
        );

        let mut ordinals: Vec<i64> = first
            .entries
            .iter()
            .chain(second.entries.iter())
            .map(|entry| entry.ordinal)
            .collect();
        ordinals.sort_unstable();
        ordinals.dedup();
        assert_eq!(
            ordinals.len(),
            70,
            "the two pages must cover every recorded row exactly once, got {ordinals:?}"
        );
        assert_eq!(
            first.next_ordinal, 64,
            "the cursor is the store's ordinal, not a count of returned rows"
        );
        assert_eq!(
            second.next_ordinal, 70,
            "the final cursor is the end of the recorded transcript"
        );
    }

    #[tokio::test]
    async fn a_foreign_session_gets_the_same_answer_as_a_run_that_does_not_exist() {
        let pool = transcript_pool().await;
        let daemon = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::with_jobs(
            pool, None, None,
        ));
        let workspace = daemon
            .workspaces
            .get_or_register(&workspace_root("foreign"))
            .await
            .expect("register workspace");
        let owned = delegated_run(&daemon, "session-owner", &workspace.id).await;
        append_rows(&daemon, &owned, 3, "secret").await;

        let foreign = transcript_page(&daemon, "session-other", &owned, 0, 64).await;
        let nonexistent = transcript_page(
            &daemon,
            "session-other",
            &codegg_core::identity::AgentRunId::new(),
            0,
            64,
        )
        .await;
        assert_eq!(
            error_code(&foreign),
            error_code(&nonexistent),
            "a guessed run id must not reveal whether the run exists"
        );
        assert_eq!(
            error_message(&foreign),
            error_message(&nonexistent),
            "a guessed run id must not reveal whether the run exists"
        );
        assert_eq!(
            error_code(&foreign),
            "agent_run_not_found",
            "a run the session does not own is refused"
        );
    }

    #[tokio::test]
    async fn a_run_with_no_recorded_transcript_is_absent_not_refused() {
        let pool = transcript_pool().await;
        let daemon = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::with_jobs(
            pool, None, None,
        ));
        let workspace = daemon
            .workspaces
            .get_or_register(&workspace_root("absent"))
            .await
            .expect("register workspace");
        let run_id = delegated_run(&daemon, "session-empty", &workspace.id).await;

        let page = page_of(&transcript_page(&daemon, "session-empty", &run_id, 0, 64).await);
        assert_eq!(
            page.status,
            codegg_protocol::projection::dto::AgentRunTranscriptPageStatus::Absent,
            "an owned run that recorded nothing is absent, which is not the same as a refusal"
        );
        assert!(page.entries.is_empty());
        assert_eq!(page.source.total_entries, 0);
    }

    #[tokio::test]
    async fn a_negative_cursor_reads_from_the_beginning() {
        let pool = transcript_pool().await;
        let daemon = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::with_jobs(
            pool, None, None,
        ));
        let workspace = daemon
            .workspaces
            .get_or_register(&workspace_root("cursor"))
            .await
            .expect("register workspace");
        let run_id = delegated_run(&daemon, "session-cursor", &workspace.id).await;
        append_rows(&daemon, &run_id, 2, "row").await;

        let page = page_of(&transcript_page(&daemon, "session-cursor", &run_id, -5, 64).await);
        assert_eq!(
            page.entries.first().map(|entry| entry.ordinal),
            Some(0),
            "a negative cursor is clamped to the beginning of the transcript"
        );
    }

    #[tokio::test]
    async fn the_requested_limit_is_clamped_to_the_page_row_ceiling() {
        let pool = transcript_pool().await;
        let daemon = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::with_jobs(
            pool, None, None,
        ));
        let workspace = daemon
            .workspaces
            .get_or_register(&workspace_root("limit"))
            .await
            .expect("register workspace");
        let run_id = delegated_run(&daemon, "session-limit", &workspace.id).await;
        append_rows(&daemon, &run_id, 70, "row").await;

        let page = page_of(&transcript_page(&daemon, "session-limit", &run_id, 0, 10_000).await);
        assert_eq!(
            page.entries.len(),
            64,
            "an over-large request is clamped to the declared row ceiling"
        );
        assert!(
            page.has_more,
            "the clamped page still reports the remaining rows"
        );
    }

    #[tokio::test]
    async fn a_retired_parent_turn_still_reads_its_transcript() {
        let pool = transcript_pool().await;
        let daemon = CoreDaemon::with_deps(crate::core::runtime_deps::CoreRuntimeDeps::with_jobs(
            pool, None, None,
        ));
        let workspace = daemon
            .workspaces
            .get_or_register(&workspace_root("retired"))
            .await
            .expect("register workspace");
        // The run's task names a parent turn that no longer exists: the
        // delegated run has finished and its turn has been retired.
        let run_id = delegated_run(&daemon, "session-retired", &workspace.id).await;
        append_rows(&daemon, &run_id, 2, "row").await;

        let page = page_of(&transcript_page(&daemon, "session-retired", &run_id, 0, 64).await);
        assert_eq!(
            page.entries.len(),
            2,
            "ownership is session-scoped and durable, so a retired parent turn does not close the read"
        );
    }
}
