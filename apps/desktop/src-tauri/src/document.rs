//! The desktop's document session (M007-E).
//!
//! # What this module owns
//!
//! The editor's substrate already exists and is shared: the frontend-neutral
//! [`DocumentController`] owns the canonical text replica, and the daemon owns
//! the document itself. This module is the desktop's *host* half of the
//! editor. It holds:
//!
//! - the [`DocumentController`] the editor edits through;
//! - the bounded undo stack of inverse transactions;
//! - the cursor, the selection and the viewport window;
//! - the mapping from the controller's state to a bounded presentation.
//!
//! # What this module deliberately does not own
//!
//! - **The document text.** The host never keeps a second copy of it. The
//!   viewport payload is a bounded slice of the controller's own snapshot,
//!   cut with [`DocumentSnapshot::line_range`]; the whole document is never
//!   materialised, in the host or in the renderer.
//! - **Any absolute path.** Every path the editor displays is
//!   workspace-relative and is validated before it is used.
//! - **The editor's key bindings.** They are the renderer's *actions*, and
//!   they are declared in the canonical keymap table
//!   (`apps/desktop/src/keymap.ts`) under the editor's scope. The host owns
//!   no chord and resolves none.
//!
//! # The client
//!
//! The controller's transport and the host's other commands share one daemon
//! connection. [`LocalSocketClient`] is a handle over `Arc`-backed internals,
//! so the controller's transport is a clone of the installed client and
//! speaks over the same socket, the same request multiplexer and the same
//! reconnect state. A daemon reconnect installs a new client, and the
//! controller's transport is replaced with it through
//! [`DocumentController::reconnect`], which reopens the attachment at the
//! same scope.

use std::collections::VecDeque;
use std::sync::Arc;

use codegg_client::{
    DiagnosticsStreamUpdate, DocumentController, DocumentState, DocumentTransport,
    LspDiagnosticsStream,
};
use codegg_document::{DocumentSnapshot, TextEdit, TextTransaction};
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::document::{
    DocumentScope, WorkspaceDirectoryListRequestDto, WorkspaceDirectoryNodeDto,
    WorkspaceDirectoryNodeKindDto,
};
use codegg_protocol::lsp::{
    LspCompletionApplicabilityDto, LspCompletionRequestDto, LspCompletionStatus,
    LspDiagnosticsStatus, LspReadOperation, LspReadRequestDto, LspReadStatus,
};
use serde::Serialize;
use std::time::Duration;
use tauri::{ipc::Channel, State};
use tokio::sync::Mutex;

use crate::{lifecycle::cancel_and_join, HostState};

/// Maximum number of visible lines one viewport slice may carry.
///
/// The viewport is the *only* place document text crosses the bridge, and it
/// is bounded here rather than by the caller: a renderer that asks for a
/// million lines gets this many.
pub const MAX_VIEWPORT_LINES: usize = 200;

/// Maximum number of undo entries the host retains.
pub const MAX_EDITOR_UNDO_DEPTH: usize = 256;

/// Maximum number of bytes the undo and redo stacks retain together.
pub const MAX_EDITOR_UNDO_BYTES: usize = 4 * 1024 * 1024;

/// Maximum number of bytes one inserted edit may carry.
pub const MAX_EDITOR_INSERT_BYTES: usize = 64 * 1024;

/// Maximum number of characters one workspace-relative path may carry.
pub const MAX_EDITOR_PATH_BYTES: usize = 1024;

/// Maximum number of tree entries one listing may carry.
pub const MAX_EDITOR_TREE_NODES: usize = 400;

/// Maximum number of diagnostics one response may carry.
pub const MAX_EDITOR_DIAGNOSTICS: usize = 200;

/// Maximum number of completion candidates one response may carry.
pub const MAX_EDITOR_COMPLETIONS: usize = 50;

/// Maximum number of navigation locations one response may carry.
pub const MAX_EDITOR_LOCATIONS: usize = 50;

/// The viewport's default height, in lines.
pub const DEFAULT_VIEWPORT_LINES: usize = 40;

// ── The bounded DTOs the host serves ─────────────────────────────────────

/// One bounded slice of the open document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorViewportView {
    /// The workspace-relative path of the open document.
    pub relative_path: String,
    /// The document's current revision.
    pub revision: u64,
    /// The first visible line, zero-based.
    pub first_line: usize,
    /// How many lines this slice carries.
    pub line_count: usize,
    /// How many lines the document has.
    pub total_lines: usize,
    /// The document's byte length.
    pub byte_length: usize,
    /// The visible lines, bounded by [`MAX_VIEWPORT_LINES`].
    ///
    /// This is the one bounded slice of document text that crosses the
    /// bridge. It is cut from the controller's own snapshot; the whole
    /// document is never materialised.
    pub visible_lines: Vec<String>,
    /// The caret's byte offset.
    pub caret: usize,
    /// The selection anchor's byte offset. Equal to the caret when there is
    /// no selection.
    pub anchor: usize,
    /// The caret's zero-based line.
    pub caret_line: u32,
    /// The caret's zero-based column, in UTF-16 code units.
    pub caret_column: u32,
    /// The anchor's zero-based line.
    pub anchor_line: u32,
    /// The anchor's zero-based column, in UTF-16 code units.
    pub anchor_column: u32,
}

/// The result of opening a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorOpenView {
    /// The workspace-relative path of the open document.
    pub relative_path: String,
    /// The document's current revision.
    pub revision: u64,
    /// The presentation key of the controller's state.
    pub state: String,
    /// The human message for that key.
    pub message: String,
    /// Whether the host holds a writer lease on the document.
    pub writable: bool,
    /// How many lines the document has.
    pub total_lines: usize,
    /// The document's byte length.
    pub byte_length: usize,
}

/// The presentation of the controller's state, without any text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorStateView {
    /// The presentation key of the controller's state.
    pub state: String,
    /// The human message for that key.
    pub message: String,
    /// The workspace-relative path of the open document, when one is open.
    pub relative_path: Option<String>,
    /// The document's revision, when one is attached.
    pub revision: Option<u64>,
    /// Whether the host holds a writer lease.
    pub writable: bool,
    /// Whether the local replica differs from the daemon's.
    pub dirty: bool,
    /// Whether the daemon reports a disk conflict.
    pub conflicted: bool,
    /// How many local changes are queued for the daemon.
    pub pending_changes: usize,
    /// Whether an undo is available.
    pub can_undo: bool,
    /// Whether a redo is available.
    pub can_redo: bool,
}

/// The result of one edit, undo or redo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorEditView {
    /// The caret's byte offset after the edit.
    pub caret: usize,
    /// The selection anchor's byte offset after the edit.
    pub anchor: usize,
    /// The document's revision after the edit.
    pub revision: u64,
    /// How many undo entries remain.
    pub undo_depth: usize,
    /// How many redo entries remain.
    pub redo_depth: usize,
}

/// One entry of the workspace file tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTreeNodeView {
    /// The entry's name.
    pub name: String,
    /// The entry's workspace-relative path.
    pub relative_path: String,
    /// How deep the entry sits below the listed prefix.
    pub depth: usize,
    /// `directory` or `file`.
    pub kind: String,
    /// The entry's children, for a directory.
    pub children: Vec<EditorTreeNodeView>,
}

/// The workspace file tree the editor's open affordance lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTreeView {
    /// The workspace the listing belongs to.
    pub workspace_id: String,
    /// The listed prefix, empty for the workspace root.
    pub prefix: String,
    /// `ready`, `busy` or `unavailable`.
    pub status: String,
    /// The human message for that status.
    pub message: String,
    /// The entries, bounded by [`MAX_EDITOR_TREE_NODES`].
    pub nodes: Vec<EditorTreeNodeView>,
    /// Whether the listing was trimmed.
    pub truncated: bool,
}

/// One diagnostic entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorDiagnosticView {
    /// The diagnostic's workspace-relative path.
    pub relative_path: String,
    /// The zero-based start line.
    pub line: u32,
    /// The zero-based start column, in UTF-16 code units.
    pub start_character: u32,
    /// The zero-based end column, in UTF-16 code units.
    pub end_character: u32,
    /// The LSP severity, as an integer (1 is an error).
    pub severity: u32,
    /// The diagnostic's message.
    pub message: String,
    /// The server that produced it.
    pub server: Option<String>,
}

/// The project's diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorDiagnosticsView {
    /// `ready` or `no_warm_server`.
    ///
    /// `no_warm_server` is **not** an empty diagnostic set: it means the
    /// daemon has no warm language server to answer with, and the renderer
    /// must not present it as "this file is clean".
    pub status: String,
    /// The human message for that status.
    pub message: String,
    /// The diagnostics, bounded by [`MAX_EDITOR_DIAGNOSTICS`].
    pub diagnostics: Vec<EditorDiagnosticView>,
    /// Whether the set was trimmed.
    pub truncated: bool,
    /// The last complete set is retained while a stream gap is reconciled.
    pub stale: bool,
}

/// One inert completion candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCompletionCandidateView {
    /// The candidate's label.
    pub label: String,
    /// The candidate's detail line.
    pub detail: Option<String>,
    /// The candidate's kind, as a stable lowercase name.
    pub kind: Option<String>,
    /// Whether the candidate may be applied.
    pub applicable: bool,
    /// Why an inapplicable candidate cannot be applied.
    pub reason: Option<String>,
}

