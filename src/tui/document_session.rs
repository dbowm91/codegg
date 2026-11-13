//! Minimal TUI ownership adapter for the shared document controller.
//!
//! This is an M005 seam only: presentation position is separate from text,
//! and no widget or second mutable text buffer lives here.
//!
//! M006-A makes this the real ownership seam for the TUI editor. The
//! controller remains the sole owner of canonical text; the presentation
//! record below holds cursor, selection, viewport, buffer mode, and a
//! bounded frontend undo history — and **no text**. See
//! `crate::tui::editor` for the model and the static guard in
//! `scripts/check_tui_editor_text_authority.py`.
use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use codegg_client::{
    DocumentAttachmentInfo, DocumentController, DocumentControllerError, DocumentState,
    DocumentTransport,
};
use codegg_document::DocumentSnapshot;
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::document::DocumentScope;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::CoreClient;

use super::editor::{
    ClampCache, EditorCommand, EditorMode, EditorPoint, EditorPrefix, EditorUndoEntry, FindMotion,
    FindNote, HistoryId, MatchNote, ParagraphNote, SelectionMotion,
};
use super::helix_selection::Selection;

struct CoreClientTransport(Arc<dyn CoreClient>);

#[async_trait]
impl DocumentTransport for CoreClientTransport {
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
        self.0
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: request,
            })
            .await
            .map_err(|error| error.to_string())
    }
}

/// Presentation-neutral document handle used by TUI editor flows.
pub struct TuiDocumentSession {
    controller: Arc<DocumentController>,
    presentation: TuiDocumentPresentation,
}

/// Editor presentation state owned by the frontend.
///
/// Holds no document text. Every offset is a position into the replica
/// owned by [`DocumentController`]; text is read on demand through
/// [`DocumentController::try_snapshot`] and dropped at the end of the call
/// that needed it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TuiDocumentPresentation {
    /// Cursor position as a UTF-8 byte offset into the local replica.
    pub cursor_byte: usize,
    /// The last repeatable **selection** motion, for `Alt-.`.
    ///
    /// A second slot beside `last_find`, not a widening of it: `f x` records a
    /// find with a `char` to scan for, and `C` records "put a cursor on the
    /// next line". Widening `last_find` would mean a motion that is not a
    /// find wearing a find's type, and `find_note` — which reports on the
    /// *attempt* — would have to learn about motions it cannot describe.
    ///
    /// One slot, every motion overwrites it, exactly as Helix's single repeat
    /// slot does. `note_motion` clears it, so `j` between two `C` presses
    /// disarms the repeat and the gap is predictable rather than surprising.
    pub last_selection_motion: Option<SelectionMotion>,
    /// The last repeatable **unimpaired** motion, for `Alt-.`.
    ///
    /// Helix's keymap calls `Alt-.` "repeat last motion (`f`, `t`, `m`, `[` or
    /// `]`)", so the unimpaired paragraph motions belong in the repeat path and
    /// not only on their own keys.
    ///
    /// A third slot for the same reason [`last_selection_motion`](Self) is a
    /// second one: `FindMotion` carries a `char` to scan for and
    /// [`SelectionMotion`](Self) names one of M008's verbs, and neither can
    /// describe "move to the next paragraph" without becoming a union of three
    /// unrelated things. One slot, every motion overwrites it, exactly as
    /// Helix's does — `note_motion` clears it, so `j` between two `]p` presses
    /// disarms the repeat.
    ///
    /// The three slots are **mutually exclusive by construction**: whichever
    /// recorder runs last clears the other two. `Alt-.` may therefore consult
    /// them in any order without inventing a precedence between two motions
    /// that were never both the last one.
    pub last_unimpaired: Option<EditorCommand>,
    /// Why the last unimpaired paragraph motion did not land, if it did not.
    ///
    /// The paragraph analogue of [`find_note`](Self::find_note), and it exists
    /// for the same reason: `]p` at the last paragraph and `]p` on a buffer
    /// whose scan hit its byte budget both leave the cursor exactly where it
    /// was, and only one of them is a true "there is none".
    ///
    /// Written by [`editor::apply_motion`](super::editor::apply_motion) next to
    /// the resolution that produced it, and lifted into the notice by the
    /// command layer immediately after — so it describes exactly one keystroke
    /// and cannot survive to be re-read as the answer to the next one.
    pub paragraph_note: ParagraphNote,
    /// Why the last bracket-match motion did not land, if it did not.
    ///
    /// The third member of the same family as [`find_note`](Self::find_note)
    /// and [`paragraph_note`](Self::paragraph_note): a motion that scans can
    /// fail, and a failure the status row cannot see is a keypress that looks
    /// like a dropped one.
    ///
    /// Written by [`editor::apply_motion`](super::editor::apply_motion) next to
    /// the resolution that produced it, and lifted into the notice by the
    /// command layer immediately after — so it describes exactly one keystroke
    /// and cannot survive to be re-read as the answer to the next one.
    pub match_note: MatchNote,
    /// The held Helix selections, in document order, or empty when nothing is
    /// selected.
    ///
    /// Points are byte offsets paired with their line — see
    /// [`EditorPoint`](super::editor::EditorPoint). Stored as the shared
    /// [`Selection`] rather than a bare range so `v` stays a mode and `x` an
    /// action here exactly as they are in `/terminal`; resolving them to
    /// ranges is [`editor::selection_ranges`](super::editor::selection_ranges)'s
    /// job, and only the effects consume a range.
    ///
    /// This is a *set*, not an optional one, because Helix selection is:
    /// `d` over several selections must delete all of them in one gesture, and
    /// that is undefined rather than merely unbound while there is room for
    /// exactly one. [`primary_index`](Self::primary_index) names the member
    /// every single-selection accessor resolves to.
    pub selections: Vec<Selection<EditorPoint>>,
    /// Index into [`selections`](Self::selections) of the distinguished member.
    ///
    /// Meaningless while the set is empty; the single-selection accessors on
    /// the editor resolve the primary, which is why they keep answering for
    /// "the" selection after this milestone replaced the optional one.
    pub primary_index: usize,
    /// First visible line, zero-based. The hard-wrap viewport is one
    /// screen row per logical line.
    pub viewport_line: usize,
    /// The text area's height in rows, as last measured by the renderer.
    ///
    /// A page motion needs the **real** height: a half page in a twelve-row
    /// pane is six rows, and the ten a fixed guess would give is more than
    /// half of what the reader can see. Zero means "not measured yet" — the
    /// first frame has not been drawn — and the nominal height stands in
    /// until it is.
    pub visible_lines: usize,
    /// The first visible line when the view has been scrolled **away from the
    /// cursor**.
    ///
    /// `None` is the ordinary case: the view follows the cursor. `Some` is
    /// Helix's view mode — the page moves and the cursor stays where it is.
    /// A cursor motion clears it, because a view that has been scrolled away
    /// is only meaningful while the reader is reading it.
    pub view_scroll: Option<usize>,
    /// Whether the editor's **keymap box** is open in its bottom-right
    /// corner.
    ///
    /// The editor's own catalogue of its keys, opened with `space ?`. It is a
    /// **box** rather than a row because the keys are a list, and a list on a
    /// row is a list that has to be truncated — and a truncated keymap names
    /// only some of the keys, which is worse than no keymap.
    ///
    /// The box is a floating overlay: it covers the corner of the text area
    /// rather than taking rows away from the document, so opening it does not
    /// reflow the text under the reader.
    pub keymap_overlay: bool,
    /// The visited-position history — Helix's **jumplist** — and its picker.
    ///
    /// Chronological, oldest first, and deliberately **not** normalized the
    /// way [`selections`](Self::selections) is: a jumplist is a history, so its
    /// order is the order things happened. Sorting it into document order
    /// would answer "where have I been" with "what is in the file".
    ///
    /// Positions only — a daemon document identity plus a byte offset, never
    /// text — bounded by [`MAX_JUMPLIST_ENTRIES`], and cleared by the same
    /// transitions that clear the undo history: `clear_history` is the single
    /// clearing point, and `reset_for_open` calls it.
    pub jumplist: Jumplist,
    /// The document's **registers** — Helix's `"` targets, and the unnamed
    /// register bare `p` reads.
    ///
    /// Frontend-local presentation state: nothing here is persisted, nothing
    /// is shared with another document, and the whole store is cleared by the
    /// same transitions that clear the undo history
    /// ([`clear_history`](super::editor::clear_history), which
    /// [`reset_for_open`](Self::reset_for_open) calls).
    ///
    /// **Not a second text buffer.** An entry is a bounded fragment the reader
    /// deliberately yanked, capped per entry by [`MAX_REGISTER_BYTES`], and
    /// the store holds at most twenty-seven of them. The shape is
    /// `BTreeMap<char, String>` rather than a `Vec<String>` because the
    /// text-authority guard reads a materialized collection as a document
    /// copy — and it is right to: the map is a sparse set of named slots, not
    /// an ordered document.
    ///
    /// The **selected** register is not a field here. It is
    /// [`EditorPrefix::RegisterNamed`](super::editor::EditorPrefix), the
    /// prefix machine's second level, so a selection cannot outlive the
    /// keystroke that follows it and there is no second field to clear.
    pub registers: Registers,
    /// The **keystroke recorder** — the bounded key log `.` repeats.
    ///
    /// Frontend-local, like the registers, and cleared by the same transitions:
    /// [`clear_history`](super::editor::clear_history) forgets both, so a
    /// recording cannot outlive the text it was made against. Keys, never
    /// document text.
    pub recorder: KeyRecorder,
    /// Horizontal scroll in display columns.
    pub viewport_column: usize,
    /// The horizontal clamp's memo (M005).
    ///
    /// An **integer** — the widest visible line, in columns — and nothing
    /// else: no text, no cached document, no second buffer. See
    /// [`ClampCache`](super::editor::ClampCache) for its key and why each part
    /// of it is there. It is dropped with this record, which is what makes
    /// closing a document drop it; nothing outside the viewport resolver
    /// writes it.
    pub clamp_cache: Option<ClampCache>,
    /// Buffer edit mode.
    pub mode: EditorMode,
    /// The multi-key grammar the editor is waiting on.
    ///
    /// Idle, or one armed [`EditorPrefix`] consumed by the next key. See that
    /// type for the grammar rule — one pending prefix, one terminal key, and
    /// a disarm whenever the thing it was aimed at changes.
    pub pending_prefix: EditorPrefix,
    /// The count being typed, or `None` when no count is pending.
    ///
    /// A count begins with `1`–`9`, and `0` only *extends* one: `0` with
    /// nothing pending is `LineStart`, and `10` is a count of ten. It saturates
    /// at [`MAX_COUNT`](super::editor::MAX_COUNT) and the motion it applies to
    /// clamps at the document.
    ///
    /// It lives **beside** the prefix machine rather than inside it: a count is
    /// a modifier that survives arming a prefix (`3gg`, `3ge`) and is consumed
    /// by the leaf that completes the gesture, so it would widen
    /// [`pending_prefix`](Self::pending_prefix)'s one-slot grammar.
    ///
    /// **Clearing contract.** [`clear_count`](Self::clear_count) is the one
    /// clearing write, and it runs at every transition that replaces the buffer
    /// or the keyboard: `reset_for_open` (open and document switch), the
    /// reconciliation path (`clear_history`: reload, resync, reconnect, close),
    /// leaving the editor route, moving focus to the tree, and `Esc`. The
    /// Normal-mode dispatch tail calls it for every key that is neither a count
    /// digit nor a prefix that will carry the count; the motion path consumes
    /// it. So a count cannot survive a mode change, a document switch, or a
    /// gesture that did not consume it.
    pub pending_count: Option<usize>,
    /// Byte column a vertical run is aiming for, or `None` when there is no
    /// run in progress.
    ///
    /// Without it, `j` onto a short line and back up returns to the short
    /// line's end column instead of where the run started — which is how a
    /// reader scrolling a ragged file loses their place every time the file
    /// narrows. It is a **byte** column, matching the editor's byte offsets,
    /// and is clamped into the target line at use time so a column recorded
    /// against older text can never place the cursor outside a line or inside
    /// a multi-byte character.
    ///
    /// Not a flag on a grammar, and not a second source of truth: it is
    /// rewritten by every motion through [`apply_motion`](super::editor::apply_motion)
    /// in one step, so it cannot disagree with the cursor it describes.
    pub desired_column: Option<usize>,
    /// The find-char motion `Alt-.` repeats, or `None` when there is none.
    ///
    /// Helix keeps one *motion* slot that every motion overwrites, so this
    /// is cleared by any other motion rather than only by another find — the
    /// gap between `f x`, `j`, and `Alt-.` is exactly what makes `Alt-.`
    /// predictable. A reconciliation clears it for the same reason
    /// `disarm_prefix` exists: the character it was aimed at may not be in
    /// the text any more.
    pub last_find: Option<FindMotion>,
    /// Why the last find motion did not land, if it did not.
    ///
    /// Disclosed in the status row. A find that stops because the character
    /// is absent and one that stops at [`MAX_EDITOR_FIND_BYTES`] otherwise
    /// look identical — like a dropped keypress — and they call for
    /// different answers, so the bound is stated rather than hidden.
    pub find_note: FindNote,
    /// The `:` command line while command mode is open, without the leading
    /// `:`. Empty when command mode is closed.
    ///
    /// Bounded to [`MAX_EDITOR_COMMAND_LINE`](super::editor::MAX_EDITOR_COMMAND_LINE).
    /// This is a command *name*, never document text, so it is exempt from
    /// the text-authority guard for the same reason
    /// [`pending_prefix`](Self::pending_prefix) is.
    pub command_line: String,
    /// Whether the `:` command line is open.
    pub command_active: bool,
    /// Index into [`complete_editor_command`](super::editor::complete_editor_command)
    /// for the highlighted palette row.
    pub command_selection: usize,
    /// The `/` search line while search mode is open, without the leading
    /// `/`. Empty when search mode is closed.
    ///
    /// A *needle*, never document text: it is bounded by
    /// [`MAX_EDITOR_SEARCH_LINE`](super::editor::MAX_EDITOR_SEARCH_LINE) and
    /// capped at the keystroke, so it cannot grow into a second buffer.
    pub search_line: String,
    /// Whether the `/` search line is open and owns the keyboard.
    pub search_active: bool,
    /// Absolute byte **spans** of every retained match for
    /// [`search_line`](Self::search_line), ascending and non-overlapping.
    ///
    /// Positions, not text — the renderer resolves each against the row it is
    /// painting. Bounded by
    /// [`MAX_EDITOR_SEARCH_MATCHES`](super::editor::MAX_EDITOR_SEARCH_MATCHES).
    pub search_matches: Vec<std::ops::Range<usize>>,
    /// Index into [`search_matches`](Self::search_matches) the cursor is
    /// parked on. Only meaningful when there is at least one match.
    pub search_index: usize,
    /// Which way the open search adopts a match: `true` for `/`, `false` for
    /// `?`.
    ///
    /// State rather than an argument because it has to survive between
    /// keystrokes of a live search — every refinement re-adopts, and a
    /// reverse search that re-adopted forwards would walk the reader away
    /// from the occurrence they are narrowing in on. `n`/`N` do not read it:
    /// their direction is their own chord's argument.
    pub search_forward: bool,
    /// Whether a search dropped matches past its bound or stopped scanning
    /// before the end of the document. Disclosed in the status row: a
    /// partial match list that reads as complete is the same defect as
    /// unvalidated LSP coverage.
    pub search_partial: bool,
    /// Frontend-local edit history: the nodes of the undo **tree**, in
    /// creation order.
    ///
    /// A node names its parent, so it never moves between containers and
    /// undo/redo are one edge travelled both ways. List order *is* creation
    /// order, which is how `u`/`U` and `Alt-u`/`Alt-U` read one structure.
    pub history: Vec<EditorUndoEntry>,
    /// Retained history payload bytes, for the history memory bound.
    pub history_bytes: usize,
    /// The node whose text is on screen: [`HistoryId`] `0` is the root state,
    /// anything else a node's [`created`](EditorUndoEntry::created) identity.
    pub history_cursor: HistoryId,
    /// The identity [`history`](Self::history)'s next node takes — monotonic,
    /// so a pruned node's identity is never handed to a different node.
    pub history_next: HistoryId,
    /// Whether the last prune could not get the history under its caps and
    /// cleared it: the disclosed fallback.
    pub history_bound_cleared: bool,
    /// The open insert session, if insert mode has typed anything.
    ///
    /// Held beside the history because it *is* history that has not been
    /// written yet: the edits of one typing burst become one undo entry
    /// when the burst ends. The name deliberately does not contain
    /// `prefix` — the prefix-state guard allows exactly one such field on
    /// this struct, and the pending prefix already holds it.
    pub insert_session: Option<crate::tui::editor::InsertSession>,
}

