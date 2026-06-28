//! # Terminal User Interface (TUI)
//!
//! This module provides the terminal-based UI for codegg, built with [ratatui].
//!
//! ## Architecture Overview
//!
//! The TUI is structured around several key components:
//!
//! - [`App`]: Main application state and event handling
//! - [`components`]: Reusable UI widgets (messages, prompt, sidebar, etc.)
//! - [`input`]: Keyboard event handling and keybindings
//! - [`layout`]: Layout management and area calculations
//! - [`theme`]: Color themes and styling
//! - [`route`]: Route/state machine management
//! - [`command`]: Slash command registry
//!
//! ## State Management
//!
//! The [`App`] struct is organized into several state domains:
//!
//! - [`UiState`](app::UiState): UI state (theme, layout, dialogs, routes)
//! - [`SessionState`](app::SessionState): Session management
//! - [`PromptState`](app::PromptState): Prompt input state
//! - [`MessagesState`](app::MessagesState): Message history and display
//! - [`DialogState`](app::DialogState): Dialog visibility and data
//! - [`AgentState`](app::AgentState): Agent and model configuration
//!
//! ### State Flow
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                      App                                │
//! │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐  │
//! │  │ UiState  │ │ Session  │ │ Prompt   │ │ Dialog    │  │
//! │  │ - theme  │ │ - session│ │ - prompt │ │ - dialogs │  │
//! │  │ - layout │ │ - store  │ │ - compl. │ │ - state   │  │
//! │  └──────────┘ └──────────┘ └──────────┘ └───────────┘  │
//! └─────────────────────────────────────────────────────────┘
//!                         │
//!                         ▼
//!              ┌─────────────────────┐
//!              │  Event Handling     │
//!              │  on_key()           │
//!              │  on_mouse()         │
//!              └─────────────────────┘
//!                         │
//!                         ▼
//!              ┌─────────────────────┐
//!              │      Render         │
//!              │  render()          │
//!              │  render_dialog()    │
//!              └─────────────────────┘
//! ```
//!
//! ## Rendering
//!
//! Rendering uses ratatui's widget model. The main render loop in
//! [`run_event_loop`] handles:
//! - Panic recovery via [`catch_unwind`](std::panic::catch_unwind)
//! - Error boundary display via [`render_error`](app::App::render_error)
//! - Terminal resize handling
//!
//! ### Render Order
//!
//! 1. **Header**: Agent name, model, session info
//! 2. **Viewport**: Messages (Home or Session view)
//! 3. **Prompt**: Input area with status indicator
//! 4. **Footer**: Token counts, session status
//! 5. **Sidebar**: Optional session/agent info panel
//! 6. **Dialog**: Modal overlay (if open)
//! 7. **Completions**: Slash/file completion popup (if active)
//! 8. **Toasts**: Notification messages
//!
//! ## Event Flow
//!
//! ```text
//! Terminal Input ──► EventStream ──► on_key() / on_mouse()
//!                                       │
//!                                       ▼
//!                              ┌─────────────────┐
//!                              │ Route to State  │
//!                              │ - dialog_key    │
//!                              │ - prompt_key    │
//!                              │ - binding_action│
//!                              └─────────────────┘
//!                                       │
//!                                       ▼
//!                              ┌─────────────────┐
//!                              │ State Mutation  │
//!                              │ - update state  │
//!                              │ - open/close    │
//!                              └─────────────────┘
//! ```
//!
//! 1. Terminal events are captured via crossterm's [`EventStream`]
//! 2. Events are routed to [`App::on_key`](app::App::on_key) or [`App::on_mouse`](app::App::on_mouse)
//! 3. Key events are matched against bindings, routed to dialog/prompt handlers
//! 4. State changes trigger re-renders via [`App::render`](app::App::render)
//!
//! ## Error Handling
//!
//! Render errors are caught and displayed gracefully without crashing the application.
//! The event loop uses `catch_unwind` to recover from rendering panics.

pub mod app;
pub mod async_cmd;
pub mod command;
pub mod components;
pub mod file_diff;
pub mod input;
pub mod layout;
pub mod route;
pub mod terminal;
pub mod theme;

pub use app::{App, Dialog, TuiCommand};
pub use input::InputAction;
pub use route::Route;
pub use theme::Theme;

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::AppError;
use crate::permission::PermissionRequest;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::async_cmd::spawn_tui_task;
use crate::tui::components::dialogs::import::ImportSource;
use crate::tui::components::toast::Toast;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::tui::app::state::AppMode;
use crate::tui::app::SessionStatus;
use md5;
use rand;
use tokio::sync::mpsc;

pub type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub fn create_terminal() -> Result<AppTerminal, AppError> {
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn render_app(
    terminal: &mut AppTerminal,
    app: &mut app::App,
) -> Result<(), Box<dyn std::error::Error>> {
    terminal.draw(|frame| app.render(frame))?;
    Ok(())
}

fn render_error(
    terminal: &mut AppTerminal,
    app: &mut app::App,
    error_msg: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    terminal.draw(|frame| app.render_error(frame, error_msg))?;
    Ok(())
}

fn clear_render_error(app: &mut app::App) {
    app.ui_state.render_panic_count = 0;
    app.ui_state.last_render_error = None;
}

const MAX_RENDER_PANICS: usize = 3;

fn latest_user_message_text(app: &app::App) -> String {
    use crate::tui::components::messages::MessageRole;
    app.messages_state
        .messages
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, MessageRole::User))
        .map(|m| m.text_content())
        .unwrap_or_default()
}

async fn ensure_local_session(app: &mut app::App) {
    if app.session_state.session.is_some() {
        return;
    }
    tracing::debug!(target: "codegg::tui::session", "no session exists, creating new session");
    if let Some(core_client) = app.core_client.clone() {
        let project_dir = app.session_state.project_dir.clone();
        let request = crate::core::new_request(
            format!("session-create-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionCreate {
                directory: project_dir,
                title: None,
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Session { session }) => {
                let session_id = session.id.clone();
                app.session_state.session =
                    Some(crate::protocol_conversions::dto_to_session(session));
                tracing::debug!(target: "codegg::tui::session", session_id = %session_id, "session created via core");
            }
            Ok(CoreResponse::Error { code, message }) => {
                tracing::debug!(target: "codegg::tui::session", code = %code, message = %message, "failed to create session via core");
            }
            Ok(_other) => {
                tracing::debug!(target: "codegg::tui::session", "unexpected session-create response");
            }
            Err(e) => {
                tracing::debug!(target: "codegg::tui::session", error = ?e, "failed to create session via core");
            }
        }
    } else {
        tracing::debug!(target: "codegg::tui::session", "no core client available for session creation");
    }
}

async fn reload_sessions(app: &mut app::App) {
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

fn start_reload_sessions(app: &mut app::App) {
    app.dialog_state.session_dialog.set_loading(true);
    app.dialog_state.session_reload_in_flight = true;

    let core_client = app.core_client.clone();
    let project_id = app.session_state.project_dir.clone();
    let show_archived = app.dialog_state.session_dialog.show_archived;
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "reload_sessions", async move {
        let Some(core_client) = core_client else {
            return Some(TuiCommand::SessionsReloaded {
                sessions: Vec::new(),
                message_counts: std::collections::HashMap::new(),
                error: Some("Core client not available".to_string()),
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
                    sessions: Vec::new(),
                    message_counts: std::collections::HashMap::new(),
                    error: Some(format!("Session list failed ({}): {}", code, message)),
                });
            }
            Ok(other) => {
                return Some(TuiCommand::SessionsReloaded {
                    sessions: Vec::new(),
                    message_counts: std::collections::HashMap::new(),
                    error: Some(format!("Unexpected response: {:?}", other)),
                });
            }
            Err(e) => {
                return Some(TuiCommand::SessionsReloaded {
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
            sessions,
            message_counts,
            error: None,
        })
    });
}

fn apply_sessions_reloaded(
    app: &mut app::App,
    sessions: Vec<crate::protocol::dto::Session>,
    message_counts: std::collections::HashMap<String, usize>,
    error: Option<String>,
) {
    app.dialog_state.session_reload_in_flight = false;
    app.dialog_state.session_dialog.set_loading(false);

    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
        return;
    }

    app.dialog_state
        .session_dialog
        .load_sessions(crate::protocol_conversions::dtos_to_sessions(sessions));
    for (id, count) in message_counts {
        app.dialog_state
            .session_dialog
            .set_message_count(&id, count);
    }
}

async fn handle_delete_session(app: &mut app::App, session_id: String) {
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

async fn handle_archive_session(app: &mut app::App, session_id: String, unarchive: bool) {
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

async fn handle_fork_session(app: &mut app::App, session_id: String) {
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

async fn handle_bulk_delete(app: &mut app::App, session_ids: Vec<String>) {
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

async fn handle_bulk_archive(app: &mut app::App, session_ids: Vec<String>, unarchive: bool) {
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

async fn handle_bulk_export(app: &mut app::App, session_ids: Vec<String>) {
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

async fn handle_share_session(app: &mut app::App, session_id: String) {
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
                app.open_dialog(Dialog::Share);
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to share: {}", message));
            }
            Ok(other) => {
                app.messages_state
                    .toasts
                    .error(&format!("Unexpected share response: {:?}", other));
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to share: {}", e));
            }
        }
    } else {
        app.messages_state.toasts.error("Core client not available");
    }
}

async fn handle_unshare_session(app: &mut app::App, session_id: String) {
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
            Ok(other) => {
                app.messages_state
                    .toasts
                    .error(&format!("Unexpected unshare response: {:?}", other));
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to unshare: {}", e));
            }
        }
    } else {
        app.messages_state.toasts.error("Core client not available");
    }
}

async fn handle_export_session(app: &mut app::App, session_id: String) {
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
            Ok(other) => {
                app.messages_state
                    .toasts
                    .error(&format!("Unexpected export response: {:?}", other));
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to export: {}", e));
            }
        }
    } else {
        app.messages_state.toasts.error("Core client not available");
    }
}

async fn handle_rename_session(app: &mut app::App, session_id: String, new_title: String) {
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
            Ok(other) => {
                app.messages_state
                    .toasts
                    .error(&format!("Unexpected rename response: {:?}", other));
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to rename: {}", e));
            }
        }
    } else {
        app.messages_state.toasts.error("Core client not available");
    }
}

