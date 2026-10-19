//! The composer's text model.
//!
//! The composer is a **frontend-local string**: it is typed into, it is
//! rendered, and it is delivered to the daemon only on submit. It is not a
//! document, it has no revision, and nothing outside the frontend can read
//! it while it is being written.
//!
//! That is why it had no model. [`crate::tui::components::prompt::PromptWidget`]
//! held a `String` and a `usize` and did the arithmetic itself, which was
//! enough to type a prompt and not enough for anything else. The Helix verbs
//! need three things the widget did not have:
//!
//! - a **point type** that satisfies the shared selection contract, so the
//!   composer's `v` and `x` are the same rules as the editor's and the
//!   terminal panel's rather than a third implementation;
//! - a **selection state**, held by the shared state machine rather than by
//!   the widget;
//! - a **bounded undo history**, so `u` and `U` can mean something.
//!
//! This module is that model. The widget keeps its rendering and its public
//! API; the buffer, the caret, the selection and the history live here.
//!
//! ## What is deliberately not here
//!
//! No key handling and no rendering. The verbs that will use this model —
//! `w`, `b`, `e`, `v`, `x`, `d`, `p`, `u`, `U` — are later milestones, and
//! until they land the model is exercised only by its own tests and by the
//! widget's existing behaviour. A model that grew a key handler would be
//! two owners for one key.

use std::cmp::Ordering;

use crate::tui::helix_selection::{Selection, SelectionMode, SelectionPoint, SelectionState};
use crate::tui::text_geometry::{self, TextSource};

/// A position in the composer's buffer.
///
/// The editor's counterpart is `EditorPoint`, and the terminal's is a
/// viewport cell. This is the composer's own: a byte offset paired with the
/// line it sits on, because [`SelectionPoint`] separates the line from the
/// point so whole-line selection can answer "which lines does this span"
/// without knowing how wide a line is.
///
/// The composer is a **peer** of the editor, not a subordinate of it, so it
/// does not borrow the editor's point type. The shape is the same because the
/// shared selection contract wants the same shape, not because one surface
/// owns the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerPoint {
    /// UTF-8 byte offset into the composer's text.
    pub offset: usize,
    /// Zero-based line containing `offset`.
    pub line: usize,
}

impl ComposerPoint {
    /// A point at `offset` on `line`.
    pub fn new(offset: usize, line: usize) -> Self {
        Self { offset, line }
    }
}

/// The points order by **reading order**: top to bottom, then left to right.
///
/// [`SelectionPoint`] requires the surface's own reading order, because a
/// selection's start is `min(anchor, head)`. For a single text buffer the
/// byte offsets already increase with reading order, so ordering by the line
/// first and the offset second is the same answer — but it is the *stated*
/// contract rather than a coincidence, and a derived `Ord` on the fields
/// would have compared the offset first and answered a different question
/// for a point whose line disagreed with its offset.
impl Ord for ComposerPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.line, self.offset).cmp(&(other.line, other.offset))
    }
}

impl PartialOrd for ComposerPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl SelectionPoint for ComposerPoint {
    type Line = usize;

    fn line(&self) -> usize {
        self.line
    }
}

/// One reversible change to the composer's buffer.
///
/// The edit records what was there and what replaced it, so undo and redo are
/// inverses of each other over the same range. A snapshot of the whole buffer
/// would be simpler, but the composer is typed into continuously and a
/// snapshot per keystroke would make the history grow with the *buffer*
/// rather than with the *changes* — which is the thing a bounded history has
/// to bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Byte offset where the edit starts, in the text before the edit.
    pub start: usize,
    /// The text that was there.
    pub removed: String,
    /// The text that replaced it.
    pub inserted: String,
    /// The caret before the edit.
    pub caret_before: usize,
    /// The caret after the edit.
    pub caret_after: usize,
}

