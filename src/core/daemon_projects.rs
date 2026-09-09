//! M002: `projects` request family for `CoreDaemon`.
//!
//! Project catalog, workspace registry/services, managed worktrees, and daemon/workspace snapshots.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;

impl CoreDaemon {
    pub(crate) async fn handle_projects_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (request_id, trusted_client_id, &authority, &authz_decision);
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
                        let mut cmd =
                            crate::git_mutations::GitEnvPolicy::default().apply(&argv, root);
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