#[allow(dead_code)]
async fn handle_open_tree_dialog(app: &mut app::App) {
    use crate::tui::components::dialogs::tree::TreeNode;
    use std::collections::HashMap;

    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.error("Core client not available");
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
        Ok(other) => {
            app.messages_state
                .toasts
                .error(&format!("Unexpected tree response: {:?}", other));
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

fn start_open_tree_dialog(app: &mut app::App) {
    let Some(current_session) = app.session_state.session.clone() else {
        app.dialog_state.tree_dialog.load_nodes(Vec::new(), None);
        return;
    };

    app.dialog_state
        .tree_dialog
        .load_nodes(Vec::new(), Some(current_session.id.clone()));
    app.open_dialog(crate::tui::Dialog::Tree);

    let core_client = app.core_client.clone();
    let project_dir = app.session_state.project_dir.clone();
    let current_session_id = current_session.id.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "open_tree_dialog", async move {
        let Some(core_client) = core_client else {
            return Some(TuiCommand::TreeDialogLoaded {
                current_session_id: Some(current_session_id),
                nodes: Vec::new(),
                error: Some("Core client not available".to_string()),
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
            Ok(other) => {
                return Some(TuiCommand::TreeDialogLoaded {
                    current_session_id: Some(current_session_id),
                    nodes: Vec::new(),
                    error: Some(format!("Unexpected tree response: {:?}", other)),
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
    });
}

fn apply_tree_dialog_loaded(
    app: &mut app::App,
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
async fn handle_preview_import(app: &mut app::App, source: ImportSource) {
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
                            Ok(other) => {
                                if let Some(ref mut import) = app.dialog_state.import_dialog {
                                    import.set_error(format!(
                                        "Unexpected import response: {:?}",
                                        other
                                    ));
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
        import.set_error("Core client not available".to_string());
    }
}

#[allow(dead_code)]
async fn handle_confirm_import(app: &mut app::App, source: ImportSource) {
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
                    Ok(other) => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_error(format!("Unexpected import response: {:?}", other));
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
        import.set_error("Core client not available".to_string());
    }
}

fn start_preview_import(app: &mut app::App, source: ImportSource) {
    app.dialog_state.import_preview_request_id += 1;
    let request_id = app.dialog_state.import_preview_request_id;

    if let Some(ref mut import) = app.dialog_state.import_dialog {
        import.set_error("Loading preview...".to_string());
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "preview_import", async move {
        let Some(core_client) = core_client else {
            return Some(TuiCommand::ImportPreviewLoaded {
                request_id,
                session: None,
                msg_count: 0,
                error: Some("Core client not available".to_string()),
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
                                Some(TuiCommand::ImportPreviewLoaded {
                                    request_id,
                                    session: Some(crate::protocol_conversions::dto_to_session(
                                        session,
                                    )),
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
                            Ok(other) => Some(TuiCommand::ImportPreviewLoaded {
                                request_id,
                                session: None,
                                msg_count: 0,
                                error: Some(format!("Unexpected import response: {:?}", other)),
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
            },
        }
    });
}

fn apply_import_preview_loaded(
    app: &mut app::App,
    request_id: u64,
    session: Option<crate::session::Session>,
    msg_count: usize,
    error: Option<String>,
) {
    if request_id != app.dialog_state.import_preview_request_id {
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

fn start_confirm_import(app: &mut app::App, source: ImportSource) {
    app.dialog_state.import_preview_request_id += 1;
    let request_id = app.dialog_state.import_preview_request_id;

    if let Some(ref mut import) = app.dialog_state.import_dialog {
        import.set_error("Importing...".to_string());
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "confirm_import", async move {
        let Some(core_client) = core_client else {
            return Some(TuiCommand::ImportConfirmed {
                request_id,
                session: None,
                error: Some("Core client not available".to_string()),
            });
        };

        match source {
            ImportSource::SessionId(id) => {
                let request = crate::core::new_request(
                    format!("session-fork-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SessionFork { session_id: id },
                );
                match core_client.request(request).await {
                    Ok(CoreResponse::Session { session }) => Some(TuiCommand::ImportConfirmed {
                        request_id,
                        session: Some(crate::protocol_conversions::dto_to_session(session)),
                        error: None,
                    }),
                    Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::ImportConfirmed {
                        request_id,
                        session: None,
                        error: Some(format!("Import failed: {}", message)),
                    }),
                    Ok(other) => Some(TuiCommand::ImportConfirmed {
                        request_id,
                        session: None,
                        error: Some(format!("Unexpected import response: {:?}", other)),
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
    });
}

fn apply_import_confirmed(
    app: &mut app::App,
    request_id: u64,
    session: Option<crate::session::Session>,
    error: Option<String>,
) {
    if request_id != app.dialog_state.import_preview_request_id {
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

async fn handle_create_from_template(
    app: &mut app::App,
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
            Ok(other) => Err(format!("Unexpected response: {:?}", other)),
            Err(e) => Err(e.to_string()),
        }
    } else {
        Err("Core client not available".to_string())
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
                let _ = tx.try_send(app::TuiCommand::LoadSessionMessages { session_id });
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

async fn handle_spawn_subagent(app: &mut app::App, agent_name: String, prompt: String) {
    use crate::agent::worker::SubAgentRequest;

    if prompt.trim().is_empty() {
        app.messages_state
            .toasts
            .error("Subagent prompt cannot be empty");
        return;
    }

    let pool = match &app.subagent_pool {
        Some(p) => Arc::clone(p),
        None => {
            app.messages_state
                .toasts
                .error("Subagent pool not initialized");
            return;
        }
    };

    let session_id = app
        .session_state
        .session
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();

    let task_id = rand::random::<u64>();

    let request = SubAgentRequest {
        task_id,
        prompt: prompt.clone(),
        agent: agent_name.clone(),
        parent_id: Some(session_id.clone()),
        denied_tools: Vec::new(),
        allowed_paths: Vec::new(),
        description: format!(
            "Task for agent '{}': {}",
            agent_name,
            prompt.chars().take(100).collect::<String>()
        ),
        depth: 1,
        max_tool_calls: None,
    };

    let spawner = pool.spawner();
    if let Err(e) = spawner.send(request).await {
        app.messages_state
            .toasts
            .error(&format!("Failed to spawn subagent: {}", e));
        return;
    }

    app.messages_state.toasts.info(&format!(
        "Spawned subagent '{}' with task #{}",
        agent_name, task_id
    ));

    app.messages_state
        .messages
        .add_user_message(format!("@{} {}", agent_name, prompt), None);
}

#[allow(dead_code)]
async fn handle_load_session_messages(app: &mut app::App, session_id: String) {
    use crate::tui::components::messages::{MessageRole, MsgPart, UIMessage};

    async fn load_via_core(
        app: &mut app::App,
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
            | Ok(CoreResponse::ResyncRequired { .. }) => None,
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
        app.messages_state.toasts.error("Core client not available");
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

fn start_load_session_messages(app: &mut app::App, session_id: String) {
    app.messages_state.toasts.info("Loading messages...");

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let sid = session_id.clone();

    spawn_tui_task(tx, "load_session_messages", async move {
        let Some(core_client) = core_client else {
            return Some(TuiCommand::SessionMessagesLoaded {
                session_id: sid,
                messages: Vec::new(),
                error: Some("Core client not available".to_string()),
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
                    session_id: sid,
                    messages,
                    error: None,
                })
            }
            Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::SessionMessagesLoaded {
                session_id: sid,
                messages: Vec::new(),
                error: Some(format!("Failed to load messages: {}", message)),
            }),
            Ok(_) => Some(TuiCommand::SessionMessagesLoaded {
                session_id: sid,
                messages: Vec::new(),
                error: Some("Unexpected response".to_string()),
            }),
            Err(e) => Some(TuiCommand::SessionMessagesLoaded {
                session_id: sid,
                messages: Vec::new(),
                error: Some(format!("Failed to load messages: {}", e)),
            }),
        }
    });
}

fn apply_session_messages_loaded(
    app: &mut app::App,
    _session_id: String,
    messages: Vec<crate::session::message::Message>,
    error: Option<String>,
) {
    use crate::tui::components::messages::{MessageRole, MsgPart, UIMessage};

    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
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

async fn handle_undo_delete(app: &mut app::App, session_id: String) {
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
                app.messages_state.toasts.error("Failed to restore session");
            }
            Ok(other) => {
                tracing::error!("Unexpected session restore response: {:?}", other);
                app.messages_state.toasts.error("Failed to restore session");
            }
            Err(e) => {
                tracing::error!("Failed to restore session: {}", e);
                app.messages_state.toasts.error("Failed to restore session");
            }
        }
    } else {
        tracing::warn!("No core client available for undo");
    }
    app.undo_session_id = None;
    app.undo_until = None;
}

async fn handle_list_tasks(app: &mut app::App) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("task-list-{}", uuid::Uuid::new_v4()),
            CoreRequest::TaskList,
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { data }) => {
                let tasks = data
                    .get("tasks")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if tasks.is_empty() {
                    app.messages_state.toasts.info("No background tasks");
                } else {
                    let list: Vec<String> = tasks
                        .iter()
                        .map(|t| {
                            let id = t.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                            let message = t
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            let interval_secs =
                                t.get("interval_secs").and_then(|v| v.as_u64()).unwrap_or(0);
                            format!(
                                "{}: {} ({}s)",
                                id.chars().take(8).collect::<String>(),
                                message.chars().take(30).collect::<String>(),
                                interval_secs
                            )
                        })
                        .collect();
                    app.messages_state.toasts.info(&list.join(" | "));
                }
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Failed to list tasks: {}", message));
            }
            Ok(other) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Unexpected task list response: {:?}", other));
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Failed to list tasks: {}", e));
            }
        }
    } else {
        app.messages_state.toasts.warning("Core client unavailable");
    }
}

async fn handle_delete_task(app: &mut app::App, id: String) {
    if let Some(core_client) = app.core_client.clone() {
        let parsed_id = id.parse::<u64>().ok();
        let Some(parsed_id) = parsed_id else {
            app.messages_state.toasts.warning("Task id must be numeric");
            return;
        };
        let request = crate::core::new_request(
            format!("task-delete-{}", uuid::Uuid::new_v4()),
            CoreRequest::TaskDelete { id: parsed_id },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Ack) => app.messages_state.toasts.info("Task deleted"),
            Ok(CoreResponse::Error { code, .. }) if code == "task_not_found" => {
                app.messages_state.toasts.warning("Task not found");
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Failed to delete task: {}", message));
            }
            Ok(other) => app
                .messages_state
                .toasts
                .warning(&format!("Unexpected task delete response: {:?}", other)),
            Err(e) => app
                .messages_state
                .toasts
                .warning(&format!("Failed to delete task: {}", e)),
        }
    } else {
        app.messages_state.toasts.warning("Core client unavailable");
    }
}

#[allow(dead_code)]
async fn handle_memory_summary(app: &mut app::App) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let project_hash = format!(
        "{:x}",
        md5::compute(app.session_state.project_dir.as_bytes())
    );
    let project_namespace = format!("project/{}", project_hash);
    let req_prefs = crate::core::new_request(
        format!("memory-list-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemoryList {
            namespace: "user/preferences".to_string(),
        },
    );
    let req_proj = crate::core::new_request(
        format!("memory-list-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemoryList {
            namespace: project_namespace.clone(),
        },
    );
    let prefs = match core_client.request(req_prefs).await {
        Ok(CoreResponse::Json { data }) => data
            .get("memories")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let proj = match core_client.request(req_proj).await {
        Ok(CoreResponse::Json { data }) => data
            .get("memories")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let total = prefs.len() + proj.len();
    if total == 0 {
        app.messages_state
            .toasts
            .info("No memories yet. Use /memory-remember <text> to save something.");
        return;
    }
    let mut lines = vec![format!("Memory Summary ({} total):", total)];
    if !prefs.is_empty() {
        lines.push(format!("  user/preferences ({}):", prefs.len()));
        for m in prefs.iter().take(5) {
            let id = m
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .chars()
                .take(8)
                .collect::<String>();
            let title = m
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            lines.push(format!("    - [{}] {}", id, title));
        }
    }
    if !proj.is_empty() {
        lines.push(format!("  {} ({}):", project_namespace, proj.len()));
        for m in proj.iter().take(5) {
            let id = m
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .chars()
                .take(8)
                .collect::<String>();
            let title = m
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            lines.push(format!("    - [{}] {}", id, title));
        }
    }
    app.messages_state.toasts.info(&lines.join("\n"));
}

#[allow(dead_code)]
async fn handle_memory_search(app: &mut app::App, query: String) {
    if query.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-search <query>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let request = crate::core::new_request(
        format!("memory-search-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemorySearch {
            query: query.clone(),
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { data }) => {
            let results = data
                .get("memories")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if results.is_empty() {
                app.messages_state
                    .toasts
                    .info(&format!("No memories found matching '{}'", query));
            } else {
                let lines: Vec<String> = results
                    .iter()
                    .take(10)
                    .map(|m| {
                        let id = m
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .chars()
                            .take(8)
                            .collect::<String>();
                        let title = m
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(untitled)");
                        format!("- [{}] {}", id, title)
                    })
                    .collect();
                app.messages_state.toasts.info(&format!(
                    "Found {} memories:\n{}",
                    results.len(),
                    lines.join("\n")
                ));
            }
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Memory search failed: {}", message)),
        Ok(other) => app
            .messages_state
            .toasts
            .warning(&format!("Unexpected memory search response: {:?}", other)),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Memory search failed: {}", e)),
    }
}

#[allow(dead_code)]
async fn handle_memory_remember(app: &mut app::App, text: String) {
    if text.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-remember <text to remember>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let request = crate::core::new_request(
        format!("memory-remember-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemoryRemember {
            text,
            namespace: Some("user/preferences".to_string()),
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { .. }) | Ok(CoreResponse::Ack) => {
            app.messages_state.toasts.info("Remembered")
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Memory remember failed: {}", message)),
        Ok(other) => app
            .messages_state
            .toasts
            .warning(&format!("Unexpected memory remember response: {:?}", other)),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Memory remember failed: {}", e)),
    }
}

#[allow(dead_code)]
async fn handle_memory_forget(app: &mut app::App, id: String) {
    if id.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-forget <id>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let request = crate::core::new_request(
        format!("memory-forget-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemoryForget { id: id.clone() },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { data }) => {
            let deleted = data
                .get("deleted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if deleted {
                app.messages_state.toasts.info("Memory deleted");
            } else {
                app.messages_state
                    .toasts
                    .warning(&format!("Memory '{}' not found", id));
            }
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Memory forget failed: {}", message)),
        Ok(other) => app
            .messages_state
            .toasts
            .warning(&format!("Unexpected memory forget response: {:?}", other)),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Memory forget failed: {}", e)),
    }
}

fn start_memory_summary(app: &mut app::App) {
    let core_client = app.core_client.clone();
    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "memory_summary", async move {
        let Some(core_client) = core_client else {
            return Some(TuiCommand::MemoryResult {
                toast_message: "Core client unavailable".to_string(),
                is_error: true,
            });
        };

        let project_hash = format!("{:x}", md5::compute(project_dir.as_bytes()));
        let project_namespace = format!("project/{}", project_hash);
        let req_prefs = crate::core::new_request(
            format!("memory-list-{}", uuid::Uuid::new_v4()),
            CoreRequest::MemoryList {
                namespace: "user/preferences".to_string(),
            },
        );
        let req_proj = crate::core::new_request(
            format!("memory-list-{}", uuid::Uuid::new_v4()),
            CoreRequest::MemoryList {
                namespace: project_namespace.clone(),
            },
        );
        let prefs = match core_client.request(req_prefs).await {
            Ok(CoreResponse::Json { data }) => data
                .get("memories")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let proj = match core_client.request(req_proj).await {
            Ok(CoreResponse::Json { data }) => data
                .get("memories")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let total = prefs.len() + proj.len();
        if total == 0 {
            return Some(TuiCommand::MemoryResult {
                toast_message: "No memories yet. Use /memory-remember <text> to save something."
                    .to_string(),
                is_error: false,
            });
        }
        let mut lines = vec![format!("Memory Summary ({} total):", total)];
        if !prefs.is_empty() {
            lines.push(format!("  user/preferences ({}):", prefs.len()));
            for m in prefs.iter().take(5) {
                let id = m
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .chars()
                    .take(8)
                    .collect::<String>();
                let title = m
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(untitled)");
                lines.push(format!("    - [{}] {}", id, title));
            }
        }
        if !proj.is_empty() {
            lines.push(format!("  {} ({}):", project_namespace, proj.len()));
            for m in proj.iter().take(5) {
                let id = m
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .chars()
                    .take(8)
                    .collect::<String>();
                let title = m
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(untitled)");
                lines.push(format!("    - [{}] {}", id, title));
            }
        }
        Some(TuiCommand::MemoryResult {
            toast_message: lines.join("\n"),
            is_error: false,
        })
    });
}

fn start_memory_search(app: &mut app::App, query: String) {
    if query.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-search <query>");
        return;
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "memory_search", async move {
        let Some(core_client) = core_client else {
            return Some(TuiCommand::MemoryResult {
                toast_message: "Core client unavailable".to_string(),
                is_error: true,
            });
        };

        let request = crate::core::new_request(
            format!("memory-search-{}", uuid::Uuid::new_v4()),
            CoreRequest::MemorySearch {
                query: query.clone(),
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { data }) => {
                let results = data
                    .get("memories")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if results.is_empty() {
                    return Some(TuiCommand::MemoryResult {
                        toast_message: format!("No memories found matching '{}'", query),
                        is_error: false,
                    });
                }
                let lines: Vec<String> = results
                    .iter()
                    .take(10)
                    .map(|m| {
                        let id = m
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .chars()
                            .take(8)
                            .collect::<String>();
                        let title = m
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(untitled)");
                        format!("- [{}] {}", id, title)
                    })
                    .collect();
                Some(TuiCommand::MemoryResult {
                    toast_message: format!(
                        "Found {} memories:\n{}",
                        results.len(),
                        lines.join("\n")
                    ),
                    is_error: false,
                })
            }
            Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::MemoryResult {
                toast_message: format!("Memory search failed: {}", message),
                is_error: true,
            }),
            Ok(other) => Some(TuiCommand::MemoryResult {
                toast_message: format!("Unexpected memory search response: {:?}", other),
                is_error: true,
            }),
            Err(e) => Some(TuiCommand::MemoryResult {
                toast_message: format!("Memory search failed: {}", e),
                is_error: true,
            }),
        }
    });
}

fn start_memory_remember(app: &mut app::App, text: String) {
    if text.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-remember <text to remember>");
        return;
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "memory_remember", async move {
        let Some(core_client) = core_client else {
            return Some(TuiCommand::MemoryResult {
                toast_message: "Core client unavailable".to_string(),
                is_error: true,
            });
        };

        let request = crate::core::new_request(
            format!("memory-remember-{}", uuid::Uuid::new_v4()),
            CoreRequest::MemoryRemember {
                text,
                namespace: Some("user/preferences".to_string()),
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { .. }) | Ok(CoreResponse::Ack) => {
                Some(TuiCommand::MemoryResult {
                    toast_message: "Remembered".to_string(),
                    is_error: false,
                })
            }
            Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::MemoryResult {
                toast_message: format!("Memory remember failed: {}", message),
                is_error: true,
            }),
            Ok(other) => Some(TuiCommand::MemoryResult {
                toast_message: format!("Unexpected memory remember response: {:?}", other),
                is_error: true,
            }),
            Err(e) => Some(TuiCommand::MemoryResult {
                toast_message: format!("Memory remember failed: {}", e),
                is_error: true,
            }),
        }
    });
}

fn start_memory_forget(app: &mut app::App, id: String) {
    if id.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-forget <id>");
        return;
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "memory_forget", async move {
        let Some(core_client) = core_client else {
            return Some(TuiCommand::MemoryResult {
                toast_message: "Core client unavailable".to_string(),
                is_error: true,
            });
        };

        let request = crate::core::new_request(
            format!("memory-forget-{}", uuid::Uuid::new_v4()),
            CoreRequest::MemoryForget { id: id.clone() },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { data }) => {
                let deleted = data
                    .get("deleted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if deleted {
                    Some(TuiCommand::MemoryResult {
                        toast_message: "Memory deleted".to_string(),
                        is_error: false,
                    })
                } else {
                    Some(TuiCommand::MemoryResult {
                        toast_message: format!("Memory '{}' not found", id),
                        is_error: false,
                    })
                }
            }
            Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::MemoryResult {
                toast_message: format!("Memory forget failed: {}", message),
                is_error: true,
            }),
            Ok(other) => Some(TuiCommand::MemoryResult {
                toast_message: format!("Unexpected memory forget response: {:?}", other),
                is_error: true,
            }),
            Err(e) => Some(TuiCommand::MemoryResult {
                toast_message: format!("Memory forget failed: {}", e),
                is_error: true,
            }),
        }
    });
}

fn apply_memory_result(app: &mut app::App, toast_message: String, is_error: bool) {
    if is_error {
        app.messages_state.toasts.error(&toast_message);
    } else {
        app.messages_state.toasts.info(&toast_message);
    }
}

async fn handle_goal_set(
    app: &mut app::App,
    session_id: String,
    project_id: String,
    objective: String,
) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    tokio::spawn(async move {
        let request = crate::core::new_request(
            format!("goal-set-{}", uuid::Uuid::new_v4()),
            CoreRequest::GoalSet {
                session_id,
                project_id,
                objective,
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { data }) => {
                let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("Goal");
                let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                tracing::info!("Goal created: {} ({})", title, id);
            }
            Ok(CoreResponse::Error { message, .. }) => {
                tracing::warn!("Goal set failed: {}", message);
            }
            Err(e) => {
                tracing::warn!("Goal set error: {}", e);
            }
            _ => {}
        }
    });
}

async fn handle_goal_from_file(
    app: &mut app::App,
    session_id: String,
    project_id: String,
    path: String,
) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    tokio::spawn(async move {
        let request = crate::core::new_request(
            format!("goal-from-file-{}", uuid::Uuid::new_v4()),
            CoreRequest::GoalFromFile {
                session_id,
                project_id,
                path,
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { data }) => {
                let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("Goal");
                let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                tracing::info!("Goal from file: {} ({})", title, id);
            }
            Ok(CoreResponse::Error { message, .. }) => {
                tracing::warn!("Goal from-file failed: {}", message);
            }
            Err(e) => {
                tracing::warn!("Goal from-file error: {}", e);
            }
            _ => {}
        }
    });
}

async fn handle_goal_show(app: &mut app::App, session_id: String) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let response = core_client
        .request(crate::core::new_request(
            format!("goal-show-{}", uuid::Uuid::new_v4()),
            CoreRequest::GoalShow { session_id },
        ))
        .await;
    match response {
        Ok(CoreResponse::Json { data }) => {
            if data.get("active").and_then(|v| v.as_bool()) == Some(false) {
                app.messages_state.toasts.info("No active goal");
            } else if let Some(rendered) = data.get("rendered").and_then(|v| v.as_str()) {
                app.messages_state.toasts.info(rendered);
            }
        }
        Ok(CoreResponse::Error { message, .. }) => {
            app.messages_state.toasts.warning(&message);
        }
        Err(e) => {
            app.messages_state
                .toasts
                .warning(&format!("Goal show error: {}", e));
        }
        _ => {}
    }
}

async fn handle_goal_simple(app: &mut app::App, request: CoreRequest, label: &str) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let label = label.to_string();
    tokio::spawn(async move {
        let response = core_client
            .request(crate::core::new_request(
                format!("goal-{}-{}", label, uuid::Uuid::new_v4()),
                request,
            ))
            .await;
        match response {
            Ok(CoreResponse::Json { data }) => {
                tracing::info!("Goal {} response: {}", label, data);
            }
            Ok(CoreResponse::Error { message, .. }) => {
                tracing::warn!("Goal {} failed: {}", label, message);
            }
            Err(e) => {
                tracing::warn!("Goal {} error: {}", label, e);
            }
            _ => {}
        }
    });
}

async fn handle_refresh_session_state(app: &mut app::App, session_id: String) {
    let Some(core_client) = app.core_client.clone() else {
        return;
    };
    // Hydrate the todo list.
    if let Ok(CoreResponse::Json { data }) = core_client
        .request(crate::core::new_request(
            format!("todo-list-{}", uuid::Uuid::new_v4()),
            CoreRequest::TodoList {
                session_id: session_id.clone(),
            },
        ))
        .await
    {
        if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
            let entries: Vec<crate::tui::app::TodoEntry> = items
                .iter()
                .filter_map(|v| {
                    Some(crate::tui::app::TodoEntry {
                        content: v.get("content")?.as_str()?.to_string(),
                        status: v.get("status")?.as_str()?.to_string(),
                        priority: v.get("priority")?.as_str()?.to_string(),
                    })
                })
                .collect();
            app.set_todos(entries);
        }
    }
    // Hydrate the active goal.
    if let Ok(CoreResponse::Json { data }) = core_client
        .request(crate::core::new_request(
            format!("active-goal-{}", uuid::Uuid::new_v4()),
            CoreRequest::ActiveGoalLoad {
                session_id: session_id.clone(),
            },
        ))
        .await
    {
        if data.get("active").and_then(|v| v.as_bool()) == Some(true) {
            if let Some(goal_val) = data.get("goal") {
                if let Ok(snap) =
                    serde_json::from_value::<crate::bus::events::GoalSnapshot>(goal_val.clone())
                {
                    app.set_active_goal(Some(snap));
                }
            }
        } else {
            app.set_active_goal(None);
        }
    }
}

async fn handle_goal_checkpoint(app: &mut app::App, session_id: String, project_id: String) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let response = core_client
        .request(crate::core::new_request(
            format!("goal-checkpoint-{}", uuid::Uuid::new_v4()),
            CoreRequest::GoalCheckpoint {
                session_id,
                project_id,
            },
        ))
        .await;
    match response {
        Ok(CoreResponse::Json { data }) => {
            if let Some(path) = data.get("checkpoint_path").and_then(|v| v.as_str()) {
                tracing::info!("Goal checkpoint: {}", path);
            }
        }
        Ok(CoreResponse::Error { message, .. }) => {
            tracing::warn!("Goal checkpoint failed: {}", message);
        }
        Err(e) => {
            tracing::warn!("Goal checkpoint error: {}", e);
        }
        _ => {}
    }
}

/// `/goal budget …` handler. Two flavors:
///   * `show` — fetch the active goal and render its budget/usage as
///     a toast (sidebar already shows live status).
///   * `raise <axis> <n>` — bump a single axis of the active goal's
///     budget. Valid axes: `tokens`, `turns`, `tool-calls`, `wallclock`.
async fn handle_goal_budget(app: &mut app::App, session_id: String, subcommand: String) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };

    let trimmed = subcommand.trim();
    let mut parts = trimmed.splitn(3, ' ');
    let head = parts.next().unwrap_or("").to_string();
    let rest = parts.collect::<Vec<_>>().join(" ").trim().to_string();

    if head == "show" {
        // Render a compact budget/usage summary by reading the live
        // GoalSnapshot already on the app, if any.
        if let Some(ref g) = app.active_goal {
            let line = crate::tui::app::format_goal_status_line(g);
            app.messages_state.toasts.info(&line);
        } else {
            app.messages_state
                .toasts
                .warning("No active goal — set one with /goal set <objective>");
        }
        return;
    }

    if head == "raise" {
        let mut parts = rest.splitn(2, ' ');
        let axis = parts.next().unwrap_or("").to_string();
        let value_str = parts.next().unwrap_or("").trim();
        if axis.is_empty() || value_str.is_empty() {
            app.messages_state
                .toasts
                .warning("Usage: /goal budget raise <tokens|turns|tool-calls|wallclock> <n>");
            return;
        }
        let value: i64 = match value_str.parse() {
            Ok(v) => v,
            Err(_) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Invalid number: {}", value_str));
                return;
            }
        };
        if value < 1 {
            app.messages_state
                .toasts
                .warning("Budget must be a positive integer");
            return;
        }
        let mut max_turns: Option<i64> = None;
        let mut max_model_tokens: Option<i64> = None;
        let mut max_tool_calls: Option<i64> = None;
        let mut max_wallclock_secs: Option<i64> = None;
        let mut label = axis.clone();
        match axis.as_str() {
            "tokens" => max_model_tokens = Some(value),
            "turns" => max_turns = Some(value),
            "tool-calls" | "tool_calls" | "calls" => {
                max_tool_calls = Some(value);
                label = "tool-calls".into();
            }
            "wallclock" | "wall" => max_wallclock_secs = Some(value),
            _ => {
                app.messages_state.toasts.warning(&format!(
                    "Unknown axis '{}'. Use tokens, turns, tool-calls, or wallclock.",
                    axis
                ));
                return;
            }
        }
        let response = core_client
            .request(crate::core::new_request(
                format!("goal-budget-raise-{}", uuid::Uuid::new_v4()),
                CoreRequest::GoalSetBudget {
                    session_id: session_id.clone(),
                    max_turns,
                    max_model_tokens,
                    max_tool_calls,
                    max_wallclock_secs,
                },
            ))
            .await;
        match response {
            Ok(CoreResponse::Json { data }) => {
                let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("ok");
                app.messages_state.toasts.info(&format!(
                    "Goal budget: {} = {} (status: {})",
                    label, value, status
                ));
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Budget update failed: {}", message));
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Budget update error: {}", e));
            }
            _ => {}
        }
        return;
    }

    app.messages_state
        .toasts
        .warning("Usage: /goal budget [show | raise <axis> <n>]");
}

async fn handle_task_schedule(app: &mut app::App, interval_secs: u64, message: String) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let session_id = app
        .session_state
        .session
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    let request = crate::core::new_request(
        format!("task-schedule-{}", uuid::Uuid::new_v4()),
        CoreRequest::TaskSchedule {
            session_id,
            interval_secs,
            message,
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { data }) => {
            let task_id = data
                .get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            app.messages_state.toasts.info(&format!(
                "Task {} scheduled (every {}s)",
                task_id.chars().take(8).collect::<String>(),
                interval_secs
            ));
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Failed to schedule task: {}", message)),
        Ok(other) => app
            .messages_state
            .toasts
            .warning(&format!("Unexpected task schedule response: {:?}", other)),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Failed to schedule task: {}", e)),
    }
}

async fn handle_worktree_list(app: &mut app::App) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let request = crate::core::new_request(
        format!("worktree-list-{}", uuid::Uuid::new_v4()),
        CoreRequest::WorktreeList {
            project_dir: app.session_state.project_dir.clone(),
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { data }) => {
            let trees = data
                .get("worktrees")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if trees.is_empty() {
                app.messages_state.toasts.info("No worktrees found");
            } else {
                let names: Vec<String> = trees
                    .iter()
                    .map(|t| {
                        let path = t.get("path").and_then(|v| v.as_str()).unwrap_or_default();
                        let branch = t.get("branch").and_then(|v| v.as_str()).unwrap_or_default();
                        format!("{} ({})", path, branch)
                    })
                    .collect();
                app.messages_state.toasts.info(&names.join(", "));
            }
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Failed to list worktrees: {}", message)),
        Ok(other) => app
            .messages_state
            .toasts
            .warning(&format!("Unexpected worktree response: {:?}", other)),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Failed to list worktrees: {}", e)),
    }
}

async fn handle_compact_session(app: &mut app::App) {
    if app.session_state.session_status == SessionStatus::Working {
        app.messages_state
            .toasts
            .info("Compaction will occur at end of current turn");
    } else {
        app.messages_state
            .toasts
            .info("Compaction happens automatically during processing");
    }
}

async fn handle_open_diff_dialog(
    app: &mut app::App,
    old_content: Box<str>,
    new_content: Box<str>,
    title: Box<str>,
) {
    let mut dialog =
        crate::tui::components::dialogs::diff::DiffDialog::new(old_content, new_content, title);
    dialog.set_theme(&app.ui_state.theme);
    app.dialog_state.diff_dialog = Some(dialog);
    app.open_dialog(Dialog::Diff);
}

async fn handle_send_notification(
    app: &mut app::App,
    notification_type: crate::tui::components::notification::NotificationType,
    body: String,
) {
    if let Some(ref notification_mgr) = app.notification_manager {
        if let Err(e) = notification_mgr.send(notification_type, &body).await {
            tracing::warn!("Failed to send notification: {}", e);
        }
    }
}

#[allow(dead_code)]
async fn handle_run_doctor(app: &mut app::App) {
    use crate::search_backend::bootstrap;
    let config = match crate::config::schema::Config::load() {
        Ok(c) => c,
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("doctor: failed to load config: {e}"));
            return;
        }
    };
    let (_svc, report) = bootstrap::bootstrap_search_backend(&config).await;
    let summary = if report.connected {
        format!(
            "doctor: {} OK ({})",
            report.search_backend.as_deref().unwrap_or("?"),
            report.tools.join(", ")
        )
    } else if let Some(err) = &report.connection_error {
        format!(
            "doctor: {} unavailable ({err})",
            report.search_backend.as_deref().unwrap_or("?")
        )
    } else {
        format!(
            "doctor: {} (no MCP service)",
            report.search_backend.as_deref().unwrap_or("?")
        )
    };
    for line in report.summary_lines() {
        tracing::info!(target: "codegg::doctor", "{}", line);
    }
    if let Some(mcp) = config.mcp.as_ref() {
        tracing::info!(target: "codegg::doctor", "MCP servers: {}", mcp.len());
    }
    app.messages_state.toasts.info(&summary);
}

fn start_run_doctor(app: &mut app::App) {
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "run_doctor", async move {
        use crate::search_backend::bootstrap;
        let config = match crate::config::schema::Config::load() {
            Ok(c) => c,
            Err(e) => {
                return Some(TuiCommand::DoctorResult {
                    summary: format!("doctor: failed to load config: {e}"),
                    is_error: true,
                });
            }
        };
        let (_svc, report) = bootstrap::bootstrap_search_backend(&config).await;
        let summary = if report.connected {
            format!(
                "doctor: {} OK ({})",
                report.search_backend.as_deref().unwrap_or("?"),
                report.tools.join(", ")
            )
        } else if let Some(err) = &report.connection_error {
            format!(
                "doctor: {} unavailable ({err})",
                report.search_backend.as_deref().unwrap_or("?")
            )
        } else {
            format!(
                "doctor: {} (no MCP service)",
                report.search_backend.as_deref().unwrap_or("?")
            )
        };
        for line in report.summary_lines() {
            tracing::info!(target: "codegg::doctor", "{}", line);
        }
        if let Some(mcp) = config.mcp.as_ref() {
            tracing::info!(target: "codegg::doctor", "MCP servers: {}", mcp.len());
        }
        Some(TuiCommand::DoctorResult {
            summary,
            is_error: false,
        })
    });
}