impl Edit {
    /// How many bytes this edit keeps alive in the history.
    fn weight(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

/// The default number of edits the history keeps.
pub const MAX_UNDO_DEPTH: usize = 200;

/// The default number of bytes the history keeps.
///
/// A prompt is short, but a paste can be long, and a history of 200 pasted
/// blocks would be 200 copies of a document. The byte bound is what stops
/// that.
pub const MAX_UNDO_BYTES: usize = 64 * 1024;

/// A bounded undo/redo history.
///
/// Two stacks and a byte budget. The budget is what makes it *bounded* in the
/// sense that matters: a depth bound alone still lets one enormous paste sit
/// in the history, and a byte bound alone still lets a thousand one-character
/// edits sit in it.
///
/// Eviction removes the **oldest** undo entry. That is only safe because a
/// new edit clears the redo stack first: the redo stack is empty whenever an
/// eviction can happen, so an eviction can never strand a redo entry whose
/// undo counterpart has been thrown away.
#[derive(Debug, Clone)]
pub struct UndoHistory {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    max_depth: usize,
    max_bytes: usize,
    bytes: usize,
}

impl Default for UndoHistory {
    fn default() -> Self {
        Self::new(MAX_UNDO_DEPTH, MAX_UNDO_BYTES)
    }
}

impl UndoHistory {
    /// A history bounded to `max_depth` edits and `max_bytes` bytes.
    pub fn new(max_depth: usize, max_bytes: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            max_depth,
            max_bytes,
            bytes: 0,
        }
    }

    /// Record a new edit, discarding the redo stack.
    pub fn push(&mut self, edit: Edit) {
        self.redo.clear();
        self.bytes += edit.weight();
        self.undo.push(edit);
        self.evict();
    }

    /// Take the newest undoable edit, moving it to the redo stack.
    pub fn take_undo(&mut self) -> Option<Edit> {
        let edit = self.undo.pop()?;
        self.bytes = self.bytes.saturating_sub(edit.weight());
        self.redo.push(edit.clone());
        Some(edit)
    }

    /// Take the newest redoable edit, moving it back to the undo stack.
    pub fn take_redo(&mut self) -> Option<Edit> {
        let edit = self.redo.pop()?;
        self.bytes += edit.weight();
        self.undo.push(edit.clone());
        Some(edit)
    }

    /// Drop every recorded edit.
    ///
    /// The composer calls this when its text is replaced or submitted: an
    /// undo must not be able to bring back text the user has already sent.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }

    /// How many edits can still be undone.
    pub fn depth(&self) -> usize {
        self.undo.len()
    }

    /// How many bytes the undo stack is holding.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whether an edit can be redone.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Drop the oldest edits until both bounds hold.
    fn evict(&mut self) {
        while self.undo.len() > self.max_depth || self.bytes > self.max_bytes {
            match self.undo.first() {
                Some(oldest) => {
                    let weight = oldest.weight();
                    self.undo.remove(0);
                    self.bytes = self.bytes.saturating_sub(weight);
                }
                None => break,
            }
        }
    }
}

/// The composer's text, caret, selection and history.
///
/// The caret is the shared selection machine's cursor, not a second copy of
/// it. A surface that kept its own cursor next to the machine's would have
/// two answers to "where is the caret", and the selection's growth rule would
/// be applied to one of them.
#[derive(Debug, Clone)]
pub struct ComposerText {
    text: String,
    state: SelectionState<ComposerPoint>,
    undo: UndoHistory,
}

impl Default for ComposerText {
    fn default() -> Self {
        Self {
            text: String::new(),
            state: SelectionState::new(ComposerPoint::new(0, 0)),
            undo: UndoHistory::default(),
        }
    }
}

impl ComposerText {
    /// An empty buffer with the caret at the start.
    pub fn new() -> Self {
        Self::default()
    }

    /// The buffer's text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The buffer's length in bytes.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The caret's byte offset.
    pub fn caret(&self) -> usize {
        self.state.cursor().offset
    }

    /// The caret's line.
    pub fn caret_line(&self) -> usize {
        self.state.cursor().line
    }

    /// The held selection, if any.
    pub fn selection(&self) -> Option<Selection<ComposerPoint>> {
        self.state.selection()
    }

