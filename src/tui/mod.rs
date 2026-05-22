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

use crate::permission::PermissionChecker;
use crate::provider::{ChatRequest, ContentPart, Message, ProviderRegistry};
use crate::session::CreateSession;

use crate::tui::app::SessionStatus;
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

fn build_conversation_context(
    ui_messages: &[crate::tui::components::messages::UIMessage],
) -> Vec<Message> {
    use crate::tui::components::messages::{MessageRole as UIMessageRole, MsgPart};

    let mut out = Vec::new();

    for m in ui_messages {
        let mut content: Vec<ContentPart> = Vec::new();
        let mut tool_calls: Vec<crate::provider::ToolCall> = Vec::new();
        let mut tool_results: Vec<Message> = Vec::new();

        for p in &m.parts {
            match p {
                MsgPart::Text { content: text } => content.push(ContentPart::Text {
                    text: text.clone().into(),
                }),
                MsgPart::Reasoning { content: text, .. } => content.push(ContentPart::Text {
                    text: format!("[Reasoning]\n{}", text).into(),
                }),
                MsgPart::ToolCall {
                    id,
                    name,
                    input,
                    output,
                    ..
                } => {
                    if let Ok(arguments) = serde_json::from_str::<serde_json::Value>(input) {
                        tool_calls.push(crate::provider::ToolCall {
                            id: id.clone().into(),
                            name: name.clone().into(),
                            arguments,
                        });
                    }
                    tool_results.push(Message::Tool {
                        tool_call_id: id.clone().into(),
                        content: output.clone().into(),
                    });
                }
            }
        }

        match m.role {
            UIMessageRole::User => {
                if !content.is_empty() {
                    out.push(Message::User { content });
                }
            }
            UIMessageRole::Assistant => {
                if !content.is_empty() || !tool_calls.is_empty() {
                    out.push(Message::Assistant {
                        content,
                        tool_calls,
                    });
                }
                out.extend(tool_results);
            }
        }
    }

    out
}

async fn reload_sessions(app: &mut app::App) {
    use std::collections::HashMap;

    let store = match &app.session_store {
        Some(s) => Arc::clone(s),
        None => return,
    };
    let project_id = app.session_state.project_dir.clone();
    let show_archived = app.dialog_state.session_dialog.show_archived;

    app.dialog_state.session_dialog.set_loading(true);

    let sessions = match if show_archived {
        store.list_all(&project_id, None).await
    } else {
        store.list(&project_id, 100).await
    } {
        Ok(sessions) => sessions,
        Err(e) => {
            tracing::warn!("failed to load sessions: {}", e);
            return;
        }
    };

    let session_ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
    let message_counts: HashMap<String, usize> = if session_ids.is_empty() {
        HashMap::new()
    } else {
        store.message_counts(&session_ids).await.unwrap_or_default()
    };

    app.dialog_state.session_dialog.load_sessions(sessions);
    for (id, count) in message_counts {
        app.dialog_state
            .session_dialog
            .set_message_count(&id, count);
    }
}

async fn handle_delete_session(app: &mut app::App, session_id: String) {
    if let Some(ref store) = app.session_store {
        if let Err(e) = store.soft_delete(&session_id).await {
            tracing::warn!("failed to soft delete session: {}", e);
        }
    }
    app.messages_state.toasts.info("Session deleted");
    reload_sessions(app).await;
}