fn apply_doctor_result(app: &mut app::App, summary: String, is_error: bool) {
    if is_error {
        app.messages_state.toasts.error(&summary);
    } else {
        app.messages_state.toasts.info(&summary);
    }
}

async fn handle_security_review_run(
    app: &mut app::App,
    id: String,
    root: std::path::PathBuf,
    args: crate::security::workflow::SecurityReviewCommandArgs,
    lsp_tool: Option<std::sync::Arc<crate::tool::lsp::LspTool>>,
) {
    let result =
        crate::security::workflow::run_security_review_background(root, args, lsp_tool).await;

    // Always clear the reentrancy guard, even on failure.
    if app.security_review_run_id() == Some(id.as_str()) {
        app.security_review_running = None;
    }

    match result {
        Ok(receipt) => apply_security_review_receipt(app, receipt),
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("Security review failed: {e}"));
        }
    }
}

/// Apply a completed security review to the App: store the latest
/// receipt, push the rendered report into the message timeline, and
/// surface a success toast. Shared by the inline `SecurityReviewRun`
/// handler and the `SecurityReviewFinished` completion arm.
fn apply_security_review_receipt(
    app: &mut app::App,
    receipt: crate::security::workflow::SecurityReviewReceipt,
) {
    use crate::tui::components::messages::{MessageRole, MsgPart, UIMessage};
    let open_panel = receipt.args.open_panel_on_complete;
    let labeled = format!("[Security Review]\n{}", receipt.rendered_report);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    app.messages_state.messages.messages.push(UIMessage {
        role: MessageRole::Assistant,
        parts: vec![MsgPart::Text { content: labeled }],
        timestamp: Some(timestamp),
        is_plan_mode: None,
    });
    app.messages_state.messages.scroll_to_bottom();
    app.set_latest_security_review(receipt);
    if open_panel {
        app.open_dialog(crate::tui::Dialog::SecurityReview);
        app.messages_state
            .toasts
            .success("Security review complete — result panel opened.");
    } else {
        app.messages_state.toasts.success(
            "Security review complete — run /security-review-show to open the result panel.",
        );
    }
}