impl TuiDocumentPresentation {
    /// Reset to a post-open presentation: cursor at the start of the
    /// document, no selection, no history. Undo never survives opening a
    /// different document.
    ///
    /// Keyboard ownership is deliberately *not* recorded here. It used to be
    /// (`focus`), and that second axis is exactly why opening a file rendered
    /// a buffer no key could reach: focus defaulted to the composer, so the
    /// editor claimed nothing and the bare-text gate swallowed everything else.
    /// The one axis is `App::ui_state.input_mode` — Insert puts the keyboard
    /// in the document, Normal leaves it with the Helix verbs and the bottom
    /// input box.
    pub fn reset_for_open(&mut self) {
        self.cursor_byte = 0;
        self.clear_selections();
        self.viewport_line = 0;
        self.viewport_column = 0;
        // A memo of the previous document's widest line is the one piece of
        // measured state that could survive an open at the *same* revision —
        // a fresh attachment is revision zero — and clamp this document's
        // scroll to the previous one's width.
        self.clamp_cache = None;
        self.mode = EditorMode::Normal;
        self.desired_column = None;
        self.disarm_prefix();
        self.clear_count();
        super::editor::clear_history(self);
        self.close_command_line();
        self.cancel_search();
    }

    /// Begin or extend the pending count with `digit`.
    ///
    /// Returns `true` when it saturated at
    /// [`MAX_COUNT`](super::editor::MAX_COUNT), so the caller can disclose the
    /// bound rather than letting a truncated count read as the typed one.
    pub fn extend_count(&mut self, digit: char) -> bool {
        let digit = digit.to_digit(10).unwrap_or(0) as usize;
        let combined = self
            .pending_count
            .unwrap_or(0)
            .saturating_mul(10)
            .saturating_add(digit);
        self.pending_count = Some(combined.min(super::editor::MAX_COUNT));
        combined > super::editor::MAX_COUNT
    }

