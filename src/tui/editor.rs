//! M006-A: TUI editor presentation model.
//!
//! This module owns **frontend presentation state only**: cursor, anchor,
//! viewport, buffer mode, and a bounded undo/redo history. It holds **no
//! document text**.
//!
//! Every offset recorded here is a position into the replica owned by
//! [`DocumentController`]. Text is read on demand through
//! `DocumentController::try_snapshot()` and dropped at the end of the
//! call that needed it; nothing in this module, in
//! [`TuiDocumentPresentation`](super::document_session::TuiDocumentPresentation),
//! or in [`EditorState`](super::app::state::EditorState) may store it.
//!
//! `scripts/check_tui_editor_text_authority.py` enforces that statically.
//!
//! ## Hard wrap
//!
//! M006-A ships hard wrap with horizontal scrolling. One logical line maps
//! to exactly one screen row, which is what makes the line-number gutter,
//! the current-line highlight, and cursor arithmetic deterministic. Soft
//! wrap changes the line-to-row mapping and is deferred; see the M006-A
//! closure record.

use codegg_document::{BytePosition, DocumentSnapshot, TextEdit, TextTransaction};

use super::document_session::TuiDocumentPresentation;

/// Maximum retained undo/redo entries per document attachment.
pub const MAX_EDITOR_UNDO_DEPTH: usize = 256;

/// Maximum retained undo/redo payload bytes per document attachment.
///
/// Bounds the inverse transactions a long editing session can retain. Old
/// entries are dropped from the bottom of the stack first, so the most
/// recent history is always the history that survives.
pub const MAX_EDITOR_UNDO_BYTES: usize = 4 * 1024 * 1024;

/// Maximum length of a pending multi-key normal-mode command prefix.
///
/// The editor accepts only `d` and `g` prefixes, so two bytes is the whole
/// grammar. Anything longer is discarded rather than buffered.
pub const MAX_EDITOR_PENDING_COMMAND: usize = 2;

/// Minimum display columns reserved for the horizontal scroll indicator.
const MIN_EDITOR_TEXT_COLUMNS: u16 = 1;

/// Buffer edit mode. `Normal` is a vi-style command mode over the buffer;
/// `Insert` routes printable keys into the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    #[default]
    Normal,
    Insert,
}

/// A local, reversible edit produced by a presentation-level command.
///
/// `transaction` is what the frontend submits to
/// `DocumentController::apply_local`; `inverse` restores the pre-edit local
/// text. Both are derived from the snapshot the caller already holds, so
/// constructing an `EditorEdit` never copies the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorEdit {
    /// Forward transaction to submit through the controller.
    pub transaction: TextTransaction,
    /// Inverse transaction that restores the pre-edit text.
    pub inverse: TextTransaction,
    /// Retained payload bytes, for the undo memory bound.
    pub bytes: usize,
    /// Cursor position to adopt once the controller accepts the edit.
    pub cursor_after: usize,
    /// Whether the controller accepting this edit should keep the editor in
    /// insert mode (open-line commands, insertion entry).
    pub stay_insert: bool,
}

/// One reversible step in the frontend-local undo history.
///
/// An entry retains **both** directions so moving it between the undo and
/// redo stacks is exact. Reusing the same transaction for both would make
/// redo re-apply the undo, silently reverting the user's re-edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorUndoEntry {
    /// Transaction that produces the newer text. Redo applies this.
    pub forward: TextTransaction,
    /// Transaction that restores the older text. Undo applies this.
    pub inverse: TextTransaction,
    /// Cursor byte offset observed before the transaction was applied.
    pub cursor_before: usize,
    /// Selection range observed before the transaction was applied.
    pub anchor_before: Option<std::ops::Range<usize>>,
    /// Retained payload bytes across both directions, for the memory bound.
    pub bytes: usize,
}

impl EditorUndoEntry {
    /// Rebase the recorded cursor onto the position the user is leaving
    /// now, so the entry is correct on the opposite stack.
    ///
    /// The two transactions are **not** swapped. They are direction
    /// labelled rather than stack labelled: `inverse` is always what
    /// undo applies and `forward` is always what redo applies, so an entry
    /// moves between stacks unchanged apart from its cursor. Swapping
    /// them would make redo re-apply the undo.
    pub fn rebased(
        self,
        cursor_before: usize,
        anchor_before: Option<std::ops::Range<usize>>,
    ) -> Self {
        Self {
            cursor_before,
            anchor_before,
            ..self
        }
    }
}

/// Which region of the editor route owns keyboard input.
///
/// `Composer` is the default and keeps the ordinary Session/Task prompt
/// editable while the editor is open, matching the `Route::Workspace`
/// precedent. The user focuses the buffer explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorFocus {
    #[default]
    Composer,
    Buffer,
}

/// A resolved, document-relative edit intent from a normal-mode key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    LineStart,
    LineEnd,
    BufferStart,
    BufferEnd,
    WordForward,
    WordBackward,
    HalfPageDown,
    HalfPageUp,
    InsertBefore,
    InsertAfter,
    InsertLineStart,
    InsertLineEnd,
    OpenLineBelow,
    OpenLineAbove,
    DeleteChar,
    DeleteLine,
    DeleteToLineEnd,
    DeleteWord,
    Undo,
    Redo,
}

// ── Offset safety ────────────────────────────────────────────────────────────