/// The completion candidates for the caret's position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCompletionView {
    /// `ready`, `no_warm_server`, `stale`, `unsynced` or `unsupported`.
    pub status: String,
    /// The human message for that status.
    pub message: String,
    /// The candidates, bounded by [`MAX_EDITOR_COMPLETIONS`].
    pub candidates: Vec<EditorCompletionCandidateView>,
    /// Whether the server trimmed the list.
    pub truncated: bool,
}

/// One navigation target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorLocationView {
    /// The target's workspace-relative path.
    pub relative_path: String,
    /// The zero-based line.
    pub line: u32,
    /// The zero-based column, in UTF-16 code units.
    pub character: u32,
}

/// The result of one navigation read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorNavigationView {
    /// `ready`, `no_warm_server`, `ambiguous` or `unsupported`.
    pub status: String,
    /// The human message for that status.
    pub message: String,
    /// The targets, bounded by [`MAX_EDITOR_LOCATIONS`].
    pub locations: Vec<EditorLocationView>,
    /// Whether the server trimmed the list.
    pub truncated: bool,
}

// ── The frontend-owned, bounded state ─────────────────────────────────────

/// The caret and the selection anchor, as byte offsets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CursorState {
    /// The caret's byte offset.
    pub caret: usize,
    /// The selection anchor's byte offset.
    pub anchor: usize,
}

/// The visible line window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportWindow {
    /// The first visible line, zero-based.
    pub first_line: usize,
    /// How many lines the viewport wants to show.
    pub line_count: usize,
}

impl Default for ViewportWindow {
    fn default() -> Self {
        Self {
            first_line: 0,
            line_count: DEFAULT_VIEWPORT_LINES,
        }
    }
}

/// The bounded undo and redo stacks.
///
/// Only *inverse* transactions are retained: the forward transaction is
/// reconstructed from the snapshot when the entry is taken back, so the stack
/// never holds the inserted text as well as the removed text.
#[derive(Debug, Default)]
pub struct UndoStack {
    undo: VecDeque<TextTransaction>,
    redo: VecDeque<TextTransaction>,
    bytes: usize,
}

impl UndoStack {
    /// Record an inverse transaction. A new edit invalidates the redo stack.
    pub fn push_undo(&mut self, inverse: TextTransaction) {
        self.clear_redo();
        self.bytes = self.bytes.saturating_add(transaction_bytes(&inverse));
        self.undo.push_back(inverse);
        self.trim();
    }

    /// Record a forward transaction for a redo.
    pub fn push_redo(&mut self, forward: TextTransaction) {
        self.bytes = self.bytes.saturating_add(transaction_bytes(&forward));
        self.redo.push_back(forward);
        self.trim();
    }

    /// Take the newest undo entry.
    pub fn take_undo(&mut self) -> Option<TextTransaction> {
        let entry = self.undo.pop_back()?;
        self.bytes = self.bytes.saturating_sub(transaction_bytes(&entry));
        Some(entry)
    }

    /// Take the newest redo entry.
    pub fn take_redo(&mut self) -> Option<TextTransaction> {
        let entry = self.redo.pop_back()?;
        self.bytes = self.bytes.saturating_sub(transaction_bytes(&entry));
        Some(entry)
    }

    /// Drop every entry. A reconciliation moves the base, so the history
    /// that was recorded against the old base is no longer applicable.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }

    /// How many undo entries remain.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// How many redo entries remain.
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    fn clear_redo(&mut self) {
        for entry in self.redo.drain(..) {
            self.bytes = self.bytes.saturating_sub(transaction_bytes(&entry));
        }
    }

    fn trim(&mut self) {
        while self.undo.len() > MAX_EDITOR_UNDO_DEPTH || self.bytes > MAX_EDITOR_UNDO_BYTES {
            match self.undo.pop_front() {
                Some(entry) => self.bytes = self.bytes.saturating_sub(transaction_bytes(&entry)),
                None => break,
            }
        }
        while self.redo.len() > MAX_EDITOR_UNDO_DEPTH || self.bytes > MAX_EDITOR_UNDO_BYTES {
            match self.redo.pop_front() {
                Some(entry) => self.bytes = self.bytes.saturating_sub(transaction_bytes(&entry)),
                None => break,
            }
        }
    }
}

/// The bytes one transaction retains.
fn transaction_bytes(transaction: &TextTransaction) -> usize {
    transaction.edits.iter().map(|edit| edit.insert.len()).sum()
}

/// The desktop's document session.
///
/// The controller is the shared substrate; everything else here is the
/// frontend-owned, bounded state the editor needs on top of it.
pub struct DesktopDocumentSession {
    controller: Arc<DocumentController>,
    frontend: Mutex<FrontendState>,
}

#[derive(Debug, Default)]
struct FrontendState {
    cursor: CursorState,
    viewport: ViewportWindow,
    undo: UndoStack,
    relative_path: String,
    connection_generation: u64,
}

impl DesktopDocumentSession {
    /// Start a session on a controller.
    pub fn new(controller: Arc<DocumentController>, relative_path: String) -> Self {
        Self {
            controller,
            frontend: Mutex::new(FrontendState {
                relative_path,
                viewport: ViewportWindow::default(),
                ..FrontendState::default()
            }),
        }
    }

    /// The controller the editor edits through.
    pub fn controller(&self) -> Arc<DocumentController> {
        Arc::clone(&self.controller)
    }

    /// The controller's current snapshot and revision.
    pub fn snapshot(&self) -> Result<(DocumentSnapshot, u64), String> {
        self.controller
            .try_snapshot()
            .ok_or_else(|| "no document is open".to_owned())
    }

    /// The workspace-relative path of the open document.
    pub async fn relative_path(&self) -> String {
        self.frontend.lock().await.relative_path.clone()
    }

    /// Reattach the controller after a daemon reconnect.
    ///
    /// The attachment reopens at the same scope, and the history is dropped:
    /// it was recorded against a base the daemon has moved past.
    pub(crate) async fn ensure_connection(
        self: &Arc<Self>,
        state: &HostState,
    ) -> Result<(), String> {
        let generation = state.current_generation();
        if generation == 0 {
            return Err("desktop is not connected".to_owned());
        }
        if generation == self.frontend.lock().await.connection_generation {
            return Ok(());
        }
        let client = state
            .client
            .lock()
            .await
            .clone()
            .ok_or_else(|| "desktop is not connected".to_owned())?;
        let transport: Arc<dyn DocumentTransport> = Arc::new(client);
        self.controller
            .reconnect(transport)
            .await
            .map_err(|error| format!("the document could not be reattached: {error}"))?;
        let mut frontend = self.frontend.lock().await;
        frontend.connection_generation = generation;
        frontend.undo.clear();
        Ok(())
    }

    /// The bounded viewport slice of the open document.
    pub async fn viewport(
        &self,
        first_line: Option<usize>,
        line_count: Option<usize>,
    ) -> Result<EditorViewportView, String> {
        let (snapshot, revision) = self.snapshot()?;
        let total_lines = snapshot.len_lines();
        let (window, cursor) = {
            let mut frontend = self.frontend.lock().await;
            if let Some(first) = first_line {
                frontend.viewport.first_line = first;
            }
            if let Some(count) = line_count {
                frontend.viewport.line_count = count;
            }
            frontend.viewport.first_line = frontend
                .viewport
                .first_line
                .min(total_lines.saturating_sub(1));
            frontend.viewport.line_count =
                frontend.viewport.line_count.clamp(1, MAX_VIEWPORT_LINES);
            frontend.cursor.caret = clamp_offset(&snapshot, frontend.cursor.caret);
            frontend.cursor.anchor = clamp_offset(&snapshot, frontend.cursor.anchor);
            (frontend.viewport, frontend.cursor)
        };
        let visible_lines = slice_lines(&snapshot, window.first_line, window.line_count);
        let (caret_line, caret_column) = display_position(&snapshot, cursor.caret)?;
        let (anchor_line, anchor_column) = display_position(&snapshot, cursor.anchor)?;
        Ok(EditorViewportView {
            relative_path: self.relative_path().await,
            revision,
            first_line: window.first_line,
            line_count: visible_lines.len(),
            total_lines,
            byte_length: snapshot.len_bytes(),
            visible_lines,
            caret: cursor.caret,
            anchor: cursor.anchor,
            caret_line,
            caret_column,
            anchor_line,
            anchor_column,
        })
    }

    /// Move the caret by one named movement.
    ///
    /// The host owns the byte arithmetic: the renderer names a movement and
    /// never computes a byte offset of its own, so a multi-byte character
    /// can never be split by the renderer.
    pub async fn move_cursor(
        &self,
        movement: &str,
        extend: bool,
    ) -> Result<EditorViewportView, String> {
        let (snapshot, _) = self.snapshot()?;
        let (caret, page) = {
            let frontend = self.frontend.lock().await;
            (frontend.cursor.caret, frontend.viewport.line_count.max(1))
        };
        let moved = move_caret(&snapshot, caret, movement, page)?;
        let mut frontend = self.frontend.lock().await;
        frontend.cursor.caret = moved;
        if !extend {
            frontend.cursor.anchor = moved;
        }
        drop(frontend);
        self.viewport(None, None).await
    }

