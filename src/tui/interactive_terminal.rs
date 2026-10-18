//! TUI interactive-terminal controller (M003).
//!
//! This module is the reference TUI interactive-terminal experience over the
//! M002 bounded attach/resume protocol. It is a **projection** of daemon
//! M002 handles and output: it owns no PTY, no process, and no scheduler
//! admission. The single canonical PTY owner remains
//! [`crate::interactive_process::InteractiveProcessService`]; attachment
//! authority remains with
//! [`crate::interactive_process_attach::InteractiveProcessProtocol`].
//!
//! ```text
//! daemon M001 engine --M002 protocol--> CoreRequest/CoreResponse
//!        (PTY/process owner)                    |
//!                                               v
//!                              InteractiveTerminalController
//!                              (bounded VT screen + scrollback + focus + link state)
//!                                               |
//!                                               v
//!                                   TUI dialog rendering (lines only)
//! ```
//!
//! # Invariants (plan §4)
//!
//! - The TUI never owns PTY/process state: every mutation goes through a
//!   `CoreRequest::InteractiveProcess*` operation and every state change is
//!   applied from a `CoreResponse` (or a typed transport notice).
//! - Keyboard bytes reach a terminal only when that terminal has explicit
//!   focus ([`TerminalFocus::Focused`]). Esc is forwarded; a second Esc
//!   (or Ctrl-]) leaves focus without sending bytes to the child.
//! - Client disconnect drops attachments only (M002 `handle_disconnect`
//!   semantics): scrollback is retained locally and the link moves to
//!   [`TerminalLinkState::Reconnecting`]. A daemon restart invalidates the
//!   ephemeral handles: every view moves to [`TerminalLinkState::Gone`].
//! - Raw terminal bytes never become session observation: this module has
//!   no dependency on session, projection, observer, or model-context
//!   types. Rendering exposes bounded lossy lines only.
//!
//! # Bounds
//!
//! Size/input/chunk bounds mirror the canonical M001/M002 constants
//! (`MAX_INPUT_WRITE_BYTES`,
//! `MAX_INTERACTIVE_CHUNK_BYTES`) rather than re-declaring them. The
//! per-view scrollback window ([`TERMINAL_VIEW_BYTES`]) matches the M002
//! default read size so one attach chunk always fits. The per-controller
//! terminal cap ([`MAX_TERMINALS_PER_VIEW`]) matches the M002 per-client
//! attachment cap so the TUI cannot hold more terminals than the daemon
//! lets one client attach to.

use std::collections::{BTreeMap, VecDeque};

use crate::protocol::interactive_process::{
    InteractiveOutputChunk, InteractiveResync, InteractiveResyncReason, MAX_ATTACHMENTS_PER_CLIENT,
    MAX_INTERACTIVE_CHUNK_BYTES, MAX_INTERACTIVE_INPUT_BYTES,
};
use crate::tui::helix_selection::{Selection, SelectionMode, SelectionPoint, SelectionState};

/// Bytes of decoded terminal output retained per viewed terminal.
///
/// Matches the M002 default read size so a single attach/resume chunk
/// always fits without truncation.
pub const TERMINAL_VIEW_BYTES: usize = 64 * 1024;

/// Maximum rendered lines per terminal view. Rendering takes the newest
/// lines within the retained byte window.
pub const TERMINAL_VIEW_LINES: usize = 500;

/// Maximum bytes per rendered line. Longer lines are truncated with a
/// marker so one escape-heavy line cannot dominate the dialog.
pub const MAX_TERMINAL_LINE_BYTES: usize = 8 * 1024;

/// Maximum terminals tracked by one controller.
///
/// Mirrors the M002 per-client attachment cap: the TUI never tracks more
/// terminals than the daemon permits one client to attach to.
pub const MAX_TERMINALS_PER_VIEW: usize = MAX_ATTACHMENTS_PER_CLIENT;

/// Maximum bytes accepted by one [`InteractiveTerminalController::queue_input`]
/// call. Mirrors the M001/M002 per-write input bound.
pub const MAX_TERMINAL_INPUT_BYTES: usize = MAX_INTERACTIVE_INPUT_BYTES;

/// Maximum bytes rendered from one terminal in a single dialog refresh.
/// Bounded by the M002 per-read chunk cap.
pub const MAX_TERMINAL_RENDER_BYTES: usize = MAX_INTERACTIVE_CHUNK_BYTES;
pub const MAX_TERMINAL_SCREEN_COLS: u16 = 300;
pub const MAX_TERMINAL_SCREEN_ROWS: u16 = 120;

/// Retained VT scrollback rows per terminal.
///
/// Bounded rather than unbounded: this is a long-lived daemon-backed process
/// view, and unbounded history is an unbounded per-terminal allocation. When
/// the oldest rows age out, viewport coordinates shift with them — which is
/// why the selection is anchored in viewport space (see
/// [`crate::tui::helix_selection`]).
pub const TERMINAL_SCROLLBACK_LINES: usize = 5_000;

/// Scrubbed column ranges one view remembers after `d`.
///
/// Bounded because the overlay is a convenience, not a second document: a
/// reader redacting a build log redacts a handful of lines, and an unbounded
/// list would let repeated `d` grow a per-view allocation forever.
pub const MAX_TERMINAL_REDACTIONS: usize = 64;

/// A terminal selection point: a viewport `(row, col)` cell.
///
/// Row-major `Ord` is what lets the shared selection machine treat this as a
/// flat reading order, and `Line = u16` is what lets it answer "which lines
/// does this span" without knowing how wide the screen is.
pub type TerminalPoint = (u16, u16);

impl SelectionPoint for TerminalPoint {
    type Line = u16;

    fn line(&self) -> u16 {
        self.0
    }
}

/// One scrubbed column range, remembered by the *text* of its row rather than
/// by its position.
///
/// Keyed on content because the VT emulator is read-only (there is no API to
/// remove a row) and, more to the point, because a bounded scrollback has no
/// stable row identity to key on: output arriving shifts every row below it,
/// and eviction removes rows from the top. Remembering what the row *said*
/// keeps a deletion attached to the text it was applied to, so scrolling away
/// and back does not silently un-delete it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Redaction {
    text: String,
    from: u16,
    to: u16,
}

/// Bounded set of [`Redaction`]s, resolved against the viewport on each read.
#[derive(Debug, Default)]
struct Redactions {
    entries: VecDeque<Redaction>,
}

impl Redactions {
    /// Remember one scrubbed range. Blank rows are refused: an empty `text`
    /// would match every blank row on screen and erase an unrelated column.
    fn push(&mut self, text: String, from: u16, to: u16) {
        if text.is_empty() || from >= to {
            return;
        }
        if self.entries.len() >= MAX_TERMINAL_REDACTIONS {
            self.entries.pop_front();
        }
        self.entries.push_back(Redaction { text, from, to });
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Match remembered redactions onto the rows currently in view.
    ///
    /// Returns one entry per viewport row, each a list of `[from, to)` column
    /// ranges. Entries claim rows in order and never claim the same row
    /// twice, so deleting two identical lines blanks the two of them rather
    /// than the same line twice.
    fn resolve(&self, row_text: &[String]) -> Vec<Vec<(u16, u16)>> {
        let mut by_row: Vec<Vec<(u16, u16)>> = vec![Vec::new(); row_text.len()];
        if self.entries.is_empty() {
            return by_row;
        }
        let mut claimed = vec![false; row_text.len()];
        for entry in &self.entries {
            let Some(row) = row_text
                .iter()
                .enumerate()
                .position(|(index, text)| !claimed[index] && *text == entry.text)
            else {
                continue;
            };
            claimed[row] = true;
            by_row[row].push((entry.from, entry.to));
        }
        by_row
    }

    /// Copy `text` with every redacted column replaced by a space.
    ///
    /// Applied on the yank path as well as the render path: a selection made
    /// *after* a deletion must not be able to copy back the text that was
    /// deleted, or `d` would be a visual effect rather than a deletion.
    fn scrub(text: &str, ranges: &[(u16, u16)]) -> String {
        if ranges.is_empty() {
            return text.to_string();
        }
        text.chars()
            .enumerate()
            .map(|(index, ch)| {
                let column = u16::try_from(index).unwrap_or(u16::MAX);
                if ranges
                    .iter()
                    .any(|&(from, to)| column >= from && column < to)
                {
                    ' '
                } else {
                    ch
                }
            })
            .collect()
    }
}

/// Explicit keyboard-focus state for one terminal view.
///
/// Input bytes are produced by [`classify_key`] only in
/// [`TerminalFocus::Focused`].
///
/// Where a terminal *starts* depends on the surface, and the difference is
/// deliberate. The terminal **box** opens in [`TerminalFocus::Focused`]: it is
/// a modal the user opened to run a command, and a focus gesture in front of
/// the first keystroke swallowed characters with nowhere for them to go. The
/// docked **panel** opens in [`TerminalFocus::Viewing`]: it is a surface the
/// user is looking at beside their work, so it must not take the keyboard
/// until they ask with `i`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFocus {
    /// Terminal is not the active view; keys belong to the prompt/dialog.
    Hidden,
    /// Terminal is visible but keys still belong to the prompt/dialog.
    Viewing,
    /// Terminal owns the keyboard; Ctrl-] is reserved to leave focus.
    Focused,
}

impl TerminalFocus {
    /// Short status label for `/terminal-list` and panel status rows.
    pub fn label(&self) -> &'static str {
        match self {
            TerminalFocus::Hidden => "hidden",
            TerminalFocus::Viewing => "viewing",
            TerminalFocus::Focused => "focused",
        }
    }
}

/// Daemon-link state for one terminal view.
///
/// Mirrors the M002 lifecycle from the TUI side: attach/detach never kill
/// the process; disconnect drops the attachment only; restart invalidates
/// the ephemeral handle; exit is terminal and rejects input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalLinkState {
    /// Attached (or attachable) and the process is live.
    Live,
    /// Transport dropped: attachment released server-side per M002
    /// `handle_disconnect`. Scrollback is retained; re-attach resumes
    /// from the last cursor when the process still lives.
    Reconnecting,
    /// The cursor can no longer resume incrementally; the embedded
    /// [`InteractiveResync`] tells the UX where to resume from.
    ResyncRequired(InteractiveResync),
    /// The process exited. Post-mortem scrollback remains readable;
    /// input is rejected.
    Exited {
        exit_code: Option<i32>,
        exit_signal: Option<i32>,
    },
    /// The handle is gone (removed, or the daemon restarted and ephemeral
    /// handles were invalidated). Scrollback is retained for reading;
    /// every mutation is rejected.
    Gone { reason: String },
}

impl TerminalLinkState {
    /// Whether the view may accept queued input or resize.
    pub fn allows_mutation(&self) -> bool {
        matches!(self, TerminalLinkState::Live)
    }

    /// Whether the process behind the view is known to have exited.
    pub fn is_exited(&self) -> bool {
        matches!(self, TerminalLinkState::Exited { .. })
    }

    /// Short status label for the dialog header.
    pub fn label(&self) -> &'static str {
        match self {
            TerminalLinkState::Live => "live",
            TerminalLinkState::Reconnecting => "reconnecting",
            TerminalLinkState::ResyncRequired(_) => "resync-required",
            TerminalLinkState::Exited { .. } => "exited",
            TerminalLinkState::Gone { .. } => "gone",
        }
    }
}

/// Typed rejection for [`InteractiveTerminalController::queue_input`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalInputError {
    UnknownHandle,
    /// The link state is not [`TerminalLinkState::Live`] (exited, gone,
    /// reconnecting, or awaiting resync).
    NotLive(&'static str),
    /// A single queue call exceeded [`MAX_TERMINAL_INPUT_BYTES`].
    TooLarge {
        len: usize,
        max: usize,
    },
}

impl std::fmt::Display for TerminalInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminalInputError::UnknownHandle => write!(f, "unknown terminal handle"),
            TerminalInputError::NotLive(label) => {
                write!(f, "terminal is {label}; input rejected")
            }
            TerminalInputError::TooLarge { len, max } => {
                write!(f, "input {len} bytes exceeds the {max} byte bound")
            }
        }
    }
}

/// Typed rejection for [`InteractiveTerminalController::queue_resize`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalResizeError {
    UnknownHandle,
    NotLive(&'static str),
    InvalidSize { cols: u16, rows: u16 },
}

impl std::fmt::Display for TerminalResizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminalResizeError::UnknownHandle => write!(f, "unknown terminal handle"),
            TerminalResizeError::NotLive(label) => {
                write!(f, "terminal is {label}; resize rejected")
            }
            TerminalResizeError::InvalidSize { cols, rows } => {
                write!(
                    f,
                    "invalid terminal size {cols}x{rows}: TUI dimensions must be 1..={} columns by 1..={} rows",
                    MAX_TERMINAL_SCREEN_COLS,
                    MAX_TERMINAL_SCREEN_ROWS,
                )
            }
        }
    }
}

/// Framework-neutral keyboard event for terminal focus handling.
///
/// The App layer maps crossterm key events to this enum so this
/// controller stays free of raw-terminal types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKey {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    CtrlC,
    CtrlD,
    /// Leave insert: hand the keyboard back to the reader (NOR), where the
    /// cursor moves and text can be selected. Never forwarded to the child.
    Esc,
    /// A literal `ESC` **for the child process** — `Ctrl+]`.
    ///
    /// Split from [`TerminalKey::Esc`] because the two want opposite things,
    /// and folding them together is what made this unusable. `Esc` is the
    /// mode key: a host that swallows it can never let full-screen programs
    /// (vim, less, tmux) see their own escape. `Ctrl+]` is what a user presses
    /// when the escape is meant for the program instead of for the host.
    LiteralEscape,
    Other,
}

/// Outcome of routing one key through [`classify_key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalKeyAction {
    /// Forward these exact bytes to the focused terminal.
    Forward(Vec<u8>),
    /// Leave terminal focus. Never forwards bytes and never submits.
    EscapeFocus,
    /// The key belongs to the prompt/dialog layer, not the terminal.
    Ignored,
}

/// Route one key through the terminal focus gate.
///
/// The gate is deliberately **stateless**, which is the whole point of this
/// revision. It used to carry a `pending_escape` flag so a lone `Esc` could be
/// forwarded to vim while *two* of them left focus. That made escape the
/// answer to two contradictory questions — "leave insert" and "pass an escape
/// byte through" — and users could not tell which one a press would do. There
/// is now one key per meaning:
///
/// - `Esc` leaves insert ([`TerminalKeyAction::EscapeFocus`]). It is the mode
///   key, and the reader it returns to is where the cursor moves and text can
///   be selected.
/// - `Ctrl+]` ([`TerminalKey::LiteralEscape`]) forwards a real `0x1b` for
///   full-screen programs, and stays in insert.
/// - Any key while unfocused yields [`TerminalKeyAction::Ignored`]: the
///   prompt owns the keyboard until focus is explicit.
/// - While focused, printable input and the explicit control bytes
///   (`Enter` as carriage return, `Backspace` as DEL, `Tab`, arrows as
///   ANSI sequences, `Ctrl-C`/`Ctrl-D`) yield
///   [`TerminalKeyAction::Forward`]. Everything else is ignored.
pub fn classify_key(focused: bool, key: TerminalKey) -> TerminalKeyAction {
    if !focused {
        return TerminalKeyAction::Ignored;
    }
    match key {
        TerminalKey::Esc => TerminalKeyAction::EscapeFocus,
        TerminalKey::LiteralEscape => TerminalKeyAction::Forward(vec![0x1b]),
        TerminalKey::Other => TerminalKeyAction::Ignored,
        TerminalKey::Enter => TerminalKeyAction::Forward(vec![b'\r']),
        TerminalKey::Backspace => TerminalKeyAction::Forward(vec![0x7f]),
        TerminalKey::Tab => TerminalKeyAction::Forward(vec![b'\t']),
        TerminalKey::CtrlC => TerminalKeyAction::Forward(vec![0x03]),
        TerminalKey::CtrlD => TerminalKeyAction::Forward(vec![0x04]),
        TerminalKey::Up => TerminalKeyAction::Forward(b"\x1b[A".to_vec()),
        TerminalKey::Down => TerminalKeyAction::Forward(b"\x1b[B".to_vec()),
        TerminalKey::Right => TerminalKeyAction::Forward(b"\x1b[C".to_vec()),
        TerminalKey::Left => TerminalKeyAction::Forward(b"\x1b[D".to_vec()),
        TerminalKey::Char(c) => {
            let mut bytes = [0u8; 4];
            TerminalKeyAction::Forward(c.encode_utf8(&mut bytes).as_bytes().to_vec())
        }
    }
}

