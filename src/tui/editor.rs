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
//! ## One keyboard axis
//!
//! Which region of the editor route owns keyboard input is **not** a question
//! this module answers, and that is the fix.
//!
//! This used to be a second axis beside [`EditorMode`], and the two could
//! disagree: opening a file defaulted to `Composer`, which made
//! `handle_editor_key` claim nothing except `Ctrl+E` while the bare-text gate
//! swallowed every printable character. The file rendered and no key reached
//! it, which read as "the editor does not work" rather than as two
//! independent toggles.
//!
//! One axis now answers it, and it is the one every other surface already
//! shows as `NOR`/`INS`: [`crate::tui::input::InputMode`]. Insert puts the
//! keyboard in the document; Normal leaves the Helix verbs the buffer and
//! hands everything else to the bottom input box — the same contract
//! `/terminal` already runs on. See `commands::editor::handle_editor_key`.
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
use super::text_geometry;

/// Maximum retained undo/redo entries per document attachment.
pub const MAX_EDITOR_UNDO_DEPTH: usize = 256;

/// Maximum retained undo/redo payload bytes per document attachment.
///
/// Bounds the inverse transactions a long editing session can retain. Old
/// entries are dropped from the bottom of the stack first, so the most
/// recent history is always the history that survives.
pub const MAX_EDITOR_UNDO_BYTES: usize = 4 * 1024 * 1024;

/// Maximum width of the pending-prefix indicator, in bytes.
///
/// This bounds what the editor can *put on screen*, not what it can hold.
/// [`EditorPrefix`] is a single enum value, so the state machine cannot grow
/// past one variant however many leaves are registered against it; the only
/// thing left to bound is the rendered text. Every
/// [`EditorPrefix::display`] is asserted to fit by
/// `every_pending_prefix_display_fits_the_indicator_bound`, so adding a
/// grammar that needs a longer label fails a test instead of silently
/// truncating into an indistinguishable blank.
pub const MAX_EDITOR_PENDING_PREFIX: usize = 5;

/// The multi-key grammar the editor is waiting on.
///
/// **Grammar rule: one pending prefix, one terminal key consumed.** A key
/// arriving while a prefix is armed either matches a leaf or disarms it;
/// nothing consumes a second key, and no prefix is ever armed on top of
/// another. The find prefixes keep the rule but change what the terminal
/// key *is*: `f` takes the next character as its target rather than as a
/// leaf, which is still exactly one key and still always disarms. A prefix
/// is also disarmed outright whenever the thing it was waiting for stops
/// being the thing on screen — opening a different document, leaving the
/// editor route, moving focus to the tree pane, or a reconciliation
/// rewriting the text under it. A prefix that outlives its target is a
/// gesture applied to the wrong buffer, which is the "relative path
/// resolved against the wrong root" defect in gesture form.
///
/// Adding a grammar means adding a variant here and its leaf in
/// `commands::editor::resolve_pending_prefix`, never a new field on the
/// presentation. That is the whole point of the type: `pending_command` and
/// `yank_prefix_pending` were one flag per feature, which is why Helix's
/// `[`/`]` grammars each wanted another one.
///
/// This module is the presentation model and deliberately knows nothing about
/// key events, so a leaf is named by the glyph it completes rather than by a
/// `KeyCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorPrefix {
    /// Nothing pending: every key resolves against the keymap.
    #[default]
    Idle,
    /// Helix goto mode, armed by `g`. Its only leaf is `gg`.
    Goto,
    /// Helix space mode, armed by `space`.
    Space,
    /// Helix's unimpaired `[` prefix. Armed; its leaves arrive in M006.
    OpenBracket,
    /// Helix's unimpaired `]` prefix. Armed; its leaves arrive in M006.
    CloseBracket,
    /// Helix `f` — waiting for the character to find.
    FindNext,
    /// Helix `F` — waiting for the character to find.
    FindPrevious,
    /// Helix `t` — waiting for the character to find.
    FindTillNext,
    /// Helix `T` — waiting for the character to find.
    FindTillPrevious,
}

impl EditorPrefix {
    /// The find motion this prefix is waiting to complete, when it is one.
    ///
    /// These four are the one place the grammar takes its next key as
    /// **data** rather than as a leaf glyph: after `f`, the character typed
    /// is the target, not a command. That is a categorically different job
    /// from `gg`'s second `g`, which is a command — and it is why
    /// [`Self::leaf`] is `None` for all of them. [`find_target_char`][target]
    /// does the capture; the machine still owns the disarm.
    ///
    /// [target]: crate::tui::commands::editor::find_target_char
    pub fn find_kind(self) -> Option<FindKind> {
        match self {
            EditorPrefix::FindNext => Some(FindKind::Next),
            EditorPrefix::FindPrevious => Some(FindKind::Previous),
            EditorPrefix::FindTillNext => Some(FindKind::TillNext),
            EditorPrefix::FindTillPrevious => Some(FindKind::TillPrevious),
            _ => None,
        }
    }

    /// Whether this prefix waits for a target character rather than a leaf.
    pub fn is_find(self) -> bool {
        self.find_kind().is_some()
    }

    /// The prefix `glyph` arms, when `glyph` is one of the find keys.
    ///
    /// The one table mapping `f`/`t`/`F`/`T` onto their kinds, so an arm
    /// cannot arm `FindPrevious` for `T`.
    pub fn armed_by(glyph: char) -> Option<Self> {
        match glyph {
            'f' => Some(EditorPrefix::FindNext),
            'F' => Some(EditorPrefix::FindPrevious),
            't' => Some(EditorPrefix::FindTillNext),
            'T' => Some(EditorPrefix::FindTillPrevious),
            _ => None,
        }
    }

    /// Whether a key is currently being waited on.
    pub fn is_pending(self) -> bool {
        self != EditorPrefix::Idle
    }

    /// What the editor shows for an armed prefix, or `""` when idle.
    ///
    /// This is the whole grammar's rendering surface, so it is a closed set
    /// rather than accumulated text. `space` renders as the word rather than
    /// the space character it is bound to: a literal space would put nothing
    /// on screen, and a pending prefix the user cannot see is a prefix they
    /// cannot predict. The find prefixes show their key for the same reason
    /// — an armed `f` that drew nothing would be indistinguishable from a
    /// dropped keypress.
    pub fn display(self) -> &'static str {
        match self {
            EditorPrefix::Idle => "",
            EditorPrefix::Goto => "g",
            EditorPrefix::Space => "space",
            EditorPrefix::OpenBracket => "[",
            EditorPrefix::CloseBracket => "]",
            EditorPrefix::FindNext => "f",
            EditorPrefix::FindPrevious => "F",
            EditorPrefix::FindTillNext => "t",
            EditorPrefix::FindTillPrevious => "T",
        }
    }

    /// The glyph completing this prefix's leaf, or `None` when it has none.
    ///
    /// Every leaf is a bare unmodified glyph today, so the dispatch layer is
    /// the only place that needs to know about modifiers. The find prefixes
    /// are `None` on purpose: their next key is a *target character*, not a
    /// leaf, so there is no glyph that could complete them.
    pub fn leaf(self) -> Option<char> {
        match self {
            EditorPrefix::Goto => Some('g'),
            EditorPrefix::Space => Some('y'),
            // `[`/`]` are armed ahead of M006's unimpaired leaves, so nothing
            // matches yet and every second key takes the mismatch path.
            EditorPrefix::Idle
            | EditorPrefix::OpenBracket
            | EditorPrefix::CloseBracket
            | EditorPrefix::FindNext
            | EditorPrefix::FindPrevious
            | EditorPrefix::FindTillNext
            | EditorPrefix::FindTillPrevious => None,
        }
    }

    /// Whether an unmatched key is consumed with the prefix or resolved next.
    ///
    /// `Goto` swallows, because that is the baseline's `gg` behavior and
    /// Helix's: the second key belongs to the chord whether or not it
    /// completes it. The unimpaired prefixes retry, because swallowing there
    /// would mean a `[` followed by a letter lost that letter on its way to
    /// the composer — and their leaves are still two milestones away, so a
    /// prefix that cannot be escaped is worse than one that occasionally
    /// passes a key through.
    ///
    /// The find prefixes retry too, and for the sharper reason: their next
    /// key is a target character, so there is no such thing as a key that
    /// "matches". A key that cannot be a target — an arrow, `Esc`, one of
    /// the two modal openers — is handed back for normal resolution rather
    /// than eaten, which is what keeps `f` from swallowing a keystroke on
    /// its way to the composer.
    pub fn swallows_unmatched(self) -> bool {
        self == EditorPrefix::Goto
    }
}

/// Minimum display columns reserved for the horizontal scroll indicator.
const MIN_EDITOR_TEXT_COLUMNS: u16 = 1;

/// Maximum source bytes examined for one visible logical line in one frame.
///
/// C002: the renderer never materializes a whole logical line larger than
/// this. A line beyond the cap renders from its prefix and is visibly marked
/// truncated; cursor and selection byte offsets stay exact, but horizontal
/// motion past the prefix end clamps to the prefix. Ordinary short lines
/// render exactly as before.
pub const MAX_RENDER_LINE_BYTES: usize = 32 * 1024;

/// Maximum source bytes examined across all visible lines in one frame.
///
/// C002: `longest_visible_width` stops measuring once the frame total reaches
/// this, so a tall viewport of huge lines cannot make one frame expensive.
/// The per-line cap above applies first; this caps their sum.
pub const MAX_FRAME_SOURCE_BYTES: usize = 256 * 1024;

/// Visible suffix marking a huge line rendered from its prefix.
///
/// The renderer appends this (warning-styled) instead of silently showing a
/// prefix as if it were the whole line.
pub const TRUNCATED_LINE_SUFFIX: &str = " …[too long]";

/// Maximum length of the `:` command line.
///
/// The line names one command and at most one argument, so a short bound is
/// the whole grammar. Anything longer is refused at the keystroke rather
/// than buffered, so this field can never grow into a second text buffer —
/// the same reasoning that bounds
/// [`MAX_EDITOR_PENDING_PREFIX`](super::editor::MAX_EDITOR_PENDING_PREFIX).
pub const MAX_EDITOR_COMMAND_LINE: usize = 64;

/// Maximum number of `:` completions offered at once.
///
/// A bounded list keeps the overlay cheap to render every frame and stops a
/// one-character query from producing an unbounded candidate set.
pub const MAX_EDITOR_COMMAND_MATCHES: usize = 8;

/// Maximum length of the `/` search line.
///
/// Same reasoning as [`MAX_EDITOR_COMMAND_LINE`]: a search needle is a query,
/// never document text, so it is refused at the keystroke rather than
/// buffered into something unbounded.
pub const MAX_EDITOR_SEARCH_LINE: usize = 128;

/// Maximum number of retained search matches.
///
/// Bounded so a one-character query in a large file cannot turn the
/// highlight set into an unbounded list. Past the bound the search still
/// works — it reports the matches it kept and says the rest were dropped,
/// rather than silently pretending the document contains only those.
pub const MAX_EDITOR_SEARCH_MATCHES: usize = 2_000;

/// Bytes of the document a `/` search will scan.
///
/// A search is an interactive keystroke, so its cost is paid on the frame
/// the user is waiting for. Beyond this bound the search reports that it
/// covered only the leading window rather than hanging on a huge file.
pub const MAX_EDITOR_SEARCH_BYTES: usize = 2 * 1024 * 1024;

/// One resolved `/` search outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOutcome {
    /// Retained match start offsets, ascending.
    pub matches: Vec<usize>,
    /// Whether matches beyond [`MAX_EDITOR_SEARCH_MATCHES`] existed and were
    /// dropped. Disclosed rather than hidden: a truncated match list that
    /// looks complete is the same defect as unvalidated LSP coverage.
    pub truncated: bool,
    /// Whether the scan stopped at [`MAX_EDITOR_SEARCH_BYTES`] before the
    /// end of the document.
    pub bounded: bool,
}

/// Find every occurrence of `needle` in `snapshot`, bounded.
///
/// Offsets are absolute UTF-8 byte offsets into the replica and are always
/// character boundaries, because they are handed straight to the renderer
/// and to the cursor. The full ascending list is returned rather than a
/// slice starting at the cursor: `n` and `N` index it directly, so the
/// cursor's position is part of what the *caller* asks for
/// ([`next_match_index`]) instead of a post-hoc adjustment here.
pub fn find_matches(snapshot: &DocumentSnapshot, needle: &str) -> SearchOutcome {
    if needle.is_empty() {
        return SearchOutcome {
            matches: Vec::new(),
            truncated: false,
            bounded: false,
        };
    }
    let total = snapshot.len_bytes();
    // Cut the scanned window at a character boundary, and never report the
    // bound as reached when the document already fits inside it.
    let mut window = total.min(MAX_EDITOR_SEARCH_BYTES);
    while window < total && !snapshot.is_byte_boundary(window) {
        window += 1;
    }
    let bounded = window < total;
    let Ok(haystack) = snapshot.read_bytes(0, window) else {
        return SearchOutcome {
            matches: Vec::new(),
            truncated: false,
            bounded,
        };
    };
    let mut matches = Vec::new();
    let mut truncated = false;
    for (offset, _) in haystack.match_indices(needle) {
        if offset + needle.len() > window {
            break;
        }
        if matches.len() == MAX_EDITOR_SEARCH_MATCHES {
            truncated = true;
            break;
        }
        matches.push(offset);
    }
    SearchOutcome {
        matches,
        truncated,
        bounded,
    }
}

// ── Character-finding motions (Helix `f` / `F` / `t` / `T`) ──────────────────