/// Clamp `offset` into `0..=len_bytes` and onto a UTF-8 character boundary.
///
/// A non-boundary offset is at most three bytes past the preceding
/// boundary in valid UTF-8, so this walks back at most three steps.
pub fn clamp_offset(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut candidate = offset.min(snapshot.len_bytes());
    while candidate > 0 && !snapshot.is_byte_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// Next character boundary strictly after `offset`, or the end of the
/// document. Returns `None` only when `offset` is already the end.
pub fn next_char_boundary(snapshot: &DocumentSnapshot, offset: usize) -> Option<usize> {
    let mut candidate = clamp_offset(snapshot, offset).saturating_add(1);
    while candidate < snapshot.len_bytes() {
        if snapshot.is_byte_boundary(candidate) {
            return Some(candidate);
        }
        candidate += 1;
    }
    None
}

/// Previous character boundary strictly before `offset`.
pub fn prev_char_boundary(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut candidate = clamp_offset(snapshot, offset);
    while candidate > 0 {
        candidate -= 1;
        if snapshot.is_byte_boundary(candidate) {
            return candidate;
        }
    }
    0
}

/// Zero-based line containing `offset`, clamped to a valid line.
pub fn cursor_line(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let offset = clamp_offset(snapshot, offset);
    snapshot
        .byte_to_position(offset)
        .map(|position| position.line)
        .unwrap_or_else(|_| snapshot.len_lines().saturating_sub(1))
}

/// Byte offset where `line` starts.
pub fn line_start(snapshot: &DocumentSnapshot, line: usize) -> Option<usize> {
    snapshot.line_range(line).ok().map(|range| range.start)
}

/// Byte offset of the last content byte on `line`, excluding the line
/// terminator. Equals the line start for an empty line.
pub fn line_content_end(snapshot: &DocumentSnapshot, line: usize) -> Option<usize> {
    let range = snapshot.line_range(line).ok()?;
    let text = snapshot.read_bytes(range.start, range.end).ok()?;
    let trimmed = text.trim_end_matches('\n').trim_end_matches('\r');
    Some(range.start + trimmed.len())
}

/// Byte offset of the end of the last line, excluding any terminator.
pub fn buffer_content_end(snapshot: &DocumentSnapshot) -> usize {
    let last = snapshot.len_lines().saturating_sub(1);
    line_content_end(snapshot, last).unwrap_or_else(|| snapshot.len_bytes())
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn byte_at(snapshot: &DocumentSnapshot, offset: usize) -> Option<u8> {
    snapshot
        .read_bytes(offset, offset + 1)
        .ok()
        .and_then(|text| text.as_bytes().first().copied())
}

/// Start of the next word at or after `offset`.
pub fn word_forward(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut cursor = clamp_offset(snapshot, offset);
    let end = snapshot.len_bytes();
    while cursor < end {
        if !is_word_byte(byte_at(snapshot, cursor).unwrap_or(0)) {
            cursor += 1;
            continue;
        }
        while cursor < end && is_word_byte(byte_at(snapshot, cursor).unwrap_or(0)) {
            cursor = next_char_boundary(snapshot, cursor).unwrap_or(end);
        }
        if cursor >= end {
            return buffer_content_end(snapshot);
        }
        // Skip the separator run, then report the next word start.
        while cursor < end && !is_word_byte(byte_at(snapshot, cursor).unwrap_or(0)) {
            cursor = next_char_boundary(snapshot, cursor).unwrap_or(end);
        }
        return cursor;
    }
    buffer_content_end(snapshot)
}

/// Start of the previous word strictly before `offset`.
pub fn word_backward(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut cursor = clamp_offset(snapshot, offset);
    while cursor > 0 {
        let previous = prev_char_boundary(snapshot, cursor);
        if is_word_byte(byte_at(snapshot, previous).unwrap_or(0)) {
            break;
        }
        cursor = previous;
    }
    while cursor > 0 {
        let previous = prev_char_boundary(snapshot, cursor);
        if !is_word_byte(byte_at(snapshot, previous).unwrap_or(0)) {
            break;
        }
        cursor = previous;
    }
    cursor
}

/// Move `offset` by `delta` lines, preserving the display column as closely
/// as the target line allows.
pub fn move_vertical(snapshot: &DocumentSnapshot, offset: usize, delta: isize) -> usize {
    let offset = clamp_offset(snapshot, offset);
    let last_line = snapshot.len_lines().saturating_sub(1);
    let current = cursor_line(snapshot, offset);
    let column = offset.saturating_sub(line_start(snapshot, current).unwrap_or(0));
    let target = if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as usize).min(last_line)
    };
    let start = line_start(snapshot, target).unwrap_or(0);
    let end = line_content_end(snapshot, target).unwrap_or(start);
    clamp_offset(snapshot, (start + column).min(end))
}

/// Normalized selection range, with inverted anchors resolved.
///
/// Both endpoints are clamped to the document and to character boundaries,
/// so a stale selection recorded against older text can never panic or
/// slice mid-codepoint during rendering.
pub fn normalized_selection(
    presentation: &TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
) -> Option<std::ops::Range<usize>> {
    let selection = presentation.selection.as_ref()?;
    let start = clamp_offset(snapshot, selection.start.min(selection.end));
    let end = clamp_offset(snapshot, selection.start.max(selection.end));
    (start < end).then_some(start..end)
}

/// Line reached by a half-page motion, from the current viewport height.
pub fn half_page_target(viewport_line: usize, visible_lines: usize, down: bool) -> usize {
    let step = (visible_lines / 2).max(1);
    if down {
        viewport_line.saturating_add(step)
    } else {
        viewport_line.saturating_sub(step)
    }
}

/// Place the cursor on `line` at the given byte column, clamped to that
/// line's content end.
pub fn offset_on_line(snapshot: &DocumentSnapshot, line: usize, column: usize) -> usize {
    let last = snapshot.len_lines().saturating_sub(1);
    let line = line.min(last);
    let start = line_start(snapshot, line).unwrap_or(0);
    let end = line_content_end(snapshot, line).unwrap_or(start);
    clamp_offset(snapshot, (start + column).min(end))
}