    /// Take the pending count down.
    ///
    /// The single clearing write every path calls — see
    /// [`pending_count`](Self::pending_count) for the contract. One operation
    /// rather than a scatter of assignments, because a count left behind is a
    /// count the *next* gesture silently inherits.
    pub fn clear_count(&mut self) {
        self.pending_count = None;
    }

    /// Arm `prefix`, waiting for its next key.
    ///
    /// Replacing rather than stacking is the grammar: one pending prefix at a
    /// time, so no gesture can ever need a third key.
    pub fn arm_prefix(&mut self, prefix: EditorPrefix) {
        self.pending_prefix = prefix;
    }

    /// Disarm any armed prefix, leaving the key that armed it unaffected.
    ///
    /// Called wherever the pending key's target stops being the thing on
    /// screen: opening a document, leaving the editor route, moving focus to
    /// the tree pane, and every reconciliation that rewrites the text.
    pub fn disarm_prefix(&mut self) {
        self.pending_prefix = EditorPrefix::Idle;
    }

    /// Drop every held selection, leaving the cursor where it is.
    ///
    /// The reset point for every place the set must not survive: opening a
    /// document, leaving Insert, and every accepted edit. Kept as one
    /// operation rather than two field writes because a set that kept its
    /// members while losing `primary_index` would silently resolve every
    /// primary-scoped accessor to a different member than the one it resolved
    /// to a moment earlier.
    pub fn clear_selections(&mut self) {
        self.selections.clear();
        self.primary_index = 0;
    }

    /// Put back a set recorded before an edit — undo and redo only.
    ///
    /// The recorded members address the **pre-edit** bytes, and undo/redo
    /// restore exactly those bytes, so they need no re-clamping: the offsets
    /// are correct again by the time this runs. Ordering and the primary are
    /// re-derived through the shared normalizer so a restored set obeys the
    /// same "document order, primary names a member" rule a live one does.
    pub fn restore_selections(&mut self, selections: &[Selection<EditorPoint>]) {
        let (selections, primary) =
            super::helix_selection::normalize_set(selections.to_vec(), self.primary_index);
        self.selections = selections;
        self.primary_index = primary;
    }

    /// Where the history cursor is standing, when that is not the only tip.
    ///
    /// `(applied, retained)`: the nodes on the path to the text on screen, and
    /// every node the tree still holds. `None` when the two agree and nothing
    /// is redoable from here — the tip of a history with no other branch,
    /// which is where a reader stands almost all the time. A time cursor
    /// nobody can see is a rename, so a branch or a redo is disclosed.
    pub fn history_position(&self) -> Option<(usize, usize)> {
        let applied = super::editor::history_depth(self, self.history_cursor);
        if applied == self.history.len() && super::editor::redo_entry(self).is_none() {
            None
        } else {
            Some((applied, self.history.len()))
        }
    }

    /// Forget the repeatable find and clear its disclosure.
    ///
    /// Called whenever something else becomes the last motion, which is how
    /// Helix's single repeat slot behaves: `f x`, `j`, `Alt-.` finds nothing
    /// to repeat, because `j` overwrote the motion `f x` recorded. Also
    /// called on a reconciliation, where the recorded character may no longer
    /// exist in the text at all.
    ///
    /// Kept as one operation rather than two assignments because the two
    /// halves have no meaning apart from each other: a disclosure about a
    /// find that cannot be repeated, or a repeatable find whose disclosure is
    /// missing, are both a status row lying about the last keystroke.
    pub fn forget_find(&mut self) {
        self.last_find = None;
        self.find_note = FindNote::None;
    }

    /// Open the `:` command line, optionally seeded with `prefix`.
    ///
    /// Opening resets the selection to the best match so `Enter` on a fresh
    /// `:` runs the first palette row rather than nothing.
    pub fn open_command_line(&mut self, prefix: &str) {
        self.command_active = true;
        self.command_line = prefix
            .chars()
            .take(super::editor::MAX_EDITOR_COMMAND_LINE)
            .collect();
        self.command_selection = 0;
    }

    /// Close the `:` command line and discard whatever was typed.
    pub fn close_command_line(&mut self) {
        self.command_active = false;
        self.command_line.clear();
        self.command_selection = 0;
    }

    /// Current palette rows for the command line.
    pub fn command_matches(&self) -> Vec<super::editor::EditorCommandMatch> {
        super::editor::complete_editor_command(&self.command_line)
    }

    /// Append one character to the command line, bounded by
    /// [`MAX_EDITOR_COMMAND_LINE`](super::editor::MAX_EDITOR_COMMAND_LINE).
    ///
    /// Returns `false` when the bound is already reached, so the caller can
    /// leave the line untouched rather than silently dropping the keystroke
    /// after mutating the cursor.
    pub fn push_command_char(&mut self, ch: char) -> bool {
        if self.command_line.chars().count() >= super::editor::MAX_EDITOR_COMMAND_LINE {
            return false;
        }
        self.command_line.push(ch);
        self.command_selection = 0;
        true
    }

    /// Delete the last character of the command line.
    pub fn pop_command_char(&mut self) {
        self.command_line.pop();
        self.command_selection = 0;
    }

    /// Move the palette selection by `delta`, clamped to the current rows.
    pub fn move_command_selection(&mut self, delta: isize) {
        let count = self.command_matches().len();
        if count == 0 {
            self.command_selection = 0;
            return;
        }
        let next = if delta.is_negative() {
            self.command_selection.saturating_sub(delta.unsigned_abs())
        } else {
            self.command_selection
                .saturating_add(delta.unsigned_abs())
                .min(count - 1)
        };
        self.command_selection = next;
    }

    /// Replace the command line with `value`, as palette selection does.
    pub fn accept_command_completion(&mut self, value: &str) {
        self.command_line = value.to_string();
        self.command_selection = 0;
    }

    // ── `/` search ──────────────────────────────────────────────────────────

    /// Open the `/` or `?` search line, seeded with `seed`.
    ///
    /// Seeding is why `host_status` on the cursor and `/` are the same
    /// gesture: the common case is refining a word you are already looking
    /// at, and starting from an empty line makes the first keystrokes
    /// redundant. `?` seeds from the last accepted pattern instead, so
    /// reversing a search never needs the pattern retyped.
    ///
    /// `forward` is the chord's own direction, and it is part of opening the
    /// line rather than a second call so no caller can open a reverse search
    /// that adopts forwards.
    pub fn open_search_line(&mut self, seed: &str, forward: bool) {
        self.search_active = true;
        self.search_forward = forward;
        self.search_line = seed
            .chars()
            .take(super::editor::MAX_EDITOR_SEARCH_LINE)
            .collect();
        self.search_index = 0;
    }

    /// Close the `/` line and keep the highlight.
    ///
    /// What `Enter` does: the search is a thing you did, not a thing you
    /// cancelled, so the matches stay painted and `n`/`N` keep working.
    pub fn close_search_line(&mut self) {
        self.search_active = false;
    }

