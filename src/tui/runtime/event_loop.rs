//! TUI event loop — terminal input, bus events, command dispatch, and render cadence.

use super::app_events;
use super::command_dispatch;
use super::render_recovery;
use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::state::AppMode;
use crate::tui::app::SessionStatus;
use crate::tui::terminal::{create_terminal, TerminalGuard};
use crate::tui::{app, AppTerminal};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const STREAM_RENDER_INTERVAL: Duration = Duration::from_millis(16);
const SPINNER_RENDER_INTERVAL: Duration = Duration::from_millis(80);
const TOAST_RENDER_INTERVAL: Duration = Duration::from_millis(250);
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(75);

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
                project_id: None,
                workspace_id: None,
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
    let mut terminal_guard = TerminalGuard::enter()?;
    let mut terminal = create_terminal()?;
    let mut reader = EventStream::new();
    let mut bus_rx = GlobalEventBus::subscribe();
    let (cmd_tx, mut cmd_rx) = mpsc::channel(100);
    if app.tui_cmd_tx.is_none() {
        tracing::warn!("No TUI command sender available in app, using new channel");
    }
    app.tui_cmd_tx = Some(cmd_tx);

    let mut last_render: Option<Instant> = None;
    let mut needs_render = true;

    if let Some(ref mut watcher) = app.config_watcher {
        if let Err(e) = watcher.start().await {
            tracing::warn!("Failed to start config watcher: {}", e);
        }
    }

    while app.ui_state.running {
        // Reap on every iteration, including idle iterations. The timer
        // branch is disabled while there is no animation, resize debounce,
        // or toast, so timer-only reaping can retain completed task records
        // until the next active frame.
        app.task_registry.reap_finished();

        // Flush the tab manifest if a pending snapshot is due.
        // Best-effort: a flush failure is logged and retried on the
        // next iteration rather than blocking the event loop.
        if app.manifest_has_pending() {
            if let Err(e) = app.manifest_persistence.flush() {
                tracing::debug!(
                    target: "codegg::tui::manifest",
                    error = %e,
                    "deferred manifest flush failed; will retry"
                );
            }
        }
        let loop_start = std::time::Instant::now();
        let panic_count = app.ui_state.render_panic_count;

        // Progressive panic recovery:
        //   1+ failures: hide optional overlays/dialogs
        //   3+ failures: reset minimal volatile UI state
        if panic_count >= render_recovery::MAX_RENDER_PANICS {
            tracing::error!(
                "Too many root render panics ({panic_count}), resetting minimal volatile state"
            );
            render_recovery::clear_render_error(app);
            app.ui_state.dialog = app::Dialog::None;
            app.ui_state.timeline_visible = false;
            app.prompt_state.show_completions = false;
        } else if panic_count >= 1 {
            // On repeated root failures, hide optional overlays
            app.ui_state.dialog = app::Dialog::None;
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
                    let msg = render_recovery::handle_render_panic(app, panic_err);
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
                        if app_events::handle_app_event_batch(app, events) {
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
                        app.messages_state.toasts.add(crate::tui::components::toast::Toast::info("Configuration reloaded"));
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
                command_dispatch::dispatch_tui_command(app, cmd).await;
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