// ── Transaction construction ─────────────────────────────────────────────────

/// Compute the inverse of `transaction` against the pre-transaction text.
///
/// Mirrors the offset arithmetic `DocumentBuffer::apply` uses so undo and
/// redo are exact local inverses without the frontend ever retaining a copy
/// of the document.
pub fn invert(
    snapshot: &DocumentSnapshot,
    transaction: &TextTransaction,
) -> Option<TextTransaction> {
    let mut inverse = Vec::with_capacity(transaction.edits.len());
    let mut shift: isize = 0;
    for edit in &transaction.edits {
        let removed = snapshot.read_bytes(edit.range.start, edit.range.end).ok()?;
        let post_start =
            usize::try_from(isize::try_from(edit.range.start).ok()?.checked_add(shift)?).ok()?;
        let post_end = post_start.checked_add(edit.insert.len())?;
        inverse.push(TextEdit::new(post_start..post_end, removed));
        let inserted = isize::try_from(edit.insert.len()).ok()?;
        let removed_len = isize::try_from(edit.range.end - edit.range.start).ok()?;
        shift = shift.checked_add(inserted - removed_len)?;
    }
    Some(TextTransaction::new(inverse))
}

fn build_edit(
    snapshot: &DocumentSnapshot,
    transaction: TextTransaction,
    cursor_after: usize,
    stay_insert: bool,
) -> Option<EditorEdit> {
    if transaction.edits.is_empty() {
        return None;
    }
    let inverse = invert(snapshot, &transaction)?;
    let bytes = transaction
        .edits
        .iter()
        .map(|edit| edit.insert.len())
        .sum::<usize>()
        + inverse
            .edits
            .iter()
            .map(|edit| edit.insert.len())
            .sum::<usize>();
    Some(EditorEdit {
        transaction,
        inverse,
        bytes,
        // Deliberately not clamped against `snapshot`: that is the
        // *pre*-edit text, and an insertion legitimately places the
        // cursor beyond it. The caller clamps against the post-edit
        // snapshot once the controller has accepted the transaction.
        cursor_after,
        stay_insert,
    })
}

/// Insert `text` at `offset`, leaving the cursor after the inserted bytes.
pub fn insert_text(snapshot: &DocumentSnapshot, offset: usize, text: &str) -> Option<EditorEdit> {
    if text.is_empty() {
        return None;
    }
    let offset = clamp_offset(snapshot, offset);
    let transaction = TextTransaction::new(vec![TextEdit::new(offset..offset, text.to_string())]);
    build_edit(snapshot, transaction, offset + text.len(), true)
}

/// Delete `range`, leaving the cursor at its start.
pub fn delete_range(
    snapshot: &DocumentSnapshot,
    range: std::ops::Range<usize>,
) -> Option<EditorEdit> {
    let start = clamp_offset(snapshot, range.start);
    let end = clamp_offset(snapshot, range.end);
    if start >= end {
        return None;
    }
    let transaction = TextTransaction::new(vec![TextEdit::new(start..end, String::new())]);
    build_edit(snapshot, transaction, start, false)
}

// ── Resolution against a presentation record ────────────────────────────────

/// Apply a motion command to the presentation. Never touches text.
pub fn apply_motion(
    presentation: &mut TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    command: EditorCommand,
    visible_lines: usize,
) {
    let cursor = clamp_offset(snapshot, presentation.cursor_byte);
    let next = match command {
        EditorCommand::MoveLeft => prev_char_boundary(snapshot, cursor),
        EditorCommand::MoveRight => next_char_boundary(snapshot, cursor).unwrap_or(cursor),
        EditorCommand::MoveUp => move_vertical(snapshot, cursor, -1),
        EditorCommand::MoveDown => move_vertical(snapshot, cursor, 1),
        EditorCommand::LineStart => {
            line_start(snapshot, cursor_line(snapshot, cursor)).unwrap_or(0)
        }
        EditorCommand::LineEnd => {
            let line = cursor_line(snapshot, cursor);
            line_content_end(snapshot, line).unwrap_or(cursor)
        }
        EditorCommand::BufferStart => 0,
        EditorCommand::BufferEnd => buffer_content_end(snapshot),
        EditorCommand::WordForward => word_forward(snapshot, cursor),
        EditorCommand::WordBackward => word_backward(snapshot, cursor),
        EditorCommand::HalfPageDown => {
            let target = half_page_target(presentation.viewport_line, visible_lines, true);
            let column = cursor
                .saturating_sub(line_start(snapshot, cursor_line(snapshot, cursor)).unwrap_or(0));
            offset_on_line(snapshot, target, column)
        }
        EditorCommand::HalfPageUp => {
            let target = half_page_target(presentation.viewport_line, visible_lines, false);
            let column = cursor
                .saturating_sub(line_start(snapshot, cursor_line(snapshot, cursor)).unwrap_or(0));
            offset_on_line(snapshot, target, column)
        }
        _ => return,
    };
    presentation.cursor_byte = clamp_offset(snapshot, next);
    presentation.selection = None;
}

