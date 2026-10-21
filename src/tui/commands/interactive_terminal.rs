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
use crate::tui::app::TerminalResumeRequest;
use crate::tui::app::{App, TuiCommand};
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::interactive_terminal::{TerminalInputError, TerminalKey, TerminalKeyAction};
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

// ── The refusal, defined once ─────────────────────────────────────────

/// The operation an interactive-terminal refusal is about.
///
/// Every entry point refuses through the same classifier, so the per-operation
/// wording lives here instead of being retyped at each site. That duplication is
/// the defect: nine copies of one sentence is nine places to get the cause
/// wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalOperation {
    Create,
    List,
    Attach,
    Resume,
    Input,
    Resize,
    Detach,
    Terminate,
    Remove,
}

impl TerminalOperation {
    /// The operation clause shared by both refusal causes.
    const fn subject(self) -> &'static str {
        match self {
            Self::Create => "cannot create a terminal",
            Self::List => "cannot list terminals",
            Self::Attach => "cannot attach to the terminal",
            Self::Resume => "cannot resume terminal output",
            Self::Input => "terminal input dropped",
            Self::Resize => "terminal resize dropped",
            Self::Detach => "cannot detach the terminal",
            Self::Terminate => "cannot terminate the terminal",
            Self::Remove => "cannot remove the terminal",
        }
    }
}

/// Why an interactive-terminal operation cannot run on this frontend.
///
/// These two are **different faults** and must never share one string.
/// `NoDaemonConnection` is a real connectivity problem on a frontend that does
/// support terminals. `TransportUnsupported` is a frontend that was never wired
/// for them: the `/tui` WebSocket attach frontend (`src/client/attach.rs`) opens
/// a `TuiMessage` socket and is never handed a `CoreClient`, so **no**
/// interactive-process operation can reach it. The daemon it is attached to is
/// healthy and answering everything else, which is exactly why blaming it sends
/// the reader to restart something that was working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalRefusal {
    /// This frontend transport carries no interactive-process traffic.
    TransportUnsupported,
    /// This frontend supports terminals but has no live daemon connection.
    NoDaemonConnection,
}

impl TerminalRefusal {
    /// Classify why `app` cannot drive a terminal, or `None` when it can.
    ///
    /// `App::is_ws_attach_transport()` is the discriminator, not the app mode:
    /// the socket-daemon frontend is `RemoteCore` too, and it *does* own a
    /// `CoreClient`. `scripts/check_tui_transport_authority.py` enforces this.
    fn classify(app: &App) -> Option<Self> {
        if app.core_client.is_some() {
            return None;
        }
        Some(if app.is_ws_attach_transport() {
            Self::TransportUnsupported
        } else {
            Self::NoDaemonConnection
        })
    }

    /// The operator-facing message. It names the faulting component, because a
    /// refusal that names a healthy one is worse than no message at all.
    fn message(self, operation: TerminalOperation) -> String {
        match self {
            Self::NoDaemonConnection => {
                format!("Daemon connection unavailable; {}", operation.subject())
            }
            Self::TransportUnsupported => format!(
                "This frontend is attached over a remote WebSocket, which does not carry \
                 interactive terminals; {}. Open one with `codegg` or `codegg daemon attach` \
                 on the machine running the daemon.",
                operation.subject()
            ),
        }
    }
}

