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
pub mod commands;
pub mod components;
pub mod file_diff;
pub mod input;
pub mod layout;
pub mod route;
pub(crate) mod runtime;
pub mod task_lifecycle;
pub mod terminal;
pub mod theme;

pub use app::{App, Dialog, SessionMutationOp, TuiCommand};
pub use input::InputAction;
pub use route::Route;
pub use theme::Theme;

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::components::toast::Toast;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::tui::app::state::AppMode;
use crate::tui::app::SessionStatus;

pub type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub fn create_terminal() -> Result<AppTerminal, crate::error::AppError> {
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

pub async fn run_event_loop(app: &mut app::App) -> Result<(), crate::error::AppError> {
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
        if panic_count >= runtime::render_recovery::MAX_RENDER_PANICS {
            tracing::error!(
                "Too many root render panics ({panic_count}), resetting minimal volatile state"
            );
            runtime::render_recovery::clear_render_error(app);
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
                    let msg = runtime::render_recovery::handle_render_panic(app, panic_err);
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
                        if runtime::app_events::handle_app_event_batch(app, events) {
                            needs_render = true;
                        }
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
                runtime::command_dispatch::dispatch_tui_command(app, cmd).await;
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

    // Cancel background tasks and kill shell commands before restoring terminal
    app.prepare_shutdown();

    terminal_guard.restore();
    Ok(())
}

#[cfg(test)]
mod shell_dispatch_tests {
    use crate::shell::types::{
        ShellCapturePolicy, ShellCommandId, ShellEnvPolicy, ShellOrigin, ShellRequest,
    };
    use crate::tui::app::App;
    use crate::tui::commands::shell::{
        handle_shell_ask, handle_shell_include, handle_shell_kill, handle_shell_list,
        handle_shell_show,
    };
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

    #[test]
    fn shell_list_shows_exit_code() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 1, "passing", b"", b"", Some(0));
        insert_completed_entry(&mut app, 2, "failing", b"", b"err\n", Some(101));
        handle_shell_list(&mut app);
        let toasts = get_toasts(&app);
        let text = toasts.join("\n");
        assert!(
            text.contains("exit=0"),
            "should show exit=0 for passing cmd, got: {text}"
        );
        assert!(
            text.contains("exit=101"),
            "should show exit=101 for failing cmd, got: {text}"
        );
    }

    #[test]
    fn shell_include_preserves_nonzero_exit_code() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo test",
            b"",
            b"test result: FAILED\n",
            Some(101),
        );
        handle_shell_include(&mut app, 1, "summary".to_string(), None);
        let msgs = get_user_messages(&app);
        let included = msgs.iter().find(|m| m.contains("cargo test")).unwrap();
        assert!(
            included.contains("Exit code: 101"),
            "should show actual exit code 101, got: {included}"
        );
    }

    #[test]
    fn shell_ask_preserves_nonzero_exit_code() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo check",
            b"",
            b"error[E0308]: mismatched\n",
            Some(1),
        );
        handle_shell_ask(&mut app, 1, "fix this error".to_string());
        let msgs = get_user_messages(&app);
        let included = msgs.iter().find(|m| m.contains("fix this error")).unwrap();
        assert!(
            included.contains("Exit code: 1"),
            "should show actual exit code 1, got: {included}"
        );
    }

    #[test]
    fn shell_kill_marks_entry_exited() {
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
        let entry = app.shell_store.get(ShellCommandId(1)).unwrap();
        assert_eq!(
            entry.status,
            crate::shell::types::ShellStatus::Killed,
            "killed entry should be marked as killed"
        );
        assert_eq!(
            entry.exit_code, None,
            "killed entry should have no exit code"
        );
    }

    #[test]
    fn shell_show_unknown_id_shows_warning() {
        let mut app = make_test_app();
        handle_shell_show(&mut app, 999);
        let toasts = get_toasts(&app);
        assert!(
            toasts
                .iter()
                .any(|t| t.contains("No shell command with id 999")),
            "should show not-found warning, got: {toasts:?}"
        );
    }

    #[test]
    fn shell_show_opens_dialog_with_metadata() {
        let mut app = make_test_app();
        insert_completed_entry(
            &mut app,
            1,
            "cargo test",
            b"running 1 test\nok\n",
            b"warning: unused\n",
            Some(0),
        );
        handle_shell_show(&mut app, 1);
        assert_eq!(
            app.ui_state.dialog,
            crate::tui::Dialog::ShellShow,
            "dialog should be set to ShellShow"
        );
        let dialog = app
            .dialog_state
            .shell_detail_dialog
            .as_ref()
            .expect("shell_detail_dialog should be Some");
        let content = dialog.content_lines();
        let text = content.join("\n");
        assert!(
            text.contains("cargo test"),
            "should show command, got: {text}"
        );
        assert!(
            text.contains("Exit:     0"),
            "should show exit code, got: {text}"
        );
        assert!(text.contains("exited"), "should show status, got: {text}");
        assert!(
            text.contains("running 1 test"),
            "should show stdout, got: {text}"
        );
        assert!(
            text.contains("warning: unused"),
            "should show stderr, got: {text}"
        );
    }

    #[test]
    fn shell_show_nonzero_exit_code() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 2, "cargo check", b"", b"error[E0308]\n", Some(1));
        handle_shell_show(&mut app, 2);
        let dialog = app
            .dialog_state
            .shell_detail_dialog
            .as_ref()
            .expect("shell_detail_dialog should be Some");
        let text = dialog.content_lines().join("\n");
        assert!(
            text.contains("Exit:     1"),
            "should show exit code 1, got: {text}"
        );
    }

    #[test]
    fn shell_show_running_command() {
        let mut app = make_test_app();
        let cmd_id = ShellCommandId(3);
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
        handle_shell_show(&mut app, 3);
        let dialog = app
            .dialog_state
            .shell_detail_dialog
            .as_ref()
            .expect("shell_detail_dialog should be Some");
        let text = dialog.content_lines().join("\n");
        assert!(
            text.contains("running"),
            "should show running status, got: {text}"
        );
        assert!(
            text.contains("sleep 999"),
            "should show command, got: {text}"
        );
    }

    #[test]
    fn shell_show_empty_output() {
        let mut app = make_test_app();
        insert_completed_entry(&mut app, 4, "true", b"", b"", Some(0));
        handle_shell_show(&mut app, 4);
        let dialog = app
            .dialog_state
            .shell_detail_dialog
            .as_ref()
            .expect("shell_detail_dialog should be Some");
        let text = dialog.content_lines().join("\n");
        assert!(
            text.contains("no output captured"),
            "should show no-output message, got: {text}"
        );
    }
}