/// Resolve an editing command into a local transaction plus the cursor and
/// mode the editor should adopt if the controller accepts it.
///
/// Pure: reads the snapshot, mutates nothing. Returns `None` when the
/// command is a no-op at the current position, or when the request needs a
/// pending prefix the caller has not supplied.
pub fn resolve_edit(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
    command: EditorCommand,
) -> Option<EditorEdit> {
    let cursor = clamp_offset(snapshot, presentation.cursor_byte);
    let line = cursor_line(snapshot, cursor);
    match command {
        EditorCommand::InsertBefore => None, // handled by mode transition
        EditorCommand::InsertAfter => None,
        EditorCommand::InsertLineStart => None,
        EditorCommand::InsertLineEnd => None,
        EditorCommand::OpenLineBelow => {
            let end = line_content_end(snapshot, line).unwrap_or(cursor);
            let insert_at = next_char_boundary(snapshot, end).unwrap_or(end);
            // Split the existing line terminator: the new line inherits it
            // and the current line gets a fresh one. With no terminator to
            // split (last line of the buffer), append a full blank line.
            let has_terminator = insert_at > end;
            let text = if has_terminator { "\n" } else { "\n\n" };
            build_edit(
                snapshot,
                TextTransaction::new(vec![TextEdit::new(insert_at..insert_at, text.to_string())]),
                insert_at + 1,
                true,
            )
        }
        EditorCommand::OpenLineAbove => {
            let start = line_start(snapshot, line).unwrap_or(0);
            build_edit(
                snapshot,
                TextTransaction::new(vec![TextEdit::new(start..start, "\n".to_string())]),
                start,
                true,
            )
        }
        EditorCommand::DeleteChar => {
            let end = next_char_boundary(snapshot, cursor).unwrap_or(cursor);
            if end > cursor {
                delete_range(snapshot, cursor..end)
            } else {
                None
            }
        }
        EditorCommand::DeleteToLineEnd => {
            let end = line_content_end(snapshot, line).unwrap_or(cursor);
            if end > cursor {
                delete_range(snapshot, cursor..end)
            } else {
                None
            }
        }
        EditorCommand::DeleteLine => {
            // vi `dd` semantics: remove the line and its terminator. The
            // last line has no terminator, so the *preceding* terminator is
            // removed instead, which leaves no trailing blank line.
            let last = snapshot.len_lines().saturating_sub(1);
            let (start, end) = if line < last {
                (
                    line_start(snapshot, line).unwrap_or(0),
                    line_start(snapshot, line + 1).unwrap_or(snapshot.len_bytes()),
                )
            } else if line > 0 {
                (
                    line_content_end(snapshot, line - 1).unwrap_or(0),
                    snapshot.len_bytes(),
                )
            } else {
                (0, snapshot.len_bytes())
            };
            if end > start {
                delete_range(snapshot, start..end)
            } else {
                None
            }
        }
        EditorCommand::DeleteWord => {
            let start = word_backward(
                snapshot,
                next_char_boundary(snapshot, cursor).unwrap_or(cursor),
            );
            if start < cursor {
                delete_range(snapshot, start..cursor)
            } else {
                None
            }
        }
        _ => None,
    }
}

// ── Undo history ────────────────────────────────────────────────────────────

/// Record an accepted edit in the undo history and clear the redo history.
///
/// Convenience wrapper over [`push_undo`] that reads the pre-edit
/// position from the presentation itself. The command layer uses
/// [`push_undo`] directly, because it has already moved the cursor by the
/// time the controller accepts the transaction.
pub fn record_undo(presentation: &mut TuiDocumentPresentation, edit: &EditorEdit) {
    push_undo(
        presentation,
        edit.transaction.clone(),
        edit.inverse.clone(),
        edit.bytes,
        presentation.cursor_byte,
        presentation.selection.clone(),
    );
}

/// Push one accepted edit onto the undo history and clear the redo history.
///
/// `cursor_before` and `anchor_before` are the positions observed *before*
/// the forward transaction, so undo restores where the user actually was.
pub fn push_undo(
    presentation: &mut TuiDocumentPresentation,
    forward: TextTransaction,
    inverse: TextTransaction,
    bytes: usize,
    cursor_before: usize,
    anchor_before: Option<std::ops::Range<usize>>,
) {
    presentation.undo.push(EditorUndoEntry {
        forward,
        inverse,
        cursor_before,
        anchor_before,
        bytes,
    });
    presentation.undo_bytes = presentation.undo_bytes.saturating_add(bytes);
    presentation.redo.clear();
    presentation.redo_bytes = 0;
    trim_history(presentation);
}

/// Take the newest undo entry without applying it, so the caller can apply
/// it through the controller and restore it on rejection.
pub fn take_undo(presentation: &mut TuiDocumentPresentation) -> Option<EditorUndoEntry> {
    let entry = presentation.undo.pop()?;
    presentation.undo_bytes = presentation.undo_bytes.saturating_sub(entry.bytes);
    Some(entry)
}

/// Take the newest redo entry without applying it.
pub fn take_redo(presentation: &mut TuiDocumentPresentation) -> Option<EditorUndoEntry> {
    let entry = presentation.redo.pop()?;
    presentation.redo_bytes = presentation.redo_bytes.saturating_sub(entry.bytes);
    Some(entry)
}

/// Return an entry to the undo history after the controller rejected it.
pub fn restore_undo(presentation: &mut TuiDocumentPresentation, entry: EditorUndoEntry) {
    presentation.undo.push(entry);
    presentation.undo_bytes = presentation
        .undo_bytes
        .saturating_add(presentation.undo.last().map_or(0, |e| e.bytes));
    trim_history(presentation);
}

/// Record an entry on the redo history after a successful undo.
pub fn record_redo(presentation: &mut TuiDocumentPresentation, entry: EditorUndoEntry) {
    presentation.redo_bytes = presentation.redo_bytes.saturating_add(entry.bytes);
    presentation.redo.push(entry);
    trim_history(presentation);
}