    /// Close the `/` line and drop the highlight entirely.
    ///
    /// What `Esc` does. The match set is derived state — leaving it behind
    /// would highlight text the user has explicitly disowned.
    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_line.clear();
        self.search_matches.clear();
        self.search_index = 0;
        self.search_partial = false;
        self.search_forward = true;
    }

    /// Append one character to the search line, bounded by
    /// [`MAX_EDITOR_SEARCH_LINE`](super::editor::MAX_EDITOR_SEARCH_LINE).
    ///
    /// Returns `false` at the bound so the caller can leave the line
    /// untouched rather than dropping the keystroke after mutating it.
    pub fn push_search_char(&mut self, ch: char) -> bool {
        if self.search_line.chars().count() >= super::editor::MAX_EDITOR_SEARCH_LINE {
            return false;
        }
        self.search_line.push(ch);
        true
    }

    /// Delete the last character of the search line.
    pub fn pop_search_char(&mut self) {
        self.search_line.pop();
    }

    /// Adopt a resolved search and park the cursor on its first match.
    ///
    /// Returns the offset the cursor should move to, or `None` when nothing
    /// matched — a search that finds nothing must leave the cursor alone
    /// rather than yanking it to the top of the file.
    ///
    /// The match chosen is the first one in the direction the line was opened
    /// with ([`search_forward`](Self::search_forward)), and it is inclusive
    /// at both ends: a cursor already sitting on a match must stay on it
    /// while the pattern is being refined, or every keystroke walks the
    /// search further away.
    pub fn adopt_search(
        &mut self,
        outcome: super::editor::SearchOutcome,
        cursor: usize,
    ) -> Option<usize> {
        self.search_matches = outcome.matches;
        self.search_partial = outcome.truncated || outcome.bounded;
        self.search_index = if self.search_forward {
            super::editor::first_match_index(&self.search_matches, cursor)
        } else {
            super::editor::last_match_index(&self.search_matches, cursor)
        }
        .unwrap_or(0);
        self.search_matches
            .get(self.search_index)
            .map(|span| span.start)
    }

    /// Move to the next/previous match, wrapping at either end.
    ///
    /// Returns the new offset so the caller can move the cursor and scroll
    /// the viewport; `None` means there is nothing to move to.
    pub fn step_search(&mut self, forward: bool) -> Option<usize> {
        let index = if forward {
            super::editor::next_match_index(&self.search_matches, self.cursor_byte)
        } else {
            super::editor::previous_match_index(&self.search_matches, self.cursor_byte)
        }?;
        self.search_index = index;
        self.search_matches.get(index).map(|span| span.start)
    }

    /// Whether a search highlight is currently painted.
    pub fn has_search(&self) -> bool {
        !self.search_matches.is_empty()
    }

    // ── Registers ───────────────────────────────────────────────────────────

    /// Store one yank or delete in `register`.
    ///
    /// The one write path, so the cap, the blackhole rule and the search
    /// register's read-only rule cannot be bypassed by a second caller. The
    /// returned outcome is what lets the command layer disclose a truncated
    /// write instead of letting it read as a complete yank.
    pub fn write_register(&mut self, register: RegisterName, text: &str) -> RegisterWrite {
        self.registers.write(register, text)
    }

    /// The text `register` currently holds, as a **borrow**.
    ///
    /// A borrow rather than a copy, and not a second buffer: the named slots
    /// live in [`registers`](Self::registers) and the search register is a
    /// projection of [`search_line`](Self::search_line) — the same bytes the
    /// search line owns, read where they already are. Refuses with a reason
    /// rather than answering empty, because a paste from a register nobody
    /// wrote must say so instead of inserting nothing.
    pub fn read_register(&self, register: RegisterName) -> Result<&str, RegisterRefusal> {
        match register {
            RegisterName::Search => {
                if self.search_line.is_empty() {
                    Err(RegisterRefusal::NoSearch)
                } else {
                    Ok(&self.search_line)
                }
            }
            other => self.registers.read(other),
        }
    }

    /// Seed the unnamed register from the system clipboard.
    ///
    /// D1 of the M007 plan: the buffer never reads the clipboard, so the App
    /// layer reads it **once**, at document open, and hands the bytes here.
    /// That is what makes `p` paste what the reader copied in another
    /// application while the paste path stays synchronous and clipboard-free.
    pub fn seed_unnamed_register(&mut self, text: &str) -> RegisterWrite {
        self.registers.write(RegisterName::Unnamed, text)
    }
}

// ── The jumplist ───────────────────────────────────────────────────────────

/// Most visited positions the jumplist keeps.
///
/// Named rather than written into the store: the bound is a claim about the
/// surface, and the picker states it. A push past it drops the **oldest**
/// entry — the one a chronological history can best spare — and reports that
/// it did, so the drop is disclosed rather than silent.
pub const MAX_JUMPLIST_ENTRIES: usize = 32;

/// Which attached document a jumplist entry addresses.
///
/// One document is attached at a time, so the store never spans documents.
/// An entry still records its identity so the picker can **refuse** an offset
/// made against a document the reader is no longer looking at, instead of
/// applying it to whatever is attached now. An identity is a daemon id plus
/// the locator and scope the daemon resolved — never document text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JumplistIdentity {
    /// The daemon's id for the attachment.
    pub document_id: String,
    /// The document's path relative to [`scope`](Self::scope).
    pub relative_path: String,
    /// The authority root the document was opened under.
    pub scope: DocumentScope,
}

impl JumplistIdentity {
    /// The identity of one attachment, as the client reported it.
    pub fn of(info: &DocumentAttachmentInfo) -> Self {
        Self {
            document_id: info.document_id.clone(),
            relative_path: info.relative_path.clone(),
            scope: info.scope,
        }
    }
}

/// One visited position: where a named jump started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumplistEntry {
    /// The document [`byte`](Self::byte) addresses.
    pub identity: JumplistIdentity,
    /// Byte offset into that document.
    pub byte: usize,
}

/// What one push did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumplistPush {
    /// Recorded below the bound.
    Recorded,
    /// Recorded at the bound: the oldest entry was dropped.
    DroppedOldest,
}

/// A bounded, chronological history of visited positions.
///
/// [`cursor`](Self::cursor) is the **picker's** highlight, not a stepping
/// cursor. D1 of the M006 plan resolved to the picker as the whole delivered
/// gesture, so there is no `Ctrl-i`/`Ctrl-o` pair to step with — see
/// `plans/implementation/tui-editor-helix-capability-parity/006-jumplist.md`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Jumplist {
    /// Oldest first: a history, not a set.
    entries: Vec<JumplistEntry>,
    /// Index into the **newest-first** rows the picker offers.
    cursor: usize,
    picker_open: bool,
}

impl Jumplist {
    /// Record `entry`, dropping the oldest when the bound is reached.
    pub fn push(&mut self, entry: JumplistEntry) -> JumplistPush {
        let mut outcome = JumplistPush::Recorded;
        if self.entries.len() >= MAX_JUMPLIST_ENTRIES {
            self.entries.remove(0);
            outcome = JumplistPush::DroppedOldest;
        }
        self.entries.push(entry);
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        outcome
    }

    /// Forget every entry and drop the picker.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.cursor = 0;
        self.picker_open = false;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries as recorded: **oldest first**.
    pub fn entries(&self) -> &[JumplistEntry] {
        &self.entries
    }

    /// Whether the store is holding as many entries as it can.
    pub fn at_bound(&self) -> bool {
        self.entries.len() >= MAX_JUMPLIST_ENTRIES
    }

    pub fn picker_open(&self) -> bool {
        self.picker_open
    }

    /// Open the picker on the **newest** entry.
    ///
    /// The most recent jump is the one a reader most often wants back, so the
    /// highlight starts there rather than on the oldest.
    pub fn open_picker(&mut self) {
        self.picker_open = true;
        self.cursor = 0;
    }

    pub fn close_picker(&mut self) {
        self.picker_open = false;
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The `index`-th row of the picker, counting from the newest.
    pub fn row(&self, index: usize) -> Option<&JumplistEntry> {
        self.entries
            .len()
            .checked_sub(index + 1)
            .and_then(|at| self.entries.get(at))
    }

    /// The highlighted row, if there is one.
    pub fn selected(&self) -> Option<&JumplistEntry> {
        self.row(self.cursor)
    }

    /// Move the highlight by `delta`, clamped to the rows that exist.
    ///
    /// Clamped rather than wrapped: a picker that jumped from the newest row
    /// to the oldest on one `k` would make `Enter` land somewhere the reader
    /// did not walk to.
    pub fn move_cursor(&mut self, delta: isize) {
        let next = if delta.is_negative() {
            self.cursor.saturating_sub(delta.unsigned_abs())
        } else {
            self.cursor.saturating_add(delta.unsigned_abs())
        };
        self.cursor = next.min(self.entries.len().saturating_sub(1));
    }
}

// ── The register store ─────────────────────────────────────────────────────

/// Most bytes one register stores.
///
/// A register is a **bounded fragment**, not a buffer. Sixteen kibibytes is
/// far more than a line, a paragraph or a function body — the things a reader
/// yanks to put somewhere else — and it is small enough that twenty-seven of
/// them cannot be a document copy. A write that would exceed the cap stores
/// the first [`MAX_REGISTER_BYTES`] bytes, stopping on a character boundary,
/// and reports the truncation: the bound is disclosed at write time rather
/// than left for the reader to notice that a paste came out short.
pub const MAX_REGISTER_BYTES: usize = 16 * 1024;

/// One register name, as `"` accepts it.
///
/// A **closed set**, parsed once at the capture, so every consumer after it —
/// the store, the read refusal, the message a refusal carries — has one
/// vocabulary rather than a `char` and a rule about which chars are legal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterName {
    /// `"` — the unnamed register, which every yank and every delete also
    /// writes. Bare `p` reads it.
    Unnamed,
    /// `_` — the blackhole. Writes go nowhere and reads are refused.
    Blackhole,
    /// `/` — the last accepted search pattern. A read-only projection of the
    /// search line, never a copy.
    Search,
    /// `a`–`z` — a named slot.
    Named(char),
}

impl RegisterName {
    /// Parse one captured character, or `None` when it is not a register.
    ///
    /// The one place the register set is written down. The dispatch reports an
    /// invalid name with the same set, so a key that is not a register is a
    /// stated refusal rather than a silent non-selection.
    pub fn parse(name: char) -> Option<Self> {
        match name {
            '"' => Some(RegisterName::Unnamed),
            '_' => Some(RegisterName::Blackhole),
            '/' => Some(RegisterName::Search),
            'a'..='z' => Some(RegisterName::Named(name)),
            _ => None,
        }
    }

    /// How a message names this register.
    pub fn label(self) -> String {
        match self {
            RegisterName::Unnamed => "the unnamed register".to_string(),
            RegisterName::Blackhole => "the blackhole register".to_string(),
            RegisterName::Search => "the search register".to_string(),
            RegisterName::Named(name) => format!("register `{name}`"),
        }
    }
}

/// What one register write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterWrite {
    /// Stored whole.
    Stored,
    /// Stored the first `stored` bytes of `requested`: the cap was reached.
    Truncated { stored: usize, requested: usize },
    /// The blackhole register: nothing was stored, by design.
    Discarded,
    /// `/` is the search register, a projection of the search line; it cannot
    /// be written.
    ReadOnly,
}

