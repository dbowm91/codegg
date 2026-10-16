//! M002: `projects` request family for `CoreDaemon`.
//!
//! Project catalog, workspace registry/services, managed worktrees, and daemon/workspace snapshots.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;

impl CoreDaemon {
    /// Post-commit cleanup shared by the daemon `ProjectDelete` handler and
    /// the HTTP `DELETE /api/projects/{id}` route. Runs only after
    /// `ProjectCatalog::delete_project` commits: resolves parked
    /// permission/question waiters for the deleted sessions, cancels their
    /// live turns, evicts their runtimes, and publishes
    /// `CoreEvent::ProjectDeleted` so WS/TUI clients refresh instead of
    /// going stale.
    pub async fn note_project_destroyed(
        &self,
        project_id: &codegg_core::identity::ProjectId,
        deleted_session_ids: &[String],
        deleted_session_count: usize,
    ) {
        for session_id in deleted_session_ids {
            crate::bus::PermissionRegistry::cancel_session(session_id);
            crate::bus::QuestionRegistry::cancel_session(session_id);
            if let Some(runtime) = self.sessions.get(session_id).map(|runtime| runtime.clone()) {
                if let Some(active_turn) = runtime.active_turn.read().await.as_ref() {
                    let _ = active_turn.cancel_tx.send(true);
                }
                self.sessions.remove(session_id);
            }
        }
        self.event_log
            .publish(
                Some(project_id.to_string()),
                None,
                CoreEvent::ProjectDeleted {
                    project_id: project_id.to_string(),
                    deleted_session_count,
                },
            )
            .await;
    }

    /// Post-commit cleanup shared by the daemon `ProjectUpdate` handler and
    /// the HTTP `PATCH /api/projects/{id}` route. Cancels live turns and
    /// evicts rebound runtimes (their stored root is stale), then publishes
    /// the caller-built `CoreEvent::ProjectUpdated`.
    pub async fn note_project_rebound(
        &self,
        project_id: &codegg_core::identity::ProjectId,
        rebound_session_ids: &[String],
        event: CoreEvent,
    ) {
        for session_id in rebound_session_ids {
            if let Some(runtime) = self.sessions.get(session_id).map(|runtime| runtime.clone()) {
                if let Some(active_turn) = runtime.active_turn.read().await.as_ref() {
                    let _ = active_turn.cancel_tx.send(true);
                }
                self.sessions.remove(session_id);
            }
        }
        self.event_log
            .publish(Some(project_id.to_string()), None, event)
            .await;
    }

