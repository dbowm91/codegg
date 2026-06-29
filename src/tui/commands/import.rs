//! Import dialog command handlers.
//!
//! Functions for previewing and confirming session imports from session IDs or file paths.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::components::dialogs::import::ImportSource;
use crate::tui::task_lifecycle::TuiTaskKind;

#[allow(dead_code)]
pub(crate) async fn handle_preview_import(app: &mut App, source: ImportSource) {
    if let Some(core_client) = app.core_client.clone() {
        match source {
            ImportSource::SessionId(id) => {
                let load_request = crate::core::new_request(
                    format!("session-load-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SessionLoad {
                        session_id: id.clone(),
                    },
                );
                let count_request = crate::core::new_request(
                    format!("session-message-counts-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SessionMessageCounts {
                        session_ids: vec![id.clone()],
                    },
                );
                match (
                    core_client.request(load_request).await,
                    core_client.request(count_request).await,
                ) {
                    (
                        Ok(CoreResponse::Session { session }),
                        Ok(CoreResponse::SessionMessageCounts { counts }),
                    ) => {
                        let msg_count = counts.get(&id).copied().unwrap_or(0);
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_preview(
                                crate::protocol_conversions::dto_to_session(session),
                                msg_count,
                            );
                        }
                    }
                    (Ok(CoreResponse::Error { message, .. }), _)
                    | (_, Ok(CoreResponse::Error { message, .. })) => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_error(format!("Failed to load session: {}", message));
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_error(format!("Failed to load session: {}", e));
                        }
                    }
                    _ => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import
                                .set_error("Unexpected response while loading session".to_string());
                        }
                    }
                }
            }
            ImportSource::FilePath(path) => match tokio::fs::read_to_string(path.as_str()).await {
                Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(data) => {
                        let import_request = crate::core::new_request(
                            format!("session-import-data-{}", uuid::Uuid::new_v4()),
                            CoreRequest::SessionImportData { data },
                        );
                        match core_client.request(import_request).await {
                            Ok(CoreResponse::Session { session }) => {
                                let count_request = crate::core::new_request(
                                    format!("session-message-counts-{}", uuid::Uuid::new_v4()),
                                    CoreRequest::SessionMessageCounts {
                                        session_ids: vec![session.id.clone()],
                                    },
                                );
                                let msg_count = match core_client.request(count_request).await {
                                    Ok(CoreResponse::SessionMessageCounts { counts }) => {
                                        counts.get(&session.id).copied().unwrap_or(0)
                                    }
                                    _ => 0,
                                };
                                if let Some(ref mut import) = app.dialog_state.import_dialog {
                                    import.set_preview(
                                        crate::protocol_conversions::dto_to_session(session),
                                        msg_count,
                                    );
                                }
                            }
                            Ok(CoreResponse::Error { message, .. }) => {
                                if let Some(ref mut import) = app.dialog_state.import_dialog {
                                    import.set_error(format!("Import failed: {}", message));
                                }
                            }
                            Ok(_other) => {
                                if let Some(ref mut import) = app.dialog_state.import_dialog {
                                    import.set_error("Unexpected import response".to_string());
                                }
                            }
                            Err(e) => {
                                if let Some(ref mut import) = app.dialog_state.import_dialog {
                                    import.set_error(format!("Import failed: {}", e));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_error(format!("Invalid JSON: {}", e));
                        }
                    }
                },
                Err(e) => {
                    if let Some(ref mut import) = app.dialog_state.import_dialog {
                        import.set_error(format!("Failed to read file: {}", e));
                    }
                }
            },
        }
        return;
    }

    if let Some(ref mut import) = app.dialog_state.import_dialog {
        import.set_error("Core unavailable — check daemon status with /doctor".to_string());
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_confirm_import(app: &mut App, source: ImportSource) {
    if let Some(core_client) = app.core_client.clone() {
        match source {
            ImportSource::SessionId(id) => {
                let request = crate::core::new_request(
                    format!("session-fork-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SessionFork { session_id: id },
                );
                match core_client.request(request).await {
                    Ok(CoreResponse::Session { session }) => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_done(crate::protocol_conversions::dto_to_session(session));
                        }
                    }
                    Ok(CoreResponse::Error { message, .. }) => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_error(format!("Import failed: {}", message));
                        }
                    }
                    Ok(_other) => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_error("Unexpected import response".to_string());
                        }
                    }
                    Err(e) => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_error(format!("Import failed: {}", e));
                        }
                    }
                }
            }
            ImportSource::FilePath(_) => {
                if let Some(ref mut import) = app.dialog_state.import_dialog {
                    import.set_error("File already imported via preview".to_string());
                }
            }
        }
        return;
    }

    if let Some(ref mut import) = app.dialog_state.import_dialog {
        import.set_error("Core unavailable — check daemon status with /doctor".to_string());
    }
}