/// Bytes one find-char motion scans away from the cursor.
///
/// Deliberately the same window a `/` search uses rather than a second
/// number to reason about: both are paid on the keystroke, so both need the
/// same reason to stop. Unlike `/`, the window is measured **from the
/// cursor** — `[cursor, cursor + MAX)` forward, `[cursor - MAX, cursor)`
/// backward — because a find jumps *toward* a target rather than
/// enumerating the document.
pub const MAX_EDITOR_FIND_BYTES: usize = MAX_EDITOR_SEARCH_BYTES;

/// Which way a find-char motion travels, and what Helix calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindKind {
    /// Helix `f` — find next char.
    Next,
    /// Helix `F` — find previous char.
    Previous,
    /// Helix `t` — find till next char.
    TillNext,
    /// Helix `T` — find till previous char.
    TillPrevious,
}

impl FindKind {
    /// Which direction this motion travels.
    pub fn forward(self) -> bool {
        matches!(self, Self::Next | Self::TillNext)
    }

    /// The `EditorCommand` this motion is, given its target.
    pub fn command(self, target: char) -> EditorCommand {
        match self {
            Self::Next => EditorCommand::FindNext(target),
            Self::Previous => EditorCommand::FindPrevious(target),
            Self::TillNext => EditorCommand::FindTillNext(target),
            Self::TillPrevious => EditorCommand::FindTillPrevious(target),
        }
    }
}

/// One resolved find-char motion: what to look for, and which way.
///
/// The repeatable-motion record is this type rather than an
/// `Option<EditorCommand>` so that "repeat only find motions" is a property
/// the compiler enforces. `Alt-.` is Helix's *repeat last motion*, and
/// Helix keeps a single slot that every motion overwrites — which is why
/// `j` between two finds is what disarms the repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindMotion {
    pub kind: FindKind,
    pub target: char,
}

impl FindMotion {
    /// Scan for this motion's target from `offset`.
    pub fn outcome(&self, snapshot: &DocumentSnapshot, offset: usize) -> FindOutcome {
        find_char(snapshot, offset, self.target, self.kind.forward())
    }

    /// The `EditorCommand` this motion is.
    pub fn command(self) -> EditorCommand {
        self.kind.command(self.target)
    }
}

/// Where one find-char scan stopped, and why it stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindOutcome {
    /// The target is at this byte offset, always on a character boundary.
    Found(usize),
    /// The whole document was scanned in that direction and the target does
    /// not occur in it.
    Absent,
    /// The scan stopped at [`MAX_EDITOR_FIND_BYTES`] first, so the target
    /// may well exist past it.
    ///
    /// Disclosed rather than folded into `Absent`, for the same reason
    /// [`SearchOutcome::bounded`] is: a result that stopped early and one
    /// that looked everywhere are different facts, and reporting them the
    /// same way is a search that lies by omission.
    Bounded,
}

/// Why the last find-char motion did not land, if it did not.
///
/// Disclosed in the status row rather than swallowed. A motion that stops
/// because the character is not there and a motion that stops because the
/// bound was reached otherwise read identically — as a keypress that was
/// simply dropped — and they call for different answers from the reader
/// (type something else vs. jump through `/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FindNote {
    /// The last find motion landed, or no find motion has run.
    #[default]
    None,
    /// The whole document was scanned; the character does not occur in that
    /// direction.
    Absent(char),
    /// The scan stopped at [`MAX_EDITOR_FIND_BYTES`] first.
    Bounded(char),
}

impl FindNote {
    /// The note for a completed scan.
    pub fn from_outcome(outcome: FindOutcome, target: char) -> Self {
        match outcome {
            FindOutcome::Found(_) => FindNote::None,
            FindOutcome::Absent => FindNote::Absent(target),
            FindOutcome::Bounded => FindNote::Bounded(target),
        }
    }

    /// Status-row text, or `""` when the last find landed.
    pub fn label(self) -> String {
        match self {
            FindNote::None => String::new(),
            FindNote::Absent(target) => format!("no {target:?} in that direction"),
            FindNote::Bounded(target) => format!(
                "no {target:?} within the {mib} MiB scan bound",
                mib = MAX_EDITOR_FIND_BYTES / (1024 * 1024)
            ),
        }
    }
}

/// Bytes read per step of a find-char scan.
///
/// The window is bounded at [`MAX_EDITOR_FIND_BYTES`], but materializing all
/// of it on one keystroke would make `f` pay a 2 MiB copy for a jump the
/// user expects to be instant. Chunks are read with a small overlap — the
/// widest target minus one byte — so a multi-byte target straddling a chunk
/// edge is still wholly inside one of them.
///
/// Overlapping means a target can be seen twice. That is harmless: the first
/// occurrence is returned and the scan stops, and UTF-8 is
/// self-synchronizing, so a valid encoding of `target` appears in the byte
/// stream only where that character actually starts — never at the tail of
/// some other character.
const FIND_SCAN_CHUNK: usize = 8 * 1024;

/// The largest character boundary at or before `offset`, never below `floor`.
fn boundary_at_or_before(snapshot: &DocumentSnapshot, offset: usize, floor: usize) -> usize {
    let mut candidate = offset;
    while candidate > floor && !snapshot.is_byte_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// The smallest character boundary at or after `offset`, never above
/// `ceiling`.
fn boundary_at_or_after(snapshot: &DocumentSnapshot, offset: usize, ceiling: usize) -> usize {
    let mut candidate = offset;
    while candidate < ceiling && !snapshot.is_byte_boundary(candidate) {
        candidate += 1;
    }
    candidate
}

/// Find `target` from `offset`, forward or backward, **across line
/// boundaries**.
///
/// Helix calls out the difference from vi in a single line — *"`f`, `F`,
/// `t` and `T` are not confined to the current line"* — because the current
/// line is what an implementation reaches for by habit. Nothing here knows
/// which line the cursor is on; this is the whole reason the cross-line
/// tests exist rather than being obvious from the code.
///
/// The scan starts **strictly past** the cursor in the direction of travel,
/// so `f x` from a cursor already sitting on `x` finds the *next* `x`
/// rather than standing still, and `F x` likewise looks strictly behind.
///
/// `DocumentSnapshot::read_bytes` refuses a range that does not start and end
/// on character boundaries, which is why this reads in aligned chunks rather
/// than a byte at a time. Reading per byte would not merely be slow: a
/// single `read_bytes(offset, offset + 1)` into the middle of a multi-byte
/// character is an error, so the byte at which a two-byte target begins
/// would be *unreadable*, and the scan would walk past every occurrence of
/// it. That was the first implementation of this function and it silently
/// failed on every document containing a non-ASCII character.
pub fn find_char(
    snapshot: &DocumentSnapshot,
    offset: usize,
    target: char,
    forward: bool,
) -> FindOutcome {
    // The widest match minus one byte, so a target straddling a chunk edge is
    // still wholly inside the next chunk. `str::match_indices` yields the
    // *byte* offset of each match, so no encoding is materialized here.
    let overlap = target.len_utf8() - 1;
    let total = snapshot.len_bytes();
    let cursor = clamp_offset(snapshot, offset);

    if forward {
        let bound = cursor.saturating_add(MAX_EDITOR_FIND_BYTES).min(total);
        let mut at = next_char_boundary(snapshot, cursor).unwrap_or(total);
        while at < bound {
            let ceiling = (at + FIND_SCAN_CHUNK).min(bound);
            let last_chunk = ceiling == bound;
            let hi = boundary_at_or_before(snapshot, ceiling, at);
            if hi == at {
                break;
            }
            let Ok(chunk) = snapshot.read_bytes(at, hi) else {
                // A window we could not read is a window we did not cover,
                // which is exactly what `Bounded` says.
                return FindOutcome::Bounded;
            };
            if let Some((found, _)) = chunk.match_indices(target).next() {
                let at_found = at + found;
                // A target whose last byte is past the bound lies outside the
                // window, so reporting it would be reporting something the
                // bound promised not to look at.
                if at_found + target.len_utf8() <= bound {
                    return FindOutcome::Found(at_found);
                }
            }
            if last_chunk {
                break;
            }
            at = hi - overlap;
        }
        if bound < total {
            FindOutcome::Bounded
        } else {
            FindOutcome::Absent
        }
    } else {
        let bound = cursor.saturating_sub(MAX_EDITOR_FIND_BYTES);
        let mut at = cursor;
        while at > bound {
            let floor = at.saturating_sub(FIND_SCAN_CHUNK).max(bound);
            let last_chunk = floor == bound;
            let lo = boundary_at_or_after(snapshot, floor, at);
            if lo >= at {
                break;
            }
            let Ok(chunk) = snapshot.read_bytes(lo, at) else {
                return FindOutcome::Bounded;
            };
            if let Some(found) = chunk.rfind(target) {
                let at_found = lo + found;
                if at_found >= bound {
                    return FindOutcome::Found(at_found);
                }
            }
            if last_chunk {
                break;
            }
            at = lo + overlap;
        }
        if bound > 0 {
            FindOutcome::Bounded
        } else {
            FindOutcome::Absent
        }
    }
}

/// The word surrounding `offset`, for seeding a `/` search.
///
/// Returns `None` when the cursor is not inside a word, so `/` on
/// whitespace opens an empty line rather than a search for a space.
pub fn word_at(snapshot: &DocumentSnapshot, offset: usize) -> Option<String> {
    let offset = clamp_offset(snapshot, offset);
    let line = cursor_line(snapshot, offset);
    let start = line_start(snapshot, line)?;
    let end = line_content_end(snapshot, line).unwrap_or(start);
    let Ok(line_text) = snapshot.read_bytes(start, end) else {
        return None;
    };
    let column = offset.saturating_sub(start);
    if column > line_text.len() || !line_text.is_char_boundary(column) {
        return None;
    }
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    // The cursor has to be *on* a word, not merely before one. Scanning
    // outward from a position with no scan-backward guard returned the whole
    // line: on the space after `let`, the backward sweep found `let`, the
    // forward sweep broke at that same space, and the gap between them was
    // the entire rest of the line — so `/` on whitespace seeded a search for
    // a whole row instead of opening empty.
    //
    // The test is on the character *at* the cursor, not the one before it.
    // Checking backward meant the space at the end of `let` was judged by the
    // `t` behind it and accepted.
    let under = line_text[column..].chars().next()?;
    if !is_word(under) {
        return None;
    }
    let mut begin = 0usize;
    let mut finish = line_text.len();
    for (index, ch) in line_text[..column].char_indices().rev() {
        if is_word(ch) {
            begin = index;
        } else {
            break;
        }
    }
    for (index, ch) in line_text[column..].char_indices() {
        if is_word(ch) {
            finish = column + index + ch.len_utf8();
        } else {
            break;
        }
    }
    begin = begin.min(column);
    finish = finish.max(column);
    if begin >= finish {
        return None;
    }
    Some(line_text[begin..finish].to_string())
}

/// Index of the first match at or after `cursor`, wrapping to the first.
///
/// Inclusive on purpose, and this is the difference from
/// [`next_match_index`]. Typing `/` while the cursor already sits on a match
/// must leave you **on** that match; a strict `>` would make every keystroke
/// of a live search hop you to the next occurrence, so refining a needle
/// you are looking at would keep running away from you.
pub fn first_match_index(matches: &[usize], cursor: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    match matches.iter().position(|offset| *offset >= cursor) {
        Some(index) => Some(index),
        None => Some(0),
    }
}

/// Index of the first match strictly after `cursor`, wrapping to the first.
///
/// Strict, because this is `n`: pressing it must move you *off* the match you
/// are on, or the first press would look like the key did nothing.
///
/// Wrapping is deliberate: a search that stopped at the last match would
/// make `n` look broken exactly when the user has found what they were
/// looking for and wants to come back to it.
pub fn next_match_index(matches: &[usize], cursor: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    match matches.iter().position(|offset| *offset > cursor) {
        Some(index) => Some(index),
        None => Some(0),
    }
}

/// Index of the last match strictly before `cursor`, wrapping to the last.
pub fn previous_match_index(matches: &[usize], cursor: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    match matches.iter().rposition(|offset| *offset < cursor) {
        Some(index) => Some(index),
        None => Some(matches.len() - 1),
    }
}

/// One `:` command the editor can execute.
///
/// Kept as data rather than a `match` arm at the call site so the palette,
/// the parser, and the help line all read the *same* list: a command that
/// cannot be discovered cannot be executed by muscle memory alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommandName {
    /// `:w` — write the buffer to disk.
    Write,
    /// `:q` — close the document.
    Quit,
    /// `:wq` / `:x` — write, then close.
    WriteQuit,
    /// `:q!` — close, discarding the local draft.
    ForceQuit,
    /// `:e` — reload the buffer from disk.
    Reload,
    /// `:wa` — write every open document (one today).
    WriteAll,
    /// `:new` — create a new document from a template.
    New,
    /// `:open <path>` — open a workspace-relative path.
    Open,
    /// `:complete` — request language-server completions at the cursor.
    Complete,
}

impl EditorCommandName {
    /// The canonical spelling, shown in the palette and matched first.
    pub fn spec(self) -> &'static str {
        match self {
            Self::Write => "w",
            Self::Quit => "q",
            Self::WriteQuit => "wq",
            Self::ForceQuit => "q!",
            Self::Reload => "e",
            Self::WriteAll => "wa",
            Self::New => "new",
            Self::Open => "open",
            Self::Complete => "complete",
        }
    }

    /// One-line help shown next to the palette entry.
    pub fn help(self) -> &'static str {
        match self {
            Self::Write => "write the buffer to disk",
            Self::Quit => "close this document",
            Self::WriteQuit => "write, then close",
            Self::ForceQuit => "close and discard unsaved changes",
            Self::Reload => "reload from disk, discarding the draft",
            Self::WriteAll => "write all open documents",
            Self::New => "create a new file from a template",
            Self::Open => "open a workspace-relative path",
            Self::Complete => "complete the word at the cursor",
        }
    }

    /// Whether the command consumes a trailing argument.
    ///
    /// Drives the fuzzy palette: an argument-taking command is still a valid
    /// completion while the user is mid-argument, and the palette narrows to
    /// the spec only once the line is complete enough to run.
    pub fn takes_argument(self) -> bool {
        matches!(self, Self::Open)
    }

    /// Every command, in palette order.
    ///
    /// One list, so a new command is discoverable the moment it is
    /// executable rather than only after a help line is also updated.
    pub const ALL: &'static [EditorCommandName] = &[
        Self::Write,
        Self::Quit,
        Self::WriteQuit,
        Self::ForceQuit,
        Self::Reload,
        Self::WriteAll,
        Self::New,
        Self::Open,
        Self::Complete,
    ];
}