fn trim_history(presentation: &mut TuiDocumentPresentation) {
    while presentation.undo.len() > MAX_EDITOR_UNDO_DEPTH
        || presentation.undo_bytes > MAX_EDITOR_UNDO_BYTES
    {
        match presentation.undo.first() {
            Some(oldest) => {
                presentation.undo_bytes = presentation.undo_bytes.saturating_sub(oldest.bytes)
            }
            None => {
                presentation.undo_bytes = 0;
                break;
            }
        }
        presentation.undo.remove(0);
    }
    while presentation.redo.len() > MAX_EDITOR_UNDO_DEPTH
        || presentation.redo_bytes > MAX_EDITOR_UNDO_BYTES
    {
        match presentation.redo.first() {
            Some(oldest) => {
                presentation.redo_bytes = presentation.redo_bytes.saturating_sub(oldest.bytes)
            }
            None => {
                presentation.redo_bytes = 0;
                break;
            }
        }
        presentation.redo.remove(0);
    }
}

/// Drop all undo and redo history.
///
/// Called on every reconciliation transition (`resync`, `reconnect`,
/// `reload_from_disk`, `close`, and opening a different document). A
/// divergent draft is retained by the controller in a recovery state; the
/// editor surfaces that state and never replays or discards the draft on the
/// user's behalf, so it must not keep inverse transactions that no longer
/// describe the local text.
pub fn clear_history(presentation: &mut TuiDocumentPresentation) {
    presentation.undo.clear();
    presentation.redo.clear();
    presentation.undo_bytes = 0;
    presentation.redo_bytes = 0;
    presentation.pending_command.clear();
}

// ── Viewport ─────────────────────────────────────────────────────────────────

/// Gutter width for a document, in columns.
///
/// Sized from the document's line count so it does not jitter as the cursor
/// moves, and stable across documents of the same length.
pub fn gutter_width(line_count: usize) -> u16 {
    let digits = line_count.max(1).to_string().len();
    (digits as u16).saturating_add(1)
}

/// Result of resolving a viewport against the current presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorViewport {
    /// First visible line, zero-based.
    pub first_line: usize,
    /// Horizontal scroll in display columns.
    pub column: usize,
    /// Gutter width in columns.
    pub gutter: u16,
    /// Columns available for text after the gutter and scrollbar.
    pub text_columns: u16,
    /// Number of visible lines.
    pub visible_lines: u16,
}

/// Resolve the viewport so the cursor is on screen, keeping the line-to-row
/// mapping one-to-one (hard wrap) and clamping the horizontal scroll to the
/// longest visible line.
pub fn resolve_viewport(
    presentation: &mut TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    area_width: u16,
    area_height: u16,
) -> EditorViewport {
    let gutter = gutter_width(snapshot.len_lines());
    let text_columns = area_width
        .saturating_sub(gutter)
        .saturating_sub(1)
        .max(MIN_EDITOR_TEXT_COLUMNS);
    let visible_lines = area_height.max(1);
    let cursor_line = cursor_line(snapshot, presentation.cursor_byte);
    let last_line = snapshot.len_lines().saturating_sub(1);
    let max_first = last_line.saturating_sub(visible_lines.saturating_sub(1) as usize);

    // Vertical: move the window minimally to keep the cursor line on screen.
    let mut first = presentation.viewport_line.min(max_first);
    if cursor_line < first {
        first = cursor_line;
    } else if cursor_line >= first + visible_lines as usize {
        first = cursor_line + 1 - visible_lines as usize;
    }
    first = first.min(max_first);

    // Horizontal: keep the cursor's display column on screen, then clamp to
    // the longest visible line so the view never scrolls past content.
    let cursor_start = line_start(snapshot, cursor_line).unwrap_or(0);
    let cursor_column = display_width(snapshot, cursor_start..presentation.cursor_byte);
    let mut column = presentation.viewport_column;
    if cursor_column < column {
        column = cursor_column;
    } else if cursor_column >= column + text_columns as usize {
        column = cursor_column + 1 - text_columns as usize;
    }
    let max_column = longest_visible_width(snapshot, first, visible_lines as usize)
        .saturating_sub(text_columns as usize);
    column = column.min(max_column);

    presentation.viewport_line = first;
    presentation.viewport_column = column;
    EditorViewport {
        first_line: first,
        column,
        gutter,
        text_columns,
        visible_lines,
    }
}

/// Display width of a byte range, in terminal columns.
pub fn display_width(snapshot: &DocumentSnapshot, range: std::ops::Range<usize>) -> usize {
    snapshot
        .read_bytes(range.start, range.end)
        .map(|text| {
            use unicode_width::UnicodeWidthStr;
            UnicodeWidthStr::width(text.as_str())
        })
        .unwrap_or(0)
}

/// Longest display width among the visible lines, in terminal columns.
pub fn longest_visible_width(
    snapshot: &DocumentSnapshot,
    first_line: usize,
    visible_lines: usize,
) -> usize {
    let mut widest = 0usize;
    for line in first_line..(first_line + visible_lines) {
        if line >= snapshot.len_lines() {
            break;
        }
        let Some(start) = line_start(snapshot, line) else {
            break;
        };
        let end = line_content_end(snapshot, line).unwrap_or(start);
        widest = widest.max(display_width(snapshot, start..end));
    }
    widest
}

/// Display column of `offset` within its own line.
pub fn display_column(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let offset = clamp_offset(snapshot, offset);
    let start = line_start(snapshot, cursor_line(snapshot, offset)).unwrap_or(0);
    display_width(snapshot, start..offset)
}

/// Byte offset for a display column on `line`, clamped to the line's content
/// end so a click or motion can never land past the terminator.
pub fn offset_at_column(snapshot: &DocumentSnapshot, line: usize, column: usize) -> Option<usize> {
    let start = line_start(snapshot, line)?;
    let end = line_content_end(snapshot, line)?;
    let mut offset = start;
    let mut consumed = 0usize;
    while offset < end {
        // `next_char_boundary` is `None` at the end of the document, which
        // is a legitimate position here, not a failure.
        let next = next_char_boundary(snapshot, offset).unwrap_or(end).min(end);
        if next == offset {
            break;
        }
        let width = display_width(snapshot, offset..next);
        if consumed + width > column {
            break;
        }
        consumed += width;
        offset = next;
    }
    Some(clamp_offset(snapshot, offset.min(end)))
}