pub(crate) fn start_preview_import(app: &mut App, source: ImportSource) {
    let request_id = app.dialog_state.import_request.begin();

    if let Some(ref mut import) = app.dialog_state.import_dialog {
        import.set_error("Loading preview...".to_string());
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "preview_import",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ImportPreviewLoaded {
                    request_id,
                    session: None,
                    msg_count: 0,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };

            match source {
                ImportSource::SessionId(id) => {
                    let load_request = crate::core::new_request(
                        format!("session-load-{}", uuid::Uuid::new_v4()),
                        CoreRequest::SessionLoad {
                            session_id: id.clone(),
                        },
                    );
                    let count_request = crate::core::new_request(
                        format!("session-message-counts-{}", uuid::Uuid::new_v4()),
                        CoreRequest::SessionMessageCounts {
                            session_ids: vec![id.clone()],
                        },
                    );
                    match (
                        core_client.request(load_request).await,
                        core_client.request(count_request).await,
                    ) {
                        (
                            Ok(CoreResponse::Session { session }),
                            Ok(CoreResponse::SessionMessageCounts { counts }),
                        ) => {
                            let msg_count = counts.get(&id).copied().unwrap_or(0);
                            Some(TuiCommand::ImportPreviewLoaded {
                                request_id,
                                session: Some(crate::protocol_conversions::dto_to_session(session)),
                                msg_count,
                                error: None,
                            })
                        }
                        (Ok(CoreResponse::Error { message, .. }), _)
                        | (_, Ok(CoreResponse::Error { message, .. })) => {
                            Some(TuiCommand::ImportPreviewLoaded {
                                request_id,
                                session: None,
                                msg_count: 0,
                                error: Some(format!("Failed to load session: {}", message)),
                            })
                        }
                        (Err(e), _) | (_, Err(e)) => Some(TuiCommand::ImportPreviewLoaded {
                            request_id,
                            session: None,
                            msg_count: 0,
                            error: Some(format!("Failed to load session: {}", e)),
                        }),
                        _ => Some(TuiCommand::ImportPreviewLoaded {
                            request_id,
                            session: None,
                            msg_count: 0,
                            error: Some("Unexpected response while loading session".to_string()),
                        }),
                    }
                }
                ImportSource::FilePath(path) => {
                    match tokio::fs::read_to_string(path.as_str()).await {
                        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                            Ok(data) => {
                                let import_request = crate::core::new_request(
                                    format!("session-import-data-{}", uuid::Uuid::new_v4()),
                                    CoreRequest::SessionImportData { data },
                                );
                                match core_client.request(import_request).await {
                                    Ok(CoreResponse::Session { session }) => {
                                        let count_request = crate::core::new_request(
                                            format!(
                                                "session-message-counts-{}",
                                                uuid::Uuid::new_v4()
                                            ),
                                            CoreRequest::SessionMessageCounts {
                                                session_ids: vec![session.id.clone()],
                                            },
                                        );
                                        let msg_count =
                                            match core_client.request(count_request).await {
                                                Ok(CoreResponse::SessionMessageCounts {
                                                    counts,
                                                }) => counts.get(&session.id).copied().unwrap_or(0),
                                                _ => 0,
                                            };
                                        Some(TuiCommand::ImportPreviewLoaded {
                                            request_id,
                                            session: Some(
                                                crate::protocol_conversions::dto_to_session(
                                                    session,
                                                ),
                                            ),
                                            msg_count,
                                            error: None,
                                        })
                                    }
                                    Ok(CoreResponse::Error { message, .. }) => {
                                        Some(TuiCommand::ImportPreviewLoaded {
                                            request_id,
                                            session: None,
                                            msg_count: 0,
                                            error: Some(format!("Import failed: {}", message)),
                                        })
                                    }
                                    Ok(_other) => Some(TuiCommand::ImportPreviewLoaded {
                                        request_id,
                                        session: None,
                                        msg_count: 0,
                                        error: Some("Unexpected import response".to_string()),
                                    }),
                                    Err(e) => Some(TuiCommand::ImportPreviewLoaded {
                                        request_id,
                                        session: None,
                                        msg_count: 0,
                                        error: Some(format!("Import failed: {}", e)),
                                    }),
                                }
                            }
                            Err(e) => Some(TuiCommand::ImportPreviewLoaded {
                                request_id,
                                session: None,
                                msg_count: 0,
                                error: Some(format!("Invalid JSON: {}", e)),
                            }),
                        },
                        Err(e) => Some(TuiCommand::ImportPreviewLoaded {
                            request_id,
                            session: None,
                            msg_count: 0,
                            error: Some(format!("Failed to read file: {}", e)),
                        }),
                    }
                }
            }
        },
    );
}