async fn handle_archive_session(app: &mut app::App, session_id: String, unarchive: bool) {
    if let Some(ref store) = app.session_store {
        if let Err(e) = if unarchive {
            store.unarchive(&session_id).await
        } else {
            store.archive(&session_id).await
        } {
            tracing::warn!("failed to archive session: {}", e);
        }
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
    if let Some(ref store) = app.session_store {
        if let Err(e) = store.fork(&session_id).await {
            tracing::warn!("failed to fork session: {}", e);
        }
    }
    app.messages_state.toasts.info("Session forked");
    reload_sessions(app).await;
}

async fn handle_bulk_delete(app: &mut app::App, session_ids: Vec<String>) {
    let count = session_ids.len();
    if let Some(ref store) = app.session_store {
        for id in &session_ids {
            if let Err(e) = store.delete(id).await {
                tracing::warn!("failed to delete session {}: {}", id, e);
            }
        }
    }
    app.messages_state
        .toasts
        .info(&format!("{} sessions deleted", count));
    reload_sessions(app).await;
}

async fn handle_bulk_archive(app: &mut app::App, session_ids: Vec<String>, unarchive: bool) {
    let count = session_ids.len();
    if let Some(ref store) = app.session_store {
        for id in &session_ids {
            let r = if unarchive {
                store.unarchive(id).await
            } else {
                store.archive(id).await
            };
            if let Err(e) = r {
                tracing::warn!("failed to archive session {}: {}", id, e);
            }
        }
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
    if let Some(ref store) = app.session_store {
        for id in &session_ids {
            match store.export_session(id).await {
                Ok(_) => tracing::info!("exported session {}", id),
                Err(e) => tracing::warn!("failed to export session {}: {}", id, e),
            }
        }
    }
    app.messages_state
        .toasts
        .info(&format!("{} sessions exported", count));
}

async fn handle_share_session(app: &mut app::App, session_id: String) {
    if let Some(ref store) = app.session_store {
        match store.share_session(&session_id).await {
            Ok(shared) => {
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
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to share: {}", e));
            }
        }
    } else {
        app.messages_state
            .toasts
            .error("Session store not available");
    }
}

async fn handle_unshare_session(app: &mut app::App, session_id: String) {
    if let Some(ref store) = app.session_store {
        match store.unshare_session(&session_id).await {
            Ok(session) => {
                app.session_state.session = Some(session);
                app.messages_state.toasts.info("Session unshared");
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
            .error("Session store not available");
    }
}

async fn handle_export_session(app: &mut app::App, session_id: String) {
    if let Some(ref store) = app.session_store {
        match store.export_session(&session_id).await {
            Ok(export) => {
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
            Err(e) => {
                app.messages_state
                    .toasts
                    .error(&format!("Failed to export: {}", e));
            }
        }
    } else {
        app.messages_state
            .toasts
            .error("Session store not available");
    }
}

async fn handle_rename_session(app: &mut app::App, session_id: String, new_title: String) {
    if let Some(ref store) = app.session_store {
        use crate::session::UpdateSession;
        match store
            .update(
                &session_id,
                UpdateSession {
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
                },
            )
            .await
        {
            Ok(session) => {
                app.session_state.session = Some(session);
                app.messages_state.toasts.info("Session renamed");
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
            .error("Session store not available");
    }
}

async fn handle_open_tree_dialog(app: &mut app::App) {
    let store = app.session_store.clone();
    let sess = app.session_state.session.clone();
    app.dialog_state
        .tree_dialog
        .build_from_session_async(sess.as_ref(), store)
        .await;
}

async fn handle_preview_import(app: &mut app::App, source: ImportSource) {
    let Some(store) = &app.session_store else {
        if let Some(ref mut import) = app.dialog_state.import_dialog {
            import.set_error("No session store available".to_string());
        }
        return;
    };
    let store = Arc::clone(store);

    match source {
        ImportSource::SessionId(id) => match store.get(&id).await {
            Ok(Some(session)) => {
                let msg_count = store.message_count(&id).await.unwrap_or(0);
                if let Some(ref mut import) = app.dialog_state.import_dialog {
                    import.set_preview(session, msg_count);
                }
            }
            Ok(None) => {
                if let Some(ref mut import) = app.dialog_state.import_dialog {
                    import.set_error(format!("Session not found: {}", id));
                }
            }
            Err(e) => {
                if let Some(ref mut import) = app.dialog_state.import_dialog {
                    import.set_error(format!("Failed to load session: {}", e));
                }
            }
        },
        ImportSource::FilePath(path) => match tokio::fs::read_to_string(path.as_str()).await {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(data) => match store.import_session(data, None).await {
                    Ok(session) => {
                        let msg_count = store.message_count(&session.id).await.unwrap_or(0);
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_preview(session, msg_count);
                        }
                    }
                    Err(e) => {
                        if let Some(ref mut import) = app.dialog_state.import_dialog {
                            import.set_error(format!("Import failed: {}", e));
                        }
                    }
                },
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
}

async fn handle_confirm_import(app: &mut app::App, source: ImportSource) {
    let Some(store) = &app.session_store else {
        if let Some(ref mut import) = app.dialog_state.import_dialog {
            import.set_error("No session store available".to_string());
        }
        return;
    };
    let store = Arc::clone(store);

    match source {
        ImportSource::SessionId(id) => match store.fork(&id).await {
            Ok(session) => {
                if let Some(ref mut import) = app.dialog_state.import_dialog {
                    import.set_done(session);
                }
            }
            Err(e) => {
                if let Some(ref mut import) = app.dialog_state.import_dialog {
                    import.set_error(format!("Import failed: {}", e));
                }
            }
        },
        ImportSource::FilePath(_) => {
            if let Some(ref mut import) = app.dialog_state.import_dialog {
                import.set_error("File already imported via preview".to_string());
            }
        }
    }
}

async fn handle_create_from_template(
    app: &mut app::App,
    _key: String,
    template: crate::config::schema::SessionTemplate,
) {
    let Some(store) = &app.session_store else {
        app.messages_state
            .toasts
            .error("No session store available");
        return;
    };
    let store = Arc::clone(store);
    let project_dir = app.session_state.project_dir.clone();
    let template_name = template.name.clone();
    let agent = template.agent.clone();
    let model = template.model.clone();

    match store
        .create_from_template(&template, &project_dir, &project_dir)
        .await
    {
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

    let store = match &app.message_store {
        Some(s) => Arc::clone(s),
        None => return,
    };

    app.messages_state.messages.clear();

    match store.list(&session_id).await {
        Ok(messages) => {
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
        Err(e) => {
            tracing::warn!("failed to load messages: {}", e);
        }
    }
}

async fn handle_undo_delete(app: &mut app::App, session_id: String) {
    if let Some(store) = &app.session_store {
        match store.restore(&session_id).await {
            Ok(_) => {
                app.messages_state
                    .toasts
                    .success("Session restored successfully");
                reload_sessions(app).await;
            }
            Err(e) => {
                tracing::error!("Failed to restore session {}: {}", session_id, e);
                app.messages_state.toasts.error("Failed to restore session");
            }
        }
    } else {
        tracing::warn!("No session store available for undo");
    }
    app.undo_session_id = None;
    app.undo_until = None;
}

async fn handle_list_tasks(app: &mut app::App) {
    if let Some(ref scheduler) = app.bg_scheduler {
        let tasks = scheduler.list().await;
        if tasks.is_empty() {
            app.messages_state.toasts.info("No background tasks");
        } else {
            let list: Vec<String> = tasks
                .iter()
                .map(|t| {
                    format!(
                        "{}: {} ({:?})",
                        t.id.chars().take(8).collect::<String>(),
                        t.message.chars().take(30).collect::<String>(),
                        t.interval
                    )
                })
                .collect();
            app.messages_state.toasts.info(&list.join(" | "));
        }
    } else {
        app.messages_state.toasts.info("No background tasks");
    }
}

async fn handle_delete_task(app: &mut app::App, id: String) {
    if let Some(ref scheduler) = app.bg_scheduler {
        let removed = scheduler.remove(&id).await;
        if removed {
            app.messages_state.toasts.info("Task deleted");
        } else {
            app.messages_state.toasts.warning("Task not found");
        }
    } else {
        app.messages_state.toasts.warning("Scheduler not available");
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
    let mut processing_task: Option<tokio::task::JoinHandle<()>> = None;
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

        if !app.ui_state.remote_mode && app.prompt_state.pending_send && processing_task.is_none() {
            debug_log!("Event loop: pending_send=true, spawning agent task");

            if app.session_state.session.is_none() {
                debug_log!("Event loop: no session exists, creating new session");
                if let Some(store) = &app.session_store {
                    let project_dir = app.session_state.project_dir.clone();
                    let store = Arc::clone(store);
                    let new_session = store
                        .create(CreateSession {
                            project_id: project_dir.clone(),
                            directory: project_dir.clone(),
                            title: None,
                            parent_id: None,
                            workspace_id: None,
                            agent: None,
                            model: None,
                            tags: None,
                        })
                        .await;
                    match new_session {
                        Ok(session) => {
                            let _session_id = session.id.clone();
                            app.session_state.session = Some(session);
                            debug_log!("Event loop: session created with id={}", _session_id);
                        }
                        Err(e) => {
                            debug_log!("Event loop: failed to create session: {:?}", e);
                        }
                    }
                } else {
                    debug_log!("Event loop: no session store available");
                }
            }

            let config = crate::config::schema::Config::load().unwrap_or_default();
            let active_agent = &app.agent_state.agents[app.agent_state.current_agent];
            debug_log!("Event loop: using model={}", app.agent_state.current_model);
            debug_log!(
                "Event loop: active agent name={}, mode={:?}, steps={:?}",
                active_agent.name,
                active_agent.mode,
                active_agent.steps
            );

            processing_task = Some(tokio::spawn({
                let model = app.agent_state.current_model.clone();
                let messages = build_conversation_context(&app.messages_state.messages.messages);
                let session_id = app
                    .session_state
                    .session
                    .as_ref()
                    .map(|s| s.id.clone())
                    .unwrap_or_default();
                let agents = app.agent_state.agents.clone();
                let current_agent_idx = app.agent_state.current_agent;
                let config = config.clone();
                let pool = app.session_store.as_ref().map(|s| s.pool());
                let subagent_pool = app.subagent_pool.clone();
                let memory_store = app.memory_store.clone();

                async move {
                    use crate::agent::prompt::load_agent_prompt;
                    use crate::agent::r#loop::AgentLoop;
                    use crate::tool::ToolRegistry;

                    let mut registry = ProviderRegistry::new();
                    crate::provider::register_builtin_with_config(&mut registry, &config);

                    let provider_name = model.split('/').next().unwrap_or("openai").to_string();
                    let model_name = model.split('/').next_back().unwrap_or(&model).to_string();
                    debug_log!(
                        "Agent task: provider_name={}, model_name={}",
                        provider_name,
                        model_name
                    );

                    if let Some(base_provider) = registry.get(&provider_name) {
                        debug_log!("Agent task: provider found, creating agent loop");
                        let provider = base_provider.clone_box();
                        let mut tool_registry = ToolRegistry::with_defaults();
                        debug_log!(
                            "Agent task: default tool registry size={}",
                            tool_registry.list().len()
                        );

                        if let Some(pool) = subagent_pool {
                            let task_tool = crate::tool::task::TaskTool::new(
                                pool.task_store(),
                                Some(pool.spawner()),
                                Some(session_id.clone()),
                                Vec::new(),
                            );
                            tool_registry.register(task_tool);
                        }

                        let permission_checker = PermissionChecker::new(Some(&config), None);

                        let memory_context = memory_store.as_ref().map(|store| {
                            let all_memories = store.list("user/preferences");
                            if all_memories.is_empty() {
                                String::new()
                            } else {
                                let summary: String = all_memories
                                    .iter()
                                    .take(10)
                                    .map(|m| {
                                        format!(
                                            "- [{}] {}",
                                            m.id,
                                            m.title.as_deref().unwrap_or("(untitled)")
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                format!("\n\n## Learned Preferences\n{}\n", summary)
                            }
                        }).unwrap_or_default();

                        let mut system = load_agent_prompt(
                            &agents[current_agent_idx],
                            &config,
                            &model_name,
                        );
                        system.push_str(&memory_context);

                        let mut agent_loop = AgentLoop::new(
                            agents,
                            provider,
                            permission_checker,
                            tool_registry,
                            config.clone(),
                            None,
                            pool,
                        );
                        agent_loop.set_session_id(&session_id);

                        let request = ChatRequest {
                            messages,
                            model: model_name,
                            tools: None,
                            system: Some(system),
                            temperature: None,
                            top_p: None,
                            max_tokens: None,
                            response_format: None,
                        };

                        debug_log!(
                            "Agent task: starting agent_loop.run() with {} messages, model={}",
                            request.messages.len(),
                            request.model
                        );
                        if let Err(e) = agent_loop.run(request).await {
                            debug_log!("Agent task: agent loop error: {}", e);
                            tracing::error!("Agent loop error: {}", e);
                            crate::bus::global::GlobalEventBus::publish(AppEvent::Error {
                                message: format!("Agent error: {}", e),
                            });
                        } else {
                            debug_log!("Agent task: agent loop completed successfully");
                            crate::bus::global::GlobalEventBus::publish(AppEvent::AgentFinished {
                                session_id: session_id.clone(),
                                stop_reason: "completed".to_string(),
                            });
                        }
                    } else {
                        debug_log!(
                            "Agent task: provider '{}' not found in registry",
                            provider_name
                        );
                        crate::bus::global::GlobalEventBus::publish(AppEvent::Error {
                            message: format!(
                                "Provider '{}' not found. Please check your configuration.",
                                provider_name
                            ),
                        });
                    }
                }
            }));
        }

        if let Some(task) = processing_task.as_mut() {
            if task.is_finished() {
                debug_log!("Event loop: processing task finished, resetting state");
                processing_task = None;
                app.event_rx = None;
                app.session_state.session_status = SessionStatus::Idle;
                app.prompt_state.pending_send = false;
            }
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
                        app.on_resize();
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
                        app.messages_state.messages.add_assistant_text(delta.to_string());
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
                    AppEvent::AgentFinished { stop_reason, .. } => {
                        debug_log!("Event loop: AgentFinished received stop_reason={}", stop_reason);
                        if stop_reason == "completed" {
                            app.session_state.session_status = SessionStatus::Idle;
                            app.prompt_state.pending_send = false;
                            app.footer.set_thinking(false, None);

                            if let Some(ref mem_store) = app.memory_store {
                                let experimental = crate::config::schema::Config::load()
                                    .ok()
                                    .and_then(|c| c.experimental)
                                    .and_then(|e| e.memory_auto_consolidate)
                                    .unwrap_or(false);

                                if experimental {
                                    let session_id = app.session_state.session.as_ref().map(|s| s.id.clone());
                                    let message_store = app.message_store.clone();
                                    let memory_store = app.memory_store.clone();
                                    let project_dir = app.session_state.project_dir.clone();

                                    tokio::spawn(async move {
                                        let project_hash = format!("{:x}", md5::compute(project_dir.as_bytes()));
                                        if let (Some(sid), Some(store)) = (session_id, message_store) {
                                            if let Ok(messages) = store.list(&sid).await {
                                                if !messages.is_empty() {
                                                    if let Some(ref mem) = memory_store {
                                                        mem.consolidate_session(&messages, &project_hash);
                                                        tracing::info!("Auto-consolidated session {} memories", messages.len());
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
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
                    }
                    AppEvent::SubagentFailed { agent, error, .. } => {
                        debug_log!("Event loop: SubagentFailed agent={}, error={}", agent, error);
                        app.messages_state.toasts.add(Toast::error(&format!("Subagent '{}' failed: {}", agent, error)));
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
