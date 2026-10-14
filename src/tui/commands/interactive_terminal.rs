//! TUI slash-command handlers for interactive terminals (M003).
//!
//! These handlers drive the daemon M002 attach/resume protocol through
//! the TUI's `CoreClient` and project every answer into
//! [`crate::tui::interactive_terminal::InteractiveTerminalController`].
//! The controller owns no PTY/process state: all mutations are daemon
//! operations (`CoreRequest::InteractiveProcess*`), all UI state changes
//! are completions applied on the event loop through typed `TuiCommand`
//! variants with stale-completion guards
//! (`AsyncUiRequestState::finish`/`fail`).
//!
//! Conventions (per `architecture/tui.md` and AGENTS.md):
//!
//! - Initiation is synchronous: slash-command arms call `start_terminal_*`,
//!   which validates, begins the async request generation, and spawns a
//!   registered background task. No `.await` in dispatch.
//! - `TUI close detaches`: closing the terminal dialog releases the
//!   caller-owned attachment (the process is unaffected) unless the user
//!   explicitly terminates.
//! - Workspace routing is canonical: the workspace comes from the active
//!   session's binding, never from process-global cwd state.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::interactive_process::{
    InteractiveOutputChunk, InteractiveProcessCreateRequest, InteractiveProcessMetadata,
    InteractiveResync,
};
use crate::tui::app::{App, TuiCommand};
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::interactive_terminal::{classify_key, TerminalKey, TerminalKeyAction};
use crate::tui::task_lifecycle::TuiTaskKind;

/// Resolve the canonical workspace for a new terminal from the active
/// session's binding.
///
/// Multi-project routing primitive: each project's session drives its
/// own terminals. There is deliberately no `current_dir` fallback — a
/// terminal without a canonical workspace binding is refused with an
/// actionable error rather than attached to an ambient directory.
pub(crate) fn resolve_terminal_workspace(app: &App) -> Result<String, String> {
    let session = app.session_state.session.as_ref().ok_or_else(|| {
        "No active session; open a project session before creating a terminal".to_string()
    })?;
    session.workspace_id.clone().ok_or_else(|| {
        "The current session has no canonical workspace binding; cannot create a terminal"
            .to_string()
    })
}

/// Currently viewed terminal handle for dialog-scoped actions.
pub(crate) fn active_terminal_handle(app: &App) -> Option<String> {
    app.dialog_state.terminal_detail_handle.clone().or_else(|| {
        app.interactive_terminals
            .active_handle()
            .map(str::to_string)
    })
}

// ── Create ──────────────────────────────────────────────────────────