    /// Whether a selection is held.
    pub fn is_selected(&self) -> bool {
        self.state.is_selected()
    }

    /// The undo history.
    pub fn history(&self) -> &UndoHistory {
        &self.undo
    }

    /// Replace the buffer's text and drop the history.
    ///
    /// Replacing the text is not an edit the user made, so it is not
    /// undoable, and the old history would be meaningless against the new
    /// text.
    ///
    /// The caret lands at the **end** of the new text, which is where a
    /// freshly-restored prompt should be: the user is about to keep typing
    /// at the end of what they had.
    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.undo.clear();
        let caret = self.text.len();
        self.state = SelectionState::new(self.point_at(caret));
    }

    /// Empty the buffer and drop the history.
    pub fn clear(&mut self) {
        self.text.clear();
        self.undo.clear();
        self.state = SelectionState::new(self.point_at(0));
    }

    /// Put the caret at `offset`, clamped onto a character boundary.
    pub fn set_caret(&mut self, offset: usize) {
        let offset = self.aligned(offset);
        self.state.set_cursor(self.point_at(offset));
    }

    /// Move the caret to `offset` as a deliberate movement.
    ///
    /// This is the path that extends a held character selection, because
    /// "moving the cursor extends the selection" is a rule of the shared
    /// machine, not a drawing detail.
    pub fn move_caret(&mut self, offset: usize) {
        let offset = self.aligned(offset);
        self.state.move_to(self.point_at(offset));
    }

    /// Move the caret one character left.
    pub fn move_left(&mut self) {
        let current = self.caret();
        let previous = text_geometry::prev_char_boundary(self.text.as_str(), current);
        self.move_caret(previous);
    }

    /// Move the caret one character right.
    pub fn move_right(&mut self) {
        let current = self.caret();
        let next = text_geometry::next_char_boundary(self.text.as_str(), current);
        self.move_caret(next.unwrap_or(self.text.len()));
    }

    /// Move the caret to the start of the buffer.
    pub fn move_home(&mut self) {
        self.move_caret(0);
    }

    /// Move the caret to the end of the buffer.
    pub fn move_end(&mut self) {
        self.move_caret(self.text.len());
    }

    /// The start of the next word, per the shared geometry.
    pub fn word_forward(&self) -> usize {
        text_geometry::word_forward(self.text.as_str(), self.caret())
    }

    /// The start of the previous word, per the shared geometry.
    pub fn word_backward(&self) -> usize {
        text_geometry::word_backward(self.text.as_str(), self.caret())
    }

    /// The end of the word at the caret, per the shared geometry.
    pub fn word_end(&self) -> usize {
        text_geometry::word_end(self.text.as_str(), self.caret())
    }