    /// Apply one edit and record its inverse.
    pub async fn edit(
        &self,
        start: usize,
        end: usize,
        insert: &str,
    ) -> Result<EditorEditView, String> {
        if insert.len() > MAX_EDITOR_INSERT_BYTES {
            return Err("the edit is larger than the host accepts".to_owned());
        }
        let (snapshot, _) = self.snapshot()?;
        let start = clamp_start(&snapshot, start);
        let end = clamp_end(&snapshot, start, end);
        let removed = snapshot
            .read_bytes(start, end)
            .map_err(|error| format!("the edit range is not readable: {error}"))?;
        let transaction = TextTransaction::new(vec![TextEdit::new(start..end, insert)]);
        self.controller
            .apply_local(transaction)
            .map_err(|error| format!("the edit was refused: {error}"))?;
        let inverse =
            TextTransaction::new(vec![TextEdit::new(start..start + insert.len(), removed)]);
        {
            let mut frontend = self.frontend.lock().await;
            frontend.undo.push_undo(inverse);
            frontend.cursor.caret = start + insert.len();
            frontend.cursor.anchor = frontend.cursor.caret;
        }
        Ok(self.edit_view().await)
    }

    /// Undo the newest edit.
    pub async fn undo(&self) -> Result<EditorEditView, String> {
        let (snapshot, _) = self.snapshot()?;
        let entry = {
            let mut frontend = self.frontend.lock().await;
            frontend
                .undo
                .take_undo()
                .ok_or_else(|| "there is nothing to undo".to_owned())?
        };
        let (start, end, removed) = {
            let edit = entry
                .edits
                .first()
                .ok_or_else(|| "the undo entry is empty".to_owned())?;
            (edit.range.start, edit.range.end, edit.insert.len())
        };
        let inserted = snapshot
            .read_bytes(start, end)
            .map_err(|error| format!("the undo range is not readable: {error}"))?;
        let forward = TextTransaction::new(vec![TextEdit::new(start..start + removed, inserted)]);
        self.controller
            .apply_local(entry)
            .map_err(|error| format!("the undo was refused: {error}"))?;
        {
            let mut frontend = self.frontend.lock().await;
            frontend.undo.push_redo(forward);
            frontend.cursor.caret = start;
            frontend.cursor.anchor = start;
        }
        Ok(self.edit_view().await)
    }

    /// Redo the newest undone edit.
    pub async fn redo(&self) -> Result<EditorEditView, String> {
        let (snapshot, _) = self.snapshot()?;
        let entry = {
            let mut frontend = self.frontend.lock().await;
            frontend
                .undo
                .take_redo()
                .ok_or_else(|| "there is nothing to redo".to_owned())?
        };
        let (start, end, inserted) = {
            let edit = entry
                .edits
                .first()
                .ok_or_else(|| "the redo entry is empty".to_owned())?;
            (edit.range.start, edit.range.end, edit.insert.len())
        };
        let removed = snapshot
            .read_bytes(start, end)
            .map_err(|error| format!("the redo range is not readable: {error}"))?;
        let inverse = TextTransaction::new(vec![TextEdit::new(start..start + inserted, removed)]);
        self.controller
            .apply_local(entry)
            .map_err(|error| format!("the redo was refused: {error}"))?;
        {
            let mut frontend = self.frontend.lock().await;
            frontend.undo.push_undo(inverse);
            frontend.cursor.caret = end;
            frontend.cursor.anchor = end;
        }
        Ok(self.edit_view().await)
    }

    /// The bounded presentation of the controller's state.
    pub async fn state_view(&self) -> EditorStateView {
        let state = self.controller.state().await;
        let (key, message) = present_state(&state);
        let attachment = self.controller.try_attachment_info();
        let frontend = self.frontend.lock().await;
        EditorStateView {
            state: key.to_owned(),
            message: message.to_owned(),
            relative_path: if attachment.is_some() {
                Some(frontend.relative_path.clone())
            } else {
                None
            },
            revision: attachment.as_ref().map(|info| info.revision),
            writable: attachment.as_ref().is_some_and(|info| info.is_writer),
            dirty: attachment.as_ref().is_some_and(|info| info.dirty),
            conflicted: attachment.as_ref().is_some_and(|info| info.conflicted),
            pending_changes: attachment
                .as_ref()
                .map(|info| info.pending_changes)
                .unwrap_or(0),
            can_undo: frontend.undo.undo_depth() > 0,
            can_redo: frontend.undo.redo_depth() > 0,
        }
    }

    /// Drop the history after a reconciliation moved the base.
    pub async fn forget_history(&self) {
        self.frontend.lock().await.undo.clear();
    }

    async fn edit_view(&self) -> EditorEditView {
        let frontend = self.frontend.lock().await;
        EditorEditView {
            caret: frontend.cursor.caret,
            anchor: frontend.cursor.anchor,
            revision: self
                .controller
                .try_snapshot()
                .map(|(_, revision)| revision)
                .unwrap_or(0),
            undo_depth: frontend.undo.undo_depth(),
            redo_depth: frontend.undo.redo_depth(),
        }
    }
}

/// The presentation key and message for one controller state.
pub fn present_state(state: &DocumentState) -> (&'static str, &'static str) {
    match state {
        DocumentState::Closed => ("closed", "no document is open"),
        DocumentState::Opening => ("opening", "the document is opening"),
        DocumentState::Synced => ("synced", "the document is in sync with the daemon"),
        DocumentState::DirtyLocal => ("dirty_local", "the document has unsaved local changes"),
        DocumentState::Flushing => ("flushing", "the local changes are being sent to the daemon"),
        DocumentState::ResyncRequired => (
            "resync_required",
            "the document needs an authoritative resync before it can be edited",
        ),
        DocumentState::Conflict => ("conflict", "the daemon reports a conflict on this document"),
        DocumentState::ReadOnly => (
            "read_only",
            "this document is read-only for the current session",
        ),
        DocumentState::Disconnected => ("disconnected", "the daemon connection was lost"),
        DocumentState::GoneWithLocalDraft => (
            "gone_with_local_draft",
            "the document is gone on disk and a local draft is retained",
        ),
    }
}

/// Clamp an offset to the document, and to a UTF-8 byte boundary.
fn clamp_offset(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut candidate = offset.min(snapshot.len_bytes());
    while candidate > 0 && !snapshot.is_byte_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// The byte offset of a replacement's start, clamped **down**.
///
/// A start that lands inside a character moves to that character's start, so
/// the range covers whole characters.
fn clamp_start(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    clamp_offset(snapshot, offset)
}

/// The byte offset of a replacement's end, clamped **up**.
///
/// An end that lands inside a character moves to that character's end, so a
/// range never splits a character. A pure insertion (start == end) clamps
/// both ends to the same boundary instead, so an insert never replaces the
/// character it landed in.
fn clamp_end(snapshot: &DocumentSnapshot, start: usize, end: usize) -> usize {
    if end <= start {
        return clamp_start(snapshot, start);
    }
    let mut candidate = end.min(snapshot.len_bytes());
    while candidate < snapshot.len_bytes() && !snapshot.is_byte_boundary(candidate) {
        candidate += 1;
    }
    candidate
}

/// The start byte offset of the character before `offset`.
fn previous_boundary(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut candidate = offset.min(snapshot.len_bytes());
    if candidate == 0 {
        return 0;
    }
    candidate -= 1;
    while candidate > 0 && !snapshot.is_byte_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// The end byte offset of the character at `offset`.
fn next_boundary(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut candidate = offset.min(snapshot.len_bytes());
    while candidate < snapshot.len_bytes() && !snapshot.is_byte_boundary(candidate) {
        candidate += 1;
    }
    if candidate < snapshot.len_bytes() {
        candidate += 1;
        while candidate < snapshot.len_bytes() && !snapshot.is_byte_boundary(candidate) {
            candidate += 1;
        }
    }
    candidate
}

/// The byte offset just after the last character of `line`, excluding the
/// line terminator.
fn line_content_end(snapshot: &DocumentSnapshot, line: usize) -> usize {
    let Ok(range) = snapshot.line_range(line) else {
        return snapshot.len_bytes();
    };
    let Ok(text) = snapshot.read_bytes(range.start, range.end) else {
        return range.start;
    };
    let trimmed = text.trim_end_matches('\n').trim_end_matches('\r');
    range.start + trimmed.len()
}

/// The byte offset of `line`'s start.
fn line_start(snapshot: &DocumentSnapshot, line: usize) -> usize {
    snapshot
        .line_range(line)
        .map(|range| range.start)
        .unwrap_or(0)
}

/// The caret's display position: the zero-based line and the UTF-16 column.
fn display_position(snapshot: &DocumentSnapshot, offset: usize) -> Result<(u32, u32), String> {
    let position = snapshot
        .byte_to_position(clamp_offset(snapshot, offset))
        .map_err(|error| format!("the caret is not at a readable position: {error}"))?;
    let range = snapshot
        .line_range(position.line)
        .map_err(|error| format!("the caret's line is not readable: {error}"))?;
    let column = snapshot
        .utf16_column_bounded(range.start, clamp_offset(snapshot, offset))
        .map_err(|error| format!("the caret's column is not readable: {error}"))?;
    Ok((position.line as u32, column))
}

/// Move the caret by one named movement.
///
/// The renderer names a movement; the host owns the byte arithmetic, so a
/// multi-byte character is never split by the renderer.
fn move_caret(
    snapshot: &DocumentSnapshot,
    caret: usize,
    movement: &str,
    page: usize,
) -> Result<usize, String> {
    let position = snapshot
        .byte_to_position(clamp_offset(snapshot, caret))
        .map_err(|error| format!("the caret is not at a readable position: {error}"))?;
    let line = position.line;
    let column = position.byte_column;
    let moved = match movement {
        "left" => previous_boundary(snapshot, caret),
        "right" => next_boundary(snapshot, caret),
        "home" => line_start(snapshot, line),
        "end" => line_content_end(snapshot, line),
        "up" => move_line(snapshot, line, column, -1),
        "down" => move_line(snapshot, line, column, 1),
        "page_up" => move_line(snapshot, line, column, -(page as isize)),
        "page_down" => move_line(snapshot, line, column, page as isize),
        other => return Err(format!("`{other}` is not a movement this host serves")),
    };
    Ok(moved)
}

/// The byte offset of the same column on another line.
fn move_line(snapshot: &DocumentSnapshot, line: usize, column: usize, delta: isize) -> usize {
    let total_lines = snapshot.len_lines();
    if total_lines == 0 {
        return 0;
    }
    let target = if delta < 0 {
        line.saturating_sub(delta.unsigned_abs())
    } else {
        line.saturating_add(delta as usize).min(total_lines - 1)
    };
    let target = target.min(total_lines - 1);
    let start = line_start(snapshot, target);
    let end = line_content_end(snapshot, target);
    (start + column).min(end)
}

/// Cut the visible line window out of the controller's own snapshot.
///
/// The window is bounded by [`MAX_VIEWPORT_LINES`], so the whole document is
/// never materialised: each visible line is read from the snapshot's line
/// range and nothing else is touched.
pub fn slice_lines(
    snapshot: &DocumentSnapshot,
    first_line: usize,
    line_count: usize,
) -> Vec<String> {
    let total_lines = snapshot.len_lines();
    let start = first_line.min(total_lines.saturating_sub(1));
    let count = line_count.clamp(1, MAX_VIEWPORT_LINES);
    let mut lines = Vec::with_capacity(count);
    for index in start..start.saturating_add(count) {
        if index >= total_lines {
            break;
        }
        let Ok(range) = snapshot.line_range(index) else {
            break;
        };
        let Ok(text) = snapshot.read_bytes(range.start, range.end) else {
            break;
        };
        lines.push(
            text.trim_end_matches('\n')
                .trim_end_matches('\r')
                .to_owned(),
        );
    }
    lines
}

/// Validate a workspace-relative path.
///
/// The daemon revalidates containment on every open, but the renderer must
/// not even be able to name an absolute path: every path the editor shows is
/// relative to the session's workspace.
pub fn safe_relative_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("the path is empty".to_owned());
    }
    if trimmed.len() > MAX_EDITOR_PATH_BYTES {
        return Err("the path is longer than the host accepts".to_owned());
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err("the path must be workspace-relative".to_owned());
    }
    if trimmed.contains('\\') {
        return Err("the path must use `/` separators".to_owned());
    }
    for component in trimmed.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err("the path must not contain empty or `.`/`..` components".to_owned());
        }
    }
    Ok(trimmed.to_owned())
}