/// Why a register could not be read.
///
/// A refusal rather than an empty string, because "the blackhole holds
/// nothing" and "you never yanked into `a`" are answers a paste owes the
/// reader. An empty paste that silently inserts nothing is the failure this
/// enum exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterRefusal {
    /// `_` — the blackhole never holds anything.
    Blackhole,
    /// `/` — no search has been accepted yet.
    NoSearch,
    /// The register has never been written, or was cleared.
    Empty(RegisterName),
}

impl RegisterRefusal {
    /// The sentence the reader is given.
    pub fn message(self) -> String {
        match self {
            RegisterRefusal::Blackhole => {
                "the blackhole register holds nothing — `_` discards by design".to_string()
            }
            RegisterRefusal::NoSearch => {
                "the search register is empty: no search has been accepted yet".to_string()
            }
            RegisterRefusal::Empty(register) => format!(
                "{} is empty: yank or delete into it first",
                register.label()
            ),
        }
    }
}

/// The bounded, document-local set of registers.
///
/// Sparse by construction: a reader who yanks to `a` and `q` allocates two
/// slots, not twenty-eight. The unnamed register is a field rather than a map
/// key because every yank and every delete writes it, so it would be the one
/// entry the map always has.
///
/// Three rules live here rather than at each caller:
///
/// - **The cap.** [`MAX_REGISTER_BYTES`] per entry, applied on every write and
///   reported as [`RegisterWrite::Truncated`].
/// - **The blackhole writes nothing.** `_` is [`RegisterWrite::Discarded`] and
///   [`RegisterRefusal::Blackhole`] — never a stored empty string, which would
///   read back as "empty" rather than "this register refuses".
/// - **The search register is read-only.** The store cannot project the search
///   line ([`TuiDocumentPresentation::read_register`] does, from the line's own
///   state), so a write to `/` is refused here and a read of it is refused
///   here only when a caller bypassed the presentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registers {
    /// The named slots, keyed by letter.
    named: BTreeMap<char, String>,
    /// The unnamed register.
    unnamed: String,
}

impl Registers {
    /// Store `text` in `register`, applying the cap and the special-register
    /// rules.
    pub fn write(&mut self, register: RegisterName, text: &str) -> RegisterWrite {
        match register {
            RegisterName::Blackhole => RegisterWrite::Discarded,
            RegisterName::Search => RegisterWrite::ReadOnly,
            RegisterName::Unnamed => self.write_unnamed(text),
            RegisterName::Named(name) => {
                let requested = text.len();
                let (bounded, stored) = bounded_register_text(text);
                self.named.insert(name, bounded.to_string());
                match stored {
                    Some(stored) => RegisterWrite::Truncated { stored, requested },
                    None => RegisterWrite::Stored,
                }
            }
        }
    }

    /// The text `register` holds, borrowed from the store.
    ///
    /// `/` is answered by [`TuiDocumentPresentation::read_register`], which is
    /// where the search line lives; reaching here with it is a caller error
    /// and gets the same refusal as an empty search rather than a panic.
    pub fn read(&self, register: RegisterName) -> Result<&str, RegisterRefusal> {
        let stored = match register {
            RegisterName::Unnamed => Some(self.unnamed.as_str()),
            RegisterName::Named(name) => self.named.get(&name).map(String::as_str),
            RegisterName::Blackhole | RegisterName::Search => None,
        };
        match register {
            RegisterName::Blackhole => Err(RegisterRefusal::Blackhole),
            RegisterName::Search => Err(RegisterRefusal::NoSearch),
            _ => match stored {
                Some(text) if !text.is_empty() => Ok(text),
                _ => Err(RegisterRefusal::Empty(register)),
            },
        }
    }

    /// Forget every register.
    pub fn clear(&mut self) {
        self.named.clear();
        self.unnamed.clear();
    }

    /// Whether no register holds anything.
    pub fn is_empty(&self) -> bool {
        self.named.is_empty() && self.unnamed.is_empty()
    }

    /// How many slots hold text. Used by the tests to prove a multi-member
    /// yank writes **one** entry rather than one per member.
    pub fn len(&self) -> usize {
        self.named
            .values()
            .filter(|text| !text.is_empty())
            .count()
            .saturating_add(usize::from(!self.unnamed.is_empty()))
    }

    fn write_unnamed(&mut self, text: &str) -> RegisterWrite {
        let requested = text.len();
        let (bounded, stored) = bounded_register_text(text);
        self.unnamed = bounded.to_string();
        match stored {
            Some(stored) => RegisterWrite::Truncated { stored, requested },
            None => RegisterWrite::Stored,
        }
    }
}

/// Bound one register write, returning the stored prefix and — when it was
/// cut — how many bytes survived.
///
/// The cut walks back to a character boundary, so a cap reached inside a
/// multi-byte character stores one byte less rather than panicking on a
/// `&str` slice. This is the one place the cap is applied; a second one would
/// be a second answer to what a bounded register holds.
fn bounded_register_text(text: &str) -> (&str, Option<usize>) {
    if text.len() <= MAX_REGISTER_BYTES {
        return (text, None);
    }
    let mut end = MAX_REGISTER_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], Some(end))
}

// ── The keystroke recorder ─────────────────────────────────────────────────

/// One recorded key: the chord a replay feeds back through the dispatch.
///
/// The event's *kind* (press, repeat, release) is transport detail and is not
/// recorded — a chord is what a replay reproduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedKey {
    /// The key code.
    pub code: KeyCode,
    /// The modifier bitmask, exactly as the reader pressed it.
    pub modifiers: KeyModifiers,
}

impl RecordedKey {
    /// Record one key event.
    pub fn of(event: KeyEvent) -> Self {
        Self {
            code: event.code,
            modifiers: event.modifiers,
        }
    }

    /// The event a replay dispatches for this key.
    pub fn event(self) -> KeyEvent {
        KeyEvent::new(self.code, self.modifiers)
    }

    /// Whether this key is `Esc`.
    ///
    /// The one key a replay reads as an **interrupt** rather than as a
    /// gesture: the `Esc` that ended the insert is never recorded, so an `Esc`
    /// in the log can only be the reader stopping a replay.
    pub fn is_escape(self) -> bool {
        self.code == KeyCode::Esc
    }
}

/// What one [`KeyRecorder::record`] call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderBound {
    /// Appended below the bound.
    Recorded,
    /// The bound was crossed: the key was **not** appended, and
    /// [`KeyRecorder::finish`] will discard this gesture rather than repeat a
    /// prefix of it. The caller discloses this once.
    ReachedCap,
    /// Still at the bound: a later key of the same gesture refuses silently,
    /// because the disclosure was made when the bound was crossed.
    AtCap,
}

/// A bounded log of key events — the insert gesture `.` repeats.
///
/// **Capture happens after dispatch.** The reader's key is recorded only once
/// the dispatch has run, so a replayed key is never recorded (a replay that
/// recorded itself would grow the log on every press) and a key the buffer
/// refused is still recorded — the reader pressed it, and a replay owes them
/// the same refusal rather than a silently shorter gesture.
///
/// **Bounded by [`MAX_RECORDED_KEYS`](super::editor::MAX_RECORDED_KEYS).**
/// Reaching the bound **discards the gesture** instead of replaying its
/// prefix, because half an insert is a different edit, and it is disclosed
/// rather than dropped silently.
///
/// The open capture and the last completed gesture are two states of one
/// recorder, not two stores: `.` sees a gesture only once it has ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyRecorder {
    /// The gesture being captured, oldest first.
    keys: Vec<RecordedKey>,
    /// The last completed gesture, if it was kept entire.
    last_insert: Vec<RecordedKey>,
    /// Whether a gesture capture is open.
    recording: bool,
    /// Whether a replay is driving the dispatch.
    replaying: bool,
    /// Whether this capture crossed the bound and must not be kept.
    overflowed: bool,
    /// Whether the replay in progress was asked to stop between keys.
    interrupted: bool,
}

impl KeyRecorder {
    /// Open a capture. A capture already open keeps its keys.
    pub fn begin(&mut self) {
        self.recording = true;
        self.overflowed = false;
    }

    /// Append `key` to the open capture.
    ///
    /// Refuses rather than truncating at
    /// [`MAX_RECORDED_KEYS`](super::editor::MAX_RECORDED_KEYS); see
    /// [`RecorderBound`].
    pub fn record(&mut self, key: RecordedKey) -> RecorderBound {
        debug_assert!(
            self.recording,
            "`record` is only called while a capture is open — an unopened \
             recorder would silently hold no keys"
        );
        if self.overflowed {
            return RecorderBound::AtCap;
        }
        if self.keys.len() >= super::editor::MAX_RECORDED_KEYS {
            self.overflowed = true;
            return RecorderBound::ReachedCap;
        }
        self.keys.push(key);
        RecorderBound::Recorded
    }

    /// Close the capture, keeping it for `.` when it did not overflow.
    ///
    /// Returns whether a gesture is now repeatable. An **empty** capture keeps
    /// the gesture `.` already had: `i` then `Esc` typed nothing, so it is not
    /// a repeatable insert.
    pub fn finish(&mut self) -> bool {
        self.recording = false;
        let kept = !self.overflowed && !self.keys.is_empty();
        if kept {
            self.last_insert = std::mem::take(&mut self.keys);
        } else {
            self.keys.clear();
        }
        self.overflowed = false;
        kept
    }