fn handle_run_human_shell(app: &mut app::App, command: String, promote_after: bool) {
    use crate::shell::policy::evaluate_command;

    let policy = evaluate_command(&command);
    match policy {
        crate::shell::policy::HumanShellPolicyDecision::Block { reason } => {
            app.messages_state
                .toasts
                .error(&format!("Blocked: {}", reason));
            return;
        }
        crate::shell::policy::HumanShellPolicyDecision::Warn { reason } => {
            let confirm_enabled = crate::config::schema::Config::load()
                .ok()
                .and_then(|c| c.human_shell)
                .map(|h| h.confirm_dangerous())
                .unwrap_or(true);
            if confirm_enabled {
                app.dialog_state.pending_shell_command = Some((command, promote_after));
                let title = "Dangerous Command".to_string();
                let msg = format!("{}\n\nRun this command anyway?", reason);
                app.ui_state.dialog = crate::tui::Dialog::Confirm;
                app.focus_manager.push(Box::new(
                    crate::tui::components::dialogs::confirm::ConfirmDialog::new(title, msg),
                ));
                return;
            } else {
                app.messages_state
                    .toasts
                    .warning(&format!("Warning: {}", reason));
            }
        }
        crate::shell::policy::HumanShellPolicyDecision::Allow => {}
    }

    spawn_human_shell(app, command, promote_after);
}

fn spawn_human_shell(app: &mut app::App, command: String, promote_after: bool) {
    use crate::shell::types::{ShellCapturePolicy, ShellEnvPolicy, ShellOrigin, ShellRequest};

    let id = app.shell_store.alloc_id();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let capture_policy = if promote_after {
        ShellCapturePolicy::StoreAndPromote
    } else {
        ShellCapturePolicy::StoreEphemeral
    };
    let req = ShellRequest {
        id,
        origin: ShellOrigin::HumanEphemeral,
        command: command.clone(),
        cwd: cwd.clone(),
        timeout: std::time::Duration::from_secs(crate::shell::DEFAULT_TIMEOUT_SECS),
        capture_policy,
        env_policy: ShellEnvPolicy::Inherit,
    };
    app.shell_store.insert_started(&req);

    app.messages_state
        .messages
        .add_shell_cell(id.0, &command, &cwd.to_string_lossy());

    let (tx, mut rx) = tokio::sync::mpsc::channel(128);
    let runtime = crate::shell::ShellRuntime::new();
    let tui_cmd_tx = app.tui_cmd_tx.clone();
    tokio::spawn(async move {
        match runtime.spawn(req, tx.clone()).await {
            Ok(_handle) => {
                if let Some(ref ttx) = tui_cmd_tx {
                    let _ = ttx.try_send(app::TuiCommand::ShellEvent(
                        crate::shell::ShellEvent::Started { id, command, cwd },
                    ));
                }
                while let Some(event) = rx.recv().await {
                    if let Some(ref ttx) = tui_cmd_tx {
                        let _ = ttx.try_send(app::TuiCommand::ShellEvent(event));
                    }
                }
            }
            Err(e) => {
                if let Some(ref ttx) = tui_cmd_tx {
                    let _ = ttx.try_send(app::TuiCommand::ShellEvent(
                        crate::shell::ShellEvent::FailedToStart { id, error: e },
                    ));
                }
            }
        }
    });
}

fn handle_shell_event(app: &mut app::App, event: crate::shell::ShellEvent) {
    match &event {
        crate::shell::ShellEvent::Started { id, .. } => {
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some("running".to_string());
            });
        }
        crate::shell::ShellEvent::Stdout { id, bytes } => {
            app.shell_store.append_stdout(*id, bytes);
            let entry = app.shell_store.get(*id);
            let preview = entry.map(|e| e.stdout.head_str_lossy()).unwrap_or_default();
            let preview_lines: Vec<&str> = preview.lines().rev().take(8).collect();
            let stdout_preview: Vec<&str> = preview_lines.into_iter().rev().collect();
            let stdout_preview = stdout_preview.join("\n");
            let truncated = entry.map(|e| e.stdout.omitted_bytes > 0).unwrap_or(false);
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.stdout_preview = Some(stdout_preview);
                cell.truncated = Some(truncated);
            });
        }
        crate::shell::ShellEvent::Stderr { id, bytes } => {
            app.shell_store.append_stderr(*id, bytes);
            let entry = app.shell_store.get(*id);
            let preview = entry.map(|e| e.stderr.head_str_lossy()).unwrap_or_default();
            let preview_lines: Vec<&str> = preview.lines().rev().take(8).collect();
            let stderr_preview: Vec<&str> = preview_lines.into_iter().rev().collect();
            let stderr_preview = stderr_preview.join("\n");
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.stderr_preview = Some(stderr_preview);
            });
        }
        crate::shell::ShellEvent::Exited {
            id,
            status,
            elapsed,
        } => {
            app.shell_store.mark_exited(*id, *status, *elapsed);
            let elapsed_ms = elapsed.as_millis() as u64;
            let exit_code = *status;
            let status_str = "exited".to_string();
            let entry = app.shell_store.get(*id);
            let stdout_preview = entry.map(|e| e.stdout.head_str_lossy()).unwrap_or_default();
            let stderr_preview = entry.map(|e| e.stderr.head_str_lossy()).unwrap_or_default();
            let truncated = entry.map(|e| e.stdout.omitted_bytes > 0).unwrap_or(false);
            let command = entry.map(|e| e.command.clone()).unwrap_or_default();
            let _cwd = entry
                .map(|e| e.cwd.to_string_lossy().to_string())
                .unwrap_or_default();
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some(status_str);
                cell.elapsed_ms = Some(elapsed_ms);
                cell.exit_code = exit_code;
                cell.stdout_preview = Some(stdout_preview);
                cell.stderr_preview = Some(stderr_preview);
                cell.truncated = Some(truncated);
            });

            let should_promote = entry
                .map(|e| e.promote_after && !e.promoted)
                .unwrap_or(false);
            if should_promote {
                if let Some(entry) = app.shell_store.get(*id) {
                    let digest = crate::shell::ShellDigest::build(
                        &command,
                        &entry.cwd,
                        exit_code,
                        *elapsed,
                        &entry.stdout,
                        &entry.stderr,
                    );
                    let include_text = if digest.has_failures() {
                        format!(
                            "Shell command output (auto-promoted on failure):\n{}",
                            digest.render()
                        )
                    } else {
                        let tail = entry.stderr.tail_str_lossy();
                        if tail.is_empty() {
                            let tail = entry.stdout.tail_str_lossy();
                            format!(
                                "Shell command output (auto-promoted):\n$ {}\n\n{}",
                                command, tail
                            )
                        } else {
                            format!(
                                "Shell command output (auto-promoted):\n$ {}\n\nstderr:\n{}",
                                command, tail
                            )
                        }
                    };
                    app.messages_state
                        .messages
                        .add_user_message(include_text, Some(false));
                    app.shell_store.mark_promoted(*id);
                    app.messages_state
                        .toasts
                        .info("Shell output auto-promoted to context");
                }
            }
        }
        crate::shell::ShellEvent::TimedOut { id, elapsed } => {
            app.shell_store.mark_timeout(*id, *elapsed);
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some("timed_out".to_string());
                cell.elapsed_ms = Some(elapsed.as_millis() as u64);
            });
        }
        crate::shell::ShellEvent::FailedToStart { id, error } => {
            app.shell_store.mark_failed_to_start(*id);
            app.messages_state.messages.update_shell_cell(id.0, |cell| {
                cell.status = Some("failed".to_string());
                cell.stderr_preview = Some(format!("Failed to start: {}", error));
            });
        }
    }
}

fn handle_shell_include(app: &mut app::App, id: u64, mode: String, _question: Option<String>) {
    use crate::shell::types::{ShellCommandId, ShellPromotionMode};

    let cmd_id = ShellCommandId(id);
    if let Some(entry) = app.shell_store.get(cmd_id) {
        let command = entry.command.clone();
        let cwd = entry.cwd.clone();
        let exit_code = match entry.status {
            crate::shell::types::ShellStatus::Exited => Some(0),
            _ => None,
        };
        let elapsed = entry.elapsed.unwrap_or_default();
        let stdout = &entry.stdout;
        let stderr = &entry.stderr;

        let promotion = ShellPromotionMode::parse(&mode);
        let include_text = match promotion {
            ShellPromotionMode::Tail { lines } => {
                let stderr_text = stderr.head_str_lossy();
                let all_lines: Vec<&str> = stderr_text.lines().collect();
                let tail: Vec<&str> = all_lines.iter().rev().take(lines).rev().copied().collect();
                format!(
                    "Shell output (tail {} lines) for `{}`:\n{}",
                    lines,
                    command,
                    tail.join("\n")
                )
            }
            ShellPromotionMode::StdoutOnly => {
                let digest = crate::shell::ShellDigest::build(
                    &command, &cwd, exit_code, elapsed, stdout, stderr,
                );
                if digest.has_failures() {
                    format!(
                        "Shell output (stdout + failures) for `{}`:\n{}",
                        command,
                        digest.render()
                    )
                } else {
                    format!(
                        "Shell output (stdout) for `{}`:\n{}",
                        command,
                        stdout.head_str_lossy()
                    )
                }
            }
            ShellPromotionMode::StderrOnly => {
                format!(
                    "Shell output (stderr) for `{}`:\n{}",
                    command,
                    stderr.head_str_lossy()
                )
            }
            ShellPromotionMode::Summary => {
                let digest = crate::shell::ShellDigest::build(
                    &command, &cwd, exit_code, elapsed, stdout, stderr,
                );
                format!(
                    "Shell output (summary) for `{}`:\n{}",
                    command,
                    digest.render()
                )
            }
            ShellPromotionMode::FailureDigest => {
                let digest = crate::shell::ShellDigest::build(
                    &command, &cwd, exit_code, elapsed, stdout, stderr,
                );
                if digest.has_failures() {
                    format!(
                        "Shell output (failure digest) for `{}`:\n{}",
                        command,
                        digest.render()
                    )
                } else {
                    format!(
                        "Shell output for `{}`:\nstdout:\n{}\nstderr:\n{}",
                        command,
                        stdout.head_str_lossy(),
                        stderr.head_str_lossy()
                    )
                }
            }
            ShellPromotionMode::Full => {
                let digest = crate::shell::ShellDigest::build(
                    &command, &cwd, exit_code, elapsed, stdout, stderr,
                );
                if digest.has_failures() {
                    format!("Shell output for `{}`:\n{}", command, digest.render())
                } else {
                    format!(
                        "Shell output for `{}`:\nstdout:\n{}\nstderr:\n{}",
                        command,
                        stdout.head_str_lossy(),
                        stderr.head_str_lossy()
                    )
                }
            }
        };
        app.shell_store.mark_promoted(cmd_id);
        app.messages_state
            .messages
            .add_user_message(include_text, Some(false));
        app.messages_state
            .toasts
            .info("Shell output included in context");
    } else {
        app.messages_state
            .toasts
            .error(&format!("Shell command {} not found", id));
    }
}

fn handle_shell_ask(app: &mut app::App, id: u64, question: String) {
    use crate::shell::types::ShellCommandId;

    let cmd_id = ShellCommandId(id);
    if let Some(entry) = app.shell_store.get(cmd_id) {
        let command = entry.command.clone();
        let cwd = entry.cwd.clone();
        let exit_code = match entry.status {
            crate::shell::types::ShellStatus::Exited => Some(0),
            _ => None,
        };
        let elapsed = entry.elapsed.unwrap_or_default();
        let digest = crate::shell::ShellDigest::build(
            &command,
            &cwd,
            exit_code,
            elapsed,
            &entry.stdout,
            &entry.stderr,
        );
        let include_text = format!(
            "Using the attached shell output, answer: {}\n\n{}",
            question,
            digest.render()
        );
        app.shell_store.mark_promoted(cmd_id);
        app.messages_state
            .messages
            .add_user_message(include_text, Some(false));
        app.messages_state
            .toasts
            .info("Shell output and question included in context");
    } else {
        app.messages_state
            .toasts
            .error(&format!("Shell command {} not found", id));
    }
}