/// One palette row: the command plus the match quality that ordered it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorCommandMatch {
    pub command: EditorCommandName,
    /// Lower is better. Ties break on [`EditorCommandName::spec`] so the
    /// ordering is deterministic across runs.
    pub score: usize,
}

/// Resolve a complete `:` line (without the leading `:`) to a command and
/// its argument.
///
/// Returns `None` for an unknown name. An extra argument is refused for a
/// command that takes none, so `:wq now` is an error rather than a silent
/// `:wq` — a mistyped command must never do half of what was asked.
pub fn parse_editor_command(line: &str) -> Option<(EditorCommandName, Option<String>)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (name, argument) = match trimmed.split_once(char::is_whitespace) {
        Some((name, rest)) => (name, Some(rest.trim())),
        None => (trimmed, None),
    };
    let command = match name {
        "w" => EditorCommandName::Write,
        "q" => EditorCommandName::Quit,
        "wq" | "x" => EditorCommandName::WriteQuit,
        "q!" => EditorCommandName::ForceQuit,
        "e" => EditorCommandName::Reload,
        "wa" | "wall" => EditorCommandName::WriteAll,
        "new" => EditorCommandName::New,
        "open" | "o" => EditorCommandName::Open,
        "complete" => EditorCommandName::Complete,
        _ => return None,
    };
    let argument = argument.filter(|value| !value.is_empty());
    if argument.is_some() && !command.takes_argument() {
        return None;
    }
    Some((command, argument.map(str::to_string)))
}

/// Fuzzy completions for a partially typed `:` line.
///
/// Matches the command name only — never the argument — because the palette
/// answers "which command did I mean", and `:open <TAB>` must complete the
/// command while the path is still being typed.
///
/// An **exact prefix always outranks a mere subsequence**. `fuzzy_score` is
/// a subsequence matcher, so the query `w` matches `new` (through its `w`)
/// with a *better* raw score than it gives `w` itself; left uncorrected, the
/// one-key query for `w` would rank `new` first. A prefix match is the
/// stronger signal, so it is ranked ahead of every non-prefix match
/// regardless of score, and only then ordered by score.
pub fn complete_editor_command(query: &str) -> Vec<EditorCommandMatch> {
    let query = query.trim();
    // An empty query is the discovery path, and it is served in *declared*
    // order — not alphabetically — so the registry reads as a curated menu
    // rather than an index. Sorting it by score would reorder the list for
    // no benefit. The whole registry is offered, not the query bound: a
    // command the empty palette hides is undiscoverable, and the registry
    // itself is the bound.
    if query.is_empty() {
        return EditorCommandName::ALL
            .iter()
            .map(|command| EditorCommandMatch {
                command: *command,
                score: 0,
            })
            .collect();
    }
    // Two tiers, because a prefix match is a categorically stronger signal
    // than a subsequence match: `fuzzy_score` is a subsequence matcher, so
    // the query `w` matches `new` (through its `w`) as well as `w` itself.
    // A flat score would let a subsequence outrank the exact prefix the user
    // is typing. `PREFIX_TIER` therefore sits below any subsequence score.
    const PREFIX_TIER: usize = 0;
    const SUBSEQUENCE_TIER: usize = 1_000_000;

    let mut matches: Vec<EditorCommandMatch> = EditorCommandName::ALL
        .iter()
        .filter_map(|command| {
            let spec = command.spec();
            if spec.starts_with(query) {
                return Some(EditorCommandMatch {
                    command: *command,
                    // Within the tier, the shortest prefix wins: query `w`
                    // ranks `w` ahead of `wq` and `wa`.
                    score: PREFIX_TIER + spec.len(),
                });
            }
            // `fuzzy_score` returns 0 for a total miss *and* for an empty
            // query, so a non-matching command must be filtered by the score
            // rather than merely sorted by it.
            let score = crate::util::fuzzy::fuzzy_score(query, spec);
            (score > 0).then_some(EditorCommandMatch {
                command: *command,
                score: SUBSEQUENCE_TIER + score,
            })
        })
        .collect();
    matches.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| left.command.spec().cmp(right.command.spec()))
    });
    matches.truncate(MAX_EDITOR_COMMAND_MATCHES);
    matches
}

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
    pub fn rebased(self, cursor_before: usize) -> Self {
        Self {
            cursor_before,
            ..self
        }
    }
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
    /// Helix `e` — the last byte of the word at or after the cursor.
    WordEnd,
    HalfPageDown,
    HalfPageUp,
    /// Helix `f {char}` — the next occurrence, spanning line boundaries.
    FindNext(char),
    /// Helix `F {char}` — the previous occurrence, spanning line boundaries.
    FindPrevious(char),
    /// Helix `t {char}` — see [`Self::FindNext`]; the two differ in Helix's
    /// *selection*, not in where the cursor lands.
    FindTillNext(char),
    /// Helix `T {char}` — see [`Self::FindPrevious`].
    FindTillPrevious(char),
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

impl EditorCommand {
    /// This command as a find motion, when it is one.
    ///
    /// The single place that answers it, so "which motions are repeatable
    /// by `Alt-.`" cannot be answered two slightly different ways by the
    /// recorder and the dispatcher.
    pub fn find_motion(self) -> Option<FindMotion> {
        let (kind, target) = match self {
            Self::FindNext(target) => (FindKind::Next, target),
            Self::FindPrevious(target) => (FindKind::Previous, target),
            Self::FindTillNext(target) => (FindKind::TillNext, target),
            Self::FindTillPrevious(target) => (FindKind::TillPrevious, target),
            _ => return None,
        };
        Some(FindMotion { kind, target })
    }

    /// Whether this command is a find motion.
    pub fn is_find_motion(self) -> bool {
        self.find_motion().is_some()
    }
}

// ── Offset safety ────────────────────────────────────────────────────────────

/// Clamp `offset` into `0..=len_bytes` and onto a UTF-8 character boundary.
///
/// A non-boundary offset is at most three bytes past the preceding
/// boundary in valid UTF-8, so this walks back at most three steps.
///
/// The rule lives in [`crate::tui::text_geometry`], shared with the composer.
pub fn clamp_offset(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    text_geometry::clamp_offset(snapshot, offset)
}

/// Next character boundary strictly after `offset`, or the end of the
/// document. Returns `None` only when `offset` is already the end.
pub fn next_char_boundary(snapshot: &DocumentSnapshot, offset: usize) -> Option<usize> {
    text_geometry::next_char_boundary(snapshot, offset)
}

/// Previous character boundary strictly before `offset`.
pub fn prev_char_boundary(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    text_geometry::prev_char_boundary(snapshot, offset)
}

/// Zero-based line containing `offset`, clamped to a valid line.
pub fn cursor_line(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    text_geometry::cursor_line(snapshot, offset)
}

/// Byte offset where `line` starts.
pub fn line_start(snapshot: &DocumentSnapshot, line: usize) -> Option<usize> {
    text_geometry::line_start(snapshot, line)
}

/// Byte length of `line` including its terminator, without reading content.
///
/// Cheap: `line_range` alone, so the renderer can decide the bounded path
/// before examining a single content byte.
pub fn line_byte_len(snapshot: &DocumentSnapshot, line: usize) -> Option<usize> {
    snapshot
        .line_range(line)
        .ok()
        .map(|range| range.end.saturating_sub(range.start))
}

/// Whether `line` exceeds the per-line render budget and will be shown
/// truncated. Cheap: length only, no content read.
pub fn line_is_truncated(snapshot: &DocumentSnapshot, line: usize) -> bool {
    line_byte_len(snapshot, line).is_some_and(|len| len > MAX_RENDER_LINE_BYTES)
}

/// Walk `offset` back onto a UTF-8 character boundary (at most three steps
/// in valid UTF-8). Used to clip a budget window without splitting a char.
fn boundary_back(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut candidate = offset.min(snapshot.len_bytes());
    while candidate > 0 && !snapshot.is_byte_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// Exclusive end of the bounded prefix of `start..end`: at most
/// `MAX_RENDER_LINE_BYTES` past `start`, adjusted back to a character
/// boundary so the caller can read it directly.
pub fn bounded_prefix_end(snapshot: &DocumentSnapshot, start: usize, end: usize) -> usize {
    let capped = start.saturating_add(MAX_RENDER_LINE_BYTES).min(end);
    if capped >= end {
        return end;
    }
    boundary_back(snapshot, capped).max(start)
}

/// Byte offset of the last content byte on `line`, excluding the line
/// terminator. Equals the line start for an empty line.
///
/// C002: inspects at most the last two bytes rather than reading the whole
/// line, so a multi-megabyte line costs a constant here.
pub fn line_content_end(snapshot: &DocumentSnapshot, line: usize) -> Option<usize> {
    text_geometry::line_content_end(snapshot, line)
}

/// Byte offset of the end of the last line, excluding any terminator.
pub fn buffer_content_end(snapshot: &DocumentSnapshot) -> usize {
    text_geometry::buffer_content_end(snapshot)
}

/// Zero-based index of the document's last line.
///
/// A document always has at least one line, so this never underflows; it is
/// the single answer to "where does the file end" that paging and viewport
/// arithmetic share.
pub fn last_line_of(snapshot: &DocumentSnapshot) -> usize {
    snapshot.len_lines().saturating_sub(1)
}

/// Start of the next word at or after `offset`.
///
/// The rule lives in [`crate::tui::text_geometry`], shared with the composer,
/// so both surfaces agree on where a word starts.
pub fn word_forward(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    text_geometry::word_forward(snapshot, offset)
}

/// Start of the previous word strictly before `offset`.
pub fn word_backward(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    text_geometry::word_backward(snapshot, offset)
}

/// Byte offset of the last byte of the word at or after `offset`.
///
/// Helix `e`, and the terminal panel's forward word jump, so both surfaces
/// agree on where "end of word" is. When the cursor is not already on word
/// bytes it first steps to the next word, so `e` from whitespace lands on a
/// word rather than standing still.
///
/// The byte-at-a-time scan is deliberate: `is_word_byte` treats any byte
/// `>= 0x80` as word content, so walking `offset + 1` can cross the interior
/// of a multi-byte character and still classify it correctly, exactly as
/// [`word_forward`] does.
///
/// The rule lives in [`crate::tui::text_geometry`], shared with the composer.
pub fn word_end(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    text_geometry::word_end(snapshot, offset)
}

/// Move `offset` by `delta` lines, preserving the display column as closely
/// as the target line allows.
pub fn move_vertical(snapshot: &DocumentSnapshot, offset: usize, delta: isize) -> usize {
    text_geometry::move_vertical(snapshot, offset, delta)
}

/// A position in the document as a Helix selection point.
///
/// The editor's counterpart to the terminal's viewport cell: a byte offset
/// paired with the line it sits on, because [`SelectionPoint`] separates the
/// line from the point so whole-line selection can answer "which lines does
/// this span" without knowing how wide a line is. The line is redundant with
/// the offset for every *motion*, but it is not derivable at selection time —
/// `x` growth walks to the next line's content start, and resolving that
/// needs the line anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EditorPoint {
    /// UTF-8 byte offset into the local replica.
    pub byte: usize,
    /// Zero-based line containing `byte`.
    pub line: usize,
}

impl crate::tui::helix_selection::SelectionPoint for EditorPoint {
    type Line = usize;

    fn line(&self) -> usize {
        self.line
    }
}

/// Resolve a byte offset into a [`SelectionPoint`].
///
/// Clamped and boundary-aligned first, so a stale offset recorded against
/// older text can never produce a point the renderer would slice
/// mid-codepoint.
pub fn editor_point(snapshot: &DocumentSnapshot, byte: usize) -> EditorPoint {
    let byte = clamp_offset(snapshot, byte);
    EditorPoint {
        byte,
        line: cursor_line(snapshot, byte),
    }
}

/// One Helix selection verb, resolved against a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelixVerb {
    /// `v` — toggle character selection at the cursor.
    SelectChar,
    /// `x` — select the cursor's line, or grow the held line selection by one.
    SelectLine,
    /// `d` — delete the held selection. Claims the key even with nothing
    /// selected, because an unclaimed `d` escapes to the global `OpenDiff`
    /// binding.
    DeleteSelection,
    /// `y` — yank the held selection to the system clipboard.
    YankSelection,
    /// Any deliberate motion: extends a character selection, leaves a line
    /// selection alone, and disarms line growth.
    Moved(EditorCommand),
}