    pub(crate) async fn handle_projects_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = trusted_client_id;
        match request {
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
            CoreRequest::ManagedWorktreeGet { worktree_id } => {
                let id = match codegg_core::identity::WorktreeId::parse(&worktree_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "managed_worktree_invalid_id".into(),
                            message: error.to_string(),
                        })
                    }
                };
                match self.worktree_service.refresh(&id).await {
                    Ok(record) => Ok(CoreResponse::ManagedWorktree {
                        worktree: codegg_core::protocol_conversions::managed_worktree_to_dto(
                            &record,
                        ),
                    }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: "managed_worktree_get_failed".into(),
                        message: error.to_string(),
                    }),
                }
            }
            CoreRequest::ManagedWorktreeList {
                workspace_id,
                repository_id,
                run_id,
                include_removed,
            } => {
                let workspace_id = match workspace_id {
                    Some(value) => match codegg_core::workspace::WorkspaceId::parse(&value) {
                        Ok(id) => Some(id),
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "managed_worktree_invalid_workspace_id".into(),
                                message: error.to_string(),
                            })
                        }
                    },
                    None => None,
                };
                let repository_id = match repository_id {
                    Some(value) => match codegg_core::identity::RepositoryId::parse(&value) {
                        Ok(id) => Some(id),
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "managed_worktree_invalid_repository_id".into(),
                                message: error.to_string(),
                            })
                        }
                    },
                    None => None,
                };
                let owner_run_id = match run_id {
                    Some(value) => match codegg_core::identity::AgentRunId::parse(&value) {
                        Ok(id) => Some(id),
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "managed_worktree_invalid_run_id".into(),
                                message: error.to_string(),
                            })
                        }
                    },
                    None => None,
                };
                match self
                    .worktree_service
                    .list(codegg_core::worktree_service::WorktreeQuery {
                        workspace_id,
                        repository_id,
                        owner_run_id,
                        include_removed,
                    })
                    .await
                {
                    Ok(records) => Ok(CoreResponse::ManagedWorktrees {
                        worktrees: records
                            .iter()
                            .map(codegg_core::protocol_conversions::managed_worktree_to_dto)
                            .collect(),
                    }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: "managed_worktree_list_failed".into(),
                        message: error.to_string(),
                    }),
                }
            }
            CoreRequest::ManagedWorktreeCleanup {
                worktree_id,
                lease_generation,
            } => {
                let id = match codegg_core::identity::WorktreeId::parse(&worktree_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "managed_worktree_invalid_id".into(),
                            message: error.to_string(),
                        })
                    }
                };
                match self.worktree_service.cleanup(&id, lease_generation).await {
                    Ok(record) => Ok(CoreResponse::ManagedWorktreeCleaned {
                        worktree: codegg_core::protocol_conversions::managed_worktree_to_dto(
                            &record,
                        ),
                    }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: "managed_worktree_cleanup_failed".into(),
                        message: error.to_string(),
                    }),
                }
            }
            CoreRequest::ManagedWorktreeArchive {
                worktree_id,
                lease_generation,
            } => {
                let id = match codegg_core::identity::WorktreeId::parse(&worktree_id) {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "managed_worktree_invalid_id".into(),
                            message: error.to_string(),
                        })
                    }
                };
                match self.worktree_service.archive(&id, lease_generation).await {
                    Ok(record) => Ok(CoreResponse::ManagedWorktreeArchived {
                        worktree: codegg_core::protocol_conversions::managed_worktree_to_dto(
                            &record,
                        ),
                    }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: "managed_worktree_archive_failed".into(),
                        message: error.to_string(),
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
            CoreRequest::WorkspaceDashboard {
                cursor,
                limit,
                include_archived,
            } => {
                // M004: bounded global aggregate over the catalog plus
                // cheap durable/in-memory counts. No project activation,
                // no service probing (see `daemon_workspace_dashboard`).
                self.handle_workspace_dashboard_request(
                    cursor,
                    limit,
                    include_archived,
                    trusted_client_id,
                )
                .await
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
                    Ok(records) => {
                        // M003: enumeration is privacy-filtered to the
                        // caller's `project.read` grants.
                        let records = self
                            .filter_projects_for_principal(trusted_client_id, records)
                            .await;
                        Ok(CoreResponse::ProjectList {
                            truncated: records.len() > limit,
                            projects: records
                                .iter()
                                .take(limit)
                                .map(codegg_core::protocol_conversions::project_catalog_record_to_dto)
                                .collect(),
                        })
                    }
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
                        self.event_log
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
                self.project_lifecycle_request(
                    &project_id,
                    false,
                    request_id,
                    &authority,
                    &authz_decision,
                )
                .await
            }
            CoreRequest::ProjectRestore { project_id } => {
                self.project_lifecycle_request(
                    &project_id,
                    true,
                    request_id,
                    &authority,
                    &authz_decision,
                )
                .await
            }
            CoreRequest::ProjectDelete { request } => {
                let project_id = match codegg_core::identity::ProjectId::parse(&request.project_id)
                {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_project_id".into(),
                            message: error.to_string(),
                        })
                    }
                };
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "project_catalog_unavailable".into(),
                        message: "project deletion requires a catalog".into(),
                    });
                };
                let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool);
                match catalog
                    .delete_project(&project_id, request.expected_session_count)
                    .await
                {
                    Ok(result) => {
                        let deleted_session_count = result.deleted_session_count();
                        self.note_project_destroyed(
                            &project_id,
                            &result.deleted_session_ids,
                            deleted_session_count,
                        )
                        .await;
                        let provenance =
                            codegg_core::authorization::audit_provenance(&authz_decision);
                        let audit = codegg_core::audit::AuditEventBuilder::new(
                            codegg_core::audit::AuditAction::ConfigChange,
                            authority.principal(),
                            &provenance,
                        )
                        .with_correlation(request_id)
                        .with_project(project_id.clone())
                        .with_metadata("project.lifecycle.operation", "destroy")
                        .with_metadata(
                            "project.deleted_session_count",
                            deleted_session_count.to_string(),
                        )
                        .with_metadata("decision.id", provenance.decision_id())
                        .with_metadata("decision.outcome", "allow");
                        self.audit_emitter().emit(audit).await;
                        Ok(CoreResponse::ProjectDeleted {
                            project_id: project_id.to_string(),
                            deleted_session_count,
                        })
                    }
                    Err(error) => Ok(Self::project_catalog_error("project delete failed", &error)),
                }
            }
            CoreRequest::ProjectUpdate { request } => {
                let project_id = match codegg_core::identity::ProjectId::parse(&request.project_id)
                {
                    Ok(id) => id,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_project_id".into(),
                            message: error.to_string(),
                        })
                    }
                };
                let requested_root = std::path::PathBuf::from(&request.root);
                if !requested_root.is_absolute() {
                    return Ok(CoreResponse::Error {
                        code: "invalid_workspace_root".into(),
                        message: "project root must be an absolute local directory".into(),
                    });
                }
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "project_catalog_unavailable".into(),
                        message: "project update requires a catalog".into(),
                    });
                };
                let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool);
                let current = match catalog.get_project(&project_id).await {
                    Ok(project) => project,
                    Err(error) => {
                        return Ok(Self::project_catalog_error("project get failed", &error))
                    }
                };
                if current.updated_at.timestamp_millis() != request.expected_updated_at {
                    return Ok(CoreResponse::Error {
                        code: "project_update_conflict".into(),
                        message: "project changed since the edit form was opened; reload and retry"
                            .into(),
                    });
                }
                let previous_root = match catalog.list_workspaces_for_project(&project_id).await {
                    Ok(workspaces) => workspaces
                        .first()
                        .map(|workspace| workspace.canonical_root.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    Err(error) => {
                        return Ok(Self::project_catalog_error(
                            "project workspace lookup failed",
                            &error,
                        ))
                    }
                };
                let workspace = match self.workspaces.get_or_register(&requested_root).await {
                    Ok(workspace) => workspace,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_workspace_root".into(),
                            message: error.to_string(),
                        })
                    }
                };
                let result = match catalog
                    .update_project(
                        &project_id,
                        &request.display_name,
                        &workspace.id,
                        request.expected_updated_at,
                        "project-picker-update",
                    )
                    .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        return Ok(Self::project_catalog_error("project update failed", &error))
                    }
                };
                let project = codegg_core::protocol_conversions::project_catalog_record_to_dto(
                    &result.project,
                );
                let workspace_dto = crate::protocol::dto::ProjectWorkspaceSummaryDto {
                    workspace_id: workspace.id.to_string(),
                    display_name: workspace.display_name.clone(),
                    canonical_root: Some(workspace.canonical_root.to_string_lossy().into_owned()),
                };
                let provenance = codegg_core::authorization::audit_provenance(&authz_decision);
                let audit = codegg_core::audit::AuditEventBuilder::new(
                    codegg_core::audit::AuditAction::ConfigChange,
                    authority.principal(),
                    &provenance,
                )
                .with_correlation(request_id)
                .with_project(project_id.clone())
                .with_metadata("project.update.operation", "rename_repath")
                .with_metadata("project.update.previous_name", &current.display_name)
                .with_metadata("project.update.new_name", &result.project.display_name)
                .with_metadata("project.update.previous_root", &previous_root)
                .with_metadata(
                    "project.update.new_root",
                    workspace.canonical_root.to_string_lossy().as_ref(),
                )
                .with_metadata(
                    "project.update.rebound_session_count",
                    result.rebound_session_ids.len().to_string(),
                )
                .with_metadata("decision.id", provenance.decision_id())
                .with_metadata("decision.outcome", "allow");
                self.audit_emitter().emit(audit).await;
                let event = CoreEvent::ProjectUpdated {
                    project: project.clone(),
                    workspace: workspace_dto.clone(),
                    previous_display_name: current.display_name.clone(),
                    previous_root: previous_root.clone(),
                };
                self.note_project_rebound(&project_id, &result.rebound_session_ids, event)
                    .await;
                Ok(CoreResponse::ProjectUpdated {
                    project,
                    workspace: workspace_dto,
                    previous_display_name: current.display_name,
                    previous_root,
                })
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
                self.event_log
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
            CoreRequest::SnapshotDaemon => {
                let event_seq = self.event_log.current_seq();
                let session_ids = self.sessions.list_sessions();
                let mut snapshots = Vec::new();
                for sid in &session_ids {
                    if let Some(runtime) = self.sessions.get(sid) {
                        // Best-effort, eventually-consistent snapshot (see
                        // `SessionRuntime::snapshot`); state may race with
                        // live turns between fields.
                        let snap = runtime.snapshot().await;
                        let status = format!("{:?}", snap.status);
                        let model = snap.selected_model;
                        let agent = snap.selected_agent;
                        let has_active_turn = snap.has_active_turn;
                        let pending_permissions = snap.pending_permissions;
                        let pending_questions = snap.pending_questions;
                        let input_tokens = snap.input_tokens;
                        let output_tokens = snap.output_tokens;
                        let active_subagents = snap.active_subagents;
                        // M004: safe controller projection from the
                        // in-memory handle (principal id plus coarse
                        // revision only). Durable daemons mirror the
                        // store into the handle on every mutation, so
                        // this stays coherent without a DB read per row.
                        let active = runtime.active_turn.read().await;
                        let (controller_principal, controller_revision) = match active.as_ref() {
                            Some(handle) => (
                                handle.controller_principal.clone(),
                                Some(handle.controller_revision),
                            ),
                            None => (None, None),
                        };
                        drop(active);
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
                            controller_principal,
                            controller_revision,
                        });
                    }
                }
                let scheduler_snapshot = match self.deps.scheduler.as_ref() {
                    Some(scheduler) => match serde_json::to_value(scheduler.snapshot().await) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!(error = %e, "failed to serialize scheduler snapshot");
                            None
                        }
                    },
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
                        let cmd = crate::git_mutations::GitEnvPolicy::default().apply(&argv, root);
                        let Ok(mut cmd) = cmd else {
                            return Err(AppError::Worktree(cmd.unwrap_err().to_string()));
                        };
                        match cmd.output().await {
                            Ok(output) => {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let changed_files = stdout.lines().count();
                                Some(serde_json::json!({
                                    "git_root": root.to_string_lossy(),
                                    "changed_files": changed_files,
                                }))
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "git status failed in snapshot");
                                None
                            }
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Corrective regression: the HTTP destroy/update routes share these
    /// post-commit hooks with the daemon handlers. A route that skips them
    /// leaves live turns running, parks permission/question waiters with no
    /// UI target, and publishes no event, so WS/TUI clients go stale.
    async fn post_commit_daemon() -> CoreDaemon {
        CoreDaemon::new(None, None, None)
    }

    async fn live_turn(
        daemon: &CoreDaemon,
        session_id: &str,
    ) -> tokio::sync::watch::Receiver<bool> {
        let workspace_id = codegg_core::workspace::WorkspaceId::new();
        let root = std::path::PathBuf::from("/tmp/codegg-post-commit-test");
        let runtime = daemon.sessions.get_or_create(
            session_id,
            workspace_id,
            root.clone(),
            "test-project".to_string(),
            root,
        );
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        *runtime.active_turn.write().await = Some(crate::core::session_runtime::TurnHandle {
            turn_id: format!("{session_id}-turn"),
            cancel_tx,
            steer_tx: None,
            started_at: chrono::Utc::now(),
            asset_pin: None,
            controller_principal: None,
            controller_client: None,
            controller_revision: 0,
        });
        cancel_rx
    }

    fn park_waiters(
        session_id: &str,
    ) -> (
        tokio::sync::oneshot::Receiver<codegg_core::bus::PermissionDecision>,
        tokio::sync::oneshot::Receiver<String>,
    ) {
        // Payload types mirror the registry waiter halves; the assertions
        // below only observe the closed sender, never a delivered value.
        let (perm_tx, perm_rx) = tokio::sync::oneshot::channel();
        let (q_tx, q_rx) = tokio::sync::oneshot::channel();
        crate::bus::PermissionRegistry::register_with_session(
            session_id.to_string(),
            None,
            format!("{session_id}-perm"),
            perm_tx,
        );
        crate::bus::QuestionRegistry::register_with_session(
            session_id.to_string(),
            None,
            format!("{session_id}-question"),
            q_tx,
        );
        (perm_rx, q_rx)
    }

    #[tokio::test]
    async fn note_project_destroyed_cancels_evicts_closes_waiters_and_publishes() {
        let daemon = post_commit_daemon().await;
        let doomed = format!("doomed-{}", uuid::Uuid::new_v4());
        let spared = format!("spared-{}", uuid::Uuid::new_v4());
        let mut cancel_rx = live_turn(&daemon, &doomed).await;
        let spared_cancel_rx = live_turn(&daemon, &spared).await;
        let (mut perm_rx, mut q_rx) = park_waiters(&doomed);
        let (mut spared_perm_rx, mut spared_q_rx) = park_waiters(&spared);
        let mut events = daemon.event_log.subscribe();

        let project_id = codegg_core::identity::ProjectId::new();
        daemon
            .note_project_destroyed(&project_id, &[doomed.clone()], 1)
            .await;

        // Live turn cancelled, runtime evicted, parked waiters resolved.
        assert!(cancel_rx.changed().await.is_ok());
        assert!(*cancel_rx.borrow());
        assert!(daemon.sessions.get(&doomed).is_none());
        assert_eq!(
            perm_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
        assert_eq!(
            q_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
        // Event published with the confirmed count.
        let envelope = events.recv().await.expect("ProjectDeleted event");
        assert!(
            matches!(
                &envelope.payload,
                CoreEvent::ProjectDeleted {
                    project_id: got,
                    deleted_session_count: 1,
                } if got == &project_id.to_string()
            ),
            "expected ProjectDeleted, got {:?}",
            envelope.payload
        );
        // Unrelated session untouched: turn live, runtime kept, waiters parked.
        assert!(!spared_cancel_rx.has_changed().unwrap_or(true));
        assert!(daemon.sessions.get(&spared).is_some());
        assert!(spared_perm_rx.try_recv().is_err());
        assert!(spared_q_rx.try_recv().is_err());
        crate::bus::PermissionRegistry::cancel_session(&spared);
        crate::bus::QuestionRegistry::cancel_session(&spared);
    }

    #[tokio::test]
    async fn note_project_rebound_cancels_evicts_and_publishes_update() {
        let daemon = post_commit_daemon().await;
        let rebound = format!("rebound-{}", uuid::Uuid::new_v4());
        let mut cancel_rx = live_turn(&daemon, &rebound).await;
        let mut events = daemon.event_log.subscribe();

        let project_id = codegg_core::identity::ProjectId::new();
        let event = CoreEvent::ProjectUpdated {
            project: crate::protocol::dto::ProjectSummaryDto {
                project_id: project_id.to_string(),
                display_name: "renamed".to_string(),
                lifecycle: "active".to_string(),
                description: None,
                tags: Vec::new(),
                time_last_opened_at: None,
                registration_source: "test".to_string(),
                archived_at: None,
                created_at: 0,
                updated_at: 0,
            },
            workspace: crate::protocol::dto::ProjectWorkspaceSummaryDto {
                workspace_id: "ws".to_string(),
                display_name: "ws".to_string(),
                canonical_root: Some("/tmp/new".to_string()),
            },
            previous_display_name: "old".to_string(),
            previous_root: "/tmp/old".to_string(),
        };
        daemon
            .note_project_rebound(&project_id, &[rebound.clone()], event)
            .await;

        assert!(cancel_rx.changed().await.is_ok());
        assert!(*cancel_rx.borrow());
        assert!(daemon.sessions.get(&rebound).is_none());
        let envelope = events.recv().await.expect("ProjectUpdated event");
        assert!(
            matches!(
                &envelope.payload,
                CoreEvent::ProjectUpdated {
                    previous_display_name,
                    ..
                } if previous_display_name == "old"
            ),
            "expected ProjectUpdated, got {:?}",
            envelope.payload
        );
    }
}