/// The `CoreClient` for a terminal operation, or `None` with the refusal already
/// reported.
///
/// The classifier runs here — before `terminal_request.begin()`, before the task
/// spawn, before any view write — so a refused command leaves the app exactly as
/// it found it.
pub(crate) fn terminal_core_client(
    app: &mut App,
    operation: TerminalOperation,
) -> Option<std::sync::Arc<dyn crate::core::CoreClient>> {
    if let Some(client) = app.core_client.clone() {
        return Some(client);
    }
    if let Some(refusal) = TerminalRefusal::classify(app) {
        app.messages_state.toasts.error(&refusal.message(operation));
    }
    None
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
    let Some(core_client) = terminal_core_client(app, TerminalOperation::Create) else {
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
    let Some(core_client) = terminal_core_client(app, TerminalOperation::List) else {
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
    let Some(core_client) = terminal_core_client(app, TerminalOperation::Attach) else {
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
    let Some(core_client) = terminal_core_client(app, TerminalOperation::Resume) else {
        return;
    };
    let request_id = app.dialog_state.terminal_request.begin();
    let owner = TerminalResumeRequest::User(request_id);
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_resume",
        async move {
            // Byte-for-byte the same request the cadence pull issues: the
            // daemon's own chunk bound, one explicit cursor, no second cap.
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
                        request: owner,
                        handle,
                        chunk: Some(chunk),
                        resync: None,
                        error: None,
                    }
                }
                Ok(CoreResponse::InteractiveProcessResyncRequired { resync, .. }) => {
                    TuiCommand::TerminalResumeFinished {
                        request: owner,
                        handle,
                        chunk: None,
                        resync: Some(resync),
                        error: None,
                    }
                }
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalResumeFinished {
                    request: owner,
                    handle,
                    chunk: None,
                    resync: None,
                    error: Some(terminal_error_hint(&code, &message)),
                },
                Ok(other) => TuiCommand::TerminalResumeFinished {
                    request: owner,
                    handle,
                    chunk: None,
                    resync: None,
                    error: Some(format!("unexpected resume response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalResumeFinished {
                    request: owner,
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
///
/// Both owners project through here. What differs is only which bookkeeping
/// slot is released, and that is decided by the typed owner rather than by
/// which completion happened to arrive first: a cadence pull releases its own
/// per-attachment read id and never the shared `terminal_request` generation,
/// so it cannot swallow an in-flight input, resize, detach or terminate
/// completion (and vice versa).
pub(crate) fn apply_terminal_resume_finished(
    app: &mut App,
    owner: TerminalResumeRequest,
    handle: String,
    chunk: Option<InteractiveOutputChunk>,
    resync: Option<InteractiveResync>,
    error: Option<String>,
) {
    let from_pull = owner.pull().is_some();
    if let Some(error) = error {
        if release_resume_owner(app, &owner, &handle, Some(error.clone())) {
            if from_pull {
                // The cadence stopped on a typed daemon refusal. Toast it once,
                // keep it verbatim on the view: a pull that keeps re-issuing
                // against a dead attachment is a silent retry loop, and one
                // that toasts every frame is a toast storm. Either is worse
                // than saying it once and waiting for the user.
                app.interactive_terminals
                    .note_read_refusal(&handle, error.clone());
                app.messages_state.toasts.error(&error);
            } else {
                app.messages_state.toasts.error(&error);
            }
        }
        return;
    }
    if !release_resume_owner(app, &owner, &handle, None) {
        return;
    }
    // A `/terminal-resume` is the documented recovery from a stopped cadence,
    // so a successful answer re-arms it.
    if !from_pull {
        app.interactive_terminals.clear_read_refusal(&handle);
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
    // Both surfaces repaint on the chunk. The box needs the explicit refresh
    // because its content is a pushed dialog; the panel reads the controller on
    // its next render, so the same completion repaints it too.
    refresh_terminal_dialog(app, &handle);
}

/// Release the bookkeeping slot a resume answer claims.
///
/// Returns `false` when the answer is superseded and must be discarded. The
/// error, when present, is recorded in the shared slot for a user-initiated
/// answer; a pull has no shared slot and never borrows one.
fn release_resume_owner(
    app: &mut App,
    owner: &TerminalResumeRequest,
    handle: &str,
    error: Option<String>,
) -> bool {
    match owner {
        TerminalResumeRequest::User(generation) => {
            let generation = *generation;
            match error {
                Some(error) => app.dialog_state.terminal_request.fail(generation, error),
                None => app.dialog_state.terminal_request.finish(generation),
            }
        }
        TerminalResumeRequest::Pull {
            read_id,
            attachment_id,
        } => app
            .interactive_terminals
            .finish_output_read(handle, *read_id, attachment_id),
    }
}

// ── Cadence pull ────────────────────────────────────────────────────
//
// The only output read the TUI ever issues on its own. It rides the event
// loop's existing pump tick, issues exactly the same bounded
// `InteractiveProcessResume` `/terminal-resume` issues, and holds per
// attachment read bookkeeping rather than a shared generation, so it cannot
// supersede a user's own operation.

/// The handle of the terminal surface currently mounted, if any.
///
/// Box first, then panel: they are peers, and a box over a docked panel is the
/// one the user is looking at. A background view that is neither — attached,
/// alive, and on no surface — is not read, because nothing it produces would
/// reach a screen.
fn mounted_terminal_surface(app: &App) -> Option<String> {
    if matches!(app.ui_state.dialog, crate::tui::Dialog::Terminal) {
        if let Some(handle) = app.dialog_state.terminal_detail_handle.as_deref() {
            return Some(handle.to_string());
        }
    }
    app.panel_host.visible_terminal_handle()
}

/// The event-loop interval at which a mounted terminal surface is polled.
///
/// A sibling of the loop's other render cadences rather than a timer of its
/// own: the terminal reads through the pump tick that already services the
/// editor's semantic refresh, and one cadence for the TUI is the whole point
/// (I4). 100ms is the same order as the working strip's and far below the
/// point at which a shell's output reads as arriving in steps.
pub(crate) const TERMINAL_PULL_RENDER_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(100);

/// Whether the pump tick has a reason to stay awake on a mounted terminal's
/// behalf.
///
/// The loop asks this **before** it selects, and `None` here is not "sleep for
/// a while" — it is `futures_util::future::pending()` at the timed arm, so
/// every service reached only from that arm is unreachable. When the only
/// contributors were a streaming turn, a working turn and a live toast, a
/// mounted terminal was read only while one of those happened to be true:
/// output became visible when the user pressed the next key, a toast was
/// alive, or a resize was debouncing.
///
/// Two rules make this correct rather than merely present:
///
/// - **Mount, never read state.** This must *not* consult
///   `begin_output_read`, `output_read_in_flight`, the recorded refusal, or
///   `TerminalLinkState`. Gating the arm on any of those latches the loop off:
///   the arm goes dark, so nothing ever wakes it to issue the read that would
///   clear the condition, and the surface is dead until a keystroke arrives.
///   Those checks belong to the pull, where refusing one pass is harmless.
/// - **Mirror the pull's free early returns only.** It mirrors the two guards
///   `service_terminal_output_pull` passes through *without doing work* — a
///   mounted surface, and a client to read with. Everything else the pull
///   checks means "do not issue anything this pass", which is the pull's
///   business. Re-deriving any of it here would be a second liveness rule
///   free to drift from the first.
pub(crate) fn terminal_pull_armed(app: &App) -> bool {
    // `mounted_terminal_surface` allocates one `String` for the handle. That is
    // bounded, once per iteration, and only reached when a terminal exists;
    // splitting it into an allocation-free twin would be a second copy of the
    // visibility rule, which is the more expensive kind of cheap.
    mounted_terminal_surface(app).is_some() && app.core_client.is_some()
}

/// Pull live output for the mounted terminal surface, if it needs it.
///
/// Called from the event loop's pump tick. Returns `true` when it issued a
/// read, so the tick can ask for a repaint on the same pass.
///
/// No second timer, no interval, no new spinner: this is one call on the tick
/// that already services the editor's semantic refresh.
pub(crate) fn service_terminal_output_pull(app: &mut App) -> bool {
    let Some(handle) = mounted_terminal_surface(app) else {
        return false;
    };
    // The panel sizes its PTY from the render path, which has no seam of its
    // own, and the only drain used to be reachable through the input flush. A
    // docked terminal the user never types into therefore stayed at its
    // create-time 80x24 and would wrap once liveness landed. Draining here
    // decouples the resize from typing without a second timer.
    // Keystrokes drain here rather than per keypress: one ordered operation
    // per tick, never one racing request per character.
    flush_terminal_input(app, handle.clone());
    let Some(core_client) = app.core_client.clone() else {
        // No daemon: there is no PTY to read, and nothing to say about it.
        return false;
    };
    let read_id = app.interactive_terminals.next_read_id();
    let Some(read) = app
        .interactive_terminals
        .begin_output_read(&handle, read_id)
    else {
        return false;
    };
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "terminal_output_pull",
        async move {
            // Identical to the `/terminal-resume` request: one explicit
            // cursor, the daemon's own chunk bound, no client-side cap and no
            // unbounded "give me everything" route.
            let request = crate::core::new_request(
                format!("terminal-pull-{}", uuid::Uuid::new_v4()),
                CoreRequest::InteractiveProcessResume {
                    attachment_id: read.attachment_id.clone(),
                    from_seq: read.from_seq,
                    max_bytes: None,
                },
            );
            let owner = TerminalResumeRequest::Pull {
                read_id: read.read_id,
                attachment_id: read.attachment_id,
            };
            let result = match core_client.request(request).await {
                Ok(CoreResponse::InteractiveProcessResumed { chunk, .. }) => {
                    TuiCommand::TerminalResumeFinished {
                        request: owner,
                        handle,
                        chunk: Some(chunk),
                        resync: None,
                        error: None,
                    }
                }
                Ok(CoreResponse::InteractiveProcessResyncRequired { resync, .. }) => {
                    TuiCommand::TerminalResumeFinished {
                        request: owner,
                        handle,
                        chunk: None,
                        resync: Some(resync),
                        error: None,
                    }
                }
                // Typed refusals stay typed: the daemon's code and message are
                // carried through `terminal_error_hint` unchanged and kept on
                // the view, rather than being flattened into a generic error
                // that hides which class of failure it was.
                Ok(CoreResponse::Error { code, message }) => TuiCommand::TerminalResumeFinished {
                    request: owner,
                    handle,
                    chunk: None,
                    resync: None,
                    error: Some(terminal_error_hint(&code, &message)),
                },
                Ok(other) => TuiCommand::TerminalResumeFinished {
                    request: owner,
                    handle,
                    chunk: None,
                    resync: None,
                    error: Some(format!("unexpected resume response: {other:?}")),
                },
                Err(error) => TuiCommand::TerminalResumeFinished {
                    request: owner,
                    handle,
                    chunk: None,
                    resync: None,
                    error: Some(format!("resume request failed: {error}")),
                },
            };
            Some(result)
        },
    );
    true
}

// ── Input / resize (bounded, coalesced) ─────────────────────────────

/// Queue explicit input bytes and flush them through one bounded M002
/// input operation. Used by `/terminal-send`, which is an explicit command
/// with a completion the caller is waiting on.
pub(crate) fn send_terminal_input(app: &mut App, handle: String, bytes: Vec<u8>) {
    if let Err(error) = app.interactive_terminals.queue_input(&handle, &bytes) {
        app.messages_state.toasts.warning(&error.to_string());
        return;
    }
    flush_terminal_input(app, handle);
}

/// Route keystroke bytes to the PTY.
///
/// Identical to [`send_terminal_input`], and kept as the named seam both key
/// routers use because *why* it is safe is not obvious from the name. Flushing
/// per keystroke used to spawn one concurrent daemon request per character and
/// those raced, so a typed command could reach the shell scrambled. The
/// ordering now comes from the single input slot rather than from coalescing:
/// the first keystroke sends immediately, and any keystrokes typed while it is
/// outstanding stay queued in order and go out behind it on the next cadence
/// pass. That keeps the send synchronous — no beat of latency on the character
/// the user is watching for — without ever running two writes at once.
pub(crate) fn queue_terminal_input(app: &mut App, handle: String, bytes: Vec<u8>) {
    send_terminal_input(app, handle, bytes);
}

/// Drain coalesced pending input into one bounded input operation.
///
/// Also drains any pending resize queued by a docked panel. Sharing the drain
/// is deliberate: the panel sizes its PTY from the render path, which has no
/// seam of its own, so the resize would otherwise sit in the queue until the
/// user happened to type something.
pub(crate) fn flush_terminal_input(app: &mut App, handle: String) {
    flush_terminal_resize(app, &handle);
    if app.interactive_terminals.input_in_flight(&handle) {
        // An operation is already outstanding. Leave the bytes queued: the
        // next tick sends them, in order, behind this one. Dropping them here
        // is how a keystroke disappears with no error anywhere.
        return;
    }
    let Some(pending) = app.interactive_terminals.take_pending_input(&handle) else {
        return;
    };
    // The classifier runs before `begin_input`, not after. Refusing *after* the
    // slot is claimed drops the bytes and leaves `input_in_flight` set with
    // nothing in flight to release it, so every later keystroke parks behind a
    // flag nobody will ever clear: one visible toast, then permanent silence.
    let Some(core_client) = terminal_core_client(app, TerminalOperation::Input) else {
        // Put the bytes back. They still want to be typed at.
        let _ = app.interactive_terminals.queue_input(&handle, &pending);
        return;
    };
    let Some(attachment_id) = app.interactive_terminals.begin_input(&handle) else {
        // Lost the race between the check and the claim. Put the bytes back.
        let _ = app.interactive_terminals.queue_input(&handle, &pending);
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
    let Some(core_client) = terminal_core_client(app, TerminalOperation::Resize) else {
        // The caller already drained this size out of the coalescing queue. Put
        // it back rather than losing the user's geometry: the next tick, or the
        // next explicit `/terminal-resize`, then delivers it unchanged.
        let _ = app.interactive_terminals.queue_resize(&handle, cols, rows);
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
    let Some(core_client) = terminal_core_client(app, TerminalOperation::Detach) else {
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
    let Some(core_client) = terminal_core_client(app, TerminalOperation::Terminate) else {
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
    let Some(core_client) = terminal_core_client(app, TerminalOperation::Remove) else {
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
    // The input slot is released by the *arrival* of its completion, not by a
    // successful one, and not only when the completion is current: a stale or
    // failed input must still free the slot or every later keystroke waits
    // behind a gate that will never open.
    if op == "input" {
        app.interactive_terminals.finish_input(&handle);
    }
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
    let Some(shown) = app.dialog_state.terminal_detail_handle.clone() else {
        return;
    };
    // Open **in insert**. Someone who typed `/terminal` wants a shell, and
    // requiring a second `i` before the first character reached the PTY is
    // what made the box feel broken: keys typed in that gap went nowhere at
    // all, neither draft nor process. Focus is best-effort — a terminal that
    // cannot take keystrokes yet (still attaching) still opens for reading.
    let focusing = app.interactive_terminals.focus(&shown);
    refresh_terminal_dialog(app, &shown);
    app.ui_state.dialog = crate::tui::Dialog::Terminal;
    if !focusing {
        app.messages_state
            .toasts
            .info("Terminal is not attached yet — keys go to the view. `i` types when it is live.");
    }
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
        .render_view(
            handle,
            200,
            // The box is a *pushed* dialog: its content is styled here and
            // re-read when this runs, not every frame. That is enough because
            // the armed cadence (C003) re-runs this on every output pull, so
            // while the box is up it is repainted at the same 100ms tick the
            // blink is measured in — five ticks per phase. A box whose link
            // stopped reading does not repaint, and its caret holds its last
            // phase; the same frame also shows that read stopped, so the frozen
            // caret is explained rather than mysterious.
            crate::tui::components::spinner::cursor_blink_visible_now(),
            selection_style,
        )
        .unwrap_or_default();
    // The box mounts the emulator's whole screen as scrollable content and
    // then follows the end of it. Trailing blank rows are not content: on a
    // fresh shell the prompt sits on row 0 of an 80x24 screen, so the end of
    // the buffer is nineteen empty lines and a window pinned to the end shows
    // the user nothing but blank space — which is indistinguishable from a
    // terminal that accepted their command and produced no output. Trimming
    // them keeps the window over the rows the program actually wrote. A
    // full-screen program that paints every row is unaffected, because there
    // are no trailing blanks to trim.
    let last_used = output
        .iter()
        .rposition(|line| {
            line.spans
                .iter()
                .any(|span| !span.content.trim().is_empty())
        })
        .map(|index| index + 1)
        .unwrap_or(1)
        .max(1);
    let output = output[..last_used].to_vec();
    if output.is_empty() {
        styled_content.push(ratatui::text::Line::from("(no output yet)"));
    } else {
        styled_content.extend(output);
    }
    let footer = format!(
        "j/k scroll  |  ^-u/^-d page  |  e/b word  |  v select chars  |  x select lines  |  d delete  |  space+y copy  |  C clear  |  i focus  |  ^-] unfocus  |  D diagnostics  |  Esc closes{}",
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
            "Terminal {handle}: INS — typing goes to the process. Esc reads, Ctrl+] sends a literal esc"
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
            "Terminal {handle}: INS — typing goes to the process. Esc reads, Ctrl+] sends a literal esc"
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
        .info("Terminal NOR: keys move the cursor here. i types; Shift+← leaves the panel");
}

/// The bytes a clear sends to the process so it repaints its own prompt.
///
/// Erase-display + cursor-home, then a bare newline. Both halves are needed
/// and neither is enough alone:
///
/// - **ED 2 + CUP** is what a terminal means by "clear", and it is what the
///   process's own terminal and our emulator both understand. Sent alone it
///   leaves the screen blank, because nothing told the shell a command
///   finished — a shell draws `PS1` after a *command*, not after an escape.
/// - **The newline** completes an empty command line, so the shell draws a
///   fresh `PS1` at the new home row.
///
/// Sent as the literal command `clear\r` instead? It would work, and it is
/// what a user types, but it depends on a `clear` binary existing on the
/// child's `PATH` — and when it does not, the user is shown `clear: command not
/// found`, an error for a keystroke they never typed. The escape sequence has
/// no such dependency and works for `sh`, `zsh`, `fish` and a custom `PS1`
/// alike. Nothing here is synthesised on our side: every byte the reader ends
/// up looking at came out of the process.
const CLEAR_THROUGH_PROCESS: &[u8] = b"\x1b[2J\x1b[H\r";

/// Clear the local scrollback and screen for `handle`, and ask the process to
/// repaint.
///
/// Two halves, and the order between them is the whole decision:
///
/// 1. **Queue the clear for the process first.** It goes through the same
///    bounded, permissioned, live-link path as any keystroke — the one thing
///    this may never become is a privileged write that bypasses input gating.
///    A typed refusal here reports itself and leaves the screen alone, because
///    a local wipe whose repaint never arrived is a blank screen the user did
///    not ask for.
/// 2. **Then wipe locally.** `clear_scrollback` still drops the retained
///    scrollback and the selection, and still preserves `next_seq`, so the
///    next honest chunk arrives as an ordinary continuation. It is also what
///    makes the clear immediate: the process's own prompt is one round trip
///    away, and until it lands the local wipe is the only thing that has
///    happened.
///
/// A view with **no live process** — exited, gone, or detached — takes the
/// local wipe alone and is not refused. The plan's failure semantics suggested
/// leaving the screen intact whenever the input path refuses, and that is right
/// for every refusal *except* this one: there is no process left to print a
/// prompt, so nothing is being suppressed, and refusing would take away the
/// only way to clear the output of a shell that has finished. The blank screen
/// it leaves is the truth about a dead process rather than a fabricated one.
///
/// Local only in the sense that nothing reaches the daemon: the process keeps
/// running and the daemon keeps streaming, so sequence tracking is preserved
/// and a clear can never manufacture a resync.
pub(crate) fn clear_terminal(app: &mut App, handle: &str) {
    if app.interactive_terminals.view(handle).is_none() {
        app.messages_state
            .toasts
            .warning("No terminal selected to clear");
        return;
    }
    match app
        .interactive_terminals
        .queue_input(handle, CLEAR_THROUGH_PROCESS)
    {
        Ok(()) | Err(TerminalInputError::NotLive(_)) => {}
        Err(error) => {
            app.messages_state.toasts.warning(&error.to_string());
            return;
        }
    }
    if app.interactive_terminals.clear_scrollback(handle) {
        // A box may be showing the same view; repaint it so the clear is
        // visible in whichever surface the user is actually looking at.
        if app.dialog_state.terminal_detail_handle.as_deref() == Some(handle) {
            refresh_terminal_dialog(app, handle);
        }
        // Out the bytes now rather than waiting for the cadence's next pass:
        // the clear is the one input whose result the user is watching for,
        // and the panel's own path already flushes per keystroke.
        flush_terminal_input(app, handle.to_string());
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
///
/// **Insert means type.** While the terminal owns the keyboard every printable
/// key, `Enter`, `Backspace`, `Tab`, arrows and `Ctrl-C`/`Ctrl-D` go straight
/// to the PTY, which is what "the cursor is in the terminal window" has to
/// mean in practice. `Esc` leaves insert for the reader — helm navigation,
/// selection, yank — and never reaches the child; a program that needs its own
/// escape gets `Ctrl+]`.
pub(crate) fn handle_terminal_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};

    let Some(handle) = app.dialog_state.terminal_detail_handle.clone() else {
        return false;
    };
    if app.interactive_terminals.view(&handle).is_none() {
        return false;
    }
    let focused = app.interactive_terminals.is_focused(&handle);

    // Explicit focus entry: `i` while reading (mirrors Normal-mode focus).
    if !focused && matches!(key.code, KeyCode::Char('i')) && key.modifiers == KeyModifiers::NONE {
        focus_terminal(app, Some(handle));
        return true;
    }

    if !focused {
        // Reading: the directional gesture docks this box into the side
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
        // A reader-mode key we do not claim belongs to the box's own scroll,
        // not to the prompt underneath a modal.
        return matches!(
            key.code,
            KeyCode::Down | KeyCode::Up | KeyCode::PageDown | KeyCode::PageUp
        );
    }

    // Focused: every key goes through the focus gate. `Esc` leaves focus and
    // lands in the reader above; `Ctrl+]` sends a real escape byte instead.
    let terminal_key = map_key(key);
    match app
        .interactive_terminals
        .classify_focused(&handle, terminal_key)
    {
        TerminalKeyAction::EscapeFocus => {
            app.interactive_terminals.escape(&handle);
            refresh_terminal_dialog(app, &handle);
            true
        }
        TerminalKeyAction::Forward(bytes) => {
            queue_terminal_input(app, handle.clone(), bytes);
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
        TerminalNavAction::ToggleDiagnostics => {
            terminals.toggle_diagnostics(handle);
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
    /// Helix `D` — fold the full diagnostics dump in or out of the box.
    ToggleDiagnostics,
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
        // Same four-spelling rule as `C`: a diagnostics toggle that works on
        // one terminal and is a dead key on another is not a toggle.
        (KeyModifiers::NONE, KeyCode::Char('D'))
        | (KeyModifiers::SHIFT, KeyCode::Char('d'))
        | (KeyModifiers::SHIFT, KeyCode::Char('D')) => TerminalNavAction::ToggleDiagnostics,
        _ => TerminalNavAction::Unhandled,
    }
}

/// Map a crossterm key event to the framework-neutral terminal key.
///
/// Shared with the side panel so both surfaces map a keystroke identically;
/// the alternative is a panel that quietly disagrees with the box about what
/// `Esc` or `Ctrl-]` means.
pub(crate) fn map_key(key: crossterm::event::KeyEvent) -> TerminalKey {
    use crossterm::event::{KeyCode, KeyModifiers};
    match key.code {
        KeyCode::Char(']') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            TerminalKey::LiteralEscape
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
    use crate::tui::interactive_terminal::{classify_key, InteractiveTerminalController};

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
    fn key_mapping_reserves_ctrl_right_bracket_for_the_child_escape() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            TerminalKey::Esc
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL)),
            TerminalKey::LiteralEscape
        );
        // `Esc` is the mode key; only `Ctrl+]` carries an escape byte onward.
        assert_eq!(
            classify_key(
                true,
                map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            ),
            TerminalKeyAction::EscapeFocus
        );
        assert_eq!(
            classify_key(
                true,
                map_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL))
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

    /// The box and the docked panel disagree about the keyboard on purpose, and
    /// this pins both halves of that decision.
    ///
    /// The **box** opens already in INS: someone who typed `/terminal` wants a
    /// shell, and a second `i` before the first character meant keys typed in
    /// that gap went nowhere at all — neither draft nor process. That gap is
    /// what made the box feel broken, so `i` there is *program input*, not a
    /// focus gesture.
    ///
    /// The **panel** is a docked surface the user is only looking at, so it
    /// must never steal the keyboard; `i` is how they ask for it.
    ///
    /// The assertion this replaces ("a freshly opened box starts viewing, never
    /// focused") was left red by the deliberate change in `01db38bd5` and was
    /// not the behaviour any user is served by.
    #[test]
    fn the_box_opens_in_insert_while_the_docked_panel_still_needs_i() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        let request_id = app.dialog_state.terminal_request.begin();
        apply_terminal_attach_finished(
            &mut app,
            request_id,
            "h-test".to_string(),
            Some("a-1".to_string()),
            Some(test_chunk("$ ")),
            None,
            None,
        );
        assert_eq!(
            app.ui_state.dialog,
            crate::tui::Dialog::Terminal,
            "the box must be open before the keystroke is meaningful"
        );
        assert!(
            app.interactive_terminals.is_focused("h-test"),
            "a freshly opened, attached box owns the keyboard: the first character a user \
             types must reach the process, not wait behind a focus gesture"
        );

        // `i` is the character `i` here. The panel's focus gesture must not leak
        // onto a surface where the process is already reading.
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert!(
            app.interactive_terminals.is_focused("h-test"),
            "typing at a focused terminal leaves it focused; `i` is program input, not a toggle"
        );

        // The panel half: same gesture, opposite meaning, because a docked
        // surface is something the user is looking at rather than typing into.
        // `terminal_panel_spawn` is the one bit of correlation the panel route
        // uses, so it has to be set before the attach lands.
        let mut app = test_app();
        app.terminal_panel_spawn = true;
        let request_id = app.dialog_state.terminal_request.begin();
        apply_terminal_attach_finished(
            &mut app,
            request_id,
            "h-panel".to_string(),
            Some("a-2".to_string()),
            Some(test_chunk("$ ")),
            None,
            None,
        );
        assert!(
            !app.interactive_terminals.is_focused("h-panel"),
            "a terminal handed to the docked panel must not arrive holding the keyboard"
        );
        assert!(
            app.ui_state.dialog != crate::tui::Dialog::Terminal,
            "the panel route must not also raise the box"
        );
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

    // ── Live output pull acceptance (C001) ──────────────────────────
    //
    // Everything below drives the *production* composition end to end: a real
    // PTY behind the real M002 protocol, reached through the TUI's own
    // `CoreClient` seam, with keystrokes injected by `App::on_key` and the
    // output read by the production cadence service. Nothing here calls the
    // protocol directly to move bytes into the view — the assertions are about
    // what the user can actually see.
    //
    // The two boxed tests are the falsification of the write-only defect: on a
    // TUI with no output read at all they fail with every byte the daemon is
    // holding in its ring still absent from the rendered buffer.

    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use codegg_core::workspace::{ExecutionContext, InMemoryWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
    };
    use tokio_util::sync::CancellationToken;

    use crate::interactive_process_attach::InteractiveProcessProtocol;

    /// Production pull cadence. Must equal the event-loop tick: the pull rides
    /// the pump tick and adds no second timer (I4).
    const PULL_TIMEOUT: Duration = Duration::from_secs(15);
    const CLIENT_ID: &str = "client-c001";

    /// A `CoreClient` backed by the **real** M002 interactive-process protocol, so
    /// the acceptance tests below travel the same requests the daemon answers.
    struct PtyCoreClient {
        protocol: InteractiveProcessProtocol,
        ctx: Arc<ExecutionContext>,
    }

    #[async_trait::async_trait]
    impl crate::core::CoreClient for PtyCoreClient {
        async fn request(
            &self,
            envelope: crate::protocol::core::RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, crate::error::AppError> {
            let response = match envelope.payload {
                CoreRequest::InteractiveProcessCreate { request } => {
                    self.protocol
                        .create(
                            CLIENT_ID,
                            &self.ctx,
                            codegg_core::approval::SandboxProfile::FullHost,
                            &request,
                            None,
                        )
                        .await
                }
                CoreRequest::InteractiveProcessAttach {
                    handle,
                    from_seq,
                    max_bytes,
                } => {
                    self.protocol
                        .attach(CLIENT_ID, &handle, from_seq, max_bytes)
                        .await
                }
                CoreRequest::InteractiveProcessResume {
                    attachment_id,
                    from_seq,
                    max_bytes,
                } => {
                    self.protocol
                        .resume(CLIENT_ID, &attachment_id, from_seq, max_bytes)
                        .await
                }
                CoreRequest::InteractiveProcessInput {
                    attachment_id,
                    data_b64,
                } => {
                    self.protocol
                        .input(CLIENT_ID, &attachment_id, &data_b64)
                        .await
                }
                CoreRequest::InteractiveProcessResize {
                    attachment_id,
                    cols,
                    rows,
                } => {
                    self.protocol
                        .resize(CLIENT_ID, &attachment_id, cols, rows)
                        .await
                }
                CoreRequest::InteractiveProcessDetach { attachment_id } => {
                    self.protocol.detach(CLIENT_ID, &attachment_id).await
                }
                other => CoreResponse::Error {
                    code: "fixture_unsupported".to_string(),
                    message: format!("{other:?}"),
                },
            };
            Ok(response)
        }

        fn subscribe(
            &self,
        ) -> tokio::sync::mpsc::Receiver<
            crate::protocol::core::EventEnvelope<crate::protocol::core::CoreEvent>,
        > {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            rx
        }
    }

    async fn pull_test_context(root: &Path) -> Arc<ExecutionContext> {
        let store = Arc::new(InMemoryWorkspaceStore::new());
        let registry = WorkspaceRegistry::load(store).await.expect("registry");
        let record = registry.get_or_register(root).await.expect("register");
        let _services = WorkspaceServiceRegistry::new(
            registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        ExecutionContext::new(
            record,
            Some("session-c001".to_string()),
            CancellationToken::new(),
        )
    }

    /// A bound session so `resolve_terminal_workspace` has its canonical
    /// workspace. Terminals never fall back to ambient cwd.
    fn bind_workspace_session(app: &mut App, workspace_id: &str, directory: &Path) {
        let session = crate::session::models::Session {
            id: "s-c001".to_string(),
            project_id: "p-c001".to_string(),
            workspace_id: Some(workspace_id.to_string()),
            directory: directory.display().to_string(),
            ..Default::default()
        };
        app.session_state.session = Some(session);
    }

    /// Apply every `TuiCommand` the spawned terminal tasks published, exactly as
    /// the event loop's `cmd_rx` arm does.
    async fn drain_tui_commands(app: &mut App) -> usize {
        let mut applied = 0usize;
        while let Some(rx) = app.tui_cmd_rx.as_mut() {
            match tokio::time::timeout(Duration::from_millis(25), rx.recv()).await {
                Ok(Some(cmd)) => {
                    crate::tui::runtime::command_dispatch::dispatch_tui_command(app, cmd).await;
                    applied += 1;
                }
                _ => break,
            }
        }
        applied
    }

    fn render_buffer_text(app: &mut App, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| app.render(frame))
            .expect("render must stay panic-free");
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    /// Type `text` then `Enter` through the real key router, one `on_key` per
    /// keystroke. There is no test-side shortcut into `send_terminal_input`: the
    /// bytes have to travel the same gate a human's keystrokes do.
    fn type_through_app(app: &mut App, text: &str) {
        for ch in text.chars() {
            app.on_key(key(crossterm::event::KeyCode::Char(ch)));
        }
        app.on_key(key(crossterm::event::KeyCode::Enter));
    }

    /// Spin the production cadence until `marker` is visible in the rendered
    /// buffer. Returns the buffer that satisfied the predicate.
    async fn pull_until_rendered(
        app: &mut App,
        handle: &str,
        marker: &str,
        width: u16,
        height: u16,
    ) -> String {
        let started = Instant::now();
        loop {
            // The cadence service the event loop's pump tick calls, plus the same
            // command drain the `cmd_rx` arm performs.
            service_terminal_output_pull(app);
            drain_tui_commands(app).await;
            // Transient notifications are cleared before asserting. Toasts
            // expire on wall-clock in production and this loop runs faster than
            // that, so an INFO left behind by the INS gesture or the first
            // resize sits on top of the panel and hides the very output this
            // test exists to see. The terminal's steady-state surface is the
            // subject; the notification layer is not.
            app.messages_state.toasts = crate::tui::components::toast::ToastManager::new();
            let rendered = render_buffer_text(app, width, height);
            if rendered.contains(marker) {
                return rendered;
            }
            if started.elapsed() > PULL_TIMEOUT {
                // Report the projection too, not just the frame: "the box is
                // empty" and "the box never learned the bytes" look identical in
                // a buffer dump and have completely different causes.
                let projected = app
                    .interactive_terminals
                    .view(handle)
                    .map(|view| {
                        format!(
                            "{}\nprojected rows: {:#?}",
                            view.status_line(),
                            app.interactive_terminals.render_lines(handle, 40)
                        )
                    })
                    .unwrap_or_else(|| "<no view>".to_string());
                panic!(
                    "the cadence never made {marker:?} visible in the rendered buffer.\n\
                 view: {projected}\n\
                 focus_manager={} dialog={:?} sidebar={} panel={:?} area={:?}\n\
                 last_render_error={:?}\n\
                 last rendered surface:\n{rendered}",
                    app.focus_manager.len(),
                    app.ui_state.dialog,
                    app.ui_state.sidebar_visible,
                    app.panel_host.active(),
                    app.dialog_area,
                    app.ui_state.last_render_error,
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// An echo-free `cat`: `stty raw -echo` means nothing the user types comes
    /// back from the line discipline, so the marker can only appear if the TUI read
    /// the PTY's output and projected it.
    fn marker_shell() -> Vec<String> {
        vec![
            "sh".to_string(),
            "-c".to_string(),
            "stty raw -echo; exec cat".to_string(),
        ]
    }

    async fn pull_test_app(dir: &Path, argv: Vec<String>) -> App {
        let mut app = pull_app(dir).await;
        start_terminal_create(&mut app, argv);
        app
    }

    /// The panel route: the shell lands in the docked side panel instead of the
    /// box. `terminal_panel_spawn` is the one bit of correlation state the panel
    /// route uses — set it directly because [`spawn_default_terminal_for_panel`]
    /// hardcodes the default shell and this fixture needs an echo-free `cat`.
    async fn pull_test_panel_app(dir: &Path, argv: Vec<String>) -> App {
        let mut app = pull_app(dir).await;
        app.ui_state.sidebar_visible = true;
        app.terminal_panel_spawn = true;
        start_terminal_create(&mut app, argv);
        app
    }

    async fn pull_app(dir: &Path) -> App {
        let ctx = pull_test_context(dir).await;
        let mut config = crate::scheduler::config::ResolvedSchedulerConfig::default();
        config.resources.max_process_slots = 8;
        let protocol = InteractiveProcessProtocol::new(Arc::new(
            crate::scheduler::admission::AdmissionController::new(config),
        ));
        let mut app = test_app();
        app.set_core_client(Arc::new(PtyCoreClient {
            protocol,
            ctx: ctx.clone(),
        }));
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        app.tui_cmd_tx = Some(tx);
        app.tui_cmd_rx = Some(rx);
        bind_workspace_session(&mut app, ctx.workspace_id.as_str(), dir);
        app
    }

    /// Select the terminal panel through the panel manager — the same keystrokes a
    /// user presses — and put the app in Normal mode, which is the only mode in
    /// which a panel holds the keyboard.
    /// Wait until the create command has been **applied** to `app`, and return the
    /// handle it produced.
    ///
    /// One [`drain_tui_commands`] is not enough, and the gap is invisible. It stops
    /// at the first 25ms silence in the command channel, but the *create* is what
    /// materialises the view, opens the box and gives the panel something to adopt.
    /// A drain that consumes the create and then finds the channel quiet for 25ms
    /// returns with no terminal at all, and the test that reads
    /// `panel_host.terminal_handle()` or `ui_state.dialog` next fails on an
    /// assertion that has nothing to do with the behaviour it means to pin.
    ///
    /// This is the same defect, one step earlier than [`await_live_attachment`]:
    /// that one waits for the attach, this one waits for the create that spawns
    /// it. Both are latent in the C003 fixtures and both surface as "the premise
    /// failed" rather than as a race, which is why they are worth naming.
    async fn await_terminal_create(app: &mut App) -> String {
        let started = Instant::now();
        loop {
            if let Some(handle) = app.interactive_terminals.active_handle() {
                return handle.to_string();
            }
            assert!(
                started.elapsed() <= PULL_TIMEOUT,
                "the terminal create never landed; toasts={:?}",
                toast_messages(app)
            );
            drain_tui_commands(app).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Wait until `handle` holds a live attachment.
    ///
    /// **A single [`drain_tui_commands`] is not enough, and the gap is invisible.**
    /// It stops at the first 25ms silence, but the attach is spawned *by* the
    /// create completion — so a drain that consumed the create and then found the
    /// channel quiet for 25ms returns with a view that exists and has no
    /// attachment. `i` against such a view is refused by
    /// [`InteractiveTerminalController::focus`], the refusal re-attaches instead of
    /// focusing, and the test that pressed `i` "to take the PTY" silently went on
    /// to assert against a terminal nobody was typing into.
    ///
    /// That was a latent race in the fixtures until the hint row narrowed to
    /// `None`: a panel that never got the keyboard looks exactly like a panel that
    /// just handed it back, so the stale fixture kept passing and reported the
    /// failure somewhere else entirely.
    async fn await_live_attachment(app: &mut App, handle: &str) {
        let started = Instant::now();
        while app.interactive_terminals.attachment_id(handle).is_none() {
            assert!(
                started.elapsed() <= PULL_TIMEOUT,
                "the attachment for {handle} never landed; view={:?}",
                app.interactive_terminals
                    .view(handle)
                    .map(|view| view.status_line())
            );
            service_terminal_output_pull(app);
            drain_tui_commands(app).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn select_terminal_panel(app: &mut App) {
        use crate::tui::panels::SidePanelId;
        app.ui_state.input_mode = crate::tui::input::InputMode::Normal;
        let from = app.panel_host.active().index();
        let to = SidePanelId::Terminal.index();
        app.on_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('S'),
            crossterm::event::KeyModifiers::SHIFT,
        ));
        for _ in 0..(to + SidePanelId::ALL.len() - from) % SidePanelId::ALL.len() {
            app.on_key(key(crossterm::event::KeyCode::Char('j')));
        }
        app.on_key(key(crossterm::event::KeyCode::Enter));
        app.ui_state.input_mode = crate::tui::input::InputMode::Normal;
        assert_eq!(
            app.panel_host.active(),
            SidePanelId::Terminal,
            "the terminal panel must be the live panel before its keys mean anything"
        );
        assert!(
            app.panel_host.is_focused(),
            "the terminal panel must hold the keyboard before its keys reach it"
        );
    }

    /// Box surface: create, attach, type, let the cadence pull, then read the
    /// rendered buffer.
    ///
    /// This is the acceptance test for the write-only defect. `cat` runs with
    /// `stty raw -echo`, so the line discipline echoes nothing: the marker can only
    /// reach the screen if the TUI read the PTY's output back and projected it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn box_rendered_buffer_shows_live_pty_output() {
        let dir = tempfile::tempdir().expect("workspace");
        let marker = "CODEGG-LIVE-PULL-BOX";
        let mut app = pull_test_app(dir.path(), marker_shell()).await;

        await_terminal_create(&mut app).await;
        assert_eq!(
            app.ui_state.dialog,
            crate::tui::Dialog::Terminal,
            "the box must be mounted before its output can be asserted on"
        );
        let handle = app
            .dialog_state
            .terminal_detail_handle
            .clone()
            .expect("the box is showing a terminal");
        assert!(
            app.interactive_terminals.is_focused(&handle),
            "the box opens in insert, so the keystrokes below are meaningful"
        );
        assert!(
            !app.focus_manager.is_empty(),
            "the box owns a pushed dialog; without one there is nothing to render"
        );

        // Let the viewport resize land *before* typing. A PTY resize flushes the
        // tty's pending input, so a resize issued mid-word is indistinguishable
        // from lost keystrokes — and this test is about liveness, not about what a
        // resize does to a half-typed line.
        service_terminal_output_pull(&mut app);
        drain_tui_commands(&mut app).await;
        assert_eq!(
            app.ui_state.dialog,
            crate::tui::Dialog::Terminal,
            "one cadence pass must not unmount the box it is reading for"
        );

        type_through_app(&mut app, marker);
        assert_eq!(
            app.ui_state.dialog,
            crate::tui::Dialog::Terminal,
            "typing at the PTY must not unmount the box either"
        );
        let rendered = pull_until_rendered(&mut app, &handle, marker, 120, 40).await;

        assert!(
            rendered.contains(marker),
            "the box must show live PTY output; rendered surface:\n{rendered}"
        );
    }

    /// Panel surface: the same chain, but the shell lands in the docked side
    /// panel, which — unlike the box — does not open focused.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panel_rendered_buffer_shows_live_pty_output() {
        let dir = tempfile::tempdir().expect("workspace");
        let marker = "CODEGG-LIVE-PULL-PANEL";
        let mut app = pull_test_panel_app(dir.path(), marker_shell()).await;

        drain_tui_commands(&mut app).await;
        let handle = app
            .panel_host
            .terminal_handle()
            .expect("the panel adopted the new terminal")
            .to_string();
        assert!(
            !app.interactive_terminals.is_focused(&handle),
            "the panel never steals the keyboard: `i` is the documented way in"
        );

        select_terminal_panel(&mut app);
        app.on_key(key(crossterm::event::KeyCode::Char('i')));
        assert!(
            app.interactive_terminals.is_focused(&handle),
            "`i` on a docked live terminal is the documented route into INS mode"
        );

        // Let the panel's first geometry resize land *before* typing, for the
        // same reason the box does: a PTY resize flushes the tty's pending
        // input, so a resize issued mid-word is indistinguishable from lost
        // keystrokes.
        service_terminal_output_pull(&mut app);
        drain_tui_commands(&mut app).await;

        type_through_app(&mut app, marker);
        let rendered = pull_until_rendered(&mut app, &handle, marker, 120, 40).await;

        assert!(
            rendered.contains(marker),
            "the panel must show live PTY output; rendered surface:\n{rendered}"
        );
    }

    // --- C003: the loop has to actually reach the pull -------------------
    //
    // Everything above drives `service_terminal_output_pull` itself. That
    // proves the pull issues a bounded read, that the completion is applied,
    // and that both surfaces repaint — and it proves none of the thing a user
    // actually complained about, which is that the bytes arrive on their own.
    //
    // The gap is one layer up. `service_terminal_output_pull` has exactly one
    // call site, in the timed arm of the loop's `select!`, and that arm parks
    // on `futures_util::future::pending()` whenever `next_wake` is `None`.
    // `next_wake` is built from `animation_interval`, which used to answer
    // `None` for a completely idle frame: not streaming, no turn running, no
    // toast. A mounted, live terminal was therefore read only while a toast
    // happened to be alive, a keystroke arrived, or a resize was debouncing —
    // so a shell's output became visible when the user pressed the *next*
    // key. The tests above supply the polling loop from the harness, which is
    // exactly the gap: they stand in for the arm they were meant to check.

    /// Assert the frame is idle in every way the loop already knows about, so
    /// a passing [`animation_interval`] can only be the terminal's doing.
    fn assert_the_frame_is_idle_except_for_the_terminal(app: &App) {
        assert!(
            !app.streaming_active,
            "a streaming turn is one of the loop's pre-existing wake sources; it would \
             mask the terminal's"
        );
        assert!(
            !matches!(
                app.session_state.session_status,
                crate::tui::app::SessionStatus::Working
            ),
            "a working turn is one of the loop's pre-existing wake sources; it would \
             mask the terminal's"
        );
        assert!(
            app.messages_state.toasts.is_empty(),
            "a live toast is one of the loop's pre-existing wake sources; it would \
             mask the terminal's"
        );
        assert!(
            app.ui_state.resize_debounce.is_none(),
            "a pending resize debounce is a pre-existing wake source; it would mask \
             the terminal's"
        );
    }

    /// Spin the **production** arming decision plus the production pull, and
    /// stop as soon as `marker` is visible.
    ///
    /// What the harness still supplies is the tick itself — the loop's sleep
    /// and `select!` cannot run without a real terminal. What it no longer
    /// supplies is the decision that decides whether the tick happens: every
    /// pass asserts [`animation_interval`] first, with all three pre-existing
    /// contributors explicitly absent. A pull that works while nothing arms it
    /// cannot pass this, which is the whole point.
    async fn pull_until_rendered_armed(
        app: &mut App,
        handle: &str,
        marker: &str,
        width: u16,
        height: u16,
    ) -> String {
        let started = Instant::now();
        loop {
            // Clear toasts *before* the assertion, not after the render: a toast
            // produced by the previous pass must not be able to arm the loop on
            // this one's behalf.
            app.messages_state.toasts = crate::tui::components::toast::ToastManager::new();
            assert_the_frame_is_idle_except_for_the_terminal(app);
            assert!(
                crate::tui::runtime::event_loop::animation_interval(app).is_some(),
                "the event loop is not armed for a mounted live terminal surface.\n\
                 `animation_interval` returned None, so the loop's timed arm selects \
                 `futures_util::future::pending()` and `service_terminal_output_pull` \
                 is never reached: output can only appear when the user presses the \
                 next key, a toast happens to be alive, or a resize debounces.\n\
                 streaming_active={} status={:?} toasts={} resize_debounce={:?} \
                 mounted={:?}",
                app.streaming_active,
                app.session_state.session_status,
                app.messages_state.toasts.len(),
                app.ui_state.resize_debounce,
                mounted_terminal_surface(app),
            );

            service_terminal_output_pull(app);
            drain_tui_commands(app).await;
            // Transient notifications are cleared before rendering for the same
            // reason the sibling helper does: they cover the surface under test.
            app.messages_state.toasts = crate::tui::components::toast::ToastManager::new();
            let rendered = render_buffer_text(app, width, height);
            if rendered.contains(marker) {
                return rendered;
            }
            if started.elapsed() > PULL_TIMEOUT {
                let projected = app
                    .interactive_terminals
                    .view(handle)
                    .map(|view| {
                        format!(
                            "{}\nprojected rows: {:#?}",
                            view.status_line(),
                            app.interactive_terminals.render_lines(handle, 40)
                        )
                    })
                    .unwrap_or_else(|| "<no view>".to_string());
                panic!(
                    "the armed cadence never made {marker:?} visible in the rendered \
                     buffer.\nview: {projected}\nlast rendered surface:\n{rendered}"
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The headline acceptance test, and the one that fails at baseline.
    ///
    /// A shell is attached, a command is typed through the real key router, and
    /// then the app is left strictly alone. Output must reach the rendered
    /// surface anyway. The only thing driving it is the loop's own arming
    /// decision and its own pull — no keystroke, no injected toast, no resize,
    /// no turn event.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_live_terminal_surface_repaints_with_no_further_user_input() {
        let dir = tempfile::tempdir().expect("workspace");
        let marker = "CODEGG-IDLE-LIVENESS";
        let mut app = pull_test_app(dir.path(), marker_shell()).await;

        drain_tui_commands(&mut app).await;
        let handle = app
            .dialog_state
            .terminal_detail_handle
            .clone()
            .expect("the box is showing a terminal");
        assert!(
            app.interactive_terminals.is_focused(&handle),
            "the box opens in insert, so the keystrokes below are meaningful"
        );

        // Let the viewport resize land before typing: a PTY resize flushes the
        // tty's pending input, so a resize issued mid-word is
        // indistinguishable from lost keystrokes, and this test is about
        // liveness rather than about what a resize does to a half-typed line.
        service_terminal_output_pull(&mut app);
        drain_tui_commands(&mut app).await;

        // The last keystroke of the whole test.
        type_through_app(&mut app, marker);
        // Named here so a later failure cannot be mistaken for a liveness one:
        // if the surface was not taking input, these bytes went to the composer,
        // which submits a turn and leaves the session `Working` — and the
        // arming assertion would then be reporting a fixture problem as a
        // cadence problem.
        assert!(
            app.interactive_terminals.is_focused(&handle),
            "the box must still hold the keyboard for the process, or the marker was \
             typed into the composer instead of the PTY"
        );
        app.messages_state.toasts = crate::tui::components::toast::ToastManager::new();

        let rendered = pull_until_rendered_armed(&mut app, &handle, marker, 120, 40).await;
        assert!(
            rendered.contains(marker),
            "a live mounted terminal must show its output without the user touching \
             anything; rendered surface:\n{rendered}"
        );
    }

    /// The reported case: a docked panel that has given the keyboard back.
    ///
    /// `Shift+Left` from a right-docked panel is how the user returns to the
    /// main buffer, and this asserts that the surface stays live afterwards.
    /// That is the case the arm used to fail, and the reason the contributor
    /// is keyed on **mount** rather than focus: focus is the one thing that
    /// changes exactly when nobody is looking at the panel.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_docked_terminal_keeps_output_after_the_keyboard_leaves_the_panel() {
        let dir = tempfile::tempdir().expect("workspace");
        // Short: the docked column is narrower than the box, and a marker that
        // wraps mid-word would prove the cadence while failing the substring.
        let marker = "IDLE-PANEL-LIVE";
        let mut app = pull_test_panel_app(dir.path(), marker_shell()).await;

        await_terminal_create(&mut app).await;
        select_terminal_panel(&mut app);
        let handle = app
            .panel_host
            .terminal_handle()
            .expect("the panel adopted the new terminal")
            .to_string();

        app.on_key(key(crossterm::event::KeyCode::Char('i'))); // Insert: reach the PTY below.
        service_terminal_output_pull(&mut app);
        drain_tui_commands(&mut app).await;
        type_through_app(&mut app, marker);
        assert!(
            app.interactive_terminals.is_focused(&handle),
            "the panel must still hold the keyboard for the process, or the marker was \
             typed into the composer — which would submit a turn, park the session in \
             `Working`, and make the arming assertion below report a fixture problem \
             as a cadence problem"
        );
        app.messages_state.toasts = crate::tui::components::toast::ToastManager::new();

        // Hand the keyboard back: `Esc` leaves Insert, then `Shift+Left` off
        // the panel's inner edge returns the keyboard to the main buffer.
        app.on_key(key(crossterm::event::KeyCode::Esc));
        app.on_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('H'),
            crossterm::event::KeyModifiers::SHIFT,
        ));
        assert!(
            !app.panel_host.is_focused(),
            "the main buffer owns the keyboard again"
        );
        assert!(
            app.panel_host
                .terminal_handle()
                .is_some_and(|h| mounted_terminal_surface(&app).as_deref() == Some(h)),
            "and the terminal is still on the panel"
        );

        // Nothing between here and the first assertion but the app being
        // alive: no keystroke, no toast, no resize, no turn event.
        let rendered = pull_until_rendered_armed(&mut app, &handle, marker, 120, 40).await;
        assert!(
            rendered.contains(marker),
            "a docked terminal must keep producing output after the keyboard leaves it; \
             rendered surface:\n{rendered}"
        );
    }

    /// The visibility rule still bounds the arm.
    ///
    /// A terminal that is attached and alive but on nobody's screen must cost
    /// nothing. Treating "adopted" as "mounted" is how a background terminal
    /// ends up polling forever for a user who cannot see it, so this pins the
    /// negative alongside the positives.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_hidden_terminal_surface_does_not_arm_the_loop() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_app(dir.path(), marker_shell()).await;

        drain_tui_commands(&mut app).await;
        assert!(
            mounted_terminal_surface(&app).is_some(),
            "the fixture must start with a mounted surface"
        );

        // Close the box: attached, alive, and on no surface at all.
        app.ui_state.dialog = crate::tui::Dialog::None;
        app.dialog_state.terminal_detail_handle = None;
        assert!(
            mounted_terminal_surface(&app).is_none(),
            "a closed box is not on anyone's screen"
        );
        assert!(
            crate::tui::runtime::event_loop::animation_interval(&app).is_none(),
            "an unmounted surface must cost nothing: the loop parks exactly as it \
             did before any terminal existed"
        );
    }

    // --- C003: one derived answer for the mode the user is in ------------
    //
    // `ui_state.input_mode` is the *composer's* mode and `focus_panel` forces
    // it to `Normal` whenever a panel takes the keyboard, so the badge and the
    // hint line both read the wrong field while the user was typing into a
    // shell. One accessor now resolves it for both.

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_badge_names_insert_while_the_terminal_box_takes_keystrokes() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_app(dir.path(), marker_shell()).await;

        drain_tui_commands(&mut app).await;
        let handle = app
            .dialog_state
            .terminal_detail_handle
            .clone()
            .expect("the box is showing a terminal");
        assert!(
            app.interactive_terminals.is_focused(&handle),
            "the box opens in insert, so this is a surface with live input"
        );

        assert_eq!(
            app.composer_input_mode(),
            crate::tui::input::InputMode::Insert,
            "a surface taking keystrokes for a process is in Insert"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_badge_returns_to_normal_after_esc_even_though_the_composer_forgot() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_app(dir.path(), marker_shell()).await;

        drain_tui_commands(&mut app).await;
        app.on_key(key(crossterm::event::KeyCode::Esc));

        // The composer's private mode was never touched by the box, so it still
        // claims Insert. Reporting it would be reporting a field nothing on
        // screen is using.
        assert_eq!(
            app.ui_state.input_mode,
            crate::tui::input::InputMode::Insert,
            "the box never writes the composer's mode, so this stale field is the trap"
        );
        assert_eq!(
            app.composer_input_mode(),
            crate::tui::input::InputMode::Normal,
            "Esc handed the keyboard to the reader, and Normal is then the honest answer"
        );
    }

    /// The reported symptom, pinned at its source.
    ///
    /// `focus_panel` forces `Normal` on panel focus, so a docked terminal in
    /// Insert leaves `ui_state.input_mode` reading `Normal` while the user types
    /// into a shell. The badge rendered that field, and the terminal panel's own
    /// Insert hint branch — which branches on the same field — was unreachable.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_docked_terminal_in_insert_is_reported_as_insert() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_panel_app(dir.path(), marker_shell()).await;

        await_terminal_create(&mut app).await;
        select_terminal_panel(&mut app);
        app.on_key(key(crossterm::event::KeyCode::Char('i')));
        let handle = app
            .panel_host
            .terminal_handle()
            .expect("the panel adopted the new terminal")
            .to_string();
        assert!(
            app.interactive_terminals.is_focused(&handle),
            "the panel now holds the keyboard for the process"
        );
        assert_eq!(
            app.ui_state.input_mode,
            crate::tui::input::InputMode::Normal,
            "focus_panel pinned the composer's own mode to Normal: this is the field \
             the badge used to render, and it is wrong here"
        );

        assert_eq!(
            app.composer_input_mode(),
            crate::tui::input::InputMode::Insert,
            "a panel holding a live PTY for typing is in Insert regardless of what the \
             composer privately believes"
        );

        let hints = app
            .composer_hint_text()
            .expect("a visible terminal panel always names itself");
        assert!(
            hints.contains("INS typing to the shell"),
            "the terminal's own Insert hint must be reachable, not dead code: {hints}"
        );
        assert!(
            !hints.contains("hjkl move"),
            "and it must not advertise reader navigation to someone typing: {hints}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_docked_terminal_reads_normal_once_it_lets_go() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_panel_app(dir.path(), marker_shell()).await;

        await_terminal_create(&mut app).await;
        select_terminal_panel(&mut app);
        // The attachment is awaited rather than assumed. See
        // `await_live_attachment`: one drain cannot cover a create whose
        // completion is what spawns the attach.
        let handle = app
            .panel_host
            .terminal_handle()
            .expect("the panel adopted the new terminal")
            .to_string();
        await_live_attachment(&mut app, &handle).await;
        app.on_key(key(crossterm::event::KeyCode::Char('i')));
        app.on_key(key(crossterm::event::KeyCode::Esc));
        assert!(
            app.panel_host.is_focused(),
            "Esc leaves the PTY, not the panel: the reader still holds the keyboard"
        );

        assert_eq!(
            app.composer_input_mode(),
            crate::tui::input::InputMode::Normal,
            "Esc handed the keys back and the reader holds them"
        );
        let hints = app
            .composer_hint_text()
            .expect("a focused terminal panel always names itself");
        assert!(
            hints.contains("hjkl move"),
            "reader navigation is the hint that applies now: {hints}"
        );
        assert!(
            !hints.contains("INS typing"),
            "and it must not tell someone reading a shell that they are typing into it: \
             {hints}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_badge_reports_the_composers_own_mode_when_no_surface_holds_it() {
        let mut app = test_app();
        app.ui_state.input_mode = crate::tui::input::InputMode::Insert;

        assert_eq!(
            app.composer_input_mode(),
            crate::tui::input::InputMode::Insert,
            "with nothing else holding the keyboard the composer's mode is the truth"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reporting_the_mode_never_moves_the_keyboard() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_app(dir.path(), marker_shell()).await;
        drain_tui_commands(&mut app).await;
        let handle = app
            .dialog_state
            .terminal_detail_handle
            .clone()
            .expect("the box is showing a terminal");

        let before = (
            app.composer_input_mode(),
            app.panel_host.is_focused(),
            app.interactive_terminals.is_focused(&handle),
            app.ui_state.input_mode,
        );

        // Read it repeatedly, then render the real frame that draws the badge.
        for _ in 0..3 {
            let _ = app.composer_input_mode();
            let _ = app.composer_hint_text();
        }
        let _ = render_buffer_text(&mut app, 120, 40);

        assert_eq!(
            (
                app.composer_input_mode(),
                app.panel_host.is_focused(),
                app.interactive_terminals.is_focused(&handle),
                app.ui_state.input_mode,
            ),
            before,
            "reading the effective mode must not change focus, the terminal, or the \
             composer's private mode"
        );
    }

    // --- C002: the refusal must name the transport, not the daemon ---------
    //
    // The daemon on the `/tui` attach frontend is present and healthy; it just
    // does not carry interactive-process traffic to this frontend. Every
    // terminal entry point used to answer that with "Daemon connection
    // unavailable", which sends the reader to restart something that is working.
    //
    // Both halves of this are load-bearing. If the attach refusal were the only
    // thing pinned, deleting the connection wording would still pass; the
    // second test below is what keeps "Daemon connection unavailable" honest
    // for the frontend where it *is* the truth.

    // ── A prompt line after new and after clear ───────────────────────

    /// A real shell that paints a prompt and repaints it after every line it
    /// reads.
    ///
    /// The marker matters more than the shell: what these tests assert is that
    /// a line the *process* printed reaches the screen. A fixture that echoed
    /// the clear back would pass with a local wipe and no process interaction
    /// at all, which is the exact fabrication the milestone forbids.
    fn prompt_shell() -> Vec<String> {
        vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf 'CLEAR-PROMPT> '; while IFS= read -r line; do printf 'CLEAR-PROMPT> '; done"
                .to_string(),
        ]
    }

    /// A newly created terminal must reach its own prompt with no keystroke.
    ///
    /// This is the "new" half of the reported defect and it is C003's armed
    /// cadence doing the work: the prompt was already arriving on attach, but
    /// nothing read it until the user pressed a key. Asserted through the
    /// armed helper, which also fails if the loop stops being armed — so this
    /// cannot pass by accidentally introducing a second wake source.
    #[tokio::test(flavor = "current_thread")]
    async fn a_new_terminal_shows_the_processes_own_prompt_without_a_keystroke() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_app(dir.path(), prompt_shell()).await;
        drain_tui_commands(&mut app).await;
        let handle = app
            .dialog_state
            .terminal_detail_handle
            .clone()
            .expect("the box is showing a terminal");
        await_live_attachment(&mut app, &handle).await;

        let rendered = pull_until_rendered_armed(&mut app, &handle, "CLEAR-PROMPT", 120, 40).await;

        assert!(
            rendered.contains("CLEAR-PROMPT"),
            "a fresh terminal must show the prompt its shell printed; rendered:\n{rendered}"
        );
    }

    /// The reported defect: `C` blanked the screen and left it blank, because
    /// the wipe was entirely local and the shell had no reason to redraw.
    ///
    /// The assertion is on *where the prompt came from*: the shell is a real
    /// `sh` reading a real PTY, and the prompt it prints after the clear is
    /// what lands on screen. A synthesised line would satisfy a weaker test and
    /// would also be a lie for any non-default `PS1`.
    #[tokio::test(flavor = "current_thread")]
    async fn a_cleared_terminal_is_replaced_by_a_prompt_the_process_printed() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_panel_app(dir.path(), prompt_shell()).await;
        await_terminal_create(&mut app).await;
        select_terminal_panel(&mut app);
        let handle = app
            .panel_host
            .terminal_handle()
            .expect("the panel adopted the new terminal")
            .to_string();
        await_live_attachment(&mut app, &handle).await;
        pull_until_rendered_armed(&mut app, &handle, "CLEAR-PROMPT", 120, 40).await;

        app.on_key(key(crossterm::event::KeyCode::Char('i')));
        assert!(app.interactive_terminals.is_focused(&handle));
        // Let the panel's first geometry resize land before typing: a PTY
        // resize flushes the tty's pending input.
        service_terminal_output_pull(&mut app);
        drain_tui_commands(&mut app).await;
        // Short: the docked column is 28 wide and already spends 14 of them on
        // the prompt, so a longer marker wraps mid-word and proves the clear
        // while failing the substring.
        type_through_app(&mut app, "PRECLR");
        pull_until_rendered(&mut app, &handle, "PRECLR", 120, 40).await;

        // Back to reading, then clear.
        app.on_key(key(crossterm::event::KeyCode::Esc));
        assert!(!app.interactive_terminals.is_focused(&handle));
        app.on_key(key(crossterm::event::KeyCode::Char('C')));

        let rendered = pull_until_rendered_armed(&mut app, &handle, "CLEAR-PROMPT", 120, 40).await;
        assert!(
            !rendered.contains("PRECLR"),
            "clear must actually clear; rendered:\n{rendered}"
        );
    }

    /// The clear may not manufacture a resync: the daemon keeps streaming
    /// from wherever it was, and a reset sequence cursor would make the very
    /// next honest chunk fail its gap check and print a "history expired"
    /// notice the reader did not cause.
    #[tokio::test(flavor = "current_thread")]
    async fn clear_preserves_the_sequence_cursor() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_app(dir.path(), prompt_shell()).await;
        drain_tui_commands(&mut app).await;
        let handle = app
            .dialog_state
            .terminal_detail_handle
            .clone()
            .expect("the box is showing a terminal");
        await_live_attachment(&mut app, &handle).await;
        pull_until_rendered(&mut app, &handle, "CLEAR-PROMPT", 120, 40).await;

        let status = |app: &App| {
            app.interactive_terminals
                .view(&handle)
                .map(|view| view.status_line())
                .unwrap_or_default()
        };
        let before = status(&app);
        clear_terminal(&mut app, &handle);
        let after = status(&app);
        let seq = |line: &str| {
            line.split("seq=")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .map(str::to_string)
        };
        assert_eq!(
            seq(&after),
            seq(&before),
            "clear must not move the sequence cursor (before {before:?}, after {after:?})"
        );
        assert!(
            !after.contains("resync"),
            "clear must not manufacture a resync: {after:?}"
        );
    }

    /// A view with no live process behind it still clears, and is not made to
    /// claim otherwise.
    ///
    /// The plan's failure semantics suggested leaving the screen intact on any
    /// input refusal. That is wrong for exactly this case: there is no process
    /// left to print a prompt, so nothing is being suppressed, and refusing
    /// would take away the only way to clear the output of a shell that has
    /// finished. What must not happen is a *pretence* — no warning about a
    /// refused write, because none was attempted.
    #[tokio::test(flavor = "current_thread")]
    async fn clear_on_a_detached_view_clears_locally_without_a_false_refusal() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_app(dir.path(), prompt_shell()).await;
        drain_tui_commands(&mut app).await;
        let handle = app
            .dialog_state
            .terminal_detail_handle
            .clone()
            .expect("the box is showing a terminal");
        await_live_attachment(&mut app, &handle).await;
        pull_until_rendered(&mut app, &handle, "CLEAR-PROMPT", 120, 40).await;

        // The attachment is released: the process runs on, and nothing can be
        // sent to it from here.
        app.interactive_terminals.apply_detached(&handle);
        clear_terminal(&mut app, &handle);

        let remaining = app
            .interactive_terminals
            .render_lines(&handle, 40)
            .expect("render")
            .join("\n")
            .trim()
            .to_string();
        assert!(
            remaining.is_empty(),
            "a detached view must still be clearable, got {remaining:?}"
        );
        assert!(
            toast_messages(&app).is_empty(),
            "no write was attempted, so nothing may be reported as refused: {:?}",
            toast_messages(&app)
        );
    }

    /// A clear that *is* refused must say so, and must not blank the screen on
    /// the strength of a repaint that was never sent.
    #[tokio::test(flavor = "current_thread")]
    async fn a_refused_clear_reports_itself_and_leaves_the_screen_intact() {
        let dir = tempfile::tempdir().expect("workspace");
        let mut app = pull_test_app(dir.path(), prompt_shell()).await;
        drain_tui_commands(&mut app).await;
        let handle = app
            .dialog_state
            .terminal_detail_handle
            .clone()
            .expect("the box is showing a terminal");
        await_live_attachment(&mut app, &handle).await;
        pull_until_rendered(&mut app, &handle, "CLEAR-PROMPT", 120, 40).await;

        // Fill the coalescing buffer to within the clear's own length, so the
        // clear's bytes cannot be admitted. This is the only refusal path that
        // is not "there is no process", and it is the one that must not wipe.
        let filler = vec![b'x'; crate::tui::interactive_terminal::MAX_TERMINAL_INPUT_BYTES - 1];
        assert!(
            app.interactive_terminals
                .queue_input(&handle, &filler)
                .is_ok(),
            "the premise: the buffer is full but not yet over the bound"
        );

        clear_terminal(&mut app, &handle);

        assert!(
            toast_messages(&app)
                .iter()
                .any(|message| message.contains("exceeds the")),
            "a refused clear must name its refusal: {:?}",
            toast_messages(&app)
        );
        let remaining = app
            .interactive_terminals
            .render_lines(&handle, 40)
            .expect("render")
            .join("\n");
        assert!(
            remaining.contains("CLEAR-PROMPT"),
            "a refused clear must leave the screen intact, got {remaining:?}"
        );
    }

    /// The `/tui` WebSocket attach frontend: `AppMode::RemoteCore`, an owned
    /// `remote_send_tx`, and — the fact that matters here — no `CoreClient`.
    /// `src/client/attach.rs` opens `/tui` and speaks `TuiMessage`; it never
    /// constructs one, so every `core_client` guard fires by construction.
    fn ws_attach_app() -> App {
        let mut app = test_app();
        app.ui_state.mode = crate::tui::app::state::AppMode::RemoteCore {
            endpoint: "ws://localhost:3000/tui".into(),
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        app.remote_send_tx = Some(tx);
        assert!(
            app.is_ws_attach_transport(),
            "the fixture must be the attach transport or it proves nothing"
        );
        app.session_state.session = Some(codegg_core::session::Session {
            id: "s-attach".into(),
            project_id: "/tmp".into(),
            workspace_id: Some("/tmp".into()),
            directory: "/tmp".into(),
            ..Default::default()
        });
        app
    }

    /// The same mode with no `remote_send_tx` and no `CoreClient`: the
    /// socket-daemon frontend before it managed to connect. Here "daemon
    /// connection unavailable" is the literal truth and must survive.
    fn socket_daemon_app_without_client() -> App {
        let mut app = test_app();
        app.ui_state.mode = crate::tui::app::state::AppMode::RemoteCore {
            endpoint: "unix:///tmp/codegg.sock".into(),
        };
        assert!(
            app.remote_send_tx.is_none() && !app.is_ws_attach_transport(),
            "without a remote channel this is the socket path, not the attach transport"
        );
        app.session_state.session = Some(codegg_core::session::Session {
            id: "s-socket".into(),
            project_id: "/tmp".into(),
            workspace_id: Some("/tmp".into()),
            directory: "/tmp".into(),
            ..Default::default()
        });
        app
    }

    fn toast_messages(app: &App) -> Vec<String> {
        app.messages_state
            .toasts
            .iter()
            .map(|t| t.message.clone())
            .collect()
    }

    /// The premise of C002, falsified in the product rather than in a comment:
    /// on the attach frontend the refusal names a daemon that is working.
    #[test]
    fn ws_attach_refusal_names_the_transport_not_the_daemon() {
        let mut app = ws_attach_app();
        start_terminal_create(&mut app, vec!["cat".to_string()]);
        let toasts = toast_messages(&app);
        assert!(
            !toasts.is_empty(),
            "a refused terminal command must say something (J2: no silent failure)"
        );
        assert!(
            !toasts
                .iter()
                .any(|t| t.contains("Daemon connection unavailable")),
            "the daemon is healthy on this frontend; blaming it sends the reader to restart \
             something that is working. toasts: {toasts:?}"
        );
        assert!(
            toasts
                .iter()
                .any(|t| t.contains("WebSocket") && t.contains("interactive terminals")),
            "the refusal must name the transport and what it does not carry. toasts: {toasts:?}"
        );
    }

    /// The same command on the socket-daemon frontend with no live connection
    /// keeps its connection wording. Without this, J1's second half would be
    /// satisfied by deleting the message entirely.
    #[test]
    fn socket_daemon_without_a_connection_still_blames_the_connection() {
        let mut app = socket_daemon_app_without_client();
        start_terminal_create(&mut app, vec!["cat".to_string()]);
        let toasts = toast_messages(&app);
        assert!(
            toasts
                .iter()
                .any(|t| t.contains("Daemon connection unavailable")),
            "no connection on a frontend that does support terminals is still a connection \
             problem. toasts: {toasts:?}"
        );
    }

    /// J3, per site: a refused command leaves the app exactly as it was — no
    /// request generation consumed, no background task registered.
    #[test]
    fn a_refused_terminal_command_mutates_nothing() {
        for (label, mut app) in [
            ("ws-attach", ws_attach_app()),
            ("socket-daemon", socket_daemon_app_without_client()),
        ] {
            let dialog_before = app.ui_state.dialog.clone();
            let terminals_before = app.interactive_terminals.len();

            start_terminal_create(&mut app, vec!["cat".to_string()]);
            start_terminal_list(&mut app);
            start_terminal_attach(&mut app, "h-test".to_string());

            assert_eq!(
                app.ui_state.dialog, dialog_before,
                "{label}: a refused command must not open or close a dialog"
            );
            assert_eq!(
                app.interactive_terminals.len(),
                terminals_before,
                "{label}: a refused command must not create a terminal view"
            );
            assert!(
                !app.dialog_state.terminal_request.is_loading(),
                "{label}: the refusal must run before `terminal_request.begin()`, so no \
                 request generation is left outstanding"
            );
            assert_eq!(
                app.task_registry.active_count(),
                0,
                "{label}: a refused command must not register a background task"
            );
        }
    }

    /// J2 on the input path, and the ordering bug this milestone found: the
    /// refusal used to run *after* `take_pending_input` and `begin_input`, so
    /// the keystrokes were dropped and the single input slot was left claimed
    /// with nothing in flight to release it. Every later keystroke then parked
    /// in the queue forever behind a flag nobody would clear — a silent,
    /// permanent stall reported once and never again.
    #[test]
    fn a_refused_flush_keeps_the_bytes_and_releases_the_input_slot() {
        let mut app = socket_daemon_app_without_client();
        let request_id = app.dialog_state.terminal_request.begin();
        apply_terminal_attach_finished(
            &mut app,
            request_id,
            "h-test".to_string(),
            Some("a-1".to_string()),
            Some(test_chunk("$ ")),
            None,
            None,
        );
        app.interactive_terminals.finish_input("h-test");

        send_terminal_input(&mut app, "h-test".to_string(), b"ls -la\n".to_vec());

        assert!(
            !app.interactive_terminals.input_in_flight("h-test"),
            "a refused flush must not leave the input slot claimed; nothing was sent, so nothing \
             will ever complete and release it"
        );
        let mut still_queued = Vec::new();
        while let Some(bytes) = app.interactive_terminals.take_pending_input("h-test") {
            still_queued.extend_from_slice(&bytes);
        }
        assert_eq!(
            String::from_utf8_lossy(&still_queued),
            "ls -la\n",
            "the refused keystrokes must stay queued rather than being dropped"
        );
    }

    /// J4: the C001 cadence must be inert where it cannot work. This mounts a
    /// terminal surface on the attach frontend — the one shape that would let
    /// the pull reach its client lookup — and asserts it issues nothing.
    ///
    /// "No crash" would be satisfied by a pull that runs, finds no client, and
    /// quietly returns every tick forever. The observable is the task: a request
    /// is only ever issued by a spawned task calling `core_client.request`, so
    /// no task means no request.
    #[test]
    fn the_pull_is_inert_on_the_unsupported_transport() {
        let mut app = ws_attach_app();
        app.ui_state.dialog = crate::tui::Dialog::Terminal;
        app.dialog_state.terminal_detail_handle = Some("h-ghost".to_string());

        for _ in 0..50 {
            assert!(
                !service_terminal_output_pull(&mut app),
                "a pull with no client and no live view must not report work"
            );
        }
        assert_eq!(
            app.task_registry.active_count(),
            0,
            "the cadence must not register one task per tick on a transport it cannot read"
        );
        assert!(
            !app.dialog_state.terminal_request.is_loading(),
            "the cadence must not consume a request generation it will never complete"
        );
        assert!(
            toast_messages(&app).is_empty(),
            "a pull that reads nothing has nothing to report; a toast per tick would be a \
             toast storm"
        );
    }

    /// J2, from the other end: not supporting terminals is not a reason to stop
    /// reading the keyboard. The obvious "fix" for a mis-routed key is a
    /// transport branch in `on_key`, and that branch would swallow every
    /// keystroke on the attach frontend — a silent regression in the one thing
    /// that frontend is for.
    #[test]
    fn the_unsupported_transport_does_not_swallow_typed_keys() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = ws_attach_app();
        for c in ['h', 'i'] {
            app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.prompt_state.prompt.get_text(),
            "hi",
            "typing must still reach the composer on the attach frontend"
        );
    }
}