/// Resolve a `BytePosition` into a clamped byte offset.
pub fn offset_for_position(snapshot: &DocumentSnapshot, position: BytePosition) -> usize {
    snapshot
        .position_to_byte(position)
        .ok()
        .map(|offset| clamp_offset(snapshot, offset))
        .unwrap_or_else(|| clamp_offset(snapshot, snapshot.len_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_document::{DocumentBuffer, DocumentLimits};

    fn snapshot_of(text: &str) -> DocumentSnapshot {
        DocumentBuffer::new(text).snapshot()
    }

    fn presentation_at(cursor: usize) -> TuiDocumentPresentation {
        TuiDocumentPresentation {
            cursor_byte: cursor,
            ..TuiDocumentPresentation::default()
        }
    }

    // ── Offset clamping ───────────────────────────────────────────────

    #[test]
    fn clamp_lands_on_a_char_boundary_inside_a_multibyte_char() {
        let snapshot = snapshot_of("aé€b");
        // é is 2 bytes, € is 3 bytes; offset 2 splits é.
        assert_eq!(clamp_offset(&snapshot, 2), 1);
        assert_eq!(clamp_offset(&snapshot, 4), 3);
        assert_eq!(clamp_offset(&snapshot, 6), 6);
    }

    #[test]
    fn clamp_on_empty_document_is_zero() {
        let snapshot = snapshot_of("");
        assert_eq!(snapshot.len_bytes(), 0);
        assert_eq!(clamp_offset(&snapshot, 0), 0);
        assert_eq!(clamp_offset(&snapshot, 99), 0);
        assert_eq!(buffer_content_end(&snapshot), 0);
        assert_eq!(cursor_line(&snapshot, 0), 0);
    }

    #[test]
    fn cursor_past_end_clamps_to_end() {
        let snapshot = snapshot_of("hello");
        assert_eq!(clamp_offset(&snapshot, 5), 5);
        assert_eq!(clamp_offset(&snapshot, 4096), 5);
    }

    #[test]
    fn selection_inversion_resolves_to_an_ordered_range() {
        let snapshot = snapshot_of("abcdef");
        let mut presentation = presentation_at(1);
        // Built from variables so the literal reversed range does not trip
        // `clippy::reversed_empty_ranges`; an inverted anchor is the point.
        let (anchor, cursor) = (4usize, 2usize);
        presentation.selection = Some(anchor..cursor);
        let range = normalized_selection(&presentation, &snapshot);
        assert_eq!(range, Some(2..4));
    }

    #[test]
    fn line_and_byte_conversion_round_trips() {
        let snapshot = snapshot_of("one\ntwo\nthree");
        for offset in 0..=snapshot.len_bytes() {
            let position = snapshot.byte_to_position(offset).unwrap();
            assert_eq!(snapshot.position_to_byte(position).unwrap(), offset);
        }
        assert_eq!(cursor_line(&snapshot, 5), 1);
        assert_eq!(line_start(&snapshot, 1), Some(4));
        assert_eq!(line_content_end(&snapshot, 1), Some(7));
        assert_eq!(buffer_content_end(&snapshot), 13);
    }

    #[test]
    fn char_boundary_walk_crosses_multibyte_text() {
        let snapshot = snapshot_of("aé€b");
        assert_eq!(next_char_boundary(&snapshot, 0), Some(1));
        assert_eq!(next_char_boundary(&snapshot, 1), Some(3));
        assert_eq!(next_char_boundary(&snapshot, 3), Some(6));
        assert_eq!(next_char_boundary(&snapshot, 6), None);
        assert_eq!(prev_char_boundary(&snapshot, 6), 3);
        assert_eq!(prev_char_boundary(&snapshot, 3), 1);
        assert_eq!(prev_char_boundary(&snapshot, 1), 0);
    }

    // ── Motion ────────────────────────────────────────────────────────

    #[test]
    fn horizontal_motion_stops_at_line_content_not_terminator() {
        let snapshot = snapshot_of("ab\ncd");
        // Offset 3 is 'c', the first byte of line 1.
        let mut presentation = presentation_at(3);
        apply_motion(&mut presentation, &snapshot, EditorCommand::LineEnd, 10);
        assert_eq!(presentation.cursor_byte, 5);
        apply_motion(&mut presentation, &snapshot, EditorCommand::LineStart, 10);
        assert_eq!(presentation.cursor_byte, 3);
        apply_motion(&mut presentation, &snapshot, EditorCommand::BufferEnd, 10);
        assert_eq!(presentation.cursor_byte, 5);
        apply_motion(&mut presentation, &snapshot, EditorCommand::BufferStart, 10);
        assert_eq!(presentation.cursor_byte, 0);
    }

    #[test]
    fn vertical_motion_clamps_to_shorter_target_lines() {
        let snapshot = snapshot_of("longest line here\nab\nanother long line");
        let mut presentation = presentation_at(10);
        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveDown, 10);
        // Target line "ab" is shorter, so the cursor clamps to its content end.
        assert_eq!(presentation.cursor_byte, 20);
        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveUp, 10);
        // Column 2 is preserved relative to the line start, not to byte 10.
        assert_eq!(presentation.cursor_byte, 2);
    }

    #[test]
    fn word_motion_finds_word_boundaries() {
        let snapshot = snapshot_of("alpha beta  gamma");
        assert_eq!(word_forward(&snapshot, 0), 6);
        assert_eq!(
            word_forward(&snapshot, 6),
            12,
            "skips both separator spaces"
        );
        assert_eq!(word_backward(&snapshot, 12), 6);
        assert_eq!(word_backward(&snapshot, 16), 12, "lands on the word start");
    }

    // ── Viewport ──────────────────────────────────────────────────────

    #[test]
    fn gutter_width_is_stable_across_cursor_movement() {
        let snapshot = snapshot_of(&"line\n".repeat(9));
        let first = gutter_width(snapshot.len_lines());
        let mut presentation = presentation_at(0);
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 10);
        assert_eq!(viewport.gutter, first);
        presentation.cursor_byte = snapshot.len_bytes();
        let moved = resolve_viewport(&mut presentation, &snapshot, 40, 10);
        assert_eq!(
            moved.gutter, first,
            "gutter must not jitter with the cursor"
        );
    }

    #[test]
    fn gutter_width_grows_only_with_line_count_digits() {
        assert_eq!(gutter_width(1), 2);
        assert_eq!(gutter_width(9), 2);
        assert_eq!(gutter_width(10), 3);
        assert_eq!(gutter_width(100), 4);
    }

    #[test]
    fn vertical_scroll_follows_the_cursor_and_clamps_at_the_last_page() {
        let snapshot = snapshot_of(&"line\n".repeat(30));
        let mut presentation = presentation_at(0);
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        assert_eq!(viewport.first_line, 0);
        presentation.cursor_byte = snapshot.len_bytes();
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        // The trailing newline yields a 31st (empty) line, so the last
        // page starts at 31 - 5.
        assert_eq!(viewport.first_line, 31 - 5);
        presentation.cursor_byte = 0;
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        assert_eq!(viewport.first_line, 0, "scroll clamps at the top edge");
    }

    #[test]
    fn horizontal_scroll_tracks_the_cursor_and_clamps_at_the_right_edge() {
        let long = "x".repeat(400);
        let snapshot = snapshot_of(&long);
        let mut presentation = presentation_at(0);
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        assert_eq!(viewport.column, 0);
        presentation.cursor_byte = 400;
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        assert!(viewport.column > 0, "cursor far right must scroll right");
        let max = longest_visible_width(&snapshot, viewport.first_line, 5)
            - viewport.text_columns as usize;
        assert_eq!(viewport.column, max, "scroll clamps at the buffer edge");
    }

    #[test]
    fn narrow_area_still_yields_a_usable_text_column() {
        let snapshot = snapshot_of("hello world");
        let mut presentation = presentation_at(0);
        let viewport = resolve_viewport(&mut presentation, &snapshot, 1, 1);
        assert_eq!(viewport.text_columns, 1);
        assert_eq!(viewport.visible_lines, 1);
    }

    // ── Edits and inverse transactions ────────────────────────────────

    #[test]
    fn inverse_of_an_insertion_restores_the_original_text() {
        let snapshot = snapshot_of("hello");
        let transaction = TextTransaction::new(vec![TextEdit::new(5..5, " world")]);
        let inverse = invert(&snapshot, &transaction).unwrap();
        assert_eq!(inverse.edits, vec![TextEdit::new(5..11, "")]);

        let mut buffer = DocumentBuffer::new("hello");
        let applied = buffer
            .apply(&transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(applied.snapshot.to_string(), "hello world");
        let restored = buffer.apply(&inverse, DocumentLimits::default()).unwrap();
        assert_eq!(restored.snapshot.to_string(), "hello");
    }

    #[test]
    fn inverse_of_a_deletion_restores_the_removed_text() {
        let snapshot = snapshot_of("hello world");
        let transaction = TextTransaction::new(vec![TextEdit::new(5..11, "")]);
        let inverse = invert(&snapshot, &transaction).unwrap();
        assert_eq!(inverse.edits, vec![TextEdit::new(5..5, " world")]);
    }

    #[test]
    fn inverse_matches_the_controllers_own_inverse_for_multi_edit_transactions() {
        let snapshot = snapshot_of("0123456789");
        let transaction =
            TextTransaction::new(vec![TextEdit::new(0..2, "AA"), TextEdit::new(5..7, "BB")]);
        let mut buffer = DocumentBuffer::new("0123456789");
        let controller_inverse = buffer
            .apply(&transaction, DocumentLimits::default())
            .unwrap()
            .inverse;
        assert_eq!(
            invert(&snapshot, &transaction).unwrap(),
            controller_inverse,
            "frontend inverse must match DocumentBuffer::apply exactly"
        );
    }

    #[test]
    fn resolve_edit_produces_the_expected_delete_line_range() {
        let snapshot = snapshot_of("one\ntwo\nthree");
        let presentation = presentation_at(4);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::DeleteLine).unwrap();
        assert_eq!(edit.transaction.edits, vec![TextEdit::new(4..8, "")]);
        let mut buffer = DocumentBuffer::new("one\ntwo\nthree");
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(buffer.snapshot().to_string(), "one\nthree");
    }

    #[test]
    fn delete_line_on_the_last_line_removes_only_that_line() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(4);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::DeleteLine).unwrap();
        assert_eq!(edit.transaction.edits, vec![TextEdit::new(4..7, "")]);
    }

    #[test]
    fn delete_char_at_the_end_of_the_buffer_is_a_noop() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(7);
        assert!(resolve_edit(&snapshot, &presentation, EditorCommand::DeleteChar).is_none());
    }

    #[test]
    fn delete_char_on_a_newline_joins_the_next_line() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(3);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::DeleteChar).unwrap();
        assert_eq!(edit.transaction.edits, vec![TextEdit::new(3..4, "")]);
        let mut buffer = DocumentBuffer::new("one\ntwo");
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(buffer.snapshot().to_string(), "onetwo");
    }

    #[test]
    fn open_line_below_splits_the_existing_terminator() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(0);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::OpenLineBelow).unwrap();
        let mut buffer = DocumentBuffer::new("one\ntwo");
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(buffer.snapshot().to_string(), "one\n\ntwo");
        assert!(edit.stay_insert);
    }

    #[test]
    fn open_line_above_the_first_line_preserves_a_leading_blank() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(0);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::OpenLineAbove).unwrap();
        let mut buffer = DocumentBuffer::new("one\ntwo");
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(buffer.snapshot().to_string(), "\none\ntwo");
    }

    // ── Undo history ──────────────────────────────────────────────────

    #[test]
    fn undo_history_pushes_caps_and_records_bytes() {
        let snapshot = snapshot_of("hello world");
        let mut presentation = presentation_at(0);
        let edit = insert_text(&snapshot, 0, "abc").unwrap();
        record_undo(&mut presentation, &edit);
        assert_eq!(presentation.undo.len(), 1);
        assert!(presentation.undo_bytes > 0);
        assert!(presentation.redo.is_empty());

        let entry = take_undo(&mut presentation).unwrap();
        assert_eq!(presentation.undo.len(), 0);
        assert_eq!(presentation.undo_bytes, 0);
        record_redo(&mut presentation, entry);
        assert_eq!(presentation.redo.len(), 1);
    }

    #[test]
    fn undo_history_drops_the_oldest_entry_past_the_depth_cap() {
        let mut presentation = presentation_at(0);
        for _ in 0..(MAX_EDITOR_UNDO_DEPTH + 10) {
            let edit = EditorEdit {
                transaction: TextTransaction::new(vec![TextEdit::new(0..0, "x")]),
                inverse: TextTransaction::new(vec![TextEdit::new(0..1, "")]),
                bytes: 1,
                cursor_after: 0,
                stay_insert: false,
            };
            let _ = &edit;
            record_undo(&mut presentation, &edit);
        }
        assert_eq!(presentation.undo.len(), MAX_EDITOR_UNDO_DEPTH);
    }

    #[test]
    fn undo_history_drops_the_oldest_entry_past_the_byte_cap() {
        let mut presentation = presentation_at(0);
        let big = MAX_EDITOR_UNDO_BYTES / 4;
        for _ in 0..8 {
            let edit = EditorEdit {
                transaction: TextTransaction::new(vec![TextEdit::new(0..0, "x")]),
                inverse: TextTransaction::new(vec![TextEdit::new(0..1, "")]),
                bytes: big,
                cursor_after: 0,
                stay_insert: false,
            };
            record_undo(&mut presentation, &edit);
        }
        assert!(presentation.undo_bytes <= MAX_EDITOR_UNDO_BYTES);
        assert!(presentation.undo.len() < 8);
    }

    #[test]
    fn rejected_undo_entry_returns_to_the_history_unchanged() {
        let snapshot = snapshot_of("hello");
        let mut presentation = presentation_at(0);
        let edit = insert_text(&snapshot, 0, "abc").unwrap();
        record_undo(&mut presentation, &edit);
        let entry = take_undo(&mut presentation).unwrap();
        let bytes = entry.bytes;
        restore_undo(&mut presentation, entry);
        assert_eq!(presentation.undo.len(), 1);
        assert_eq!(presentation.undo_bytes, bytes);
    }

    #[test]
    fn clear_history_drops_undo_redo_and_pending_command() {
        let snapshot = snapshot_of("hello");
        let mut presentation = presentation_at(0);
        record_undo(
            &mut presentation,
            &insert_text(&snapshot, 0, "abc").unwrap(),
        );
        presentation.pending_command = "d".into();
        clear_history(&mut presentation);
        assert!(presentation.undo.is_empty());
        assert!(presentation.redo.is_empty());
        assert_eq!(presentation.undo_bytes, 0);
        assert_eq!(presentation.redo_bytes, 0);
        assert!(presentation.pending_command.is_empty());
    }

    #[test]
    fn reset_for_open_clears_history_and_viewport() {
        let mut presentation = presentation_at(42);
        presentation.viewport_line = 9;
        presentation.viewport_column = 7;
        presentation.mode = EditorMode::Insert;
        presentation.reset_for_open();
        assert_eq!(presentation.cursor_byte, 0);
        assert_eq!(presentation.viewport_line, 0);
        assert_eq!(presentation.viewport_column, 0);
        assert_eq!(presentation.mode, EditorMode::Normal);
        assert_eq!(presentation.focus, EditorFocus::Composer);
    }

    // ── Display helpers ───────────────────────────────────────────────

    #[test]
    fn display_column_counts_terminal_columns_not_bytes() {
        let snapshot = snapshot_of("aé€b");
        assert_eq!(display_column(&snapshot, 0), 0);
        assert_eq!(display_column(&snapshot, 1), 1);
        assert_eq!(display_column(&snapshot, 3), 2);
        assert_eq!(display_column(&snapshot, 6), 3);
    }

    #[test]
    fn offset_at_column_never_passes_the_line_content_end() {
        let snapshot = snapshot_of("ab\ncdef");
        assert_eq!(offset_at_column(&snapshot, 0, 0), Some(0));
        assert_eq!(offset_at_column(&snapshot, 0, 2), Some(2));
        assert_eq!(offset_at_column(&snapshot, 0, 99), Some(2));
        assert_eq!(offset_at_column(&snapshot, 1, 2), Some(5));
        assert_eq!(offset_at_column(&snapshot, 1, 4), Some(7));
    }
}