/// The navigation operation a renderer may ask for.
///
/// A closed set, so the renderer cannot name an operation the host does not
/// support.
fn navigation_operation(name: &str) -> Option<LspReadOperation> {
    match name {
        "definition" => Some(LspReadOperation::Definition),
        "declaration" => Some(LspReadOperation::Declaration),
        "implementation" => Some(LspReadOperation::Implementation),
        "references" => Some(LspReadOperation::References),
        _ => None,
    }
}

// ── The commands ─────────────────────────────────────────────────────────

/// Open one workspace-relative document.
#[tauri::command]
pub(crate) async fn desktop_document_open(
    state: State<'_, Arc<HostState>>,
    path: String,
    route_generation: u64,
) -> Result<EditorOpenView, String> {
    desktop_document_open_with_state(state.inner(), path, route_generation).await
}

pub(crate) async fn desktop_document_open_with_state(
    state: &Arc<HostState>,
    path: String,
    route_generation: u64,
) -> Result<EditorOpenView, String> {
    let (project_id, workspace_id, _) = state.route_selection(route_generation, true).await?;
    let relative_path = safe_relative_path(&path)?;
    // A second open closes the first. The daemon's writer lease belongs to
    // the attachment, so a dropped controller would leave the lease held
    // until the daemon expires it — and a document with unsaved changes
    // would lose them silently.
    if let Some(previous) = state.document.lock().await.clone() {
        previous.controller().close().await.map_err(|error| {
            format!("the open document could not be closed; save or close it first: {error}")
        })?;
    }
    let client = state
        .client
        .lock()
        .await
        .clone()
        .ok_or_else(|| "desktop is not connected".to_owned())?;
    let transport: Arc<dyn DocumentTransport> = Arc::new(client);
    let controller = Arc::new(DocumentController::new(transport));
    controller
        .open(
            project_id,
            workspace_id,
            relative_path.clone(),
            true,
            DocumentScope::Workspace,
        )
        .await
        .map_err(|error| format!("the document could not be opened: {error}"))?;
    let session = Arc::new(DesktopDocumentSession::new(
        controller,
        relative_path.clone(),
    ));
    *state.document.lock().await = Some(Arc::clone(&session));
    let (snapshot, revision) = session
        .snapshot()
        .map_err(|error| format!("the document could not be read: {error}"))?;
    let state_view = session.state_view().await;
    Ok(EditorOpenView {
        relative_path,
        revision,
        state: state_view.state,
        message: state_view.message,
        writable: state_view.writable,
        total_lines: snapshot.len_lines(),
        byte_length: snapshot.len_bytes(),
    })
}

/// Read one bounded viewport slice of the open document.
#[tauri::command]
pub(crate) async fn desktop_document_viewport(
    state: State<'_, Arc<HostState>>,
    first_line: Option<usize>,
    line_count: Option<usize>,
) -> Result<EditorViewportView, String> {
    let session = live_session(&state).await?;
    session.viewport(first_line, line_count).await
}

/// Move the caret by one named movement.
#[tauri::command]
pub(crate) async fn desktop_document_cursor(
    state: State<'_, Arc<HostState>>,
    movement: String,
    extend: bool,
) -> Result<EditorViewportView, String> {
    let session = live_session(&state).await?;
    session.move_cursor(&movement, extend).await
}

/// Apply one edit to the open document.
#[tauri::command]
pub(crate) async fn desktop_document_edit(
    state: State<'_, Arc<HostState>>,
    start: usize,
    end: usize,
    insert: String,
) -> Result<EditorEditView, String> {
    let session = live_session(&state).await?;
    session.edit(start, end, &insert).await
}

/// Undo the newest edit.
#[tauri::command]
pub(crate) async fn desktop_document_undo(
    state: State<'_, Arc<HostState>>,
) -> Result<EditorEditView, String> {
    let session = live_session(&state).await?;
    session.undo().await
}

/// Redo the newest undone edit.
#[tauri::command]
pub(crate) async fn desktop_document_redo(
    state: State<'_, Arc<HostState>>,
) -> Result<EditorEditView, String> {
    let session = live_session(&state).await?;
    session.redo().await
}

/// Save the open document.
#[tauri::command]
pub(crate) async fn desktop_document_save(
    state: State<'_, Arc<HostState>>,
) -> Result<EditorStateView, String> {
    let session = live_session(&state).await?;
    let controller = session.controller();
    controller
        .save()
        .await
        .map_err(|error| format!("the document could not be saved: {error}"))?;
    session.forget_history().await;
    Ok(session.state_view().await)
}

/// Reload the open document from disk.
#[tauri::command]
pub(crate) async fn desktop_document_reload(
    state: State<'_, Arc<HostState>>,
) -> Result<EditorStateView, String> {
    let session = live_session(&state).await?;
    session
        .controller()
        .reload_from_disk()
        .await
        .map_err(|error| format!("the document could not be reloaded: {error}"))?;
    session.forget_history().await;
    Ok(session.state_view().await)
}

/// Resynchronise the open document with the daemon.
#[tauri::command]
pub(crate) async fn desktop_document_resync(
    state: State<'_, Arc<HostState>>,
) -> Result<EditorStateView, String> {
    let session = live_session(&state).await?;
    session
        .controller()
        .resync()
        .await
        .map_err(|error| format!("the document could not be resynchronised: {error}"))?;
    session.forget_history().await;
    Ok(session.state_view().await)
}

/// Close the open document.
#[tauri::command]
pub(crate) async fn desktop_document_close(
    state: State<'_, Arc<HostState>>,
) -> Result<EditorStateView, String> {
    let session = live_session(&state).await?;
    session
        .controller()
        .close()
        .await
        .map_err(|error| format!("the document could not be closed: {error}"))?;
    *state.document.lock().await = None;
    Ok(EditorStateView {
        state: "closed".to_owned(),
        message: "no document is open".to_owned(),
        relative_path: None,
        revision: None,
        writable: false,
        dirty: false,
        conflicted: false,
        pending_changes: 0,
        can_undo: false,
        can_redo: false,
    })
}