    /// Insert `text` at the caret.
    pub fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let caret = self.caret();
        self.replace_range(caret, caret, text, caret + text.len());
    }

    /// Insert a newline at the caret.
    pub fn insert_newline(&mut self) {
        let caret = self.caret();
        self.replace_range(caret, caret, "\n", caret + 1);
    }

    /// Delete the character before the caret.
    pub fn backspace(&mut self) {
        let caret = self.caret();
        if caret == 0 {
            return;
        }
        let start = text_geometry::prev_char_boundary(self.text.as_str(), caret);
        self.replace_range(start, caret, "", start);
    }

    /// Delete the character under the caret.
    pub fn delete_forward(&mut self) {
        let caret = self.caret();
        if caret >= self.text.len() {
            return;
        }
        let end =
            text_geometry::next_char_boundary(self.text.as_str(), caret).unwrap_or(self.text.len());
        self.replace_range(caret, end, "", caret);
    }

    /// Delete the held selection, if there is one.
    ///
    /// Returns whether anything was deleted. The deletion is an ordinary
    /// edit, so it is undoable like any other.
    ///
    /// The ranges follow the editor's convention, so the two surfaces delete
    /// the same text for the same selection: a character selection is
    /// **inclusive of the head character** (`v` then `l` selects two
    /// characters, not one-and-a-bit), and a line selection spans whole lines
    /// including their terminators.
    pub fn delete_selection(&mut self) -> bool {
        let Some(selection) = self.state.selection() else {
            return false;
        };
        let (first, last) = selection.bounds();
        let (start, end) = match selection.mode {
            SelectionMode::Char => {
                let start = self.aligned(first.offset);
                let end = text_geometry::next_char_boundary(self.text.as_str(), last.offset)
                    .unwrap_or(last.offset);
                (start, end)
            }
            SelectionMode::Line => {
                let start = self.text.as_str().line_start(first.line).unwrap_or(0);
                let end = self
                    .text
                    .as_str()
                    .line_start(last.line + 1)
                    .unwrap_or(self.text.len())
                    .min(self.text.len());
                (start, end)
            }
        };
        if start >= end {
            return false;
        }
        self.state.deactivate();
        self.replace_range(start, end, "", start);
        true
    }

    /// Undo the newest edit.
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.take_undo() else {
            return false;
        };
        let end = edit.start + edit.inserted.len();
        self.text.replace_range(edit.start..end, &edit.removed);
        self.set_caret(edit.caret_before);
        true
    }

    /// Redo the newest undone edit.
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.undo.take_redo() else {
            return false;
        };
        let end = edit.start + edit.removed.len();
        self.text.replace_range(edit.start..end, &edit.inserted);
        self.set_caret(edit.caret_after);
        true
    }

    /// Replace `start..end` with `replacement` and record the edit.
    fn replace_range(&mut self, start: usize, end: usize, replacement: &str, caret: usize) {
        let removed = self.text[start..end].to_string();
        let edit = Edit {
            start,
            removed: removed.clone(),
            inserted: replacement.to_string(),
            caret_before: self.caret(),
            caret_after: caret,
        };
        self.text.replace_range(start..end, replacement);
        self.undo.push(edit);
        self.set_caret(caret);
    }

    /// Clamp `offset` into the buffer and onto a character boundary.
    fn aligned(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        if self.text.is_char_boundary(offset) {
            return offset;
        }
        self.text
            .char_indices()
            .map(|(boundary, _)| boundary)
            .take_while(|&boundary| boundary < offset)
            .last()
            .unwrap_or(0)
    }

    /// The point at `offset`, with the line it sits on.
    fn point_at(&self, offset: usize) -> ComposerPoint {
        let offset = offset.min(self.text.len());
        let line = text_geometry::cursor_line(self.text.as_str(), offset);
        ComposerPoint::new(offset, line)
    }
}

impl TextSource for ComposerText {
    fn len_bytes(&self) -> usize {
        self.text.len()
    }

    fn read_range(&self, start: usize, end: usize) -> Option<String> {
        self.text.get(start..end).map(|slice| slice.to_string())
    }

    fn is_char_boundary(&self, offset: usize) -> bool {
        self.text.is_char_boundary(offset)
    }

    fn len_lines(&self) -> usize {
        self.text.split('\n').count()
    }

    fn line_start(&self, line: usize) -> Option<usize> {
        self.text.as_str().line_start(line)
    }

    fn line_end(&self, line: usize) -> Option<usize> {
        self.text.as_str().line_end(line)
    }

    fn line_of(&self, offset: usize) -> usize {
        text_geometry::cursor_line(self.text.as_str(), offset)
    }

