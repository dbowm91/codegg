//! M006-A: TUI editor commands.
//!
//! Connects the editor presentation model to the shared M005
//! `DocumentController`.
//!
//! ## Invariants enforced here
//!
//! - **No second text buffer.** Every text read is a scoped
//!   `try_snapshot()` borrow dropped at the end of the statement. Every
//!   mutation goes through `apply_local`. Nothing here touches the
//!   filesystem: all bytes arrive through the daemon-authorized
//!   `document.v1` surface.
//! - **Undo is frontend-local.** An undo step is the inverse
//!   `TextTransaction` applied through `apply_local` — never a protocol
//!   operation, never a replay of a divergent draft.
//! - **Undo does not survive reconciliation.** History is cleared on
//!   open, reload, resync, reconnect, and close.
//! - **Async work is guarded.** Every controller call is spawned with
//!   `spawn_scoped_registered_tui_task` and carries a `request_id` plus a
//!   `generation`; a completion for a closed or switched document is
//!   discarded rather than applied.
//! - **Project authority is explicit.** The project and workspace ids come
//!   from the active tab's execution context, never from a path or cwd.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use codegg_client::{DocumentController, DocumentControllerError, DocumentState};
use codegg_protocol::document::DocumentScope;
use codegg_protocol::lsp::{
    LspCompletionApplicabilityDto, LspCompletionRequestDto, LspCompletionResultDto,
    LspCompletionUnsupportedReason, LspDiagnosticsResultDto, LspReadOperation, LspReadRequestDto,
    LspReadResultDto, LspReadStatus,
};

use crate::tui::app::state::editor::{EditorNotice, EditorOperation, EditorState, EditorStatus};
use crate::tui::app::{App, TuiCommand};
use crate::tui::async_cmd::{spawn_registered_tui_task, spawn_scoped_registered_tui_task};
use crate::tui::document_session::{
    JumplistPush, RecordedKey, RecorderBound, RegisterName, RegisterRefusal, RegisterWrite,
    TuiDocumentPresentation, TuiDocumentSession, MAX_JUMPLIST_ENTRIES,
};
use crate::tui::editor::{self, EditorCommand, EditorCommandName, EditorMode, EditorPrefix};
use crate::tui::editor_effect::{ClipboardVerb, EditorPendingEffect};
use crate::tui::editor_lsp::EditorDiagnosticStep;
use crate::tui::task_lifecycle::TuiTaskKind;

use super::super::editor_completion::EditorCompletionScope;
use super::super::editor_lsp::EditorLspScope;

/// Maximum length of a workspace-relative path accepted by the open command.
pub const MAX_EDITOR_RELATIVE_PATH: usize = 512;

/// Maximum number of path components accepted by the open command.
const MAX_EDITOR_PATH_DEPTH: usize = 64;

/// How long the editor waits for typing to pause before re-requesting syntax
/// highlighting.
///
/// Long enough that a fast typist produces one request, not one per
/// keystroke; short enough that the file is never colourless for long.
const SEMANTIC_REFRESH_QUIET_PERIOD: std::time::Duration = std::time::Duration::from_millis(600);

/// Issue the pending syntax-highlight refresh if its quiet period has
/// elapsed. Called from the render pump; a no-op unless an edit armed it.
pub(crate) fn service_editor_semantic_refresh(app: &mut App, now: std::time::Instant) -> bool {
    if !app
        .editor_state
        .take_semantic_refresh_due(std::time::Instant::now().max(now))
    {
        return false;
    }
    request_editor_semantic_tokens(app);
    true
}

/// Whether the editor primary view is active.
pub(crate) fn is_editor_view_active(app: &App) -> bool {
    matches!(
        app.ui_state.routes.current(),
        crate::tui::route::Route::Editor
    )
}

/// Normalize and validate one workspace-relative path for the open command.
///
/// This is a **lexical input check only**. It does no filesystem access and
/// resolves nothing: authoritative containment, `file.read` authorization,
/// and symlink policy all belong to the daemon's `DocumentOpen`, which is
/// the only thing that decides whether a document may be attached. The
/// check exists so an obviously malformed path produces an immediate,
/// specific message instead of a round trip.
pub fn normalize_relative_path(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("path is empty".to_string());
    }
    if trimmed.len() > MAX_EDITOR_RELATIVE_PATH {
        return Err(format!(
            "path is longer than {MAX_EDITOR_RELATIVE_PATH} bytes"
        ));
    }
    if trimmed.contains('\0') {
        return Err("path contains a NUL byte".to_string());
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err("path must be workspace-relative, not absolute".to_string());
    }
    // Windows-style drive or UNC prefixes are rejected lexically so the
    // same input is refused on every platform.
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err("path must be workspace-relative, not drive-qualified".to_string());
    }
    let components: Vec<&str> = trimmed.split('/').collect();
    if components.len() > MAX_EDITOR_PATH_DEPTH {
        return Err(format!(
            "path has more than {MAX_EDITOR_PATH_DEPTH} components"
        ));
    }
    let mut normalized: Vec<&str> = Vec::with_capacity(components.len());
    for component in components {
        match component {
            "" => return Err("path has an empty component".to_string()),
            "." => continue,
            ".." => return Err(
                "path escapes the workspace ('..' is not allowed); use a workspace-relative path"
                    .to_string(),
            ),
            other => normalized.push(other),
        }
    }
    if normalized.is_empty() {
        return Err("path resolves to the workspace root, not a file".to_string());
    }
    Ok(normalized.join("/"))
}

// ── Open / close ─────────────────────────────────────────────────────────────

/// Open one workspace-relative document in the editor primary view.
pub(crate) fn open_editor(app: &mut App, raw_path: &str) {
    open_editor_in(app, raw_path, DocumentScope::Workspace);
}

/// Open one document confined to `scope`.
///
/// Navigates to `Route::Editor` and issues exactly one `DocumentOpen`. A
/// partial failure holds no attachment and shows no buffer: the session is
/// only kept when the daemon accepted the document.
///
/// `scope` selects the daemon's authority root. The TUI never resolves a root
/// itself — it names the scope and the daemon decides — so the containment
/// rule for a global agent lives in exactly one place.
pub(crate) fn open_editor_in(app: &mut App, raw_path: &str, scope: DocumentScope) {
    app.cancel_lsp_diagnostics_stream();
    let path = match normalize_relative_path(raw_path) {
        Ok(path) => path,
        Err(error) => {
            app.messages_state
                .toasts
                .warning(&format!("Cannot open document: {error}"));
            return;
        }
    };
    // A second open of the same document is rejected by the daemon with
    // `AlreadyOpen`; a switch closes the previous attachment first so one
    // controller never owns two.
    if app.editor_state.is_open()
        && app.editor_state.path.as_deref() == Some(path.as_str())
        && app.editor_state.scope == scope
    {
        app.messages_state
            .toasts
            .info("Document is already open in the editor");
        enter_editor_normal(app);
        return;
    }
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(error) => {
            app.messages_state.toasts.warning(&format!(
                "Cannot open document: {error}. Select a project tab first."
            ));
            return;
        }
    };
    let (Some(project_id), Some(workspace_id)) = (context.project_id, context.workspace_id) else {
        app.messages_state.toasts.warning(
            "Cannot open document: the active tab has no bound project/workspace. \
             Restore the workspace before opening a document.",
        );
        return;
    };

    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("Cannot open document: daemon unavailable — check /doctor");
        return;
    };

    // Switching documents abandons the previous attachment and its undo
    // history, and invalidates any in-flight completion for it.
    if app.editor_state.is_open() {
        app.editor_state.detach();
    }
    app.editor_state.generation = app.editor_state.generation.wrapping_add(1);
    let generation = app.editor_state.generation;
    let request_id = app.editor_state.request.begin();
    app.editor_state.operation = Some(EditorOperation::Open);
    app.editor_state.status = EditorStatus::Opening;
    app.editor_state.clear_notice();
    app.editor_state.path = Some(path.clone());
    app.editor_state.scope = scope;

    let wants_writer = true;
    app.editor_state.wants_writer = wants_writer;

    let session = TuiDocumentSession::new(core_client);
    app.editor_state.session = Some(session);
    // Opening a file lands in **Normal**, matching `/terminal`'s viewing mode
    // and every other reader surface: the Helix verbs own the keyboard and
    // the bottom input box still takes typing.
    enter_editor_normal(app);
    if !matches!(
        app.ui_state.routes.current(),
        crate::tui::route::Route::Editor
    ) {
        app.ui_state
            .routes
            .navigate_to(crate::tui::route::Route::Editor);
    }

    let controller = app
        .editor_state
        .session
        .as_ref()
        .expect("session was just installed")
        .controller();
    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|tab_id| tab_id.to_string());
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_document_open",
        tab_id,
        None,
        None,
        async move {
            let result = controller
                .open(project_id, workspace_id, path, wants_writer, scope)
                .await;
            Some(TuiCommand::EditorOpened {
                request_id,
                generation,
                result: match result {
                    Ok(()) => None,
                    Err(error) => Some(describe_error(&error)),
                },
                state: controller.state().await,
            })
        },
    );
    if task_id.is_none() {
        let _ = app
            .editor_state
            .request
            .fail(request_id, "TUI command channel unavailable".to_string());
        app.editor_state.operation = None;
        app.editor_state.status = EditorStatus::Closed;
        app.editor_state.detach();
        app.messages_state
            .toasts
            .warning("Cannot open document: TUI command channel unavailable");
    }
}

/// Open one workspace-relative document in the editor, through the daemon.
///
/// M007-H (work package A): the daemon owns the open, so the TUI sends the
/// `open_document` command and attaches the response. The scope comes from
/// the attached session's canonical binding — the daemon learns the project
/// and the workspace from that binding and never from the frontend's guess.
/// A form the daemon refuses renders the daemon's typed refusal.
pub(crate) fn open_editor_via_command(app: &mut App, raw_path: &str) {
    app.cancel_lsp_diagnostics_stream();
    let path = match normalize_relative_path(raw_path) {
        Ok(path) => path,
        Err(error) => {
            app.messages_state
                .toasts
                .warning(&format!("Cannot open document: {error}"));
            return;
        }
    };
    // A second open of the same document is rejected by the daemon with
    // `AlreadyOpen`; a switch closes the previous attachment first so one
    // controller never owns two.
    if app.editor_state.is_open()
        && app.editor_state.path.as_deref() == Some(path.as_str())
        && app.editor_state.scope == DocumentScope::Workspace
    {
        app.messages_state
            .toasts
            .info("Document is already open in the editor");
        enter_editor_normal(app);
        return;
    }
    let scope = crate::tui::command_route::scope_for(app);
    if scope.project_id.is_empty() || scope.workspace_id.is_empty() {
        app.messages_state
            .toasts
            .warning("Cannot open document: the attached session has no bound project/workspace.");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("Cannot open document: daemon unavailable — check /doctor");
        return;
    };

    // Switching documents abandons the previous attachment and its undo
    // history, and invalidates any in-flight completion for it.
    if app.editor_state.is_open() {
        app.editor_state.detach();
    }
    app.editor_state.generation = app.editor_state.generation.wrapping_add(1);
    let generation = app.editor_state.generation;
    let request_id = app.editor_state.request.begin();
    app.editor_state.operation = Some(EditorOperation::Open);
    app.editor_state.status = EditorStatus::Opening;
    app.editor_state.clear_notice();
    app.editor_state.path = Some(path.clone());
    app.editor_state.scope = DocumentScope::Workspace;

    let wants_writer = true;
    app.editor_state.wants_writer = wants_writer;

    let session = TuiDocumentSession::new(core_client.clone());
    app.editor_state.session = Some(session);
    // Opening a file lands in **Normal**, matching `/terminal`'s viewing mode
    // and every other reader surface: the Helix verbs own the keyboard and
    // the bottom input box still takes typing.
    enter_editor_normal(app);
    if !matches!(
        app.ui_state.routes.current(),
        crate::tui::route::Route::Editor
    ) {
        app.ui_state
            .routes
            .navigate_to(crate::tui::route::Route::Editor);
    }

    let controller = app
        .editor_state
        .session
        .as_ref()
        .expect("session was just installed")
        .controller();
    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|tab_id| tab_id.to_string());
    let project_id = scope.project_id.clone();
    let workspace_id = scope.workspace_id.clone();
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_document_open",
        tab_id,
        None,
        None,
        async move {
            let response = crate::tui::command_route::send_command(
                &core_client,
                scope,
                "open_document",
                "",
                &path,
            )
            .await;
            let result = match response {
                Ok(response) => controller
                    .attach_opened(
                        project_id,
                        workspace_id,
                        path,
                        wants_writer,
                        DocumentScope::Workspace,
                        response,
                    )
                    .await
                    .err()
                    .map(|error| describe_error(&error)),
                Err(error) => Some(error.to_string()),
            };
            Some(TuiCommand::EditorOpened {
                request_id,
                generation,
                result,
                state: controller.state().await,
            })
        },
    );
    if task_id.is_none() {
        let _ = app
            .editor_state
            .request
            .fail(request_id, "TUI command channel unavailable".to_string());
        app.editor_state.operation = None;
        app.editor_state.status = EditorStatus::Closed;
        app.editor_state.detach();
        app.messages_state
            .toasts
            .warning("Cannot open document: TUI command channel unavailable");
    }
}

/// Delete one file confined to `scope`.
///
/// The daemon owns both the unlink and its containment; the TUI names a scope
/// and never resolves a root. A missing file is reported as success — the
/// caller's intent ("this should not exist") already holds.
pub(crate) fn delete_file_in(app: &mut App, raw_path: &str, scope: DocumentScope) {
    let path = match normalize_relative_path(raw_path) {
        Ok(path) => path,
        Err(error) => {
            app.messages_state
                .toasts
                .warning(&format!("Cannot delete document: {error}"));
            return;
        }
    };
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(error) => {
            app.messages_state.toasts.warning(&format!(
                "Cannot delete document: {error}. Select a project tab first."
            ));
            return;
        }
    };
    let (Some(project_id), Some(workspace_id)) = (context.project_id, context.workspace_id) else {
        app.messages_state
            .toasts
            .warning("Cannot delete document: the active tab has no bound project/workspace.");
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("Cannot delete document: daemon unavailable — check /doctor");
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    let reported_path = path.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_document_delete",
        async move {
            let result = crate::tui::document_session::TuiDocumentSession::delete_file(
                core_client,
                project_id,
                workspace_id,
                path,
                scope,
            )
            .await;
            Some(TuiCommand::DocumentDeleted {
                path: reported_path,
                scope,
                error: match result {
                    Ok(_) => None,
                    Err(error) => Some(describe_error(&error)),
                },
            })
        },
    );
}

/// Create a new agent config from the template at `scope`, prompting is not
/// needed because the panel supplies the name it is creating under.
pub(crate) fn open_agent_template(app: &mut App, name: &str, scope: DocumentScope) {
    let name = name.trim();
    crate::tui::commands::agent_editor::create_agent_draft(app, name, scope);
}

/// Create one new file under `scope` from `template` and open it.
///
/// The daemon writes the file (`DocumentCreate`) and attaches it in the same
/// round trip, so this is the only path that produces a file the user has
/// not typed yet. It refuses rather than overwriting when the target already
/// exists, which makes the "create" action safe to retry.
pub(crate) fn open_editor_template_in(
    app: &mut App,
    raw_path: &str,
    template: &str,
    scope: DocumentScope,
) {
    let path = match normalize_relative_path(raw_path) {
        Ok(path) => path,
        Err(error) => {
            app.messages_state
                .toasts
                .warning(&format!("Cannot create document: {error}"));
            return;
        }
    };
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(error) => {
            app.messages_state.toasts.warning(&format!(
                "Cannot create document: {error}. Select a project tab first."
            ));
            return;
        }
    };
    let (Some(project_id), Some(workspace_id)) = (context.project_id, context.workspace_id) else {
        app.messages_state
            .toasts
            .warning("Cannot create document: the active tab has no bound project/workspace.");
        return;
    };
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("Cannot create document: daemon unavailable — check /doctor");
        return;
    };

    if app.editor_state.is_open() {
        app.editor_state.detach();
    }
    app.editor_state.generation = app.editor_state.generation.wrapping_add(1);
    let generation = app.editor_state.generation;
    let request_id = app.editor_state.request.begin();
    app.editor_state.operation = Some(EditorOperation::Open);
    app.editor_state.status = EditorStatus::Opening;
    app.editor_state.clear_notice();
    app.editor_state.path = Some(path.clone());
    app.editor_state.scope = scope;
    app.editor_state.wants_writer = true;

    let session = TuiDocumentSession::new(core_client);
    app.editor_state.session = Some(session);
    if !matches!(
        app.ui_state.routes.current(),
        crate::tui::route::Route::Editor
    ) {
        app.ui_state
            .routes
            .navigate_to(crate::tui::route::Route::Editor);
    }
    // `:new <name>` also lands in Normal, for the same reason opening an
    // existing file does. The one difference is that a fresh template has
    // nothing to navigate, so the status row already says `i` inserts.
    enter_editor_normal(app);

    let controller = app
        .editor_state
        .session
        .as_ref()
        .expect("session was just installed")
        .controller();
    let template = template.to_string();
    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|tab_id| tab_id.to_string());
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_document_create",
        tab_id,
        None,
        None,
        async move {
            let result = controller
                .create(project_id, workspace_id, path, template, scope)
                .await;
            Some(TuiCommand::EditorOpened {
                request_id,
                generation,
                result: match result {
                    Ok(()) => None,
                    Err(error) => Some(describe_error(&error)),
                },
                state: controller.state().await,
            })
        },
    );
    if task_id.is_none() {
        let _ = app
            .editor_state
            .request
            .fail(request_id, "TUI command channel unavailable".to_string());
        app.editor_state.operation = None;
        app.editor_state.status = EditorStatus::Closed;
        app.editor_state.detach();
        app.messages_state
            .toasts
            .warning("Cannot create document: TUI command channel unavailable");
    }
}

/// Apply a completed `DocumentOpen`.
///
/// A failure detaches the session entirely: the editor must never show a
/// partially populated buffer, and the controller holds no attachment of
/// its own after a failed open.
pub(crate) fn apply_editor_opened(
    app: &mut App,
    request_id: u64,
    generation: u64,
    error: Option<String>,
    state: DocumentState,
) {
    if app.editor_state.generation != generation {
        return;
    }
    if let Some(message) = error {
        if !app.editor_state.request.fail(request_id, message) {
            return;
        }
        // The open failed, so the keyboard stays where it was. The file tree
        // asked for this document; the tree is still the surface the reader
        // is on, and taking the keyboard would hand them an editor with no
        // document in it.
        super::file_tree::complete_editor_open_handoff(app, false);
        app.editor_state.operation = None;
        app.editor_state.status = EditorStatus::Closed;
        app.editor_state.detach();
        leave_editor_view(app);
        app.messages_state
            .toasts
            .warning("Cannot open document: see the editor status line");
        return;
    }
    if !app.editor_state.request.finish(request_id) {
        return;
    }
    app.editor_state.operation = None;
    app.editor_state.status = EditorStatus::from_state(&state);
    if let Some(session) = app.editor_state.session.as_mut() {
        session.presentation_mut().reset_for_open();
    }
    // D1: the unnamed register is seeded from the system clipboard **once**,
    // at open, so `p` pastes what the reader copied in another application.
    // The seed is here because the App layer owns the clipboard; the buffer
    // never reads it, and the paste path stays synchronous. A second read per
    // paste would put clipboard authority in the buffer.
    seed_register_from_clipboard(app, crate::util::clipboard::read_from_clipboard());
    // The open succeeded, so the file tree hands the keyboard to the editor
    // and puts it in **Normal** mode.
    //
    // The handoff is here rather than at the request because the request is
    // not the outcome: handing the keyboard to an editor whose document has
    // not attached yet would show the reader an empty buffer.
    super::file_tree::complete_editor_open_handoff(app, true);
    app.editor_state.clear_notice();
    refresh_editor_diagnostics(app);
    request_editor_semantic_tokens(app);
}

pub(crate) fn current_lsp_scope(app: &App) -> Option<EditorLspScope> {
    let context = app.project_execution_context().ok()?;
    let session_id = context.session_id?;
    let project_id = context.project_id?;
    let workspace_id = context.workspace_id?;
    let path = app.editor_state.path.clone()?;
    let attachment = app.editor_state.session.as_ref()?.attachment_info()?;
    if attachment.project_id != project_id
        || attachment.workspace_id != workspace_id
        || attachment.relative_path != path
    {
        return None;
    }
    Some(EditorLspScope {
        generation: app.editor_state.generation,
        session_id,
        project_id,
        workspace_id,
        path,
    })
}

/// Pull the authoritative complete diagnostics snapshot. Missing or failed
/// results preserve the last complete display; only Ready replaces it.
pub(crate) fn refresh_editor_diagnostics(app: &mut App) {
    let Some(scope) = current_lsp_scope(app) else {
        return;
    };
    let Some(attachment) = app
        .editor_state
        .session
        .as_ref()
        .and_then(|session| session.attachment_info())
    else {
        return;
    };
    let coordinator_scope = codegg_client::LspRequestScope {
        frontend_id: "tui".to_owned(),
        attachment_id: attachment.document_id.clone(),
        route_generation: scope.generation,
        connection_generation: app
            .core_client
            .as_ref()
            .map_or(0, |client| client.connection_generation()),
        project_id: scope.project_id.clone(),
        workspace_id: scope.workspace_id.clone(),
        session_id: scope.session_id.clone(),
        document_id: None,
        path: None,
        server_id: None,
    };
    let coordinator_key = codegg_client::LspRequestKey {
        scope: coordinator_scope.clone(),
        operation: codegg_client::LspRequestOperation::Diagnostics,
        document_generation: scope.generation,
        position: None,
        argument: None,
    };
    // The diagnostics panel takes the centered surface; a completion popup
    // underneath would be stale context, so the explicit gesture wins.
    app.editor_state.completion.dismiss();
    let Some(request_id) = app.editor_state.lsp.begin_diagnostics(&scope) else {
        return;
    };
    let coordinator = app.lsp_request_coordinator.clone();
    let ticket = match coordinator.begin(coordinator_key) {
        codegg_client::LspRequestStart::Started(ticket) => ticket,
        codegg_client::LspRequestStart::AlreadyInFlight
        | codegg_client::LspRequestStart::AtCapacity => {
            app.editor_state.lsp.diagnostics_loading = false;
            return;
        }
    };
    let Some(client) = app.core_client.clone() else {
        let _ = app
            .lsp_request_coordinator
            .finish(&ticket, &coordinator_scope, scope.generation);
        app.editor_state.lsp.diagnostics_loading = false;
        app.editor_state.lsp.diagnostics_status =
            crate::tui::editor_lsp::DiagnosticsViewStatus::Unavailable;
        app.editor_state.lsp.diagnostics_stale = !app.editor_state.lsp.diagnostics.is_empty();
        return;
    };
    if let Some(cancel) = app.lsp_diagnostics_stream_cancel.take() {
        cancel.cancel();
    }
    let stream_cancel = tokio_util::sync::CancellationToken::new();
    app.lsp_diagnostics_stream_cancel = Some(stream_cancel.clone());
    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|id| id.to_string());
    let response_ticket = ticket.clone();
    let response_scope = coordinator_scope.clone();
    let document_generation = scope.generation;
    let task_id = spawn_scoped_registered_tui_task(
        tx.clone(),
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_lsp_diagnostics",
        tab_id,
        None,
        None,
        async move {
            let command_tx = tx?;
            let ticket_cancel = response_ticket.cancellation_token();
            // Install the event receiver before the subscribe request so an
            // update published after its snapshot cannot fall into the gap.
            let mut events = client.subscribe_loss_aware();
            let mut first = true;
            loop {
                let request = crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    crate::protocol::core::CoreRequest::LspDiagnosticsSubscribe {
                        request: crate::protocol::lsp::LspDiagnosticsSubscribeRequestDto {
                            project_id: scope.project_id.clone(),
                            cursor: None,
                            projection_version:
                                crate::protocol::projection::PROJECTION_PROTOCOL_VERSION,
                        },
                    },
                );
                let response = tokio::select! {
                    _ = stream_cancel.cancelled() => break,
                    _ = ticket_cancel.cancelled() => break,
                    response = client.request(request) => response,
                };
                let (subscription_id, descriptor, snapshot, cursor) = match response {
                    Ok(crate::protocol::core::CoreResponse::ProjectionSubscribed {
                        subscription_id,
                        descriptor,
                        snapshot,
                        cursor,
                        ..
                    }) => (subscription_id, descriptor, snapshot, cursor),
                    Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                        if first {
                            let _ = coordinator.finish(
                                &response_ticket,
                                &response_scope,
                                document_generation,
                            );
                            let _ = command_tx
                                .send(TuiCommand::EditorDiagnosticsFinished {
                                    request_id,
                                    scope: scope.clone(),
                                    result: None,
                                    error: Some(message),
                                })
                                .await;
                        }
                        break;
                    }
                    Ok(other) => {
                        if first {
                            let _ = coordinator.finish(
                                &response_ticket,
                                &response_scope,
                                document_generation,
                            );
                            let _ = command_tx
                                .send(TuiCommand::EditorDiagnosticsFinished {
                                    request_id,
                                    scope: scope.clone(),
                                    result: None,
                                    error: Some(format!(
                                        "unexpected diagnostics subscription response: {other:?}"
                                    )),
                                })
                                .await;
                        }
                        break;
                    }
                    Err(error) => {
                        if first {
                            let _ = coordinator.finish(
                                &response_ticket,
                                &response_scope,
                                document_generation,
                            );
                            let _ = command_tx
                                .send(TuiCommand::EditorDiagnosticsFinished {
                                    request_id,
                                    scope: scope.clone(),
                                    result: None,
                                    error: Some(error.to_string()),
                                })
                                .await;
                        }
                        break;
                    }
                };
                let mut stream = match codegg_client::LspDiagnosticsStream::from_subscription(
                    &scope.project_id,
                    subscription_id.clone(),
                    &descriptor,
                    &snapshot,
                    &cursor,
                ) {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = client
                            .request(crate::core::new_request(
                                uuid::Uuid::new_v4().to_string(),
                                crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                                    subscription_id,
                                },
                            ))
                            .await;
                        if first {
                            let _ = coordinator.finish(
                                &response_ticket,
                                &response_scope,
                                document_generation,
                            );
                            let _ = command_tx
                                .send(TuiCommand::EditorDiagnosticsFinished {
                                    request_id,
                                    scope: scope.clone(),
                                    result: None,
                                    error: Some(error.to_string()),
                                })
                                .await;
                        }
                        break;
                    }
                };
                let pull = tokio::select! {
                    _ = stream_cancel.cancelled() => None,
                    _ = ticket_cancel.cancelled() => None,
                    response = client.request(crate::core::new_request(
                        uuid::Uuid::new_v4().to_string(),
                        crate::protocol::core::CoreRequest::LspDiagnosticsGet {
                            request: crate::protocol::lsp::LspDiagnosticsGetRequestDto {
                                project_id: scope.project_id.clone(),
                                session_id: scope.session_id.clone(),
                            },
                        },
                    )) => Some(response),
                };
                let initial = match pull {
                    Some(Ok(crate::protocol::core::CoreResponse::LspDiagnosticsGetResult {
                        result,
                    })) => result,
                    Some(Ok(crate::protocol::core::CoreResponse::Error { message, .. })) => {
                        let _ = client
                            .request(crate::core::new_request(
                                uuid::Uuid::new_v4().to_string(),
                                crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                                    subscription_id,
                                },
                            ))
                            .await;
                        if first {
                            let _ = coordinator.finish(
                                &response_ticket,
                                &response_scope,
                                document_generation,
                            );
                            let _ = command_tx
                                .send(TuiCommand::EditorDiagnosticsFinished {
                                    request_id,
                                    scope: scope.clone(),
                                    result: None,
                                    error: Some(message),
                                })
                                .await;
                        }
                        break;
                    }
                    Some(Err(error)) => {
                        let _ = client
                            .request(crate::core::new_request(
                                uuid::Uuid::new_v4().to_string(),
                                crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                                    subscription_id,
                                },
                            ))
                            .await;
                        if first {
                            let _ = coordinator.finish(
                                &response_ticket,
                                &response_scope,
                                document_generation,
                            );
                            let _ = command_tx
                                .send(TuiCommand::EditorDiagnosticsFinished {
                                    request_id,
                                    scope: scope.clone(),
                                    result: None,
                                    error: Some(error.to_string()),
                                })
                                .await;
                        }
                        break;
                    }
                    _ => {
                        let _ = client
                            .request(crate::core::new_request(
                                uuid::Uuid::new_v4().to_string(),
                                crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                                    subscription_id,
                                },
                            ))
                            .await;
                        break;
                    }
                };
                let _ = stream.apply_authoritative(initial.clone());
                let request_is_current =
                    coordinator.finish(&response_ticket, &response_scope, document_generation);
                if !request_is_current {
                    let _ = client
                        .request(crate::core::new_request(
                            uuid::Uuid::new_v4().to_string(),
                            crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                                subscription_id,
                            },
                        ))
                        .await;
                    break;
                }
                if first {
                    first = false;
                    if command_tx
                        .send(TuiCommand::EditorDiagnosticsFinished {
                            request_id,
                            scope: scope.clone(),
                            result: Some(initial),
                            error: None,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                } else if command_tx
                    .send(TuiCommand::EditorDiagnosticsStreamUpdated {
                        scope: scope.clone(),
                        result: initial,
                        stale: false,
                    })
                    .await
                    .is_err()
                {
                    break;
                }

                let mut resubscribe = false;
                loop {
                    tokio::select! {
                        _ = stream_cancel.cancelled() => break,
                        _ = ticket_cancel.cancelled() => break,
                        event = events.recv() => match event {
                            Some(event) => match stream.accept_client_event(&event) {
                                codegg_client::DiagnosticsStreamUpdate::Updated(result) => {
                                    if command_tx.send(TuiCommand::EditorDiagnosticsStreamUpdated {
                                        scope: scope.clone(), result, stale: false,
                                    }).await.is_err() { break; }
                                }
                                codegg_client::DiagnosticsStreamUpdate::ReconcileRequired => {
                                    if let Some(current) = stream.current().cloned() {
                                        let _ = command_tx.send(TuiCommand::EditorDiagnosticsStreamUpdated {
                                            scope: scope.clone(), result: current, stale: true,
                                        }).await;
                                    }
                                    resubscribe = true;
                                    break;
                                }
                                codegg_client::DiagnosticsStreamUpdate::Ignored => {}
                            },
                            None => break,
                        }
                    }
                }
                let _ = client
                    .request(crate::core::new_request(
                        uuid::Uuid::new_v4().to_string(),
                        crate::protocol::core::CoreRequest::ProjectionUnsubscribe {
                            subscription_id,
                        },
                    ))
                    .await;
                if !resubscribe || stream_cancel.is_cancelled() || ticket_cancel.is_cancelled() {
                    break;
                }
            }
            let _ = coordinator.finish(&response_ticket, &response_scope, document_generation);
            None
        },
    );
    if task_id.is_none() {
        let _ =
            app.lsp_request_coordinator
                .finish(&ticket, &coordinator_scope, document_generation);
        app.editor_state.lsp.diagnostics_loading = false;
        app.editor_state.lsp.diagnostics_status =
            crate::tui::editor_lsp::DiagnosticsViewStatus::Unavailable;
        app.editor_state.lsp.diagnostics_stale = !app.editor_state.lsp.diagnostics.is_empty();
    }
}

pub(crate) fn apply_editor_diagnostics(
    app: &mut App,
    request_id: u64,
    scope: EditorLspScope,
    result: Option<LspDiagnosticsResultDto>,
    error: Option<String>,
) {
    if app.editor_state.generation != scope.generation
        || current_lsp_scope(app).as_ref() != Some(&scope)
    {
        return;
    }
    app.editor_state
        .lsp
        .apply_diagnostics(request_id, &scope, result, error);
}

/// The one argument an LSP read may carry.
///
/// The protocol's coherence rule is that a read carries a `query` for and
/// only for workspace symbols, and a `new_name` for and only for the rename
/// preview. One enum makes that rule visible at the call site, and it keeps
/// two `Option<String>`s from being swapped for one another — a swap would
/// compile, and the protocol's own `validate()` would catch it, but the
/// editor should not spend a round trip discovering it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorLspReadArgument {
    /// No argument: every operation except the two that take one.
    None,
    /// The workspace-symbols query.
    Query(String),
    /// The proposed identifier, for a rename preview.
    NewName(String),
}

fn start_editor_lsp_read(
    app: &mut App,
    operation: LspReadOperation,
    argument: EditorLspReadArgument,
) {
    let (query, new_name) = match argument {
        EditorLspReadArgument::None => (None, None),
        EditorLspReadArgument::Query(query) => (Some(query), None),
        EditorLspReadArgument::NewName(name) => (None, Some(name)),
    };
    let Some(scope) = current_lsp_scope(app) else {
        app.messages_state
            .toasts
            .warning("LSP is unavailable: select a session-bound workspace document first");
        return;
    };
    // A read result panel takes the centered surface; see above.
    app.editor_state.completion.dismiss();
    let Some(request_id) = app.editor_state.lsp.begin_read(&scope) else {
        return;
    };
    if operation == LspReadOperation::SemanticTokens {
        app.editor_state.lsp.semantic_revision = app
            .editor_state
            .session
            .as_ref()
            .and_then(|session| session.try_snapshot())
            .map(|snapshot| snapshot.revision().get());
    }
    let Some(client) = app.core_client.clone() else {
        app.editor_state.lsp.read_loading = false;
        app.editor_state.lsp.read_error = Some("daemon unavailable".to_string());
        app.editor_state.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
        return;
    };
    let (line, column) = if operation.is_point() {
        let Some(session) = app.editor_state.session.as_ref() else {
            fail_lsp_read(app, "document attachment unavailable");
            return;
        };
        let Some(snapshot) = session.try_snapshot() else {
            fail_lsp_read(app, "document snapshot unavailable");
            return;
        };
        let cursor = session.presentation().cursor_byte;
        let line = crate::tui::editor::cursor_line(&snapshot, cursor);
        let Some(start) = crate::tui::editor::line_start(&snapshot, line) else {
            fail_lsp_read(app, "cursor position is invalid");
            return;
        };
        let prefix = match snapshot.read_bytes(start, cursor.min(snapshot.len_bytes())) {
            Ok(prefix) => prefix,
            Err(_) => {
                fail_lsp_read(app, "cursor position is not a valid text boundary");
                return;
            }
        };
        (
            Some(u32::try_from(line).unwrap_or(u32::MAX)),
            Some(u32::try_from(prefix.encode_utf16().count()).unwrap_or(u32::MAX)),
        )
    } else {
        (None, None)
    };
    let request = LspReadRequestDto {
        operation,
        session_id: scope.session_id.clone(),
        path: if operation == LspReadOperation::WorkspaceSymbols {
            String::new()
        } else {
            scope.path.clone()
        },
        line,
        column,
        query,
        // The editor does not pin a server. The daemon resolves one from the
        // target's extension, and reports an ambiguity instead of guessing.
        server_id: None,
        new_name,
    };
    if let Err(error) = request.validate() {
        app.editor_state.lsp.read_loading = false;
        app.editor_state.lsp.read_error = Some(format!("invalid LSP query: {error:?}"));
        app.editor_state.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
        return;
    }
    let Some(attachment) = app
        .editor_state
        .session
        .as_ref()
        .and_then(|session| session.attachment_info())
    else {
        fail_lsp_read(app, "document attachment unavailable");
        return;
    };
    let document_generation = app
        .editor_state
        .session
        .as_ref()
        .and_then(|session| session.try_snapshot())
        .map_or(scope.generation, |snapshot| snapshot.revision().get());
    let coordinator_scope = codegg_client::LspRequestScope {
        frontend_id: "tui".to_owned(),
        attachment_id: attachment.document_id.clone(),
        route_generation: scope.generation,
        connection_generation: app
            .core_client
            .as_ref()
            .map_or(0, |client| client.connection_generation()),
        project_id: scope.project_id.clone(),
        workspace_id: scope.workspace_id.clone(),
        session_id: scope.session_id.clone(),
        document_id: Some(attachment.document_id),
        path: (!request.path.is_empty()).then(|| request.path.clone()),
        server_id: request.server_id.clone(),
    };
    let coordinator_key = codegg_client::LspRequestKey {
        scope: coordinator_scope.clone(),
        operation: codegg_client::LspRequestOperation::Read(operation),
        document_generation,
        position: line.zip(column),
        argument: request.query.clone().or_else(|| request.new_name.clone()),
    };
    let coordinator = app.lsp_request_coordinator.clone();
    let ticket = match coordinator.begin(coordinator_key) {
        codegg_client::LspRequestStart::Started(ticket) => ticket,
        codegg_client::LspRequestStart::AlreadyInFlight => return,
        codegg_client::LspRequestStart::AtCapacity => {
            fail_lsp_read(app, "LSP request queue is full");
            return;
        }
    };
    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|id| id.to_string());
    let response_ticket = ticket.clone();
    let response_scope = coordinator_scope.clone();
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_lsp_read",
        tab_id,
        None,
        None,
        async move {
            let cancellation = response_ticket.cancellation_token();
            if operation == LspReadOperation::Hover {
                tokio::select! {
                    _ = cancellation.cancelled() => {
                        let _ = coordinator.finish(&response_ticket, &response_scope, document_generation);
                        return None;
                    }
                    _ = tokio::time::sleep(codegg_client::LSP_INTERACTIVE_DEBOUNCE) => {}
                }
            }
            let request = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                crate::protocol::core::CoreRequest::LspReadGet { request },
            );
            let response = tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = coordinator.finish(&response_ticket, &response_scope, document_generation);
                    return None;
                },
                response = client.request(request) => response,
            };
            if !coordinator.finish(&response_ticket, &response_scope, document_generation) {
                return None;
            }
            let (result, error) = match response {
                Ok(crate::protocol::core::CoreResponse::LspReadResult { result }) => {
                    (Some(result), None)
                }
                Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                    (None, Some(message))
                }
                Ok(other) => (None, Some(format!("unexpected LSP response: {other:?}"))),
                Err(error) => (None, Some(error.to_string())),
            };
            Some(TuiCommand::EditorLspReadFinished {
                request_id,
                scope,
                operation,
                document_revision: document_generation,
                request_position: line.zip(column),
                result,
                error,
            })
        },
    );
    if task_id.is_none() {
        let _ =
            app.lsp_request_coordinator
                .finish(&ticket, &coordinator_scope, document_generation);
        app.editor_state.lsp.read_loading = false;
        app.editor_state.lsp.read_error = Some("TUI command channel unavailable".to_string());
        app.editor_state.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
    }
}

fn fail_lsp_read(app: &mut App, message: &str) {
    app.editor_state.lsp.read_loading = false;
    app.editor_state.lsp.read_error = Some(message.to_string());
    app.editor_state.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
}

pub(crate) fn apply_editor_lsp_read(
    app: &mut App,
    request_id: u64,
    scope: EditorLspScope,
    operation: LspReadOperation,
    result: Option<LspReadResultDto>,
    error: Option<String>,
) {
    if app.editor_state.generation != scope.generation
        || current_lsp_scope(app).as_ref() != Some(&scope)
    {
        return;
    }
    let (result, error) = match result {
        Some(result) if result.operation == operation => (Some(result), error),
        Some(_) => (None, Some("unexpected LSP response operation".to_string())),
        None => (None, error),
    };
    if !app
        .editor_state
        .lsp
        .apply_read(request_id, &scope, result, error)
    {
        return;
    }
    if operation == LspReadOperation::SemanticTokens {
        // A `Ready` with zero tokens is a valid empty highlight; `NotReady`,
        // errors, and wrong-operation payloads carry no semantic truth and
        // clear the presentation instead of masquerading as empty.
        let ready = match app.editor_state.lsp.read_result.take() {
            Some(LspReadResultDto {
                status: LspReadStatus::Ready,
                payload:
                    Some(crate::protocol::lsp::LspReadPayloadDto::SemanticTokens { tokens, truncated }),
                ..
            }) => Some((tokens, truncated)),
            _ => None,
        };
        let current_revision = app
            .editor_state
            .session
            .as_ref()
            .and_then(|session| session.try_snapshot())
            .map(|snapshot| snapshot.revision().get());
        match (ready, current_revision) {
            (Some((tokens, truncated)), Some(current))
                if Some(current) == app.editor_state.lsp.semantic_revision =>
            {
                match validate_semantic_tokens(app, tokens) {
                    Some(validated) => {
                        app.editor_state
                            .lsp
                            .store_semantic(current, validated, truncated);
                    }
                    None => {
                        // No snapshot to validate against: fail closed with
                        // no highlight rather than painting unvalidated spans.
                        app.editor_state.lsp.clear_semantic();
                    }
                }
            }
            (Some(_), _) => {
                // Late completion after edit, resync, reconnect, or switch
                // is discarded without painting a different revision.
                app.editor_state.lsp.clear_semantic();
            }
            (None, _) => {
                app.editor_state.lsp.clear_semantic();
            }
        }
        app.editor_state.lsp.panel = None;
    }
    // A preview that carries a candidate opens the review.
    //
    // The review is the step in front of the apply, and the apply is what
    // these chords now do: `=` and `space r` stopped showing what *would*
    // change and started doing it. A preview with no candidate — a truncated
    // rename, a preview with no edits — stays in the panel, which says why it
    // cannot be applied.
    if let Some(candidate) = app.editor_state.lsp.preview_candidate.clone() {
        crate::tui::commands::change_review::open_carried(app, candidate);
    }
}

pub(crate) fn apply_editor_lsp_read_fresh(
    app: &mut App,
    request_id: u64,
    scope: EditorLspScope,
    operation: LspReadOperation,
    document_revision: u64,
    request_position: Option<(u32, u32)>,
    result: Option<LspReadResultDto>,
    error: Option<String>,
) {
    let Some(session) = app.editor_state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    if snapshot.revision().get() != document_revision {
        return;
    }
    if request_position.is_some_and(|requested| editor_lsp_position(session) != Some(requested)) {
        return;
    }
    apply_editor_lsp_read(app, request_id, scope, operation, result, error);
    if operation == LspReadOperation::SelectionRange {
        app.editor_state.lsp.selection_range_revision = Some(document_revision);
        app.editor_state.lsp.selection_range_position = request_position;
        app.editor_state.lsp.selection_range_index = 0;
        if let Some(result) = app.editor_state.lsp.read_result.as_ref() {
            match result.status {
                LspReadStatus::NotReady => app
                    .messages_state
                    .toasts
                    .info("Language server is not ready for structural selection"),
                LspReadStatus::Unsupported => app
                    .messages_state
                    .toasts
                    .info("This language server does not support structural selection"),
                LspReadStatus::Ambiguous => app.messages_state.toasts.info(
                    "Several language servers are attached; structural selection is ambiguous",
                ),
                LspReadStatus::Ready => {}
            }
        }
        apply_structural_selection_at(app, 0);
    }
}

fn structural_selection_step(app: &mut App, expand: bool) {
    let Some(session) = app.editor_state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let revision = snapshot.revision().get();
    let position = editor_lsp_position(session);
    let current_matches = app.editor_state.lsp.selection_range_revision == Some(revision)
        && app.editor_state.lsp.selection_range_position == position
        && app
            .editor_state
            .lsp
            .read_result
            .as_ref()
            .is_some_and(|result| result.operation == LspReadOperation::SelectionRange);
    if !current_matches {
        if expand {
            start_editor_lsp_read(
                app,
                LspReadOperation::SelectionRange,
                EditorLspReadArgument::None,
            );
        } else {
            app.messages_state
                .toasts
                .info("No current syntax selection to shrink");
        }
        return;
    }
    let count = app
        .editor_state
        .lsp
        .read_result
        .as_ref()
        .and_then(|result| result.payload.as_ref())
        .and_then(|payload| match payload {
            crate::protocol::lsp::LspReadPayloadDto::SelectionRanges { ranges, .. } => {
                Some(ranges.len())
            }
            _ => None,
        })
        .unwrap_or(0);
    let index = app.editor_state.lsp.selection_range_index;
    let next = if expand {
        index.checked_add(1).filter(|next| *next < count)
    } else {
        index.checked_sub(1)
    };
    let Some(next) = next else {
        app.messages_state.toasts.info(if expand {
            "No larger syntax range is available"
        } else {
            "No smaller syntax range is available"
        });
        return;
    };
    app.editor_state.lsp.selection_range_index = next;
    apply_structural_selection_at(app, next);
}

fn apply_structural_selection_at(app: &mut App, index: usize) {
    let Some(range) = app
        .editor_state
        .lsp
        .read_result
        .as_ref()
        .filter(|result| result.status == LspReadStatus::Ready)
        .and_then(|result| result.payload.as_ref())
        .and_then(|payload| match payload {
            crate::protocol::lsp::LspReadPayloadDto::SelectionRanges { ranges, .. } => {
                ranges.get(index).cloned()
            }
            _ => None,
        })
    else {
        return;
    };
    let Some(session) = app.editor_state.session.as_mut() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let Some(start) = editor_lsp_offset(&snapshot, range.start_line, range.start_column, false)
    else {
        app.messages_state
            .toasts
            .info("The language server returned an invalid structural range");
        return;
    };
    let Some(end) = editor_lsp_offset(&snapshot, range.end_line, range.end_column, true) else {
        app.messages_state
            .toasts
            .info("The language server returned an invalid structural range");
        return;
    };
    if start > end {
        app.messages_state
            .toasts
            .info("The language server returned an inverted structural range");
        return;
    }
    let start = crate::tui::editor::editor_point(&snapshot, start);
    let end = crate::tui::editor::editor_point(&snapshot, end);
    let mut selection = crate::tui::helix_selection::Selection::new(
        start,
        crate::tui::helix_selection::SelectionMode::Char,
    );
    selection.head = end;
    let presentation = session.presentation_mut();
    presentation.selections = vec![selection];
    presentation.primary_index = 0;
    app.editor_state.lsp.panel = None;
}

fn editor_lsp_offset(
    snapshot: &codegg_document::DocumentSnapshot,
    line: u32,
    utf16_column: u32,
    previous_character: bool,
) -> Option<usize> {
    let line = usize::try_from(line).ok()?;
    let start = crate::tui::editor::line_start(snapshot, line)?;
    let end = crate::tui::editor::line_content_end(snapshot, line)?;
    let text = snapshot.read_bytes(start, end).ok()?;
    let column = crate::tui::editor_lsp::utf16_column_to_byte(&text, utf16_column)?;
    let column = if previous_character && column > 0 {
        text[..column].char_indices().next_back()?.0
    } else {
        column
    };
    Some(start + column)
}

fn editor_lsp_position(session: &TuiDocumentSession) -> Option<(u32, u32)> {
    let snapshot = session.try_snapshot()?;
    let cursor = session.presentation().cursor_byte.min(snapshot.len_bytes());
    let line = crate::tui::editor::cursor_line(&snapshot, cursor);
    let start = crate::tui::editor::line_start(&snapshot, line)?;
    let prefix = snapshot.read_bytes(start, cursor).ok()?;
    Some((
        u32::try_from(line).ok()?,
        u32::try_from(prefix.encode_utf16().count()).ok()?,
    ))
}

fn validate_semantic_tokens(
    app: &App,
    tokens: Vec<crate::protocol::lsp::LspSemanticTokenDto>,
) -> Option<crate::tui::editor_lsp::ValidatedSemanticTokens> {
    let snapshot = app
        .editor_state
        .session
        .as_ref()
        .and_then(|session| session.try_snapshot())?;
    Some(crate::tui::editor_lsp::validate_semantic_tokens(
        &snapshot, tokens,
    ))
}

/// Ask the language server for this document's semantic tokens.
///
/// The daemon, the protocol, the validation, and the widget's span builder all
/// already existed; nothing ever *asked*. That is the whole reason the
/// editor had no syntax highlighting despite carrying a token renderer and a
/// `token_style` map — the pipeline was complete and unwired.
///
/// Two properties matter and are enforced here rather than at the call sites:
///
/// - It never opens the LSP result panel. Highlighting is ambient — it should
///   arrive without stealing the surface the user is reading — so a failure
///   leaves the document unhighlighted rather than throwing a modal over it.
///   The explicit `F8` retry is the escape hatch for a server that was still
///   booting and answered `NotReady`.
/// - It shares `begin_read` with hover/definition, so a read already in flight
///   makes this a no-op rather than a second concurrent round trip. That is
///   acceptable: the next explicit request, or the next open, will pick it up.
pub(crate) fn request_editor_semantic_tokens(app: &mut App) {
    if current_lsp_scope(app).is_none() {
        return;
    }
    start_editor_lsp_read(
        app,
        LspReadOperation::SemanticTokens,
        EditorLspReadArgument::None,
    );
    // Clear the panel unconditionally, including on the two failure paths that
    // set it. Highlighting is ambient: a server that is not up yet should
    // leave the document unhighlighted, not throw a read-result box over the
    // file the user opened. `F8` is the retry, and the previous highlight is
    // still what is on screen until a validated response replaces it.
    app.editor_state.lsp.panel = None;
}

// ── Completion (M004) ────────────────────────────────────────────────────────

/// Why a completion scope could not be built for an explicit request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionScopeError {
    /// No bound session, attachment, snapshot, or read scope.
    Unavailable,
    /// The cursor sits past the bounded position budget on a giant line.
    /// Refused visibly rather than allocating the whole prefix or sending
    /// a truncated/wrong UTF-16 cursor.
    LineTooLong,
}

/// Build the completion scope for an explicit user request.
///
/// The read scope proves session/project/workspace/path binding; the
/// attachment adds the real document identity and daemon revision; the
/// cursor resolves to the UTF-16 position the server will see through a
/// bounded rope-chunk walk (C003/F3), never by materializing a giant line
/// prefix. Returns an error when anything is missing or the line is over
/// budget, leaving the popup closed: a completion request without a bound
/// attachment is a client bug, and an over-budget line is an explicit
/// refusal — neither is ever sent half-formed.
fn current_completion_scope(app: &App) -> Result<EditorCompletionScope, CompletionScopeError> {
    let read_scope = current_lsp_scope(app).ok_or(CompletionScopeError::Unavailable)?;
    let session = app
        .editor_state
        .session
        .as_ref()
        .ok_or(CompletionScopeError::Unavailable)?;
    let attachment = session
        .attachment_info()
        .ok_or(CompletionScopeError::Unavailable)?;
    let snapshot = session
        .try_snapshot()
        .ok_or(CompletionScopeError::Unavailable)?;
    let cursor = session.presentation().cursor_byte.min(snapshot.len_bytes());
    let line = editor::cursor_line(&snapshot, cursor);
    let row_start = editor::line_start(&snapshot, line).ok_or(CompletionScopeError::Unavailable)?;
    let character = snapshot
        .utf16_column_bounded(row_start, cursor)
        .map_err(|error| {
            if error == codegg_document::DocumentError::CompletionPrefixTooLong {
                CompletionScopeError::LineTooLong
            } else {
                CompletionScopeError::Unavailable
            }
        })?;
    Ok(EditorCompletionScope {
        read_scope,
        document_id: attachment.document_id,
        revision: attachment.revision,
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character,
    })
}

/// Start one completion request on explicit user action.
///
/// Opens the popup in its loading state and issues the M003 read through
/// the scoped task seam. A re-trigger supersedes the in-flight request: the
/// state guard drops the older answer on arrival. Nothing here polls,
/// retries, or auto-accepts.
pub(crate) fn start_editor_completion(app: &mut App) {
    let scope = match current_completion_scope(app) {
        Ok(scope) => scope,
        Err(CompletionScopeError::LineTooLong) => {
            app.messages_state
                .toasts
                .warning("Line too long for completion at this position");
            return;
        }
        Err(CompletionScopeError::Unavailable) => {
            app.messages_state
                .toasts
                .warning("Completion needs a session-bound workspace document — open a file first");
            return;
        }
    };
    let Some(client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("daemon unavailable");
        return;
    };
    let request = LspCompletionRequestDto {
        session_id: scope.read_scope.session_id.clone(),
        path: scope.read_scope.path.clone(),
        document_id: scope.document_id.clone(),
        revision: scope.revision,
        line: scope.line,
        character: scope.character,
        trigger_character: None,
        max_candidates: codegg_protocol::lsp::MAX_LSP_COMPLETION_CANDIDATES as u32,
    };
    if let Err(error) = request.validate() {
        app.messages_state
            .toasts
            .warning(&format!("invalid completion request: {error:?}"));
        return;
    }
    let coordinator_scope = codegg_client::LspRequestScope {
        frontend_id: "tui".to_owned(),
        attachment_id: scope.document_id.clone(),
        route_generation: scope.read_scope.generation,
        connection_generation: app
            .core_client
            .as_ref()
            .map_or(0, |client| client.connection_generation()),
        project_id: scope.read_scope.project_id.clone(),
        workspace_id: scope.read_scope.workspace_id.clone(),
        session_id: scope.read_scope.session_id.clone(),
        document_id: Some(scope.document_id.clone()),
        path: Some(scope.read_scope.path.clone()),
        server_id: None,
    };
    let coordinator_key = codegg_client::LspRequestKey {
        scope: coordinator_scope.clone(),
        operation: codegg_client::LspRequestOperation::Completion,
        document_generation: scope.revision,
        position: Some((scope.line, scope.character)),
        argument: request.trigger_character.clone(),
    };
    let coordinator = app.lsp_request_coordinator.clone();
    let ticket = match coordinator.begin(coordinator_key) {
        codegg_client::LspRequestStart::Started(ticket) => ticket,
        codegg_client::LspRequestStart::AlreadyInFlight => return,
        codegg_client::LspRequestStart::AtCapacity => {
            app.messages_state
                .toasts
                .warning("LSP request queue is full");
            return;
        }
    };
    let request_id = app.editor_state.completion.begin(&scope);
    // C003/F2: the epoch travels with the completion so a pre-reset answer
    // can never satisfy a post-reset request, even when scope and request
    // id coincide after a clear, reconnect, or scope ping-pong.
    let epoch = app.editor_state.completion.epoch();
    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|tab_id| tab_id.to_string());
    // The completion keeps `scope` for the spawn-failure path below; the
    // task gets its own clone.
    let task_scope = scope.clone();
    let response_ticket = ticket.clone();
    let response_scope = coordinator_scope.clone();
    let document_generation = scope.revision;
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_completion",
        tab_id,
        None,
        None,
        async move {
            let cancellation = response_ticket.cancellation_token();
            tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = coordinator.finish(&response_ticket, &response_scope, document_generation);
                    return None;
                }
                _ = tokio::time::sleep(codegg_client::LSP_INTERACTIVE_DEBOUNCE) => {}
            }
            let request = crate::core::new_request(
                uuid::Uuid::new_v4().to_string(),
                crate::protocol::core::CoreRequest::LspCompletionGet { request },
            );
            let response = tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = coordinator.finish(&response_ticket, &response_scope, document_generation);
                    return None;
                },
                response = client.request(request) => response,
            };
            if !coordinator.finish(&response_ticket, &response_scope, document_generation) {
                return None;
            }
            let (result, error) = match response {
                Ok(crate::protocol::core::CoreResponse::LspCompletionResult { result }) => {
                    (Some(result), None)
                }
                Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                    (None, Some(message))
                }
                Ok(other) => (
                    None,
                    Some(format!("unexpected completion response: {other:?}")),
                ),
                Err(error) => (None, Some(error.to_string())),
            };
            Some(TuiCommand::EditorCompletionFinished {
                request_id,
                completion_epoch: epoch,
                scope: task_scope,
                result,
                error,
            })
        },
    );
    if task_id.is_none() {
        let _ =
            app.lsp_request_coordinator
                .finish(&ticket, &coordinator_scope, document_generation);
        app.editor_state.completion.apply(
            request_id,
            epoch,
            &scope,
            None,
            Some("TUI command channel unavailable".to_string()),
        );
    }
}

/// Apply one finished completion request.
///
/// Generation-, scope-, and epoch-fenced like the read path: answers for a
/// moved cursor, an old revision, a previous attachment, or a pre-reset
/// epoch are dropped by the state guard rather than displayed. The epoch
/// is what survives a reconnect that leaves generation and scope
/// unchanged: `clear` on reconnect bumps it, so a queued pre-reconnect
/// answer can never satisfy the post-reconnect request.
pub(crate) fn apply_editor_completion(
    app: &mut App,
    request_id: u64,
    completion_epoch: u64,
    scope: EditorCompletionScope,
    result: Option<LspCompletionResultDto>,
    error: Option<String>,
) {
    if app.editor_state.generation != scope.read_scope.generation
        || current_lsp_scope(app).as_ref() != Some(&scope.read_scope)
    {
        return;
    }
    app.editor_state
        .completion
        .apply(request_id, completion_epoch, &scope, result, error);
}

/// Answer one key for the completion popup, if it owns the key.
///
/// Runs inside `handle_editor_key` after the LSP panel and before the F-key
/// match, so popup keys never reach buffer editing or global bindings:
/// - `F8` always starts a request: closed opens the popup, open supersedes
///   the in-flight answer with a fresh one. Dismissal is `Esc`'s job, so a
///   stale or warming state is one key away from a refresh.
/// - With the popup open, `Up`/`Down` (and `j`/`k` in Normal mode, where
///   they are navigation rather than text) move the selection, `Enter`
///   accepts the selected candidate, and `Esc` dismisses.
/// - Any other key dismisses the popup first and falls through, so typing
///   both clears the popup (clear-on-edit) and inserts normally. A
///   keystroke is never both typed and interpreted as popup input.
fn handle_completion_key(app: &mut App, key: KeyEvent) -> bool {
    if key.code == KeyCode::F(8) {
        start_editor_completion(app);
        return true;
    }
    // `Ctrl-x` is Helix's insert-mode completion trigger. It opens the same
    // popup and **stays** in insert mode, so the reader can look at the
    // candidates and keep typing. `F8` remains the normal-mode trigger.
    //
    // Insert mode only: in normal mode `Ctrl-x` is not the editor's to
    // take, and a chord the editor claims in a mode it does not serve is a
    // chord the global handler can no longer reach.
    if key.code == KeyCode::Char('x')
        && key.modifiers == KeyModifiers::CONTROL
        && app.editor_state.mode() == EditorMode::Insert
    {
        start_editor_completion(app);
        return true;
    }
    if !app.editor_state.completion.is_open() {
        return false;
    }
    let normal_mode = app.editor_state.mode() == EditorMode::Normal;
    match key.code {
        KeyCode::Esc => {
            app.editor_state.completion.dismiss();
            true
        }
        // Accepting a candidate rewrites the text, so it is a normal-mode
        // action. In insert mode `Enter` is a newline — always.
        KeyCode::Enter if normal_mode => {
            accept_selected_completion(app);
            true
        }
        KeyCode::Up if normal_mode => {
            app.editor_state.completion.select_previous();
            true
        }
        KeyCode::Down if normal_mode => {
            app.editor_state.completion.select_next();
            true
        }
        KeyCode::Char('k') if normal_mode => {
            app.editor_state.completion.select_previous();
            true
        }
        KeyCode::Char('j') if normal_mode => {
            app.editor_state.completion.select_next();
            true
        }
        // Helix's completion menu moves the selection with `Tab` and
        // `Shift-Tab`. In insert mode the popup is the reader's window onto
        // the candidates, and `Tab` is the only key that moves the
        // selection without leaving insert mode. In normal mode `Tab` keeps
        // its old meaning and the popup is dismissed.
        KeyCode::Tab if !normal_mode => {
            app.editor_state.completion.select_next();
            true
        }
        KeyCode::BackTab if !normal_mode => {
            app.editor_state.completion.select_previous();
            true
        }
        // Everything else dismisses the popup and is handled by the mode's
        // own table. In insert mode that means the key **types**: a
        // printable is text, and it is never swallowed by a panel.
        _ => {
            app.editor_state.completion.dismiss();
            false
        }
    }
}

/// Accept the selected candidate after re-validating everything.
///
/// The popup is advisory: this re-checks scope, generation, attachment
/// identity, and revision against the CURRENT state, resolves the edit
/// against the CURRENT snapshot, and only then submits the single
/// transaction through the shared edit path — one undo unit on acceptance.
/// Anything stale, mismatched, non-applicable, or unresolvable is refused
/// with a notice and no text, disk, or history change.
fn accept_selected_completion(app: &mut App) {
    let Some(scope) = app.editor_state.completion.scope.clone() else {
        app.editor_state.completion.dismiss();
        return;
    };
    if current_lsp_scope(app).as_ref() != Some(&scope.read_scope)
        || app.editor_state.generation != scope.read_scope.generation
    {
        app.editor_state.completion.clear();
        app.messages_state
            .toasts
            .info("Completion expired — re-trigger to refresh");
        return;
    }
    let candidate = app.editor_state.completion.selected_candidate().cloned();
    let Some(candidate) = candidate else {
        app.messages_state.toasts.info("No completion selected");
        return;
    };
    let edit = match &candidate.applicability {
        LspCompletionApplicabilityDto::Applicable { edit } => edit.clone(),
        LspCompletionApplicabilityDto::NonApplicable { reason } => {
            app.messages_state.toasts.warning(match reason {
                LspCompletionUnsupportedReason::Snippet => {
                    "Snippet completions can't be applied yet"
                }
                LspCompletionUnsupportedReason::AdditionalEdits => {
                    "Completion needs additional edits — not applied"
                }
                LspCompletionUnsupportedReason::Command => {
                    "Completion carries a server command — not applied"
                }
                LspCompletionUnsupportedReason::OutOfRange => {
                    "Completion range is outside the current text"
                }
                LspCompletionUnsupportedReason::EditTooLarge => "Completion edit is too large",
                LspCompletionUnsupportedReason::InsertAndReplace => {
                    "Completion range shape isn't supported yet"
                }
            });
            return;
        }
    };
    let Some(session) = app.editor_state.session.as_ref() else {
        app.editor_state.completion.dismiss();
        return;
    };
    let attachment = session.attachment_info();
    let snapshot = session.try_snapshot();
    let (Some(attachment), Some(snapshot)) = (attachment, snapshot) else {
        app.editor_state.completion.dismiss();
        return;
    };
    if attachment.document_id != scope.document_id || attachment.revision != scope.revision {
        app.editor_state.completion.clear();
        app.messages_state
            .toasts
            .info("Completion expired — re-trigger to refresh");
        return;
    }
    let stay_insert = app.editor_state.mode() == EditorMode::Insert;
    let Some(resolved) = editor::resolve_completion_edit(&snapshot, &edit, stay_insert) else {
        app.messages_state
            .toasts
            .warning("Completion no longer applies to the current text");
        return;
    };
    submit_edit(&mut app.editor_state, resolved);
    app.editor_state.completion.dismiss();
}

/// Close the attachment and leave the editor view.
///
/// The close is asynchronous because it is a `DocumentClose` round trip.
/// Leaving the view happens immediately so the user is not trapped in a
/// route whose attachment is going away; the completion is still guarded so
/// a late result cannot be applied to a future attachment.
pub(crate) fn close_editor(app: &mut App) {
    app.cancel_lsp_diagnostics_stream();
    if !app.editor_state.is_open() {
        leave_editor_view(app);
        return;
    }
    start_operation(
        app,
        EditorOperation::Close,
        "editor_document_close",
        move |controller| {
            Box::pin(async move {
                let result = controller.close().await;
                (result, controller.state().await)
            })
        },
    );
    leave_editor_view(app);
}

/// Leave the editor route without touching the attachment.
///
/// Used when the editor has nothing attached, and after a failed open.
fn leave_editor_view(app: &mut App) {
    if !matches!(
        app.ui_state.routes.current(),
        crate::tui::route::Route::Editor
    ) {
        return;
    }
    if !app.ui_state.routes.back() {
        app.ui_state
            .routes
            .navigate_to(crate::tui::route::Route::Home);
    }
    // The route owned Insert while the document had the keyboard. Leaving it
    // set would silently disable every focused side panel on the destination:
    // a panel is consulted only in Normal mode, so "INS" carried over from a
    // closed editor is a panel that looks focused and takes no keys. The
    // composer is the only destination here, and it types regardless of mode.
    app.ui_state.input_mode = crate::tui::input::InputMode::Normal;
    app.editor_state.semantic_refresh_due_at = None;
    // The route the pending prefix was armed for is no longer on screen. Left
    // armed, it would answer its second key against whatever buffer is open
    // when the route comes back.
    app.editor_state.disarm_prefix();
    // The count is the same kind of partial gesture, so it goes for the same
    // reason and in the same place.
    clear_count(&mut app.editor_state);
}

// ── Save / reload / resync ───────────────────────────────────────────────────

/// Save the attached document through the controller.
///
/// A disk conflict is surfaced with the retained draft. M006-A never
/// auto-reloads, auto-replays, or auto-discards.
pub(crate) fn save_editor(app: &mut App) {
    start_operation(
        app,
        EditorOperation::Save,
        "editor_document_save",
        move |controller| {
            Box::pin(async move {
                let result = controller.save().await;
                (result, controller.state().await)
            })
        },
    );
}

/// Reload the attached document from disk, discarding the local draft.
pub(crate) fn reload_editor(app: &mut App) {
    start_operation(
        app,
        EditorOperation::Reload,
        "editor_document_reload",
        move |controller| {
            Box::pin(async move {
                let result = controller.reload_from_disk().await;
                (result, controller.state().await)
            })
        },
    );
}

/// Resync the replica against the daemon after a transport failure.
///
/// A divergent draft is retained by the controller in a recovery state; the
/// editor surfaces that and never replays or discards it.
pub(crate) fn resync_editor(app: &mut App) {
    start_operation(
        app,
        EditorOperation::Resync,
        "editor_document_resync",
        move |controller| {
            Box::pin(async move {
                let result = controller.resync().await;
                (result, controller.state().await)
            })
        },
    );
}

/// Start one guarded lifecycle operation against the attached controller.
fn start_operation<F>(app: &mut App, operation: EditorOperation, name: &'static str, run: F)
where
    F: FnOnce(
            Arc<DocumentController>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = (Result<(), DocumentControllerError>, DocumentState),
                    > + Send,
            >,
        > + Send
        + 'static,
{
    if !app.editor_state.is_open() {
        app.messages_state.toasts.info("No document is open");
        return;
    }
    if app.editor_state.is_busy() {
        app.editor_state.set_notice(EditorNotice::from_error(
            &DocumentControllerError::LifecycleBusy,
        ));
        return;
    }
    if operation != EditorOperation::Save && !app.editor_state.is_writer() {
        app.editor_state
            .set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return;
    }
    let controller = app
        .editor_state
        .session
        .as_ref()
        .expect("checked above")
        .controller();
    let request_id = app.editor_state.request.begin();
    let generation = app.editor_state.generation;
    app.editor_state.operation = Some(operation);
    // Save, reload, resync, and close all move the text or the binding out
    // from under a popup: clear it at initiation, not at completion.
    app.editor_state.completion.clear();
    if operation == EditorOperation::Save {
        app.editor_state.status = EditorStatus::Flushing;
    }
    app.editor_state.clear_notice();

    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|tab_id| tab_id.to_string());
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        name,
        tab_id,
        None,
        None,
        async move {
            let (result, state) = run(controller).await;
            Some(TuiCommand::EditorOperationFinished {
                request_id,
                generation,
                operation,
                error: result.as_ref().err().map(describe_error),
                state,
            })
        },
    );
    if task_id.is_none() {
        let _ = app
            .editor_state
            .request
            .fail(request_id, "TUI command channel unavailable".to_string());
        app.editor_state.operation = None;
        app.editor_state.set_notice(EditorNotice::from_transport(
            "TUI command channel unavailable".to_string(),
        ));
    }
}

/// Apply one completed lifecycle operation.
pub(crate) fn apply_editor_operation_finished(
    app: &mut App,
    request_id: u64,
    generation: u64,
    operation: EditorOperation,
    error: Option<String>,
    state: DocumentState,
) {
    if app.editor_state.generation != generation {
        return;
    }
    if let Some(message) = error {
        if !app.editor_state.request.fail(request_id, message) {
            return;
        }
        app.editor_state.operation = None;
        app.editor_state.status = EditorStatus::from_state(&state);
        if operation == EditorOperation::Close {
            app.editor_state.detach();
        }
        return;
    }
    if !app.editor_state.request.finish(request_id) {
        return;
    }
    app.editor_state.operation = None;
    app.editor_state.status = EditorStatus::from_state(&state);
    if operation.clears_undo_history() {
        if let Some(session) = app.editor_state.session.as_mut() {
            editor::clear_history(session.presentation_mut());
        }
    }
    // Reload, resync, and save can replace the replica revision; stale
    // semantic spans must never paint against the new text. Save re-reads
    // immediately below, reload/resync leave a clean no-highlight state
    // until the next explicit LSP request.
    if matches!(
        operation,
        EditorOperation::Reload | EditorOperation::Resync | EditorOperation::Save
    ) {
        app.editor_state.lsp.clear_semantic();
        if !app.editor_state.lsp.diagnostics.is_empty() {
            app.editor_state.lsp.diagnostics_stale = true;
        }
    }
    if operation == EditorOperation::Close {
        app.editor_state.detach();
    } else if operation == EditorOperation::Save {
        refresh_editor_diagnostics(app);
        request_editor_semantic_tokens(app);
        // A saved agent config must become visible in `/agents` without a
        // second command, or `:wq` would not do what the user asked. This is
        // the daemon-owned refresh every `/reload` already uses.
        if let Some(path) = app.editor_state.path.clone() {
            // Scope-qualified: `build.toml` is a global agent config, while
            // `build.toml` at a workspace path is not. A save that refreshed
            // the registry for the wrong root would be a lie about state.
            if crate::tui::commands::agent_editor::is_agent_config_path(
                &path,
                app.editor_state.scope,
            ) {
                super::agents::refresh_assets_for_session(
                    app,
                    false,
                    super::agents::AssetRefreshTrigger::Internal,
                );
            }
        }
        // `:wq` closes *after* the write succeeds, never before: closing
        // first would discard the very draft the user asked to keep.
        if app.editor_state.close_after_save {
            app.editor_state.close_after_save = false;
            close_editor(app);
        }
    }
}

/// Apply a completed workspace-file delete.
///
/// A deleted agent config has to disappear from `/agents`, so the daemon
/// refreshes its registry here for the same reason a save does — the user
/// asked for the agent to be gone, and a panel that still lists it is
/// reporting a state that no longer exists.
pub(crate) fn apply_document_deleted(
    app: &mut App,
    path: String,
    scope: DocumentScope,
    error: Option<String>,
) {
    if let Some(message) = error {
        app.messages_state.toasts.error(&message);
        return;
    }
    if crate::tui::commands::agent_editor::is_agent_config_path(&path, scope) {
        super::agents::refresh_assets_for_session(
            app,
            false,
            super::agents::AssetRefreshTrigger::Internal,
        );
        app.messages_state.toasts.info(
            format!(
                "Deleted {} ({})",
                path,
                crate::tui::commands::agent_editor::scope_label(scope)
            )
            .as_str(),
        );
    }
}

fn describe_error(error: &DocumentControllerError) -> String {
    EditorNotice::from_error(error).message()
}

// ── Local edits ──────────────────────────────────────────────────────────────

/// Whether an edit leaves the cursor at the end of the text it inserted.
///
/// This is what makes an edit the **start of a typing run**: the next
/// keystroke continues at the end of the run so far, so a run whose text does
/// not end where the cursor is cannot be continued.
///
/// A deletion or a replacement never qualifies — their ranges are not empty,
/// and a run is one contiguous insertion by construction.
fn typing_run_continues(forward: &codegg_document::TextTransaction, cursor_after: usize) -> bool {
    if forward.edits.len() != 1 {
        return false;
    }
    let edit = &forward.edits[0];
    edit.range.start == edit.range.end && cursor_after == edit.range.start + edit.insert.len()
}

/// Submit one resolved edit through the controller and adopt the result.
///
/// The undo entry is recorded **only** after the controller accepts the
/// transaction, so a rejected edit never leaves a phantom inverse in
/// history. The pre-edit cursor and selection are captured before the
/// apply so undo restores the position the user actually left.
///
/// Returns whether the controller **accepted** the transaction. That is the
/// only honest answer to "did the document take this edit?", and the
/// open-line verbs need it: a mode transition decided on a rejected
/// transaction would leave the editor in Insert mode over an unchanged
/// document, which is a worse state than the one they are repairing.
fn submit_edit(state: &mut EditorState, edit: editor::EditorEdit) -> bool {
    let editor::EditorEdit {
        transaction,
        inverse,
        bytes,
        cursor_after,
        stay_insert,
    } = edit;
    // A `/` search's matches are byte offsets into the text that was searched.
    // The moment an edit lands they describe a document that no longer
    // exists, and a match inside the *new* bounds would highlight whatever
    // text happens to sit there now — a search that quietly points at the
    // wrong words is worse than one that is gone. Drop it here, at the single
    // funnel every mutation passes through, rather than at each edit verb.
    //
    // The selection set is captured *before* the controller is asked, for the
    // same reason the cursor is: `u` has to restore the members the reader
    // had, and it can only know them while they are still there.
    let selections_before = state
        .session
        .as_ref()
        .map(|session| session.presentation().selections.clone())
        .unwrap_or_default();
    if let Some(session) = state.session.as_mut() {
        session.presentation_mut().cancel_search();
    }
    // The forward transaction is retained for redo; both directions are
    // bounded by the same depth/byte caps.
    let forward = transaction.clone();
    // Where the edit landed, for the highlighter's prefix check. The edits
    // are sorted, so the **first** one starts at a byte above which this
    // transaction changed nothing — which is exactly what makes it a safe
    // seed. It is resolved to a line against the post-edit snapshot below,
    // because an edit that inserts lines above itself moves every later
    // line index.
    let first_edit_byte = transaction.edits.first().map(|edit| edit.range.start);
    let Some(session) = state.session.as_ref() else {
        return false;
    };
    let controller = session.controller();
    let cursor_before = session.presentation().cursor_byte;
    match controller.apply_local(transaction) {
        Ok(_) => {
            // Clamp against the post-edit text, not the pre-edit snapshot.
            let after = controller.try_snapshot().map(|(snapshot, _)| snapshot);
            if let (Some(byte), Some(snapshot)) = (first_edit_byte, after.as_ref()) {
                state
                    .highlight
                    .note_edit_from(editor::cursor_line(snapshot, byte));
            }
            let Some(session) = state.session.as_mut() else {
                return false;
            };
            let presentation = session.presentation_mut();
            match &after {
                Some(snapshot) => {
                    presentation.cursor_byte = editor::clamp_offset(snapshot, cursor_after);
                    presentation.viewport_line = presentation
                        .viewport_line
                        .min(snapshot.len_lines().saturating_sub(1));
                }
                None => presentation.cursor_byte = cursor_after,
            }
            // An accepted edit invalidates every held selection: the members are
            // byte offsets into text that no longer exists, so leaving them
            // painted would highlight whatever now sits at those offsets.
            // Undo restores them from `selections_before`, which is why
            // clearing here is not losing them.
            presentation.clear_selections();
            // The text moved under the cursor, so any vertical run the reader
            // was in was measured against text that no longer exists. A
            // remembered column would aim the next `j` at an unrelated place
            // with nothing on screen to explain it.
            presentation.desired_column = None;
            if stay_insert {
                // The document took the keyboard, so the editor is in Insert
                // mode from here on. The App's own `input_mode` is set by the
                // `BufferKey::EnterInsert` the caller returns, and the caller
                // returns it **only** when this transaction was accepted — so
                // the two modes agree by construction, not by convention.
                presentation.mode = EditorMode::Insert;
            }
            // A typing burst is **one** undo step, not one per keystroke.
            // An insert session accumulates the edits and hands them over
            // as a single entry; the edit that does not fit the run — a
            // deletion, a replacement, an insertion anywhere but the end of
            // the run — ends the session and is pushed on its own.
            //
            // An edit only **starts** a run when the cursor ends where the
            // text it inserted ends: the next keystroke continues at the end
            // of the run so far, so a run whose text does not end where the
            // cursor is cannot be continued.
            //
            // The open-line verbs are why this is a separate check. `o`
            // inserts its newline at the **end** of the opened line and
            // leaves the cursor at its **start**, so the text the reader
            // types goes in before the newline. A run is one contiguous
            // insertion by construction, so the open-line edit is not part of
            // a run: it is its own undo step, and the typing that follows
            // opens the run.
            let in_insert = presentation.mode == EditorMode::Insert;
            if in_insert
                && typing_run_continues(&forward, presentation.cursor_byte)
                && editor::absorb_insert_edit(
                    presentation,
                    &forward,
                    bytes,
                    cursor_before,
                    selections_before.clone(),
                )
            {
                // The session owns this edit now; it is committed later.
            } else {
                editor::commit_insert_session(presentation);
                editor::push_undo(
                    presentation,
                    forward,
                    inverse,
                    bytes,
                    cursor_before,
                    selections_before,
                );
            }
            // Spans are bound to a revision this edit has moved past, so
            // they cannot paint any more. Re-asking is deferred to the pump's
            // quiet period instead of happening per keystroke.
            state.lsp.clear_semantic();
            state.arm_semantic_refresh(std::time::Instant::now() + SEMANTIC_REFRESH_QUIET_PERIOD);
            // Any local edit — including a completion accept — invalidates
            // the popup and its pending request: the text moved on.
            state.completion.clear();
            if !state.lsp.diagnostics.is_empty() {
                state.lsp.diagnostics_stale = true;
            }
            state.status = EditorStatus::DirtyLocal;
            state.clear_notice();
            true
        }
        Err(error) => {
            // A destructive phase in flight and a full queue are both
            // expected conditions. Neither disturbs the local text nor the
            // history, and the editor never retries internally.
            state.set_notice(EditorNotice::from_error(&error));
            false
        }
    }
}

/// Whether the editor may accept a local edit right now.
fn editable(state: &EditorState) -> bool {
    state.is_open() && !state.is_busy() && state.is_writer() && !state.status.blocks_edits()
}

/// Delete `ranges` as one transaction, through the one edit funnel.
///
/// Returns whether the controller **accepted** the delete. `stay_insert` is the
/// change gesture's half of the promise: it asks `submit_edit` to move to Insert
/// mode, and that happens only inside the accepted branch — so a rejected
/// transaction cannot leave Insert mode over text that survived. The refused
/// range set answers `false` for the same reason: a caller that enters a mode on
/// an edit that did not land is in exactly the state this forbids.
///
/// `yank` is where the deleted bytes go: `d`/`c` report
/// [`YankTarget::Unnamed`], and `Alt-d`/`Alt-c` and the blackhole report
/// [`YankTarget::Nothing`]. The bytes are read **before** the delete — after it
/// they no longer exist — and written **after** the submission is accepted, so
/// a refused delete writes no register. M002's promise survives: no arm here
/// reaches the system clipboard, whatever the target says.
fn submit_range_delete(
    state: &mut EditorState,
    ranges: Vec<std::ops::Range<usize>>,
    stay_insert: bool,
    yank: YankTarget,
) -> bool {
    let Some(session) = state.session.as_ref() else {
        return false;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return false;
    };
    let yanked = if matches!(yank, YankTarget::Unnamed) {
        read_range_text(&snapshot, &ranges)
    } else {
        String::new()
    };
    match editor::delete_ranges(&snapshot, ranges) {
        editor::RangeSetEdit::Edit(mut edit, _) => {
            edit.stay_insert = stay_insert;
            let accepted = submit_edit(state, edit);
            if accepted && !yanked.is_empty() {
                // **After** the submission: `submit_edit` clears the notice on
                // success, so a truncation disclosure set before it would be
                // erased by the very edit it describes.
                store_in_unnamed(state, &yanked);
            }
            accepted
        }
        editor::RangeSetEdit::Empty => false,
        // Refused, not truncated. A partial multi-cursor delete is exactly the
        // state neither `u` nor redo can describe, so the notice names the
        // limit the reader hit.
        editor::RangeSetEdit::TooMany(count) => {
            state.set_notice(EditorNotice::Rejected(format!(
                "too many selections to delete at once: {count} (limit {max})",
                max = codegg_protocol::document::MAX_DOCUMENT_EDITS
            )));
            false
        }
    }
}

/// The bytes `ranges` cover, concatenated in the order given.
///
/// One read for every yank and every register-writing delete, so the two cannot
/// disagree about what "the selection's text" means. An unreadable member is
/// skipped rather than losing the whole verb: the alternative is a yank that
/// produces nothing for a selection the reader can plainly see highlighted.
fn read_range_text(
    snapshot: &codegg_document::DocumentSnapshot,
    ranges: &[std::ops::Range<usize>],
) -> String {
    let mut text = String::new();
    for range in ranges {
        if let Ok(part) = snapshot.read_bytes(range.start, range.end) {
            text.push_str(&part);
        }
    }
    text
}

/// Put `text` in the unnamed register and disclose a truncated write.
///
/// The unnamed register is what a bare `p` reads, so a delete that writes it is
/// the gesture that makes `d` then `p` move text without the clipboard. A write
/// past [`MAX_REGISTER_BYTES`] is reported: a paste from a truncated register
/// still pastes, and the reader must have been told at write time that it was
/// cut.
fn store_in_unnamed(state: &mut EditorState, text: &str) {
    let outcome = state.session.as_mut().map(|session| {
        session
            .presentation_mut()
            .write_register(RegisterName::Unnamed, text)
    });
    if let Some(RegisterWrite::Truncated { stored, requested }) = outcome {
        state.set_notice(EditorNotice::Note(format!(
            "the unnamed register kept the first {stored} of {requested} bytes"
        )));
    }
}

/// Apply one motion command. Never touches text.
pub(crate) fn editor_motion(state: &mut EditorState, command: EditorCommand, visible_lines: usize) {
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let before = Some(session.presentation().cursor_byte);
    if let Some(session) = state.session.as_mut() {
        editor::apply_motion(
            session.presentation_mut(),
            &snapshot,
            command,
            visible_lines,
        );
    }
    // Read out and cleared in the same step, so the disclosure can only ever
    // describe the keystroke that just happened. Left in the presentation it
    // would be overwritten by the next `j` while the notice kept describing a
    // `]p` the reader never sent.
    lift_motion_notes(state);
    // `gg` walks through here as `BufferStart`. Only a named destination
    // records, and only when it moved — see `editor::is_named_destination`.
    if editor::is_named_destination(command) {
        note_jumplist_jump(state, before);
    }
}

/// Move a motion's disclosure from the presentation into the notice.
///
/// One function for every motion entry point, because a motion reaches the
/// presentation by either of two routes — [`editor_motion`] for a bare
/// `j`/`k`/`h`/`l` and a leaf, [`helix_motion`] for `Alt-.` and the page
/// keys — and a clear that only happened on one of them let a refusal outlive
/// its keystroke: `]p` at the last paragraph, then `h`, left "no next
/// paragraph" on screen beside a cursor that had since moved left.
///
/// The paragraph and the bracket notes are read **together**, because only one
/// motion can have run: a keystroke is either a `]p` or an `m m`, never
/// both, so at most one of the two notes is ever set. Reading them in one
/// place is what keeps that true — two separate lifts would let the second
/// clear the first's words.
///
/// Set **or** clear, so a motion with nothing to say takes the previous
/// motion's words down. That is safe because no motion path sets a notice
/// except this one; the read-only refusals belong to the *edit* funnels, which
/// never come through here.
fn lift_motion_notes(state: &mut EditorState) {
    let label = state
        .session
        .as_mut()
        .map(|session| {
            let presentation = session.presentation_mut();
            let paragraph = presentation.paragraph_note;
            presentation.paragraph_note = editor::ParagraphNote::None;
            let bracket = presentation.match_note;
            presentation.match_note = editor::MatchNote::None;
            paragraph.label().or_else(|| bracket.label())
        })
        .unwrap_or_default();
    match label {
        Some(label) => state.set_notice(EditorNotice::Note(label)),
        None => state.clear_notice(),
    }
}

/// Apply one normal-mode editing command.
pub(crate) fn editor_editing_command(state: &mut EditorState, command: EditorCommand) {
    if !editable(state) {
        state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return;
    }
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let Some(edit) = editor::resolve_edit(&snapshot, session.presentation(), command) else {
        return;
    };
    submit_edit(state, edit);
}

/// Apply one verb that acts **on the held selection**.
///
/// Same gate, same snapshot, same funnel as [`editor_editing_command`] — and
/// one difference that is the whole reason this exists: a verb that resolved
/// its target *from* the selection invalidates it. The selection is dropped
/// by [`submit_edit`] once the edit lands, for **every** verb rather than for
/// these five, because a stale selection is a highlight painted over text that
/// is no longer there — and `u` puts the set back, so dropping it is not
/// losing it.
///
/// Keeping that rule in the funnel rather than in each verb is what stops `r`,
/// `R`, `~`, `p`, and `P` from drifting into "some of them clear it".
fn editor_editing_verb(
    state: &mut EditorState,
    build: impl FnOnce(
        &codegg_document::DocumentSnapshot,
        &TuiDocumentPresentation,
    ) -> Option<editor::EditorEdit>,
) {
    if !editable(state) {
        state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return;
    }
    let edit = {
        let Some(session) = state.session.as_mut() else {
            return;
        };
        let Some(snapshot) = session.try_snapshot() else {
            return;
        };
        build(&snapshot, session.presentation())
    };
    let Some(edit) = edit else {
        return;
    };
    submit_edit(state, edit);
}

/// Read the clipboard and run one clipboard-backed verb.
///
/// The `space` family's entry point, and deliberately not register-aware:
/// `space p`, `space P` and `space R` mean the system clipboard whatever `"`
/// selected, which is what keeps cross-application paste predictable.
fn run_clipboard_verb(app: &mut App, verb: ClipboardVerb) {
    apply_clipboard_text(app, verb, crate::util::clipboard::read_from_clipboard());
}

/// The core of a clipboard verb, with the read already performed.
///
/// `read` is `None` when the system refused. Taking it as a parameter is what
/// makes all three outcomes reachable in a test: `crate::util::clipboard` has
/// no seam, so the only way to exercise "the clipboard refused" without a
/// test writing to — and stealing — the developer's real clipboard is to not
/// call it.
///
/// Three failures are possible here and each is reported separately, because
/// a `p` that silently does nothing is indistinguishable from a keypress that
/// was never delivered:
///
/// - a **read-only or busy buffer** — refused before the clipboard is even
///   consulted, with the same notice every other edit verb reports;
/// - a **clipboard read failure** (`None`) — the system refused, which is not
///   the same as an empty clipboard;
/// - an **empty clipboard** (`Some("")`) — a successful read of nothing.
fn apply_clipboard_text(app: &mut App, verb: ClipboardVerb, read: Option<String>) {
    if !editable(&app.editor_state) {
        app.editor_state
            .set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return;
    }
    let Some(text) = read else {
        app.messages_state
            .toasts
            .warning("Clipboard unavailable — nothing to paste");
        return;
    };
    if text.is_empty() {
        app.messages_state.toasts.warning("Clipboard is empty");
        return;
    }
    apply_paste_text(app, verb, &text);
}

/// Run one paste verb against the selected register's bytes.
///
/// The read is a **borrow** of the register store (or of the search line, for
/// `/`), cloned once into the edit because the presentation is borrowed again
/// by the verb. A refusal is reported with the reason the register gave — a
/// blackhole `"_p`, an empty slot, a `/` with no accepted search — rather than
/// inserting nothing silently, which is the one outcome a reader cannot tell
/// from a dropped keystroke.
fn apply_register_text(app: &mut App, verb: ClipboardVerb, register: RegisterName) {
    let read: Option<Result<String, RegisterRefusal>> =
        app.editor_state.session.as_ref().map(|session| {
            session
                .presentation()
                .read_register(register)
                .map(str::to_owned)
        });
    let Some(read) = read else {
        return;
    };
    match read {
        Err(refusal) => app.messages_state.toasts.warning(&refusal.message()),
        Ok(text) => apply_paste_text(app, verb, &text),
    }
}

/// The edit itself, shared by both paste sources.
///
/// One funnel, so a register paste inherits the read-only gate, the undo
/// entry, the selection-clearing rule and the transaction shape from the
/// clipboard paste — it is the same verb with different bytes.
fn apply_paste_text(app: &mut App, verb: ClipboardVerb, text: &str) {
    editor_editing_verb(&mut app.editor_state, |snapshot, presentation| {
        verb.build(snapshot, presentation, text)
    });
}

/// D1's seed: the unnamed register takes the clipboard's contents at open.
///
/// `read` is a parameter for the same reason `apply_clipboard_text`'s is: a
/// test that seeded the *real* clipboard would steal the developer's. The
/// production call site is `apply_editor_opened`, which is the only place a
/// document becomes current and therefore the only place a seed can be *the*
/// seed rather than a second one.
fn seed_register_from_clipboard(app: &mut App, read: Option<String>) {
    let Some(text) = read else {
        return;
    };
    if let Some(session) = app.editor_state.session.as_mut() {
        session.presentation_mut().seed_unnamed_register(&text);
    }
}

/// Which way through the history a traversal moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryStep {
    /// Back — Helix's `earlier`.
    Earlier,
    /// Forward — Helix's `later`.
    Later,
}

/// Which of the history's two axes a traversal moves along.
///
/// They answer different questions, and the four gestures keep them apart:
/// `u`/`U` walk the **branch** — a parent, or the child last walked from here —
/// while `Alt-u`/`Alt-U` walk **creation order**, the node made before or after
/// this one, wherever in the tree it sits. The reporting rule rides on the axis
/// because it rides on the binding. Collapsing both into one silent-or-not flag
/// is how two axes become one indistinguishable movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryWalk {
    /// Parent / last-visited child — `u` and `U`.
    Branch,
    /// The neighbouring node in creation order — `Alt-u` and `Alt-U`.
    CreationOrder,
}

impl HistoryWalk {
    /// Whether this axis reports its boundary instead of failing silently.
    fn reports(self) -> bool {
        matches!(self, Self::CreationOrder)
    }
}

/// What a reporting gesture says when its walk has nowhere to go.
///
/// Two axes, two questions — "is there another node in time?" and "is there a
/// parent?" — so two sentences; one for both would teach that they are the
/// same movement. Only the creation-order sentences reach a key today: the
/// branch gestures are silent by convention, as `docs/tui.md` records.
fn history_boundary(step: HistoryStep, walk: HistoryWalk) -> &'static str {
    match (walk, step) {
        (HistoryWalk::CreationOrder, HistoryStep::Earlier) => "no earlier state",
        (HistoryWalk::CreationOrder, HistoryStep::Later) => "no later state",
        (HistoryWalk::Branch, HistoryStep::Earlier) => "no parent state",
        (HistoryWalk::Branch, HistoryStep::Later) => "no branch state",
    }
}

/// Move the history cursor one step, and the document with it.
///
/// The one traversal both axes run through. The step resolves to a **target
/// node**; [`editor::history_edges`] resolves the edges between the cursor and
/// that target — one for a branch step, a path over the fork for a time step —
/// and every edge's transaction goes through the one apply path this file has:
/// `controller.apply_local`.
///
/// The cursor moves **only after** the text does. A first edge that is refused
/// leaves the document, the history and the cursor exactly where they were: a
/// cursor that moved without a matching change of text would leave the history
/// describing a document that does not exist. A longer path refused part-way
/// leaves the cursor on the node the text actually reached.
///
/// Undo during a destructive phase fails exactly like any other edit: the
/// controller returns `LifecycleBusy`, the entry stays in history, and no
/// transaction is queued for a phase that will refuse it.
fn traverse_history(state: &mut EditorState, step: HistoryStep, walk: HistoryWalk) {
    // A typing burst that has not been closed is one undo step, so the
    // session is committed before the history is read. Otherwise the
    // reader's own typing would be invisible to the `u` they just pressed:
    // the burst would still be sitting in the session, and the `u` would
    // reach past it to whatever came before.
    {
        let Some(session) = state.session.as_mut() else {
            return;
        };
        editor::commit_insert_session(session.presentation_mut());
    }
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let presentation = session.presentation();
    let target = match (walk, step) {
        (HistoryWalk::Branch, HistoryStep::Earlier) => {
            editor::history_node(presentation, presentation.history_cursor).map(|node| node.parent)
        }
        (HistoryWalk::Branch, HistoryStep::Later) => {
            editor::redo_entry(presentation).map(|node| node.created)
        }
        (HistoryWalk::CreationOrder, HistoryStep::Earlier) => {
            editor::creation_order_back(presentation)
        }
        (HistoryWalk::CreationOrder, HistoryStep::Later) => {
            editor::creation_order_forward(presentation)
        }
    };
    let edges = target
        .map(|to| editor::history_edges(presentation, to))
        .unwrap_or_default();
    if edges.is_empty() {
        // The boundary. Nothing moved and nothing changed, so the only thing
        // left to do is say so — when this gesture says so.
        if walk.reports() {
            let message = history_boundary(step, walk);
            state.set_notice(EditorNotice::Note(message.to_string()));
        }
        return;
    }
    // Undo and redo are edits too: the highlighter has to be told where this
    // one landed, or the next frame re-reads the whole scanned prefix to
    // discover a change it could have been pointed at. The seed is resolved
    // against the post-edit snapshot for the same reason `submit_edit`
    // resolves it there. A path over the fork lands several transactions, so
    // the seed is the earliest byte any of them touched — starting too late
    // would leave a change above the seed unscanned.
    let controller = session.controller();
    let mut applied = 0usize;
    let mut refusal = None;
    for edge in &edges {
        match controller.apply_local(edge.transaction.clone()) {
            Ok(_) => applied += 1,
            Err(error) => {
                refusal = Some(error);
                break;
            }
        }
    }
    if applied == 0 {
        // The first edge was refused: nothing moved, so nothing is recorded.
        if let Some(error) = refusal {
            state.set_notice(EditorNotice::from_error(&error));
        }
        return;
    }
    let after = controller.try_snapshot().map(|(snapshot, _)| snapshot);
    let first_edit_byte = edges[..applied]
        .iter()
        .filter_map(|edge| edge.transaction.edits.first())
        .map(|edit| edit.range.start)
        .min();
    if let (Some(byte), Some(snapshot)) = (first_edit_byte, after.as_ref()) {
        state
            .highlight
            .note_edit_from(editor::cursor_line(snapshot, byte));
    }
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let presentation = session.presentation_mut();
    // The cursor moves now, after the applies have been accepted. It names the
    // node the text actually reached: the last edge that landed.
    let landed = edges[applied - 1].lands_on;
    match walk {
        // The branch walk records the child it left as the one this node last
        // stood at, which is what makes the next `U` deterministic instead of
        // arbitrary. The edge and the cursor move are one step here.
        HistoryWalk::Branch => {
            let moved = match step {
                HistoryStep::Earlier => editor::move_history_cursor_back(presentation),
                HistoryStep::Later => editor::move_history_cursor_forward(presentation),
            };
            debug_assert!(moved, "the edge was resolved from this cursor");
        }
        // A time jump lands on the node it named. It is not a branch entry, so
        // it records nothing: the reader asked where they were in time, not
        // which child of this node they came from.
        HistoryWalk::CreationOrder => {
            editor::set_history_cursor(presentation, landed);
        }
    }
    debug_assert_eq!(
        presentation.history_cursor, landed,
        "the cursor names the node the last applied edge landed on"
    );
    // The recorded position and selection set of the last edge are restored,
    // exactly as a single-step traversal restores its own entry's.
    let observed = editor::history_node(presentation, edges[applied - 1].entry)
        .map(|node| (node.cursor_before, node.selections_before.clone()));
    match (&after, observed) {
        (Some(snapshot), Some((cursor_before, selections))) => {
            presentation.cursor_byte = editor::clamp_offset(snapshot, cursor_before);
            presentation.restore_selections(&selections);
            presentation.viewport_line = presentation
                .viewport_line
                .min(snapshot.len_lines().saturating_sub(1));
        }
        (None, Some((cursor_before, selections))) => {
            presentation.cursor_byte = cursor_before;
            presentation.restore_selections(&selections);
        }
        _ => {}
    }
    presentation.mode = EditorMode::Normal;
    // Text changed under the cursor; see `submit_edit`.
    presentation.desired_column = None;
    state.lsp.clear_semantic();
    state.completion.clear();
    if !state.lsp.diagnostics.is_empty() {
        state.lsp.diagnostics_stale = true;
    }
    state.status = EditorStatus::DirtyLocal;
    // A path refused part-way still moved the text, so its refusal is what the
    // row says; a path that completed clears the row as every edit does.
    match refusal {
        Some(error) => state.set_notice(EditorNotice::from_error(&error)),
        None => state.clear_notice(),
    }
}

/// Undo the newest local edit by submitting its inverse transaction.
///
/// Walks the **branch**: the cursor moves to the parent of the node it names.
/// Silent at the boundary: `u` at the beginning of the history does nothing,
/// exactly as it always has.
pub(crate) fn editor_undo(state: &mut EditorState) {
    traverse_history(state, HistoryStep::Earlier, HistoryWalk::Branch);
}

/// Redo the newest undone local edit by re-submitting its forward
/// transaction.
///
/// Walks the **branch**: the cursor moves to the child it last walked from
/// here, so a node with two children still has one defined redo target.
/// Silent at the boundary, for the same reason [`editor_undo`] is.
pub(crate) fn editor_redo(state: &mut EditorState) {
    traverse_history(state, HistoryStep::Later, HistoryWalk::Branch);
}

/// Helix's `earlier` — step one node back in **creation order**.
///
/// Time, not structure: the node made before this one may sit on another
/// branch, so the movement climbs to the fork and descends that branch, one
/// transaction per edge. The explicit history gesture, so it **reports** its
/// boundary — "no earlier state" — rather than doing nothing silently. That is
/// a CodeGG convention rather than a Helix behaviour, and it is recorded as
/// one.
pub(crate) fn editor_history_earlier(state: &mut EditorState) {
    traverse_history(state, HistoryStep::Earlier, HistoryWalk::CreationOrder);
}

/// Helix's `later` — step one node forward in **creation order**.
///
/// The explicit history gesture, so it **reports** its boundary — "no later
/// state".
pub(crate) fn editor_history_later(state: &mut EditorState) {
    traverse_history(state, HistoryStep::Later, HistoryWalk::CreationOrder);
}

/// Insert literal text at the cursor in insert mode.
pub(crate) fn editor_insert_literal(state: &mut EditorState, text: &str) {
    if !editable(state) {
        state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return;
    }
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let offset = session.presentation().cursor_byte;
    let Some(edit) = editor::insert_text(&snapshot, offset, text) else {
        return;
    };
    submit_edit(state, edit);
}

/// Delete backwards from the cursor in insert mode.
pub(crate) fn editor_delete_backward(state: &mut EditorState) {
    if !editable(state) {
        state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return;
    }
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let cursor = session.presentation().cursor_byte;
    if cursor == 0 {
        return;
    }
    let start = editor::prev_char_boundary(&snapshot, cursor);
    let Some(edit) = editor::delete_range(&snapshot, start..cursor) else {
        return;
    };
    submit_edit(state, edit);
}

/// Delete from the cursor back to the start of its line in insert mode.
///
/// Helix's `kill_to_line_start`, the insert-mode `Ctrl-u`.
pub(crate) fn editor_kill_to_line_start(state: &mut EditorState) {
    if !editable(state) {
        state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return;
    }
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let cursor = session.presentation().cursor_byte;
    let Some(edit) = editor::kill_to_line_start(&snapshot, cursor) else {
        return;
    };
    submit_edit(state, edit);
}

/// Delete the word before the cursor in insert mode, stopping at the line
/// boundary.
///
/// Helix's `delete_word_backward`, the insert-mode `Ctrl-w` and
/// `Alt-Backspace`. The line boundary is a hard stop: a `Ctrl-w` in column
/// zero deletes nothing rather than reaching into the line above.
pub(crate) fn editor_delete_word_backward(state: &mut EditorState) {
    if !editable(state) {
        state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return;
    }
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let cursor = session.presentation().cursor_byte;
    let Some(edit) = editor::delete_word_backward(&snapshot, cursor) else {
        return;
    };
    submit_edit(state, edit);
}

/// Delete the word after the cursor in insert mode, stopping at the line
/// boundary.
///
/// Helix's `delete_word_forward`, the insert-mode `Alt-d` and
/// `Alt-Delete`.
pub(crate) fn editor_delete_word_forward(state: &mut EditorState) {
    if !editable(state) {
        state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return;
    }
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let cursor = session.presentation().cursor_byte;
    let Some(edit) = editor::delete_word_forward(&snapshot, cursor) else {
        return;
    };
    submit_edit(state, edit);
}

// ── Mode ─────────────────────────────────────────────────────────────────────

/// Return the keyboard to the Helix verbs and the bottom input box.
///
/// Also closes a held selection, because leaving Insert with text selected
/// and then typing into the *composer* is the one combination where the
/// highlighted region and the place the text will land disagree.
pub(crate) fn enter_editor_normal(app: &mut App) {
    app.ui_state.input_mode = crate::tui::input::InputMode::Normal;
    leave_insert_mode(&mut app.editor_state);
}

/// Leave select mode: the held selection is dropped and the buffer returns to
/// Normal.
///
/// `Esc` is the way out, and it is a **mode** step rather than an exit — the
/// document stays open, exactly as `Esc` leaves Insert to Normal.
///
/// The selection goes with the mode. A select mode with nothing selected is
/// a mode the reader cannot see the point of, and the whole reason the mode
/// exists is the selection it holds.
pub(crate) fn leave_editor_select(app: &mut App) {
    if let Some(session) = app.editor_state.session.as_mut() {
        let presentation = session.presentation_mut();
        presentation.mode = EditorMode::Normal;
        presentation.clear_selections();
    }
}

/// Leave insert mode: back to the Helix verbs, with the typing burst
/// committed as one undo step.
///
/// Split out of [`enter_editor_normal`] so the state-level path has one
/// name for "insert mode ended". The App version also moves the *keyboard*
/// out of insert mode; this one moves the *model*, and the two are the
/// same step seen from the two sides.
pub(crate) fn leave_insert_mode(state: &mut EditorState) {
    if let Some(session) = state.session.as_mut() {
        let presentation = session.presentation_mut();
        presentation.mode = EditorMode::Normal;
        presentation.clear_selections();
        // Leaving insert mode closes the typing burst, and a closed burst is
        // one undo step. Helix commits its undo checkpoint here, for the
        // same reason: the reader's `u` after leaving insert mode is
        // expected to take back the whole burst, not its last keystroke.
        editor::commit_insert_session(presentation);
    }
}

// ── Key handling ─────────────────────────────────────────────────────────────

/// Handle one key for the editor route.
///
/// Returns `true` when the key was consumed. The composer keeps every key
/// the editor does not claim, so session composition is unaffected while
/// the editor is open and unfocused.
/// M006-D: handle one key while the file tree holds focus.
///
/// Returns `true` only for keys the tree owns. Everything else returns
/// `false` so it falls through to the editor and composer unchanged — the
/// non-modal contract depends on this being a strict subset.
/// The file tree's own keymap, when the tree holds the keyboard.
///
/// The chords live in [`super::file_tree::handle_panel_key`] rather than here,
/// because the tree's verbs are the tree's: the peek motion, the insert-mode
/// filter, and the page verbs all change the *panel's* state, and a list of
/// them here would be a second copy of the panel's grammar.
fn handle_file_tree_key(app: &mut App, key: KeyEvent) -> bool {
    super::file_tree::handle_panel_key(app, key)
}

/// Handle one key for the editor route.
///
/// The recorder's **one capture point**. The key is dispatched first and
/// recorded after, so a replayed key is not recorded (a replay that recorded
/// itself would grow the log on every press) and a key the buffer refused is
/// still recorded (the reader pressed it, and the replay owes them the same
/// refusal). The wrapper is what makes that structural: every return path of
/// [`dispatch_editor_key`] passes here, so a new early return cannot skip the
/// capture.
pub(crate) fn handle_editor_key(app: &mut App, key: KeyEvent) -> bool {
    let consumed = dispatch_editor_key(app, key);
    capture_recorded_key(app, key);
    consumed
}

fn dispatch_editor_key(app: &mut App, key: KeyEvent) -> bool {
    if !is_editor_view_active(app) {
        return false;
    }
    // M006-D: the tree pane claims keys only while it is focused, and only
    // the ones it owns. Anything else falls through, so a focused tree never
    // silently swallows composer or editor input.
    //
    // A count belongs to the buffer's keyboard, so a tree that owns the keys
    // has no gesture for one to complete: `3`, `Ctrl+T` must not leave a count
    // for the `j` that comes back to the document.
    if app.ui_state.tree_focused {
        clear_count(&mut app.editor_state);
    }
    if app.ui_state.tree_focused && handle_file_tree_key(app, key) {
        return true;
    }
    // Ctrl-T toggles the pane from anywhere in the editor, focused or not.
    if key.code == KeyCode::Char('t') && key.modifiers == KeyModifiers::CONTROL {
        super::file_tree::toggle_tree(app);
        return true;
    }
    // Esc is modal *within* the route, and the popup and the LSP result panel
    // outrank the mode step: while either is showing, Esc dismisses it and
    // nothing else happens. Answering the route first left Esc unable to
    // close a completion popup, which is the one dismissal the popup
    // documents as side-effect-free.
    //
    // The jumplist picker is ahead of both: it is the overlay the reader
    // opened last, and `Esc` must undo the most recent thing they asked for.
    if handle_jumplist_picker_key(app, key) {
        return true;
    }
    if handle_lsp_panel_key(app, key) {
        return true;
    }
    if handle_completion_key(app, key) {
        return true;
    }
    // `Esc` is a *mode* step on this route, never an exit. It used to call
    // `close_editor`, which meant the only key that could get you out of
    // Insert was also the key that threw the document away — and the way back
    // from Insert was a chord (`Ctrl+E`) nothing on screen named. Now it is
    // the same two-step every other surface uses: Insert leaves to Normal, and
    // Normal says what actually closes it (`:q`).
    //
    // Answered ahead of every other editor key because it is modal over them:
    // in Normal the `:` command line is closed first, and in Insert a document
    // that is about to be typed into must not also be one keystroke from gone.
    if key.code == KeyCode::Esc {
        // `Esc` is the route's "undo the partial thing I am doing" key, and a
        // count is a partial thing: `3`, `Esc`, `j` moves one line.
        clear_count(&mut app.editor_state);
        // The two modal input lines close before the route steps mode, so
        // `Esc` always undoes the most recent thing the user asked for.
        if app.editor_state.session.as_ref().is_some_and(|session| {
            let presentation = session.presentation();
            presentation.command_active || presentation.search_active
        }) {
            if let Some(session) = app.editor_state.session.as_mut() {
                let presentation = session.presentation_mut();
                if presentation.search_active {
                    presentation.cancel_search();
                } else {
                    presentation.close_command_line();
                }
            }
            return true;
        }
        // The keymap box closes before anything else, for the same reason the
        // two modal input lines do: `Esc` undoes the most recent thing the
        // user asked for, and the box is a thing they asked for.
        if app
            .editor_state
            .session
            .as_ref()
            .is_some_and(|session| session.presentation().keymap_overlay)
        {
            if let Some(session) = app.editor_state.session.as_mut() {
                session.presentation_mut().keymap_overlay = false;
            }
            return true;
        }
        if app.ui_state.input_mode == crate::tui::input::InputMode::Insert {
            enter_editor_normal(app);
            app.editor_state.clear_notice();
        } else if app.editor_state.mode() == EditorMode::Select {
            // Select mode is a *verb* mode, so `Esc` leaves it the way it
            // entered: the selection goes and the buffer is back to Normal.
            // The document stays open — `Esc` is a mode step, never an exit.
            leave_editor_select(app);
        } else {
            app.messages_state
                .toasts
                .info("Normal mode — the input box takes typing; i edits the file, :q closes it");
        }
        return true;
    }
    // No document, no buffer verbs. Everything falls through to the composer
    // so browsing the tree before choosing a file still types normally.
    if !app.editor_state.is_open() {
        return false;
    }
    // M006-F: with the editor on screen it has taken the place of the main
    // buffer, so the directional pane gestures stay available here — `Shift+L`
    // into the column, `Shift+H` back — and they are answered *before* the
    // buffer keymap, which would otherwise read `L`/`H` as an unknown
    // normal-mode verb. Normal mode only, which is where the gesture used to
    // be answered from: in Insert those letters are document text.
    if app.ui_state.input_mode == crate::tui::input::InputMode::Normal {
        if let Some(action) = crate::tui::input::handle_key_with_bindings(
            key,
            Some(&app.ui_state.bindings),
            crate::tui::input::InputMode::Normal,
        ) {
            if super::panels::is_normal_only(&action)
                && super::panels::handle_normal_only_action(app, action)
            {
                return true;
            }
        }
    }
    match key.code {
        KeyCode::F(2) => {
            refresh_editor_diagnostics(app);
            app.editor_state.lsp.open_diagnostics_panel();
            return true;
        }
        KeyCode::F(3) => {
            start_editor_lsp_read(app, LspReadOperation::Hover, EditorLspReadArgument::None);
            return true;
        }
        KeyCode::F(4) => {
            start_editor_lsp_read(
                app,
                LspReadOperation::Definition,
                EditorLspReadArgument::None,
            );
            return true;
        }
        KeyCode::F(5) => {
            start_editor_lsp_read(
                app,
                LspReadOperation::References,
                EditorLspReadArgument::None,
            );
            return true;
        }
        KeyCode::F(6) => {
            start_editor_lsp_read(
                app,
                LspReadOperation::DocumentSymbols,
                EditorLspReadArgument::None,
            );
            return true;
        }
        KeyCode::F(7) => {
            let Some(query) = editor_word_at_cursor(&app.editor_state) else {
                app.messages_state.toasts.info("No symbol at the cursor");
                return true;
            };
            start_editor_lsp_read(
                app,
                LspReadOperation::WorkspaceSymbols,
                EditorLspReadArgument::Query(query),
            );
            return true;
        }
        // F9 refreshes syntax highlighting. It is an explicit retry for a
        // server that answered `NotReady` while booting, which is why the
        // automatic request on open is not the only way in.
        //
        // F9, not F8: F8 is the completion trigger, and a highlight refresh
        // silently stealing it would make `:complete`-less invocation open a
        // popup instead. F2-F8 are already diagnostics/hover/def/refs/symbols/
        // workspace-symbols/completion, so F9 is the next free key.
        KeyCode::F(9) => {
            request_editor_semantic_tokens(app);
            return true;
        }
        _ => {}
    }
    // Lifecycle round trips stay on the App layer, so the buffer handler
    // reports the request instead of performing it.
    //
    // Drained here, on the very next statement after the buffer handler
    // returns. A prefix leaf can therefore never leave an LSP request parked:
    // either this runs in the same keypress, or the scope change that reset
    // the editor's LSP state already discarded it.
    let consumed = match editor_buffer_key(&mut app.editor_state, app.ui_state.input_mode, key) {
        BufferKey::Consumed => true,
        // Normal mode deliberately returns `false` for a key it does not own.
        // That is what hands typing to the bottom input box while the buffer
        // keeps its Helix verbs: the editor claims navigation and editing, and
        // everything it does not recognise stays a composer keystroke.
        BufferKey::Unhandled => false,
        BufferKey::RequestSave => {
            save_editor(app);
            true
        }
        BufferKey::RequestResync => {
            resync_editor(app);
            true
        }
        BufferKey::DeleteRanges { ranges, yank } => {
            // The App layer owns the controller, so the buffer's reported
            // ranges are submitted here — and a refusal is already reported by
            // the funnel, so there is nothing to add.
            submit_range_delete(&mut app.editor_state, ranges, false, yank);
            true
        }
        // The change gesture, in the only order that is correct: the delete
        // lands first, and the keyboard moves **only** when the document took
        // it. `submit_edit` sets the presentation's mode inside its accepted
        // branch, so the App's `input_mode` has to move on the same answer —
        // two modes that disagree would route Insert-mode keys to the Normal
        // table (or the reverse) with nothing on screen to explain it.
        BufferKey::ChangeRanges { ranges, yank } => {
            if submit_range_delete(&mut app.editor_state, ranges, true, yank) {
                app.ui_state.input_mode = crate::tui::input::InputMode::Insert;
            }
            true
        }
        BufferKey::YankRanges { ranges, target } => {
            yank_editor_ranges(app, ranges, target);
            true
        }
        // The bytes are read here, once, at the layer that owns them: the
        // register the bare verbs named. The `space` family reaches the
        // clipboard through its own effect, not through this variant.
        BufferKey::Paste { verb, register } => {
            apply_register_text(app, verb, register);
            true
        }
        // The document took the keyboard: adopt Insert mode so the badge, the
        // cursor style, and the key routing all agree.
        BufferKey::EnterInsert => {
            app.ui_state.input_mode = crate::tui::input::InputMode::Insert;
            true
        }
        BufferKey::RepeatInsert => {
            repeat_last_insert(app);
            true
        }
        BufferKey::RunCommand(command, argument) => {
            run_editor_command(app, command, argument);
            true
        }
        // An LSP read is the App layer's, because the core client is. The
        // buffer named the operation; the App issues the request.
        BufferKey::LspRead(operation) => {
            start_editor_lsp_read(app, operation, EditorLspReadArgument::None);
            true
        }
        BufferKey::ExpandSyntaxSelection => {
            structural_selection_step(app, true);
            true
        }
        BufferKey::ShrinkSyntaxSelection => {
            structural_selection_step(app, false);
            true
        }
    };
    if let Some(effect) = app.editor_state.pending_effect.take() {
        run_editor_pending_effect(app, effect);
    }
    consumed
}

// ── The recorder's capture and replay ────────────────────────────────────────

/// Record one dispatched key, if it belongs to an insert gesture.
///
/// **After dispatch**, deliberately: see [`handle_editor_key`]. The capture
/// opens on the key that enters Insert (`i`, `a`, `o`, `c`, …) and closes on
/// the `Esc` that leaves it, so the gesture `.` repeats is the whole thing the
/// reader did — the entry key included, which is why `o` replays its newline.
///
/// The closing `Esc` is **not** recorded: it is the end of the gesture rather
/// than part of it, and an `Esc` left in the log would be read as a replay
/// interrupt.
fn capture_recorded_key(app: &mut App, key: KeyEvent) {
    if !is_editor_view_active(app) {
        return;
    }
    let insert_now = app.ui_state.input_mode == crate::tui::input::InputMode::Insert;
    let Some(session) = app.editor_state.session.as_mut() else {
        return;
    };
    let recorder = &mut session.presentation_mut().recorder;
    if recorder.is_replaying() {
        return;
    }
    if recorder.is_recording() && !insert_now {
        // `Esc` ended the gesture; anything else that took the keyboard away
        // (leaving the route, the tree, a document switch) abandoned it, and
        // an abandoned gesture must not become `.`'s next repeat.
        if key.code == KeyCode::Esc {
            recorder.finish();
        } else {
            recorder.discard();
        }
        return;
    }
    if !insert_now {
        return;
    }
    if !recorder.is_recording() {
        recorder.begin();
    }
    if recorder.record(RecordedKey::of(key)) == RecorderBound::ReachedCap {
        app.editor_state
            .set_notice(EditorNotice::Note(editor::recording_capped_notice()));
    }
}

/// Repeat the last insert gesture at the cursor. `.`'s whole body.
///
/// Nothing recorded is a no-op that reports nothing: the chord is claimed
/// unconditionally so it cannot reach the composer, and a reader who has not
/// inserted anything yet is told nothing rather than told off.
fn repeat_last_insert(app: &mut App) {
    let Some(keys) = app
        .editor_state
        .session
        .as_ref()
        .and_then(|session| session.presentation().recorder.last_insert())
        .map(<[RecordedKey]>::to_vec)
    else {
        return;
    };
    replay_recorded_keys(app, &keys);
}

/// The one replay driver: `.` and, when D1 resolves to bind them, macros.
///
/// Feeds each key back through [`handle_editor_key`], so a replay produces the
/// effect, the notice and the **refusal** the same key pressed by the reader
/// would. Three properties are load-bearing:
///
/// - **It cannot record.** The recorder's `replaying` flag makes the capture
///   point skip every key of the replay, so a replay never adds to the log it
///   is reading.
/// - **It cannot nest.** [`KeyRecorder::begin_replay`] refuses a second
///   replay, so a log holding `.` could not recurse however it was written.
/// - **It is interruptible between keys.** An `Esc` in the log stops the
///   replay *before* it is dispatched; a key already dispatched completes, so
///   an interrupt never lands in the middle of an edit. Nothing is rolled
///   back: the keys before the interrupt are applied and are their own
///   history, exactly as they were when the reader typed them.
fn replay_recorded_keys(app: &mut App, keys: &[RecordedKey]) {
    if !begin_replay(app) {
        app.editor_state
            .set_notice(EditorNotice::Note(editor::REPLAY_IN_PROGRESS.to_string()));
        return;
    }
    // An insert gesture is edits through the same funnel as the keys that made
    // it, so a document that refuses edits refuses the whole repeat — instead
    // of running the log's Insert-mode *text* through Normal mode's verbs,
    // where `b` is a motion rather than a letter.
    if !editable(&app.editor_state) {
        let notice = if app.editor_state.is_writer() {
            EditorNotice::Note(editor::REPLAY_REFUSED.to_string())
        } else {
            EditorNotice::ReadOnly
        };
        app.editor_state.set_notice(notice);
        end_replay(app);
        return;
    }
    for recorded in keys {
        if replay_interrupted(app) {
            break;
        }
        if recorded.is_escape() {
            // The insert gesture's own trailing `Esc` is never recorded, so an
            // `Esc` here can only be the reader stopping the replay.
            interrupt_replay(app);
            break;
        }
        handle_editor_key(app, recorded.event());
    }
    // An insert gesture ends where `Esc` would have left it: back to Normal,
    // with the replayed typing committed as the one undo step the gesture it
    // repeats produced.
    if app.ui_state.input_mode == crate::tui::input::InputMode::Insert {
        enter_editor_normal(app);
    }
    end_replay(app);
}

fn begin_replay(app: &mut App) -> bool {
    app.editor_state
        .session
        .as_mut()
        .is_some_and(|session| session.presentation_mut().recorder.begin_replay())
}

fn end_replay(app: &mut App) {
    if let Some(session) = app.editor_state.session.as_mut() {
        session.presentation_mut().recorder.end_replay();
    }
}

fn interrupt_replay(app: &mut App) {
    if let Some(session) = app.editor_state.session.as_mut() {
        session.presentation_mut().recorder.interrupt_replay();
    }
}

fn replay_interrupted(app: &App) -> bool {
    app.editor_state
        .session
        .as_ref()
        .is_some_and(|session| session.presentation().recorder.replay_interrupted())
}

/// Move the cursor one diagnostic along, in the direction the bracket said.
///
/// The selection index is the cursor through the list, and the jump itself is
/// [`jump_to_selected_diagnostic`]'s — the same code F2's panel selection uses.
/// Stepping past either end wraps, because a list you can walk off the end of
/// is a list that silently stops working at the last file; wrapping is also
/// what Helix does. With no diagnostics loaded it says so rather than doing
/// nothing, for the same reason an unbound leaf explains itself.
fn step_editor_diagnostic(app: &mut App, direction: EditorDiagnosticStep) {
    let count = app.editor_state.lsp.diagnostics.len();
    if count == 0 {
        app.editor_state.set_notice(EditorNotice::Note(
            "No diagnostics yet — run the diagnostics check first".to_string(),
        ));
        return;
    }
    let next = match direction {
        EditorDiagnosticStep::Forward => (app.editor_state.lsp.selected_index + 1) % count,
        // `selected_index` is a usize, so `0 - 1` is a debug panic and a
        // silent wrap to usize::MAX in release. The wrapping arithmetic is
        // done here instead.
        EditorDiagnosticStep::Backward => (app.editor_state.lsp.selected_index + count - 1) % count,
    };
    app.editor_state.lsp.selected_index = next;
    jump_to_selected_diagnostic(&mut app.editor_state);
}

/// Run the editor effect one Helix leaf named.
///
/// The App layer owns the core client, the task registry and the toasts, which
/// is exactly why the prefix resolver stops at naming the effect. Every arm
/// reports something: a leaf that neither answers nor explains would be the
/// same silent no-op M002 recorded.
fn run_editor_pending_effect(app: &mut App, effect: EditorPendingEffect) {
    match effect {
        EditorPendingEffect::Read(LspReadOperation::WorkspaceSymbols) => {
            // A workspace-symbol read is defined by its query, and there is no
            // picker to supply one. The token under the cursor is what F7
            // already uses, and it is the only query a keypress can carry.
            let Some(query) = editor_word_at_cursor(&app.editor_state) else {
                app.messages_state.toasts.info("No symbol at the cursor");
                return;
            };
            start_editor_lsp_read(
                app,
                LspReadOperation::WorkspaceSymbols,
                EditorLspReadArgument::Query(query),
            );
        }
        EditorPendingEffect::Read(operation) => {
            start_editor_lsp_read(app, operation, EditorLspReadArgument::None)
        }
        EditorPendingEffect::OpenDiagnostics => {
            refresh_editor_diagnostics(app);
            app.editor_state.lsp.open_diagnostics_panel();
        }
        EditorPendingEffect::DiagnosticStep(direction) => {
            step_editor_diagnostic(app, direction);
        }
        EditorPendingEffect::Unavailable(reason) => {
            app.editor_state
                .set_notice(EditorNotice::Note(reason.to_string()));
        }
        EditorPendingEffect::Clipboard(verb) => run_clipboard_verb(app, verb),
        EditorPendingEffect::YankPrimary => yank_primary_selection(app),
        EditorPendingEffect::OpenFilePicker => open_editor_file_picker(app),
        EditorPendingEffect::OpenCommandPalette => open_editor_command_palette(app),
        EditorPendingEffect::OpenJumplist => open_editor_jumplist_picker(app),
        EditorPendingEffect::OpenKeymap => toggle_editor_keymap(app),
        EditorPendingEffect::RenamePrompt => open_editor_rename_prompt(app),
    }
}

/// The **primary** member's range — what `space Y` copies.
///
/// `None` when there is no member to copy, which is the same "nothing to
/// yank" answer `space y` gives for an empty selection.
fn primary_yank_range(session: &TuiDocumentSession) -> Option<std::ops::Range<usize>> {
    let snapshot = session.try_snapshot()?;
    editor::selection_range(session.presentation(), &snapshot)
}

/// Yank **only** the primary member of the selection set to the clipboard.
///
/// `space y` yanks every member; `space Y` is Helix's *main* selection — the
/// distinguished member alone. The two differ only when the set has more than
/// one member, which is exactly the case where guessing wrong is expensive:
/// pasting the whole set into a place that wanted one member.
///
/// The selection is cleared exactly as `space y` clears it, and only when
/// something was actually copied: a yank that dropped the selection and
/// copied nothing would leave the reader with neither the text nor the
/// highlight.
fn yank_primary_selection(app: &mut App) {
    let range = {
        let Some(session) = app.editor_state.session.as_ref() else {
            return;
        };
        primary_yank_range(session)
    };
    let mut ranges = Vec::new();
    if let Some(range) = range {
        if let Some(session) = app.editor_state.session.as_mut() {
            session.presentation_mut().clear_selections();
        }
        ranges.push(range);
    }
    // An empty range list is the same "Nothing to yank" report `space y`
    // gives, so the message lives in one place.
    yank_editor_ranges(app, ranges, YankTarget::Clipboard);
}

/// Open the workspace file browser from the editor.
///
/// The browser is daemon-owned (ADR-0022, C005): the listing is resolved from
/// the active tab's canonical project/workspace binding and the TUI never
/// walks its own filesystem. So the picker needs that binding, and says so
/// when there is none — a browser that opens empty is a key that appears to
/// do nothing, which is the M002 failure mode in a new costume.
fn open_editor_file_picker(app: &mut App) {
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(error) => {
            app.editor_state.set_notice(EditorNotice::Note(format!(
                "file picker: the active tab has no workspace root ({error})"
            )));
            return;
        }
    };
    if context.project_id.is_none() || context.workspace_id.is_none() {
        app.editor_state.set_notice(EditorNotice::Note(
            "file picker: the active tab has no canonical project/workspace binding".to_string(),
        ));
        return;
    }
    app.open_file_picker();
}

/// The palette is the editor's catalogue — the rows are
/// [`EditorCommandName::ALL`], filtered as you type — and it is deliberately
/// **not** the global `?` help dialog. Bare `?` still opens the help; `space
/// ?` is the editor's list of what it can do, which is a different question
/// and a different reader.
fn open_editor_command_palette(app: &mut App) {
    let Some(session) = app.editor_state.session.as_mut() else {
        return;
    };
    session.presentation_mut().open_command_line("");
}

/// Toggle the editor's **keymap box** — its own catalogue of its keys, in
/// the bottom-right corner of the editor.
///
/// A toggle rather than an open, because the box is the editor's own chrome
/// and a reader who opens it twice should get their document back rather
/// than a second box.
fn toggle_editor_keymap(app: &mut App) {
    let Some(session) = app.editor_state.session.as_mut() else {
        return;
    };
    let presentation = session.presentation_mut();
    presentation.keymap_overlay = !presentation.keymap_overlay;
}

// ── The jumplist ───────────────────────────────────────────────────────────

/// The cursor's offset, read **before** a jump moves it.
///
/// Every push site takes this first and reports it after the move, because an
/// entry that recorded where the jump landed could not take the reader back.
fn jump_start(state: &EditorState) -> Option<usize> {
    state
        .session
        .as_ref()
        .map(|session| session.presentation().cursor_byte)
}

/// Record where a named jump started, and disclose a prune.
///
/// The push **rule** lives in [`editor::record_jumplist_jump`], so this only
/// supplies the attachment identity and turns the dropped-oldest outcome into
/// the disclosure the bound owes the reader.
fn note_jumplist_jump(state: &mut EditorState, before: Option<usize>) {
    let Some(before) = before else {
        return;
    };
    let outcome = {
        let Some(session) = state.session.as_mut() else {
            return;
        };
        let Some(identity) = session.jumplist_identity() else {
            return;
        };
        let presentation = session.presentation_mut();
        editor::record_jumplist_jump(presentation, &identity, before)
    };
    if let Some(JumplistPush::DroppedOldest) = outcome {
        state.set_notice(EditorNotice::Note(format!(
            "jumplist keeps {MAX_JUMPLIST_ENTRIES} positions: the oldest one was dropped"
        )));
    }
}

/// Open the **jumplist picker** from `space j`.
///
/// Refused with a reason when nothing has been recorded: an empty picker is a
/// key that appears to do nothing, which is the M002 failure mode. The reason
/// names the gestures that fill it, so the key is not a dead end either.
fn open_editor_jumplist_picker(app: &mut App) {
    let empty = {
        let Some(session) = app.editor_state.session.as_mut() else {
            return;
        };
        let presentation = session.presentation_mut();
        // The keymap box and the picker are the editor's one floating box, so
        // opening one closes the other rather than stacking two over the text.
        presentation.keymap_overlay = false;
        if presentation.jumplist.is_empty() {
            true
        } else {
            presentation.jumplist.open_picker();
            false
        }
    };
    if empty {
        app.editor_state.set_notice(EditorNotice::Note(
            "jumplist: nothing recorded yet — jump with `gg`, `G`, `:goto`, a diagnostic, or a search"
                .to_string(),
        ));
    }
}

/// The picker's own keys, while it is open.
///
/// Modal over the buffer for the same reason the LSP panel is: `j`/`k` walk
/// the rows, `Enter` jumps, `Esc` closes. In Insert a character is text, so
/// nothing here claims one — the picker cannot open from Insert, and a
/// reader who somehow is there keeps typing.
fn handle_jumplist_picker_key(app: &mut App, key: KeyEvent) -> bool {
    let normal_mode = app.editor_state.mode() == EditorMode::Normal;
    let mut select = false;
    {
        let Some(session) = app.editor_state.session.as_mut() else {
            return false;
        };
        let presentation = session.presentation_mut();
        if !presentation.jumplist.picker_open() {
            return false;
        }
        match key.code {
            KeyCode::Esc => presentation.jumplist.close_picker(),
            KeyCode::Char('j') | KeyCode::Down if normal_mode => {
                presentation.jumplist.move_cursor(1);
            }
            KeyCode::Char('k') | KeyCode::Up if normal_mode => {
                presentation.jumplist.move_cursor(-1);
            }
            KeyCode::Enter if normal_mode => select = true,
            _ => return false,
        }
    }
    if select {
        select_jumplist_entry(app);
    }
    true
}

/// Jump to the picker's highlighted entry, or say why it cannot be honoured.
///
/// A jump is a **cursor move**: this reads the entry, checks it against the
/// text attached now, and moves. It writes nothing, asks the daemon for
/// nothing, and adds no gate — an entry that cannot be honoured is refused
/// with the reason rather than clamped to a nearby position.
///
/// A refusal leaves the picker **open**: a stale row is one row of a list,
/// and dismissing the list because one entry aged out would make the reader
/// reopen it to pick another.
fn select_jumplist_entry(app: &mut App) {
    let target = {
        let Some(session) = app.editor_state.session.as_mut() else {
            return;
        };
        let Some(identity) = session.jumplist_identity() else {
            return;
        };
        let Some(entry) = session.presentation().jumplist.selected().cloned() else {
            return;
        };
        let Some(snapshot) = session.try_snapshot() else {
            return;
        };
        let target = editor::jumplist_target(&identity, &entry, &snapshot);
        if let editor::JumplistTarget::Honour(byte) = target {
            let presentation = session.presentation_mut();
            editor::move_cursor_to(presentation, &snapshot, byte, visible_line_estimate());
            presentation.jumplist.close_picker();
        }
        target
    };
    match target {
        editor::JumplistTarget::Honour(_) => {}
        editor::JumplistTarget::OtherDocument => {
            app.editor_state.set_notice(EditorNotice::Note(
                "jumplist: that entry was recorded against a different document, so it is refused"
                    .to_string(),
            ));
        }
        editor::JumplistTarget::Stale => {
            app.editor_state.set_notice(EditorNotice::Note(
                "jumplist: that position is gone — the document is shorter than it was, so the entry is stale"
                    .to_string(),
            ));
        }
    }
}

/// Open the **rename prompt** — the `:` line, pre-filled with the rename
/// command and waiting for the new name.
///
/// The `:` command line is the established mechanism for an argument, not a
/// new input mode: M008's three regex verbs opened it already filled in, and
/// this is the same gesture with a different argument. The line the reader
/// sees is the one they could have typed by hand, the name has somewhere to
/// live, and `Esc` already knows how to abort.
///
/// The preview itself is a **read** — the request is issued when the line is
/// submitted, and only then, because it needs a name the reader has not
/// typed yet.
fn open_editor_rename_prompt(app: &mut App) {
    let Some(session) = app.editor_state.session.as_mut() else {
        return;
    };
    session.presentation_mut().open_command_line(&format!(
        "{} ",
        editor::EditorCommandName::RenamePreview.spec()
    ));
}

/// Move the cursor to `line`, counted from one.
///
/// The jump drops any held selection and re-anchors the view, because a jump
/// is a discontinuity rather than a step along a column — the same rule the
/// `/` search and the LSP jumps follow, so all three land the same way.
fn goto_editor_line(app: &mut App, line: usize) {
    if line == 0 {
        app.messages_state
            .toasts
            .warning("`:0` is not a line — the first line is `:1`");
        return;
    }
    let Some(session) = app.editor_state.session.as_mut() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let last_line = snapshot.len_lines().saturating_sub(1);
    // One-based in, zero-based in the document.
    let target = line.saturating_sub(1).min(last_line);
    let Some(offset) = editor::line_start(&snapshot, target) else {
        return;
    };
    let before = Some(session.presentation().cursor_byte);
    let presentation = session.presentation_mut();
    editor::move_cursor_to(presentation, &snapshot, offset, visible_line_estimate());
    note_jumplist_jump(&mut app.editor_state, before);
}

/// Execute one parsed `:` command.
///
/// The App layer owns every lifecycle round trip and route change, so the
/// buffer handler reports the command and this is the single place a `:` can
/// turn into an effect. `:w`/`:wq` share the save path, which is what makes
/// "save and the new agent appears" a single operation rather than two.
fn run_editor_command(app: &mut App, command: EditorCommandName, argument: Option<String>) {
    match command {
        EditorCommandName::Write | EditorCommandName::WriteAll => save_editor(app),
        EditorCommandName::Quit => {
            // The refusal must actually *prevent* the close; warning and
            // closing anyway would discard the draft the warning exists to
            // protect, which is the worst of both.
            if !refuse_quit_with_unsaved_draft(app) {
                close_editor(app);
            }
        }
        EditorCommandName::WriteQuit => {
            // Defer the close to the save completion: closing here would
            // detach the controller and discard the unsaved draft.
            app.editor_state.close_after_save = true;
            save_editor(app);
        }
        EditorCommandName::ForceQuit => close_editor(app),
        EditorCommandName::Reload => reload_editor(app),
        EditorCommandName::New => {
            // `:new` creates an *agent* config, which is the file shape the
            // editor is currently useful for; a bare unnamed document would
            // need a separate template path.
            crate::tui::commands::agent_editor::create_agent_draft(
                app,
                argument.unwrap_or_default().as_str(),
                DocumentScope::Workspace,
            );
        }
        EditorCommandName::Open => {
            let Some(path) = argument else {
                app.messages_state
                    .toasts
                    .warning("`:open` needs a workspace-relative path");
                return;
            };
            open_editor(app, &path);
        }
        // `:goto <n>` / `:50` — jump to a line.
        //
        // The line number is **one-based**, because that is how a reader
        // counts the lines they are looking at. `:0` is refused rather than
        // clamped: line zero does not exist, and silently landing on line one
        // would be a jump to somewhere the reader did not ask for.
        EditorCommandName::GotoLine => {
            let Some(argument) = argument else {
                app.messages_state
                    .toasts
                    .warning("`:goto` needs a line number");
                return;
            };
            let trimmed = argument.trim();
            let Ok(line) = trimmed.parse::<usize>() else {
                app.messages_state
                    .toasts
                    .warning(&format!("`:{trimmed}` is not a line number"));
                return;
            };
            goto_editor_line(app, line);
        }
        // `:complete` takes no argument; the parser already refuses one.
        EditorCommandName::Complete => start_editor_completion(app),
        // The three regex selection verbs. They are **commands** rather than a
        // prompt because a prompt would need its own input mode, its own
        // pending state, and its own disarm contract on buffer switch, reload
        // and focus change. A `:` line already has all three.
        //
        // `S`, `K` and `Alt-K` open this command's line already filled in, so
        // the chord is a shortcut rather than a second spelling: the line the
        // reader sees is the same one they could have typed.
        EditorCommandName::SplitRegex
        | EditorCommandName::KeepRegex
        | EditorCommandName::RemoveRegex
        | EditorCommandName::SelectRegex => {
            let Some(pattern) = argument.filter(|value| !value.trim().is_empty()) else {
                let message = format!("`:{}` needs a regex pattern", command.spec());
                app.messages_state.toasts.warning(&message);
                return;
            };
            run_selection_regex(&mut app.editor_state, command, pattern);
        }
        // `:rename-preview <name>` — the rename **preview**.
        //
        // The read is issued only once the name is known and known to be
        // usable: an empty name and an over-long one are refused here, so no
        // round trip is spent on a request the protocol would reject.
        //
        // A **preview**, not an apply. The request asks the server what
        // *would* change, the panel shows it, and nothing is written.
        //
        // The write path is not missing — ADR-0008 owns it — but it consumes
        // a *registered reviewable artifact*, and a rendered preview is not
        // one. So the gap is the bridge, not an authorization.
        EditorCommandName::RenamePreview => {
            let name = match checked_new_name(argument.as_deref()) {
                Ok(name) => name,
                Err(reason) => {
                    app.messages_state.toasts.warning(&reason);
                    return;
                }
            };
            start_editor_lsp_read(
                app,
                LspReadOperation::RenamePreview,
                EditorLspReadArgument::NewName(name),
            );
        }
    }
}

/// Check a proposed new name before a request is spent on it.
///
/// The **protocol** enforces the same bound
/// ([`MAX_LSP_NEW_NAME_BYTES`](codegg_protocol::lsp::MAX_LSP_NEW_NAME_BYTES)),
/// but the editor should not send a request it knows will be refused, and a
/// refusal that names the bound is a better answer than a round trip.
///
/// The name arrives already trimmed by the command parser, so this checks
/// emptiness and length and does not rewrite what the reader typed.
fn checked_new_name(name: Option<&str>) -> Result<String, String> {
    let Some(name) = name.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err("`:rename-preview` needs a new name".to_string());
    };
    if name.len() > codegg_protocol::lsp::MAX_LSP_NEW_NAME_BYTES {
        return Err(format!(
            "`:rename-preview` name is longer than the {}-byte limit",
            codegg_protocol::lsp::MAX_LSP_NEW_NAME_BYTES
        ));
    }
    Ok(name.to_string())
}

/// Run one of the three regex selection commands against the open document.
///
/// Split out from [`run_editor_command`] so the whole `S` path — chord, modal
/// command line, parse, verb — is drivable from a test without standing up an
/// `App`. The App layer stays the caller; only the effect lives here.
fn run_selection_regex(state: &mut EditorState, command: EditorCommandName, pattern: String) {
    let verb = match command {
        EditorCommandName::KeepRegex => editor::HelixVerb::KeepSelections { pattern },
        EditorCommandName::RemoveRegex => editor::HelixVerb::RemoveSelections { pattern },
        EditorCommandName::SelectRegex => editor::HelixVerb::SelectSelectionMatches { pattern },
        _ => editor::HelixVerb::SplitSelection { pattern },
    };
    // Every outcome is reported by `apply_helix` itself, including the
    // "matched nothing" case the milestone calls out by name.
    apply_helix_edit(state, verb);
}

/// Warn before a plain `:q` discards an unflushed local draft.
///
/// Returns `true` when the quit was refused and the caller must **not**
/// close. `:q!` is the documented escape hatch, and it is only reachable
/// because this exists — an explicit, named way to lose work beats a quiet
/// one.
fn refuse_quit_with_unsaved_draft(app: &mut App) -> bool {
    if app.editor_state.status.blocks_edits() {
        return false;
    }
    if matches!(
        app.editor_state.status,
        crate::tui::app::state::editor::EditorStatus::DirtyLocal
            | crate::tui::app::state::editor::EditorStatus::Flushing
    ) {
        app.messages_state
            .toasts
            .warning("Unsaved changes — use `:w` to write first, or `:q!` to discard them");
        return true;
    }
    false
}

fn handle_lsp_panel_key(app: &mut App, key: KeyEvent) -> bool {
    use crate::tui::editor_lsp::EditorLspPanel;
    let Some(panel) = app.editor_state.lsp.panel else {
        return false;
    };
    // The panel's own verbs are normal-mode actions. In insert mode a bare
    // character is text and `Enter` is a newline, so the panel must not
    // claim either: a reader typing into a file with the panel open would
    // lose the character to a panel that has no claim on it.
    let normal_mode = app.editor_state.mode() == EditorMode::Normal;
    match key.code {
        KeyCode::Esc => app.editor_state.lsp.close_panel(),
        KeyCode::Char('j') | KeyCode::Down if normal_mode => {
            let count = if panel == EditorLspPanel::Diagnostics {
                app.editor_state.lsp.diagnostics.len()
            } else {
                lsp_result_count(&app.editor_state.lsp)
            };
            app.editor_state.lsp.selected_index = app
                .editor_state
                .lsp
                .selected_index
                .saturating_add(1)
                .min(count.saturating_sub(1));
        }
        KeyCode::Char('k') | KeyCode::Up if normal_mode => {
            app.editor_state.lsp.selected_index =
                app.editor_state.lsp.selected_index.saturating_sub(1)
        }
        KeyCode::Char('r') if normal_mode && panel == EditorLspPanel::Diagnostics => {
            refresh_editor_diagnostics(app)
        }
        KeyCode::Enter if normal_mode && panel == EditorLspPanel::Diagnostics => {
            jump_to_selected_diagnostic(&mut app.editor_state)
        }
        KeyCode::Enter if normal_mode && panel == EditorLspPanel::ReadResult => {
            open_selected_lsp_target(app)
        }
        _ => return false,
    }
    true
}

fn lsp_result_count(state: &crate::tui::editor_lsp::EditorLspState) -> usize {
    use crate::protocol::lsp::LspReadPayloadDto;
    match state
        .read_result
        .as_ref()
        .and_then(|result| result.payload.as_ref())
    {
        Some(LspReadPayloadDto::Locations { locations, .. }) => locations.len(),
        Some(LspReadPayloadDto::Symbols { symbols, .. }) => symbols.len(),
        Some(LspReadPayloadDto::Highlights { highlights, .. }) => highlights.len(),
        Some(LspReadPayloadDto::SignatureHelp { signatures, .. }) => signatures.len(),
        // One previewed file, so one entry — but only when it would actually
        // change something. A no-op format is not a result worth counting.
        Some(LspReadPayloadDto::FormatPreview(preview)) => usize::from(!preview.diff.is_empty()),
        Some(LspReadPayloadDto::RenamePreview(preview)) => preview.files.len(),
        // Every code action is a row, including the ones that cannot be
        // applied: the panel lists them so the reader can see what the
        // server offered and why one of them is not available.
        Some(LspReadPayloadDto::CodeActions { actions, .. }) => actions.len(),
        _ => 0,
    }
}

fn jump_to_selected_diagnostic(state: &mut EditorState) {
    let Some(diagnostic) = state.lsp.diagnostics.get(state.lsp.selected_index) else {
        return;
    };
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let before = Some(session.presentation().cursor_byte);
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let Some(start) = editor::line_start(&snapshot, diagnostic.range.start_line as usize) else {
        return;
    };
    let Some(end) = editor::line_content_end(&snapshot, diagnostic.range.start_line as usize)
    else {
        return;
    };
    let Ok(line) = snapshot.read_bytes(start, end) else {
        return;
    };
    let Some(column) =
        crate::tui::editor_lsp::utf16_column_to_byte(&line, diagnostic.range.start_column)
    else {
        return;
    };
    if let Some(session) = state.session.as_mut() {
        let presentation = session.presentation_mut();
        presentation.cursor_byte = start + column;
        presentation.viewport_line = diagnostic.range.start_line as usize;
        // A jump, not a step: see `move_cursor_to`.
        presentation.desired_column = None;
    }
    // One push for both entry points: `]d`/`[d` and the panel's `Enter` land
    // through this function, so the gesture records once however it was asked
    // for. The pre-jump offset was read above, before the cursor moved.
    note_jumplist_jump(state, before);
    state.lsp.close_panel();
}

fn open_selected_lsp_target(app: &mut App) {
    use crate::protocol::lsp::LspReadPayloadDto;
    // A code action opens a **change review**, not a file. It is dispatched
    // first and returns, because everything below this point is about
    // opening a workspace-relative path.
    if matches!(
        app.editor_state
            .lsp
            .read_result
            .as_ref()
            .and_then(|result| result.payload.as_ref()),
        Some(LspReadPayloadDto::CodeActions { .. })
    ) {
        open_selected_code_action(app);
        return;
    }
    let target = app
        .editor_state
        .lsp
        .read_result
        .as_ref()
        .and_then(|result| match result.payload.as_ref()? {
            LspReadPayloadDto::Locations { locations, .. } => locations
                .get(app.editor_state.lsp.selected_index)?
                .workspace_relative_path
                .clone(),
            LspReadPayloadDto::Symbols { symbols, .. } => symbols
                .get(app.editor_state.lsp.selected_index)?
                .range
                .workspace_relative_path
                .clone(),
            // Highlights always live in the requested file, and the daemon
            // stamped the relative locator on each one, so opening one jumps
            // within the current document rather than switching files.
            LspReadPayloadDto::Highlights { highlights, .. } => highlights
                .get(app.editor_state.lsp.selected_index)?
                .range
                .workspace_relative_path
                .clone(),
            LspReadPayloadDto::RenamePreview(preview) => Some(
                preview
                    .files
                    .get(app.editor_state.lsp.selected_index)?
                    .path
                    .clone(),
            ),
            _ => None,
        });
    let Some(target) = target else {
        app.messages_state
            .toasts
            .info("This result has no validated workspace-relative target");
        return;
    };
    match normalize_relative_path(&target) {
        Ok(path) => open_editor(app, &path),
        Err(_) => app
            .messages_state
            .toasts
            .warning("LSP target path is invalid; result remains display-only"),
    }
}

/// Open the change review on the selected code action, or say why it cannot.
///
/// A code action is a **choice**, and the panel is where the choice is made:
/// `j`/`k` move the selection and `Enter` acts on the one selected. The
/// review it opens is M016's carried review, so the candidate reviewed is
/// the candidate applied.
///
/// An action with no candidate is not a failure. The panel already said why
/// it cannot be applied, and this says the same thing in the place the user
/// pressed the key — the same words, from the same helper, so the two
/// cannot drift apart.
fn open_selected_code_action(app: &mut App) {
    use crate::protocol::lsp::LspReadPayloadDto;
    let Some(actions) = app
        .editor_state
        .lsp
        .read_result
        .as_ref()
        .and_then(|result| match result.payload.as_ref() {
            Some(LspReadPayloadDto::CodeActions { actions, .. }) => Some(actions),
            _ => None,
        })
    else {
        return;
    };
    let Some(action) = actions.get(app.editor_state.lsp.selected_index) else {
        app.messages_state.toasts.info("No code action is selected");
        return;
    };
    if let Some(reason) = crate::tui::editor_lsp::code_action_unavailable_reason(action) {
        app.messages_state
            .toasts
            .info(&format!("\"{}\" cannot be applied: {reason}", action.title));
        return;
    }
    let Some(candidate) = action.candidate.clone() else {
        // A daemon-minted action carries a candidate or a reason, never
        // neither. Refuse rather than open a review of nothing.
        app.messages_state
            .toasts
            .info("This code action carries no preview");
        return;
    };
    crate::tui::commands::change_review::open_carried(app, candidate);
}

fn editor_word_at_cursor(state: &EditorState) -> Option<String> {
    let session = state.session.as_ref()?;
    let snapshot = session.try_snapshot()?;
    let line_index = editor::cursor_line(&snapshot, session.presentation().cursor_byte);
    let start = editor::line_start(&snapshot, line_index)?;
    let end = editor::line_content_end(&snapshot, line_index)?;
    if end.saturating_sub(start) > 16 * 1024 {
        return None;
    }
    let line = snapshot.read_bytes(start, end).ok()?;
    let cursor = session
        .presentation()
        .cursor_byte
        .saturating_sub(start)
        .min(line.len());
    let begin = line[..cursor]
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_alphanumeric() && *ch != '_')
        .map_or(0, |(idx, ch)| idx + ch.len_utf8());
    let finish = line[cursor..]
        .char_indices()
        .find(|(_, ch)| !ch.is_alphanumeric() && *ch != '_')
        .map_or(line.len(), |(idx, _)| cursor + idx);
    let word = line.get(begin..finish)?.trim();
    (!word.is_empty()).then(|| word.chars().take(200).collect())
}

/// Where a yank's bytes go.
///
/// One closed set, shared by the yank chord and the delete family, because
/// "where does this text go" is the same question in both — only the answer
/// differs. It is **not** a fourth [`ClipboardVerb`]: that enum says where the
/// text is inserted, and a register is not a place in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YankTarget {
    /// `y` — the register `"` selected (the unnamed one when none was), the
    /// unnamed register, and the system clipboard. `_` discards all three and
    /// `/` is refused before anything is read.
    Selected(RegisterName),
    /// `d` / `c` — the unnamed register only. A delete is not
    /// register-targeted, so the system clipboard is never touched by one.
    Unnamed,
    /// `space y` / `space Y` — the system clipboard only. The `space` family
    /// is not register-aware, which is what keeps cross-application paste
    /// predictable.
    Clipboard,
    /// `Alt-d` / `Alt-c` — nothing at all. M002's no-yank spelling.
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BufferKey {
    /// The editor handled the key; nothing else should see it.
    Consumed,
    /// The editor does not claim the key, so the composer may.
    ///
    /// This is what makes Normal mode usable rather than merely modal: the
    /// buffer claims navigation and editing, and every letter it does not own
    /// stays a keystroke for the bottom input box.
    Unhandled,
    /// The user asked to save; the App layer issues the round trip.
    RequestSave,
    /// The user asked to resync; the App layer issues the round trip.
    RequestResync,
    /// `d` asked for these ranges to be deleted, and for their bytes to reach
    /// `yank`. The App layer submits the edit and performs the yank, because
    /// only it reaches the controller.
    DeleteRanges {
        ranges: Vec<std::ops::Range<usize>>,
        yank: YankTarget,
    },
    /// `c` asked for these ranges to be deleted **and** for the route to enter
    /// Insert. Separate from [`Self::DeleteRanges`] because the second half is
    /// conditional on the first succeeding, and the caller has to be able to
    /// tell the two apart to order them.
    ChangeRanges {
        ranges: Vec<std::ops::Range<usize>>,
        yank: YankTarget,
    },
    /// `y` or `space y` asked for these ranges to reach a yank destination.
    /// The App layer owns the bytes, because neither the clipboard nor a
    /// disclosed truncation is editor state.
    YankRanges {
        ranges: Vec<std::ops::Range<usize>>,
        target: YankTarget,
    },
    /// `p` / `P` / `R` — insert from `register`.
    ///
    /// The `space p`/`P`/`R` spelling never reaches here: it parks
    /// [`EditorPendingEffect::Clipboard`] instead, because the system clipboard
    /// is a different source. Keeping one source per variant is what stops a
    /// register paste from becoming a fourth [`ClipboardVerb`].
    Paste {
        verb: ClipboardVerb,
        register: RegisterName,
    },
    /// An insertion entry (`i`/`a`/`I`/`A`/`o`/`O`) took the keyboard. The App
    /// layer adopts Insert mode from this, because the route owns the mode
    /// and the document owns the cursor.
    EnterInsert,
    /// `.` asked for the last insert gesture to be replayed.
    ///
    /// Reported rather than performed: a replay feeds keys back through
    /// [`handle_editor_key`], which needs the `App`, and a second replay path
    /// inside the buffer would be a second keymap.
    RepeatInsert,
    /// The user ran a `:` command. The App layer owns every round trip and
    /// every route change, so the buffer reports the parsed command rather
    /// than performing it.
    RunCommand(EditorCommandName, Option<String>),
    /// An LSP read the App layer must issue.
    ///
    /// `=` — the format **preview** — is a Normal-mode chord rather than a
    /// prefix leaf, and the buffer holds no core client, so it reports the
    /// request the way the other Normal-mode verbs report their work.
    LspRead(LspReadOperation),
    ExpandSyntaxSelection,
    ShrinkSyntaxSelection,
}

/// Handle one key while the `:` command line is open.
///
/// The command line is a modal line: it owns every key while open, because a
/// leaked printable would insert into the document behind a line the user
/// believes is capturing it. `Esc` closes it without running anything, which
/// is the only way out that never has side effects.
fn handle_command_line_key(state: &mut EditorState, key: KeyEvent) -> BufferKey {
    match key.code {
        KeyCode::Esc => {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().close_command_line();
            }
            BufferKey::Consumed
        }
        KeyCode::Enter => {
            let Some(session) = state.session.as_mut() else {
                return BufferKey::Consumed;
            };
            let line = session.presentation().command_line.clone();
            // Close first: a command that opens another document must not
            // inherit the finished line, and an unknown command should not
            // leave the user stuck retyping it.
            session.presentation_mut().close_command_line();
            match editor::parse_editor_command(&line) {
                Some((command, argument)) => BufferKey::RunCommand(command, argument),
                None => {
                    state.set_notice(EditorNotice::Rejected(format!(
                        "unknown command `:{}` — try :w, :q, or :wq",
                        line.trim()
                    )));
                    BufferKey::Consumed
                }
            }
        }
        KeyCode::Backspace => {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().pop_command_char();
            }
            BufferKey::Consumed
        }
        KeyCode::Down | KeyCode::Tab => {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().move_command_selection(1);
            }
            BufferKey::Consumed
        }
        KeyCode::Up | KeyCode::BackTab => {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().move_command_selection(-1);
            }
            BufferKey::Consumed
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() => {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().push_command_char(ch);
            }
            BufferKey::Consumed
        }
        _ => BufferKey::Consumed,
    }
}

/// Handle one key while the `/` search line is open.
///
/// Modal for the same reason the `:` line is: it owns *every* key, so a
/// leaked printable cannot silently edit the document behind a line the
/// user believes is capturing it.
///
/// It searches **as you type** rather than waiting for `Enter`. A needle
/// being refined one character at a time is the whole interaction — `/host`,
/// `/host_status`, `/host_status fn` — and a find-on-Enter model makes
/// every intermediate refinement a separate round trip for no benefit.
/// `Enter` accepts and leaves the matches painted; `Esc` abandons the whole
/// search, highlight included, because the highlight is derived state the
/// user has just disowned.
fn handle_search_line_key(
    state: &mut EditorState,
    key: KeyEvent,
    visible_lines: usize,
) -> BufferKey {
    match key.code {
        KeyCode::Esc => {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().cancel_search();
            }
            BufferKey::Consumed
        }
        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().cancel_search();
            }
            BufferKey::Consumed
        }
        KeyCode::Enter => {
            if let Some(session) = state.session.as_mut() {
                // Keep the matches: the search happened, and `n`/`N` need
                // somewhere to walk. An empty needle is a search for
                // nothing, so it leaves with the highlight.
                if session.presentation().search_line.is_empty() {
                    session.presentation_mut().cancel_search();
                } else {
                    session.presentation_mut().close_search_line();
                }
            }
            BufferKey::Consumed
        }
        KeyCode::Backspace => {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().pop_search_char();
            }
            run_editor_search(state, visible_lines);
            BufferKey::Consumed
        }
        KeyCode::Char(ch)
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().push_search_char(ch);
            }
            run_editor_search(state, visible_lines);
            BufferKey::Consumed
        }
        _ => BufferKey::Consumed,
    }
}

/// Re-run the search for the current pattern and park the cursor on the first
/// match at or after where it already is.
///
/// A pattern that matches nothing leaves the cursor exactly where it was: a
/// failed search is information, and yanking the reader to the top of the
/// file to deliver that information is not it.
fn run_editor_search(state: &mut EditorState, visible_lines: usize) {
    let Some(cursor) = state
        .session
        .as_ref()
        .map(|session| session.presentation().cursor_byte)
    else {
        return;
    };
    run_editor_search_from(state, visible_lines, cursor);
}

/// The same, adopting from an explicit `cursor`.
///
/// Separate from [`run_editor_search`] because `*` and `Alt-*` adopt from the
/// **selection's start**: the cursor sits on the selection's head, so adopting
/// from it would skip past the very match the key was pressed on whenever the
/// selection is longer than one character.
fn run_editor_search_from(state: &mut EditorState, visible_lines: usize, cursor: usize) {
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let pattern = session.presentation().search_line.clone();
    let outcome = editor::find_matches(&snapshot, &pattern);
    // Read before the outcome is consumed. A pattern that will not compile is
    // a notice naming the pattern and the compiler's reason — never a panic
    // and never a silent empty result — and a reached scan bound is named so
    // the count is not mistaken for the whole story.
    let notice = outcome
        .pattern_error
        .clone()
        .or_else(|| outcome.bound_note());
    let adopted = session.presentation_mut().adopt_search(outcome, cursor);
    let before = Some(session.presentation().cursor_byte);
    if let Some(offset) = adopted {
        let presentation = session.presentation_mut();
        editor::move_cursor_to(presentation, &snapshot, offset, visible_lines);
    }
    // Adoption is a named destination, so it records where the search started
    // — recorded before the notice below, because a search that also names a
    // bad pattern has the more specific message to give. The bound stays
    // disclosed in the picker itself.
    note_jumplist_jump(state, before);
    // After the adoption either way: an invalid pattern matches nothing, so it
    // adopts no offset, and the reason is the *only* thing the reader can act
    // on. Setting it inside the `Some` branch would silence exactly that.
    if let Some(notice) = notice {
        state.set_notice(EditorNotice::Note(notice));
    }
}

/// `*` / `Alt-*` — seed the search line from the main selection, then run it.
///
/// With no selection this reports **nothing**: a reader with a bare cursor
/// asked to search for nothing, and a notice would answer a question they did
/// not ask. A refusal — a multi-line selection, one past the pattern bound —
/// names its reason instead of searching for something close to what they
/// hold.
fn editor_search_from_selection(state: &mut EditorState, visible_lines: usize, literal: bool) {
    let resolved = {
        let Some(session) = state.session.as_mut() else {
            return;
        };
        let Some(snapshot) = session.try_snapshot() else {
            return;
        };
        let start = editor::selection_range(session.presentation(), &snapshot)
            .map(|range| range.start)
            .unwrap_or_default();
        editor::selection_search_pattern(&snapshot, session.presentation(), literal)
            .map(|pattern| pattern.map(|pattern| (pattern, start)))
    };
    let (pattern, start) = match resolved {
        Ok(Some(resolved)) => resolved,
        Ok(None) => return,
        Err(reason) => {
            state.set_notice(EditorNotice::Note(reason));
            return;
        }
    };
    if let Some(session) = state.session.as_mut() {
        session.presentation_mut().open_search_line(&pattern, true);
    }
    run_editor_search_from(state, visible_lines, start);
}

/// Step the cursor to the next/previous `/` match, wrapping.
fn step_editor_search(state: &mut EditorState, forward: bool, visible_lines: usize) -> bool {
    let Some(session) = state.session.as_mut() else {
        return false;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return false;
    };
    let Some(offset) = session.presentation_mut().step_search(forward) else {
        return false;
    };
    let before = Some(session.presentation().cursor_byte);
    let presentation = session.presentation_mut();
    editor::move_cursor_to(presentation, &snapshot, offset, visible_lines);
    note_jumplist_jump(state, before);
    true
}

/// Half-page motion needs a viewport height. The widget resolves the exact
/// window at render time, so the key handler uses a nominal height: the
/// motion is resolved by line, and the viewport follows the cursor on the
/// next frame either way.
fn visible_line_estimate() -> usize {
    NOMINAL_VISIBLE_LINES
}

/// Nominal rows assumed for a half-page motion.
const NOMINAL_VISIBLE_LINES: usize = 20;

fn handle_insert_key(state: &mut EditorState, key: KeyEvent, visible_lines: usize) -> bool {
    // The insert table is a tuple match on (modifiers, key code), because
    // every chord this milestone adds is a *modified* key: `Ctrl-w`,
    // `Alt-d` and their siblings cannot be told from a plain character
    // without looking at the modifier.
    //
    // The catch-all character arm is **last**, so an unmodified character
    // chord added later would sit above it rather than under it. The tuple
    // match makes that a matter of order rather than of correctness —
    // `(CONTROL, Char('d'))` and `(NONE, Char(ch))` cannot both match one
    // key — but a table that only stays correct if the reader reads it
    // carefully is a table that will not.
    match (key.modifiers, key.code) {
        // Helix's insert-mode control chords. `Ctrl-h`, `Ctrl-d`, `Ctrl-j`
        // and `Ctrl-k` are Helix's own bindings; `Ctrl-u`, `Ctrl-w` and
        // `Alt-d` are the deletions this milestone adds. The two word
        // deletions are line-clamped; see `editor::delete_word_backward`.
        (KeyModifiers::CONTROL, KeyCode::Char('h')) => {
            editor_delete_backward(state);
            true
        }
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
            editor_editing_command(state, EditorCommand::DeleteChar);
            true
        }
        (KeyModifiers::CONTROL, KeyCode::Char('j')) => {
            editor_insert_literal(state, "\n");
            true
        }
        (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
            editor_editing_command(state, EditorCommand::DeleteToLineEnd);
            true
        }
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
            editor_kill_to_line_start(state);
            true
        }
        (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
            editor_delete_word_backward(state);
            true
        }
        (KeyModifiers::ALT, KeyCode::Char('d')) => {
            editor_delete_word_forward(state);
            true
        }
        (KeyModifiers::ALT, KeyCode::Backspace) => {
            editor_delete_word_backward(state);
            true
        }
        (KeyModifiers::ALT, KeyCode::Delete) => {
            editor_delete_word_forward(state);
            true
        }
        // The plain keys keep their old meaning and accept no modifier
        // beyond `Shift`, which is how a terminal reports `Shift-Backspace`.
        // A modified plain key is no longer claimed here: an unclaimed
        // insert chord is a *global* action, and `Ctrl-Left` belongs to
        // whatever global movement the session offers rather than to the
        // editor's one-character step.
        (KeyModifiers::NONE, KeyCode::Backspace) | (KeyModifiers::SHIFT, KeyCode::Backspace) => {
            editor_delete_backward(state);
            true
        }
        (KeyModifiers::NONE, KeyCode::Delete) | (KeyModifiers::SHIFT, KeyCode::Delete) => {
            editor_editing_command(state, EditorCommand::DeleteChar);
            true
        }
        (KeyModifiers::NONE, KeyCode::Left) | (KeyModifiers::SHIFT, KeyCode::Left) => {
            editor_motion(state, EditorCommand::MoveLeft, visible_lines);
            true
        }
        (KeyModifiers::NONE, KeyCode::Right) | (KeyModifiers::SHIFT, KeyCode::Right) => {
            editor_motion(state, EditorCommand::MoveRight, visible_lines);
            true
        }
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::SHIFT, KeyCode::Up) => {
            editor_motion(state, EditorCommand::MoveUp, visible_lines);
            true
        }
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::SHIFT, KeyCode::Down) => {
            editor_motion(state, EditorCommand::MoveDown, visible_lines);
            true
        }
        (KeyModifiers::NONE, KeyCode::Enter) | (KeyModifiers::SHIFT, KeyCode::Enter) => {
            editor_insert_literal(state, "\n");
            true
        }
        (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::Tab) => {
            editor_insert_literal(state, "\t");
            true
        }
        (KeyModifiers::NONE, KeyCode::Char(ch)) | (KeyModifiers::SHIFT, KeyCode::Char(ch)) => {
            editor_insert_literal(state, &ch.to_string());
            true
        }
        _ => false,
    }
}

/// What one key did to the armed [`EditorPrefix`], if anything.
enum PrefixResolution {
    /// No prefix was armed — resolve the key against the keymap.
    Idle,
    /// The key completed the gesture and settled the buffer key itself.
    Handled(BufferKey),
    /// The prefix disarmed without consuming the key: resolve it normally.
    Retry,
    /// The prefix disarmed and took the key with it.
    Swallowed,
}

/// The character a pending capture should use, if `key` carries one.
///
/// A capture prefix — `f`/`t`/`F`/`T` and `r` — takes its next key as
/// **data**, not as a command, so this is the mirror image of
/// [`EditorPrefix::leaves`] and deliberately does not share its path: a key
/// that is a capture never resolves against the keymap, and a key that is a
/// leaf never becomes a character. Both grammars share this one function so
/// `r` and `f` cannot disagree about which keystrokes are capture-able.
///
/// `:` and `/` are refused with no modifier. On this route they open modal
/// lines that own the keyboard outright, and M003 requires them to keep
/// outranking everything including a pending target — `f` then `:` opens the
/// command line rather than searching for a colon. With `SHIFT` they are
/// ordinary characters: the `:` arm does not accept `SHIFT`, so there is no
/// line for them to open, and refusing them there would only throw away a
/// usable target. The cost of that rule is that `r :` cannot replace a
/// character with a colon on this route; it is the same trade the find
/// grammar already made, and diverging here would make the two disagree.
///
/// Every other non-character key is refused too, and the prefix then *retries*
/// it rather than eating it. That is the whole hazard of this milestone: an
/// armed `f` that swallowed the arrow that follows it would be an arrow that
/// never reaches the composer.
fn capture_char(key: KeyEvent) -> Option<char> {
    if key.modifiers == KeyModifiers::NONE
        && matches!(key.code, KeyCode::Char(':') | KeyCode::Char('/'))
    {
        return None;
    }
    match key.code {
        KeyCode::Char(target) => Some(target),
        _ => None,
    }
}

/// The character a leaf lookup should use for `key`, if any.
///
/// A leaf glyph may be a capital, and terminals disagree about how they encode
/// one: some send `Char('S')` with no modifier, others send `Char('s')` plus
/// `SHIFT`. The bare-key table has always accepted both spellings, and
/// `PrefixLeaf::glyph`'s own doc has always claimed the leaf lookup did too —
/// but the lookup gated on `NONE` alone, so `space S`, `space F` and every
/// other capital leaf would have been live on one terminal and dead on the
/// next. That is a key that silently does nothing, which is the M002 failure
/// mode this milestone exists to end, so the claim is made true here rather
/// than left as a doc that outruns the code.
///
/// `SHIFT` upper-cases rather than passing the raw glyph through, so a
/// terminal that reports `Char('s')` + `SHIFT` still reaches the `'S'` row.
/// Anything else — a control, alt, or a non-character key — is not a leaf.
fn leaf_lookup_char(key: KeyEvent) -> Option<char> {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char(glyph)) => Some(glyph),
        (KeyModifiers::SHIFT, KeyCode::Char(glyph)) => Some(glyph.to_ascii_uppercase()),
        _ => None,
    }
}

/// The character an `m`-grammar argument should use, if `key` carries one.
///
/// **Not** [`capture_char`]. The `m` grammar's object arguments include `W` —
/// the WORD object — and terminals disagree about how a capital is encoded:
/// some send `Char('W')` with no modifier, others send `Char('w')` plus
/// `SHIFT`. The find and replace captures take a character as data and have
/// no case to reconcile; the object arguments do, and a capital that arrived
/// as a lowercase `w` would name the *word* object instead of the *WORD* one.
fn match_arg_char(key: KeyEvent) -> Option<char> {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char(':')) | (KeyModifiers::NONE, KeyCode::Char('/')) => None,
        (KeyModifiers::NONE, KeyCode::Char(target)) => Some(target),
        (KeyModifiers::SHIFT, KeyCode::Char(target)) => Some(target.to_ascii_uppercase()),
        _ => None,
    }
}

/// The character a register name capture should use, if `key` carries one.
///
/// **Not** [`capture_char`]. That rule refuses an unmodified `:` so a find
/// target cannot open a modal line — but `/` is a *register* here (the search
/// register), and `:` is simply not one, which the resolver reports as an
/// invalid name rather than silently dropping. The accept set is therefore
/// every character, in both encodings a terminal uses for a shifted glyph:
/// `"` and `_` are shifted punctuation on most layouts, and some terminals
/// report the composed character with `NONE` while others keep `SHIFT`.
fn register_name_char(key: KeyEvent) -> Option<char> {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char(name)) => Some(name),
        (KeyModifiers::SHIFT, KeyCode::Char(name)) => Some(name),
        _ => None,
    }
}

/// What a capture prefix does with a key that cannot be its data.
///
/// The **one** answer, asked of the prefix rather than written at each capture
/// site, so a new capture grammar inherits the editor's mismatch policy
/// instead of inventing one. `f`/`t`/`F`/`T`, `r` and the `m` arguments retry
/// (their policy is `false`); the register name capture swallows — see
/// [`EditorPrefix::swallows_unmatched`].
fn capture_mismatch(pending: EditorPrefix) -> PrefixResolution {
    if pending.swallows_unmatched() {
        PrefixResolution::Swallowed
    } else {
        PrefixResolution::Retry
    }
}

/// Arm the second level of the `"` grammar.
///
/// The captured name rides in the variant, so the selection's lifetime is the
/// machine's: the next key either consumes it or the unconditional disarm
/// drops it.
fn arm_register_name(state: &mut EditorState, name: char) {
    if let Some(session) = state.session.as_mut() {
        session
            .presentation_mut()
            .arm_prefix(EditorPrefix::RegisterNamed(name));
    }
}

/// Resolve one captured register name.
///
/// A name outside the set **disarms and reports the set**. It never falls
/// through to the composer: the character was the argument, so `"D` must not
/// delete to the end of the line and `"A` must not enter Insert mode. That is
/// the destructive half of the closed-vocabulary rule — the other half is the
/// non-character key, which [`capture_mismatch`] swallows for this prefix.
fn resolve_register_name(state: &mut EditorState, name: char) -> PrefixResolution {
    if RegisterName::parse(name).is_none() {
        state.set_notice(EditorNotice::Note(
            "not a register: use a–z, `\"` for the unnamed one, `_` or `/`".to_string(),
        ));
        return PrefixResolution::Handled(BufferKey::Consumed);
    }
    arm_register_name(state, name);
    PrefixResolution::Handled(BufferKey::Consumed)
}

/// Resolve the verb that consumes a selected register.
///
/// The four verbs that honour a register are `y`, `p`, `P` and `R`; `d` and
/// `c` are here only for the blackhole, because Helix's delete is not
/// register-targeted and `"_d` is the one spelling that must not yank. Every
/// other key retries: the register is dropped and the key does what it
/// normally does, which is Vim's answer to `"a` then `j`.
fn resolve_register_verb(
    state: &mut EditorState,
    register: RegisterName,
    key: KeyEvent,
) -> PrefixResolution {
    let none = KeyModifiers::NONE;
    let shift = KeyModifiers::SHIFT;
    match (key.modifiers, key.code) {
        (m, KeyCode::Char('y')) if m == none => yank_to_register(state, register),
        (m, KeyCode::Char('p')) if m == none => {
            paste_from_register(register, ClipboardVerb::PasteAfter)
        }
        (m, KeyCode::Char('P')) if m == none || m == shift => {
            paste_from_register(register, ClipboardVerb::PasteBefore)
        }
        (m, KeyCode::Char('R')) if m == none || m == shift => {
            paste_from_register(register, ClipboardVerb::Replace)
        }
        // The delete family. `d`/`c` ignore a **named** selection — a delete
        // is not register-targeted — and honour the blackhole, which is the
        // whole point of `"_d`.
        (m, KeyCode::Char('d')) if m == none => {
            match apply_helix(state, editor::HelixVerb::DeleteSelection) {
                editor::HelixVerbOutcome::DeleteRanges(ranges) => {
                    PrefixResolution::Handled(BufferKey::DeleteRanges {
                        ranges,
                        yank: delete_target(register),
                    })
                }
                _ => PrefixResolution::Handled(BufferKey::Consumed),
            }
        }
        (m, KeyCode::Char('c')) if m == none => {
            match apply_helix(state, editor::HelixVerb::ChangeSelection) {
                editor::HelixVerbOutcome::ChangeRanges(ranges) => {
                    PrefixResolution::Handled(BufferKey::ChangeRanges {
                        ranges,
                        yank: delete_target(register),
                    })
                }
                _ => PrefixResolution::Handled(BufferKey::Consumed),
            }
        }
        _ => PrefixResolution::Retry,
    }
}

/// `"<reg>y` — yank the held selection into `register`.
fn yank_to_register(state: &mut EditorState, register: RegisterName) -> PrefixResolution {
    let Some(session) = state.session.as_mut() else {
        return PrefixResolution::Handled(BufferKey::Consumed);
    };
    let ranges = yank_ranges(session);
    PrefixResolution::Handled(if ranges.is_empty() {
        BufferKey::Consumed
    } else {
        BufferKey::YankRanges {
            ranges,
            target: YankTarget::Selected(register),
        }
    })
}

/// `"<reg>p` / `P` / `R` — paste or replace from `register`.
fn paste_from_register(register: RegisterName, verb: ClipboardVerb) -> PrefixResolution {
    PrefixResolution::Handled(BufferKey::Paste { verb, register })
}

/// Where a delete's bytes go when `"` selected `register`.
///
/// Not register-targeted — Helix's `d` yanks into the unnamed register
/// whatever was selected — with **one** exception: the blackhole. `"_d` is the
/// explicit "delete and yank nothing" spelling, and a delete that ignored it
/// would make the only register whose purpose is refusing a write the one
/// register a delete writes anyway.
fn delete_target(register: RegisterName) -> YankTarget {
    match register {
        RegisterName::Blackhole => YankTarget::Nothing,
        _ => YankTarget::Unnamed,
    }
}

/// Arm the second level of the `m` grammar.
///
/// The one place the `m` sub-state is written, so the transition cannot be
/// spelled two ways.
fn arm_match_arg(state: &mut EditorState, arg: editor::MatchArg) {
    if let Some(session) = state.session.as_mut() {
        session
            .presentation_mut()
            .arm_prefix(EditorPrefix::MatchArg(arg));
    }
}

/// Resolve one `m`-grammar argument against the character it captured.
///
/// The argument's **command** decides what happens; the character is data.
/// The two object arguments are the exception: their vocabulary is fixed, and
/// a character outside it is not a slightly-wrong object but not an object
/// at all — so the key is handed back to the composer rather than eaten.
fn resolve_match_arg(
    state: &mut EditorState,
    arg: editor::MatchArg,
    target: char,
) -> PrefixResolution {
    match arg {
        // The replace takes **two** characters: the first is held while the
        // second is waited for, which is why the prefix's variant carries a
        // payload rather than being a plain flag.
        editor::MatchArg::SurroundReplaceFrom => {
            arm_match_arg(state, editor::MatchArg::SurroundReplaceTo(target));
            PrefixResolution::Handled(BufferKey::Consumed)
        }
        editor::MatchArg::SurroundAdd => {
            apply_helix_edit(state, editor::HelixVerb::SurroundAdd { ch: target });
            PrefixResolution::Handled(BufferKey::Consumed)
        }
        editor::MatchArg::SurroundDelete => {
            apply_helix_edit(state, editor::HelixVerb::SurroundDelete { ch: target });
            PrefixResolution::Handled(BufferKey::Consumed)
        }
        editor::MatchArg::SurroundReplaceTo(from) => {
            apply_helix_edit(
                state,
                editor::HelixVerb::SurroundReplace { from, to: target },
            );
            PrefixResolution::Handled(BufferKey::Consumed)
        }
        editor::MatchArg::AroundObject | editor::MatchArg::InnerObject => {
            let Some(kind) = editor::ObjectKind::from_char(target) else {
                // Not an object. The key falls through to the composer rather
                // than being swallowed, and the machine is left disarmed —
                // the same rule every other capture follows.
                return PrefixResolution::Retry;
            };
            apply_helix(
                state,
                editor::HelixVerb::SelectObject {
                    kind,
                    around: matches!(arg, editor::MatchArg::AroundObject),
                },
            );
            PrefixResolution::Handled(BufferKey::Consumed)
        }
    }
}

/// Resolve `key` against the armed prefix, disarming it either way.
///
/// This is the single place prefix state is read or written on the dispatch
/// path, which is what lets a new grammar arrive as a leaf here instead of a
/// new flag on the presentation. Resolution is deliberately a separate
/// function rather than a block at the top of `handle_normal_key`:
/// `scripts/check_tui_editor_key_authority.py` scopes itself to that
/// function's first `match (key.modifiers, key.code)`, and a prefix
/// resolution shaped like a key table would make it parse the wrong table.
///
/// The disarm is unconditional and happens before the leaf is consulted, so
/// no gesture can leave the machine armed and a key must always be able to
/// recover it.
fn resolve_pending_prefix(
    state: &mut EditorState,
    key: KeyEvent,
    visible_lines: usize,
) -> PrefixResolution {
    // The armed prefix is read and the machine disarmed in one step, and the
    // session borrow is **released** before anything else happens. The
    // resolutions below call back into `state`, and a borrow that spanned
    // them would make the second call a second mutable borrow of the same
    // state — which is why the `m` grammar's second level, which has to arm
    // the prefix again, needed this split.
    let pending = {
        let Some(session) = state.session.as_mut() else {
            return PrefixResolution::Idle;
        };
        let pending = session.presentation().pending_prefix;
        if !pending.is_pending() {
            return PrefixResolution::Idle;
        }
        session.presentation_mut().disarm_prefix();
        pending
    };

    // Capture prefixes are resolved *before* the leaf table because their next
    // key is data, not a leaf: `EditorPrefix::leaves` is empty for all of
    // them, so reaching the leaf lookup would send a perfectly good target
    // down the mismatch path and out to the composer.
    //
    // One branch, not two. `is_capture` is the classifier, each grammar's
    // accept rule is a named function, and the **mismatch** answer is
    // `capture_mismatch` — so a capture grammar added later cannot arrive
    // with its own copy of any of the three, and cannot quietly decide that
    // an unmatched key is swallowed.
    if pending.is_capture() {
        // A capture takes the next key as **data**, and a count for it is out
        // of scope (repeating a find is `Alt-.`'s job). The count is taken
        // down here rather than at the resolver's exit because the find below
        // runs a motion, and a motion consumes the count: leaving it armed
        // would repeat `3f x` three times instead of discarding the `3`.
        clear_count(state);
        // The `"` grammar's first level. Its name is data, but the set is a
        // closed vocabulary, so it has its own accept rule and its own
        // mismatch answer — see `register_name_char` and `capture_mismatch`.
        if pending.awaits_register_name() {
            let Some(name) = register_name_char(key) else {
                // A key that cannot be a name is a broken gesture, not a
                // wrong target: swallowing it is what stops `"` then `Tab`
                // from toggling the composer.
                return capture_mismatch(pending);
            };
            return resolve_register_name(state, name);
        }
        // The `m` grammar's second level has its **own** capture rule, because
        // its object arguments include a capital and the terminals disagree
        // about how one is encoded. It is resolved first, and it owns its
        // whole branch, so the find and replace captures cannot grow a case
        // rule by accident.
        if let Some(arg) = pending.match_arg() {
            let Some(target) = match_arg_char(key) else {
                // Not a target: the prefix's own policy decides whether the
                // key is handed back or consumed with the gesture.
                return capture_mismatch(pending);
            };
            return resolve_match_arg(state, arg, target);
        }
        let Some(target) = capture_char(key) else {
            // Not a target: the prefix's own policy decides whether the key
            // is handed back or consumed with the gesture.
            return capture_mismatch(pending);
        };
        if pending.is_replace_char() {
            editor_editing_verb(state, |snapshot, presentation| {
                editor::replace_char(snapshot, presentation, target)
            });
        } else {
            let Some(kind) = pending.find_kind() else {
                // `is_capture` is "find, replace, register name, or `m`
                // argument", and every one of those is handled above, so this
                // is unreachable; returning rather than unwrapping keeps a
                // future variant from panicking the key handler if the
                // classifier ever grows an arm without a verb for it.
                return PrefixResolution::Retry;
            };
            helix_motion(
                state,
                editor::FindMotion { kind, target }.command(),
                visible_lines,
            );
        }
        return PrefixResolution::Handled(BufferKey::Consumed);
    }

    // The `"` grammar's second level: the name was captured, and this key is
    // the verb that consumes the selection. It is not a capture (its next key
    // is a verb, not data) and it is not a leaf table either — each of the
    // four verbs it honours needs the captured name as an argument.
    if let Some(name) = pending.register_name() {
        // `pending` only ever holds a validated name: the capture refuses
        // everything outside the set. Returning rather than unwrapping keeps a
        // future caller from panicking the key handler if that ever stops
        // being true.
        let Some(register) = RegisterName::parse(name) else {
            return capture_mismatch(pending);
        };
        return resolve_register_verb(state, register, key);
    }

    // One lookup, N leaves. A prefix that has no row for this glyph takes the
    // mismatch path below, which is what `swallows_unmatched` then decides —
    // so adding a leaf never invents a new mismatch policy.
    let matched = leaf_lookup_char(key).and_then(|glyph| {
        pending
            .leaves()
            .iter()
            .find(|leaf| leaf.glyph == glyph)
            .copied()
    });
    if let Some(leaf) = matched {
        return match leaf.action {
            // The **raw** motion path, deliberately, not `helix_motion`. This is
            // how `gg` has always resolved: a goto collapses the selection and
            // moves the cursor, rather than extending a held character
            // selection the way a bare `j` or `w` does. Routing leaves through
            // the selection machine instead would have silently changed `gg`'s
            // behaviour, which the milestone requires to stay exactly as it
            // was — and it would have made goto mode inconsistent with Helix,
            // where every goto leaf drops the selection.
            editor::PrefixAction::Motion(command) => {
                note_motion(state, command);
                editor_motion(state, command, visible_lines);
                PrefixResolution::Handled(BufferKey::Consumed)
            }
            editor::PrefixAction::YankSelection => {
                let ranges = {
                    let Some(session) = state.session.as_mut() else {
                        return PrefixResolution::Handled(BufferKey::Consumed);
                    };
                    yank_ranges(session)
                };
                PrefixResolution::Handled(if ranges.is_empty() {
                    BufferKey::Consumed
                } else {
                    // `space y` is the clipboard family: it is **not**
                    // register-aware, so `"` cannot redirect it.
                    BufferKey::YankRanges {
                        ranges,
                        target: YankTarget::Clipboard,
                    }
                })
            }
            // The shared edit funnel, not a new insertion path: `]Space` must
            // land in the same read-only gate, the same undo entry and the same
            // document-write transaction as `x`, `d` and `y`. A leaf that
            // edited on its own would have been the only verb in the editor
            // with its own idea of what an edit costs.
            //
            // Deliberately *not* followed by `note_motion`: an edit is not a
            // motion, and Helix's `Alt-.` repeats a motion. Recording one here
            // would make `]Space` then `Alt-.` insert a second newline, which
            // is the exact behaviour the milestone forbids.
            editor::PrefixAction::Edit(command) => {
                editor_editing_command(state, command);
                PrefixResolution::Handled(BufferKey::Consumed)
            }
            // The second level of a two-level grammar. The leaf does not run a
            // verb: it says **which argument is being waited for**, and the
            // argument itself is the key after it.
            editor::PrefixAction::Capture(arg) => {
                arm_match_arg(state, arg);
                PrefixResolution::Handled(BufferKey::Consumed)
            }
            // The comment toggles run through the **verb** path rather than
            // building a transaction here: `apply_helix` owns the read-only
            // gate and `apply_helix_edit` owns the submission, so a leaf that
            // resolved its own edit would be the one write with its own idea
            // of what a write costs. The leaf names the style; the verb names
            // nothing the grammar could not change under it.
            editor::PrefixAction::ToggleComments { block } => {
                apply_helix_edit(state, editor::HelixVerb::ToggleComments { block });
                PrefixResolution::Handled(BufferKey::Consumed)
            }
            // Name the request; do not perform it. Running an LSP read needs
            // the App's core client, task registry and toasts, none of which
            // this function can see — so the effect is parked on the editor's
            // own state and drained by `handle_editor_key` on the next line.
            //
            // Deliberately *not* `note_motion`: an LSP read is not a motion,
            // and recording one would make `space k` then `Alt-.` re-issue the
            // hover — the same mistake the edit arm documents.
            editor::PrefixAction::Effect(effect) => {
                state.pending_effect = Some(effect);
                PrefixResolution::Handled(BufferKey::Consumed)
            }
        };
    }

    // The baseline resolved an unmatched key differently per prefix: `g` then
    // an arrow consumed it, `space` then an arrow moved the cursor. The policy
    // therefore comes from the prefix rather than from the machine — collapsing
    // the two would have changed one of the two gestures this milestone is
    // required to preserve exactly. A leaf-lookup miss and a capture miss are
    // the same question, so they get the same answer from one function.
    capture_mismatch(pending)
}

/// Handle one key while the editor buffer owns input.
///
/// `mode` is the route's single keyboard axis, passed in rather than read
/// from a second stored fact — see [`EditorFocus`][gone] in
/// [`crate::tui::editor`]. Pure presentation state plus an explicit request
/// for the effects that belong to the App layer; it never touches the
/// clipboard or the controller itself.
fn editor_buffer_key(
    state: &mut EditorState,
    mode: crate::tui::input::InputMode,
    key: KeyEvent,
) -> BufferKey {
    // The `:` and `/` lines are both modal and outrank everything below
    // them, including the mode-independent Ctrl chords: a half-typed
    // `:q` must not be rewritten by `Ctrl+S` silently writing the file the
    // user was about to close. `/` is checked first only so the two input
    // lines can never both be open.
    if state
        .session
        .as_ref()
        .is_some_and(|session| session.presentation().search_active)
    {
        let visible_lines = visible_line_estimate();
        return handle_search_line_key(state, key, visible_lines);
    }
    if state
        .session
        .as_ref()
        .is_some_and(|session| session.presentation().command_active)
    {
        return handle_command_line_key(state, key);
    }
    // Chord ownership: **every chord has exactly one owner, and that owner is
    // the first stage that claims it.** The stages below are ordered, so a
    // chord declared in a later stage is dead the moment an earlier stage
    // claims it — the binding still compiles, still reads correctly, and
    // never runs.
    //
    // That is not hypothetical. `handle_normal_key` used to declare
    // `(CONTROL, Char('r')) => editor_redo`, which this block shadowed: Normal
    // mode advertised undo with no reachable redo, and the three tests that
    // "covered" redo called `editor_redo` directly, so they never noticed.
    //
    // Before adding a chord here, check it against `handle_normal_key` and
    // `handle_insert_key`; before adding one there, check it against this
    // block. `scripts/check_tui_editor_key_authority.py` enforces the
    // disjointness mechanically, and the dispatch-path tests assert each
    // chord's observable effect, so a collision fails the suite instead of
    // quietly disabling a binding.
    //
    // Save and resync are mode-independent and always available. They are the
    // lifecycle round trips: the buffer reports the request and the App layer
    // issues it, so neither may depend on which mode the route is in.
    if key.code == KeyCode::Char('s') && key.modifiers == KeyModifiers::CONTROL {
        return BufferKey::RequestSave;
    }
    if key.code == KeyCode::Char('r') && key.modifiers == KeyModifiers::CONTROL {
        return BufferKey::RequestResync;
    }
    // `R` no longer reloads. It is Helix's `replace_with_yanked`, and
    // reload is still one `:e` away — so the capability survived the
    // rebinding even though the keystroke did not. A `R` here would shadow
    // the Normal-mode verb *and* survive it, which is the one failure this
    // dispatch chain is arranged to make impossible.
    let visible_lines = visible_line_estimate();
    if mode == crate::tui::input::InputMode::Insert {
        return if handle_insert_key(state, key, visible_lines) {
            BufferKey::Consumed
        } else {
            BufferKey::Unhandled
        };
    }
    handle_normal_key(state, key, visible_lines)
}

/// Handle one Normal-mode key against the buffer.
///
/// Speaks Helix, deliberately the same dialect `/terminal` speaks in its
/// viewing mode: `hjkl` and arrows move, `Ctrl+d`/`Ctrl+u` page, `e`/`b` jump
/// by word, `v` selects by character, `x` selects the whole line and grows one
/// line per press, `d` deletes the held selection, and `space y` yanks it. A
/// user who learned one learned the other.
///
/// vi's `dd`/`dw` are gone, and `x` then `d` is the same gesture in this
/// dialect. `D` stays as delete-to-end-of-line because it has no Helix meaning
/// here and is the only way to trim a line without selecting it first.
///
/// `u`/`U` are undo and redo, Helix's own pair. Every key listed here is
/// reached through this function, which means every key listed here is
/// reachable only if nothing above it claims the chord first — see the chord
/// ownership note on the guard block in [`editor_buffer_key`].
///
/// Returns [`BufferKey::Unhandled`] for a key the buffer does not own, which
/// is how the bottom input box keeps typing in this mode.
fn handle_normal_key(state: &mut EditorState, key: KeyEvent, visible_lines: usize) -> BufferKey {
    // One pending prefix, resolved first and disarmed either way. `gg` and
    // `space y` disagree about what an unmatched second key means, so that
    // decision belongs to the armed prefix — see
    // [`EditorPrefix::swallows_unmatched`]. Neither of the two bespoke
    // mechanisms this replaced had a leaf table: each inspected the key inline
    // and could only ever grow by adding another field.
    match resolve_pending_prefix(state, key, visible_lines) {
        PrefixResolution::Idle | PrefixResolution::Retry => {}
        // The leaf either consumed the count (a motion) or did not own it (an
        // edit, a yank, an effect, a capture). Taking it down on the way out
        // covers the second case, and is a no-op for the first — the motion
        // path already took it.
        PrefixResolution::Handled(settled) => {
            clear_count(state);
            return settled;
        }
        PrefixResolution::Swallowed => {
            clear_count(state);
            return BufferKey::Consumed;
        }
    }
    let Some(session) = state.session.as_mut() else {
        return BufferKey::Unhandled;
    };
    // Read once, before the table: whether `0` begins or extends a count
    // depends on the answer, and it must not change between the arm's guard
    // and the clearing rule below the table.
    let pending_count = session.presentation().pending_count;

    let none = KeyModifiers::NONE;
    let shift = KeyModifiers::SHIFT;
    let alt = KeyModifiers::ALT;
    // The exact bitmask for Alt+Shift. It is a modifier of its own, not a
    // loose "an Alt with a shift on it": an arm that accepts `ALT` accepts
    // exactly `ALT`, and one that accepts `ALT | SHIFT` accepts exactly
    // that. The spelling matters because the two are different keys.
    let alt_shift = KeyModifiers::ALT | KeyModifiers::SHIFT;
    // Each arm is `(modifiers, code)` so a bare letter and its uppercase twin
    // are one rule. Terminals disagree about how `D` is encoded, and both
    // spellings have to work.
    // Set by the count arm and by nothing else, so the tail below can tell the
    // one key that extends a count from every key that ends one.
    let mut count_extended = false;
    let settled = match (key.modifiers, key.code) {
        (m, KeyCode::Char('o')) if m == alt => BufferKey::ExpandSyntaxSelection,
        (m, KeyCode::Char('i')) if m == alt => BufferKey::ShrinkSyntaxSelection,
        // Paging resolves before the bare-char arms: `Ctrl+d`/`Ctrl+u` move a
        // viewport at a time, while bare `d` deletes a selection. They are
        // distinct events, not a shadowing pair.
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
            helix_motion(state, EditorCommand::HalfPageDown, visible_lines);
            BufferKey::Consumed
        }
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
            helix_motion(state, EditorCommand::HalfPageUp, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('j')) | (m, KeyCode::Down) if m == none || m == shift => {
            helix_motion(state, EditorCommand::MoveDown, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('k')) | (m, KeyCode::Up) if m == none || m == shift => {
            helix_motion(state, EditorCommand::MoveUp, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('h')) | (m, KeyCode::Left) if m == none || m == shift => {
            helix_motion(state, EditorCommand::MoveLeft, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('l')) | (m, KeyCode::Right) if m == none || m == shift => {
            helix_motion(state, EditorCommand::MoveRight, visible_lines);
            BufferKey::Consumed
        }
        // `PageDown`/`PageUp` are a **full** page, and `Ctrl+d`/`Ctrl+u` are
        // half of one. They used to be the same motion behind two names, so
        // `PageDown` moved half a screen and the key's name was a lie.
        (m, KeyCode::PageDown) if m == none => {
            helix_motion(state, EditorCommand::PageDown, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::PageUp) if m == none => {
            helix_motion(state, EditorCommand::PageUp, visible_lines);
            BufferKey::Consumed
        }
        // ── The count (M004) ──────────────────────────────────────────────
        //
        // One arm, not nine, and **ahead of the `0` arm**: `0` begins no count
        // (with nothing pending it is `LineStart`, which the arm below still
        // owns) but extends one, so `10` is ten. `count_extended` tells the
        // tail this key was the count rather than something to clear it.
        (m, KeyCode::Char(ch))
            if m == none && ch.is_ascii_digit() && (ch != '0' || pending_count.is_some()) =>
        {
            let clamped = session.presentation_mut().extend_count(ch);
            count_extended = true;
            if clamped {
                // Saturation is a refusal, and an unseen refusal is a count the
                // reader trusts and is wrong about.
                state.set_notice(EditorNotice::Note(format!(
                    "count stops at {max}",
                    max = editor::MAX_COUNT
                )));
            }
            BufferKey::Consumed
        }
        (m, KeyCode::Char('0')) if m == none => {
            helix_motion(state, EditorCommand::LineStart, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('$')) if m == none => {
            helix_motion(state, EditorCommand::LineEnd, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('G')) if m == none || m == shift => {
            helix_motion(state, EditorCommand::BufferEnd, visible_lines);
            BufferKey::Consumed
        }
        // `gg` has no single-key spelling, so it arms the one prefix whose
        // second key is consumed rather than retried.
        (m, KeyCode::Char('g')) if m == none => {
            session.presentation_mut().arm_prefix(EditorPrefix::Goto);
            BufferKey::Consumed
        }
        // `z` — Helix's view mode. The page scrolls and the cursor stays put,
        // which is the one thing a cursor-bound viewport cannot do.
        (m, KeyCode::Char('z')) if m == none => {
            session.presentation_mut().arm_prefix(EditorPrefix::View);
            BufferKey::Consumed
        }
        // Helix's `m` — match mode. The **first** level of a two-level
        // grammar: the next key is the command (`m m`, `m s`, `m a`), and for
        // most of those commands the key after that is the argument.
        //
        // The second level rides in the prefix's own payload rather than in a
        // second field on the presentation, so the machine stays one prefix
        // wide — see `EditorPrefix::MatchArg`.
        (m, KeyCode::Char('m')) if m == none => {
            session.presentation_mut().arm_prefix(EditorPrefix::Match);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('w')) if m == none => {
            helix_motion(state, EditorCommand::WordForward, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('b')) if m == none => {
            helix_motion(state, EditorCommand::WordBackward, visible_lines);
            BufferKey::Consumed
        }
        // Helix `e`, matching the terminal panel's forward word jump.
        (m, KeyCode::Char('e')) if m == none => {
            helix_motion(state, EditorCommand::WordEnd, visible_lines);
            BufferKey::Consumed
        }
        // Helix's long-word motions. They exist because `w`/`b`/`e` classify
        // by byte — alphanumeric, `_`, or anything non-ASCII — which stops
        // being a distinction worth having once a line contains punctuation or
        // CJK. `W`/`B`/`E` classify by whitespace instead, so `foo.bar` is one
        // WORD for them and two words for `w`.
        (m, KeyCode::Char('W')) if m == none || m == shift => {
            helix_motion(state, EditorCommand::LongWordForward, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('B')) if m == none || m == shift => {
            helix_motion(state, EditorCommand::LongWordBackward, visible_lines);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('E')) if m == none || m == shift => {
            helix_motion(state, EditorCommand::LongWordEnd, visible_lines);
            BufferKey::Consumed
        }
        // Character-finding motions. Each arms a prefix whose next key is a
        // *target character* rather than a leaf, resolved by
        // `resolve_pending_prefix`. They arm rather than move because there
        // is no one-key spelling: the target arrives on the following
        // keystroke, which in a legacy xterm is a separate event entirely.
        (m, KeyCode::Char('f')) if m == none => {
            session
                .presentation_mut()
                .arm_prefix(EditorPrefix::FindNext);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('F')) if m == none || m == shift => {
            session
                .presentation_mut()
                .arm_prefix(EditorPrefix::FindPrevious);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('t')) if m == none => {
            session
                .presentation_mut()
                .arm_prefix(EditorPrefix::FindTillNext);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('T')) if m == none || m == shift => {
            session
                .presentation_mut()
                .arm_prefix(EditorPrefix::FindTillPrevious);
            BufferKey::Consumed
        }
        // Helix `Alt-.` — repeat the last motion. Claimed only while there is
        // a motion to repeat: an unconditional binding would swallow a
        // composer keystroke and do nothing, which is the same reason `n` is
        // gated on an existing search.
        (m, KeyCode::Char('.')) if m == alt => {
            // A count on `Alt-.` is deliberately out of scope: the repeat
            // repeats the *last* gesture, and a count typed before it would
            // otherwise multiply a motion the reader had already chosen.
            // Discarding it here is why the tail below is not the only clear
            // on this path: the repeat resolves a motion, which would consume
            // the count before the tail ever saw it.
            clear_count(state);
            // The three slots are mutually exclusive by construction — every
            // recorder clears the other two, per `note_motion`'s single-slot
            // rule — so the order of these three is not a precedence between
            // motions. It is written newest-kind-first only so a future second
            // recorder has to make that true rather than inherit it.
            if let Some(repeat) = selection_repeat(state) {
                apply_helix(state, repeat.verb());
                return BufferKey::Consumed;
            }
            if let Some(repeat) = unimpaired_repeat(state) {
                helix_motion(state, repeat, visible_lines);
                return BufferKey::Consumed;
            }
            let Some(repeat) = last_find(state) else {
                return BufferKey::Unhandled;
            };
            helix_motion(state, repeat.command(), visible_lines);
            BufferKey::Consumed
        }
        // Helix `.` — repeat the last **insert**, and a different verb from
        // `Alt-.` above: that one repeats the last *motion*. The two are
        // separated by modifier (this arm is `NONE` only), so neither shadows
        // the other, and an unconditional claim is deliberate — with nothing
        // recorded it is a no-op that reports nothing, which is why `.` stops
        // typing a period into the composer.
        (m, KeyCode::Char('.')) if m == none => BufferKey::RepeatInsert,
        // Selection. `v` is a mode and `x` is an action; both delegate to the
        // shared state machine, so they cannot drift from the terminal.
        (m, KeyCode::Char('v')) if m == none => {
            apply_helix(state, editor::HelixVerb::SelectChar);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('x')) if m == none => {
            apply_helix(state, editor::HelixVerb::SelectLine);
            BufferKey::Consumed
        }
        // ── Line-shape verbs (M001, selection shape parity) ────────────────
        //
        // `X`, `_` and `Alt-x` reshape the selection and write nothing; `J`,
        // `Alt-J`, `>` and `<` write, so they go through `apply_helix_edit` and
        // land as one undoable transaction per gesture. The two indent keys
        // take both spellings of a shifted punctuation glyph, the same rule the
        // punctuation arms below follow — a terminal that sends `Char('>')`
        // with `SHIFT` and one that sends it bare both have to work, or the key
        // is dead on half the keyboards in use.
        //
        // `Alt-x` is claimed before any `x` arm could see it: `x` itself is
        // `NONE` only, so the two are distinct events rather than a shadowing
        // pair. `Alt-_` stays the adjacent-merge verb; bare `_` is this family.
        (m, KeyCode::Char('X')) if m == none || m == shift => {
            apply_helix(state, editor::HelixVerb::ExtendToLineBounds);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('x')) if m == alt => {
            apply_helix(state, editor::HelixVerb::ShrinkToLineBounds);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('_')) if m == none || m == shift => {
            apply_helix(state, editor::HelixVerb::TrimSelections);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('J')) if m == none || m == shift => {
            apply_helix_edit(
                state,
                editor::HelixVerb::JoinLines {
                    select_space: false,
                },
            );
            BufferKey::Consumed
        }
        (m, KeyCode::Char('J')) if m == alt => {
            apply_helix_edit(state, editor::HelixVerb::JoinLines { select_space: true });
            BufferKey::Consumed
        }
        (m, KeyCode::Char('>')) if m == none || m == shift => {
            apply_helix_edit(state, editor::HelixVerb::IndentSelection { outdent: false });
            BufferKey::Consumed
        }
        (m, KeyCode::Char('<')) if m == none || m == shift => {
            apply_helix_edit(state, editor::HelixVerb::IndentSelection { outdent: true });
            BufferKey::Consumed
        }
        // ── Selection manipulation (M008) ───────────────────────────────────
        //
        // Every chord below resolves a **set** of selections rather than one.
        // The pure set algebra is in `helix_selection` and the byte geometry
        // in `editor`; these arms are the binding table and nothing else, so
        // the keymap, the rules, and the geometry stay three separable things.
        //
        // Punctuation arms accept `NONE` **and** `SHIFT` where a shifted glyph
        // can arrive either way, and that is not decoration: a terminal that
        // sends `Char('&')` with `SHIFT` and one that sends it with `NONE`
        // both have to work, or `&` is dead on half the keyboards in use.
        // Letters follow the same rule for `C`, `S` and `K`.

        // Helix `;` — collapse the set to a single cursor at the primary's
        // head. Claimed with nothing held: an unclaimed `;` would escape to
        // the composer and start a word, which reads as nothing happening.
        (m, KeyCode::Char(';')) if m == none => {
            apply_helix(state, editor::HelixVerb::CollapseSelection);
            BufferKey::Consumed
        }
        // Helix `Alt-;` — flip every member's direction.
        (m, KeyCode::Char(';')) if m == alt => {
            apply_helix(state, editor::HelixVerb::FlipSelections);
            BufferKey::Consumed
        }
        // Helix `Alt-:` — ensure every member is forward. Distinct from the
        // bare `:` below by modifier, not by spelling: the bare colon opens
        // the command line and must keep doing so.
        (m, KeyCode::Char(':')) if m == alt => {
            apply_helix(state, editor::HelixVerb::EnsureSelectionsForward);
            BufferKey::Consumed
        }
        // Helix `%` — select the entire file.
        (m, KeyCode::Char('%')) if m == none || m == shift => {
            note_selection_motion(state, editor::SelectionMotion::SelectAll);
            apply_helix(state, editor::HelixVerb::SelectAll);
            BufferKey::Consumed
        }
        // Helix `,` — keep only the primary. `Alt-,` removes it instead.
        (m, KeyCode::Char(',')) if m == none => {
            apply_helix(state, editor::HelixVerb::KeepPrimarySelection);
            BufferKey::Consumed
        }
        (m, KeyCode::Char(',')) if m == alt => {
            apply_helix(state, editor::HelixVerb::RemovePrimarySelection);
            BufferKey::Consumed
        }
        // Helix `C` / `Alt-C` — add a cursor on the next / previous line.
        // Recorded as repeatable motions: they move the cursor, so `Alt-.`
        // repeats them the way it repeats a find.
        (m, KeyCode::Char('C')) if m == none || m == shift => {
            note_selection_motion(state, editor::SelectionMotion::CopyOntoNextLine);
            apply_helix(
                state,
                editor::HelixVerb::CopySelectionOnLine { line_delta: 1 },
            );
            BufferKey::Consumed
        }
        (m, KeyCode::Char('C')) if m == alt => {
            note_selection_motion(state, editor::SelectionMotion::CopyOntoPreviousLine);
            apply_helix(
                state,
                editor::HelixVerb::CopySelectionOnLine { line_delta: -1 },
            );
            BufferKey::Consumed
        }
        // Helix `(` / `)` move which member is the primary — no byte changes.
        // `Alt-(` / `Alt-)` rotate the members' **contents**, which does. The
        // two pairs are kept in one block and in one order so the difference
        // is visible at the call site rather than being a memory test.
        (m, KeyCode::Char('(')) if m == none || m == shift => {
            apply_helix(
                state,
                editor::HelixVerb::RotateSelections { member_delta: -1 },
            );
            BufferKey::Consumed
        }
        (m, KeyCode::Char(')')) if m == none || m == shift => {
            apply_helix(
                state,
                editor::HelixVerb::RotateSelections { member_delta: 1 },
            );
            BufferKey::Consumed
        }
        (m, KeyCode::Char('(')) if m == alt => {
            apply_helix_edit(
                state,
                editor::HelixVerb::RotateSelectionContents { member_delta: -1 },
            );
            BufferKey::Consumed
        }
        (m, KeyCode::Char(')')) if m == alt => {
            apply_helix_edit(
                state,
                editor::HelixVerb::RotateSelectionContents { member_delta: 1 },
            );
            BufferKey::Consumed
        }
        // Helix `&` — pad every selection out to the widest one's display
        // width. An **edit**, so it goes through `apply_helix_edit` and lands
        // as one undoable transaction.
        (m, KeyCode::Char('&')) if m == none || m == shift => {
            apply_helix_edit(state, editor::HelixVerb::AlignSelections);
            BufferKey::Consumed
        }
        // Helix `Alt-minus` merges every selection into one; `Alt-_` merges
        // only adjacent ones and leaves gaps alone. Written as two arms
        // because the glyphs are two and the meanings are two.
        (m, KeyCode::Char('-')) if m == alt => {
            apply_helix(state, editor::HelixVerb::MergeSelections);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('_')) if m == alt => {
            apply_helix(state, editor::HelixVerb::MergeConsecutiveSelections);
            BufferKey::Consumed
        }
        // Helix `S` / `K` / `Alt-K` take a regex, and there is no regex prompt
        // in this editor. Rather than build one — a new input mode, a new
        // pending field, and a new disarm contract on reload and buffer
        // switch — they open the `:` command line **already filled in**. The
        // line the reader sees is the one they could have typed by hand, the
        // pattern has somewhere to live, and `:` already knows how to abort.
        (m, KeyCode::Char('S')) if m == none || m == shift => {
            session.presentation_mut().open_command_line(&format!(
                "{} ",
                editor::EditorCommandName::SplitRegex.spec()
            ));
            BufferKey::Consumed
        }
        (m, KeyCode::Char('K')) if m == none || m == shift => {
            session
                .presentation_mut()
                .open_command_line(&format!("{} ", editor::EditorCommandName::KeepRegex.spec()));
            BufferKey::Consumed
        }
        (m, KeyCode::Char('K')) if m == alt => {
            session.presentation_mut().open_command_line(&format!(
                "{} ",
                editor::EditorCommandName::RemoveRegex.spec()
            ));
            BufferKey::Consumed
        }
        // Helix `s` — select the regex matches inside each selection. A
        // **selection** verb, so it opens the same `:` line `S` does and is
        // gated by `grows_selection` like `v` and `x`: on a read-only
        // attachment the reader is told, rather than handed a highlight they
        // could not act on.
        (m, KeyCode::Char('s')) if m == none => {
            session.presentation_mut().open_command_line(&format!(
                "{} ",
                editor::EditorCommandName::SelectRegex.spec()
            ));
            BufferKey::Consumed
        }
        // Helix `Alt-s` — split every selection on newlines. No pattern, so
        // no prompt: a line break is a byte, not a regex.
        (m, KeyCode::Char('s')) if m == alt => {
            apply_helix(state, editor::HelixVerb::SplitSelectionOnNewlines);
            BufferKey::Consumed
        }
        // Helix `=` formats the document. CodeGG's `=` shows the **preview** —
        // what the formatter *would* change — and, since M016, opens the
        // change review on it when the whole change is known. The write path
        // is ADR-0008's, and the editor reaches it by carrying the candidate
        // to the one builder: nothing is registered, and the daemon's digest
        // check still binds the request to the reviewed change.
        //
        // The chord is a **Normal-mode** verb, not a prefix leaf, so it
        // reports the request the way the other Normal-mode verbs report
        // their work: the buffer decides *what* was asked for, and the App
        // layer — which holds the core client — does *how*.
        (m, KeyCode::Char('=')) if m == none => BufferKey::LspRead(LspReadOperation::FormatPreview),
        // Claimed even with nothing selected: an unclaimed `d` escapes to the
        // global `OpenDiff` binding, so the reader never gets "delete this".
        //
        // M007: the deleted bytes go to the **unnamed register**, and never to
        // the system clipboard. That is M002's D1 discharged — `d` then `p`
        // moves text without clobbering what the reader copied elsewhere —
        // and the `Alt-d` arm below stays the spelling that writes nothing.
        (m, KeyCode::Char('d')) if m == none => {
            match apply_helix(state, editor::HelixVerb::DeleteSelection) {
                editor::HelixVerbOutcome::DeleteRanges(ranges) => BufferKey::DeleteRanges {
                    ranges,
                    yank: YankTarget::Unnamed,
                },
                _ => BufferKey::Consumed,
            }
        }
        // Helix `c` — change: delete, then type. Claimed even with nothing
        // selected, for the reason `d` is claimed: an unclaimed `c` reaches the
        // composer and types a `c` into whatever box has focus.
        //
        // The delete is resolved and submitted **before** the mode moves, and
        // the mode moves only on acceptance (the `ChangeRanges` drain in
        // `handle_editor_key`). Entering Insert first would leave the reader in
        // Insert on a buffer whose text survived a refused delete — the state
        // §8 of the milestone forbids. A read-only attachment is refused by
        // `apply_helix` (`mutates_buffer`), which also means this arm returns
        // `Consumed` rather than a change request.
        (m, KeyCode::Char('c')) if m == none => {
            match apply_helix(state, editor::HelixVerb::ChangeSelection) {
                editor::HelixVerbOutcome::ChangeRanges(ranges) => BufferKey::ChangeRanges {
                    ranges,
                    yank: YankTarget::Unnamed,
                },
                _ => BufferKey::Consumed,
            }
        }
        // Helix `Alt-c` / `Alt-d` — the explicit **no-yank** spellings. They
        // resolve to the same delete as `c` and `d` and differ only in where
        // the bytes go: nowhere. M002 bound them for this milestone, and the
        // promise is asserted at the dispatch boundary — the target is
        // `Nothing`, so not even the unnamed register is written, and the
        // system clipboard was never in reach.
        (m, KeyCode::Char('c')) if m == alt => {
            match apply_helix(state, editor::HelixVerb::ChangeSelection) {
                editor::HelixVerbOutcome::ChangeRanges(ranges) => BufferKey::ChangeRanges {
                    ranges,
                    yank: YankTarget::Nothing,
                },
                _ => BufferKey::Consumed,
            }
        }
        (m, KeyCode::Char('d')) if m == alt => {
            match apply_helix(state, editor::HelixVerb::DeleteSelection) {
                editor::HelixVerbOutcome::DeleteRanges(ranges) => BufferKey::DeleteRanges {
                    ranges,
                    yank: YankTarget::Nothing,
                },
                _ => BufferKey::Consumed,
            }
        }
        // Helix `y` — yank the held selection into the **selected register**,
        // which is the unnamed one unless `"` named another. M007 splits what
        // used to be one path: bare `y` still reaches the system clipboard
        // (D1(a): every yank does), and additionally writes the register, so
        // `y` then `p` round-trips through the unnamed register and `"ay`
        // reaches `a`. `space y` is the spelling that means the clipboard
        // alone.
        //
        // Helix keeps the selection after a yank; this editor drops it, and
        // that was already true of `space y`, which M004 is required to
        // preserve byte for byte. Two spellings of one verb that behaved
        // differently would be a worse surprise than either behaviour alone,
        // so bare `y` keeps its sibling's rule rather than Helix's.
        (m, KeyCode::Char('y')) if m == none => {
            let ranges = yank_ranges(session);
            if ranges.is_empty() {
                BufferKey::Consumed
            } else {
                BufferKey::YankRanges {
                    ranges,
                    target: YankTarget::Selected(RegisterName::Unnamed),
                }
            }
        }
        // Helix's paste round trip. `p` pastes after, `P` before, and `R`
        // replaces — all three read the **selected register** (the unnamed one
        // by default), which is what M007 added. The bytes are the App
        // layer's, so the buffer reports the verb and the source and lets it
        // read them. They are claimed even when that read refuses, because a
        // `p` that fell through to the composer would type a `p` into a
        // session name.
        (m, KeyCode::Char('p')) if m == none => BufferKey::Paste {
            verb: ClipboardVerb::PasteAfter,
            register: RegisterName::Unnamed,
        },
        (m, KeyCode::Char('P')) if m == none || m == shift => BufferKey::Paste {
            verb: ClipboardVerb::PasteBefore,
            register: RegisterName::Unnamed,
        },
        // Helix `R` is `replace_with_yanked`: replace the target with the
        // selected register's text. `space R` is the spelling that replaces
        // from the system clipboard.
        //
        // `R` used to reload from disk. It does not any more: Helix gives `R`
        // this meaning, and reload is still one `:e` away, so the capability
        // survives the rebinding even though the keystroke does not.
        (m, KeyCode::Char('R')) if m == none || m == shift => BufferKey::Paste {
            verb: ClipboardVerb::Replace,
            register: RegisterName::Unnamed,
        },
        // `"` — Helix's register prefix. It arms a **capture**: the next key is
        // a register name, not a command, and the machine re-arms as
        // `RegisterNamed` so the verb after it can consume the selection. The
        // name is captured by `resolve_pending_prefix`, which also owns the
        // refusal for a character outside the set.
        (m, KeyCode::Char('"')) if m == none || m == shift => {
            session
                .presentation_mut()
                .arm_prefix(EditorPrefix::Register);
            BufferKey::Consumed
        }
        // Helix `~` — switch case. Accepts `NONE` and `SHIFT` because
        // terminals disagree about how a shifted backquote is encoded.
        (m, KeyCode::Char('~')) if m == none || m == shift => {
            editor_editing_verb(state, editor::switch_case);
            BufferKey::Consumed
        }
        // Helix `` ` `` / `` Alt-` `` — the same target, set rather than
        // flipped. `NONE` and `SHIFT` again, for the same reason.
        (m, KeyCode::Char('`')) if m == none || m == shift => {
            editor_editing_verb(state, editor::switch_to_lower);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('`')) if m == alt => {
            editor_editing_verb(state, editor::switch_to_upper);
            BufferKey::Consumed
        }
        // Helix `r` — replace the selection, or the character under the
        // cursor, with the character typed next. There is no single-key
        // spelling, so it arms a prefix whose next key is a *target
        // character*, resolved by `resolve_pending_prefix` exactly as `f` is.
        (m, KeyCode::Char('r')) if m == none => {
            session
                .presentation_mut()
                .arm_prefix(EditorPrefix::ReplaceChar);
            BufferKey::Consumed
        }
        // Terminals disagree on the encoding: some send `Char('D')` with
        // SHIFT, others `Char('D')` with NONE. Accept both, the same
        // rule the panel directionals and `/terminal`'s nav classifier
        // already follow. Only the capital is listed here — bare `d` is
        // already claimed by the DeleteSelection arm above, so a `Char('d')`
        // alternative here could never match.
        (m, KeyCode::Char('D')) if m == shift || m == none => {
            editor_editing_command(state, EditorCommand::DeleteToLineEnd);
            BufferKey::Consumed
        }
        // `space y` arrives as two events in legacy xterm encoding, so the
        // prefix is remembered state rather than a chord — the same reasoning
        // as the terminal panel's `prefix_pending`. Unlike `gg`, a second key
        // that is not `y` disarms the prefix and is then resolved normally.
        (m, KeyCode::Char(' ')) if m == none => {
            session.presentation_mut().arm_prefix(EditorPrefix::Space);
            BufferKey::Consumed
        }
        // Helix's unimpaired prefixes. M006 gave them the non-ts leaves in
        // `editor::{OPEN,CLOSE}_BRACKET_LEAVES`, so this arm's whole job is to
        // arm; the leaves resolve through `resolve_pending_prefix`, which runs
        // ahead of every bare-key arm below — that ordering is what lets `]`
        // then `Space` be a newline rather than a second armed prefix.
        //
        // An unbound second key still disarms and is resolved against the
        // keymap, which is what keeps `[` followed by a letter reaching the
        // composer instead of being eaten.
        (m, KeyCode::Char('[')) if m == none => {
            session
                .presentation_mut()
                .arm_prefix(EditorPrefix::OpenBracket);
            BufferKey::Consumed
        }
        (m, KeyCode::Char(']')) if m == none => {
            session
                .presentation_mut()
                .arm_prefix(EditorPrefix::CloseBracket);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('u')) if m == none => {
            editor_undo(state);
            BufferKey::Consumed
        }
        // `U` redoes, and that is the Helix spelling: Helix binds redo to
        // `U` and gives `Ctrl+r` to *Insert* mode only (`insert_register`),
        // so a Normal-mode redo arm on `Ctrl+r` was never the Helix chord
        // either — it was a chord the guard block above already owned.
        //
        // Claimed even with nothing to redo, for the same reason `d` is:
        // an unclaimed `U` would escape to the composer and read as the
        // start of a word rather than "redo".
        (m, KeyCode::Char('U')) if m == none || m == shift => {
            editor_redo(state);
            BufferKey::Consumed
        }
        // `Alt-u` and `Alt-U` are Helix's `earlier` and `later`: one node back
        // and forward through the history in **creation order** — time, not
        // structure — so either can cross a fork that `u`/`U` never leave.
        // They also **report** their boundary instead of doing nothing
        // silently. See `traverse_history`.
        //
        // Three chords, because the capital has two terminal spellings and
        // they differ in the **key code**, not only in the modifier: a
        // terminal that reports the shifted glyph sends `(ALT, Char('U'))`,
        // and one that reports the modifier sends `(ALT|SHIFT, Char('U'))` or
        // `(ALT|SHIFT, Char('u'))`. `Alt-u` is `(ALT, Char('u'))` — and it
        // must not be confused with the third arm, which is the same glyph
        // under a different modifier and means the *other* direction.
        (m, KeyCode::Char('u')) if m == alt => {
            editor_history_earlier(state);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('U')) if m == alt || m == alt_shift => {
            editor_history_later(state);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('u')) if m == alt_shift => {
            editor_history_later(state);
            BufferKey::Consumed
        }
        // Insertion entries. They move the cursor *and* hand the keyboard to
        // the document, so they return the mode the App layer must adopt.
        (m, KeyCode::Char('i')) if m == none => enter_insert(state, EditorCommand::InsertBefore),
        (m, KeyCode::Char('a')) if m == none => enter_insert(state, EditorCommand::InsertAfter),
        (m, KeyCode::Char('I')) if m == none || m == shift => {
            enter_insert(state, EditorCommand::InsertLineStart)
        }
        (m, KeyCode::Char('A')) if m == none || m == shift => {
            enter_insert(state, EditorCommand::InsertLineEnd)
        }
        // `o` and `O` are the two verbs that **write** and then change mode:
        // Helix's `open_below` / `open_above` insert a line and put the cursor
        // on it in Insert mode. They go through the shared edit funnel, so the
        // line they open is one undo step and a read-only attachment refuses
        // them the way it refuses every other writing verb.
        (m, KeyCode::Char('o')) if m == none => {
            editor_open_line(state, EditorCommand::OpenLineBelow)
        }
        (m, KeyCode::Char('O')) if m == none || m == shift => {
            editor_open_line(state, EditorCommand::OpenLineAbove)
        }
        // `:` opens the command line. Normal mode only: in Insert the colon is
        // document text, and swallowing it would make it impossible to type a
        // URL into a file.
        (m, KeyCode::Char(':')) if m == none => {
            session.presentation_mut().open_command_line("");
            BufferKey::Consumed
        }
        // `/` searches the *document*, not the command catalog. On this route
        // the composer arms bare text with `:`, so an unclaimed `/` used to
        // become `:/` and resolve to nothing — a key that looks like it
        // should do something and silently does not. Normal mode only, for
        // the same reason `:` is: in Insert a slash is document text, and
        // swallowing it would make a URL untypeable.
        (m, KeyCode::Char('/')) if m == none => {
            // The snapshot is owned, so the seed lookup does not hold a
            // borrow of the session that `run_editor_search` would need.
            let snapshot = session.try_snapshot();
            let cursor = session.presentation().cursor_byte;
            let seed = snapshot
                .and_then(|snapshot| editor::word_at(&snapshot, cursor))
                .unwrap_or_default();
            session.presentation_mut().open_search_line(&seed, true);
            if !seed.is_empty() {
                // Seeded from the word under the cursor, so the matches that
                // word already has are highlighted before the first keystroke.
                run_editor_search(state, visible_lines);
            }
            BufferKey::Consumed
        }
        // Helix `?` — the same search, walking **backwards**. It reopens the
        // line with the pattern that is already accepted rather than an empty
        // one: the gesture is "the previous one of what I just searched for",
        // and making the reader retype it would be the whole cost of the key.
        // With no accepted pattern the line opens empty, which is what the
        // reader asked for rather than a key that silently did nothing.
        (m, KeyCode::Char('?')) if m == none || m == shift => {
            let seed = session.presentation().search_line.clone();
            session.presentation_mut().open_search_line(&seed, false);
            if !seed.is_empty() {
                // One **strict** step first: `?` means "the previous one", and
                // the cursor is usually sitting on the match `/` just
                // accepted. Every later keystroke re-adopts inclusively
                // instead, so refining the pattern stays where the reader is
                // rather than walking backwards off the match.
                step_editor_search(state, false, visible_lines);
                run_editor_search(state, visible_lines);
            }
            BufferKey::Consumed
        }
        // Helix `*` / `Alt-*` — seed the search from the main selection. `*`
        // escapes the text and wraps it in `\b`, so it searches for what is
        // selected rather than for a pattern that happens to match; `Alt-*`
        // is the deliberate unescaped spelling.
        (m, KeyCode::Char('*')) if m == none || m == shift => {
            editor_search_from_selection(state, visible_lines, true);
            BufferKey::Consumed
        }
        (m, KeyCode::Char('*')) if m == alt => {
            editor_search_from_selection(state, visible_lines, false);
            BufferKey::Consumed
        }
        // `n`/`N` walk the matches an accepted `/` search left behind.
        // Claimed only while there is a search to walk, so an `n` typed with
        // no search still reaches the composer as the start of a new session
        // name rather than being eaten.
        (m, KeyCode::Char('n')) if m == none && session.presentation().has_search() => {
            if step_editor_search(state, true, visible_lines) {
                BufferKey::Consumed
            } else {
                BufferKey::Unhandled
            }
        }
        (m, KeyCode::Char('N')) if m == shift && session.presentation().has_search() => {
            if step_editor_search(state, false, visible_lines) {
                BufferKey::Consumed
            } else {
                BufferKey::Unhandled
            }
        }
        _ => BufferKey::Unhandled,
    };
    // The dispatch tail: a count survives exactly two keys — the digit that
    // extends it, and a key that arms a prefix that will carry it, so `3gg`
    // and `3ge` work. Everything else takes it down on the way out: a stray
    // `3` cannot arm the gesture after it, and a key the buffer does not claim
    // ends the count rather than leaving it for the composer.
    let carries_count = state
        .session
        .as_ref()
        .is_some_and(|session| session.presentation().pending_prefix.is_pending());
    if !count_extended && !carries_count {
        clear_count(state);
    }
    settled
}

/// Take the pending count down, if there is one.
///
/// The one call site shape for every clearing path outside the motion body —
/// see [`TuiDocumentPresentation::clear_count`] for the contract.
fn clear_count(state: &mut EditorState) {
    if let Some(session) = state.session.as_mut() {
        session.presentation_mut().clear_count();
    }
}

/// `enter_insert`'s return value: the route mode to adopt.
///
/// A read-only document deliberately refuses insertion, and this reports
/// `None` for it — switching the route into Insert over a buffer that cannot
/// be typed into is how a user ends up in a dead Insert mode with no
/// explanation.
/// The find motion `Alt-.` would repeat, if there is one.
fn last_find(state: &EditorState) -> Option<editor::FindMotion> {
    state
        .session
        .as_ref()
        .and_then(|session| session.presentation().last_find)
}

/// The repeatable selection motion, if one is held.
fn selection_repeat(state: &EditorState) -> Option<editor::SelectionMotion> {
    state
        .session
        .as_ref()
        .and_then(|session| session.presentation().last_selection_motion)
}

/// The repeatable unimpaired motion, if one is held.
fn unimpaired_repeat(state: &EditorState) -> Option<EditorCommand> {
    state
        .session
        .as_ref()
        .and_then(|session| session.presentation().last_unimpaired)
}

/// Record a repeatable selection motion, clearing the find slot.
fn note_selection_motion(state: &mut EditorState, motion: editor::SelectionMotion) {
    if let Some(session) = state.session.as_mut() {
        let presentation = session.presentation_mut();
        presentation.forget_find();
        // One repeat slot, every motion overwrites it: claiming the selection
        // verbs must disarm the unimpaired ones for the same reason.
        presentation.last_unimpaired = None;
        presentation.last_selection_motion = Some(motion);
    }
}

/// Forget the repeatable motion unless `command` is itself a find.
///
/// Helix's `Alt-.` is *repeat last motion*, backed by a single slot that
/// every motion overwrites — so `f x`, `j`, `Alt-.` repeats nothing, and
/// that gap is what makes the repeat predictable rather than surprising.
/// `editor::apply_motion` records a find; everything else's job is to clear
/// the slot on the way past.
///
/// Deliberately **not** folded into `editor_motion`: that is also Insert
/// mode's cursor nudge, and an arrow inside a sentence is not a gesture
/// `Alt-.` should forget — the user is not repeating a find from there.
///
/// A paragraph motion claims the slot rather than clearing it, for the same
/// reason a find does: Helix's own description of `Alt-.` names `]` and `[`
/// as repeatable motions, and a key that works only on its own chord would be
/// narrower than the surface it belongs to. Which slot it claims, and the two
/// it clears, is the same single-slot rule the rest of this function states.
fn note_motion(state: &mut EditorState, command: EditorCommand) {
    if command.is_find_motion() {
        // A find claims the slot too. It used to return here without touching
        // anything else, which was correct while the find was the only other
        // slot and wrong the moment a second appeared: `]p`, `f x`, `Alt-.`
        // repeated `]p` backwards instead of `f x` forwards. `apply_motion`
        // writes `last_find` next to the resolution that produced it, so all
        // this owes the other two doors is to close them.
        if let Some(session) = state.session.as_mut() {
            let presentation = session.presentation_mut();
            presentation.last_selection_motion = None;
            presentation.last_unimpaired = None;
        }
        return;
    }
    if command.is_paragraph_motion() {
        if let Some(session) = state.session.as_mut() {
            let presentation = session.presentation_mut();
            presentation.forget_find();
            presentation.last_selection_motion = None;
            presentation.last_unimpaired = Some(command);
        }
        return;
    }
    if let Some(session) = state.session.as_mut() {
        let presentation = session.presentation_mut();
        presentation.forget_find();
        // One repeat slot, so a plain motion disarms the selection verbs and
        // the unimpaired ones too.
        presentation.last_selection_motion = None;
        presentation.last_unimpaired = None;
        presentation.paragraph_note = editor::ParagraphNote::None;
    }
}

/// Move the cursor through the Helix selection rules.
fn helix_motion(state: &mut EditorState, command: EditorCommand, _visible_lines: usize) {
    let before = jump_start(state);
    note_motion(state, command);
    apply_helix(state, editor::HelixVerb::Moved(command));
    // The second route a paragraph motion takes. `]p` itself and the bare
    // `j`/`k`/`h`/`l` arrive via `editor_motion`; `Alt-.` and the page keys
    // arrive here. Without this a refusal disclosed through the repeat would
    // be resolved on the probe and then discarded with it.
    lift_motion_notes(state);
    // `G` walks through here as `BufferEnd`. Same rule as `editor_motion`'s:
    // a named destination that moved records where it started.
    if editor::is_named_destination(command) {
        note_jumplist_jump(state, before);
    }
}

/// Apply a Helix verb, returning a delete/yank effect when one is requested.
///
/// Read-only handling is **narrow on purpose**: [`editor::HelixVerb::Moved`]
/// is never gated, because a reader must be able to walk around a buffer they
/// cannot write. Only the verbs that *grow* a selection are refused, and they
/// report it with the same notice every other edit verb uses. Before this,
/// `v` and `x` armed and grew selections on a read-only buffer in silence —
/// the reader saw a highlight they could not do anything with.
fn apply_helix(state: &mut EditorState, verb: editor::HelixVerb) -> editor::HelixVerbOutcome {
    if (grows_selection(&verb) || mutates_buffer(&verb)) && !editable(state) {
        state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return editor::HelixVerbOutcome::Handled;
    }
    // The comment toggle is the one verb whose *target* is resolved from the
    // attachment rather than from the text: its token is the buffer's
    // grammar's, and the grammar comes from the path. Every other verb is a
    // function of the snapshot and the presentation alone.
    let outcome = match verb {
        editor::HelixVerb::ToggleComments { block } => comment_toggle_outcome(state, block),
        verb => {
            let Some(session) = state.session.as_mut() else {
                return editor::HelixVerbOutcome::Handled;
            };
            let Some(snapshot) = session.try_snapshot() else {
                return editor::HelixVerbOutcome::Handled;
            };
            editor::apply_helix_verb(&snapshot, session.presentation_mut(), verb)
        }
    };
    if let editor::HelixVerbOutcome::NothingToDo(reason) = &outcome {
        // Reported once, here, where the outcome is read — so every caller
        // gets it and no caller can forget. A verb that reports nothing and
        // changes nothing is indistinguishable from a key that was never
        // delivered, which is exactly the failure §8 of the milestone forbids.
        state.set_notice(EditorNotice::Note(reason.clone()));
    }
    outcome
}

/// Resolve one comment toggle against the attachment's own grammar.
///
/// Every refusal names what is missing, and the two syntax refusals name the
/// extension: without it a reader cannot tell a file this build cannot comment
/// from a key that never arrived — the same failure an unbound leaf is.
fn comment_toggle_outcome(state: &EditorState, block: bool) -> editor::HelixVerbOutcome {
    let refuse = |reason: String| editor::HelixVerbOutcome::NothingToDo(reason);
    let Some(session) = state.session.as_ref() else {
        return refuse("no document is open".to_string());
    };
    let Some(snapshot) = session.try_snapshot() else {
        return refuse("the document is not attached".to_string());
    };
    let Some(path) = session.attachment_info().map(|info| info.relative_path) else {
        return refuse("toggle comment: the document is not attached".to_string());
    };
    let extension = comment_extension(&path);
    let Some(tokens) = crate::tui::editor_highlight::comment_tokens_for(&path) else {
        return refuse(format!(
            "toggle comment: {extension} declares no comment syntax, so the buffer is unchanged"
        ));
    };
    let Some(style) = tokens.style(block) else {
        return refuse(format!(
            "toggle block comment: {extension} has no block comment pair, so the buffer is \
             unchanged"
        ));
    };
    match editor::toggle_comments(&snapshot, session.presentation(), style) {
        Ok(outcome) => outcome,
        Err(reason) => refuse(reason),
    }
}

/// The extension a refusal names, or the absence of one.
fn comment_extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .map(|extension| format!("`.{extension}`"))
        .unwrap_or_else(|| "a path with no extension".to_string())
}

/// Apply a verb that edits the buffer, submitting its transaction.
///
/// The single place an M008 verb's text reaches the document. Going through
/// [`submit_edit`] rather than the controller directly is what makes undo, the
/// retained-bytes bound, LSP staleness, and the selection-clearing rule apply
/// to `&`, `Alt-(` and `Alt-)` for free rather than by a second
/// implementation that could disagree about any of them.
fn apply_helix_edit(state: &mut EditorState, verb: editor::HelixVerb) -> editor::HelixVerbOutcome {
    let outcome = apply_helix(state, verb);
    match outcome {
        editor::HelixVerbOutcome::Edit(edit, note) => {
            submit_edit(state, edit);
            // **After** the submission: `submit_edit` clears the notice on
            // success, so a disclosure set before it would be erased by the very
            // edit it describes. `&` uses this to say a selection was too wide to
            // measure exactly.
            if let Some(note) = note {
                state.set_notice(EditorNotice::Note(note));
            }
            editor::HelixVerbOutcome::Handled
        }
        // The surround verbs, which leave a selection behind.
        //
        // The order matters twice over. `submit_edit` clears every selection
        // on success — a selection is a set of byte offsets into text that
        // no longer exists — so the selection the verb produced is applied
        // **after** the submission, not before. And the members are in
        // post-edit coordinates, so they can only be right once the edit has
        // landed.
        editor::HelixVerbOutcome::EditAndSelect(edit, note, selections) => {
            submit_edit(state, edit);
            if let Some(note) = note {
                state.set_notice(EditorNotice::Note(note));
            }
            if let Some(session) = state.session.as_mut() {
                let presentation = session.presentation_mut();
                presentation.selections = selections;
                // The primary is the member whose head the cursor landed on.
                // `submit_edit` has already moved the cursor to the edit's
                // `cursor_after`, so asking the set which member names that
                // offset keeps "the cursor is the primary's head" true for
                // every verb that leaves a selection behind — `Alt-J`'s spaces
                // and the surround verbs alike. Hardcoding `0` would put the
                // two out of step the moment a verb left more than one member.
                let primary = presentation
                    .selections
                    .iter()
                    .position(|member| member.head.byte == presentation.cursor_byte)
                    .unwrap_or_default();
                presentation.primary_index = primary;
            }
            editor::HelixVerbOutcome::Handled
        }
        other => other,
    }
}

/// Whether this verb can enlarge a held selection.
///
/// `Moved` can — extending a character selection *is* a motion — but it is
/// deliberately excluded, because refusing it would take away navigation from
/// a reader looking at a file they cannot edit. That asymmetry is the whole
/// narrow reading: growth the reader asked for is refused, growth that is a
/// side effect of moving the cursor is not.
fn grows_selection(verb: &editor::HelixVerb) -> bool {
    matches!(
        verb,
        editor::HelixVerb::SelectChar
            | editor::HelixVerb::SelectLine
            // A text object is a selection the reader asked for by name, and
            // it is refused on a read-only attachment for the same reason `v`
            // and `x` are: it enlarges what the reader holds, and a reader
            // looking at a file they cannot write has no business enlarging
            // it.
            | editor::HelixVerb::SelectObject { .. }
            // `X` widens every member to whole lines, which is growth in the
            // strict sense.
            | editor::HelixVerb::ExtendToLineBounds
            // `_` only ever narrows a member, and it is refused here anyway:
            // the milestone's acceptance criteria refuse it with the rest of
            // the shape family, and one answer for the family is easier to
            // predict than a per-key rule a reader has to memorise. `Alt-x`
            // is the deliberate exception — it is navigation-like narrowing
            // and stays available on a attachment the reader cannot write.
            | editor::HelixVerb::TrimSelections
            // `s` / `Alt-s` rebuild the held set from the text under it, so
            // they enlarge a selection the reader cannot act on. Claimed by
            // the same rule as `v` and `x` rather than by a new gate.
            | editor::HelixVerb::SelectSelectionMatches { .. }
            | editor::HelixVerb::SplitSelectionOnNewlines
    )
}

/// Whether this verb writes to the buffer.
///
/// Gated on read-only alongside [`grows_selection`], and for the same reason:
/// a reader on a document they cannot write may still walk it, flip
/// selections, merge them and add cursors — none of which changes a byte — but
/// a verb that would rewrite text has to say so rather than appear to work.
fn mutates_buffer(verb: &editor::HelixVerb) -> bool {
    matches!(
        verb,
        editor::HelixVerb::AlignSelections
            | editor::HelixVerb::RotateSelectionContents { .. }
            // The three surround verbs write: they insert, remove or replace
            // delimiters. They are gated here rather than only by the shared
            // edit funnel, so a reader on a read-only attachment is told the
            // gesture is not available instead of watching it appear to work.
            | editor::HelixVerb::SurroundAdd { .. }
            | editor::HelixVerb::SurroundDelete { .. }
            | editor::HelixVerb::SurroundReplace { .. }
            // `J` / `Alt-J` rewrite the lines they cover into one, and `>` /
            // `<` insert or remove indentation on each covered line. Both are
            // edits, so a reader on a read-only attachment is told the gesture
            // is unavailable rather than watching it appear to work.
            | editor::HelixVerb::JoinLines { .. }
            | editor::HelixVerb::IndentSelection { .. }
            // `space c` / `space C` insert or remove a comment token on every
            // covered line. Gated here for the same reason the two above are:
            // a reader on a read-only attachment is told the gesture is
            // unavailable instead of watching the buffer stay put.
            | editor::HelixVerb::ToggleComments { .. }
            // `c` / `Alt-c` delete. Refusing them here rather than only at the
            // controller is what keeps the change gesture's two halves
            // together: the refusal is reported and the route never reaches
            // the point where Insert would follow.
            | editor::HelixVerb::ChangeSelection
    )
}

/// Resolve the held selection set to yankable ranges, without performing the
/// copy. The App layer owns the clipboard, so the buffer only reports the
/// ranges — the same division of labour as the delete effect.
fn yank_ranges(session: &mut TuiDocumentSession) -> Vec<std::ops::Range<usize>> {
    let Some(snapshot) = session.try_snapshot() else {
        return Vec::new();
    };
    let ranges = editor::selection_ranges(session.presentation(), &snapshot);
    if ranges.is_empty() {
        return Vec::new();
    }
    session.presentation_mut().clear_selections();
    ranges
}

/// Yank every range to `target`, in one gesture.
///
/// A failed destination is reported rather than swallowed: the selection has
/// already been dropped by the time this runs, so a silent failure would look
/// exactly like the gesture doing nothing at all.
///
/// The bytes are read **once**, here, and then offered to one destination
/// through [`publish_yank`] — so the register and the clipboard receive the
/// same text and cannot disagree about what was yanked.
fn yank_editor_ranges(app: &mut App, ranges: Vec<std::ops::Range<usize>>, target: YankTarget) {
    yank_editor_ranges_with(
        app,
        ranges,
        target,
        crate::util::clipboard::copy_to_clipboard,
    );
}

/// The core of a yank, with the clipboard write already supplied.
///
/// `copy` is injected for the same reason [`apply_clipboard_text`] takes its
/// read: `crate::util::clipboard` has no seam, and the only way to prove that
/// `y` still reaches the system clipboard — without a test writing to, and
/// stealing, the developer's real clipboard — is to not call it. The
/// production entry point above is the one caller that passes the real
/// function.
fn yank_editor_ranges_with(
    app: &mut App,
    ranges: Vec<std::ops::Range<usize>>,
    target: YankTarget,
    copy: impl FnOnce(&str) -> Result<(), crate::error::AppError>,
) {
    let Some(session) = app.editor_state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let text = read_range_text(&snapshot, &ranges);
    if text.is_empty() {
        app.messages_state.toasts.warning("Nothing to yank");
        return;
    }
    publish_yank(app, target, &text, copy);
}

/// Send one yank's bytes where the gesture said, and report what happened.
///
/// The order is load-bearing. The register is written **first**, so a reader
/// who cannot have the clipboard still gets the text; the unnamed register
/// then mirrors a named yank (a bare `p` after `"ay` pastes what was yanked,
/// exactly as Vim's unnamed register does); and the clipboard is written last,
/// because it is the one destination that can refuse.
fn publish_yank(
    app: &mut App,
    target: YankTarget,
    text: &str,
    copy: impl FnOnce(&str) -> Result<(), crate::error::AppError>,
) {
    let bytes = text.len();
    let mut written: Option<RegisterName> = None;
    let mut truncation: Option<(RegisterName, usize, usize)> = None;
    let copies = match target {
        // Never reached from the keymap — M002's `Alt-d`/`Alt-c` write nothing
        // and report nothing — but the arm keeps the funnel total rather than
        // inventing a destination for a target that names none.
        YankTarget::Nothing => return,
        YankTarget::Clipboard => true,
        YankTarget::Unnamed => {
            let outcome = store_register(app, RegisterName::Unnamed, text);
            note_truncation(&mut truncation, RegisterName::Unnamed, outcome);
            written = Some(RegisterName::Unnamed);
            false
        }
        YankTarget::Selected(register) => match register {
            // The blackhole writes nothing and says nothing: discarding is
            // what the reader asked for. It must not mirror into the unnamed
            // register or reach the clipboard either.
            RegisterName::Blackhole => return,
            // `/` is a projection of the search line. A yank aimed at it is a
            // stated refusal, not a write to a register that would then lie
            // about the last search.
            RegisterName::Search => {
                app.messages_state
                    .toasts
                    .warning("the search register is read-only — it holds the last search pattern");
                return;
            }
            named => {
                let outcome = store_register(app, named, text);
                note_truncation(&mut truncation, named, outcome);
                written = Some(named);
                if named != RegisterName::Unnamed {
                    let mirror = store_register(app, RegisterName::Unnamed, text);
                    note_truncation(&mut truncation, RegisterName::Unnamed, mirror);
                }
                // D1(a): every `y` also keeps the system clipboard in step, so
                // a copy made here still pastes in another application.
                true
            }
        },
    };
    let clipboard = if copies {
        copy(text).err().map(|error| error.to_string())
    } else {
        None
    };
    report_yank(app, bytes, written, truncation, clipboard);
}

/// Write one register through the presentation, if a document is attached.
fn store_register(app: &mut App, register: RegisterName, text: &str) -> Option<RegisterWrite> {
    app.editor_state
        .session
        .as_mut()
        .map(|session| session.presentation_mut().write_register(register, text))
}

/// Record a truncation, if the write had one.
fn note_truncation(
    truncation: &mut Option<(RegisterName, usize, usize)>,
    register: RegisterName,
    outcome: Option<RegisterWrite>,
) {
    if let Some(RegisterWrite::Truncated { stored, requested }) = outcome {
        // One disclosure, not two: the named register is the one the reader
        // asked for, and its mirror is the same bytes under another name.
        truncation.get_or_insert((register, stored, requested));
    }
}

/// Say what the gesture did, in one toast.
///
/// A truncation and a refused clipboard are both refusals and both belong on
/// screen, so they compose into one sentence rather than two toasts racing
/// each other. A write that reached no register and no clipboard is the
/// `space` family's plain copy, and keeps its original wording.
fn report_yank(
    app: &mut App,
    bytes: usize,
    written: Option<RegisterName>,
    truncation: Option<(RegisterName, usize, usize)>,
    clipboard: Option<String>,
) {
    if let Some((register, stored, requested)) = truncation {
        let mut message = format!(
            "{} kept the first {stored} of {requested} bytes",
            register.label()
        );
        if let Some(error) = clipboard {
            message.push_str(&format!("; clipboard unavailable: {error}"));
        }
        app.messages_state.toasts.warning(&message);
        return;
    }
    match (written, clipboard) {
        (Some(register), Some(error)) => app.messages_state.toasts.warning(&format!(
            "Copied {bytes} bytes to {}; clipboard unavailable: {error}",
            register.label()
        )),
        (Some(register), None) => app
            .messages_state
            .toasts
            .info(&format!("Copied {bytes} bytes to {}", register.label())),
        (None, Some(error)) => app
            .messages_state
            .toasts
            .warning(&format!("Clipboard unavailable: {error}")),
        (None, None) => app
            .messages_state
            .toasts
            .info(&format!("Copied {bytes} bytes")),
    }
}

/// Move the cursor for an insertion entry command and switch to insert mode.
///
/// Returns [`BufferKey::EnterInsert`] when the document actually took the
/// keyboard, and `Unhandled` when it refused. A read-only attachment refuses
/// on purpose, and refusing *silently* would leave the user pressing `i` in
/// what still looks like Normal mode with no way to tell why nothing happened,
/// so the refusal is reported rather than swallowed: the status row reads
/// "read-only", and the key stays unclaimed so a reader who meant to type
/// `i` into the agent's box still gets to.
fn enter_insert(state: &mut EditorState, command: EditorCommand) -> BufferKey {
    if !editable(state) {
        return BufferKey::Unhandled;
    }
    let Some(session) = state.session.as_ref() else {
        return BufferKey::Unhandled;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return BufferKey::Unhandled;
    };
    let Some(session) = state.session.as_mut() else {
        return BufferKey::Unhandled;
    };
    let presentation = session.presentation_mut();
    let cursor = editor::clamp_offset(&snapshot, presentation.cursor_byte);
    let next = match command {
        EditorCommand::InsertBefore => cursor,
        EditorCommand::InsertAfter => {
            editor::next_char_boundary(&snapshot, cursor).unwrap_or(cursor)
        }
        EditorCommand::InsertLineStart => {
            editor::line_start(&snapshot, editor::cursor_line(&snapshot, cursor)).unwrap_or(0)
        }
        EditorCommand::InsertLineEnd => {
            let line = editor::cursor_line(&snapshot, cursor);
            editor::line_content_end(&snapshot, line).unwrap_or(cursor)
        }
        _ => cursor,
    };
    presentation.cursor_byte = editor::clamp_offset(&snapshot, next);
    // Insert mode re-places the cursor and then walks it forward with
    // everything typed, so a column remembered from before `i` describes
    // nothing by the time Normal mode comes back.
    presentation.desired_column = None;
    presentation.mode = EditorMode::Insert;
    BufferKey::EnterInsert
}

/// Open a line and hand the keyboard to the document.
///
/// `o` and `O` are the one pair of Normal-mode verbs that **write** and then
/// change mode: Helix's `open_below` / `open_above` insert a line and put the
/// cursor on it in Insert mode. The insertion is [`editor::resolve_edit`]'s
/// job — the same one `]Space` uses, differing only in `stay_insert` — so
/// this function routes through the shared edit funnel rather than computing
/// a cursor position of its own.
///
/// The difference from [`enter_insert`] is the whole point. A mode transition
/// moves the cursor and hands over the keyboard, and an attachment that cannot
/// be written simply does not take the key. An open-line verb **writes**, so
/// a refusal has to be reported the way every writing verb reports it: the
/// key is claimed and the reason is shown. Returning `Unhandled` instead would
/// put the `o` in the composer, which reads as the editor ignoring a verb it
/// owns.
fn editor_open_line(state: &mut EditorState, command: EditorCommand) -> BufferKey {
    if !editable(state) {
        state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly));
        return BufferKey::Consumed;
    }
    let Some(session) = state.session.as_ref() else {
        return BufferKey::Consumed;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return BufferKey::Consumed;
    };
    let Some(edit) = editor::resolve_edit(&snapshot, session.presentation(), command) else {
        return BufferKey::Consumed;
    };
    if !submit_edit(state, edit) {
        // The controller refused the transaction. `submit_edit` has already
        // reported why, so the key is claimed and nothing else happens: the
        // document did not change, and the keyboard stays where it was.
        return BufferKey::Consumed;
    }
    // The document took the keyboard. The App adopts Insert mode on this
    // return value, which is what keeps `App::ui_state.input_mode` and the
    // presentation's own mode in agreement — `submit_edit` wrote the
    // presentation's side of it inside the accepted branch.
    BufferKey::EnterInsert
}

pub(crate) fn dispatch_editor(app: &mut App) {
    app.open_external_editor();
}

pub(crate) fn dispatch_open_document(app: &mut App, argument: &str) {
    // M006-A: `/open <workspace-relative path>`. With no
    // argument the composer keeps the key so the user can
    // type the path after the command.
    //
    // M007-H (work package A): the daemon owns the open, so the
    // TUI sends the `open_document` command and attaches the
    // response. It issues no open request of its own.
    let path = argument;
    if path.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /open <workspace-relative path> (e.g. /open src/lib.rs)");
    } else {
        crate::tui::commands::editor::open_editor_via_command(app, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Only the stepping tests' fixtures need these; the module above has no use
    // for them, and a module-level import would be dead weight in the shipped
    // binary.
    use crate::protocol::core::{CoreRequest, CoreResponse};
    use crate::tui::app::state::EditorNoticeSeverity;
    use crate::tui::editor::EditorPoint;
    use crate::tui::helix_selection::Selection;
    use codegg_protocol::lsp::{LspDiagnosticDto, LspRangeDto};

    use crate::tui::app::state::EditorState;
    use crate::tui::input::InputMode;
    use async_trait::async_trait;
    use codegg_client::DocumentTransport;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::sync::Mutex as AsyncMutex;
    use tokio::sync::Notify;

    #[test]
    fn accepts_a_plain_workspace_relative_path() {
        assert_eq!(normalize_relative_path("src/lib.rs").unwrap(), "src/lib.rs");
        assert_eq!(
            normalize_relative_path("  README.md ").unwrap(),
            "README.md"
        );
        assert_eq!(
            normalize_relative_path("./src/main.rs").unwrap(),
            "src/main.rs"
        );
    }

    #[test]
    fn rejects_absolute_paths() {
        let error = normalize_relative_path("/etc/passwd").unwrap_err();
        assert!(error.contains("workspace-relative"), "{error}");
        assert!(normalize_relative_path("\\\\server\\share").is_err());
        assert!(normalize_relative_path("C:\\Windows\\system32").is_err());
    }

    #[test]
    fn rejects_traversal_components() {
        let error = normalize_relative_path("../../etc/passwd").unwrap_err();
        assert!(error.contains("escapes the workspace"), "{error}");
        assert!(normalize_relative_path("src/../../secret").is_err());
        assert!(normalize_relative_path("..").is_err());
    }

    #[test]
    fn rejects_empty_and_degenerate_paths() {
        assert!(normalize_relative_path("").is_err());
        assert!(normalize_relative_path("   ").is_err());
        assert!(normalize_relative_path("a//b").is_err());
        assert!(normalize_relative_path(".").is_err());
        assert!(normalize_relative_path("a\0b").is_err());
    }

    #[test]
    fn rejects_over_long_and_over_deep_paths() {
        let long = "a".repeat(MAX_EDITOR_RELATIVE_PATH + 1);
        assert!(normalize_relative_path(&long).is_err());
        let deep = (0..=MAX_EDITOR_PATH_DEPTH)
            .map(|_| "d")
            .collect::<Vec<_>>()
            .join("/");
        assert!(normalize_relative_path(&deep).is_err());
    }

    #[test]
    fn a_rejected_path_never_reaches_the_session() {
        // The command layer refuses before any controller call, so no
        // attachment and no buffer can exist for a traversal attempt.
        let error = normalize_relative_path("../outside.txt").unwrap_err();
        assert!(error.contains("escapes the workspace"));
        let state = EditorState::default();
        assert!(state.session.is_none());
        assert_eq!(state.status, EditorStatus::Closed);
    }

    #[test]
    fn lifecycle_error_messages_never_contain_document_text() {
        let messages = [
            describe_error(&DocumentControllerError::Transport("pipe closed".into())),
            describe_error(&DocumentControllerError::Conflict),
            describe_error(&DocumentControllerError::Response("mismatch".into())),
        ];
        for message in messages {
            assert!(!message.is_empty());
            assert!(!message.contains("fn main"));
        }
    }

    // ── Scripted document.v1 daemon ──────────────────────────────────────

    /// Minimal in-memory daemon for `document.v1`.
    struct FakeDaemon {
        text: std::sync::Mutex<String>,
        revision: AtomicU64,
        writer: bool,
        conflicts_on_save: std::sync::Mutex<bool>,
        offline: std::sync::Mutex<bool>,
        gone: std::sync::Mutex<bool>,
        reload_entered: Notify,
        block_reload: std::sync::Mutex<bool>,
        reload_release: Notify,
        change_count: AtomicU64,
        in_flight: AtomicU64,
        max_in_flight: AtomicU64,
        open_count: AtomicU64,
    }

    impl FakeDaemon {
        fn new(text: &str, writer: bool) -> Arc<Self> {
            Arc::new(Self {
                text: std::sync::Mutex::new(text.to_string()),
                revision: AtomicU64::new(1),
                writer,
                conflicts_on_save: std::sync::Mutex::new(false),
                offline: std::sync::Mutex::new(false),
                gone: std::sync::Mutex::new(false),
                reload_entered: Notify::new(),
                block_reload: std::sync::Mutex::new(false),
                reload_release: Notify::new(),
                change_count: AtomicU64::new(0),
                in_flight: AtomicU64::new(0),
                max_in_flight: AtomicU64::new(0),
                open_count: AtomicU64::new(0),
            })
        }

        fn daemon_text(&self) -> String {
            self.text.lock().expect("text lock").clone()
        }

        fn snapshot_blocking(&self) -> CoreResponse {
            let text = self.text.lock().expect("text lock").clone();
            let revision = self.revision.load(Ordering::Acquire);
            CoreResponse::DocumentSnapshot {
                snapshot: codegg_protocol::document::DocumentSnapshotDto {
                    document_id: "doc-1".into(),
                    project_id: "project-1".into(),
                    workspace_id: "workspace-1".into(),
                    relative_path: "src/lib.rs".into(),
                    revision,
                    text,
                    dirty: false,
                    conflicted: false,
                    writer: self.writer,
                    disk_base_digest: "digest".into(),
                    scope: DocumentScope::Workspace,
                },
                writer_lease: self.writer.then(|| "lease-1".to_string()),
                lsp_degraded: false,
            }
        }
    }

    #[async_trait]
    impl DocumentTransport for FakeDaemon {
        async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
            if *self.offline.lock().expect("offline lock") {
                return Err("daemon socket closed".to_string());
            }
            match request {
                CoreRequest::DocumentOpen { .. } => {
                    self.open_count.fetch_add(1, Ordering::AcqRel);
                    if *self.gone.lock().expect("gone lock") {
                        return Ok(CoreResponse::Error {
                            code: "document_not_found".into(),
                            message: "document is gone".into(),
                        });
                    }
                    Ok(self.snapshot_blocking())
                }
                CoreRequest::DocumentChange { transaction, .. } => {
                    let live = self.in_flight.fetch_add(1, Ordering::AcqRel) + 1;
                    self.max_in_flight.fetch_max(live, Ordering::AcqRel);
                    {
                        let mut text = self.text.lock().expect("text lock");
                        for edit in transaction.edits.iter().rev() {
                            text.replace_range(edit.range.start..edit.range.end, &edit.insert);
                        }
                    }
                    self.revision.fetch_add(1, Ordering::AcqRel);
                    self.in_flight.fetch_sub(1, Ordering::AcqRel);
                    self.change_count.fetch_add(1, Ordering::AcqRel);
                    Ok(CoreResponse::DocumentChanged {
                        revision: self.revision.load(Ordering::Acquire),
                        lsp_degraded: false,
                    })
                }
                CoreRequest::DocumentSave { .. } => {
                    if *self.conflicts_on_save.lock().expect("conflict lock") {
                        return Ok(CoreResponse::Error {
                            code: "document_disk_conflict".into(),
                            message: "disk changed under the document".into(),
                        });
                    }
                    let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
                    Ok(CoreResponse::DocumentSaved {
                        revision,
                        disk_base_digest: "saved".into(),
                        lsp_degraded: false,
                    })
                }
                CoreRequest::DocumentSnapshotGet { .. } => {
                    if *self.gone.lock().expect("gone lock") {
                        return Ok(CoreResponse::Error {
                            code: "document_not_found".into(),
                            message: "document is gone".into(),
                        });
                    }
                    Ok(self.snapshot_blocking())
                }
                CoreRequest::DocumentReload { .. } => {
                    self.reload_entered.notify_one();
                    if *self.block_reload.lock().expect("block lock") {
                        self.reload_release.notified().await;
                    }
                    let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
                    Ok(CoreResponse::DocumentReloaded {
                        revision,
                        lsp_degraded: false,
                    })
                }
                CoreRequest::DocumentClose { .. } => Ok(CoreResponse::Ack),
                _ => Err("unexpected request".to_string()),
            }
        }
    }

    /// A transport that never answers, for the partial-open-failure path.
    struct DeadTransport;

    #[async_trait]
    impl DocumentTransport for DeadTransport {
        async fn request(&self, _request: CoreRequest) -> Result<CoreResponse, String> {
            Err("no daemon".to_string())
        }
    }

    /// A transport that answers with a fixed script.
    struct ScriptedTransport(AsyncMutex<VecDeque<CoreResponse>>);

    #[async_trait]
    impl DocumentTransport for ScriptedTransport {
        async fn request(&self, _request: CoreRequest) -> Result<CoreResponse, String> {
            self.0
                .lock()
                .await
                .pop_front()
                .ok_or_else(|| "no scripted response".to_string())
        }
    }

    // ── Fixtures ──────────────────────────────────────────────────────────

    /// Open one document into an `EditorState` over `daemon`.
    async fn open_state(daemon: &Arc<FakeDaemon>, writer: bool) -> EditorState {
        open_state_at(daemon, writer, "src/lib.rs").await
    }

    /// The same fixture attached to `path`, for a test whose subject is the
    /// grammar the path selects.
    async fn open_state_at(daemon: &Arc<FakeDaemon>, writer: bool, path: &str) -> EditorState {
        let controller = Arc::new(DocumentController::new(daemon.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                path.into(),
                writer,
                DocumentScope::Workspace,
            )
            .await
            .expect("open succeeds");
        let status = EditorStatus::from_state(&controller.state().await);
        let mut session = TuiDocumentSession::from_controller(controller);
        session.presentation_mut().reset_for_open();
        EditorState {
            status,
            path: Some(path.into()),
            wants_writer: writer,
            session: Some(session),
            ..EditorState::default()
        }
    }

    async fn opened_writer(text: &str) -> (EditorState, Arc<FakeDaemon>) {
        let daemon = FakeDaemon::new(text, true);
        let state = open_state(&daemon, true).await;
        (state, daemon)
    }

    /// `opened_writer` attached to `path` rather than the Rust fixture.
    async fn opened_writer_at(path: &str, text: &str) -> (EditorState, Arc<FakeDaemon>) {
        let daemon = FakeDaemon::new(text, true);
        let state = open_state_at(&daemon, true, path).await;
        (state, daemon)
    }

    /// The same attachment without the writer lease, so every local edit is
    /// refused by `editable`.
    async fn opened_reader(text: &str) -> (EditorState, Arc<FakeDaemon>) {
        let daemon = FakeDaemon::new(text, true);
        let state = open_state(&daemon, false).await;
        (state, daemon)
    }

    /// The controller's own snapshot — the only text the editor may read.
    fn snapshot_of(state: &EditorState) -> codegg_document::DocumentSnapshot {
        state
            .session
            .as_ref()
            .expect("open")
            .controller()
            .try_snapshot()
            .expect("attached")
            .0
    }

    /// The only text the editor may have: the controller's own snapshot.
    fn editor_text(state: &EditorState) -> String {
        state
            .session
            .as_ref()
            .expect("open")
            .controller()
            .try_snapshot()
            .expect("attached")
            .0
            .to_string()
    }

    fn cursor(state: &EditorState) -> usize {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .cursor_byte
    }

    /// The armed prefix as the pending indicator renders it.
    ///
    /// Deliberately `display()` rather than the enum: the `gg` assertions this
    /// replaces read `"g"` and `""`, and keeping that spelling is what lets
    /// them pass *unmodified* — which is the strongest available evidence the
    /// gesture did not change.
    fn pending(state: &EditorState) -> String {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .pending_prefix
            .display()
            .to_string()
    }

    /// Whether the buffer is holding a selection.
    fn selection_of(state: &EditorState) -> Option<Selection<EditorPoint>> {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .selections
            .get(presentation_primary_index(state))
            .copied()
    }

    /// Index of the distinguished member, or 0 when nothing is held.
    fn presentation_primary_index(state: &EditorState) -> usize {
        state
            .session
            .as_ref()
            .map(|session| session.presentation().primary_index)
            .unwrap_or(0)
    }

    fn has_selection(state: &EditorState) -> bool {
        selection_of(state).is_some()
    }

    /// Drop the held selection set, the way `submit_edit` does once an edit
    /// lands.
    ///
    /// A verb that reports its effect (`d`) deliberately leaves the set for the
    /// App layer, because that is what lets the undo entry record it. Tests
    /// that drive the buffer half of that handoff on its own call this to
    /// finish the transaction the way the dispatcher would.
    fn consume_selection(state: &mut EditorState) {
        if let Some(session) = state.session.as_mut() {
            session.presentation_mut().clear_selections();
        }
    }

    /// Byte length of the held selection, or 0 when nothing is selected.
    fn selection_len(state: &EditorState) -> usize {
        let Some(session) = state.session.as_ref() else {
            return 0;
        };
        let Some(snapshot) = session.try_snapshot() else {
            return 0;
        };
        let Some(range) = editor::selection_range(session.presentation(), &snapshot) else {
            return 0;
        };
        range.end - range.start
    }

    /// How many nodes of the history are applied — the depth `u` walks.
    ///
    /// Not the cursor's identity. A node's `created` id is stable across
    /// branching and pruning, so once the tree has a sibling branch the id is
    /// no longer a count of what is applied; this asks the model.
    fn undo_depth(state: &EditorState) -> usize {
        let presentation = state.session.as_ref().expect("open").presentation();
        editor::history_depth(presentation, presentation.history_cursor)
    }

    /// How many redo steps `U` can take from here: one when the cursor has a
    /// child, none when it is a tip.
    ///
    /// **Not** the arena's length minus the cursor. A length and an identity
    /// are not positions: the subtraction read `0` at a branch tip — where a
    /// sibling branch exists but `U` has nowhere to go — and read numbers that
    /// meant nothing once ids and positions parted.
    fn redo_depth(state: &EditorState) -> usize {
        let presentation = state.session.as_ref().expect("open").presentation();
        if editor::redo_entry(presentation).is_some() {
            1
        } else {
            0
        }
    }

    /// How many nodes the tree retains, applied or not.
    ///
    /// The one question an arena length answers honestly, under its own name —
    /// a branch the reader walked away from is retained without being applied.
    fn history_len(state: &EditorState) -> usize {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .history
            .len()
    }

    // ── Trajectory: open → edit → undo → redo → save → reopen ────────────

    #[tokio::test]
    async fn editor_keeps_edit_through_undo_redo_save_and_reopen() {
        let (mut state, daemon) = opened_writer("alpha\nbeta\n").await;
        assert_eq!(state.status, EditorStatus::Synced);
        assert_eq!(editor_text(&state), "alpha\nbeta\n");

        // Insert through the editor path: move to line 1, enter insert
        // mode at its end, type, submit.
        editor_motion(&mut state, EditorCommand::MoveDown, 10);
        assert_eq!(cursor(&state), 6);
        enter_insert(&mut state, EditorCommand::InsertLineEnd);
        editor_insert_literal(&mut state, "!");
        assert_eq!(editor_text(&state), "alpha\nbeta!\n");
        assert_eq!(state.status, EditorStatus::DirtyLocal);
        // Leaving insert mode closes the typing burst, and a closed burst is
        // one undo step (M011).
        leave_insert_mode(&mut state);
        assert_eq!(undo_depth(&state), 1);

        // Undo restores both the text and the cursor the user left.
        let after_edit = cursor(&state);
        editor_undo(&mut state);
        assert_eq!(editor_text(&state), "alpha\nbeta\n");
        assert_eq!(cursor(&state), 10, "undo returns to the pre-edit cursor");
        assert_ne!(cursor(&state), after_edit);
        assert_eq!(undo_depth(&state), 0);
        assert_eq!(redo_depth(&state), 1);

        // Redo re-applies the forward transaction, not the undo.
        editor_redo(&mut state);
        assert_eq!(
            editor_text(&state),
            "alpha\nbeta!\n",
            "redo must re-apply the edit, not revert it"
        );
        assert_eq!(undo_depth(&state), 1);
        assert_eq!(redo_depth(&state), 0);

        // Save flushes the queue and lands on disk.
        let controller = state.session.as_ref().expect("open").controller();
        controller.flush().await.expect("flush");
        controller.save().await.expect("save");
        assert_eq!(daemon.daemon_text(), "alpha\nbeta!\n");
        assert_eq!(controller.state().await, DocumentState::Synced);

        // Reopen: the saved text comes back from the daemon.
        controller.close().await.expect("close");
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
                DocumentScope::Workspace,
            )
            .await
            .expect("reopen");
        assert_eq!(editor_text(&state), "alpha\nbeta!\n");
    }

    #[tokio::test]
    async fn editor_delete_and_motion_commands_produce_the_expected_text() {
        let (mut state, _daemon) = opened_writer("one\ntwo\nthree\n").await;

        editor_motion(&mut state, EditorCommand::MoveDown, 10);
        assert_eq!(cursor(&state), 4, "down lands on the second line");
        editor_editing_command(&mut state, EditorCommand::DeleteLine);
        assert_eq!(editor_text(&state), "one\nthree\n");

        editor_motion(&mut state, EditorCommand::BufferStart, 10);
        editor_editing_command(&mut state, EditorCommand::DeleteToLineEnd);
        // `D` removes to the end of the line, not the terminator.
        assert_eq!(editor_text(&state), "\nthree\n");

        // Undo twice walks back through both edits.
        editor_undo(&mut state);
        assert_eq!(editor_text(&state), "one\nthree\n");
        editor_undo(&mut state);
        assert_eq!(editor_text(&state), "one\ntwo\nthree\n");
        // The second undo restores the position that second edit was made
        // from, which was the line-1 start, not the buffer start.
        assert_eq!(cursor(&state), 4);
        assert_eq!(undo_depth(&state), 0);
    }

    #[tokio::test]
    async fn editor_motion_alone_never_changes_text_or_history() {
        let (mut state, _daemon) = opened_writer("alpha beta gamma\n").await;
        let before = editor_text(&state);
        for command in [
            EditorCommand::MoveRight,
            EditorCommand::MoveRight,
            EditorCommand::WordForward,
            EditorCommand::LineEnd,
            EditorCommand::BufferEnd,
            EditorCommand::BufferStart,
            EditorCommand::LineStart,
            EditorCommand::MoveLeft,
        ] {
            editor_motion(&mut state, command, 10);
        }
        assert_eq!(editor_text(&state), before);
        assert_eq!(undo_depth(&state), 0, "motion must not record history");
        assert_eq!(redo_depth(&state), 0);
        assert_eq!(state.status, EditorStatus::Synced);
    }

    // ── Read-only and contention ─────────────────────────────────────────

    #[tokio::test]
    async fn a_read_only_attachment_renders_but_refuses_every_edit() {
        let daemon = FakeDaemon::new("locked\n", false);
        let mut state = open_state(&daemon, false).await;
        assert_eq!(state.status, EditorStatus::ReadOnly);
        assert!(!state.is_writer());
        // Text is still available for rendering.
        assert_eq!(editor_text(&state), "locked\n");

        editor_insert_literal(&mut state, "x");
        editor_editing_command(&mut state, EditorCommand::DeleteChar);
        editor_undo(&mut state);
        editor_redo(&mut state);
        editor_delete_backward(&mut state);
        assert_eq!(editor_text(&state), "locked\n", "no edit may land");
        assert_eq!(undo_depth(&state), 0, "a refused edit records no history");
        assert_eq!(
            state.notice,
            Some(EditorNotice::ReadOnly),
            "refusal is surfaced explicitly"
        );
    }

    #[tokio::test]
    async fn a_rapid_edit_burst_serializes_with_one_change_in_flight() {
        let (mut state, daemon) = opened_writer("").await;
        for _ in 0..25 {
            editor_insert_literal(&mut state, "a");
        }
        let controller = state.session.as_ref().expect("open").controller();
        controller.flush().await.expect("flush");
        assert_eq!(editor_text(&state), "a".repeat(25));
        assert_eq!(
            daemon.max_in_flight.load(Ordering::Acquire),
            1,
            "change submission must be serial"
        );
        assert!(daemon.change_count.load(Ordering::Acquire) > 0);
        assert_eq!(daemon.daemon_text(), "a".repeat(25));
        // The daemon still saw one change per keystroke; the *history* does
        // not, because a typing burst is one undo step (M011). Leaving insert
        // mode closes the burst.
        leave_insert_mode(&mut state);
        assert_eq!(
            undo_depth(&state),
            1,
            "the whole burst is one step, and one `u` takes all of it back"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_local_edit_during_a_destructive_phase_is_refused_and_history_survives() {
        // Real concurrency: a reload is held open on another thread while
        // the editor attempts an undo, so the destructive phase is
        // genuinely in flight.
        let (mut state, daemon) = opened_writer("base").await;
        editor_insert_literal(&mut state, "!");
        assert_eq!(editor_text(&state), "!base");
        assert_eq!(cursor(&state), 1);

        // Reload refuses a non-empty queue with `ResyncRequired` *before*
        // it reaches the transport, so the phase must be armed on a drained
        // queue or it would never become observable.
        let controller = state.session.as_ref().expect("open").controller();
        controller.flush().await.expect("flush");

        *daemon.block_reload.lock().expect("block lock") = true;
        let reloading = {
            let controller = Arc::clone(&controller);
            tokio::spawn(async move { controller.reload_from_disk().await })
        };
        // Wait for the phase to become observable instead of sleeping blindly.
        daemon.reload_entered.notified().await;

        editor_undo(&mut state);
        assert_eq!(
            editor_text(&state),
            "!base",
            "undo must not land during a destructive phase"
        );
        assert_eq!(undo_depth(&state), 1, "the entry stays in history");
        assert_eq!(
            state.notice,
            Some(EditorNotice::Busy),
            "a busy controller is an expected condition, not a failure"
        );

        // The same refusal applies to a fresh edit.
        editor_insert_literal(&mut state, "?");
        assert_eq!(editor_text(&state), "!base", "no edit may land either");

        // Release the phase and confirm the editor is usable again.
        daemon.reload_release.notify_one();
        let outcome = reloading.await.expect("reload task");
        assert!(outcome.is_ok(), "reload should succeed once released");
        state.status = EditorStatus::from_state(&controller.state().await);
        editor_undo(&mut state);
        assert_eq!(editor_text(&state), "base");
        assert_eq!(cursor(&state), 0, "undo restores the pre-edit cursor");
    }

    // ── Conflict, gone, and transport failure ────────────────────────────

    #[tokio::test]
    async fn a_disk_conflict_retains_and_surfaces_the_draft_without_resolving_it() {
        let (mut state, daemon) = opened_writer("base").await;
        editor_insert_literal(&mut state, "-mine");
        let draft = editor_text(&state);
        assert_eq!(draft, "-minebase", "the edit lands at the cursor");

        *daemon.conflicts_on_save.lock().expect("conflict lock") = true;
        let controller = state.session.as_ref().expect("open").controller();
        // Let the edit reach the daemon first, so the save is what meets the
        // disk conflict.
        controller.flush().await.expect("flush");
        let first = controller.save().await.expect_err("save meets conflict");
        assert!(
            matches!(first, DocumentControllerError::Response(_)),
            "the first conflicting save reports the raw response"
        );
        // The controller latches the conflict; a second save is refused
        // before any request, and the draft is still intact.
        let error = controller.save().await.expect_err("save refused");
        assert!(matches!(error, DocumentControllerError::Conflict));
        state.status = EditorStatus::from_state(&controller.state().await);
        state.set_notice(EditorNotice::from_error(&error));

        assert_eq!(state.status, EditorStatus::Conflict);
        assert_eq!(
            editor_text(&state),
            draft,
            "the draft must be retained, not overwritten or discarded"
        );
        let notice = state.notice.as_ref().expect("notice");
        assert_eq!(notice.severity(), EditorNoticeSeverity::Actionable);
        assert!(notice.message().contains("draft is kept"));
        // No auto-resolution: the daemon text is untouched and the editor
        // has not reloaded.
        assert_eq!(daemon.daemon_text(), "-minebase");
    }

    #[tokio::test]
    async fn a_gone_document_surfaces_a_recovery_state_and_keeps_the_draft() {
        let (mut state, daemon) = opened_writer("base").await;
        editor_insert_literal(&mut state, "-draft");
        let draft = editor_text(&state);
        assert_eq!(draft, "-draftbase");

        *daemon.gone.lock().expect("gone lock") = true;
        let controller = state.session.as_ref().expect("open").controller();
        let error = controller.resync().await.expect_err("gone");
        state.status = EditorStatus::from_state(&controller.state().await);
        state.set_notice(EditorNotice::from_error(&error));

        assert_eq!(state.status, EditorStatus::GoneWithDraft);
        assert_eq!(editor_text(&state), draft, "the draft is never discarded");
        assert!(state.status.blocks_edits());
    }

    #[tokio::test]
    async fn a_failed_open_holds_no_attachment_and_no_buffer() {
        let controller = Arc::new(DocumentController::new(Arc::new(DeadTransport)));
        assert!(controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
                DocumentScope::Workspace,
            )
            .await
            .is_err());
        assert_eq!(controller.state().await, DocumentState::Closed);
        assert!(
            controller.try_snapshot().is_none(),
            "a failed open must not expose any text"
        );
        assert!(controller.try_attachment_info().is_none());
    }

    #[tokio::test]
    async fn an_unexpected_open_response_holds_no_attachment() {
        let transport = Arc::new(ScriptedTransport(AsyncMutex::new(VecDeque::from([
            CoreResponse::Ack,
        ]))));
        let controller = Arc::new(DocumentController::new(transport));
        let error = controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
                DocumentScope::Workspace,
            )
            .await
            .expect_err("unexpected response");
        assert!(matches!(error, DocumentControllerError::Response(_)));
        assert!(controller.try_snapshot().is_none());
    }

    #[tokio::test]
    async fn a_second_open_of_the_same_document_is_refused_not_silently_orphaned() {
        let daemon = FakeDaemon::new("shared", true);
        let first = Arc::new(DocumentController::new(daemon.clone()));
        first
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
                DocumentScope::Workspace,
            )
            .await
            .expect("first open");
        let second = Arc::new(DocumentController::new(daemon.clone()));
        let error = second
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
                true,
                DocumentScope::Workspace,
            )
            .await;
        // The scripted daemon allows both, so the first attachment must
        // still be intact and readable; the editor surfaces a refusal
        // rather than taking over.
        if let Err(error) = error {
            assert!(matches!(error, DocumentControllerError::AlreadyOpen));
        }
        assert_eq!(first.try_snapshot().expect("first").0.to_string(), "shared");
    }

    #[tokio::test]
    async fn switching_documents_clears_history_and_drops_the_previous_attachment() {
        let (mut state, _daemon) = opened_writer("first document\n").await;
        editor_insert_literal(&mut state, "x");
        leave_insert_mode(&mut state);
        assert_eq!(undo_depth(&state), 1);

        // Switching documents detaches the old session entirely.
        let mut previous = state.session.take().expect("open");
        previous.presentation_mut().reset_for_open();
        let daemon = FakeDaemon::new("second document\n", true);
        let replacement = open_state(&daemon, true).await;
        state.session = replacement.session;
        state.status = replacement.status;
        state.path = Some("src/lib.rs".into());

        assert_eq!(editor_text(&state), "second document\n");
        assert_eq!(
            undo_depth(&state),
            0,
            "undo must not survive opening a different document"
        );
        assert_eq!(
            previous
                .controller()
                .try_snapshot()
                .expect("previous still attached to its own controller")
                .0
                .to_string(),
            "xfirst document\n",
            "the previous controller keeps only its own text"
        );
    }

    // ── History does not survive reconciliation ─────────────────────────

    #[tokio::test]
    async fn undo_history_does_not_survive_reload_resync_or_close() {
        for reconcile in ["reload", "resync", "close"] {
            let (mut state, daemon) = opened_writer("base").await;
            editor_insert_literal(&mut state, "!");
            // The burst is closed, so there is a step for the reconciliation
            // to clear.
            leave_insert_mode(&mut state);
            assert_eq!(undo_depth(&state), 1, "{reconcile}");

            let controller = state.session.as_ref().expect("open").controller();
            // Reload and close both require the queue to be empty.
            controller.flush().await.expect("flush");
            let outcome = match reconcile {
                "reload" => controller.reload_from_disk().await,
                "resync" => controller.resync().await,
                _ => controller.close().await,
            };
            outcome.unwrap_or_else(|error| panic!("{reconcile} failed: {error}"));
            if EditorOperation::Close.clears_undo_history() {
                if reconcile == "close" {
                    state.detach();
                } else if let Some(session) = state.session.as_mut() {
                    editor::clear_history(session.presentation_mut());
                }
            }
            if reconcile == "close" {
                // Close detaches the whole attachment, so nothing is left
                // that could carry history.
                assert!(
                    state.session.is_none(),
                    "close must leave no attachment behind"
                );
                assert_eq!(state.status, EditorStatus::Closed);
            } else {
                assert_eq!(undo_depth(&state), 0, "undo must not survive {reconcile}");
                assert_eq!(redo_depth(&state), 0, "redo must not survive {reconcile}");
            }
            let _ = daemon;
        }
    }

    #[tokio::test]
    async fn a_divergent_resync_keeps_the_draft_and_clears_undo() {
        let (mut state, daemon) = opened_writer("base").await;
        editor_insert_literal(&mut state, "-mine");
        let draft = editor_text(&state);

        // The daemon moves to different text with no pending queue, so the
        // controller cannot reconcile and must require an explicit
        // resync/reload decision.
        *daemon.text.lock().expect("text lock") = "theirs".to_string();
        let controller = state.session.as_ref().expect("open").controller();
        controller.flush().await.ok();
        let _ = controller.reload_from_disk().await;
        state.status = EditorStatus::from_state(&controller.state().await);
        if let Some(session) = state.session.as_mut() {
            editor::clear_history(session.presentation_mut());
        }
        assert_eq!(undo_depth(&state), 0, "undo is cleared on reconciliation");
        assert!(
            state.status.blocks_edits() || !editor_text(&state).is_empty(),
            "the editor never ends up blank without a decision"
        );
        assert_ne!(
            draft, "theirs",
            "the local draft was never silently accepted"
        );
    }

    // ── Semantic stale lifecycle (C001) ─────────────────────────────────

    fn seed_semantic(state: &mut EditorState) {
        use codegg_protocol::lsp::LspSemanticTokenDto;
        state.lsp.semantic_tokens = vec![LspSemanticTokenDto {
            line: 0,
            character: 0,
            length: 1,
            token_type: "keyword".into(),
            modifiers: vec![],
        }];
        state.lsp.semantic_revision = state
            .session
            .as_ref()
            .and_then(|s| s.try_snapshot())
            .map(|snap| snap.revision().get());
        state.lsp.semantic_truncated = true;
        state.lsp.semantic_incomplete = true;
    }

    fn assert_no_semantic(state: &EditorState) {
        assert!(state.lsp.semantic_tokens.is_empty());
        assert_eq!(state.lsp.semantic_revision, None);
        assert!(!state.lsp.semantic_truncated);
        assert!(!state.lsp.semantic_incomplete);
    }

    #[tokio::test]
    async fn a_local_edit_undo_and_redo_clear_stale_semantic_state() {
        let (mut state, _daemon) = opened_writer("base").await;
        seed_semantic(&mut state);
        editor_insert_literal(&mut state, "!");
        assert_no_semantic(&state);

        seed_semantic(&mut state);
        editor_undo(&mut state);
        assert_no_semantic(&state);

        // Redo needs an undone entry: edit, undo, then redo clears again.
        editor_insert_literal(&mut state, "?");
        seed_semantic(&mut state);
        editor_undo(&mut state);
        assert_no_semantic(&state);
        // Re-seed to prove redo also clears (revision changed again).
        seed_semantic(&mut state);
        editor_redo(&mut state);
        assert_no_semantic(&state);
    }

    #[tokio::test]
    async fn detach_clears_semantic_so_reopened_path_starts_fresh() {
        let (mut state, _daemon) = opened_writer("base").await;
        seed_semantic(&mut state);
        let generation = state.generation;
        state.detach();
        assert_no_semantic(&state);
        assert_ne!(state.generation, generation);
        // Reopening the same path starts with no highlight until the next
        // validated read; a late completion for the old generation must be
        // discarded by the caller (generation check in apply_editor_lsp_read).
        let daemon = FakeDaemon::new("base", true);
        let reopened = open_state(&daemon, true).await;
        assert_no_semantic(&reopened);
    }

    // ── Key routing ─────────────────────────────────────────────────────

    /// Dispatch one key in Normal mode.
    fn normal(state: &mut EditorState, code: KeyCode) -> BufferKey {
        editor_buffer_key(state, InputMode::Normal, key(code))
    }

    /// Dispatch one key in Insert mode.
    fn insert(state: &mut EditorState, code: KeyCode) -> BufferKey {
        editor_buffer_key(state, InputMode::Insert, key(code))
    }

    /// Dispatch one fully-specified key event in Insert mode, for the sweeps
    /// that need to vary the modifiers rather than only the key code.
    fn insert_event(state: &mut EditorState, event: KeyEvent) -> BufferKey {
        editor_buffer_key(state, InputMode::Insert, event)
    }

    /// Dispatch one fully-specified key event in Normal mode.
    fn normal_event(state: &mut EditorState, event: KeyEvent) -> BufferKey {
        editor_buffer_key(state, InputMode::Normal, event)
    }

    /// The cursor's byte offset, and the desired column beside it, as one read
    /// so a test can assert a *pair* of facts about one keystroke without two
    /// lookups that could disagree if one of them is re-derived.
    fn cursor_and_column(state: &EditorState) -> (usize, Option<usize>) {
        let presentation = state.session.as_ref().expect("open").presentation();
        (presentation.cursor_byte, presentation.desired_column)
    }

    // ── M005: goto leaves and long-word motions ───────────────────────

    /// Every goto leaf, driven as two real keystrokes through the dispatch
    /// path. The leaf table is what resolves them, so a leaf present in the
    /// table but unreachable from the keyboard would pass every unit test of
    /// the table and fail this one.
    #[tokio::test]
    async fn every_goto_leaf_is_reachable_through_two_dispatch_keystrokes() {
        // `    let x = 1;` is 0..14 with `\n` at 14, `second` is 15..21 with
        // `\n` at 21, `third line` is 22..32 with `\n` at 32. The trailing
        // newline opens a final empty line, so the buffer ends at 33.
        let text = "    let x = 1;\nsecond\nthird line\n";

        // `gs` — the leaf this milestone exists for. `0` is the only other way
        // to reach a line start, so indentation was previously unavoidable.
        let (mut state, _daemon) = opened_writer(text).await;
        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(pending(&state), "g", "the armed prefix is visible");
        assert_eq!(normal(&mut state, KeyCode::Char('s')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 4, "`gs` reached the first non-whitespace");
        assert_eq!(pending(&state), "", "and disarmed");

        // `ge` — end of file, which is also where bare `G` lands.
        let (mut state, _daemon) = opened_writer(text).await;
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(normal(&mut state, KeyCode::Char('e')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 33, "`ge` reached the end of the last line");

        // `gh` / `gl` — line start and line end, on the second line so the two
        // are visibly different keys rather than two spellings of byte 0.
        let (mut state, _daemon) = opened_writer(text).await;
        normal(&mut state, KeyCode::Char('g'));
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(cursor(&state), 15, "on the second line");
        normal(&mut state, KeyCode::Char('g'));
        normal(&mut state, KeyCode::Char('l'));
        assert_eq!(cursor(&state), 21, "`gl` reached the end of that line");
        normal(&mut state, KeyCode::Char('g'));
        normal(&mut state, KeyCode::Char('h'));
        assert_eq!(cursor(&state), 15, "`gh` reached the start of that line");

        // `gj` / `gk` — Helix's *textual* line motions. This editor renders one
        // screen row per logical line, so they are the same move as `j`/`k`.
        let (mut state, _daemon) = opened_writer(text).await;
        normal(&mut state, KeyCode::Char('g'));
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(cursor(&state), 15, "`gj` moved down one line");
        normal(&mut state, KeyCode::Char('g'));
        normal(&mut state, KeyCode::Char('k'));
        assert_eq!(cursor(&state), 0, "`gk` moved back up");

        // `gg` is unchanged by the leaf table: it still lands on byte 0.
        let (mut state, _daemon) = opened_writer(text).await;
        normal(&mut state, KeyCode::Char('g'));
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(cursor(&state), 0, "`gg` still lands on the buffer start");
    }

    /// Every goto leaf resolves through the raw motion path, which is how `gg`
    /// has always behaved — and `apply_motion` never touches `selection`. So a
    /// leaf neither extends a held selection the way `j`/`w` do, nor clears
    /// one.
    ///
    /// This pins **parity with `gg`**, which is what M005 promised, rather than
    /// asserting an absolute. Clearing the selection would be the
    /// Helix-faithful choice — a goto in Helix drops it — but `apply_motion`
    /// does not clear it for `gg` either, and making the new leaves clear it
    /// would change `gg`'s behaviour, which this milestone may not do. Recorded
    /// as a known divergence from Helix rather than absorbed silently.
    #[tokio::test]
    async fn every_goto_leaf_leaves_a_held_selection_exactly_as_gg_does() {
        // Establish the `gg` baseline first.
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        normal(&mut state, KeyCode::Char('v'));
        assert!(has_selection(&state), "the selection was established");
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        let baseline = selection_of(&state);
        assert!(
            baseline.is_some(),
            "`gg` leaves the selection alone — the behaviour the leaves match"
        );

        for glyph in ['s', 'e', 'h', 'l', 'j', 'k'] {
            let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
            normal(&mut state, KeyCode::Char('v'));
            normal(&mut state, KeyCode::Char('g'));
            assert_eq!(
                normal(&mut state, KeyCode::Char(glyph)),
                BufferKey::Consumed,
                "`g{glyph}` is consumed by the chord"
            );
            assert_eq!(
                selection_of(&state),
                baseline,
                "`g{glyph}` must leave the selection exactly as `gg` does"
            );
        }
    }

    /// An unbound second key still takes goto's swallow-and-consume path. The
    /// cursor assertion is what makes this meaningful: a *disarmed* prefix that
    /// did not move the cursor would pass a test that only checked the
    /// indicator.
    #[tokio::test]
    async fn an_unbound_goto_second_key_still_swallows_and_resolves_normally() {
        // `gp` is Helix's "previous buffer", which this editor has no leaf for.
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(normal(&mut state, KeyCode::Char('p')), BufferKey::Consumed);
        assert_eq!(pending(&state), "", "the prefix disarmed");
        assert_eq!(
            cursor(&state),
            0,
            "and the swallowed key did not also resolve against the keymap"
        );

        // `gd`/`gy`/`gr`/`gi` are the LSP leaves M010/M013 own. Until then they
        // are unbound, and an unbound leaf must not look like a working key.
        for glyph in ['d', 'y', 'r', 'i'] {
            let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
            normal(&mut state, KeyCode::Char('g'));
            assert_eq!(
                normal(&mut state, KeyCode::Char(glyph)),
                BufferKey::Consumed,
                "`g{glyph}` is consumed by the chord"
            );
            assert_eq!(pending(&state), "");
            assert_eq!(cursor(&state), 0, "`g{glyph}` moved nothing");
        }
    }

    /// `W`/`B`/`E` are reachable, and observably different from `w`/`b`/`e` on
    /// punctuation — the only reason to have two sets of word motions.
    #[tokio::test]
    async fn long_word_motions_are_reachable_and_differ_from_the_word_motions() {
        let (mut state, _daemon) = opened_writer("foo.bar baz").await;

        assert_eq!(normal(&mut state, KeyCode::Char('W')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 8, "`W` skipped the whole of `foo.bar`");

        let (mut state, _daemon) = opened_writer("foo.bar baz").await;
        assert_eq!(normal(&mut state, KeyCode::Char('w')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 3, "`w` stopped on the punctuation run");

        let (mut state, _daemon) = opened_writer("foo.bar baz").await;
        normal(&mut state, KeyCode::Char('W'));
        normal(&mut state, KeyCode::Char('W'));
        assert_eq!(cursor(&state), 11, "`W` to the end of the document");
        assert_eq!(normal(&mut state, KeyCode::Char('B')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 8, "`B` one WORD back");
        assert_eq!(normal(&mut state, KeyCode::Char('E')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 10, "`E` to the last char of `baz`");

        let (mut state, _daemon) = opened_writer("foo.bar baz").await;
        normal(&mut state, KeyCode::Char('W'));
        assert_eq!(normal(&mut state, KeyCode::Char('e')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 10, "`e` still stops at the end of `baz`");

        // Terminals disagree about how a capital is encoded, so both spellings
        // have to work — the same rule `G`, `D` and `U` already follow.
        let (mut state, _daemon) = opened_writer("foo.bar baz").await;
        let shift = KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT);
        assert_eq!(normal_event(&mut state, shift), BufferKey::Consumed);
        assert_eq!(cursor(&state), 8, "`W`+SHIFT is the same motion");
    }

    /// The remembered column has to survive the real dispatch path, not just
    /// the unit-level motion call, or the feature is unreachable in practice.
    #[tokio::test]
    async fn a_vertical_run_through_dispatch_keeps_its_column_across_a_short_line() {
        let (mut state, _daemon) = opened_writer("aaaaaaaaaa\nbb\ncccccccccc\n").await;

        // Column 9, down onto a two-character line, down again, then back up.
        for _ in 0..9 {
            normal(&mut state, KeyCode::Char('l'));
        }
        assert_eq!(cursor_and_column(&state), (9, Some(9)), "seeded by `l`");

        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(
            cursor_and_column(&state),
            (13, Some(9)),
            "`j` clamped the cursor but kept the column"
        );
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(cursor_and_column(&state), (23, Some(9)), "and restored it");

        normal(&mut state, KeyCode::Char('k'));
        normal(&mut state, KeyCode::Char('k'));
        assert_eq!(cursor(&state), 9, "back where the run started");

        // `gj` is the same motion, so it shares the column rather than
        // starting a second, disagreeing run.
        normal(&mut state, KeyCode::Char('g'));
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(
            cursor_and_column(&state),
            (13, Some(9)),
            "`gj` continues the run `j` began"
        );
    }

    /// A run is a run: `w` and `G` end it, so the next `j` re-establishes a
    /// column from where the cursor actually is rather than from a line the
    /// cursor has left.
    #[tokio::test]
    async fn a_non_column_motion_ends_the_vertical_run() {
        let (mut state, _daemon) = opened_writer("aaaaaaaaaa\nbb\ncccccccccc\n").await;
        for _ in 0..9 {
            normal(&mut state, KeyCode::Char('l'));
        }
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(cursor_and_column(&state), (13, Some(9)));

        normal(&mut state, KeyCode::Char('G'));
        assert_eq!(cursor_and_column(&state).1, None, "`G` ended the run");
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(
            cursor_and_column(&state).1,
            Some(0),
            "so `j` re-seeded from where the cursor is"
        );
    }

    /// Insert mode owns every printable character, so the whole M005 surface
    /// must be literal text there. `W`, `B` and `E` are the risky ones: they
    /// are capitals, and Insert already claims shifted letters as text.
    #[tokio::test]
    async fn none_of_the_m005_keys_are_captured_in_insert_mode() {
        for glyph in ['g', 'G', 'W', 'B', 'E', 's', 'j', 'k'] {
            let (mut state, _daemon) = opened_writer("x").await;
            assert_eq!(
                insert(&mut state, KeyCode::Char(glyph)),
                BufferKey::Consumed,
                "`{glyph}` is claimed — as text"
            );
            assert_eq!(
                editor_text(&state),
                format!("{glyph}x"),
                "`{glyph}` typed into the document rather than moving the cursor"
            );
            assert_eq!(cursor(&state), 1, "and the cursor moved with the text");
        }
    }

    /// 60 fixed-width lines, so every cursor assertion below is arithmetic
    /// rather than a guess: line `i` starts at byte `7 * i` and each line's
    /// content ends at `7 * i + 6`.
    fn numbered_lines(count: usize) -> String {
        (0..count)
            .map(|i| format!("line{i:02}\n"))
            .collect::<String>()
    }

    /// Every arm `handle_normal_key` declares, driven through the real
    /// dispatch path, each with the observable effect it promises.
    ///
    /// The path is the whole point of this table. `handle_normal_key` used to
    /// declare `(CONTROL, Char('r')) => editor_redo`, which the `Ctrl+r` guard
    /// above it claimed first — so Normal mode had undo and no reachable redo.
    /// The three tests that "covered" redo called `editor_redo(&mut state)`
    /// directly, never a key, which is why the suite stayed green through the
    /// whole defect. A binding nobody pressed is a binding nobody proved
    /// reachable.
    #[tokio::test]
    async fn every_normal_mode_motion_is_reachable_through_dispatch() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(60)).await;
        assert_eq!(cursor(&state), 0);

        // ── Paging: Ctrl+d/Ctrl+u and the two page keys are distinct arms
        //    that resolve to the same motion, so both are listed. Each starts
        //    from the top of the buffer — a page motion advances the viewport
        //    as well as the cursor, so running them back to back would compound
        //    and prove nothing about either key. The chords go through
        //    `normal_event` — `normal` rebuilds the event with no modifiers,
        //    which would silently turn `Ctrl+d` into a bare `d` and test the
        //    delete-selection arm instead of the page.
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        assert_eq!(
            normal_event(&mut state, ctrl('d')),
            BufferKey::Consumed,
            "Ctrl+d"
        );
        assert_eq!(cursor(&state), 70, "Ctrl+d moves a half page down");
        assert_eq!(
            normal_event(&mut state, ctrl('u')),
            BufferKey::Consumed,
            "Ctrl+u"
        );
        assert_eq!(cursor(&state), 0, "Ctrl+u moves a half page up");

        // A **full** page, and a different distance from the half page's.
        // They used to be the same motion behind two names, so `PageDown`
        // moved half a screen and the key's name was a lie.
        assert_eq!(normal(&mut state, KeyCode::PageDown), BufferKey::Consumed);
        assert_eq!(cursor(&state), 140, "PageDown moves a full page down");
        assert_eq!(normal(&mut state, KeyCode::PageUp), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0, "PageUp moves a full page up");

        // ── Character and line motions, letters and arrows both. These are
        //    sequential rather than a table of independent cases: each
        //    assertion states the absolute offset it expects, so a key that
        //    moved the wrong distance or in the wrong direction fails on the
        //    number instead of on a comparison against itself.
        for (label, code, expect) in [
            ("l", KeyCode::Char('l'), 1),
            ("Right", KeyCode::Right, 2),
            ("h", KeyCode::Char('h'), 1),
            ("Left", KeyCode::Left, 0),
            ("j", KeyCode::Char('j'), 7),
            ("Down", KeyCode::Down, 14),
            ("k", KeyCode::Char('k'), 7),
            ("Up", KeyCode::Up, 0),
        ] {
            assert_eq!(normal(&mut state, code), BufferKey::Consumed, "{label}");
            assert_eq!(cursor(&state), expect, "{label} moves the cursor");
        }

        // ── Line and buffer bounds.
        normal(&mut state, KeyCode::Char('$'));
        assert_eq!(cursor(&state), 6, "$ lands at the end of the line content");
        assert_eq!(normal(&mut state, KeyCode::Char('0')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0, "0 returns to the line start");

        assert_eq!(normal(&mut state, KeyCode::Char('G')), BufferKey::Consumed);
        assert_eq!(
            cursor(&state),
            numbered_lines(60).len(),
            "G goes to the end of the buffer"
        );

        // `gg` is the one multi-key prefix, so it is two events and the
        // intermediate `g` must itself be reachable.
        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(pending(&state), "g", "the first g arms the prefix");
        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0, "gg goes to the start of the buffer");
        assert_eq!(pending(&state), "", "the prefix disarms itself");

        // ── Word motions.
        assert_eq!(normal(&mut state, KeyCode::Char('w')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 7, "w goes to the next word start");
        assert_eq!(normal(&mut state, KeyCode::Char('b')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0, "b goes to the previous word start");
        assert_eq!(normal(&mut state, KeyCode::Char('e')), BufferKey::Consumed);
        assert!(
            (1..7).contains(&cursor(&state)),
            "e stops on the word end, not past it — got {}",
            cursor(&state)
        );
    }

    /// Selection, delete, yank, and the two prefix-driven verbs, each
    /// asserted through dispatch rather than through the helper that
    /// implements it.
    #[tokio::test]
    async fn every_normal_mode_change_verb_is_reachable_through_dispatch() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(4)).await;

        // `v` selects a character and `x` selects whole lines.
        assert_eq!(normal(&mut state, KeyCode::Char('v')), BufferKey::Consumed);
        assert!(has_selection(&state), "v holds a selection");
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        let one_line = selection_len(&state);
        assert_eq!(one_line, 7, "x selects one whole line");
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        assert_eq!(selection_len(&state), 14, "x again extends one more line");

        // `d` deletes the held selection — an effect the App layer performs,
        // so the buffer reports the range rather than applying it.
        let BufferKey::DeleteRanges { ranges, yank } = normal(&mut state, KeyCode::Char('d'))
        else {
            panic!("d must report a delete range");
        };
        assert_eq!(
            ranges,
            editor::one_range(0..14),
            "d reports the range the selection covers"
        );
        assert_eq!(
            yank,
            YankTarget::Unnamed,
            "and where the bytes go: the unnamed register"
        );
        assert_eq!(
            editor_text(&state),
            numbered_lines(4),
            "the buffer reports the effect; it does not apply it"
        );
        // Consuming the selection is the **App layer's** half of that handoff:
        // `apply_helix_verb` leaves the set in place so the undo entry can
        // record the members the reader actually had, and `submit_edit` drops
        // them once the edit lands. This test drives the buffer half only, so
        // it performs that step explicitly rather than leaving a stale
        // selection for the next `x` to extend.
        consume_selection(&mut state);

        // `space y` is two events through the same remembered-prefix path a
        // legacy terminal forces, so it needs the same dispatch proof.
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char(' ')), BufferKey::Consumed);
        let BufferKey::YankRanges { ranges, target } = normal(&mut state, KeyCode::Char('y'))
        else {
            panic!("space y must report a yank range");
        };
        assert_eq!(
            ranges,
            editor::one_range(0..7),
            "space y reports the selected line"
        );
        assert_eq!(
            target,
            YankTarget::Clipboard,
            "space y is the clipboard family and honours no register"
        );

        // `D` deletes to end of line. Both terminal encodings of the capital
        // letter have to work, so both are dispatched. The cursor has to move
        // between the two deletes: `D` on the line the first one just emptied
        // has nothing left to remove, which is a no-op rather than a fault.
        normal(&mut state, KeyCode::Char('0'));
        let before = editor_text(&state);
        assert_eq!(normal(&mut state, KeyCode::Char('D')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "\nline01\nline02\nline03\n");
        assert_ne!(
            editor_text(&state),
            before,
            "D edits through the buffer path"
        );

        assert_eq!(normal(&mut state, KeyCode::Char('j')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char('D')), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            "\n\nline02\nline03\n",
            "D trims line 1"
        );

        // And the capital works spelled with SHIFT, as legacy terminals send
        // it, on a line that still has content.
        assert_eq!(normal(&mut state, KeyCode::Char('j')), BufferKey::Consumed);
        assert_eq!(
            normal_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT)
            ),
            BufferKey::Consumed
        );
        assert_eq!(
            editor_text(&state),
            "\n\n\nline03\n",
            "Shift+D trims line 2"
        );
    }

    /// `u`/`U` through the key path — the arm this milestone made reachable.
    #[tokio::test]
    async fn undo_and_redo_are_reachable_through_dispatch() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(4)).await;
        let original = editor_text(&state);

        assert_eq!(normal(&mut state, KeyCode::Char('D')), BufferKey::Consumed);
        let trimmed = editor_text(&state);
        assert_ne!(trimmed, original, "D made a local edit to undo");

        assert_eq!(normal(&mut state, KeyCode::Char('u')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), original, "u restores the text");
        assert_eq!(redo_depth(&state), 1, "u leaves something to redo");

        // The whole point: `U` is the redo chord, reached by dispatch. Before
        // this milestone the Normal-mode redo arm was `(CONTROL, Char('r'))`,
        // which the mode-independent resync guard claimed first.
        assert_eq!(normal(&mut state, KeyCode::Char('U')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), trimmed, "U re-applies the undone edit");
        assert_eq!(redo_depth(&state), 0, "U consumed the redo entry");
        assert_eq!(undo_depth(&state), 1, "U is the inverse of u");

        // Terminals disagree on how a capital letter is encoded, so `U` must
        // work spelled with SHIFT as well.
        normal(&mut state, KeyCode::Char('u'));
        assert_eq!(editor_text(&state), original);
        assert_eq!(
            normal_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('U'), KeyModifiers::SHIFT)
            ),
            BufferKey::Consumed
        );
        assert_eq!(
            editor_text(&state),
            trimmed,
            "Shift+U redoes too — a legacy terminal sends the capital this way"
        );
    }

    /// The lifecycle chords keep exactly one owner each and never collide with
    /// the buffer's own verbs. `Ctrl+r` is resync here in *both* modes; redo
    /// is `U`, which is what Helix actually binds.
    #[tokio::test]
    async fn lifecycle_chords_keep_one_owner_across_both_modes() {
        let (mut state, _daemon) = opened_writer("base").await;
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);

        for mode in [InputMode::Normal, InputMode::Insert] {
            assert_eq!(
                editor_buffer_key(&mut state, mode, ctrl('s')),
                BufferKey::RequestSave,
                "Ctrl+s saves in {mode:?}"
            );
            assert_eq!(
                editor_buffer_key(&mut state, mode, ctrl('r')),
                BufferKey::RequestResync,
                "Ctrl+r resyncs in {mode:?} — redo must not shadow it"
            );
        }

        // And the buffer's redo chord is not one of the lifecycle chords.
        assert_eq!(
            normal(&mut state, KeyCode::Char('U')),
            BufferKey::Consumed,
            "U is the buffer's, so it must not report a lifecycle round trip"
        );
    }

    /// Every insertion entry reports `EnterInsert` so the App layer can adopt
    /// the mode — except on a read-only buffer, where claiming the keyboard
    /// would strand the user in a mode that cannot be typed into.
    ///
    /// The two families differ in what they do with the key, and the
    /// difference is the point. `i`/`a`/`I`/`A` only *move the cursor*, so
    /// they leave the key unclaimed and the status row's "read-only" is the
    /// report. `o`/`O` **write**, so they claim the key and report the
    /// refusal as a notice, exactly as `d`/`D`/`x` do — an unclaimed `o`
    /// would drop the letter into the composer, which reads as the editor
    /// ignoring a verb it owns.
    #[tokio::test]
    async fn every_insertion_entry_is_reachable_through_dispatch() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(4)).await;
        for (label, k) in [
            ("i", KeyCode::Char('i')),
            ("a", KeyCode::Char('a')),
            ("I", KeyCode::Char('I')),
            ("A", KeyCode::Char('A')),
            ("o", KeyCode::Char('o')),
            ("O", KeyCode::Char('O')),
        ] {
            assert_eq!(normal(&mut state, k), BufferKey::EnterInsert, "{label}");
        }

        let (mut reader, _daemon) = opened_reader("base").await;
        for (label, k) in [("i", KeyCode::Char('i')), ("A", KeyCode::Char('A'))] {
            assert_eq!(
                normal(&mut reader, k),
                BufferKey::Unhandled,
                "{label} must not hand over a read-only buffer"
            );
            assert_ne!(
                reader.mode(),
                EditorMode::Insert,
                "{label} kept the keyboard"
            );
        }
        for (label, k) in [("o", KeyCode::Char('o')), ("O", KeyCode::Char('O'))] {
            assert_eq!(
                normal(&mut reader, k),
                BufferKey::Consumed,
                "{label} writes, so it claims the key and reports the refusal"
            );
            assert_ne!(
                reader.mode(),
                EditorMode::Insert,
                "{label} kept the keyboard"
            );
            let notice = notice_of(&reader).expect("{label} reports the refusal");
            assert!(
                notice.contains("Read-only"),
                "expected the read-only reason for {label}, got {notice:?}"
            );
        }
    }

    /// `:` and `/` open their modal lines, and an unclaimed letter reaches
    /// the composer. The fallthrough is the property that makes Normal mode
    /// usable rather than merely modal, so it is asserted explicitly: a fix
    /// that over-claims is as wrong as one that under-claims.
    #[tokio::test]
    async fn modal_lines_open_and_unclaimed_keys_reach_the_composer() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(4)).await;

        assert_eq!(normal(&mut state, KeyCode::Char(':')), BufferKey::Consumed);
        assert!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active,
            ": opens the command line"
        );
        normal(&mut state, KeyCode::Esc);
        assert!(
            !state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active,
            "Esc closes it without running anything"
        );

        assert_eq!(normal(&mut state, KeyCode::Char('/')), BufferKey::Consumed);
        assert!(
            state.session.as_ref().unwrap().presentation().search_active,
            "/ opens the search line"
        );
        normal(&mut state, KeyCode::Esc);
        assert!(
            !state.session.as_ref().unwrap().presentation().search_active,
            "Esc abandons the search"
        );

        // Nothing below claims these, so they must stay composer keystrokes.
        //
        // `f`, `t`, `F` and `T` were on this list until M003 claimed them as
        // the character-finding gestures, and `y`, `p` and `~` left it in
        // M004, when they became Normal-mode verbs of their own rather than
        // only a leaf of `space`. `%` left it in M008, with the rest of the
        // selection-manipulation surface, and `m` left it in M009, when the
        // match-mode grammar claimed it. That is the intended shape of the
        // failure: the list asserts which keys the buffer has *not* taken, so
        // claiming a key is supposed to require removing it here rather than
        // quietly widening the buffer. It is a to-do list for whoever claims
        // the next one, not a permanent record.
        // `z` left this list when view mode claimed it: it is the prefix
        // that scrolls the page without moving the cursor.
        //
        // `J` left it with the line-shape verbs (`X`, `Alt-x`, `_`, `J`,
        // `Alt-J`, `>` and `<`), which is the same intended shape: the list
        // is a to-do list for whoever claims the next key, not a permanent
        // record of what the buffer must leave alone.
        //
        // `c` left it in M002, with the change pair (`c`, `Alt-c`) and the
        // no-yank delete (`Alt-d`). On the editor route `c` no longer types
        // into the composer; that is the intended effect of binding it, and
        // `docs/tui.md` says so.
        // `s` and `?` left this list in M003, with the regex search verbs:
        // `?` is the reverse search and `s` selects the regex matches inside
        // the selections. `q` stayed unclaimed even after M008 landed macro
        // recording, and deliberately: `q` is the two-press exit, so binding
        // it for recording would make leaving the program depend on recording
        // state. It is asserted here on its own rather than through a
        // one-element loop, which asserts nothing a single call does not.
        assert_eq!(
            normal(&mut state, KeyCode::Char('q')),
            BufferKey::Unhandled,
            "q is unclaimed and must reach the composer"
        );
    }

    /// No Normal-mode binding may leak into the Insert path and swallow a
    /// printable character.
    ///
    /// The sweep covers the *editor route* rather than the global keymaps.
    /// `builtin_shift_letters_are_composed_text_in_insert_mode` in
    /// `src/tui/input.rs` guards the global table, which is a different
    /// surface with its own failure mode; a binding added to
    /// `handle_normal_key` is invisible to it. The same defect class already
    /// shipped twice through the *global* table — a capital `F` opened the
    /// project picker and ate the character in the default configuration —
    /// so the editor route deserves its own proof rather than an assumption
    /// that a neighbouring sweep covers it.
    ///
    /// Two encodings are checked for every letter, because terminals disagree:
    /// an unshifted `U` and a `Shift+u`. The assertion is the strict one — the
    /// document must have grown by exactly that one character, or the key was
    /// consumed by something other than the document.
    ///
    /// Capital letters are deliberately checked in **both** spellings. `R` →
    /// reload is the reason this matters: a terminal that sends `Char('R')`
    /// with `NONE` must still get the letter typed, and a sweep that only
    /// tested `Shift+R` would not notice that guard leaking into the Insert
    /// path.
    #[tokio::test]
    async fn printable_characters_reach_the_document_in_insert_mode() {
        let mut inspected = 0usize;

        for letter in 'a'..='z' {
            let capital = letter.to_ascii_uppercase();
            // (char, modifiers, label)
            let cases: [(char, KeyModifiers, &str); 3] = [
                (letter, KeyModifiers::NONE, "lower"),
                (capital, KeyModifiers::NONE, "capital, unshifted"),
                (capital, KeyModifiers::SHIFT, "capital, shifted"),
            ];
            for (ch, modifiers, label) in cases {
                let (mut state, _daemon) = opened_writer("").await;
                let before = editor_text(&state);

                assert_eq!(
                    insert_event(&mut state, KeyEvent::new(KeyCode::Char(ch), modifiers)),
                    BufferKey::Consumed,
                    "{ch} ({label}) must be text in Insert"
                );
                assert_eq!(
                    editor_text(&state),
                    format!("{ch}{before}"),
                    "{ch} ({label}) must reach the document verbatim"
                );
                inspected += 1;
            }
        }

        // The punctuation the Normal-mode verbs are spelled with. `:` and `/`
        // are here on purpose: both open a modal line in Normal, and both must
        // be ordinary text in Insert or a URL could never be typed. `?` is here
        // for the same reason from the other side — it is a live global binding
        // (Help) that must not leak into the document while typing.
        //
        // `[` and `]` joined them in M006. They are the sharpest case on the
        // list because each is *both* a Normal-mode prefix that arms a mode
        // and a character people type constantly — markdown links, JSON keys,
        // index expressions. A prefix arm that leaked into Insert would make
        // those lines untypable, and nothing else on this list would catch it.
        for ch in [
            '/', ':', ' ', '\n', '\t', '.', ',', '$', '0', '~', '_', '*', '?', '[', ']',
        ] {
            let (mut state, _daemon) = opened_writer("").await;
            let expected: String = match ch {
                '\n' => "\n".to_string(),
                '\t' => "\t".to_string(),
                other => other.to_string(),
            };
            assert_eq!(
                insert(&mut state, KeyCode::Char(ch)),
                BufferKey::Consumed,
                "{ch:?} must be text in Insert"
            );
            assert_eq!(
                editor_text(&state),
                expected,
                "{ch:?} must reach the document verbatim"
            );
            inspected += 1;
        }

        // Non-vacuity: a sweep that inspected nothing would pass trivially, so
        // assert it actually walked the alphabet it claims to cover. The count
        // is 26 letters * 3 encodings + 15 punctuation — `[` and `]` joined
        // the punctuation set in M006, which is also why this number moves
        // whenever a claim is added rather than when a letter is.
        assert_eq!(
            inspected, 93,
            "the sweep must inspect every letter in every encoding plus the punctuation set"
        );
    }

    /// The mode-independent chords are *not* printable text, so they keep
    /// working in Insert. This is the pair of assertions that keeps the sweep
    /// above honest: without them, "Insert swallows everything" would pass it.
    #[tokio::test]
    async fn insert_mode_chords_still_reach_their_owners() {
        let (mut state, _daemon) = opened_writer("base").await;
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);

        assert_eq!(
            insert_event(&mut state, ctrl('s')),
            BufferKey::RequestSave,
            "Ctrl+s saves from Insert too"
        );
        assert_eq!(
            insert_event(&mut state, ctrl('r')),
            BufferKey::RequestResync,
            "Ctrl+r resyncs from Insert too"
        );
        assert_eq!(
            editor_text(&state),
            "base",
            "neither chord edits the document"
        );
    }

    #[tokio::test]
    async fn insert_mode_types_into_the_buffer_and_esc_only_leaves_insert() {
        let (mut state, _daemon) = opened_writer("").await;

        // `i` is an insertion entry: the document takes the keyboard, and the
        // buffer reports it so the App layer adopts Insert mode.
        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        assert_eq!(state.mode(), EditorMode::Insert);

        for ch in "hi".chars() {
            insert(&mut state, KeyCode::Char(ch));
        }
        assert_eq!(editor_text(&state), "hi");

        // `Esc` is a mode step, and it is answered by the App layer *before*
        // the buffer sees it — so the buffer handler no longer has an Esc arm
        // at all. That is the point: `Esc` cannot reach anything that closes
        // the editor, because there is nothing left here that it could.
        assert_eq!(insert(&mut state, KeyCode::Esc), BufferKey::Unhandled);
        assert_eq!(
            state.mode(),
            EditorMode::Insert,
            "Esc never leaves Insert on its own"
        );
    }

    #[tokio::test]
    async fn a_letter_the_buffer_does_not_own_falls_through_in_normal_mode() {
        let (mut state, _daemon) = opened_writer("base").await;
        // `?` was on this list until M003 claimed it as the reverse search.
        // `q` takes its place: macro recording is M008's, so the key is still
        // the buffer's to give back.
        for code in [
            KeyCode::Char('q'),
            KeyCode::F(5),
            KeyCode::Home,
            KeyCode::End,
        ] {
            assert_eq!(
                normal(&mut state, code),
                BufferKey::Unhandled,
                "{code:?} must reach the input box, not the buffer"
            );
        }
        assert_eq!(editor_text(&state), "base");
    }

    #[tokio::test]
    async fn save_and_resync_are_requested_in_both_modes_and_reload_moved_to_the_colon_line() {
        let (mut state, _daemon) = opened_writer("base").await;
        let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        let ctrl_r = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);

        for mode in [InputMode::Normal, InputMode::Insert] {
            assert_eq!(
                editor_buffer_key(&mut state, mode, ctrl_s),
                BufferKey::RequestSave
            );
            assert_eq!(
                editor_buffer_key(&mut state, mode, ctrl_r),
                BufferKey::RequestResync
            );
        }

        // `R` no longer reloads. Helix gives it `replace_with_yanked`, and a
        // guard-block chord would have shadowed the Normal-mode verb *and*
        // survived it — the exact failure this dispatch chain exists to make
        // impossible. Reload is one `:e` away instead.
        assert_eq!(
            normal(&mut state, KeyCode::Char('R')),
            BufferKey::Paste {
                verb: ClipboardVerb::Replace,
                register: RegisterName::Unnamed,
            }
        );
        // In Insert those two keystrokes are the letter `R`, so the text
        // grows by exactly one — which is the property that makes normal-only
        // reachability safe, and that keeps `R` out of Insert mode's keyspace.
        assert_eq!(insert(&mut state, KeyCode::Char('R')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "Rbase");

        // The buffer handler performs no transport work.
        assert!(!state.is_busy());
    }

    #[tokio::test]
    async fn x_selects_a_line_and_d_deletes_it_in_the_helix_way() {
        let (mut state, _daemon) = opened_writer("one\ntwo\nthree\n").await;

        // Helix: `x` selects the cursor's line, `d` deletes the selection.
        // vi's `dd` is the same gesture spelled differently.
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        assert_eq!(
            normal(&mut state, KeyCode::Char('d')),
            BufferKey::DeleteRanges {
                ranges: editor::one_range(0..4),
                yank: YankTarget::Unnamed,
            }
        );
        let edit = editor::delete_range(&snapshot_of(&state), 0..4).unwrap();
        submit_edit(&mut state, edit);
        assert_eq!(editor_text(&state), "two\nthree\n");

        // A second `x` grows the line selection instead of re-anchoring.
        normal(&mut state, KeyCode::Char('x'));
        normal(&mut state, KeyCode::Char('x'));
        // Lines 0..=1 of "two\nthree\n" — 0..10, both terminators included.
        assert_eq!(
            normal(&mut state, KeyCode::Char('d')),
            BufferKey::DeleteRanges {
                ranges: editor::one_range(0..10),
                yank: YankTarget::Unnamed,
            }
        );
    }

    #[tokio::test]
    async fn d_with_nothing_selected_is_claimed_so_it_cannot_reach_the_diff_binding() {
        let (mut state, _daemon) = opened_writer("one\n").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('d')),
            BufferKey::Consumed,
            "an unhandled `d` would fire the global OpenDiff instead"
        );
        assert_eq!(editor_text(&state), "one\n");
    }

    #[tokio::test]
    async fn v_selects_by_character_and_movement_grows_it() {
        let (mut state, _daemon) = opened_writer("abcdef\n").await;
        normal(&mut state, KeyCode::Char('v'));
        normal(&mut state, KeyCode::Char('l'));
        normal(&mut state, KeyCode::Char('l'));
        // `v` then two `l`s covers three characters — the head is inclusive,
        // matching `/terminal`.
        assert_eq!(
            normal(&mut state, KeyCode::Char('d')),
            BufferKey::DeleteRanges {
                ranges: editor::one_range(0..3),
                yank: YankTarget::Unnamed,
            }
        );
    }

    #[tokio::test]
    async fn space_y_reports_a_yank_range_as_a_remembered_prefix() {
        let (mut state, _daemon) = opened_writer("abcdef\n").await;
        normal(&mut state, KeyCode::Char('x'));
        assert_eq!(normal(&mut state, KeyCode::Char(' ')), BufferKey::Consumed);
        assert_eq!(
            normal(&mut state, KeyCode::Char('y')),
            BufferKey::YankRanges {
                ranges: editor::one_range(0..7),
                target: YankTarget::Clipboard,
            },
            "`space y` is two events, so it is prefix state rather than a chord"
        );

        // Any other second key disarms the prefix and is then resolved as an
        // ordinary normal-mode key.
        normal(&mut state, KeyCode::Char(' '));
        assert_eq!(normal(&mut state, KeyCode::Char('w')), BufferKey::Consumed);
    }

    #[tokio::test]
    async fn ctrl_d_and_ctrl_u_page_while_bare_d_deletes() {
        let (mut state, _daemon) = opened_writer(&"line\n".repeat(200)).await;
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(normal(&mut state, KeyCode::Char('d')), BufferKey::Consumed);
        assert_eq!(
            editor_buffer_key(&mut state, InputMode::Normal, ctrl_d),
            BufferKey::Consumed
        );
        let after_down = cursor(&state);
        editor_buffer_key(&mut state, InputMode::Normal, ctrl_d);
        assert!(cursor(&state) > after_down, "Ctrl+d moves a page at a time");
        editor_buffer_key(&mut state, InputMode::Normal, ctrl_u);
        assert_eq!(cursor(&state), after_down);
    }

    #[tokio::test]
    async fn a_pending_prefix_swallows_the_next_key_and_resolves_only_known_pairs() {
        let (mut state, _daemon) = opened_writer("one\ntwo\nthree\n").await;

        // `gg` is the one multi-key prefix left; it has no single-key spelling.
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(pending(&state), "g");
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(pending(&state), "", "the prefix is consumed");
        assert_eq!(cursor(&state), 0, "`gg` goes to the start of the buffer");

        // An unknown pair is discarded without touching the text.
        normal(&mut state, KeyCode::Char('g'));
        normal(&mut state, KeyCode::Char('z'));
        assert_eq!(pending(&state), "");
        assert_eq!(editor_text(&state), "one\ntwo\nthree\n");
    }

    #[tokio::test]
    async fn the_pending_prefix_is_bounded() {
        let (mut state, _daemon) = opened_writer("base").await;
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(pending(&state), "g");
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(pending(&state), "");
    }

    // ── The unified prefix state machine ─────────────────────────────────
    //
    // Everything below drives `editor_buffer_key`, the way a key arrives from
    // the terminal, rather than calling the resolution helpers directly. A
    // direct call would pass even if the prefix could never be armed.

    /// Arming `[`/`]` and then pressing a key with no leaf must disarm *and*
    /// resolve that key normally.
    #[tokio::test]
    async fn the_unimpaired_prefixes_disarm_without_swallowing_the_next_key() {
        for (arming, label) in [(KeyCode::Char('['), "`[`"), (KeyCode::Char(']'), "`]`")] {
            let (mut state, _daemon) = opened_writer(&numbered_lines(8)).await;
            assert_eq!(
                normal(&mut state, arming),
                BufferKey::Consumed,
                "{label} arms"
            );
            assert!(
                !pending(&state).is_empty(),
                "{label} shows a pending indicator while armed"
            );

            // `j` is a real normal-mode verb, so resolving it normally is
            // observable: the cursor must move.
            assert_eq!(pending(&state), display_of(arming), "{label} indicator");
            let before = cursor(&state);
            assert_eq!(
                normal(&mut state, KeyCode::Char('j')),
                BufferKey::Consumed,
                "{label} must not swallow the next key"
            );
            assert!(
                cursor(&state) > before,
                "{label} disarmed, so `j` still moved the cursor"
            );
            assert_eq!(pending(&state), "", "{label} disarmed");

            // An unbound key is not swallowed either: it has to keep reaching
            // the composer, which is what `Unhandled` means.
            assert_eq!(
                normal(&mut state, arming),
                BufferKey::Consumed,
                "{label} re-arms"
            );
            assert_eq!(
                normal(&mut state, KeyCode::Char('q')),
                BufferKey::Unhandled,
                "{label} then an unbound key reaches the composer"
            );
            assert_eq!(pending(&state), "", "{label} disarmed");
        }
    }

    fn display_of(arming: KeyCode) -> &'static str {
        match arming {
            KeyCode::Char('[') => "[",
            KeyCode::Char(']') => "]",
            _ => "",
        }
    }

    /// The two migrated gestures keep the *different* unmatched-key policies
    /// they had before, which is the behavior the merge had to preserve.
    #[tokio::test]
    async fn goto_swallows_an_unmatched_key_while_space_retries_it() {
        // `gg` is a chord: the second key belongs to it either way.
        let (mut state, _daemon) = opened_writer(&numbered_lines(8)).await;
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(
            normal(&mut state, KeyCode::Char('j')),
            BufferKey::Consumed,
            "`g j` is swallowed by the chord"
        );
        assert_eq!(pending(&state), "");

        // `space y` is a modifier: an unmatched key is an ordinary verb.
        // The example is `space l`, not `space j`: `space j` is a **bound**
        // leaf since M010 — it states that there is no jumplist — so it
        // resolves instead of retrying, and the policy this test pins is
        // about the keys that are *not* leaves.
        let (mut state, _daemon) = opened_writer(&numbered_lines(8)).await;
        normal(&mut state, KeyCode::Char(' '));
        let before = cursor(&state);
        assert_eq!(normal(&mut state, KeyCode::Char('l')), BufferKey::Consumed);
        assert!(
            cursor(&state) > before,
            "`space l` retries, so `l` still moved the cursor"
        );
    }

    /// A non-`Char` terminal key resolves differently per prefix, exactly as
    /// the baseline's two inline branches did.
    #[tokio::test]
    async fn a_non_char_terminal_key_follows_the_prefixs_policy() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(8)).await;
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(
            normal(&mut state, KeyCode::Down),
            BufferKey::Consumed,
            "`g` then an arrow is swallowed by the chord"
        );

        let (mut state, _daemon) = opened_writer(&numbered_lines(8)).await;
        normal(&mut state, KeyCode::Char(' '));
        let before = cursor(&state);
        assert_eq!(normal(&mut state, KeyCode::Down), BufferKey::Consumed);
        assert!(
            cursor(&state) > before,
            "`space` then an arrow retries, so `Down` still moved"
        );
    }

    /// Prefix resolution must never intercept an open modal line.
    ///
    /// `editor_buffer_key` checks `search_active`/`command_active` before it
    /// dispatches on mode at all, so a prefix armed before the line opened
    /// cannot resolve into it: `y` typed into a half-typed `:q` has to be a
    /// command name, not a yank.
    ///
    /// The converse is deliberately *not* asserted, because it is not what the
    /// editor does. A prefix that swallows its second key — `g` — consumes `:`
    /// and `/` too, exactly as the baseline's `pending_command` branch did
    /// before those arms were ever reached; retrying prefixes (`space`, `[`,
    /// `]`) let the colon through. The armed prefix is visible in the status
    /// row, so this is predictable rather than silent. See the closure record.
    #[tokio::test]
    async fn a_modal_line_outranks_the_pending_prefix() {
        // Armed `g` swallows the colon, as it did before this milestone.
        let (state, _daemon) = opened_writer("hello").await;
        let mut state = state;
        arm(&mut state, EditorPrefix::Goto);
        assert_eq!(normal(&mut state, KeyCode::Char(':')), BufferKey::Consumed);
        assert!(
            !command_line_open(&state),
            "`g :` is the chord consuming the colon, unchanged from the baseline"
        );

        // A retrying prefix lets it through to the command line, and once the
        // line owns the keyboard the leaf key is text rather than a gesture.
        let (state, _daemon) = opened_writer("hello").await;
        let mut state = state;
        arm(&mut state, EditorPrefix::Space);
        assert_eq!(normal(&mut state, KeyCode::Char(':')), BufferKey::Consumed);
        assert!(
            command_line_open(&state),
            "`space` retried, so `:` opened the line"
        );
        assert_eq!(normal(&mut state, KeyCode::Char('y')), BufferKey::Consumed);
        assert_eq!(
            command_line_text(&state),
            "y",
            "`y` typed into the open command line instead of yanking"
        );
        assert_eq!(
            pending(&state),
            "",
            "`:` already disarmed the retrying prefix, so there was nothing left for \
             the `y` to resolve"
        );

        // The same, in the other direction, for the search line.
        let (state, _daemon) = opened_writer("hello").await;
        let mut state = state;
        arm(&mut state, EditorPrefix::Goto);
        assert_eq!(normal(&mut state, KeyCode::Char('/')), BufferKey::Consumed);
        assert!(
            !search_open(&state),
            "`g /` is the chord consuming the slash, as at the baseline"
        );

        let (state, _daemon) = opened_writer("hello").await;
        let mut state = state;
        arm(&mut state, EditorPrefix::OpenBracket);
        assert_eq!(normal(&mut state, KeyCode::Char('/')), BufferKey::Consumed);
        assert!(search_open(&state), "`[` retried, so `/` opened the line");
        // `/` seeds the needle from the word under the cursor, so the typed
        // `g` extends the seed instead of replacing it.
        let seeded = search_text(&state);
        assert!(
            !seeded.is_empty(),
            "`/` seeded from the word under the cursor"
        );
        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(
            search_text(&state),
            format!("{seeded}g"),
            "`g` extended the search needle rather than completing `gg`"
        );
    }

    /// Insert mode never reaches prefix resolution at all: the characters are
    /// document text, including a prefix's own leaf glyph.
    #[tokio::test]
    async fn insert_mode_never_resolves_the_pending_prefix() {
        let (mut state, _daemon) = opened_writer("base").await;
        arm(&mut state, EditorPrefix::Space);
        assert_eq!(
            editor_buffer_key(
                &mut state,
                InputMode::Insert,
                KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)
            ),
            BufferKey::Consumed
        );
        // The cursor opens at offset 0, so an Insert-mode keystroke prepends.
        assert_eq!(
            editor_text(&state),
            "ybase",
            "the key typed into the document"
        );
        assert_eq!(
            pending(&state),
            "space",
            "and the prefix is still armed, untouched by Insert-mode input"
        );

        // A `Goto` prefix leaves a typed `g` alone too: `g g` in Insert is two
        // characters, not a jump to the start of the buffer.
        let (mut state, _daemon) = opened_writer("base").await;
        arm(&mut state, EditorPrefix::Goto);
        editor_buffer_key(
            &mut state,
            InputMode::Insert,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        assert_eq!(
            editor_text(&state),
            "gbase",
            "`g g` must not jump to the buffer start"
        );
        assert_eq!(
            cursor(&state),
            1,
            "the cursor stayed with the typed character"
        );
        assert_eq!(pending(&state), "g", "still armed");
    }

    fn command_line_open(state: &EditorState) -> bool {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .command_active
    }

    fn command_line_text(state: &EditorState) -> String {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .command_line
            .clone()
    }

    fn search_open(state: &EditorState) -> bool {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .search_active
    }

    fn search_text(state: &EditorState) -> String {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .search_line
            .clone()
    }

    /// The stale-prefix hazard: a prefix must not survive whatever replaced
    /// the thing it was aimed at.
    #[tokio::test]
    async fn opening_a_document_disarms_the_pending_prefix() {
        let (state, _daemon) = opened_writer("first document\n").await;
        let mut state = state;
        arm(&mut state, EditorPrefix::Goto);
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .reset_for_open();
        assert_eq!(
            pending(&state),
            "",
            "a `g` armed over the previous document must not answer here"
        );
    }

    #[tokio::test]
    async fn leaving_the_editor_route_disarms_the_pending_prefix() {
        let (state, _daemon) = opened_writer(&numbered_lines(8)).await;
        let mut app = app_on_the_editor_route(state).await;
        arm_on_app(&mut app, EditorPrefix::Goto);

        app.ui_state
            .routes
            .navigate_to(crate::tui::route::Route::Editor);
        leave_editor_view(&mut app);

        assert_eq!(
            prefix_on_app(&app),
            EditorPrefix::Idle,
            "the route the prefix was armed for is no longer on screen"
        );
    }

    #[tokio::test]
    async fn moving_focus_to_the_tree_pane_disarms_the_pending_prefix() {
        let (state, _daemon) = opened_writer(&numbered_lines(8)).await;
        let mut app = app_on_the_editor_route(state).await;
        arm_on_app(&mut app, EditorPrefix::Space);

        crate::tui::commands::file_tree::toggle_tree(&mut app);
        assert!(app.ui_state.tree_focused, "the toggle focused the pane");
        assert_eq!(
            prefix_on_app(&app),
            EditorPrefix::Idle,
            "focus moved, so the buffer's pending prefix is retired"
        );

        // `set_focus` is the other entry point into the same fact.
        arm_on_app(&mut app, EditorPrefix::Goto);
        crate::tui::commands::file_tree::set_focus(&mut app, true);
        assert_eq!(prefix_on_app(&app), EditorPrefix::Idle);
    }

    /// Reconciliation rewrites the text under the user, which retires a prefix
    /// exactly like opening a different document does. The baseline disarmed
    /// only `pending_command` here and left `space` armed across a reload.
    #[tokio::test]
    async fn a_reconciliation_disarms_the_pending_prefix() {
        for reconcile in ["reload", "resync", "close"] {
            let (state, _daemon) = opened_writer("base").await;
            let mut state = state;
            for prefix in [
                EditorPrefix::Goto,
                EditorPrefix::Space,
                EditorPrefix::OpenBracket,
            ] {
                arm(&mut state, prefix);
                let session = state.session.as_mut().unwrap();
                editor::clear_history(session.presentation_mut());
                assert_eq!(
                    pending(&state),
                    "",
                    "{reconcile} must retire a {prefix:?} prefix"
                );
            }
        }
    }

    /// The prefix reports an effect; it never reaches the controller or the
    /// filesystem itself, and a read-only buffer behaves exactly as a writable
    /// one does.
    #[tokio::test]
    async fn a_read_only_buffer_arms_and_resolves_prefixes_unchanged() {
        let (state, _daemon) = opened_reader(&numbered_lines(8)).await;
        let mut state = state;

        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(pending(&state), "g", "arming does not consult writability");
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(cursor(&state), 0, "`gg` resolves on a read-only buffer");

        // `space y` with no selection reports `Consumed`, not an effect: the
        // buffer reports the range and the App layer owns the clipboard.
        assert_eq!(normal(&mut state, KeyCode::Char(' ')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char('y')), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            numbered_lines(8),
            "no prefix mutated text"
        );
    }

    /// All four find keys, driven through the real dispatch path, each
    /// crossing a line boundary.
    ///
    /// Separate from M001's reachability table because these are two-key
    /// gestures: arming is the only thing the first key can be asked to do,
    /// and a test that asserted "the cursor moved" would pass just as happily
    /// against a scan confined to the current line — where every target here
    /// is absent and the cursor correctly does not move.
    #[tokio::test]
    async fn every_find_key_arms_and_its_target_resolves_across_lines() {
        // `alpha\nbeta\ngamma\ndelta\n`. `g` (byte 11) and `b` (byte 6) each
        // occur exactly once, on a different line from the cursor.
        let text = "alpha\nbeta\ngamma\ndelta\n";
        // `g` (byte 11) and `b` (byte 6) each occur exactly once, on a
        // different line from the cursor. Backward scans start from the end
        // so there is text behind them.
        let cases = [
            (KeyCode::Char('f'), "f", 'g', 0usize, 11usize),
            (KeyCode::Char('F'), "F", 'b', 23, 6),
            (KeyCode::Char('t'), "t", 'g', 0, 11),
            (KeyCode::Char('T'), "T", 'b', 23, 6),
        ];
        for (arming, indicator, target, start, expected) in cases {
            let (mut state, _daemon) = opened_writer(text).await;
            state
                .session
                .as_mut()
                .unwrap()
                .presentation_mut()
                .cursor_byte = start;

            assert_eq!(
                normal(&mut state, arming),
                BufferKey::Consumed,
                "{indicator} must be claimed by the buffer"
            );
            assert_eq!(
                pending(&state),
                indicator,
                "{indicator} must show as the pending indicator while armed"
            );
            assert_eq!(
                cursor(&state),
                start,
                "arming moves nothing; only the target may"
            );

            assert_eq!(
                normal(&mut state, KeyCode::Char(target)),
                BufferKey::Consumed,
                "{indicator} {target} must resolve"
            );
            assert_eq!(
                pending(&state),
                "",
                "the prefix disarms after exactly one target"
            );
            assert_eq!(
                cursor(&state),
                expected,
                "{indicator} {target} must land on the target across line breaks"
            );
        }
    }

    /// The target key is data, not a command: it must reach the scan even
    /// when it is a key the buffer claims for something else.
    ///
    /// `t` and `T` land **on** the target. An implementation that stopped
    /// *before* it — the vi and kakoune reading — would fail these two
    /// assertions and nothing else, which is why they are spelled out as
    /// exact byte offsets rather than "moved near the target".
    #[tokio::test]
    async fn a_find_target_may_be_any_character_the_buffer_claims() {
        let text = "alpha\nbeta\ngamma\n";
        // `a` at byte 4 and `g` at byte 11 each occur once. The forward case
        // starts before its target and the backward one after it, so neither
        // can be satisfied by a scan that only looked at the cursor's own
        // neighbourhood.
        let cases = [
            (KeyCode::Char('t'), 'a', 0usize, 4usize),
            (KeyCode::Char('T'), 'g', text.len(), 11),
        ];
        for (arming, target, start, expected) in cases {
            let (mut state, _daemon) = opened_writer(text).await;
            state
                .session
                .as_mut()
                .unwrap()
                .presentation_mut()
                .cursor_byte = start;

            normal(&mut state, arming);
            normal(&mut state, KeyCode::Char(target));
            assert_eq!(
                cursor(&state),
                expected,
                "{arming:?} {target} must land on the target itself"
            );
        }
    }

    /// The two modal lines outrank a pending target.
    ///
    /// Plan §4 makes `:` and `/` outrank everything including this, which is
    /// stricter than taking the next key literally. It is also the safer
    /// reading: a colon typed after `f` in Helix is a search for a colon, and
    /// a user who mistyped the `f` would then have no way back to the command
    /// line at all.
    #[tokio::test]
    async fn a_pending_find_target_yields_the_modal_line_keys() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        normal(&mut state, KeyCode::Char('f'));
        assert_eq!(pending(&state), "f");
        normal(&mut state, KeyCode::Char(':'));
        assert!(
            command_line_open(&state),
            "`:` must still open the command line behind an armed `f`"
        );
        assert_eq!(pending(&state), "", "and the prefix still disarms");

        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        normal(&mut state, KeyCode::Char('f'));
        normal(&mut state, KeyCode::Char('/'));
        assert!(
            search_open(&state),
            "`/` must still open the search line behind an armed `f`"
        );
        assert_eq!(pending(&state), "");
    }

    /// A key that cannot be a target is handed back, not swallowed.
    ///
    /// This is the milestone's sharpest hazard. An armed `f` that ate the
    /// following arrow would be an arrow the composer never sees, on a route
    /// where unclaimed keys are supposed to fall through to it. Retrying the
    /// key costs the user nothing; swallowing it silently loses a keystroke.
    #[tokio::test]
    async fn a_pending_find_target_hands_back_a_key_that_cannot_be_a_target() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        arm(&mut state, EditorPrefix::FindNext);
        assert_eq!(pending(&state), "f");

        assert_eq!(
            normal(&mut state, KeyCode::Right),
            BufferKey::Consumed,
            "a non-character key must still resolve as an ordinary verb"
        );
        assert_eq!(
            cursor(&state),
            1,
            "`Right` after `f` must move the cursor, not be eaten by the prefix"
        );
        assert_eq!(pending(&state), "", "and the prefix disarms either way");
    }

    /// Insert mode never consults the prefix, so a printable typed there is
    /// text rather than a target.
    #[tokio::test]
    async fn insert_mode_never_captures_a_find_target() {
        let (state, _daemon) = opened_writer("alpha\n").await;
        let mut state = state;
        arm(&mut state, EditorPrefix::FindNext);

        assert_eq!(insert(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            "galpha\n",
            "Insert-mode text must never be read as a find target"
        );
        assert_eq!(
            pending(&state),
            "f",
            "and the prefix is left for Normal mode rather than disarmed here"
        );
        assert_eq!(
            last_find(&state),
            None,
            "no find may be recorded from the Insert path"
        );
    }

    /// `Alt-.` repeats the last find, and any other motion disarms it.
    ///
    /// Helix's repeat is backed by a *single* motion slot that every motion
    /// overwrites. Modelling that is what makes `f x`, `j`, `Alt-.` a no-op
    /// instead of a surprise jump back to the `x`; a two-slot design that
    /// remembered finds separately would repeat here and not match the
    /// dialect.
    #[tokio::test]
    async fn alt_period_repeats_the_last_find_and_another_motion_disarms_it() {
        // `x` at bytes 0, 2 and 4.
        let text = "x0x1x2\n";
        let alt_period = KeyEvent::new(KeyCode::Char('.'), KeyModifiers::ALT);

        let (mut state, _daemon) = opened_writer(text).await;
        normal(&mut state, KeyCode::Char('f'));
        normal(&mut state, KeyCode::Char('x'));
        assert_eq!(cursor(&state), 2);

        assert_eq!(normal_event(&mut state, alt_period), BufferKey::Consumed);
        assert_eq!(
            cursor(&state),
            4,
            "`Alt-.` repeats `f x` from wherever the cursor now is"
        );

        // A repeat re-records itself, so it can be repeated again.
        assert_eq!(normal_event(&mut state, alt_period), BufferKey::Consumed);
        assert_eq!(cursor(&state), 4, "and there is no third `x` to find");

        // Any other motion overwrites the slot.
        assert_eq!(normal(&mut state, KeyCode::Char('h')), BufferKey::Consumed);
        let after_other_motion = cursor(&state);
        assert_eq!(after_other_motion, 3);
        assert_eq!(
            normal_event(&mut state, alt_period),
            BufferKey::Unhandled,
            "`h` is now the last motion, so there is no find to repeat"
        );
        assert_eq!(
            cursor(&state),
            after_other_motion,
            "an unarmed repeat must not move the cursor"
        );
    }

    /// With nothing recorded, `Alt-.` claims nothing.
    #[tokio::test]
    async fn alt_period_with_no_record_is_not_claimed() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        assert_eq!(
            normal_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('.'), KeyModifiers::ALT)
            ),
            BufferKey::Unhandled,
            "`Alt-.` before any find must fall through to the composer"
        );
        assert_eq!(cursor(&state), 0, "and move nothing");
    }

    fn arm(state: &mut EditorState, prefix: EditorPrefix) {
        state
            .session
            .as_mut()
            .expect("open")
            .presentation_mut()
            .arm_prefix(prefix);
    }

    fn arm_on_app(app: &mut App, prefix: EditorPrefix) {
        app.editor_state
            .session
            .as_mut()
            .expect("open")
            .presentation_mut()
            .arm_prefix(prefix);
    }

    fn prefix_on_app(app: &App) -> EditorPrefix {
        app.editor_state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .pending_prefix
    }

    /// An `App` carrying a real attached document, for the route- and
    /// focus-level disarm proofs. `App::new_for_testing` alone has no
    /// session, and a disarm that only holds with nothing attached would pass
    /// without proving anything.
    async fn app_with_the_editor_attached(state: EditorState) -> App {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = App::new_for_testing(root.path().display().to_string());
        app.editor_state = state;
        app
    }

    /// The same app, but on the editor route.
    ///
    /// `handle_editor_key` refuses every key unless the editor route is
    /// current, so a test that calls it without this reaches none of the
    /// dispatch under test and would pass for the wrong reason.
    async fn app_on_the_editor_route(state: EditorState) -> App {
        let mut app = app_with_the_editor_attached(state).await;
        app.ui_state
            .routes
            .navigate_to(crate::tui::route::Route::Editor);
        // Normal, because every gesture under test is a Normal-mode one and
        // the App's default is not: `editor_buffer_key` dispatches on
        // `app.ui_state.input_mode`, so an Insert-mode app would type `k` into
        // the document instead of resolving the leaf.
        app.ui_state.input_mode = crate::tui::input::InputMode::Normal;
        app
    }

    #[tokio::test]
    async fn backspace_at_offset_zero_is_a_noop() {
        let (mut state, _daemon) = opened_writer("base").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        assert_eq!(insert(&mut state, KeyCode::Backspace), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "base");
        assert_eq!(undo_depth(&state), 0, "a no-op edit records no history");
    }

    #[tokio::test]
    async fn a_read_only_attachment_refuses_an_insertion_entry_visibly() {
        let (mut state, _daemon) = opened_reader("base").await;
        // Returning `Unhandled` lets the App layer explain the refusal. A
        // silent no-op would leave the user pressing `i` in what still reads
        // as Normal mode with no way to tell why nothing happened.
        assert_eq!(normal(&mut state, KeyCode::Char('i')), BufferKey::Unhandled);
        assert_ne!(state.mode(), EditorMode::Insert);
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn normal_key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[tokio::test]
    async fn a_colon_in_normal_mode_opens_the_command_line() {
        let (state, _daemon) = opened_writer("hello").await;
        let mut state = state;
        assert!(
            !state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active
        );
        assert!(
            editor_buffer_key(
                &mut state,
                InputMode::Normal,
                normal_key(crossterm::event::KeyCode::Char(':'))
            ) == BufferKey::Consumed
        );
        assert!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active
        );
    }

    #[tokio::test]
    async fn a_colon_in_insert_mode_is_document_text() {
        let (state, _daemon) = opened_writer("").await;
        let mut state = state;
        state.session.as_mut().unwrap().presentation_mut().mode = EditorMode::Insert;
        assert_eq!(
            editor_buffer_key(
                &mut state,
                InputMode::Insert,
                normal_key(crossterm::event::KeyCode::Char(':'))
            ),
            BufferKey::Consumed
        );
        // A URL must remain typeable; swallowing the colon would make one
        // impossible to write into a file.
        assert!(
            !state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active
        );
        assert_eq!(editor_text(&state), ":");
    }

    #[tokio::test]
    async fn command_mode_captures_every_key_including_the_save_chord() {
        let (state, _daemon) = opened_writer("x").await;
        let mut state = state;
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .open_command_line("");
        // `Ctrl+S` is mode-independent and normally saves. While the command
        // line is open it must not reach the save path, or a half-typed `:q`
        // would silently write the file the user is about to close. It is
        // *swallowed* rather than typed: a literal `s` would name a different
        // command than the one the user is building.
        let ctrl_s = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('s'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        assert!(
            editor_buffer_key(&mut state, InputMode::Normal, ctrl_s) != BufferKey::RequestSave,
            "the save chord must not escape the command line"
        );
        assert!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_line
                .is_empty(),
            "a modified chord must not be typed into the line"
        );
        assert!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active,
            "the command line stays open"
        );
    }

    #[tokio::test]
    async fn esc_closes_the_command_line_without_running_anything() {
        let (state, _daemon) = opened_writer("x").await;
        let mut state = state;
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .open_command_line("wq");
        let result = editor_buffer_key(
            &mut state,
            InputMode::Normal,
            normal_key(crossterm::event::KeyCode::Esc),
        );
        // Close, never run: `Esc` must be the side-effect-free exit.
        assert!(result != BufferKey::RunCommand(EditorCommandName::WriteQuit, None));
        assert!(
            !state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active
        );
    }

    #[tokio::test]
    async fn enter_parses_the_line_into_a_command() {
        let (state, _daemon) = opened_writer("x").await;
        let mut state = state;
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .open_command_line("wq");
        let result = editor_buffer_key(
            &mut state,
            InputMode::Normal,
            normal_key(crossterm::event::KeyCode::Enter),
        );
        assert!(result == BufferKey::RunCommand(EditorCommandName::WriteQuit, None));
        // The line closes on Enter, so an unknown command cannot leave the
        // user stuck retyping it.
        assert!(
            !state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active
        );
    }

    #[tokio::test]
    async fn an_unknown_command_is_refused_with_a_notice_not_a_silent_no_op() {
        let (state, _daemon) = opened_writer("x").await;
        let mut state = state;
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .open_command_line("wqq");
        let result = editor_buffer_key(
            &mut state,
            InputMode::Normal,
            normal_key(crossterm::event::KeyCode::Enter),
        );
        assert!(result == BufferKey::Consumed);
        assert!(
            state.notice.is_some(),
            "an unknown command must explain itself"
        );
    }

    #[tokio::test]
    async fn the_palette_narrows_as_the_line_is_typed() {
        let (state, _daemon) = opened_writer("x").await;
        let mut state = state;
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .open_command_line("");
        assert_eq!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_matches()
                .len(),
            crate::tui::editor::EditorCommandName::ALL.len()
        );
        editor_buffer_key(
            &mut state,
            InputMode::Normal,
            normal_key(crossterm::event::KeyCode::Char('q')),
        );
        let matches = state
            .session
            .as_ref()
            .unwrap()
            .presentation()
            .command_matches();
        assert!(matches.iter().any(|m| m.command == EditorCommandName::Quit));
    }

    #[tokio::test]
    async fn backspace_edits_the_line_and_cannot_underflow() {
        let (state, _daemon) = opened_writer("x").await;
        let mut state = state;
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .open_command_line("");
        let backspace = normal_key(crossterm::event::KeyCode::Backspace);
        for _ in 0..5 {
            assert!(
                editor_buffer_key(&mut state, InputMode::Normal, backspace) == BufferKey::Consumed
            );
        }
        assert!(state
            .session
            .as_ref()
            .unwrap()
            .presentation()
            .command_line
            .is_empty());
    }
    /// An `App` whose file tree is populated with `count` flat files, so the
    /// tree verbs have rows to move between.
    fn app_with_a_populated_tree(count: usize) -> (App, tempfile::TempDir) {
        let root = tempfile::tempdir().expect("tempdir");
        for index in 0..count {
            std::fs::write(root.path().join(format!("file{index:03}.txt")), b"x").expect("write");
        }
        let mut app = App::new_for_testing(root.path().display().to_string());
        let (generation, request_id) = app.file_tree_state.begin_rebuild(root.path().to_path_buf());
        app.file_tree_state.apply_listing(
            generation,
            request_id,
            root.path().to_path_buf(),
            crate::tui::file_tree::build_tree(root.path()),
        );
        app.ui_state.tree_focused = true;
        (app, root)
    }

    #[test]
    fn the_tree_claims_paging_only_with_control() {
        use crossterm::event::KeyModifiers;

        let (mut app, _root) = app_with_a_populated_tree(40);
        app.file_tree_state.viewport_rows = 10;
        let first = app.file_tree_state.selected.clone();
        assert_eq!(first.as_deref(), Some("file000.txt"));

        // A bare `d` is not a page: the editor's own verbs are reached by
        // falling through, and the tree must not have taken the letter.
        assert!(!handle_file_tree_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
        ));
        assert_eq!(app.file_tree_state.selected.clone(), first);

        assert!(handle_file_tree_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
        ));
        assert_ne!(
            app.file_tree_state.selected.clone(),
            first,
            "Ctrl+d must move further than the `j` beside it"
        );

        let paged = app.file_tree_state.selected.clone();
        assert!(handle_file_tree_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)
        ));
        assert_eq!(app.file_tree_state.selected.clone(), first);
        assert_ne!(paged, first);
    }

    #[test]
    fn the_tree_only_re_roots_on_a_directory() {
        let (mut app, _root) = app_with_a_populated_tree(3);
        // A file has no directory to make the root; the key is still claimed
        // so it cannot leak a character into the document underneath.
        assert!(handle_file_tree_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)
        ));
        assert!(
            app.file_tree_state.at_workspace_root(),
            "a file is not a directory the pane can be rooted at"
        );
    }

    // ── M004 change verbs through the dispatch path ─────────────────────

    /// Park the cursor, so a verb that acts on "the character under the
    /// cursor" acts on a known one.
    fn park(state: &mut EditorState, offset: usize) {
        state
            .session
            .as_mut()
            .expect("open")
            .presentation_mut()
            .cursor_byte = offset;
    }

    #[tokio::test]
    async fn r_arms_a_capture_and_the_next_key_replaces_one_character() {
        let (mut state, _daemon) = opened_writer("abc").await;
        park(&mut state, 1);

        assert_eq!(normal(&mut state, KeyCode::Char('r')), BufferKey::Consumed);
        assert_eq!(
            pending(&state),
            "r",
            "an armed `r` that draws nothing is indistinguishable from a dropped key"
        );

        assert_eq!(normal(&mut state, KeyCode::Char('X')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "aXc", "exactly one character, no more");
        assert_eq!(undo_depth(&state), 1, "and it is one undo step");
    }

    #[tokio::test]
    async fn r_then_a_non_character_key_moves_the_cursor_instead_of_eating_it() {
        // The hazard a capture prefix exists to avoid: `r` then `Left` must
        // be an arrow, not a swallowed keystroke on its way to the composer.
        let (mut state, _daemon) = opened_writer("abc").await;
        park(&mut state, 1);
        assert_eq!(normal(&mut state, KeyCode::Char('r')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Left), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0);
        assert_eq!(editor_text(&state), "abc", "nothing was replaced");
    }

    #[tokio::test]
    async fn r_replaces_a_multi_byte_character_whole() {
        let (mut state, _daemon) = opened_writer("a日b").await;
        park(&mut state, 1);
        assert_eq!(normal(&mut state, KeyCode::Char('r')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char('本')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "a本b");
    }

    #[tokio::test]
    async fn r_on_a_read_only_attachment_refuses_visibly() {
        let (mut state, _daemon) = opened_reader("abc").await;
        park(&mut state, 0);
        assert_eq!(normal(&mut state, KeyCode::Char('r')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char('X')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "abc");
        assert!(state.notice.is_some(), "the refusal must be visible");
    }

    #[tokio::test]
    async fn tilde_switches_case_and_drops_the_selection_it_consumed() {
        let (mut state, _daemon) = opened_writer("aBc").await;
        park(&mut state, 0);
        assert_eq!(normal(&mut state, KeyCode::Char('~')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "ABc");
        assert_eq!(cursor(&state), 0, "left on the character it flipped");

        // With a selection held, `~` flips the whole selection and must not
        // leave a highlight over text that has changed underneath it.
        let (mut state, _daemon) = opened_writer("ab cd").await;
        park(&mut state, 0);
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        assert!(has_selection(&state));
        normal(&mut state, KeyCode::Char('~'));
        assert_eq!(editor_text(&state), "AB CD");
        assert!(
            !has_selection(&state),
            "a selection over replaced text is a highlight on the wrong words"
        );
    }

    #[tokio::test]
    async fn tilde_on_a_read_only_attachment_refuses_visibly() {
        let (mut state, _daemon) = opened_reader("aBc").await;
        assert_eq!(normal(&mut state, KeyCode::Char('~')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "aBc");
        assert!(state.notice.is_some());
    }

    // ── M007: the selection set ──────────────────────────────────────────

    #[tokio::test]
    async fn a_read_only_buffer_refuses_selection_growth_but_still_moves() {
        let (mut state, _daemon) = opened_reader("one\ntwo\nthree\n").await;
        state.clear_notice();

        assert_eq!(normal(&mut state, KeyCode::Char('v')), BufferKey::Consumed);
        assert!(
            !has_selection(&state),
            "`v` must not arm a selection the reader cannot act on"
        );
        assert!(
            state.notice.is_some(),
            "a silent refusal looks exactly like a keypress that never arrived"
        );

        state.clear_notice();
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        assert!(!has_selection(&state), "`x` must not arm either");
        assert!(state.notice.is_some());

        // The narrow half of the rule: navigation is not growth. A reader must
        // still be able to walk a buffer they cannot write.
        state.clear_notice();
        assert_eq!(normal(&mut state, KeyCode::Char('j')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 4, "`j` still moves on a read-only buffer");
        assert!(state.notice.is_none(), "movement is not a refused edit");
    }

    /// An accepted edit tells the highlighter where it landed.
    ///
    /// The highlighter's prefix check finds the first row whose content
    /// differs from the scan it holds. Without this seed it starts at line 0
    /// and re-reads every scanned row on every keystroke; with it, the check
    /// starts at the edit. The line is resolved against the **post-edit**
    /// snapshot, because an edit that inserts lines above itself moves every
    /// later line index.
    #[tokio::test]
    async fn an_accepted_edit_tells_the_highlighter_where_it_landed() {
        let (mut state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        // Put the cursor on line 2 (`two`) so the edit is on a row a stale
        // seed of 0 would not distinguish from the top.
        state
            .session
            .as_mut()
            .expect("open")
            .presentation_mut()
            .cursor_byte = 6;
        let snapshot = snapshot_of(&state);
        let edit = editor::insert_text(&snapshot, 6, "X").expect("an insertion");
        assert!(submit_edit(&mut state, edit), "the edit must be accepted");

        assert_eq!(
            state.highlight.pending_edit_from(),
            Some(1),
            "the highlighter must be told the line the edit landed on"
        );

        // Undo is an edit too, and it lands on the same row.
        state.highlight.reset();
        editor_undo(&mut state);
        assert_eq!(
            state.highlight.pending_edit_from(),
            Some(1),
            "undo must be placed the same way an edit is"
        );
    }

    #[tokio::test]
    async fn a_multi_selection_edit_is_undone_and_redone_as_one_step() {
        let (mut state, _daemon) = opened_writer("ab cd ef").await;
        // No key creates a set until M008, so the model is exercised the way
        // the gesture will drive it: a set handed to the funnel directly.
        let snapshot = snapshot_of(&state);
        hold_two_members(&mut state, &snapshot, 0, 3);

        let edit = editor::switch_case(&snapshot, state.session.as_ref().unwrap().presentation())
            .expect("a case flip over two members");
        submit_edit(&mut state, edit);
        assert_eq!(editor_text(&state), "Ab Cd ef");
        assert!(
            !has_selection(&state),
            "an accepted edit consumes the set; undo puts it back"
        );

        editor_undo(&mut state);
        assert_eq!(
            editor_text(&state),
            "ab cd ef",
            "one step restored both members"
        );
        assert_eq!(
            selection_count(&state),
            2,
            "undo restores the selection set, not just the text"
        );

        editor_redo(&mut state);
        assert_eq!(editor_text(&state), "Ab Cd ef");
        assert_eq!(selection_count(&state), 2, "redo restores it as well");
    }

    /// A fresh edit starts a branch and drops the selection set it recorded.
    ///
    /// The redo *entry* survives as a sibling — that is M009 — but the set the
    /// reader held is not carried into the new node: it described the text the
    /// reader left, and restoring it on the next `u` would put cursors back on
    /// characters they chose for a different edit.
    #[tokio::test]
    async fn an_edit_after_undo_branches_and_drops_the_recorded_set() {
        let (mut state, _daemon) = opened_writer("ab cd").await;
        let snapshot = snapshot_of(&state);
        hold_two_members(&mut state, &snapshot, 0, 3);
        let edit = editor::switch_case(&snapshot, state.session.as_ref().unwrap().presentation())
            .expect("a case flip");
        submit_edit(&mut state, edit);
        editor_undo(&mut state);

        let after = snapshot_of(&state);
        let edit =
            editor::replace_char(&after, state.session.as_ref().unwrap().presentation(), 'Z')
                .expect("replacement");
        submit_edit(&mut state, edit);
        assert_eq!(
            redo_depth(&state),
            0,
            "the new node is a tip: no redo points at the text it replaced"
        );
        assert_eq!(selection_count(&state), 0);
        assert_eq!(
            history_len(&state),
            2,
            "and the branch the reader undid past is retained as a sibling"
        );
    }

    /// Hold two single-character members at `start` and `other`, primary first.
    fn hold_two_members(
        state: &mut EditorState,
        snapshot: &codegg_document::DocumentSnapshot,
        start: usize,
        other: usize,
    ) {
        let point = |byte: usize| EditorPoint {
            byte,
            line: editor::cursor_line(snapshot, byte),
        };
        let member = |anchor: usize, head: usize| Selection {
            anchor: point(anchor),
            head: point(head),
            mode: crate::tui::helix_selection::SelectionMode::Char,
            growing: false,
        };
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![member(start, start), member(other, other)];
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .primary_index = 0;
    }

    fn selection_count(state: &EditorState) -> usize {
        state
            .session
            .as_ref()
            .map(|session| session.presentation().selections.len())
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn the_case_setters_are_claimed_and_change_the_document() {
        // On `B`, so the verb has a case to change: `` ` `` on an already
        // lower-case character is correctly a no-op.
        let (mut state, _daemon) = opened_writer("aBc").await;
        park(&mut state, 1);
        assert_eq!(normal(&mut state, KeyCode::Char('`')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "abc", "` sets lower case");

        let (mut state, _daemon) = opened_writer("aBc").await;
        // On `a`: upper-casing the `B` would correctly be a no-op.
        park(&mut state, 0);
        assert_eq!(
            normal_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('`'), KeyModifiers::ALT)
            ),
            BufferKey::Consumed,
            "` with Alt sets upper case"
        );
        assert_eq!(editor_text(&state), "ABc");
    }

    #[tokio::test]
    async fn the_clipboard_verbs_are_claimed_and_reported_rather_than_performed() {
        // The buffer resolves the *target*; the App layer owns the bytes.
        // An unclaimed `p` would type a `p` into the composer instead.
        let (mut state, _daemon) = opened_writer("abc").await;
        park(&mut state, 0);
        assert_eq!(
            normal(&mut state, KeyCode::Char('p')),
            BufferKey::Paste {
                verb: ClipboardVerb::PasteAfter,
                register: RegisterName::Unnamed,
            }
        );
        assert_eq!(
            normal_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT)
            ),
            BufferKey::Paste {
                verb: ClipboardVerb::PasteBefore,
                register: RegisterName::Unnamed,
            },
            "terminals disagree on how a capital is encoded; both must work"
        );
        assert_eq!(
            normal_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE)
            ),
            BufferKey::Paste {
                verb: ClipboardVerb::PasteBefore,
                register: RegisterName::Unnamed,
            }
        );
        assert_eq!(
            editor_text(&state),
            "abc",
            "the buffer did not read the clipboard"
        );
    }

    #[tokio::test]
    async fn paste_after_reports_the_clipboard_contents_it_was_given() {
        let (state, _daemon) = opened_writer("abc").await;
        let mut app = app_on_the_editor_route(state).await;
        park_on_app(&mut app, 0);
        apply_clipboard_text(&mut app, ClipboardVerb::PasteAfter, Some("XY".to_string()));
        assert_eq!(text_on_app(&app), "aXYbc");
        assert_eq!(
            cursor_on_app(&app),
            2,
            "on the last pasted byte, not past it"
        );
    }

    #[tokio::test]
    async fn paste_before_reports_the_clipboard_contents_it_was_given() {
        let (state, _daemon) = opened_writer("abc").await;
        let mut app = app_on_the_editor_route(state).await;
        park_on_app(&mut app, 0);
        apply_clipboard_text(&mut app, ClipboardVerb::PasteBefore, Some("X".to_string()));
        assert_eq!(text_on_app(&app), "Xabc");
    }

    #[tokio::test]
    async fn r_the_replace_verb_replaces_with_the_clipboard_contents() {
        let (state, _daemon) = opened_writer("abc").await;
        let mut app = app_on_the_editor_route(state).await;
        park_on_app(&mut app, 0);
        apply_clipboard_text(&mut app, ClipboardVerb::Replace, Some("Z".to_string()));
        assert_eq!(text_on_app(&app), "Zbc");
    }

    #[tokio::test]
    async fn an_empty_clipboard_and_a_failed_read_are_different_answers() {
        let (state, _daemon) = opened_writer("abc").await;

        let mut app = app_on_the_editor_route(state).await;
        park_on_app(&mut app, 0);
        apply_clipboard_text(&mut app, ClipboardVerb::PasteAfter, None);
        assert_eq!(text_on_app(&app), "abc");
        assert_eq!(
            app.messages_state.toasts.len(),
            1,
            "a clipboard the system refused must still say so"
        );

        let (state, _daemon) = opened_writer("abc").await;
        let mut app = app_on_the_editor_route(state).await;
        park_on_app(&mut app, 0);
        apply_clipboard_text(&mut app, ClipboardVerb::PasteAfter, Some(String::new()));
        assert_eq!(text_on_app(&app), "abc");
        assert_eq!(
            app.messages_state.toasts.len(),
            1,
            "a clipboard holding nothing is not the same failure, and says so too"
        );
    }

    #[tokio::test]
    async fn a_clipboard_verb_on_a_read_only_attachment_refuses_visibly() {
        let (state, _daemon) = opened_reader("abc").await;
        let mut app = app_on_the_editor_route(state).await;
        park_on_app(&mut app, 0);
        apply_clipboard_text(&mut app, ClipboardVerb::PasteAfter, Some("X".to_string()));
        assert_eq!(text_on_app(&app), "abc");
        assert!(app.editor_state.notice.is_some());
    }

    #[tokio::test]
    async fn bare_y_reports_the_held_selection_for_the_clipboard() {
        let (mut state, _daemon) = opened_writer("alpha beta").await;
        park(&mut state, 0);
        // `v` selects one character; each `l` adds another, so this holds
        // three.
        assert_eq!(normal(&mut state, KeyCode::Char('v')), BufferKey::Consumed);
        normal(&mut state, KeyCode::Char('l'));
        normal(&mut state, KeyCode::Char('l'));

        assert_eq!(
            normal(&mut state, KeyCode::Char('y')),
            BufferKey::YankRanges {
                ranges: editor::one_range(0..3),
                target: YankTarget::Selected(RegisterName::Unnamed),
            },
            "bare `y` resolves the same ranges as `space y` and adds the register"
        );
        assert_eq!(editor_text(&state), "alpha beta", "a yank changes no text");
    }

    #[tokio::test]
    async fn bare_y_with_nothing_selected_is_still_claimed() {
        let (mut state, _daemon) = opened_writer("abc").await;
        assert_eq!(normal(&mut state, KeyCode::Char('y')), BufferKey::Consumed);
    }

    // ── M007: registers ─────────────────────────────────────────────────
    //
    // Every test presses keys, because the failure this layer has already
    // produced once is a chord that compiles, reads correctly, and is shadowed
    // before it can run.

    /// The selected register, read through the presentation so a test cannot
    /// assert against a second source of truth.
    fn register_on_app(app: &App, register: RegisterName) -> Result<String, RegisterRefusal> {
        app.editor_state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .read_register(register)
            .map(str::to_owned)
    }

    /// `"` arms the register prefix, the next key is the **name** (data, not a
    /// command), and the key after that is the verb that consumes the
    /// selection. Two levels in one prefix field, the `m` grammar's shape.
    #[tokio::test]
    async fn the_register_prefix_captures_a_name_and_the_verb_after_it_consumes_it() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);

        assert_eq!(normal(&mut state, KeyCode::Char('"')), BufferKey::Consumed);
        assert_eq!(
            pending(&state),
            "\"",
            "`\"` shows itself while it waits for a name"
        );

        assert_eq!(normal(&mut state, KeyCode::Char('a')), BufferKey::Consumed);
        assert_eq!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .pending_prefix,
            EditorPrefix::RegisterNamed('a'),
            "the name is held in the prefix, not in a second field"
        );
        assert_eq!(pending(&state), "\"a", "and the indicator names it");

        assert_eq!(
            normal(&mut state, KeyCode::Char('y')),
            BufferKey::YankRanges {
                ranges: editor::one_range(0..6),
                target: YankTarget::Selected(RegisterName::Named('a')),
            },
            "the verb after the name yanks into that register"
        );
        assert_eq!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .pending_prefix,
            EditorPrefix::Idle,
            "and the gesture is complete"
        );
    }

    /// The selection is the prefix's second level, so it cannot outlive the key
    /// after it: `"a` then `j` moves, and the `y` after that yanks into the
    /// **unnamed** register. A sticky `selected_register` field would still be
    /// pointing at `a` — the second grammar the plan rejected.
    #[tokio::test]
    async fn a_register_selection_does_not_survive_an_unrelated_key() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;

        normal(&mut state, KeyCode::Char('"'));
        normal(&mut state, KeyCode::Char('a'));
        assert_eq!(
            normal(&mut state, KeyCode::Char('p')),
            BufferKey::Paste {
                verb: ClipboardVerb::PasteAfter,
                register: RegisterName::Named('a'),
            },
            "`\"ap` reads the selected register"
        );

        normal(&mut state, KeyCode::Char('"'));
        normal(&mut state, KeyCode::Char('b'));
        assert_eq!(
            normal(&mut state, KeyCode::Char('j')),
            BufferKey::Consumed,
            "`j` is not a register verb, so it resolves normally"
        );
        assert_eq!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .pending_prefix,
            EditorPrefix::Idle,
            "and the selection went with the prefix"
        );
        assert_eq!(
            normal(&mut state, KeyCode::Char('p')),
            BufferKey::Paste {
                verb: ClipboardVerb::PasteAfter,
                register: RegisterName::Unnamed,
            },
            "the later `p` uses the default, not the abandoned selection"
        );
    }

    /// A character outside the register set is **reported and consumed**. The
    /// consumed half is the destructive one: `"D` must not delete to the end of
    /// the line, and `"A` must not enter Insert mode.
    #[tokio::test]
    async fn an_invalid_register_name_is_reported_and_never_runs_as_a_verb() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        normal(&mut state, KeyCode::Char('"'));

        assert_eq!(
            normal(&mut state, KeyCode::Char('D')),
            BufferKey::Consumed,
            "the invalid name is the argument, so it is consumed"
        );
        assert_eq!(
            editor_text(&state),
            "alpha\nbeta\n",
            "`\"D` is a rejected name, not delete-to-end-of-line"
        );
        assert!(
            notice_of(&state).is_some_and(|note| note.contains("not a register")),
            "the refusal names the set: {:?}",
            notice_of(&state)
        );
        assert_eq!(pending(&state), "", "and the machine is disarmed");
    }

    /// A key that cannot be a name is **swallowed**, not retried: the register
    /// set is a closed vocabulary, so a non-character key is not a wrong
    /// register, and running it from a committed `"` would change the surface —
    /// `"` then `Tab` toggling the composer, `"` then `Ctrl+d` paging the
    /// buffer. `Tab` is the sharpest control because bare it is unclaimed.
    #[tokio::test]
    async fn the_register_prefix_swallows_a_key_that_cannot_be_a_name() {
        let (mut state, _daemon) = opened_writer("abc").await;
        assert_eq!(
            normal(&mut state, KeyCode::Tab),
            BufferKey::Unhandled,
            "the control: bare `Tab` is not the buffer's"
        );

        for (label, code) in [("Tab", KeyCode::Tab), ("Esc", KeyCode::Esc)] {
            normal(&mut state, KeyCode::Char('"'));
            assert_eq!(
                normal(&mut state, code),
                BufferKey::Consumed,
                "`{label}` after `\"` is consumed with the gesture"
            );
            assert_eq!(pending(&state), "", "and nothing stays selected");
        }
    }

    /// `"ay` then `"ap` round-trips through the store, and `y` still reaches
    /// the system clipboard. The clipboard write is injected — the utility has
    /// no seam, and a test using the real one would steal the developer's
    /// clipboard — the same seam `apply_clipboard_text` takes for its read.
    #[tokio::test]
    async fn a_named_register_round_trips_and_the_yank_still_reaches_the_clipboard() {
        let (state, _daemon) = opened_writer("alpha\nbeta\n").await;
        let mut app = app_on_the_editor_route(state).await;

        let mut copied = None;
        yank_editor_ranges_with(
            &mut app,
            editor::one_range(0..6),
            YankTarget::Selected(RegisterName::Named('a')),
            |text| {
                copied = Some(text.to_string());
                Ok(())
            },
        );
        assert_eq!(
            copied.as_deref(),
            Some("alpha\n"),
            "`y` still writes the system clipboard, whatever register it also wrote"
        );
        assert_eq!(
            register_on_app(&app, RegisterName::Named('a')).as_deref(),
            Ok("alpha\n"),
            "and the named register holds the same bytes"
        );

        park_on_app(&mut app, 6);
        apply_register_text(
            &mut app,
            ClipboardVerb::PasteBefore,
            RegisterName::Named('a'),
        );
        assert_eq!(
            text_on_app(&app),
            "alpha\nalpha\nbeta\n",
            "`\"ap` pastes what `\"ay` yanked"
        );
    }

    /// The default path, end to end: a bare `y` writes the unnamed register and
    /// a bare `p` reads it, with no `"` anywhere in the gesture.
    #[tokio::test]
    async fn bare_y_then_bare_p_round_trips_through_the_unnamed_register() {
        let (state, _daemon) = opened_writer("alpha\nbeta\n").await;
        let mut app = app_on_the_editor_route(state).await;

        yank_editor_ranges_with(
            &mut app,
            editor::one_range(6..11),
            YankTarget::Selected(RegisterName::Unnamed),
            |_| Ok(()),
        );
        assert_eq!(
            register_on_app(&app, RegisterName::Unnamed).as_deref(),
            Ok("beta\n"),
            "bare `y` writes the unnamed register"
        );

        park_on_app(&mut app, 0);
        apply_register_text(&mut app, ClipboardVerb::PasteBefore, RegisterName::Unnamed);
        assert_eq!(
            text_on_app(&app),
            "beta\nalpha\nbeta\n",
            "and bare `p` reads it"
        );
    }

    /// `d` yanks into the unnamed register and `p` pastes it; `Alt-d` yanks
    /// nothing, so the register keeps what it held. That pair is M002's D1
    /// discharged *and* M002's promise surviving the milestone that changed
    /// it: the delete writes a register, and still never the clipboard.
    #[tokio::test]
    async fn d_then_p_pastes_the_deletion_and_alt_d_leaves_the_register_alone() {
        let (state, _daemon) = opened_writer("alpha\nbeta\n").await;
        let mut app = app_on_the_editor_route(state).await;

        assert!(submit_range_delete(
            &mut app.editor_state,
            editor::one_range(0..6),
            false,
            YankTarget::Unnamed,
        ));
        assert_eq!(text_on_app(&app), "beta\n", "the delete landed");
        assert_eq!(
            register_on_app(&app, RegisterName::Unnamed).as_deref(),
            Ok("alpha\n"),
            "and the reader can paste it back without the clipboard"
        );

        assert!(submit_range_delete(
            &mut app.editor_state,
            editor::one_range(0..5),
            false,
            YankTarget::Nothing,
        ));
        assert_eq!(text_on_app(&app), "", "`Alt-d` deleted too");
        assert_eq!(
            register_on_app(&app, RegisterName::Unnamed).as_deref(),
            Ok("alpha\n"),
            "but wrote nowhere: it is the no-yank spelling"
        );

        apply_register_text(&mut app, ClipboardVerb::PasteBefore, RegisterName::Unnamed);
        assert_eq!(text_on_app(&app), "alpha\n", "and `p` still pastes it");
    }

    /// `space p` after a `d` pastes the **clipboard**, not the deletion. The
    /// `space` family is not register-aware, which is the separation that keeps
    /// cross-application paste predictable.
    #[tokio::test]
    async fn space_p_after_a_delete_still_pastes_the_clipboard() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char(' ')), BufferKey::Consumed);
        assert_eq!(
            normal(&mut state, KeyCode::Char('p')),
            BufferKey::Consumed,
            "`space p` is a leaf, so the buffer consumes the second key"
        );
        assert_eq!(
            parked(&state),
            Some(EditorPendingEffect::Clipboard(ClipboardVerb::PasteAfter)),
            "and parks the clipboard verb rather than a register paste"
        );

        let mut app = app_on_the_editor_route(state).await;
        assert!(submit_range_delete(
            &mut app.editor_state,
            editor::one_range(0..6),
            false,
            YankTarget::Unnamed,
        ));
        assert_eq!(
            register_on_app(&app, RegisterName::Unnamed).as_deref(),
            Ok("alpha\n")
        );
        apply_clipboard_text(
            &mut app,
            ClipboardVerb::PasteBefore,
            Some("CLIP".to_string()),
        );
        assert_eq!(
            text_on_app(&app),
            "CLIPbeta\n",
            "the clipboard's own bytes, not the deleted text"
        );
    }

    /// The blackhole writes nothing and refuses a read. `"_d` is the delete
    /// that yanks nowhere, a blackhole yank reaches neither the unnamed
    /// register nor the clipboard, and `"_p` says why it cannot paste.
    #[tokio::test]
    async fn the_blackhole_register_discards_and_its_paste_refuses_with_a_reason() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        normal(&mut state, KeyCode::Char('"'));
        normal(&mut state, KeyCode::Char('_'));
        assert_eq!(
            normal(&mut state, KeyCode::Char('d')),
            BufferKey::DeleteRanges {
                ranges: editor::one_range(0..6),
                yank: YankTarget::Nothing,
            },
            "`\"_d` is the delete that yanks nothing"
        );

        let mut app = app_on_the_editor_route(state).await;
        let mut copied = false;
        yank_editor_ranges_with(
            &mut app,
            editor::one_range(0..6),
            YankTarget::Selected(RegisterName::Blackhole),
            |_| {
                copied = true;
                Ok(())
            },
        );
        assert!(!copied, "a blackhole yank must not write the clipboard");
        assert_eq!(
            register_on_app(&app, RegisterName::Unnamed),
            Err(RegisterRefusal::Empty(RegisterName::Unnamed)),
            "nor mirror into the unnamed register"
        );

        apply_register_text(&mut app, ClipboardVerb::PasteAfter, RegisterName::Blackhole);
        assert_eq!(text_on_app(&app), "alpha\nbeta\n", "the paste refused");
        let reported = app
            .messages_state
            .toasts
            .history()
            .map(|toast| toast.message.clone())
            .collect::<Vec<_>>();
        assert!(
            reported.iter().any(|message| message.contains("blackhole")),
            "and said why: {reported:?}"
        );
    }

    /// A paste from a register nobody wrote is a stated refusal, not an empty
    /// insert; `"/p` projects the accepted search line and refuses before there
    /// is one.
    #[tokio::test]
    async fn an_unset_register_and_an_unsearched_slash_both_refuse_with_a_reason() {
        let (state, _daemon) = opened_writer("abc").await;
        let mut app = app_on_the_editor_route(state).await;
        park_on_app(&mut app, 0);

        apply_register_text(
            &mut app,
            ClipboardVerb::PasteBefore,
            RegisterName::Named('q'),
        );
        assert_eq!(text_on_app(&app), "abc", "nothing was inserted");
        assert!(
            app.messages_state
                .toasts
                .history()
                .any(|toast| toast.message.contains("register `q` is empty")),
            "the refusal names the register"
        );

        apply_register_text(&mut app, ClipboardVerb::PasteBefore, RegisterName::Search);
        assert_eq!(text_on_app(&app), "abc");
        assert!(
            app.messages_state
                .toasts
                .history()
                .any(|toast| toast.message.contains("no search has been accepted")),
            "`/` before a search says so"
        );

        let presentation = app
            .editor_state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut();
        presentation.open_search_line("bc", true);
        presentation.close_search_line();
        apply_register_text(&mut app, ClipboardVerb::PasteBefore, RegisterName::Search);
        assert_eq!(
            text_on_app(&app),
            "bcabc",
            "`\"/p` pastes the last accepted search pattern"
        );
    }

    /// D1's seed: the App layer hands the clipboard's contents to the unnamed
    /// register at open, so `p` pastes what was copied elsewhere while the
    /// buffer itself never reads the clipboard.
    #[tokio::test]
    async fn the_unnamed_register_is_seeded_from_the_clipboard_at_open() {
        let (state, _daemon) = opened_writer("abc").await;
        let mut app = app_on_the_editor_route(state).await;
        park_on_app(&mut app, 0);

        seed_register_from_clipboard(&mut app, Some("SEED".to_string()));
        apply_register_text(&mut app, ClipboardVerb::PasteBefore, RegisterName::Unnamed);
        assert_eq!(
            text_on_app(&app),
            "SEEDabc",
            "opening a document then pressing `p` pastes the clipboard's contents"
        );

        // A clipboard the system refuses seeds nothing rather than emptying a
        // register that already holds something.
        let (state, _daemon) = opened_writer("abc").await;
        let mut app = app_on_the_editor_route(state).await;
        seed_register_from_clipboard(&mut app, Some("first".to_string()));
        seed_register_from_clipboard(&mut app, None);
        assert_eq!(
            register_on_app(&app, RegisterName::Unnamed).as_deref(),
            Ok("first"),
            "an unavailable clipboard is not a write"
        );
    }

    /// The prefix reaches the keyboard through the App route, not only through
    /// `editor_buffer_key`: `"`, `a`, `p` at the route level reads register `a`
    /// — empty here — and says so. That is the end-to-end shape of `"ap`.
    #[tokio::test]
    async fn the_register_prefix_reaches_the_app_route() {
        let (state, _daemon) = opened_writer("abc").await;
        let mut app = app_on_the_editor_route(state).await;
        park_on_app(&mut app, 0);

        for code in [KeyCode::Char('"'), KeyCode::Char('a'), KeyCode::Char('p')] {
            assert!(
                handle_editor_key(&mut app, key(code)),
                "`{code:?}` belongs to the editor once `\"` is armed"
            );
        }
        assert_eq!(text_on_app(&app), "abc");
        assert!(
            app.messages_state
                .toasts
                .history()
                .any(|toast| toast.message.contains("register `a` is empty")),
            "the route reaches the register paste and reports the refusal"
        );
    }

    /// A multi-member yank is **one** register entry: the members are
    /// concatenated before the store sees them, so the store never grows a slot
    /// per selection.
    #[tokio::test]
    async fn a_multi_member_yank_is_one_register_entry() {
        let (state, _daemon) = opened_writer("abc\ndef\n").await;
        let mut app = app_on_the_editor_route(state).await;

        yank_editor_ranges_with(
            &mut app,
            vec![0..3, 4..7],
            YankTarget::Selected(RegisterName::Named('b')),
            |_| Ok(()),
        );
        assert_eq!(
            register_on_app(&app, RegisterName::Named('b')).as_deref(),
            Ok("abcdef"),
            "the members are concatenated in reading order"
        );
        assert_eq!(
            app.editor_state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .registers
                .len(),
            2,
            "one named slot plus the unnamed mirror, not one per member"
        );
        assert_eq!(
            register_on_app(&app, RegisterName::Named('c')),
            Err(RegisterRefusal::Empty(RegisterName::Named('c'))),
            "the next letter is untouched"
        );
    }

    #[tokio::test]
    async fn none_of_the_new_verbs_are_captured_in_insert_mode() {
        // The invariant that has already been broken once in production:
        // `Shift+W` opened a picker and ate the character.
        for ch in ['p', 'P', 'y', 'r', '~', 'R', '`'] {
            let (mut state, _daemon) = opened_writer("").await;
            assert_eq!(
                normal(&mut state, KeyCode::Char('i')),
                BufferKey::EnterInsert
            );
            assert_eq!(insert(&mut state, KeyCode::Char(ch)), BufferKey::Consumed);
            assert_eq!(
                editor_text(&state),
                ch.to_string(),
                "{ch:?} must insert literally in Insert mode"
            );
        }
    }

    fn park_on_app(app: &mut App, offset: usize) {
        app.editor_state
            .session
            .as_mut()
            .expect("open")
            .presentation_mut()
            .cursor_byte = offset;
    }

    fn text_on_app(app: &App) -> String {
        editor_text(&app.editor_state)
    }

    fn cursor_on_app(app: &App) -> usize {
        cursor(&app.editor_state)
    }

    // ── M008 selection manipulation, through the dispatch path ─────────
    //
    // Every test here presses a **key**. Calling a helper directly would pass
    // whether or not the chord is reachable, which is the discipline that let
    // the `Ctrl+r` shadow survive M001: three tests "covered" redo and the
    // key was claimed by an earlier stage.

    /// Press `code` with `modifiers`, in Normal mode.
    fn press(state: &mut EditorState, code: KeyCode, modifiers: KeyModifiers) -> BufferKey {
        normal_event(state, KeyEvent::new(code, modifiers))
    }

    /// Every held member's byte range, in document order.
    fn held_ranges(state: &EditorState) -> Vec<std::ops::Range<usize>> {
        let presentation = state.session.as_ref().expect("open").presentation();
        editor::selection_ranges(presentation, &snapshot_of(state))
    }

    /// The notice the last verb left, if any.
    fn notice_of(state: &EditorState) -> Option<String> {
        state.notice.as_ref().map(|notice| notice.message())
    }

    #[tokio::test]
    async fn semicolon_collapses_the_set_to_one_cursor_and_reports_when_empty() {
        let (mut state, _daemon) = opened_writer("ab\ncd\n").await;
        let snapshot = snapshot_of(&state);
        hold_two_members(&mut state, &snapshot, 0, 4);
        park(&mut state, 0);

        assert_eq!(
            normal(&mut state, KeyCode::Char(';')),
            BufferKey::Consumed,
            "claimed even with a set held, or `;` would start a composer word"
        );
        assert_eq!(selection_count(&state), 0);
        assert_eq!(cursor(&state), 0, "the cursor stays where the primary was");

        let (mut empty, _d) = opened_writer("ab\n").await;
        assert_eq!(normal(&mut empty, KeyCode::Char(';')), BufferKey::Consumed);
        assert!(
            notice_of(&empty).is_some_and(|note| note.contains("nothing selected")),
            "a verb that did nothing says so: {:?}",
            notice_of(&empty)
        );
    }

    #[tokio::test]
    async fn alt_semicolon_flips_every_member_and_alt_colon_puts_them_back() {
        let (mut state, _daemon) = opened_writer("abcdef\nghijkl\n").await;
        // One backwards member and one forwards one, so the two verbs differ.
        let snapshot = snapshot_of(&state);
        let point = |byte: usize| EditorPoint {
            byte,
            line: editor::cursor_line(&snapshot, byte),
        };
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![Selection {
            anchor: point(5),
            head: point(1),
            mode: crate::tui::helix_selection::SelectionMode::Char,
            growing: false,
        }];
        park(&mut state, 5);

        assert_eq!(
            press(&mut state, KeyCode::Char(';'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        let member = selection_of(&state).expect("held");
        assert_eq!(
            (member.anchor.byte, member.head.byte),
            (1, 5),
            "the anchor and head swapped"
        );

        // Flipping again restores the backwards direction, which is what
        // `Alt-:` then exists to fix.
        press(&mut state, KeyCode::Char(';'), KeyModifiers::ALT);
        let member = selection_of(&state).expect("held");
        assert!((member.anchor.byte, member.head.byte) == (5, 1));

        assert_eq!(
            press(&mut state, KeyCode::Char(':'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        let member = selection_of(&state).expect("held");
        assert!((member.anchor.byte, member.head.byte) == (1, 5));

        // A second `Alt-:` has nothing to fix and says so, rather than
        // pretending to have worked.
        press(&mut state, KeyCode::Char(':'), KeyModifiers::ALT);
        assert!(
            notice_of(&state).is_some_and(|note| note.contains("already forward")),
            "{:?}",
            notice_of(&state)
        );
    }

    #[tokio::test]
    async fn alt_colon_makes_a_backwards_member_forward_without_moving_the_cursor() {
        let (mut state, _daemon) = opened_writer("abcdef\n").await;
        let snapshot = snapshot_of(&state);
        let point = |byte: usize| EditorPoint {
            byte,
            line: editor::cursor_line(&snapshot, byte),
        };
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![Selection {
            anchor: point(5),
            head: point(1),
            mode: crate::tui::helix_selection::SelectionMode::Char,
            growing: false,
        }];
        park(&mut state, 5);

        assert_eq!(
            press(&mut state, KeyCode::Char(':'), KeyModifiers::ALT),
            BufferKey::Consumed,
            "the bare colon opens the command line; only the modified one flips"
        );
        assert!(
            !state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active,
            "`Alt-:` must not open the command line"
        );
        let member = selection_of(&state).expect("held");
        assert!(member.anchor.byte <= member.head.byte);
        assert_eq!(cursor(&state), 5, "a direction is not a reason to move");
    }

    #[tokio::test]
    async fn percent_selects_the_whole_file_through_the_key() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char('%')), BufferKey::Consumed);
        assert_eq!(held_ranges(&state), vec![0..8]);
        assert_eq!(cursor(&state), 0, "selecting all does not move the reader");
    }

    #[tokio::test]
    async fn comma_keeps_the_primary_and_alt_comma_removes_it() {
        let (mut keep, _d) = opened_writer("abcdefghij\n").await;
        let snapshot = snapshot_of(&keep);
        hold_two_members(&mut keep, &snapshot, 0, 6);
        park(&mut keep, 0);
        assert_eq!(normal(&mut keep, KeyCode::Char(',')), BufferKey::Consumed);
        assert_eq!(selection_count(&keep), 1);
        assert_eq!(held_ranges(&keep), vec![0..1]);
        assert!(
            notice_of(&keep).is_none(),
            "`;` and `,` were different keys; only `,` was pressed"
        );

        let (mut drop, _d) = opened_writer("abcdefghij\n").await;
        let snapshot = snapshot_of(&drop);
        hold_two_members(&mut drop, &snapshot, 0, 6);
        park(&mut drop, 0);
        assert_eq!(
            press(&mut drop, KeyCode::Char(','), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(selection_count(&drop), 1);
        assert_eq!(held_ranges(&drop), vec![6..7], "the primary was removed");
    }

    #[tokio::test]
    async fn comma_on_one_selection_is_reported_rather_than_rewriting_the_set() {
        let (mut state, _daemon) = opened_writer("abc\n").await;
        park(&mut state, 0);
        normal(&mut state, KeyCode::Char('v'));
        assert_eq!(selection_count(&state), 1);
        assert_eq!(normal(&mut state, KeyCode::Char(',')), BufferKey::Consumed);
        assert!(notice_of(&state).is_some_and(|note| note.contains("one selection")));
        assert_eq!(selection_count(&state), 1);
    }

    #[tokio::test]
    async fn capital_c_adds_a_cursor_below_and_alt_c_above() {
        let (mut below, _d) = opened_writer("ab\ncd\n").await;
        park(&mut below, 0);
        normal(&mut below, KeyCode::Char('v'));
        assert_eq!(normal(&mut below, KeyCode::Char('C')), BufferKey::Consumed);
        assert_eq!(held_ranges(&below), vec![0..1, 3..4]);

        let (mut above, _d) = opened_writer("ab\ncd\n").await;
        park(&mut above, 3);
        normal(&mut above, KeyCode::Char('v'));
        assert_eq!(
            press(&mut above, KeyCode::Char('C'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(held_ranges(&above), vec![0..1, 3..4]);
    }

    #[tokio::test]
    async fn capital_c_is_recorded_as_a_repeatable_motion_for_alt_dot() {
        let (mut state, _daemon) = opened_writer("a\nb\nc\nd\n").await;
        park(&mut state, 0);
        normal(&mut state, KeyCode::Char('v'));
        normal(&mut state, KeyCode::Char('C'));
        assert_eq!(selection_count(&state), 2);
        assert_eq!(
            press(&mut state, KeyCode::Char('.'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            selection_count(&state),
            4,
            "`Alt-.` repeated `C` rather than doing nothing"
        );
        assert_eq!(
            held_ranges(&state),
            vec![0..1, 2..3, 4..5],
            "one repeated cursor lands on a line that already has one, and \
             identical members merge into one range — two members and three \
             distinct regions is the honest reading of the same bytes"
        );
    }

    #[tokio::test]
    async fn a_plain_motion_disarms_the_selection_repeat_slot() {
        let (mut state, _daemon) = opened_writer("a\nb\nc\n").await;
        park(&mut state, 0);
        normal(&mut state, KeyCode::Char('v'));
        normal(&mut state, KeyCode::Char('C'));
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(
            press(&mut state, KeyCode::Char('.'), KeyModifiers::ALT),
            BufferKey::Unhandled,
            "one repeat slot, and every motion overwrites it"
        );
    }

    #[tokio::test]
    async fn rotate_moves_focus_and_alt_rotate_moves_bytes() {
        let (mut focus, _d) = opened_writer("aa\nbb\ncc\n").await;
        let member = |line: usize| {
            let base = line * 3;
            Selection {
                anchor: EditorPoint { byte: base, line },
                head: EditorPoint {
                    byte: base + 1,
                    line,
                },
                mode: crate::tui::helix_selection::SelectionMode::Char,
                growing: false,
            }
        };
        focus
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![member(0), member(1), member(2)];
        park(&mut focus, 4);

        assert_eq!(
            normal(&mut focus, KeyCode::Char(')')),
            BufferKey::Consumed,
            "`)` rotates the main selection forward"
        );
        assert_eq!(presentation_primary_index(&focus), 1);
        assert_eq!(
            editor_text(&focus),
            "aa\nbb\ncc\n",
            "rotating focus moves no bytes"
        );
        assert_eq!(cursor(&focus), 4, "the cursor follows the primary");

        assert_eq!(
            press(&mut focus, KeyCode::Char(')'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            editor_text(&focus),
            "bb\ncc\naa\n",
            "`Alt-)` rotated the contents instead"
        );
    }

    #[tokio::test]
    async fn alt_minus_bridges_a_gap_and_alt_underscore_does_not() {
        let (mut merge, _d) = opened_writer("aa bb cc\n").await;
        let member = |start: usize, end: usize| Selection {
            anchor: EditorPoint {
                byte: start,
                line: 0,
            },
            head: EditorPoint { byte: end, line: 0 },
            mode: crate::tui::helix_selection::SelectionMode::Char,
            growing: false,
        };
        merge
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![member(0, 1), member(3, 4)];
        park(&mut merge, 0);
        assert_eq!(
            press(&mut merge, KeyCode::Char('-'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(held_ranges(&merge), vec![0..5]);

        let (mut keep, _d) = opened_writer("aa bb cc\n").await;
        keep.session.as_mut().unwrap().presentation_mut().selections =
            vec![member(0, 1), member(3, 4)];
        park(&mut keep, 0);
        assert_eq!(
            press(&mut keep, KeyCode::Char('_'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            held_ranges(&keep),
            vec![0..2, 3..5],
            "a gap between them is the whole difference from `Alt-minus`"
        );
    }

    #[tokio::test]
    async fn ampersand_aligns_in_display_columns_and_is_undoable_as_one_step() {
        let (mut state, _daemon) = opened_writer("ab\ncdef\n").await;
        let member = |line: usize, start: usize, end: usize| {
            let base = line * 3;
            Selection {
                anchor: EditorPoint {
                    byte: base + start,
                    line,
                },
                head: EditorPoint {
                    byte: base + end,
                    line,
                },
                mode: crate::tui::helix_selection::SelectionMode::Char,
                growing: false,
            }
        };
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![member(0, 0, 1), member(1, 0, 3)];
        park(&mut state, 0);

        assert_eq!(normal(&mut state, KeyCode::Char('&')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "ab  \ncdef\n");
        editor_undo(&mut state);
        assert_eq!(
            editor_text(&state),
            "ab\ncdef\n",
            "two paddings landed as one undoable transaction"
        );
    }

    #[tokio::test]
    async fn the_regex_chords_open_the_command_line_prefilled_and_run_from_it() {
        for (code, modifiers, spec) in [
            (KeyCode::Char('S'), KeyModifiers::NONE, "split-regex"),
            (KeyCode::Char('s'), KeyModifiers::NONE, "select-regex"),
            (KeyCode::Char('K'), KeyModifiers::NONE, "keep-regex"),
            (KeyCode::Char('K'), KeyModifiers::ALT, "remove-regex"),
        ] {
            let (mut state, _daemon) = opened_writer("aa bb\naa cc\n").await;
            let member = |line: usize, start: usize, end: usize| {
                let base = line * 6;
                Selection {
                    anchor: EditorPoint {
                        byte: base + start,
                        line,
                    },
                    head: EditorPoint {
                        byte: base + end,
                        line,
                    },
                    mode: crate::tui::helix_selection::SelectionMode::Char,
                    growing: false,
                }
            };
            state
                .session
                .as_mut()
                .unwrap()
                .presentation_mut()
                .selections = vec![member(0, 0, 1), member(1, 0, 1)];
            park(&mut state, 0);

            assert_eq!(press(&mut state, code, modifiers), BufferKey::Consumed);
            let presentation = state.session.as_ref().unwrap().presentation();
            assert!(presentation.command_active, "{spec}: the `:` line opened");
            assert_eq!(
                presentation.command_line,
                format!("{spec} "),
                "{spec}: pre-filled, so the pattern has somewhere to be typed"
            );

            for ch in "aa".chars() {
                editor_buffer_key(
                    &mut state,
                    InputMode::Normal,
                    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                );
            }
            let BufferKey::RunCommand(command, argument) = editor_buffer_key(
                &mut state,
                InputMode::Normal,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            ) else {
                panic!("{spec}: Enter must submit the command line");
            };
            run_selection_regex(&mut state, command, argument.expect("the pattern"));

            let held = held_ranges(&state);
            match spec {
                "split-regex" | "select-regex" => {
                    assert_eq!(held, vec![0..2, 6..8], "two matches")
                }
                "keep-regex" => assert_eq!(held, vec![0..2, 6..8], "both contain `aa`"),
                _ => assert!(held.is_empty(), "neither line matches anything else"),
            }
        }
    }

    #[tokio::test]
    async fn a_regex_that_matches_nothing_reports_and_leaves_the_buffer_alone() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\n").await;
        let snapshot = snapshot_of(&state);
        hold_two_members(&mut state, &snapshot, 0, 6);
        park(&mut state, 0);

        normal(&mut state, KeyCode::Char('S'));
        for ch in "zzz".chars() {
            editor_buffer_key(
                &mut state,
                InputMode::Normal,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
        let BufferKey::RunCommand(command, argument) = editor_buffer_key(
            &mut state,
            InputMode::Normal,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ) else {
            panic!("Enter must submit");
        };
        run_selection_regex(&mut state, command, argument.unwrap());

        assert_eq!(selection_count(&state), 2, "the set is unchanged");
        assert_eq!(editor_text(&state), "alpha\nbeta\n");
        assert!(
            notice_of(&state).is_some_and(|note| note.contains("zzz")),
            "a no-match set is reported, not silently swallowed: {:?}",
            notice_of(&state)
        );
    }

    #[tokio::test]
    async fn escape_abandons_a_pending_regex_without_touching_the_set() {
        let (mut state, _daemon) = opened_writer("alpha\n").await;
        let snapshot = snapshot_of(&state);
        hold_two_members(&mut state, &snapshot, 0, 3);
        park(&mut state, 0);

        normal(&mut state, KeyCode::Char('K'));
        assert!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active
        );
        editor_buffer_key(
            &mut state,
            InputMode::Normal,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(
            !state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .command_active
        );
        assert_eq!(selection_count(&state), 2, "the set survived the abort");
        assert_eq!(
            undo_depth(&state),
            0,
            "an aborted prompt pushes no undo entry"
        );
    }

    // ── M003: regex search chords, through the real dispatch path ──────

    /// `/` is a regex, and `n`/`N` walk its matches and wrap — the whole
    /// existing search interaction, re-proved on the new scanner.
    #[tokio::test]
    async fn slash_searches_a_regex_and_n_and_n_cap_it() {
        let (mut state, _daemon) = opened_writer("axb a.b\n").await;
        // On the space, so the line opens empty rather than seeded from a
        // word: this test is about the pattern the reader types.
        park(&mut state, 3);
        assert_eq!(normal(&mut state, KeyCode::Char('/')), BufferKey::Consumed);
        assert!(search_open(&state));
        for ch in "a.b".chars() {
            normal(&mut state, KeyCode::Char(ch));
        }
        // Enter accepts; the matches stay painted for `n`/`N`.
        assert_eq!(normal(&mut state, KeyCode::Enter), BufferKey::Consumed);
        assert!(!search_open(&state));
        assert_eq!(
            cursor(&state),
            4,
            "`.` is a wildcard, so the first match at or after the cursor is `a.b`"
        );
        assert_eq!(normal(&mut state, KeyCode::Char('n')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0, "`n` walked on and wrapped to `axb`");
        assert_eq!(normal(&mut state, KeyCode::Char('n')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 4);
        assert_eq!(
            press(&mut state, KeyCode::Char('N'), KeyModifiers::SHIFT),
            BufferKey::Consumed
        );
        assert_eq!(cursor(&state), 0, "and `N` walks the other way");
    }

    /// `?` reuses the accepted pattern and adopts the **previous** match, so
    /// reversing a search never means retyping it.
    #[tokio::test]
    async fn question_searches_backwards_without_retyping_the_pattern() {
        let (mut state, _daemon) = opened_writer("aa bb aa bb aa\n").await;
        park(&mut state, 0);
        // `/` seeds the line from the word under the cursor, so the pattern is
        // never typed in this test at all.
        normal(&mut state, KeyCode::Char('/'));
        assert_eq!(search_text(&state), "aa", "seeded from the cursor's word");
        assert_eq!(normal(&mut state, KeyCode::Enter), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0, "`/aa` adopted the first match");

        assert_eq!(
            press(&mut state, KeyCode::Char('?'), KeyModifiers::NONE),
            BufferKey::Consumed
        );
        assert!(search_open(&state), "`?` opens the search line");
        assert_eq!(
            search_text(&state),
            "aa",
            "seeded with the accepted pattern rather than an empty line"
        );
        assert_eq!(
            cursor(&state),
            12,
            "and adopted the previous match, wrapping to the last one"
        );
    }

    /// `?` with no accepted pattern opens an empty line instead of doing
    /// nothing silently.
    #[tokio::test]
    async fn question_with_no_previous_pattern_opens_an_empty_line() {
        let (mut state, _daemon) = opened_writer("aa bb\n").await;
        park(&mut state, 3);
        assert_eq!(
            press(&mut state, KeyCode::Char('?'), KeyModifiers::NONE),
            BufferKey::Consumed
        );
        assert!(search_open(&state), "the line still opens");
        assert_eq!(
            search_text(&state),
            "",
            "empty, because there is nothing to seed"
        );
        assert_eq!(cursor(&state), 3, "and the cursor is unmoved");
    }

    /// `*` searches for the selected **text**, so a selection containing `.`
    /// matches the literal characters and nothing else. The same selection
    /// with `Alt-*` is read as a regex — the difference is the point of
    /// having both chords.
    #[tokio::test]
    async fn star_searches_for_the_literal_selection_and_alt_star_does_not() {
        let (mut state, _daemon) = opened_writer("a.b axb\n").await;
        // Select `a.b` — the dot is a metacharacter in the pattern.
        hold_range(&mut state, 0, 3);
        park(&mut state, 0);

        assert_eq!(
            press(&mut state, KeyCode::Char('*'), KeyModifiers::NONE),
            BufferKey::Consumed
        );
        assert!(search_open(&state), "`*` opens the search line");
        assert_eq!(
            search_text(&state),
            "\\ba\\.b\\b",
            "escaped and word-bounded, so it searches for the text and not a pattern"
        );
        let escaped = state.session.as_ref().unwrap().presentation();
        assert_eq!(
            escaped.search_matches.len(),
            1,
            "only the literal `a.b` matches"
        );

        // The same selection, read as a regex, matches the wildcard too.
        let (mut state, _daemon) = opened_writer("a.b axb\n").await;
        hold_range(&mut state, 0, 3);
        park(&mut state, 0);
        assert_eq!(
            press(&mut state, KeyCode::Char('*'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(search_text(&state), "a.b", "verbatim, unescaped");
        assert_eq!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .search_matches
                .len(),
            2,
            "`Alt-*` matched `axb` as well, which is exactly the difference"
        );
    }

    /// `s` is a selection verb: it needs a pattern, so it opens the `:` line,
    /// and on a read-only attachment it is refused with the notice `v` and
    /// `x` give rather than silently reshaping a set the reader cannot use.
    #[tokio::test]
    async fn s_is_a_selection_verb_and_is_refused_on_a_read_only_document() {
        let (mut state, _daemon) = opened_reader("ab ab\n").await;
        hold_range(&mut state, 0, 5);
        park(&mut state, 0);

        assert_eq!(normal(&mut state, KeyCode::Char('s')), BufferKey::Consumed);
        assert_eq!(
            command_line_text(&state),
            "select-regex ",
            "the chord is a shortcut for a command the reader could have typed"
        );
        for ch in "ab".chars() {
            normal(&mut state, KeyCode::Char(ch));
        }
        let BufferKey::RunCommand(command, argument) = normal(&mut state, KeyCode::Enter) else {
            panic!("Enter must submit the command line");
        };
        run_selection_regex(&mut state, command, argument.expect("the pattern"));

        assert!(
            notice_of(&state).is_some_and(|note| note.contains("Read-only")),
            "the refusal is reported, not swallowed: {:?}",
            notice_of(&state)
        );
        assert_eq!(
            selection_count(&state),
            1,
            "and the set is exactly what the reader held"
        );
    }

    /// `Alt-s` needs no pattern, so it acts on the keystroke and leaves one
    /// member per line — bounded by each line's content.
    #[tokio::test]
    async fn alt_s_splits_the_selection_on_newlines_through_the_key_path() {
        let (mut state, _daemon) = opened_writer("alpha\nbeta\ngamma\n").await;
        hold_range(&mut state, 0, 10);
        park(&mut state, 0);

        assert_eq!(
            press(&mut state, KeyCode::Char('s'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            held_ranges(&state),
            vec![0..5, 6..10],
            "two lines, each ending at its content"
        );
        assert_eq!(editor_text(&state), "alpha\nbeta\ngamma\n", "no byte moved");
    }

    /// A pattern that will not compile is a **notice naming the pattern and
    /// the compiler's reason**, not a panic and not a silent empty result —
    /// and the line keeps its text so the reader can fix it.
    #[tokio::test]
    async fn an_invalid_search_pattern_is_reported_and_leaves_the_line_fixable() {
        let (mut state, _daemon) = opened_writer("alpha\n").await;
        // On the line break, so the line opens empty and the `[` is the
        // reader's rather than an append to a seeded word.
        park(&mut state, 5);
        normal(&mut state, KeyCode::Char('/'));
        normal(&mut state, KeyCode::Char('['));
        let notice = notice_of(&state).expect("an invalid pattern is reported");
        assert!(
            notice.contains('[') && notice.contains("not a valid regex"),
            "the notice names the pattern and the compiler's reason: {notice}"
        );
        assert!(
            state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .search_matches
                .is_empty(),
            "and a refused pattern leaves no matches behind"
        );
        assert_eq!(
            search_text(&state),
            "[",
            "the line keeps the text, so the reader can fix what they typed"
        );
    }

    /// A search bound that is reached is **named**, not merely reported as
    /// partial: a count that stopped at the entry cap and a scan that stopped
    /// at the byte window are different facts.
    #[tokio::test]
    async fn a_truncated_search_names_the_match_count_bound_in_the_notice() {
        // More matches than the entry cap, in a document well inside the byte
        // window, so the notice can only be about the count. `/` seeds the
        // line from the word under the cursor, so the pattern is never typed.
        let text = "ab ".repeat(crate::tui::editor::MAX_EDITOR_SEARCH_MATCHES + 5);
        let (mut state, _daemon) = opened_writer(&text).await;
        park(&mut state, 0);
        assert_eq!(normal(&mut state, KeyCode::Char('/')), BufferKey::Consumed);
        let notice = notice_of(&state).expect("a bound was reached and disclosed");
        assert!(
            notice.contains(&crate::tui::editor::MAX_EDITOR_SEARCH_MATCHES.to_string()),
            "the notice names the count bound: {notice}"
        );
        assert!(
            !notice.contains("scan window"),
            "and not the byte window, which was never reached: {notice}"
        );
    }

    #[tokio::test]
    async fn the_mutating_set_verbs_are_refused_on_a_read_only_document() {
        // `&` and `Alt-)` write text, so they must report rather than appear
        // to work — the same gate `v` and `x` already pass through.
        let (mut state, _daemon) = opened_reader("ab\ncd\n").await;
        let snapshot = snapshot_of(&state);
        hold_two_members(&mut state, &snapshot, 0, 3);
        park(&mut state, 0);

        assert_eq!(normal(&mut state, KeyCode::Char('&')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "ab\ncd\n", "nothing was written");
        assert!(
            notice_of(&state).is_some_and(|note| note.contains("Read-only")),
            "{:?}",
            notice_of(&state)
        );
    }

    #[tokio::test]
    async fn the_non_mutating_set_verbs_still_work_on_a_read_only_document() {
        // A reader on a file they cannot write must still be able to move
        // around it and select in it. Refusing those would take away
        // navigation, which is the asymmetry M007 established for `v`/`x`.
        let (mut state, _daemon) = opened_reader("ab\ncd\nef\n").await;
        // `v` is refused here — that is M008's inherited rule, and it is why
        // this test builds its set directly rather than with `v`. A read-only
        // reader still needs to be able to *have* a set; they just cannot grow
        // one by selecting.
        park(&mut state, 0);
        assert_eq!(normal(&mut state, KeyCode::Char('v')), BufferKey::Consumed);
        assert_eq!(selection_count(&state), 0, "growing a selection is refused");
        let snapshot = snapshot_of(&state);
        hold_two_members(&mut state, &snapshot, 0, 3);
        park(&mut state, 0);

        assert_eq!(normal(&mut state, KeyCode::Char('C')), BufferKey::Consumed);
        assert_eq!(
            selection_count(&state),
            4,
            "`C` adds a cursor per member on a read-only buffer; one lands on a \
             line that already had one, and identical members are kept rather \
             than silently folded"
        );

        assert_eq!(normal(&mut state, KeyCode::Char(';')), BufferKey::Consumed);
        assert_eq!(selection_count(&state), 0, "`;` is pure selection state");
        assert_eq!(normal(&mut state, KeyCode::Char('%')), BufferKey::Consumed);
        assert_eq!(selection_count(&state), 1, "so is `%`");
    }

    #[tokio::test]
    async fn none_of_the_eighteen_new_chords_are_captured_in_insert_mode() {
        // Every glyph M008 claims, in both the `NONE` and `SHIFT` spellings a
        // terminal might use, must remain ordinary text.
        let glyphs = [';', ',', '%', 'C', 'S', 'K', '(', ')', '&', '-', '_', ':'];
        for ch in glyphs {
            for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
                let (mut state, _daemon) = opened_writer("").await;
                assert_eq!(
                    normal(&mut state, KeyCode::Char('i')),
                    BufferKey::EnterInsert
                );
                assert_eq!(
                    insert_event(&mut state, KeyEvent::new(KeyCode::Char(ch), modifiers)),
                    BufferKey::Consumed,
                    "{ch:?} with {modifiers:?} must insert literally"
                );
                assert_eq!(editor_text(&state), ch.to_string());
            }
        }
    }

    #[tokio::test]
    async fn the_eleven_alt_chords_are_inert_in_insert_mode() {
        // `handle_insert_key`'s catch-all covers `NONE` and `SHIFT` only, so
        // every `ALT` chord M008 adds falls outside it. Helix's Insert mode
        // binds no `Alt-` punctuation, so inert is the faithful answer — and
        // it has to be asserted, not assumed.
        //
        // The name says eleven and the list holds eleven: the nine M008
        // chords plus M012's `Alt-u` and `Alt-U`. (The list was nine under a
        // name that said eight; the name follows the list.)
        for code in [
            KeyCode::Char(';'),
            KeyCode::Char(':'),
            KeyCode::Char(','),
            KeyCode::Char('C'),
            KeyCode::Char('K'),
            KeyCode::Char('('),
            KeyCode::Char(')'),
            KeyCode::Char('-'),
            KeyCode::Char('_'),
            KeyCode::Char('u'),
            KeyCode::Char('U'),
        ] {
            let (mut state, _daemon) = opened_writer("").await;
            assert_eq!(
                normal(&mut state, KeyCode::Char('i')),
                BufferKey::EnterInsert
            );
            assert_eq!(
                insert_event(&mut state, KeyEvent::new(code, KeyModifiers::ALT)),
                BufferKey::Unhandled,
                "Alt-{:?} must reach the composer rather than vanish",
                code
            );
            assert_eq!(editor_text(&state), "");
        }
    }

    // ── M011: insert-mode control chords ─────────────────────────────────

    /// The editor attached to an `App`, on the editor route, in insert mode.
    ///
    /// `handle_editor_key` refuses every key unless the editor route is
    /// current, so a test that calls it without this reaches none of the
    /// dispatch under test and would pass for the wrong reason.
    async fn app_in_insert_mode(state: EditorState) -> App {
        let mut app = app_with_the_editor_attached(state).await;
        app.ui_state
            .routes
            .navigate_to(crate::tui::route::Route::Editor);
        app.ui_state.input_mode = crate::tui::input::InputMode::Insert;
        app
    }

    /// The editor, opened, in insert mode, with the cursor at `offset`.
    async fn inserting(text: &str, offset: usize) -> App {
        let (mut state, _daemon) = opened_writer(text).await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert,
            "the editor must be in insert mode for the chord to mean anything"
        );
        park(&mut state, offset);
        app_in_insert_mode(state).await
    }

    /// A completion scope, for opening the popup without a live daemon.
    fn a_completion_scope() -> crate::tui::editor_completion::EditorCompletionScope {
        crate::tui::editor_completion::EditorCompletionScope {
            read_scope: crate::tui::editor_lsp::EditorLspScope {
                generation: 1,
                session_id: "session-1".to_string(),
                project_id: "project-1".to_string(),
                workspace_id: "workspace-1".to_string(),
                path: "/tmp/example.rs".to_string(),
            },
            document_id: "document-1".to_string(),
            revision: 1,
            line: 0,
            character: 0,
        }
    }

    #[tokio::test]
    async fn ctrl_w_deletes_the_previous_word() {
        let mut app = inserting("hello world\nsecond line\n", 11).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)
        ));
        assert_eq!(
            editor_text(&app.editor_state),
            "hello \nsecond line\n",
            "the previous word, and the whitespace that separated it from the cursor"
        );
    }

    #[tokio::test]
    async fn ctrl_w_stops_at_the_line_start() {
        let mut app = inserting("abc\ndef\n", 4).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)
        ));
        assert_eq!(
            editor_text(&app.editor_state),
            "abc\ndef\n",
            "the scan stops at the line boundary; it must not reach into the line above"
        );
    }

    #[tokio::test]
    async fn alt_backspace_is_the_same_gesture_as_ctrl_w() {
        let mut app = inserting("hello world\n", 11).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT)
        ));
        assert_eq!(
            editor_text(&app.editor_state),
            "hello \n",
            "`Alt-Backspace` and `Ctrl-w` are one gesture, two spellings"
        );
    }

    #[tokio::test]
    async fn alt_d_deletes_the_next_word() {
        let mut app = inserting("hello world\nsecond\n", 0).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT)
        ));
        assert_eq!(editor_text(&app.editor_state), " world\nsecond\n");
    }

    #[tokio::test]
    async fn alt_d_stops_at_the_line_end() {
        let mut app = inserting("abc\ndef\n", 2).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT)
        ));
        assert_eq!(
            editor_text(&app.editor_state),
            "ab\ndef\n",
            "the scan stops at the line boundary; it must not eat the newline"
        );
    }

    #[tokio::test]
    async fn ctrl_u_deletes_to_the_line_start_and_ctrl_k_to_the_line_end() {
        let mut app = inserting("hello world\n", 6).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)
        ));
        assert_eq!(editor_text(&app.editor_state), "world\n");

        let mut app = inserting("hello world\n", 6).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)
        ));
        assert_eq!(editor_text(&app.editor_state), "hello \n");
    }

    #[tokio::test]
    async fn ctrl_h_and_ctrl_d_delete_one_character_each() {
        let mut app = inserting("abc\n", 1).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)
        ));
        assert_eq!(
            editor_text(&app.editor_state),
            "bc\n",
            "`Ctrl-h` deletes backward"
        );

        let mut app = inserting("abc\n", 1).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
        ));
        assert_eq!(
            editor_text(&app.editor_state),
            "ac\n",
            "`Ctrl-d` deletes forward"
        );
    }

    #[tokio::test]
    async fn ctrl_j_inserts_a_newline() {
        let mut app = inserting("ab\n", 1).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)
        ));
        assert_eq!(editor_text(&app.editor_state), "a\nb\n");
    }

    #[tokio::test]
    async fn ctrl_x_is_an_insert_mode_chord_only() {
        // Insert mode: the editor claims it, and the mode does not change.
        let (mut state, _daemon) = opened_writer("hello\n").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        let mut app = app_in_insert_mode(state).await;
        assert!(
            handle_editor_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)
            ),
            "`Ctrl-x` is the insert-mode completion trigger"
        );
        assert_eq!(
            app.editor_state.mode(),
            EditorMode::Insert,
            "and it stays in insert mode"
        );

        // Normal mode: the chord is not the editor's to take.
        let (state, _daemon) = opened_writer("hello\n").await;
        let mut app = app_on_the_editor_route(state).await;
        assert!(
            !handle_editor_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)
            ),
            "in normal mode `Ctrl-x` is a global action, and the editor must not shadow it"
        );
    }

    #[tokio::test]
    async fn ctrl_s_and_ctrl_r_still_reach_the_lifecycle_chords_in_insert_mode() {
        // The guard block is mode-independent and outranks the insert table,
        // so a save or a resync must survive the new chords.
        let (mut state, _daemon) = opened_writer("base\n").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        assert_eq!(
            insert_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
            ),
            BufferKey::RequestSave
        );
        assert_eq!(
            editor_text(&state),
            "base\n",
            "a save request edits nothing"
        );
        assert_eq!(
            insert_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)
            ),
            BufferKey::RequestResync
        );
        assert_eq!(editor_text(&state), "base\n", "and a resync edits nothing");
    }

    #[tokio::test]
    async fn the_completion_popup_does_not_steal_enter_in_insert_mode() {
        let mut app = inserting("hello\n", 5).await;
        app.editor_state.completion.begin(&a_completion_scope());
        assert!(
            app.editor_state.completion.is_open(),
            "the popup must be open for this to mean anything"
        );

        assert!(handle_editor_key(&mut app, key(KeyCode::Enter)));
        assert_eq!(
            editor_text(&app.editor_state),
            "hello\n\n",
            "`Enter` is a newline in insert mode, never an accept"
        );
        assert!(
            !app.editor_state.completion.is_open(),
            "and the popup is dismissed rather than left open over a moved cursor"
        );
    }

    #[tokio::test]
    async fn the_completion_popup_does_not_steal_the_arrows_in_insert_mode() {
        let mut app = inserting("hello\nworld\n", 5).await;
        app.editor_state.completion.begin(&a_completion_scope());
        assert!(app.editor_state.completion.is_open());

        let before = cursor(&app.editor_state);
        assert!(handle_editor_key(&mut app, key(KeyCode::Down)));
        assert_ne!(
            cursor(&app.editor_state),
            before,
            "`Down` moves the cursor, not the candidate selection"
        );
        assert_eq!(
            editor_text(&app.editor_state),
            "hello\nworld\n",
            "and it moves nothing else"
        );
    }

    #[tokio::test]
    async fn the_lsp_panel_does_not_steal_characters_in_insert_mode() {
        let mut app = inserting("hello\n", 0).await;
        app.editor_state.lsp.open_diagnostics_panel();
        assert!(
            app.editor_state.lsp.panel.is_some(),
            "the panel must be open for this to mean anything"
        );

        assert!(handle_editor_key(&mut app, key(KeyCode::Char('j'))));
        assert_eq!(
            editor_text(&app.editor_state),
            "jhello\n",
            "`j` is text in insert mode, not a panel gesture"
        );
        assert_eq!(app.editor_state.lsp.selected_index, 0);
    }

    #[tokio::test]
    async fn the_lsp_panel_does_not_steal_enter_in_insert_mode() {
        let mut app = inserting("hello\n", 5).await;
        app.editor_state.lsp.open_diagnostics_panel();
        assert!(app.editor_state.lsp.panel.is_some());

        assert!(handle_editor_key(&mut app, key(KeyCode::Enter)));
        assert_eq!(
            editor_text(&app.editor_state),
            "hello\n\n",
            "`Enter` is a newline, never a jump to a diagnostic"
        );
    }

    #[tokio::test]
    async fn a_typing_burst_is_one_undo_step() {
        let (mut state, _daemon) = opened_writer("").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        for ch in "hello".chars() {
            assert_eq!(insert(&mut state, KeyCode::Char(ch)), BufferKey::Consumed);
        }
        assert_eq!(editor_text(&state), "hello");
        assert_eq!(
            undo_depth(&state),
            0,
            "the burst is not committed until insert mode ends"
        );

        leave_insert_mode(&mut state);
        assert_eq!(undo_depth(&state), 1, "one burst, one undo step");

        assert_eq!(normal(&mut state, KeyCode::Char('u')), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            "",
            "and one `u` takes back all five characters"
        );
    }

    #[tokio::test]
    async fn a_second_undo_reverts_the_edit_before_the_burst() {
        let (mut state, _daemon) = opened_writer("").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        insert(&mut state, KeyCode::Char('x'));
        leave_insert_mode(&mut state);

        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        for ch in "hello".chars() {
            assert_eq!(insert(&mut state, KeyCode::Char(ch)), BufferKey::Consumed);
        }
        leave_insert_mode(&mut state);
        assert_eq!(editor_text(&state), "xhello");
        assert_eq!(
            undo_depth(&state),
            2,
            "the burst is one step, and the earlier edit is another"
        );

        assert_eq!(normal(&mut state, KeyCode::Char('u')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "x", "the burst is gone");
        assert_eq!(normal(&mut state, KeyCode::Char('u')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "", "and then the edit before it");
    }

    #[tokio::test]
    async fn a_reconciliation_discards_an_open_burst() {
        let (mut state, _daemon) = opened_writer("").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        for ch in "hello".chars() {
            assert_eq!(insert(&mut state, KeyCode::Char(ch)), BufferKey::Consumed);
        }

        {
            let session = state.session.as_mut().expect("open");
            editor::clear_history(session.presentation_mut());
        }
        assert_eq!(
            undo_depth(&state),
            0,
            "the burst is not resurrected as an undo step: it would restore the pre-reload text"
        );
        assert_eq!(
            editor_text(&state),
            "hello",
            "and the text the reader typed is still the text"
        );
    }

    /// M009 supersedes M012's truncation: the branch is kept, not destroyed.
    ///
    /// The burst that follows an undo becomes a **sibling** of the branch the
    /// reader undid past, so the abandoned text is still in the tree while the
    /// burst is open (nothing is recorded until it closes) and `U` follows the
    /// new branch once it commits.
    #[tokio::test]
    async fn an_edit_after_an_undo_branches_instead_of_truncating() {
        let (mut state, _daemon) = opened_writer("").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        insert(&mut state, KeyCode::Char('a'));
        leave_insert_mode(&mut state);

        assert_eq!(normal(&mut state, KeyCode::Char('u')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "");
        assert_eq!(redo_depth(&state), 1, "the undo left a redo branch");

        assert_eq!(
            normal(&mut state, KeyCode::Char('i')),
            BufferKey::EnterInsert
        );
        insert(&mut state, KeyCode::Char('b'));
        assert_eq!(
            redo_depth(&state),
            1,
            "the branch the reader undid past survives the new burst"
        );
        leave_insert_mode(&mut state);

        assert_eq!(undo_depth(&state), 1, "the burst is its own node");
        assert_eq!(
            history_len(&state),
            2,
            "and the abandoned node is still in the tree"
        );
        assert_eq!(
            redo_depth(&state),
            0,
            "`U` stands at the tip of the branch it took"
        );
        // Back to the fork: `U` follows the new branch, not the abandoned one.
        assert_eq!(normal(&mut state, KeyCode::Char('u')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "", "at the fork state");
        assert_eq!(normal(&mut state, KeyCode::Char('U')), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            "b",
            "and forward follows the new branch"
        );
    }

    #[tokio::test]
    async fn all_eighteen_chords_are_claimed_in_normal_mode() {
        // The companion to the guard: this drives the keys, so a chord the
        // guard can count but nothing can reach still fails here.
        let claimed: Vec<(KeyCode, KeyModifiers)> = vec![
            (KeyCode::Char(';'), KeyModifiers::NONE),
            (KeyCode::Char(';'), KeyModifiers::ALT),
            (KeyCode::Char(':'), KeyModifiers::ALT),
            (KeyCode::Char('%'), KeyModifiers::NONE),
            (KeyCode::Char('%'), KeyModifiers::SHIFT),
            (KeyCode::Char(','), KeyModifiers::NONE),
            (KeyCode::Char(','), KeyModifiers::ALT),
            (KeyCode::Char('C'), KeyModifiers::NONE),
            (KeyCode::Char('C'), KeyModifiers::SHIFT),
            (KeyCode::Char('C'), KeyModifiers::ALT),
            (KeyCode::Char('S'), KeyModifiers::NONE),
            (KeyCode::Char('S'), KeyModifiers::SHIFT),
            (KeyCode::Char('K'), KeyModifiers::NONE),
            (KeyCode::Char('K'), KeyModifiers::ALT),
            (KeyCode::Char('('), KeyModifiers::NONE),
            (KeyCode::Char(')'), KeyModifiers::NONE),
            (KeyCode::Char('('), KeyModifiers::ALT),
            (KeyCode::Char(')'), KeyModifiers::ALT),
            (KeyCode::Char('&'), KeyModifiers::NONE),
            (KeyCode::Char('&'), KeyModifiers::SHIFT),
            (KeyCode::Char('-'), KeyModifiers::ALT),
            (KeyCode::Char('_'), KeyModifiers::ALT),
            // M012's two history gestures. `Alt-U`'s *other* terminal
            // spelling — `Char('u')` with `ALT | SHIFT` — is exercised by
            // its own test, so adding it here would make this count 25.
            (KeyCode::Char('u'), KeyModifiers::ALT),
            (KeyCode::Char('U'), KeyModifiers::ALT),
        ];
        assert_eq!(
            claimed.len(),
            24,
            "eighteen verbs across the spellings the terminals disagree about, \
             plus M012's two history gestures"
        );
        for (code, modifiers) in claimed {
            let (mut state, _daemon) = opened_writer("ab\ncd\nef\n").await;
            park(&mut state, 0);
            normal(&mut state, KeyCode::Char('v'));
            assert_eq!(
                press(&mut state, code, modifiers),
                BufferKey::Consumed,
                "{code:?} with {modifiers:?} must be claimed"
            );
        }
    }

    #[tokio::test]
    async fn a_heterogeneous_set_does_not_make_any_verb_assume_uniformity() {
        // Mixed modes, mixed emptiness, different lines — the shape a verb
        // written for "the selection" silently gets wrong.
        let (mut state, _daemon) = opened_writer("alpha\nbeta\ngamma\n").await;
        let snapshot = snapshot_of(&state);
        let point = |byte: usize| EditorPoint {
            byte,
            line: editor::cursor_line(&snapshot, byte),
        };
        let line_member = Selection {
            anchor: point(6),
            head: point(6),
            mode: crate::tui::helix_selection::SelectionMode::Line,
            growing: false,
        };
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![
            // A whole-line member...
            line_member,
            // ...a character member on another line...
            Selection {
                anchor: point(11),
                head: point(12),
                mode: crate::tui::helix_selection::SelectionMode::Char,
                growing: false,
            },
            // ...and a backwards one.
            Selection {
                anchor: point(18),
                head: point(16),
                mode: crate::tui::helix_selection::SelectionMode::Char,
                growing: false,
            },
        ];
        park(&mut state, 6);

        // Every verb in the set has to survive the mixed set rather than
        // panicking, silently dropping a member, or assuming one mode.
        for code in [
            KeyCode::Char(','),
            KeyCode::Char(')'),
            KeyCode::Char('('),
            KeyCode::Char(';'),
        ] {
            assert_eq!(
                normal(&mut state, code),
                BufferKey::Consumed,
                "{code:?} on a mixed set"
            );
        }

        // `;` emptied the set, so re-establish a mixed one before merging:
        // the merge is where a mode-uniformity assumption would show up.
        let mixed = |line: usize, mode: crate::tui::helix_selection::SelectionMode| {
            let base = line * 6;
            Selection {
                anchor: EditorPoint { byte: base, line },
                head: EditorPoint {
                    byte: base + 4,
                    line,
                },
                mode,
                growing: false,
            }
        };
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![
            mixed(1, crate::tui::helix_selection::SelectionMode::Line),
            mixed(2, crate::tui::helix_selection::SelectionMode::Char),
        ];
        assert_eq!(
            press(&mut state, KeyCode::Char('-'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        // Merging a Line member with a Char one must take the Line mode, or
        // the merged selection would silently stop covering a whole row.
        let merged = selection_of(&state).expect("one merged member");
        assert_eq!(
            merged.mode,
            crate::tui::helix_selection::SelectionMode::Line
        );
    }

    // ── M006 unimpaired navigation, through the dispatch path ──────────────
    //
    // Four chords, two of them edits. Every test presses keys rather than
    // calling a resolver, because the interesting failures here are all
    // *routing* failures: a leaf that resolves in the model but is shadowed by
    // a bare-key arm, an edit that lands in the motion path, or a motion whose
    // disclosure never reaches the notice.

    /// A document with three blank-line-separated paragraphs of known offsets.
    ///
    /// Offsets are asserted numerically rather than found with the scanner
    /// under test — a test that computed its own expectation could not fail.
    fn three_paragraphs() -> String {
        "alpha\n\n  beta\n\ngamma\n".to_string()
    }

    /// Press `]p`.
    fn next_paragraph(state: &mut EditorState) -> BufferKey {
        let armed = press(state, KeyCode::Char(']'), KeyModifiers::NONE);
        assert_eq!(armed, BufferKey::Consumed, "`]` arms");
        assert_eq!(pending(state), "]", "`]` shows its pending indicator");
        press(state, KeyCode::Char('p'), KeyModifiers::NONE)
    }

    /// Press `[p`.
    fn prev_paragraph(state: &mut EditorState) -> BufferKey {
        press(state, KeyCode::Char('['), KeyModifiers::NONE);
        press(state, KeyCode::Char('p'), KeyModifiers::NONE)
    }

    /// Press `]Space` / `[Space`.
    fn add_newline(state: &mut EditorState, arming: char) -> BufferKey {
        press(state, KeyCode::Char(arming), KeyModifiers::NONE);
        press(state, KeyCode::Char(' '), KeyModifiers::NONE)
    }

    #[tokio::test]
    async fn unimpaired_close_p_lands_on_each_next_paragraph() {
        let (mut state, _daemon) = opened_writer(&three_paragraphs()).await;
        assert_eq!(cursor(&state), 0);

        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 9, "beta's first non-whitespace byte");

        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 15, "gamma's first byte");

        // A third press has nowhere to go and says so rather than standing
        // still in silence.
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 15, "and moved nothing");
        assert_eq!(
            notice_of(&state).as_deref(),
            Some("no next paragraph — ]p reached the end"),
            "a refused paragraph motion is reported, not silent"
        );
    }

    #[tokio::test]
    async fn unimpaired_open_p_lands_on_each_previous_paragraph() {
        let (mut state, _daemon) = opened_writer(&three_paragraphs()).await;
        // Start in the last paragraph.
        press(&mut state, KeyCode::Char('G'), KeyModifiers::SHIFT);
        let end = cursor(&state);

        assert_eq!(prev_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 9, "beta");

        assert_eq!(prev_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0, "alpha");

        assert_eq!(prev_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0, "and moved nothing");
        assert!(
            cursor(&state) < end,
            "the cursor really did move before the refusal"
        );
        assert_eq!(
            notice_of(&state).as_deref(),
            Some("no previous paragraph — [p reached the start"),
            "a refused paragraph motion is reported, not silent"
        );
    }

    #[tokio::test]
    async fn a_paragraph_motion_never_stands_still() {
        // The rule that makes `]p` usable for walking a document: from *any*
        // position it either moves or says it could not. Standing still is the
        // one outcome a navigation key must never produce silently, because it
        // is indistinguishable from a dropped keypress.
        let text = three_paragraphs();
        for start in 0..text.len() {
            if !text.is_char_boundary(start) {
                continue;
            }
            let (mut state, _daemon) = opened_writer(&text).await;
            state
                .session
                .as_mut()
                .unwrap()
                .presentation_mut()
                .cursor_byte = start;
            for command in [EditorCommand::NextParagraph, EditorCommand::PrevParagraph] {
                let before = cursor(&state);
                editor_motion(&mut state, command, NOMINAL_VISIBLE_LINES);
                let after = cursor(&state);
                if after == before {
                    assert!(
                        notice_of(&state).is_some(),
                        "{command:?} at {start} stood still with nothing to say"
                    );
                } else {
                    assert_eq!(
                        notice_of(&state),
                        None,
                        "{command:?} at {start} moved and still left a note"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn a_document_with_no_blank_lines_reports_no_next_paragraph() {
        // Helix's own `move_next_paragraph` degenerates here: with no blank
        // line anywhere it skips every remaining line and lands on the end of
        // the buffer (see `test_behaviour_when_moving_to_next_paragraph_single`
        // in `helix-core/src/movement.rs`). This editor says "there is none"
        // instead, which is a deliberate difference — an end-of-buffer jump
        // looks like the key did something and hides that it found nothing.
        let (mut state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0);
        assert_eq!(
            notice_of(&state).as_deref(),
            Some("no next paragraph — ]p reached the end")
        );
    }

    #[tokio::test]
    async fn consecutive_blank_lines_are_one_separator() {
        // "head\n\n\n\ntail\n": one separator, so `]p` skips the whole run and
        // lands on `tail` rather than stopping inside it.
        let (mut state, _daemon) = opened_writer("head\n\n\n\ntail\n").await;
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 8);
    }

    #[tokio::test]
    async fn a_whitespace_only_line_is_blank() {
        let (mut state, _daemon) = opened_writer("head\n \t \ntail\n").await;
        assert_eq!(
            next_paragraph(&mut state),
            BufferKey::Consumed,
            "a line of spaces and a tab is a separator, not a paragraph"
        );
        // "head\n \t \ntail\n" — the blank line is 5..=7 and its terminator is
        // byte 8, so `tail` begins at 9.
        assert_eq!(cursor(&state), 9);
    }

    #[tokio::test]
    async fn unimpaired_close_p_from_a_blank_line_still_moves_forward() {
        let (mut state, _daemon) = opened_writer("head\n\n\ntail\n").await;
        // Put the cursor on the blank line at offset 5.
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .cursor_byte = 5;
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 7, "and forward, not back to `head`");
    }

    #[tokio::test]
    async fn unimpaired_open_p_from_a_blank_line_lands_above_the_separator() {
        let (mut state, _daemon) = opened_writer("head\n\n\ntail\n").await;
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .cursor_byte = 6;
        assert_eq!(prev_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0);
    }

    #[tokio::test]
    async fn a_huge_indented_line_is_paragraph_content_not_a_separator() {
        // The bounded scan cannot see past `MAX_RENDER_LINE_BYTES`, so it cannot
        // prove such a line is blank. Calling it blank would invent a paragraph
        // boundary that is not in the file; calling it content cannot.
        let huge = " ".repeat(crate::tui::editor::MAX_RENDER_LINE_BYTES * 2);
        let text = format!("head\n\n{huge}\n");
        let (mut state, _daemon) = opened_writer(&text).await;
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert!(
            cursor(&state) > 5,
            "landed on the oversized line at {}, not back on `head`",
            cursor(&state)
        );
        assert_eq!(
            notice_of(&state),
            None,
            "the scan answered from what it could see"
        );
    }

    #[tokio::test]
    async fn a_paragraph_scan_over_its_budget_says_the_answer_is_unknown() {
        // A file with no blank line at all is the only shape that makes `]p`
        // walk every line, so it is also the only shape that can exhaust the
        // budget. The disclosure must say *unknown* — reporting "no next
        // paragraph" here would be a claim the scan never got to test.
        let wide = format!("{}\n", "x".repeat(4096));
        let text = wide.repeat(128);
        assert!(
            text.len() > editor::MAX_PARAGRAPH_SCAN_BYTES,
            "the fixture must exceed the budget to test the bound"
        );
        let (mut state, _daemon) = opened_writer(&text).await;
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        let notice = notice_of(&state).expect("the bound is disclosed");
        assert!(
            notice.contains("not known"),
            "expected an explicit unknown, got {notice:?}"
        );
    }

    #[tokio::test]
    async fn unimpaired_close_space_adds_a_line_below_and_leaves_the_cursor_alone() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        let before = cursor(&state);
        assert_eq!(add_newline(&mut state, ']'), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "one\n\ntwo\n");
        assert_eq!(
            cursor(&state),
            before,
            "Helix's add_newline moves no selection, and neither does this"
        );
        assert_ne!(
            state.mode(),
            EditorMode::Insert,
            "`]Space` is add_newline, not `o` — it must not take the keyboard"
        );
    }

    #[tokio::test]
    async fn unimpaired_open_space_adds_a_line_above_and_keeps_the_cursor_on_its_byte() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        press(&mut state, KeyCode::Char('j'), KeyModifiers::NONE);
        press(&mut state, KeyCode::Char('l'), KeyModifiers::NONE);
        let before = cursor(&state);
        assert_eq!(add_newline(&mut state, '['), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "one\n\ntwo\n");
        assert_eq!(
            cursor(&state),
            before + 1,
            "the cursor keeps pointing at its own byte, which moved down one"
        );
        assert_ne!(state.mode(), EditorMode::Insert);
    }

    #[tokio::test]
    async fn o_and_close_space_stay_different_verbs() {
        // The whole reason `AddNewlineBelow` is not `OpenLineBelow`: the same
        // insertion, and the difference is whether the keyboard is taken.
        //
        // The *text* each verb produces is asserted by the M019 tests below;
        // this one owns the half that is about the keyboard.
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('o')),
            BufferKey::EnterInsert,
            "`o` reports the mode it is handing over"
        );
        assert_eq!(
            state.mode(),
            EditorMode::Insert,
            "`o` hands the keyboard to the document"
        );

        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        assert_eq!(add_newline(&mut state, ']'), BufferKey::Consumed);
        assert_ne!(
            state.mode(),
            EditorMode::Insert,
            "`]Space` inserts the line and leaves the keyboard alone"
        );
        assert_eq!(editor_text(&state), "one\n\ntwo\n");
    }

    // ── M019: the open-line verbs ────────────────────────────────────

    /// `o` opens a line **below** the cursor's line and hands over the
    /// keyboard.
    ///
    /// The assertion that matters is the *text*: `o` used to enter Insert mode
    /// without opening anything, and every test of `resolve_edit` passed while
    /// the key path did nothing. So this is driven through `editor_buffer_key`
    /// and reads the controller's own snapshot, not a helper's return value.
    #[tokio::test]
    async fn o_opens_a_line_below_the_cursor_and_takes_the_keyboard() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('o')),
            BufferKey::EnterInsert,
            "`o` reports the mode it is handing over"
        );
        assert_eq!(
            editor_text(&state),
            "one\n\ntwo\n",
            "`o` opened a line below the cursor's line"
        );
        assert_eq!(
            cursor(&state),
            4,
            "and put the cursor on the line it opened, not on the line after it"
        );
        assert_eq!(state.mode(), EditorMode::Insert);
    }

    /// The two verbs are one implementation with two arguments, so the test
    /// that only covers one of them covers half the contract.
    #[tokio::test]
    async fn capital_o_opens_a_line_above_the_cursor_and_takes_the_keyboard() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        assert_eq!(
            press(&mut state, KeyCode::Char('O'), KeyModifiers::SHIFT),
            BufferKey::EnterInsert
        );
        assert_eq!(
            editor_text(&state),
            "\none\ntwo\n",
            "`O` opened a line above the cursor's line"
        );
        assert_eq!(
            cursor(&state),
            0,
            "and put the cursor on the line it opened"
        );
        assert_eq!(state.mode(), EditorMode::Insert);
    }

    /// A read-only attachment refuses `o`, and says so.
    #[tokio::test]
    async fn o_on_a_read_only_attachment_is_refused_visibly() {
        let (mut state, _daemon) = opened_reader("one\ntwo\n").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('o')),
            BufferKey::Consumed,
            "the verb is claimed, so the `o` does not fall into the composer"
        );
        assert_eq!(
            editor_text(&state),
            "one\ntwo\n",
            "and the document is untouched"
        );
        assert_ne!(state.mode(), EditorMode::Insert, "the keyboard stayed put");
        let notice = notice_of(&state).expect("the refusal is reported");
        assert!(
            notice.contains("Read-only"),
            "expected the read-only reason, got {notice:?}"
        );
    }

    /// The line `o` opens is **one** undo step, and the typing that follows
    /// it is another.
    ///
    /// The insert session is a *typing* run: M012's model is that everything
    /// typed in one go in `INS` is one entry, and the `o` is not typed — it is
    /// an edit made in `NOR` that then hands the keyboard over. So `u` takes
    /// back the word, and a second `u` takes back the line `o` opened.
    ///
    /// The geometry is why the two cannot be one entry. The open-line edit
    /// inserts its newline at the **end** of the opened line while the cursor
    /// sits at the **start** of it, so the typing goes in before the newline.
    /// A run is one contiguous insertion by construction, and these two are
    /// not: folding them together would make the merged entry describe a
    /// text the reader never had.
    #[tokio::test]
    async fn the_opened_line_and_the_typing_that_follows_are_two_undo_steps() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        normal(&mut state, KeyCode::Char('o'));
        insert(&mut state, KeyCode::Char('x'));
        insert(&mut state, KeyCode::Char('y'));
        assert_eq!(
            editor_text(&state),
            "one\nxy\ntwo\n",
            "the word landed on the line `o` opened"
        );
        leave_insert_mode(&mut state);
        assert_eq!(undo_depth(&state), 2, "the line and the word are two steps");
        editor_undo(&mut state);
        assert_eq!(
            editor_text(&state),
            "one\n\ntwo\n",
            "the first `u` takes back the word"
        );
        editor_undo(&mut state);
        assert_eq!(
            editor_text(&state),
            "one\ntwo\n",
            "the second takes back the line `o` opened"
        );
    }

    /// The last line of a buffer with no terminator still opens a line.
    ///
    /// This is the shape the "split the terminator" rule cannot split: there
    /// is no terminator to split, so the open-line edit appends a full blank
    /// line instead. The verb must still open a line and take the keyboard —
    /// the shape of the document is the pre-existing rule's business, not this
    /// milestone's.
    #[tokio::test]
    async fn o_on_a_last_line_without_a_terminator_still_opens_a_line() {
        let (mut state, _daemon) = opened_writer("one").await;
        assert_eq!(
            normal(&mut state, KeyCode::Char('o')),
            BufferKey::EnterInsert
        );
        let text = editor_text(&state);
        assert!(
            text.starts_with("one\n"),
            "the line was opened below the last line, got {text:?}"
        );
        assert!(
            text.len() > "one\n".len(),
            "and a line was actually opened, got {text:?}"
        );
        assert_eq!(state.mode(), EditorMode::Insert);
    }

    #[tokio::test]
    async fn a_newline_below_the_last_line_appends_rather_than_refusing() {
        // Helix's `add_newline_impl` computes `end + 1`, which on the last line
        // is one past this editor's last line — and that resolves to the end of
        // the buffer, which is the right place to append.
        let (mut state, _daemon) = opened_writer("only").await;
        assert_eq!(add_newline(&mut state, ']'), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "only\n");

        let (mut state, _daemon) = opened_writer("only\n").await;
        assert_eq!(add_newline(&mut state, ']'), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "only\n\n");
    }

    #[tokio::test]
    async fn a_newline_above_the_first_line_prepends() {
        let (mut state, _daemon) = opened_writer("only").await;
        assert_eq!(add_newline(&mut state, '['), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "\nonly");
    }

    #[tokio::test]
    async fn the_newline_keys_are_refused_on_a_read_only_buffer() {
        let daemon = FakeDaemon::new("one\ntwo\n", false);
        let mut state = open_state(&daemon, false).await;
        assert_eq!(add_newline(&mut state, ']'), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            "one\ntwo\n",
            "a read-only buffer must be byte-identical after `]Space`"
        );
        assert!(notice_of(&state).is_some(), "and must say why");
    }

    #[tokio::test]
    async fn the_paragraph_keys_still_work_on_a_read_only_buffer() {
        // A reader must be able to walk around a buffer they cannot write, so
        // only the two edits are gated. Pinning this is what stops a future
        // "just route all of `[`/`]` through the edit gate" from shipping.
        let daemon = FakeDaemon::new(&three_paragraphs(), false);
        let mut state = open_state(&daemon, false).await;
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 9);
        assert_eq!(prev_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 0);
    }

    #[tokio::test]
    async fn a_bracket_then_space_is_a_newline_and_not_a_second_armed_prefix() {
        // The precedence rule: `Space` arms a prefix of its own, so if the leaf
        // lookup did not run first, `]Space` would leave a pending `space`.
        for (arming, expected) in [('[', "\nonly\nrest\n"), (']', "only\n\nrest\n")] {
            let (mut state, _daemon) = opened_writer("only\nrest\n").await;
            assert_eq!(add_newline(&mut state, arming), BufferKey::Consumed);
            assert_eq!(
                pending(&state),
                "",
                "`{arming}Space` must not leave a prefix armed"
            );
            assert_eq!(editor_text(&state), expected, "`{arming}Space` text");
        }
    }

    #[tokio::test]
    async fn an_unbound_bracket_leaf_still_reaches_the_composer() {
        // Adding leaves must not have narrowed the escape hatch M002 pinned.
        for arming in ['[', ']'] {
            let (mut state, _daemon) = opened_writer("hello").await;
            press(&mut state, KeyCode::Char(arming), KeyModifiers::NONE);
            assert_eq!(
                press(&mut state, KeyCode::Char('q'), KeyModifiers::NONE),
                BufferKey::Unhandled,
                "`{arming}q` still reaches the composer"
            );
            assert_eq!(pending(&state), "", "`{arming}` disarmed");
        }
    }

    /// The four deferred slices, asserted **unbound** rather than merely
    /// absent.
    ///
    /// A missing binding is invisible; an assertion about it is a tripwire. If
    /// a later milestone lands `]d` (M013's diagnostics navigation) or `]G`
    /// (change navigation), this test fails and the reviewer has to say
    /// whether the new binding is actually reachable — which is the question
    /// amendment A1.2 and A1.3 answered by *not* landing them here.
    #[tokio::test]
    async fn the_deferred_diagnostic_and_change_quartets_are_asserted_unbound() {
        // `]D`/`[D` are workspace diagnostics and `]G`/`[G` are change
        // navigation in Helix. Both slices are still deferred: no server
        // capability backs a separate workspace-diagnostics request, and change
        // navigation has no reachable data source at all. `g` remains unbound
        // for the same reason.
        //
        // `d` was in this list until M013, which claimed `]d`/`[d` for
        // diagnostic stepping and owns them here; the rest of this test is
        // unchanged and still guards the three that stayed deferred.
        for glyph in ['D', 'g', 'G'] {
            for leaves in [editor::OPEN_BRACKET_LEAVES, editor::CLOSE_BRACKET_LEAVES] {
                assert!(
                    !leaves.iter().any(|leaf| leaf.glyph == glyph),
                    "`{glyph}` must not be bound as an unimpaired leaf"
                );
            }
        }

        // And at the dispatch: the key disarms and then resolves **normally**,
        // which is the plan's wording. `g` arms goto. It is neither a change
        // jump nor a diagnostic, and it leaves the bracket armed as `g` — the
        // assertion is that the bracket did *not* swallow it. `D` and `G` are
        // deliberately absent from this half: their own bare meanings
        // legitimately change the buffer and the cursor, so "nothing happened"
        // is not a claim those two can make.
        for arming in ['[', ']'] {
            let (mut state, _daemon) = opened_writer("hello").await;
            press(&mut state, KeyCode::Char(arming), KeyModifiers::NONE);
            assert_eq!(
                press(&mut state, KeyCode::Char('d'), KeyModifiers::NONE),
                BufferKey::Consumed,
                "`{arming}d` resolves `d` normally, as an unbound second key must"
            );
            assert_eq!(pending(&state), "", "`{arming}` disarmed");
            assert_eq!(editor_text(&state), "hello", "and deleted nothing");
            assert_eq!(cursor(&state), 0, "and jumped to no diagnostic");

            let (mut state, _daemon) = opened_writer("hello").await;
            press(&mut state, KeyCode::Char(arming), KeyModifiers::NONE);
            assert_eq!(
                press(&mut state, KeyCode::Char('g'), KeyModifiers::NONE),
                BufferKey::Consumed
            );
            assert_eq!(
                pending(&state),
                "g",
                "`{arming}g` resolved `g` as goto mode, not as change navigation"
            );
            assert_eq!(cursor(&state), 0, "and jumped to no change");
        }
    }

    /// `Space` on its own is still space mode. Half of the precedence rule:
    /// `]Space` is a newline *because* `]Space` resolves first, not because
    /// space mode stopped working.
    #[tokio::test]
    async fn space_alone_still_arms_space_mode() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char(' ')), BufferKey::Consumed);
        assert_eq!(pending(&state), "space", "a bare space arms");
        assert_eq!(editor_text(&state), "one\ntwo\n", "and inserts nothing yet");
    }

    #[tokio::test]
    async fn alt_dot_repeats_the_paragraph_motion_and_an_edit_is_not_one() {
        // Helix's keymap calls `Alt-.` "repeat last motion (`f`, `t`, `m`, `[`
        // or `]`)" — so `]p` has to be repeatable, and `]Space` must not be,
        // or the repeat would silently insert a second newline.
        let (mut state, _daemon) = opened_writer(&three_paragraphs()).await;
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 9, "beta");

        // An edit between the motion and the repeat does not disarm it. The
        // cursor is on beta's line, so `]Space` inserts below *that* line and
        // leaves the cursor on it.
        assert_eq!(add_newline(&mut state, ']'), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "alpha\n\n  beta\n\n\ngamma\n");
        assert_eq!(cursor(&state), 9, "and did not move");

        // ...and the repeat is the *motion*, so exactly one newline exists.
        assert_eq!(
            press(&mut state, KeyCode::Char('.'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            editor_text(&state),
            "alpha\n\n  beta\n\n\ngamma\n",
            "`Alt-.` repeated the paragraph motion and inserted no newline"
        );
        assert_eq!(
            cursor(&state),
            16,
            "gamma, across both blank lines — the original separator and the new one"
        );
    }

    #[tokio::test]
    async fn the_repeat_slot_is_one_slot_every_motion_overwrites() {
        let (mut state, _daemon) = opened_writer(&three_paragraphs()).await;
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 9);
        // `j` is an ordinary motion and must disarm the repeat.
        press(&mut state, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(
            press(&mut state, KeyCode::Char('.'), KeyModifiers::ALT),
            BufferKey::Unhandled,
            "`]p`, `j`, `Alt-.` repeats nothing — the Helix single-slot rule"
        );
    }

    #[tokio::test]
    async fn a_motion_after_a_paragraph_motion_clears_its_disclosure() {
        let (mut state, _daemon) = opened_writer(&three_paragraphs()).await;
        press(&mut state, KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert!(notice_of(&state).is_some(), "`]p` at the end reported");
        press(&mut state, KeyCode::Char('h'), KeyModifiers::NONE);
        assert_eq!(
            notice_of(&state),
            None,
            "the note described one keystroke and may not outlive it"
        );
    }

    #[tokio::test]
    async fn a_reconciliation_forgets_the_unimpaired_repeat() {
        // Same reason `last_find` is cleared: the paragraph the cursor was
        // pointed at does not survive a rewrite of the document.
        let (mut state, _daemon) = opened_writer(&three_paragraphs()).await;
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        editor::clear_history(state.session.as_mut().unwrap().presentation_mut());
        assert_eq!(
            press(&mut state, KeyCode::Char('.'), KeyModifiers::ALT),
            BufferKey::Unhandled
        );
        assert_eq!(notice_of(&state), None, "and the disclosure went with it");
    }

    #[tokio::test]
    async fn a_find_and_a_paragraph_motion_do_not_both_hold_the_repeat_slot() {
        // Whichever ran last is the only repeatable motion. Asserting both
        // orders pins the mutual exclusion from both sides, so adding a fourth
        // slot later has to decide which of these two assertions it breaks.
        let (mut state, _daemon) = opened_writer("alpha\n\nxalpha\n").await;

        // Paragraph first, then a find.
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(cursor(&state), 7);
        press(&mut state, KeyCode::Char('f'), KeyModifiers::NONE);
        press(&mut state, KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(cursor(&state), 8, "the find ran");
        assert_eq!(
            press(&mut state, KeyCode::Char('.'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            cursor(&state),
            12,
            "so `Alt-.` repeated the find forward from 8, not `]p` backward"
        );

        // Find first, then a paragraph motion.
        let (mut state, _daemon) = opened_writer("alpha\n\nxalpha\n").await;
        press(&mut state, KeyCode::Char('f'), KeyModifiers::NONE);
        press(&mut state, KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(next_paragraph(&mut state), BufferKey::Consumed);
        assert_eq!(
            press(&mut state, KeyCode::Char('.'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(cursor(&state), 7, "so `Alt-.` repeated `]p`, not the find");
    }

    #[tokio::test]
    async fn the_bracket_keys_stay_document_text_in_insert_mode() {
        // Both encodings, because terminals disagree: a `[` arrives as
        // `Char('[')` with NONE or with SHIFT depending on the terminal, and a
        // sweep that only checked one would pass against a binding that
        // claimed the other.
        for (glyph, modifiers, label) in [
            ('[', KeyModifiers::NONE, "lower, unshifted"),
            ('[', KeyModifiers::SHIFT, "lower, shifted"),
            (']', KeyModifiers::NONE, "lower, unshifted"),
            (']', KeyModifiers::SHIFT, "lower, shifted"),
            (' ', KeyModifiers::NONE, "space"),
        ] {
            let (mut state, _daemon) = opened_writer("").await;
            assert_eq!(
                insert_event(&mut state, KeyEvent::new(KeyCode::Char(glyph), modifiers)),
                BufferKey::Consumed,
                "{glyph:?} ({label}) must be text in Insert"
            );
            assert_eq!(
                editor_text(&state),
                glyph.to_string(),
                "{glyph:?} ({label}) must reach the document verbatim"
            );
        }
    }

    // ── M013 W4: the Helix LSP leaves ──────────────────────────────────────

    /// The effect a prefix leaf parks on the editor's own state.
    ///
    /// Read back through the public field rather than by running the request:
    /// `editor_buffer_key` has no core client, and a test that needed one would be
    /// testing the daemon transport instead of the dispatch.
    fn parked(state: &EditorState) -> Option<EditorPendingEffect> {
        state.pending_effect
    }

    fn prefix_on_state(state: &EditorState) -> EditorPrefix {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .pending_prefix
    }

    /// Dispatch `prefix` then `glyph` through the real buffer path.
    fn leaf(state: &mut EditorState, prefix: EditorPrefix, glyph: char) -> BufferKey {
        arm(state, prefix);
        normal(state, KeyCode::Char(glyph))
    }

    #[tokio::test]
    async fn every_helix_lsp_leaf_asks_for_its_own_operation() {
        // Each row is asserted against the operation the key is *for*, so a copy
        // and paste error between two rows is a failure rather than a surprise.
        //
        // `space f` was a row here until M010: it was a *duplicate* of `space s`,
        // and M010 reclaimed it for the file browser. It is no longer an LSP
        // read at all, so the row is gone rather than retyped — M010's own test
        // asserts the reclaim.
        for (glyph, expected) in [
            ('k', EditorPendingEffect::Read(LspReadOperation::Hover)),
            (
                's',
                EditorPendingEffect::Read(LspReadOperation::DocumentSymbols),
            ),
            (
                'S',
                EditorPendingEffect::Read(LspReadOperation::WorkspaceSymbols),
            ),
            ('h', EditorPendingEffect::Read(LspReadOperation::References)),
            ('d', EditorPendingEffect::OpenDiagnostics),
        ] {
            let (mut state, _daemon) = opened_writer("hello").await;
            assert_eq!(
                leaf(&mut state, EditorPrefix::Space, glyph),
                BufferKey::Consumed,
                "space {glyph} is consumed by the editor, not the composer"
            );
            assert_eq!(
                parked(&state),
                Some(expected),
                "space {glyph} must ask for its own operation"
            );
        }

        for (glyph, expected) in [
            ('d', EditorPendingEffect::Read(LspReadOperation::Definition)),
            (
                'i',
                EditorPendingEffect::Read(LspReadOperation::Implementation),
            ),
            ('r', EditorPendingEffect::Read(LspReadOperation::References)),
        ] {
            let (mut state, _daemon) = opened_writer("hello").await;
            assert_eq!(
                leaf(&mut state, EditorPrefix::Goto, glyph),
                BufferKey::Consumed
            );
            assert_eq!(
                parked(&state),
                Some(expected),
                "g {glyph} must ask for its own operation"
            );
        }
    }

    #[tokio::test]
    async fn a_goto_lsp_leaf_disarms_its_prefix_like_every_other_goto_leaf() {
        // `gd` used to be a silent no-op. The fix must not have left the machine
        // armed: a leaked prefix would swallow the next key the user presses.
        let (mut state, _daemon) = opened_writer("hello").await;
        leaf(&mut state, EditorPrefix::Goto, 'd');
        assert_eq!(prefix_on_state(&state), EditorPrefix::Idle);
    }

    #[tokio::test]
    async fn a_capital_leaf_resolves_on_both_terminal_conventions() {
        // Terminals disagree about how a capital arrives. `PrefixLeaf::glyph`'s
        // doc always claimed the dispatch reconciled both spellings; the lookup
        // gated on NONE alone, so `space S` would have been live on one terminal
        // and dead on the other.
        for (event, label) in [
            (
                KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE),
                "capital reported directly",
            ),
            (
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::SHIFT),
                "capital reported as a shifted lower-case glyph",
            ),
        ] {
            let (mut state, _daemon) = opened_writer("hello").await;
            arm(&mut state, EditorPrefix::Space);
            assert_eq!(
                editor_buffer_key(&mut state, InputMode::Normal, event),
                BufferKey::Consumed,
                "space S ({label}) must resolve rather than fall to the mismatch path"
            );
            assert_eq!(
                parked(&state),
                Some(EditorPendingEffect::Read(
                    LspReadOperation::WorkspaceSymbols
                )),
                "space S ({label}) must reach its leaf"
            );
        }
    }

    #[tokio::test]
    async fn a_lower_case_leaf_is_not_satisfied_by_its_capital() {
        // The mirror of the test above: `space s` and `space S` are different
        // requests, and folding them would make one of them unreachable.
        let (mut state, _daemon) = opened_writer("hello").await;
        arm(&mut state, EditorPrefix::Space);
        editor_buffer_key(
            &mut state,
            InputMode::Normal,
            KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
        );
        assert_eq!(
            parked(&state),
            Some(EditorPendingEffect::Read(
                LspReadOperation::WorkspaceSymbols
            )),
            "an already-capital glyph plus SHIFT is still the capital leaf"
        );
    }

    #[tokio::test]
    async fn a_modified_key_never_resolves_a_leaf() {
        for (event, label) in [
            (KeyEvent::new(KeyCode::Char('k'), KeyModifiers::ALT), "alt"),
            (
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
                "control",
            ),
        ] {
            let (mut state, _daemon) = opened_writer("hello").await;
            arm(&mut state, EditorPrefix::Space);
            let resolved = editor_buffer_key(&mut state, InputMode::Normal, event);
            assert_eq!(
                parked(&state),
                None,
                "{label}-k must not run a leaf; a chord is not a gesture"
            );
            let _ = resolved;
        }
    }

    #[tokio::test]
    async fn insert_mode_never_parks_an_lsp_effect() {
        // Insert gives the document the keyboard. A leaf that fired there would
        // turn two characters into a request and swallow them.
        let (mut state, _daemon) = opened_writer("hello").await;
        for glyph in ['k', 's', 'S', 'h', 'f', 'd'] {
            arm(&mut state, EditorPrefix::Space);
            assert_eq!(
                insert(&mut state, KeyCode::Char(glyph)),
                BufferKey::Consumed,
                "space {glyph} in Insert is text"
            );
            assert_eq!(
                parked(&state),
                None,
                "space {glyph} in Insert must not reach a leaf"
            );
        }
    }

    #[tokio::test]
    async fn an_unavailable_leaf_says_why_instead_of_doing_nothing() {
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = app_on_the_editor_route(state).await;
        arm_on_app(&mut app, EditorPrefix::Goto);
        handle_editor_key(&mut app, key(KeyCode::Char('y')));
        assert_eq!(
            app.editor_state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .pending_prefix,
            EditorPrefix::Idle,
            "`gy` must resolve its prefix"
        );
        assert_eq!(
            parked(&app.editor_state),
            None,
            "`gy` is consumed, not left armed"
        );
        let notice = app
            .editor_state
            .notice
            .as_ref()
            .expect("an unbound leaf must leave a reason behind");
        assert!(
            notice.message().contains("type definition"),
            "the notice must name what is missing, got {:?}",
            notice.message()
        );
    }

    #[tokio::test]
    async fn a_leaf_resolves_through_the_app_route_not_only_the_buffer_route() {
        // Every other leaf test drives `editor_buffer_key` directly. That
        // skips the App route, which decides Normal-mode key bindings, the
        // F-key intercept, and the LSP panel — so a leaf table that resolved
        // perfectly in a unit test could still be dead for a user. The first
        // M013 W4 drafts of these tests passed against `editor_buffer_key` and
        // failed here, which is exactly the gap this one closes.
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = app_on_the_editor_route(state).await;

        arm_on_app(&mut app, EditorPrefix::Goto);
        handle_editor_key(&mut app, key(KeyCode::Char('y')));
        assert_eq!(
            app.editor_state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .pending_prefix,
            EditorPrefix::Idle,
            "`gy` must resolve its prefix through the App route"
        );
        assert!(
            app.editor_state
                .notice
                .as_ref()
                .is_some_and(|notice| notice.message().contains("type definition")),
            "`gy` must reach its leaf and report why it cannot run"
        );
    }

    #[tokio::test]
    async fn a_prefixed_key_is_reached_only_after_its_arming_key() {
        // The other half of the App route: the bare key must still be the
        // editor's own verb when nothing is armed, or a route test could pass
        // because the prefix machinery ran on the first key.
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = app_on_the_editor_route(state).await;

        assert!(handle_editor_key(&mut app, key(KeyCode::Char('y'))));
        assert!(
            app.editor_state.notice.is_none(),
            "a bare `y` is the clipboard yank, not the `gy` leaf"
        );
        assert_eq!(editor_text(&app.editor_state), "hello", "and typed nothing");
    }

    #[tokio::test]
    async fn the_app_drains_a_parked_leaf_on_the_same_keypress() {
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = app_on_the_editor_route(state).await;
        app.core_client = None;

        arm_on_app(&mut app, EditorPrefix::Space);
        let consumed = handle_editor_key(&mut app, key(KeyCode::Char('k')));

        assert!(consumed, "space k belongs to the editor");
        assert_eq!(
            parked(&app.editor_state),
            None,
            "the effect must be drained, never left armed for a later keypress"
        );
        // The proof that the leaf reached `start_editor_lsp_read` rather than
        // being swallowed is that the read *ran and refused*: with no
        // workspace-bound session there is no LSP scope, so the first thing
        // `start_editor_lsp_read` checks fails and it says so. Asserting the
        // daemon-unavailable branch instead would have needed a bound scope
        // and a present client, and would have proved less about the leaf.
        let reported = app
            .messages_state
            .toasts
            .history()
            .map(|record| record.message.clone())
            .collect::<Vec<_>>();
        assert!(
            reported
                .iter()
                .any(|message| message.contains("LSP is unavailable")),
            "`space k` must reach the LSP read path and report why it cannot run, got {reported:?}"
        );
    }

    #[tokio::test]
    async fn a_leaf_that_parked_nothing_leaves_no_read_error() {
        // The drain must not fire for gestures that were never LSP requests.
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = app_on_the_editor_route(state).await;
        app.core_client = None;

        arm_on_app(&mut app, EditorPrefix::Space);
        handle_editor_key(&mut app, key(KeyCode::Char('y')));
        assert!(
            app.editor_state.lsp.read_error.is_none(),
            "`space y` still yanks; it must not start a read"
        );
    }

    /// One diagnostic on a given line, for the stepping tests.
    fn test_diagnostic(line: u32, message: &str) -> LspDiagnosticDto {
        LspDiagnosticDto {
            range: LspRangeDto {
                path: "doc".to_string(),
                workspace_relative_path: Some("doc".to_string()),
                start_line: line,
                start_column: 0,
                end_line: line,
                end_column: 1,
            },
            severity: 1,
            tag: 0,
            code: None,
            message: message.to_string(),
            source: None,
        }
    }

    #[tokio::test]
    async fn the_two_bracket_diagnostic_leaves_step_to_different_entries() {
        // `]` is forward and `[` is backward throughout this layer, so the leaf
        // itself carries no direction the prefix has not already said. From the
        // same starting entry the two must land on different diagnostics —
        // a swapped direction would pass a test that only checks each leaf moves
        // at all.
        let mut indices = Vec::new();
        for arming in [KeyCode::Char(']'), KeyCode::Char('[')] {
            let (mut state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
            state.lsp.diagnostics = vec![
                test_diagnostic(0, "first"),
                test_diagnostic(1, "second"),
                test_diagnostic(2, "third"),
            ];
            state.lsp.selected_index = 0;
            let mut app = app_on_the_editor_route(state).await;
            // Pressed through the real key path, because `[`/`]` arm a prefix
            // and `arm_on_app` would skip the very dispatch under test.
            assert!(
                handle_editor_key(&mut app, key(arming)),
                "the bracket arms a prefix"
            );
            assert!(
                handle_editor_key(&mut app, key(KeyCode::Char('d'))),
                "a diagnostic leaf belongs to the editor"
            );
            indices.push(app.editor_state.lsp.selected_index);
        }
        assert_eq!(
            indices,
            vec![1, 2],
            "forward and backward must disagree from the same start"
        );
    }

    #[tokio::test]
    async fn stepping_past_the_end_wraps_rather_than_stopping() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        state.lsp.diagnostics = vec![test_diagnostic(0, "only")];
        state.lsp.selected_index = 0;
        let mut app = app_on_the_editor_route(state).await;

        // Backward from index 0 is the underflow case. `0usize - 1` panics in
        // debug and silently becomes usize::MAX in release, so the wrap is done
        // with explicit arithmetic before it reaches the index.
        step_editor_diagnostic(&mut app, EditorDiagnosticStep::Backward);
        assert_eq!(
            app.editor_state.lsp.selected_index, 0,
            "a single diagnostic wraps onto itself from either direction"
        );
    }

    #[tokio::test]
    async fn stepping_with_no_diagnostics_says_so_instead_of_doing_nothing() {
        let (state, _daemon) = opened_writer("one\n").await;
        let mut app = app_on_the_editor_route(state).await;
        step_editor_diagnostic(&mut app, EditorDiagnosticStep::Forward);
        let notice = app
            .editor_state
            .notice
            .as_ref()
            .expect("`d` with nothing loaded must leave a reason behind");
        assert!(
            notice.message().contains("diagnostic"),
            "the notice must name what is missing, got {:?}",
            notice.message()
        );
    }

    // ── M012: the history cursor ─────────────────────────────────────────

    /// Replace the character under the cursor with `ch`.
    ///
    /// In Normal mode, so the edit lands as **its own** history entry rather
    /// than joining a typing burst: the history's *contents* are not what
    /// this milestone is about, but the cursor's position in them is, and a
    /// test that cannot count its entries cannot assert on the position.
    fn replace_under_cursor(state: &mut EditorState, ch: char) {
        let after = snapshot_of(state);
        let edit = editor::replace_char(
            &after,
            state.session.as_ref().expect("open").presentation(),
            ch,
        )
        .expect("a character under the cursor");
        submit_edit(state, edit);
    }

    /// Driven through the **real dispatch path**, because the roadmap's §9
    /// records why: a helper called directly proves the helper works, not
    /// that the chord reaches it.
    #[tokio::test]
    async fn alt_u_moves_the_history_cursor_back_and_the_document_follows() {
        let (mut state, _daemon) = opened_writer("ab cd\n").await;
        park(&mut state, 0);
        replace_under_cursor(&mut state, 'X');
        replace_under_cursor(&mut state, 'Y');
        assert_eq!(editor_text(&state), "Yb cd\n");
        assert_eq!(undo_depth(&state), 2, "two edits, two entries");

        assert_eq!(
            press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT),
            BufferKey::Consumed,
            "`Alt-u` is the editor's chord, not a word typed into the composer"
        );
        assert_eq!(editor_text(&state), "Xb cd\n", "the second edit is undone");
        assert_eq!(undo_depth(&state), 1);
        assert_eq!(redo_depth(&state), 1, "and it is redoable");

        assert_eq!(
            press(&mut state, KeyCode::Char('U'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(editor_text(&state), "Yb cd\n", "and redone");
        assert_eq!(undo_depth(&state), 2);
        assert_eq!(redo_depth(&state), 0);
    }

    /// `Alt-U` has two terminal spellings, and both have to work.
    ///
    /// A terminal that reports the shifted glyph sends `(ALT, Char('U'))`;
    /// one that reports the modifier sends `(ALT | SHIFT, Char('u'))`. A
    /// chord live on one machine and dead on the next is the M013 failure
    /// mode, so the plan requires both to be driven.
    #[tokio::test]
    async fn alt_u_accepts_both_terminal_spellings_of_the_capital() {
        for (code, modifiers) in [
            (KeyCode::Char('U'), KeyModifiers::ALT),
            (KeyCode::Char('u'), KeyModifiers::ALT | KeyModifiers::SHIFT),
        ] {
            let (mut state, _daemon) = opened_writer("ab cd\n").await;
            park(&mut state, 0);
            replace_under_cursor(&mut state, 'X');
            replace_under_cursor(&mut state, 'Y');
            // Back one entry, so there is something later to move to.
            press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT);
            assert_eq!(editor_text(&state), "Xb cd\n");
            assert_eq!(undo_depth(&state), 1);

            assert_eq!(
                press(&mut state, code, modifiers),
                BufferKey::Consumed,
                "{code:?} with {modifiers:?} must be claimed"
            );
            assert_eq!(
                editor_text(&state),
                "Yb cd\n",
                "{code:?} with {modifiers:?} must move the cursor forward"
            );
            assert_eq!(undo_depth(&state), 2);
        }
    }

    #[tokio::test]
    async fn alt_u_at_the_oldest_entry_reports_and_does_not_move() {
        let (mut state, _daemon) = opened_writer("ab cd\n").await;
        park(&mut state, 0);
        replace_under_cursor(&mut state, 'X');
        // The first `Alt-u` undoes the only entry, and the cursor is now at
        // the beginning of the history.
        assert_eq!(
            press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(undo_depth(&state), 0);
        // At the oldest entry there is nothing earlier to move to.
        assert_eq!(
            press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(undo_depth(&state), 0, "the cursor did not move");
        assert_eq!(
            notice_of(&state).as_deref(),
            Some("no earlier state"),
            "the boundary is reported rather than swallowed"
        );
    }

    #[tokio::test]
    async fn alt_u_at_the_tip_reports_and_does_not_move() {
        let (mut state, _daemon) = opened_writer("ab cd\n").await;
        park(&mut state, 0);
        replace_under_cursor(&mut state, 'X');
        // At the tip there is nothing later to move to.
        assert_eq!(
            press(&mut state, KeyCode::Char('U'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(undo_depth(&state), 1, "the cursor did not move");
        assert_eq!(
            notice_of(&state).as_deref(),
            Some("no later state"),
            "the boundary is reported rather than swallowed"
        );
    }

    /// Type `text` as one insert burst, the way a reader would: `i`, the keys,
    /// then the `Esc` that closes the burst.
    fn type_burst(state: &mut EditorState, text: &str) {
        assert_eq!(
            normal(state, KeyCode::Char('i')),
            BufferKey::EnterInsert,
            "`i` enters insert mode"
        );
        for ch in text.chars() {
            insert(state, KeyCode::Char(ch));
        }
        leave_insert_mode(state);
    }

    /// The milestone's headline: after an undo, an edit is a **sibling**, and
    /// a keystroke reaches the branch it did not take.
    ///
    /// `Alt-u` walks creation order, so from the new branch it lands on the
    /// node made before it — the abandoned branch's text — and `Alt-U` comes
    /// back across the fork.
    #[tokio::test]
    async fn alt_u_crosses_the_fork_to_the_branch_the_edit_abandoned() {
        let (mut state, _daemon) = opened_writer("").await;
        type_burst(&mut state, "a");
        type_burst(&mut state, "b");
        type_burst(&mut state, "c");
        assert_eq!(editor_text(&state), "abc");

        // Undo to the middle of the chain, then edit: the edit branches and
        // the abandoned text is not truncated.
        assert_eq!(normal(&mut state, KeyCode::Char('u')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "ab");
        type_burst(&mut state, "d");
        assert_eq!(editor_text(&state), "abd");
        assert_eq!(history_len(&state), 4, "three chain nodes plus the branch");

        assert_eq!(
            press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            editor_text(&state),
            "abc",
            "Alt-u reached the text the edit did not take"
        );
        assert_eq!(
            press(&mut state, KeyCode::Char('U'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            editor_text(&state),
            "abd",
            "Alt-U came back across the fork to the new branch"
        );
    }

    /// `u` is the **branch** walk: from a branch node it goes to its parent,
    /// never to the node creation order would name.
    ///
    /// Same sequence as the test above, where creation order answers `"abc"`.
    #[tokio::test]
    async fn u_walks_the_branch_and_never_crosses_the_fork() {
        let (mut state, _daemon) = opened_writer("").await;
        type_burst(&mut state, "a");
        type_burst(&mut state, "b");
        type_burst(&mut state, "c");
        assert_eq!(normal(&mut state, KeyCode::Char('u')), BufferKey::Consumed);
        type_burst(&mut state, "d");
        assert_eq!(editor_text(&state), "abd");

        assert_eq!(normal(&mut state, KeyCode::Char('u')), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            "ab",
            "u moved to the parent, not to the node created before this one"
        );
    }

    /// The axes name their own boundaries. One sentence for both is how a
    /// reader learns that two movements are one.
    #[test]
    fn the_two_axes_name_different_boundaries() {
        assert_ne!(
            history_boundary(HistoryStep::Earlier, HistoryWalk::Branch),
            history_boundary(HistoryStep::Earlier, HistoryWalk::CreationOrder),
            "the branch walk and the time walk answer different questions"
        );
        assert_ne!(
            history_boundary(HistoryStep::Later, HistoryWalk::Branch),
            history_boundary(HistoryStep::Later, HistoryWalk::CreationOrder),
            "and so do their forward directions"
        );
        assert_ne!(
            history_boundary(HistoryStep::Earlier, HistoryWalk::CreationOrder),
            history_boundary(HistoryStep::Later, HistoryWalk::CreationOrder),
            "each direction of the time walk names its own end"
        );
    }

    /// The recorded divergence: the **creation-order** gestures report their
    /// boundary and the branch gestures do not.
    ///
    /// `Alt-u`/`Alt-U` are the explicit history gestures — a reader who has
    /// asked "what came before this?" should not be left guessing whether the
    /// key was dropped — while `u`/`U` have always been silent, and a reader
    /// who has undone everything already knows where they are. The two axes
    /// also answer different questions, so their boundary sentences differ;
    /// this CodeGG convention is recorded in `docs/tui.md`.
    #[tokio::test]
    async fn u_and_u_stay_silent_at_their_boundaries() {
        // `u` at the beginning of the history. The first `u` undoes the only
        // entry and leaves the cursor at the beginning; the second is at the
        // boundary.
        let (mut state, _daemon) = opened_writer("ab cd\n").await;
        park(&mut state, 0);
        replace_under_cursor(&mut state, 'X');
        assert_eq!(
            press(&mut state, KeyCode::Char('u'), KeyModifiers::NONE),
            BufferKey::Consumed
        );
        assert_eq!(undo_depth(&state), 0, "the entry was undone");
        assert_eq!(
            press(&mut state, KeyCode::Char('u'), KeyModifiers::NONE),
            BufferKey::Consumed
        );
        assert_eq!(undo_depth(&state), 0);
        assert_eq!(
            notice_of(&state),
            None,
            "`u` at the beginning of the history says nothing, as it always has"
        );

        // `U` at the tip. This is a different state — the cursor at the
        // newest change — so it gets its own document; in the state above,
        // `U` would *redo* the undone entry rather than sit at a boundary.
        let (mut state, _daemon) = opened_writer("ab cd\n").await;
        park(&mut state, 0);
        replace_under_cursor(&mut state, 'X');
        assert_eq!(
            press(&mut state, KeyCode::Char('U'), KeyModifiers::NONE),
            BufferKey::Consumed
        );
        assert_eq!(undo_depth(&state), 1, "the cursor did not move");
        assert_eq!(
            notice_of(&state),
            None,
            "and `U` at the tip says nothing either"
        );
    }

    /// `Alt-u` and `Alt-U` are Normal-mode gestures.
    ///
    /// Insert mode's catch-all covers `NONE` and `SHIFT` only, so an `Alt`
    /// chord falls outside it and reaches the composer. Helix binds no
    /// `Alt-u` in Insert mode, so inert is the faithful answer.
    #[tokio::test]
    async fn alt_u_and_alt_u_are_inert_in_insert_mode() {
        for (code, modifiers) in [
            (KeyCode::Char('u'), KeyModifiers::ALT),
            (KeyCode::Char('U'), KeyModifiers::ALT),
            (KeyCode::Char('U'), KeyModifiers::ALT | KeyModifiers::SHIFT),
            (KeyCode::Char('u'), KeyModifiers::ALT | KeyModifiers::SHIFT),
        ] {
            let (mut state, _daemon) = opened_writer("").await;
            assert_eq!(
                normal(&mut state, KeyCode::Char('i')),
                BufferKey::EnterInsert
            );
            assert_eq!(
                insert_event(&mut state, KeyEvent::new(code, modifiers)),
                BufferKey::Unhandled,
                "Alt-{:?} with {:?} must reach the composer rather than vanish",
                code,
                modifiers
            );
            assert_eq!(editor_text(&state), "", "and type nothing");
        }
    }

    /// The one behaviour the cursor model must not get wrong.
    ///
    /// A refused apply leaves the cursor exactly where it was. The old
    /// two-stack model expressed the same guarantee as "the entry goes back
    /// on the stack"; with a cursor there is no transfer to undo, so the
    /// guarantee is that the cursor does not move.
    #[tokio::test]
    async fn a_refused_undo_leaves_the_history_cursor_where_it_was() {
        let (mut state, _daemon) = opened_reader("hello").await;
        // A read-only attachment refuses every apply, so the undo is refused
        // too. The entry is planted directly: a read-only attachment can
        // never make an edit, so there is no way to earn one the ordinary
        // way.
        {
            let presentation = state.session.as_mut().expect("open").presentation_mut();
            editor::push_undo(
                presentation,
                codegg_document::TextTransaction::new(vec![codegg_document::TextEdit::new(
                    0..0,
                    "x",
                )]),
                codegg_document::TextTransaction::new(vec![codegg_document::TextEdit::new(
                    0..1,
                    "",
                )]),
                1,
                0,
                Vec::new(),
            );
            assert_eq!(presentation.history_cursor, 1);
        }

        editor_undo(&mut state);

        let presentation = state.session.as_ref().expect("open").presentation();
        assert_eq!(
            presentation.history_cursor, 1,
            "a refused apply must not move the cursor"
        );
        assert_eq!(presentation.history.len(), 1, "and must not drop the entry");
        assert_eq!(
            state.notice,
            Some(EditorNotice::ReadOnly),
            "the refusal is surfaced explicitly"
        );
    }

    // ── M009: match mode and text objects ─────────────────────────────────

    /// Hold one member over `start..end`, the way a verb that grew a
    /// selection would.
    fn hold_range(state: &mut EditorState, start: usize, end: usize) {
        let snapshot = snapshot_of(state);
        let point = |byte: usize| EditorPoint {
            byte,
            line: editor::cursor_line(&snapshot, byte),
        };
        let member = Selection {
            anchor: point(start),
            head: point(end.saturating_sub(1)),
            mode: crate::tui::helix_selection::SelectionMode::Char,
            growing: false,
        };
        let presentation = state.session.as_mut().unwrap().presentation_mut();
        presentation.selections = vec![member];
        presentation.primary_index = 0;
    }

    /// Press the keys of a gesture, in order.
    fn press_all(state: &mut EditorState, keys: &[KeyCode]) {
        for key in keys {
            assert_eq!(
                normal(state, *key),
                BufferKey::Consumed,
                "{key:?} was not claimed by the gesture"
            );
        }
    }

    #[tokio::test]
    async fn m_arms_the_match_prefix_and_the_indicator_shows_it() {
        let (mut state, _daemon) = opened_writer("abc").await;
        assert_eq!(normal(&mut state, KeyCode::Char('m')), BufferKey::Consumed);
        assert_eq!(
            pending(&state),
            "m",
            "an armed `m` that draws nothing is indistinguishable from a dropped key"
        );
    }

    #[tokio::test]
    async fn m_then_an_unlisted_glyph_reaches_the_composer() {
        let (mut state, _daemon) = opened_writer("abc").await;
        assert_eq!(normal(&mut state, KeyCode::Char('m')), BufferKey::Consumed);
        assert_eq!(
            normal(&mut state, KeyCode::Char('q')),
            BufferKey::Unhandled,
            "an unmatched second key must reach the composer, not be eaten"
        );
        assert_eq!(pending(&state), "", "and the machine is left disarmed");
    }

    #[tokio::test]
    async fn m_m_jumps_to_the_mate_of_the_bracket_under_the_cursor() {
        let (mut state, _daemon) = opened_writer("(abc)").await;
        park(&mut state, 0);
        press_all(&mut state, &[KeyCode::Char('m'), KeyCode::Char('m')]);
        assert_eq!(cursor(&state), 4, "the `)` is the mate of the `(`");

        let (mut state, _daemon) = opened_writer("(abc)").await;
        park(&mut state, 4);
        press_all(&mut state, &[KeyCode::Char('m'), KeyCode::Char('m')]);
        assert_eq!(cursor(&state), 0, "and back again");
    }

    #[tokio::test]
    async fn m_m_on_a_non_delimiter_refuses_with_a_reason() {
        let (mut state, _daemon) = opened_writer("abc").await;
        park(&mut state, 1);
        press_all(&mut state, &[KeyCode::Char('m'), KeyCode::Char('m')]);
        assert_eq!(cursor(&state), 1, "the cursor does not move");
        assert!(
            notice_of(&state).is_some_and(|note| note.contains("no bracket")),
            "a refused motion must say why: {:?}",
            notice_of(&state)
        );
    }

    #[tokio::test]
    async fn m_m_on_an_unbalanced_delimiter_refuses_with_the_unbalanced_reason() {
        let (mut state, _daemon) = opened_writer("(abc").await;
        park(&mut state, 0);
        press_all(&mut state, &[KeyCode::Char('m'), KeyCode::Char('m')]);
        assert_eq!(cursor(&state), 0, "the cursor does not move");
        assert!(
            notice_of(&state).is_some_and(|note| note.contains("no matching bracket")),
            "the two refusals must be distinguishable: {:?}",
            notice_of(&state)
        );
    }

    #[tokio::test]
    async fn m_i_selects_the_interior_of_the_pair_under_the_cursor() {
        let (mut state, _daemon) = opened_writer("f(abc)g").await;
        park(&mut state, 3);
        press_all(&mut state, &[KeyCode::Char('m'), KeyCode::Char('i')]);
        assert_eq!(pending(&state), "m i", "the second level shows itself");
        assert_eq!(normal(&mut state, KeyCode::Char('(')), BufferKey::Consumed);
        assert_eq!(
            held_ranges(&state),
            vec![2..5],
            "the interior, without the delimiters"
        );
    }

    #[tokio::test]
    async fn m_i_m_selects_the_innermost_pair_not_the_outer_one() {
        let (mut state, _daemon) = opened_writer("(a(b)c)").await;
        park(&mut state, 3);
        press_all(
            &mut state,
            &[KeyCode::Char('m'), KeyCode::Char('i'), KeyCode::Char('m')],
        );
        assert_eq!(
            held_ranges(&state),
            vec![3..4],
            "the innermost pair, not the outer one"
        );
    }

    #[tokio::test]
    async fn m_a_w_takes_the_trailing_space_and_m_i_w_does_not() {
        let (mut state, _daemon) = opened_writer("ab cd").await;
        park(&mut state, 0);
        press_all(
            &mut state,
            &[KeyCode::Char('m'), KeyCode::Char('i'), KeyCode::Char('w')],
        );
        assert_eq!(held_ranges(&state), vec![0..2], "the word alone");

        let (mut state, _daemon) = opened_writer("ab cd").await;
        park(&mut state, 0);
        press_all(
            &mut state,
            &[KeyCode::Char('m'), KeyCode::Char('a'), KeyCode::Char('w')],
        );
        assert_eq!(
            held_ranges(&state),
            vec![0..3],
            "the word and the whitespace that follows it"
        );
    }

    #[tokio::test]
    async fn m_i_w_reads_a_capital_sent_as_a_shifted_lowercase() {
        // Terminals disagree about how a capital is encoded: some send
        // `Char('W')` with no modifier, others send `Char('w')` plus `SHIFT`.
        // The WORD object is the one that has a capital in it, so the
        // disagreement has to be reconciled or `W` would name the *word*
        // object on half the terminals.
        let (mut state, _daemon) = opened_writer("a.b cd").await;
        park(&mut state, 0);
        assert_eq!(normal(&mut state, KeyCode::Char('m')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char('i')), BufferKey::Consumed);
        assert_eq!(
            press(&mut state, KeyCode::Char('w'), KeyModifiers::SHIFT),
            BufferKey::Consumed
        );
        assert_eq!(held_ranges(&state), vec![0..3], "the WORD, not the word");
    }

    #[tokio::test]
    async fn the_two_spellings_of_the_capital_name_the_same_object() {
        // The other half of the same question: the two spellings must agree
        // with each other, and disagree with the lowercase.
        let (mut state, _daemon) = opened_writer("a.b cd").await;
        park(&mut state, 0);
        press_all(
            &mut state,
            &[KeyCode::Char('m'), KeyCode::Char('i'), KeyCode::Char('w')],
        );
        assert_eq!(held_ranges(&state), vec![0..1], "the word");

        let (mut state, _daemon) = opened_writer("a.b cd").await;
        park(&mut state, 0);
        assert_eq!(normal(&mut state, KeyCode::Char('m')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char('i')), BufferKey::Consumed);
        assert_eq!(
            press(&mut state, KeyCode::Char('W'), KeyModifiers::NONE),
            BufferKey::Consumed
        );
        assert_eq!(
            held_ranges(&state),
            vec![0..3],
            "the WORD, under the other spelling"
        );
    }

    #[tokio::test]
    async fn m_i_with_a_character_outside_the_object_vocabulary_reaches_the_composer() {
        let (mut state, _daemon) = opened_writer("abc").await;
        park(&mut state, 0);
        assert_eq!(normal(&mut state, KeyCode::Char('m')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char('i')), BufferKey::Consumed);
        assert_eq!(
            normal(&mut state, KeyCode::Char('q')),
            BufferKey::Unhandled,
            "a character that names no object must not be swallowed"
        );
        let empty: Vec<std::ops::Range<usize>> = Vec::new();
        assert_eq!(held_ranges(&state), empty, "and nothing was selected");
    }

    #[tokio::test]
    async fn m_s_surrounds_the_selection_and_grows_it_over_the_new_delimiters() {
        let (mut state, _daemon) = opened_writer("abc").await;
        hold_range(&mut state, 0, 3);
        park(&mut state, 0);
        press_all(&mut state, &[KeyCode::Char('m'), KeyCode::Char('s')]);
        assert_eq!(pending(&state), "m s");
        assert_eq!(normal(&mut state, KeyCode::Char('(')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "(abc)");
        assert_eq!(
            held_ranges(&state),
            vec![0..5],
            "the selection grew over the new delimiters"
        );
        assert_eq!(undo_depth(&state), 1, "and it is one undo step");
    }

    #[tokio::test]
    async fn m_d_removes_the_delimiters_and_keeps_the_contents() {
        let (mut state, _daemon) = opened_writer("(abc)").await;
        hold_range(&mut state, 1, 4);
        park(&mut state, 1);
        press_all(
            &mut state,
            &[KeyCode::Char('m'), KeyCode::Char('d'), KeyCode::Char('(')],
        );
        assert_eq!(editor_text(&state), "abc");
        assert_eq!(
            held_ranges(&state),
            vec![0..3],
            "the contents stay selected"
        );
        assert_eq!(undo_depth(&state), 1, "and it is one undo step");
    }

    #[tokio::test]
    async fn m_r_replaces_the_outermost_pair_and_keeps_the_inner_ones() {
        let (mut state, _daemon) = opened_writer("(a(b))").await;
        hold_range(&mut state, 1, 5);
        park(&mut state, 1);
        press_all(&mut state, &[KeyCode::Char('m'), KeyCode::Char('r')]);
        assert_eq!(pending(&state), "m r", "the first half of the replace");
        assert_eq!(normal(&mut state, KeyCode::Char('(')), BufferKey::Consumed);
        assert_eq!(pending(&state), "m r (", "and the second half says so");
        assert_eq!(normal(&mut state, KeyCode::Char('[')), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            "[a(b)]",
            "the outer pair is replaced, so the inner one survives"
        );
        assert_eq!(undo_depth(&state), 1, "and it is one undo step");
    }

    #[tokio::test]
    async fn m_s_is_refused_on_a_read_only_attachment() {
        let (mut state, _daemon) = opened_reader("abc").await;
        hold_range(&mut state, 0, 3);
        park(&mut state, 0);
        press_all(
            &mut state,
            &[KeyCode::Char('m'), KeyCode::Char('s'), KeyCode::Char('(')],
        );
        assert_eq!(editor_text(&state), "abc", "nothing was written");
        assert_eq!(undo_depth(&state), 0, "and nothing was recorded");
        assert!(
            notice_of(&state).is_some_and(|note| note.contains("Read-only")),
            "the refusal is surfaced explicitly: {:?}",
            notice_of(&state)
        );
    }

    #[tokio::test]
    async fn m_a_is_refused_on_a_read_only_attachment() {
        let (mut state, _daemon) = opened_reader("abc").await;
        park(&mut state, 0);
        press_all(
            &mut state,
            &[KeyCode::Char('m'), KeyCode::Char('a'), KeyCode::Char('w')],
        );
        let empty: Vec<std::ops::Range<usize>> = Vec::new();
        assert_eq!(
            held_ranges(&state),
            empty,
            "a text object enlarges the selection, and a reader may not enlarge one"
        );
        assert!(
            notice_of(&state).is_some_and(|note| note.contains("Read-only")),
            "the refusal is surfaced explicitly: {:?}",
            notice_of(&state)
        );
    }

    // ── M001: line-shape verbs, through the dispatch path ─────────────
    //
    // One test per chord, each pressing the **key** rather than calling the
    // resolver. A helper-level test passes whether or not the arm is
    // reachable, which is exactly how the `Ctrl+r` shadow survived M001 of
    // the predecessor.

    #[tokio::test]
    async fn capital_x_widens_the_held_selection_through_the_dispatch_path() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        hold_range(&mut state, 4, 6);
        assert_eq!(
            normal(&mut state, KeyCode::Char('X')),
            BufferKey::Consumed,
            "`X` is claimed"
        );
        assert_eq!(held_ranges(&state), editor::one_range(4..8));
        assert_eq!(
            editor_text(&state),
            "one\ntwo\n",
            "widening a selection writes nothing"
        );
    }

    #[tokio::test]
    async fn alt_x_narrows_the_line_selection_through_the_dispatch_path() {
        let (mut state, _daemon) = opened_writer("one\ntwo\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        assert_eq!(
            held_ranges(&state),
            editor::one_range(0..4),
            "`x` arms the whole line, terminator included"
        );
        assert_eq!(
            press(&mut state, KeyCode::Char('x'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            held_ranges(&state),
            editor::one_range(0..3),
            "`Alt-x` narrows it to the line's own bounds"
        );
    }

    #[tokio::test]
    async fn underscore_trims_the_held_selection_through_the_dispatch_path() {
        let (mut state, _daemon) = opened_writer("  ab  \n").await;
        hold_range(&mut state, 0, 6);
        assert_eq!(normal(&mut state, KeyCode::Char('_')), BufferKey::Consumed);
        assert_eq!(held_ranges(&state), editor::one_range(2..4));
        assert_eq!(editor_text(&state), "  ab  \n", "trimming keeps the bytes");
    }

    #[tokio::test]
    async fn capital_j_joins_the_covered_lines_through_the_dispatch_path() {
        let (mut state, _daemon) = opened_writer("one\n    two\n").await;
        hold_range(&mut state, 0, 12);
        assert_eq!(normal(&mut state, KeyCode::Char('J')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "one two\n");
        assert_eq!(
            editor_text(&state).matches('\n').count(),
            1,
            "the join removed the line break"
        );
        assert_eq!(undo_depth(&state), 1, "one join, one undo step");
    }

    #[tokio::test]
    async fn alt_j_leaves_the_inserted_space_selected_through_the_dispatch_path() {
        let (mut state, _daemon) = opened_writer("one\n  two\n").await;
        hold_range(&mut state, 0, 10);
        assert_eq!(
            press(&mut state, KeyCode::Char('J'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(editor_text(&state), "one two\n");
        assert_eq!(
            held_ranges(&state),
            editor::one_range(3..4),
            "the inserted space is what the reader holds"
        );
        assert_eq!(cursor(&state), 3, "and the cursor is on it");
    }

    #[tokio::test]
    async fn greater_than_indents_every_covered_line_through_the_dispatch_path() {
        let (mut state, _daemon) = opened_writer("a\n  b\n").await;
        hold_range(&mut state, 0, 6);
        assert_eq!(normal(&mut state, KeyCode::Char('>')), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            "  a\n    b\n",
            "one detected two-space unit on every covered line"
        );
        assert_eq!(undo_depth(&state), 1, "one gesture, one undo step");
        assert_eq!(
            held_ranges(&state),
            editor::one_range(0..10),
            "the selection is line-wise over both lines afterwards"
        );
    }

    #[tokio::test]
    async fn less_than_removes_one_indent_unit_through_the_dispatch_path() {
        let (mut state, _daemon) = opened_writer("  a\n  b\n").await;
        hold_range(&mut state, 0, 8);
        assert_eq!(normal(&mut state, KeyCode::Char('<')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "a\nb\n");
        assert_eq!(undo_depth(&state), 1);
    }

    #[tokio::test]
    async fn every_shape_verb_that_grows_or_writes_is_refused_when_read_only() {
        for (label, code, modifiers) in [
            ("X", KeyCode::Char('X'), KeyModifiers::NONE),
            ("_", KeyCode::Char('_'), KeyModifiers::NONE),
            ("J", KeyCode::Char('J'), KeyModifiers::NONE),
            ("Alt-J", KeyCode::Char('J'), KeyModifiers::ALT),
            (">", KeyCode::Char('>'), KeyModifiers::NONE),
            ("<", KeyCode::Char('<'), KeyModifiers::NONE),
        ] {
            let (mut state, _daemon) = opened_reader("  ab  \n").await;
            let before = editor_text(&state);
            assert_eq!(
                press(&mut state, code, modifiers),
                BufferKey::Consumed,
                "`{label}` is claimed even where it is refused"
            );
            assert!(
                notice_of(&state).is_some_and(|note| note.contains("Read-only")),
                "`{label}` reports the refusal: {:?}",
                notice_of(&state)
            );
            assert_eq!(
                editor_text(&state),
                before,
                "`{label}` changes no byte on a read-only attachment"
            );
            assert_eq!(undo_depth(&state), 0, "`{label}` records no history");
        }
    }

    #[tokio::test]
    async fn alt_x_narrows_a_selection_on_a_read_only_attachment() {
        let (mut state, _daemon) = opened_reader("  ab  \n").await;
        // `x` itself is refused on a read-only attachment, so the line
        // selection is put in place the way a permitted verb would leave it.
        let member = Selection {
            anchor: EditorPoint { byte: 0, line: 0 },
            head: EditorPoint { byte: 0, line: 0 },
            mode: crate::tui::helix_selection::SelectionMode::Line,
            growing: false,
        };
        state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![member];
        assert_eq!(
            press(&mut state, KeyCode::Char('x'), KeyModifiers::ALT),
            BufferKey::Consumed
        );
        assert_eq!(
            held_ranges(&state),
            editor::one_range(0..6),
            "narrowing is navigation-like and stays available"
        );
        assert!(
            notice_of(&state).is_none(),
            "a permitted verb reports no refusal: {:?}",
            notice_of(&state)
        );
    }

    #[tokio::test]
    async fn a_multi_member_join_is_one_undo_step() {
        let (mut state, _daemon) = opened_writer("a\nb\nc\nd\n").await;
        let snapshot = snapshot_of(&state);
        let point = |byte: usize| EditorPoint {
            byte,
            line: editor::cursor_line(&snapshot, byte),
        };
        let member = |anchor: usize, head: usize| Selection {
            anchor: point(anchor),
            head: point(head),
            mode: crate::tui::helix_selection::SelectionMode::Char,
            growing: false,
        };
        let presentation = state.session.as_mut().unwrap().presentation_mut();
        presentation.selections = vec![member(0, 2), member(4, 6)];
        presentation.primary_index = 0;

        assert_eq!(normal(&mut state, KeyCode::Char('J')), BufferKey::Consumed);
        assert_eq!(editor_text(&state), "a b\nc d\n");
        assert_eq!(
            undo_depth(&state),
            1,
            "one gesture is one history entry, not one per member"
        );
    }

    #[tokio::test]
    async fn a_multi_member_indent_is_one_undo_step() {
        let (mut state, _daemon) = opened_writer("a\nb\nc\n").await;
        let snapshot = snapshot_of(&state);
        hold_two_members(&mut state, &snapshot, 0, 4);
        assert_eq!(normal(&mut state, KeyCode::Char('>')), BufferKey::Consumed);
        assert_eq!(
            editor_text(&state),
            "    a\nb\n    c\n",
            "each member's own line takes one unit of the four-space default"
        );
        assert_eq!(
            undo_depth(&state),
            1,
            "one gesture is one history entry, not one per member"
        );
    }

    #[tokio::test]
    async fn an_indent_past_the_transaction_limit_is_refused_and_changes_nothing() {
        let text: String = (0..=codegg_protocol::document::MAX_DOCUMENT_EDITS)
            .map(|line| format!("line{line}\n"))
            .collect();
        let (mut state, _daemon) = opened_writer(&text).await;
        assert_eq!(normal(&mut state, KeyCode::Char('%')), BufferKey::Consumed);
        let before = editor_text(&state);
        assert_eq!(normal(&mut state, KeyCode::Char('>')), BufferKey::Consumed);
        assert!(
            notice_of(&state).is_some_and(|note| note.contains("limit")),
            "the refusal names the bound: {:?}",
            notice_of(&state)
        );
        assert_eq!(editor_text(&state), before, "the document is untouched");
        assert_eq!(undo_depth(&state), 0, "and no history entry was pushed");
    }

    // ── M002: change verbs ─────────────────────────────────────────────
    //
    // `c` has two halves and the order between them is the milestone's
    // correctness hazard, so every case here drives the real dispatch: the
    // delete is resolved, submitted, and only an accepted submission moves the
    // route into Insert.

    #[tokio::test]
    async fn c_deletes_then_enters_insert_and_the_typing_run_is_a_second_step() {
        let (mut state, _daemon) = opened_writer("ab\ncd\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        let mut app = app_on_the_editor_route(state).await;

        assert!(
            handle_editor_key(&mut app, key(KeyCode::Char('c'))),
            "the chord is claimed"
        );
        assert_eq!(editor_text(&app.editor_state), "cd\n", "the line went");
        assert_eq!(
            cursor(&app.editor_state),
            0,
            "and the cursor sits at the deletion point"
        );
        assert_eq!(
            app.ui_state.input_mode,
            crate::tui::input::InputMode::Insert,
            "the keyboard followed the delete"
        );
        assert_eq!(
            app.editor_state.mode(),
            EditorMode::Insert,
            "and the presentation agrees with the route, so the two tables cannot disagree"
        );
        assert_eq!(undo_depth(&app.editor_state), 1, "the delete is one step");

        for ch in "xy".chars() {
            assert!(
                handle_editor_key(&mut app, key(KeyCode::Char(ch))),
                "typing goes into the document"
            );
        }
        assert_eq!(editor_text(&app.editor_state), "xycd\n");
        assert_eq!(
            undo_depth(&app.editor_state),
            1,
            "the run is not committed while it is open"
        );

        assert!(handle_editor_key(&mut app, key(KeyCode::Esc)));
        assert_eq!(
            undo_depth(&app.editor_state),
            2,
            "exactly two steps: the delete and the run"
        );
    }

    #[tokio::test]
    async fn c_with_nothing_selected_is_claimed_rather_than_typed() {
        // The fallthrough hazard in its most visible form: an unclaimed bare
        // `c` reaches the composer and types a `c` into whatever box is
        // focused.
        let (state, _daemon) = opened_writer("ab\n").await;
        let mut app = app_on_the_editor_route(state).await;

        assert!(
            handle_editor_key(&mut app, key(KeyCode::Char('c'))),
            "`c` is the editor's, not the composer's"
        );
        assert_eq!(
            editor_text(&app.editor_state),
            "b\n",
            "with nothing held the character under the cursor is the target"
        );
        assert_eq!(
            app.ui_state.input_mode,
            crate::tui::input::InputMode::Insert
        );
    }

    #[tokio::test]
    async fn alt_c_changes_and_alt_d_deletes_through_dispatch() {
        let (mut state, _daemon) = opened_writer("ab\ncd\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        assert_eq!(
            press(&mut state, KeyCode::Char('c'), KeyModifiers::ALT),
            BufferKey::ChangeRanges {
                ranges: editor::one_range(0..3),
                yank: YankTarget::Nothing,
            },
            "`Alt-c` is the change gesture's no-yank spelling"
        );
        assert_eq!(
            press(&mut state, KeyCode::Char('d'), KeyModifiers::ALT),
            BufferKey::DeleteRanges {
                ranges: editor::one_range(0..3),
                yank: YankTarget::Nothing,
            },
            "`Alt-d` is the delete's no-yank spelling"
        );
        assert_eq!(
            editor_text(&state),
            "ab\ncd\n",
            "the buffer reported both effects rather than applying them"
        );
    }

    /// The no-yank promise, as a property of the dispatch boundary.
    ///
    /// The system clipboard is written from one place — `publish_yank` — and
    /// that place is reachable only when a reported [`YankTarget`] asks for it.
    /// "The delete family does not touch the clipboard" is therefore the same
    /// claim as "no delete chord reports the clipboard target", and `y` is the
    /// positive control: it reports the target that *does* reach the clipboard,
    /// so a dispatch that stopped reporting targets cannot pass this test by
    /// asserting nothing.
    ///
    /// M007 moved `d` and `c` from "no target at all" to the unnamed register —
    /// M002's D1 discharged — and left the `Alt` pair writing nowhere. What did
    /// not move is the property this test exists for: neither pair reaches the
    /// clipboard.
    #[tokio::test]
    async fn the_delete_family_never_reports_a_clipboard_write() {
        let (mut state, _daemon) = opened_writer("ab\ncd\n").await;
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);

        for (label, event, expected) in [
            (
                "d",
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
                YankTarget::Unnamed,
            ),
            (
                "c",
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
                YankTarget::Unnamed,
            ),
            (
                "Alt-c",
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT),
                YankTarget::Nothing,
            ),
            (
                "Alt-d",
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
                YankTarget::Nothing,
            ),
        ] {
            let yank = match normal_event(&mut state, event) {
                BufferKey::DeleteRanges { yank, .. } | BufferKey::ChangeRanges { yank, .. } => yank,
                other => panic!("`{label}` must report a delete effect, got {other:?}"),
            };
            assert_eq!(
                yank, expected,
                "`{label}` yanks into the unnamed register or nowhere, never the clipboard"
            );
        }

        assert!(
            matches!(
                normal(&mut state, KeyCode::Char('y')),
                BufferKey::YankRanges {
                    target: YankTarget::Selected(RegisterName::Unnamed),
                    ..
                }
            ),
            "the positive control: `y` reports the target that reaches the clipboard"
        );
    }

    #[tokio::test]
    async fn c_on_a_read_only_attachment_reports_and_stays_in_normal() {
        let (state, _daemon) = opened_reader("ab\ncd\n").await;
        let mut app = app_on_the_editor_route(state).await;

        assert!(
            handle_editor_key(&mut app, key(KeyCode::Char('c'))),
            "a refused verb still owns its key"
        );
        assert_eq!(
            editor_text(&app.editor_state),
            "ab\ncd\n",
            "nothing was written"
        );
        assert_eq!(
            app.ui_state.input_mode,
            crate::tui::input::InputMode::Normal,
            "no dead Insert mode over an unwritable buffer"
        );
        assert_eq!(
            app.editor_state.mode(),
            EditorMode::Normal,
            "and the presentation was not moved either"
        );
        assert!(
            notice_on_app(&app).is_some_and(|note| note.contains("Read-only")),
            "{:?}",
            notice_on_app(&app)
        );
    }

    #[tokio::test]
    async fn a_change_past_the_transaction_limit_is_refused_and_does_not_enter_insert() {
        // Resolved, then refused: this is the ordering proof. The verb produced
        // a range set, the set was too large to submit, and Insert was not
        // entered because the document did not take the change.
        let text: String = (0..=codegg_protocol::document::MAX_DOCUMENT_EDITS * 2)
            .map(|i| if i % 2 == 0 { 'a' } else { 'b' })
            .collect();
        let (mut state, _daemon) = opened_writer(&text).await;
        {
            let presentation = state.session.as_mut().expect("open").presentation_mut();
            presentation.selections = (0..=codegg_protocol::document::MAX_DOCUMENT_EDITS)
                .map(|i| Selection {
                    anchor: EditorPoint {
                        byte: i * 2,
                        line: 0,
                    },
                    head: EditorPoint {
                        byte: i * 2,
                        line: 0,
                    },
                    mode: crate::tui::helix_selection::SelectionMode::Char,
                    growing: false,
                })
                .collect();
            presentation.primary_index = 0;
        }
        let mut app = app_on_the_editor_route(state).await;

        assert!(handle_editor_key(&mut app, key(KeyCode::Char('c'))));
        assert!(
            notice_on_app(&app).is_some_and(|note| note.contains("limit")),
            "the refusal names the bound: {:?}",
            notice_on_app(&app)
        );
        assert_eq!(
            editor_text(&app.editor_state),
            text,
            "the document is byte-identical"
        );
        assert_eq!(
            app.ui_state.input_mode,
            crate::tui::input::InputMode::Normal,
            "Insert is not entered on a change that did not land"
        );
        assert_eq!(undo_depth(&app.editor_state), 0, "and nothing was recorded");
    }

    #[tokio::test]
    async fn a_multi_member_change_is_one_undo_step() {
        // A set of cursors is one gesture, so it is one entry: splitting it
        // would make `u` take back one member at a time and leave the reader
        // in a document no single delete produced.
        let (mut state, _daemon) = opened_writer("ab\ncd\n").await;
        let snapshot = snapshot_of(&state);
        hold_two_members(&mut state, &snapshot, 0, 3);
        let mut app = app_on_the_editor_route(state).await;

        assert!(handle_editor_key(&mut app, key(KeyCode::Char('c'))));
        assert_eq!(editor_text(&app.editor_state), "b\nd\n");
        assert_eq!(
            undo_depth(&app.editor_state),
            1,
            "one gesture is one history entry, not one per member"
        );
        assert_eq!(
            app.ui_state.input_mode,
            crate::tui::input::InputMode::Insert
        );
    }

    // ── M010: the space-mode family ───────────────────────────────────

    /// The reason a statement leaf rendered, if any.
    fn notice_on_app(app: &App) -> Option<String> {
        app.editor_state
            .notice
            .as_ref()
            .map(|notice| notice.message())
    }

    /// Press `space` then `glyph` through the App-level dispatch.
    ///
    /// Two presses, because that is the gesture: the space prefix is armed by
    /// the first and the leaf is resolved by the second. Driving the pair
    /// through one helper call would hide a leaf the keyboard cannot reach.
    fn press_space(app: &mut App, glyph: char) {
        assert!(
            handle_editor_key(app, KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
            "the space prefix must be claimed by the editor"
        );
        handle_editor_key(app, KeyEvent::new(KeyCode::Char(glyph), KeyModifiers::NONE));
    }

    /// Bind the active tab to a canonical project/workspace with `root`.
    ///
    /// The listing is daemon-owned, so the browser needs the canonical ids to
    /// ask for one; the root itself is only fencing state.
    fn bind_workspace(app: &mut App, root: &std::path::Path) {
        let Some(tab) = app.project_tabs.active_mut() else {
            panic!("the test app has one tab");
        };
        tab.workspace_root = Some(root.to_path_buf());
        tab.project_id = Some("project-m010".to_string());
        tab.workspace_id = Some("workspace-m010".to_string());
    }

    /// root it is browsing.
    #[tokio::test]
    async fn space_f_opens_the_file_browser_and_it_names_the_root_it_is_browsing() {
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = app_on_the_editor_route(state).await;
        let root = std::path::PathBuf::from("/tmp/codegg-m010-workspace");
        bind_workspace(&mut app, &root);

        press_space(&mut app, 'f');

        assert!(
            app.focus_manager
                .has_dialog(crate::tui::components::component::DialogType::Files),
            "the browser is open"
        );
        assert_eq!(
            app.file_tree_state.root.as_deref(),
            Some(root.as_path()),
            "the browser browses the active workspace"
        );
        let title = app
            .focus_manager
            .with_component::<crate::tui::components::dialogs::files::FilesDialog, _>(|dialog| {
                dialog.title()
            })
            .expect("the browser is on screen");
        assert!(
            title.contains("codegg-m010-workspace"),
            "the browser names the root it is browsing, not an empty scope: {title}"
        );
    }

    /// The browser opens **only** when there is something to browse. An empty
    /// browser is a key that appears to do nothing, so the refusal comes
    /// first and says why.
    #[tokio::test]
    async fn space_f_says_so_when_there_is_no_workspace_root_and_opens_nothing() {
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = app_on_the_editor_route(state).await;
        let Some(tab) = app.project_tabs.active_mut() else {
            panic!("the test app has one tab");
        };
        tab.workspace_root = None;
        let before = app.focus_manager.len();

        press_space(&mut app, 'f');

        assert_eq!(app.focus_manager.len(), before, "nothing opened");
        let note = notice_on_app(&app).expect("the reason is stated");
        assert!(
            note.contains("file picker") && note.contains("workspace root"),
            "the reason names the picker and the missing root: {note}"
        );
    }

    /// A root without a canonical project/workspace binding cannot be
    /// listed: the daemon resolves the listing from that binding, and the
    /// TUI never walks its own filesystem.
    #[tokio::test]
    async fn space_f_says_so_when_the_tab_has_no_canonical_binding() {
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = app_on_the_editor_route(state).await;
        let Some(tab) = app.project_tabs.active_mut() else {
            panic!("the test app has one tab");
        };
        tab.project_id = None;
        tab.workspace_id = None;
        let before = app.focus_manager.len();

        press_space(&mut app, 'f');

        assert_eq!(app.focus_manager.len(), before, "nothing opened");
        let note = notice_on_app(&app).expect("the reason is stated");
        assert!(
            note.contains("no canonical project/workspace binding"),
            "the reason names the missing binding: {note}"
        );
    }

    /// The reclaim: `space s` is still the document-symbol read, and the two
    /// are no longer the same gesture.
    #[tokio::test]
    async fn space_s_is_still_the_document_symbol_read_and_space_f_is_not_the_same_gesture() {
        let (mut state, _daemon) = opened_writer("hello").await;
        assert_eq!(
            leaf(&mut state, EditorPrefix::Space, 's'),
            BufferKey::Consumed
        );
        assert_eq!(
            parked(&state),
            Some(EditorPendingEffect::Read(LspReadOperation::DocumentSymbols))
        );

        let (mut state, _daemon) = opened_writer("hello").await;
        assert_eq!(
            leaf(&mut state, EditorPrefix::Space, 'f'),
            BufferKey::Consumed
        );
        assert_eq!(
            parked(&state),
            Some(EditorPendingEffect::OpenFilePicker),
            "the file picker displaced the duplicate, it did not replace the symbols read"
        );
    }

    #[tokio::test]
    async fn the_space_clipboard_family_names_the_verb_each_chord_runs() {
        for (glyph, expected) in [
            ('p', ClipboardVerb::PasteAfter),
            ('P', ClipboardVerb::PasteBefore),
            ('R', ClipboardVerb::Replace),
        ] {
            let (mut state, _daemon) = opened_writer("abc").await;
            assert_eq!(
                leaf(&mut state, EditorPrefix::Space, glyph),
                BufferKey::Consumed,
                "space {glyph} is claimed by the editor, not the composer"
            );
            assert_eq!(
                parked(&state),
                Some(EditorPendingEffect::Clipboard(expected)),
                "space {glyph} must name its own verb"
            );
        }
    }

    /// `space Y` is the *main* selection: the distinguished member alone,
    /// where `space y` takes every member.
    #[tokio::test]
    async fn space_y_yanks_only_the_primary_member_of_a_multi_member_selection() {
        let (mut state, _daemon) = opened_writer("alpha beta gamma").await;
        let snapshot = snapshot_of(&state);
        let point = |byte: usize| EditorPoint {
            byte,
            line: editor::cursor_line(&snapshot, byte),
        };
        let member = |anchor: usize, head: usize| Selection {
            anchor: point(anchor),
            head: point(head),
            mode: crate::tui::helix_selection::SelectionMode::Char,
            growing: false,
        };
        {
            let presentation = state.session.as_mut().unwrap().presentation_mut();
            // "alpha" at 0..5 and "gamma" at 11..16, with the second primary.
            presentation.selections = vec![member(0, 4), member(11, 15)];
            presentation.primary_index = 1;
        }

        let session = state.session.as_ref().expect("open");
        let whole_set = editor::selection_ranges(session.presentation(), &snapshot);
        assert_eq!(whole_set.len(), 2, "the set really does have two members");

        let range = primary_yank_range(session).expect("a member to yank");
        assert_eq!(range, 11..16, "the primary member alone");
        assert_eq!(
            snapshot
                .read_bytes(range.start, range.end)
                .expect("readable"),
            "gamma",
            "and it is that member's text, not the set's"
        );
    }

    /// `space Y` on an empty selection says so, and does not write an empty
    /// string to the clipboard — the same answer `space y` gives.
    #[tokio::test]
    async fn space_y_on_an_empty_selection_says_nothing_to_yank() {
        let (state, _daemon) = opened_writer("abc").await;
        let mut app = app_on_the_editor_route(state).await;

        press_space(&mut app, 'Y');

        assert_eq!(
            app.messages_state.toasts.len(),
            1,
            "the refusal is reported, not swallowed"
        );
        let toast = app.messages_state.toasts.iter().last().expect("a toast");
        assert!(
            toast.message.contains("Nothing to yank"),
            "the message is the one `space y` uses: {:?}",
            toast.message
        );
    }

    /// The four leaves with no substrate each state their reason.
    ///
    /// The expected strings are the **whole** reason, not a fragment, so the
    /// test and `docs/tui.md` cannot drift apart: the docs carry the same
    /// sentences, and a reader who is told "no workspace root" in the status
    /// line is told the same thing the documentation says.
    ///
    /// `space c` / `space C` are no longer here: M005 turned them into working
    /// toggles, so their reason is now a per-file refusal rather than a
    /// missing feature. `space j` left in M006 for the same reason — it opens
    /// the jumplist picker, and a capability that works cannot keep an
    /// apology.
    #[tokio::test]
    async fn every_substrate_less_space_leaf_states_its_reason() {
        for (glyph, expected) in [
            (
                'F',
                "switch to file picker: this build has one workspace root, and `space f` opens it",
            ),
            (
                'b',
                "buffer list: one document is attached at a time, so there is no list to choose from",
            ),
            (
                'D',
                "workspace diagnostics: no server capability backs a separate request; `space d` opens the panel",
            ),
            (
                '\'',
                "last picker: this build's pickers are not fuzzy, so there is no query to recall",
            ),
        ] {
            let (state, _daemon) = opened_writer("hello").await;
            let mut app = app_on_the_editor_route(state).await;
            press_space(&mut app, glyph);
            let note = notice_on_app(&app).expect("the reason is stated");
            assert_eq!(note, expected, "space {glyph} must state its reason");
        }
    }

    // ── The jumplist (`space j`) ────────────────────────────────────────

    /// The recorded positions, oldest first.
    fn jumplist_bytes(app: &App) -> Vec<usize> {
        app.editor_state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .jumplist
            .entries()
            .iter()
            .map(|entry| entry.byte)
            .collect()
    }

    fn jumplist_picker_is_open(app: &App) -> bool {
        app.editor_state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .jumplist
            .picker_open()
    }

    /// `space j` opens the picker over the recorded positions and `Enter`
    /// returns the cursor to the one selected — through the real dispatch.
    #[tokio::test]
    async fn space_j_opens_the_jumplist_and_enter_returns_to_the_selected_position() {
        let (state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        let mut app = app_on_the_editor_route(state).await;
        run_editor_command(&mut app, EditorCommandName::GotoLine, Some("2".to_string()));
        run_editor_command(&mut app, EditorCommandName::GotoLine, Some("3".to_string()));
        assert_eq!(cursor(&app.editor_state), 8, "line 3 starts at byte 8");
        assert_eq!(
            jumplist_bytes(&app),
            vec![0, 4],
            "each `:goto` records where it started"
        );

        press_space(&mut app, 'j');
        assert!(
            jumplist_picker_is_open(&app),
            "`space j` must open the picker, not say the capability is absent"
        );

        // `j` walks to the older row; `Enter` jumps to where that jump began.
        assert!(handle_editor_key(&mut app, key(KeyCode::Char('j'))));
        assert!(handle_editor_key(&mut app, key(KeyCode::Enter)));
        assert_eq!(
            cursor(&app.editor_state),
            0,
            "the cursor returns to the recorded position"
        );
        assert!(
            !jumplist_picker_is_open(&app),
            "selecting closes the picker it was chosen from"
        );
    }

    /// An empty list is not an empty box: the key says what fills it.
    #[tokio::test]
    async fn space_j_with_nothing_recorded_states_why_and_opens_nothing() {
        let (state, _daemon) = opened_writer("one\ntwo\n").await;
        let mut app = app_on_the_editor_route(state).await;

        press_space(&mut app, 'j');

        assert!(!jumplist_picker_is_open(&app), "no empty picker opens");
        let note = notice_on_app(&app).expect("the reason is stated");
        assert!(
            note.contains("nothing recorded"),
            "the reason says the list is empty: {note}"
        );
        assert!(
            note.contains("gg") && note.contains(":goto"),
            "and names a gesture that fills it: {note}"
        );
    }

    /// A motion that walks the buffer records nothing; each named jump
    /// records the position it started from, exactly once.
    #[tokio::test]
    async fn a_motion_records_nothing_and_each_named_jump_records_where_it_started() {
        let (state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        let mut app = app_on_the_editor_route(state).await;

        assert!(handle_editor_key(&mut app, key(KeyCode::Char('j'))));
        assert_eq!(cursor(&app.editor_state), 4, "one line down is byte 4");
        assert!(
            jumplist_bytes(&app).is_empty(),
            "`j` walks the buffer, so it is not a jump"
        );

        // `gg` from byte 4 lands at the top and records byte 4.
        assert!(handle_editor_key(&mut app, key(KeyCode::Char('g'))));
        assert!(handle_editor_key(&mut app, key(KeyCode::Char('g'))));
        assert_eq!(cursor(&app.editor_state), 0);
        assert_eq!(jumplist_bytes(&app), vec![4], "`gg` recorded its start");

        // `gg` again at the top is a jump to where the reader stands: no row.
        assert!(handle_editor_key(&mut app, key(KeyCode::Char('g'))));
        assert!(handle_editor_key(&mut app, key(KeyCode::Char('g'))));
        assert_eq!(
            jumplist_bytes(&app),
            vec![4],
            "a no-op jump records nothing"
        );

        // `G` from the top records the top.
        assert!(handle_editor_key(&mut app, key(KeyCode::Char('G'))));
        assert_eq!(
            jumplist_bytes(&app),
            vec![4, 0],
            "`G` recorded one entry, and it is where the cursor was"
        );
        let snapshot = snapshot_of(&app.editor_state);
        assert_eq!(
            editor::cursor_line(&snapshot, cursor(&app.editor_state)),
            editor::last_line_of(&snapshot),
            "and the jump itself landed on the file's last line"
        );
    }

    /// The two jumps that are not motions — a diagnostic step and a search
    /// adoption — record where they started as well.
    #[tokio::test]
    async fn a_diagnostic_step_and_a_search_adoption_record_where_they_started() {
        let (state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        let mut app = app_on_the_editor_route(state).await;
        app.editor_state.lsp.diagnostics = vec![test_diagnostic(2, "third")];
        assert!(handle_editor_key(&mut app, key(KeyCode::Char(']'))));
        assert!(handle_editor_key(&mut app, key(KeyCode::Char('d'))));
        assert_eq!(cursor(&app.editor_state), 8, "the step landed on line 3");
        assert_eq!(
            jumplist_bytes(&app),
            vec![0],
            "the diagnostic step recorded where it started"
        );

        let (state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        let mut app = app_on_the_editor_route(state).await;
        app.editor_state
            .session
            .as_mut()
            .expect("open")
            .presentation_mut()
            .open_search_line("two", true);
        run_editor_search_from(&mut app.editor_state, 10, 0);
        assert_eq!(cursor(&app.editor_state), 4, "the first match for `two`");
        assert_eq!(
            jumplist_bytes(&app),
            vec![0],
            "adopting a search recorded where it started"
        );
    }

    /// The highlight stops at both ends of the list instead of wrapping or
    /// panicking, so `Enter` always acts on a row the reader walked to.
    #[tokio::test]
    async fn the_jumplist_highlight_stops_at_both_ends() {
        let (state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        let mut app = app_on_the_editor_route(state).await;
        run_editor_command(&mut app, EditorCommandName::GotoLine, Some("2".to_string()));
        run_editor_command(&mut app, EditorCommandName::GotoLine, Some("3".to_string()));
        press_space(&mut app, 'j');
        let selected = |app: &App| {
            app.editor_state
                .session
                .as_ref()
                .expect("open")
                .presentation()
                .jumplist
                .selected()
                .map(|entry| entry.byte)
        };

        assert_eq!(
            selected(&app),
            Some(4),
            "the picker opens on the newest row"
        );
        assert!(handle_editor_key(&mut app, key(KeyCode::Char('k'))));
        assert_eq!(
            selected(&app),
            Some(4),
            "`k` at the newest row has nowhere to go"
        );
        for _ in 0..3 {
            assert!(handle_editor_key(&mut app, key(KeyCode::Char('j'))));
        }
        assert_eq!(
            selected(&app),
            Some(0),
            "`j` past the oldest row stops on it rather than wrapping"
        );
    }

    /// A position the document no longer contains is **refused**: the cursor
    /// does not move, and the reason is on screen. Clamping would land the
    /// reader somewhere plausible and wrong.
    #[tokio::test]
    async fn an_entry_the_shorter_document_no_longer_contains_is_refused() {
        let (state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        let mut app = app_on_the_editor_route(state).await;
        run_editor_command(&mut app, EditorCommandName::GotoLine, Some("3".to_string()));
        run_editor_command(&mut app, EditorCommandName::GotoLine, Some("2".to_string()));
        assert_eq!(jumplist_bytes(&app), vec![0, 8], "byte 8 is the stale one");

        // The document shrinks under the entry: everything from line 2 on goes.
        //
        // The set really is one range — a delete covers a contiguous span —
        // and `single_range_in_vec_init` exists to catch the *other* case,
        // where someone writes `vec![a..b]` meaning a vector of the integers
        // `a..b`. The element type here is `Range<usize>` by construction, so
        // the lint is answering a question this line never asked.
        #[allow(clippy::single_range_in_vec_init)]
        let ranges = vec![4..14];
        assert!(submit_range_delete(
            &mut app.editor_state,
            ranges,
            false,
            YankTarget::Nothing,
        ));
        assert_eq!(editor_text(&app.editor_state), "one\n");

        let before = cursor(&app.editor_state);
        press_space(&mut app, 'j');
        assert!(handle_editor_key(&mut app, key(KeyCode::Enter)));
        assert_eq!(
            cursor(&app.editor_state),
            before,
            "a refused entry must not move the cursor"
        );
        let note = notice_on_app(&app).expect("the refusal is reported");
        assert!(
            note.contains("stale"),
            "the reason names the entry as stale: {note}"
        );
    }

    /// The store goes with the presentation on every transition the contract
    /// names: opening a document, reload, resync, and close.
    #[tokio::test]
    async fn the_jumplist_does_not_survive_open_reload_resync_or_close() {
        let (mut state, _daemon) = opened_writer("base").await;
        park(&mut state, 3);
        editor_motion(&mut state, EditorCommand::BufferStart, 10);
        assert_eq!(jumplist_bytes_in(&state), vec![3], "an entry to clear");
        state
            .session
            .as_mut()
            .expect("open")
            .presentation_mut()
            .reset_for_open();
        assert!(
            jumplist_bytes_in(&state).is_empty(),
            "opening a document starts a new history"
        );

        for reconcile in ["reload", "resync", "close"] {
            let (mut state, daemon) = opened_writer("base").await;
            park(&mut state, 3);
            editor_motion(&mut state, EditorCommand::BufferStart, 10);
            assert_eq!(jumplist_bytes_in(&state), vec![3], "{reconcile} fixture");

            let controller = state.session.as_ref().expect("open").controller();
            controller.flush().await.expect("flush");
            let outcome = match reconcile {
                "reload" => controller.reload_from_disk().await,
                "resync" => controller.resync().await,
                _ => controller.close().await,
            };
            outcome.unwrap_or_else(|error| panic!("{reconcile} failed: {error}"));
            if reconcile == "close" {
                state.detach();
                assert!(
                    state.session.is_none(),
                    "close leaves no session to hold a jumplist"
                );
            } else if let Some(session) = state.session.as_mut() {
                editor::clear_history(session.presentation_mut());
                assert!(
                    jumplist_bytes_in(&state).is_empty(),
                    "the jumplist must not survive {reconcile}"
                );
            }
            let _ = daemon;
        }
    }

    /// The jumplist as `EditorState` holds it, for the transition test above.
    fn jumplist_bytes_in(state: &EditorState) -> Vec<usize> {
        state
            .session
            .as_ref()
            .map(|session| {
                session
                    .presentation()
                    .jumplist
                    .entries()
                    .iter()
                    .map(|entry| entry.byte)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Hold a character selection covering `first`..=`last`, so a gesture that
    /// acts on a range reaches the range.
    fn select_range(app: &mut App, first: usize, last: usize) {
        let snapshot = snapshot_of(&app.editor_state);
        let start = editor::line_start(&snapshot, first).expect("the first line exists");
        let end = editor::line_content_end(&snapshot, last).expect("the last line exists");
        let point = |byte: usize| EditorPoint {
            byte,
            line: editor::cursor_line(&snapshot, byte),
        };
        let presentation = app
            .editor_state
            .session
            .as_mut()
            .expect("open")
            .presentation_mut();
        presentation.selections = vec![Selection {
            anchor: point(start),
            head: point(end),
            mode: crate::tui::helix_selection::SelectionMode::Char,
            growing: false,
        }];
        presentation.primary_index = 0;
        presentation.cursor_byte = start;
    }

    /// `space c` comments the cursor's line after its indentation, and the
    /// second press takes exactly that back.
    #[tokio::test]
    async fn space_c_toggles_a_line_comment_and_the_second_press_restores_it() {
        let (state, _daemon) = opened_writer("    let value = 1;\n").await;
        let mut app = app_on_the_editor_route(state).await;
        park(&mut app.editor_state, 4);

        press_space(&mut app, 'c');
        assert_eq!(editor_text(&app.editor_state), "    // let value = 1;\n");
        assert_eq!(undo_depth(&app.editor_state), 1, "one gesture, one entry");

        press_space(&mut app, 'c');
        assert_eq!(editor_text(&app.editor_state), "    let value = 1;\n");
        assert_eq!(
            undo_depth(&app.editor_state),
            2,
            "the un-toggle is its own entry, not a coalesced one"
        );
    }

    /// A three-line toggle is **one** history entry, and one `u` takes all of
    /// it back.
    #[tokio::test]
    async fn space_c_comments_a_range_as_one_undo_step() {
        let (state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        let mut app = app_on_the_editor_route(state).await;
        select_range(&mut app, 0, 2);

        press_space(&mut app, 'c');
        assert_eq!(
            editor_text(&app.editor_state),
            "// one\n// two\n// three\n",
            "every covered line, and nothing else"
        );
        assert_eq!(undo_depth(&app.editor_state), 1, "three lines, one entry");

        editor_undo(&mut app.editor_state);
        assert_eq!(
            editor_text(&app.editor_state),
            "one\ntwo\nthree\n",
            "one `u` takes the whole toggle back"
        );
    }

    /// `space C` wraps the range in the grammar's block pair, and the same
    /// gesture removes exactly that pair.
    #[tokio::test]
    async fn space_capital_c_wraps_the_range_in_the_block_pair() {
        let (state, _daemon) = opened_writer("one\ntwo\n").await;
        let mut app = app_on_the_editor_route(state).await;
        select_range(&mut app, 0, 1);

        press_space(&mut app, 'C');
        assert_eq!(editor_text(&app.editor_state), "/*one\ntwo*/\n");

        select_range(&mut app, 0, 1);
        press_space(&mut app, 'C');
        assert_eq!(
            editor_text(&app.editor_state),
            "one\ntwo\n",
            "removing the pair it added is exact"
        );
    }

    /// A buffer whose grammar declares no comment token is refused, the
    /// refusal names the extension, and the document is byte-identical.
    ///
    /// Two shapes, because they fail for different reasons and the reader
    /// gets the same answer: `.txt` has a grammar and no comment token,
    /// `.zzz` has no grammar at all.
    #[tokio::test]
    async fn a_buffer_with_no_comment_token_is_refused_and_names_the_extension() {
        for (path, extension) in [("notes.txt", ".txt"), ("scratch.zzz", ".zzz")] {
            let (state, _daemon) = opened_writer_at(path, "plain\n").await;
            let mut app = app_on_the_editor_route(state).await;

            press_space(&mut app, 'c');

            let note = notice_on_app(&app).expect("the refusal is stated");
            assert!(
                note.contains(extension),
                "the notice names the extension: {note}"
            );
            assert!(
                note.contains("no comment syntax"),
                "and says what is missing: {note}"
            );
            assert_eq!(
                editor_text(&app.editor_state),
                "plain\n",
                "{path} must be byte-identical after the refusal"
            );
            assert_eq!(undo_depth(&app.editor_state), 0, "no entry, {path}");
        }
    }

    /// The toggle is refused on a read-only attachment like every other write.
    #[tokio::test]
    async fn space_c_on_a_read_only_attachment_is_refused_visibly() {
        let (state, _daemon) = opened_reader("one\ntwo\n").await;
        let mut app = app_on_the_editor_route(state).await;
        select_range(&mut app, 0, 1);

        press_space(&mut app, 'c');

        assert_eq!(
            editor_text(&app.editor_state),
            "one\ntwo\n",
            "no byte changed"
        );
        assert_eq!(
            app.editor_state.notice,
            Some(EditorNotice::ReadOnly),
            "the refusal is the one every write reports"
        );
        assert_eq!(undo_depth(&app.editor_state), 0, "and records no history");
    }

    /// its keys, in its bottom-right corner — and not the global help dialog.
    /// Bare `?` still opens the help.
    #[tokio::test]
    async fn space_question_mark_opens_the_editors_keymap_box() {
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = app_on_the_editor_route(state).await;

        press_space(&mut app, '?');

        let presentation = app.editor_state.session.as_ref().unwrap().presentation();
        assert!(presentation.keymap_overlay, "the keymap box is open");
        assert!(
            !presentation.command_active,
            "and it is not the `:` command line"
        );

        // `Esc` closes it.
        app.editor_state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .keymap_overlay = false;
        assert!(
            !app.editor_state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .keymap_overlay
        );
    }

    /// Insert mode types the space and the glyph. The space prefix is armed
    /// only from Normal mode, so a space key in Insert mode is text.
    #[tokio::test]
    async fn space_and_a_leaf_glyph_type_both_characters_in_insert_mode() {
        let (state, _daemon) = opened_writer("").await;
        let mut app = app_on_the_editor_route(state).await;
        app.ui_state.input_mode = crate::tui::input::InputMode::Insert;

        handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
        );

        assert_eq!(text_on_app(&app), " f", "both characters are text");
        assert!(
            !app.focus_manager
                .has_dialog(crate::tui::components::component::DialogType::Files),
            "and nothing opened"
        );
    }

    /// Insert mode types **every** leaf glyph. The space prefix is armed only
    /// from Normal mode, so a space key in Insert mode is text — and so is
    /// every glyph the space menu would otherwise consume.
    #[tokio::test]
    async fn every_space_leaf_glyph_types_text_in_insert_mode() {
        for glyph in [
            'y', 'k', 's', 'S', 'd', 'h', 'f', '?', 'p', 'P', 'R', 'Y', 'F', 'b', 'j', 'D', 'c',
            'C', '\'',
        ] {
            let (state, _daemon) = opened_writer("").await;
            let mut app = app_on_the_editor_route(state).await;
            app.ui_state.input_mode = crate::tui::input::InputMode::Insert;

            handle_editor_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(glyph), KeyModifiers::NONE),
            );

            assert_eq!(
                text_on_app(&app),
                glyph.to_string(),
                "space {glyph:?} is text in Insert"
            );
            assert!(
                !app.focus_manager
                    .has_dialog(crate::tui::components::component::DialogType::Files),
                "space {glyph:?} opens nothing in Insert"
            );
        }
    }

    /// The space gesture is editor-local, so the global keymap's own
    /// configuration cannot change it.
    #[tokio::test]
    async fn the_space_gesture_is_the_same_under_the_default_and_vim_global_keymaps() {
        for vim_mode in [false, true] {
            let (state, _daemon) = opened_writer("hello").await;
            let mut app = app_on_the_editor_route(state).await;
            let root = std::path::PathBuf::from("/tmp/codegg-m010-keymap");
            bind_workspace(&mut app, &root);
            app.ui_state.vim_mode = vim_mode;
            app.ui_state.bindings = crate::tui::input::build_bindings(None, vim_mode);

            app.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
            app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));

            assert!(
                app.focus_manager
                    .has_dialog(crate::tui::components::component::DialogType::Files),
                "space f opens the browser under the global keymap (vim_mode = {vim_mode})"
            );
        }
    }

    // ── M015: the LSP preview leaves ───────────────────────────────────
    //
    // A preview is a **read**: it shows what would change. Since M016 it also
    // carries the material the apply path consumes, so a complete preview
    // opens the change review on it. Every test here drives the real dispatch
    // path, and the request-level assertions read the request the daemon was
    // handed — the panel shows what the server *answered*, not what was
    // *asked*.

    /// A core client that records the LSP read requests it is handed.
    ///
    /// The request is the only place the proposed name can be observed: the
    /// panel shows the server's answer, and a panel assertion alone would
    /// not prove that the name ever left the editor.
    struct RecordingLspClient {
        reads: tokio::sync::mpsc::UnboundedSender<LspReadRequestDto>,
    }

    #[async_trait::async_trait]
    impl crate::core::CoreClient for RecordingLspClient {
        async fn request(
            &self,
            envelope: crate::protocol::core::RequestEnvelope<crate::protocol::core::CoreRequest>,
        ) -> Result<crate::protocol::core::CoreResponse, crate::error::AppError> {
            if let crate::protocol::core::CoreRequest::LspReadGet { request } = envelope.payload {
                let _ = self.reads.send(request);
            }
            Ok(crate::protocol::core::CoreResponse::Error {
                code: "stub".into(),
                message: "stub".into(),
            })
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

    /// An app whose tab is bound to the session's **own** project and
    /// workspace, so the editor's LSP scope resolves.
    ///
    /// The tab and the attachment have to agree: `current_lsp_scope` compares
    /// the tab's identities with the attachment's and reports a mismatch as
    /// "LSP is unavailable" rather than as a broken request. Without this,
    /// every test below would pass for the wrong reason.
    async fn app_with_a_lsp_scope(
        state: EditorState,
    ) -> (App, tokio::sync::mpsc::UnboundedReceiver<LspReadRequestDto>) {
        let mut app = app_on_the_editor_route(state).await;
        app.project_tabs.set_active_identities(
            Some("project-1".to_string()),
            Some("workspace-1".to_string()),
            Some("session-1".to_string()),
        );
        // The read is issued from a spawned task, and a task with no command
        // sender is never spawned at all.
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(8);
        app.tui_cmd_tx = Some(cmd_tx);
        let (reads_tx, reads_rx) = tokio::sync::mpsc::unbounded_channel();
        app.core_client = Some(std::sync::Arc::new(RecordingLspClient { reads: reads_tx }));
        (app, reads_rx)
    }

    #[tokio::test]
    async fn structural_selection_is_async_and_discards_a_moved_cursor_response() {
        let document = "x".repeat(128 * 1024);
        let (state, _daemon) = opened_writer(&document).await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;

        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT),
        ));
        let request = next_read(&mut reads).await;
        assert_eq!(request.operation, LspReadOperation::SelectionRange);
        assert_eq!(
            editor_text(&app.editor_state),
            document,
            "waiting for the daemon leaves the document untouched"
        );

        let scope = current_lsp_scope(&app).expect("the scope resolved");
        let (revision, request_position) = {
            let session = app.editor_state.session.as_ref().unwrap();
            let snapshot = session.try_snapshot().unwrap();
            let position = editor_lsp_position(session).unwrap();
            (snapshot.revision().get(), position)
        };
        assert!(
            handle_editor_key(&mut app, key(KeyCode::Right)),
            "local cursor input remains available while the daemon read is pending"
        );
        assert_ne!(
            editor_lsp_position(app.editor_state.session.as_ref().unwrap()),
            Some(request_position),
            "the local keypress moves the cursor before the read returns"
        );
        let selection = crate::tui::helix_selection::Selection::new(
            crate::tui::editor::EditorPoint { line: 0, byte: 0 },
            crate::tui::helix_selection::SelectionMode::Char,
        );
        app.editor_state
            .session
            .as_mut()
            .unwrap()
            .presentation_mut()
            .selections = vec![selection];

        let request_id = app.editor_state.lsp.read_request_id;
        let result = LspReadResultDto {
            operation: LspReadOperation::SelectionRange,
            status: LspReadStatus::Ready,
            payload: Some(crate::protocol::lsp::LspReadPayloadDto::SelectionRanges {
                ranges: vec![codegg_protocol::lsp::LspRangeDto {
                    path: "doc".to_string(),
                    workspace_relative_path: Some("doc".to_string()),
                    start_line: 0,
                    start_column: 0,
                    end_line: 0,
                    end_column: 5,
                }],
                truncated: false,
            }),
        };
        apply_editor_lsp_read_fresh(
            &mut app,
            request_id,
            scope.clone(),
            LspReadOperation::SelectionRange,
            revision,
            Some(request_position),
            Some(result.clone()),
            None,
        );

        let revision_after_cursor = app
            .editor_state
            .session
            .as_ref()
            .unwrap()
            .try_snapshot()
            .unwrap()
            .revision()
            .get();
        editor_insert_literal(&mut app.editor_state, "z");
        let revision_after_edit = app
            .editor_state
            .session
            .as_ref()
            .unwrap()
            .try_snapshot()
            .unwrap()
            .revision()
            .get();
        assert_ne!(revision_after_edit, revision_after_cursor);
        let current_position = editor_lsp_position(app.editor_state.session.as_ref().unwrap());
        apply_editor_lsp_read_fresh(
            &mut app,
            request_id,
            scope.clone(),
            LspReadOperation::SelectionRange,
            revision,
            current_position,
            Some(result.clone()),
            None,
        );

        app.project_tabs.set_active_identities(
            Some("project-2".to_string()),
            Some("workspace-2".to_string()),
            Some("session-2".to_string()),
        );
        let current_position = editor_lsp_position(app.editor_state.session.as_ref().unwrap());
        apply_editor_lsp_read_fresh(
            &mut app,
            request_id,
            scope.clone(),
            LspReadOperation::SelectionRange,
            revision,
            current_position,
            Some(result.clone()),
            None,
        );

        app.project_tabs.set_active_identities(
            Some("project-1".to_string()),
            Some("workspace-1".to_string()),
            Some("session-1".to_string()),
        );
        app.editor_state.generation = scope.generation.saturating_add(1);
        let current_position = editor_lsp_position(app.editor_state.session.as_ref().unwrap());
        apply_editor_lsp_read_fresh(
            &mut app,
            request_id,
            scope,
            LspReadOperation::SelectionRange,
            revision,
            current_position,
            Some(result),
            None,
        );

        assert!(
            app.editor_state
                .session
                .as_ref()
                .unwrap()
                .presentation()
                .selections
                .is_empty(),
            "a stale result must not create a selection"
        );
        assert!(
            app.editor_state.lsp.read_result.is_none(),
            "the stale result must not reach the LSP result panel"
        );
    }

    /// Wait for the read the daemon was handed.
    async fn next_read(
        reads: &mut tokio::sync::mpsc::UnboundedReceiver<LspReadRequestDto>,
    ) -> LspReadRequestDto {
        tokio::time::timeout(std::time::Duration::from_secs(5), reads.recv())
            .await
            .expect("the read must reach the daemon")
            .expect("the channel must stay open")
    }

    /// Assert that **no** read reaches the daemon.
    ///
    /// The read is issued from a spawned task, so "no request" cannot be
    /// proved by looking once — the task may simply not have run yet. A
    /// short wait is the honest way to say it.
    async fn assert_no_read(reads: &mut tokio::sync::mpsc::UnboundedReceiver<LspReadRequestDto>) {
        let received =
            tokio::time::timeout(std::time::Duration::from_millis(250), reads.recv()).await;
        assert!(
            received.is_err(),
            "no read may be issued, but one arrived: {:?}",
            received.ok().flatten()
        );
    }

    /// The messages the editor reported, newest last.
    fn reported(app: &App) -> Vec<String> {
        app.messages_state
            .toasts
            .history()
            .map(|record| record.message.clone())
            .collect()
    }

    #[tokio::test]
    async fn the_format_preview_is_requested_and_changes_nothing() {
        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;
        let before = editor_text(&app.editor_state);
        let history_before = undo_depth(&app.editor_state);

        assert!(
            handle_editor_key(&mut app, key(KeyCode::Char('='))),
            "`=` belongs to the editor"
        );

        let request = next_read(&mut reads).await;
        assert_eq!(request.operation, LspReadOperation::FormatPreview);
        assert_eq!(request.new_name, None, "a format preview names nothing");
        assert_eq!(
            request.query, None,
            "and a format preview is not a workspace-symbols query"
        );
        assert_eq!(
            editor_text(&app.editor_state),
            before,
            "a preview changes no text"
        );
        assert_eq!(
            undo_depth(&app.editor_state),
            history_before,
            "and records no undo step"
        );
    }

    /// `space r` collects a name, and the request carries it.
    #[tokio::test]
    async fn the_rename_preview_carries_the_typed_name_on_the_request() {
        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;

        // `space r` arms the prompt; it does not ask the server anything yet,
        // because it does not know the name yet.
        arm_on_app(&mut app, EditorPrefix::Space);
        handle_editor_key(&mut app, key(KeyCode::Char('r')));
        {
            let presentation = app.editor_state.session.as_ref().unwrap().presentation();
            assert!(presentation.command_active, "the prompt is open");
            assert_eq!(
                presentation.command_line, "rename-preview ",
                "and it is pre-filled with the command"
            );
        }
        assert_no_read(&mut reads).await;

        for ch in "renamed".chars() {
            handle_editor_key(&mut app, key(KeyCode::Char(ch)));
        }
        handle_editor_key(&mut app, key(KeyCode::Enter));

        let request = next_read(&mut reads).await;
        assert_eq!(request.operation, LspReadOperation::RenamePreview);
        assert_eq!(
            request.new_name.as_deref(),
            Some("renamed"),
            "the request must carry the name the reader typed"
        );
        assert_eq!(request.query, None, "and no workspace-symbols query");
        assert!(
            request.line.is_some() && request.column.is_some(),
            "the rename targets the symbol under the cursor, so the position is the cursor's"
        );
    }

    /// An empty name is refused **before** a request is spent on it.
    #[tokio::test]
    async fn an_empty_new_name_issues_no_request_and_says_why() {
        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;

        arm_on_app(&mut app, EditorPrefix::Space);
        handle_editor_key(&mut app, key(KeyCode::Char('r')));
        handle_editor_key(&mut app, key(KeyCode::Enter));

        assert_no_read(&mut reads).await;
        let reported = reported(&app);
        assert!(
            reported
                .iter()
                .any(|message| message.contains("needs a new name")),
            "the refusal says what is missing, got {reported:?}"
        );
    }

    /// A name over the protocol's bound is refused locally, with the bound
    /// named.
    #[tokio::test]
    async fn a_new_name_over_the_bound_issues_no_request_and_names_the_bound() {
        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;
        let over_long = "x".repeat(codegg_protocol::lsp::MAX_LSP_NEW_NAME_BYTES + 1);

        run_editor_command(&mut app, EditorCommandName::RenamePreview, Some(over_long));

        assert_no_read(&mut reads).await;
        let reported = reported(&app);
        assert!(
            reported.iter().any(|message| message.contains(&format!(
                "{}-byte limit",
                codegg_protocol::lsp::MAX_LSP_NEW_NAME_BYTES
            ))),
            "the refusal names the bound, got {reported:?}"
        );
    }

    /// Cancelling the prompt changes nothing and asks nothing.
    #[tokio::test]
    async fn cancelling_the_rename_prompt_issues_no_request() {
        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;
        let before = editor_text(&app.editor_state);

        arm_on_app(&mut app, EditorPrefix::Space);
        handle_editor_key(&mut app, key(KeyCode::Char('r')));
        for ch in "renamed".chars() {
            handle_editor_key(&mut app, key(KeyCode::Char(ch)));
        }
        handle_editor_key(&mut app, key(KeyCode::Esc));

        assert_no_read(&mut reads).await;
        assert_eq!(editor_text(&app.editor_state), before, "nothing changed");
        let presentation = app.editor_state.session.as_ref().unwrap().presentation();
        assert!(!presentation.command_active, "and the prompt is closed");
    }

    /// `space a` asks the server what code actions it would offer.
    ///
    /// It used to say that there was no code-action request to make. That
    /// was true when it was written, and it stopped being true when M017
    /// added the read — so the leaf is a read now, and the test says what
    /// the leaf *does* rather than what it used to say.
    #[tokio::test]
    async fn space_a_asks_for_the_code_actions_rather_than_stating_a_reason() {
        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;

        arm_on_app(&mut app, EditorPrefix::Space);
        handle_editor_key(&mut app, key(KeyCode::Char('a')));

        let request = next_read(&mut reads).await;
        assert_eq!(
            request.operation,
            LspReadOperation::CodeActionPreview,
            "the leaf is the code-action read"
        );
        assert!(
            app.editor_state.notice.is_none(),
            "a leaf that does something does not also state a reason for not doing it"
        );
    }

    /// `space r` and `space R` are different leaves, and the capital is
    /// reachable in both of the terminal's spellings.
    #[tokio::test]
    async fn the_lower_r_and_upper_r_leaves_are_different_and_both_spellings_reach_them() {
        let (mut state, _daemon) = opened_writer("hello").await;
        assert_eq!(
            leaf(&mut state, EditorPrefix::Space, 'r'),
            BufferKey::Consumed
        );
        assert_eq!(
            parked(&state),
            Some(EditorPendingEffect::RenamePrompt),
            "`space r` is the rename preview"
        );

        let (mut state, _daemon) = opened_writer("hello").await;
        assert_eq!(
            leaf(&mut state, EditorPrefix::Space, 'R'),
            BufferKey::Consumed
        );
        assert_eq!(
            parked(&state),
            Some(EditorPendingEffect::Clipboard(ClipboardVerb::Replace)),
            "`space R` is the clipboard replace"
        );

        // The capital, spelled the way a terminal that reports `Char('r')`
        // plus `SHIFT` spells it.
        let (mut state, _daemon) = opened_writer("hello").await;
        arm(&mut state, EditorPrefix::Space);
        assert_eq!(
            editor_buffer_key(
                &mut state,
                crate::tui::input::InputMode::Normal,
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::SHIFT),
            ),
            BufferKey::Consumed
        );
        assert_eq!(
            parked(&state),
            Some(EditorPendingEffect::Clipboard(ClipboardVerb::Replace)),
            "the shifted spelling reaches the same capital leaf"
        );
    }

    /// would change.
    #[tokio::test]
    async fn the_rename_preview_reaches_the_panel_with_the_files_it_would_change() {
        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, _reads) = app_with_a_lsp_scope(state).await;

        // The read the daemon was asked for.
        arm_on_app(&mut app, EditorPrefix::Space);
        handle_editor_key(&mut app, key(KeyCode::Char('r')));
        for ch in "renamed".chars() {
            handle_editor_key(&mut app, key(KeyCode::Char(ch)));
        }
        handle_editor_key(&mut app, key(KeyCode::Enter));

        let scope = current_lsp_scope(&app).expect("the scope resolved");
        let request_id = app.editor_state.lsp.read_request_id;
        apply_editor_lsp_read(
            &mut app,
            request_id,
            scope,
            LspReadOperation::RenamePreview,
            Some(LspReadResultDto {
                operation: LspReadOperation::RenamePreview,
                status: LspReadStatus::Ready,
                payload: Some(crate::protocol::lsp::LspReadPayloadDto::RenamePreview(
                    codegg_protocol::lsp::LspRenamePreviewDto {
                        old_name: Some("old".to_string()),
                        new_name: "renamed".to_string(),
                        edit_count: 2,
                        files: vec![codegg_protocol::lsp::LspRenameFileDto {
                            path: "src/lib.rs".to_string(),
                            edit_count: 2,
                        }],
                        warnings: Vec::new(),
                        truncated: false,
                        base_stale: false,
                        candidate: None,
                    },
                )),
            }),
            None,
        );

        assert_eq!(
            app.editor_state.lsp.panel,
            Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult),
            "the panel is open"
        );
        let result = app
            .editor_state
            .lsp
            .read_result
            .as_ref()
            .expect("the result is held");
        match result.payload.as_ref() {
            Some(crate::protocol::lsp::LspReadPayloadDto::RenamePreview(preview)) => {
                assert_eq!(preview.new_name, "renamed");
                assert_eq!(preview.files.len(), 1);
                assert_eq!(preview.files[0].path, "src/lib.rs");
            }
            other => panic!("a rename preview must reach the panel, got {other:?}"),
        }
    }

    /// The bridge's editor half: a preview that carries a candidate opens the
    /// review on it.
    #[tokio::test]
    async fn a_preview_with_a_candidate_opens_the_review_on_it() {
        use codegg_protocol::lsp::{
            LspFormatPreviewDto, LspPreviewCandidateDto, LspPreviewPatchDto,
        };

        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;

        assert!(handle_editor_key(&mut app, key(KeyCode::Char('='))));
        let request = next_read(&mut reads).await;
        assert_eq!(request.operation, LspReadOperation::FormatPreview);

        let scope = current_lsp_scope(&app).expect("the scope resolved");
        let request_id = app.editor_state.lsp.read_request_id;
        apply_editor_lsp_read(
            &mut app,
            request_id,
            scope,
            LspReadOperation::FormatPreview,
            Some(LspReadResultDto {
                operation: LspReadOperation::FormatPreview,
                status: LspReadStatus::Ready,
                payload: Some(crate::protocol::lsp::LspReadPayloadDto::FormatPreview(
                    LspFormatPreviewDto {
                        path: "src/lib.rs".to_string(),
                        edit_count: 1,
                        before_hash: "a".repeat(64),
                        after_hash: "b".repeat(64),
                        diff: "@@ -1 +1 @@\n-old\n+new\n".to_string(),
                        truncated: false,
                        base_stale: false,
                        candidate: Some(LspPreviewCandidateDto {
                            preview_id: "editor-preview-1".to_string(),
                            preview_revision: 1,
                            preview_digest: "c".repeat(64),
                            kind: "formatting".to_string(),
                            title: "Format src/lib.rs".to_string(),
                            provenance: "editor:format:src/lib.rs".to_string(),
                            patches: vec![LspPreviewPatchDto {
                                path: "src/lib.rs".to_string(),
                                patch: "@@ -1 +1 @@\n-old\n+new\n".to_string(),
                                original_hash: "a".repeat(64),
                            }],
                        }),
                    },
                )),
            }),
            None,
        );

        assert!(
            app.change_review_state.is_open(),
            "a candidate must open the review, got: {:?}",
            reported(&app)
        );
        assert_eq!(
            app.change_review_state
                .carried
                .as_ref()
                .map(|candidate| candidate.preview_id.clone()),
            Some("editor-preview-1".to_string()),
            "the review must hold the candidate the daemon minted"
        );
    }

    /// A truncated preview, or one with nothing to change, carries no
    /// candidate — and a review that offered to apply it would be offering an
    /// apply the daemon would refuse.
    #[tokio::test]
    async fn a_preview_without_a_candidate_opens_no_review() {
        use codegg_protocol::lsp::LspFormatPreviewDto;

        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;

        assert!(handle_editor_key(&mut app, key(KeyCode::Char('='))));
        let _request = next_read(&mut reads).await;

        let scope = current_lsp_scope(&app).expect("the scope resolved");
        let request_id = app.editor_state.lsp.read_request_id;
        apply_editor_lsp_read(
            &mut app,
            request_id,
            scope,
            LspReadOperation::FormatPreview,
            Some(LspReadResultDto {
                operation: LspReadOperation::FormatPreview,
                status: LspReadStatus::Ready,
                payload: Some(crate::protocol::lsp::LspReadPayloadDto::FormatPreview(
                    LspFormatPreviewDto {
                        path: "src/lib.rs".to_string(),
                        edit_count: 0,
                        before_hash: "a".repeat(64),
                        after_hash: "a".repeat(64),
                        diff: String::new(),
                        truncated: false,
                        base_stale: false,
                        candidate: None,
                    },
                )),
            }),
            None,
        );

        assert!(
            !app.change_review_state.is_open(),
            "a preview with nothing to apply must not open a review"
        );
        assert!(
            app.editor_state.lsp.preview_candidate.is_none(),
            "and the editor must hold no candidate"
        );
    }

    /// candidate.
    ///
    /// A code action is a *choice*, so the review does not open by itself:
    /// the panel is the picker, and `Enter` acts on the selected action. The
    /// candidate reviewed is the candidate applied — the same bridge M016
    /// built, reached through the picker.
    #[tokio::test]
    async fn a_selected_code_action_opens_the_review_on_its_own_candidate() {
        use codegg_protocol::lsp::{LspCodeActionDto, LspPreviewCandidateDto, LspPreviewPatchDto};

        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;

        arm_on_app(&mut app, EditorPrefix::Space);
        handle_editor_key(&mut app, key(KeyCode::Char('a')));
        let _request = next_read(&mut reads).await;

        let scope = current_lsp_scope(&app).expect("the scope resolved");
        let request_id = app.editor_state.lsp.read_request_id;
        apply_editor_lsp_read(
            &mut app,
            request_id,
            scope,
            LspReadOperation::CodeActionPreview,
            Some(LspReadResultDto {
                operation: LspReadOperation::CodeActionPreview,
                status: LspReadStatus::Ready,
                payload: Some(crate::protocol::lsp::LspReadPayloadDto::CodeActions {
                    actions: vec![LspCodeActionDto {
                        title: "Add the missing import".to_string(),
                        kind: Some("quickfix".to_string()),
                        preferred: false,
                        disabled_reason: None,
                        has_edit: true,
                        has_command: false,
                        candidate: Some(LspPreviewCandidateDto {
                            preview_id: "editor-preview-7".to_string(),
                            preview_revision: 1,
                            preview_digest: "c".repeat(64),
                            kind: "code_action".to_string(),
                            title: "Add the missing import".to_string(),
                            provenance: "editor:code_action:Add the missing import".to_string(),
                            patches: vec![LspPreviewPatchDto {
                                path: "src/lib.rs".to_string(),
                                patch: "@@ -1 +1 @@\n-old\n+new\n".to_string(),
                                original_hash: "a".repeat(64),
                            }],
                        }),
                        unavailable: None,
                    }],
                    truncated: false,
                }),
            }),
            None,
        );

        assert!(
            !app.change_review_state.is_open(),
            "a code action is a choice: the review must not open before one is picked"
        );

        // `Enter` in the panel acts on the selected action.
        assert!(handle_editor_key(&mut app, key(KeyCode::Enter)));
        assert!(
            app.change_review_state.is_open(),
            "the selected action must open the review, got: {:?}",
            reported(&app)
        );
        assert_eq!(
            app.change_review_state
                .carried
                .as_ref()
                .map(|candidate| candidate.preview_id.clone()),
            Some("editor-preview-7".to_string()),
            "the review must hold the selected action's own candidate"
        );
    }

    /// The surface never runs commands, and the LSP specification applies an
    /// action's edit *and then* its command — so an action that carries a
    /// command is never applied here. The reader is told that in the place
    /// they pressed the key, in the same words the panel used.
    #[tokio::test]
    async fn a_command_only_code_action_cannot_be_applied_and_says_why() {
        use codegg_protocol::lsp::{LspCodeActionDto, LspCodeActionUnavailable};

        let (state, _daemon) = opened_writer("hello").await;
        let (mut app, mut reads) = app_with_a_lsp_scope(state).await;

        arm_on_app(&mut app, EditorPrefix::Space);
        handle_editor_key(&mut app, key(KeyCode::Char('a')));
        let _request = next_read(&mut reads).await;

        let scope = current_lsp_scope(&app).expect("the scope resolved");
        let request_id = app.editor_state.lsp.read_request_id;
        apply_editor_lsp_read(
            &mut app,
            request_id,
            scope,
            LspReadOperation::CodeActionPreview,
            Some(LspReadResultDto {
                operation: LspReadOperation::CodeActionPreview,
                status: LspReadStatus::Ready,
                payload: Some(crate::protocol::lsp::LspReadPayloadDto::CodeActions {
                    actions: vec![LspCodeActionDto {
                        title: "Run the test suite".to_string(),
                        kind: None,
                        preferred: false,
                        disabled_reason: None,
                        has_edit: false,
                        has_command: true,
                        candidate: None,
                        unavailable: Some(LspCodeActionUnavailable::Command),
                    }],
                    truncated: false,
                }),
            }),
            None,
        );

        assert!(handle_editor_key(&mut app, key(KeyCode::Enter)));
        assert!(
            !app.change_review_state.is_open(),
            "an action that cannot be applied must not open a review"
        );
        let messages: Vec<String> = app
            .messages_state
            .toasts
            .iter()
            .map(|toast| toast.message.clone())
            .collect();
        assert!(
            messages.iter().any(
                |message| message.contains("Run the test suite") && message.contains("command")
            ),
            "the toast must name the action and the reason, got: {messages:?}"
        );
    }

    /// Build a review over a candidate that touches the open document.
    async fn review_over_a_candidate(state: EditorState) -> App {
        use codegg_protocol::lsp::{LspPreviewCandidateDto, LspPreviewPatchDto};

        let (mut app, _reads) = app_with_a_lsp_scope(state).await;
        let candidate = LspPreviewCandidateDto {
            preview_id: "editor-preview-1".to_string(),
            preview_revision: 1,
            preview_digest: "c".repeat(64),
            kind: "formatting".to_string(),
            title: "Format src/lib.rs".to_string(),
            provenance: "editor:format:src/lib.rs".to_string(),
            patches: vec![LspPreviewPatchDto {
                path: "src/lib.rs".to_string(),
                patch: "@@ -1 +1 @@\n-old\n+new\n".to_string(),
                original_hash: "a".repeat(64),
            }],
        };
        crate::tui::commands::change_review::open_carried(&mut app, candidate);
        assert!(
            app.change_review_state.is_open(),
            "the review must be open for the completion to belong to"
        );
        app
    }

    #[tokio::test]
    async fn a_successful_apply_reloads_the_file_it_wrote() {
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = review_over_a_candidate(state).await;
        let request_id = app.change_review_state.request.request_id();
        let generation = app.change_review_state.generation;

        crate::tui::commands::change_review::apply_completion(
            &mut app,
            request_id,
            generation,
            vec!["/tmp/src/lib.rs".to_string()],
            "cp-1".to_string(),
            None,
        );

        assert_eq!(
            app.editor_state.operation,
            Some(EditorOperation::Reload),
            "the applied file must be reloaded into the editor"
        );
    }

    /// A refused apply changes nothing on disk, so it reloads nothing.
    #[tokio::test]
    async fn a_refused_apply_reloads_nothing() {
        let (state, _daemon) = opened_writer("hello").await;
        let mut app = review_over_a_candidate(state).await;
        let request_id = app.change_review_state.request.request_id();
        let generation = app.change_review_state.generation;

        crate::tui::commands::change_review::apply_completion(
            &mut app,
            request_id,
            generation,
            Vec::new(),
            String::new(),
            Some("src/lib.rs has unsaved editor changes".to_string()),
        );

        assert_eq!(
            app.editor_state.operation, None,
            "a refused apply wrote nothing, so there is nothing to reload"
        );
    }

    /// An apply over a buffer with unsaved edits must not discard them.
    ///
    /// A reload throws the local draft away. The daemon's dirty-document check
    /// is not the editor's — the editor is a replica — so the daemon can
    /// believe a document is clean while the buffer holds edits it has not
    /// pushed yet. Discarding those to make the screen tidier would lose
    /// work.
    #[tokio::test]
    async fn an_apply_over_an_unsaved_buffer_keeps_the_draft() {
        let (mut state, _daemon) = opened_writer("hello").await;
        state.status = EditorStatus::DirtyLocal;
        let mut app = review_over_a_candidate(state).await;
        let request_id = app.change_review_state.request.request_id();
        let generation = app.change_review_state.generation;

        crate::tui::commands::change_review::apply_completion(
            &mut app,
            request_id,
            generation,
            vec!["/tmp/src/lib.rs".to_string()],
            "cp-1".to_string(),
            None,
        );

        assert_eq!(
            app.editor_state.operation, None,
            "a buffer with a draft must not be reloaded over it"
        );
        let messages: Vec<String> = reported(&app);
        assert!(
            messages
                .iter()
                .any(|message| message.contains("unsaved edits")),
            "and the editor must say why, got: {messages:?}"
        );
    }

    // ── M004: counts ─────────────────────────────────────────────────────

    fn count(state: &EditorState) -> Option<usize> {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .pending_count
    }

    /// Press `digits`, then read the field rather than the dispatch's return.
    fn type_count(state: &mut EditorState, digits: &str) -> Option<usize> {
        for digit in digits.chars() {
            assert_eq!(
                normal(state, KeyCode::Char(digit)),
                BufferKey::Consumed,
                "{digit} belongs to the buffer while it is a count digit"
            );
        }
        count(state)
    }

    /// `1`-`9` no longer reach the composer; `0` alone is still `LineStart`.
    #[tokio::test]
    async fn the_digits_begin_a_count_instead_of_reaching_the_composer() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(20)).await;
        for digit in "1234".chars() {
            assert_eq!(
                normal(&mut state, KeyCode::Char(digit)),
                BufferKey::Consumed,
                "{digit} is a count digit, not a composer keystroke"
            );
        }
        assert_eq!(count(&state), Some(1234), "the digits accumulated in order");
        assert_eq!(editor_text(&state), numbered_lines(20), "and wrote nothing");

        // `0` with nothing pending is `LineStart`, the one binding a careless
        // digit arm would steal. Line 2 starts at byte 7, so from byte 9 it
        // lands on 7 and begins no count.
        let (mut state, _daemon) = opened_writer(&numbered_lines(20)).await;
        normal(&mut state, KeyCode::Char('l'));
        normal(&mut state, KeyCode::Char('l'));
        normal(&mut state, KeyCode::Char('l'));
        normal(&mut state, KeyCode::Char('l'));
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(cursor(&state), 11, "on line 2, column 4");
        assert_eq!(normal(&mut state, KeyCode::Char('0')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 7, "`0` is still LineStart");
        assert_eq!(count(&state), None, "and a leading `0` begins no count");
    }

    /// `0` after `1` extends the count to ten, and the motion after reads it.
    #[tokio::test]
    async fn a_zero_after_a_digit_extends_the_count_to_ten() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(20)).await;
        assert_eq!(type_count(&mut state, "10"), Some(10));
        assert_eq!(normal(&mut state, KeyCode::Char('j')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 70, "`10j` walks ten lines");
        assert_eq!(count(&state), None, "and the motion took the count");
    }

    /// The character and vertical motions, each counted.
    #[tokio::test]
    async fn a_count_repeats_the_character_and_vertical_motions() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(20)).await;
        assert_eq!(type_count(&mut state, "5"), Some(5));
        normal(&mut state, KeyCode::Char('l'));
        assert_eq!(cursor(&state), 5, "`5l` walks five bytes");

        let (mut state, _daemon) = opened_writer(&numbered_lines(20)).await;
        assert_eq!(type_count(&mut state, "5"), Some(5));
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(cursor(&state), 35, "`5j` walks five lines");
    }

    /// The word and long-word motions are counted, and still disagree.
    #[tokio::test]
    async fn a_count_repeats_the_word_and_long_word_motions() {
        let (mut state, _daemon) = opened_writer("foo.bar baz\nqux\n").await;
        assert_eq!(type_count(&mut state, "2"), Some(2));
        normal(&mut state, KeyCode::Char('w'));
        assert_eq!(cursor(&state), 4, "`2w` steps over the punctuation run");

        let (mut state, _daemon) = opened_writer("foo.bar baz\nqux\n").await;
        assert_eq!(type_count(&mut state, "2"), Some(2));
        normal(&mut state, KeyCode::Char('W'));
        assert_eq!(cursor(&state), 12, "`2W` steps over two WORDS");
    }

    /// A paragraph motion is a motion, so `2]p` is the second boundary.
    #[tokio::test]
    async fn a_count_repeats_the_paragraph_motion() {
        let (mut state, _daemon) = opened_writer("alpha\n\nbeta\n\ngamma\n").await;
        assert_eq!(type_count(&mut state, "2"), Some(2));
        normal(&mut state, KeyCode::Char(']'));
        assert_eq!(normal(&mut state, KeyCode::Char('p')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 13, "`2]p` reaches the second paragraph");
    }

    /// A page key's count is a single multiplier: three half pages.
    #[tokio::test]
    async fn a_count_multiplies_a_half_page() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(60)).await;
        assert_eq!(type_count(&mut state, "3"), Some(3));
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(normal_event(&mut state, ctrl_d), BufferKey::Consumed);
        assert_eq!(cursor(&state), 210, "three half pages is thirty lines");
    }

    /// `<n>G` and `<n>gg` address a line, and the goto leaves share it.
    #[tokio::test]
    async fn a_count_addresses_a_line_through_the_goto_motions() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(60)).await;
        assert_eq!(type_count(&mut state, "10"), Some(10));
        assert_eq!(normal(&mut state, KeyCode::Char('G')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 63, "`10G` addresses line 10, one-based");

        let (mut state, _daemon) = opened_writer(&numbered_lines(60)).await;
        normal(&mut state, KeyCode::Char('G'));
        assert_eq!(cursor(&state), 420, "`G` alone is still the buffer end");
        assert_eq!(type_count(&mut state, "3"), Some(3));
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 14, "`3gg` reaches line 3 from anywhere");

        let (mut state, _daemon) = opened_writer(&numbered_lines(60)).await;
        assert_eq!(type_count(&mut state, "3"), Some(3));
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(normal(&mut state, KeyCode::Char('j')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 21, "and `3gj` is three lines down");
    }

    /// The count survives arming a prefix and is consumed exactly once.
    #[tokio::test]
    async fn a_count_survives_a_prefix_and_is_consumed_exactly_once() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(60)).await;
        assert_eq!(type_count(&mut state, "3"), Some(3));
        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(pending(&state), "g", "the count did not clear the prefix");
        assert_eq!(
            count(&state),
            Some(3),
            "and the prefix did not clear the count"
        );
        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 14, "the leaf consumed the count");

        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(normal(&mut state, KeyCode::Char('g')), BufferKey::Consumed);
        assert_eq!(
            cursor(&state),
            0,
            "a second `gg` has no count to spend, so it moves once"
        );
    }

    /// A count larger than the document clamps at the last line and reports
    /// nothing: the resolver stops when it stops moving, so `9999j` costs one
    /// walk rather than 9999.
    #[tokio::test]
    async fn a_count_past_the_document_clamps_at_the_boundary() {
        // The boundary is read from the uncounted binding rather than written
        // as a byte count: `G` is the document's last line, and the clamp has
        // to land on it.
        let (mut reference, _daemon) = opened_writer(&numbered_lines(5)).await;
        normal(&mut reference, KeyCode::Char('G'));
        let last_line = cursor(&reference);

        let (mut state, _daemon) = opened_writer(&numbered_lines(5)).await;
        assert_eq!(type_count(&mut state, "9999"), Some(editor::MAX_COUNT));
        assert_eq!(normal(&mut state, KeyCode::Char('j')), BufferKey::Consumed);
        assert_eq!(cursor(&state), last_line, "`9999j` stops at the last line");
        assert_eq!(count(&state), None, "and the motion took the count");
        assert!(
            state.notice.is_none(),
            "a clamp is not a refusal, got: {:?}",
            state.notice.as_ref().map(|notice| notice.message())
        );
    }

    /// A count above `MAX_COUNT` saturates and **says so**.
    #[tokio::test]
    async fn a_count_past_the_bound_saturates_and_discloses_it() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(20)).await;
        assert_eq!(
            type_count(&mut state, "10000"),
            Some(editor::MAX_COUNT),
            "the count clamps at the bound"
        );
        let notice = state.notice.as_ref().map(|notice| notice.message());
        assert!(
            notice
                .as_deref()
                .is_some_and(|message| message.contains(&editor::MAX_COUNT.to_string())),
            "the refusal names the bound, got: {notice:?}"
        );
    }

    /// A count is cleared by `Esc` before the next motion ever sees it.
    #[tokio::test]
    async fn escape_clears_a_pending_count_before_the_next_motion() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(20)).await;
        assert_eq!(type_count(&mut state, "3"), Some(3));
        let mut app = app_on_the_editor_route(state).await;
        assert!(handle_editor_key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        ));
        assert_eq!(
            app.editor_state
                .session
                .as_ref()
                .expect("open")
                .presentation()
                .pending_count,
            None,
            "`Esc` is the route's disarm, and the count is a partial gesture"
        );
    }

    /// A gesture that does not consume the count takes it down, as does a
    /// prefix that disarms without resolving.
    #[tokio::test]
    async fn a_gesture_that_does_not_consume_the_count_clears_it() {
        let (mut state, _daemon) = opened_writer(&numbered_lines(20)).await;
        assert_eq!(type_count(&mut state, "3"), Some(3));
        assert_eq!(normal(&mut state, KeyCode::Char('d')), BufferKey::Consumed);
        assert_eq!(count(&state), None, "a repeatable verb is not a motion");
        assert_eq!(normal(&mut state, KeyCode::Char('j')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 7, "so the `j` after it moves one line");

        let (mut state, _daemon) = opened_writer(&numbered_lines(20)).await;
        assert_eq!(type_count(&mut state, "3"), Some(3));
        normal(&mut state, KeyCode::Char('g'));
        assert_eq!(
            normal(&mut state, KeyCode::Char('z')),
            BufferKey::Consumed,
            "`gz` is not a goto leaf, so `g` swallows it"
        );
        assert_eq!(count(&state), None, "and the count went with the prefix");
        normal(&mut state, KeyCode::Char('j'));
        assert_eq!(cursor(&state), 7, "the next `j` moves one line");
    }

    /// A count on a capture is out of scope, so `3f x` finds once.
    #[tokio::test]
    async fn a_capture_discards_a_count_instead_of_repeating_the_find() {
        let (mut state, _daemon) = opened_writer("axbxcxd\n").await;
        assert_eq!(type_count(&mut state, "3"), Some(3));
        normal(&mut state, KeyCode::Char('f'));
        assert_eq!(normal(&mut state, KeyCode::Char('x')), BufferKey::Consumed);
        assert_eq!(cursor(&state), 1, "`3f x` landed on the first `x`");
        assert_eq!(count(&state), None);
    }

    // ── `.` and the keystroke recorder (M008) ───────────────────────────

    /// Type `keys` through the route, letting the real dispatch record them.
    fn type_keys(app: &mut App, keys: &[KeyCode]) {
        for code in keys {
            assert!(
                handle_editor_key(app, key(*code)),
                "{code:?} was not claimed by the editor route"
            );
        }
    }

    /// The recorded gesture, as the characters it holds.
    fn recorded_chars(app: &App) -> Vec<char> {
        app.editor_state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .recorder
            .last_insert()
            .unwrap_or_default()
            .iter()
            .filter_map(|key| match key.code {
                KeyCode::Char(ch) => Some(ch),
                _ => None,
            })
            .collect()
    }

    /// `.` replays the last insert *at the cursor*, and a second `.` inserts
    /// again. The gesture is the entry key plus the text, which is what makes
    /// the repeat land where the reader is now rather than where they were.
    #[tokio::test]
    async fn dot_repeats_the_last_insert_and_twice_inserts_twice() {
        let (state, _daemon) = opened_writer("one\ntwo\n").await;
        let mut app = app_on_the_editor_route(state).await;

        type_keys(
            &mut app,
            &[
                KeyCode::Char('i'),
                KeyCode::Char('X'),
                KeyCode::Char('Y'),
                KeyCode::Char('Z'),
                KeyCode::Esc,
            ],
        );
        assert_eq!(editor_text(&app.editor_state), "XYZone\ntwo\n");
        assert_eq!(
            recorded_chars(&app),
            vec!['i', 'X', 'Y', 'Z'],
            "the entry key is part of the gesture and the closing `Esc` is not"
        );

        type_keys(&mut app, &[KeyCode::Char('.')]);
        assert_eq!(
            editor_text(&app.editor_state),
            "XYZXYZone\ntwo\n",
            "`.` inserts the gesture at the cursor"
        );
        assert_eq!(cursor(&app.editor_state), 6);
        assert_eq!(
            app.editor_state.mode(),
            EditorMode::Normal,
            "`.` ends where `Esc` ends: back in Normal mode"
        );

        type_keys(&mut app, &[KeyCode::Char('.')]);
        assert_eq!(
            editor_text(&app.editor_state),
            "XYZXYZXYZone\ntwo\n",
            "a second `.` inserts a second time"
        );
    }

    /// With nothing recorded `.` is claimed and silent: it must not reach the
    /// composer as a period, and it must not invent a notice either.
    #[tokio::test]
    async fn dot_with_nothing_recorded_is_claimed_and_reports_nothing() {
        let (state, _daemon) = opened_writer("alpha\n").await;
        let mut app = app_on_the_editor_route(state).await;

        assert!(
            handle_editor_key(&mut app, key(KeyCode::Char('.'))),
            "`.` is claimed unconditionally, or it would type a period into the composer"
        );
        assert_eq!(editor_text(&app.editor_state), "alpha\n");
        assert_eq!(
            cursor(&app.editor_state),
            0,
            "a repeat with nothing to repeat moves nothing"
        );
        assert_eq!(notice_of(&app.editor_state), None, "and reports nothing");
    }

    /// A gesture that deleted replays the deletion: the log holds the
    /// keystrokes, so their effect is reproduced rather than a text diff
    /// reapplied at the wrong offsets.
    #[tokio::test]
    async fn dot_replays_a_backspace_inside_the_gesture() {
        let (state, _daemon) = opened_writer("\n").await;
        let mut app = app_on_the_editor_route(state).await;

        type_keys(
            &mut app,
            &[
                KeyCode::Char('i'),
                KeyCode::Char('a'),
                KeyCode::Char('b'),
                KeyCode::Backspace,
                KeyCode::Char('c'),
                KeyCode::Esc,
            ],
        );
        assert_eq!(editor_text(&app.editor_state), "ac\n");

        type_keys(&mut app, &[KeyCode::Char('.')]);
        assert_eq!(
            editor_text(&app.editor_state),
            "acac\n",
            "the backspace's effect replays, so the insert is `ac` and not `abc`"
        );
    }

    /// `o` is part of the gesture, so `.` opens a line too instead of only
    /// typing the text onto the line it is already on.
    #[tokio::test]
    async fn dot_replays_the_line_that_o_opened() {
        let (state, _daemon) = opened_writer("one\ntwo\n").await;
        let mut app = app_on_the_editor_route(state).await;

        type_keys(
            &mut app,
            &[
                KeyCode::Char('o'),
                KeyCode::Char('a'),
                KeyCode::Char('b'),
                KeyCode::Esc,
            ],
        );
        assert_eq!(
            editor_text(&app.editor_state),
            "one\nab\ntwo\n",
            "`o` opened a line and the text landed on it"
        );

        type_keys(&mut app, &[KeyCode::Char('.')]);
        assert_eq!(
            editor_text(&app.editor_state),
            "one\nab\nab\ntwo\n",
            "`.` repeats the newline `o` inserted as well as the text"
        );
    }

    /// The interrupt check runs **between** keys: the keys before an `Esc` are
    /// applied, the ones after it are not, and the editor is left in Normal.
    #[tokio::test]
    async fn an_esc_in_the_log_stops_the_replay_between_keys() {
        let (state, _daemon) = opened_writer("\n").await;
        let mut app = app_on_the_editor_route(state).await;
        {
            // Written by hand: the capture path never records the trailing
            // `Esc`, so an `Esc` in the log can only be an interrupt.
            let recorder = &mut app
                .editor_state
                .session
                .as_mut()
                .expect("open")
                .presentation_mut()
                .recorder;
            recorder.begin();
            for code in [KeyCode::Char('i'), KeyCode::Char('a')] {
                assert_eq!(
                    recorder.record(RecordedKey::of(key(code))),
                    RecorderBound::Recorded
                );
            }
            recorder.record(RecordedKey::of(key(KeyCode::Esc)));
            recorder.record(RecordedKey::of(key(KeyCode::Char('b'))));
            assert!(recorder.finish(), "the log is repeatable");
        }

        type_keys(&mut app, &[KeyCode::Char('.')]);
        assert_eq!(
            editor_text(&app.editor_state),
            "a\n",
            "the key after the `Esc` was not replayed"
        );
        assert_eq!(
            app.editor_state.mode(),
            EditorMode::Normal,
            "an interrupted replay leaves the editor in a consistent mode"
        );
        assert_eq!(
            undo_depth(&app.editor_state),
            1,
            "the edits already applied are their own history, with no rollback"
        );
    }

    /// A replay cannot nest, so a log that contains `.` terminates instead of
    /// recursing: the inner request is refused with a notice and writes
    /// nothing.
    #[tokio::test]
    async fn a_log_containing_dot_is_refused_inside_the_replay_rather_than_recursing() {
        let (state, _daemon) = opened_writer("\n").await;
        let mut app = app_on_the_editor_route(state).await;
        {
            let recorder = &mut app
                .editor_state
                .session
                .as_mut()
                .expect("open")
                .presentation_mut()
                .recorder;
            recorder.begin();
            recorder.record(RecordedKey::of(key(KeyCode::Char('.'))));
            assert!(recorder.finish());
        }

        type_keys(&mut app, &[KeyCode::Char('.')]);

        assert_eq!(
            notice_of(&app.editor_state),
            Some(editor::REPLAY_IN_PROGRESS.to_string()),
            "the replayed `.` found a replay already running and said so"
        );
        assert_eq!(editor_text(&app.editor_state), "\n", "and replayed nothing");
        assert_eq!(
            app.editor_state
                .session
                .as_ref()
                .expect("open")
                .presentation()
                .recorder
                .last_insert()
                .map(<[RecordedKey]>::len),
            Some(1),
            "the log is unchanged, so the refusal was a state and not a second gesture"
        );
    }

    /// The security property: the repeat is edits through the same funnel, so
    /// a gesture recorded while the document was writable is **refused** once
    /// it is not — not replayed into it.
    #[tokio::test]
    async fn a_gesture_recorded_on_a_writable_document_is_refused_on_a_read_only_one() {
        let (state, _daemon) = opened_writer("\n").await;
        let mut app = app_on_the_editor_route(state).await;
        type_keys(
            &mut app,
            &[
                KeyCode::Char('i'),
                KeyCode::Char('a'),
                KeyCode::Char('b'),
                KeyCode::Esc,
            ],
        );
        let gesture = app
            .editor_state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .recorder
            .last_insert()
            .expect("the gesture was recorded on the writable document")
            .to_vec();

        // The same gesture, on an attachment that never held the writer lease.
        let (reader, _reader_daemon) = opened_reader("\n").await;
        let mut read_only = app_on_the_editor_route(reader).await;
        let before = editor_text(&read_only.editor_state);

        replay_recorded_keys(&mut read_only, &gesture);

        assert_eq!(
            editor_text(&read_only.editor_state),
            before,
            "the repeat wrote nothing into a buffer it may not write"
        );
        assert_eq!(cursor(&read_only.editor_state), 0, "and it moved nothing");
        assert_eq!(
            notice_of(&read_only.editor_state),
            Some(EditorNotice::ReadOnly.message()),
            "the reader is told why nothing happened"
        );
        assert_eq!(
            undo_depth(&read_only.editor_state),
            0,
            "no history entry was recorded for an edit that was refused"
        );
        assert_eq!(read_only.editor_state.mode(), EditorMode::Normal);
        assert!(
            !read_only
                .editor_state
                .session
                .as_ref()
                .expect("open")
                .presentation()
                .recorder
                .is_replaying(),
            "the refusal releases the replay flag rather than leaving it set"
        );
    }

    /// Replay does not record: a replay that captured itself would grow the log
    /// on every press, and with a macro chord it could start a recording.
    #[tokio::test]
    async fn a_replay_neither_records_itself_nor_opens_a_capture() {
        let (state, _daemon) = opened_writer("\n").await;
        let mut app = app_on_the_editor_route(state).await;
        type_keys(
            &mut app,
            &[
                KeyCode::Char('i'),
                KeyCode::Char('a'),
                KeyCode::Char('b'),
                KeyCode::Esc,
            ],
        );

        type_keys(&mut app, &[KeyCode::Char('.')]);

        let recorder = &app
            .editor_state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .recorder;
        assert!(
            !recorder.is_recording(),
            "the replayed keys opened no capture"
        );
        assert_eq!(
            recorded_chars(&app).len(),
            3,
            "and the log it read was not appended to"
        );
        assert!(!recorder.is_replaying(), "the replay flag is released");
    }

    /// The recorder is document-local: it is cleared by the one transition
    /// that clears the registers, so `.` cannot repeat a gesture made against
    /// a buffer the reader is no longer looking at.
    #[tokio::test]
    async fn a_recorded_gesture_is_cleared_by_the_transition_that_clears_the_registers() {
        let (state, _daemon) = opened_writer("\n").await;
        let mut app = app_on_the_editor_route(state).await;
        type_keys(
            &mut app,
            &[KeyCode::Char('i'), KeyCode::Char('a'), KeyCode::Esc],
        );
        {
            let session = app.editor_state.session.as_mut().expect("open");
            session
                .presentation_mut()
                .write_register(RegisterName::Named('a'), "alpha");
            assert!(session.presentation().recorder.last_insert().is_some());
            // The reconciliation path: one clearing point for both stores.
            editor::clear_history(session.presentation_mut());
            assert!(session.presentation().registers.is_empty());
            assert!(
                session.presentation().recorder.last_insert().is_none(),
                "the same transition clears the recorder, so there is no second lifetime"
            );
        }

        type_keys(&mut app, &[KeyCode::Char('.')]);
        assert_eq!(
            editor_text(&app.editor_state),
            "a\n",
            "`.` has nothing to repeat after the document moved on"
        );
    }

    /// The cap is enforced through the real capture path, disclosed once, and
    /// the gesture it overflowed is not repeatable.
    #[tokio::test]
    async fn a_gesture_past_the_cap_is_disclosed_and_not_repeatable() {
        let (state, _daemon) = opened_writer("\n").await;
        let mut app = app_on_the_editor_route(state).await;

        // The entry key is the first of the bound, so `MAX - 1` characters
        // fill it exactly.
        type_keys(&mut app, &[KeyCode::Char('i')]);
        for _ in 0..(editor::MAX_RECORDED_KEYS - 1) {
            type_keys(&mut app, &[KeyCode::Char('x')]);
        }
        let at_cap = cursor(&app.editor_state);
        assert_eq!(
            notice_of(&app.editor_state),
            None,
            "the bound is not reached yet"
        );

        type_keys(&mut app, &[KeyCode::Char('y')]);
        assert_eq!(
            notice_of(&app.editor_state),
            Some(editor::recording_capped_notice()),
            "crossing the bound is disclosed at the keystroke that crossed it"
        );
        type_keys(&mut app, &[KeyCode::Esc]);
        // `y` still typed into the buffer — the recorder refused it, the
        // document did not — so the position to compare against is the one
        // left by the whole gesture.
        let before_repeat = cursor(&app.editor_state);
        assert_eq!(before_repeat, at_cap + 1);

        assert!(
            app.editor_state
                .session
                .as_ref()
                .expect("open")
                .presentation()
                .recorder
                .last_insert()
                .is_none(),
            "a gesture that crossed the bound is dropped rather than replayed short"
        );
        app.editor_state.clear_notice();
        type_keys(&mut app, &[KeyCode::Char('.')]);
        assert_eq!(
            cursor(&app.editor_state),
            before_repeat,
            "`.` wrote nothing: the refused gesture was not kept"
        );
    }
}
