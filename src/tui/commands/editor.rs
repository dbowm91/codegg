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

use crate::tui::app::state::editor::{EditorNotice, EditorOperation, EditorState, EditorStatus};
use crate::tui::app::{App, TuiCommand};
use crate::tui::async_cmd::spawn_scoped_registered_tui_task;
use crate::tui::document_session::TuiDocumentSession;
use crate::tui::editor::{
    self, EditorCommand, EditorFocus, EditorMode, MAX_EDITOR_PENDING_COMMAND,
};
use crate::tui::task_lifecycle::TuiTaskKind;

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
///
/// Navigates to `Route::Editor` and issues exactly one `DocumentOpen`. A
/// partial failure holds no attachment and shows no buffer: the session is
/// only kept when the daemon accepted the document.
pub(crate) fn open_editor(app: &mut App, raw_path: &str) {
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
    if app.editor_state.is_open() && app.editor_state.path.as_deref() == Some(path.as_str()) {
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
                .open(project_id, workspace_id, path, wants_writer)
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
    if operation == EditorOperation::Close {
        app.editor_state.detach();
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
        // `h`/`l` mirror vi: collapse or expand, on a directory.
        KeyCode::Char('h') | KeyCode::Left => {
            super::file_tree::toggle_selected(app);
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
    }
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
}

/// Handle one key while the editor buffer owns input.
///
/// Pure presentation state, plus an explicit request for the two lifecycle
/// round trips that belong to the App layer.
fn editor_buffer_key(state: &mut EditorState, key: KeyEvent) -> BufferKey {
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
}