/// Bounded scrollback window for one terminal view.
///
/// Holds the newest [`TERMINAL_VIEW_BYTES`] of terminal output plus the
/// M002 sequence cursors (`base_seq`/`next_seq`). A discontinuous chunk
/// (gap flag or cursor mismatch) replaces the window and records a
/// resync notice: the view shows the newest retained bytes and tells the
/// user history expired, mirroring the typed M002 resync contract.
#[derive(Debug, Clone)]
struct BoundedScrollback {
    bytes: VecDeque<u8>,
    base_seq: u64,
    next_seq: u64,
    truncated: bool,
    notice: Option<String>,
}

impl BoundedScrollback {
    fn new() -> Self {
        Self {
            bytes: VecDeque::new(),
            base_seq: 0,
            next_seq: 0,
            truncated: false,
            notice: None,
        }
    }

    /// Apply one M002 output chunk to the window.
    fn apply_chunk(&mut self, chunk: &InteractiveOutputChunk, raw: &[u8]) -> bool {
        let reset = chunk.gap || chunk.from_seq != self.next_seq;
        if reset {
            self.bytes.clear();
            self.base_seq = chunk.from_seq;
            if chunk.gap {
                self.notice = Some(format!(
                    "history expired: showing newest output from seq {} (wanted {})",
                    chunk.next_seq, chunk.from_seq
                ));
            } else if chunk.from_seq != self.next_seq {
                self.notice = Some(format!(
                    "stream resynchronized at seq {} (view was at {})",
                    chunk.next_seq, self.next_seq
                ));
            }
            self.truncated = true;
        }
        self.next_seq = chunk.next_seq;
        if self.base_seq == 0 && chunk.from_seq == 0 && self.bytes.is_empty() {
            self.base_seq = chunk.from_seq;
        }
        self.bytes.extend(raw.iter().copied());
        while self.bytes.len() > TERMINAL_VIEW_BYTES {
            let overflow = self.bytes.len() - TERMINAL_VIEW_BYTES;
            self.bytes.drain(..overflow);
            self.truncated = true;
        }
        reset
    }

    fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }
}

/// One TUI terminal view: projection state for a single M002 handle.
///
/// Carries no PTY and no process: `attachment_id` names the
/// caller-owned M002 attachment (resolved server-side by the daemon),
/// `workspace_id` routes the view to its multi-project owner, and
/// `scrollback` holds the newest bounded output window.
#[derive(Debug)]
pub struct InteractiveTerminalView {
    handle: String,
    workspace_id: String,
    command: String,
    attachment_id: Option<String>,
    link: TerminalLinkState,
    focus: TerminalFocus,
    scrollback: BoundedScrollback,
    screen: TerminalScreen,
    screen_degraded: bool,
    pending_input: Vec<u8>,
    pending_resize: Option<(u16, u16)>,
    cols: u16,
    rows: u16,
    /// Rows the viewport is parked above the live bottom; 0 means "following".
    viewport_offset: usize,
    /// Helix cursor and any held selection, in viewport coordinates.
    ///
    /// Coordinates are **viewport** `(row, col)` — row 0 is the top visible
    /// row once a scrollback offset is applied. Viewport rather than absolute
    /// buffer coordinates is what keeps a selection on the *same text* while
    /// the user scrolls: a scroll of `delta` rows moves the viewport by
    /// `delta`, so every stored point is moved by the same `delta` through
    /// [`SelectionState::translate`]. Absolute coordinates would instead drift
    /// whenever the bounded scrollback evicted its oldest row underneath a
    /// live selection.
    nav: SelectionState<TerminalPoint>,
    /// Columns scrubbed by `d`, resolved against the viewport on every read.
    redactions: Redactions,
    /// A bare `Space` is waiting for its second key (`space+y` yanks).
    ///
    /// Legacy xterm encoding delivers `Space` and `y` as two separate key
    /// events rather than one chord, so the prefix has to be remembered.
    pending_prefix: bool,
    /// Whether the box prints the full diagnostics dump above the output.
    ///
    /// Off by default. The dump is six lines of handle, workspace, attachment
    /// id and sequence cursors, which is what a bug report needs but is the
    /// first thing on screen for someone who just asked for a shell — and it
    /// pushed the actual terminal rows off a small dialog. `D` restores it.
    diagnostics: bool,
    /// This attachment's cadence-read slot. Never shared with
    /// `dialog_state.terminal_request`.
    read: TerminalReadState,
    /// One input operation at a time, so keystrokes cannot race each other.
    input_in_flight: bool,
}

impl InteractiveTerminalView {
    fn new(handle: String, workspace_id: String, command: String, cols: u16, rows: u16) -> Self {
        Self {
            handle,
            workspace_id,
            command,
            attachment_id: None,
            link: TerminalLinkState::Live,
            focus: TerminalFocus::Viewing,
            scrollback: BoundedScrollback::new(),
            screen: TerminalScreen::new(rows, cols),
            screen_degraded: false,
            pending_input: Vec::new(),
            pending_resize: None,
            cols,
            rows,
            viewport_offset: 0,
            nav: SelectionState::new((rows.saturating_sub(1), 0)),
            redactions: Redactions::default(),
            pending_prefix: false,
            diagnostics: false,
            read: TerminalReadState::default(),
            input_in_flight: false,
        }
    }

    /// Park the emulator viewport at this view's offset and pull the cursor back
    /// inside the screen.
    ///
    /// Called at the head of every read and navigation entry point so a stale
    /// offset can never reach the emulator: `Grid::visible_rows` computes
    /// `scrollback.len() - scrollback_offset` unguarded, so an offset that
    /// outgrew the retained history would underflow rather than clamp.
    fn sync_viewport(&mut self) {
        let max = self.screen.max_offset();
        if self.viewport_offset > max {
            self.viewport_offset = max;
        }
        self.screen.set_offset(self.viewport_offset);
        let max_row = self.rows.saturating_sub(1);
        let (row, col) = self.nav.cursor();
        if row > max_row {
            // `set_cursor`, not `move_to`: a clamp is bookkeeping, and letting
            // it count as deliberate movement would disarm line-select growth
            // every time the screen shrank under the cursor.
            self.nav.set_cursor((max_row, col));
        }
    }

    /// Move every stored viewport point by `delta` rows, clamped to the screen.
    fn translate_rows(&mut self, delta: i32) {
        if delta == 0 {
            return;
        }
        let rows = self.rows;
        self.nav
            .translate(|(row, col)| (shift_row(row, delta, rows), col));
    }

    /// Put the reader's cursor where the reader was working.
    ///
    /// Called on the way *out* of Insert, and the only place the cursor is
    /// placed rather than kept — while Insert is live the program's cursor is
    /// the truth and the reader's is parked at whatever the last clear left it
    /// on, which is the bottom row. `Esc` then handed back a keyboard and a
    /// cursor parked at the bottom of a mostly-empty screen, so the first `k`
    /// appeared to do nothing and the first `x` armed a line the reader could
    /// not see.
    ///
    /// **The rule: the last row that has content, or row 0 when nothing has.**
    /// A fixed "one row above the bottom" was rejected because the common case
    /// is a shell whose prompt sits on row 0 of a 24-row screen — the cursor
    /// would have been parked in blank space *below* the only line on screen,
    /// trading one wrong place for another. Anchoring on content instead puts
    /// it on the prompt line after a short command and on the last line of the
    /// block after a long one, which is where the reader was either way.
    ///
    /// `sync_viewport` first, so the row is measured against the offset the
    /// screen is actually parked at; `set_cursor` rather than `move_to`, because
    /// placement is bookkeeping and must not count as a deliberate movement
    /// that extends a `v` selection. An armed selection keeps its own anchor:
    /// the reader built it, and Esc from Insert is not a gesture against it.
    fn place_reader_cursor_after_insert(&mut self) {
        self.sync_viewport();
        let max_row = self.rows.saturating_sub(1);
        let row = (0..=max_row)
            .rev()
            .find(|row| !self.screen.row_text(*row).is_empty())
            .unwrap_or(0);
        self.nav.set_cursor((row, 0));
    }

    /// Current daemon-link state for the view.
    pub fn link_state(&self) -> &TerminalLinkState {
        &self.link
    }

    /// One-line status for dialog headers and `/terminal-list` output.
    pub fn status_line(&self) -> String {
        let attached = if self.attachment_id.is_some() {
            "attached"
        } else {
            "detached"
        };
        let read = match self.read.refused.as_deref() {
            Some(refusal) => format!(" read-stopped:{refusal}"),
            None if self.read.in_flight.is_some() => " reading".to_string(),
            None => String::new(),
        };
        format!(
            "{} [{}] {} {} {}x{} seq={}{} {}",
            self.handle,
            self.link.label(),
            attached,
            self.focus.label(),
            self.cols,
            self.rows,
            self.scrollback.next_seq,
            read,
            self.command,
        )
    }
}

/// Incremental terminal emulator state. `vt100` keeps escape interpretation
/// and screen memory separate from the host TUI, so untrusted terminal bytes
/// are never written to the real terminal device.
struct TerminalScreen(vt100::Parser);

impl std::fmt::Debug for TerminalScreen {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TerminalScreen(<bounded VT state>)")
    }
}

impl TerminalScreen {
    fn new(rows: u16, cols: u16) -> Self {
        Self(vt100::Parser::new(
            rows.clamp(1, MAX_TERMINAL_SCREEN_ROWS),
            cols.clamp(1, MAX_TERMINAL_SCREEN_COLS),
            TERMINAL_SCROLLBACK_LINES,
        ))
    }

    /// How many rows the viewport may scroll back.
    ///
    /// `Screen::scrollback()` reports the *current* offset, not how far back
    /// the history reaches, and the filled length of the underlying deque is
    /// not exposed — only its allocated capacity is. Parking the offset at
    /// `usize::MAX` and reading it back asks `set_scrollback` to clamp against
    /// the real length, which is the only way to learn the true maximum; the
    /// saved offset is restored so a read never disturbs the view.
    fn max_offset(&mut self) -> usize {
        let saved = self.0.screen().scrollback();
        self.0.screen_mut().set_scrollback(usize::MAX);
        let max = self.0.screen().scrollback();
        self.0.screen_mut().set_scrollback(saved);
        max
    }

    /// Park the viewport `offset` rows above the live bottom (0 = live).
    fn set_offset(&mut self, offset: usize) {
        self.0.screen_mut().set_scrollback(offset);
    }

    /// Cell text of one viewport row, trailing blanks trimmed.
    fn row_text(&self, row: u16) -> String {
        let (_, cols) = self.0.screen().size();
        self.row_range_text(row, 0, cols).trim_end().to_string()
    }

    /// Cell text of one viewport row restricted to columns `[from, to)`.
    ///
    /// `Screen::rows` takes a *column* start and a width, and yields every
    /// visible row — so the row is selected with `nth`, not by passing it as
    /// the start.
    fn row_range_text(&self, row: u16, from: u16, to: u16) -> String {
        let (_, cols) = self.0.screen().size();
        let to = to.min(cols);
        if from >= to {
            return String::new();
        }
        self.0
            .screen()
            .rows(from, to - from)
            .nth(usize::from(row))
            .unwrap_or_default()
    }

    /// Whether a viewport row is a continuation of the row above it.
    fn row_wrapped(&self, row: u16) -> bool {
        self.0.screen().row_wrapped(row)
    }

    /// Extract the selected text as clipboard-ready text.
    ///
    /// Honours row wrapping: a row that is a wrap continuation contributes no
    /// newline, so a selection spanning a wrapped command reads back as one
    /// line rather than as a stack of fragments. Line mode trims each row's
    /// trailing blanks and joins with `\n`.
    ///
    /// `redactions` are the columns `d` has already scrubbed. They are applied
    /// here as well as on the render path, because a yank is the one route by
    /// which deleted text could otherwise leave the view — and a deletion the
    /// user can copy straight back out is not a deletion.
    fn selection_text(
        &self,
        selection: &Selection<TerminalPoint>,
        redactions: &[Vec<(u16, u16)>],
    ) -> String {
        let (start, end) = selection.line_range();
        let (first, last) = selection.bounds();
        let cols = self.0.screen().size().1;
        let single_row = start == end;
        let mut out = String::new();
        for row in start..=end {
            // Line mode takes whole rows. Character mode takes the start row
            // from the first selected column to the end of the row, the end row
            // from column 0 through the last selected column, and everything
            // between in full — which is what makes a selection dragged down
            // two lines read back as whole middle lines instead of a diagonal.
            // Deriving both cuts from the *ordered bounds* rather than from the
            // anchor and head separately is what makes a backwards selection
            // read the same as a forwards one.
            let (from, to) = match selection.mode {
                SelectionMode::Line => (0, cols),
                SelectionMode::Char if single_row => (first.1, last.1.saturating_add(1)),
                SelectionMode::Char if row == start => (first.1, cols),
                SelectionMode::Char if row == end => (0, last.1.saturating_add(1)),
                SelectionMode::Char => (0, cols),
            };
            let text = self.row_range_text(row, from, to);
            let text = match redactions.get(usize::from(row)) {
                Some(ranges) if !ranges.is_empty() => Redactions::scrub(&text, ranges),
                _ => text,
            };
            // A wrapped row is a continuation of the row above, so it must not
            // be newline-separated; an unwrapped one is a real line boundary.
            if !out.is_empty() && row > start && !self.row_wrapped(row) {
                out.push('\n');
            }
            out.push_str(text.trim_end());
        }
        out
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.0.screen_mut().set_size(
            rows.clamp(1, MAX_TERMINAL_SCREEN_ROWS),
            cols.clamp(1, MAX_TERMINAL_SCREEN_COLS),
        );
    }

    fn reset(&mut self, rows: u16, cols: u16) {
        *self = Self::new(rows, cols);
    }

    fn process(&mut self, bytes: &[u8]) {
        self.0.process(bytes);
    }

    fn lines(&self, max_lines: usize) -> Vec<String> {
        let contents = self.0.screen().contents();
        let mut lines: Vec<_> = contents.lines().collect();
        if lines.len() > max_lines {
            lines.drain(..lines.len() - max_lines);
        }
        lines
            .into_iter()
            .map(|line| {
                let mut end = line.len().min(MAX_TERMINAL_LINE_BYTES);
                while !line.is_char_boundary(end) {
                    end -= 1;
                }
                if end < line.len() {
                    format!("{}... [line truncated]", &line[..end])
                } else {
                    line.to_string()
                }
            })
            .collect()
    }

    /// Visible rows in the emulator right now.
    fn row_count(&self) -> u16 {
        self.0.screen().size().0
    }

    /// Where the child process's own cursor is, in screen coordinates.
    ///
    /// This is the *program's* cursor — where the shell is accepting the next
    /// character — as opposed to the reader's selection cursor. In insert
    /// mode this is the cell that has to be highlighted: drawing the nav
    /// cursor there instead showed a block at the bottom-left of the output
    /// while the user typed at a prompt somewhere else entirely.
    fn program_cursor(&self) -> TerminalPoint {
        let (row, col) = self.0.screen().cursor_position();
        (row, col)
    }

    /// Columns in the emulator right now.
    fn col_count(&self) -> u16 {
        self.0.screen().size().1
    }

    /// Trimmed text of every visible row, in viewport order.
    ///
    /// The `d` overlay is keyed on row text, so it needs this snapshot to match
    /// remembered redactions against what is actually on screen.
    fn viewport_row_text(&self) -> Vec<String> {
        (0..self.row_count())
            .map(|row| self.row_text(row))
            .collect()
    }