/// Read the open document's presentation state.
#[tauri::command]
pub(crate) async fn desktop_document_state(
    state: State<'_, Arc<HostState>>,
) -> Result<EditorStateView, String> {
    let session = live_session(&state).await?;
    Ok(session.state_view().await)
}

/// List the route-selected workspace's files.
#[tauri::command]
pub(crate) async fn desktop_document_tree(
    state: State<'_, Arc<HostState>>,
    prefix: String,
    route_generation: u64,
) -> Result<EditorTreeView, String> {
    let (project_id, workspace_id, _) = state.route_selection(route_generation, true).await?;
    let request = WorkspaceDirectoryListRequestDto {
        project_id,
        workspace_id: workspace_id.clone(),
        prefix: prefix.clone(),
    };
    let normalized = request
        .validate_prefix()
        .map_err(|error| format!("the listing prefix was refused: {error}"))?;
    let (client, _) = state.route_request().await?;
    let response = client
        .request(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: CoreRequest::WorkspaceDirectoryList { request },
        })
        .await
        .map_err(|error| error.to_string())?;
    let CoreResponse::WorkspaceDirectoryList { result } = response else {
        return Err("the daemon returned an unexpected directory listing".to_owned());
    };
    Ok(tree_view(&result, &workspace_id, &normalized))
}

/// Read the project's diagnostics.
#[tauri::command]
pub(crate) async fn desktop_document_diagnostics(
    state: State<'_, Arc<HostState>>,
    route_generation: u64,
) -> Result<EditorDiagnosticsView, String> {
    desktop_document_diagnostics_with_state(state.inner(), route_generation).await
}

pub(crate) async fn desktop_document_diagnostics_with_state(
    state: &Arc<HostState>,
    route_generation: u64,
) -> Result<EditorDiagnosticsView, String> {
    let (project_id, workspace_id, _) = state.route_selection(route_generation, false).await?;
    let session_id = route_session(state).await?;
    let (client, _) = state.route_request().await?;
    let coordinator_scope = codegg_client::LspRequestScope {
        frontend_id: "desktop".to_owned(),
        attachment_id: format!("route:{route_generation}"),
        route_generation,
        connection_generation: state.current_generation(),
        project_id: project_id.clone(),
        workspace_id: workspace_id.clone(),
        session_id: session_id.clone(),
        document_id: None,
        path: None,
        server_id: None,
    };
    let document_generation = state.current_generation();
    let ticket = match state
        .lsp_request_coordinator
        .begin(codegg_client::LspRequestKey {
            scope: coordinator_scope.clone(),
            operation: codegg_client::LspRequestOperation::Diagnostics,
            document_generation,
            position: None,
            argument: None,
        }) {
        codegg_client::LspRequestStart::Started(ticket) => ticket,
        codegg_client::LspRequestStart::AlreadyInFlight => {
            return Err("a diagnostics request is already in flight".to_owned());
        }
        codegg_client::LspRequestStart::AtCapacity => {
            return Err("the LSP request queue is full".to_owned());
        }
    };
    let cancellation = ticket.cancellation_token();
    let response = tokio::select! {
        _ = cancellation.cancelled() => {
            let _ = state.lsp_request_coordinator.finish(&ticket, &coordinator_scope, document_generation);
            return Err("the diagnostics request was superseded".to_owned());
        },
        response = client.lsp_diagnostics(project_id, session_id) => response,
    }
    .map_err(|error| format!("the diagnostics could not be read: {error}"));
    let (active_project, active_workspace, _) =
        match state.route_selection(route_generation, false).await {
            Ok(selection) => selection,
            Err(_) => {
                let _ = state.lsp_request_coordinator.finish(
                    &ticket,
                    &coordinator_scope,
                    document_generation,
                );
                return Err("the editor route changed during the diagnostics request".to_owned());
            }
        };
    let active_session = match route_session(state).await {
        Ok(session) => session,
        Err(error) => {
            let _ = state.lsp_request_coordinator.finish(
                &ticket,
                &coordinator_scope,
                document_generation,
            );
            return Err(error);
        }
    };
    let active_scope = codegg_client::LspRequestScope {
        frontend_id: "desktop".to_owned(),
        attachment_id: format!("route:{route_generation}"),
        route_generation,
        connection_generation: state.current_generation(),
        project_id: active_project,
        workspace_id: active_workspace,
        session_id: active_session,
        document_id: None,
        path: None,
        server_id: None,
    };
    if !state
        .lsp_request_coordinator
        .finish(&ticket, &active_scope, document_generation)
    {
        return Err("the diagnostics response belongs to a stale editor route".to_owned());
    }
    let result = response?;
    Ok(diagnostics_view(result))
}