    fn byte_at(&self, offset: usize) -> Option<u8> {
        self.text.as_bytes().get(offset).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> ComposerText {
        let mut buffer = ComposerText::new();
        buffer.insert(text);
        buffer
    }

    #[test]
    fn a_point_carries_its_line() {
        let buffer = typed("one\ntwo\nthree");
        let point = buffer.point_at(5);
        assert_eq!(point.offset, 5);
        assert_eq!(point.line, 1);
        assert_eq!(point.line(), 1);
    }

    #[test]
    fn the_caret_sits_where_the_text_was_typed() {
        let mut buffer = ComposerText::new();
        buffer.insert("hello");
        assert_eq!(buffer.caret(), 5);
        buffer.move_left();
        assert_eq!(buffer.caret(), 4);
        buffer.move_right();
        assert_eq!(buffer.caret(), 5);
    }

    #[test]
    fn undo_and_redo_are_inverse() {
        let mut buffer = typed("hello");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "");
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "hello");
        assert_eq!(buffer.caret(), 5);
    }

    #[test]
    fn undo_walks_back_through_several_edits() {
        let mut buffer = ComposerText::new();
        buffer.insert("a");
        buffer.insert("b");
        buffer.insert("c");
        assert_eq!(buffer.text(), "abc");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "ab");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "a");
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "ab");
    }

    #[test]
    fn replacing_the_text_drops_the_history() {
        let mut buffer = typed("hello");
        buffer.set_text("replaced".to_string());
        assert!(!buffer.undo(), "a replaced buffer must not be undoable");
        assert_eq!(buffer.text(), "replaced");
    }

    #[test]
    fn clearing_the_text_drops_the_history() {
        let mut buffer = typed("hello");
        buffer.clear();
        assert!(!buffer.undo(), "a submitted buffer must not be undoable");
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn the_history_is_bounded_in_depth() {
        let mut history = UndoHistory::new(3, usize::MAX);
        for index in 0..10 {
            history.push(Edit {
                start: 0,
                removed: String::new(),
                inserted: index.to_string(),
                caret_before: 0,
                caret_after: 0,
            });
        }
        assert_eq!(history.depth(), 3);
    }

    #[test]
    fn the_history_is_bounded_in_bytes() {
        let mut history = UndoHistory::new(usize::MAX, 16);
        for _ in 0..10 {
            history.push(Edit {
                start: 0,
                removed: "x".repeat(8),
                inserted: "y".repeat(8),
                caret_before: 0,
                caret_after: 0,
            });
        }
        assert!(
            history.bytes() <= 16,
            "the history held {} bytes, over its bound",
            history.bytes()
        );
    }

    #[test]
    fn an_eviction_does_not_strand_a_redo() {
        let mut history = UndoHistory::new(2, usize::MAX);
        for index in 0..5 {
            history.push(Edit {
                start: 0,
                removed: String::new(),
                inserted: index.to_string(),
                caret_before: 0,
                caret_after: 0,
            });
        }
        // Undo once, then push a new edit: the redo stack is cleared, so the
        // eviction that follows cannot leave a redo without its undo.
        assert!(history.take_undo().is_some());
        assert!(history.can_redo());
        history.push(Edit {
            start: 0,
            removed: String::new(),
            inserted: "z".to_string(),
            caret_before: 0,
            caret_after: 0,
        });
        assert!(!history.can_redo(), "a new edit must clear the redo stack");
        assert_eq!(history.depth(), 2);
    }

    #[test]
    fn a_selection_deletion_is_undoable() {
        let mut buffer = typed("hello world");
        buffer.set_caret(0);
        // Arm a character selection the way the shared machine does, then
        // move: in character mode the head follows the cursor.
        buffer.state.select_char();
        buffer.move_caret(5);
        assert!(buffer.delete_selection());
        // A character selection is inclusive of the head character, so
        // offsets 0..=5 — "hello " — are removed.
        assert_eq!(buffer.text(), "world");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "hello world");
    }

    #[test]
    fn the_composer_uses_the_shared_word_geometry() {
        let mut buffer = typed("one two three");
        buffer.set_caret(0);
        assert_eq!(buffer.word_forward(), 4);
        assert_eq!(buffer.word_backward(), 0);
        assert_eq!(buffer.word_end(), 2);
    }

    #[test]
    fn the_composer_and_the_editor_agree_on_words() {
        let text = "fn main() { let x = hello_world; }";
        let mut buffer = typed(text);
        for offset in 0..=text.len() {
            buffer.set_caret(offset);
            assert_eq!(
                buffer.word_forward(),
                text_geometry::word_forward(text, offset),
                "the composer and the editor disagreed at offset {offset}"
            );
            assert_eq!(
                buffer.word_backward(),
                text_geometry::word_backward(text, offset),
                "the composer and the editor disagreed at offset {offset}"
            );
        }
    }
}
