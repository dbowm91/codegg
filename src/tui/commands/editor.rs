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
    LspCompletionUnsupportedReason, LspDiagnosticsGetRequestDto, LspDiagnosticsResultDto,
    LspReadOperation, LspReadRequestDto, LspReadResultDto, LspReadStatus,
};

use crate::tui::app::state::editor::{EditorNotice, EditorOperation, EditorState, EditorStatus};
use crate::tui::app::{App, TuiCommand};
use crate::tui::async_cmd::{spawn_registered_tui_task, spawn_scoped_registered_tui_task};
use crate::tui::document_session::TuiDocumentSession;
use crate::tui::editor::{
    self, EditorCommand, EditorCommandName, EditorFocus, EditorMode, MAX_EDITOR_PENDING_COMMAND,
};
use crate::tui::task_lifecycle::TuiTaskKind;

use super::super::editor_completion::EditorCompletionScope;
use super::super::editor_lsp::EditorLspScope;

/// Maximum length of a workspace-relative path accepted by the open command.
pub const MAX_EDITOR_RELATIVE_PATH: usize = 512;

/// Maximum number of path components accepted by the open command.
const MAX_EDITOR_PATH_DEPTH: usize = 64;

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
        focus_editor_buffer(&mut app.editor_state);
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
    // Focus the buffer immediately: the user asked for a new file to edit,
    // and landing in the composer would make the template look unfocused.
    focus_editor_buffer(&mut app.editor_state);

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
    app.editor_state.clear_notice();
    refresh_editor_diagnostics(app);
    start_editor_lsp_read(app, LspReadOperation::SemanticTokens, None);
}

fn current_lsp_scope(app: &App) -> Option<EditorLspScope> {
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
    // The diagnostics panel takes the centered surface; a completion popup
    // underneath would be stale context, so the explicit gesture wins.
    app.editor_state.completion.dismiss();
    let Some(request_id) = app.editor_state.lsp.begin_diagnostics(&scope) else {
        return;
    };
    let Some(client) = app.core_client.clone() else {
        app.editor_state.lsp.diagnostics_loading = false;
        app.editor_state.lsp.diagnostics_status =
            crate::tui::editor_lsp::DiagnosticsViewStatus::Unavailable;
        app.editor_state.lsp.diagnostics_stale = !app.editor_state.lsp.diagnostics.is_empty();
        return;
    };
    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|id| id.to_string());
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_lsp_diagnostics",
        tab_id,
        None,
        None,
        async move {
            let response = client
                .request(crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    crate::protocol::core::CoreRequest::LspDiagnosticsGet {
                        request: LspDiagnosticsGetRequestDto {
                            project_id: scope.project_id.clone(),
                            session_id: scope.session_id.clone(),
                        },
                    },
                ))
                .await;
            let (result, error) = match response {
                Ok(crate::protocol::core::CoreResponse::LspDiagnosticsGetResult { result }) => {
                    (Some(result), None)
                }
                Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                    (None, Some(message))
                }
                Ok(other) => (
                    None,
                    Some(format!("unexpected diagnostics response: {other:?}")),
                ),
                Err(error) => (None, Some(error.to_string())),
            };
            Some(TuiCommand::EditorDiagnosticsFinished {
                request_id,
                scope,
                result,
                error,
            })
        },
    );
    if task_id.is_none() {
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

fn start_editor_lsp_read(app: &mut App, operation: LspReadOperation, query: Option<String>) {
    let Some(scope) = current_lsp_scope(app) else {
        app.messages_state
            .toasts
            .warning("LSP is unavailable: select a session-bound workspace document first");
        return;
    };
    // A read result panel takes the centered surface; see above.
    app.editor_state.completion.dismiss();
    let Some(request_id) = app.editor_state.lsp.begin_read(&scope) else {
        app.messages_state
            .toasts
            .info("An LSP request is already running");
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
    };
    if let Err(error) = request.validate() {
        app.editor_state.lsp.read_loading = false;
        app.editor_state.lsp.read_error = Some(format!("invalid LSP query: {error:?}"));
        app.editor_state.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
        return;
    }
    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|id| id.to_string());
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_lsp_read",
        tab_id,
        None,
        None,
        async move {
            let response = client
                .request(crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    crate::protocol::core::CoreRequest::LspReadGet { request },
                ))
                .await;
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
                result,
                error,
            })
        },
    );
    if task_id.is_none() {
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

// ── Completion (M004) ────────────────────────────────────────────────────────

/// Build the completion scope for an explicit user request.
///
/// The read scope proves session/project/workspace/path binding; the
/// attachment adds the real document identity and daemon revision; the
/// cursor resolves to the UTF-16 position the server will see. Returns
/// `None` when anything is missing, leaving the popup closed: a completion
/// request without a bound attachment is a client bug, reported by the
/// caller rather than sent half-formed.
fn current_completion_scope(app: &App) -> Option<EditorCompletionScope> {
    let read_scope = current_lsp_scope(app)?;
    let session = app.editor_state.session.as_ref()?;
    let attachment = session.attachment_info()?;
    let snapshot = session.try_snapshot()?;
    let cursor = session.presentation().cursor_byte.min(snapshot.len_bytes());
    let line = editor::cursor_line(&snapshot, cursor);
    let row_start = editor::line_start(&snapshot, line)?;
    let prefix = snapshot.read_bytes(row_start, cursor).ok()?;
    Some(EditorCompletionScope {
        read_scope,
        document_id: attachment.document_id,
        revision: attachment.revision,
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(prefix.encode_utf16().count()).unwrap_or(u32::MAX),
    })
}

/// Start one completion request on explicit user action.
///
/// Opens the popup in its loading state and issues the M003 read through
/// the scoped task seam. A re-trigger supersedes the in-flight request: the
/// state guard drops the older answer on arrival. Nothing here polls,
/// retries, or auto-accepts.
pub(crate) fn start_editor_completion(app: &mut App) {
    let Some(scope) = current_completion_scope(app) else {
        app.messages_state
            .toasts
            .warning("Completion needs a session-bound workspace document — open a file first");
        return;
    };
    let request_id = app.editor_state.completion.begin(&scope);
    let Some(client) = app.core_client.clone() else {
        app.editor_state.completion.apply(
            request_id,
            &scope,
            None,
            Some("daemon unavailable".to_string()),
        );
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
        app.editor_state.completion.apply(
            request_id,
            &scope,
            None,
            Some(format!("invalid completion request: {error:?}")),
        );
        return;
    }
    let tx = app.tui_cmd_tx.clone();
    let tab_id = app.active_tab_id().map(|tab_id| tab_id.to_string());
    // The completion keeps `scope` for the spawn-failure path below; the
    // task gets its own clone.
    let task_scope = scope.clone();
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Editor,
        "editor_completion",
        tab_id,
        None,
        None,
        async move {
            let response = client
                .request(crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    crate::protocol::core::CoreRequest::LspCompletionGet { request },
                ))
                .await;
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
                scope: task_scope,
                result,
                error,
            })
        },
    );
    if task_id.is_none() {
        app.editor_state.completion.apply(
            request_id,
            &scope,
            None,
            Some("TUI command channel unavailable".to_string()),
        );
    }
}