    /// Close the capture and throw it away, keeping the previous gesture.
    ///
    /// What a key that is not `Esc` does when it takes the keyboard away
    /// mid-gesture — leaving the route, moving focus to the tree, a document
    /// switch. Abandoning a partial gesture is not the same as ending one, and
    /// only `Esc` ends one.
    pub fn discard(&mut self) {
        self.recording = false;
        self.overflowed = false;
        self.keys.clear();
    }

    /// Forget everything: the capture, the gesture and the replay flags.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The gesture `.` repeats, or `None` when nothing complete is recorded.
    pub fn last_insert(&self) -> Option<&[RecordedKey]> {
        (!self.last_insert.is_empty()).then_some(self.last_insert.as_slice())
    }

    /// How many keys the open capture holds.
    pub fn captured_len(&self) -> usize {
        self.keys.len()
    }

    /// Whether a gesture capture is open.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Whether a replay is driving the dispatch.
    pub fn is_replaying(&self) -> bool {
        self.replaying
    }

    /// Take the replay flag, refusing a nested replay.
    ///
    /// `false` means one is already running: a replay re-entering itself would
    /// run a second gesture over the first one's half-applied edits.
    pub fn begin_replay(&mut self) -> bool {
        if self.replaying {
            return false;
        }
        self.replaying = true;
        self.interrupted = false;
        true
    }

    /// Release the replay flag.
    pub fn end_replay(&mut self) {
        self.replaying = false;
        self.interrupted = false;
    }

    /// Ask the replay in progress to stop at the next key boundary.
    pub fn interrupt_replay(&mut self) {
        self.interrupted = true;
    }

    /// Whether a replay has been asked to stop.
    ///
    /// Read **between** keys: a key already dispatched completes, so an
    /// interrupt never lands in the middle of an edit.
    pub fn replay_interrupted(&self) -> bool {
        self.interrupted
    }
}

impl TuiDocumentSession {
    pub fn new(client: Arc<dyn CoreClient>) -> Self {
        Self {
            controller: Arc::new(DocumentController::new(Arc::new(CoreClientTransport(
                client,
            )))),
            presentation: TuiDocumentPresentation::default(),
        }
    }

    /// Adopt an existing controller.
    ///
    /// Used to qualify this seam against a scripted transport, and by
    /// reconnect flows that keep one controller across a transport
    /// replacement. Ownership and semantics are unchanged: the adopted
    /// controller is still the sole owner of the text.
    pub fn from_controller(controller: Arc<DocumentController>) -> Self {
        Self {
            controller,
            presentation: TuiDocumentPresentation::default(),
        }
    }

    pub fn controller(&self) -> Arc<DocumentController> {
        Arc::clone(&self.controller)
    }

    pub fn presentation(&self) -> &TuiDocumentPresentation {
        &self.presentation
    }

    pub fn presentation_mut(&mut self) -> &mut TuiDocumentPresentation {
        &mut self.presentation
    }

    /// Read the current replica for rendering, without retaining it.
    ///
    /// The returned snapshot is a handle to the controller's own buffer and
    /// is dropped at the end of the caller's scope, so the TUI never holds a
    /// second text buffer.
    pub fn try_snapshot(&self) -> Option<DocumentSnapshot> {
        self.controller.try_snapshot().map(|(snapshot, _)| snapshot)
    }

    /// Non-textual attachment metadata, safe to hold across frames.
    pub fn attachment_info(&self) -> Option<DocumentAttachmentInfo> {
        self.controller.try_attachment_info()
    }

    /// The identity a jumplist entry records, when a document is attached.
    ///
    /// Read from the attachment rather than kept on the presentation: the
    /// identity is the controller's, and a copy beside it could disagree with
    /// the document actually on screen.
    pub fn jumplist_identity(&self) -> Option<JumplistIdentity> {
        self.attachment_info()
            .map(|info| JumplistIdentity::of(&info))
    }

    /// Attach one document under `scope`.
    ///
    /// `scope` decides which authority root the daemon confines the document
    /// to; the TUI forwards it rather than choosing, so the daemon remains
    /// the only place a root is resolved.
    pub async fn open(
        &self,
        project_id: String,
        workspace_id: String,
        relative_path: String,
        writable: bool,
        scope: DocumentScope,
    ) -> Result<(), DocumentControllerError> {
        self.controller
            .open(project_id, workspace_id, relative_path, writable, scope)
            .await
    }

    /// Create a new file holding `initial_text` under `scope` and attach it.
    pub async fn create(
        &self,
        project_id: String,
        workspace_id: String,
        relative_path: String,
        initial_text: String,
        scope: DocumentScope,
    ) -> Result<(), DocumentControllerError> {
        self.controller
            .create(project_id, workspace_id, relative_path, initial_text, scope)
            .await
    }

    /// Delete one file under `scope` through a fresh controller over the same
    /// transport.
    ///
    /// A delete is not an attachment operation, so it needs no replica; it
    /// only needs the same daemon and the same containment checks.
    pub async fn delete_file(
        client: Arc<dyn CoreClient>,
        project_id: String,
        workspace_id: String,
        relative_path: String,
        scope: DocumentScope,
    ) -> Result<bool, DocumentControllerError> {
        let controller = DocumentController::new(Arc::new(CoreClientTransport(client)));
        controller
            .delete_file(project_id, workspace_id, relative_path, scope)
            .await
    }

    /// The scope the attached document was opened under, if any.
    ///
    /// Drives the global/project badge in the editor chrome and decides
    /// whether a successful save should refresh the agent registry.
    pub fn scope(&self) -> Option<DocumentScope> {
        self.controller.try_attachment_info().map(|info| info.scope)
    }

    pub async fn state(&self) -> DocumentState {
        self.controller.state().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use tokio::sync::Mutex;

    struct FakeCoreClient(Mutex<VecDeque<CoreResponse>>);

    #[async_trait]
    impl CoreClient for FakeCoreClient {
        async fn request(
            &self,
            _request: RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, crate::error::AppError> {
            self.0.lock().await.pop_front().ok_or_else(|| {
                crate::error::AppError::Other(anyhow::anyhow!("no scripted response"))
            })
        }

        fn subscribe(
            &self,
        ) -> tokio::sync::mpsc::Receiver<
            codegg_protocol::core::EventEnvelope<codegg_protocol::core::CoreEvent>,
        > {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            rx
        }
    }

    #[tokio::test]
    async fn tui_session_opens_through_the_shared_replica() {
        let client = Arc::new(FakeCoreClient(Mutex::new(VecDeque::from([
            CoreResponse::DocumentSnapshot {
                snapshot: codegg_protocol::document::DocumentSnapshotDto {
                    document_id: "doc".into(),
                    project_id: "project".into(),
                    workspace_id: "workspace".into(),
                    relative_path: "src/lib.rs".into(),
                    revision: 2,
                    text: "text".into(),
                    dirty: false,
                    conflicted: false,
                    writer: true,
                    disk_base_digest: "digest".into(),
                    scope: DocumentScope::Workspace,
                },
                writer_lease: Some("lease".into()),
                lsp_degraded: false,
            },
        ]))));
        let session = TuiDocumentSession::new(client);
        let controller = session.controller();
        session
            .open(
                "project".into(),
                "workspace".into(),
                "src/lib.rs".into(),
                true,
                DocumentScope::Workspace,
            )
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&controller, &session.controller()));
        assert_eq!(session.state().await, DocumentState::Synced);
        assert_eq!(controller.snapshot().await.unwrap().0.to_string(), "text");

        let mut session = session;
        *session.presentation_mut() = TuiDocumentPresentation {
            cursor_byte: 2,
            viewport_line: 4,
            ..TuiDocumentPresentation::default()
        };
        assert_eq!(session.presentation().cursor_byte, 2);
        assert_eq!(
            session.controller().snapshot().await.unwrap().0.to_string(),
            "text"
        );
    }

    #[test]
    fn a_reset_presentation_opens_in_normal_and_the_two_axes_agree() {
        let mut presentation = TuiDocumentPresentation {
            cursor_byte: 9,
            mode: EditorMode::Insert,
            ..TuiDocumentPresentation::default()
        };
        presentation.reset_for_open();

        // A file opens in Normal, like every other reader surface. The
        // danger is not the value itself but that there are two axes: the
        // route's `input_mode` and this presentation's `mode`. A reset that
        // sets one and not the other is exactly the drift that once let a
        // document render with the keyboard nowhere in it.
        assert_eq!(presentation.mode, EditorMode::Normal);
        assert_eq!(presentation.cursor_byte, 0);
        assert_eq!(presentation.viewport_line, 0);
        assert!(presentation.selections.is_empty());
        assert_eq!(presentation.primary_index, 0);
    }