    /// Render the visible viewport as styled lines, honouring the current
    /// scrollback offset plus any active selection, scrub and cursor.
    ///
    /// `selection` carries its own head, so one `contains` test drives the
    /// highlight whether the reader built the selection with `x` (where the
    /// head is pinned and the cursor is free to wander) or `v` (where the head
    /// tracks the cursor). `redactions` is the resolved `d` scrub, one entry
    /// per viewport row.
    ///
    /// `cursor_visible` is the *blink* phase, and it changes the modifier only
    /// — never the caret's position. The cell stays painted while the caret is
    /// dark, because a cursor that stops reserving its own cell would let the
    /// line trim back to its last character and then re-extend on the next
    /// phase: the text would appear to flicker instead of the caret.
    fn styled_lines(
        &self,
        max_lines: usize,
        selection: Option<Selection<TerminalPoint>>,
        redactions: &[Vec<(u16, u16)>],
        cursor: Option<TerminalPoint>,
        cursor_visible: bool,
        highlight: ratatui::style::Style,
    ) -> Vec<ratatui::text::Line<'static>> {
        use ratatui::style::{Modifier, Style};
        use ratatui::text::{Line, Span};

        let (rows, cols) = self.0.screen().size();
        let start = rows.saturating_sub(max_lines.min(usize::from(rows)) as u16);
        let mut lines = Vec::with_capacity(usize::from(rows.saturating_sub(start)));
        for row in start..rows {
            let mut last_visible = 0;
            for col in 0..cols {
                if let Some(cell) = self.0.screen().cell(row, col) {
                    if cell.has_contents()
                        || cell.bgcolor() != vt100::Color::Default
                        || cell.is_wide_continuation()
                    {
                        last_visible = col.saturating_add(1);
                    }
                }
            }
            // A fully line-selected row is highlighted across its whole width,
            // so a blank row inside the selection must still be painted.
            if let Some(sel) = selection {
                if sel.mode == SelectionMode::Line && sel.contains((row, 0)) {
                    last_visible = last_visible.max(cols);
                }
            }
            // The cursor has to be painted even when its cell is empty. A shell
            // prompt sits on a blank cell by definition — the cursor is the one
            // cell with nothing in it — so trimming trailing blanks and then
            // drawing the cursor "only where a cell matched" hid the caret
            // exactly when the user was typing.
            if let Some((cur_row, cur_col)) = cursor {
                if cur_row == row {
                    last_visible = last_visible.max(cur_col.saturating_add(1).min(cols));
                }
            }

            let scrubbed = redactions.get(usize::from(row));
            let mut spans = Vec::new();
            let mut active_style = Style::default();
            let mut active_text = String::new();
            for col in 0..last_visible {
                let Some(cell) = self.0.screen().cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                // A scrubbed cell renders as a blank with default styling: the
                // deletion has to read as *gone*, so it must not keep the
                // colours it happened to be printed in.
                if scrubbed
                    .is_some_and(|ranges| ranges.iter().any(|&(from, to)| col >= from && col < to))
                {
                    if !active_text.is_empty() && active_style != Style::default() {
                        spans.push(Span::styled(std::mem::take(&mut active_text), active_style));
                    }
                    active_style = Style::default();
                    active_text.push(' ');
                    continue;
                }
                let mut style = Style::default();
                if cell.fgcolor() != vt100::Color::Default {
                    style = style.fg(vt_color(cell.fgcolor()));
                }
                if cell.bgcolor() != vt100::Color::Default {
                    style = style.bg(vt_color(cell.bgcolor()));
                }
                let mut modifiers = Modifier::empty();
                if cell.bold() {
                    modifiers |= Modifier::BOLD;
                }
                if cell.dim() {
                    modifiers |= Modifier::DIM;
                }
                if cell.italic() {
                    modifiers |= Modifier::ITALIC;
                }
                if cell.underline() {
                    modifiers |= Modifier::UNDERLINED;
                }
                if cell.inverse() {
                    modifiers |= Modifier::REVERSED;
                }
                style = style.add_modifier(modifiers);
                // Selection paints a background, never a foreground: `theme.selection`
                // is a bg, and using it as fg renders dark-on-dark. Paired with the
                // theme foreground so selected body text stays readable.
                if selection.is_some_and(|sel| sel.contains((row, col))) {
                    if let Some(bg) = highlight.bg {
                        style = style.bg(bg);
                    }
                    if let Some(fg) = highlight.fg {
                        style = style.fg(fg);
                    }
                }
                if let Some((cur_row, cur_col)) = cursor {
                    if cur_row == row && cur_col == col && cursor_visible {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                }
                if !active_text.is_empty() && style != active_style {
                    spans.push(Span::styled(std::mem::take(&mut active_text), active_style));
                }
                active_style = style;
                if cell.has_contents() {
                    active_text.push_str(cell.contents());
                } else {
                    active_text.push(' ');
                }
            }
            if !active_text.is_empty() {
                spans.push(Span::styled(active_text, active_style));
            }
            lines.push(Line::from(spans));
        }
        lines
    }
}

fn vt_color(color: vt100::Color) -> ratatui::style::Color {
    match color {
        vt100::Color::Default => ratatui::style::Color::Reset,
        vt100::Color::Idx(index) => ratatui::style::Color::Indexed(index),
        vt100::Color::Rgb(red, green, blue) => ratatui::style::Color::Rgb(red, green, blue),
    }
}

/// Shift a stored viewport row by `delta`, clamping inside the screen.
///
/// Never panics on a negative intermediate: the cursor and the selection
/// anchor are `u16` and the scroll delta is `i32`, so an upward scroll near
/// row 0 is ordinary rather than exceptional.
fn shift_row(row: u16, delta: i32, rows: u16) -> u16 {
    let max = i32::from(rows.saturating_sub(1));
    (i32::from(row) + delta).clamp(0, max) as u16
}

/// Column of the last character of the next word, or `None` at end of line.
///
/// Helix `e`, landing *on* the final character rather than one past it. That
/// matters here more than it does in an editor: `v` then `e` is how a reader
/// grabs one word out of a build log, and a cursor parked on the following
/// space would copy the space with it every time.
///
/// Already sitting on a word's last character takes the *next* word, so
/// repeated `e` advances instead of stalling.
fn next_word_end(text: &str, from: u16) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut index = usize::from(from).min(chars.len());
    let at_word_end = index > 0
        && !chars[index].is_whitespace()
        && (index + 1 == chars.len() || chars[index + 1].is_whitespace());
    if at_word_end {
        index += 1;
    }
    while index < chars.len() && chars[index].is_whitespace() {
        index += 1;
    }
    if index >= chars.len() {
        return None;
    }
    while index + 1 < chars.len() && !chars[index + 1].is_whitespace() {
        index += 1;
    }
    Some(index)
}

/// Column of the first character of the previous word, or `None` at BOL.
///
/// Helix `b`, the mirror of [`next_word_end`]. Landing on a word's first
/// character steps back over the whitespace to the word *before* it, so
/// repeated `b` walks the line rather than stalling on the boundary it just
/// arrived at.
fn previous_word_start(text: &str, from: u16) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut index = usize::from(from).min(chars.len());
    if index == 0 || chars.is_empty() {
        return None;
    }
    if index == chars.len() {
        index -= 1;
    }
    let at_word_start =
        !chars[index].is_whitespace() && (index == 1 || chars[index - 1].is_whitespace());
    if at_word_start {
        let mut back = index;
        while back > 0 && chars[back - 1].is_whitespace() {
            back -= 1;
        }
        index = back;
    }
    while index > 0 && chars[index].is_whitespace() {
        index -= 1;
    }
    if index == 0 {
        return None;
    }
    while index > 0 && !chars[index - 1].is_whitespace() {
        index -= 1;
    }
    Some(index)
}

/// Reference TUI controller over M002 handles.
///
/// Owns a bounded set of [`InteractiveTerminalView`]s keyed by daemon
/// handle plus the currently active handle. All mutating operations are
/// applied from daemon answers; the async command layer (which owns the
/// `CoreClient`) translates controller intent into
/// `CoreRequest::InteractiveProcess*` calls and feeds the responses back
/// here.
#[derive(Debug, Default)]
pub struct InteractiveTerminalController {
    views: BTreeMap<String, InteractiveTerminalView>,
    active: Option<String>,
    /// Source of cadence read ids. Controller-wide so two terminals polling in
    /// the same frame cannot mint the same id.
    read_counter: u64,
}

impl InteractiveTerminalController {
    pub fn new() -> Self {
        Self {
            views: BTreeMap::new(),
            active: None,
            read_counter: 0,
        }
    }

    /// Number of tracked terminals.
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// Whether no terminal is tracked.
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// Currently active handle, if any.
    pub fn active_handle(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Read one view by handle.
    pub fn view(&self, handle: &str) -> Option<&InteractiveTerminalView> {
        self.views.get(handle)
    }

    /// Current daemon viewport associated with one view.
    pub fn dimensions(&self, handle: &str) -> Option<(u16, u16)> {
        self.views.get(handle).map(|view| (view.cols, view.rows))
    }

    /// Handles routed to one workspace, sorted. This is the
    /// multi-project routing primitive: the dialog layer shows only the
    /// active project's terminals.
    pub fn handles_for_workspace(&self, workspace_id: &str) -> Vec<String> {
        self.views
            .values()
            .filter(|view| view.workspace_id == workspace_id)
            .map(|view| view.handle.clone())
            .collect()
    }

    /// Whether `handle` is routed to `workspace_id`.
    pub fn workspace_matches(&self, handle: &str, workspace_id: &str) -> bool {
        self.views
            .get(handle)
            .is_some_and(|view| view.workspace_id == workspace_id)
    }

    /// Track a freshly created process (from `InteractiveProcessCreated`).
    ///
    /// Returns `false` (and tracks nothing) when the per-controller cap
    /// is reached or the handle is already tracked.
    pub fn apply_created(
        &mut self,
        handle: String,
        workspace_id: String,
        command: String,
        cols: u16,
        rows: u16,
    ) -> bool {
        if self.views.contains_key(&handle) || self.views.len() >= MAX_TERMINALS_PER_VIEW {
            return false;
        }
        let view = InteractiveTerminalView::new(handle.clone(), workspace_id, command, cols, rows);
        self.views.insert(handle.clone(), view);
        if self.active.is_none() {
            self.active = Some(handle);
        }
        true
    }

    /// Apply a metadata list snapshot (from `InteractiveProcessList`).
    ///
    /// Known handles keep their scrollback/attachments and refresh their
    /// size; unknown handles are registered up to the cap; handles absent
    /// from the list keep their views (the daemon list is a point-in-time
    /// snapshot, not a deletion signal — removal is explicit).
    pub fn apply_list(
        &mut self,
        processes: &[crate::protocol::interactive_process::InteractiveProcessMetadata],
    ) {
        for metadata in processes {
            if let Some(view) = self.views.get_mut(&metadata.handle) {
                view.cols = metadata.cols;
                view.rows = metadata.rows;
                let exited = metadata.state == "exited" || metadata.state == "terminated";
                if exited
                    && !view.link.is_exited()
                    && !matches!(view.link, TerminalLinkState::Gone { .. })
                {
                    view.link = TerminalLinkState::Exited {
                        exit_code: metadata.exit_code,
                        exit_signal: metadata.exit_signal,
                    };
                }
            } else if self.views.len() < MAX_TERMINALS_PER_VIEW {
                let mut view = InteractiveTerminalView::new(
                    metadata.handle.clone(),
                    metadata.workspace_id.clone(),
                    metadata.command.clone(),
                    metadata.cols,
                    metadata.rows,
                );
                if metadata.state == "exited" || metadata.state == "terminated" {
                    view.link = TerminalLinkState::Exited {
                        exit_code: metadata.exit_code,
                        exit_signal: metadata.exit_signal,
                    };
                }
                self.views.insert(metadata.handle.clone(), view);
            }
        }
        if self
            .active
            .as_ref()
            .is_none_or(|h| !self.views.contains_key(h))
        {
            self.active = self.views.keys().next().cloned();
        }
    }

    /// Apply a successful attach (from `InteractiveProcessAttached`).
    ///
    /// Decodes `chunk.data_b64` (invalid base64 is reported as an error
    /// string, never panicked on) and projects the bytes into the bounded
    /// scrollback. A `resync` on the attach response moves the link to
    /// [`TerminalLinkState::ResyncRequired`]; otherwise the link is
    /// [`TerminalLinkState::Live`] with the new attachment recorded.
    pub fn apply_attached(
        &mut self,
        handle: &str,
        attachment_id: String,
        chunk: &InteractiveOutputChunk,
        resync: Option<&InteractiveResync>,
    ) -> Result<Option<String>, String> {
        let raw = decode_chunk(chunk)?;
        let view = self
            .views
            .get_mut(handle)
            .ok_or_else(|| format!("unknown terminal handle: {handle}"))?;
        view.attachment_id = Some(attachment_id);
        // A fresh attachment is a fresh read: the slot is re-armed and any
        // refusal recorded against the previous one no longer describes this
        // link.
        view.read = TerminalReadState {
            attachment_id: None,
            in_flight: None,
            refused: None,
        };
        if view.scrollback.apply_chunk(chunk, &raw) {
            view.screen.reset(view.rows, view.cols);
            view.screen_degraded = true;
        }
        view.screen.process(&raw);
        let scroll_notice = view.scrollback.take_notice();
        if let Some(resync) = resync {
            view.link = TerminalLinkState::ResyncRequired(resync.clone());
            let reason = resync_reason_label(&resync.reason);
            return Ok(Some(format!(
                "{handle}: {reason}; base={} next={}{}",
                resync.base_seq,
                resync.next_seq,
                scroll_notice.map(|n| format!(" ({n})")).unwrap_or_default()
            )));
        }
        if !matches!(view.link, TerminalLinkState::Live) {
            view.link = TerminalLinkState::Live;
        }
        self.active = Some(handle.to_string());
        Ok(scroll_notice)
    }

    /// Apply a successful resume (from `InteractiveProcessResumed`).
    pub fn apply_resumed(
        &mut self,
        handle: &str,
        chunk: &InteractiveOutputChunk,
    ) -> Result<Option<String>, String> {
        let raw = decode_chunk(chunk)?;
        let view = self
            .views
            .get_mut(handle)
            .ok_or_else(|| format!("unknown terminal handle: {handle}"))?;
        if view.scrollback.apply_chunk(chunk, &raw) {
            view.screen.reset(view.rows, view.cols);
            view.screen_degraded = true;
        }
        view.screen.process(&raw);
        let notice = view.scrollback.take_notice();
        if matches!(view.link, TerminalLinkState::ResyncRequired(_)) {
            view.link = TerminalLinkState::Live;
        }
        Ok(notice)
    }

    /// Apply a typed resync answer (from `InteractiveProcessResyncRequired`
    /// or a resync-carrying attach).
    pub fn apply_resync(&mut self, handle: &str, resync: InteractiveResync) {
        if let Some(view) = self.views.get_mut(handle) {
            if resync.reason == InteractiveResyncReason::HandleGone {
                view.link = TerminalLinkState::Gone {
                    reason: "handle gone: removed or daemon restarted".to_string(),
                };
                view.attachment_id = None;
                // A gone handle has no attachment to resume. The cadence stops
                // on its own here rather than by being refused one poll later,
                // which is the same outcome with one fewer doomed round trip.
                view.read = TerminalReadState::default();
            } else {
                view.screen.reset(view.rows, view.cols);
                view.screen_degraded = true;
                view.link = TerminalLinkState::ResyncRequired(resync);
            }
        }
    }

    /// Apply a detach answer: the attachment is released, the process is
    /// unaffected, and scrollback is retained.
    pub fn apply_detached(&mut self, handle: &str) {
        if let Some(view) = self.views.get_mut(handle) {
            view.attachment_id = None;
            view.focus = TerminalFocus::Viewing;
            // The attachment a read was issued against is gone, so its
            // completion is superseded whatever id it carries. Clearing the
            // slot stops a stale answer from landing on the next attachment.
            view.read = TerminalReadState::default();
        }
    }

    /// Apply a terminate answer (from `InteractiveProcessTerminated`).
    ///
    /// Post-mortem scrollback stays readable; the attachment is retained
    /// so remaining output can still be resumed per the M002 contract;
    /// input is rejected from here on.
    pub fn apply_terminated(
        &mut self,
        handle: &str,
        exit_code: Option<i32>,
        exit_signal: Option<i32>,
    ) {
        if let Some(view) = self.views.get_mut(handle) {
            view.link = TerminalLinkState::Exited {
                exit_code,
                exit_signal,
            };
            view.focus = TerminalFocus::Viewing;
            view.pending_input.clear();
            view.pending_resize = None;
            // An exited process has no attachment to resume; releasing the slot
            // keeps one outstanding read from looking live forever.
            view.read = TerminalReadState::default();
        }
    }

    /// Apply the daemon exit event (`CoreEvent::InteractiveProcessExited`).
    ///
    /// Carries no output bytes: remaining scrollback is picked up through
    /// the attach/resume operations, exactly as the M002 event contract
    /// requires.
    pub fn apply_exited_event(
        &mut self,
        handle: &str,
        exit_code: Option<i32>,
        exit_signal: Option<i32>,
    ) {
        self.apply_terminated(handle, exit_code, exit_signal);
    }

    /// Apply a remove answer: the view is dropped, freeing scrollback.
    ///
    /// Returns `true` when a view was removed.
    pub fn apply_removed(&mut self, handle: &str) -> bool {
        let removed = self.views.remove(handle).is_some();
        if removed && self.active.as_deref() == Some(handle) {
            self.active = self.views.keys().next().cloned();
        }
        removed
    }

    /// Note a transport disconnect (M002 `handle_disconnect` semantics).
    ///
    /// Attachments are dropped (the daemon releases them server-side) but
    /// processes are unaffected: live views move to
    /// [`TerminalLinkState::Reconnecting`] with scrollback retained so a
    /// later re-attach resumes from the last cursor. Exited and gone
    /// views are untouched.
    pub fn note_transport_disconnect(&mut self) {
        for view in self.views.values_mut() {
            view.attachment_id = None;
            view.focus = TerminalFocus::Viewing;
            view.pending_input.clear();
            view.pending_resize = None;
            view.read = TerminalReadState::default();
            if matches!(view.link, TerminalLinkState::Live)
                || matches!(view.link, TerminalLinkState::ResyncRequired(_))
            {
                view.link = TerminalLinkState::Reconnecting;
            }
        }
    }

    /// Note a daemon restart: ephemeral handles are invalidated (M002
    /// restart fixture), so every non-exited view moves to
    /// [`TerminalLinkState::Gone`] with scrollback retained for reading.
    pub fn note_daemon_restart(&mut self) {
        for view in self.views.values_mut() {
            view.attachment_id = None;
            view.focus = TerminalFocus::Viewing;
            view.pending_input.clear();
            view.pending_resize = None;
            view.read = TerminalReadState::default();
            if !view.link.is_exited() {
                view.link = TerminalLinkState::Gone {
                    reason: "daemon restarted: ephemeral handles invalidated".to_string(),
                };
            }
        }
    }

    /// Whether `handle` still has a process that can take input.
    ///
    /// Distinct from [`Self::focus`] succeeding, which additionally demands a
    /// live attachment. The two failure modes need opposite answers — a
    /// detached process is re-attached, an exited one is replaced — so callers
    /// that are recovering need to tell them apart.
    pub fn is_live(&self, handle: &str) -> bool {
        self.views
            .get(handle)
            .is_some_and(|view| view.link.allows_mutation())
    }

    /// Make `handle` the active view (viewing, not focused).
    pub fn set_active(&mut self, handle: &str) -> bool {
        if self.views.contains_key(handle) {
            self.active = Some(handle.to_string());
            return true;
        }
        false
    }

    /// Move a view to explicit keyboard focus. Returns `false` when the
    /// handle is unknown or the link is not live.
    pub fn focus(&mut self, handle: &str) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        if !view.link.allows_mutation() || view.attachment_id.is_none() {
            return false;
        }
        for (id, other) in self.views.iter_mut() {
            other.focus = if id == handle {
                TerminalFocus::Focused
            } else {
                TerminalFocus::Hidden
            };
        }
        self.active = Some(handle.to_string());
        true
    }

