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
                        cols: Some(80),
                        rows: Some(24),
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
    handle_terminal_show(app, handle);
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
pub(crate) fn flush_terminal_input(app: &mut App, handle: String) {
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

/// Queue a resize from `/terminal-resize <cols> <rows>` and flush the
/// coalesced (last-wins) size through one M002 resize operation.
pub(crate) fn start_terminal_resize(app: &mut App, handle: String, cols: u16, rows: u16) {
    if let Err(error) = app.interactive_terminals.queue_resize(&handle, cols, rows) {
        app.messages_state.toasts.warning(&error.to_string());
        return;
    }
    let (Some(attachment_id), Some((cols, rows))) = (
        app.interactive_terminals.attachment_id(&handle),
        app.interactive_terminals.take_pending_resize(&handle),
    ) else {
        return;
    };
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
    lines.push("── output (newest, bounded) ──".to_string());
    match app.interactive_terminals.render_lines(handle, 200) {
        Some(output) if output.is_empty() => {
            lines.push("(no output yet)".to_string());
        }
        Some(output) => lines.extend(output),
        None => lines.push("(no output yet)".to_string()),
    }
    let footer =
        "i focus  |  Esc unfocus/close  |  j/k scroll  |  /terminal-resume refresh".to_string();
    if let Some(dialog) = app
        .focus_manager
        .dialog_mut_any::<crate::tui::components::dialogs::info::InfoDialog>()
    {
        dialog.set_info_type(crate::tui::components::dialogs::info::InfoType::TerminalShow);
        dialog.set_content(lines);
        dialog.set_theme(&app.ui_state.theme);
        dialog.set_custom_footer(footer);
    } else {
        let mut dialog = crate::tui::components::dialogs::info::InfoDialog::new(
            std::sync::Arc::clone(&app.ui_state.theme),
            crate::tui::components::dialogs::info::InfoType::TerminalShow,
            lines,
        );
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
            "Terminal {handle} focused: keystrokes go to the process; Esc leaves focus"
        ));
        refresh_terminal_dialog(app, &handle);
    } else {
        app.messages_state.toasts.warning(&format!(
            "Cannot focus {handle}: attach a live terminal first (/terminal-attach {handle})"
        ));
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
/// Escape while focused only leaves focus; it never submits anything.
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
        // Viewing: Esc closes (detach); navigation falls through to the
        // dialog scroller (return false); everything else is ignored here
        // and must not reach the prompt while the dialog is modal.
        if matches!(key.code, KeyCode::Esc) {
            close_terminal_view(app, false);
            app.close_dialog();
            return true;
        }
        return false;
    }

    // Focused: every key goes through the focus gate. Esc leaves focus
    // (never forwards, never submits); other keys forward or are ignored.
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

/// Map a crossterm key event to the framework-neutral terminal key.
fn map_key(key: crossterm::event::KeyEvent) -> TerminalKey {
    use crossterm::event::{KeyCode, KeyModifiers};
    match key.code {
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
