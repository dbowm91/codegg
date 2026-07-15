//! Session handler functions for the TUI.
//!
//! Contains start/apply async command pairs for session CRUD operations,
//! tree dialog loading, message loading, template creation, share/unshare,
//! and legacy test-only synchronous handlers.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::App;
use crate::tui::app::SessionMutationOp;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Production start/apply pairs
// ---------------------------------------------------------------------------

pub(crate) fn start_reload_sessions(app: &mut App) {
    app.dialog_state.session_dialog.set_loading(true);
    let request_id = app.dialog_state.session_reload_request.begin();

    let core_client = app.core_client.clone();
    let project_id = app.session_state.project_dir.clone();
    let show_archived = app.dialog_state.session_dialog.show_archived;
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "reload_sessions",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionsReloaded {
                    request_id,
                    sessions: Vec::new(),
                    message_counts: std::collections::HashMap::new(),
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };

            let request = crate::core::new_request(
                format!("session-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionList {
                    project_id: project_id.clone(),
                    show_archived,
                    limit: 100,
                },
            );
            let sessions = match core_client.request(request).await {
                Ok(CoreResponse::SessionList { sessions }) => sessions,
                Ok(CoreResponse::Error { code, message }) => {
                    return Some(TuiCommand::SessionsReloaded {
                        request_id,
                        sessions: Vec::new(),
                        message_counts: std::collections::HashMap::new(),
                        error: Some(format!("Session list failed ({}): {}", code, message)),
                    });
                }
                Ok(_other) => {
                    return Some(TuiCommand::SessionsReloaded {
                        request_id,
                        sessions: Vec::new(),
                        message_counts: std::collections::HashMap::new(),
                        error: Some("Unexpected core response".to_string()),
                    });
                }
                Err(e) => {
                    return Some(TuiCommand::SessionsReloaded {
                        request_id,
                        sessions: Vec::new(),
                        message_counts: std::collections::HashMap::new(),
                        error: Some(format!("Session list error: {}", e)),
                    });
                }
            };

            let session_ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
            let message_counts = if session_ids.is_empty() {
                std::collections::HashMap::new()
            } else {
                let count_request = crate::core::new_request(
                    format!("session-message-counts-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SessionMessageCounts {
                        session_ids: session_ids.clone(),
                    },
                );
                match core_client.request(count_request).await {
                    Ok(CoreResponse::SessionMessageCounts { counts }) => counts,
                    _ => std::collections::HashMap::new(),
                }
            };

            Some(TuiCommand::SessionsReloaded {
                request_id,
                sessions,
                message_counts,
                error: None,
            })
        },
    );
}

pub(crate) fn apply_sessions_reloaded(
    app: &mut App,
    request_id: u64,
    sessions: Vec<crate::protocol::dto::Session>,
    message_counts: std::collections::HashMap<String, usize>,
    error: Option<String>,
) {
    if let Some(err) = error {
        if !app
            .dialog_state
            .session_reload_request
            .fail(request_id, err.clone())
        {
            return;
        }
        app.dialog_state.session_dialog.set_loading(false);
        app.messages_state.toasts.error(&err);
        return;
    }
    if !app.dialog_state.session_reload_request.finish(request_id) {
        return;
    }
    app.dialog_state.session_dialog.set_loading(false);

    app.dialog_state
        .session_dialog
        .load_sessions(crate::protocol_conversions::dtos_to_sessions(sessions));
    for (id, count) in message_counts {
        app.dialog_state
            .session_dialog
            .set_message_count(&id, count);
    }
}

pub(crate) fn next_session_mutation_id(app: &mut App) -> u64 {
    app.dialog_state.session_mutation_request.begin()
}

pub(crate) fn apply_session_mutation_finished(
    app: &mut App,
    request_id: u64,
    op: SessionMutationOp,
    _affected_ids: Vec<String>,
    message: String,
    reload_after: bool,
    error: Option<String>,
) {
    if let Some(err) = error {
        if !app
            .dialog_state
            .session_mutation_request
            .fail(request_id, err.clone())
        {
            return;
        }
        app.messages_state.toasts.error(&err);
        return;
    }

    if !app.dialog_state.session_mutation_request.finish(request_id) {
        return;
    }
    if !message.is_empty() {
        app.messages_state.toasts.info(&message);
    }
    if op == SessionMutationOp::UndoDelete {
        app.undo_session_id = None;
        app.undo_until = None;
    }
    if reload_after {
        start_reload_sessions(app);
    }
}