/// Start an interactive terminal from `/terminal-create <argv...>`.
pub(crate) fn start_terminal_create(app: &mut App, argv: Vec<String>) {
    if argv.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /terminal-create <command> [args...]");
        return;
    }
    let workspace_id = match resolve_terminal_workspace(app) {
        Ok(workspace_id) => workspace_id,
        Err(error) => {
            app.messages_state.toasts.error(&error);
            return;
        }
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; cannot create a terminal");
        return;
    };
    let request_id = app.dialog_state.terminal_request.begin();
    let command_label = argv.join(" ");
    let (cols, rows) = terminal_viewport_size();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_create",
        async move {
            let request = crate::core::new_request(
                format!("terminal-create-{}", uuid::Uuid::new_v4()),
                CoreRequest::InteractiveProcessCreate {
                    request: InteractiveProcessCreateRequest {
                        workspace_id: workspace_id.clone(),
                        argv,
                        cwd: None,
                        env_overrides: Vec::new(),
                        cols: Some(cols),
                        rows: Some(rows),
                        scrollback_bytes: None,
                    },
                },
            );
            let result = match core_client.request(request).await {
                Ok(CoreResponse::InteractiveProcessCreated { handle, metadata }) => {
                    TuiCommand::TerminalCreateFinished {
                        request_id,
                        handle: Some(handle),
                        workspace_id: Some(metadata.workspace_id),
                        command_label: Some(command_label),
                        error: None,
                    }
                }
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalCreateFinished {
                    request_id,
                    handle: None,
                    workspace_id: None,
                    command_label: None,
                    error: Some(format!("{code}: {message}")),
                },
                Ok(other) => TuiCommand::TerminalCreateFinished {
                    request_id,
                    handle: None,
                    workspace_id: None,
                    command_label: None,
                    error: Some(format!("unexpected create response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalCreateFinished {
                    request_id,
                    handle: None,
                    workspace_id: None,
                    command_label: None,
                    error: Some(format!("create request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

/// argv for the everyday interactive shell.
///
/// `$SHELL` is the user's own answer to "which shell do I run", and the
/// `sh` fallback matches [`crate::shell::ShellRuntime`] so this surface and
/// human `!cmd` execution cannot disagree about it. No `-l`: the daemon gives
/// the process a real PTY, so the shell detects an interactive terminal on its
/// own, and a login flag would only re-run rc files nobody asked for.
pub(crate) fn default_shell_argv() -> Vec<String> {
    let shell = std::env::var("SHELL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "cmd.exe".to_string()
            } else {
                "sh".to_string()
            }
        });
    vec![shell]
}

/// Start the default shell and show it in the box.
///
/// This is what `/terminal` with no usable terminal behind it calls. Opening a
/// terminal is expected to produce a terminal: making the user first invent
/// an argv for `/terminal-create` and then re-run `/terminal` to see it was
/// the single thing standing between this command and being usable.
pub(crate) fn spawn_default_terminal(app: &mut App) {
    start_terminal_create(app, default_shell_argv());
}

/// Start the default shell *for the side panel*.
///
/// The panel is already on screen, so the completion has to land there instead
/// of throwing a box over the surface the user is reading. The route is one
/// bit of correlation state because create and attach are asynchronous and
/// several frames wide, and a panel cannot see app state by construction.
pub(crate) fn spawn_default_terminal_for_panel(app: &mut App) {
    if app.terminal_panel_spawn {
        // A create is already in flight for this panel. A second one would
        // race the first to become the active handle and orphan a shell.
        return;
    }
    app.terminal_panel_spawn = true;
    start_terminal_create(app, default_shell_argv());
}

/// Match the PTY to the usable dialog area, with a minimum useful viewport.
pub(crate) fn terminal_viewport_size() -> (u16, u16) {
    let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
    (
        width.saturating_sub(4).clamp(
            1,
            crate::tui::interactive_terminal::MAX_TERMINAL_SCREEN_COLS,
        ),
        height.saturating_sub(12).clamp(
            1,
            crate::tui::interactive_terminal::MAX_TERMINAL_SCREEN_ROWS,
        ),
    )
}

/// Resize the active PTY after the outer terminal has settled to a new size.
pub(crate) fn resize_terminal_to_viewport(app: &mut App) {
    let Some(handle) = app.dialog_state.terminal_detail_handle.clone() else {
        return;
    };
    let (cols, rows) = terminal_viewport_size();
    if app.interactive_terminals.dimensions(&handle) != Some((cols, rows)) {
        start_terminal_resize(app, handle, cols, rows);
    }
}

/// Apply a terminal-create completion on the event loop.
pub(crate) fn apply_terminal_create_finished(
    app: &mut App,
    request_id: u64,
    handle: Option<String>,
    workspace_id: Option<String>,
    command_label: Option<String>,
    error: Option<String>,
) {
    if let Some(error) = error {
        // Drop the panel route too: a failed create must not divert the next
        // unrelated terminal into the side panel.
        app.terminal_panel_spawn = false;
        if app
            .dialog_state
            .terminal_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.error(&error);
        }
        return;
    }
    let (Some(handle), Some(workspace_id), Some(command_label)) =
        (handle, workspace_id, command_label)
    else {
        app.terminal_panel_spawn = false;
        if app.dialog_state.terminal_request.fail(
            request_id,
            "terminal create completed without a handle".to_string(),
        ) {
            app.messages_state
                .toasts
                .error("Terminal create completed without a handle");
        }
        return;
    };
    if !app.dialog_state.terminal_request.finish(request_id) {
        return;
    }
    if !app
        .interactive_terminals
        .apply_created(handle.clone(), workspace_id, command_label, 80, 24)
    {
        app.terminal_panel_spawn = false;
        app.messages_state
            .toasts
            .warning("Terminal created but the TUI view is full; use /terminal-list");
        return;
    }
    // Creation mints the handle but no attachment: attach immediately so
    // the user sees output without a second command.
    start_terminal_attach(app, handle);
}

// ── List ────────────────────────────────────────────────────────────

/// Start a bounded process-list refresh from `/terminal-list`.
pub(crate) fn start_terminal_list(app: &mut App) {
    let workspace_id = match resolve_terminal_workspace(app) {
        Ok(workspace_id) => workspace_id,
        Err(error) => {
            app.messages_state.toasts.error(&error);
            return;
        }
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; cannot list terminals");
        return;
    };
    let request_id = app.dialog_state.terminal_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_list",
        async move {
            let request = crate::core::new_request(
                format!("terminal-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::InteractiveProcessList {
                    workspace_id: Some(workspace_id),
                    limit: None,
                },
            );
            let result = match core_client.request(request).await {
                Ok(CoreResponse::InteractiveProcessList {
                    processes,
                    truncated,
                }) => {
                    let _ = truncated;
                    TuiCommand::TerminalListFinished {
                        request_id,
                        processes: Some(processes),
                        error: None,
                    }
                }
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalListFinished {
                    request_id,
                    processes: None,
                    error: Some(format!("{code}: {message}")),
                },
                Ok(other) => TuiCommand::TerminalListFinished {
                    request_id,
                    processes: None,
                    error: Some(format!("unexpected list response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalListFinished {
                    request_id,
                    processes: None,
                    error: Some(format!("list request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

/// Apply a terminal-list completion on the event loop.
pub(crate) fn apply_terminal_list_finished(
    app: &mut App,
    request_id: u64,
    processes: Option<Vec<InteractiveProcessMetadata>>,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .terminal_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.error(&error);
        }
        return;
    }
    let Some(processes) = processes else {
        if app.dialog_state.terminal_request.fail(
            request_id,
            "terminal list completed without processes".to_string(),
        ) {
            app.messages_state
                .toasts
                .error("Terminal list completed without processes");
        }
        return;
    };
    if !app.dialog_state.terminal_request.finish(request_id) {
        return;
    }
    app.interactive_terminals.apply_list(&processes);
    let lines = app.interactive_terminals.list_lines();
    if lines.is_empty() {
        app.messages_state
            .toasts
            .info("No interactive terminals in this workspace");
    } else if lines.len() > 5 {
        app.open_info_dialog(
            crate::tui::components::dialogs::info::InfoType::TerminalShow,
            lines,
        );
    } else {
        app.messages_state.toasts.info(&lines.join("\n"));
    }
}

// ── Attach / resume ─────────────────────────────────────────────────

/// Start an attach from `/terminal-attach <handle>` (or internally after
/// create and on reconnect re-attach).
pub(crate) fn start_terminal_attach(app: &mut App, handle: String) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; cannot attach to the terminal");
        return;
    };
    // Resume from the retained cursor when re-attaching after a
    // disconnect; a fresh view starts at the oldest retained output.
    let from_seq = app.interactive_terminals.resume_cursor(&handle);
    let request_id = app.dialog_state.terminal_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_attach",
        async move {
            let request = crate::core::new_request(
                format!("terminal-attach-{}", uuid::Uuid::new_v4()),
                CoreRequest::InteractiveProcessAttach {
                    handle: handle.clone(),
                    from_seq,
                    max_bytes: None,
                },
            );
            let result = match core_client.request(request).await {
                Ok(CoreResponse::InteractiveProcessAttached {
                    attachment_id,
                    handle,
                    chunk,
                    resync,
                }) => TuiCommand::TerminalAttachFinished {
                    request_id,
                    handle,
                    attachment_id: Some(attachment_id),
                    chunk: Some(chunk),
                    resync,
                    error: None,
                },
                Ok(CoreResponse::InteractiveProcessResyncRequired {
                    attachment_id,
                    resync,
                }) => TuiCommand::TerminalAttachFinished {
                    request_id,
                    handle,
                    attachment_id: Some(attachment_id),
                    chunk: None,
                    resync: Some(resync),
                    error: None,
                },
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalAttachFinished {
                    request_id,
                    handle,
                    attachment_id: None,
                    chunk: None,
                    resync: None,
                    error: Some(terminal_error_hint(&code, &message)),
                },
                Ok(other) => TuiCommand::TerminalAttachFinished {
                    request_id,
                    handle,
                    attachment_id: None,
                    chunk: None,
                    resync: None,
                    error: Some(format!("unexpected attach response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalAttachFinished {
                    request_id,
                    handle,
                    attachment_id: None,
                    chunk: None,
                    resync: None,
                    error: Some(format!("attach request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

/// Apply an attach completion on the event loop.
pub(crate) fn apply_terminal_attach_finished(
    app: &mut App,
    request_id: u64,
    handle: String,
    attachment_id: Option<String>,
    chunk: Option<InteractiveOutputChunk>,
    resync: Option<InteractiveResync>,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .terminal_request
            .fail(request_id, error.clone())
        {
            app.terminal_panel_spawn = false;
            app.messages_state.toasts.error(&error);
        }
        return;
    }
    // A resync-only answer (no chunk) still carries a usable attachment.
    if chunk.is_none() && resync.is_none() {
        if app.dialog_state.terminal_request.fail(
            request_id,
            "terminal attach completed without output".to_string(),
        ) {
            app.terminal_panel_spawn = false;
            app.messages_state
                .toasts
                .error("Terminal attach completed without output");
        }
        return;
    }
    if !app.dialog_state.terminal_request.finish(request_id) {
        return;
    }
    let Some(attachment_id) = attachment_id else {
        app.terminal_panel_spawn = false;
        app.messages_state
            .toasts
            .error("Terminal attach completed without an attachment");
        return;
    };
    // A view for the handle must exist (create registers it; list
    // registers unknown handles). If the daemon knows a handle the TUI
    // never saw, register a minimal view so output is never dropped.
    if app.interactive_terminals.view(&handle).is_none() {
        let workspace_id =
            resolve_terminal_workspace(app).unwrap_or_else(|_| "unknown-workspace".to_string());
        app.interactive_terminals.apply_created(
            handle.clone(),
            workspace_id,
            "interactive".to_string(),
            80,
            24,
        );
    }
    if let Some(chunk) = chunk.as_ref() {
        match app.interactive_terminals.apply_attached(
            &handle,
            attachment_id,
            chunk,
            resync.as_ref(),
        ) {
            Ok(Some(notice)) => app.messages_state.toasts.warning(&notice),
            Ok(None) => {}
            Err(error) => {
                app.messages_state.toasts.error(&error);
                return;
            }
        }
    } else if let Some(resync) = resync {
        app.interactive_terminals.apply_resync(&handle, resync);
        app.messages_state
            .toasts
            .warning("Terminal requires resync; use /terminal-resume to follow live output");
    }
    if app.terminal_panel_spawn {
        // The side panel is the surface the user is already reading, so the
        // new shell lands there. Opening the box as well would cover it and
        // leave two views of one PTY fighting over the keyboard.
        app.terminal_panel_spawn = false;
        app.panel_host.show_terminal(Some(&handle));
    } else {
        handle_terminal_show(app, handle);
    }
}

/// Start a resume from `/terminal-resume [handle]` (defaults to the
/// active view). Follows live output after a resync notice.
pub(crate) fn start_terminal_resume(app: &mut App, handle: Option<String>) {
    let Some(handle) = handle.or_else(|| active_terminal_handle(app)) else {
        app.messages_state
            .toasts
            .warning("No terminal selected; use /terminal-list then /terminal-attach <handle>");
        return;
    };
    let (Some(attachment_id), Some(from_seq)) = (
        app.interactive_terminals.attachment_id(&handle),
        app.interactive_terminals.resume_cursor(&handle),
    ) else {
        app.messages_state.toasts.warning(&format!(
            "Terminal {handle} has no attachment; use /terminal-attach {handle}"
        ));
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; cannot resume terminal output");
        return;
    };
    let request_id = app.dialog_state.terminal_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_resume",
        async move {
            let request = crate::core::new_request(
                format!("terminal-resume-{}", uuid::Uuid::new_v4()),
                CoreRequest::InteractiveProcessResume {
                    attachment_id: attachment_id.clone(),
                    from_seq,
                    max_bytes: None,
                },
            );
            let result = match core_client.request(request).await {
                Ok(CoreResponse::InteractiveProcessResumed { chunk, .. }) => {
                    TuiCommand::TerminalResumeFinished {
                        request_id,
                        handle,
                        chunk: Some(chunk),
                        resync: None,
                        error: None,
                    }
                }
                Ok(CoreResponse::InteractiveProcessResyncRequired { resync, .. }) => {
                    TuiCommand::TerminalResumeFinished {
                        request_id,
                        handle,
                        chunk: None,
                        resync: Some(resync),
                        error: None,
                    }
                }
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalResumeFinished {
                    request_id,
                    handle,
                    chunk: None,
                    resync: None,
                    error: Some(terminal_error_hint(&code, &message)),
                },
                Ok(other) => TuiCommand::TerminalResumeFinished {
                    request_id,
                    handle,
                    chunk: None,
                    resync: None,
                    error: Some(format!("unexpected resume response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalResumeFinished {
                    request_id,
                    handle,
                    chunk: None,
                    resync: None,
                    error: Some(format!("resume request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

/// Apply a resume completion on the event loop.
pub(crate) fn apply_terminal_resume_finished(
    app: &mut App,
    request_id: u64,
    handle: String,
    chunk: Option<InteractiveOutputChunk>,
    resync: Option<InteractiveResync>,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .terminal_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.error(&error);
        }
        return;
    }
    if !app.dialog_state.terminal_request.finish(request_id) {
        return;
    }
    if let Some(chunk) = chunk {
        match app.interactive_terminals.apply_resumed(&handle, &chunk) {
            Ok(Some(notice)) => app.messages_state.toasts.warning(&notice),
            Ok(None) => {}
            Err(error) => {
                app.messages_state.toasts.error(&error);
                return;
            }
        }
    } else if let Some(resync) = resync {
        app.interactive_terminals.apply_resync(&handle, resync);
        app.messages_state
            .toasts
            .warning("Terminal requires resync; use /terminal-resume to follow live output");
    }
    refresh_terminal_dialog(app, &handle);
}

// ── Input / resize (bounded, coalesced) ─────────────────────────────

/// Queue explicit input bytes and flush them through one bounded M002
/// input operation. Used by `/terminal-send` and the focus key router.
pub(crate) fn send_terminal_input(app: &mut App, handle: String, bytes: Vec<u8>) {
    if let Err(error) = app.interactive_terminals.queue_input(&handle, &bytes) {
        app.messages_state.toasts.warning(&error.to_string());
        return;
    }
    flush_terminal_input(app, handle);
}

/// Drain coalesced pending input into one bounded input operation.
///
/// Also drains any pending resize queued by a docked panel. Sharing the drain
/// is deliberate: the panel sizes its PTY from the render path, which has no
/// seam of its own, so the resize would otherwise sit in the queue until the
/// user happened to type something.
pub(crate) fn flush_terminal_input(app: &mut App, handle: String) {
    flush_terminal_resize(app, &handle);
    let (Some(attachment_id), Some(pending)) = (
        app.interactive_terminals.attachment_id(&handle),
        app.interactive_terminals.take_pending_input(&handle),
    ) else {
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; terminal input dropped");
        return;
    };
    let request_id = app.dialog_state.terminal_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_input",
        async move {
            let result = match core_client
                .request(crate::core::new_request(
                    format!("terminal-input-{}", uuid::Uuid::new_v4()),
                    CoreRequest::InteractiveProcessInput {
                        attachment_id,
                        data_b64: B64.encode(&pending),
                    },
                ))
                .await
            {
                Ok(CoreResponse::InteractiveProcessInputAccepted { .. }) => {
                    TuiCommand::TerminalOpFinished {
                        request_id,
                        op: "input".to_string(),
                        handle,
                        exit_code: None,
                        exit_signal: None,
                        cols: None,
                        rows: None,
                        error: None,
                    }
                }
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "input".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(terminal_error_hint(&code, &message)),
                },
                Ok(other) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "input".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("unexpected input response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "input".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("input request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

/// Flush a resize already queued on the controller through one M002 resize
/// operation.
///
/// Split from [`start_terminal_resize`] because the docked panel queues its
/// own size from the render path and has no `App` to reach: the panel leaves
/// the size on the controller and this drain, shared with the input flush,
/// sends it.
pub(crate) fn flush_terminal_resize(app: &mut App, handle: &str) {
    let (Some(attachment_id), Some((cols, rows))) = (
        app.interactive_terminals.attachment_id(handle),
        app.interactive_terminals.take_pending_resize(handle),
    ) else {
        return;
    };
    send_terminal_resize(app, handle.to_string(), attachment_id, cols, rows);
}

/// Queue a resize from `/terminal-resize <cols> <rows>` and flush the
/// coalesced (last-wins) size through one M002 resize operation.
pub(crate) fn start_terminal_resize(app: &mut App, handle: String, cols: u16, rows: u16) {
    if let Err(error) = app.interactive_terminals.queue_resize(&handle, cols, rows) {
        app.messages_state.toasts.warning(&error.to_string());
        return;
    }
    flush_terminal_resize(app, &handle);
}

/// Issue one M002 resize for an already-resolved attachment and size.
fn send_terminal_resize(
    app: &mut App,
    handle: String,
    attachment_id: String,
    cols: u16,
    rows: u16,
) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; terminal resize dropped");
        return;
    };
    let request_id = app.dialog_state.terminal_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_resize",
        async move {
            let result = match core_client
                .request(crate::core::new_request(
                    format!("terminal-resize-{}", uuid::Uuid::new_v4()),
                    CoreRequest::InteractiveProcessResize {
                        attachment_id,
                        cols,
                        rows,
                    },
                ))
                .await
            {
                Ok(CoreResponse::InteractiveProcessResized { cols, rows, .. }) => {
                    TuiCommand::TerminalOpFinished {
                        request_id,
                        op: "resize".to_string(),
                        handle,
                        exit_code: None,
                        exit_signal: None,
                        cols: Some(cols),
                        rows: Some(rows),
                        error: None,
                    }
                }
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "resize".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(terminal_error_hint(&code, &message)),
                },
                Ok(other) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "resize".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("unexpected resize response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "resize".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("resize request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

// ── Detach / terminate / remove ─────────────────────────────────────

/// Detach from `/terminal-detach [handle]`: releases the caller-owned
/// attachment; the process is unaffected and scrollback is retained.
pub(crate) fn start_terminal_detach(app: &mut App, handle: Option<String>) {
    let Some(handle) = handle.or_else(|| active_terminal_handle(app)) else {
        app.messages_state
            .toasts
            .warning("No terminal selected; use /terminal-list then /terminal-detach <handle>");
        return;
    };
    let Some(attachment_id) = app.interactive_terminals.attachment_id(&handle) else {
        // No attachment to release: still clear local focus state so the
        // view is truthful.
        app.interactive_terminals.apply_detached(&handle);
        refresh_terminal_dialog(app, &handle);
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; cannot detach the terminal");
        return;
    };
    let request_id = app.dialog_state.terminal_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_detach",
        async move {
            let result = match core_client
                .request(crate::core::new_request(
                    format!("terminal-detach-{}", uuid::Uuid::new_v4()),
                    CoreRequest::InteractiveProcessDetach { attachment_id },
                ))
                .await
            {
                Ok(CoreResponse::InteractiveProcessDetached { .. }) => {
                    TuiCommand::TerminalOpFinished {
                        request_id,
                        op: "detach".to_string(),
                        handle,
                        exit_code: None,
                        exit_signal: None,
                        cols: None,
                        rows: None,
                        error: None,
                    }
                }
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "detach".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(terminal_error_hint(&code, &message)),
                },
                Ok(other) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "detach".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("unexpected detach response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "detach".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("detach request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

/// Terminate from `/terminal-terminate [handle]`: bounded SIGTERM then
/// SIGKILL escalation owned by the daemon engine.
pub(crate) fn start_terminal_terminate(app: &mut App, handle: Option<String>) {
    let Some(handle) = handle.or_else(|| active_terminal_handle(app)) else {
        app.messages_state
            .toasts
            .warning("No terminal selected; use /terminal-list then /terminal-terminate <handle>");
        return;
    };
    let Some(attachment_id) = app.interactive_terminals.attachment_id(&handle) else {
        app.messages_state.toasts.warning(&format!(
            "Terminal {handle} has no attachment; attach before terminating"
        ));
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; cannot terminate the terminal");
        return;
    };
    let request_id = app.dialog_state.terminal_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_terminate",
        async move {
            let result = match core_client
                .request(crate::core::new_request(
                    format!("terminal-terminate-{}", uuid::Uuid::new_v4()),
                    CoreRequest::InteractiveProcessTerminate { attachment_id },
                ))
                .await
            {
                Ok(CoreResponse::InteractiveProcessTerminated {
                    handle,
                    exit_code,
                    exit_signal,
                    ..
                }) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "terminate".to_string(),
                    handle,
                    exit_code,
                    exit_signal,
                    cols: None,
                    rows: None,
                    error: None,
                },
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "terminate".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(terminal_error_hint(&code, &message)),
                },
                Ok(other) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "terminate".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("unexpected terminate response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "terminate".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("terminate request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

/// Remove from `/terminal-remove [handle]`: terminates live processes
/// first (bounded escalation), then drops the handle and frees
/// scrollback. The view disappears from the TUI.
pub(crate) fn start_terminal_remove(app: &mut App, handle: Option<String>) {
    let Some(handle) = handle.or_else(|| active_terminal_handle(app)) else {
        app.messages_state
            .toasts
            .warning("No terminal selected; use /terminal-list then /terminal-remove <handle>");
        return;
    };
    let Some(attachment_id) = app.interactive_terminals.attachment_id(&handle) else {
        app.messages_state.toasts.warning(&format!(
            "Terminal {handle} has no attachment; attach before removing"
        ));
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; cannot remove the terminal");
        return;
    };
    let request_id = app.dialog_state.terminal_request.begin();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_remove",
        async move {
            let result = match core_client
                .request(crate::core::new_request(
                    format!("terminal-remove-{}", uuid::Uuid::new_v4()),
                    CoreRequest::InteractiveProcessRemove { attachment_id },
                ))
                .await
            {
                Ok(CoreResponse::InteractiveProcessRemoved { handle, .. }) => {
                    TuiCommand::TerminalOpFinished {
                        request_id,
                        op: "remove".to_string(),
                        handle,
                        exit_code: None,
                        exit_signal: None,
                        cols: None,
                        rows: None,
                        error: None,
                    }
                }
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "remove".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(terminal_error_hint(&code, &message)),
                },
                Ok(other) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "remove".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("unexpected remove response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalOpFinished {
                    request_id,
                    op: "remove".to_string(),
                    handle,
                    exit_code: None,
                    exit_signal: None,
                    cols: None,
                    rows: None,
                    error: Some(format!("remove request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

/// Apply an input/resize/detach/terminate/remove completion.
///
/// Nine arguments mirror the `TuiCommand::TerminalOpFinished` completion
/// shape one-to-one (op outcome + exit/size echoes + error); splitting
/// them would diverge the completion from its apply site.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_terminal_op_finished(
    app: &mut App,
    request_id: u64,
    op: String,
    handle: String,
    exit_code: Option<i32>,
    exit_signal: Option<i32>,
    cols: Option<u16>,
    rows: Option<u16>,
    error: Option<String>,
) {
    if let Some(error) = error {
        if app
            .dialog_state
            .terminal_request
            .fail(request_id, error.clone())
        {
            app.messages_state.toasts.error(&error);
        }
        return;
    }
    if !app.dialog_state.terminal_request.finish(request_id) {
        return;
    }
    match op.as_str() {
        "detach" => {
            app.interactive_terminals.apply_detached(&handle);
            app.messages_state.toasts.info(&format!(
                "Detached from {handle}; the process keeps running"
            ));
        }
        "terminate" => {
            app.interactive_terminals
                .apply_terminated(&handle, exit_code, exit_signal);
            app.messages_state.toasts.info(&format!(
                "Terminal {handle} terminated (code={} signal={})",
                exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                exit_signal
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".to_string())
            ));
        }
        "remove" => {
            app.interactive_terminals.apply_removed(&handle);
            if app.dialog_state.terminal_detail_handle.as_deref() == Some(handle.as_str()) {
                app.dialog_state.terminal_detail_handle = None;
                if matches!(app.ui_state.dialog, crate::tui::Dialog::Terminal) {
                    app.close_dialog();
                }
            }
            app.messages_state
                .toasts
                .info(&format!("Terminal {handle} removed"));
            return;
        }
        "resize" => {
            // The daemon echoes the applied size; record it so the view
            // matches the authoritative value.
            if let (Some(cols), Some(rows)) = (cols, rows) {
                app.interactive_terminals
                    .applied_resize(&handle, cols, rows);
                app.messages_state
                    .toasts
                    .info(&format!("Terminal {handle} resized to {cols}x{rows}"));
            } else {
                app.messages_state
                    .toasts
                    .info(&format!("Terminal {handle} resized"));
            }
        }
        _ => {}
    }
    refresh_terminal_dialog(app, &handle);
}

// ── Dialog view ─────────────────────────────────────────────────────

/// Render one terminal into the terminal dialog (`/terminal-show`).
/// Show a terminal in the box. The one entry point for every caller that
/// wants the terminal surface open — `/terminal`, `/terminal-show`, and the
/// live-refresh path all land here so they cannot disagree about which
/// handle is on screen.
pub(crate) fn show_terminal(app: &mut App, handle: String) {
    handle_terminal_show(app, handle);
}

pub(crate) fn handle_terminal_show(app: &mut App, handle: String) {
    if app.interactive_terminals.view(&handle).is_none() {
        app.messages_state
            .toasts
            .warning(&format!("Unknown terminal {handle}; use /terminal-list"));
        return;
    }
    app.interactive_terminals.set_active(&handle);
    app.dialog_state.terminal_detail_handle = Some(handle);
    let shown = app
        .dialog_state
        .terminal_detail_handle
        .clone()
        .expect("terminal detail handle just set");
    refresh_terminal_dialog(app, &shown);
    app.ui_state.dialog = crate::tui::Dialog::Terminal;
}

/// Re-render the open terminal dialog for `handle` when it is the
/// currently viewed terminal. Never opens or focuses anything: pure
/// projection refresh for completion/event paths.
pub(crate) fn refresh_terminal_dialog(app: &mut App, handle: &str) {
    if app.dialog_state.terminal_detail_handle.as_deref() != Some(handle) {
        return;
    }
    let Some(headers) = app.interactive_terminals.header_lines(handle) else {
        return;
    };
    let mut lines = headers;
    lines.push(String::new());
    let offset = app
        .interactive_terminals
        .viewport_offset(handle)
        .unwrap_or(0);
    lines.push(if offset > 0 {
        format!("── output ({offset} rows back) ──")
    } else {
        "── output (following) ──".to_string()
    });
    // The box has its own wrapped-row scroller on top of the controller's
    // scrollback offset. When the terminal is following, keep that scroller
    // pinned to the newest row; when the reader has scrolled back, leave it
    // alone so the two offsets cannot fight each other.
    let follow = offset == 0;
    let mut styled_content: Vec<ratatui::text::Line<'static>> = lines
        .iter()
        .map(|line| {
            ratatui::text::Line::from(ratatui::text::Span::styled(
                line.clone(),
                ratatui::style::Style::default().fg(app.ui_state.theme.foreground),
            ))
        })
        .collect();
    let selection_style = ratatui::style::Style::default().bg(app.ui_state.theme.selection);
    let output = app
        .interactive_terminals
        .render_view(handle, 200, selection_style)
        .unwrap_or_default();
    if output.is_empty() {
        styled_content.push(ratatui::text::Line::from("(no output yet)"));
    } else {
        styled_content.extend(output);
    }
    let footer = format!(
        "j/k scroll  |  ^-u/^-d page  |  e/b word  |  v select chars  |  x select lines  |  d delete  |  space+y copy  |  C clear  |  i focus  |  ^-] unfocus  |  Esc closes{}",
        if app.interactive_terminals.redaction_count(handle) > 0 {
            format!(
                "  |  {} deleted",
                app.interactive_terminals.redaction_count(handle)
            )
        } else {
            String::new()
        },
    );
    if let Some(dialog) = app
        .focus_manager
        .dialog_mut_any::<crate::tui::components::dialogs::info::InfoDialog>()
    {
        dialog.set_info_type(crate::tui::components::dialogs::info::InfoType::TerminalShow);
        dialog.set_styled_content_preserving_scroll(styled_content);
        if follow {
            dialog.scroll_to_end();
        }
        dialog.set_theme(&app.ui_state.theme);
        dialog.set_custom_footer(footer);
    } else {
        let mut dialog = crate::tui::components::dialogs::info::InfoDialog::new(
            std::sync::Arc::clone(&app.ui_state.theme),
            crate::tui::components::dialogs::info::InfoType::TerminalShow,
            lines,
        );
        dialog.set_styled_content(styled_content);
        dialog.set_custom_footer(footer);
        app.focus_manager.push(Box::new(dialog));
    }
}

/// Focus one terminal's keyboard from `/terminal-focus [handle]`.
pub(crate) fn focus_terminal(app: &mut App, handle: Option<String>) {
    let Some(handle) = handle.or_else(|| active_terminal_handle(app)) else {
        app.messages_state
            .toasts
            .warning("No terminal selected; use /terminal-list then /terminal-focus <handle>");
        return;
    };
    if app.interactive_terminals.focus(&handle) {
        app.messages_state.toasts.info(&format!(
            "Terminal {handle} focused: keystrokes go to the process; Ctrl-] leaves focus"
        ));
        refresh_terminal_dialog(app, &handle);
    } else {
        app.messages_state.toasts.warning(&format!(
            "Cannot focus {handle}: attach a live terminal first (/terminal-attach {handle})"
        ));
    }
}

// ── Panel surface ───────────────────────────────────────────────────
//
// The side panel is a peer of the box, not a second implementation: it maps
// keys through the same `map_key`/`classify_key` gate and sends through the
// same `send_terminal_input`. These four functions are the only app-side
// entry points, because focusing and reporting need message state the panel
// deliberately cannot see.

/// Take the PTY's keyboard for the terminal the panel is showing.
pub(crate) fn focus_terminal_panel(app: &mut App, handle: &str) {
    if app.interactive_terminals.view(handle).is_none() {
        app.panel_host.forget_terminal(handle);
        app.messages_state
            .toasts
            .warning("That terminal is gone — press i for a new shell");
        return;
    }
    if app.interactive_terminals.focus(handle) {
        app.messages_state.toasts.info(&format!(
            "Terminal {handle} focused: keystrokes go to the process; Ctrl-] leaves focus"
        ));
        return;
    }
    // Focus was refused, so this view cannot take keystrokes — and that
    // refusal is the whole reason the panel read as broken. Two distinct
    // causes need opposite answers: a live process that only lost its
    // attachment is re-attached, while an exited or gone one can only be
    // replaced. Typing into either used to produce `interactive_not_running`
    // with no way forward from the surface the user was standing on.
    if app.interactive_terminals.is_live(handle) {
        start_terminal_attach(app, handle.to_string());
    } else {
        spawn_default_terminal_for_panel(app);
    }
}

/// Return the keyboard to the host, leaving the PTY running.
pub(crate) fn blur_terminal_panel(app: &mut App, handle: &str) {
    app.interactive_terminals.escape(handle);
    app.messages_state
        .toasts
        .info("Terminal unfocused: keystrokes go to the prompt; Shift+← leaves the panel");
}

/// Clear the local scrollback and screen for `handle`.
///
/// Local only: the process keeps running and the daemon keeps streaming.
/// Sequence tracking is preserved by `clear_scrollback`, so the next chunk
/// arrives as an ordinary continuation rather than a gap the user cannot
/// explain.
pub(crate) fn clear_terminal(app: &mut App, handle: &str) {
    if app.interactive_terminals.clear_scrollback(handle) {
        // A box may be showing the same view; repaint it so the clear is
        // visible in whichever surface the user is actually looking at.
        if app.dialog_state.terminal_detail_handle.as_deref() == Some(handle) {
            refresh_terminal_dialog(app, handle);
        }
    } else {
        app.messages_state
            .toasts
            .warning("No terminal selected to clear");
    }
}

/// Close the terminal dialog. Detaches the viewed terminal (releasing
/// the caller-owned attachment; the process keeps running) unless
/// `terminate` is set by an explicit terminate-then-close flow.
pub(crate) fn close_terminal_view(app: &mut App, terminate: bool) {
    if let Some(handle) = app.dialog_state.terminal_detail_handle.clone() {
        if terminate {
            start_terminal_terminate(app, Some(handle));
        } else {
            // `start_terminal_detach` is a no-op locally when there is no
            // attachment, and issues one bounded detach otherwise.
            start_terminal_detach(app, Some(handle));
        }
    }
    app.dialog_state.terminal_detail_handle = None;
}

/// Route one dialog key through the terminal focus gate.
///
/// Returns `true` when the key was consumed by terminal handling (the
/// caller must not route it to the prompt or the dialog scroller).
/// Ctrl-] while focused only leaves focus; literal Esc is sent to the PTY.
/// Escape while viewing closes the view (detach); the prompt is never
/// submitted from the terminal dialog.
pub(crate) fn handle_terminal_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};

    let Some(handle) = app.dialog_state.terminal_detail_handle.clone() else {
        return false;
    };
    if app.interactive_terminals.view(&handle).is_none() {
        return false;
    }
    let focused = app.interactive_terminals.is_focused(&handle);

    // Explicit focus entry: `i` while viewing (mirrors Normal-mode focus).
    if !focused && matches!(key.code, KeyCode::Char('i')) && key.modifiers == KeyModifiers::NONE {
        focus_terminal(app, Some(handle));
        return true;
    }

    if !focused {
        // Viewing: the directional gesture docks this box into the side
        // panel. Handled here rather than by the dialog scroller below,
        // which would swallow it — a modal short-circuits key routing
        // before the binding map is consulted, so the box has to recognise
        // the gesture itself.
        if let Some(dir) = crate::tui::panels::direction_of(key) {
            app.process_msg(crate::tui::app::TuiMsg::MoveDirection(dir));
            return true;
        }
        // Helm-style navigation and selection. The box answers these itself
        // rather than letting the `InfoDialog` scroller move, because the
        // scrollable content is the terminal viewport itself and the
        // scrollback offset lives on the controller.
        if handle_terminal_nav(app, &handle, key) {
            return true;
        }
        // Esc closes (detach) — unless a selection is armed, in which case Esc
        // drops it and keeps the view open. Closing a box the user was
        // mid-copy in loses the selection with no way back.
        if matches!(key.code, KeyCode::Esc) {
            if app.interactive_terminals.selection(&handle).is_some() {
                app.interactive_terminals.cancel_selection(&handle);
                refresh_terminal_dialog(app, &handle);
                return true;
            }
            close_terminal_view(app, false);
            app.close_dialog();
            return true;
        }
        return false;
    }

    // Focused: every key goes through the focus gate. Ctrl-] leaves focus;
    // literal Esc and other supported input forward to the PTY.
    let terminal_key = map_key(key);
    match classify_key(true, terminal_key) {
        TerminalKeyAction::EscapeFocus => {
            app.interactive_terminals.escape(&handle);
            app.messages_state
                .toasts
                .info("Terminal unfocused: keystrokes go to the prompt; Esc closes the view");
            refresh_terminal_dialog(app, &handle);
            true
        }
        TerminalKeyAction::Forward(bytes) => {
            send_terminal_input(app, handle.clone(), bytes);
            refresh_terminal_dialog(app, &handle);
            true
        }
        TerminalKeyAction::Ignored => true,
    }
}

/// Helix-style navigation, selection, and clear for one terminal.
///
/// Shared verbatim by the box and the docked panel so the same key means the
/// same thing on both surfaces — the failure mode this module already documents
/// for [`map_key`], extended to the verbs that are not PTY input.
///
/// Keys here are claimed only while *viewing*. In focused (INS) mode the PTY
/// owns the keyboard and everything goes through [`classify_key`], so a `v` or
/// `d` typed at a running program stays program input.
///
/// Returns whether the key was consumed.
pub(crate) fn handle_terminal_nav(
    app: &mut App,
    handle: &str,
    key: crossterm::event::KeyEvent,
) -> bool {
    let outcome = apply_nav(&mut app.interactive_terminals, handle, key);
    if outcome == NavOutcome::ClearRequested {
        clear_terminal(app, handle);
        return true;
    }
    if outcome == NavOutcome::Unhandled {
        return false;
    }
    refresh_terminal_dialog(app, handle);
    true
}

/// What [`apply_nav`] did, from the caller's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavOutcome {
    /// Navigation, selection, or a yank happened; redraw.
    Handled,
    /// The key was not a navigation key; route it normally.
    Unhandled,
    /// The clear verb fired and the caller must run it through the daemon.
    ClearRequested,
}

/// Apply one navigation key to the controller.
///
/// Split from [`handle_terminal_nav`] because the docked panel has no `App`:
/// it owns the same controller through `PanelCtx`, so the verbs live here and
/// only the daemon-bound `clear` needs routing back through a command.
pub(crate) fn apply_nav(
    terminals: &mut crate::tui::interactive_terminal::InteractiveTerminalController,
    handle: &str,
    key: crossterm::event::KeyEvent,
) -> NavOutcome {
    use crossterm::event::{KeyCode, KeyModifiers};

    if terminals.view(handle).is_none() {
        return NavOutcome::Unhandled;
    }

    // `space+y` arrives as two keys in legacy xterm encoding, so the prefix is
    // state. Any second key other than `y` disarms it and is then resolved
    // normally — which is why `y` must not also fall through to its own arm.
    if terminals.prefix_pending(handle) {
        terminals.set_prefix_pending(handle, false);
        if matches!(key.code, KeyCode::Char('y')) && key.modifiers == KeyModifiers::NONE {
            // A failed yank is deliberately not reported here: this seam has no
            // `App` (the panel holds the same controller through `PanelCtx`),
            // and the selection survives a failed copy, so the reader can
            // retry `space+y` rather than having lost the text.
            if let Some(text) = terminals.selected_text(handle) {
                let _ = crate::util::clipboard::copy_to_clipboard(&text);
            }
            return NavOutcome::Handled;
        }
    }

    match classify_nav_key(key) {
        TerminalNavAction::Move(drow, dcol) => {
            terminals.move_cursor(handle, drow, dcol);
        }
        TerminalNavAction::Page(direction) => {
            terminals.page_by(handle, direction);
        }
        TerminalNavAction::Word(forward) => {
            terminals.word_motion(handle, forward);
        }
        TerminalNavAction::SelectChar => {
            terminals.toggle_char_selection(handle);
        }
        TerminalNavAction::SelectLine => {
            terminals.toggle_line_selection(handle);
        }
        TerminalNavAction::DeleteSelection => {
            // A bare `d` with nothing selected is a no-op, but it is *claimed*
            // rather than passed through: the key is the delete operator here,
            // and an unclaimed `d` in a focused panel would reach the global
            // `OpenDiff` binding instead.
            terminals.delete_selection(handle);
        }
        TerminalNavAction::YankPrefix => {
            terminals.set_prefix_pending(handle, true);
        }

        TerminalNavAction::Clear => {
            terminals.cancel_selection(handle);
            terminals.set_prefix_pending(handle, false);
            return NavOutcome::ClearRequested;
        }
        TerminalNavAction::Unhandled => return NavOutcome::Unhandled,
    }
    NavOutcome::Handled
}

/// One navigation verb resolved from a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalNavAction {
    /// Move the cursor by rows and columns; scrolls at the edges.
    Move(i32, i32),
    /// Page the viewport; positive walks toward older output.
    Page(i32),
    /// Helix `e` / `b` — jump to the next/previous word boundary.
    Word(bool),
    /// Helix `v` — toggle character-granularity selection.
    SelectChar,
    /// Helix `x` — whole-line selection, growing one line per press.
    SelectLine,
    /// Helix `d` — delete the held selection from this view.
    DeleteSelection,
    /// Arm the `space` prefix awaiting `y`.
    YankPrefix,
    /// Helix `C` — clear scrollback and screen; the process keeps running.
    Clear,
    /// Not a navigation key; let the caller route it.
    Unhandled,
}

/// Resolve a Normal-mode key event into a terminal navigation verb.
///
/// `d` and `C` are deliberately here rather than left to the global binding
/// map: a focused panel is consulted *before* Normal-mode actions, so an
/// unclaimed `d` would silently become `OpenDiff` and never delete anything.
///
/// Uppercase is bound with *and* without `SHIFT`, because terminals disagree on
/// the encoding — the same rule the panel directionals already follow.
pub(crate) fn classify_nav_key(key: crossterm::event::KeyEvent) -> TerminalNavAction {
    use crossterm::event::{KeyCode, KeyModifiers};

    match (key.modifiers, key.code) {
        // Page keys before the bare-char arms: ctrl+d is PageDown and ctrl+u is
        // PageUp, while bare `d` deletes a selection. They are distinct
        // events, not a shadowing pair — and both page a whole viewport at a
        // time, which is the point of paging rather than stepping.
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => TerminalNavAction::Page(1),
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => TerminalNavAction::Page(-1),
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            TerminalNavAction::Move(1, 0)
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            TerminalNavAction::Move(-1, 0)
        }
        (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => {
            TerminalNavAction::Move(0, -1)
        }
        (KeyModifiers::NONE, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Right) => {
            TerminalNavAction::Move(0, 1)
        }
        (KeyModifiers::NONE, KeyCode::Char('e')) => TerminalNavAction::Word(true),
        (KeyModifiers::NONE, KeyCode::Char('b')) => TerminalNavAction::Word(false),
        (KeyModifiers::NONE, KeyCode::Char('v')) => TerminalNavAction::SelectChar,
        (KeyModifiers::NONE, KeyCode::Char('x')) => TerminalNavAction::SelectLine,
        (KeyModifiers::NONE, KeyCode::Char(' ')) => TerminalNavAction::YankPrefix,
        (KeyModifiers::NONE, KeyCode::Char('d')) => TerminalNavAction::DeleteSelection,
        // Clear moved from `d` to `C` so `d` could become the delete operator.
        // Clear moved from `d` to `C` so `d` could become the delete operator.
        // All four spellings: terminals disagree about whether a shifted letter
        // arrives as `Char('C')` with NONE, `Char('C')` with SHIFT, `Char('c')`
        // with SHIFT, or `Char('c')` with NONE, and a clear key that works on
        // one terminal is a dead key on another.
        (KeyModifiers::NONE, KeyCode::Char('c'))
        | (KeyModifiers::NONE, KeyCode::Char('C'))
        | (KeyModifiers::SHIFT, KeyCode::Char('c'))
        | (KeyModifiers::SHIFT, KeyCode::Char('C')) => TerminalNavAction::Clear,
        _ => TerminalNavAction::Unhandled,
    }
}

/// Map a crossterm key event to the framework-neutral terminal key.
///
/// Shared with the side panel so both surfaces map a keystroke identically;
/// the alternative is a panel that quietly disagrees with the box about what
/// `Ctrl-]` or a literal `Esc` means.
pub(crate) fn map_key(key: crossterm::event::KeyEvent) -> TerminalKey {
    use crossterm::event::{KeyCode, KeyModifiers};
    match key.code {
        KeyCode::Char(']') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            TerminalKey::ExitFocus
        }
        KeyCode::Esc => TerminalKey::Esc,
        KeyCode::Enter => TerminalKey::Enter,
        KeyCode::Backspace => TerminalKey::Backspace,
        KeyCode::Tab => TerminalKey::Tab,
        KeyCode::Up => TerminalKey::Up,
        KeyCode::Down => TerminalKey::Down,
        KeyCode::Left => TerminalKey::Left,
        KeyCode::Right => TerminalKey::Right,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => TerminalKey::CtrlC,
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => TerminalKey::CtrlD,
        KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE => TerminalKey::Char(c),
        KeyCode::Char(c) if key.modifiers == KeyModifiers::SHIFT && !c.is_control() => {
            TerminalKey::Char(c)
        }
        _ => TerminalKey::Other,
    }
}

/// Translate daemon wire-error codes into actionable terminal UX hints.
///
/// Unknown/foreign attachments share one code (M002 probing resistance),
/// so both read as "attach again"; gone handles read as restart/remove.
fn terminal_error_hint(code: &str, message: &str) -> String {
    match code {
        code if code.contains("attachment_gone") => format!(
            "{code}: attachment unknown or owned by another client; re-attach ({message})"
        ),
        code if code.contains("handle_gone") => format!(
            "{code}: terminal handle gone (removed or daemon restarted); use /terminal-list ({message})"
        ),
        code if code.contains("not_running") => {
            format!("{code}: process already exited; input rejected ({message})")
        }
        code if code.contains("shutting_down") => {
            format!("{code}: daemon is shutting down ({message})")
        }
        _ => format!("{code}: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::App;
    use crate::tui::interactive_terminal::InteractiveTerminalController;

    fn test_chunk(text: &str) -> InteractiveOutputChunk {
        InteractiveOutputChunk {
            handle: "h-test".to_string(),
            from_seq: 0,
            next_seq: text.len() as u64,
            gap: false,
            data_b64: B64.encode(text.as_bytes()),
        }
    }

    fn test_app() -> App {
        App::new_for_testing("terminal-test".to_string())
    }

    #[test]
    fn key_mapping_reserves_ctrl_right_bracket_and_forwards_escape() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            TerminalKey::Esc
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL)),
            TerminalKey::ExitFocus
        );
        assert_eq!(
            classify_key(
                true,
                map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            ),
            TerminalKeyAction::Forward(vec![0x1b])
        );
    }

    #[test]
    fn stale_attach_completion_is_dropped() {
        let mut app = test_app();
        let stale = app.dialog_state.terminal_request.begin();
        let _current = app.dialog_state.terminal_request.begin();
        apply_terminal_attach_finished(
            &mut app,
            stale,
            "h-test".to_string(),
            Some("a-1".to_string()),
            Some(test_chunk("hello\n")),
            None,
            None,
        );
        assert!(
            app.interactive_terminals.view("h-test").is_none(),
            "stale attach must not create a view"
        );
        assert!(app.dialog_state.terminal_request.is_loading());
        assert_ne!(app.ui_state.dialog, crate::tui::Dialog::Terminal);
    }

    #[test]
    fn stale_attach_error_is_suppressed() {
        let mut app = test_app();
        let stale = app.dialog_state.terminal_request.begin();
        let _current = app.dialog_state.terminal_request.begin();
        apply_terminal_attach_finished(
            &mut app,
            stale,
            "h-test".to_string(),
            None,
            None,
            None,
            Some("interactive_handle_gone: gone".to_string()),
        );
        assert!(app.dialog_state.terminal_request.last_error().is_none());
        assert!(app.dialog_state.terminal_request.is_loading());
    }

    #[test]
    fn current_attach_completion_applies_and_opens_dialog() {
        let mut app = test_app();
        let request_id = app.dialog_state.terminal_request.begin();
        apply_terminal_attach_finished(
            &mut app,
            request_id,
            "h-test".to_string(),
            Some("a-1".to_string()),
            Some(test_chunk("hello\n")),
            None,
            None,
        );
        let lines = app
            .interactive_terminals
            .render_lines("h-test", 10)
            .expect("view exists");
        assert!(lines.join("\n").contains("hello"));
        assert_eq!(app.ui_state.dialog, crate::tui::Dialog::Terminal);
        assert_eq!(
            app.dialog_state.terminal_detail_handle.as_deref(),
            Some("h-test")
        );
        assert!(!app.dialog_state.terminal_request.is_loading());
    }

    #[test]
    fn stale_op_completion_is_dropped() {
        let mut app = test_app();
        let request_id = app.dialog_state.terminal_request.begin();
        apply_terminal_attach_finished(
            &mut app,
            request_id,
            "h-test".to_string(),
            Some("a-1".to_string()),
            Some(test_chunk("hello\n")),
            None,
            None,
        );
        assert!(app.interactive_terminals.view("h-test").is_some());

        let stale = app.dialog_state.terminal_request.begin();
        let _current = app.dialog_state.terminal_request.begin();
        apply_terminal_op_finished(
            &mut app,
            stale,
            "terminate".to_string(),
            "h-test".to_string(),
            Some(0),
            None,
            None,
            None,
            None,
        );
        assert!(
            !app.interactive_terminals
                .view("h-test")
                .expect("view")
                .link_state()
                .is_exited(),
            "stale terminate must not mark the view exited"
        );
    }

    #[test]
    fn stale_list_completion_is_dropped() {
        let mut app = test_app();
        let stale = app.dialog_state.terminal_request.begin();
        let _current = app.dialog_state.terminal_request.begin();
        apply_terminal_list_finished(&mut app, stale, Some(Vec::new()), None);
        assert!(app.interactive_terminals.is_empty());
        assert!(app.dialog_state.terminal_request.is_loading());
    }

    #[test]
    fn terminal_slash_commands_are_registered() {
        let registry = crate::tui::command::CommandRegistry::new();
        for name in [
            "/terminal-create",
            "/terminal-list",
            "/terminal-attach",
            "/terminal-show",
            "/terminal-focus",
            "/terminal-send",
            "/terminal-resize",
            "/terminal-resume",
            "/terminal-detach",
            "/terminal-terminate",
            "/terminal-remove",
        ] {
            assert!(
                registry.find_by_name_or_alias(name).is_some(),
                "{name} missing from the slash registry"
            );
        }
    }

    #[test]
    fn the_default_shell_is_never_empty() {
        // `/terminal` has to produce something to show. An empty argv is
        // rejected pre-admission, so an empty fallback would turn the command
        // back into a failure with nothing the user can do about it.
        let argv = default_shell_argv();
        assert_eq!(argv.len(), 1, "a plain shell takes no arguments");
        assert!(
            !argv[0].trim().is_empty(),
            "a blank $SHELL must fall through to the real default"
        );
    }

    // ── Navigation keymap ──────────────────────────────────────────────

    fn nav_key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
        nav_key_mod(code, crossterm::event::KeyModifiers::NONE)
    }

    fn nav_key_mod(
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent {
            code,
            modifiers,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn nav_keymap_covers_the_documented_verbs() {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Line movement, vim and arrows alike.
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('j'))),
            TerminalNavAction::Move(1, 0)
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('k'))),
            TerminalNavAction::Move(-1, 0)
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Down)),
            TerminalNavAction::Move(1, 0)
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Up)),
            TerminalNavAction::Move(-1, 0)
        );

        // Character movement.
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('h'))),
            TerminalNavAction::Move(0, -1)
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('l'))),
            TerminalNavAction::Move(0, 1)
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Left)),
            TerminalNavAction::Move(0, -1)
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Right)),
            TerminalNavAction::Move(0, 1)
        );

        // Paging. Ctrl+u must page toward older output and Ctrl+d toward newer,
        // the same orientation as the global PageUp/PageDown map.
        assert_eq!(
            classify_nav_key(nav_key_mod(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            TerminalNavAction::Page(-1)
        );
        assert_eq!(
            classify_nav_key(nav_key_mod(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            TerminalNavAction::Page(1)
        );

        // Selection and clear.
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('v'))),
            TerminalNavAction::SelectChar
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('x'))),
            TerminalNavAction::SelectLine
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char(' '))),
            TerminalNavAction::YankPrefix
        );

        // Word motions, the delete operator, and clear.
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('e'))),
            TerminalNavAction::Word(true)
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('b'))),
            TerminalNavAction::Word(false)
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('d'))),
            TerminalNavAction::DeleteSelection
        );
        assert_eq!(
            classify_nav_key(nav_key(KeyCode::Char('c'))),
            TerminalNavAction::Clear
        );
        // Uppercase is bound with and without SHIFT, because terminals disagree
        // on which of the two a shifted letter arrives as.
        for (code, modifiers) in [
            (KeyCode::Char('C'), KeyModifiers::NONE),
            (KeyCode::Char('C'), KeyModifiers::SHIFT),
            (KeyCode::Char('c'), KeyModifiers::SHIFT),
        ] {
            assert_eq!(
                classify_nav_key(nav_key_mod(code, modifiers)),
                TerminalNavAction::Clear,
                "{code:?}/{modifiers:?} must clear"
            );
        }
    }

    #[test]
    fn nav_keymap_claims_nothing_it_does_not_mean() {
        use crossterm::event::KeyCode;

        // `i` is focus, not navigation, and the directionals are the host's
        // dock gesture — both are claimed elsewhere, so claiming them here
        // would make docking unreachable.
        for code in [
            KeyCode::Char('i'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Char('H'),
            KeyCode::Char('J'),
            KeyCode::Char('K'),
            KeyCode::Char('L'),
            KeyCode::Tab,
            KeyCode::Char('y'),
        ] {
            assert_eq!(
                classify_nav_key(nav_key(code)),
                TerminalNavAction::Unhandled,
                "{code:?} must not be a navigation verb"
            );
        }
    }

    #[test]
    fn space_y_copies_and_arms_only_while_the_prefix_is_pending() {
        use crossterm::event::KeyCode;

        let mut terminals = InteractiveTerminalController::new();
        assert!(terminals.apply_created(
            "h".to_string(),
            "ws-1".to_string(),
            "cat".to_string(),
            40,
            6
        ));
        let text = "alpha\r\nbravo\r\ncharlie\r\ndelta\r\necho\r\nfoxtrot";
        let body = test_chunk(text);
        assert!(terminals
            .apply_attached("h", "a-1".to_string(), &body, None)
            .expect("attach")
            .is_none());

        // A bare `y` with no armed prefix is not a yank.
        assert_eq!(
            apply_nav(&mut terminals, "h", nav_key(KeyCode::Char('y'))),
            NavOutcome::Unhandled
        );

        // `x` then `space` `y`: the prefix consumes the `y` and disarms.
        assert_eq!(
            apply_nav(&mut terminals, "h", nav_key(KeyCode::Char('x'))),
            NavOutcome::Handled
        );
        assert_eq!(
            apply_nav(&mut terminals, "h", nav_key(KeyCode::Char(' '))),
            NavOutcome::Handled
        );
        assert!(terminals.prefix_pending("h"));
        assert_eq!(
            apply_nav(&mut terminals, "h", nav_key(KeyCode::Char('y'))),
            NavOutcome::Handled
        );
        assert!(
            !terminals.prefix_pending("h"),
            "the yank must disarm the prefix or the next `y` yanks again"
        );

        // A second key that is not `y` also disarms the prefix.
        terminals.set_prefix_pending("h", true);
        assert_eq!(
            apply_nav(&mut terminals, "h", nav_key(KeyCode::Char('v'))),
            NavOutcome::Handled,
            "an unarmed key after Space is handled normally"
        );
        assert!(!terminals.prefix_pending("h"));

        // Unknown handles are refused rather than panicking.
        assert_eq!(
            apply_nav(&mut terminals, "nope", nav_key(KeyCode::Char('j'))),
            NavOutcome::Unhandled
        );
    }

    #[test]
    fn clear_is_routed_to_the_daemon_not_applied_inline() {
        use crossterm::event::KeyCode;

        let mut terminals = InteractiveTerminalController::new();
        assert!(terminals.apply_created(
            "h".to_string(),
            "ws-1".to_string(),
            "cat".to_string(),
            40,
            6
        ));
        let body = test_chunk("alpha\r\nbravo");
        assert!(terminals
            .apply_attached("h", "a-1".to_string(), &body, None)
            .expect("attach")
            .is_none());
        terminals.toggle_line_selection("h");

        // The verb reports intent; only the command layer owns the clear, so a
        // key press can never clear a view the user is not looking at.
        assert_eq!(
            apply_nav(&mut terminals, "h", nav_key(KeyCode::Char('C'))),
            NavOutcome::ClearRequested
        );
        assert!(
            terminals.selection("h").is_none(),
            "clear drops the selection it was holding"
        );
    }

    #[test]
    fn terminal_workspace_requires_a_bound_session() {
        let app = test_app();
        // The fixture has no session, so workspace resolution must fail
        // with an actionable error rather than falling back to ambient cwd.
        assert!(resolve_terminal_workspace(&app).is_err());
    }

    #[test]
    fn terminal_error_hints_are_actionable() {
        assert!(terminal_error_hint("interactive_attachment_gone", "x").contains("re-attach"));
        assert!(terminal_error_hint("interactive_handle_gone", "x").contains("/terminal-list"));
        assert!(terminal_error_hint("interactive_not_running", "x").contains("already exited"));
    }
}