#[cfg(test)]
mod async_cmd_tests {
    use super::*;
    use crate::tui::app::App;
    use crate::tui::commands::diagnostics::apply_doctor_result;
    use crate::tui::commands::import::apply_import_preview_loaded;
    use crate::tui::commands::memory::apply_memory_result;
    use crate::tui::commands::research::apply_research_run_loaded;
    use crate::tui::commands::sessions::{
        apply_session_messages_loaded, apply_session_mutation_finished, apply_sessions_reloaded,
    };
    use std::collections::HashMap;

    fn make_test_app() -> App {
        App::new_for_testing("/tmp".into())
    }

    fn test_session() -> crate::session::Session {
        crate::session::Session {
            id: "test-session-1".into(),
            project_id: "/tmp".into(),
            workspace_id: None,
            parent_id: None,
            slug: "test".into(),
            directory: "/tmp".into(),
            title: "Test Session".into(),
            version: "1".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            tags: Vec::new(),
            time_created: 0,
            time_updated: 0,
            time_compacting: None,
            time_archived: None,
            time_deleted: None,
        }
    }

    #[test]
    fn apply_sessions_reloaded_with_error_shows_toast() {
        let mut app = make_test_app();
        let request_id = app.dialog_state.session_reload_request.begin();
        apply_sessions_reloaded(
            &mut app,
            request_id,
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
        let request_id = app.dialog_state.session_reload_request.begin();
        apply_sessions_reloaded(&mut app, request_id, Vec::new(), HashMap::new(), None);
        assert!(!app.dialog_state.session_reload_request.is_loading());
    }

    #[test]
    fn apply_session_messages_loaded_with_error_preserves_old_messages() {
        let mut app = make_test_app();
        let request_id = app.dialog_state.session_messages_request.begin();
        app.messages_state
            .messages
            .add_user_message("old message".to_string(), None);
        apply_session_messages_loaded(
            &mut app,
            request_id,
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
    fn import_stale_preview_is_ignored() {
        let mut app = make_test_app();
        app.dialog_state.import_dialog =
            Some(crate::tui::components::dialogs::import::ImportDialog::default());

        // Start preview A
        let id_a = app.dialog_state.import_request.begin();
        // Start preview B (supersedes A)
        let id_b = app.dialog_state.import_request.begin();

        // Apply A's result -- should be ignored (stale)
        apply_import_preview_loaded(&mut app, id_a, Some(test_session()), 10, None);
        // Import dialog's preview should still be None (A was ignored, B hasn't arrived)
        let import = app.dialog_state.import_dialog.as_ref().unwrap();
        assert!(
            import.preview_session.is_none(),
            "preview A should be ignored, preview_session should still be None"
        );

        // Apply B's result -- should succeed
        apply_import_preview_loaded(&mut app, id_b, Some(test_session()), 5, None);
        let import = app.dialog_state.import_dialog.as_ref().unwrap();
        assert!(
            import.preview_session.is_some(),
            "preview B should be applied"
        );
    }

    #[test]
    fn import_cancelled_result_is_ignored() {
        let mut app = make_test_app();
        app.dialog_state.import_dialog =
            Some(crate::tui::components::dialogs::import::ImportDialog::default());

        let id = app.dialog_state.import_request.begin();
        app.dialog_state.import_request.cancel();

        // Apply result after cancel -- should be ignored
        apply_import_preview_loaded(&mut app, id, Some(test_session()), 5, None);
        let import = app.dialog_state.import_dialog.as_ref().unwrap();
        assert!(
            import.preview_session.is_none(),
            "result after cancel should be ignored"
        );
    }

    #[test]
    fn research_stale_run_is_ignored() {
        let mut app = make_test_app();
        app.dialog_state.research_browser = Some(
            crate::tui::components::dialogs::research::ResearchBrowserDialog::new(
                std::sync::Arc::new(Theme::dark()),
            ),
        );

        // Start load run A
        let id_a = app.dialog_state.research_request.begin();
        // Simulate A setting browser.loading = true
        if let Some(ref mut b) = app.dialog_state.research_browser {
            b.loading = true;
        }
        // Start load run B (supersedes A)
        let id_b = app.dialog_state.research_request.begin();

        // Apply A -- stale, should be ignored (loading stays true from B's perspective)
        apply_research_run_loaded(&mut app, id_a, "run-a".into(), None, None);
        assert!(
            app.dialog_state.research_browser.as_ref().unwrap().loading,
            "research should still be loading (A was stale)"
        );

        // Apply B -- should succeed and clear loading
        apply_research_run_loaded(&mut app, id_b, "run-b".into(), None, None);
        assert!(
            !app.dialog_state.research_browser.as_ref().unwrap().loading,
            "research should not be loading after B applied"
        );
    }

    #[test]
    fn close_dialog_cancels_import_request() {
        let mut app = make_test_app();
        app.dialog_state.import_dialog =
            Some(crate::tui::components::dialogs::import::ImportDialog::default());
        app.ui_state.dialog = Dialog::Import;

        let id = app.dialog_state.import_request.begin();
        assert!(app.dialog_state.import_request.is_loading());

        app.close_dialog();

        assert!(!app.dialog_state.import_request.is_loading());
        assert!(app.dialog_state.import_request.is_cancelled());
        // Old request ID should be stale
        assert!(!app.dialog_state.import_request.is_current(id));
    }

    #[test]
    fn close_dialog_cancels_research_request() {
        let mut app = make_test_app();
        app.dialog_state.research_browser = Some(
            crate::tui::components::dialogs::research::ResearchBrowserDialog::new(
                std::sync::Arc::new(Theme::dark()),
            ),
        );
        app.ui_state.dialog = Dialog::ResearchBrowser;

        let id = app.dialog_state.research_request.begin();
        assert!(app.dialog_state.research_request.is_loading());

        app.close_dialog();

        assert!(!app.dialog_state.research_request.is_loading());
        assert!(app.dialog_state.research_request.is_cancelled());
        assert!(!app.dialog_state.research_request.is_current(id));
    }

    #[test]
    fn session_messages_stale_result_ignored() {
        let mut app = make_test_app();
        // Begin a request for session A
        let id_a = app.dialog_state.session_messages_request.begin();

        // Simulate user switching to session B (supersedes request A)
        let id_b = app.dialog_state.session_messages_request.begin();
        app.session_state.session = Some(crate::session::Session {
            id: "session-b".into(),
            project_id: String::new(),
            workspace_id: None,
            parent_id: None,
            slug: String::new(),
            directory: String::new(),
            title: String::new(),
            version: String::new(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            revert: None,
            permission: None,
            tags: Vec::new(),
            time_created: 0,
            time_updated: 0,
            time_compacting: None,
            time_archived: None,
            time_deleted: None,
        });

        // Apply stale result for session A (should be ignored by staleness guard)
        apply_session_messages_loaded(&mut app, id_a, "session-a".into(), Vec::new(), None);

        // Apply current result for session B (should succeed)
        apply_session_messages_loaded(&mut app, id_b, "session-b".into(), Vec::new(), None);

        // Messages should be empty (no messages provided)
        assert_eq!(
            app.messages_state.messages.message_count(),
            0,
            "stale result should not overwrite current session"
        );
    }

    #[test]
    fn session_mutation_stale_is_ignored() {
        let mut app = make_test_app();
        let id1 = app.dialog_state.session_mutation_request.begin();
        let id2 = app.dialog_state.session_mutation_request.begin();

        // Apply mutation with stale id1 -- should be ignored
        apply_session_mutation_finished(
            &mut app,
            id1,
            SessionMutationOp::Delete,
            vec!["session-1".into()],
            "deleted".into(),
            false,
            None,
        );
        // No toast should appear for stale result
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            !toasts.iter().any(|t| t.contains("deleted")),
            "stale mutation result should not show toast"
        );

        // Apply with current id2 -- should succeed
        apply_session_mutation_finished(
            &mut app,
            id2,
            SessionMutationOp::Delete,
            vec!["session-2".into()],
            "deleted".into(),
            false,
            None,
        );
        let toasts: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect();
        assert!(
            toasts.iter().any(|t| t.contains("deleted")),
            "current mutation result should show toast"
        );
    }

    #[tokio::test]
    async fn prepare_shutdown_cancels_registered_tasks() {
        use crate::tui::task_lifecycle::TuiTaskKind;
        let mut app = make_test_app();

        // Spawn a few tasks that would block forever
        app.task_registry
            .spawn(TuiTaskKind::Command, "cmd1", async {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            });
        app.task_registry
            .spawn(TuiTaskKind::Research, "research1", async {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            });
        assert_eq!(app.task_registry.active_count(), 2);

        app.prepare_shutdown();

        // All registered tasks should be cancelled
        assert_eq!(app.task_registry.cancelled_count(), 2);
        assert_eq!(app.task_registry.active_count(), 0);
    }

    #[tokio::test]
    async fn prepare_shutdown_drains_shell_handles() {
        let mut app = make_test_app();

        // Insert a shell handle (aborts a task on kill)
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        });
        app.shell_handles.insert(
            42,
            crate::shell::runtime::ShellHandle::new_for_test(
                crate::shell::types::ShellCommandId(42),
                handle.abort_handle(),
            ),
        );
        assert_eq!(app.shell_handles.len(), 1);

        app.prepare_shutdown();

        // Shell handles should be drained
        assert!(app.shell_handles.is_empty());
    }
}