    /// Leave keyboard focus on `handle` (back to viewing). Never forwards
    /// bytes and never submits anything: this is the escape path.
    ///
    /// Leaving Insert is the one moment the reader's cursor is placed rather
    /// than merely kept, and it is placed here so both surfaces get it from
    /// one call. See [`TerminalView::place_reader_cursor_after_insert`].
    pub fn escape(&mut self, handle: &str) {
        if let Some(view) = self.views.get_mut(handle) {
            if view.focus == TerminalFocus::Focused {
                view.focus = TerminalFocus::Viewing;
                view.place_reader_cursor_after_insert();
            }
        }
        // The sweep stays a second, separate pass and deliberately never
        // touches `nav`. A hidden view becoming visible is not a gesture the
        // reader made, so it must not move a cursor they did not move — and
        // folding this into the block above is how one gesture's placement ends
        // up being applied to every view on screen.
        for other in self.views.values_mut() {
            if other.focus == TerminalFocus::Hidden {
                other.focus = TerminalFocus::Viewing;
            }
        }
    }

    /// Route one key for the focused `handle`.
    ///
    /// The single entry point both surfaces call, so the box and the panel
    /// cannot disagree about what a key means.
    pub fn classify_focused(&self, handle: &str, key: TerminalKey) -> TerminalKeyAction {
        let Some(view) = self.views.get(handle) else {
            return TerminalKeyAction::Ignored;
        };
        classify_key(view.focus == TerminalFocus::Focused, key)
    }

    /// Whether `handle` currently owns the keyboard.
    pub fn is_focused(&self, handle: &str) -> bool {
        self.views
            .get(handle)
            .is_some_and(|view| view.focus == TerminalFocus::Focused)
    }