/// The held selection resolved to a byte range.
///
/// Character mode is **inclusive of the head character**, matching
/// [`helix_selection::SelectionMode::Char`] and the terminal: `v` then `l`
/// selects two characters, not one-and-a-bit. Line mode spans whole lines
/// including their terminators, so deleting the selection removes the lines
/// rather than leaving blank ones behind.
///
/// Both endpoints are clamped to the document and to character boundaries, so
/// a stale selection recorded against older text can never panic or slice
/// mid-codepoint during rendering.
pub fn selection_range(
    presentation: &TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
) -> Option<std::ops::Range<usize>> {
    let selection = presentation.selection.as_ref()?;
    let (first, last) = selection.bounds();
    let (start, end) = match selection.mode {
        crate::tui::helix_selection::SelectionMode::Char => {
            let start = clamp_offset(snapshot, first.byte);
            let end = next_char_boundary(snapshot, last.byte).unwrap_or(last.byte);
            (start, end)
        }
        crate::tui::helix_selection::SelectionMode::Line => {
            let last_line = snapshot.len_lines().saturating_sub(1);
            let first_line = first.line.min(last_line);
            let final_line = last.line.min(last_line);
            let start = line_start(snapshot, first_line).unwrap_or(0);
            // `final_line + 1` is the start of the following line, which for
            // the last line is the end of the buffer. That is what makes a
            // whole-line delete leave no trailing blank line.
            let end = line_start(snapshot, final_line + 1)
                .unwrap_or_else(|| snapshot.len_bytes())
                .min(snapshot.len_bytes());
            (start, end)
        }
    };
    (start < end).then_some(start..end)
}

/// Apply one Helix selection verb.
///
/// The **rules** live in [`crate::tui::helix_selection`]; this owns the
/// geometry only, which is what the shared module deliberately does not know:
/// which byte starts the next line, and which offset a motion resolved to.
/// Restoring the state machine per verb rather than re-deriving `v`/`x` here
/// is what keeps the editor and `/terminal` agreeing about what `x` does.
pub fn apply_helix_verb(
    snapshot: &DocumentSnapshot,
    presentation: &mut TuiDocumentPresentation,
    verb: HelixVerb,
) -> HelixVerbOutcome {
    use crate::tui::helix_selection::SelectionState;
    let mut state = SelectionState::restore(
        editor_point(snapshot, presentation.cursor_byte),
        presentation.selection,
    );
    let mut outcome = HelixVerbOutcome::Handled;
    let mut scrolled_to = presentation.viewport_line;
    // Only a find motion writes a record; `None` means this verb moved the
    // cursor and deliberately left the repeatable-motion slot alone.
    let mut find_record: Option<(Option<FindMotion>, FindNote)> = None;
    match verb {
        HelixVerb::SelectChar => state.toggle_char(),
        HelixVerb::SelectLine => {
            // `x` is an action, not a mode: the second press grows the
            // selection by exactly one line, and only while nothing has moved
            // since. That condition belongs to the shared machine because it
            // is the same one the terminal uses.
            if state.line_select_extends() {
                if let Some(selection) = state.selection() {
                    let next = next_line_content_start(snapshot, selection.head.line);
                    state.grow_to(editor_point(snapshot, next));
                }
            } else {
                state.select_line();
            }
        }
        // Delete and yank are *effects* over a range the machine describes,
        // not transitions of the selection, so they read the range and drop
        // the selection outright. Both still claim the key with nothing held:
        // an unclaimed `d` escapes to the global `OpenDiff` binding, and an
        // unclaimed `y` reads as a dead key.
        HelixVerb::DeleteSelection => {
            return match selection_range(presentation, snapshot) {
                Some(range) => {
                    presentation.selection = None;
                    HelixVerbOutcome::DeleteRange(range)
                }
                None => HelixVerbOutcome::Handled,
            };
        }
        HelixVerb::YankSelection => {
            return match selection_range(presentation, snapshot) {
                Some(range) => {
                    presentation.selection = None;
                    HelixVerbOutcome::YankRange(range)
                }
                None => HelixVerbOutcome::Handled,
            };
        }
        HelixVerb::Moved(command) => {
            // Resolved against a throwaway presentation so the real one is
            // written exactly once, below, through the shared machine — which
            // is what decides whether this move extends a character selection
            // or only disarms line growth.
            //
            // The find record and its disclosure ride along with the cursor
            // and the scroll for the same reason: a find motion is the one
            // motion that also writes state about the *attempt*, and it
            // resolves on the probe. Both are seeded from the real
            // presentation first so a non-find motion — which writes neither
            // — cannot erase a record the dispatcher deliberately kept.
            let mut probe = TuiDocumentPresentation {
                viewport_line: presentation.viewport_line,
                cursor_byte: presentation.cursor_byte,
                last_find: presentation.last_find,
                find_note: presentation.find_note,
                ..TuiDocumentPresentation::default()
            };
            let next = apply_motion(&mut probe, snapshot, command, NOMINAL_HELIX_PAGES);
            // The scroll has to come back with the cursor: it was resolved on
            // the probe, and dropping it is what made a second `Ctrl+d` land on
            // the same line as the first.
            scrolled_to = probe.viewport_line;
            state.move_to(editor_point(snapshot, next));
            outcome = HelixVerbOutcome::Moved(next);
            find_record = Some((probe.last_find, probe.find_note));
        }
    }
    presentation.cursor_byte = state.cursor().byte;
    presentation.selection = state.selection();
    presentation.viewport_line = scrolled_to;
    if let Some((last_find, find_note)) = find_record {
        presentation.last_find = last_find;
        presentation.find_note = find_note;
    }
    outcome
}

/// Nominal viewport height used to resolve a motion driven by a Helix verb.
///
/// The widget resolves the exact window at render time; a motion only needs
/// *a* height to turn a page into a line count, and the viewport follows the
/// cursor on the next frame.
const NOMINAL_HELIX_PAGES: usize = 20;

/// Byte offset of the first content byte on the line after `line`.
///
/// Clamped to the end of the buffer, so `x` on the last line grows into the
/// terminator rather than past the document.
fn next_line_content_start(snapshot: &DocumentSnapshot, line: usize) -> usize {
    let last = snapshot.len_lines().saturating_sub(1);
    line_start(snapshot, (line + 1).min(last))
        .unwrap_or_else(|| line_content_end(snapshot, last).unwrap_or_else(|| snapshot.len_bytes()))
}

/// What a Helix verb asked the caller to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelixVerbOutcome {
    /// Pure navigation or selection; presentation state was updated.
    Handled,
    /// The cursor moved to this byte offset.
    Moved(usize),
    /// Delete or yank this range, then clear the selection.
    DeleteRange(std::ops::Range<usize>),
    /// Copy this range to the clipboard, then clear the selection.
    YankRange(std::ops::Range<usize>),
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

/// Resolve one M003 applicable completion edit against the current snapshot.
///
/// Validates the DTO's UTF-16 range against `snapshot` and builds the single
/// forward/inverse pair through the shared [`build_edit`], so an accepted
/// completion is exactly one controller edit with one undo unit — the same
/// shape as a typed edit. Returns `None` for out-of-snapshot ranges,
/// reversed ranges, oversized replacement text, and no-op edits: each is a
/// refusal the caller reports rather than applying something the server did
/// not mean. Pure deletions (range plus empty text) are meaningful and kept.
pub fn resolve_completion_edit(
    snapshot: &DocumentSnapshot,
    edit: &codegg_protocol::lsp::LspCompletionEditDto,
    stay_insert: bool,
) -> Option<EditorEdit> {
    if edit.new_text.len() > codegg_protocol::lsp::MAX_LSP_COMPLETION_EDIT_BYTES {
        return None;
    }
    let start_line = usize::try_from(edit.start_line).ok()?;
    let end_line = usize::try_from(edit.end_line).ok()?;
    let start_row = line_start(snapshot, start_line)?;
    let start_row_end = line_content_end(snapshot, start_line)?;
    let end_row = line_start(snapshot, end_line)?;
    let end_row_end = line_content_end(snapshot, end_line)?;
    let start_text = snapshot.read_bytes(start_row, start_row_end).ok()?;
    let end_text = snapshot.read_bytes(end_row, end_row_end).ok()?;
    let start_col =
        crate::tui::editor_lsp::utf16_column_to_byte(&start_text, edit.start_character)?;
    let end_col = crate::tui::editor_lsp::utf16_column_to_byte(&end_text, edit.end_character)?;
    let start = start_row + start_col;
    let end = end_row + end_col;
    if end < start {
        return None;
    }
    if start == end && edit.new_text.is_empty() {
        return None;
    }
    let transaction = TextTransaction::new(vec![TextEdit::new(start..end, edit.new_text.clone())]);
    build_edit(
        snapshot,
        transaction,
        start + edit.new_text.len(),
        stay_insert,
    )
}

// ── Resolution against a presentation record ────────────────────────────────

/// Apply a motion command to the presentation. Never touches text.
///
/// Deliberate movement that arrived through the Helix verbs goes through
/// [`apply_helix_verb`], which owns the selection rules. This is the
/// *raw* motion: it always drops a held selection, so an Insert-mode arrow or
/// an LSP jump leaves nothing highlighted rather than dragging an old anchor
/// onto wherever the cursor landed.
///
/// A find motion is the one command resolved here rather than in
/// [`apply_motion_inner`], because it is the only motion that has an
/// *outcome* to disclose: the bounded scan can stop short, and that has to
/// reach the status row whether or not the cursor moved. It is recorded
/// here, next to the resolution that produced it, rather than in the
/// dispatch layer that pressed the key — otherwise the note and the move
/// would be two facts about the same keystroke that can disagree.
pub fn apply_motion(
    presentation: &mut TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    command: EditorCommand,
    visible_lines: usize,
) -> usize {
    let from = clamp_offset(snapshot, presentation.cursor_byte);
    let find = command
        .find_motion()
        .map(|motion| (motion, motion.outcome(snapshot, from)));
    let resolved = match find {
        Some((_, FindOutcome::Found(offset))) => offset,
        // A find that found nothing leaves the cursor exactly where it was,
        // and `apply_motion_inner`'s catch-all would do the same — but only
        // by accident, and it would leave the disclosure unwritten.
        Some((_, FindOutcome::Absent | FindOutcome::Bounded)) => from,
        None => apply_motion_inner(presentation, snapshot, command, visible_lines),
    };
    presentation.cursor_byte = resolved;
    presentation.viewport_line =
        viewport_after_motion(presentation, snapshot, command, resolved, visible_lines);
    if let Some((motion, outcome)) = find {
        // Recorded whether or not it landed: Helix's repeat slot is set by
        // running the command, so `f z` with no `z` ahead still leaves `Alt-.`
        // armed to try `f z` again from wherever the cursor is.
        presentation.last_find = Some(motion);
        presentation.find_note = FindNote::from_outcome(outcome, motion.target);
    }
    resolved
}

/// The geometry half of [`apply_motion`]: resolve the offset a motion lands
/// on, without writing anything. Split out so the Helix driver can resolve a
/// motion against a throwaway presentation and then commit the result exactly
/// once through the shared selection state machine.
fn apply_motion_inner(
    presentation: &TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    command: EditorCommand,
    visible_lines: usize,
) -> usize {
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
        EditorCommand::WordEnd => word_end(snapshot, cursor),
        EditorCommand::HalfPageDown => {
            // A page that would land past the end continues to the end
            // instead of stopping short. Half a window from the last page
            // is a very ordinary place to be — and a motion that quietly
            // refuses there reads as a dropped keypress rather than as the
            // end of the document.
            let target = half_page_target(presentation.viewport_line, visible_lines, true);
            if target > last_line_of(snapshot) {
                buffer_content_end(snapshot)
            } else {
                let column = cursor.saturating_sub(
                    line_start(snapshot, cursor_line(snapshot, cursor)).unwrap_or(0),
                );
                offset_on_line(snapshot, target, column)
            }
        }
        EditorCommand::HalfPageUp => {
            let target = half_page_target(presentation.viewport_line, visible_lines, false);
            let column = cursor
                .saturating_sub(line_start(snapshot, cursor_line(snapshot, cursor)).unwrap_or(0));
            offset_on_line(snapshot, target, column)
        }
        // A command this surface does not own (`DeleteChar`, `DeleteWord`, …)
        // is an *editing* intent, not a motion. Returning the cursor
        // unchanged keeps it out of `resolve_edit`, which has its own arm for
        // it.
        _ => return clamp_offset(snapshot, cursor),
    };
    clamp_offset(snapshot, next)
}

/// Where `viewport_line` ends up after a motion.
///
/// Two different questions, deliberately split:
///
/// - A **page** (`Ctrl+d`/`Ctrl+u`) advances the view by half a window each
///   press, exactly as Helix does, and the cursor lands relative to the *new*
///   viewport. Merely keeping the cursor visible is not enough here: after the
///   first `Ctrl+d` the cursor is still inside the window, so the view would
///   not move and a second press would resolve against the same anchor and
///   visibly do nothing.
/// - Every **other** motion scrolls the minimum amount needed to reveal the
///   cursor, so `j` does not reframe the document under the reader.
///
/// The widget still resolves the exact window at render time; this keeps the
/// motion arithmetic independent of render cadence, which is what made
/// paging behave differently depending on whether a frame happened to land
/// between two presses.
fn viewport_after_motion(
    presentation: &TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    command: EditorCommand,
    cursor: usize,
    visible_lines: usize,
) -> usize {
    let last_line = snapshot.len_lines().saturating_sub(1);
    let window = visible_lines.max(1);
    let viewport = match command {
        EditorCommand::HalfPageDown => presentation.viewport_line + (window / 2).max(1),
        EditorCommand::HalfPageUp => presentation
            .viewport_line
            .saturating_sub((window / 2).max(1)),
        _ => presentation.viewport_line,
    }
    .min(last_line);
    reveal_cursor(
        viewport,
        cursor_line(snapshot, clamp_offset(snapshot, cursor)),
        window,
        last_line,
    )
}

/// Scroll the minimum amount needed to bring `target_line` into a window of
/// `window` rows anchored at `viewport`.
///
/// The one reveal rule, shared by motions and by `/` search, so a jump to a
/// match lands the same way a jump to the end of the buffer does.
pub fn reveal_cursor(
    viewport: usize,
    target_line: usize,
    window: usize,
    last_line: usize,
) -> usize {
    if target_line < viewport {
        target_line
    } else if target_line >= viewport + window {
        (target_line + 1 - window).min(last_line)
    } else {
        viewport
    }
}

