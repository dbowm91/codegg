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
/// [`MAX_EDITOR_PENDING_COMMAND`](super::editor::MAX_EDITOR_PENDING_COMMAND).
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
    let range = snapshot.line_range(line).ok()?;
    if range.start >= range.end {
        return Some(range.start);
    }
    // The terminator, if any, is `\n`, `\r`, or `\r\n` at the very end.
    let mut tail_start = range.end.saturating_sub(2).max(range.start);
    while tail_start < range.end && !snapshot.is_byte_boundary(tail_start) {
        tail_start += 1;
    }
    let tail = snapshot.read_bytes(tail_start, range.end).ok()?;
    let terminator = if tail.ends_with("\r\n") {
        2
    } else if tail.ends_with('\n') || tail.ends_with('\r') {
        1
    } else {
        0
    };
    Some(range.end.saturating_sub(terminator))
}

/// Byte offset of the end of the last line, excluding any terminator.
pub fn buffer_content_end(snapshot: &DocumentSnapshot) -> usize {
    let last = snapshot.len_lines().saturating_sub(1);
    line_content_end(snapshot, last).unwrap_or_else(|| snapshot.len_bytes())
}

/// Zero-based index of the document's last line.
///
/// A document always has at least one line, so this never underflows; it is
/// the single answer to "where does the file end" that paging and viewport
/// arithmetic share.
pub fn last_line_of(snapshot: &DocumentSnapshot) -> usize {
    snapshot.len_lines().saturating_sub(1)
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
pub fn word_end(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let end = snapshot.len_bytes();
    let mut cursor = clamp_offset(snapshot, offset);
    if !is_word_byte(byte_at(snapshot, cursor).unwrap_or(0)) {
        cursor = word_forward(snapshot, cursor);
        if cursor >= end || !is_word_byte(byte_at(snapshot, cursor).unwrap_or(0)) {
            return buffer_content_end(snapshot);
        }
    }
    while cursor < end.saturating_sub(1) && is_word_byte(byte_at(snapshot, cursor + 1).unwrap_or(0))
    {
        cursor = next_char_boundary(snapshot, cursor).unwrap_or(cursor);
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
            let mut probe = TuiDocumentPresentation {
                viewport_line: presentation.viewport_line,
                cursor_byte: presentation.cursor_byte,
                ..TuiDocumentPresentation::default()
            };
            let next = apply_motion(&mut probe, snapshot, command, NOMINAL_HELIX_PAGES);
            // The scroll has to come back with the cursor: it was resolved on
            // the probe, and dropping it is what made a second `Ctrl+d` land on
            // the same line as the first.
            scrolled_to = probe.viewport_line;
            state.move_to(editor_point(snapshot, next));
            outcome = HelixVerbOutcome::Moved(next);
        }
    }
    presentation.cursor_byte = state.cursor().byte;
    presentation.selection = state.selection();
    presentation.viewport_line = scrolled_to;
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
pub fn apply_motion(
    presentation: &mut TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    command: EditorCommand,
    visible_lines: usize,
) -> usize {
    let resolved = apply_motion_inner(presentation, snapshot, command, visible_lines);
    presentation.cursor_byte = resolved;
    presentation.viewport_line =
        viewport_after_motion(presentation, snapshot, command, resolved, visible_lines);
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