    /// Drop the scrollback window and reset the emulator for `handle`.
    ///
    /// The clear a user asks for is **local**: the daemon keeps streaming
    /// from wherever it was, and the PTY keeps running. So `next_seq` is
    /// deliberately preserved — resetting it would make the very next honest
    /// chunk fail the gap check and print a "history expired" notice the user
    /// did not cause and cannot act on. A clear must not manufacture a
    /// resync event.
    ///
    /// Returns `false` for an unknown handle so a stale keypress cannot
    /// silently do nothing.
    pub fn clear_scrollback(&mut self, handle: &str) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        view.scrollback.bytes.clear();
        view.scrollback.truncated = false;
        view.scrollback.notice = None;
        view.screen.reset(view.rows, view.cols);
        view.screen_degraded = false;
        // The emulator was rebuilt, so every coordinate the reader was holding
        // — cursor, selection anchor, scroll offset — now names different text
        // or nothing. Resetting the viewport and dropping the selection is the
        // only truthful option: keeping them would silently move a yank to
        // whatever happens to occupy those cells on the fresh screen.
        view.viewport_offset = 0;
        view.nav = SelectionState::new((view.rows.saturating_sub(1), 0));
        // The scrub overlay is keyed on row text, and the text it was keyed on
        // is gone. Carrying it forward would hide whichever new output happens
        // to reuse the deleted line's wording.
        view.redactions.clear();
        view.pending_prefix = false;
        true
    }

    /// Queue bounded input for a live terminal.
    ///
    /// Rejects input after exit, while gone/reconnecting/resyncing, for
    /// unknown handles, and over the per-write bound. Rapid keystrokes
    /// coalesce in the pending buffer; the command layer drains it with
    /// [`Self::take_pending_input`] into bounded M002 input operations.
    pub fn queue_input(&mut self, handle: &str, bytes: &[u8]) -> Result<(), TerminalInputError> {
        let view = self
            .views
            .get_mut(handle)
            .ok_or(TerminalInputError::UnknownHandle)?;
        if !view.link.allows_mutation() {
            return Err(TerminalInputError::NotLive(view.link.label()));
        }
        if view.attachment_id.is_none() {
            return Err(TerminalInputError::NotLive("detached"));
        }
        if bytes.len() > MAX_TERMINAL_INPUT_BYTES {
            return Err(TerminalInputError::TooLarge {
                len: bytes.len(),
                max: MAX_TERMINAL_INPUT_BYTES,
            });
        }
        if view.pending_input.len() + bytes.len() > MAX_TERMINAL_INPUT_BYTES {
            return Err(TerminalInputError::TooLarge {
                len: view.pending_input.len() + bytes.len(),
                max: MAX_TERMINAL_INPUT_BYTES,
            });
        }
        view.pending_input.extend_from_slice(bytes);
        Ok(())
    }

    /// Drain coalesced pending input for one terminal.
    pub fn take_pending_input(&mut self, handle: &str) -> Option<Vec<u8>> {
        let view = self.views.get_mut(handle)?;
        if view.pending_input.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut view.pending_input))
    }

    /// Claim the single input slot for `handle`.
    ///
    /// Returns the attachment to write to, or `None` when an input operation
    /// is already outstanding. Input is ordered: every keystroke the user
    /// typed between two ticks becomes one operation, and the next one waits
    /// for this one to land. Without this, each keystroke spawned its own
    /// concurrent daemon request, so fast typing arrived **scrambled** —
    /// `ls -la` could reach the shell as `lsa- l`. That is the same silent
    /// character loss as the resize storm above, and it is not something the
    /// user can see the cause of.
    pub fn begin_input(&mut self, handle: &str) -> Option<String> {
        let view = self.views.get_mut(handle)?;
        if view.input_in_flight || !view.link.allows_mutation() {
            return None;
        }
        let attachment_id = view.attachment_id.clone()?;
        view.input_in_flight = true;
        Some(attachment_id)
    }

    /// Release the single input slot for `handle`.
    pub fn finish_input(&mut self, handle: &str) {
        if let Some(view) = self.views.get_mut(handle) {
            view.input_in_flight = false;
        }
    }

    /// Whether an input operation is outstanding for `handle`.
    pub fn input_in_flight(&self, handle: &str) -> bool {
        self.views
            .get(handle)
            .is_some_and(|view| view.input_in_flight)
    }

    /// Queue a resize for a live terminal. Rapid resizes coalesce:
    /// only the newest size is kept until drained.
    pub fn queue_resize(
        &mut self,
        handle: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), TerminalResizeError> {
        if cols == 0
            || rows == 0
            || cols > MAX_TERMINAL_SCREEN_COLS
            || rows > MAX_TERMINAL_SCREEN_ROWS
        {
            return Err(TerminalResizeError::InvalidSize { cols, rows });
        }
        let view = self
            .views
            .get_mut(handle)
            .ok_or(TerminalResizeError::UnknownHandle)?;
        if !view.link.allows_mutation() {
            return Err(TerminalResizeError::NotLive(view.link.label()));
        }
        // Resizing to the size you are already at is a no-op, and it must be a
        // *client-side* no-op: the render path queues the panel's geometry on
        // every frame, so queueing unconditionally turns a steady surface into
        // a resize storm. A PTY resize flushes the tty's pending input, which
        // drops the half-typed word the user is in the middle of — the keystroke
        // is lost with no error anywhere, and the shell looks like it is eating
        // characters. Coalescing here is what makes the geometry track the panel
        // without standing between the user and their own typing.
        if view.pending_resize == Some((cols, rows)) || (view.cols, view.rows) == (cols, rows) {
            return Ok(());
        }
        view.pending_resize = Some((cols, rows));
        Ok(())
    }

    /// Drain the coalesced pending resize for one terminal.
    pub fn take_pending_resize(&mut self, handle: &str) -> Option<(u16, u16)> {
        self.views.get_mut(handle)?.pending_resize.take()
    }

    /// Record that a resize was applied by the daemon.
    pub fn applied_resize(&mut self, handle: &str, cols: u16, rows: u16) {
        if let Some(view) = self.views.get_mut(handle) {
            view.cols = cols;
            view.rows = rows;
            view.screen.resize(rows, cols);
        }
    }

    // ── Viewport and Helix-style selection ─────────────────────────────
    //
    // Navigation state lives on the controller rather than on either surface,
    // so the box and the docked panel cannot disagree about where the cursor
    // is or what is selected. Nothing here forwards to the PTY: these are
    // read/navigation verbs only.

    /// Rows the viewport is parked above the live bottom; 0 means following.
    pub fn viewport_offset(&self, handle: &str) -> Option<usize> {
        self.views.get(handle).map(|view| view.viewport_offset)
    }

    /// Whether a `Space` prefix is armed awaiting its second key.
    pub fn prefix_pending(&self, handle: &str) -> bool {
        self.views
            .get(handle)
            .is_some_and(|view| view.pending_prefix)
    }

    /// Arm or disarm the `space` prefix.
    pub fn set_prefix_pending(&mut self, handle: &str, pending: bool) -> bool {
        match self.views.get_mut(handle) {
            Some(view) => {
                view.pending_prefix = pending;
                true
            }
            None => false,
        }
    }

    /// Scroll by `delta` rows; positive walks toward older output.
    ///
    /// The cursor and any selection anchor ride along with the content rather
    /// than staying pinned to a row, so a selection survives scrolling.
    pub fn scroll_by(&mut self, handle: &str, delta: i32) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        view.sync_viewport();
        let max = i64::try_from(view.screen.max_offset()).unwrap_or(0);
        let current = i64::try_from(view.viewport_offset).unwrap_or(0);
        let next = (current + i64::from(delta)).clamp(0, max);
        let shift = i32::try_from(next - current).unwrap_or(0);
        view.viewport_offset = usize::try_from(next).unwrap_or(0);
        view.screen.set_offset(view.viewport_offset);
        if shift != 0 {
            // Scrolling toward older output moves the content *down*, so every
            // stored point increases by the same amount. Riding along with the
            // content is what keeps a cursor and a selection pointing at the
            // same text instead of at the same cells.
            view.translate_rows(shift);
        }
        true
    }

    /// Return the viewport to the live bottom.
    pub fn scroll_to_bottom(&mut self, handle: &str) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        view.sync_viewport();
        let shift = -(i32::try_from(view.viewport_offset).unwrap_or(0));
        view.viewport_offset = 0;
        view.screen.set_offset(0);
        if shift != 0 {
            view.translate_rows(shift);
        }
        true
    }

    /// Page the viewport by whole viewport heights.
    pub fn page_by(&mut self, handle: &str, direction: i32) -> bool {
        let Some(rows) = self.views.get(handle).map(|view| view.rows) else {
            return false;
        };
        let step = i32::from(rows.max(1));
        self.scroll_by(handle, direction.saturating_mul(step))
    }

    /// Move the Helix cursor, scrolling the viewport when it hits an edge.
    ///
    /// The move goes through [`SelectionState::move_to`], which is the one
    /// place that knows a movement extends a character selection but must not
    /// extend a line one.
    pub fn move_cursor(&mut self, handle: &str, drow: i32, dcol: i32) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        view.sync_viewport();
        let (row, col) = view.nav.cursor();
        let mut next_row = row;
        let mut scroll = 0i32;
        if drow != 0 {
            let next = i32::from(row) + drow;
            let max_row = i32::from(view.rows.saturating_sub(1));
            if !(0..=max_row).contains(&next) {
                next_row = next.clamp(0, max_row) as u16;
                // Walking off an edge scrolls the viewport by the *opposite*
                // sign: `k` past the top row must reveal older output, which is
                // a *larger* scrollback offset, not a smaller one. Copying the
                // movement sign here pinned the viewport at offset 0 and made
                // `k` do nothing exactly where scrolling matters most.
                scroll = -drow;
            } else {
                next_row = next as u16;
            }
        }
        let mut next_col = col;
        if dcol != 0 {
            let width = view.screen.row_text(next_row).chars().count() as i32;
            next_col = (i32::from(col) + dcol).clamp(0, width) as u16;
        }
        view.nav.move_to((next_row, next_col));
        if scroll != 0 {
            // The scroll translated the selection along with the content, which
            // is right; only the cursor needs re-pinning to the edge it walked
            // off, so that edge now names the line the scroll just revealed
            // instead of bouncing back to row 1 and stalling the viewport there.
            let edge = view.nav.cursor();
            self.scroll_by(handle, scroll);
            let Some(view) = self.views.get_mut(handle) else {
                return false;
            };
            view.nav.repin_cursor(edge);
        }
        true
    }

    /// Helix `e` / `b`: jump the cursor to the next/previous word boundary.
    ///
    /// A deliberate movement like any other, so it extends a character
    /// selection and disarms line-select growth — which is the whole reason
    /// the shared state machine owns the "does this extend?" rule rather than
    /// each motion deciding for itself.
    pub fn word_motion(&mut self, handle: &str, forward: bool) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        view.sync_viewport();
        let (row, col) = view.nav.cursor();
        let text = view.screen.row_text(row);
        let target = if forward {
            next_word_end(&text, col)
        } else {
            previous_word_start(&text, col)
        };
        let Some(target) = target else {
            return false;
        };
        view.nav
            .move_to((row, u16::try_from(target).unwrap_or(col)));
        true
    }

    /// Enter a selection mode anchored at the cursor.
    pub fn begin_selection(&mut self, handle: &str, mode: SelectionMode) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        view.sync_viewport();
        match mode {
            SelectionMode::Char => view.nav.select_char(),
            SelectionMode::Line => view.nav.select_line(),
        }
        true
    }

    /// Helix `v`: toggle character selection at the cursor.
    pub fn toggle_char_selection(&mut self, handle: &str) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        view.sync_viewport();
        view.nav.toggle_char();
        true
    }

    /// Helix `x`: select the cursor's whole line, and grow by one more line on
    /// every further press.
    ///
    /// Growth moves the *head* one row down, not the selection by whatever the
    /// cursor happens to be sitting on. That distinction is the whole feature:
    /// "press `x` again for the next line" and "press `x` on the line I moved
    /// to" are the same key, and only the shared state machine's `growing`
    /// flag can tell them apart.
    pub fn toggle_line_selection(&mut self, handle: &str) -> bool {
        {
            let Some(view) = self.views.get_mut(handle) else {
                return false;
            };
            view.sync_viewport();
            if !view.nav.line_select_extends() {
                view.nav.select_line();
                return true;
            }
        }
        let Some((head_row, col, max_row, offset)) = self.views.get(handle).map(|view| {
            (
                view.nav
                    .selection()
                    .map_or_else(|| view.nav.cursor().0, |selection| selection.head.0),
                view.nav.cursor().1,
                view.rows.saturating_sub(1),
                view.viewport_offset,
            )
        }) else {
            return false;
        };
        // At the last visible row there is nothing below to select until the
        // viewport reveals it. Scrolling toward *newer* output translates every
        // stored point up by one, so the growth below lands on the row that was
        // just revealed. At offset 0 there is no newer output, so growth stops.
        if head_row >= max_row && offset > 0 {
            self.scroll_by(handle, -1);
        }
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        let head_row = view
            .nav
            .selection()
            .map_or_else(|| view.nav.cursor().0, |selection| selection.head.0);
        let next = head_row.saturating_add(1).min(max_row);
        // `set_cursor` deliberately does not move the head — growth below is
        // the only thing that may extend a line selection.
        view.nav.set_cursor((next, col));
        view.nav.grow_to((next, col));
        true
    }

    /// Helix `d`: delete the held selection from this view.
    ///
    /// The VT emulator is read-only, so "delete" means the selected cells stop
    /// being shown and stop being copyable — it does not touch the daemon-side
    /// process or its real scrollback. Rows are remembered by their text so a
    /// deletion stays attached to the line it was applied to instead of
    /// drifting onto whatever scrolls into the same cells later.
    ///
    /// Returns whether anything was deleted; a bare `d` with no selection is
    /// a no-op, which is why it is still consumed by the caller — an unclaimed
    /// `d` would fall through to the global `OpenDiff` binding.
    pub fn delete_selection(&mut self, handle: &str) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        view.sync_viewport();
        let Some(selection) = view.nav.selection() else {
            return false;
        };
        let (start, end) = selection.line_range();
        let (first, last) = selection.bounds();
        let cols = view.screen.col_count();
        let whole_line = matches!(selection.mode, SelectionMode::Line);
        for row in start..=end {
            let text = view.screen.row_text(row);
            if text.is_empty() {
                continue;
            }
            let width = u16::try_from(text.chars().count()).unwrap_or(cols);
            let from = if whole_line || row != start {
                0
            } else {
                first.1
            };
            let to = if whole_line || row != end {
                width
            } else {
                last.1.saturating_add(1)
            };
            view.redactions.push(text, from, to);
        }
        // `d` is an operator, not a mode: it consumes the selection it acted on.
        view.nav.deactivate();
        true
    }

    /// Drop any active selection and disarm the `space` prefix.
    ///
    /// Deliberately leaves the scrub overlay alone: cancelling a selection is
    /// not an undo, and a reader who deleted three lines then mis-selected
    /// should not get them back.
    pub fn cancel_selection(&mut self, handle: &str) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        view.nav.deactivate();
        view.pending_prefix = false;
        true
    }

    /// Resolve the `d` scrub overlay against the rows currently in view.
    fn resolved_redactions(view: &InteractiveTerminalView) -> Vec<Vec<(u16, u16)>> {
        if view.redactions.is_empty() {
            return Vec::new();
        }
        view.redactions.resolve(&view.screen.viewport_row_text())
    }

    /// Selected text, ready for the system clipboard.
    pub fn selected_text(&mut self, handle: &str) -> Option<String> {
        let view = self.views.get_mut(handle)?;
        view.sync_viewport();
        let selection = view.nav.selection()?;
        let redactions = Self::resolved_redactions(view);
        let text = view.screen.selection_text(&selection, &redactions);
        (!text.is_empty()).then_some(text)
    }

    /// Active selection, for renderers.
    pub fn selection(&self, handle: &str) -> Option<Selection<TerminalPoint>> {
        self.views.get(handle)?.nav.selection()
    }

    /// Cursor position in viewport coordinates.
    pub fn cursor(&self, handle: &str) -> Option<TerminalPoint> {
        self.views.get(handle).map(|view| view.nav.cursor())
    }

    /// How many column ranges `d` has scrubbed from this view.
    pub fn redaction_count(&self, handle: &str) -> usize {
        self.views
            .get(handle)
            .map_or(0, |view| view.redactions.entries.len())
    }

    /// Render the visible viewport with selection highlight, scrub and cursor
    /// applied.
    ///
    /// The cursor is mode-dependent, and this is the whole difference between
    /// "the terminal is where I type" reading as true and reading as a claim:
    ///
    /// - **INS** draws the *program's* cursor, the cell the child process is
    ///   accepting the next character at.
    /// - **NOR** draws the *reader's* cursor, which `hjkl`/`v`/`x` move and
    ///   select with.
    ///
    /// Both cells exist at once; showing the wrong one is what made an
    /// otherwise-correct focus flag look like a lie.
    ///
    /// `caret_blink_on` is the phase from the one frame clock, and it is
    /// consulted **only** in INS. The reader's cursor is a selection anchor, so
    /// a NOR surface ignores the phase entirely and keeps a solid cursor: a
    /// blinking anchor makes `x` and `d` guesswork about which line is armed.
    pub fn render_view(
        &mut self,
        handle: &str,
        max_lines: usize,
        caret_blink_on: bool,
        highlight: ratatui::style::Style,
    ) -> Option<Vec<ratatui::text::Line<'static>>> {
        let view = self.views.get_mut(handle)?;
        view.sync_viewport();
        let selection = view.nav.selection();
        let redactions = Self::resolved_redactions(view);
        let cursor = match view.focus {
            TerminalFocus::Focused => Some(view.screen.program_cursor()),
            _ => Some(view.nav.cursor()),
        };
        let caret_visible = !matches!(view.focus, TerminalFocus::Focused) || caret_blink_on;
        Some(view.screen.styled_lines(
            max_lines,
            selection,
            &redactions,
            cursor,
            caret_visible,
            highlight,
        ))
    }

    /// Render the newest lines from the bounded VT screen for one view.
    /// Escape sequences have already been interpreted and are never returned
    /// to the host TUI. Returns `None` for unknown handles.
    pub fn render_lines(&self, handle: &str, max_lines: usize) -> Option<Vec<String>> {
        let view = self.views.get(handle)?;
        let max_lines = max_lines.min(TERMINAL_VIEW_LINES);
        Some(view.screen.lines(max_lines))
    }

    /// Render interpreted screen cells with their safe SGR styles applied.
    ///
    /// Honours the parked viewport offset but draws no selection highlight;
    /// prefer [`Self::render_view`] for anything a human is looking at.
    pub fn render_styled_lines(
        &mut self,
        handle: &str,
        max_lines: usize,
    ) -> Option<Vec<ratatui::text::Line<'static>>> {
        self.render_view(
            handle,
            max_lines,
            // No clock in this path: it is the read-only projection API, not a
            // surface a user is typing into, so the caret is simply present.
            true,
            ratatui::style::Style::default(),
        )
    }

    /// Header lines describing one terminal for the dialog view, never
    /// including raw terminal output.
    ///
    /// The compact form is the default: one line naming the command and the
    /// thing a reader actually needs to know (is it live, is it taking my
    /// keys). The full diagnostics dump stays one `D` away because it is what
    /// a bug report needs, not what an everyday shell needs.
    pub fn header_lines(&self, handle: &str) -> Option<Vec<String>> {
        let view = self.views.get(handle)?;
        if view.diagnostics {
            return self.full_header_lines(handle);
        }
        let mut lines = vec![format!(
            "{} · {}{}",
            view.command,
            view.link.label(),
            match view.focus {
                TerminalFocus::Focused => " · INS typing (Esc reads, Ctrl+] sends esc)",
                TerminalFocus::Viewing =>
                    " · NOR reading (hjkl, v/x select, space+y copy, i types)",
                TerminalFocus::Hidden => "",
            }
        )];
        // Anything the reader must not miss stays visible even when the dump
        // is folded away: a degraded screen, a resync, or an exit all change
        // what the rows on screen mean.
        if view.screen_degraded {
            lines.push(
                "VT display reset after an output gap; current screen may be incomplete"
                    .to_string(),
            );
        }
        match &view.link {
            TerminalLinkState::ResyncRequired(resync) => lines.push(format!(
                "resync needed ({}) — use /terminal-resume to follow live output",
                resync_reason_label(&resync.reason)
            )),
            TerminalLinkState::Exited {
                exit_code,
                exit_signal,
            } => lines.push(format!(
                "exited (code={} signal={}); scrollback is readable, input is rejected",
                exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "-".into()),
                exit_signal
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".into())
            )),
            TerminalLinkState::Gone { reason } => lines.push(format!("gone: {reason}")),
            TerminalLinkState::Live | TerminalLinkState::Reconnecting => {}
        }
        // A live link with no attachment reads as a live session on screen and
        // is not one: nothing is reading the PTY, so no output can arrive. Say
        // so rather than leaving an empty box that looks merely quiet, and
        // keep the recovery on the same line as the diagnosis.
        if view.attachment_id.is_none() && !view.link.is_exited() {
            lines.push(format!(
                "no attachment — output is not being read; use /terminal-attach {}",
                view.handle
            ));
        }
        // A typed daemon refusal that stopped the cadence, verbatim. The class
        // is the useful part, so it is never flattened into a generic message.
        if let Some(refusal) = view.read.refused.as_deref() {
            lines.push(format!(
                "live output stopped: {refusal} (re-attach, or /terminal-resume to retry)"
            ));
        }
        Some(lines)
    }

    /// Toggle the diagnostics dump for one terminal (`D`).
    pub fn toggle_diagnostics(&mut self, handle: &str) -> bool {
        match self.views.get_mut(handle) {
            Some(view) => {
                view.diagnostics = !view.diagnostics;
                true
            }
            None => false,
        }
    }

    /// Whether one terminal is currently printing its full diagnostics dump.
    pub fn diagnostics_visible(&self, handle: &str) -> bool {
        self.views.get(handle).is_some_and(|v| v.diagnostics)
    }

    /// The full six-line diagnostics dump: handles, cursors, and every
    /// resync/exit detail. Reachable with `D`; never the default.
    fn full_header_lines(&self, handle: &str) -> Option<Vec<String>> {
        let view = self.views.get(handle)?;
        let mut lines = vec![
            format!("handle:      {}", view.handle),
            format!("workspace:   {}", view.workspace_id),
            format!("command:     {}", view.command),
            format!(
                "state:       {} ({})",
                view.link.label(),
                if view.attachment_id.is_some() {
                    format!(
                        "attachment {}",
                        view.attachment_id.as_deref().unwrap_or("?")
                    )
                } else {
                    "no attachment".to_string()
                }
            ),
            format!(
                "focus:       {} (Esc leaves focus; input needs explicit focus)",
                match view.focus {
                    TerminalFocus::Hidden => "hidden",
                    TerminalFocus::Viewing => "viewing",
                    TerminalFocus::Focused => "FOCUSED",
                }
            ),
            format!(
                "size:        {}x{} | seq: {} (retained {} bytes{})",
                view.cols,
                view.rows,
                view.scrollback.next_seq,
                view.scrollback.bytes.len(),
                if view.scrollback.truncated {
                    ", truncated"
                } else {
                    ""
                }
            ),
        ];
        if view.screen_degraded {
            lines.push(
                "VT display reset after an output gap; current screen may be incomplete"
                    .to_string(),
            );
        }
        match &view.link {
            TerminalLinkState::ResyncRequired(resync) => {
                lines.push(format!(
                    "resync:      {} (base={} next={}); use /terminal-resume to follow live output",
                    resync_reason_label(&resync.reason),
                    resync.base_seq,
                    resync.next_seq
                ));
            }
            TerminalLinkState::Exited {
                exit_code,
                exit_signal,
            } => {
                lines.push(format!(
                    "exit:        code={} signal={} (input rejected; scrollback readable)",
                    exit_code
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    exit_signal
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "-".to_string())
                ));
            }
            TerminalLinkState::Gone { reason } => {
                lines.push(format!("gone:        {reason}"));
            }
            TerminalLinkState::Reconnecting => {
                lines.push(
                    "reconnect:   transport dropped; re-attach resumes from the last cursor if the process lives".to_string(),
                );
            }
            TerminalLinkState::Live => {}
        }
        Some(lines)
    }

    /// One-line summaries for every tracked terminal (for `/terminal-list`).
    pub fn list_lines(&self) -> Vec<String> {
        self.views.values().map(|view| view.status_line()).collect()
    }

    /// Cursor to resume from for `handle` (the view's `next_seq`).
    pub fn resume_cursor(&self, handle: &str) -> Option<u64> {
        self.views.get(handle).map(|view| view.scrollback.next_seq)
    }

    /// Attachment currently owned for `handle`, if any.
    pub fn attachment_id(&self, handle: &str) -> Option<String> {
        self.views
            .get(handle)
            .and_then(|view| view.attachment_id.clone())
    }

    // ── Cadence output read ───────────────────────────────────────────
    //
    // The read is bounded by construction: it is an
    // `InteractiveProcessResume` at an explicit cursor. There is no
    // "read everything" entry point here, no second byte cap, and no path
    // that shifts history silently — a cursor the daemon no longer retains
    // comes back as its own typed resync and is applied as one.

    /// Mint the next read id.
    ///
    /// Monotonic and never zero: `0` is what a caller that has no read to
    /// identify passes, and a wrapped counter must not be able to produce it.
    pub fn next_read_id(&mut self) -> u64 {
        self.read_counter = self.read_counter.wrapping_add(1).max(1);
        self.read_counter
    }

    /// Whether a cadence read is already outstanding for `handle`.
    pub fn output_read_in_flight(&self, handle: &str) -> bool {
        self.views
            .get(handle)
            .is_some_and(|view| view.read.in_flight.is_some())
    }

    /// Claim the read slot for `handle` and return what to resume.
    ///
    /// Returns `None` — issuing nothing — when the view is unknown, has no
    /// attachment, is not [`TerminalLinkState::Live`], has been refused by the
    /// daemon (see [`Self::note_read_refusal`]), or already has a read
    /// outstanding. One read per attachment at a time is what keeps a cadence
    /// poll from racing itself.
    pub fn begin_output_read(&mut self, handle: &str, read_id: u64) -> Option<TerminalOutputRead> {
        let view = self.views.get_mut(handle)?;
        if view.read.in_flight.is_some() || view.read.refused.is_some() {
            return None;
        }
        let attachment_id = view.attachment_id.clone()?;
        if !matches!(view.link, TerminalLinkState::Live) {
            return None;
        }
        let read = TerminalOutputRead {
            attachment_id: attachment_id.clone(),
            from_seq: view.scrollback.next_seq,
            read_id,
        };
        view.read.in_flight = Some(read_id);
        view.read.attachment_id = Some(attachment_id);
        Some(read)
    }

    /// Release the read slot for `handle`, reporting whether the completion
    /// still owns it.
    ///
    /// `false` means the read was superseded: a different read holds the slot,
    /// or the attachment changed under it. The caller must then discard the
    /// answer — applying it would mix one attachment's output into another.
    /// The slot is cleared either way that it owns, so a lost completion can
    /// never pin the cadence off permanently.
    pub fn finish_output_read(&mut self, handle: &str, read_id: u64, attachment_id: &str) -> bool {
        let Some(view) = self.views.get_mut(handle) else {
            return false;
        };
        let owned = view.read.in_flight == Some(read_id)
            && view.read.attachment_id.as_deref() == Some(attachment_id);
        if owned {
            view.read.in_flight = None;
        }
        owned
    }

    /// Record a typed daemon refusal against `handle` and stop the cadence.
    ///
    /// A pull that keeps re-issuing against a process that has exited, or a
    /// handle that is gone, is the silent retry loop this state exists to end:
    /// the error is kept verbatim — code and message — so the surface can
    /// report the class it actually got, and the cadence stays off until a
    /// user-driven action re-arms it.
    pub fn note_read_refusal(&mut self, handle: &str, error: String) {
        if let Some(view) = self.views.get_mut(handle) {
            view.read.refused = Some(error);
        }
    }

    /// Re-arm a refused cadence read.
    ///
    /// Only reached from user-driven recovery — a fresh attach, or the
    /// documented `/terminal-resume`. The cadence never re-arms itself,
    /// because a self-re-arming refusal is the retry loop.
    pub fn clear_read_refusal(&mut self, handle: &str) {
        if let Some(view) = self.views.get_mut(handle) {
            view.read.refused = None;
        }
    }

    /// The typed refusal currently stopping this view's cadence, if any.
    pub fn read_refusal(&self, handle: &str) -> Option<&str> {
        self.views
            .get(handle)
            .and_then(|view| view.read.refused.as_deref())
    }
}