fn handle_shell_rerun(app: &mut app::App, id: u64) {
    use crate::shell::types::ShellCommandId;

    let cmd_id = ShellCommandId(id);
    if let Some(entry) = app.shell_store.get(cmd_id) {
        let command = entry.command.clone();
        let promote_after = entry.promote_after;
        if let Some(ref tx) = app.tui_cmd_tx {
            let _ = tx.try_send(app::TuiCommand::RunHumanShell {
                command,
                promote_after,
            });
        }
    } else {
        app.messages_state
            .toasts
            .error(&format!("Shell command {} not found", id));
    }
}

fn handle_shell_kill(app: &mut app::App, id: u64) {
    if let Some(handle) = app.shell_handles.remove(&id) {
        handle.kill();
        app.messages_state
            .toasts
            .info(&format!("Killed shell command {}", id));
    } else {
        app.messages_state
            .toasts
            .error(&format!("No running shell command with id {}", id));
    }
}

fn handle_shell_list(app: &mut app::App) {
    let recent = app.shell_store.list_recent(10);
    if recent.is_empty() {
        app.messages_state
            .toasts
            .info("No shell commands in history");
        return;
    }
    let lines: Vec<String> = recent
        .iter()
        .map(|e| {
            format!(
                "[{}] ${} ({})",
                e.id.0,
                e.command,
                match e.status {
                    crate::shell::types::ShellStatus::Running => "running",
                    crate::shell::types::ShellStatus::Exited => "done",
                    crate::shell::types::ShellStatus::TimedOut => "timeout",
                    crate::shell::types::ShellStatus::FailedToStart => "failed",
                }
            )
        })
        .collect();
    app.messages_state.toasts.info(&lines.join("\n"));
}

fn handle_file_diff_stats_ready(
    app: &mut app::App,
    path: std::path::PathBuf,
    generation: u64,
    result: crate::tui::file_diff::FileDiffStatsResult,
) {
    use crate::tui::app::state::session::DiffStatsState;

    // Find the changed-file entry by path.
    if let Some(entry) = app
        .session_state
        .changed_files
        .iter_mut()
        .find(|f| f.path == path)
    {
        // Ignore stale completions.
        if entry.diff_state.generation() != generation {
            return;
        }
        entry.diff_state = DiffStatsState::from_result(generation, result);
    } else {
        return;
    }

    // Refresh sidebar.
    let changes = app
        .session_state
        .changed_files
        .iter()
        .map(|file| crate::tui::components::sidebar::SidebarFileChange {
            path: file.path.to_string_lossy().into_owned(),
            action: file.action.clone(),
            diff_preview: file.diff_preview.clone(),
            diff_state: file.diff_state.clone(),
        })
        .collect();
    app.sidebar.set_file_changes(changes);
}

/// Handle a `TuiCommand::SecurityReviewFinished` notification from the
/// spawned background task. Stale completions (id mismatch) are
/// silently ignored so cancellation cannot be undone by a late
/// delivery.
fn handle_security_review_finished(
    app: &mut app::App,
    id: String,
    receipt: Option<Box<crate::security::workflow::SecurityReviewReceipt>>,
    error: Option<String>,
) {
    // Stale completion: a different (or cancelled) run is now active.
    if app.security_review_run_id() != Some(id.as_str()) {
        return;
    }
    app.security_review_running = None;
    match (receipt, error) {
        (Some(receipt), None) => {
            apply_security_review_receipt(app, *receipt);
        }
        (_, Some(e)) => {
            app.messages_state
                .toasts
                .error(&format!("Security review failed: {e}"));
        }
        _ => {
            app.messages_state
                .toasts
                .error("Security review failed: no result returned");
        }
    }
}

#[allow(dead_code)]
async fn handle_research_list_runs(app: &mut app::App) {
    let project_dir = app.session_state.project_dir.clone();
    let service =
        crate::research::service::ResearchService::new(std::path::PathBuf::from(&project_dir));
    match service.list_runs().await {
        Ok(runs) => {
            if let Some(ref mut browser) = app.dialog_state.research_browser {
                browser.loading = false;
                browser.set_runs(runs);
            } else {
                app.messages_state
                    .toasts
                    .info("No research browser dialog open");
            }
        }
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("Failed to list research runs: {}", e));
            if let Some(ref mut browser) = app.dialog_state.research_browser {
                browser.loading = false;
            }
        }
    }
}

#[allow(dead_code)]
async fn handle_research_load_run(app: &mut app::App, run_id: String) {
    let project_dir = app.session_state.project_dir.clone();
    let service =
        crate::research::service::ResearchService::new(std::path::PathBuf::from(&project_dir));
    match service.load_run(&run_id).await {
        Ok(bundle) => {
            if let Some(ref mut browser) = app.dialog_state.research_browser {
                browser.loading = false;
                browser.set_bundle(bundle);
            } else {
                app.messages_state
                    .toasts
                    .info("No research browser dialog open");
            }
        }
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("Failed to load research run: {}", e));
            if let Some(ref mut browser) = app.dialog_state.research_browser {
                browser.loading = false;
            }
        }
    }
}

#[allow(dead_code)]
async fn handle_research_load_section(app: &mut app::App, run_id: String, section: String) {
    let project_dir = app.session_state.project_dir.clone();
    let service =
        crate::research::service::ResearchService::new(std::path::PathBuf::from(&project_dir));

    let result = match section.as_str() {
        "Research Plan" => {
            if let Ok(bundle) = service.load_run(&run_id).await {
                if let Some(ref plan) = bundle.plan {
                    let content = format!(
                        "Scope: {}\n\nComparison Axes:\n{}\n\nSource Classes:\n{}\n\nExclusion Criteria:\n{}\n\nStopping Conditions:\n{}\n\nExpected Outputs:\n{}",
                        plan.scope,
                        plan.comparison_axes.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        plan.source_classes.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        plan.exclusion_criteria.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        plan.stopping_conditions.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        plan.expected_outputs.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                    );
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Report,
                        content,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        }
        "Sources" => {
            if let Ok(bundle) = service.load_run(&run_id).await {
                if bundle.sources.is_empty() {
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Brief,
                        "No sources collected.".to_string(),
                    ))
                } else {
                    let lines: Vec<String> = bundle
                        .sources
                        .iter()
                        .enumerate()
                        .map(|(i, src)| {
                            let title = src.title.as_deref().unwrap_or(&src.uri);
                            format!(
                                "{}. {} [{:?}]\n   URI: {}",
                                i + 1,
                                title,
                                src.source_type,
                                src.uri
                            )
                        })
                        .collect();
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Brief,
                        lines.join("\n\n"),
                    ))
                }
            } else {
                None
            }
        }
        "Claims" => {
            if let Ok(bundle) = service.load_run(&run_id).await {
                if bundle.claims.is_empty() {
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::AgentAnswer,
                        "No claims derived.".to_string(),
                    ))
                } else {
                    let lines: Vec<String> = bundle.claims.iter().map(|claim| {
                        format!("[{}] {} (confidence: {:?})\n   Evidence: {} sources\n   Caveats: {}",
                            claim.claim_type.as_str(), claim.text, claim.confidence,
                            claim.evidence_ids.len(),
                            if claim.caveats.is_empty() { "none".to_string() } else { claim.caveats.join("; ") })
                    }).collect();
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::AgentAnswer,
                        lines.join("\n\n"),
                    ))
                }
            } else {
                None
            }
        }
        "Contradictions" => {
            if let Ok(bundle) = service.load_run(&run_id).await {
                if bundle.contradictions.is_empty() {
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Handoff,
                        "No contradictions detected.".to_string(),
                    ))
                } else {
                    let lines: Vec<String> = bundle
                        .contradictions
                        .iter()
                        .map(|c| {
                            format!(
                                "[{:?}] {}\n   Claims: {}",
                                c.severity,
                                c.description,
                                c.claim_ids.join(", ")
                            )
                        })
                        .collect();
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Handoff,
                        lines.join("\n\n"),
                    ))
                }
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(ref mut browser) = app.dialog_state.research_browser {
        if let Some((section, content)) = result {
            browser.set_report_content(section, content);
        } else {
            app.messages_state
                .toasts
                .warning("Could not load section content");
        }
    }
}

fn start_research_list_runs(app: &mut app::App) {
    app.dialog_state.research_request_id += 1;
    let request_id = app.dialog_state.research_request_id;

    if let Some(ref mut browser) = app.dialog_state.research_browser {
        browser.loading = true;
    }

    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "research_list_runs", async move {
        let service =
            crate::research::service::ResearchService::new(std::path::PathBuf::from(&project_dir));
        match service.list_runs().await {
            Ok(runs) => Some(TuiCommand::ResearchRunsLoaded {
                request_id,
                runs,
                error: None,
            }),
            Err(e) => Some(TuiCommand::ResearchRunsLoaded {
                request_id,
                runs: Vec::new(),
                error: Some(format!("Failed to list research runs: {}", e)),
            }),
        }
    });
}

fn apply_research_runs_loaded(
    app: &mut app::App,
    request_id: u64,
    runs: Vec<crate::research::service::ResearchRunSummary>,
    error: Option<String>,
) {
    if request_id != app.dialog_state.research_request_id {
        return;
    }
    if let Some(ref mut browser) = app.dialog_state.research_browser {
        browser.loading = false;
        if let Some(err) = error {
            app.messages_state.toasts.error(&err);
        } else {
            browser.set_runs(runs);
        }
    }
}

fn start_research_load_run(app: &mut app::App, run_id: String) {
    app.dialog_state.research_request_id += 1;
    let request_id = app.dialog_state.research_request_id;

    if let Some(ref mut browser) = app.dialog_state.research_browser {
        browser.loading = true;
    }

    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "research_load_run", async move {
        let service =
            crate::research::service::ResearchService::new(std::path::PathBuf::from(&project_dir));
        match service.load_run(&run_id).await {
            Ok(bundle) => Some(TuiCommand::ResearchRunLoaded {
                request_id,
                run_id,
                bundle: Some(Box::new(bundle)),
                error: None,
            }),
            Err(e) => Some(TuiCommand::ResearchRunLoaded {
                request_id,
                run_id,
                bundle: None,
                error: Some(format!("Failed to load research run: {}", e)),
            }),
        }
    });
}

fn apply_research_run_loaded(
    app: &mut app::App,
    request_id: u64,
    _run_id: String,
    bundle: Option<Box<crate::research::types::ResearchBundle>>,
    error: Option<String>,
) {
    if request_id != app.dialog_state.research_request_id {
        return;
    }
    if let Some(ref mut browser) = app.dialog_state.research_browser {
        browser.loading = false;
        if let Some(err) = error {
            app.messages_state.toasts.error(&err);
        } else if let Some(bundle) = bundle {
            browser.set_bundle(*bundle);
        }
    }
}