/// Attach a loss-aware, project-scoped diagnostics stream for the active
/// editor route. The renderer receives the subscription snapshot first and
/// then complete-file updates; gaps retain the last view as stale while a
/// fresh authorized subscription establishes a new baseline.
#[tauri::command]
pub(crate) async fn desktop_document_diagnostics_subscribe(
    state: State<'_, Arc<HostState>>,
    route_generation: u64,
    channel: Channel<EditorDiagnosticsView>,
) -> Result<String, String> {
    let _subscription_lifecycle = state.lsp_diagnostics_lifecycle.lock().await;
    let (project_id, workspace_id, _) = state.route_selection(route_generation, false).await?;
    let session_id = route_session(&state).await?;
    let (client, _) = state.route_request().await?;
    let connection_generation = state.current_generation();
    let previous_task = state.lsp_diagnostics_subscriptions.take_active().await;
    if let Some((_, task)) = previous_task {
        cancel_and_join(task).await;
    }
    let request_scope = codegg_client::LspRequestScope {
        frontend_id: "desktop".to_owned(),
        attachment_id: format!("route:{route_generation}"),
        route_generation,
        connection_generation,
        project_id: project_id.clone(),
        workspace_id: workspace_id.clone(),
        session_id: session_id.clone(),
        document_id: None,
        path: None,
        server_id: None,
    };
    state.lsp_request_coordinator.invalidate_operation(
        &request_scope,
        codegg_client::LspRequestOperation::Diagnostics,
    );
    if let Some((_, previous_daemon_id)) = state
        .lsp_diagnostics_daemon_subscription
        .lock()
        .await
        .take()
    {
        let _ = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::ProjectionUnsubscribe {
                    subscription_id: previous_daemon_id,
                },
            })
            .await;
    }
    let subscription_id = state
        .lsp_diagnostics_subscriptions
        .allocate_subscription_id();
    let owned_id = subscription_id.clone();
    let task_state = Arc::clone(&state);
    let (task_start, task_wait) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        if task_wait.await.is_err() {
            return;
        }
        let mut events = client.subscribe_events();
        'subscribe: loop {
            if task_state.current_generation() != connection_generation
                || task_state
                    .route_selection(route_generation, false)
                    .await
                    .is_err()
            {
                break;
            }
            let ticket =
                match task_state
                    .lsp_request_coordinator
                    .begin(codegg_client::LspRequestKey {
                        scope: request_scope.clone(),
                        operation: codegg_client::LspRequestOperation::Diagnostics,
                        document_generation: route_generation,
                        position: None,
                        argument: None,
                    }) {
                    codegg_client::LspRequestStart::Started(ticket) => ticket,
                    codegg_client::LspRequestStart::AlreadyInFlight
                    | codegg_client::LspRequestStart::AtCapacity => break,
                };
            let cancellation = ticket.cancellation_token();
            let request = RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::LspDiagnosticsSubscribe {
                    request: codegg_protocol::lsp::LspDiagnosticsSubscribeRequestDto {
                        project_id: project_id.clone(),
                        cursor: None,
                        projection_version:
                            codegg_protocol::projection::PROJECTION_PROTOCOL_VERSION,
                    },
                },
            };
            let response = tokio::select! {
                response = client.request(request) => response,
                _ = cancellation.cancelled() => {
                    let _ = task_state.lsp_request_coordinator.finish(
                        &ticket, &request_scope, route_generation,
                    );
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    let _ = task_state.lsp_request_coordinator.finish(
                        &ticket, &request_scope, route_generation,
                    );
                    let _ = channel.send(unavailable_diagnostics(
                        "the diagnostics subscription timed out".to_owned(),
                    ));
                    break;
                },
            };
            let (daemon_subscription, descriptor, snapshot, cursor) = match response {
                Ok(CoreResponse::ProjectionSubscribed {
                    subscription_id,
                    descriptor,
                    snapshot,
                    cursor,
                    ..
                }) => (subscription_id, descriptor, snapshot, cursor),
                Ok(CoreResponse::Error { message, .. }) => {
                    let _ = channel.send(unavailable_diagnostics(message));
                    break;
                }
                Ok(other) => {
                    let _ = channel.send(unavailable_diagnostics(format!(
                        "unexpected diagnostics subscription response: {other:?}"
                    )));
                    break;
                }
                Err(error) => {
                    let _ = channel.send(unavailable_diagnostics(error.to_string()));
                    break;
                }
            };
            let mut stream = match LspDiagnosticsStream::from_subscription(
                &project_id,
                daemon_subscription.clone(),
                &descriptor,
                &snapshot,
                &cursor,
            ) {
                Ok(stream) => stream,
                Err(_) => {
                    let _ = client
                        .request(RequestEnvelope {
                            protocol_version: PROTOCOL_VERSION,
                            request_id: uuid::Uuid::new_v4().to_string(),
                            payload: CoreRequest::ProjectionUnsubscribe {
                                subscription_id: daemon_subscription,
                            },
                        })
                        .await;
                    break;
                }
            };
            *task_state.lsp_diagnostics_daemon_subscription.lock().await =
                Some((owned_id.clone(), daemon_subscription.clone()));
            let pull = tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = task_state.lsp_request_coordinator.finish(
                        &ticket, &request_scope, route_generation,
                    );
                    let _ = client.request(RequestEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        payload: CoreRequest::ProjectionUnsubscribe {
                            subscription_id: daemon_subscription.clone(),
                        },
                    }).await;
                    clear_diagnostics_daemon_subscription(&task_state, &owned_id).await;
                    break;
                }
                response = client.request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: CoreRequest::LspDiagnosticsGet {
                        request: codegg_protocol::lsp::LspDiagnosticsGetRequestDto {
                            project_id: project_id.clone(),
                            session_id: session_id.clone(),
                        },
                    },
                }) => response,
            };
            let initial = match pull {
                Ok(CoreResponse::LspDiagnosticsGetResult { result }) => result,
                Ok(CoreResponse::Error { message, .. }) => {
                    let _ = task_state.lsp_request_coordinator.finish(
                        &ticket,
                        &request_scope,
                        route_generation,
                    );
                    let _ = channel.send(unavailable_diagnostics(message));
                    let _ = client
                        .request(RequestEnvelope {
                            protocol_version: PROTOCOL_VERSION,
                            request_id: uuid::Uuid::new_v4().to_string(),
                            payload: CoreRequest::ProjectionUnsubscribe {
                                subscription_id: daemon_subscription.clone(),
                            },
                        })
                        .await;
                    clear_diagnostics_daemon_subscription(&task_state, &owned_id).await;
                    break;
                }
                Ok(other) => {
                    let _ = task_state.lsp_request_coordinator.finish(
                        &ticket,
                        &request_scope,
                        route_generation,
                    );
                    let _ = channel.send(unavailable_diagnostics(format!(
                        "unexpected diagnostics baseline response: {other:?}"
                    )));
                    let _ = client
                        .request(RequestEnvelope {
                            protocol_version: PROTOCOL_VERSION,
                            request_id: uuid::Uuid::new_v4().to_string(),
                            payload: CoreRequest::ProjectionUnsubscribe {
                                subscription_id: daemon_subscription.clone(),
                            },
                        })
                        .await;
                    clear_diagnostics_daemon_subscription(&task_state, &owned_id).await;
                    break;
                }
                Err(error) => {
                    let _ = task_state.lsp_request_coordinator.finish(
                        &ticket,
                        &request_scope,
                        route_generation,
                    );
                    let _ = channel.send(unavailable_diagnostics(error.to_string()));
                    let _ = client
                        .request(RequestEnvelope {
                            protocol_version: PROTOCOL_VERSION,
                            request_id: uuid::Uuid::new_v4().to_string(),
                            payload: CoreRequest::ProjectionUnsubscribe {
                                subscription_id: daemon_subscription.clone(),
                            },
                        })
                        .await;
                    clear_diagnostics_daemon_subscription(&task_state, &owned_id).await;
                    break;
                }
            };
            if !task_state
                .lsp_request_coordinator
                .finish(&ticket, &request_scope, route_generation)
            {
                let _ = client
                    .request(RequestEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        payload: CoreRequest::ProjectionUnsubscribe {
                            subscription_id: daemon_subscription.clone(),
                        },
                    })
                    .await;
                clear_diagnostics_daemon_subscription(&task_state, &owned_id).await;
                break;
            }
            let _ = stream.apply_authoritative(initial.clone());
            if channel.send(diagnostics_view(initial)).is_err() {
                let _ = client
                    .request(RequestEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        payload: CoreRequest::ProjectionUnsubscribe {
                            subscription_id: daemon_subscription,
                        },
                    })
                    .await;
                clear_diagnostics_daemon_subscription(&task_state, &owned_id).await;
                break;
            }
            let mut restart = false;
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        if task_state.current_generation() != connection_generation
                            || task_state.route_selection(route_generation, false).await.is_err()
                        {
                            break;
                        }
                    }
                    event = events.recv() => match event {
                        Some(event) => match stream.accept_client_event(&event) {
                            DiagnosticsStreamUpdate::Updated(result) => {
                                if channel.send(diagnostics_view(result)).is_err() { break; }
                            }
                            DiagnosticsStreamUpdate::ReconcileRequired => {
                                if let Some(current) = stream.current().cloned() {
                                    let mut view = diagnostics_view(current);
                                    view.stale = true;
                                    view.message = format!("{}; reconciling missed updates", view.message);
                                    if channel.send(view).is_err() { break; }
                                }
                                restart = true;
                                break;
                            }
                            DiagnosticsStreamUpdate::Ignored => {}
                        },
                        None => break,
                    }
                }
            }
            let _ = client
                .request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: CoreRequest::ProjectionUnsubscribe {
                        subscription_id: daemon_subscription,
                    },
                })
                .await;
            clear_diagnostics_daemon_subscription(&task_state, &owned_id).await;
            if !restart {
                break 'subscribe;
            }
        }
        let _ = task_state
            .lsp_diagnostics_subscriptions
            .clear_if_matching(&owned_id)
            .await;
    });
    {
        let _lifecycle = state.lifecycle.lock().await;
        if state.current_generation() != connection_generation {
            task.abort();
            return Err("the daemon connection changed while diagnostics were attaching".into());
        }
        state
            .lsp_diagnostics_subscriptions
            .install_with_id(subscription_id.clone(), connection_generation, task)
            .await;
    }
    let _ = task_start.send(());
    Ok(subscription_id)
}

#[tauri::command]
pub(crate) async fn desktop_document_diagnostics_unsubscribe(
    state: State<'_, Arc<HostState>>,
    subscription_id: String,
) -> Result<(), String> {
    let _subscription_lifecycle = state.lsp_diagnostics_lifecycle.lock().await;
    let daemon_subscription = {
        let mut active = state.lsp_diagnostics_daemon_subscription.lock().await;
        match active.as_ref() {
            Some((owner, _)) if owner == &subscription_id => {
                active.take().map(|(_, daemon)| daemon)
            }
            _ => None,
        }
    };
    if let Some(daemon_subscription) = daemon_subscription {
        if let Some(client) = state.client.lock().await.clone() {
            let _ = client
                .request(RequestEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: CoreRequest::ProjectionUnsubscribe {
                        subscription_id: daemon_subscription,
                    },
                })
                .await;
        }
    }
    let _ = state
        .lsp_diagnostics_subscriptions
        .unsubscribe(&subscription_id)
        .await;
    Ok(())
}

async fn clear_diagnostics_daemon_subscription(state: &HostState, owner_id: &str) {
    let mut active = state.lsp_diagnostics_daemon_subscription.lock().await;
    if active.as_ref().is_some_and(|(owner, _)| owner == owner_id) {
        *active = None;
    }
}

/// Read the completion candidates for the caret's position.
#[tauri::command]
pub(crate) async fn desktop_document_completion(
    state: State<'_, Arc<HostState>>,
    trigger: Option<String>,
) -> Result<EditorCompletionView, String> {
    desktop_document_completion_with_state(state.inner(), trigger).await
}

