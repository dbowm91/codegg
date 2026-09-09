//! M002: `assets` request family for `CoreDaemon`.
//!
//! Runtime-asset refresh/status over the daemon-owned AssetRefreshCoordinator.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;

use codegg_core::context::ProjectContextRequest;

impl CoreDaemon {
    pub(crate) async fn handle_assets_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (request_id, trusted_client_id, &authority, &authz_decision);
        match request {
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
                self.event_log
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