fn start_research_load_section(app: &mut app::App, run_id: String, section: String) {
    app.dialog_state.research_request_id += 1;
    let request_id = app.dialog_state.research_request_id;

    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_tui_task(tx, "research_load_section", async move {
        let service =
            crate::research::service::ResearchService::new(std::path::PathBuf::from(&project_dir));

        let result = match section.as_str() {
            "Research Plan" => {
                if let Ok(bundle) = service.load_run(&run_id).await {
                    if let Some(ref plan) = bundle.plan {
                        let content = format!(
                            "Scope: {}\n\nComparison Axes:\n{}\n\nSource Classes:\n{}\n\nExclusion Criteria:\n{}\n\nStopping Conditions:\n{}\n\nExpected Outputs:\n{}",
                            plan.scope,
                            plan.comparison_axes.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                            plan.source_classes.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                            plan.exclusion_criteria.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                            plan.stopping_conditions.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                            plan.expected_outputs.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        );
                        Some((
                            crate::tui::components::dialogs::research::ReportSection::Report,
                            content,
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            "Sources" => {
                if let Ok(bundle) = service.load_run(&run_id).await {
                    if bundle.sources.is_empty() {
                        Some((
                            crate::tui::components::dialogs::research::ReportSection::Brief,
                            "No sources collected.".to_string(),
                        ))
                    } else {
                        let lines: Vec<String> = bundle
                            .sources
                            .iter()
                            .enumerate()
                            .map(|(i, src)| {
                                let title = src.title.as_deref().unwrap_or(&src.uri);
                                format!(
                                    "{}. {} [{:?}]\n   URI: {}",
                                    i + 1,
                                    title,
                                    src.source_type,
                                    src.uri
                                )
                            })
                            .collect();
                        Some((
                            crate::tui::components::dialogs::research::ReportSection::Brief,
                            lines.join("\n\n"),
                        ))
                    }
                } else {
                    None
                }
            }
            "Claims" => {
                if let Ok(bundle) = service.load_run(&run_id).await {
                    if bundle.claims.is_empty() {
                        Some((
                            crate::tui::components::dialogs::research::ReportSection::AgentAnswer,
                            "No claims derived.".to_string(),
                        ))
                    } else {
                        let lines: Vec<String> = bundle.claims.iter().map(|claim| {
                            format!("[{}] {} (confidence: {:?})\n   Evidence: {} sources\n   Caveats: {}",
                                claim.claim_type.as_str(), claim.text, claim.confidence,
                                claim.evidence_ids.len(),
                                if claim.caveats.is_empty() { "none".to_string() } else { claim.caveats.join("; ") })
                        }).collect();
                        Some((
                            crate::tui::components::dialogs::research::ReportSection::AgentAnswer,
                            lines.join("\n\n"),
                        ))
                    }
                } else {
                    None
                }
            }
            "Contradictions" => {
                if let Ok(bundle) = service.load_run(&run_id).await {
                    if bundle.contradictions.is_empty() {
                        Some((
                            crate::tui::components::dialogs::research::ReportSection::Handoff,
                            "No contradictions detected.".to_string(),
                        ))
                    } else {
                        let lines: Vec<String> = bundle
                            .contradictions
                            .iter()
                            .map(|c| {
                                format!(
                                    "[{:?}] {}\n   Claims: {}",
                                    c.severity,
                                    c.description,
                                    c.claim_ids.join(", ")
                                )
                            })
                            .collect();
                        Some((
                            crate::tui::components::dialogs::research::ReportSection::Handoff,
                            lines.join("\n\n"),
                        ))
                    }
                } else {
                    None
                }
            }
            _ => None,
        };

        Some(TuiCommand::ResearchSectionLoaded {
            request_id,
            section,
            content: result,
            error: None,
        })
    });
}

fn apply_research_section_loaded(
    app: &mut app::App,
    request_id: u64,
    _section: String,
    content: Option<(
        crate::tui::components::dialogs::research::ReportSection,
        String,
    )>,
    error: Option<String>,
) {
    if request_id != app.dialog_state.research_request_id {
        return;
    }
    if let Some(ref mut browser) = app.dialog_state.research_browser {
        if let Some(err) = error {
            app.messages_state.toasts.warning(&err);
        } else if let Some((section_type, content)) = content {
            browser.set_report_content(section_type, content);
        } else {
            app.messages_state
                .toasts
                .warning("Could not load section content");
        }
    }
}

pub async fn run_event_loop(app: &mut app::App) -> Result<(), AppError> {
    let mut terminal_guard = terminal::TerminalGuard::enter()?;
    let mut terminal = create_terminal()?;
    let mut reader = EventStream::new();
    let mut bus_rx = GlobalEventBus::subscribe();
    let (cmd_tx, mut cmd_rx) = mpsc::channel(100);
    if app.tui_cmd_tx.is_none() {
        tracing::warn!("No TUI command sender available in app, using new channel");
    }
    app.tui_cmd_tx = Some(cmd_tx);

    const STREAM_RENDER_INTERVAL: Duration = Duration::from_millis(16); // cap streaming redraws
    const SPINNER_RENDER_INTERVAL: Duration = Duration::from_millis(80);
    const TOAST_RENDER_INTERVAL: Duration = Duration::from_millis(250);
    const RESIZE_DEBOUNCE: Duration = Duration::from_millis(75);
    let mut last_render: Option<Instant> = None;
    let mut needs_render = true;

    if let Some(ref mut watcher) = app.config_watcher {
        if let Err(e) = watcher.start().await {
            tracing::warn!("Failed to start config watcher: {}", e);
        }
    }

    while app.ui_state.running {
        let loop_start = std::time::Instant::now();
        let panic_count = app.ui_state.render_panic_count;

        // Progressive panic recovery:
        //   1+ failures: hide optional overlays/dialogs
        //   3+ failures: reset minimal volatile UI state
        if panic_count >= MAX_RENDER_PANICS {
            tracing::error!(
                "Too many root render panics ({panic_count}), resetting minimal volatile state"
            );
            clear_render_error(app);
            app.ui_state.dialog = Dialog::None;
            app.ui_state.timeline_visible = false;
            app.prompt_state.show_completions = false;
        } else if panic_count >= 1 {
            // On repeated root failures, hide optional overlays
            app.ui_state.dialog = Dialog::None;
            app.ui_state.timeline_visible = false;
            app.prompt_state.show_completions = false;
        }

        if app.messages_state.toasts.tick() {
            needs_render = true;
        }

        let render_interval = if app.streaming_active {
            STREAM_RENDER_INTERVAL
        } else {
            Duration::ZERO
        };
        let should_render = needs_render
            && last_render
                .map(|last| last.elapsed() >= render_interval)
                .unwrap_or(true);
        if should_render {
            last_render = Some(Instant::now());
            needs_render = false;

            let render_start = Instant::now();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                render_app(&mut terminal, app)
            }));
            let render_elapsed = render_start.elapsed();
            let render_elapsed_ms = render_elapsed.as_millis();
            if render_elapsed_ms > 16 && app.streaming_active {
                tracing::debug!(
                    target: "codegg::tui::render",
                    elapsed_ms = render_elapsed_ms,
                    streaming_active = app.streaming_active,
                    "slow render frame"
                );
                app.ui_state
                    .diagnostics
                    .record_slow_render(render_elapsed_ms, app.streaming_active);
            }
            if render_elapsed_ms > 100 {
                tracing::warn!(
                    target: "codegg::tui::render",
                    elapsed_ms = render_elapsed_ms,
                    "render exceeded 100ms"
                );
                app.ui_state
                    .diagnostics
                    .record_slow_render(render_elapsed_ms, app.streaming_active);
            }

            match result {
                Err(panic_err) => {
                    let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_err.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Unknown render panic".to_string()
                    };
                    tracing::error!("Render panic: {}", msg);
                    app.ui_state.render_panic_count += 1;
                    app.ui_state.diagnostics.render_panic_count =
                        app.ui_state.render_panic_count as u64;
                    app.ui_state.last_render_error = Some(msg.clone());
                    app.ui_state.diagnostics.last_render_error = Some(msg.clone());
                    if let Err(e) = render_error(&mut terminal, app, &msg) {
                        tracing::error!("Failed to render error state: {}", e);
                    }
                    continue;
                }
                Ok(Err(draw_err)) => {
                    tracing::error!("Draw error: {}", draw_err);
                    app.ui_state.last_render_error = Some(draw_err.to_string());
                    app.ui_state.diagnostics.last_render_error = Some(draw_err.to_string());
                    if let Err(e) = render_error(&mut terminal, app, &draw_err.to_string()) {
                        tracing::error!("Failed to render error state: {}", e);
                    }
                    continue;
                }
                Ok(Ok(())) => {
                    app.ui_state.render_panic_count = 0;
                    app.ui_state.last_render_error = None;
                    app.ui_state.diagnostics.render_panic_count = 0;
                    app.ui_state.diagnostics.last_render_error = None;
                }
            }
        }

        if !matches!(app.ui_state.mode, AppMode::RemoteCore { .. }) && app.prompt_state.pending_send
        {
            tracing::debug!(target: "codegg::tui::events", "pending_send=true, submitting through core facade");
            let Some(_) = app.core_client else {
                app.prompt_state.pending_send = false;
                app.session_state.session_status = SessionStatus::Error;
                app.messages_state
                    .toasts
                    .error("Core client not configured; cannot execute prompt");
                needs_render = true;
                continue;
            };
            ensure_local_session(app).await;
            app.dispatch_turn_submit_request(latest_user_message_text(app));
            app.prompt_state.pending_send = false;
            needs_render = true;
            continue;
        }

        let animation_interval = if app.streaming_active
            || matches!(app.session_state.session_status, SessionStatus::Working)
        {
            Some(if app.streaming_active {
                STREAM_RENDER_INTERVAL
            } else {
                SPINNER_RENDER_INTERVAL
            })
        } else if !app.messages_state.toasts.is_empty() {
            Some(TOAST_RENDER_INTERVAL)
        } else {
            None
        };
        let render_delay = if needs_render {
            last_render.and_then(|last| {
                let elapsed = last.elapsed();
                (elapsed < render_interval).then_some(render_interval - elapsed)
            })
        } else {
            animation_interval
        };
        let resize_delay = app.ui_state.resize_debounce.map(|started| {
            let elapsed = started.elapsed();
            if elapsed >= RESIZE_DEBOUNCE {
                Duration::ZERO
            } else {
                RESIZE_DEBOUNCE - elapsed
            }
        });
        let next_wake = match (render_delay, resize_delay) {
            (Some(render), Some(resize)) => Some(render.min(resize)),
            (Some(delay), None) | (None, Some(delay)) => Some(delay),
            (None, None) => None,
        };

        // Loop-block diagnostics: warn if the loop iteration took too long
        let loop_elapsed = loop_start.elapsed();
        if loop_elapsed.as_millis() > 250 {
            tracing::warn!(
                target: "codegg::tui::loop",
                elapsed_ms = loop_elapsed.as_millis(),
                "TUI event loop iteration exceeded 250ms threshold"
            );
            app.ui_state.diagnostics.record_slow_loop(loop_elapsed);
        }

        tokio::select! {
            biased;

            Some(result) = reader.next() => {
                if let Ok(event) = result {
                    if let Event::Paste(text) = &event {
                        if let Some(msg) = app.focus_manager.handle_paste(text.clone()) {
                            app.process_msg(msg);
                        } else {
                            app.on_paste(text.clone());
                        }
                        needs_render = true;
                        continue;
                    }
                    if let Event::Key(key) = event {
                        tracing::debug!(target: "codegg::tui::input", kind = ?key.kind, code = ?key.code, modifiers = ?key.modifiers, "key event");
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat {
                            app.on_key(key);
                            needs_render = true;
                        }
                    }
                    if let Event::Resize(_, _) = event {
                        app.ui_state.resize_debounce = Some(Instant::now());
                    }
                    if let Event::Mouse(mouse) = event {
                        app.on_mouse(mouse);
                        needs_render = true;
                    }
                }
            }

            result = bus_rx.recv() => {
                match result {
                    Ok(first_event) => {
                        tracing::debug!(target: "codegg::tui::events", discriminant = ?std::mem::discriminant(&first_event), "received bus event");
                        // Coalesce: drain any additional events already in the
                        // bus buffer (non-blocking) so we don't pay parse cost
                        // N times for N small deltas in the same frame.
                        let mut events: Vec<AppEvent> = Vec::new();
                        events.push(first_event);
                        while let Ok(more) = bus_rx.try_recv() {
                            events.push(more);
                        }
                        for event in events {
                match event {
                    AppEvent::TextDelta { delta, session_id, .. } => {
                        tracing::debug!(target: "codegg::tui::events", session_id = %session_id, delta_len = delta.len(), "TextDelta received");
                        let delta_str = delta.to_string();
                        if delta_str.contains('\n') {
                            app.messages_state.messages.finalize_streaming();
                        }
                        app.add_live_output_delta(&delta_str);
                        app.messages_state.messages.add_streaming_token(&delta_str);
                        app.streaming_active = true;
                        needs_render = true;
                        if matches!(app.session_state.session_status, SessionStatus::Working) {
                            app.status_bar.set_thinking(true, Some("Thinking...".to_string()));
                        }
                    }
                    AppEvent::ReasoningDelta { delta, .. } => {
                        tracing::debug!(target: "codegg::tui::events", "ReasoningDelta received");
                        app.messages_state.messages.add_reasoning(delta);
                        app.streaming_active = true;
                        needs_render = true;
                    }
                    AppEvent::ToolCallStarted { tool_name, tool_id, arguments, .. } => {
                        tracing::debug!(target: "codegg::tui::events", tool = %tool_name, "ToolCallStarted");
                        app.messages_state.messages.finalize_streaming();
                        app.streaming_active = true;
                        needs_render = true;
                        match serde_json::from_str::<serde_json::Value>(&arguments) {
                            Ok(args_val) => {
                                app.messages_state.messages.add_tool_call(tool_id.clone(), tool_name, args_val);
                                app.messages_state.messages.mark_tool_call_running(&tool_id);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to parse tool call arguments for {}: {} (raw: {:?})",
                                    tool_name, e, &arguments[..arguments.len().min(200)]
                                );
                                app.messages_state.messages.add_tool_call(
                                    tool_id.clone(),
                                    tool_name,
                                    serde_json::Value::Null,
                                );
                                app.messages_state.messages.mark_tool_call_running(&tool_id);
                            }
                        }
                    }
                    AppEvent::ToolResult { tool_id, tool_name: _, output, success, .. } => {
                        tracing::debug!(target: "codegg::tui::events", tool_id = %tool_id, "ToolResult received");
                        let status = if success {
                            crate::session::message::ToolStatus::Completed
                        } else {
                            crate::session::message::ToolStatus::Error
                        };
                        app.messages_state.messages.update_tool_call(&tool_id, output, status, None, None, None);
                        needs_render = true;
                    }
                    AppEvent::AgentFinished { stop_reason, input_tokens, output_tokens, cached_tokens, reasoning_tokens, .. } => {
                        tracing::debug!(target: "codegg::tui::events", stop_reason = %stop_reason, "AgentFinished received");
                        if stop_reason != "tool_calls" {
                            app.session_state.session_status = SessionStatus::Idle;
                            app.prompt_state.pending_send = false;
                            app.status_bar.set_thinking(false, None);
                            app.streaming_active = false;

                            if let (Some(in_tok), Some(out_tok)) = (input_tokens, output_tokens) {
                                app.set_tokens(in_tok as u64, out_tok as u64);
                                if let Some(ct) = cached_tokens {
                                    app.session_state.cached_tokens = ct as u64;
                                }
                            }
                            if let Some(rt) = reasoning_tokens {
                                app.session_state.reasoning_tokens += rt;
                            }

                            if let Some(ref _mem_store) = app.memory_store {
                                let experimental = crate::config::schema::Config::load()
                                    .ok()
                                    .and_then(|c| c.experimental)
                                    .and_then(|e| e.memory_auto_consolidate)
                                    .unwrap_or(false);

                                if experimental {
                                    let session_id = app.session_state.session.as_ref().map(|s| s.id.clone());
                                    let message_store = app.message_store.clone();
                                    let core_client = app.core_client.clone();
                                    let memory_store = app.memory_store.clone();
                                    let project_dir = app.session_state.project_dir.clone();

                                    tokio::spawn(async move {
                                        let project_hash = format!("{:x}", md5::compute(project_dir.as_bytes()));
                                        let messages = if let (Some(client), Some(sid)) = (core_client, session_id.clone()) {
                                            let request = crate::core::new_request(
                                                format!("session-messages-{}", uuid::Uuid::new_v4()),
                                                CoreRequest::SessionMessagesLoad { session_id: sid },
                                            );
                                            match client.request(request).await {
                                                Ok(CoreResponse::SessionMessages { messages, .. }) => crate::protocol_conversions::dtos_to_messages(messages),
                                                _ => Vec::new(),
                                            }
                                        } else if let (Some(sid), Some(store)) = (session_id, message_store) {
                                            store.list(&sid).await.unwrap_or_default()
                                        } else {
                                            Vec::new()
                                        };
                                        if !messages.is_empty() {
                                            if let Some(ref mem) = memory_store {
                                                mem.consolidate_session(&messages, &project_hash);
                                                tracing::info!("Auto-consolidated session {} memories", messages.len());
                                            }
                                        }
                                    });
                                }
                            }

                            if let Some(ref notif_mgr) = app.notification_manager {
                                let notif_type = crate::tui::components::notification::NotificationType::Success;
                                let body = format!("Agent finished: {}", stop_reason);
                                let mgr = notif_mgr.clone();
                                tokio::task::spawn_blocking(move || {
                                    if let Err(e) = mgr.blocking_send_with_config(notif_type, &body) {
                                        tracing::warn!("Failed to send notification: {}", e);
                                    }
                                });
                            }

                            app.messages_state.messages.finalize_streaming();
                            needs_render = true;

                            let tts = app.ui_state.tts.clone();
                            if tts.is_speaking() && !matches!(app.ui_state.mode, AppMode::RemoteCore { .. }) {
                                tokio::spawn(async move {
                                    if let Err(e) = tts.stop().await {
                                        tracing::debug!("TTS stop error: {}", e);
                                    }
                                });
                            }
                        } else if matches!(app.session_state.session_status, SessionStatus::Working) {
                            app.status_bar.set_thinking(true, Some("Thinking...".to_string()));
                            needs_render = true;
                        }
                    }
                    AppEvent::PermissionPending { perm_id, tool, path, args, .. } => {
                        tracing::debug!(target: "codegg::tui::events", tool = %tool, ?path, "PermissionPending");
                        app.show_permission_dialog(perm_id, PermissionRequest {
                            tool,
                            path,
                            args,
                        });
                        needs_render = true;
                    }
                    AppEvent::QuestionPending { session_id, questions, .. } => {
                        tracing::debug!(target: "codegg::tui::events", session_id = %session_id, "QuestionPending");
                        if let Ok(questions_vec) = serde_json::from_str::<Vec<crate::tui::components::dialogs::question::QuestionSpec>>(&questions) {
                            app.show_question_dialog(questions_vec, session_id);
                            needs_render = true;
                        }
                    }
                    AppEvent::FileChanged { path, action, old_content } => {
                        tracing::debug!(target: "codegg::tui::events", path = %path, action = %action, "FileChanged");
                        let path_buf = std::path::PathBuf::from(&path);

                        // Increment generation for this path.
                        let generation = if let Some(existing) = app
                            .session_state
                            .changed_files
                            .iter_mut()
                            .find(|file| file.path == path_buf)
                        {
                            let new_gen = existing.diff_state.generation().saturating_add(1);
                            existing.action = action.clone();
                            existing.diff_preview = Vec::new();
                            existing.diff_state = crate::tui::app::state::session::DiffStatsState::Pending { generation: new_gen };
                            new_gen
                        } else {
                            app.session_state.changed_files.push(
                                crate::tui::app::state::session::ChangedFile {
                                    path: path_buf.clone(),
                                    action: action.clone(),
                                    diff_preview: Vec::new(),
                                    diff_state: crate::tui::app::state::session::DiffStatsState::Pending { generation: 0 },
                                },
                            );
                            0
                        };

                        // Update sidebar immediately with pending state.
                        let changes = app
                            .session_state
                            .changed_files
                            .iter()
                            .map(|file| crate::tui::components::sidebar::SidebarFileChange {
                                path: file.path.to_string_lossy().into_owned(),
                                action: file.action.clone(),
                                diff_preview: file.diff_preview.clone(),
                                diff_state: file.diff_state.clone(),
                            })
                            .collect();
                        app.sidebar.set_file_changes(changes);

                        // Spawn background diff computation.
                        crate::tui::file_diff::spawn_sidebar_diff_stats(
                            app.tui_cmd_tx.clone(),
                            app.session_state.project_dir.clone(),
                            path,
                            old_content,
                            generation,
                        );

                        needs_render = true;
                    }
                    AppEvent::Error { message } => {
                        tracing::debug!(target: "codegg::tui::events", message = %message, "Error received");
                        tracing::error!("Agent error: {}", message);
                        app.session_state.session_status = SessionStatus::Error;
                        app.status_bar.set_thinking(false, None);
                        app.streaming_active = false;
                        app.messages_state.toasts.add(Toast::error(&message));
                        needs_render = true;
                    }
                    AppEvent::CompactionTriggered { tokens_before, tokens_after, .. } => {
                        tracing::debug!(target: "codegg::tui::events", "CompactionTriggered");
                        let compact_count = app.session_state.compaction_count + 1;
                        app.session_state.compaction_count = compact_count;
                        let before_str = if tokens_before > 0 {
                            format!("~{}k", tokens_before / 1000)
                        } else {
                            "unknown".to_string()
                        };
                        let after_str = if tokens_after > 0 {
                            format!("~{}k", tokens_after / 1000)
                        } else {
                            "unknown".to_string()
                        };
                        let toast = Toast::info(&format!(
                            "Compacted: {} → {} tokens",
                            before_str, after_str
                        ));
                        app.messages_state.toasts.add(toast);
                        needs_render = true;
                    }
                    AppEvent::ModelChanged { model, complexity } => {
                        tracing::debug!(target: "codegg::tui::events", model = %model, complexity = %complexity, "ModelChanged");
                        let short = model.split('/').next_back().unwrap_or(&model);
                        app.messages_state
                            .toasts
                            .info(&format!("Routed: {} ({})", short, complexity));
                        needs_render = true;
                    }
                    AppEvent::SubagentStarted { agent, description, .. } => {
                        tracing::debug!(target: "codegg::tui::events", agent = %agent, "SubagentStarted");
                        app.messages_state.toasts.add(Toast::info(&format!("Subagent '{}' started: {}", agent, description)));
                        needs_render = true;
                    }
                    AppEvent::SubagentProgress { agent, message, .. } => {
                        tracing::debug!(target: "codegg::tui::events", agent = %agent, "SubagentProgress");
                        app.messages_state.toasts.add(Toast::info(&format!("[{}] {}", agent, message)));
                        needs_render = true;
                    }
                    AppEvent::SubagentCompleted { agent, result_summary: _, .. } => {
                        tracing::debug!(target: "codegg::tui::events", agent = %agent, "SubagentCompleted");
                        app.messages_state.toasts.add(Toast::success(&format!("Subagent '{}' completed", agent)));
                        needs_render = true;
                        if let Some(ref notif_mgr) = app.notification_manager {
                            let notif_type = crate::tui::components::notification::NotificationType::Success;
                            let body = format!("Subagent '{}' completed", agent);
                            let mgr = notif_mgr.clone();
                            tokio::task::spawn_blocking(move || {
                                if let Err(e) = mgr.blocking_send_with_config(notif_type, &body) {
                                    tracing::warn!("Failed to send notification: {}", e);
                                }
                            });
                        }
                    }
                    AppEvent::SubagentFailed { agent, error, .. } => {
                        tracing::debug!(target: "codegg::tui::events", agent = %agent, error = %error, "SubagentFailed");
                        app.messages_state.toasts.add(Toast::error(&format!("Subagent '{}' failed: {}", agent, error)));
                        needs_render = true;
                        if let Some(ref notif_mgr) = app.notification_manager {
                            let notif_type = crate::tui::components::notification::NotificationType::Error;
                            let body = format!("Subagent '{}' failed: {}", agent, error);
                            let mgr = notif_mgr.clone();
                            tokio::task::spawn_blocking(move || {
                                if let Err(e) = mgr.blocking_send_with_config(notif_type, &body) {
                                    tracing::warn!("Failed to send notification: {}", e);
                                }
                            });
                        }
                    }
                    AppEvent::ContextUpdated { context_tokens, context_limit, .. } => {
                        tracing::debug!(target: "codegg::tui::events", context_tokens, context_limit, "ContextUpdated");
                        app.session_state.context_tokens = context_tokens;
                        app.session_state.context_limit = context_limit;
                        needs_render = true;
                    }
                    AppEvent::TodoUpdated { session_id: event_session, items, .. } => {
                        if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                            if event_session == active_id {
                                let entries: Vec<crate::tui::app::TodoEntry> = items
                                    .iter()
                                    .map(|item| crate::tui::app::TodoEntry {
                                        content: item.content.clone(),
                                        status: item.status.clone(),
                                        priority: item.priority.clone(),
                                    })
                                    .collect();
                                app.set_todos(entries);
                                needs_render = true;
                            }
                        }
                    }
                    AppEvent::GoalUpdated { session_id: event_session, goal } => {
                        if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                            if event_session == active_id {
                                if let Some(snap) = *goal {
                                    app.set_active_goal(Some(snap));
                                } else {
                                    app.set_active_goal(None);
                                }
                                needs_render = true;
                            }
                        }
                    }
                    AppEvent::GoalUsageUpdated { session_id: event_session, goal_id, usage, budget } => {
                        if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                            if event_session == active_id {
                                if let Some(ref mut active) = app.active_goal {
                                    if active.id == goal_id {
                                        active.usage = usage.clone();
                                        active.budget = budget.clone();
                                        needs_render = true;
                                    }
                                }
                            }
                        }
                    }
                    AppEvent::GoalBudgetLimited { session_id: event_session, goal_id, reason } => {
                        if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                            if event_session == active_id {
                                if let Some(ref mut active) = app.active_goal {
                                    if active.id == goal_id {
                                        active.status = "budget_limited".to_string();
                                    }
                                }
                                app.messages_state.toasts.warning(&format!(
                                    "Goal budget limited: {}",
                                    reason
                                ));
                                needs_render = true;
                            }
                        }
                    }
                    AppEvent::GoalCompleted { session_id: event_session, goal_id, evidence } => {
                        if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                            if event_session == active_id {
                                if let Some(ref mut active) = app.active_goal {
                                    if active.id == goal_id {
                                        active.status = "complete".to_string();
                                    }
                                }
                                app.messages_state.toasts.info(&format!(
                                    "Goal completed: {}",
                                    evidence
                                ));
                                needs_render = true;
                            }
                        }
                    }
                    _ => {
                        tracing::debug!(target: "codegg::tui::events", "unhandled bus event");
                    }
                }
                        } // end for event in events
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(
                            target: "codegg::tui::events",
                            dropped = n,
                            total_dropped = app.ui_state.diagnostics.dropped_bus_events + n,
                            "event bus lagged"
                        );
                        app.ui_state.diagnostics.add_dropped_bus_events(n);
                        app.messages_state.toasts.warning(&format!("Events dropped ({})", n));
                        needs_render = true;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("Event bus closed, exiting event loop");
                        break;
                    }
                }
            }

            Some(config_result) = async {
                if let Some(ref mut watcher) = app.config_watcher {
                    watcher.recv().await
                } else {
                    futures::future::pending().await
                }
            } => {
                match config_result {
                    Ok(_config) => {
                        tracing::info!("Configuration changed, reloading...");
                        GlobalEventBus::publish(AppEvent::ConfigChanged);
                        app.messages_state.toasts.add(Toast::info("Configuration reloaded"));
                        needs_render = true;
                    }
                    Err(e) => {
                        tracing::warn!("Config reload error: {}", e);
                    }
                }
            }

            _ = async {
                if let Some(delay) = next_wake {
                    tokio::time::sleep(delay).await;
                } else {
                    futures::future::pending::<()>().await;
                }
            } => {
                if let Some(debounce_start) = app.ui_state.resize_debounce {
                    if debounce_start.elapsed() >= RESIZE_DEBOUNCE {
                        app.ui_state.resize_debounce = None;
                        app.on_resize();
                        needs_render = true;
                    }
                }
                if app.streaming_active
                    || matches!(app.session_state.session_status, SessionStatus::Working)
                    || !app.messages_state.toasts.is_empty()
                {
                    needs_render = true;
                }
            }

            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    TuiCommand::DeleteSession { session_id } => {
                        handle_delete_session(app, session_id).await;
                    }
                    TuiCommand::ArchiveSession { session_id, unarchive } => {
                        handle_archive_session(app, session_id, unarchive).await;
                    }
                    TuiCommand::ForkSession { session_id } => {
                        handle_fork_session(app, session_id).await;
                    }
                    TuiCommand::ShareSession { session_id } => {
                        handle_share_session(app, session_id).await;
                    }
                    TuiCommand::UnshareSession { session_id } => {
                        handle_unshare_session(app, session_id).await;
                    }
                    TuiCommand::ExportSession { session_id } => {
                        handle_export_session(app, session_id).await;
                    }
                    TuiCommand::RenameSession { session_id, new_title } => {
                        handle_rename_session(app, session_id, new_title).await;
                    }
                    TuiCommand::BulkDelete { session_ids } => {
                        handle_bulk_delete(app, session_ids).await;
                    }
                    TuiCommand::BulkArchive {
                        session_ids,
                        unarchive,
                    } => {
                        handle_bulk_archive(app, session_ids, unarchive).await;
                    }
                    TuiCommand::BulkExport { session_ids } => {
                        handle_bulk_export(app, session_ids).await;
                    }
                    TuiCommand::ReloadSessions => {
                        start_reload_sessions(app);
                    }
                    TuiCommand::OpenTreeDialog => {
                        start_open_tree_dialog(app);
                    }
                    TuiCommand::PreviewImport { source } => {
                        start_preview_import(app, source);
                    }
                    TuiCommand::ConfirmImport { source } => {
                        start_confirm_import(app, source);
                    }
                    TuiCommand::CreateFromTemplate { key, template } => {
                        handle_create_from_template(app, key, template).await;
                    }
                    TuiCommand::LoadSessionMessages { session_id } => {
                        start_load_session_messages(app, session_id);
                    }
                    TuiCommand::RefreshSessionState { session_id } => {
                        handle_refresh_session_state(app, session_id).await;
                    }
                    TuiCommand::SpawnSubagent { agent_name, prompt } => {
                        handle_spawn_subagent(app, agent_name, prompt).await;
                    }
                    TuiCommand::UndoDelete { session_id } => {
                        handle_undo_delete(app, session_id).await;
                    }
                    TuiCommand::ListTasks => {
                        handle_list_tasks(app).await;
                    }
                    TuiCommand::UpdateModels(models) => {
                        app.set_models(models);
                        app.messages_state.toasts.add(Toast::success("Models list updated"));
                    }
                    TuiCommand::DeleteTask { id } => {
                        handle_delete_task(app, id).await;
                    }
                    TuiCommand::TaskSchedule {
                        interval_secs,
                        message,
                    } => {
                        handle_task_schedule(app, interval_secs, message).await;
                    }
                    TuiCommand::WorktreeList => {
                        handle_worktree_list(app).await;
                    }
                    TuiCommand::MemorySummary => {
                        start_memory_summary(app);
                    }
                    TuiCommand::MemorySearch { query } => {
                        start_memory_search(app, query);
                    }
                    TuiCommand::MemoryRemember { text } => {
                        start_memory_remember(app, text);
                    }
                    TuiCommand::MemoryForget { id } => {
                        start_memory_forget(app, id);
                    }
                    TuiCommand::CompactSession => {
                        handle_compact_session(app).await;
                    }
                    TuiCommand::OpenDiffDialog { old_content, new_content, title } => {
                        handle_open_diff_dialog(app, old_content, new_content, title).await;
                    }
                    TuiCommand::SendNotification {
                        notification_type,
                        body,
                    } => {
                        handle_send_notification(app, notification_type, body).await;
                    }
                    TuiCommand::GoalSet {
                        session_id,
                        project_id,
                        objective,
                    } => {
                        handle_goal_set(app, session_id, project_id, objective).await;
                    }
                    TuiCommand::GoalFromFile {
                        session_id,
                        project_id,
                        path,
                    } => {
                        handle_goal_from_file(app, session_id, project_id, path).await;
                    }
                    TuiCommand::GoalShow { session_id } => {
                        handle_goal_show(app, session_id).await;
                    }
                    TuiCommand::GoalPause { session_id } => {
                        handle_goal_simple(app, CoreRequest::GoalPause { session_id }, "pause").await;
                    }
                    TuiCommand::GoalResume { session_id } => {
                        handle_goal_simple(app, CoreRequest::GoalResume { session_id }, "resume").await;
                    }
                    TuiCommand::GoalClear { session_id } => {
                        handle_goal_simple(app, CoreRequest::GoalClear { session_id }, "clear").await;
                    }
                    TuiCommand::GoalDone { session_id } => {
                        handle_goal_simple(app, CoreRequest::GoalDone { session_id }, "done").await;
                    }
                    TuiCommand::GoalCheckpoint {
                        session_id,
                        project_id,
                    } => {
                        handle_goal_checkpoint(app, session_id, project_id).await;
                    }
                    TuiCommand::GoalBudget {
                        session_id,
                        subcommand,
                    } => {
                        handle_goal_budget(app, session_id, subcommand).await;
                    }
                    TuiCommand::ResearchListRuns => {
                        start_research_list_runs(app);
                    }
                    TuiCommand::ResearchLoadRun { run_id } => {
                        start_research_load_run(app, run_id);
                    }
                    TuiCommand::ResearchLoadSection { run_id, section } => {
                        start_research_load_section(app, run_id, section);
                    }
                    TuiCommand::RunDoctor => {
                        start_run_doctor(app);
                    }
                    TuiCommand::SecurityReviewRun {
                        id,
                        root,
                        args,
                        lsp_tool,
                    } => {
                        handle_security_review_run(app, id, root, args, lsp_tool).await;
                    }
                    TuiCommand::SecurityReviewFinished { id, receipt, error } => {
                        handle_security_review_finished(app, id, receipt, error);
                    }
                    TuiCommand::SessionsReloaded { sessions, message_counts, error } => {
                        apply_sessions_reloaded(app, sessions, message_counts, error);
                    }
                    TuiCommand::SessionMessagesLoaded { session_id, messages, error } => {
                        apply_session_messages_loaded(app, session_id, messages, error);
                    }
                    TuiCommand::TreeDialogLoaded { current_session_id, nodes, error } => {
                        apply_tree_dialog_loaded(app, current_session_id, nodes, error);
                    }
                    TuiCommand::ImportPreviewLoaded { request_id, session, msg_count, error } => {
                        apply_import_preview_loaded(app, request_id, session, msg_count, error);
                    }
                    TuiCommand::ImportConfirmed { request_id, session, error } => {
                        apply_import_confirmed(app, request_id, session, error);
                    }
                    TuiCommand::ResearchRunsLoaded { request_id, runs, error } => {
                        apply_research_runs_loaded(app, request_id, runs, error);
                    }
                    TuiCommand::ResearchRunLoaded { request_id, run_id, bundle, error } => {
                        apply_research_run_loaded(app, request_id, run_id, bundle, error);
                    }
                    TuiCommand::ResearchSectionLoaded { request_id, section, content, error } => {
                        apply_research_section_loaded(app, request_id, section, content, error);
                    }
                    TuiCommand::MemoryResult { toast_message, is_error } => {
                        apply_memory_result(app, toast_message, is_error);
                    }
                    TuiCommand::DoctorResult { summary, is_error } => {
                        apply_doctor_result(app, summary, is_error);
                    }
                    TuiCommand::RunHumanShell {
                        command,
                        promote_after,
                    } => {
                        handle_run_human_shell(app, command, promote_after);
                    }
                    TuiCommand::ShellEvent(event) => {
                        handle_shell_event(app, event);
                    }
                    TuiCommand::ShellInclude { id, mode, question } => {
                        handle_shell_include(app, id, mode, question);
                    }
                    TuiCommand::ShellRerun { id } => {
                        handle_shell_rerun(app, id);
                    }
                    TuiCommand::ShellKill { id } => {
                        handle_shell_kill(app, id);
                    }
                    TuiCommand::ShellList => {
                        handle_shell_list(app);
                    }
                    TuiCommand::ShellAsk { id, question } => {
                        handle_shell_ask(app, id, question);
                    }
                    TuiCommand::FileDiffStatsReady { path, generation, result } => {
                        handle_file_diff_stats_ready(app, path, generation, result);
                    }
                    TuiCommand::TuiStats => {
                        let summary = app.ui_state.diagnostics.summary();
                        app.messages_state.toasts.info(&summary);
                    }
                }
                needs_render = true;
            }

            Some(remote_event) = async {
                if let Some(ref mut rx) = app.remote_event_rx {
                    rx.recv().await
                } else {
                    futures::future::pending().await
                }
            } => {
                tracing::debug!(target: "codegg::tui::events", "processing remote event");
                app.handle_remote_event(remote_event);
                needs_render = true;
            }

            else => {}
        }
    }

    terminal_guard.restore();
    Ok(())
}