pub(crate) async fn desktop_document_completion_with_state(
    state: &Arc<HostState>,
    trigger: Option<String>,
) -> Result<EditorCompletionView, String> {
    let session = live_session(state).await?;
    let attachment = session
        .controller()
        .try_attachment_info()
        .ok_or_else(|| "no document is open".to_owned())?;
    let (snapshot, revision) = session.snapshot()?;
    let caret = session.frontend.lock().await.cursor.caret;
    let (line, character) = utf16_position(&snapshot, caret)?;
    let request = LspCompletionRequestDto {
        session_id: route_session(state).await?,
        path: session.relative_path().await,
        document_id: attachment.document_id.clone(),
        revision,
        line,
        character,
        trigger_character: trigger.filter(|value| !value.is_empty()),
        max_candidates: MAX_EDITOR_COMPLETIONS as u32,
    };
    let (client, _) = state.route_request().await?;
    let coordinator_scope = codegg_client::LspRequestScope {
        frontend_id: "desktop".to_owned(),
        attachment_id: attachment.document_id.clone(),
        route_generation: state.current_generation(),
        connection_generation: state.current_generation(),
        project_id: attachment.project_id.clone(),
        workspace_id: attachment.workspace_id.clone(),
        session_id: request.session_id.clone(),
        document_id: Some(attachment.document_id.clone()),
        path: Some(request.path.clone()),
        server_id: None,
    };
    let document_generation = revision;
    let ticket = match state
        .lsp_request_coordinator
        .begin(codegg_client::LspRequestKey {
            scope: coordinator_scope.clone(),
            operation: codegg_client::LspRequestOperation::Completion,
            document_generation,
            position: Some((line, character)),
            argument: request.trigger_character.clone(),
        }) {
        codegg_client::LspRequestStart::Started(ticket) => ticket,
        codegg_client::LspRequestStart::AlreadyInFlight => {
            return Err("an equivalent completion request is already in flight".to_owned());
        }
        codegg_client::LspRequestStart::AtCapacity => {
            return Err("the LSP request queue is full".to_owned());
        }
    };
    let cancellation = ticket.cancellation_token();
    let response = tokio::select! {
        _ = cancellation.cancelled() => {
            let _ = state.lsp_request_coordinator.finish(&ticket, &coordinator_scope, document_generation);
            return Err("the completion request was superseded".to_owned());
        },
        _ = tokio::time::sleep(codegg_client::LSP_INTERACTIVE_DEBOUNCE) => {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = state.lsp_request_coordinator.finish(&ticket, &coordinator_scope, document_generation);
                    return Err("the completion request was superseded".to_owned());
                }
                response = client.lsp_completion(request) => response,
            }
        }
    }
    .map_err(|error| format!("the completion could not be read: {error}"));
    let active = match live_session(state).await {
        Ok(active) => active,
        Err(_) => {
            let _ = state.lsp_request_coordinator.finish(
                &ticket,
                &coordinator_scope,
                document_generation,
            );
            return Err("the editor attachment changed during completion".to_owned());
        }
    };
    let active_attachment = match active.controller().try_attachment_info() {
        Some(attachment) => attachment,
        None => {
            let _ = state.lsp_request_coordinator.finish(
                &ticket,
                &coordinator_scope,
                document_generation,
            );
            return Err("the editor attachment changed during completion".to_owned());
        }
    };
    let active_session_id = match route_session(state).await {
        Ok(session_id) => session_id,
        Err(error) => {
            let _ = state.lsp_request_coordinator.finish(
                &ticket,
                &coordinator_scope,
                document_generation,
            );
            return Err(error);
        }
    };
    let active_scope = codegg_client::LspRequestScope {
        frontend_id: "desktop".to_owned(),
        attachment_id: active_attachment.document_id.clone(),
        route_generation: state.current_generation(),
        connection_generation: state.current_generation(),
        project_id: active_attachment.project_id,
        workspace_id: active_attachment.workspace_id,
        session_id: active_session_id,
        document_id: Some(active_attachment.document_id),
        path: Some(active.relative_path().await),
        server_id: None,
    };
    let (active_snapshot, active_revision) = match active.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ =
                state
                    .lsp_request_coordinator
                    .finish(&ticket, &active_scope, document_generation);
            return Err(error);
        }
    };
    let active_caret = active.frontend.lock().await.cursor.caret;
    let active_position = utf16_position(&active_snapshot, active_caret).ok();
    if !state
        .lsp_request_coordinator
        .finish(&ticket, &active_scope, active_revision)
        || active_revision != document_generation
        || active_position != Some((line, character))
    {
        return Err("the completion response belongs to a stale editor attachment".to_owned());
    }
    let result = response?;
    Ok(completion_view(result))
}

/// Read one navigation target set for the caret's position.
#[tauri::command]
pub(crate) async fn desktop_document_navigate(
    state: State<'_, Arc<HostState>>,
    operation: String,
) -> Result<EditorNavigationView, String> {
    desktop_document_navigate_with_state(state.inner(), operation).await
}

pub(crate) async fn desktop_document_navigate_with_state(
    state: &Arc<HostState>,
    operation: String,
) -> Result<EditorNavigationView, String> {
    let session = live_session(state).await?;
    let read = navigation_operation(&operation)
        .ok_or_else(|| format!("`{operation}` is not a navigation this host serves"))?;
    let attachment = session
        .controller()
        .try_attachment_info()
        .ok_or_else(|| "the document attachment is unavailable".to_owned())?;
    let (snapshot, _) = session.snapshot()?;
    let caret = session.frontend.lock().await.cursor.caret;
    let (line, character) = utf16_position(&snapshot, caret)?;
    let request = LspReadRequestDto {
        operation: read,
        session_id: route_session(state).await?,
        path: session.relative_path().await,
        line: Some(line),
        column: Some(character),
        query: None,
        server_id: None,
        new_name: None,
    };
    let (client, _) = state.route_request().await?;
    let coordinator_scope = codegg_client::LspRequestScope {
        frontend_id: "desktop".to_owned(),
        attachment_id: attachment.document_id.clone(),
        route_generation: state.current_generation(),
        connection_generation: state.current_generation(),
        project_id: attachment.project_id.clone(),
        workspace_id: attachment.workspace_id.clone(),
        session_id: request.session_id.clone(),
        document_id: Some(attachment.document_id.clone()),
        path: Some(request.path.clone()),
        server_id: request.server_id.clone(),
    };
    let document_generation = attachment.revision;
    let ticket = match state
        .lsp_request_coordinator
        .begin(codegg_client::LspRequestKey {
            scope: coordinator_scope.clone(),
            operation: codegg_client::LspRequestOperation::Read(read),
            document_generation,
            position: Some((line, character)),
            argument: None,
        }) {
        codegg_client::LspRequestStart::Started(ticket) => ticket,
        codegg_client::LspRequestStart::AlreadyInFlight => {
            return Err("an equivalent navigation request is already in flight".to_owned());
        }
        codegg_client::LspRequestStart::AtCapacity => {
            return Err("the LSP request queue is full".to_owned());
        }
    };
    let cancellation = ticket.cancellation_token();
    let response = tokio::select! {
        _ = cancellation.cancelled() => {
            let _ = state.lsp_request_coordinator.finish(&ticket, &coordinator_scope, document_generation);
            return Err("the navigation request was superseded".to_owned());
        },
        response = client.lsp_read(request) => response,
    }
    .map_err(|error| format!("the navigation could not be read: {error}"));
    let active_session = match live_session(state).await {
        Ok(active) => active,
        Err(_) => {
            let _ = state.lsp_request_coordinator.finish(
                &ticket,
                &coordinator_scope,
                document_generation,
            );
            return Err("the editor attachment changed during navigation".to_owned());
        }
    };
    let active_attachment = active_session.controller().try_attachment_info();
    let active_scope = match active_attachment {
        Some(info) => {
            let active_session_id = match route_session(state).await {
                Ok(session_id) => session_id,
                Err(error) => {
                    let _ = state.lsp_request_coordinator.finish(
                        &ticket,
                        &coordinator_scope,
                        document_generation,
                    );
                    return Err(error);
                }
            };
            codegg_client::LspRequestScope {
                frontend_id: "desktop".to_owned(),
                attachment_id: info.document_id.clone(),
                route_generation: state.current_generation(),
                connection_generation: state.current_generation(),
                project_id: info.project_id,
                workspace_id: info.workspace_id,
                session_id: active_session_id,
                document_id: Some(info.document_id),
                path: Some(active_session.relative_path().await),
                server_id: None,
            }
        }
        None => {
            let _ = state.lsp_request_coordinator.finish(
                &ticket,
                &coordinator_scope,
                document_generation,
            );
            return Err("the editor attachment changed during navigation".to_owned());
        }
    };
    let (active_snapshot, active_revision) = match active_session.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = state.lsp_request_coordinator.finish(
                &ticket,
                &coordinator_scope,
                document_generation,
            );
            return Err(error);
        }
    };
    let active_caret = active_session.frontend.lock().await.cursor.caret;
    let active_position = utf16_position(&active_snapshot, active_caret).ok();
    if !state
        .lsp_request_coordinator
        .finish(&ticket, &active_scope, active_revision)
        || active_revision != document_generation
        || active_position != Some((line, character))
    {
        return Err("the navigation response belongs to a stale editor attachment".to_owned());
    }
    let result = response?;
    Ok(navigation_view(result))
}

/// The live session, with the controller reattached after a reconnect.
async fn live_session(state: &HostState) -> Result<Arc<DesktopDocumentSession>, String> {
    let session = state
        .document
        .lock()
        .await
        .clone()
        .ok_or_else(|| "no document is open".to_owned())?;
    session.ensure_connection(state).await?;
    Ok(session)
}

/// The route's session id, when a session is selected.
async fn route_session(state: &HostState) -> Result<String, String> {
    state
        .route
        .lock()
        .await
        .session_id
        .clone()
        .ok_or_else(|| "no session is selected for the current route".to_owned())
}

/// The caret's UTF-16 position, as the language server expects it.
fn utf16_position(snapshot: &DocumentSnapshot, caret: usize) -> Result<(u32, u32), String> {
    let position = snapshot
        .byte_to_position(caret)
        .map_err(|error| format!("the caret is not at a readable position: {error}"))?;
    let range = snapshot
        .line_range(position.line)
        .map_err(|error| format!("the caret's line is not readable: {error}"))?;
    let character = snapshot
        .utf16_column_bounded(range.start, caret)
        .map_err(|error| format!("the caret's column is not readable: {error}"))?;
    Ok((position.line as u32, character))
}

/// Map one daemon listing to the editor's bounded tree view.
pub fn tree_view(
    result: &codegg_protocol::document::WorkspaceDirectoryListResultDto,
    workspace_id: &str,
    prefix: &str,
) -> EditorTreeView {
    let mut remaining = MAX_EDITOR_TREE_NODES;
    let mut truncated = result.truncated;
    let nodes = result
        .nodes
        .iter()
        .map(|node| tree_node(node, &mut remaining, &mut truncated))
        .collect();
    let (status, message) = match result.status {
        codegg_protocol::document::WorkspaceDirectoryListStatusDto::Ready => {
            ("ready", "the workspace listing is current")
        }
        codegg_protocol::document::WorkspaceDirectoryListStatusDto::Busy => (
            "busy",
            "the daemon is scanning the workspace; retry the listing",
        ),
        codegg_protocol::document::WorkspaceDirectoryListStatusDto::Unavailable => {
            ("unavailable", "the daemon cannot list this workspace")
        }
    };
    EditorTreeView {
        workspace_id: workspace_id.to_owned(),
        prefix: prefix.to_owned(),
        status: status.to_owned(),
        message: message.to_owned(),
        nodes,
        truncated,
    }
}

