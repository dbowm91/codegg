//! M002: `sessions` request family for `CoreDaemon`.
//!
//! Session CRUD, selection reads, message reads, and import/export over daemon-owned session stores.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;

impl CoreDaemon {
    pub(crate) async fn handle_sessions_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (request_id, trusted_client_id, &authority, &authz_decision);
        match request {
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
                        ) => {
                            // M003: the provider selection carries the
                            // requesting principal's origin.
                            self.record_origin_with_decision(
                                &authority,
                                &authz_decision,
                                "provider",
                                connection_id.as_str(),
                            )
                            .await;
                            // M005: structural provider-selection event.
                            {
                                let provenance =
                                    codegg_core::authorization::audit_provenance(&authz_decision);
                                let mut chain =
                                    codegg_core::audit_instrumentation::AuditChainContext::new();
                                chain.project = authz_decision.project_id.clone();
                                chain.session_id = Some(req.session_id.clone());
                                chain.provider_connection_id =
                                    Some(connection_id.as_str().to_owned());
                                let builder =
                                    codegg_core::audit_instrumentation::provider_select_event(
                                        authority.principal(),
                                        &provenance,
                                        &chain,
                                        req.session_id.as_str(),
                                        connection_id.as_str(),
                                        req.model_id.as_str(),
                                        "allow",
                                    );
                                self.append_audit_event(builder).await;
                            }
                            Ok(CoreResponse::SessionSelectionUpdated {
                                session_id: req.session_id,
                                selection,
                            })
                        }
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
                        messages: crate::protocol_conversions::messages_to_dtos(messages)
                            .unwrap_or_else(|e| {
                                tracing::error!(error = %e, "messages_to_dtos conversion failed");
                                Default::default()
                            }),
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
                                    let created = Self::session_dto(session, Some(&context));
                                    // M003: capture originating-principal
                                    // attribution for the new session.
                                    self.record_origin_with_decision(
                                        &authority,
                                        &authz_decision,
                                        "session",
                                        created.id.as_str(),
                                    )
                                    .await;
                                    // M005: structural session-create event
                                    // with the durable session id.
                                    {
                                        let provenance =
                                            codegg_core::authorization::audit_provenance(
                                                &authz_decision,
                                            );
                                        let mut chain =
                                            codegg_core::audit_instrumentation::AuditChainContext::new(
                                            );
                                        chain.project = Some(context.project_id.clone());
                                        chain.session_id = Some(created.id.clone());
                                        let builder =
                                            codegg_core::audit_instrumentation::session_create_event(
                                                authority.principal(),
                                                &provenance,
                                                &chain,
                                                created.id.as_str(),
                                                "allow",
                                            );
                                        self.append_audit_event(builder).await;
                                    }
                                    Ok(CoreResponse::Session { session: created })
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
                        // Presence M001: meaningful session activity.
                        // Best-effort only; session correctness never
                        // depends on presence.
                        self.note_presence_activity(
                            &authority,
                            trusted_client_id,
                            authz_decision.project_id.clone(),
                            Some(session_id.as_str()),
                            codegg_core::presence::PresenceActivity::Active,
                        );
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
                            if let Err(delete_error) = store.delete(&child.id).await {
                                tracing::warn!(
                                    error = %delete_error,
                                    session_id = %child.id,
                                    "failed to clean up unbound forked session"
                                );
                            }
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
                        session: crate::protocol_conversions::session_to_dto(session)
                            .unwrap_or_else(|e| {
                                tracing::error!(error = %e, "session_to_dto conversion failed");
                                Default::default()
                            }),
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
                        session: crate::protocol_conversions::session_to_dto(session)
                            .unwrap_or_else(|e| {
                                tracing::error!(error = %e, "session_to_dto conversion failed");
                                Default::default()
                            }),
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
                        session: crate::protocol_conversions::session_to_dto(session)
                            .unwrap_or_else(|e| {
                                tracing::error!(error = %e, "session_to_dto conversion failed");
                                Default::default()
                            }),
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
                        session: crate::protocol_conversions::session_to_dto(session)
                            .unwrap_or_else(|e| {
                                tracing::error!(error = %e, "session_to_dto conversion failed");
                                Default::default()
                            }),
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
                                    let created = Self::session_dto(session, Some(&context));
                                    // M003: capture originating-principal
                                    // attribution for the new session.
                                    self.record_origin_with_decision(
                                        &authority,
                                        &authz_decision,
                                        "session",
                                        created.id.as_str(),
                                    )
                                    .await;
                                    // M005: structural session-create event
                                    // with the durable session id.
                                    {
                                        let provenance =
                                            codegg_core::authorization::audit_provenance(
                                                &authz_decision,
                                            );
                                        let mut chain =
                                            codegg_core::audit_instrumentation::AuditChainContext::new(
                                            );
                                        chain.project = Some(context.project_id.clone());
                                        chain.session_id = Some(created.id.clone());
                                        let builder =
                                            codegg_core::audit_instrumentation::session_create_event(
                                                authority.principal(),
                                                &provenance,
                                                &chain,
                                                created.id.as_str(),
                                                "allow",
                                            );
                                        self.append_audit_event(builder).await;
                                    }
                                    Ok(CoreResponse::Session { session: created })
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
                let template = match crate::protocol_conversions::dto_to_session_template(template)
                {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::error!(error = %e, "dto_to_session_template conversion failed");
                        return Ok(CoreResponse::Error {
                            code: "template_conversion_failed".to_string(),
                            message: e.to_string(),
                        });
                    }
                };
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
                                    let created = Self::session_dto(session, Some(&context));
                                    // M003: capture originating-principal
                                    // attribution for the new session.
                                    self.record_origin_with_decision(
                                        &authority,
                                        &authz_decision,
                                        "session",
                                        created.id.as_str(),
                                    )
                                    .await;
                                    // M005: structural session-create event
                                    // with the durable session id.
                                    {
                                        let provenance =
                                            codegg_core::authorization::audit_provenance(
                                                &authz_decision,
                                            );
                                        let mut chain =
                                            codegg_core::audit_instrumentation::AuditChainContext::new(
                                            );
                                        chain.project = Some(context.project_id.clone());
                                        chain.session_id = Some(created.id.clone());
                                        let builder =
                                            codegg_core::audit_instrumentation::session_create_event(
                                                authority.principal(),
                                                &provenance,
                                                &chain,
                                                created.id.as_str(),
                                                "allow",
                                            );
                                        self.append_audit_event(builder).await;
                                    }
                                    Ok(CoreResponse::Session { session: created })
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