#[cfg(test)]
mod shell_dispatch_tests {
    use super::*;
    use crate::shell::types::{
        ShellCapturePolicy, ShellCommandId, ShellEnvPolicy, ShellOrigin, ShellRequest,
    };
    use crate::tui::app::App;
    use crate::tui::components::messages::MessageRole;
    use std::time::Duration;

    fn make_test_app() -> App {
        App::new_for_testing("/tmp".into())
    }

    fn insert_completed_entry(
        app: &mut App,
        id: u64,
        command: &str,
        stdout: &[u8],
        stderr: &[u8],
        exit_code: Option<i32>,
    ) {
        let cmd_id = ShellCommandId(id);
        let req = ShellRequest {
            id: cmd_id,
            origin: ShellOrigin::HumanEphemeral,
            command: command.to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };
        app.shell_store.insert_started(&req);
        app.shell_store.append_stdout(cmd_id, stdout);
        app.shell_store.append_stderr(cmd_id, stderr);
        let exit = exit_code.unwrap_or(0);
        app.shell_store
            .mark_exited(cmd_id, Some(exit), Duration::from_secs(1));
    }

    fn get_toasts(app: &App) -> Vec<String> {
        app.messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect()
    }

    fn get_user_messages(app: &App) -> Vec<String> {
        app.messages_state
            .messages
            .messages
            .iter()
            .filter(|m| m.role == MessageRole::User)
            .map(|m| m.text_content())
            .collect()
    }