/// Map one daemon node to the editor's bounded tree view.
fn tree_node(
    node: &WorkspaceDirectoryNodeDto,
    remaining: &mut usize,
    truncated: &mut bool,
) -> EditorTreeNodeView {
    let relative_path = match safe_relative_path(&node.path) {
        Ok(path) => path,
        Err(_) => {
            // A node the host cannot vouch for is not shown, and the listing
            // says it was trimmed rather than implying it was complete.
            *truncated = true;
            return EditorTreeNodeView {
                name: node.name.clone(),
                relative_path: String::new(),
                depth: node.depth,
                kind: "unavailable".to_owned(),
                children: Vec::new(),
            };
        }
    };
    if *remaining == 0 {
        *truncated = true;
        return EditorTreeNodeView {
            name: node.name.clone(),
            relative_path,
            depth: node.depth,
            kind: "unavailable".to_owned(),
            children: Vec::new(),
        };
    }
    *remaining -= 1;
    let children = node
        .children
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|child| tree_node(child, remaining, truncated))
        .collect();
    EditorTreeNodeView {
        name: node.name.clone(),
        relative_path,
        depth: node.depth,
        kind: match node.kind {
            WorkspaceDirectoryNodeKindDto::Directory => "directory".to_owned(),
            WorkspaceDirectoryNodeKindDto::File => "file".to_owned(),
        },
        children,
    }
}

/// Map one diagnostics result to the editor's bounded view.
///
/// `NotReady` is **not** an empty diagnostic set: it is the explicit "no
/// warm language server" answer, and the renderer must be able to tell the
/// two apart.
pub fn diagnostics_view(
    result: codegg_protocol::lsp::LspDiagnosticsResultDto,
) -> EditorDiagnosticsView {
    let (status, message) = match result.status {
        LspDiagnosticsStatus::Ready => ("ready", "the project's diagnostics are current"),
        LspDiagnosticsStatus::NotReady => (
            "no_warm_server",
            "no language server is warm for this project yet",
        ),
    };
    let mut remaining = MAX_EDITOR_DIAGNOSTICS;
    let mut truncated = result.truncated;
    let mut diagnostics = Vec::new();
    for file in &result.files {
        for diagnostic in &file.diagnostics {
            if remaining == 0 {
                truncated = true;
                break;
            }
            remaining -= 1;
            let Ok(relative_path) = safe_relative_path(&file.path) else {
                truncated = true;
                continue;
            };
            diagnostics.push(EditorDiagnosticView {
                relative_path,
                line: diagnostic.range.start_line,
                start_character: diagnostic.range.start_column,
                end_character: diagnostic.range.end_column,
                severity: diagnostic.severity,
                message: diagnostic.message.clone(),
                server: diagnostic.source.clone(),
            });
        }
    }
    EditorDiagnosticsView {
        status: status.to_owned(),
        message: message.to_owned(),
        diagnostics,
        truncated,
        stale: false,
    }
}

fn unavailable_diagnostics(message: String) -> EditorDiagnosticsView {
    EditorDiagnosticsView {
        status: "unavailable".to_owned(),
        message,
        diagnostics: Vec::new(),
        truncated: false,
        stale: false,
    }
}

/// Map one completion result to the editor's bounded view.
pub fn completion_view(
    result: codegg_protocol::lsp::LspCompletionResultDto,
) -> EditorCompletionView {
    let (status, message) = match result.status {
        LspCompletionStatus::Ready => ("ready", "the completion candidates are current"),
        LspCompletionStatus::NotReady => (
            "no_warm_server",
            "no language server is warm for this document yet",
        ),
        LspCompletionStatus::Stale => (
            "stale",
            "the document moved on; the candidates are out of date",
        ),
        LspCompletionStatus::Unsynced => (
            "unsynced",
            "the language server has not seen the current revision yet",
        ),
        LspCompletionStatus::Unsupported => (
            "unsupported",
            "the language server offers no completion here",
        ),
    };
    let mut candidates = Vec::new();
    for candidate in result.candidates.iter().take(MAX_EDITOR_COMPLETIONS) {
        let (applicable, reason) = match &candidate.applicability {
            LspCompletionApplicabilityDto::Applicable { .. } => (true, None),
            LspCompletionApplicabilityDto::NonApplicable { reason } => {
                (false, Some(format!("{reason:?}")))
            }
        };
        candidates.push(EditorCompletionCandidateView {
            label: candidate.label.clone(),
            detail: candidate.detail.clone(),
            kind: candidate.kind.clone(),
            applicable,
            reason,
        });
    }
    EditorCompletionView {
        status: status.to_owned(),
        message: message.to_owned(),
        candidates,
        truncated: result.truncated || result.candidates.len() > MAX_EDITOR_COMPLETIONS,
    }
}

/// Map one navigation read to the editor's bounded view.
pub fn navigation_view(result: codegg_protocol::lsp::LspReadResultDto) -> EditorNavigationView {
    let (status, message) = match result.status {
        LspReadStatus::Ready => ("ready", "the navigation targets are current"),
        LspReadStatus::NotReady => (
            "no_warm_server",
            "no language server is warm for this document yet",
        ),
        LspReadStatus::Ambiguous => (
            "ambiguous",
            "more than one language server could answer; the request named none",
        ),
        LspReadStatus::Unsupported => (
            "unsupported",
            "the language server does not offer this read",
        ),
    };
    let (locations, truncated) = match &result.payload {
        Some(codegg_protocol::lsp::LspReadPayloadDto::Locations {
            locations,
            truncated,
        }) => {
            let mut trimmed = Vec::new();
            let mut was_truncated = *truncated;
            for location in locations.iter().take(MAX_EDITOR_LOCATIONS) {
                let path = location
                    .workspace_relative_path
                    .as_deref()
                    .unwrap_or(location.path.as_str());
                let Ok(relative_path) = safe_relative_path(path) else {
                    was_truncated = true;
                    continue;
                };
                trimmed.push(EditorLocationView {
                    relative_path,
                    line: location.start_line,
                    character: location.start_column,
                });
            }
            if locations.len() > MAX_EDITOR_LOCATIONS {
                was_truncated = true;
            }
            (trimmed, was_truncated)
        }
        _ => (Vec::new(), false),
    };
    EditorNavigationView {
        status: status.to_owned(),
        message: message.to_owned(),
        locations,
        truncated,
    }
}

#[cfg(test)]
mod lsp_view_tests {
    use super::*;
    use codegg_protocol::lsp::{LspDiagnosticsResultDto, LspDiagnosticsStatus};

    #[test]
    fn diagnostics_readiness_does_not_collapse_not_ready_into_clean() {
        let not_ready = diagnostics_view(LspDiagnosticsResultDto {
            project_id: "project-1".to_owned(),
            status: LspDiagnosticsStatus::NotReady,
            files: Vec::new(),
            truncated: false,
        });
        let ready_empty = diagnostics_view(LspDiagnosticsResultDto {
            project_id: "project-1".to_owned(),
            status: LspDiagnosticsStatus::Ready,
            files: Vec::new(),
            truncated: false,
        });

        assert_eq!(not_ready.status, "no_warm_server");
        assert!(not_ready.diagnostics.is_empty());
        assert_eq!(ready_empty.status, "ready");
        assert!(ready_empty.diagnostics.is_empty());
        assert_ne!(not_ready.status, ready_empty.status);
    }

    #[test]
    fn desktop_completion_keeps_non_ready_status_vocabulary() {
        use codegg_protocol::lsp::LspCompletionResultDto;

        assert_eq!(
            completion_view(LspCompletionResultDto::not_ready("doc-1".into())).status,
            "no_warm_server"
        );
        assert_eq!(
            completion_view(LspCompletionResultDto::stale("doc-1".into(), 3, Some(2))).status,
            "stale"
        );
        assert_eq!(
            completion_view(LspCompletionResultDto::unsynced("doc-1".into(), 3, Some(2))).status,
            "unsynced"
        );
        assert_eq!(
            completion_view(LspCompletionResultDto::unsupported(
                "doc-1".into(),
                3,
                Some(3)
            ))
            .status,
            "unsupported"
        );
    }

    #[test]
    fn desktop_navigation_route_accepts_all_visible_operations_only() {
        assert_eq!(
            navigation_operation("definition"),
            Some(LspReadOperation::Definition)
        );
        assert_eq!(
            navigation_operation("declaration"),
            Some(LspReadOperation::Declaration)
        );
        assert_eq!(
            navigation_operation("implementation"),
            Some(LspReadOperation::Implementation)
        );
        assert_eq!(
            navigation_operation("references"),
            Some(LspReadOperation::References)
        );
        assert_eq!(navigation_operation("workspace_symbols"), None);
    }
}

#[cfg(test)]
#[path = "document_lsp_unicode_tests.rs"]
mod lsp_unicode_tests;