/// Apply one finished completion request.
///
/// Generation- and scope-fenced like the read path: answers for a moved
/// cursor, an old revision, or a previous attachment are dropped by the
/// state guard rather than displayed.
pub(crate) fn apply_editor_completion(
    app: &mut App,
    request_id: u64,
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
        .apply(request_id, &scope, result, error);
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
    if !app.editor_state.completion.is_open() {
        return false;
    }
    let normal_mode = app.editor_state.mode() == EditorMode::Normal;
    match key.code {
        KeyCode::Esc => {
            app.editor_state.completion.dismiss();
            true
        }
        KeyCode::Enter => {
            accept_selected_completion(app);
            true
        }
        KeyCode::Up => {
            app.editor_state.completion.select_previous();
            true
        }
        KeyCode::Down => {
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
        start_editor_lsp_read(app, LspReadOperation::SemanticTokens, None);
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
                super::agents::refresh_assets_for_session(app, false);
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
        super::agents::refresh_assets_for_session(app, false);
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

/// Submit one resolved edit through the controller and adopt the result.
///
/// The undo entry is recorded **only** after the controller accepts the
/// transaction, so a rejected edit never leaves a phantom inverse in
/// history. The pre-edit cursor and selection are captured before the
/// apply so undo restores the position the user actually left.
fn submit_edit(state: &mut EditorState, edit: editor::EditorEdit) {
    let editor::EditorEdit {
        transaction,
        inverse,
        bytes,
        cursor_after,
        stay_insert,
    } = edit;
    // The forward transaction is retained for redo; both directions are
    // bounded by the same depth/byte caps.
    let forward = transaction.clone();
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let controller = session.controller();
    let (cursor_before, anchor_before) = {
        let presentation = session.presentation();
        (presentation.cursor_byte, presentation.selection.clone())
    };
    match controller.apply_local(transaction) {
        Ok(_) => {
            // Clamp against the post-edit text, not the pre-edit snapshot.
            let after = controller.try_snapshot().map(|(snapshot, _)| snapshot);
            let Some(session) = state.session.as_mut() else {
                return;
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
            if stay_insert {
                presentation.mode = EditorMode::Insert;
            }
            editor::push_undo(
                presentation,
                forward,
                inverse,
                bytes,
                cursor_before,
                anchor_before,
            );
            state.lsp.clear_semantic();
            // Any local edit — including a completion accept — invalidates
            // the popup and its pending request: the text moved on.
            state.completion.clear();
            if !state.lsp.diagnostics.is_empty() {
                state.lsp.diagnostics_stale = true;
            }
            state.status = EditorStatus::DirtyLocal;
            state.clear_notice();
        }
        Err(error) => {
            // A destructive phase in flight and a full queue are both
            // expected conditions. Neither disturbs the local text nor the
            // history, and the editor never retries internally.
            state.set_notice(EditorNotice::from_error(&error));
        }
    }
}

/// Whether the editor may accept a local edit right now.
fn editable(state: &EditorState) -> bool {
    state.is_open() && !state.is_busy() && state.is_writer() && !state.status.blocks_edits()
}

/// Apply one motion command. Never touches text.
pub(crate) fn editor_motion(state: &mut EditorState, command: EditorCommand, visible_lines: usize) {
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    if let Some(session) = state.session.as_mut() {
        editor::apply_motion(
            session.presentation_mut(),
            &snapshot,
            command,
            visible_lines,
        );
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

/// Undo the newest local edit by submitting its inverse transaction.
///
/// Undo during a destructive phase fails exactly like any other edit: the
/// controller returns `LifecycleBusy`, the entry stays in history, and no
/// transaction is queued for a phase that will refuse it.
pub(crate) fn editor_undo(state: &mut EditorState) {
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(entry) = session.presentation().undo.last().cloned() else {
        return;
    };
    let cursor_leaving = session.presentation().cursor_byte;
    let controller = session.controller();
    if let Err(error) = controller.apply_local(entry.inverse.clone()) {
        state.set_notice(EditorNotice::from_error(&error));
        return;
    }
    let after = controller.try_snapshot().map(|(snapshot, _)| snapshot);
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let presentation = session.presentation_mut();
    let Some(entry) = editor::take_undo(presentation) else {
        return;
    };
    match &after {
        Some(snapshot) => {
            presentation.cursor_byte = editor::clamp_offset(snapshot, entry.cursor_before);
            presentation.selection = None;
            presentation.viewport_line = presentation
                .viewport_line
                .min(snapshot.len_lines().saturating_sub(1));
        }
        None => presentation.cursor_byte = entry.cursor_before,
    }
    presentation.mode = EditorMode::Normal;
    // The redo entry keeps the forward transaction and is rebased onto the
    // position the user is leaving, so redoing returns to where the edit
    // left the cursor rather than to the undo point.
    editor::record_redo(presentation, entry.rebased(cursor_leaving, None));
    state.lsp.clear_semantic();
    state.completion.clear();
    if !state.lsp.diagnostics.is_empty() {
        state.lsp.diagnostics_stale = true;
    }
    state.status = EditorStatus::DirtyLocal;
    state.clear_notice();
}

/// Redo the newest undone local edit by re-submitting its forward
/// transaction.
pub(crate) fn editor_redo(state: &mut EditorState) {
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(entry) = session.presentation().redo.last().cloned() else {
        return;
    };
    let cursor_leaving = session.presentation().cursor_byte;
    let controller = session.controller();
    if let Err(error) = controller.apply_local(entry.forward.clone()) {
        state.set_notice(EditorNotice::from_error(&error));
        return;
    }
    let after = controller.try_snapshot().map(|(snapshot, _)| snapshot);
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let presentation = session.presentation_mut();
    let Some(entry) = editor::take_redo(presentation) else {
        return;
    };
    match &after {
        Some(snapshot) => {
            presentation.cursor_byte = editor::clamp_offset(snapshot, entry.cursor_before);
            presentation.selection = None;
            presentation.viewport_line = presentation
                .viewport_line
                .min(snapshot.len_lines().saturating_sub(1));
        }
        None => presentation.cursor_byte = entry.cursor_before,
    }
    editor::restore_undo(presentation, entry.rebased(cursor_leaving, None));
    state.lsp.clear_semantic();
    state.completion.clear();
    if !state.lsp.diagnostics.is_empty() {
        state.lsp.diagnostics_stale = true;
    }
    state.status = EditorStatus::DirtyLocal;
    state.clear_notice();
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

// ── Focus ────────────────────────────────────────────────────────────────────

/// Give the buffer keyboard focus, leaving the composer untouched.
pub(crate) fn focus_editor_buffer(state: &mut EditorState) {
    if let Some(session) = state.session.as_mut() {
        session.presentation_mut().focus = EditorFocus::Buffer;
    }
}

/// Return keyboard focus to the ordinary Session/Task composer.
pub(crate) fn focus_editor_composer(state: &mut EditorState) {
    if let Some(session) = state.session.as_mut() {
        session.presentation_mut().focus = EditorFocus::Composer;
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
fn handle_file_tree_key(app: &mut App, key: KeyEvent) -> bool {
    use crossterm::event::KeyCode;
    match key.code {
        // Leaving the tree returns focus to the editor buffer without
        // changing the route, so the composer stays reachable.
        KeyCode::Esc => {
            super::file_tree::set_focus(app, false);
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            super::file_tree::move_selection(app, 1);
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            super::file_tree::move_selection(app, -1);
            true
        }
        // `l`/`h` walk in and out of a directory, matching the `/files` box.
        // Left alone, `h` used to toggle the selection instead of leaving it,
        // so backing out and collapsing were the same key.
        KeyCode::Char('l') | KeyCode::Right => {
            super::file_tree::enter_selected(app);
            true
        }
        KeyCode::Char('h') | KeyCode::Left => {
            super::file_tree::leave_selected(app);
            true
        }
        // Enter opens a file, or expands a directory, through the existing
        // controller-backed open path.
        KeyCode::Enter => {
            super::file_tree::activate_selected(app);
            true
        }
        // `r` re-walks the workspace.
        KeyCode::Char('r') => {
            super::file_tree::rebuild(app);
            true
        }
        _ => false,
    }
}

pub(crate) fn handle_editor_key(app: &mut App, key: KeyEvent) -> bool {
    if !is_editor_view_active(app) {
        return false;
    }
    // M006-D: the tree pane claims keys only while it is focused, and only
    // the ones it owns. Anything else falls through, so a focused tree never
    // silently swallows composer or editor input.
    if app.ui_state.tree_focused && handle_file_tree_key(app, key) {
        return true;
    }
    // Ctrl-T toggles the pane from anywhere in the editor, focused or not.
    if key.code == KeyCode::Char('t') && key.modifiers == KeyModifiers::CONTROL {
        super::file_tree::toggle_tree(app);
        return true;
    }
    // With the composer focused, Esc leaves the view. With the buffer
    // focused, Esc is handled by the buffer key handler so the step from
    // insert mode to focus stays explicit.
    if key.code == KeyCode::Esc && !app.editor_state.buffer_focused() {
        close_editor(app);
        return true;
    }
    if !app.editor_state.buffer_focused() {
        // While the composer owns input, only the buffer focus toggle is an
        // editor concern. Everything else belongs to the prompt.
        if key.code == KeyCode::Char('e') && key.modifiers == KeyModifiers::CONTROL {
            focus_editor_buffer(&mut app.editor_state);
            app.messages_state
                .toasts
                .info("Editor buffer focused — Esc returns to the composer, Esc again leaves");
            return true;
        }
        return false;
    }
    // M006-F: with the buffer focused the editor has taken the place of the
    // main buffer, so the directional pane gestures stay available here —
    // `Shift+L` into the column, `Shift+H` back — and they are answered
    // *before* the buffer keymap, which would otherwise read `L`/`H` as an
    // unknown normal-mode verb. Normal mode only: in Insert those letters
    // are document text.
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
    if handle_lsp_panel_key(app, key) {
        return true;
    }
    if handle_completion_key(app, key) {
        return true;
    }
    match key.code {
        KeyCode::F(2) => {
            refresh_editor_diagnostics(app);
            app.editor_state.lsp.open_diagnostics_panel();
            return true;
        }
        KeyCode::F(3) => {
            start_editor_lsp_read(app, LspReadOperation::Hover, None);
            return true;
        }
        KeyCode::F(4) => {
            start_editor_lsp_read(app, LspReadOperation::Definition, None);
            return true;
        }
        KeyCode::F(5) => {
            start_editor_lsp_read(app, LspReadOperation::References, None);
            return true;
        }
        KeyCode::F(6) => {
            start_editor_lsp_read(app, LspReadOperation::DocumentSymbols, None);
            return true;
        }
        KeyCode::F(7) => {
            let Some(query) = editor_word_at_cursor(&app.editor_state) else {
                app.messages_state.toasts.info("No symbol at the cursor");
                return true;
            };
            start_editor_lsp_read(app, LspReadOperation::WorkspaceSymbols, Some(query));
            return true;
        }
        _ => {}
    }
    // Lifecycle round trips stay on the App layer, so the buffer handler
    // reports the request instead of performing it.
    match editor_buffer_key(&mut app.editor_state, key) {
        BufferKey::Consumed => true,
        BufferKey::Unhandled => false,
        BufferKey::RequestSave => {
            save_editor(app);
            true
        }
        BufferKey::RequestResync => {
            resync_editor(app);
            true
        }
        BufferKey::RequestReload => {
            reload_editor(app);
            true
        }
        BufferKey::RequestLeaveBuffer => {
            leave_editor_buffer(&mut app.editor_state);
            true
        }
        BufferKey::RunCommand(command, argument) => {
            run_editor_command(app, command, argument);
            true
        }
    }
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
            //
            // The scope comes from the argument (`global foo` / `project
            // foo`); `resolve_new_agent` owns the split so the same rule
            // applies to the panel and to this command.
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
        // `:complete` takes no argument; the parser already refuses one.
        EditorCommandName::Complete => start_editor_completion(app),
    }
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
    match key.code {
        KeyCode::Esc => app.editor_state.lsp.close_panel(),
        KeyCode::Char('j') | KeyCode::Down => {
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
        KeyCode::Char('k') | KeyCode::Up => {
            app.editor_state.lsp.selected_index =
                app.editor_state.lsp.selected_index.saturating_sub(1)
        }
        KeyCode::Char('r') if panel == EditorLspPanel::Diagnostics => {
            refresh_editor_diagnostics(app)
        }
        KeyCode::Enter if panel == EditorLspPanel::Diagnostics => {
            jump_to_selected_diagnostic(&mut app.editor_state)
        }
        KeyCode::Enter if panel == EditorLspPanel::ReadResult => open_selected_lsp_target(app),
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
        session.presentation_mut().cursor_byte = start + column;
        session.presentation_mut().viewport_line = diagnostic.range.start_line as usize;
    }
    state.lsp.close_panel();
}

fn open_selected_lsp_target(app: &mut App) {
    use crate::protocol::lsp::LspReadPayloadDto;
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

/// Leave the buffer, one step at a time: insert mode first, then focus.
fn leave_editor_buffer(state: &mut EditorState) {
    if state.mode() == EditorMode::Insert {
        if let Some(session) = state.session.as_mut() {
            session.presentation_mut().mode = EditorMode::Normal;
        }
        state.clear_notice();
        return;
    }
    focus_editor_composer(state);
}

/// Outcome of one key while the editor buffer owns input.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BufferKey {
    /// The editor handled the key; nothing else should see it.
    Consumed,
    /// The editor does not claim the key, so the composer may.
    Unhandled,
    /// The user asked to save; the App layer issues the round trip.
    RequestSave,
    /// The user asked to resync; the App layer issues the round trip.
    RequestResync,
    /// The user asked to reload from disk, discarding the local draft.
    RequestReload,
    /// The user asked to leave the buffer. What that means depends on
    /// state the App layer owns: insert mode becomes normal mode, and
    /// normal mode returns focus to the composer.
    RequestLeaveBuffer,
    /// The user ran a `:` command. The App layer owns every round trip and
    /// every route change, so the buffer reports the parsed command rather
    /// than performing it.
    RunCommand(EditorCommandName, Option<String>),
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

/// Handle one key while the editor buffer owns input.
///
/// Pure presentation state, plus an explicit request for the two lifecycle
/// round trips that belong to the App layer.
fn editor_buffer_key(state: &mut EditorState, key: KeyEvent) -> BufferKey {
    // Command mode is modal and outranks everything below it, including the
    // mode-independent Ctrl chords: a half-typed `:q` must not be rewritten
    // by `Ctrl+S` silently writing the file the user was about to close.
    if state
        .session
        .as_ref()
        .is_some_and(|session| session.presentation().command_active)
    {
        return handle_command_line_key(state, key);
    }
    // Save and resync are mode-independent and always available.
    if key.code == KeyCode::Char('s') && key.modifiers == KeyModifiers::CONTROL {
        return BufferKey::RequestSave;
    }
    if key.code == KeyCode::Char('r') && key.modifiers == KeyModifiers::CONTROL {
        return BufferKey::RequestResync;
    }
    // `R` reloads from disk, discarding the local draft. It is
    // destructive, so it is only reachable in normal mode; the App layer
    // decides whether to issue the round trip.
    if key.code == KeyCode::Char('R')
        && key.modifiers == KeyModifiers::NONE
        && state.mode() == EditorMode::Normal
    {
        return BufferKey::RequestReload;
    }
    // Esc always leaves the buffer. Leaving insert mode and returning
    // focus are both pure state changes, but the App layer decides which
    // one applies, so the request is reported rather than performed.
    if key.code == KeyCode::Esc {
        return BufferKey::RequestLeaveBuffer;
    }
    let visible_lines = visible_line_estimate();
    let consumed = if state.mode() == EditorMode::Insert {
        handle_insert_key(state, key, visible_lines)
    } else {
        handle_normal_key(state, key, visible_lines)
    };
    if consumed {
        BufferKey::Consumed
    } else {
        BufferKey::Unhandled
    }
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
    match key.code {
        KeyCode::Char(ch)
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            editor_insert_literal(state, &ch.to_string());
            true
        }
        KeyCode::Backspace => {
            editor_delete_backward(state);
            true
        }
        KeyCode::Delete => {
            editor_editing_command(state, EditorCommand::DeleteChar);
            true
        }
        KeyCode::Left => {
            editor_motion(state, EditorCommand::MoveLeft, visible_lines);
            true
        }
        KeyCode::Right => {
            editor_motion(state, EditorCommand::MoveRight, visible_lines);
            true
        }
        KeyCode::Up => {
            editor_motion(state, EditorCommand::MoveUp, visible_lines);
            true
        }
        KeyCode::Down => {
            editor_motion(state, EditorCommand::MoveDown, visible_lines);
            true
        }
        KeyCode::Enter => {
            editor_insert_literal(state, "\n");
            true
        }
        KeyCode::Tab => {
            editor_insert_literal(state, "\t");
            true
        }
        _ => false,
    }
}

fn handle_normal_key(state: &mut EditorState, key: KeyEvent, visible_lines: usize) -> bool {
    // Multi-key prefixes resolve first.
    let pending = state
        .session
        .as_ref()
        .map(|session| session.presentation().pending_command.clone())
        .unwrap_or_default();
    if !pending.is_empty() {
        if let Some(session) = state.session.as_mut() {
            session.presentation_mut().pending_command.clear();
        }
        let KeyCode::Char(ch) = key.code else {
            return true;
        };
        let resolved = match (pending.as_str(), ch) {
            ("d", 'd') => Some(EditorCommand::DeleteLine),
            ("d", 'w') => Some(EditorCommand::DeleteWord),
            ("g", 'g') => Some(EditorCommand::BufferStart),
            _ => None,
        };
        if let Some(command) = resolved {
            editor_editing_command(state, command);
        }
        return true;
    }

    match key.code {
        KeyCode::Char('h') | KeyCode::Left => {
            editor_motion(state, EditorCommand::MoveLeft, visible_lines);
            true
        }
        KeyCode::Char('l') | KeyCode::Right => {
            editor_motion(state, EditorCommand::MoveRight, visible_lines);
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            editor_motion(state, EditorCommand::MoveDown, visible_lines);
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            editor_motion(state, EditorCommand::MoveUp, visible_lines);
            true
        }
        KeyCode::PageDown => {
            editor_motion(state, EditorCommand::HalfPageDown, visible_lines);
            true
        }
        KeyCode::PageUp => {
            editor_motion(state, EditorCommand::HalfPageUp, visible_lines);
            true
        }
        KeyCode::Char('0') if key.modifiers == KeyModifiers::NONE => {
            editor_motion(state, EditorCommand::LineStart, visible_lines);
            true
        }
        KeyCode::Char('$') => {
            editor_motion(state, EditorCommand::LineEnd, visible_lines);
            true
        }
        KeyCode::Char('G') => {
            editor_motion(state, EditorCommand::BufferEnd, visible_lines);
            true
        }
        KeyCode::Char('w') => {
            editor_motion(state, EditorCommand::WordForward, visible_lines);
            true
        }
        KeyCode::Char('b') => {
            editor_motion(state, EditorCommand::WordBackward, visible_lines);
            true
        }
        KeyCode::Char('d') => {
            push_pending(state, 'd');
            true
        }
        KeyCode::Char('g') => {
            push_pending(state, 'g');
            true
        }
        KeyCode::Char('i') => {
            enter_insert(state, EditorCommand::InsertBefore);
            true
        }
        KeyCode::Char('a') => {
            enter_insert(state, EditorCommand::InsertAfter);
            true
        }
        KeyCode::Char('I') => {
            enter_insert(state, EditorCommand::InsertLineStart);
            true
        }
        KeyCode::Char('A') => {
            enter_insert(state, EditorCommand::InsertLineEnd);
            true
        }
        KeyCode::Char('o') => {
            enter_insert(state, EditorCommand::OpenLineBelow);
            true
        }
        KeyCode::Char('O') => {
            enter_insert(state, EditorCommand::OpenLineAbove);
            true
        }
        KeyCode::Char('x') => {
            editor_editing_command(state, EditorCommand::DeleteChar);
            true
        }
        KeyCode::Char('D') => {
            editor_editing_command(state, EditorCommand::DeleteToLineEnd);
            true
        }
        KeyCode::Char('u') => {
            editor_undo(state);
            true
        }
        // `:` opens the command line. Normal mode only: in Insert the colon
        // is document text, and silently swallowing it would make it
        // impossible to type a URL into a file.
        KeyCode::Char(':') if key.modifiers == KeyModifiers::NONE => {
            if let Some(session) = state.session.as_mut() {
                session.presentation_mut().open_command_line("");
            }
            true
        }
        KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => {
            editor_redo(state);
            true
        }
        _ => false,
    }
}

fn push_pending(state: &mut EditorState, ch: char) {
    let Some(session) = state.session.as_mut() else {
        return;
    };
    let presentation = session.presentation_mut();
    if presentation.pending_command.len() >= MAX_EDITOR_PENDING_COMMAND {
        presentation.pending_command.clear();
        return;
    }
    presentation.pending_command.push(ch);
}

/// Move the cursor for an insertion entry command and switch to insert mode.
fn enter_insert(state: &mut EditorState, command: EditorCommand) {
    let Some(session) = state.session.as_ref() else {
        return;
    };
    let Some(snapshot) = session.try_snapshot() else {
        return;
    };
    let Some(session) = state.session.as_mut() else {
        return;
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
    presentation.mode = EditorMode::Insert;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::core::{CoreRequest, CoreResponse};
    use crate::tui::app::state::EditorNoticeSeverity;
    use crate::tui::app::state::EditorState;
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
    ///
    /// Enough to qualify the editor's lifecycle end to end: it serves
    /// snapshots, applies transactions, and can be told to conflict, to go
    /// offline, to report the document gone, or to block a reload so a
    /// destructive phase can be observed while it is in flight.
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
        let controller = Arc::new(DocumentController::new(daemon.clone()));
        controller
            .open(
                "project-1".into(),
                "workspace-1".into(),
                "src/lib.rs".into(),
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
            path: Some("src/lib.rs".into()),
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

    fn pending(state: &EditorState) -> String {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .pending_command
            .clone()
    }

    fn undo_depth(state: &EditorState) -> usize {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .undo
            .len()
    }

    fn redo_depth(state: &EditorState) -> usize {
        state
            .session
            .as_ref()
            .expect("open")
            .presentation()
            .redo
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
        // Every keystroke is individually reversible.
        assert_eq!(undo_depth(&state), 25);
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

    #[tokio::test]
    async fn insert_mode_keys_type_into_the_buffer_and_escape_leaves_insert() {
        let (mut state, _daemon) = opened_writer("").await;
        focus_editor_buffer(&mut state);
        assert_eq!(state.mode(), EditorMode::Normal);

        assert!(matches!(
            editor_buffer_key(&mut state, key(KeyCode::Char('i'))),
            BufferKey::Consumed
        ));
        assert_eq!(state.mode(), EditorMode::Insert);
        for ch in "hi".chars() {
            editor_buffer_key(&mut state, key(KeyCode::Char(ch)));
        }
        assert_eq!(editor_text(&state), "hi");

        // Esc leaves the buffer one step at a time: insert mode first, then
        // focus. The buffer handler reports the step; the App layer applies
        // it, because only it knows the route.
        assert!(matches!(
            editor_buffer_key(&mut state, key(KeyCode::Esc)),
            BufferKey::RequestLeaveBuffer
        ));
        leave_editor_buffer(&mut state);
        assert_eq!(state.mode(), EditorMode::Normal);
        assert!(state.buffer_focused(), "one Esc leaves insert mode only");

        assert!(matches!(
            editor_buffer_key(&mut state, key(KeyCode::Esc)),
            BufferKey::RequestLeaveBuffer
        ));
        leave_editor_buffer(&mut state);
        assert!(
            !state.buffer_focused(),
            "a second Esc returns focus to the composer"
        );

        // With focus on the composer the buffer handler sees no keys, which
        // is what lets the ordinary prompt keep them.
        focus_editor_buffer(&mut state);
        // Normal mode now interprets letters as commands, not text.
        editor_motion(&mut state, EditorCommand::BufferStart, 10);
        editor_buffer_key(&mut state, key(KeyCode::Char('x')));
        assert_eq!(editor_text(&state), "i", "`x` deletes, it does not type");
    }

    #[tokio::test]
    async fn save_and_resync_are_requested_rather_than_performed_in_the_buffer() {
        let (mut state, _daemon) = opened_writer("base").await;
        focus_editor_buffer(&mut state);
        assert!(matches!(
            editor_buffer_key(
                &mut state,
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
            ),
            BufferKey::RequestSave
        ));
        assert!(matches!(
            editor_buffer_key(
                &mut state,
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)
            ),
            BufferKey::RequestResync
        ));
        assert!(matches!(
            editor_buffer_key(
                &mut state,
                KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE)
            ),
            BufferKey::RequestReload
        ));
        // The buffer handler performs no transport work.
        assert_eq!(editor_text(&state), "base");
        assert!(!state.is_busy());
    }

    #[tokio::test]
    async fn unclaimed_keys_fall_through_to_the_composer() {
        let (mut state, _daemon) = opened_writer("base").await;
        focus_editor_buffer(&mut state);
        for code in [
            KeyCode::Char('?'),
            KeyCode::F(5),
            KeyCode::Home,
            KeyCode::End,
        ] {
            assert!(
                matches!(
                    editor_buffer_key(&mut state, key(code)),
                    BufferKey::Unhandled
                ),
                "{code:?} must not be swallowed by the editor"
            );
        }
        assert_eq!(editor_text(&state), "base");
    }

    #[tokio::test]
    async fn a_pending_prefix_swallows_the_next_key_and_resolves_only_known_pairs() {
        let (mut state, _daemon) = opened_writer("one\ntwo\nthree\n").await;
        focus_editor_buffer(&mut state);

        editor_buffer_key(&mut state, key(KeyCode::Char('d')));
        assert_eq!(pending(&state), "d");
        editor_buffer_key(&mut state, key(KeyCode::Char('d')));
        assert_eq!(editor_text(&state), "two\nthree\n");
        assert_eq!(pending(&state), "", "the prefix is consumed");

        // An unknown pair is discarded without touching the text.
        editor_buffer_key(&mut state, key(KeyCode::Char('d')));
        editor_buffer_key(&mut state, key(KeyCode::Char('z')));
        assert_eq!(editor_text(&state), "two\nthree\n");
    }

    #[tokio::test]
    async fn the_pending_prefix_is_bounded() {
        let (mut state, _daemon) = opened_writer("base").await;
        focus_editor_buffer(&mut state);
        editor_buffer_key(&mut state, key(KeyCode::Char('d')));
        assert_eq!(pending(&state), "d");
        // The second `d` resolves the pair and consumes the prefix.
        editor_buffer_key(&mut state, key(KeyCode::Char('d')));
        assert_eq!(pending(&state), "");
        // A third key starts a fresh prefix, which `gg` then resolves.
        editor_buffer_key(&mut state, key(KeyCode::Char('g')));
        assert_eq!(pending(&state), "g");
        editor_buffer_key(&mut state, key(KeyCode::Char('g')));
        assert_eq!(pending(&state), "");
    }

    #[tokio::test]
    async fn backspace_at_offset_zero_is_a_noop() {
        let (mut state, _daemon) = opened_writer("base").await;
        focus_editor_buffer(&mut state);
        editor_buffer_key(&mut state, key(KeyCode::Char('i')));
        assert!(matches!(
            editor_buffer_key(&mut state, key(KeyCode::Backspace)),
            BufferKey::Consumed
        ));
        assert_eq!(editor_text(&state), "base");
        assert_eq!(undo_depth(&state), 0, "a no-op edit records no history");
    }

    // ── Focus ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn focus_moves_between_the_buffer_and_the_composer_without_touching_text() {
        let (mut state, _daemon) = opened_writer("base").await;
        focus_editor_buffer(&mut state);
        assert!(state.buffer_focused());
        focus_editor_composer(&mut state);
        assert!(!state.buffer_focused());
        assert_eq!(editor_text(&state), "base");
        assert_eq!(undo_depth(&state), 0);
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
            editor_buffer_key(&mut state, normal_key(crossterm::event::KeyCode::Char(':')))
                == BufferKey::Consumed
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
        assert!(
            editor_buffer_key(&mut state, normal_key(crossterm::event::KeyCode::Char(':')))
                == BufferKey::Consumed
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
            editor_buffer_key(&mut state, ctrl_s) != BufferKey::RequestSave,
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
        let result = editor_buffer_key(&mut state, normal_key(crossterm::event::KeyCode::Esc));
        // Leave-buffer, never RunCommand: `Esc` must be the side-effect-free
        // exit.
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
        let result = editor_buffer_key(&mut state, normal_key(crossterm::event::KeyCode::Enter));
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
        let result = editor_buffer_key(&mut state, normal_key(crossterm::event::KeyCode::Enter));
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
        editor_buffer_key(&mut state, normal_key(crossterm::event::KeyCode::Char('q')));
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
            assert!(editor_buffer_key(&mut state, backspace) == BufferKey::Consumed);
        }
        assert!(state
            .session
            .as_ref()
            .unwrap()
            .presentation()
            .command_line
            .is_empty());
    }
}
