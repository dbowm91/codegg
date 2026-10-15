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
pub(crate) fn start_selection_refresh(
    app: &mut App,
    session_id: String,
    focus_connection_id: Option<String>,
) {
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

            // 3. Resolve which connection's catalog to show, then fetch it.
            //
            // The model pane is the only way to bind a session to a
            // connection, so it has to populate when nothing is bound yet.
            // Deriving the focus only from an existing selection made an
            // unbound session show a permanently empty model list — with no
            // model to pick, there was no way to bind one, which is the
            // deadlock this dialog is meant to resolve. Prefer an active
            // connection when the session is unbound, since a disabled row
            // cannot be selected anyway.
            let focused_connection_id = focus_connection_id
                .filter(|focused| connections.iter().any(|c| &c.id == focused))
                .or_else(|| match selection.as_ref() {
                    Some(codegg_protocol::provider::SessionSelectionDto::Selected {
                        connection,
                        ..
                    }) => Some(connection.id.clone()),
                    _ => connections
                        .iter()
                        .find(|connection| connection.state == "active")
                        .or_else(|| connections.first())
                        .map(|connection| connection.id.clone()),
                });
            let models = match focused_connection_id.as_deref() {
                Some(cid) => match fetch_models(&client, &session_id, cid).await {
                    Ok(models) => models,
                    Err(err) => {
                        return Some(TuiCommand::SessionSelectionLoaded {
                            session_id,
                            selection,
                            connections,
                            models: Vec::new(),
                            focused_connection_id: Some(cid.to_string()),
                            error: Some(err),
                        });
                    }
                },
                None => Vec::new(),
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
        // The daemon's `catalog_revision` is the authority for this catalog:
        // it is the value `update_selection` compares `expected_catalog_revision`
        // against, and it is what the rendered model list must be able to prove
        // it came from. Discarding it for a literal `"0"` made every Enter a
        // guaranteed `selection_catalog_stale` — the dialog could list a
        // connection's models and then never bind one, because the placeholder
        // never equals the store's digest. A refresh or a daemon restart could
        // not help: the wrong value was minted locally, not observed stale.
        CoreResponse::ProviderConnectionModels {
            catalog_revision,
            models,
            ..
        } => {
            let catalog_revision = catalog_revision.unwrap_or_else(|| "0".to_string());
            Ok(models
                .into_iter()
                .map(|m| SelectedModelDto {
                    connection_id: connection_id.to_string(),
                    model_id: m.id,
                    model_name: m.name,
                    context_window: m.context_window,
                    max_output_tokens: m.max_output_tokens,
                    supports_tools: m.supports_tools,
                    supports_vision: m.supports_vision,
                    catalog_revision: catalog_revision.clone(),
                })
                .collect())
        }
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

/// Persist a model chosen in the `/model` dialog to the daemon.
///
/// The daemon resolves every turn from its durable `SessionSelection`,
/// so a choice made in the UI is inert until it is stored there.
/// `ModelSelect` is the compatibility adapter that maps a
/// `provider/model` catalog id onto the explicit connection + model the
/// durable service stores; it performs the same authorization and audit
/// as a `SessionSelectionUpdate`.
pub(crate) fn start_model_select_persist(app: &mut App, session_id: String, model: String) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let Some(core_client) = core_client else {
        // No daemon attached: the choice is still recorded in the tab
        // manifest and applied when a session binds. Nothing to persist.
        return;
    };
    let model_for_reply = model.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "model_select_persist",
        async move {
            let outcome = match core_client
                .request(envelope(CoreRequest::ModelSelect { session_id, model }))
                .await
            {
                Ok(CoreResponse::Ack) | Ok(CoreResponse::ModelsSnapshot { .. }) => None,
                Ok(CoreResponse::Error { message, .. }) => Some(message),
                Ok(other) => Some(format!("unexpected response: {other:?}")),
                Err(e) => Some(format!("selection could not be saved: {e}")),
            };
            Some(TuiCommand::ModelSelectPersisted {
                model: model_for_reply,
                error: outcome,
            })
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CoreClient;
    use crate::error::AppError;
    use codegg_protocol::core::{CoreEvent, EventEnvelope};
    use codegg_protocol::provider::ProviderModelDto;

    /// Replies to `SessionSelectionModels` with one model under a non-zero
    /// catalog revision, and records nothing else.
    struct CatalogDaemon {
        catalog_revision: Option<String>,
    }

    #[async_trait::async_trait]
    impl CoreClient for CatalogDaemon {
        async fn request(
            &self,
            _request: RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, AppError> {
            Ok(CoreResponse::ProviderConnectionModels {
                connection_id: "conn-1".to_string(),
                catalog_revision: self.catalog_revision.clone(),
                models: vec![ProviderModelDto {
                    id: "mimo-v2.6-flash".to_string(),
                    name: "Mimo v2.6 Flash".to_string(),
                    context_window: 200_000,
                    max_output_tokens: Some(8_192),
                    supports_tools: true,
                    supports_vision: false,
                }],
            })
        }

        fn subscribe(&self) -> tokio::sync::mpsc::Receiver<EventEnvelope<CoreEvent>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            rx
        }
    }

    /// The daemon's catalog revision must reach the model rows the dialog
    /// renders, because `pending_update` forwards it verbatim as
    /// `expected_catalog_revision` and `update_selection` refuses the bind
    /// when it disagrees with the store.
    ///
    /// The TUI used to mint a literal `"0"` here instead, which made every
    /// Enter a guaranteed `selection_catalog_stale`: the dialog could list a
    /// connection's models and still never bind one. No refresh and no daemon
    /// restart could change that, because the wrong value was manufactured
    /// locally rather than observed as stale.
    #[tokio::test]
    async fn fetched_models_carry_the_daemons_catalog_revision() {
        let client: Arc<dyn CoreClient> = Arc::new(CatalogDaemon {
            catalog_revision: Some(
                "821b5774ea14568654a16e19fe2b02eae16685db03a5dbda1ebf6bf33ff2b069".to_string(),
            ),
        });
        let models = fetch_models(&client, "sess-1", "conn-1")
            .await
            .expect("model fetch");
        assert_eq!(models.len(), 1);
        assert_eq!(
            models[0].catalog_revision,
            "821b5774ea14568654a16e19fe2b02eae16685db03a5dbda1ebf6bf33ff2b069"
        );
    }

    /// A connection with no catalog yet keeps the `"0"` placeholder rather
    /// than failing the fetch: an empty catalog is a legitimate state, and the
    /// store treats a missing revision and `"0"` alike.
    #[tokio::test]
    async fn a_connection_without_a_catalog_still_reports_a_revision() {
        let client: Arc<dyn CoreClient> = Arc::new(CatalogDaemon {
            catalog_revision: None,
        });
        let models = fetch_models(&client, "sess-1", "conn-1")
            .await
            .expect("model fetch");
        assert_eq!(models[0].catalog_revision, "0");
    }
}