    #[test]
    fn a_reset_presentation_drops_a_carried_over_selection_set() {
        use crate::tui::editor::EditorPoint;
        use crate::tui::helix_selection::{Selection, SelectionMode};
        let mut presentation = TuiDocumentPresentation {
            selections: vec![
                Selection {
                    anchor: EditorPoint { byte: 0, line: 0 },
                    head: EditorPoint { byte: 3, line: 0 },
                    mode: SelectionMode::Char,
                    growing: false,
                },
                Selection {
                    anchor: EditorPoint { byte: 9, line: 1 },
                    head: EditorPoint { byte: 12, line: 1 },
                    mode: SelectionMode::Line,
                    growing: true,
                },
            ],
            primary_index: 1,
            ..TuiDocumentPresentation::default()
        };
        presentation.reset_for_open();
        // Not just empty: the primary index must reset with it. A set that
        // kept `primary_index` pointing past the end would make every
        // primary-scoped accessor answer about a member that no longer exists.
        assert!(presentation.selections.is_empty());
        assert_eq!(presentation.primary_index, 0);
    }

    /// The count accumulates one digit at a time, `0` extends rather than
    /// begins, and the bound saturates instead of wrapping.
    #[test]
    fn a_count_accumulates_digits_and_saturates_at_the_bound() {
        let mut presentation = TuiDocumentPresentation::default();
        assert_eq!(presentation.pending_count, None, "no count to begin with");
        assert!(
            !presentation.extend_count('1'),
            "no refusal at the first digit"
        );
        assert!(!presentation.extend_count('0'));
        assert_eq!(
            presentation.pending_count,
            Some(10),
            "`0` extends a count rather than beginning one"
        );
        for digit in "9999".chars() {
            presentation.extend_count(digit);
        }
        assert_eq!(
            presentation.pending_count,
            Some(super::super::editor::MAX_COUNT),
            "the count is clamped, not wrapped"
        );
        assert!(
            presentation.extend_count('9'),
            "a refused digit is disclosed to the caller"
        );
        assert_eq!(
            presentation.pending_count,
            Some(super::super::editor::MAX_COUNT),
            "and the clamped value stays on screen"
        );
    }

    /// Opening a document takes the count down with the prefix and the
    /// history: it addresses positions in a buffer that is gone.
    #[test]
    fn a_reset_presentation_clears_a_pending_count() {
        let mut presentation = TuiDocumentPresentation {
            pending_count: Some(3),
            ..TuiDocumentPresentation::default()
        };
        presentation.reset_for_open();
        assert_eq!(presentation.pending_count, None);
    }