pub(crate) fn apply_import_preview_loaded(
    app: &mut App,
    request_id: u64,
    session: Option<crate::session::Session>,
    msg_count: usize,
    error: Option<String>,
) {
    if !app.dialog_state.import_request.is_current(request_id) {
        return;
    }
    if let Some(ref mut import) = app.dialog_state.import_dialog {
        if let Some(err) = error {
            import.set_error(err);
        } else if let Some(session) = session {
            import.set_preview(session, msg_count);
        }
    }
}

pub(crate) fn start_confirm_import(app: &mut App, source: ImportSource) {
    let request_id = app.dialog_state.import_request.begin();

    if let Some(ref mut import) = app.dialog_state.import_dialog {
        import.set_error("Importing...".to_string());
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "confirm_import",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ImportConfirmed {
                    request_id,
                    session: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };

            match source {
                ImportSource::SessionId(id) => {
                    let request = crate::core::new_request(
                        format!("session-fork-{}", uuid::Uuid::new_v4()),
                        CoreRequest::SessionFork { session_id: id },
                    );
                    match core_client.request(request).await {
                        Ok(CoreResponse::Session { session }) => {
                            Some(TuiCommand::ImportConfirmed {
                                request_id,
                                session: Some(crate::protocol_conversions::dto_to_session(session)),
                                error: None,
                            })
                        }
                        Ok(CoreResponse::Error { message, .. }) => {
                            Some(TuiCommand::ImportConfirmed {
                                request_id,
                                session: None,
                                error: Some(format!("Import failed: {}", message)),
                            })
                        }
                        Ok(_other) => Some(TuiCommand::ImportConfirmed {
                            request_id,
                            session: None,
                            error: Some("Unexpected import response".to_string()),
                        }),
                        Err(e) => Some(TuiCommand::ImportConfirmed {
                            request_id,
                            session: None,
                            error: Some(format!("Import failed: {}", e)),
                        }),
                    }
                }
                ImportSource::FilePath(_) => Some(TuiCommand::ImportConfirmed {
                    request_id,
                    session: None,
                    error: Some("File already imported via preview".to_string()),
                }),
            }
        },
    );
}

pub(crate) fn apply_import_confirmed(
    app: &mut App,
    request_id: u64,
    session: Option<crate::session::Session>,
    error: Option<String>,
) {
    if !app.dialog_state.import_request.is_current(request_id) {
        return;
    }
    if let Some(ref mut import) = app.dialog_state.import_dialog {
        if let Some(err) = error {
            import.set_error(err);
        } else if let Some(session) = session {
            import.set_done(session);
        }
    }
}
