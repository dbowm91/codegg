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
pub mod command;
pub mod components;
pub mod input;
pub mod layout;
pub mod route;
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
use crate::tui::components::dialogs::import::ImportSource;
use crate::tui::components::toast::Toast;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::tui::app::SessionStatus;
use md5;
use rand;
use std::fs::OpenOptions;
use tokio::sync::mpsc;

macro_rules! debug_log {
    ($($arg:tt)*) => {
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open("codegg_debug.log")
            .and_then(|mut file| {
                std::io::Write::write_all(&mut file, format!("[MOD-DEBUG] {}\n", format!($($arg)*)).as_bytes())
            });
    };
}

pub fn enter_raw() -> Result<(), AppError> {
    execute!(stdout(), EnterAlternateScreen)?;
    crossterm::terminal::enable_raw_mode()?;
    execute!(stdout(), EnableBracketedPaste)?;
    execute!(stdout(), EnableMouseCapture)?;
    Ok(())
}

pub fn exit_raw() {
    print!("\x1b[?1049l");
    let _ = execute!(stdout(), DisableBracketedPaste);
    let _ = execute!(stdout(), DisableMouseCapture);
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);
}

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
    app.ui_state.dialog = Dialog::None;
    app.ui_state.command_mode = false;
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
    debug_log!("Event loop: no session exists, creating new session");
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
                app.session_state.session = Some(session);
                debug_log!("Event loop: session created via core with id={}", session_id);
            }
            Ok(CoreResponse::Error { code, message }) => {
                debug_log!(
                    "Event loop: failed to create session via core ({}): {}",
                    code,
                    message
                );
            }
            Ok(other) => {
                debug_log!("Event loop: unexpected session-create response: {:?}", other);
            }
            Err(e) => {
                debug_log!("Event loop: failed to create session via core: {:?}", e);
            }
        }
    } else {
        debug_log!("Event loop: no core client available for session creation");
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

    app.dialog_state.session_dialog.load_sessions(sessions);
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
                    tracing::warn!("failed to permanently delete session {} via core: {}", id, e);
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
                    tracing::warn!("failed to export session {} via core ({}): {}", id, code, message)
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
                app.session_state.session = Some(shared.clone());
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
                app.session_state.session = Some(session);
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
                app.session_state.session = Some(session);
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

async fn handle_open_tree_dialog(app: &mut app::App) {
    use std::collections::HashMap;
    use crate::tui::components::dialogs::tree::TreeNode;

    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Core client not available");
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
        .map(|s| (s.id.clone(), s))
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
                .push(session.clone());
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
        let mut children = children_map
            .get(&session.id)
            .cloned()
            .unwrap_or_default();
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
                            import.set_preview(session, msg_count);
                        }
                    }
                    (Ok(CoreResponse::Error { message, .. }), _) | (_, Ok(CoreResponse::Error { message, .. })) => {
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
                            import.set_error("Unexpected response while loading session".to_string());
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
                                    import.set_preview(session, msg_count);
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
                            import.set_done(session);
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
                template: template.clone(),
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
            app.session_state.session = Some(session.clone());
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
        depth: 0,
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
            Ok(CoreResponse::SessionMessages { messages, .. }) => Some(messages),
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
            | Ok(CoreResponse::Json { .. }) => None,
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
            .error("Core client not available");
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
                        },
                        crate::session::message::PartData::Image { .. }
                        | crate::session::message::PartData::File { .. } => MsgPart::Text {
                            content: "[File/Image]".to_string(),
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
                            let interval_secs = t
                                .get("interval_secs")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
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

async fn handle_memory_summary(app: &mut app::App) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let project_hash = format!("{:x}", md5::compute(app.session_state.project_dir.as_bytes()));
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

async fn handle_memory_search(app: &mut app::App, query: String) {
    if query.is_empty() {
        app.messages_state.toasts.warning("Usage: /memory-search <query>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let request = crate::core::new_request(
        format!("memory-search-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemorySearch { query: query.clone() },
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
        Ok(CoreResponse::Json { .. }) | Ok(CoreResponse::Ack) => app.messages_state.toasts.info("Remembered"),
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
            let deleted = data.get("deleted").and_then(|v| v.as_bool()).unwrap_or(false);
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
                        let branch = t
                            .get("branch")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
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

pub async fn run_event_loop(app: &mut app::App) -> Result<(), AppError> {
    enter_raw()?;
    let mut terminal = create_terminal()?;
    let mut reader = EventStream::new();
    let mut bus_rx = GlobalEventBus::subscribe();
    let (cmd_tx, mut cmd_rx) = mpsc::channel(100);
    if app.tui_cmd_tx.is_none() {
        tracing::warn!("No TUI command sender available in app, using new channel");
    }
    app.tui_cmd_tx = Some(cmd_tx);

    const RENDER_INTERVAL: Duration = Duration::from_millis(16);
    let mut last_render: Option<Instant> = None;

    if let Some(ref mut watcher) = app.config_watcher {
        if let Err(e) = watcher.start().await {
            tracing::warn!("Failed to start config watcher: {}", e);
        }
    }

    while app.ui_state.running {
        let needs_reset = app.ui_state.render_panic_count >= MAX_RENDER_PANICS;
        if needs_reset {
            tracing::error!(
                "Too many render panics ({}), attempting state reset",
                app.ui_state.render_panic_count
            );
            clear_render_error(app);
            app.reset_state();
        }

        let now = Instant::now();
        let should_render = last_render
            .map(|last| now.duration_since(last) >= RENDER_INTERVAL)
            .unwrap_or(true);
        if should_render {
            last_render = Some(now);

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                render_app(&mut terminal, app)
            }));

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
                    app.ui_state.last_render_error = Some(msg.clone());
                    if let Err(e) = render_error(&mut terminal, app, &msg) {
                        tracing::error!("Failed to render error state: {}", e);
                    }
                    continue;
                }
                Ok(Err(draw_err)) => {
                    tracing::error!("Draw error: {}", draw_err);
                    app.ui_state.last_render_error = Some(draw_err.to_string());
                    if let Err(e) = render_error(&mut terminal, app, &draw_err.to_string()) {
                        tracing::error!("Failed to render error state: {}", e);
                    }
                    continue;
                }
                Ok(Ok(())) => {
                    app.ui_state.render_panic_count = 0;
                    app.ui_state.last_render_error = None;
                }
            }
        }

        app.messages_state.toasts.tick();

        if !app.ui_state.remote_mode && app.prompt_state.pending_send {
            debug_log!("Event loop: pending_send=true, submitting through core facade");
            let Some(_) = app.core_client else {
                app.prompt_state.pending_send = false;
                app.session_state.session_status = SessionStatus::Error;
                app.messages_state
                    .toasts
                    .error("Core client not configured; cannot execute prompt");
                continue;
            };
            ensure_local_session(app).await;
            app.dispatch_turn_submit_request(latest_user_message_text(app));
            app.prompt_state.pending_send = false;
            continue;
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
                        continue;
                    }
                    if let Event::Key(key) = event {
                        debug_log!(
                            "key event: kind={:?}, code={:?}, modifiers={:?}",
                            key.kind, key.code, key.modifiers
                        );
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat {
                            app.on_key(key);
                        }
                    }
                    if let Event::Resize(_, _) = event {
                        app.ui_state.resize_debounce = Some(std::time::Instant::now());
                    }
                    if let Event::Mouse(mouse) = event {
                        app.on_mouse(mouse);
                    }
                }
            }

            Ok(event) = bus_rx.recv() => {
                debug_log!("Event loop: received event: {:?}", std::mem::discriminant(&event));
                match event {
                    AppEvent::TextDelta { delta, session_id, .. } => {
                        debug_log!("Event loop: TextDelta received session_id={}, delta_len={}", session_id, delta.len());
                        let delta_str = delta.to_string();
                        if delta_str.contains('\n') {
                            app.messages_state.messages.finalize_streaming();
                        }
                        app.messages_state.messages.add_streaming_token(&delta_str);
                        if matches!(app.session_state.session_status, SessionStatus::Working) {
                            app.footer.set_thinking(true, Some("Thinking...".to_string()));
                        }
                    }
                    AppEvent::ReasoningDelta { delta, .. } => {
                        debug_log!("Event loop: ReasoningDelta received");
                        app.messages_state.messages.add_reasoning(delta);
                    }
                    AppEvent::ToolCallStarted { tool_name, tool_id, arguments, .. } => {
                        debug_log!("Event loop: ToolCallStarted for tool={}", tool_name);
                        if let Ok(args_val) = serde_json::from_str(&arguments) {
                            app.messages_state.messages.add_tool_call(tool_id, tool_name, args_val);
                        }
                    }
                    AppEvent::ToolResult { tool_id, tool_name: _, output, success, .. } => {
                        debug_log!("Event loop: ToolResult for tool_id={}", tool_id);
                        let status = if success {
                            crate::session::message::ToolStatus::Completed
                        } else {
                            crate::session::message::ToolStatus::Error
                        };
                        app.messages_state.messages.update_tool_call(&tool_id, output, status, None, None, None);
                    }
                    AppEvent::AgentFinished { stop_reason, input_tokens, output_tokens, cached_tokens, .. } => {
                        debug_log!("Event loop: AgentFinished received stop_reason={}", stop_reason);
                        if stop_reason == "completed" {
                            app.session_state.session_status = SessionStatus::Idle;
                            app.prompt_state.pending_send = false;
                            app.footer.set_thinking(false, None);

                            if let (Some(in_tok), Some(out_tok)) = (input_tokens, output_tokens) {
                                app.set_tokens(in_tok as u64, out_tok as u64);
                                if let Some(ct) = cached_tokens {
                                    app.session_state.cached_tokens = ct as u64;
                                }
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
                                                Ok(CoreResponse::SessionMessages { messages, .. }) => messages,
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
                                let body = "Agent finished: completed".to_string();
                                let mgr = notif_mgr.clone();
                                tokio::task::spawn_blocking(move || {
                                    if let Err(e) = mgr.blocking_send_with_config(notif_type, &body) {
                                        tracing::warn!("Failed to send notification: {}", e);
                                    }
                                });
                            }

                            app.messages_state.messages.finalize_streaming();

                            let tts = app.ui_state.tts.clone();
                            if tts.is_speaking() {
                                tokio::spawn(async move {
                                    if let Err(e) = tts.stop().await {
                                        tracing::debug!("TTS stop error: {}", e);
                                    }
                                });
                            }
                        } else if matches!(app.session_state.session_status, SessionStatus::Working) {
                            app.footer.set_thinking(true, Some("Thinking...".to_string()));
                        }
                    }
                    AppEvent::PermissionPending { perm_id, tool, path, args, .. } => {
                        debug_log!("Event loop: PermissionPending for tool={}, path={:?}", tool, path);
                        app.show_permission_dialog(perm_id, PermissionRequest {
                            tool,
                            path,
                            args,
                        });
                    }
                    AppEvent::QuestionPending { session_id, questions } => {
                        debug_log!("Event loop: QuestionPending for session={}", session_id);
                        if let Ok(questions_vec) = serde_json::from_str::<Vec<crate::tui::components::dialogs::question::QuestionSpec>>(&questions) {
                            app.show_question_dialog(questions_vec, session_id);
                        }
                    }
                    AppEvent::FileChanged { path, action, old_content } => {
                        debug_log!("Event loop: FileChanged for path={}, action={}", path, action);
                        // Note: old_content is available for snapshot checkpointing
                        let _ = old_content; // Suppress unused warning
                        app.session_state.changed_files.push(
                            crate::tui::app::state::session::ChangedFile {
                                path: std::path::PathBuf::from(&path),
                                action: action.clone(),
                            },
                        );
                        app.sidebar.set_file_changes(vec![format!("{} ({})", path, action)]);
                    }
                    AppEvent::Error { message } => {
                        debug_log!("Event loop: Error received: {}", message);
                        tracing::error!("Agent error: {}", message);
                        app.session_state.session_status = SessionStatus::Error;
                        app.footer.set_thinking(false, None);
                        app.messages_state.toasts.add(Toast::error(&message));
                    }
                    AppEvent::CompactionTriggered { .. } => {
                        debug_log!("Event loop: CompactionTriggered");
                        app.messages_state.toasts.add(Toast::info("Context compacted"));
                    }
                    AppEvent::SubagentStarted { agent, description, .. } => {
                        debug_log!("Event loop: SubagentStarted agent={}", agent);
                        app.messages_state.toasts.add(Toast::info(&format!("Subagent '{}' started: {}", agent, description)));
                    }
                    AppEvent::SubagentProgress { agent, message, .. } => {
                        debug_log!("Event loop: SubagentProgress agent={}", agent);
                        app.messages_state.toasts.add(Toast::info(&format!("[{}] {}", agent, message)));
                    }
                    AppEvent::SubagentCompleted { agent, result_summary: _, .. } => {
                        debug_log!("Event loop: SubagentCompleted agent={}", agent);
                        app.messages_state.toasts.add(Toast::success(&format!("Subagent '{}' completed", agent)));
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
                        debug_log!("Event loop: SubagentFailed agent={}, error={}", agent, error);
                        app.messages_state.toasts.add(Toast::error(&format!("Subagent '{}' failed: {}", agent, error)));
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
                    _ => {
                        debug_log!("Event loop: unhandled event");
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
                    }
                    Err(e) => {
                        tracing::warn!("Config reload error: {}", e);
                    }
                }
            }

            _ = tokio::time::sleep(Duration::from_millis(75)) => {
                if let Some(debounce_start) = app.ui_state.resize_debounce {
                    if debounce_start.elapsed() >= Duration::from_millis(75) {
                        app.ui_state.resize_debounce = None;
                        app.on_resize();
                    }
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
                        reload_sessions(app).await;
                    }
                    TuiCommand::OpenTreeDialog => {
                        handle_open_tree_dialog(app).await;
                    }
                    TuiCommand::PreviewImport { source } => {
                        handle_preview_import(app, source).await;
                    }
                    TuiCommand::ConfirmImport { source } => {
                        handle_confirm_import(app, source).await;
                    }
                    TuiCommand::CreateFromTemplate { key, template } => {
                        handle_create_from_template(app, key, template).await;
                    }
                    TuiCommand::LoadSessionMessages { session_id } => {
                        handle_load_session_messages(app, session_id).await;
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
                        handle_memory_summary(app).await;
                    }
                    TuiCommand::MemorySearch { query } => {
                        handle_memory_search(app, query).await;
                    }
                    TuiCommand::MemoryRemember { text } => {
                        handle_memory_remember(app, text).await;
                    }
                    TuiCommand::MemoryForget { id } => {
                        handle_memory_forget(app, id).await;
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
                }
            }

            Some(remote_event) = async {
                if let Some(ref mut rx) = app.remote_event_rx {
                    rx.recv().await
                } else {
                    futures::future::pending().await
                }
            } => {
                debug_log!("Event loop: processing remote event");
                app.handle_remote_event(remote_event);
            }

            else => {}
        }
    }

    exit_raw();
    Ok(())
}
