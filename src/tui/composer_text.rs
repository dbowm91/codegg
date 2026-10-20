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
    ///
    /// The **last remaining entry is never dropped**. The byte bound exists to
    /// cap how much history the composer holds, not to decide which edits are
    /// worth keeping: evicting the newest edit because it is large would
    /// remove the very edit the user is most likely to want to undo — a large
    /// paste was unundoable the moment it was made, because the eviction
    /// removed it and left the history empty.
    ///
    /// So the bound is enforced by dropping *older* entries, and an edit that
    /// is on its own over the bound stays. The history is then over its bound
    /// by exactly that one edit, which is the price of "the last thing I did
    /// is always undoable".
    fn evict(&mut self) {
        while self.undo.len() > self.max_depth || self.bytes > self.max_bytes {
            if self.undo.len() <= 1 {
                break;
            }
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

    /// The buffer's line count — an empty buffer is one line.
    ///
    /// The count is what the line motions' condition is written against: a
    /// single-line buffer leaves `j` and `k` to the transcript.
    pub fn line_count(&self) -> usize {
        self.text.split('\n').count()
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

    /// Move the caret to the start of the next word.
    ///
    /// The word boundaries come from the shared geometry, so the composer and
    /// the editor answer "where does the next word start" identically — the
    /// composer does not get a second opinion on word boundaries.
    ///
    /// The caret moves through [`Self::move_caret`], not [`Self::set_caret`],
    /// so a held character selection grows with the caret. That is the
    /// difference between a selection that extends and one that is re-anchored
    /// on every motion, and it is why the motion verbs and the selection verbs
    /// are one mechanism rather than two.
    pub fn move_word_forward(&mut self) {
        let target = self.word_forward();
        self.move_caret(target);
    }

    /// Move the caret to the start of the previous word.
    ///
    /// The mirror of [`Self::move_word_forward`], and the same shared
    /// geometry.
    pub fn move_word_backward(&mut self) {
        let target = self.word_backward();
        self.move_caret(target);
    }

    /// Move the caret to the last character of the word at the caret.
    ///
    /// The shared geometry answers with a byte offset, which for a multi-byte
    /// character can be the middle of a code point; the caret is snapped onto
    /// a character boundary, so the caret lands *on* the word's last character
    /// rather than inside it.
    pub fn move_word_end(&mut self) {
        let target = self.word_end();
        self.move_caret(target);
    }

    /// Move the caret one line down, keeping the column where the target line
    /// allows.
    ///
    /// The vertical target comes from the shared geometry, so the composer
    /// and the editor move between lines the same way. The caret is clamped to
    /// the target line, so `j` on the last line is a no-op rather than an
    /// error.
    pub fn move_line_down(&mut self) {
        let target = text_geometry::move_vertical(self.text.as_str(), self.caret(), 1);
        self.move_caret(target);
    }

    /// Move the caret one line up, keeping the column where the target line
    /// allows.
    pub fn move_line_up(&mut self) {
        let target = text_geometry::move_vertical(self.text.as_str(), self.caret(), -1);
        self.move_caret(target);
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

    /// Move the caret **after** the character it sits on.
    ///
    /// This is the composer's half of Helix `a`: "insert after the caret". The
    /// caret moves one character forward and the next keystroke is typed
    /// there, so the model only has to place the caret — the app switches to
    /// Insert mode afterwards.
    ///
    /// The move is a **deliberate movement**, so it goes through
    /// [`Self::move_caret`] rather than [`Self::set_caret`]: the deliberate
    /// path is what extends a held character selection, and a re-anchored
    /// caret would silently throw away the selection the user had built. This
    /// is the same distinction the motion verbs make.
    ///
    /// The caret is clamped to the end of its **own line**: `a` at the end of
    /// a line stays on that line. Opening a line below is `o`'s job, and two
    /// verbs that did the same thing would make one of them wrong.
    ///
    /// Nothing is recorded in the history: the buffer's text does not change,
    /// so there is nothing to undo.
    pub fn insert_after(&mut self) {
        let caret = self.caret();
        let line = text_geometry::cursor_line(self.text.as_str(), caret);
        let line_end =
            text_geometry::line_content_end(self.text.as_str(), line).unwrap_or(self.text.len());
        // `next_char_boundary` returns `None` at the **end of the buffer** —
        // the editor's rule, because the position after the last character is
        // not a character a selection can cover. For an insertion point it is
        // exactly the position we want: `a` on the last character moves the
        // caret to the end of the buffer, so the next keystroke is typed after
        // that character.
        let next = text_geometry::next_char_boundary(self.text.as_str(), caret)
            .unwrap_or(self.text.len())
            .min(line_end);
        self.move_caret(next);
    }

    /// Open an empty line **below** the caret's line.
    ///
    /// This is the composer's half of Helix `o`: the line below the caret's
    /// line becomes a new empty line and the caret moves into it.
    ///
    /// Unlike [`Self::insert_after`], this is an **edit**: the buffer gains a
    /// line, so it is recorded in the history and `u` takes it back. An
    /// ordinary undoable edit is the whole of the difference between the two
    /// verbs.
    ///
    /// The newline goes at the end of the caret's line's *content*, so the
    /// line's own terminator stays where it was and the new empty line is
    /// the one below it — the caret's line keeps its text.
    pub fn open_line_below(&mut self) {
        let caret = self.caret();
        let line = text_geometry::cursor_line(self.text.as_str(), caret);
        let line_end =
            text_geometry::line_content_end(self.text.as_str(), line).unwrap_or(self.text.len());
        // The caret lands on the first byte of the new line, which is the
        // byte after the newline that was just inserted.
        self.replace_range(line_end, line_end, "\n", line_end + 1);
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

    /// The byte range the held selection covers, if there is one.
    ///
    /// **One range, three consumers**: the delete verb, the copy verb and the
    /// renderer all read this. A selection that each of them computed for
    /// itself would be three answers to one question, and "copy" and
    /// "delete" would stop meaning the same thing.
    ///
    /// The ranges follow the editor's convention, so the two surfaces treat
    /// the same selection the same way: a character selection is **inclusive
    /// of the head character** (`v` then `l` selects two characters, not
    /// one-and-a-bit), and a line selection spans whole lines **including
    /// their terminators**, so deleting it removes the lines rather than
    /// leaving blank ones behind.
    ///
    /// The endpoints are clamped into the buffer and onto character
    /// boundaries, so a selection recorded against older text can neither
    /// panic nor slice mid-code-point. The selection's points are byte
    /// offsets, and an edit does not move them — the same convention the
    /// editor uses.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let selection = self.state.selection()?;
        let (first, last) = selection.bounds();
        let (start, end) = match selection.mode {
            SelectionMode::Char => {
                let start = self.aligned(first.offset);
                let last = last.offset.min(self.text.len());
                let end =
                    text_geometry::next_char_boundary(self.text.as_str(), last).unwrap_or(last);
                (start, end)
            }
            SelectionMode::Line => {
                // The line indices are clamped to the buffer's last line, so
                // a selection recorded against older text degrades to "the
                // last line" rather than to "no line at all". This is the
                // editor's rule, term for term.
                let last_line = self.text.as_str().len_lines().saturating_sub(1);
                let first_line = first.line.min(last_line);
                let final_line = last.line.min(last_line);
                let start = text_geometry::line_start(self.text.as_str(), first_line).unwrap_or(0);
                // `final_line + 1` is the start of the following line, which
                // for the last line is the end of the buffer. That is what
                // makes a whole-line delete leave no trailing blank line.
                let end = text_geometry::line_start(self.text.as_str(), final_line + 1)
                    .unwrap_or(self.text.len())
                    .min(self.text.len());
                (start, end)
            }
        };
        (start < end).then_some((start, end))
    }

    /// The text the held selection covers.
    ///
    /// Reads the same range as [`Self::delete_selection`] and the renderer,
    /// so what is copied is what is deleted and what is painted.
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        self.text.get(start..end).map(|slice| slice.to_string())
    }

    /// Arm a character selection at the caret, or discard the held one.
    ///
    /// `v` is a **mode**, not an action: pressing it again throws the
    /// selection away rather than re-anchoring it on the caret. The rule is
    /// the shared machine's, so the composer and the editor agree about what
    /// a second `v` means.
    pub fn toggle_select_chars(&mut self) {
        self.state.toggle_char();
    }

    /// Select the caret's line, or extend the held line selection by one line.
    ///
    /// `x` is an **action**, not a mode: the second press grows the
    /// selection by exactly one line, and only while nothing has moved since.
    /// The "has something moved" condition belongs to the shared machine, and
    /// the next line's offset is geometry this surface owns — so the two
    /// halves come from where they belong.
    pub fn select_line(&mut self) {
        if self.state.line_select_extends() {
            let Some(selection) = self.state.selection() else {
                return;
            };
            let next = self.next_line_content_start(selection.head.line);
            self.state.grow_to(self.point_at(next));
            return;
        }
        self.state.select_line();
    }

    /// Delete the held selection, if there is one.
    ///
    /// Returns whether anything was deleted. The deletion is an ordinary
    /// edit, so it is undoable like any other.
    ///
    /// The range is [`Self::selection_range`]'s — the editor's convention, so
    /// the two surfaces delete the same text for the same selection: a
    /// character selection is **inclusive of the head character** (`v` then
    /// `l` selects two characters, not one-and-a-bit), and a line selection
    /// spans whole lines including their terminators.
    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        // The held selection is dropped by the edit itself — `replace_range`
        // invalidates it, because the edit changes the buffer's length. The
        // range was read *before* the edit, which is the whole point: the
        // deletion acts on what was selected, not on what is selected after.
        self.replace_range(start, end, "", start);
        true
    }

    /// Byte offset of the first content byte on the line after `line`.
    ///
    /// Clamped to the end of the buffer, so `x` on the last line grows into
    /// the terminator rather than past the buffer. The same rule the editor
    /// uses, because "the next line" has to mean the same thing on both
    /// surfaces.
    fn next_line_content_start(&self, line: usize) -> usize {
        let last = self.text.as_str().len_lines().saturating_sub(1);
        text_geometry::line_start(self.text.as_str(), (line + 1).min(last)).unwrap_or_else(|| {
            text_geometry::line_content_end(self.text.as_str(), last).unwrap_or(self.text.len())
        })
    }

    /// Undo the newest edit.
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.take_undo() else {
            return false;
        };
        let end = edit.start + edit.inserted.len();
        self.text.replace_range(edit.start..end, &edit.removed);
        // The undo changes the buffer's length, so the held selection — whose
        // byte offsets name positions in the text that was just replaced — is
        // dropped. Carrying it across would point at different characters,
        // or past the end of the buffer.
        self.state.deactivate();
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
        // The same rule as [`Self::undo`]: the buffer's length changes, so the
        // held selection does not survive it.
        self.state.deactivate();
        self.set_caret(edit.caret_after);
        true
    }

    /// Replace `start..end` with `replacement` and record the edit.
    fn replace_range(&mut self, start: usize, end: usize, replacement: &str, caret: usize) {
        let removed = self.text[start..end].to_string();
        // An edit that changes the buffer's length invalidates the held
        // selection. The selection's byte offsets name positions in the text
        // *before* the edit; keeping them across an edit that moved the text
        // would make the selection cover different characters — or reach past
        // the end of the buffer. The rule belongs here, in the one place every
        // edit passes through, rather than in each caller's judgement.
        if removed.len() != replacement.len() {
            self.state.deactivate();
        }
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
        // All three word motions, over every offset. The composer and the
        // editor share `text_geometry`, so this is a fact about the tree
        // rather than an inference from it: a composer with its own word rule
        // would be two answers to one question.
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
            assert_eq!(
                buffer.word_end(),
                text_geometry::word_end(text, offset),
                "the composer and the editor disagreed at offset {offset}"
            );
        }
    }

    #[test]
    fn a_word_motion_moves_the_caret_to_the_geometrys_answer() {
        let text = "one two three";
        let mut buffer = typed(text);
        for offset in 0..=text.len() {
            buffer.set_caret(offset);
            buffer.move_word_forward();
            assert_eq!(
                buffer.caret(),
                text_geometry::word_forward(text, offset),
                "`w` disagreed with the shared geometry at offset {offset}"
            );
        }
        for offset in 0..=text.len() {
            buffer.set_caret(offset);
            buffer.move_word_backward();
            assert_eq!(
                buffer.caret(),
                text_geometry::word_backward(text, offset),
                "`b` disagreed with the shared geometry at offset {offset}"
            );
        }
    }

    #[test]
    fn a_word_end_motion_moves_the_caret_to_the_geometrys_answer() {
        let text = "one two three";
        let mut buffer = typed(text);
        for offset in 0..=text.len() {
            buffer.set_caret(offset);
            buffer.move_word_end();
            assert_eq!(
                buffer.caret(),
                text_geometry::word_end(text, offset),
                "`e` disagreed with the shared geometry at offset {offset}"
            );
        }
    }

    #[test]
    fn a_word_motion_lands_on_a_character_boundary() {
        // The shared geometry answers with a byte offset, and a multi-byte
        // character's last byte is not a boundary. The caret has to be on one.
        let mut buffer = typed("café au lait");
        buffer.set_caret(0);
        buffer.move_word_end();
        assert!(
            buffer.text().is_char_boundary(buffer.caret()),
            "the caret landed inside a code point at offset {}",
            buffer.caret()
        );
    }

    #[test]
    fn a_vertical_motion_moves_the_caret_to_the_geometrys_answer() {
        let text = "one\ntwo\nthree";
        let mut buffer = typed(text);
        for offset in 0..=text.len() {
            buffer.set_caret(offset);
            buffer.move_line_down();
            assert_eq!(
                buffer.caret(),
                text_geometry::move_vertical(text, offset, 1),
                "`j` disagreed with the shared geometry at offset {offset}"
            );
            buffer.set_caret(offset);
            buffer.move_line_up();
            assert_eq!(
                buffer.caret(),
                text_geometry::move_vertical(text, offset, -1),
                "`k` disagreed with the shared geometry at offset {offset}"
            );
        }
    }

    #[test]
    fn a_vertical_motion_keeps_the_column_where_the_target_line_allows() {
        let mut buffer = typed("one\ntwo");
        buffer.set_caret(2);
        buffer.move_line_down();
        assert_eq!(buffer.caret(), 6, "the column should be kept");
    }

    #[test]
    fn a_vertical_motion_clamps_to_a_shorter_target_line() {
        let mut buffer = typed("one\nt");
        buffer.set_caret(2);
        buffer.move_line_down();
        assert_eq!(buffer.caret(), 5, "the caret clamps to the line's end");
    }

    #[test]
    fn a_motion_extends_a_held_character_selection() {
        // The point of `move_caret` rather than `set_caret`: in character mode
        // the head follows the cursor, so a motion grows the selection. A
        // selection that re-anchored on every motion would make the selection
        // verbs impossible to combine with the motion verbs.
        let mut buffer = typed("one two three");
        buffer.set_caret(0);
        buffer.state.select_char();
        buffer.move_word_forward();
        let selection = buffer.selection().expect("the selection is held");
        assert_eq!(selection.anchor.offset, 0);
        assert_eq!(
            selection.head.offset, 4,
            "the head should have followed the caret"
        );
    }

    #[test]
    fn a_motion_does_not_extend_a_held_line_selection() {
        // Line selection is excluded from the extension rule on purpose: a
        // motion must not turn "this line" into "every line I walked over".
        let mut buffer = typed("one\ntwo\nthree");
        buffer.set_caret(0);
        buffer.state.select_line();
        buffer.move_line_down();
        let selection = buffer.selection().expect("the selection is held");
        assert_eq!(selection.anchor.line, 0);
        assert_eq!(selection.head.line, 0, "the head should not have moved");
    }

    // ── The selection verbs ──────────────────────────────────────────────

    #[test]
    fn a_character_selection_is_armed_at_the_caret_and_toggled_off() {
        let mut buffer = typed("one two");
        buffer.set_caret(1);
        buffer.toggle_select_chars();

        let selection = buffer.selection().expect("the selection is held");
        assert_eq!(selection.mode, SelectionMode::Char);
        assert_eq!(selection.anchor.offset, 1);
        assert_eq!(selection.head.offset, 1);

        // `v` is a mode: a second press discards the selection rather than
        // re-anchoring it on the caret.
        buffer.toggle_select_chars();
        assert!(!buffer.is_selected(), "a second `v` discards the selection");
    }

    #[test]
    fn a_character_selection_is_inclusive_of_the_head_character() {
        let mut buffer = typed("hello");
        buffer.set_caret(0);
        buffer.toggle_select_chars();
        buffer.move_caret(1);

        assert_eq!(
            buffer.selection_range(),
            Some((0, 2)),
            "`v` then one character is two characters"
        );
        assert_eq!(buffer.selected_text().as_deref(), Some("he"));
    }

    #[test]
    fn a_line_selection_grows_a_line_at_a_time_and_stops_at_the_last_line() {
        let mut buffer = typed("one\ntwo\nthree");
        buffer.set_caret(0);

        buffer.select_line();
        assert_eq!(buffer.selection_range(), Some((0, 4)), "one line");

        buffer.select_line();
        assert_eq!(buffer.selection_range(), Some((0, 8)), "two lines");

        buffer.select_line();
        assert_eq!(buffer.selection_range(), Some((0, 13)), "three lines");

        // The last line is the end of the growth.
        buffer.select_line();
        assert_eq!(
            buffer.selection_range(),
            Some((0, 13)),
            "the growth stops at the last line"
        );
    }

    #[test]
    fn a_line_selection_covers_whole_lines_including_their_terminators() {
        let mut buffer = typed("one\ntwo\nthree");
        buffer.set_caret(5);
        buffer.select_line();

        assert_eq!(
            buffer.selected_text().as_deref(),
            Some("two\n"),
            "a line selection includes the terminator, so deleting it leaves no blank line"
        );
    }

    #[test]
    fn a_line_selection_re_anchors_after_a_movement() {
        // The growth is disarmed by any deliberate movement, so the next `x`
        // selects from where the caret now is rather than growing the old
        // selection over everything in between.
        let mut buffer = typed("one\ntwo\nthree");
        buffer.set_caret(0);
        buffer.select_line();
        buffer.move_line_down();
        buffer.select_line();

        assert_eq!(
            buffer.selection_range(),
            Some((4, 8)),
            "the selection re-anchored on the caret's line"
        );
    }

    #[test]
    fn the_delete_and_the_copy_read_the_same_range() {
        let mut buffer = typed("hello world");
        buffer.set_caret(0);
        buffer.toggle_select_chars();
        buffer.move_caret(4);

        assert_eq!(buffer.selected_text().as_deref(), Some("hello"));
        assert!(buffer.delete_selection());
        assert_eq!(
            buffer.text(),
            " world",
            "the copy's range is the delete's range"
        );
    }

    #[test]
    fn deleting_with_nothing_selected_changes_nothing() {
        let mut buffer = typed("hello");
        assert!(!buffer.delete_selection());
        assert_eq!(buffer.text(), "hello");
    }

    #[test]
    fn an_edit_drops_the_held_selection() {
        // The selection's points are byte offsets into the text that the edit
        // replaces. The rule is "an edit that changes the buffer's length
        // drops the held selection", so a stale selection cannot arise at
        // all — there is no edit left to leave one behind.
        let mut buffer = typed("hello world");
        buffer.set_caret(6);
        buffer.toggle_select_chars();
        buffer.move_caret(10);
        assert_eq!(buffer.selection_range(), Some((6, 10)));

        // Delete the text in front of the selection.
        buffer.set_caret(6);
        for _ in 0..6 {
            buffer.backspace();
        }
        assert_eq!(buffer.text(), "world");
        assert!(
            !buffer.is_selected(),
            "the edit kept a selection recorded against the old text"
        );
    }

    // ── The undo verbs ───────────────────────────────────────────────────

    #[test]
    fn an_oversized_edit_stays_undoable() {
        // The byte bound used to evict the **oldest** entry, and on an
        // oversized paste the oldest entry *was* the paste — it was evicted
        // the moment it was made, so the history was empty and the user's
        // last action could not be undone at all.
        let mut history = UndoHistory::new(usize::MAX, 16);
        history.push(Edit {
            start: 0,
            removed: String::new(),
            inserted: "y".repeat(64),
            caret_before: 0,
            caret_after: 0,
        });
        assert_eq!(
            history.depth(),
            1,
            "the only edit was evicted; the bound must never drop the last one"
        );
    }

    #[test]
    fn an_oversized_paste_is_still_undoable() {
        let mut buffer = ComposerText::new();
        buffer.insert(&"x".repeat(MAX_UNDO_BYTES * 2));
        assert!(buffer.undo(), "an oversized paste was not undoable");
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn the_depth_bound_still_evicts_the_oldest_first() {
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
        let newest = history.take_undo().expect("an edit to undo");
        assert_eq!(newest.inserted, "4");
        let next = history.take_undo().expect("an edit to undo");
        assert_eq!(next.inserted, "3");
        assert!(
            history.take_undo().is_none(),
            "the oldest edits should have been evicted"
        );
    }

    #[test]
    fn an_undo_drops_the_held_selection() {
        let mut buffer = typed("hello world");
        buffer.set_caret(0);
        buffer.toggle_select_chars();
        buffer.move_caret(4);
        buffer.insert("!");
        assert!(!buffer.is_selected(), "the edit kept the selection");

        // Re-arm, undo, and the selection is gone again: the undo changed the
        // buffer's length, so the offsets it named no longer mean anything.
        buffer.toggle_select_chars();
        assert!(buffer.is_selected());
        assert!(buffer.undo());
        assert!(
            !buffer.is_selected(),
            "the undo kept a selection recorded against the old text"
        );
    }

    #[test]
    fn a_redo_drops_the_held_selection() {
        let mut buffer = typed("hello world");
        buffer.set_caret(0);
        buffer.insert("!");
        assert!(buffer.undo());

        buffer.toggle_select_chars();
        assert!(buffer.is_selected());
        assert!(buffer.redo());
        assert!(
            !buffer.is_selected(),
            "the redo kept a selection recorded against the old text"
        );
    }

    // ── The insert transitions ───────────────────────────────────────────

    #[test]
    fn insert_after_moves_the_caret_one_character_forward() {
        let mut buffer = typed("hello");
        buffer.set_caret(0);
        buffer.insert_after();
        assert_eq!(buffer.caret(), 1);
    }

    #[test]
    fn insert_after_stays_on_its_own_line() {
        let mut buffer = typed("one\ntwo");
        buffer.set_caret(2);
        buffer.insert_after();
        assert_eq!(
            buffer.caret(),
            3,
            "`insert_after` ran past the end of its own line"
        );
    }

    #[test]
    fn insert_after_at_the_end_of_the_buffer_leaves_the_caret_where_it_is() {
        let mut buffer = typed("hello");
        buffer.set_caret(5);
        buffer.insert_after();
        assert_eq!(buffer.caret(), 5);
    }

    #[test]
    fn insert_after_on_the_last_character_moves_to_the_end_of_the_buffer() {
        // The position after the last character is not a character a
        // selection can cover, so `next_char_boundary` reports `None` there.
        // For an insertion point it is exactly the position wanted: the next
        // keystroke is typed after the character under the caret.
        let mut buffer = typed("hello");
        buffer.set_caret(4);
        buffer.insert_after();
        assert_eq!(buffer.caret(), 5);
    }

    #[test]
    fn insert_after_extends_a_held_character_selection() {
        // The move is a *deliberate* movement, so it goes through the shared
        // machine's movement path — the one that grows a selection. A
        // re-anchored caret would throw the selection away.
        let mut buffer = typed("hello");
        buffer.set_caret(0);
        buffer.toggle_select_chars();
        buffer.insert_after();
        assert_eq!(buffer.selection_range(), Some((0, 2)));
    }

    #[test]
    fn insert_after_records_nothing() {
        // `a` is a movement: the buffer's text does not change, so there is
        // nothing to undo. The baseline is the depth *before* the move,
        // because typing the text in the first place was an edit.
        let mut buffer = typed("hello");
        buffer.set_caret(0);
        let depth = buffer.history().depth();
        buffer.insert_after();
        assert_eq!(buffer.text(), "hello");
        assert_eq!(
            buffer.history().depth(),
            depth,
            "`insert_after` recorded an edit"
        );
    }

    #[test]
    fn open_line_below_opens_a_line_below_the_carets_line() {
        let mut buffer = typed("one\ntwo");
        buffer.set_caret(0);
        buffer.open_line_below();
        assert_eq!(buffer.text(), "one\n\ntwo");
        assert_eq!(buffer.caret(), 4, "the caret did not enter the new line");
    }

    #[test]
    fn open_line_below_keeps_the_carets_line_text() {
        let mut buffer = typed("one\ntwo");
        buffer.set_caret(0);
        buffer.open_line_below();
        let lines: Vec<&str> = buffer.text().split('\n').collect();
        assert_eq!(lines[0], "one");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "two");
    }

    #[test]
    fn open_line_below_on_the_last_line_opens_a_line_below_it() {
        let mut buffer = typed("one\ntwo");
        buffer.set_caret(4);
        buffer.open_line_below();
        assert_eq!(buffer.text(), "one\ntwo\n");
        assert_eq!(buffer.caret(), 8);
    }

    #[test]
    fn open_line_below_is_undoable() {
        // The whole difference between `o` and `a`: `o` changes the buffer, so
        // it is an edit, so `u` takes it back.
        let mut buffer = typed("one\ntwo");
        buffer.set_caret(0);
        buffer.open_line_below();
        assert_eq!(buffer.text(), "one\n\ntwo");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "one\ntwo");
    }
}