/// One bounded output read the cadence is permitted to issue.
///
/// `from_seq` is the view's own cursor, so the read is always incremental from
/// what is already on screen. It is not an adjustable knob: the cadence cannot
/// widen its own window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalOutputRead {
    /// The caller-owned attachment to resume.
    pub attachment_id: String,
    /// Explicit resume cursor.
    pub from_seq: u64,
    /// This read's identity, for matching its completion.
    pub read_id: u64,
}

/// Per-attachment output-read bookkeeping.
///
/// Deliberately **not** `dialog_state.terminal_request`: that is one shared
/// slot behind a single generation counter for all nine terminal operations, so
/// a cadence read that called `begin()` there would supersede an in-flight
/// input, resize, detach or terminate completion and drop it on the floor.
/// Keeping the read's identity here makes "a background read must not
/// invalidate a user action" structural rather than a matter of timing.
#[derive(Debug, Clone, Default)]
struct TerminalReadState {
    /// Attachment this slot is currently reading. A completion whose
    /// attachment no longer matches is superseded whatever its id.
    attachment_id: Option<String>,
    /// Id of the outstanding read, if any.
    in_flight: Option<u64>,
    /// Typed daemon refusal that stops the cadence until a user-driven action
    /// re-arms it.
    refused: Option<String>,
}

fn decode_chunk(chunk: &InteractiveOutputChunk) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    B64.decode(&chunk.data_b64)
        .map_err(|error| format!("invalid base64 in output chunk: {error}"))
}

