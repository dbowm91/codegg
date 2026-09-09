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
//!                              (bounded scrollback + focus + link state)
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
//!   focus ([`TerminalFocus::Focused`]). [`classify_key`] maps `Esc` to
//!   [`TerminalKeyAction::EscapeFocus`] unconditionally: escape/focus can
//!   never submit a prompt and the raw `Esc` byte is never forwarded.
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
//! (`MAX_PTY_DIMENSION`, `MAX_INPUT_WRITE_BYTES`,
//! `MAX_INTERACTIVE_CHUNK_BYTES`) rather than re-declaring them. The
//! per-view scrollback window ([`TERMINAL_VIEW_BYTES`]) matches the M002
//! default read size so one attach chunk always fits. The per-controller
//! terminal cap ([`MAX_TERMINALS_PER_VIEW`]) matches the M002 per-client
//! attachment cap so the TUI cannot hold more terminals than the daemon
//! lets one client attach to.

use std::collections::{BTreeMap, VecDeque};

use crate::interactive_process::MAX_PTY_DIMENSION;
use crate::protocol::interactive_process::{
    InteractiveOutputChunk, InteractiveResync, InteractiveResyncReason, MAX_ATTACHMENTS_PER_CLIENT,
    MAX_INTERACTIVE_CHUNK_BYTES, MAX_INTERACTIVE_INPUT_BYTES,
};

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

/// Explicit keyboard-focus state for one terminal view.
///
/// Input bytes are produced by [`classify_key`] only in
/// [`TerminalFocus::Focused`]. Opening a terminal always starts in
/// [`TerminalFocus::Viewing`] so stray keystrokes keep going to the
/// prompt until the user explicitly focuses the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFocus {
    /// Terminal is not the active view; keys belong to the prompt/dialog.
    Hidden,
    /// Terminal is visible but keys still belong to the prompt/dialog.
    Viewing,
    /// Terminal owns the keyboard (except `Esc`, which always escapes).
    Focused,
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
                    "invalid terminal size {cols}x{rows}: dimensions must be 1..={}",
                    MAX_PTY_DIMENSION
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
    Esc,
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
/// - `Esc` while focused always yields [`TerminalKeyAction::EscapeFocus`]:
///   the raw escape byte is never forwarded and a prompt is never
///   submitted from terminal focus.
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
    fn apply_chunk(&mut self, chunk: &InteractiveOutputChunk, raw: &[u8]) {
        if chunk.gap || chunk.from_seq != self.next_seq {
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
#[derive(Debug, Clone)]
pub struct InteractiveTerminalView {
    handle: String,
    workspace_id: String,
    command: String,
    attachment_id: Option<String>,
    link: TerminalLinkState,
    focus: TerminalFocus,
    scrollback: BoundedScrollback,
    pending_input: Vec<u8>,
    pending_resize: Option<(u16, u16)>,
    cols: u16,
    rows: u16,
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
            pending_input: Vec::new(),
            pending_resize: None,
            cols,
            rows,
        }
    }

    /// Current daemon-link state for the view.
    pub fn link_state(&self) -> &TerminalLinkState {
        &self.link
    }

    /// One-line status for dialog headers and `/terminal-list` output.
    pub fn status_line(&self) -> String {
        let focus = match self.focus {
            TerminalFocus::Hidden => "hidden",
            TerminalFocus::Viewing => "viewing",
            TerminalFocus::Focused => "focused",
        };
        let attached = if self.attachment_id.is_some() {
            "attached"
        } else {
            "detached"
        };
        format!(
            "{} [{}] {} {} {}x{} seq={} {}",
            self.handle,
            self.link.label(),
            attached,
            focus,
            self.cols,
            self.rows,
            self.scrollback.next_seq,
            self.command,
        )
    }
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
}

impl InteractiveTerminalController {
    pub fn new() -> Self {
        Self {
            views: BTreeMap::new(),
            active: None,
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
            .map_or(true, |h| !self.views.contains_key(h))
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
        view.scrollback.apply_chunk(chunk, &raw);
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
        view.scrollback.apply_chunk(chunk, &raw);
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
            } else {
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
            if !view.link.is_exited() {
                view.link = TerminalLinkState::Gone {
                    reason: "daemon restarted: ephemeral handles invalidated".to_string(),
                };
            }
        }
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
    pub fn escape(&mut self, handle: &str) {
        if let Some(view) = self.views.get_mut(handle) {
            if view.focus == TerminalFocus::Focused {
                view.focus = TerminalFocus::Viewing;
            }
        }
        for other in self.views.values_mut() {
            if other.focus == TerminalFocus::Hidden {
                other.focus = TerminalFocus::Viewing;
            }
        }
    }

    /// Whether `handle` currently owns the keyboard.
    pub fn is_focused(&self, handle: &str) -> bool {
        self.views
            .get(handle)
            .is_some_and(|view| view.focus == TerminalFocus::Focused)
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

    /// Queue a resize for a live terminal. Rapid resizes coalesce:
    /// only the newest size is kept until drained.
    pub fn queue_resize(
        &mut self,
        handle: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), TerminalResizeError> {
        if cols == 0 || rows == 0 || cols > MAX_PTY_DIMENSION || rows > MAX_PTY_DIMENSION {
            return Err(TerminalResizeError::InvalidSize { cols, rows });
        }
        let view = self
            .views
            .get_mut(handle)
            .ok_or(TerminalResizeError::UnknownHandle)?;
        if !view.link.allows_mutation() {
            return Err(TerminalResizeError::NotLive(view.link.label()));
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
        }
    }

    /// Render the newest bounded lines for one terminal view.
    ///
    /// Decodes scrollback lossily (terminal bytes are arbitrary), keeps
    /// the newest [`TERMINAL_VIEW_LINES`] lines, and truncates overlong
    /// lines. Returns `None` for unknown handles.
    pub fn render_lines(&self, handle: &str, max_lines: usize) -> Option<Vec<String>> {
        let view = self.views.get(handle)?;
        let max_lines = max_lines.min(TERMINAL_VIEW_LINES);
        // Reassemble from the deque without extra allocation surprises.
        let bytes: Vec<u8> = view.scrollback.bytes.iter().copied().collect();
        let take_bytes = bytes.len().min(MAX_TERMINAL_RENDER_BYTES);
        let window = &bytes[bytes.len().saturating_sub(take_bytes)..];
        let text = String::from_utf8_lossy(window);
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|line| {
                let stripped = line.strip_suffix('\r').unwrap_or(line);
                if stripped.len() > MAX_TERMINAL_LINE_BYTES {
                    format!(
                        "{}... [line truncated]",
                        &stripped[..MAX_TERMINAL_LINE_BYTES]
                    )
                } else {
                    stripped.to_string()
                }
            })
            .collect();
        if lines.len() > max_lines {
            lines.drain(..lines.len() - max_lines);
        }
        Some(lines)
    }

    /// Header lines describing one terminal for the dialog view,
    /// including link/focus state, cursors, and any resync/exit detail.
    /// Never includes raw terminal output.
    pub fn header_lines(&self, handle: &str) -> Option<Vec<String>> {
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

    #[test]
    fn escape_leaves_focus_and_never_forwards_or_submits() {
        assert_eq!(
            classify_key(true, TerminalKey::Esc),
            TerminalKeyAction::EscapeFocus
        );
        // No key that escapes focus also forwards bytes.
        let action = classify_key(true, TerminalKey::Esc);
        assert!(!matches!(action, TerminalKeyAction::Forward(_)));
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
            .queue_resize("h", MAX_PTY_DIMENSION + 1, 24)
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
}
