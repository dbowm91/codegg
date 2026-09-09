//! M002: `providers` request family for `CoreDaemon`.
//!
//! Eggpool provisioning and provider-connection lifecycle over the daemon-owned provisioner.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;
use super::daemon::{
    connection_detail_dto, connection_lifecycle_response, eggpool_error_code,
    eggpool_error_message, purge_blocker_dto,
};

impl CoreDaemon {
    pub(crate) async fn handle_providers_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (request_id, trusted_client_id, &authority, &authz_decision);
        match request {
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
                let handle = match codegg_protocol::provider::SecretInputRef::new(format!(
                    "rot-secret-{}",
                    uuid::Uuid::new_v4()
                )) {
                    Ok(handle) => handle,
                    Err(reason) => {
                        return Ok(CoreResponse::Error {
                            code: "connection_rotation_secret_rejected".to_string(),
                            message: format!("Generated rotation secret handle rejected: {reason}"),
                        });
                    }
                };
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
                        self.event_log
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