/// Park the cursor on `offset` and scroll the minimum amount to reveal it.
///
/// Used by anything that moves the cursor *to a place* rather than by a
/// relative step: `/` search, `n`/`N`, and an LSP jump. It shares the reveal
/// rule with the Helix motions so the two cannot disagree about where the
/// view lands, and it drops a held selection rather than dragging the old
/// anchor onto wherever the cursor ended up.
pub fn move_cursor_to(
    presentation: &mut TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    offset: usize,
    visible_lines: usize,
) -> usize {
    let cursor = clamp_offset(snapshot, offset);
    presentation.cursor_byte = cursor;
    presentation.selection = None;
    presentation.viewport_line = reveal_cursor(
        presentation.viewport_line,
        cursor_line(snapshot, cursor),
        visible_lines.max(1),
        last_line_of(snapshot),
    );
    cursor
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
    );
}

/// Push one accepted edit onto the undo history and clear the redo history.
///
/// `cursor_before` is the position observed *before*
/// the forward transaction, so undo restores where the user actually was.
pub fn push_undo(
    presentation: &mut TuiDocumentPresentation,
    forward: TextTransaction,
    inverse: TextTransaction,
    bytes: usize,
    cursor_before: usize,
) {
    presentation.undo.push(EditorUndoEntry {
        forward,
        inverse,
        cursor_before,
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
///
/// Disarms the pending prefix too, which is the same fact in a different
/// register: the gesture the user was halfway through was aimed at text that
/// has just been replaced underneath it. The baseline only disarmed
/// `pending_command` here and left `yank_prefix_pending` armed across a
/// reload, so a `space` typed before a resync yanked in the reloaded
/// buffer.
///
/// The repeatable find and its disclosure go for the same reason and in the
/// same breath: after a reconciliation the character `Alt-.` would look for,
/// and the answer to whether it is there at all, describe text that no
/// longer exists.
pub fn clear_history(presentation: &mut TuiDocumentPresentation) {
    presentation.undo.clear();
    presentation.redo.clear();
    presentation.undo_bytes = 0;
    presentation.redo_bytes = 0;
    presentation.disarm_prefix();
    presentation.forget_find();
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
    // C002: the cursor column is the bounded prefix width when the cursor
    // sits past the render window of a huge line; the byte offset is
    // untouched, only the scroll target saturates.
    let cursor_start = line_start(snapshot, cursor_line).unwrap_or(0);
    let cursor_column = bounded_display_width(snapshot, cursor_start..presentation.cursor_byte).0;
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
///
/// The caller must pass a bounded range on the render path; use
/// [`bounded_display_width`] when the range can come from a pathological
/// line. Exact for ordinary short ranges.
pub fn display_width(snapshot: &DocumentSnapshot, range: std::ops::Range<usize>) -> usize {
    snapshot
        .read_bytes(range.start, range.end)
        .map(|text| {
            use unicode_width::UnicodeWidthStr;
            UnicodeWidthStr::width(text.as_str())
        })
        .unwrap_or(0)
}

/// Display width of `range` capped to the per-line render budget.
///
/// Returns the width of the examined prefix and whether the range was cut.
/// The render path uses this (via [`bounded_display_column`] and
/// [`longest_visible_width`]) so a cursor at the far end of a multi-megabyte
/// line costs 32 KiB, not the whole line.
pub fn bounded_display_width(
    snapshot: &DocumentSnapshot,
    range: std::ops::Range<usize>,
) -> (usize, bool) {
    let end = bounded_prefix_end(snapshot, range.start, range.end);
    let truncated = end < range.end;
    (display_width(snapshot, range.start..end), truncated)
}

/// Longest display width among the visible lines, in terminal columns.
///
/// C002: each line contributes at most its 32 KiB prefix, and measurement
/// stops once the frame total reaches 256 KiB. Lines past the frame budget
/// contribute nothing further; use [`frame_truncated_lines`] for the honest
/// truncation signal shown in the UI.
pub fn longest_visible_width(
    snapshot: &DocumentSnapshot,
    first_line: usize,
    visible_lines: usize,
) -> usize {
    let mut widest = 0usize;
    let mut examined = 0usize;
    for line in first_line..(first_line + visible_lines) {
        if line >= snapshot.len_lines() {
            break;
        }
        if examined >= MAX_FRAME_SOURCE_BYTES {
            break;
        }
        let Some(start) = line_start(snapshot, line) else {
            break;
        };
        let end = line_content_end(snapshot, line).unwrap_or(start);
        let window_end = bounded_prefix_end(snapshot, start, end);
        examined = examined.saturating_add(window_end.saturating_sub(start));
        widest = widest.max(display_width(snapshot, start..window_end));
    }
    widest
}

/// Number of visible lines exceeding the per-line render budget.
///
/// Cheap length check, no content read. The banner shows this so a truncated
/// frame is never presented as complete.
pub fn frame_truncated_lines(
    snapshot: &DocumentSnapshot,
    first_line: usize,
    visible_lines: usize,
) -> usize {
    (first_line..first_line.saturating_add(visible_lines))
        .take_while(|line| *line < snapshot.len_lines())
        .filter(|line| line_is_truncated(snapshot, *line))
        .count()
}

/// Deterministic per-frame input cost for the visible window.
///
/// Counts source bytes the bounded render path would examine (prefix windows
/// only), the largest single `read_bytes` window, how many visible lines are
/// truncated, and how many column-seeking scans run (one per visible line).
/// Tests assert budgets on this rather than on wall-clock time, which shared
/// CI cannot calibrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCost {
    pub total_bytes: usize,
    pub max_line_bytes: usize,
    pub truncated_lines: usize,
    pub column_scans: usize,
}

/// Measure [`FrameCost`] for `visible_lines` starting at `first_line`.
///
/// Mirrors the `render_text` loop exactly: row sizes come from the cheap
/// length check, rows past the 256 KiB frame budget cost zero bytes and
/// count as truncated, and one column-seeking scan runs per rendered row.
pub fn measure_frame_cost(
    snapshot: &DocumentSnapshot,
    first_line: usize,
    visible_lines: usize,
) -> FrameCost {
    let mut total_bytes = 0usize;
    let mut max_line_bytes = 0usize;
    let mut truncated_lines = 0usize;
    let mut column_scans = 0usize;
    let mut remaining = MAX_FRAME_SOURCE_BYTES;
    for line in first_line..first_line.saturating_add(visible_lines) {
        if line >= snapshot.len_lines() {
            break;
        }
        let row_bytes = line_byte_len(snapshot, line)
            .unwrap_or(0)
            .min(MAX_RENDER_LINE_BYTES);
        max_line_bytes = max_line_bytes.max(row_bytes);
        let per_line_truncated = line_is_truncated(snapshot, line);
        if row_bytes > remaining {
            // Frame-capped: the renderer shows a marker without reading.
            truncated_lines += 1;
            continue;
        }
        remaining = remaining.saturating_sub(row_bytes);
        total_bytes = total_bytes.saturating_add(row_bytes);
        column_scans += 1;
        if per_line_truncated {
            truncated_lines += 1;
        }
    }
    FrameCost {
        total_bytes,
        max_line_bytes,
        truncated_lines,
        column_scans,
    }
}

/// Display column of `offset` within its own line.
///
/// C002: saturates at the prefix width when `offset` lies past the 32 KiB
/// render window. The byte offset itself stays exact; only the displayed
/// column is bounded. Pair with [`display_column_truncated`] for the UI
/// signal.
pub fn display_column(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let offset = clamp_offset(snapshot, offset);
    let start = line_start(snapshot, cursor_line(snapshot, offset)).unwrap_or(0);
    bounded_display_width(snapshot, start..offset).0
}

/// Whether [`display_column`] saturated for `offset` (cursor past the render
/// window of a huge line). The banner shows this instead of a bare column.
pub fn display_column_truncated(snapshot: &DocumentSnapshot, offset: usize) -> bool {
    let offset = clamp_offset(snapshot, offset);
    let start = line_start(snapshot, cursor_line(snapshot, offset)).unwrap_or(0);
    bounded_display_width(snapshot, start..offset).1
}

/// Byte offset for a display column on `line`, clamped to the line's content
/// end so a click or motion can never land past the terminator.
///
/// C002: the scan examines at most 32 KiB. A column past the prefix end
/// resolves to the prefix end — horizontal motion on a huge line is limited
/// to its rendered prefix, which the UI marks with [`TRUNCATED_LINE_SUFFIX`].
/// Normal lines are unaffected.
pub fn offset_at_column(snapshot: &DocumentSnapshot, line: usize, column: usize) -> Option<usize> {
    let start = line_start(snapshot, line)?;
    let end = line_content_end(snapshot, line)?;
    let window_end = bounded_prefix_end(snapshot, start, end);
    let mut offset = start;
    let mut consumed = 0usize;
    while offset < window_end {
        // `next_char_boundary` is `None` at the end of the document, which
        // is a legitimate position here, not a failure.
        let next = next_char_boundary(snapshot, offset)
            .unwrap_or(window_end)
            .min(window_end);
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
    Some(clamp_offset(snapshot, offset.min(window_end)))
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
    use crate::tui::helix_selection::{Selection, SelectionMode};
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

    /// Park the cursor on `offset`, as the `/` search handler does between
    /// two `step_search` calls.
    fn editor_move(
        presentation: &mut TuiDocumentPresentation,
        snapshot: &DocumentSnapshot,
        offset: usize,
    ) {
        move_cursor_to(presentation, snapshot, offset, 10);
    }

    // ── `Ctrl+d` continues to the end ───────────────────────────────────

    #[test]
    fn half_page_down_continues_to_the_last_line() {
        // 30 lines, a 10-row window. From viewport 20 a half page would land
        // on 25 — still inside — so this first press pages normally.
        let text = (0..30).map(|i| format!("line{i}\n")).collect::<String>();
        let snapshot = snapshot_of(&text);
        let mut presentation = presentation_at(0);
        presentation.viewport_line = 20;
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::HalfPageDown,
            10,
        );
        assert_eq!(cursor_line(&snapshot, presentation.cursor_byte), 25);

        // From 25 the next half page would land on 30, which is past the last
        // line. It must continue to the end instead of refusing there.
        presentation.viewport_line = 25;
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::HalfPageDown,
            10,
        );
        assert_eq!(
            cursor_line(&snapshot, presentation.cursor_byte),
            last_line_of(&snapshot),
            "a half page that would overshoot must continue to the last line"
        );
    }

    #[test]
    fn half_page_down_already_at_the_end_stays_at_the_end() {
        let text = (0..12).map(|i| format!("line{i}\n")).collect::<String>();
        let snapshot = snapshot_of(&text);
        let mut presentation = presentation_at(buffer_content_end(&snapshot));
        presentation.viewport_line = last_line_of(&snapshot);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::HalfPageDown,
            10,
        );
        assert_eq!(
            cursor_line(&snapshot, presentation.cursor_byte),
            last_line_of(&snapshot)
        );
    }

    // ── `/` search ─────────────────────────────────────────────────────

    #[test]
    fn find_matches_returns_every_offset_in_order() {
        let snapshot = snapshot_of("let host_status = 1;\nfn host_status() {}\n");
        let outcome = find_matches(&snapshot, "host_status");
        assert_eq!(outcome.matches.len(), 2);
        assert!(outcome.matches[0] < outcome.matches[1]);
        assert!(!outcome.truncated && !outcome.bounded);
        // Every offset is a cursor position, so every one must be a char
        // boundary — a non-boundary offset would panic the renderer.
        for offset in outcome.matches {
            assert!(snapshot.is_byte_boundary(offset));
        }
    }

    #[test]
    fn an_empty_needle_matches_nothing() {
        let snapshot = snapshot_of("alpha\n");
        let outcome = find_matches(&snapshot, "");
        assert!(outcome.matches.is_empty());
        assert!(!outcome.truncated && !outcome.bounded);
    }

    #[test]
    fn a_search_that_finds_nothing_leaves_the_cursor_alone() {
        let snapshot = snapshot_of("alpha\nbeta\n");
        let mut presentation = presentation_at(7);
        let outcome = find_matches(&snapshot, "zzz");
        let landed = presentation.adopt_search(outcome, presentation.cursor_byte);
        assert_eq!(landed, None, "a failed search must not move the cursor");
        assert_eq!(presentation.cursor_byte, 7);
    }

    #[test]
    fn search_lands_on_the_match_the_cursor_is_already_on() {
        let snapshot = snapshot_of("aaa bbb aaa\n");
        let mut presentation = presentation_at(0);
        let outcome = find_matches(&snapshot, "aaa");
        // The cursor sits on the first `aaa`. A live search must leave it
        // there: a strict `>` would hop to the second match on the very first
        // keystroke, so refining a needle you are looking at would run away
        // from you.
        assert_eq!(
            presentation.adopt_search(outcome, presentation.cursor_byte),
            Some(0)
        );
    }

    #[test]
    fn stepping_walks_matches_and_wraps_at_both_ends() {
        let snapshot = snapshot_of("aaa bbb aaa\n");
        let mut presentation = presentation_at(0);
        let outcome = find_matches(&snapshot, "aaa");
        presentation.adopt_search(outcome, 0);
        let landed = |presentation: &mut TuiDocumentPresentation, forward: bool| {
            let offset = presentation.step_search(forward).expect("two matches");
            editor_move(presentation, &snapshot, offset);
            offset
        };
        // `n` must move *off* the match the cursor is on, not re-report it.
        assert_eq!(landed(&mut presentation, true), 8);
        // And from the last match it wraps rather than stalling.
        assert_eq!(landed(&mut presentation, true), 0);
        // `N` walks the other way and wraps at the top.
        assert_eq!(landed(&mut presentation, false), 8);
    }

    #[test]
    fn search_stepping_with_no_matches_reports_nothing_to_do() {
        let mut presentation = presentation_at(0);
        presentation.cancel_search();
        assert_eq!(presentation.step_search(true), None);
        assert_eq!(presentation.step_search(false), None);
        assert!(!presentation.has_search());
    }

    #[test]
    fn the_search_line_is_bounded_at_the_keystroke() {
        let mut presentation = presentation_at(0);
        presentation.open_search_line("");
        for _ in 0..(MAX_EDITOR_SEARCH_LINE + 50) {
            presentation.push_search_char('a');
        }
        assert_eq!(
            presentation.search_line.chars().count(),
            MAX_EDITOR_SEARCH_LINE,
            "the needle is a query, not a buffer; it must not grow without bound"
        );
        // And a refused keystroke leaves the line exactly as it was.
        presentation.push_search_char('b');
        assert_eq!(
            presentation.search_line.chars().count(),
            MAX_EDITOR_SEARCH_LINE
        );
        assert!(presentation.search_line.ends_with('a'));
    }

    #[test]
    fn accepting_a_search_keeps_the_highlight_and_cancelling_drops_it() {
        let snapshot = snapshot_of("aaa bbb aaa\n");
        let mut presentation = presentation_at(0);
        presentation.open_search_line("aaa");
        let outcome = find_matches(&snapshot, "aaa");
        presentation.adopt_search(outcome, 0);
        presentation.close_search_line();
        assert!(!presentation.search_active, "Enter closes the line");
        assert!(presentation.has_search(), "and keeps the matches for n/N");

        presentation.open_search_line("aaa");
        presentation.cancel_search();
        assert!(!presentation.search_active);
        assert!(!presentation.has_search(), "Esc disowns the highlight too");
    }

    #[test]
    fn an_empty_search_confirms_to_nothing() {
        let mut presentation = presentation_at(0);
        presentation.open_search_line("");
        presentation.close_search_line();
        // Enter on an empty needle is a search for nothing, so there is
        // nothing to leave painted.
        assert!(!presentation.has_search());
    }

    #[test]
    fn word_at_seeds_the_search_line_from_the_cursor() {
        let snapshot = snapshot_of("let host_status = 1;\n");
        assert_eq!(
            word_at(&snapshot, 6),
            Some("host_status".to_string()),
            "`/` on a word should start from that word"
        );
        // On whitespace there is no word, so the line opens empty rather than
        // searching for a space.
        assert_eq!(word_at(&snapshot, 3), None);
    }

    #[test]
    fn word_at_handles_multibyte_text_without_slicing_a_character() {
        let snapshot = snapshot_of("let café = 1;\n");
        // Byte 8 is the start of `é`; the seed must be the whole word.
        assert_eq!(word_at(&snapshot, 8), Some("café".to_string()));
        // A mid-character offset is refused rather than sliced — callers
        // clamp to boundaries, so reaching one means something is wrong
        // upstream and it must not be papered over with a panic.
        assert_eq!(word_at(&snapshot, 9), None);
    }

    #[test]
    fn move_cursor_to_reveals_the_target_and_drops_a_held_selection() {
        let text = (0..40).map(|i| format!("line{i}\n")).collect::<String>();
        let snapshot = snapshot_of(&text);
        let mut presentation = presentation_at(0);
        presentation.selection = Some(Selection {
            anchor: EditorPoint { byte: 0, line: 0 },
            head: EditorPoint { byte: 3, line: 0 },
            mode: SelectionMode::Char,
            growing: false,
        });
        let end = line_start(&snapshot, 30).expect("line 30 starts somewhere");
        move_cursor_to(&mut presentation, &snapshot, end, 10);
        assert_eq!(presentation.cursor_byte, end);
        assert_eq!(cursor_line(&snapshot, presentation.cursor_byte), 30);
        assert!(
            presentation.selection.is_none(),
            "a jump must not drag a stale selection anchor along with it"
        );
        // Revealed, so the target is inside the window the viewport shows.
        assert!(30 >= presentation.viewport_line && 30 < presentation.viewport_line + 10);
    }

    // ── Completion edit resolution ────────────────────────────────────

    fn completion_edit(
        start: (u32, u32),
        end: (u32, u32),
        new_text: &str,
    ) -> codegg_protocol::lsp::LspCompletionEditDto {
        codegg_protocol::lsp::LspCompletionEditDto {
            start_line: start.0,
            start_character: start.1,
            end_line: end.0,
            end_character: end.1,
            new_text: new_text.to_string(),
        }
    }

    #[test]
    fn a_completion_replace_resolves_to_one_forward_and_inverse() {
        // Line 0 is 12 bytes, so "    prin" is bytes 12..20 and the word
        // itself is 16..20: the assertion is on bytes, not UTF-16 units.
        let snapshot = snapshot_of("fn main() {\n    prin\n}\n");
        let edit = completion_edit((1, 4), (1, 8), "println!");
        let resolved = resolve_completion_edit(&snapshot, &edit, true).expect("resolves");
        assert_eq!(resolved.transaction.edits.len(), 1);
        assert_eq!(
            resolved.transaction.edits[0].range.start, 16,
            "byte range, not UTF-16"
        );
        assert_eq!(resolved.transaction.edits[0].range.end, 20);
        assert_eq!(resolved.transaction.edits[0].insert, "println!");
        assert_eq!(resolved.inverse.edits.len(), 1);
        assert_eq!(resolved.inverse.edits[0].insert, "prin");
        assert_eq!(resolved.cursor_after, 16 + "println!".len());
        assert!(resolved.stay_insert);
    }

    #[test]
    fn a_completion_insert_at_cursor_resolves_to_a_zero_width_replace() {
        let snapshot = snapshot_of("fn main() {\n    prin\n}\n");
        let edit = completion_edit((1, 8), (1, 8), "tln!");
        let resolved = resolve_completion_edit(&snapshot, &edit, false).expect("resolves");
        assert_eq!(resolved.transaction.edits[0].range.start, 20);
        assert_eq!(resolved.transaction.edits[0].range.end, 20);
        assert!(!resolved.stay_insert);
    }

    #[test]
    fn completion_ranges_validate_utf16_exactly() {
        // "a😀b": the emoji is 2 UTF-16 units; column 2 splits the pair.
        let snapshot = snapshot_of("a😀b\n");
        let split = completion_edit((0, 0), (0, 2), "x");
        assert!(
            resolve_completion_edit(&snapshot, &split, false).is_none(),
            "a split surrogate must refuse"
        );
        let whole = completion_edit((0, 0), (0, 3), "x");
        assert!(
            resolve_completion_edit(&snapshot, &whole, false).is_some(),
            "exact surrogate span resolves"
        );
        // "a😀b" is 4 UTF-16 units, so 4 is the valid line end and 5 is
        // past it.
        let past_end = completion_edit((0, 0), (0, 5), "x");
        assert!(
            resolve_completion_edit(&snapshot, &past_end, false).is_none(),
            "past-the-end refuses"
        );
        let reversed = completion_edit((0, 3), (0, 0), "x");
        assert!(
            resolve_completion_edit(&snapshot, &reversed, false).is_none(),
            "reversed ranges refuse"
        );
    }

    #[test]
    fn completion_refuses_absent_lines_noop_edits_and_oversized_text() {
        let snapshot = snapshot_of("ab\n");
        let absent = completion_edit((9, 0), (9, 1), "x");
        assert!(resolve_completion_edit(&snapshot, &absent, false).is_none());
        let noop = completion_edit((0, 1), (0, 1), "");
        assert!(
            resolve_completion_edit(&snapshot, &noop, false).is_none(),
            "a no-op is not an edit"
        );
        let deletion = completion_edit((0, 0), (0, 1), "");
        let resolved = resolve_completion_edit(&snapshot, &deletion, false).expect("deletes");
        assert_eq!(resolved.transaction.edits[0].insert, "");
        assert_eq!(resolved.inverse.edits[0].insert, "a");
        let big = "x".repeat(codegg_protocol::lsp::MAX_LSP_COMPLETION_EDIT_BYTES + 1);
        let oversized = completion_edit((0, 0), (0, 1), &big);
        assert!(
            resolve_completion_edit(&snapshot, &oversized, false).is_none(),
            "oversized edits refuse even when the DTO claims otherwise"
        );
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
    fn a_char_selection_reads_back_as_an_ordered_inclusive_range() {
        let snapshot = snapshot_of("abcdef");
        let mut presentation = presentation_at(2);
        // A backwards anchor is the point: `v` then `h` leaves the head left
        // of the anchor, and the resolved range must still read left-to-right.
        let (anchor, head) = (4usize, 2usize);
        presentation.selection = Some(Selection {
            anchor: editor_point(&snapshot, anchor),
            head: editor_point(&snapshot, head),
            mode: SelectionMode::Char,
            growing: false,
        });
        // Helix selections are inclusive at both ends, so `v` at `d` then `h`
        // back to `c` covers `c`, `d`, and `e`: bytes 2..5.
        assert_eq!(selection_range(&presentation, &snapshot), Some(2..5));
    }

    #[test]
    fn a_line_selection_spans_whole_lines_including_their_terminators() {
        let snapshot = snapshot_of("one\ntwo\nthree\n");
        let mut presentation = presentation_at(4);
        // Byte 7 is the newline that *ends* line 1; line 2 starts at byte 8.
        // Anchoring on the terminator is the whole hazard here — resolve it
        // as line 1 and the "span two lines" assertion would pass on a
        // one-line selection.
        assert_eq!(editor_point(&snapshot, 7).line, 1);
        assert_eq!(editor_point(&snapshot, 8).line, 2);
        presentation.selection = Some(Selection {
            anchor: editor_point(&snapshot, 4),
            head: editor_point(&snapshot, 8),
            mode: SelectionMode::Line,
            growing: true,
        });
        // Lines 1..=2, i.e. "two\nthree\n". "one\ntwo\nthree\n" is 14
        // bytes and line_start(3) is the end of the buffer, so the span runs
        // to 14 — deleting it must not leave the trailing blank line that a
        // content-only span would.
        assert_eq!(selection_range(&presentation, &snapshot), Some(4..14));
    }

    #[test]
    fn selecting_a_line_then_pressing_x_grows_it_by_exactly_one_line() {
        let snapshot = snapshot_of("one\ntwo\nthree\nfour\n");
        let mut presentation = presentation_at(1);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectLine);
        assert_eq!(presentation.selection.unwrap().line_range(), (0, 0));

        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectLine);
        assert_eq!(presentation.selection.unwrap().line_range(), (0, 1));

        // Any deliberate movement disarms growth, so the next `x` re-anchors
        // instead of swallowing everything walked past.
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::Moved(EditorCommand::MoveDown),
        );
        assert_eq!(presentation.selection.unwrap().line_range(), (0, 1));
        // The cursor is on line 1 now, so the next `x` re-anchors there
        // rather than swallowing everything walked past.
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectLine);
        assert_eq!(presentation.selection.unwrap().line_range(), (1, 1));
    }

    #[test]
    fn movement_extends_a_char_selection_but_not_a_line_selection() {
        let snapshot = snapshot_of("one\ntwo\nthree\n");
        let mut presentation = presentation_at(1);

        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectChar);
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::Moved(EditorCommand::MoveRight),
        );
        let (first, last) = presentation.selection.unwrap().bounds();
        assert_eq!((first.byte, last.byte), (1, 2));

        // Same movement, but the selection was anchored by `x`.
        let mut line_selection = presentation_at(1);
        apply_helix_verb(&snapshot, &mut line_selection, HelixVerb::SelectLine);
        apply_helix_verb(
            &snapshot,
            &mut line_selection,
            HelixVerb::Moved(EditorCommand::MoveDown),
        );
        assert_eq!(
            line_selection.selection.unwrap().line_range(),
            (0, 0),
            "a bare movement must not add lines to a line selection"
        );
    }

    #[test]
    fn deleting_with_nothing_selected_is_a_claimed_no_op() {
        let snapshot = snapshot_of("one\ntwo\n");
        let mut presentation = presentation_at(1);
        assert_eq!(
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::DeleteSelection),
            HelixVerbOutcome::Handled
        );
        assert_eq!(presentation.cursor_byte, 1);
    }

    #[test]
    fn deleting_a_line_selection_returns_the_whole_line_range() {
        let snapshot = snapshot_of("one\ntwo\nthree\n");
        let mut presentation = presentation_at(4);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectLine);
        assert_eq!(
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::DeleteSelection),
            HelixVerbOutcome::DeleteRange(4..8)
        );
        assert!(presentation.selection.is_none());
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
    fn clear_history_drops_undo_redo_and_the_pending_prefix() {
        let snapshot = snapshot_of("hello");
        let mut presentation = presentation_at(0);
        record_undo(
            &mut presentation,
            &insert_text(&snapshot, 0, "abc").unwrap(),
        );
        presentation.pending_prefix = EditorPrefix::Goto;
        // A reconciliation replaces the text, so a recorded find aimed at the
        // old bytes — and the disclosure about whether the target is there —
        // are both statements about text that no longer exists.
        presentation.last_find = Some(FindMotion {
            kind: FindKind::Next,
            target: 'h',
        });
        presentation.find_note = FindNote::Absent('h');
        clear_history(&mut presentation);
        assert!(presentation.undo.is_empty());
        assert!(presentation.redo.is_empty());
        assert_eq!(presentation.undo_bytes, 0);
        assert_eq!(presentation.redo_bytes, 0);
        assert_eq!(presentation.pending_prefix, EditorPrefix::Idle);
        assert_eq!(
            presentation.last_find, None,
            "`Alt-.` must not repeat a motion resolved against the replaced text"
        );
        assert_eq!(presentation.find_note, FindNote::None);
    }

    // ── Character-finding motions ─────────────────────────────────────────

    /// The defect this milestone exists to not ship. Helix states the
    /// difference from vi in one sentence — *"`f`, `F`, `t` and `T` are not
    /// confined to the current line"* — because the current line is what an
    /// implementation reaches for by habit.
    ///
    /// Every target here sits on a *different* line from the cursor, so a
    /// scan bounded by `line_content_end` would report `Absent` and leave the
    /// cursor where it was, which is exactly the same observable result as
    /// the motion working. Asserting only "the cursor moved" would pass on
    /// both implementations.
    #[test]
    fn a_find_motion_spans_line_boundaries() {
        // `alpha\nbeta\ngamma\ndelta\n` — `g` is at byte 11, in the third
        // line; `b` is at byte 6, in the second. Both targets are the *only*
        // occurrence of their character anywhere in the document, so a
        // line-confined scan has nothing to find and reports "absent".
        let text = "alpha\nbeta\ngamma\ndelta\n";
        let snapshot = snapshot_of(text);

        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('g'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 11,
            "`f g` must cross the two line breaks between `alpha` and `gamma`"
        );

        let end = text.len();
        let mut presentation = presentation_at(end);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindPrevious('b'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 6,
            "`F b` must cross three line breaks back to the `b` in `beta`"
        );
    }

    /// `t`/`T` land **on** the target character, as does `f`/`F`.
    ///
    /// The plan rejects the other reading five times, and it is the reading an
    /// implementer arrives at from vi and kakoune, where a till-motion stops
    /// *before* the character. That mistake is invisible in a suite that only
    /// asserts "the cursor moved somewhere near the target", which is why the
    /// expected offset here is the target's own byte, spelled out.
    #[test]
    fn till_motions_land_on_the_target_character() {
        let text = "alpha\nbeta\ngamma\n";
        let snapshot = snapshot_of(text);

        // `alpha`: a(0) l(1) p(2) h(3) a(4) \n(5)
        let cases = [
            (EditorCommand::FindNext('l'), 1),
            (EditorCommand::FindTillNext('l'), 1),
            (EditorCommand::FindNext('a'), 4),
            (EditorCommand::FindTillNext('a'), 4),
        ];
        for (command, expected) in cases {
            let mut presentation = presentation_at(0);
            apply_motion(&mut presentation, &snapshot, command, 10);
            assert_eq!(
                presentation.cursor_byte, expected,
                "{command:?} must land on the target, not before it"
            );
        }

        // `gamma` starts at byte 11: g(11) a(12) m(13) m(14) a(15).
        let cases = [
            (EditorCommand::FindPrevious('g'), 11),
            (EditorCommand::FindTillPrevious('g'), 11),
            (EditorCommand::FindPrevious('m'), 14),
            (EditorCommand::FindTillPrevious('m'), 14),
        ];
        for (command, expected) in cases {
            let mut presentation = presentation_at(16);
            apply_motion(&mut presentation, &snapshot, command, 10);
            assert_eq!(
                presentation.cursor_byte, expected,
                "{command:?} must land on the target, not past it"
            );
        }
    }

    /// The scan starts strictly past the cursor in the direction of travel.
    ///
    /// Without this, `f x` with the cursor already on `x` is a no-op that
    /// looks exactly like a failed find, and `F x` cannot find the `x` the
    /// cursor is sitting on — which is the one `x` the user can see.
    #[test]
    fn a_find_motion_scans_strictly_past_the_cursor() {
        let snapshot = snapshot_of("xax");
        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('x'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 2,
            "`f x` on an `x` must find the next one, not stand still"
        );

        let mut presentation = presentation_at(2);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindPrevious('x'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 0,
            "`F x` on an `x` must find the previous one"
        );
    }

    /// A multi-byte target matches whole characters, never a byte of a
    /// neighbour — and the scan advances across multi-byte characters.
    ///
    /// The load-bearing case is the *interior* byte. The second byte of `é` is
    /// `0xA9`, and `read_bytes` refuses any range that does not end on a
    /// character boundary — so a scan that reads one byte at a time cannot
    /// read byte 1 of `"aé…"` at all, and walks straight past the `é`. That
    /// implementation passed on ASCII and failed silently on every document
    /// containing a non-ASCII character, which is why the two-byte and
    /// four-byte targets are both spelled out here.
    #[test]
    fn a_multi_byte_target_matches_whole_characters_only() {
        // `a`(1) `é`(2) `€`(3) `a`(1) — 7 bytes, 4 characters.
        let snapshot = snapshot_of("aé€a");
        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('é'),
            10,
        );
        assert_eq!(presentation.cursor_byte, 1);

        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('€'),
            10,
        );
        assert_eq!(presentation.cursor_byte, 3);

        let mut presentation = presentation_at(6);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindPrevious('é'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 1,
            "backward multi-byte targets resolve to their own byte offset"
        );

        // A four-byte target: the scan must step over one whole character to
        // reach the byte after it, and must not mistake the emoji's own
        // continuation bytes for a match.
        let snapshot = snapshot_of("z😀z");
        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('😀'),
            10,
        );
        assert_eq!(presentation.cursor_byte, 1);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('z'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 5,
            "the scan must advance by whole characters across a 4-byte one"
        );
    }

    /// A target past the bound is disclosed, not reported as absent.
    ///
    /// Both mean "the cursor did not move", and they call for opposite
    /// answers from the reader — type something else, or reach for `/`. A
    /// scan that stopped at the bound and said "not found" would be a search
    /// that lies by omission, which is the `search_partial` defect one layer
    /// down.
    #[test]
    fn a_find_beyond_the_scan_bound_is_disclosed_rather_than_called_absent() {
        // A target well past the window, and none within it at all.
        let filler = "a".repeat(MAX_EDITOR_FIND_BYTES + 1024);
        let text = format!("{filler}z");
        let snapshot = snapshot_of(&text);

        assert_eq!(
            find_char(&snapshot, 0, 'z', true),
            FindOutcome::Bounded,
            "a target past the window must be disclosed, not called absent"
        );
        assert_eq!(
            find_char(&snapshot, 0, 'q', true),
            FindOutcome::Bounded,
            "an absent target is also only known to be absent within the window"
        );

        // Scanning backwards from the end of a document *larger* than the
        // window still only covers the window, so that direction is bounded
        // too — it cannot claim "absent" any more than the forward scan can.
        assert_eq!(
            find_char(&snapshot, text.len(), 'q', false),
            FindOutcome::Bounded,
            "a backward scan past the window is bounded as well"
        );
        assert_eq!(
            find_char(&snapshot, text.len(), 'z', false),
            FindOutcome::Found(MAX_EDITOR_FIND_BYTES + 1024),
            "the target just past the forward bound is reachable backwards"
        );

        // A document inside the window is different: the whole of it was
        // scanned, so "absent" is a fact about the document rather than
        // about the scan.
        let small = snapshot_of("abc");
        assert_eq!(
            find_char(&small, 3, 'q', false),
            FindOutcome::Absent,
            "a backward scan inside the window may say absent for real"
        );
        assert_eq!(
            find_char(&small, 0, 'q', true),
            FindOutcome::Absent,
            "and so may a forward one"
        );

        // And the disclosure says the bound, so the two are distinguishable
        // in the status row and not only in a test.
        assert_eq!(
            FindNote::from_outcome(FindOutcome::Absent, 'z'),
            FindNote::Absent('z')
        );
        assert_eq!(
            FindNote::from_outcome(FindOutcome::Bounded, 'z'),
            FindNote::Bounded('z')
        );
        assert_eq!(FindNote::Absent('z').label(), "no 'z' in that direction");
        assert!(
            FindNote::Bounded('z').label().contains("2 MiB"),
            "the bounded note must state the bound: {}",
            FindNote::Bounded('z').label()
        );
        assert_ne!(
            FindNote::Absent('z').label(),
            FindNote::Bounded('z').label(),
            "the two not-found cases must be distinguishable on screen"
        );
        assert_eq!(
            FindNote::from_outcome(FindOutcome::Found(4), 'z'),
            FindNote::None
        );
        assert_eq!(FindNote::None.label(), "");
    }

    /// A find motion records itself for `Alt-.` and discloses how it went,
    /// including when it went nowhere.
    ///
    /// Recording only on success would be the intuitive choice and the wrong
    /// one: Helix sets the repeat slot by *running* the command, so `f z` with
    /// no `z` ahead still leaves `Alt-.` armed to try `f z` again from
    /// wherever the cursor is.
    #[test]
    fn a_find_motion_records_itself_for_repeat_and_discloses_its_outcome() {
        let snapshot = snapshot_of("abc\nxyz\n");
        let mut presentation = presentation_at(0);

        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('x'),
            10,
        );
        assert_eq!(
            presentation.last_find,
            Some(FindMotion {
                kind: FindKind::Next,
                target: 'x'
            })
        );
        assert_eq!(
            presentation.find_note,
            FindNote::None,
            "a find that landed has nothing to disclose"
        );

        // Now one that finds nothing: recorded, and disclosed.
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('q'),
            10,
        );
        assert_eq!(
            presentation.last_find,
            Some(FindMotion {
                kind: FindKind::Next,
                target: 'q'
            }),
            "a failed find is still the last find"
        );
        assert_eq!(presentation.find_note, FindNote::Absent('q'));

        // Only a find motion is ever recorded, and only a find motion maps
        // back to the command that produced it.
        for command in [
            EditorCommand::MoveRight,
            EditorCommand::WordForward,
            EditorCommand::BufferStart,
        ] {
            assert_eq!(command.find_motion(), None, "{command:?}");
            assert!(!command.is_find_motion(), "{command:?}");
        }
        for (kind, target, expected) in [
            (FindKind::Next, 'a', EditorCommand::FindNext('a')),
            (FindKind::Previous, 'b', EditorCommand::FindPrevious('b')),
            (FindKind::TillNext, 'c', EditorCommand::FindTillNext('c')),
            (
                FindKind::TillPrevious,
                'd',
                EditorCommand::FindTillPrevious('d'),
            ),
        ] {
            let motion = FindMotion { kind, target };
            assert_eq!(motion.command(), expected);
            assert_eq!(expected.find_motion(), Some(motion), "{expected:?}");
        }
    }

    /// Every armed prefix, with the leaf it completes and what it does to a key
    /// that completes nothing. `gg` and `space y` are the reason this is a table
    /// and not a single boolean: the baseline disagreed about the unmatched key,
    /// and merging them while picking one answer would have changed a gesture.
    ///
    /// The four find prefixes appear here too, and their `leaf` is `None` on
    /// purpose — their next key is a target *character*, not a leaf, so there
    /// is no glyph that could complete them. They are the reason the
    /// mismatched-key column is also a table: a key that cannot be a target
    /// must be handed back, not eaten.
    #[test]
    fn each_prefix_declares_its_leaf_and_its_unmatched_key_policy() {
        let cases = [
            (EditorPrefix::Idle, None, false, "", None),
            (EditorPrefix::Goto, Some('g'), true, "g", None),
            (EditorPrefix::Space, Some('y'), false, "space", None),
            (EditorPrefix::OpenBracket, None, false, "[", None),
            (EditorPrefix::CloseBracket, None, false, "]", None),
            (
                EditorPrefix::FindNext,
                None,
                false,
                "f",
                Some(FindKind::Next),
            ),
            (
                EditorPrefix::FindPrevious,
                None,
                false,
                "F",
                Some(FindKind::Previous),
            ),
            (
                EditorPrefix::FindTillNext,
                None,
                false,
                "t",
                Some(FindKind::TillNext),
            ),
            (
                EditorPrefix::FindTillPrevious,
                None,
                false,
                "T",
                Some(FindKind::TillPrevious),
            ),
        ];
        for (prefix, leaf, swallows, display, find_kind) in cases {
            assert_eq!(prefix.leaf(), leaf, "{prefix:?} leaf");
            assert_eq!(
                prefix.swallows_unmatched(),
                swallows,
                "{prefix:?} unmatched-key policy"
            );
            assert_eq!(prefix.display(), display, "{prefix:?} indicator");
            assert_eq!(prefix.find_kind(), find_kind, "{prefix:?} find kind");
            assert_eq!(
                prefix.is_find(),
                find_kind.is_some(),
                "{prefix:?} takes a target character"
            );
            assert_eq!(
                prefix.is_pending(),
                prefix != EditorPrefix::Idle,
                "{prefix:?}"
            );
        }
    }

    /// The one table mapping the four find keys onto the kinds they arm.
    ///
    /// Asserted against the dispatch layer's arms rather than trusted to them:
    /// a mistyped table here would arm `FindPrevious` for `T`, and the result
    /// would still be a pending prefix that resolves — just the wrong motion.
    #[test]
    fn the_four_find_keys_arm_the_four_find_kinds() {
        let cases = [
            ('f', EditorPrefix::FindNext, FindKind::Next, true),
            ('F', EditorPrefix::FindPrevious, FindKind::Previous, false),
            ('t', EditorPrefix::FindTillNext, FindKind::TillNext, true),
            (
                'T',
                EditorPrefix::FindTillPrevious,
                FindKind::TillPrevious,
                false,
            ),
        ];
        for (glyph, prefix, kind, forward) in cases {
            let armed = EditorPrefix::armed_by(glyph)
                .unwrap_or_else(|| panic!("{glyph:?} must arm a prefix"));
            assert_eq!(armed, prefix, "{glyph:?} arms");
            assert_eq!(armed.find_kind(), Some(kind), "{glyph:?} kind");
            assert_eq!(kind.forward(), forward, "{glyph:?} direction");
        }
        // Nothing else arms a find. Without this, the next milestone's leaves
        // could quietly join the table.
        for glyph in ['g', ' ', '[', ']', 'y', 'd', 'w', '\n', '.'] {
            assert_eq!(
                EditorPrefix::armed_by(glyph),
                None,
                "{glyph:?} must not arm a find prefix"
            );
        }
    }

    /// The indicator's width is bounded by construction: the pending prefix is a
    /// typed value rather than accumulated text, so the bound is asserted
    /// against every label instead of against a length a caller might pass.
    #[test]
    fn every_pending_prefix_display_fits_the_indicator_bound() {
        for prefix in [
            EditorPrefix::Idle,
            EditorPrefix::Goto,
            EditorPrefix::Space,
            EditorPrefix::OpenBracket,
            EditorPrefix::CloseBracket,
            EditorPrefix::FindNext,
            EditorPrefix::FindPrevious,
            EditorPrefix::FindTillNext,
            EditorPrefix::FindTillPrevious,
        ] {
            assert!(
                prefix.display().len() <= MAX_EDITOR_PENDING_PREFIX,
                "{prefix:?} renders {:?}, over the {MAX_EDITOR_PENDING_PREFIX} byte bound",
                prefix.display()
            );
        }
        assert_eq!(
            EditorPrefix::Space.display().len(),
            MAX_EDITOR_PENDING_PREFIX,
            "the bound is the longest label, not a guess"
        );
        assert_eq!(
            MAX_EDITOR_FIND_BYTES, MAX_EDITOR_SEARCH_BYTES,
            "a find motion inherits the search window rather than inventing a second bound"
        );
    }

    /// The grammar is one prefix deep: arming while armed replaces, because a
    /// stacked grammar is how a gesture ends up needing a third key.
    #[test]
    fn arming_replaces_rather_than_stacking_and_disarm_returns_to_idle() {
        let mut presentation = presentation_at(0);
        assert_eq!(presentation.pending_prefix, EditorPrefix::Idle);
        assert!(!presentation.pending_prefix.is_pending());

        presentation.arm_prefix(EditorPrefix::Goto);
        assert_eq!(presentation.pending_prefix, EditorPrefix::Goto);
        presentation.arm_prefix(EditorPrefix::Space);
        assert_eq!(
            presentation.pending_prefix,
            EditorPrefix::Space,
            "the second prefix replaced the first rather than stacking on it"
        );

        presentation.disarm_prefix();
        assert_eq!(presentation.pending_prefix, EditorPrefix::Idle);
        assert_eq!(presentation.pending_prefix.display(), "");
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

    // ── C002 bounded render budgets ─────────────────────────────────────

    fn huge_single_line(megabytes: usize) -> DocumentSnapshot {
        DocumentBuffer::new("x".repeat(megabytes * 1024 * 1024)).snapshot()
    }

    #[test]
    fn line_content_end_does_not_read_a_huge_line() {
        let bytes = 4 * 1024 * 1024;
        let snapshot = huge_single_line(4);
        let end = line_content_end(&snapshot, 0).expect("line exists");
        assert_eq!(end, bytes, "a one-line document has no terminator");
        assert_eq!(line_byte_len(&snapshot, 0), Some(bytes));
        assert!(line_is_truncated(&snapshot, 0));
        // The cost proof lives in `frame_cost_*` below; this asserts the
        // answer is exact without materializing the line.
    }

    #[test]
    fn frame_cost_stays_within_budget_for_a_4mib_single_line() {
        let snapshot = huge_single_line(4);
        let cost = measure_frame_cost(&snapshot, 0, 24);
        assert!(
            cost.total_bytes <= MAX_FRAME_SOURCE_BYTES,
            "frame must cost at most 256 KiB, got {}",
            cost.total_bytes
        );
        assert!(
            cost.max_line_bytes <= MAX_RENDER_LINE_BYTES,
            "one line must cost at most 32 KiB, got {}",
            cost.max_line_bytes
        );
        assert_eq!(cost.total_bytes, MAX_RENDER_LINE_BYTES);
        assert_eq!(cost.truncated_lines, 1);
        // The rope reports a trailing empty line past the final newline (or
        // end of text); it costs zero bytes and one scan, like the renderer.
        assert_eq!(cost.column_scans, 2);
    }

    #[test]
    fn frame_cost_stays_within_budget_for_many_huge_lines() {
        let text = (0..30)
            .map(|_| "y".repeat(1024 * 1024))
            .collect::<Vec<_>>()
            .join("\n");
        let snapshot = snapshot_of(&text);
        let cost = measure_frame_cost(&snapshot, 0, 30);
        assert!(
            cost.total_bytes <= MAX_FRAME_SOURCE_BYTES,
            "frame total must stay within 256 KiB, got {}",
            cost.total_bytes
        );
        assert!(cost.max_line_bytes <= MAX_RENDER_LINE_BYTES);
        // 8 full 32 KiB windows fit in 256 KiB; the remaining 22 rows are
        // frame-capped markers, still counted as truncated.
        assert_eq!(cost.truncated_lines, 30);
        assert_eq!(cost.total_bytes, MAX_FRAME_SOURCE_BYTES);
    }

    #[test]
    fn frame_cost_is_exact_for_ordinary_lines() {
        let snapshot = snapshot_of("ab\ncdef\n");
        let cost = measure_frame_cost(&snapshot, 0, 10);
        // Row sizes include the terminator (3 + 5); the budget is
        // deliberately conservative by one byte per line.
        assert_eq!(cost.total_bytes, 3 + 5);
        assert_eq!(cost.max_line_bytes, 5);
        assert_eq!(cost.truncated_lines, 0);
        // Two content lines plus the rope's trailing empty line, which costs
        // zero bytes — the same rows the renderer walks.
        assert_eq!(cost.column_scans, 3);
    }

    #[test]
    fn cursor_at_the_far_right_of_a_huge_line_saturates_honestly() {
        let snapshot = huge_single_line(1);
        let end = snapshot.len_bytes();
        let column = display_column(&snapshot, end);
        assert!(
            column <= MAX_RENDER_LINE_BYTES,
            "column display must saturate at the window, got {column}"
        );
        assert!(
            display_column_truncated(&snapshot, end),
            "the banner signal must fire for a cursor past the window"
        );
        assert!(!display_column_truncated(&snapshot, 0));
        assert_eq!(display_column(&snapshot, 0), 0);
    }

    #[test]
    fn motion_past_a_huge_line_prefix_clamps_to_the_prefix() {
        let snapshot = huge_single_line(1);
        let start = line_start(&snapshot, 0).expect("line exists");
        let landed = offset_at_column(&snapshot, 0, usize::MAX).expect("clamps");
        let window_end = bounded_prefix_end(&snapshot, start, snapshot.len_bytes());
        assert_eq!(landed, window_end);
        assert!(landed <= start + MAX_RENDER_LINE_BYTES);
        assert!(landed > start, "the whole prefix stays reachable");
        // Ordinary lines keep exact motion.
        let small = snapshot_of("ab\ncdef");
        assert_eq!(offset_at_column(&small, 0, 99), Some(2));
    }

    #[test]
    fn bounded_prefix_matches_exact_for_short_lines_tabs_and_unicode() {
        // Tabs (variable advance), wide CJK (width 2), and a combining mark
        // (width 0) must measure exactly when inside the budget.
        let snapshot = snapshot_of("a\t\te\u{301}€\t汉\nend");
        let line = 0;
        assert!(!line_is_truncated(&snapshot, line));
        let start = line_start(&snapshot, line).expect("line exists");
        let end = line_content_end(&snapshot, line).expect("end exists");
        assert_eq!(bounded_prefix_end(&snapshot, start, end), end);
        let (bounded, truncated) = bounded_display_width(&snapshot, start..end);
        assert!(!truncated);
        // Whatever the width walk computes for tabs, wide, and combining
        // characters, the bounded path must agree exactly inside the budget.
        assert_eq!(bounded, display_width(&snapshot, start..end));
    }

    #[test]
    fn bounded_prefix_end_never_splits_a_multibyte_char() {
        // Fill the window edge with a 3-byte char so the 32 KiB cut lands
        // inside one.
        let mut text = "a".repeat(MAX_RENDER_LINE_BYTES - 2);
        text.push('€');
        text.push_str(&"b".repeat(1024));
        let snapshot = snapshot_of(&text);
        assert!(line_is_truncated(&snapshot, 0));
        let start = line_start(&snapshot, 0).expect("line exists");
        let end = line_content_end(&snapshot, 0).expect("end exists");
        let window_end = bounded_prefix_end(&snapshot, start, end);
        assert!(window_end <= start + MAX_RENDER_LINE_BYTES);
        assert!(snapshot.is_byte_boundary(window_end));
        assert!(
            snapshot.read_bytes(start, window_end).is_ok(),
            "the window must be directly readable"
        );
    }

    #[test]
    fn command_spellings_parse_to_their_canonical_names() {
        assert_eq!(
            parse_editor_command("w"),
            Some((EditorCommandName::Write, None))
        );
        assert_eq!(
            parse_editor_command("q"),
            Some((EditorCommandName::Quit, None))
        );
        assert_eq!(
            parse_editor_command("wq"),
            Some((EditorCommandName::WriteQuit, None))
        );
        // `:x` is the vim spelling of the same intent, and must not become
        // a second command with its own identity.
        assert_eq!(
            parse_editor_command("x"),
            Some((EditorCommandName::WriteQuit, None))
        );
        assert_eq!(
            parse_editor_command("q!"),
            Some((EditorCommandName::ForceQuit, None))
        );
        assert_eq!(
            parse_editor_command("  w  "),
            Some((EditorCommandName::Write, None))
        );
    }

    #[test]
    fn a_stray_argument_refuses_the_whole_command() {
        // `:wq now` must not silently run a `:wq`. Half of what was asked
        // for is worse than refusing it.
        assert_eq!(parse_editor_command("wq now"), None);
        assert_eq!(parse_editor_command("q extra"), None);
        assert_eq!(parse_editor_command(""), None);
        assert_eq!(parse_editor_command("zz"), None);
    }

    #[test]
    fn only_open_takes_an_argument() {
        assert_eq!(
            parse_editor_command("open src/main.rs"),
            Some((EditorCommandName::Open, Some("src/main.rs".to_string())))
        );
        assert!(EditorCommandName::Open.takes_argument());
        assert!(!EditorCommandName::Write.takes_argument());
    }

    #[test]
    fn complete_parses_without_an_argument() {
        assert_eq!(
            parse_editor_command("complete"),
            Some((EditorCommandName::Complete, None))
        );
        assert!(!EditorCommandName::Complete.takes_argument());
        assert_eq!(parse_editor_command("complete now"), None);
        assert_eq!(EditorCommandName::Complete.spec(), "complete");
        assert!(EditorCommandName::ALL.contains(&EditorCommandName::Complete));
    }

    #[test]
    fn an_empty_query_offers_the_whole_registry_in_declared_order() {
        let matches = complete_editor_command("");
        assert_eq!(matches.len(), EditorCommandName::ALL.len());
        // Declared order, not alphabetical: the empty palette is the
        // discovery menu and reads as one.
        assert_eq!(matches[0].command, EditorCommandName::ALL[0]);
        assert_eq!(matches[1].command, EditorCommandName::ALL[1]);
    }

    #[test]
    fn fuzzy_completion_narrows_and_ranks_a_prefix_first() {
        let matches = complete_editor_command("w");
        let names: Vec<&str> = matches.iter().map(|m| m.command.spec()).collect();
        assert!(names.contains(&"w"), "{names:?}");
        assert!(names.contains(&"wq"), "{names:?}");
        // `w` is an exact prefix and `new` only a subsequence match, so `w`
        // must rank first even though `fuzzy_score("w", "new")` is no worse.
        assert_eq!(names[0], "w", "{names:?}");
        // Within the prefix tier the shortest wins.
        let wq_at = names.iter().position(|name| *name == "wq").unwrap();
        assert!(wq_at > 0, "{names:?}");
    }

    #[test]
    fn fuzzy_completion_excludes_commands_that_do_not_match() {
        // `fuzzy_score` returns 0 for both an empty query and a total
        // miss, so a non-matching command must be filtered by the prefix
        // test rather than the score alone.
        let names: Vec<&str> = complete_editor_command("qz")
            .iter()
            .map(|m| m.command.spec())
            .collect();
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn the_completion_list_stays_bounded() {
        let matches = complete_editor_command("a");
        assert!(
            matches.len() <= MAX_EDITOR_COMMAND_MATCHES,
            "{}",
            matches.len()
        );
    }

    #[test]
    fn the_command_line_is_bounded_so_it_cannot_grow_into_a_buffer() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_command_line("");
        for _ in 0..(MAX_EDITOR_COMMAND_LINE + 10) {
            presentation.push_command_char('x');
        }
        assert_eq!(
            presentation.command_line.chars().count(),
            MAX_EDITOR_COMMAND_LINE
        );
        // A refused keystroke must not have disturbed the line.
        assert!(!presentation.push_command_char('y'));
        assert_eq!(
            presentation.command_line.chars().count(),
            MAX_EDITOR_COMMAND_LINE
        );
    }

    #[test]
    fn closing_the_command_line_discards_what_was_typed() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_command_line("");
        presentation.push_command_char('w');
        assert!(presentation.command_active);
        presentation.close_command_line();
        assert!(!presentation.command_active);
        assert!(presentation.command_line.is_empty());
    }

    #[test]
    fn palette_selection_stays_inside_the_current_rows() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_command_line("");
        let count = presentation.command_matches().len();
        presentation.move_command_selection(-5);
        assert_eq!(presentation.command_selection, 0);
        for _ in 0..(count + 5) {
            presentation.move_command_selection(1);
        }
        assert_eq!(presentation.command_selection, count - 1);
    }

    #[test]
    fn an_unknown_command_yields_nothing_to_run() {
        // `parse_editor_command` returning `None` is what turns a typo into
        // a notice rather than a silent no-op.
        assert_eq!(parse_editor_command("write"), None);
        assert_eq!(parse_editor_command("q!q"), None);
    }
}