    #[test]
    fn shell_list_empty_shows_toast() {
        let mut app = make_test_app();
        handle_shell_list(&mut app);
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("No shell commands")),
            "should show empty message, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_list_with_entries_shows_recent() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "echo hello", b"hello\n", b"", Some(0));
        insert_completed_entry(&mut app, 2, "cargo test", b"", b"fail\n", Some(1));
        handle_shell_list(&mut app);
        let toasts = get_toasts(&app);
        let text = toasts.join("\n");
        assert!(
            text.contains("echo hello"),
            "should list command, got: {text}"
        );
        assert!(
            text.contains("cargo test"),
            "should list command, got: {text}"
        );
    }

    #[test]
    fn shell_include_unknown_id_shows_warning() {
        let mut app = make_test_app();
        handle_shell_include(&mut app, 999, "all".to_string(), None);
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("not found")),
            "should show not-found warning, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_include_full_mode_promotes_output() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "echo hello", b"hello\n", b"", Some(0));
        handle_shell_include(&mut app, 1, "all".to_string(), None);
        let msgs = get_user_messages(&app);
        assert!(
            msgs.iter().any(|m| m.contains("echo hello")),
            "should include command in promoted message, got: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("hello")),
            "should include stdout in promoted message, got: {msgs:?}"
        );
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("included")),
            "should show success toast, got: {toasts:?}"
        );
        let entry = app.shell_store.get(ShellCommandId(1)).unwrap();
        assert!(entry.promoted, "entry should be marked as promoted");
    }

    #[test]
    fn shell_include_stdout_mode() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo check",
            b"checking...\n",
            b"warning: unused\n",
            Some(0),
        );
        handle_shell_include(&mut app, 1, "stdout".to_string(), None);
        let msgs = get_user_messages(&app);
        assert!(
            msgs.iter().any(|m| m.contains("checking...")),
            "should include stdout, got: {msgs:?}"
        );
    }

    #[test]
    fn shell_include_stderr_mode() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo check",
            b"checking...\n",
            b"error[E0308]: mismatched\n",
            Some(1),
        );
        handle_shell_include(&mut app, 1, "stderr".to_string(), None);
        let msgs = get_user_messages(&app);
        assert!(
            msgs.iter().any(|m| m.contains("error[E0308]")),
            "should include stderr, got: {msgs:?}"
        );
    }

    #[test]
    fn shell_include_tail_mode() {
        let mut app = make_test_app();
        let big_stderr = (0..500).map(|i| format!("line {i}\n")).collect::<String>();
        insert_completed_entry(
            &mut app,
            1,
            "big output",
            b"",
            big_stderr.as_bytes(),
            Some(1),
        );
        handle_shell_include(&mut app, 1, "tail 5".to_string(), None);
        let msgs = get_user_messages(&app);
        let included = msgs.iter().find(|m| m.contains("tail 5")).unwrap();
        assert!(
            included.contains("line 499"),
            "tail should include last lines, got: {included}"
        );
        assert!(
            !included.contains("line 0"),
            "tail should not include first lines"
        );
    }

    #[test]
    fn shell_ask_unknown_id_shows_warning() {
        let mut app = make_test_app();
        handle_shell_ask(&mut app, 999, "why did this fail?".to_string());
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("not found")),
            "should show not-found warning, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_ask_includes_question_and_output() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo test",
            b"",
            b"test result: FAILED\n",
            Some(101),
        );
        handle_shell_ask(&mut app, 1, "why did this fail?".to_string());
        let msgs = get_user_messages(&app);
        assert!(
            msgs.iter().any(|m| m.contains("why did this fail?")),
            "should include question, got: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("cargo test")),
            "should include command, got: {msgs:?}"
        );
        let entry = app.shell_store.get(ShellCommandId(1)).unwrap();
        assert!(entry.promoted, "entry should be marked as promoted");
    }

    #[test]
    fn shell_kill_nonexistent_shows_error() {
        let mut app = make_test_app();
        handle_shell_kill(&mut app, 999);
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("No running")),
            "should show error for unknown id, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_kill_running_command() {
        let mut app = make_test_app();
        let cmd_id = ShellCommandId(1);
        let req = ShellRequest {
            id: cmd_id,
            origin: ShellOrigin::HumanEphemeral,
            command: "sleep 999".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };
        app.shell_store.insert_started(&req);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let abort_handle = rt.block_on(async { tokio::spawn(async {}).abort_handle() });
        let handle = crate::shell::runtime::ShellHandle::new_for_test(cmd_id, abort_handle);
        app.shell_handles.insert(1, handle);

        handle_shell_kill(&mut app, 1);
        assert!(
            !app.shell_handles.contains_key(&1),
            "handle should be removed"
        );
        let toasts = get_toasts(&app);
        assert!(
            toasts.iter().any(|t| t.contains("Killed")),
            "should show kill confirmation, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_include_promotes_only_once() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "echo test", b"test\n", b"", Some(0));
        handle_shell_include(&mut app, 1, "all".to_string(), None);
        handle_shell_include(&mut app, 1, "all".to_string(), None);
        let msgs = get_user_messages(&app);
        let include_count = msgs.iter().filter(|m| m.contains("echo test")).count();
        assert_eq!(
            include_count, 2,
            "each /shell-include creates a new message"
        );
        let entry = app.shell_store.get(ShellCommandId(1)).unwrap();
        assert!(entry.promoted, "entry should remain promoted");
    }

    #[test]
    fn shell_list_shows_status_labels() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "cmd1", b"", b"", Some(0));
        let cmd_id = ShellCommandId(2);
        let req = ShellRequest {
            id: cmd_id,
            origin: ShellOrigin::HumanEphemeral,
            command: "cmd2".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(300),
            capture_policy: ShellCapturePolicy::StoreEphemeral,
            env_policy: ShellEnvPolicy::Inherit,
        };
        app.shell_store.insert_started(&req);
        handle_shell_list(&mut app);
        let toasts = get_toasts(&app);
        let text = toasts.join("\n");
        assert!(
            text.contains("done"),
            "should show done status, got: {text}"
        );
        assert!(
            text.contains("running"),
            "should show running status, got: {text}"
        );
    }
}

#[cfg(test)]
mod async_cmd_tests {
    use super::*;
    use crate::tui::app::App;
    use std::collections::HashMap;

    fn make_test_app() -> App {
        App::new_for_testing("/tmp".into())
    }

    #[test]
    fn apply_sessions_reloaded_with_error_shows_toast() {
        let mut app = make_test_app();
        apply_sessions_reloaded(
            &mut app,
            Vec::new(),
            HashMap::new(),
            Some("test error".into()),
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("test error")),
            "should show error toast, got: {toasts:?}"
        );
    }

    #[test]
    fn apply_sessions_reloaded_clears_loading() {
        let mut app = make_test_app();
        app.dialog_state.session_reload_in_flight = true;
        apply_sessions_reloaded(&mut app, Vec::new(), HashMap::new(), None);
        assert!(!app.dialog_state.session_reload_in_flight);
    }

    #[test]
    fn apply_session_messages_loaded_with_error_preserves_old_messages() {
        let mut app = make_test_app();
        app.messages_state
            .messages
            .add_user_message("old message".to_string(), None);
        apply_session_messages_loaded(
            &mut app,
            "session-1".into(),
            Vec::new(),
            Some("load failed".into()),
        );
        assert_eq!(
            app.messages_state.messages.message_count(),
            1,
            "old messages should be preserved on error"
        );
    }

    #[test]
    fn apply_memory_result_shows_info_toast() {
        let mut app = make_test_app();
        apply_memory_result(&mut app, "operation succeeded".to_string(), false);
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(toasts.iter().any(|t| t.contains("operation succeeded")));
    }

    #[test]
    fn apply_memory_result_error_shows_error_toast() {
        let mut app = make_test_app();
        apply_memory_result(&mut app, "something failed".to_string(), true);
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(toasts.iter().any(|t| t.contains("something failed")));
    }

    #[test]
    fn apply_doctor_result_shows_summary() {
        let mut app = make_test_app();
        apply_doctor_result(&mut app, "doctor: OK (mcp, provider)".to_string(), false);
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(toasts.iter().any(|t| t.contains("doctor: OK")));
    }

    #[test]
    fn import_preview_stale_request_id_ignored() {
        let mut app = make_test_app();
        // Set a high request id on the dialog to simulate a newer request
        app.dialog_state.import_preview_request_id = 5;
        app.dialog_state.import_dialog =
            Some(crate::tui::components::dialogs::import::ImportDialog::new(
                std::sync::Arc::new(crate::tui::theme::Theme::dark()),
            ));
        // Completion with old request_id should be ignored
        apply_import_preview_loaded(&mut app, 3, None, 0, Some("old".into()));
        // The dialog should NOT show the error (it was for a stale request)
        if let Some(ref dialog) = app.dialog_state.import_dialog {
            // The error should NOT have been set since request_id was stale
            assert!(
                dialog.error.is_none()
                    || !dialog
                        .error
                        .as_ref()
                        .map(|e| e.contains("old"))
                        .unwrap_or(false),
                "stale import preview should be ignored"
            );
        }
    }
}