    #[test]
    fn a_reset_presentation_drops_a_carried_over_search() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_search_line("needle", true);
        presentation.search_matches = vec![0..6, 4..10];
        presentation.search_index = 1;
        presentation.search_partial = true;
        presentation.reset_for_open();
        assert!(!presentation.has_search());
        assert!(!presentation.search_active);
        assert!(presentation.search_line.is_empty());
        assert!(!presentation.search_partial);
    }

    #[test]
    fn the_search_line_is_a_needle_and_never_grows_into_a_buffer() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_search_line("", true);
        for _ in 0..(super::super::editor::MAX_EDITOR_SEARCH_LINE * 2) {
            presentation.push_search_char('x');
        }
        assert_eq!(
            presentation.search_line.chars().count(),
            super::super::editor::MAX_EDITOR_SEARCH_LINE
        );
        presentation.pop_search_char();
        assert_eq!(
            presentation.search_line.chars().count(),
            super::super::editor::MAX_EDITOR_SEARCH_LINE - 1
        );
    }

    // ── The jumplist store ──────────────────────────────────────────────

    /// The store keeps what it was given, oldest first, and the picker reads
    /// it newest first — the order is the history, so it is never sorted.
    #[test]
    fn the_jumplist_keeps_the_order_it_was_given_and_the_picker_reads_it_backwards() {
        let mut jumplist = Jumplist::default();
        let entry = |byte: usize| JumplistEntry {
            identity: JumplistIdentity::default(),
            byte,
        };
        assert_eq!(jumplist.push(entry(3)), JumplistPush::Recorded);
        assert_eq!(jumplist.push(entry(9)), JumplistPush::Recorded);
        assert_eq!(
            jumplist
                .entries()
                .iter()
                .map(|entry| entry.byte)
                .collect::<Vec<_>>(),
            vec![3, 9],
            "chronological: the oldest value stays first"
        );
        jumplist.open_picker();
        assert_eq!(
            jumplist.selected().map(|entry| entry.byte),
            Some(9),
            "the picker opens on the newest entry"
        );
        assert_eq!(jumplist.row(1).map(|entry| entry.byte), Some(3));
        assert_eq!(jumplist.row(2), None, "there is no third row");
    }

    /// The bound is named, the push reports the prune, and the oldest entry
    /// is the one that goes.
    #[test]
    fn pushing_past_the_bound_drops_the_oldest_entry_and_reports_it() {
        let mut jumplist = Jumplist::default();
        for byte in 0..MAX_JUMPLIST_ENTRIES {
            assert_eq!(
                jumplist.push(JumplistEntry {
                    identity: JumplistIdentity::default(),
                    byte,
                }),
                JumplistPush::Recorded,
                "below the bound nothing is dropped"
            );
        }
        assert!(jumplist.at_bound(), "the bound is observable");
        assert_eq!(
            jumplist.push(JumplistEntry {
                identity: JumplistIdentity::default(),
                byte: MAX_JUMPLIST_ENTRIES,
            }),
            JumplistPush::DroppedOldest,
            "at the bound the push says what it cost"
        );
        assert_eq!(jumplist.len(), MAX_JUMPLIST_ENTRIES);
        assert_eq!(
            jumplist.entries().first().map(|entry| entry.byte),
            Some(1),
            "the oldest entry went, not the newest"
        );
    }

    /// The highlight is clamped at both ends and survives an empty list — a
    /// picker that wrapped would make `Enter` land somewhere unwalked.
    #[test]
    fn the_picker_highlight_stops_at_both_ends_instead_of_panicking() {
        let mut jumplist = Jumplist::default();
        jumplist.move_cursor(-1);
        assert_eq!(jumplist.cursor(), 0);
        assert!(jumplist.selected().is_none(), "no rows, no selection");

        for byte in 0..3 {
            jumplist.push(JumplistEntry {
                identity: JumplistIdentity::default(),
                byte,
            });
        }
        jumplist.open_picker();
        jumplist.move_cursor(-1);
        assert_eq!(jumplist.cursor(), 0, "the newest row is the upper end");
        jumplist.move_cursor(9);
        assert_eq!(jumplist.cursor(), 2, "the oldest row is the lower end");
        jumplist.move_cursor(1);
        assert_eq!(jumplist.cursor(), 2, "and it stays there");
        assert_eq!(jumplist.selected().map(|entry| entry.byte), Some(0));
    }

    /// One clear takes the entries and the picker down together: a picker
    /// left open over an empty list is a box with nothing to select.
    #[test]
    fn clearing_the_jumplist_takes_the_picker_with_it() {
        let mut jumplist = Jumplist::default();
        jumplist.push(JumplistEntry {
            identity: JumplistIdentity::default(),
            byte: 4,
        });
        jumplist.open_picker();
        assert!(jumplist.picker_open());

        jumplist.clear();
        assert!(jumplist.is_empty());
        assert!(!jumplist.picker_open());
        assert_eq!(jumplist.cursor(), 0);
    }

    // ── The register store ──────────────────────────────────────────────

    /// The register set is closed and every member parses to its own name.
    /// The two directions matter: a name that parses to the wrong register
    /// would paste from the wrong slot, and a character that parses at all
    /// but is outside the documented set would be a register the docs deny.
    #[test]
    fn every_valid_register_name_parses_and_the_set_is_closed() {
        for letter in 'a'..='z' {
            assert_eq!(
                RegisterName::parse(letter),
                Some(RegisterName::Named(letter)),
                "`{letter}` is a named register"
            );
        }
        for (glyph, expected) in [
            ('"', RegisterName::Unnamed),
            ('_', RegisterName::Blackhole),
            ('/', RegisterName::Search),
        ] {
            assert_eq!(RegisterName::parse(glyph), Some(expected), "`{glyph}`");
        }
        for invalid in ['A', 'Z', '0', '9', ':', '-', ' ', 'é'] {
            assert_eq!(
                RegisterName::parse(invalid),
                None,
                "`{invalid}` is not in the register set"
            );
        }
    }

    /// The cap is a named bound, it is applied on every write, and reaching it
    /// is reported rather than silent. The second half is the char-boundary
    /// half: a cut inside a multi-byte character must store a shorter prefix
    /// rather than panic on a `&str` slice.
    #[test]
    fn a_register_write_is_bounded_and_the_truncation_is_reported() {
        let mut registers = Registers::default();
        let oversized = "x".repeat(MAX_REGISTER_BYTES + 512);
        assert_eq!(
            registers.write(RegisterName::Named('a'), &oversized),
            RegisterWrite::Truncated {
                stored: MAX_REGISTER_BYTES,
                requested: oversized.len(),
            },
            "a write past the cap says how much survived and how much was offered"
        );
        assert_eq!(
            registers
                .read(RegisterName::Named('a'))
                .map(str::len)
                .expect("stored"),
            MAX_REGISTER_BYTES,
            "and the stored value is the bound, not the request"
        );

        // `é` is two bytes, so `MAX_REGISTER_BYTES + 1` bytes ends inside the
        // last character: the cut must back up to the boundary.
        let mut registers = Registers::default();
        let multi_byte = "é".repeat(MAX_REGISTER_BYTES / 2 + 1);
        let outcome = registers.write(RegisterName::Named('b'), &multi_byte);
        assert_eq!(
            outcome,
            RegisterWrite::Truncated {
                stored: MAX_REGISTER_BYTES,
                requested: multi_byte.len(),
            }
        );
        let stored = registers
            .read(RegisterName::Named('b'))
            .expect("stored, and it is valid UTF-8");
        assert_eq!(
            stored.len(),
            MAX_REGISTER_BYTES,
            "the cut lands on a character boundary, not one byte inside one"
        );
        assert_eq!(stored.chars().count(), MAX_REGISTER_BYTES / 2);
    }

    /// The blackhole discards writes and refuses reads. It is not "an empty
    /// register": a stored empty string would read back as merely unwritten,
    /// and the reader would never learn that `_` is a refusal by design.
    #[test]
    fn the_blackhole_register_discards_writes_and_refuses_reads() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.write_register(RegisterName::Unnamed, "kept");

        assert_eq!(
            presentation.write_register(RegisterName::Blackhole, "discarded"),
            RegisterWrite::Discarded
        );
        assert_eq!(
            presentation.read_register(RegisterName::Blackhole),
            Err(RegisterRefusal::Blackhole)
        );
        assert_eq!(
            presentation
                .read_register(RegisterName::Unnamed)
                .expect("the blackhole wrote nothing anywhere"),
            "kept",
            "a blackhole write must not mirror into the unnamed register"
        );
        assert_eq!(presentation.registers.len(), 1, "no slot was created");
    }

    /// `/` projects the search line's own bytes, and refuses before a search
    /// has been accepted. The projection half is what makes it a *read-only
    /// view* rather than a copy that could disagree with the line.
    #[test]
    fn the_search_register_projects_the_search_line_and_refuses_a_read_before_one() {
        let mut presentation = TuiDocumentPresentation::default();
        assert_eq!(
            presentation.read_register(RegisterName::Search),
            Err(RegisterRefusal::NoSearch)
        );

        presentation.open_search_line("needle", true);
        presentation.close_search_line();
        assert_eq!(
            presentation
                .read_register(RegisterName::Search)
                .expect("accepted"),
            presentation.search_line,
            "the register reads the line itself, not a copy of it"
        );

        assert_eq!(
            presentation.write_register(RegisterName::Search, "other"),
            RegisterWrite::ReadOnly
        );
        assert_eq!(
            presentation.search_line, "needle",
            "a write to `/` cannot rewrite the search line"
        );
    }

    /// Opening a document clears every register, named and unnamed: a
    /// register holds bytes from a buffer that is gone, so it goes with the
    /// history, the jumplist and the count.
    #[test]
    fn opening_a_document_clears_every_register() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.write_register(RegisterName::Named('a'), "alpha");
        presentation.write_register(RegisterName::Unnamed, "unnamed");
        assert!(!presentation.registers.is_empty());

        presentation.reset_for_open();

        assert!(presentation.registers.is_empty());
        assert_eq!(
            presentation.read_register(RegisterName::Named('a')),
            Err(RegisterRefusal::Empty(RegisterName::Named('a')))
        );
        assert_eq!(
            presentation.read_register(RegisterName::Unnamed),
            Err(RegisterRefusal::Empty(RegisterName::Unnamed))
        );
    }

    /// One write is one entry: a multi-member yank is concatenated before it
    /// reaches the store, so the store never grows a slot per selection.
    #[test]
    fn one_write_is_one_register_entry_however_many_members_it_came_from() {
        let mut registers = Registers::default();
        assert_eq!(
            registers.write(RegisterName::Named('a'), "one two three"),
            RegisterWrite::Stored
        );
        assert_eq!(registers.len(), 1);
        assert_eq!(
            registers.read(RegisterName::Named('b')),
            Err(RegisterRefusal::Empty(RegisterName::Named('b')))
        );
    }

    // ── The keystroke recorder ──────────────────────────────────────────

    fn key(ch: char) -> RecordedKey {
        RecordedKey::of(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
    }

    /// A gesture is visible to `.` only once it has ended, and it keeps the
    /// order the keys arrived in — a replay that reordered them would be a
    /// different edit.
    #[test]
    fn the_recorder_keeps_a_finished_gesture_in_the_order_its_keys_arrived() {
        let mut recorder = KeyRecorder::default();
        recorder.begin();
        assert!(
            recorder.last_insert().is_none(),
            "a live capture is not a repeatable gesture"
        );
        assert_eq!(recorder.record(key('i')), RecorderBound::Recorded);
        assert_eq!(recorder.record(key('a')), RecorderBound::Recorded);
        assert_eq!(recorder.record(key('b')), RecorderBound::Recorded);
        assert!(
            recorder.last_insert().is_none(),
            "still no completed gesture"
        );
        assert!(recorder.finish(), "a short gesture is kept");

        let recorded: Vec<char> = recorder
            .last_insert()
            .expect("a finished gesture is repeatable")
            .iter()
            .filter_map(|key| match key.code {
                KeyCode::Char(ch) => Some(ch),
                _ => None,
            })
            .collect();
        assert_eq!(
            recorded,
            vec!['i', 'a', 'b'],
            "capture order is replay order"
        );
        assert!(!recorder.is_recording());
    }

    /// The bound **refuses** and the gesture is dropped rather than replayed
    /// short: half an insert is a different edit. The log is observably
    /// bounded — it holds the cap and not the key that crossed it.
    #[test]
    fn the_recorder_refuses_at_its_bound_and_discards_the_gesture() {
        let mut recorder = KeyRecorder::default();
        recorder.begin();
        for _ in 0..crate::tui::editor::MAX_RECORDED_KEYS {
            assert_eq!(recorder.record(key('x')), RecorderBound::Recorded);
        }
        assert_eq!(
            recorder.captured_len(),
            crate::tui::editor::MAX_RECORDED_KEYS,
            "the log stops at the bound rather than growing past it"
        );
        assert_eq!(
            recorder.record(key('y')),
            RecorderBound::ReachedCap,
            "the key that crosses the bound is refused, and disclosed once"
        );
        assert_eq!(
            recorder.record(key('z')),
            RecorderBound::AtCap,
            "later keys of the same gesture refuse without a second disclosure"
        );
        assert_eq!(
            recorder.captured_len(),
            crate::tui::editor::MAX_RECORDED_KEYS,
            "and the refusal did not append the key it refused"
        );
        assert!(
            !recorder.finish(),
            "an overflowed gesture is not repeatable"
        );
        assert!(
            recorder.last_insert().is_none(),
            "a truncated prefix must never become `.`'s gesture"
        );
    }

    /// An abandoned capture is not a gesture, and it does not overwrite the
    /// one `.` already has.
    #[test]
    fn discarding_a_capture_keeps_the_previous_gesture() {
        let mut recorder = KeyRecorder::default();
        recorder.begin();
        recorder.record(key('a'));
        assert!(recorder.finish());

        recorder.begin();
        recorder.record(key('b'));
        recorder.discard();

        assert_eq!(recorder.last_insert().map(<[RecordedKey]>::len), Some(1));
        assert_eq!(recorder.last_insert().unwrap()[0], key('a'));
        assert!(!recorder.is_recording(), "the abandoned capture is closed");
    }

    /// Clearing is one transition, not two: the recorder and the registers
    /// share it, so a gesture cannot outlive the text its registers were
    /// cleared for.
    #[test]
    fn opening_a_document_clears_the_recording_with_the_registers() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.write_register(RegisterName::Named('a'), "alpha");
        presentation.recorder.begin();
        presentation.recorder.record(key('a'));
        assert!(presentation.recorder.finish());
        assert!(presentation.recorder.last_insert().is_some());

        presentation.reset_for_open();

        assert!(presentation.registers.is_empty());
        assert!(
            presentation.recorder.last_insert().is_none(),
            "the gesture is cleared by the same transition as the registers"
        );
        assert!(!presentation.recorder.is_recording());
        assert!(!presentation.recorder.is_replaying());
    }

    /// A replay cannot nest, and ending one releases the flag so the next
    /// repeat can run.
    #[test]
    fn a_second_replay_is_refused_until_the_first_one_ends() {
        let mut recorder = KeyRecorder::default();
        assert!(recorder.begin_replay());
        assert!(recorder.is_replaying());
        assert!(
            !recorder.begin_replay(),
            "a nested replay would run over its own half-applied edits"
        );
        recorder.end_replay();
        assert!(!recorder.is_replaying());
        assert!(
            recorder.begin_replay(),
            "the refusal is a state, not a permanent lock"
        );
    }

    /// The interrupt is a flag the driver sets and reads **between** keys, so
    /// starting a replay clears a leftover interrupt rather than skipping the
    /// whole gesture.
    #[test]
    fn an_interrupt_survives_to_the_next_key_boundary_and_not_past_a_new_replay() {
        let mut recorder = KeyRecorder::default();
        assert!(recorder.begin_replay());
        assert!(!recorder.replay_interrupted());
        recorder.interrupt_replay();
        assert!(recorder.replay_interrupted());
        recorder.end_replay();
        assert!(!recorder.replay_interrupted());

        assert!(recorder.begin_replay());
        assert!(
            !recorder.replay_interrupted(),
            "a stale interrupt must not stop a replay that has not started"
        );
    }
}
