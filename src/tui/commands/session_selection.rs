//! Provider Connections Milestone 3: TUI command runner for session
//! connection/model selection.
//!
//! The runner reads from the daemon through the typed selection service
//! (`CoreRequest::SessionSelectionGet` / `List` / `Models` / `Update`).
//! It never constructs a provider, never stores a credential, and never
//! resolves a secret locally. The TUI receives redacted DTOs only.

use std::sync::Arc;

use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::provider::{SelectedModelDto, UpdateSessionSelectionRequest};

use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Spawn a refresh of the current session's connection/model selection.
/// Calls `SessionSelectionGet`, then `SessionSelectionList`, then (if a
/// connection is selected) `SessionSelectionModels`, and finally posts a
/// `SessionSelectionLoaded` command back to the TUI event loop.
pub(crate) fn start_selection_refresh(app: &mut App, session_id: String) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "session_selection_refresh",
        async move {
            let Some(client) = core_client else {
                return Some(TuiCommand::SessionSelectionLoaded {
                    session_id,
                    selection: None,
                    connections: Vec::new(),
                    models: Vec::new(),
                    focused_connection_id: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };

            // 1. Current selection (optional).
            let selection = match client
                .request(envelope(CoreRequest::SessionSelectionGet {
                    session_id: session_id.clone(),
                }))
                .await
            {
                Ok(CoreResponse::SessionSelection { selection, .. }) => Some(selection),
                Ok(CoreResponse::Error { code, message }) => {
                    return Some(TuiCommand::SessionSelectionLoaded {
                        session_id,
                        selection: None,
                        connections: Vec::new(),
                        models: Vec::new(),
                        focused_connection_id: None,
                        error: Some(format!("Selection get failed ({code}): {message}")),
                    });
                }
                Ok(_) => {
                    return Some(TuiCommand::SessionSelectionLoaded {
                        session_id,
                        selection: None,
                        connections: Vec::new(),
                        models: Vec::new(),
                        focused_connection_id: None,
                        error: Some("Unexpected selection response".to_string()),
                    });
                }
                Err(e) => {
                    return Some(TuiCommand::SessionSelectionLoaded {
                        session_id,
                        selection: None,
                        connections: Vec::new(),
                        models: Vec::new(),
                        focused_connection_id: None,
                        error: Some(format!("Selection request failed: {e}")),
                    });
                }
            };

            // 2. Connections eligible for selection.
            let connections = match client
                .request(envelope(CoreRequest::SessionSelectionList {
                    session_id: session_id.clone(),
                }))
                .await
            {
                Ok(CoreResponse::ProviderConnections { connections }) => connections,
                Ok(CoreResponse::Error { code, message }) => {
                    return Some(TuiCommand::SessionSelectionLoaded {
                        session_id,
                        selection,
                        connections: Vec::new(),
                        models: Vec::new(),
                        focused_connection_id: None,
                        error: Some(format!("Connection list failed ({code}): {message}")),
                    });
                }
                Ok(_) => {
                    return Some(TuiCommand::SessionSelectionLoaded {
                        session_id,
                        selection,
                        connections: Vec::new(),
                        models: Vec::new(),
                        focused_connection_id: None,
                        error: Some("Unexpected connection list response".to_string()),
                    });
                }
                Err(e) => {
                    return Some(TuiCommand::SessionSelectionLoaded {
                        session_id,
                        selection,
                        connections: Vec::new(),
                        models: Vec::new(),
                        focused_connection_id: None,
                        error: Some(format!("Connection list failed: {e}")),
                    });
                }
            };

            // 3. If a connection is already selected, fetch its catalog.
            let (focused_connection_id, models) = match selection.as_ref() {
                Some(codegg_protocol::provider::SessionSelectionDto::Selected {
                    connection,
                    ..
                }) => {
                    let cid = connection.id.clone();
                    match fetch_models(&client, &session_id, &cid).await {
                        Ok(models) => (Some(cid), models),
                        Err(err) => {
                            return Some(TuiCommand::SessionSelectionLoaded {
                                session_id,
                                selection,
                                connections,
                                models: Vec::new(),
                                focused_connection_id: Some(cid),
                                error: Some(err),
                            });
                        }
                    }
                }
                _ => (None, Vec::new()),
            };

            Some(TuiCommand::SessionSelectionLoaded {
                session_id,
                selection,
                connections,
                models,
                focused_connection_id,
                error: None,
            })
        },
    );
}

async fn fetch_models(
    client: &Arc<dyn crate::core::CoreClient>,
    session_id: &str,
    connection_id: &str,
) -> Result<Vec<SelectedModelDto>, String> {
    let response = client
        .request(envelope(CoreRequest::SessionSelectionModels {
            session_id: session_id.to_string(),
            connection_id: connection_id.to_string(),
        }))
        .await
        .map_err(|e| e.to_string())?;
    match response {
        CoreResponse::ProviderConnectionModels { models, .. } => Ok(models
            .into_iter()
            .map(|m| SelectedModelDto {
                connection_id: connection_id.to_string(),
                model_id: m.id,
                model_name: m.name,
                context_window: m.context_window,
                max_output_tokens: m.max_output_tokens,
                supports_tools: m.supports_tools,
                supports_vision: m.supports_vision,
                catalog_revision: "0".to_string(),
            })
            .collect()),
        CoreResponse::Error { code, message } => Err(format!("{code}: {message}")),
        _ => Err("Unexpected models response".to_string()),
    }
}

/// Spawn a selection-update request to the daemon. The TUI never
/// computes or persists a credential; the daemon owns that path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn start_selection_update(
    app: &mut App,
    session_id: String,
    connection_id: String,
    model_id: String,
    expected_connection_revision: Option<u64>,
    expected_catalog_revision: Option<String>,
) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "session_selection_update",
        async move {
            let Some(client) = core_client else {
                return Some(TuiCommand::SessionSelectionLoaded {
                    session_id,
                    selection: None,
                    connections: Vec::new(),
                    models: Vec::new(),
                    focused_connection_id: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = envelope(CoreRequest::SessionSelectionUpdate {
                request: Box::new(UpdateSessionSelectionRequest {
                    session_id: session_id.clone(),
                    connection_id,
                    model_id,
                    expected_connection_revision,
                    expected_catalog_revision,
                }),
            });
            match client.request(request).await {
                Ok(CoreResponse::SessionSelectionUpdated { selection, .. }) => {
                    Some(TuiCommand::SessionSelectionLoaded {
                        session_id,
                        selection: Some(selection),
                        connections: Vec::new(),
                        models: Vec::new(),
                        focused_connection_id: None,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::SessionSelectionLoaded {
                        session_id,
                        selection: None,
                        connections: Vec::new(),
                        models: Vec::new(),
                        focused_connection_id: None,
                        error: Some(format!("Selection update failed ({code}): {message}")),
                    })
                }
                Ok(_) => Some(TuiCommand::SessionSelectionLoaded {
                    session_id,
                    selection: None,
                    connections: Vec::new(),
                    models: Vec::new(),
                    focused_connection_id: None,
                    error: Some("Unexpected update response".to_string()),
                }),
                Err(e) => Some(TuiCommand::SessionSelectionLoaded {
                    session_id,
                    selection: None,
                    connections: Vec::new(),
                    models: Vec::new(),
                    focused_connection_id: None,
                    error: Some(format!("Selection update failed: {e}")),
                }),
            }
        },
    );
}

fn envelope(payload: CoreRequest) -> RequestEnvelope<CoreRequest> {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        payload,
    }
}
