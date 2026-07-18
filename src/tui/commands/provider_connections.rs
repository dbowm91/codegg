//! TUI lifecycle commands for daemon-owned provider connections.

use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};

use crate::tui::app::{App, ConnectionLifecycleAction, TuiCommand};
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

pub(crate) fn start_connection_lifecycle(
    app: &mut App,
    action: ConnectionLifecycleAction,
    connection_id: String,
    expected_revision: u64,
) {
    let client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "provider_connection_lifecycle",
        async move {
            let Some(client) = client else {
                return Some(TuiCommand::ConnectionLifecycleFinished {
                    action,
                    connection_id,
                    message: None,
                    error: Some("Daemon connection is unavailable".to_string()),
                });
            };
            let request = match action {
                ConnectionLifecycleAction::Refresh => CoreRequest::ConnectionRefreshBegin {
                    connection_id: connection_id.clone(),
                    expected_revision,
                },
                ConnectionLifecycleAction::Enable => CoreRequest::ConnectionEnable {
                    connection_id: connection_id.clone(),
                    expected_revision,
                    require_probe: false,
                },
                ConnectionLifecycleAction::Disable => CoreRequest::ConnectionDisable {
                    connection_id: connection_id.clone(),
                    expected_revision,
                },
                ConnectionLifecycleAction::Delete => CoreRequest::ConnectionDelete {
                    connection_id: connection_id.clone(),
                    expected_revision,
                },
                ConnectionLifecycleAction::Restore => CoreRequest::ConnectionRestore {
                    connection_id: connection_id.clone(),
                    expected_revision,
                },
                ConnectionLifecycleAction::Purge => CoreRequest::ConnectionPurge {
                    connection_id: connection_id.clone(),
                    expected_revision,
                },
            };
            let response = client
                .request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: request,
                })
                .await;
            match response {
                Ok(CoreResponse::Ack) => Some(TuiCommand::ConnectionLifecycleFinished {
                    action,
                    connection_id,
                    message: Some(format!("Provider connection {action:?} completed")),
                    error: None,
                }),
                Ok(CoreResponse::ConnectionRefreshResult { result }) => {
                    if let Some(error) = result.error_code {
                        Some(TuiCommand::ConnectionLifecycleFinished {
                            action,
                            connection_id,
                            message: None,
                            error: Some(error),
                        })
                    } else {
                        Some(TuiCommand::ConnectionLifecycleFinished {
                            action,
                            connection_id,
                            message: Some(format!(
                                "Catalog refreshed at revision {}",
                                result.revision.unwrap_or(expected_revision)
                            )),
                            error: None,
                        })
                    }
                }
                Ok(CoreResponse::ConnectionPurge { outcome }) => {
                    Some(TuiCommand::ConnectionLifecycleFinished {
                        action,
                        connection_id,
                        message: Some(format!("Purge result: {outcome:?}")),
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::ConnectionLifecycleFinished {
                        action,
                        connection_id,
                        message: None,
                        error: Some(format!("{code}: {message}")),
                    })
                }
                Ok(other) => Some(TuiCommand::ConnectionLifecycleFinished {
                    action,
                    connection_id,
                    message: None,
                    error: Some(format!("Unexpected response: {other:?}")),
                }),
                Err(error) => Some(TuiCommand::ConnectionLifecycleFinished {
                    action,
                    connection_id,
                    message: None,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
}