fn resync_reason_label(reason: &InteractiveResyncReason) -> &'static str {
    match reason {
        InteractiveResyncReason::HistoryExpired => "history expired",
        InteractiveResyncReason::CursorAhead => "cursor ahead",
        InteractiveResyncReason::HandleGone => "handle gone",
        InteractiveResyncReason::VersionMismatch => "version mismatch",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::interactive_process::{
        InteractiveOutputChunk, InteractiveProcessMetadata, InteractiveResync,
        InteractiveResyncReason,
    };

    fn chunk(from_seq: u64, next_seq: u64, gap: bool, text: &str) -> InteractiveOutputChunk {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
        InteractiveOutputChunk {
            handle: "h".to_string(),
            from_seq,
            next_seq,
            gap,
            data_b64: B64.encode(text.as_bytes()),
        }
    }

    fn metadata(handle: &str, state: &str) -> InteractiveProcessMetadata {
        InteractiveProcessMetadata {
            handle: handle.to_string(),
            workspace_id: "ws-1".to_string(),
            command: "sh".to_string(),
            state: state.to_string(),
            cols: 80,
            rows: 24,
            child_pid: None,
            next_seq: 0,
            retained_bytes: 0,
            total_bytes: 0,
            truncated: false,
            exit_code: None,
            exit_signal: None,
        }
    }

    fn live_attached() -> InteractiveTerminalController {
        let mut controller = InteractiveTerminalController::new();
        assert!(controller.apply_created(
            "h".to_string(),
            "ws-1".to_string(),
            "cat".to_string(),
            80,
            24
        ));
        let body = chunk(0, 3, false, "hello\n");
        assert!(controller
            .apply_attached("h", "a-1".to_string(), &body, None)
            .expect("attach")
            .is_none());
        controller
    }

    #[test]
    fn create_is_capped_like_the_daemon_attachment_bound() {
        let mut controller = InteractiveTerminalController::new();
        for index in 0..MAX_TERMINALS_PER_VIEW {
            assert!(
                controller.apply_created(
                    format!("h-{index}"),
                    "ws-1".to_string(),
                    "sh".to_string(),
                    80,
                    24
                ),
                "should accept {index}"
            );
        }
        assert!(!controller.apply_created(
            "h-overflow".to_string(),
            "ws-1".to_string(),
            "sh".to_string(),
            80,
            24
        ));
        // Duplicates never alias.
        assert!(!controller.apply_created(
            "h-0".to_string(),
            "ws-1".to_string(),
            "sh".to_string(),
            80,
            24
        ));
    }

    #[test]
    fn unfocused_keys_never_reach_the_terminal() {
        for key in [
            TerminalKey::Char('x'),
            TerminalKey::Enter,
            TerminalKey::Backspace,
            TerminalKey::CtrlC,
            TerminalKey::Esc,
        ] {
            assert_eq!(
                classify_key(false, key),
                TerminalKeyAction::Ignored,
                "unfocused {key:?} must belong to the prompt"
            );
        }
    }

    /// `Esc` is the mode key and nothing else. It used to be a prefix: one
    /// `Esc` went to the program, a second one left. That made the answer to
    /// "what does this key do" depend on a hidden latch, which is why the
    /// surface read as unusable — there was no way to tell, from what you
    /// pressed, where your keystrokes were going.
    #[test]
    fn escape_leaves_focus_and_never_forwards_or_submits() {
        assert_eq!(
            classify_key(true, TerminalKey::Esc),
            TerminalKeyAction::EscapeFocus
        );
        // The mode key escapes without forwarding bytes.
        let action = classify_key(true, TerminalKey::Esc);
        assert!(!matches!(action, TerminalKeyAction::Forward(_)));
    }

    /// The child still gets a real escape, on its own key.
    ///
    /// vim, less and tmux readline need `0x1b`, so the capability cannot just
    /// be dropped — `Ctrl+]` is the chord that carries it, and it keeps the
    /// user in insert rather than leaving.
    #[test]
    fn literal_escape_is_forwarded_on_its_own_key() {
        assert_eq!(
            classify_key(true, TerminalKey::LiteralEscape),
            TerminalKeyAction::Forward(vec![0x1b])
        );
        assert_ne!(
            classify_key(true, TerminalKey::LiteralEscape),
            TerminalKeyAction::EscapeFocus,
            "the child's escape must not double as the mode key"
        );
    }

    /// One `Esc` leaves insert on any keyboard.
    ///
    /// This replaces the `Esc Esc` rule, whose only motivation was that
    /// `Ctrl-]` is unreachable on a US Mac keyboard. `Esc` is on every
    /// keyboard, so making it the mode key gives everyone a one-press exit and
    /// removes the need for the latch entirely.
    #[test]
    fn a_single_escape_leaves_focus_on_any_keyboard() {
        let mut controller = live_attached();
        let handle = "h";
        assert!(controller.focus(handle));
        assert_eq!(
            controller.classify_focused(handle, TerminalKey::Esc),
            TerminalKeyAction::EscapeFocus
        );
        controller.escape(handle);
        assert!(!controller.is_focused(handle));
        // And pressing it again does not re-arm anything: unfocused, it is
        // simply not ours.
        assert_eq!(
            controller.classify_focused(handle, TerminalKey::Esc),
            TerminalKeyAction::Ignored
        );
    }

    /// Typing must not be affected by any prior escape.
    ///
    /// The old classifier cleared a latch on every printable key; the whole
    /// point of removing it is that ordinary input no longer has to be checked
    /// against a remembered flag at all.
    #[test]
    fn typing_between_escapes_is_unaffected() {
        let mut controller = live_attached();
        let handle = "h";
        assert!(controller.focus(handle));
        assert_eq!(
            controller.classify_focused(handle, TerminalKey::Esc),
            TerminalKeyAction::EscapeFocus
        );
        controller.escape(handle);
        assert!(controller.focus(handle));
        for (key, want) in [
            (
                TerminalKey::Char('j'),
                TerminalKeyAction::Forward(vec![b'j']),
            ),
            (TerminalKey::Esc, TerminalKeyAction::EscapeFocus),
        ] {
            assert_eq!(controller.classify_focused(handle, key), want, "{key:?}");
            if matches!(want, TerminalKeyAction::EscapeFocus) {
                controller.escape(handle);
                assert!(controller.focus(handle));
            }
        }
    }

    /// The header is the only place the exit is advertised, and `Esc` is now
    /// that exit — so the box must say so.
    #[test]
    fn the_focused_header_advertises_a_typeable_exit() {
        let mut controller = live_attached();
        controller.focus("h");
        let header = controller.header_lines("h").expect("header");
        assert!(
            header[0].contains("Esc"),
            "the box must show how to get back to reading: {header:?}"
        );
    }

    #[test]
    fn focused_printable_and_control_keys_forward_exact_bytes() {
        assert_eq!(
            classify_key(true, TerminalKey::Char('a')),
            TerminalKeyAction::Forward(vec![b'a'])
        );
        assert_eq!(
            classify_key(true, TerminalKey::Enter),
            TerminalKeyAction::Forward(vec![b'\r'])
        );
        assert_eq!(
            classify_key(true, TerminalKey::Backspace),
            TerminalKeyAction::Forward(vec![0x7f])
        );
        assert_eq!(
            classify_key(true, TerminalKey::Tab),
            TerminalKeyAction::Forward(vec![b'\t'])
        );
        assert_eq!(
            classify_key(true, TerminalKey::CtrlC),
            TerminalKeyAction::Forward(vec![0x03])
        );
        assert_eq!(
            classify_key(true, TerminalKey::CtrlD),
            TerminalKeyAction::Forward(vec![0x04])
        );
        assert_eq!(
            classify_key(true, TerminalKey::Up),
            TerminalKeyAction::Forward(b"\x1b[A".to_vec())
        );
        assert_eq!(
            classify_key(true, TerminalKey::Other),
            TerminalKeyAction::Ignored
        );
    }

    #[test]
    fn vt_screen_handles_split_cursor_clear_sgr_and_alternate_screen() {
        let mut screen = TerminalScreen::new(3, 12);
        let transcript = b"old\rnew\x1b[2J\x1b[H\x1b[31mred";
        for byte in transcript {
            screen.process(std::slice::from_ref(byte));
        }
        let cell = screen.0.screen().cell(0, 0).expect("red cell");
        assert_eq!(cell.fgcolor(), vt100::Color::Idx(1));
        screen.process(b"\x1b[?1049halt");
        let lines = screen.lines(3);
        assert!(lines[0].starts_with("alt"), "screen: {lines:?}");
        screen.process(b"\x1b[?1049l");
        assert!(screen.lines(3)[0].starts_with("red"));
    }

    #[test]
    fn vt_screen_discards_osc_payload_and_clamps_dimensions() {
        let mut screen = TerminalScreen::new(u16::MAX, u16::MAX);
        assert_eq!(
            screen.0.screen().size(),
            (MAX_TERMINAL_SCREEN_ROWS, MAX_TERMINAL_SCREEN_COLS)
        );
        screen.process(b"\x1b]52;c;clipboard-secret\x07safe");
        let rendered = screen.lines(2).join("\n");
        assert!(rendered.contains("safe"));
        assert!(!rendered.contains("clipboard-secret"));
        assert!(!rendered.contains('\x1b'));
    }

    #[test]
    fn history_gap_resets_vt_state_and_labels_degraded_screen() {
        let mut controller = InteractiveTerminalController::new();
        controller.apply_created("h".into(), "ws".into(), "sh".into(), 20, 3);
        controller
            .apply_attached("h", "a".into(), &chunk(0, 3, false, "old"), None)
            .unwrap();
        controller
            .apply_resumed("h", &chunk(20, 23, true, "new"))
            .unwrap();
        let rendered = controller.render_lines("h", 3).unwrap().join("\n");
        assert!(rendered.contains("new"));
        assert!(!rendered.contains("old"));
        assert!(controller
            .header_lines("h")
            .unwrap()
            .iter()
            .any(|line| line.contains("may be incomplete")));
    }

    #[test]
    fn focus_requires_a_live_attachment() {
        let mut controller = InteractiveTerminalController::new();
        controller.apply_created(
            "h".to_string(),
            "ws-1".to_string(),
            "sh".to_string(),
            80,
            24,
        );
        // No attachment yet: focus is refused so keys cannot be misrouted.
        assert!(!controller.focus("h"));
        assert!(!controller.focus("missing"));
    }

    #[test]
    fn focus_and_escape_cycle_without_submitting() {
        let mut controller = live_attached();
        assert!(controller.focus("h"));
        assert!(controller.is_focused("h"));
        controller.escape("h");
        assert!(!controller.is_focused("h"));
        // Escaping twice is harmless.
        controller.escape("h");
        assert!(!controller.is_focused("h"));
    }

    #[test]
    fn the_box_leads_with_the_command_not_a_handle_dump() {
        let controller = live_attached();
        let header = controller.header_lines("h").expect("header");

        assert!(
            !header[0].contains("handle:"),
            "the default header is what a reader sees first; the UUID dump is not it: {header:?}"
        );
        assert!(
            header[0].contains("cat"),
            "the command is what identifies the terminal to a human: {header:?}"
        );
        assert!(
            header[0].contains("live"),
            "link state stays visible without the dump: {header:?}"
        );
        assert!(
            header.len() <= 2,
            "the compact header is one line plus at most a warning: {header:?}"
        );
    }

    #[test]
    fn diagnostics_stay_one_key_away_and_are_lossless() {
        let mut controller = live_attached();
        assert!(!controller.diagnostics_visible("h"));
        assert!(controller.toggle_diagnostics("h"));
        assert!(controller.diagnostics_visible("h"));

        let full = controller.header_lines("h").expect("header");
        assert!(
            full.iter().any(|line| line.contains("handle:")),
            "the dump is what a bug report needs, so it must still carry the handle"
        );
        assert!(full.iter().any(|line| line.contains("workspace:")));
        assert!(full.iter().any(|line| line.contains("focus:")));

        assert!(controller.toggle_diagnostics("h"));
        assert!(!controller.diagnostics_visible("h"));
        assert!(!controller.toggle_diagnostics("missing"));
    }

    #[test]
    fn an_exited_terminal_says_so_even_with_the_dump_folded() {
        let mut controller = live_attached();
        controller.apply_terminated("h", Some(0), None);
        let header = controller.header_lines("h").expect("header");
        assert!(
            header.iter().any(|line| line.contains("exited")),
            "input is rejected after exit; hiding that behind a toggle is how a dead terminal reads as live: {header:?}"
        );
    }

    #[test]
    fn input_after_exit_is_rejected() {
        let mut controller = live_attached();
        controller.apply_terminated("h", Some(0), None);
        assert_eq!(
            controller.queue_input("h", b"ls\n"),
            Err(TerminalInputError::NotLive("exited"))
        );
        assert!(controller.take_pending_input("h").is_none());
    }

    #[test]
    fn oversized_input_is_rejected_before_side_effects() {
        let mut controller = live_attached();
        let big = vec![b'x'; MAX_TERMINAL_INPUT_BYTES + 1];
        assert!(matches!(
            controller.queue_input("h", &big),
            Err(TerminalInputError::TooLarge { .. })
        ));
        assert!(controller.take_pending_input("h").is_none());
        assert_eq!(
            controller.queue_input("h", b"echo hi\n"),
            Ok(()),
            "bounded input still accepted"
        );
    }

    #[test]
    fn rapid_input_coalesces_into_one_drain() {
        let mut controller = live_attached();
        controller.queue_input("h", b"echo ").expect("first");
        controller.queue_input("h", b"hi\n").expect("second");
        assert_eq!(
            controller.take_pending_input("h").as_deref(),
            Some(b"echo hi\n".as_slice())
        );
        assert!(controller.take_pending_input("h").is_none());
    }

    #[test]
    fn resize_validates_and_coalesces_last_wins() {
        let mut controller = live_attached();
        assert!(controller.queue_resize("h", 0, 24).is_err());
        assert!(controller
            .queue_resize("h", MAX_TERMINAL_SCREEN_COLS + 1, 24)
            .is_err());
        controller.queue_resize("h", 80, 24).expect("first");
        controller.queue_resize("h", 120, 40).expect("second");
        assert_eq!(controller.take_pending_resize("h"), Some((120, 40)));
        assert!(controller.take_pending_resize("h").is_none());
        controller.applied_resize("h", 120, 40);
        assert_eq!(controller.view("h").expect("view").cols, 120);
    }

    #[test]
    fn scrollback_is_bounded_and_gap_replaces_with_notice() {
        let mut controller = live_attached();
        let big = "x".repeat(TERMINAL_VIEW_BYTES + 1024);
        let body = chunk(3, 3 + big.len() as u64, false, &big);
        let notice = controller
            .apply_resumed("h", &body)
            .expect("resume applies");
        assert!(notice.is_none(), "continuous stream has no notice");
        let lines = controller.render_lines("h", 10).expect("render");
        assert!(!lines.is_empty());
        assert!(lines.join("\n").len() <= TERMINAL_VIEW_BYTES + 1024);

        // A gapped chunk replaces the window and reports history expiry.
        let gapped = chunk(0, 999, true, "newest\n");
        let notice = controller
            .apply_resumed("h", &gapped)
            .expect("gapped resume applies");
        assert!(
            notice.is_some_and(|n| n.contains("history expired")),
            "gap must surface a notice"
        );
        let lines = controller.render_lines("h", 10).expect("render");
        assert!(lines.join("\n").contains("newest"));
    }

    #[test]
    fn resync_exit_and_gone_are_typed() {
        let mut controller = live_attached();
        controller.apply_resync(
            "h",
            InteractiveResync {
                reason: InteractiveResyncReason::HistoryExpired,
                handle: "h".to_string(),
                base_seq: 10,
                next_seq: 20,
                snapshot: None,
            },
        );
        assert!(matches!(
            controller.view("h").expect("view").link,
            TerminalLinkState::ResyncRequired(_)
        ));
        assert!(controller.queue_input("h", b"x").is_err());
        let headers = controller.header_lines("h").expect("headers");
        assert!(headers.join("\n").contains("history expired"));

        controller.apply_exited_event("h", Some(1), None);
        assert!(controller.view("h").expect("view").link.is_exited());
        let headers = controller.header_lines("h").expect("headers");
        assert!(headers.join("\n").contains("code=1"));

        controller.apply_resync(
            "h",
            InteractiveResync {
                reason: InteractiveResyncReason::HandleGone,
                handle: "h".to_string(),
                base_seq: 0,
                next_seq: 0,
                snapshot: None,
            },
        );
        assert!(matches!(
            controller.view("h").expect("view").link,
            TerminalLinkState::Gone { .. }
        ));
    }

    #[test]
    fn disconnect_keeps_scrollback_and_permits_reattach() {
        let mut controller = live_attached();
        controller.note_transport_disconnect();
        let view = controller.view("h").expect("view");
        assert!(matches!(view.link, TerminalLinkState::Reconnecting));
        assert!(view.attachment_id.is_none());
        // Scrollback survives the disconnect.
        assert!(controller
            .render_lines("h", 10)
            .expect("render")
            .join("\n")
            .contains("hello"));
        assert!(controller.queue_input("h", b"x").is_err());

        // Re-attach resumes from the retained cursor.
        assert_eq!(controller.resume_cursor("h"), Some(3));
        let body = chunk(3, 6, false, "back\n");
        assert!(controller
            .apply_attached("h", "a-2".to_string(), &body, None)
            .expect("reattach")
            .is_none());
        assert!(matches!(
            controller.view("h").expect("view").link,
            TerminalLinkState::Live
        ));
    }

    #[test]
    fn restart_invalidates_ephemeral_handles_but_keeps_exited_state() {
        let mut controller = live_attached();
        controller.apply_created(
            "dead".to_string(),
            "ws-1".to_string(),
            "true".to_string(),
            80,
            24,
        );
        controller.apply_terminated("dead", Some(0), None);
        controller.note_daemon_restart();
        assert!(matches!(
            controller.view("h").expect("view").link,
            TerminalLinkState::Gone { .. }
        ));
        assert!(controller.view("dead").expect("view").link.is_exited());
    }

    #[test]
    fn workspace_routing_selects_only_the_active_project() {
        let mut controller = InteractiveTerminalController::new();
        controller.apply_created(
            "h-1".to_string(),
            "ws-1".to_string(),
            "sh".to_string(),
            80,
            24,
        );
        controller.apply_created(
            "h-2".to_string(),
            "ws-2".to_string(),
            "sh".to_string(),
            80,
            24,
        );
        assert_eq!(
            controller.handles_for_workspace("ws-1"),
            vec!["h-1".to_string()]
        );
        assert!(controller.workspace_matches("h-2", "ws-2"));
        assert!(!controller.workspace_matches("h-2", "ws-1"));
        assert!(!controller.workspace_matches("missing", "ws-1"));
    }

    #[test]
    fn list_snapshot_registers_unknown_handles_and_marks_exits() {
        let mut controller = InteractiveTerminalController::new();
        controller.apply_list(&[metadata("h-9", "running"), metadata("h-10", "exited")]);
        assert_eq!(controller.len(), 2);
        assert!(controller.view("h-10").expect("view").link.is_exited());
        // Re-listing is idempotent and never duplicates handles.
        controller.apply_list(&[metadata("h-9", "running")]);
        assert_eq!(controller.len(), 2);
    }

    #[test]
    fn detach_releases_attachment_without_killing_or_clearing_output() {
        let mut controller = live_attached();
        assert!(controller.focus("h"));
        controller.apply_detached("h");
        let view = controller.view("h").expect("view");
        assert!(view.attachment_id.is_none());
        assert!(!controller.is_focused("h"));
        assert!(controller
            .render_lines("h", 10)
            .expect("render")
            .join("\n")
            .contains("hello"));
    }

    #[test]
    fn remove_frees_the_view_and_advances_the_active_handle() {
        let mut controller = InteractiveTerminalController::new();
        controller.apply_created(
            "h-1".to_string(),
            "ws-1".to_string(),
            "a".to_string(),
            80,
            24,
        );
        controller.apply_created(
            "h-2".to_string(),
            "ws-1".to_string(),
            "b".to_string(),
            80,
            24,
        );
        assert_eq!(controller.active_handle(), Some("h-1"));
        assert!(controller.apply_removed("h-1"));
        assert_eq!(controller.active_handle(), Some("h-2"));
        assert!(!controller.apply_removed("h-1"));
    }

    #[test]
    fn headers_carry_no_raw_terminal_bytes() {
        let controller = live_attached();
        let headers = controller.header_lines("h").expect("headers").join("\n");
        assert!(
            !headers.contains("hello"),
            "headers describe; they never carry output"
        );
    }

    // ── Scrollback, viewport, and Helix selection ────────────────────

    /// A controller whose live screen holds `lines`, one per row.
    ///
    /// Lines are joined CRLF with **no trailing newline**, so the cursor comes to
    /// rest on the last line and the visible rows are exactly `lines` in order —
    /// row index `n` is `lines[n]`. Callers that want history pass more lines than
    /// the screen has rows; the extra ones become retained scrollback.
    ///
    /// Tests below index rows against this rather than counting CRLFs, because a
    /// trailing newline would scroll the screen by one and silently shift every
    /// row index by one.
    fn scrolled_history(rows: u16, lines: &[&str]) -> InteractiveTerminalController {
        let mut controller = InteractiveTerminalController::new();
        assert!(controller.apply_created(
            "h".to_string(),
            "ws-1".to_string(),
            "cat".to_string(),
            40,
            rows
        ));
        let text = lines.join("\r\n");
        let body = chunk(0, text.len() as u64, false, &text);
        assert!(controller
            .apply_attached("h", "a-1".to_string(), &body, None)
            .expect("attach")
            .is_none());
        controller
    }

    #[test]
    fn scrollback_is_retained_and_reachable_by_scrolling_back() {
        let rows = 5;
        let lines: Vec<String> = (0..40).map(|n| format!("page-{n:02}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut controller = scrolled_history(rows, &refs);

        // Following the tail: only the newest page-one lines are on screen.
        let tail = controller.render_lines("h", 50).expect("render").join("\n");
        assert!(tail.contains("page-39"));
        assert!(
            !tail.contains("page-00"),
            "with zero scrollback the oldest output is gone"
        );

        // Scrolling back brings the aged-out rows back into reach. This is the
        // behaviour the previous zero-length scrollback made impossible.
        assert!(controller.scroll_by("h", 100));
        assert!(controller
            .viewport_offset("h")
            .is_some_and(|offset| offset > 0));
        let history = controller
            .render_lines("h", 200)
            .expect("render")
            .join("\n");
        assert!(
            history.contains("page-00"),
            "scrolled-back viewport must expose retained scrollback"
        );

        // And the offset is bounded: scrolling past the end stops at the top
        // of the retained history rather than running away.
        let settled = controller.viewport_offset("h").expect("offset");
        controller.scroll_by("h", 10_000);
        assert_eq!(controller.viewport_offset("h"), Some(settled));

        assert!(controller.scroll_to_bottom("h"));
        assert_eq!(controller.viewport_offset("h"), Some(0));
    }

    #[test]
    fn scroll_moves_the_cursor_and_anchor_with_the_content() {
        let rows = 8;
        let lines: Vec<String> = (0..40).map(|n| format!("line-{n:02}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut controller = scrolled_history(rows, &refs);
        // Park the cursor well above the bottom so a downward shift has room.
        controller.move_cursor("h", -3, 0);
        controller.begin_selection("h", SelectionMode::Char);
        let text_before = controller.selected_text("h");
        let cursor_before = controller.cursor("h").expect("cursor");
        let anchor_before = controller.selection("h").expect("selection").anchor;

        // Scrolling up moves the content down, so both rows increase — the
        // selection stays on the same text instead of drifting.
        controller.scroll_by("h", 2);
        let selection_after = controller.selection("h").expect("selection");
        let cursor_after = controller.cursor("h").expect("cursor");
        assert_eq!(cursor_after.0, cursor_before.0 + 2);
        assert_eq!(selection_after.anchor.0, anchor_before.0 + 2);
        assert_eq!(
            controller.selected_text("h"),
            text_before,
            "the selection must still cover the same text after scrolling"
        );
    }

    #[test]
    fn character_selection_spans_and_reads_back_the_right_text() {
        let rows = 6;
        let lines = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
        let mut controller = scrolled_history(rows, &lines);
        // Row 4 is "echo", row 5 is "foxtrot"; the cursor starts on row 5.
        controller.move_cursor("h", -1, 0); // row 4 = "echo"
        assert!(controller.begin_selection("h", SelectionMode::Char));
        controller.move_cursor("h", 0, 3); // head col 3 on "echo"
        controller.move_cursor("h", 1, 0); // head row 5

        // The anchor is a whole-cell boundary, so the row it starts on is taken
        // from its first column: (row 4, col 0) → (row 5, col 3) covers all of
        // "echo" and the first four characters of "foxtrot".
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("echo\nfoxt"),
            "char selection runs anchor→head inclusively, taking the anchor row whole"
        );

        // Dragging back up shortens the selection rather than reordering it.
        controller.move_cursor("h", -1, 0);
        assert_eq!(controller.selected_text("h").as_deref(), Some("echo"));
    }

    #[test]
    fn line_selection_takes_whole_lines_and_grows_one_per_press() {
        let rows = 6;
        let lines = ["one", "two", "three", "four", "five", "six"];
        let mut controller = scrolled_history(rows, &lines);
        // Cursor starts on the bottom row (5 = "six"); row 3 is "four".
        controller.move_cursor("h", -2, 0); // row 3 = "four"

        assert!(controller.toggle_line_selection("h"));
        assert_eq!(controller.selected_text("h").as_deref(), Some("four"));

        // Pressing `x` again extends the selection by exactly one line, which
        // is what makes "hit x repeatedly to take the next line" work.
        assert!(controller.toggle_line_selection("h"));
        assert_eq!(controller.selected_text("h").as_deref(), Some("four\nfive"));

        // The third press reaches the bottom screen row, which still holds a real
        // line, so it is included. A fourth has nowhere left to go.
        assert!(controller.toggle_line_selection("h"));
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("four\nfive\nsix")
        );
        assert!(controller.toggle_line_selection("h"));
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("four\nfive\nsix"),
            "growth stops at the last visible row"
        );
    }

    #[test]
    fn line_selection_reanchors_after_the_cursor_moves() {
        let rows = 6;
        let lines = ["one", "two", "three", "four", "five", "six"];
        let mut controller = scrolled_history(rows, &lines);
        controller.move_cursor("h", -4, 0); // row 1 = "two"
        assert!(controller.toggle_line_selection("h"));
        assert_eq!(controller.selected_text("h").as_deref(), Some("two"));

        // Moving the cursor ends growth, so the next `x` re-anchors on the line
        // the reader moved to rather than extending the old selection.
        controller.move_cursor("h", 1, 0); // row 2 = "three"
        assert!(controller.toggle_line_selection("h"));
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("three"),
            "x after a move selects the cursor's line, not a grown range"
        );

        // And growth resumes from there.
        assert!(controller.toggle_line_selection("h"));
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("three\nfour")
        );
    }

    #[test]
    fn line_selection_ignores_column_so_whole_line_is_copied() {
        let rows = 4;
        let lines = ["  indented", "tail", "x", "y"];
        let mut controller = scrolled_history(rows, &lines);
        // Cursor starts on row 3 ("y"); row 0 is the indented line.
        controller.move_cursor("h", -3, 0); // row 0 = "  indented"
        controller.move_cursor("h", 0, 7); // mid-line
        assert!(controller.begin_selection("h", SelectionMode::Line));
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("  indented"),
            "the cursor column must not narrow a whole-line selection"
        );

        controller.move_cursor("h", 0, 7); // past the end of the line
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("  indented"),
            "a column clamped to the line end must not change the selection"
        );
    }

    #[test]
    fn selection_is_dropped_by_cancel_and_by_clear() {
        let rows = 5;
        let lines = ["a", "b", "c", "d", "e"];
        let mut controller = scrolled_history(rows, &lines);
        controller.begin_selection("h", SelectionMode::Line);
        assert!(controller.selection("h").is_some());
        assert!(controller.cancel_selection("h"));
        assert!(controller.selection("h").is_none());
        assert!(controller.selected_text("h").is_none());

        controller.toggle_line_selection("h");
        assert!(controller.clear_scrollback("h"));
        assert!(
            controller.selection("h").is_none(),
            "clear rebuilds the screen, so a held selection would name other text"
        );
        assert_eq!(controller.viewport_offset("h"), Some(0));
    }

    // ── Caret blink ─────────────────────────────────────────────────────

    /// Cells carrying the caret's `REVERSED` modifier in one rendered frame.
    ///
    /// Counted rather than addressed: the blink's contract is "a phase changes
    /// whether the caret is painted", and a count says that directly. Nothing
    /// here paints an inverse cell of its own — the fixtures are plain text —
    /// so a reversed cell can only be the caret.
    fn reversed_cells(lines: &[ratatui::text::Line<'_>]) -> usize {
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .filter(|span| {
                span.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::REVERSED)
            })
            .count()
    }

    fn rendered(blink_on: bool) -> Vec<ratatui::text::Line<'static>> {
        let mut controller = scrolled_history(3, &["alpha", "bravo"]);
        assert!(controller.focus("h"), "the fixture is live and attached");
        controller
            .render_view("h", 200, blink_on, ratatui::style::Style::default())
            .expect("render")
    }

    /// The reported defect: the caret was painted `REVERSED` and nothing
    /// changed it, so a user could not tell from the terminal that the PTY was
    /// live — the one thing the `[INS]` badge claims on the frame above.
    ///
    /// Both phases are *pinned*, so this pins the production decision rather
    /// than sampling a live clock: `render_view` takes the phase as an input,
    /// which is the only reason a blink can be asserted at all.
    #[test]
    fn the_insert_caret_blinks_on_the_phase_it_is_given() {
        assert_eq!(
            reversed_cells(&rendered(true)),
            1,
            "the visible phase paints exactly one caret cell"
        );
        assert_eq!(
            reversed_cells(&rendered(false)),
            0,
            "the hidden phase paints none"
        );
    }

    /// The dark phase must not change the *text*.
    ///
    /// A cursor that stopped reserving its cell would let `styled_lines` trim
    /// the line back to its last character, and the next visible phase would
    /// re-extend it — the line would appear to flicker instead of the caret,
    /// and at the end of a prompt the whole line would vanish for half of every
    /// second.
    #[test]
    fn the_hidden_phase_keeps_the_caret_cell_it_reserves() {
        let on = rendered(true);
        let off = rendered(false);
        assert_eq!(
            on.iter().map(|line| line.to_string()).collect::<Vec<_>>(),
            off.iter().map(|line| line.to_string()).collect::<Vec<_>>(),
            "the blink must change the modifier and nothing else"
        );
    }

    /// NOR is a selection anchor, not a caret: `x`, `v` and `d` all act from
    /// wherever it sits, so a blinking anchor would make "which line am I about
    /// to delete" a coin flip. The phase is ignored there entirely.
    #[test]
    fn the_reader_cursor_never_blinks() {
        let mut controller = scrolled_history(3, &["alpha", "bravo"]);
        assert!(!controller.is_focused("h"), "the premise: reading");
        for blink_on in [true, false] {
            let lines = controller
                .render_view("h", 200, blink_on, ratatui::style::Style::default())
                .expect("render");
            assert_eq!(
                reversed_cells(&lines),
                1,
                "a NOR cursor is an anchor and stays solid (blink_on={blink_on})"
            );
        }
    }

    // ── Leaving Insert ────────────────────────────────────────────────

    /// The reported defect: `Esc` handed the keyboard back and left the reader's
    /// cursor where the last clear had parked it — the bottom row — so the
    /// first `k` looked inert and the first `x` armed a line nobody could see.
    ///
    /// Anchored on *content*, so a short command puts the cursor on the prompt
    /// line rather than in the blank space under it.
    #[test]
    fn leaving_insert_lands_the_reader_cursor_on_the_last_line_with_content() {
        let mut controller = scrolled_history(6, &["one", "two", "three"]);
        assert!(controller.focus("h"));
        assert_eq!(
            controller.cursor("h"),
            Some((5, 0)),
            "the premise: Insert leaves the reader parked at the bottom"
        );

        controller.escape("h");

        assert!(!controller.is_focused("h"));
        assert_eq!(
            controller.cursor("h"),
            Some((2, 0)),
            "the cursor belongs on the last line the process painted"
        );
    }

    /// A full screen is the other end of the same rule: the block's last line
    /// *is* the last row, so placement must not underflow past it or stop one
    /// row short of the output the reader just produced.
    #[test]
    fn leaving_insert_on_a_full_screen_lands_on_the_last_row() {
        let rows = 4;
        let lines = ["l0", "l1", "l2", "l3"];
        let mut controller = scrolled_history(rows, &lines);
        assert!(controller.focus("h"));
        controller.escape("h");
        assert_eq!(controller.cursor("h"), Some((3, 0)));
    }

    /// Nothing has been printed yet — a shell that has not echoed its prompt,
    /// or a screen cleared a moment ago. Row 0 is the only honest answer; the
    /// bottom row is the placement this rule exists to stop.
    #[test]
    fn leaving_insert_on_an_empty_screen_lands_on_row_zero() {
        let mut controller = scrolled_history(5, &["", "", "", "", ""]);
        assert!(controller.focus("h"));
        controller.escape("h");
        assert_eq!(controller.cursor("h"), Some((0, 0)));
    }

    /// A one-row screen is the degenerate geometry: `rows.saturating_sub(1)` is
    /// 0 and the cursor must stay there rather than wrapping.
    #[test]
    fn leaving_insert_cannot_underflow_on_a_one_row_screen() {
        let mut controller = scrolled_history(1, &["only"]);
        assert!(controller.focus("h"));
        controller.escape("h");
        assert_eq!(controller.cursor("h"), Some((0, 0)));
    }

    /// The sweep that reveals a hidden view is not a gesture the reader made,
    /// so it must not move a cursor they did not move. A hidden view reaching
    /// `Viewing` is the one part of `escape` with no user behind it.
    #[test]
    fn an_unrelated_escape_does_not_move_a_hidden_views_cursor() {
        let mut controller = scrolled_history(4, &["one", "two", "three"]);
        // A second view, focused, so the first one is hidden by the sweep's
        // own rule.
        assert!(controller.apply_created(
            "h2".to_string(),
            "ws-1".to_string(),
            "cat".to_string(),
            40,
            4
        ));
        let other = chunk(0, 3, false, "two");
        assert!(controller
            .apply_attached("h2", "a-2".to_string(), &other, None)
            .expect("attach")
            .is_none());
        assert!(controller.focus("h2"));
        assert!(!controller.is_focused("h"));

        // The reader presses Esc in the *box*, aimed at nothing this view did.
        controller.escape("h2");
        assert!(
            !controller.is_focused("h"),
            "the premise: `h` was hidden and stayed hidden until the sweep"
        );
        assert_eq!(
            controller.cursor("h"),
            Some((3, 0)),
            "being swept out of Hidden must not place a cursor this reader never asked for"
        );
    }

    /// Placement is bookkeeping, so it must not read as a deliberate movement.
    /// A `v` selection armed before Insert is still armed after it, anchored
    /// where the reader put it.
    #[test]
    fn leaving_insert_keeps_the_selection_the_reader_built() {
        let mut controller = scrolled_history(4, &["alpha", "bravo"]);
        assert!(controller.begin_selection("h", SelectionMode::Char));
        assert!(controller.focus("h"));
        controller.escape("h");
        assert!(
            controller.selection("h").is_some(),
            "Esc from Insert is not a gesture against a selection the reader built"
        );
    }

    #[test]
    fn unknown_handle_navigation_is_refused_not_panicking() {
        let mut controller = InteractiveTerminalController::new();
        assert!(!controller.scroll_by("nope", 5));
        assert!(!controller.move_cursor("nope", 1, 0));
        assert!(!controller.begin_selection("nope", SelectionMode::Char));
        assert!(!controller.toggle_line_selection("nope"));
        assert!(!controller.cancel_selection("nope"));
        assert!(!controller.scroll_to_bottom("nope"));
        assert_eq!(controller.viewport_offset("nope"), None);
        assert_eq!(controller.cursor("nope"), None);
    }

    #[test]
    fn viewport_cannot_underflow_when_history_shrinks_under_it() {
        // `Grid::visible_rows` computes `scrollback.len() - offset` unguarded,
        // so an offset left larger than the retained history is a panic, not a
        // clamp. A clear is exactly how that happens.
        let rows = 5;
        let lines: Vec<String> = (0..40).map(|n| format!("page-{n:02}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut controller = scrolled_history(rows, &refs);
        controller.scroll_by("h", 100);
        assert!(controller.clear_scrollback("h"));
        // Reading after the clear must be safe, and must report the fresh
        // bottom rather than the offset the reader had before it.
        assert!(controller.render_lines("h", 20).is_some());
        assert_eq!(controller.viewport_offset("h"), Some(0));
    }

    // ── Word motion ────────────────────────────────────────────────────

    #[test]
    fn word_motion_walks_word_boundaries_within_the_cursor_row() {
        // "cargo build --release --target x86_64"
        let rows = 3;
        let lines = ["cargo build --release --target x86_64", "", ""];
        let mut controller = scrolled_history(rows, &lines);
        // The cursor starts on the last row, which the fixture leaves blank;
        // word motions are about the row the reader is actually on.
        controller.move_cursor("h", -2, 0);
        assert_eq!(controller.cursor("h"), Some((0, 0)));

        // `e` lands *on* the last character of a word, not one past it, so
        // `v` then `e` copies "cargo" rather than "cargo ". Repeated `e`
        // advances because a cursor already at a word's end takes the next one.
        for end in [4u16, 10, 20, 29, 36] {
            assert!(
                controller.word_motion("h", true),
                "e must reach column {end}"
            );
            assert_eq!(controller.cursor("h").map(|(_, col)| col), Some(end));
        }
        assert!(
            !controller.word_motion("h", true),
            "there is no word after the last one"
        );
        assert_eq!(
            controller.cursor("h").map(|(_, col)| col),
            Some(36),
            "a refused jump must leave the cursor alone rather than resetting it"
        );

        // `b` is the mirror: the first character of each word, walking back.
        for start in [31u16, 22, 12, 6, 0] {
            assert!(
                controller.word_motion("h", false),
                "b must reach column {start}"
            );
            assert_eq!(controller.cursor("h").map(|(_, col)| col), Some(start));
        }
        assert!(!controller.word_motion("h", false));
    }

    #[test]
    fn word_motion_is_refused_for_an_unknown_handle() {
        let mut controller = InteractiveTerminalController::new();
        assert!(!controller.word_motion("nope", true));
        assert!(!controller.word_motion("nope", false));
    }

    #[test]
    fn word_motion_widens_a_char_selection_but_never_a_line_one() {
        let rows = 3;
        let lines = ["cargo build --release", "", ""];
        let mut controller = scrolled_history(rows, &lines);
        controller.move_cursor("h", -2, 0);

        controller.toggle_char_selection("h");
        controller.word_motion("h", true);
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("cargo"),
            "`e` after `v` extends, which is the whole point of `v` being a mode"
        );
        controller.word_motion("h", true);
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("cargo build")
        );

        controller.cancel_selection("h");
        controller.begin_selection("h", SelectionMode::Line);
        let before = controller.selected_text("h");
        controller.word_motion("h", true);
        controller.word_motion("h", true);
        assert_eq!(
            controller.selected_text("h"),
            before,
            "a bare movement must not add text to a line selection"
        );
    }

    #[test]
    fn v_twice_releases_the_selection_and_stops_extending() {
        let rows = 3;
        let lines = ["cargo build --release", "", ""];
        let mut controller = scrolled_history(rows, &lines);
        controller.move_cursor("h", -2, 0);
        controller.toggle_char_selection("h");
        controller.move_cursor("h", 0, 5);
        assert!(controller.selection("h").is_some());

        controller.toggle_char_selection("h");
        assert!(
            controller.selection("h").is_none(),
            "`v` toggles, it does not re-arm"
        );
        let parked = controller.cursor("h").expect("cursor");
        controller.move_cursor("h", 0, 12);
        assert!(
            controller.selection("h").is_none(),
            "movement after `v` was released must not re-arm it"
        );
        assert_ne!(controller.cursor("h").expect("cursor"), parked);
    }

    // ── Delete ──────────────────────────────────────────────────────────

    #[test]
    fn delete_blanks_the_selection_and_takes_it_back_out_of_the_clipboard() {
        let rows = 3;
        let lines = ["secret token abc", "keep me", ""];
        let mut controller = scrolled_history(rows, &lines);
        controller.move_cursor("h", -2, 0); // row 0
        controller.begin_selection("h", SelectionMode::Char);
        controller.move_cursor("h", 0, 6); // covers "secret"

        assert!(controller.delete_selection("h"));
        assert!(
            controller.selection("h").is_none(),
            "`d` is an operator, so it consumes what it acted on"
        );
        assert_eq!(
            controller.redaction_count("h"),
            1,
            "only the start row is inside a single-row selection"
        );

        // Deleted text must not come back through the yank path — that is the
        // difference between a deletion and a visual effect.
        controller.begin_selection("h", SelectionMode::Line);
        assert_eq!(
            controller.selected_text("h").as_deref(),
            Some("       token abc"),
            "the scrubbed columns read back as blanks and the rest of the row \
             survives; `keep me` on the next row is untouched"
        );
    }

    #[test]
    fn delete_of_a_line_selection_blanks_every_line_it_spans() {
        let rows = 4;
        let lines = ["one", "two", "three", "four"];
        let mut controller = scrolled_history(rows, &lines);
        controller.move_cursor("h", -3, 0); // row 0 = "one"
        controller.toggle_line_selection("h");
        controller.toggle_line_selection("h"); // rows 0..=1
        assert!(controller.delete_selection("h"));
        assert_eq!(controller.redaction_count("h"), 2);

        controller.move_cursor("h", -2, 0);
        controller.begin_selection("h", SelectionMode::Line);
        assert_eq!(controller.selected_text("h").as_deref(), None);
    }

    #[test]
    fn a_deletion_stays_attached_to_its_line_when_the_viewport_moves() {
        let rows = 5;
        let lines: Vec<String> = (0..40).map(|n| format!("page-{n:02}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut controller = scrolled_history(rows, &refs);
        controller.move_cursor("h", -4, 0);
        controller.toggle_line_selection("h");
        let before = controller.selected_text("h").expect("text");
        assert!(controller.delete_selection("h"));

        // Scroll away and come back. The overlay is keyed on row *text*
        // precisely so this round trip does not resurrect the line.
        controller.scroll_by("h", 4);
        controller.scroll_by("h", -4);
        controller.begin_selection("h", SelectionMode::Line);
        assert_eq!(
            controller.selected_text("h").as_deref(),
            None,
            "scrolling away and back must not un-delete {before:?}"
        );
    }

    #[test]
    fn delete_with_nothing_selected_deletes_nothing() {
        let rows = 3;
        let lines = ["alpha", "bravo", "charlie"];
        let mut controller = scrolled_history(rows, &lines);
        assert!(!controller.delete_selection("h"));
        assert_eq!(controller.redaction_count("h"), 0);
        assert!(!controller.delete_selection("nope"));

        controller.begin_selection("h", SelectionMode::Line);
        assert!(controller.delete_selection("h"));
        assert!(
            !controller.delete_selection("h"),
            "the selection is consumed"
        );
    }

    #[test]
    fn clear_drops_the_scrub_overlay_with_the_screen() {
        let rows = 3;
        let lines = ["alpha", "bravo", "charlie"];
        let mut controller = scrolled_history(rows, &lines);
        controller.begin_selection("h", SelectionMode::Line);
        assert!(controller.delete_selection("h"));
        assert_eq!(controller.redaction_count("h"), 1);

        assert!(controller.clear_scrollback("h"));
        assert_eq!(
            controller.redaction_count("h"),
            0,
            "the overlay is keyed on text the clear threw away; carrying it \
             forward would hide whichever new output reuses that wording"
        );
    }

    #[test]
    fn the_scrub_overlay_is_bounded() {
        let rows = 5;
        let lines: Vec<String> = (0..200).map(|n| format!("unique-{n:03}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut controller = scrolled_history(rows, &refs);
        for _ in 0..(MAX_TERMINAL_REDACTIONS + 25) {
            controller.toggle_line_selection("h");
            controller.delete_selection("h");
        }
        assert_eq!(controller.redaction_count("h"), MAX_TERMINAL_REDACTIONS);
    }
}