pub(crate) fn start_delete_session(app: &mut App, session_id: String) {
    let request_id = next_session_mutation_id(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "delete_session",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Delete,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("session-delete-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionDelete {
                    session_id: session_id.clone(),
                    permanent: false,
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Ack) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Delete,
                    affected_ids: vec![session_id],
                    message: "Session deleted".to_string(),
                    reload_after: true,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::SessionMutationFinished {
                        request_id,
                        op: SessionMutationOp::Delete,
                        affected_ids: vec![session_id],
                        message: String::new(),
                        reload_after: false,
                        error: Some(format!("Session delete failed ({}): {}", code, message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Delete,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Unexpected core response".to_string()),
                }),
                Err(e) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Delete,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some(format!("Session delete error: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn start_archive_session(app: &mut App, session_id: String, unarchive: bool) {
    let request_id = next_session_mutation_id(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "archive_session",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: if unarchive {
                        SessionMutationOp::Unarchive
                    } else {
                        SessionMutationOp::Archive
                    },
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("session-archive-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionArchive {
                    session_id: session_id.clone(),
                    unarchive,
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Ack) => {
                    let msg = if unarchive {
                        "Session unarchived"
                    } else {
                        "Session archived"
                    };
                    Some(TuiCommand::SessionMutationFinished {
                        request_id,
                        op: if unarchive {
                            SessionMutationOp::Unarchive
                        } else {
                            SessionMutationOp::Archive
                        },
                        affected_ids: vec![session_id],
                        message: msg.to_string(),
                        reload_after: true,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::SessionMutationFinished {
                        request_id,
                        op: if unarchive {
                            SessionMutationOp::Unarchive
                        } else {
                            SessionMutationOp::Archive
                        },
                        affected_ids: vec![session_id],
                        message: String::new(),
                        reload_after: false,
                        error: Some(format!("Session archive failed ({}): {}", code, message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: if unarchive {
                        SessionMutationOp::Unarchive
                    } else {
                        SessionMutationOp::Archive
                    },
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Unexpected core response".to_string()),
                }),
                Err(e) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: if unarchive {
                        SessionMutationOp::Unarchive
                    } else {
                        SessionMutationOp::Archive
                    },
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some(format!("Session archive error: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn start_fork_session(app: &mut App, session_id: String) {
    let request_id = next_session_mutation_id(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "fork_session",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Fork,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("session-fork-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionFork {
                    session_id: session_id.clone(),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Ack) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Fork,
                    affected_ids: vec![session_id],
                    message: "Session forked".to_string(),
                    reload_after: true,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::SessionMutationFinished {
                        request_id,
                        op: SessionMutationOp::Fork,
                        affected_ids: vec![session_id],
                        message: String::new(),
                        reload_after: false,
                        error: Some(format!("Session fork failed ({}): {}", code, message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Fork,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Unexpected core response".to_string()),
                }),
                Err(e) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Fork,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some(format!("Session fork error: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn start_bulk_delete(app: &mut App, session_ids: Vec<String>) {
    let request_id = next_session_mutation_id(app);
    let count = session_ids.len();
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "bulk_delete",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::BulkDelete,
                    affected_ids: session_ids,
                    message: String::new(),
                    reload_after: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let mut succeeded = 0usize;
            let mut failed = 0usize;
            for id in &session_ids {
                let request = crate::core::new_request(
                    format!("session-delete-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SessionDelete {
                        session_id: id.clone(),
                        permanent: true,
                    },
                );
                match core_client.request(request).await {
                    Ok(CoreResponse::Ack) => succeeded += 1,
                    Ok(CoreResponse::Error { code, message }) => {
                        tracing::warn!(
                            "failed to permanently delete session {} via core ({}): {}",
                            id,
                            code,
                            message
                        );
                        failed += 1;
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "unexpected core response for permanent session delete {}: {:?}",
                            id,
                            other
                        );
                        failed += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "failed to permanently delete session {} via core: {}",
                            id,
                            e
                        );
                        failed += 1;
                    }
                }
            }
            let (message, error) = if failed == 0 {
                (format!("{} sessions deleted", count), None)
            } else if succeeded == 0 {
                (
                    String::new(),
                    Some(format!("Failed to delete {} sessions", count)),
                )
            } else {
                (format!("Deleted {}/{} sessions", succeeded, count), None)
            };
            Some(TuiCommand::SessionMutationFinished {
                request_id,
                op: SessionMutationOp::BulkDelete,
                affected_ids: session_ids,
                message,
                reload_after: true,
                error,
            })
        },
    );
}

pub(crate) fn start_bulk_archive(app: &mut App, session_ids: Vec<String>, unarchive: bool) {
    let request_id = next_session_mutation_id(app);
    let count = session_ids.len();
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "bulk_archive",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: if unarchive {
                        SessionMutationOp::Unarchive
                    } else {
                        SessionMutationOp::BulkArchive
                    },
                    affected_ids: session_ids,
                    message: String::new(),
                    reload_after: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let mut succeeded = 0usize;
            let mut failed = 0usize;
            for id in &session_ids {
                let request = crate::core::new_request(
                    format!("session-archive-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SessionArchive {
                        session_id: id.clone(),
                        unarchive,
                    },
                );
                match core_client.request(request).await {
                    Ok(CoreResponse::Ack) => succeeded += 1,
                    Ok(CoreResponse::Error { code, message }) => {
                        tracing::warn!(
                            "failed to archive session {} via core ({}): {}",
                            id,
                            code,
                            message
                        );
                        failed += 1;
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "unexpected core response for session archive {}: {:?}",
                            id,
                            other
                        );
                        failed += 1;
                    }
                    Err(e) => {
                        tracing::warn!("failed to archive session {} via core: {}", id, e);
                        failed += 1;
                    }
                }
            }
            let op_name = if unarchive { "unarchived" } else { "archived" };
            let (message, error) = if failed == 0 {
                (format!("{} sessions {}", count, op_name), None)
            } else if succeeded == 0 {
                (
                    String::new(),
                    Some(format!("Failed to {} {} sessions", op_name, count)),
                )
            } else {
                (
                    format!("{} {}/{} sessions", op_name, succeeded, count),
                    None,
                )
            };
            Some(TuiCommand::SessionMutationFinished {
                request_id,
                op: if unarchive {
                    SessionMutationOp::Unarchive
                } else {
                    SessionMutationOp::BulkArchive
                },
                affected_ids: session_ids,
                message,
                reload_after: true,
                error,
            })
        },
    );
}

pub(crate) fn start_bulk_export(app: &mut App, session_ids: Vec<String>) {
    let request_id = next_session_mutation_id(app);
    let count = session_ids.len();
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "bulk_export",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::BulkExport,
                    affected_ids: session_ids,
                    message: String::new(),
                    reload_after: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let mut succeeded = 0usize;
            let mut failed = 0usize;
            for id in &session_ids {
                let request = crate::core::new_request(
                    format!("session-export-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SessionExport {
                        session_id: id.clone(),
                    },
                );
                match core_client.request(request).await {
                    Ok(CoreResponse::Json { .. }) => {
                        tracing::info!("exported session {}", id);
                        succeeded += 1;
                    }
                    Ok(CoreResponse::Error { code, message }) => {
                        tracing::warn!(
                            "failed to export session {} via core ({}): {}",
                            id,
                            code,
                            message
                        );
                        failed += 1;
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "unexpected core response for session export {}: {:?}",
                            id,
                            other
                        );
                        failed += 1;
                    }
                    Err(e) => {
                        tracing::warn!("failed to export session {} via core: {}", id, e);
                        failed += 1;
                    }
                }
            }
            let (message, error) = if failed == 0 {
                (format!("{} sessions exported", count), None)
            } else if succeeded == 0 {
                (
                    String::new(),
                    Some(format!("Failed to export {} sessions", count)),
                )
            } else {
                (format!("Exported {}/{} sessions", succeeded, count), None)
            };
            Some(TuiCommand::SessionMutationFinished {
                request_id,
                op: SessionMutationOp::BulkExport,
                affected_ids: session_ids,
                message,
                reload_after: false,
                error,
            })
        },
    );
}

pub(crate) fn start_rename_session(app: &mut App, session_id: String, new_title: String) {
    let request_id = next_session_mutation_id(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "rename_session",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Rename,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("session-rename-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionRename {
                    session_id: session_id.clone(),
                    new_title,
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Session { .. }) | Ok(CoreResponse::Ack) => {
                    Some(TuiCommand::SessionMutationFinished {
                        request_id,
                        op: SessionMutationOp::Rename,
                        affected_ids: vec![session_id],
                        message: "Session renamed".to_string(),
                        reload_after: true,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::SessionMutationFinished {
                        request_id,
                        op: SessionMutationOp::Rename,
                        affected_ids: vec![session_id],
                        message: String::new(),
                        reload_after: false,
                        error: Some(format!("Failed to rename: {}", message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Rename,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Unexpected rename response".to_string()),
                }),
                Err(e) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::Rename,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some(format!("Failed to rename: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn start_undo_delete(app: &mut App, session_id: String) {
    let request_id = next_session_mutation_id(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "undo_delete",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::UndoDelete,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("session-restore-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionRestore {
                    session_id: session_id.clone(),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Session { .. }) | Ok(CoreResponse::Ack) => {
                    Some(TuiCommand::SessionMutationFinished {
                        request_id,
                        op: SessionMutationOp::UndoDelete,
                        affected_ids: vec![session_id],
                        message: "Session restored successfully".to_string(),
                        reload_after: true,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::SessionMutationFinished {
                        request_id,
                        op: SessionMutationOp::UndoDelete,
                        affected_ids: vec![session_id],
                        message: String::new(),
                        reload_after: false,
                        error: Some(format!("Failed to restore session: {}", message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::UndoDelete,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some("Unexpected session restore response".to_string()),
                }),
                Err(e) => Some(TuiCommand::SessionMutationFinished {
                    request_id,
                    op: SessionMutationOp::UndoDelete,
                    affected_ids: vec![session_id],
                    message: String::new(),
                    reload_after: false,
                    error: Some(format!("Failed to restore session: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn start_share_session(app: &mut App, session_id: String) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "share_session",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ShareSessionFinished {
                    session_id,
                    session: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("session-share-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionShare {
                    session_id: session_id.clone(),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Session { session }) => Some(TuiCommand::ShareSessionFinished {
                    session_id,
                    session: Some(session),
                    error: None,
                }),
                Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::ShareSessionFinished {
                    session_id,
                    session: None,
                    error: Some(format!("Failed to share: {}", message)),
                }),
                Ok(_other) => Some(TuiCommand::ShareSessionFinished {
                    session_id,
                    session: None,
                    error: Some("Unexpected share response".to_string()),
                }),
                Err(e) => Some(TuiCommand::ShareSessionFinished {
                    session_id,
                    session: None,
                    error: Some(format!("Failed to share: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_share_session_finished(
    app: &mut App,
    session_id: String,
    session: Option<crate::protocol::dto::Session>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
        return;
    }
    if let Some(shared) = session {
        app.session_state.session =
            Some(crate::protocol_conversions::dto_to_session(shared.clone()));
        let url = shared.share_url.unwrap_or_default();
        let mut dialog = crate::tui::components::dialogs::share::ShareDialog::new(Arc::clone(
            &app.ui_state.theme,
        ));
        dialog.set_theme(&app.ui_state.theme);
        dialog.set_url(url);
        app.dialog_state.share_dialog = Some(dialog);
        app.open_dialog(crate::tui::app::Dialog::Share);
    }
    let _ = session_id;
}

pub(crate) fn start_unshare_session(app: &mut App, session_id: String) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "unshare_session",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::UnshareSessionFinished {
                    session_id,
                    session: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("session-unshare-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionUnshare {
                    session_id: session_id.clone(),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Session { session }) => Some(TuiCommand::UnshareSessionFinished {
                    session_id,
                    session: Some(session),
                    error: None,
                }),
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::UnshareSessionFinished {
                        session_id,
                        session: None,
                        error: Some(format!("Failed to unshare: {}", message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::UnshareSessionFinished {
                    session_id,
                    session: None,
                    error: Some("Unexpected unshare response".to_string()),
                }),
                Err(e) => Some(TuiCommand::UnshareSessionFinished {
                    session_id,
                    session: None,
                    error: Some(format!("Failed to unshare: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_unshare_session_finished(
    app: &mut App,
    _session_id: String,
    session: Option<crate::protocol::dto::Session>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
        return;
    }
    if let Some(dto) = session {
        app.session_state.session = Some(crate::protocol_conversions::dto_to_session(dto));
        app.messages_state.toasts.info("Session unshared");
    }
}

pub(crate) fn start_export_session(app: &mut App, session_id: String) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "export_session",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ExportSessionFinished {
                    session_id,
                    json: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("session-export-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionExport {
                    session_id: session_id.clone(),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Json { data: export }) => {
                    let json = serde_json::to_string_pretty(&export).unwrap_or_default();
                    Some(TuiCommand::ExportSessionFinished {
                        session_id,
                        json: Some(json),
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::ExportSessionFinished {
                        session_id,
                        json: None,
                        error: Some(format!("Failed to export: {}", message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::ExportSessionFinished {
                    session_id,
                    json: None,
                    error: Some("Unexpected export response".to_string()),
                }),
                Err(e) => Some(TuiCommand::ExportSessionFinished {
                    session_id,
                    json: None,
                    error: Some(format!("Failed to export: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_export_session_finished(
    app: &mut App,
    _session_id: String,
    json: Option<String>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
        return;
    }
    if let Some(json) = json {
        match crate::util::clipboard::copy_to_clipboard(&json) {
            Ok(_) => {
                app.messages_state
                    .toasts
                    .info("Session exported to clipboard");
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Clipboard error: {}", e));
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_open_tree_dialog(app: &mut App) {
    use crate::tui::components::dialogs::tree::TreeNode;
    use std::collections::HashMap;

    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Core unavailable — check daemon status with /doctor");
        return;
    };
    let Some(current_session) = app.session_state.session.clone() else {
        app.dialog_state.tree_dialog.load_nodes(Vec::new(), None);
        return;
    };

    let list_request = crate::core::new_request(
        format!("session-tree-list-{}", uuid::Uuid::new_v4()),
        CoreRequest::SessionList {
            project_id: app.session_state.project_dir.clone(),
            show_archived: true,
            limit: 1000,
        },
    );
    let sessions = match core_client.request(list_request).await {
        Ok(CoreResponse::SessionList { sessions }) => sessions,
        Ok(CoreResponse::Error { message, .. }) => {
            app.messages_state
                .toasts
                .error(&format!("Failed to load tree sessions: {}", message));
            return;
        }
        Ok(_other) => {
            app.messages_state.toasts.error("Unexpected tree response");
            return;
        }
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("Failed to load tree sessions: {}", e));
            return;
        }
    };

    let by_id: HashMap<String, crate::session::Session> = sessions
        .iter()
        .cloned()
        .map(|s| (s.id.clone(), crate::protocol_conversions::dto_to_session(s)))
        .collect();
    let mut root_id = current_session.id.clone();
    while let Some(parent_id) = by_id.get(&root_id).and_then(|s| s.parent_id.clone()) {
        if !by_id.contains_key(&parent_id) {
            break;
        }
        root_id = parent_id;
    }

    let mut children_map: HashMap<String, Vec<crate::session::Session>> = HashMap::new();
    for session in &sessions {
        if let Some(parent_id) = &session.parent_id {
            children_map
                .entry(parent_id.clone())
                .or_default()
                .push(crate::protocol_conversions::dto_to_session(session.clone()));
        }
    }

    let counts_request = crate::core::new_request(
        format!("session-tree-counts-{}", uuid::Uuid::new_v4()),
        CoreRequest::SessionMessageCounts {
            session_ids: sessions.iter().map(|s| s.id.clone()).collect(),
        },
    );
    let counts = match core_client.request(counts_request).await {
        Ok(CoreResponse::SessionMessageCounts { counts }) => counts,
        _ => HashMap::new(),
    };

    fn build_node(
        session: &crate::session::Session,
        depth: usize,
        current_session_id: &str,
        children_map: &HashMap<String, Vec<crate::session::Session>>,
        counts: &HashMap<String, usize>,
    ) -> TreeNode {
        let mut children = children_map.get(&session.id).cloned().unwrap_or_default();
        children.sort_by_key(|s| s.time_updated);
        let child_nodes = children
            .iter()
            .map(|child| build_node(child, depth + 1, current_session_id, children_map, counts))
            .collect();
        TreeNode {
            id: session.id.clone(),
            session_id: session.id.clone(),
            label: session.title.clone(),
            time_updated: session.time_updated,
            message_count: counts.get(&session.id).copied(),
            is_current: session.id == current_session_id,
            is_archived: session.time_archived.is_some(),
            children: child_nodes,
            depth,
        }
    }

    let tree_nodes = if let Some(root) = by_id.get(&root_id) {
        vec![build_node(
            root,
            0,
            &current_session.id,
            &children_map,
            &counts,
        )]
    } else {
        Vec::new()
    };
    app.dialog_state
        .tree_dialog
        .load_nodes(tree_nodes, Some(current_session.id));
}

pub(crate) fn start_open_tree_dialog(app: &mut App) {
    let Some(current_session) = app.session_state.session.clone() else {
        app.dialog_state.tree_dialog.load_nodes(Vec::new(), None);
        return;
    };

    app.dialog_state
        .tree_dialog
        .load_nodes(Vec::new(), Some(current_session.id.clone()));
    app.open_dialog(crate::tui::app::Dialog::Tree);

    let core_client = app.core_client.clone();
    let project_dir = app.session_state.project_dir.clone();
    let current_session_id = current_session.id.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "open_tree_dialog",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TreeDialogLoaded {
                    current_session_id: Some(current_session_id),
                    nodes: Vec::new(),
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };

            let list_request = crate::core::new_request(
                format!("session-tree-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionList {
                    project_id: project_dir,
                    show_archived: true,
                    limit: 1000,
                },
            );
            let sessions = match core_client.request(list_request).await {
                Ok(CoreResponse::SessionList { sessions }) => sessions,
                Ok(CoreResponse::Error { message, .. }) => {
                    return Some(TuiCommand::TreeDialogLoaded {
                        current_session_id: Some(current_session_id),
                        nodes: Vec::new(),
                        error: Some(format!("Failed to load tree sessions: {}", message)),
                    });
                }
                Ok(_other) => {
                    return Some(TuiCommand::TreeDialogLoaded {
                        current_session_id: Some(current_session_id),
                        nodes: Vec::new(),
                        error: Some("Unexpected tree response".to_string()),
                    });
                }
                Err(e) => {
                    return Some(TuiCommand::TreeDialogLoaded {
                        current_session_id: Some(current_session_id),
                        nodes: Vec::new(),
                        error: Some(format!("Failed to load tree sessions: {}", e)),
                    });
                }
            };

            let counts_request = crate::core::new_request(
                format!("session-tree-counts-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionMessageCounts {
                    session_ids: sessions.iter().map(|s| s.id.clone()).collect(),
                },
            );
            let counts = match core_client.request(counts_request).await {
                Ok(CoreResponse::SessionMessageCounts { counts }) => counts,
                _ => std::collections::HashMap::new(),
            };

            let by_id: std::collections::HashMap<String, crate::session::Session> = sessions
                .iter()
                .cloned()
                .map(|s| (s.id.clone(), crate::protocol_conversions::dto_to_session(s)))
                .collect();

            let mut root_id = current_session_id.clone();
            while let Some(parent_id) = by_id.get(&root_id).and_then(|s| s.parent_id.clone()) {
                if !by_id.contains_key(&parent_id) {
                    break;
                }
                root_id = parent_id;
            }

            let mut children_map: std::collections::HashMap<String, Vec<crate::session::Session>> =
                std::collections::HashMap::new();
            for session in &sessions {
                if let Some(parent_id) = &session.parent_id {
                    children_map
                        .entry(parent_id.clone())
                        .or_default()
                        .push(crate::protocol_conversions::dto_to_session(session.clone()));
                }
            }

            fn build_node(
                session: &crate::session::Session,
                depth: usize,
                current_session_id: &str,
                children_map: &std::collections::HashMap<String, Vec<crate::session::Session>>,
                counts: &std::collections::HashMap<String, usize>,
            ) -> crate::tui::components::dialogs::tree::TreeNode {
                use crate::tui::components::dialogs::tree::TreeNode;
                let mut children = children_map.get(&session.id).cloned().unwrap_or_default();
                children.sort_by_key(|s| s.time_updated);
                let child_nodes = children
                    .iter()
                    .map(|child| {
                        build_node(child, depth + 1, current_session_id, children_map, counts)
                    })
                    .collect();
                TreeNode {
                    id: session.id.clone(),
                    session_id: session.id.clone(),
                    label: session.title.clone(),
                    time_updated: session.time_updated,
                    message_count: counts.get(&session.id).copied(),
                    is_current: session.id == current_session_id,
                    is_archived: session.time_archived.is_some(),
                    children: child_nodes,
                    depth,
                }
            }

            let tree_nodes = if let Some(root) = by_id.get(&root_id) {
                vec![build_node(
                    root,
                    0,
                    &current_session_id,
                    &children_map,
                    &counts,
                )]
            } else {
                Vec::new()
            };

            Some(TuiCommand::TreeDialogLoaded {
                current_session_id: Some(current_session_id),
                nodes: tree_nodes,
                error: None,
            })
        },
    );
}

pub(crate) fn apply_tree_dialog_loaded(
    app: &mut App,
    current_session_id: Option<String>,
    nodes: Vec<crate::tui::components::dialogs::tree::TreeNode>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
        return;
    }
    app.dialog_state
        .tree_dialog
        .load_nodes(nodes, current_session_id);
}

#[allow(dead_code)]
pub(crate) async fn handle_load_session_messages(app: &mut App, session_id: String) {
    use crate::tui::components::messages::{MessageRole, MsgPart, UIMessage};

    async fn load_via_core(
        app: &mut App,
        session_id: &str,
    ) -> Option<Vec<crate::session::message::Message>> {
        let client = app.core_client.clone()?;
        let request = crate::core::new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::SessionMessagesLoad {
                session_id: session_id.to_string(),
            },
        );
        match client.request(request).await {
            Ok(CoreResponse::SessionMessages { messages, .. }) => {
                Some(crate::protocol_conversions::dtos_to_messages(messages))
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to load messages: {}", message));
                None
            }
            Ok(CoreResponse::Ack)
            | Ok(CoreResponse::SessionList { .. })
            | Ok(CoreResponse::Session { .. })
            | Ok(CoreResponse::SessionMessageCounts { .. })
            | Ok(CoreResponse::Json { .. })
            | Ok(CoreResponse::SnapshotSession { .. })
            | Ok(CoreResponse::SnapshotDaemon { .. })
            | Ok(CoreResponse::ModelsSnapshot { .. })
            | Ok(CoreResponse::Events { .. })
            | Ok(CoreResponse::ResyncRequired { .. })
            | Ok(CoreResponse::WorkspaceList { .. })
            | Ok(CoreResponse::WorkspaceSnapshot { .. }) => None,
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to load messages: {}", e));
                None
            }
        }
    }

    app.messages_state.messages.clear();

    let messages = if app.core_client.is_some() {
        match load_via_core(app, &session_id).await {
            Some(messages) => messages,
            None => return,
        }
    } else {
        app.messages_state
            .toasts
            .error("Core unavailable — check daemon status with /doctor");
        return;
    };

    for msg in messages {
        let role = {
            fn determine_role(msg: &crate::session::message::Message) -> MessageRole {
                for part in &msg.data.parts {
                    match &part.data {
                        crate::session::message::PartData::Reasoning { .. }
                        | crate::session::message::PartData::ToolCall { .. } => {
                            return MessageRole::Assistant;
                        }
                        _ => {}
                    }
                }
                MessageRole::User
            }
            determine_role(&msg)
        };
        let parts = {
            fn convert(parts: &[crate::session::message::PartInfo]) -> Vec<MsgPart> {
                parts
                    .iter()
                    .map(|p| match &p.data {
                        crate::session::message::PartData::Text { text } => MsgPart::Text {
                            content: text.clone(),
                        },
                        crate::session::message::PartData::Reasoning { reasoning } => {
                            MsgPart::Reasoning {
                                content: reasoning.clone(),
                                collapsed: false,
                            }
                        }
                        crate::session::message::PartData::ToolCall {
                            id,
                            name,
                            input,
                            output,
                            status,
                        } => MsgPart::ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            input: serde_json::to_string(input).unwrap_or_default(),
                            output: output.clone().unwrap_or_default(),
                            status: status.clone(),
                            duration_ms: None,
                            exit_code: None,
                            output_lines: None,
                            expanded: false,
                        },
                        crate::session::message::PartData::Image { url } => MsgPart::Image {
                            data_uri: url.clone(),
                            alt_text: "[Image]".to_string(),
                            width: 0,
                            height: 0,
                        },
                        crate::session::message::PartData::File { .. } => MsgPart::Text {
                            content: "[File]".to_string(),
                        },
                    })
                    .collect()
            }
            convert(&msg.data.parts)
        };

        if !parts.is_empty() || role == MessageRole::Assistant {
            app.messages_state.messages.messages.push(UIMessage {
                role,
                parts,
                timestamp: None,
                is_plan_mode: None,
            });
        }
    }
}

pub(crate) fn start_load_session_messages(app: &mut App, session_id: String) {
    app.messages_state.toasts.info("Loading messages...");
    let request_id = app.dialog_state.session_messages_request.begin();

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let sid = session_id.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "load_session_messages",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionMessagesLoaded {
                    request_id,
                    session_id: sid,
                    messages: Vec::new(),
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };

            let request = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                CoreRequest::SessionMessagesLoad {
                    session_id: sid.clone(),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::SessionMessages { messages, .. }) => {
                    let messages = crate::protocol_conversions::dtos_to_messages(messages);
                    Some(TuiCommand::SessionMessagesLoaded {
                        request_id,
                        session_id: sid,
                        messages,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::SessionMessagesLoaded {
                        request_id,
                        session_id: sid,
                        messages: Vec::new(),
                        error: Some(format!("Failed to load messages: {}", message)),
                    })
                }
                Ok(_) => Some(TuiCommand::SessionMessagesLoaded {
                    request_id,
                    session_id: sid,
                    messages: Vec::new(),
                    error: Some("Unexpected response".to_string()),
                }),
                Err(e) => Some(TuiCommand::SessionMessagesLoaded {
                    request_id,
                    session_id: sid,
                    messages: Vec::new(),
                    error: Some(format!("Failed to load messages: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_session_messages_loaded(
    app: &mut App,
    request_id: u64,
    session_id: String,
    messages: Vec<crate::session::message::Message>,
    error: Option<String>,
) {
    use crate::tui::components::messages::{MessageRole, MsgPart, UIMessage};

    if let Some(err) = error {
        if !app
            .dialog_state
            .session_messages_request
            .fail(request_id, err.clone())
        {
            return;
        }
        app.messages_state.toasts.error(&err);
        return;
    }
    if !app.dialog_state.session_messages_request.finish(request_id) {
        return;
    }

    // Only update messages if this is still the active session
    if app.session_state.session.as_ref().map(|s| s.id.as_str()) != Some(&session_id) {
        return;
    }

    app.messages_state.messages.clear();

    for msg in messages {
        let role = {
            fn determine_role(msg: &crate::session::message::Message) -> MessageRole {
                for part in &msg.data.parts {
                    match &part.data {
                        crate::session::message::PartData::Reasoning { .. }
                        | crate::session::message::PartData::ToolCall { .. } => {
                            return MessageRole::Assistant;
                        }
                        _ => {}
                    }
                }
                MessageRole::User
            }
            determine_role(&msg)
        };
        let parts = {
            fn convert(parts: &[crate::session::message::PartInfo]) -> Vec<MsgPart> {
                parts
                    .iter()
                    .map(|p| match &p.data {
                        crate::session::message::PartData::Text { text } => MsgPart::Text {
                            content: text.clone(),
                        },
                        crate::session::message::PartData::Reasoning { reasoning } => {
                            MsgPart::Reasoning {
                                content: reasoning.clone(),
                                collapsed: false,
                            }
                        }
                        crate::session::message::PartData::ToolCall {
                            id,
                            name,
                            input,
                            output,
                            status,
                        } => MsgPart::ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            input: serde_json::to_string(input).unwrap_or_default(),
                            output: output.clone().unwrap_or_default(),
                            status: status.clone(),
                            duration_ms: None,
                            exit_code: None,
                            output_lines: None,
                            expanded: false,
                        },
                        crate::session::message::PartData::Image { url } => MsgPart::Image {
                            data_uri: url.clone(),
                            alt_text: "[Image]".to_string(),
                            width: 0,
                            height: 0,
                        },
                        crate::session::message::PartData::File { .. } => MsgPart::Text {
                            content: "[File]".to_string(),
                        },
                    })
                    .collect()
            }
            convert(&msg.data.parts)
        };

        if !parts.is_empty() || role == MessageRole::Assistant {
            app.messages_state.messages.messages.push(UIMessage {
                role,
                parts,
                timestamp: None,
                is_plan_mode: None,
            });
        }
    }
}

pub(crate) fn start_create_from_template(
    app: &mut App,
    _key: String,
    template: crate::config::schema::SessionTemplate,
) {
    let request_id = app.dialog_state.template_create_request.begin();
    let core_client = app.core_client.clone();
    let project_dir = app.session_state.project_dir.clone();
    let template_name = template.name.clone();
    let agent = template.agent.clone();
    let model = template.model.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "create_from_template",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TemplateSessionCreated {
                    request_id,
                    session: None,
                    agent,
                    model,
                    template_name,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("session-create-template-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionCreateFromTemplate {
                    template: crate::protocol_conversions::session_template_to_dto(template),
                    project_id: project_dir.clone(),
                    directory: project_dir,
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Session { session }) => Some(TuiCommand::TemplateSessionCreated {
                    request_id,
                    session: Some(session),
                    agent,
                    model,
                    template_name,
                    error: None,
                }),
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::TemplateSessionCreated {
                        request_id,
                        session: None,
                        agent,
                        model,
                        template_name,
                        error: Some(message),
                    })
                }
                Ok(_other) => Some(TuiCommand::TemplateSessionCreated {
                    request_id,
                    session: None,
                    agent,
                    model,
                    template_name,
                    error: Some("Unexpected core response".to_string()),
                }),
                Err(e) => Some(TuiCommand::TemplateSessionCreated {
                    request_id,
                    session: None,
                    agent,
                    model,
                    template_name,
                    error: Some(e.to_string()),
                }),
            }
        },
    );
}

pub(crate) fn apply_template_session_created(
    app: &mut App,
    request_id: u64,
    session: Option<crate::protocol::dto::Session>,
    agent: Option<String>,
    model: Option<String>,
    template_name: String,
    error: Option<String>,
) {
    if let Some(err) = error {
        if !app
            .dialog_state
            .template_create_request
            .fail(request_id, err.clone())
        {
            return;
        }
        app.messages_state
            .toasts
            .error(&format!("Failed to create session from template: {}", err));
        return;
    }
    if !app.dialog_state.template_create_request.finish(request_id) {
        return;
    }
    let Some(session) = session else {
        return;
    };
    app.session_state.session = Some(crate::protocol_conversions::dto_to_session(session.clone()));
    app.ui_state
        .routes
        .navigate_to(crate::tui::Route::Session(session.id.clone()));

    if let Some(agent_name) = agent {
        if let Some(idx) = app
            .agent_state
            .agents
            .iter()
            .position(|a| a.name == agent_name)
        {
            app.agent_state.current_agent = idx;
        }
    }
    if let Some(model_name) = model {
        if let Some(idx) = app.agent_state.models.iter().position(|m| m == &model_name) {
            app.agent_state.current_model = model_name.clone();
            app.agent_state.model_idx = idx;
        }
    }

    if let Some(ref tx) = app.tui_cmd_tx {
        let session_id = session.id.clone();
        let _ = tx.try_send(TuiCommand::LoadSessionMessages { session_id });
    }
    app.messages_state.toasts.info(&format!(
        "Session '{}' created from template",
        template_name
    ));
}

#[allow(dead_code)]
pub(crate) async fn handle_create_from_template(
    app: &mut App,
    _key: String,
    template: crate::config::schema::SessionTemplate,
) {
    let project_dir = app.session_state.project_dir.clone();
    let template_name = template.name.clone();
    let agent = template.agent.clone();
    let model = template.model.clone();
    let created = if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-create-template-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionCreateFromTemplate {
                template: crate::protocol_conversions::session_template_to_dto(template.clone()),
                project_id: project_dir.clone(),
                directory: project_dir.clone(),
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Session { session }) => Ok(session),
            Ok(CoreResponse::Error { message, .. }) => Err(message),
            Ok(_other) => Err("Unexpected core response".to_string()),
            Err(e) => Err(e.to_string()),
        }
    } else {
        Err("Core unavailable — check daemon status with /doctor".to_string())
    };
    match created {
        Ok(session) => {
            app.session_state.session =
                Some(crate::protocol_conversions::dto_to_session(session.clone()));
            app.ui_state
                .routes
                .navigate_to(crate::tui::Route::Session(session.id.clone()));

            if let Some(agent_name) = agent {
                if let Some(idx) = app
                    .agent_state
                    .agents
                    .iter()
                    .position(|a| a.name == agent_name)
                {
                    app.agent_state.current_agent = idx;
                }
            }

            if let Some(model_name) = model {
                if let Some(idx) = app.agent_state.models.iter().position(|m| m == &model_name) {
                    app.agent_state.current_model = model_name.clone();
                    app.agent_state.model_idx = idx;
                }
            }

            if let Some(ref tx) = app.tui_cmd_tx {
                let session_id = session.id.clone();
                let _ = tx.try_send(TuiCommand::LoadSessionMessages { session_id });
            }

            app.messages_state.toasts.info(&format!(
                "Session '{}' created from template",
                template_name
            ));
        }
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("Failed to create session from template: {}", e));
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy test-only handlers
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn reload_sessions(app: &mut App) {
    use std::collections::HashMap;

    let project_id = app.session_state.project_dir.clone();
    let show_archived = app.dialog_state.session_dialog.show_archived;

    app.dialog_state.session_dialog.set_loading(true);

    let sessions = if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-list-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionList {
                project_id: project_id.clone(),
                show_archived,
                limit: 100,
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::SessionList { sessions }) => sessions,
            Ok(CoreResponse::Error { code, message }) => {
                tracing::warn!("failed to load sessions via core ({}): {}", code, message);
                return;
            }
            Ok(other) => {
                tracing::warn!("unexpected core response for session list: {:?}", other);
                return;
            }
            Err(e) => {
                tracing::warn!("failed to load sessions via core: {}", e);
                return;
            }
        }
    } else {
        tracing::warn!("core client unavailable for session list");
        return;
    };

    let session_ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
    let message_counts: HashMap<String, usize> = if session_ids.is_empty() {
        HashMap::new()
    } else if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-message-counts-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionMessageCounts {
                session_ids: session_ids.clone(),
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::SessionMessageCounts { counts }) => counts,
            Ok(CoreResponse::Error { code, message }) => {
                tracing::warn!(
                    "failed to load session message counts via core ({}): {}",
                    code,
                    message
                );
                HashMap::new()
            }
            Ok(other) => {
                tracing::warn!(
                    "unexpected core response for session message counts: {:?}",
                    other
                );
                HashMap::new()
            }
            Err(e) => {
                tracing::warn!("failed to load session message counts via core: {}", e);
                HashMap::new()
            }
        }
    } else {
        tracing::warn!("core client unavailable for session message counts");
        HashMap::new()
    };

    app.dialog_state
        .session_dialog
        .load_sessions(crate::protocol_conversions::dtos_to_sessions(sessions));
    for (id, count) in message_counts {
        app.dialog_state
            .session_dialog
            .set_message_count(&id, count);
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_delete_session(app: &mut App, session_id: String) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-delete-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionDelete {
                session_id: session_id.clone(),
                permanent: false,
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Ack) => {}
            Ok(CoreResponse::Error { code, message }) => {
                tracing::warn!("failed to delete session via core ({}): {}", code, message);
            }
            Ok(other) => {
                tracing::warn!("unexpected core response for session delete: {:?}", other);
            }
            Err(e) => {
                tracing::warn!("failed to delete session via core: {}", e);
            }
        }
    } else {
        tracing::warn!("core client unavailable for delete session");
    }
    app.messages_state.toasts.info("Session deleted");
    reload_sessions(app).await;
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_archive_session(app: &mut App, session_id: String, unarchive: bool) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-archive-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionArchive {
                session_id: session_id.clone(),
                unarchive,
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Ack) => {}
            Ok(CoreResponse::Error { code, message }) => {
                tracing::warn!("failed to archive session via core ({}): {}", code, message);
            }
            Ok(other) => {
                tracing::warn!("unexpected core response for session archive: {:?}", other);
            }
            Err(e) => {
                tracing::warn!("failed to archive session via core: {}", e);
            }
        }
    } else {
        tracing::warn!("core client unavailable for archive session");
    }
    let msg = if unarchive {
        "Session unarchived"
    } else {
        "Session archived"
    };
    app.messages_state.toasts.info(msg);
    reload_sessions(app).await;
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_fork_session(app: &mut App, session_id: String) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-fork-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionFork {
                session_id: session_id.clone(),
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Ack) => {}
            Ok(CoreResponse::Error { code, message }) => {
                tracing::warn!("failed to fork session via core ({}): {}", code, message);
            }
            Ok(other) => {
                tracing::warn!("unexpected core response for session fork: {:?}", other);
            }
            Err(e) => {
                tracing::warn!("failed to fork session via core: {}", e);
            }
        }
    } else {
        tracing::warn!("core client unavailable for fork session");
    }
    app.messages_state.toasts.info("Session forked");
    reload_sessions(app).await;
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_bulk_delete(app: &mut App, session_ids: Vec<String>) {
    let count = session_ids.len();
    if let Some(core_client) = app.core_client.clone() {
        for id in &session_ids {
            let request = crate::core::new_request(
                format!("session-delete-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionDelete {
                    session_id: id.clone(),
                    permanent: true,
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Ack) => {}
                Ok(CoreResponse::Error { code, message }) => {
                    tracing::warn!(
                        "failed to permanently delete session {} via core ({}): {}",
                        id,
                        code,
                        message
                    );
                }
                Ok(other) => {
                    tracing::warn!(
                        "unexpected core response for permanent session delete {}: {:?}",
                        id,
                        other
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to permanently delete session {} via core: {}",
                        id,
                        e
                    );
                }
            }
        }
    } else {
        tracing::warn!("core client unavailable for bulk delete");
    }
    app.messages_state
        .toasts
        .info(&format!("{} sessions deleted", count));
    reload_sessions(app).await;
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_bulk_archive(app: &mut App, session_ids: Vec<String>, unarchive: bool) {
    let count = session_ids.len();
    if let Some(core_client) = app.core_client.clone() {
        for id in &session_ids {
            let request = crate::core::new_request(
                format!("session-archive-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionArchive {
                    session_id: id.clone(),
                    unarchive,
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Ack) => {}
                Ok(CoreResponse::Error { code, message }) => {
                    tracing::warn!(
                        "failed to archive session {} via core ({}): {}",
                        id,
                        code,
                        message
                    );
                }
                Ok(other) => {
                    tracing::warn!(
                        "unexpected core response for session archive {}: {:?}",
                        id,
                        other
                    );
                }
                Err(e) => {
                    tracing::warn!("failed to archive session {} via core: {}", id, e);
                }
            }
        }
    } else {
        tracing::warn!("core client unavailable for bulk archive");
    }
    let msg = if unarchive {
        format!("{} sessions unarchived", count)
    } else {
        format!("{} sessions archived", count)
    };
    app.messages_state.toasts.info(&msg);
    reload_sessions(app).await;
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_bulk_export(app: &mut App, session_ids: Vec<String>) {
    let count = session_ids.len();
    if let Some(core_client) = app.core_client.clone() {
        for id in &session_ids {
            let request = crate::core::new_request(
                format!("session-export-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionExport {
                    session_id: id.clone(),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Json { .. }) => tracing::info!("exported session {}", id),
                Ok(CoreResponse::Error { code, message }) => {
                    tracing::warn!(
                        "failed to export session {} via core ({}): {}",
                        id,
                        code,
                        message
                    )
                }
                Ok(other) => tracing::warn!(
                    "unexpected core response for session export {}: {:?}",
                    id,
                    other
                ),
                Err(e) => tracing::warn!("failed to export session {} via core: {}", id, e),
            }
        }
    } else {
        tracing::warn!("core client unavailable for bulk export");
    }
    app.messages_state
        .toasts
        .info(&format!("{} sessions exported", count));
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_share_session(app: &mut App, session_id: String) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-share-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionShare { session_id },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Session { session: shared }) => {
                app.session_state.session =
                    Some(crate::protocol_conversions::dto_to_session(shared.clone()));
                let url = shared.share_url.unwrap_or_default();
                let mut dialog = crate::tui::components::dialogs::share::ShareDialog::new(
                    Arc::clone(&app.ui_state.theme),
                );
                dialog.set_theme(&app.ui_state.theme);
                dialog.set_url(url);
                app.dialog_state.share_dialog = Some(dialog);
                app.open_dialog(crate::tui::app::Dialog::Share);
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to share: {}", message));
            }
            Ok(_other) => {
                app.messages_state.toasts.error("Unexpected share response");
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to share: {}", e));
            }
        }
    } else {
        app.messages_state
            .toasts
            .error("Core unavailable — check daemon status with /doctor");
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_unshare_session(app: &mut App, session_id: String) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-unshare-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionUnshare { session_id },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Session { session }) => {
                app.session_state.session =
                    Some(crate::protocol_conversions::dto_to_session(session));
                app.messages_state.toasts.info("Session unshared");
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to unshare: {}", message));
            }
            Ok(_other) => {
                app.messages_state
                    .toasts
                    .error("Unexpected unshare response");
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to unshare: {}", e));
            }
        }
    } else {
        app.messages_state
            .toasts
            .error("Core unavailable — check daemon status with /doctor");
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_export_session(app: &mut App, session_id: String) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-export-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionExport { session_id },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { data: export }) => {
                let json = serde_json::to_string_pretty(&export).unwrap_or_default();
                match crate::util::clipboard::copy_to_clipboard(&json) {
                    Ok(_) => {
                        app.messages_state
                            .toasts
                            .info("Session exported to clipboard");
                    }
                    Err(e) => {
                        app.messages_state
                            .toasts
                            .error(&format!("Clipboard error: {}", e));
                    }
                }
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to export: {}", message));
            }
            Ok(_other) => {
                app.messages_state
                    .toasts
                    .error("Unexpected export response");
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to export: {}", e));
            }
        }
    } else {
        app.messages_state
            .toasts
            .error("Core unavailable — check daemon status with /doctor");
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_rename_session(app: &mut App, session_id: String, new_title: String) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-rename-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionRename {
                session_id,
                new_title,
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Session { session }) => {
                app.session_state.session =
                    Some(crate::protocol_conversions::dto_to_session(session));
                app.messages_state.toasts.info("Session renamed");
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to rename: {}", message));
            }
            Ok(_other) => {
                app.messages_state
                    .toasts
                    .error("Unexpected rename response");
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to rename: {}", e));
            }
        }
    } else {
        app.messages_state
            .toasts
            .error("Core unavailable — check daemon status with /doctor");
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_undo_delete(app: &mut App, session_id: String) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("session-restore-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionRestore { session_id },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Session { .. }) | Ok(CoreResponse::Ack) => {
                app.messages_state
                    .toasts
                    .success("Session restored successfully");
                reload_sessions(app).await;
            }
            Ok(CoreResponse::Error { message, .. }) => {
                tracing::error!("Failed to restore session: {}", message);
                app.messages_state
                    .toasts
                    .error(&format!("Restore failed: {}", message));
            }
            Ok(other) => {
                tracing::error!("Unexpected session restore response: {:?}", other);
                app.messages_state
                    .toasts
                    .error("Restore failed: unexpected response");
            }
            Err(e) => {
                tracing::error!("Failed to restore session: {}", e);
                app.messages_state
                    .toasts
                    .error(&format!("Restore failed: {}", e));
            }
        }
    } else {
        tracing::warn!("No core client available for undo");
    }
    app.undo_session_id = None;
    app.undo_until = None;
}
