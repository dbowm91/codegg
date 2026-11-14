//! One chat browser, two surfaces.
//!
//! `/chat` ([`ChatDialog`](crate::tui::components::dialogs::chat::ChatDialog))
//! and the chat side panel
//! ([`ChatPanel`](crate::tui::panels::chat::ChatPanel)) were written as
//! peers with the same keymap copied between them. They then drifted in ways
//! only a user could report: the panel could not reply or edit, and the
//! panel opened scrolled to the oldest message so a populated channel looked
//! empty. A fix applied to one surface and not the other is a fix that comes
//! back.
//!
//! This module is that one implementation. It owns everything the two
//! surfaces previously duplicated:
//!
//! * the message cursor, the channel highlight, and the armed-redact slot;
//! * the Normal-mode keymap and what each key *means*;
//! * message row rendering, including the selection contrast;
//! * the viewport anchor, including the tail-follow rule;
//! * the contextual key hints.
//!
//! What it deliberately does **not** own is text entry. The bottom composer is
//! the single input surface for every destination, so a key that needs typing
//! resolves to [`ChatKeyAction::Compose`] and the *host* materializes the
//! editor — the dialog uses its own bounded composer, the panel arms the
//! shared one. That is the whole difference between the two surfaces, and it
//! is a difference of where the caret lives, not of what the keys do.
//!
//! ## Authority
//!
//! The browser owns view state only. It never mutates a message: every
//! mutation resolves to a [`TuiMsg`] that `App::process_msg` routes into the
//! existing `commands/chat.rs` `start_chat_*` seams, so daemon round-trips,
//! authorization, idempotency keys, and the request-id / reconnect-epoch stale
//! guards stay where they already are.

use std::collections::VecDeque;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::protocol::core::{ChatChannelDto, ChatMessageDto};
use crate::tui::app::state::chat::ChatStatus;
use crate::tui::app::TuiMsg;
use crate::tui::components::hint_line::HintLine;
use crate::tui::render_cost;
use crate::tui::theme::Theme;
use crate::tui::wrap::wrap_to_strings;

/// Rows one page key moves the cursor.
///
/// Key handling runs before render and has no frame, so this is a fixed step
/// rather than the true viewport height. Render re-anchors on the cursor, so a
/// mis-sized page still lands on a visible row.
pub const PAGE_STEP: usize = 10;

/// A request to open a text editor, produced by a Normal-mode verb.
///
/// The browser cannot satisfy this itself — it holds no text. `i`/`r`/`e`
/// all land here and the host decides where the caret goes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatComposeSeed {
    /// Text to place in the editor. `None` leaves whatever the host's
    /// editor already holds, which is what plain `i` means: keep my draft.
    pub prefill: Option<String>,
    /// Message this send replies to.
    pub reply_to: Option<String>,
    /// Message this send replaces.
    pub edit_target: Option<String>,
    /// Revision captured when the edit began, so conflict detection stays
    /// daemon-side rather than becoming a client-side guess.
    pub edit_revision: u64,
}

impl ChatComposeSeed {
    pub(crate) fn say() -> Self {
        Self::default()
    }

    fn reply(author: &str, message_id: String) -> Self {
        Self {
            prefill: Some(format!("@{author} ")),
            reply_to: Some(message_id),
            ..Self::default()
        }
    }

    fn edit(message_id: String, revision: u64, body: String) -> Self {
        Self {
            prefill: Some(body),
            edit_target: Some(message_id),
            edit_revision: revision,
            ..Self::default()
        }
    }

    /// Badge/label wording for the armed editor.
    pub fn label(&self) -> &'static str {
        if self.edit_target.is_some() {
            "edit"
        } else if self.reply_to.is_some() {
            "reply"
        } else {
            "say"
        }
    }

    /// Whether this seed carries enough context to warrant showing it in the
    /// composer badge. A bare `i` keeps the existing draft and needs no
    /// decoration.
    pub fn is_contextual(&self) -> bool {
        self.reply_to.is_some() || self.edit_target.is_some()
    }
}

/// What one key meant to the browser.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatKeyAction {
    /// Swallowed. Navigation and toggles resolve here.
    Consumed,
    /// The surface's own exit gesture (`Esc` and `q`). The browser declines to
    /// decide what it means: for the dialog it closes the window, for a panel
    /// it backs out of the armed compose context and never the focus.
    Exit,
    /// A daemon-bound mutation to route through `App::process_msg`.
    Intent(TuiMsg),
    /// A text-entry transition for the host to materialize.
    Compose(ChatComposeSeed),
}

/// The projection a browser renders and navigates, in one value.
///
/// Both surfaces already hold this data in different shapes — the dialog as a
/// pushed [`ChatSnapshot`](crate::tui::components::dialogs::chat::ChatSnapshot),
/// the panel reading `ChatState` live. This is the common denominator.
#[derive(Debug, Clone, Copy)]
pub struct ChatView<'a> {
    /// The window to browse. A `VecDeque` because that is what the reducer
    /// retains and what the side panel borrows directly; `VecDeque` does not
    /// deref to a slice, so the shared view would otherwise force the panel
    /// to copy the window on every frame just to hand it to the browser.
    pub messages: &'a VecDeque<ChatMessageDto>,
    pub channels: &'a [ChatChannelDto],
    pub active_channel_id: Option<&'a str>,
    pub status: ChatStatus,
    pub unread: usize,
    /// Daemon advertises `ChatHistoryOlder`; false makes `u` an honest no-op.
    pub backward_paging: bool,
}

impl<'a> ChatView<'a> {
    /// True when the daemon authorized enough state for a mutation. Sending
    /// into an unavailable project would only produce a guaranteed
    /// `project_not_found`, so the verbs stay closed.
    pub fn interactive(&self) -> bool {
        matches!(self.status, ChatStatus::Ready | ChatStatus::Error)
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Name of the active channel, for the header row.
    pub fn channel_name(&self) -> String {
        self.channels
            .iter()
            .find(|c| Some(c.channel_id.as_str()) == self.active_channel_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "general".to_string())
    }
}

/// Chat view state: which message is selected, which channel is highlighted,
/// and which destructive action is armed.
///
/// Nothing here survives a project change, and nothing here is chat truth —
/// the messages live in `ChatState`.
#[derive(Debug, Clone)]
pub struct ChatBrowser {
    cursor: usize,
    channel_cursor: usize,
    redact_armed: Option<String>,
    show_help: bool,
    /// Whether the cursor tracks the newest message as it arrives.
    ///
    /// Without it the cursor started at index 0 and the viewport showed the
    /// *oldest* messages — a live channel looked empty because the only rows
    /// on screen were from before the user arrived.
    follow_tail: bool,
    /// The wrapped row model, built once per key and read by the surface.
    ///
    /// The browser owns it because the browser owns row rendering: both
    /// surfaces ask this one place, so neither can build `ChatRow`s beside
    /// it. The cache is **per surface** because the browser is: the panel's
    /// cursor and the window's cursor are different, and the key carries the
    /// cursor, so a shared cache would serve one surface's selection to the
    /// other.
    rows: ChatRowCache,
}

impl Default for ChatBrowser {
    fn default() -> Self {
        Self {
            cursor: 0,
            channel_cursor: 0,
            redact_armed: None,
            show_help: false,
            follow_tail: true,
            rows: ChatRowCache::default(),
        }
    }
}

impl ChatBrowser {
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn channel_cursor(&self) -> usize {
        self.channel_cursor
    }

    pub fn show_help(&self) -> bool {
        self.show_help
    }

    pub fn redact_armed(&self) -> Option<&str> {
        self.redact_armed.as_deref()
    }

    /// Disarm a pending destructive action without touching the cursor.
    ///
    /// Used when the panel loses focus: leaving must not strand an armed
    /// redact on a row the user can no longer see.
    pub fn clear_redact(&mut self) {
        self.redact_armed = None;
    }

    /// Drop every piece of per-view state.
    ///
    /// Called on project change and revocation so nothing composed against one
    /// projection survives into another. The cached row model goes with it:
    /// the cache holds message bodies, so it may not outlive the projection
    /// that produced them.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Reconcile the cursor with a freshly applied window.
    ///
    /// Called on every snapshot/live event rather than only on navigation, so
    /// a message arriving while the user is pinned to the newest one keeps
    /// them on it.
    pub fn sync(&mut self, view: &ChatView<'_>) {
        let count = view.message_count();
        if self.follow_tail {
            self.cursor = count.saturating_sub(1);
        } else {
            self.cursor = self.cursor.min(count.saturating_sub(1));
        }
        // The highlight follows the channel the reducer reports as active, so
        // a completed switch lands where the user aimed and the next press
        // continues from there.
        self.channel_cursor = view
            .channels
            .iter()
            .position(|c| Some(c.channel_id.as_str()) == view.active_channel_id)
            .unwrap_or(0)
            .min(view.channel_count().saturating_sub(1));
    }

    /// Selected message, if the window holds one.
    pub fn selected<'a>(&self, view: &'a ChatView<'a>) -> Option<&'a ChatMessageDto> {
        view.messages.get(self.cursor)
    }

    /// Route one key in Normal mode.
    ///
    /// Callers gate on Normal mode themselves: a panel only reaches the
    /// browser outside Insert, and the dialog dispatches its own composer
    /// keys before getting here.
    pub fn handle_key(&mut self, key: KeyEvent, view: &ChatView<'_>) -> ChatKeyAction {
        // Ctrl-based paging works in every browser mode so a long history
        // stays reachable without leaving an editor first.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => {
                    self.page_down(view);
                    return ChatKeyAction::Consumed;
                }
                KeyCode::Char('u') => {
                    self.page_up(view);
                    return ChatKeyAction::Consumed;
                }
                _ => {}
            }
        }

        // The directional gesture is resolved by the shared matcher, not by a
        // `KeyCode` arm below, because its meaning depends on the modifier
        // and because every box has to agree on what `Shift+L` means. It is
        // checked before the match so it cannot be shadowed by a bare `H`,
        // `J`, `K` or `L` verb, and after the Ctrl paging block because a
        // chord is a different gesture.
        //
        // A box is why this browser has the keyboard, so the gesture always
        // means "dock" here; the pane-selection branch of the same
        // dispatcher is unreachable from this path by construction.
        if let Some(dir) = crate::tui::panels::direction_of(key) {
            return ChatKeyAction::Intent(TuiMsg::MoveDirection(dir));
        }

        let last = view.message_count().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => ChatKeyAction::Exit,
            KeyCode::Char('j') | KeyCode::Down => {
                // Pressing `j` at the newest message keeps the tail pinned
                // instead of dropping it, so holding `j` behaves like a
                // "jump to live" key rather than silently freezing.
                if self.cursor >= last {
                    self.follow_tail = true;
                } else {
                    self.follow_tail = false;
                    self.cursor += 1;
                }
                ChatKeyAction::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.follow_tail = false;
                self.cursor = self.cursor.saturating_sub(1);
                ChatKeyAction::Consumed
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.follow_tail = false;
                self.cursor = 0;
                ChatKeyAction::Consumed
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.follow_tail = true;
                self.cursor = last;
                ChatKeyAction::Consumed
            }
            KeyCode::PageDown => {
                self.page_down(view);
                ChatKeyAction::Consumed
            }
            KeyCode::PageUp => {
                self.page_up(view);
                ChatKeyAction::Consumed
            }
            KeyCode::Char('i') | KeyCode::Char('a') | KeyCode::Enter => {
                if !view.interactive() {
                    return ChatKeyAction::Consumed;
                }
                ChatKeyAction::Compose(ChatComposeSeed::say())
            }
            KeyCode::Char('r') => self.reply(view),
            KeyCode::Char('e') => self.edit(view),
            KeyCode::Char('X') => self.arm_or_redact(view),
            KeyCode::Char('m') => ChatKeyAction::Intent(TuiMsg::ChatMarkRead),
            KeyCode::Char('u') => ChatKeyAction::Intent(if view.backward_paging {
                TuiMsg::ChatLoadOlder
            } else {
                // The daemon cannot serve backward paging. Say so rather than
                // letting the key look broken.
                TuiMsg::ChatPagingUnsupported
            }),
            KeyCode::Char('R') => ChatKeyAction::Intent(TuiMsg::ChatRefresh),
            KeyCode::Char('[') => self.step_channel(view, -1),
            KeyCode::Char(']') => self.step_channel(view, 1),
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                ChatKeyAction::Consumed
            }
            // Everything else is swallowed. A leaked `j` would submit a turn;
            // a leaked `/` would open command mode in the middle of browsing.
            _ => ChatKeyAction::Consumed,
        }
    }

    fn page_down(&mut self, view: &ChatView<'_>) {
        let last = view.message_count().saturating_sub(1);
        let next = self.cursor.saturating_add(PAGE_STEP).min(last);
        // Paging past the newest message re-pins, for the same reason `j`
        // does: the user asked to go forward and forward ends at live.
        self.follow_tail = next >= last;
        self.cursor = next;
    }

    fn page_up(&mut self, _view: &ChatView<'_>) {
        self.follow_tail = false;
        self.cursor = self.cursor.saturating_sub(PAGE_STEP);
    }

    /// `r` — reply to the selected message.
    fn reply(&mut self, view: &ChatView<'_>) -> ChatKeyAction {
        if !view.interactive() {
            return ChatKeyAction::Consumed;
        }
        let Some(message) = view.messages.get(self.cursor) else {
            return ChatKeyAction::Consumed;
        };
        let (author, message_id) = (message.author_principal.clone(), message.message_id.clone());
        ChatKeyAction::Compose(ChatComposeSeed::reply(&author, message_id))
    }

    /// `e` — replace the selected message's body.
    fn edit(&mut self, view: &ChatView<'_>) -> ChatKeyAction {
        if !view.interactive() {
            return ChatKeyAction::Consumed;
        }
        let Some(message) = view.messages.get(self.cursor) else {
            return ChatKeyAction::Consumed;
        };
        if message.redacted {
            return ChatKeyAction::Consumed;
        }
        // Authorship is the daemon's call, not the client's — the same
        // principal string is not proof of ownership across transports. The
        // client does not guess and hide the true error.
        let (message_id, revision, body) = (
            message.message_id.clone(),
            message.revision,
            message.body.clone(),
        );
        ChatKeyAction::Compose(ChatComposeSeed::edit(message_id, revision, body))
    }

    /// First `X` arms, second on the same message redacts.
    ///
    /// Redaction is irreversible daemon-side, so it cannot be a single key.
    fn arm_or_redact(&mut self, view: &ChatView<'_>) -> ChatKeyAction {
        let Some(message) = view.messages.get(self.cursor) else {
            self.redact_armed = None;
            return ChatKeyAction::Consumed;
        };
        if message.redacted {
            self.redact_armed = None;
            return ChatKeyAction::Consumed;
        }
        let message_id = message.message_id.clone();
        if self.redact_armed.as_deref() == Some(message_id.as_str()) {
            self.redact_armed = None;
            return ChatKeyAction::Intent(TuiMsg::ChatRedact { message_id });
        }
        self.redact_armed = Some(message_id);
        ChatKeyAction::Consumed
    }

    /// Move the channel highlight one step and emit the switch intent.
    ///
    /// Stepping from the highlight rather than the active channel is what
    /// makes repeated presses before a page lands walk the right way: the
    /// reducer only adopts a new active channel once its response applies,
    /// so rebasing on `active_channel_id` would resolve a second press from
    /// the channel the user is already leaving.
    fn step_channel(&mut self, view: &ChatView<'_>, delta: i8) -> ChatKeyAction {
        let count = view.channel_count();
        if count < 2 {
            // A single-channel project has nothing to switch to; staying
            // quiet beats re-fetching the channel the user is already on.
            return ChatKeyAction::Consumed;
        }
        let next =
            (self.channel_cursor as i64 + i64::from(delta)).rem_euclid(count as i64) as usize;
        self.channel_cursor = next;
        match view.channels.get(next) {
            Some(channel) => ChatKeyAction::Intent(TuiMsg::ChatSwitchChannel {
                channel_id: channel.channel_id.clone(),
            }),
            None => ChatKeyAction::Consumed,
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────

/// One rendered display row, tagged with the message it belongs to so the
/// cursor highlights a whole message rather than one wrapped row.
#[derive(Debug, Clone)]
pub struct ChatRow {
    pub message_index: usize,
    pub line: Line<'static>,
}

/// What a cached chat row model depends on, as a frame can state it.
///
/// The key is deliberately **coarse**: the whole `Vec<ChatRow>` is cached
/// against it rather than each message's wrapped chunks. It is bounded,
/// correct and sufficient — during streaming the cursor does not move, so the
/// cache hits at frame rate, and a scroll or a channel switch rebuilds at
/// human speed.
///
/// Only the inputs a [`ChatView`] cannot state live here. The message window,
/// the unread count and the active channel come from the view the model is
/// built from, so a caller cannot describe one window and draw another; the
/// cursor lives on the browser for the same reason.
#[derive(Debug, Clone, Copy)]
pub struct ChatRowsRequest<'a> {
    /// The reducer's own monotonic sequence for the project — the value it
    /// bumps when it applies a page or a live event. The `/chat` window,
    /// which is handed a bounded snapshot rather than reading the reducer,
    /// passes its applied-snapshot revision here instead: both mean "the
    /// content moved", which is the only thing this key needs to know.
    pub sequence: u64,
    /// The column width the bodies wrap to.
    pub width: u16,
    /// The theme the rows are styled with. The whole value is the identity —
    /// a theme whose colours changed while its `id` stayed the same still
    /// invalidates, and comparing it allocates nothing.
    pub theme: &'a Arc<Theme>,
}

/// The cached row model a surface draws, and its viewport anchor.
#[derive(Debug, Clone, Copy)]
pub struct ChatRowModel<'a> {
    /// The wrapped display rows.
    pub rows: &'a [ChatRow],
    /// Last row belonging to the selected message, computed when the model
    /// was built. The frame reads it rather than reverse-scanning every row.
    pub selected_end: usize,
}

/// The row model's cache: the rows, the anchor, and the key that produced them.
#[derive(Debug, Clone, Default)]
struct ChatRowCache {
    /// `None` until the first build, and after an invalidation.
    key: Option<ChatRowsKey>,
    rows: Vec<ChatRow>,
    selected_end: usize,
}

/// The owned form of [`ChatRowsRequest`], plus the cursor.
#[derive(Debug, Clone)]
struct ChatRowsKey {
    sequence: u64,
    unread: usize,
    channel_id: Option<String>,
    width: u16,
    cursor: usize,
    theme: Arc<Theme>,
}

impl ChatRowsKey {
    /// Whether this key already describes the requested frame.
    ///
    /// Written as an explicit comparison rather than a derived `PartialEq` so
    /// the channel is compared as a `&str` — the view borrows it — and so
    /// every input of the row model is named in one place. Dropping any one
    /// of them serves a stale model: without `sequence` a redaction or an
    /// edit applies to the messages and never reaches the screen, without
    /// `unread` the divider stays where the old marker put it, and without
    /// `channel_id` one channel's rows are drawn under another's name.
    fn covers(&self, request: &ChatRowsRequest<'_>, view: &ChatView<'_>, cursor: usize) -> bool {
        self.sequence == request.sequence
            && self.unread == view.unread
            && self.channel_id.as_deref() == view.active_channel_id
            && self.width == request.width
            && self.cursor == cursor
            && self.theme.as_ref() == request.theme.as_ref()
    }
}

impl ChatBrowser {
    /// The row model for `view`, rebuilt only when its key moved.
    ///
    /// This is the **only** producer of `ChatRow`s: both surfaces call it and
    /// draw what it returns. A hit costs a key comparison and no allocation;
    /// a miss wraps every retained body once and derives the viewport anchor,
    /// which is the work the frame used to do unconditionally.
    pub fn rows<'a>(
        &'a mut self,
        view: &ChatView<'_>,
        request: ChatRowsRequest<'_>,
    ) -> ChatRowModel<'a> {
        let cursor = self.cursor;
        let hit = self
            .rows
            .key
            .as_ref()
            .is_some_and(|key| key.covers(&request, view, cursor));
        if !hit {
            render_cost::count_chat_row_build();
            let rows = build_rows(view, cursor, request.width, request.theme);
            self.rows.selected_end = selected_row_end(&rows, cursor);
            self.rows.key = Some(ChatRowsKey {
                sequence: request.sequence,
                unread: view.unread,
                channel_id: view.active_channel_id.map(str::to_string),
                width: request.width,
                cursor,
                theme: Arc::clone(request.theme),
            });
            self.rows.rows = rows;
        }
        ChatRowModel {
            rows: &self.rows.rows,
            selected_end: self.rows.selected_end,
        }
    }
}

/// The `range` rows of a cached model, as lines that **borrow** them.
///
/// `ratatui`'s `Paragraph` needs an owned `Vec<Line>`, so a draw materializes
/// the visible window. What it must not do is clone every row's `String`
/// content as well: the window is contiguous and the cache outlives the
/// frame, so each span borrows the cached text and a draw allocates only the
/// per-row span vectors. That is the difference between one viewport-bounded
/// copy and a full rebuild plus a copy.
pub fn window_lines(rows: &[ChatRow], range: std::ops::Range<usize>) -> Vec<Line<'_>> {
    range
        .map(|index| {
            Line::from(
                rows[index]
                    .line
                    .spans
                    .iter()
                    .map(|span| Span::styled(span.content.as_ref(), span.style))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// Flatten the message window into wrapped display rows.
///
/// Private on purpose: [`ChatBrowser::rows`] is the producer, and a second
/// caller here would be a second producer of the row model.
fn build_rows(view: &ChatView<'_>, cursor: usize, width: u16, theme: &Theme) -> Vec<ChatRow> {
    let width = width.max(12);
    let mut out: Vec<ChatRow> = Vec::new();
    let unread_from = unread_boundary(view).unwrap_or(usize::MAX);
    for (index, message) in view.messages.iter().enumerate() {
        let selected = index == cursor;
        let marker = if selected { "▍" } else { " " };
        let style = row_style(selected, theme);
        let author_style = if selected {
            style
        } else {
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD)
        };
        // Two columns go to the cursor marker and one space separates the
        // author from the body, so continuation lines are indented under the
        // first body column instead of the marker column.
        let prefix_cols = 2 + display_width(&message.author_principal);
        let available = width.saturating_sub(u16::try_from(prefix_cols).unwrap_or(u16::MAX));
        // The revision marker is appended to the message's **last** row, so that
        // row has to leave room for it. A row already at `available` pushed the
        // marker past the drawn columns and ratatui clipped it, which left an
        // edited message looking unedited in the side panel. Narrowing the whole
        // message is the cost of always showing it, and only a revised message
        // pays it.
        let revision_marker = (message.revision > 1).then(|| format!(" (r{})", message.revision));
        let body_width = match revision_marker.as_deref() {
            Some(revision_marker) => available
                .saturating_sub(u16::try_from(display_width(revision_marker)).unwrap_or(u16::MAX))
                .max(8),
            None => available.max(8),
        };
        let chunks = if message.redacted {
            vec!["[redacted]".to_string()]
        } else {
            render_cost::count_chat_wrap_pass();
            wrap_to_strings(&message.body, body_width)
        };
        let mut emitted = 0usize;
        for (line_index, chunk) in chunks.iter().enumerate() {
            if unread_from != usize::MAX && index == unread_from && line_index == 0 {
                out.push(ChatRow {
                    message_index: index,
                    line: Line::from(Span::styled(
                        format!("── {} unread ──", view.unread),
                        Style::default()
                            .fg(theme.warning)
                            .add_modifier(Modifier::DIM),
                    )),
                });
            }
            let mut spans = if line_index == 0 {
                vec![
                    Span::styled(marker, style),
                    Span::styled(format!("{} ", message.author_principal), author_style),
                    Span::styled(chunk.clone(), style),
                ]
            } else {
                vec![Span::styled(format!("  {chunk}"), style)]
            };
            // Pad the final row out to the viewport so the selection reads as
            // a full-width bar rather than a highlight that stops mid-sentence.
            //
            // The revision marker is appended **after** the padding, so the row
            // it lands on has to reserve its columns. Without that the padded
            // row filled the width and pushed the marker past the drawn columns,
            // where ratatui clipped it — the marker disappeared on exactly the
            // rows a reader was looking at.
            if selected {
                let used: usize = spans
                    .iter()
                    .map(|s| display_width(s.content.as_ref()))
                    .sum();
                let is_marker_row = line_index + 1 == chunks.len();
                let reserved = if is_marker_row {
                    revision_marker.as_deref().map_or(0, display_width)
                } else {
                    0
                };
                let pad = width.saturating_sub(u16::try_from(used + reserved).unwrap_or(u16::MAX));
                spans.push(Span::styled(" ".repeat(usize::from(pad)), style));
            }
            out.push(ChatRow {
                message_index: index,
                line: Line::from(spans),
            });
            emitted += 1;
        }
        if let Some(revision_marker) = revision_marker.as_deref().filter(|_| emitted > 0) {
            let last = out.len().saturating_sub(1);
            out[last].line.spans.push(Span::styled(
                revision_marker.to_string(),
                Style::default().fg(theme.muted),
            ));
        }
        if message.reply_to.is_some() && emitted > 0 {
            let last = out.len().saturating_sub(1);
            out[last]
                .line
                .spans
                .push(Span::styled(" ↩", Style::default().fg(theme.link)));
        }
    }
    out
}

/// Index of the first unread message, or `None` when nothing is unread.
///
/// The unread *count* is authoritative — the reducer fails closed when the
/// read marker is unknown — so the boundary is derived by counting back from
/// the newest message rather than by trusting a stored seq. A count larger
/// than the held window (which a stale marker can produce) resolves to the
/// oldest row rather than indexing out of bounds.
fn unread_boundary(view: &ChatView<'_>) -> Option<usize> {
    if view.unread == 0 {
        return None;
    }
    Some(
        view.messages
            .len()
            .saturating_sub(view.unread)
            .min(view.messages.len().saturating_sub(1)),
    )
}

/// That field is a *background* — every other selected surface in the TUI
/// pairs it with a foreground (`bg(selection).fg(primary)`). Used alone it
/// made the author's name a dark slate on a dark slate: technically styled,
/// entirely unreadable. Selection is a background, so it is used as one.
fn row_style(selected: bool, theme: &Theme) -> Style {
    if selected {
        Style::default()
            .fg(theme.foreground)
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.foreground)
    }
}

/// The `height` rows around the selected message.
///
/// The viewport is *centred* on the selection and then clamped to the row
/// range. It used to end exactly on the selected message's last row, which
/// made every message after the selection unreachable: scrolling up moved a
/// full screen of blank rows into view *below* the selection and cut the rest
/// of the conversation off entirely, so a partially-read history looked
/// padded rather than scrollable.
///
/// Anchoring on the selected message's *last* wrapped row still matters — a
/// long message can span more rows than the body has, and anchoring on its
/// first line would scroll it off-screen — but it is now only the centre of
/// the window, not its lower bound. Clamping at both edges keeps the
/// tail-follow landing on the newest message exactly as before.
pub fn window(rows_len: usize, selected_end: usize, height: usize) -> std::ops::Range<usize> {
    if rows_len == 0 || height == 0 {
        return 0..0;
    }
    let selected_end = selected_end.min(rows_len - 1);
    // A viewport taller than the content shows all of it; without this the
    // clamp below would underflow on `rows_len - height`.
    let height = height.min(rows_len);
    if height == rows_len {
        return 0..rows_len;
    }
    let above = (height - 1) / 2;
    let start = selected_end.saturating_sub(above).min(rows_len - height);
    start..(start + height)
}

/// Last row belonging to the selected message.
pub fn selected_row_end(rows: &[ChatRow], cursor: usize) -> usize {
    rows.iter()
        .rposition(|row| row.message_index == cursor)
        .unwrap_or(rows.len().saturating_sub(1))
}

/// Contextual keys for Normal mode, before the host's own exit hint.
///
/// Shared verbatim by both surfaces so the two can never advertise different
/// verbs for the same keymap.
pub fn normal_hints(show_help: bool) -> HintLine {
    if show_help {
        HintLine::from_items(&[
            ("jk", "move"),
            ("i", "compose"),
            ("r", "reply"),
            ("e", "edit"),
            ("X", "redact"),
            ("m", "read"),
            ("u", "older"),
            ("R", "refresh"),
            ("[ ]", "channel"),
            ("L", "dock"),
            ("?", "hide help"),
        ])
    } else {
        HintLine::from_items(&[
            ("jk", "move"),
            ("r", "reply"),
            ("e", "edit"),
            ("X", "redact"),
            ("m", "read"),
            ("u", "older"),
            ("[ ]", "channel"),
            ("L", "dock"),
            ("?", "help"),
        ])
    }
}

/// Contextual keys for an armed editor.
pub fn input_hints(seed: &ChatComposeSeed) -> HintLine {
    let mut hint = HintLine::new();
    if seed.is_contextual() {
        hint.push_required("Esc", &format!("cancel {} (keeps text)", seed.label()))
            .push("↵", &format!("send {}", seed.label()));
    } else {
        hint.push_required("Esc", "cancel").push("↵", "send");
    }
    hint
}

/// Terminal columns a string occupies. Used to reserve the author prefix so
/// wrapped bodies align; a char count would under-reserve for CJK/emoji.
pub fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    s.width()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, seq: u64, author: &str, body: &str) -> ChatMessageDto {
        ChatMessageDto {
            message_id: id.to_string(),
            channel_id: "ch-1".to_string(),
            project_id: "p".to_string(),
            seq,
            author_principal: author.to_string(),
            author_agent: None,
            body: body.to_string(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            revision: 1,
            redacted: false,
            created_at_ms: 0,
            edited_at_ms: None,
        }
    }

    fn channel() -> ChatChannelDto {
        ChatChannelDto {
            channel_id: "ch-1".to_string(),
            project_id: "p".to_string(),
            name: "general".to_string(),
            created_by: "alice".to_string(),
            created_at_ms: 0,
        }
    }

    /// The default three-message window.
    fn seed() -> (VecDeque<ChatMessageDto>, Vec<ChatChannelDto>) {
        (
            VecDeque::from(vec![
                message("m1", 1, "alice", "first"),
                message("m2", 2, "bob", "second"),
                message("m3", 3, "alice", "third"),
            ]),
            vec![channel()],
        )
    }

    fn view<'a>(
        messages: &'a VecDeque<ChatMessageDto>,
        channels: &'a [ChatChannelDto],
    ) -> ChatView<'a> {
        ChatView {
            messages,
            channels,
            active_channel_id: Some("ch-1"),
            status: ChatStatus::Ready,
            unread: 0,
            backward_paging: true,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn opens_on_the_newest_message_not_the_oldest() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        assert_eq!(browser.cursor(), 2);
    }

    #[test]
    fn tail_follows_new_arrivals_until_the_user_scrolls_up() {
        let mut messages = seed().0;
        let channels = seed().1;
        let mut browser = ChatBrowser::default();
        browser.sync(&view(&messages, &channels));
        assert_eq!(browser.cursor(), 2);

        // A fourth message arrives while pinned: the cursor follows it.
        messages.push_back(message("m4", 4, "bob", "fourth"));
        browser.sync(&view(&messages, &channels));
        assert_eq!(browser.cursor(), 3);

        // Scrolling up unpins, so later arrivals no longer move the cursor.
        browser.handle_key(key(KeyCode::Char('k')), &view(&messages, &channels));
        assert_eq!(browser.cursor(), 2);
        browser.sync(&view(&messages, &channels));
        assert_eq!(browser.cursor(), 2);

        // `G` re-pins.
        browser.handle_key(key(KeyCode::Char('G')), &view(&messages, &channels));
        assert_eq!(browser.cursor(), 3);
        browser.sync(&view(&messages, &channels));
        assert_eq!(browser.cursor(), 3);
    }

    #[test]
    fn j_at_the_newest_message_keeps_the_tail_pinned() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        browser.handle_key(key(KeyCode::Char('j')), &view);
        assert_eq!(browser.cursor(), 2);
        browser.handle_key(key(KeyCode::Char('j')), &view);
        assert_eq!(browser.cursor(), 2);
    }

    #[test]
    fn navigation_on_an_empty_window_is_safe() {
        let messages: VecDeque<ChatMessageDto> = VecDeque::new();
        let channels = seed().1;
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        assert_eq!(
            browser.handle_key(key(KeyCode::Char('j')), &view),
            ChatKeyAction::Consumed
        );
        assert_eq!(browser.cursor(), 0);
        assert_eq!(
            browser.handle_key(key(KeyCode::Char('G')), &view),
            ChatKeyAction::Consumed
        );
    }

    #[test]
    fn r_asks_for_a_reply_seeded_with_the_author() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        // Newest is m3/alice.
        let action = browser.handle_key(key(KeyCode::Char('r')), &view);
        let ChatKeyAction::Compose(seed) = action else {
            panic!("r must request a composer, got {action:?}");
        };
        assert_eq!(seed.label(), "reply");
        assert_eq!(seed.prefill.as_deref(), Some("@alice "));
        assert_eq!(seed.reply_to.as_deref(), Some("m3"));
        assert!(seed.edit_target.is_none());
    }

    #[test]
    fn r_on_an_empty_window_does_nothing() {
        let messages: VecDeque<ChatMessageDto> = VecDeque::new();
        let channels = seed().1;
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        assert_eq!(
            browser.handle_key(key(KeyCode::Char('r')), &view),
            ChatKeyAction::Consumed
        );
    }

    #[test]
    fn e_asks_for_an_edit_seeded_with_the_existing_body() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        let ChatKeyAction::Compose(seed) = browser.handle_key(key(KeyCode::Char('e')), &view)
        else {
            panic!("e must request a composer");
        };
        assert_eq!(seed.label(), "edit");
        assert_eq!(seed.edit_target.as_deref(), Some("m3"));
        assert_eq!(seed.prefill.as_deref(), Some("third"));
        assert_eq!(seed.edit_revision, 1);
    }

    #[test]
    fn e_refuses_a_redacted_message() {
        let (mut messages, channels) = seed();
        messages[2].redacted = true;
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        assert_eq!(
            browser.handle_key(key(KeyCode::Char('e')), &view),
            ChatKeyAction::Consumed
        );
    }

    #[test]
    fn verbs_are_closed_until_the_daemon_authorizes() {
        let (messages, channels) = seed();
        let view = ChatView {
            status: ChatStatus::Unavailable,
            ..view(&messages, &channels)
        };
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        // Navigation still works — the user must be able to look.
        assert_eq!(
            browser.handle_key(key(KeyCode::Char('k')), &view),
            ChatKeyAction::Consumed
        );
        for verb in ['i', 'r', 'e'] {
            assert_eq!(
                browser.handle_key(key(KeyCode::Char(verb)), &view),
                ChatKeyAction::Consumed,
                "{verb} must be closed while chat is unavailable"
            );
        }
    }

    #[test]
    fn x_needs_two_presses_and_arms_on_the_selected_message() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        assert_eq!(
            browser.handle_key(key(KeyCode::Char('X')), &view),
            ChatKeyAction::Consumed
        );
        assert_eq!(browser.redact_armed(), Some("m3"));
        assert_eq!(
            browser.handle_key(key(KeyCode::Char('X')), &view),
            ChatKeyAction::Intent(TuiMsg::ChatRedact {
                message_id: "m3".to_string()
            })
        );
        assert_eq!(browser.redact_armed(), None);
    }

    #[test]
    fn clear_redact_disarms_without_moving_the_cursor() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        browser.handle_key(key(KeyCode::Char('k')), &view);
        browser.handle_key(key(KeyCode::Char('X')), &view);
        assert!(browser.redact_armed().is_some());
        browser.clear_redact();
        assert!(browser.redact_armed().is_none());
        assert_eq!(browser.cursor(), 1);
    }

    #[test]
    fn u_fails_closed_without_backward_paging() {
        let (messages, channels) = seed();
        let view = ChatView {
            backward_paging: false,
            ..view(&messages, &channels)
        };
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        assert_eq!(
            browser.handle_key(key(KeyCode::Char('u')), &view),
            ChatKeyAction::Intent(TuiMsg::ChatPagingUnsupported)
        );
    }

    #[test]
    fn esc_and_q_are_the_hosts_exit_not_the_browsers() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        assert_eq!(
            browser.handle_key(key(KeyCode::Esc), &view),
            ChatKeyAction::Exit
        );
        assert_eq!(
            browser.handle_key(key(KeyCode::Char('q')), &view),
            ChatKeyAction::Exit
        );
    }

    #[test]
    fn unbound_printable_keys_are_swallowed() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        for code in ['/', 'z', 'Z', ' '] {
            assert_eq!(
                browser.handle_key(key(KeyCode::Char(code)), &view),
                ChatKeyAction::Consumed,
                "{code} must not leak"
            );
        }
    }

    #[test]
    fn a_single_channel_project_does_not_re_fetch_on_bracket() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        assert_eq!(
            browser.handle_key(key(KeyCode::Char(']')), &view),
            ChatKeyAction::Consumed
        );
    }

    #[test]
    fn bracket_steps_channels_and_emits_the_switch() {
        let messages: VecDeque<ChatMessageDto> = VecDeque::new();
        let channels = vec![
            channel(),
            ChatChannelDto {
                channel_id: "ch-2".to_string(),
                ..channel()
            },
        ];
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        assert_eq!(
            browser.handle_key(key(KeyCode::Char(']')), &view),
            ChatKeyAction::Intent(TuiMsg::ChatSwitchChannel {
                channel_id: "ch-2".to_string()
            })
        );
        assert_eq!(browser.channel_cursor(), 1);
        // Stepping again wraps back to the first.
        assert_eq!(
            browser.handle_key(key(KeyCode::Char(']')), &view),
            ChatKeyAction::Intent(TuiMsg::ChatSwitchChannel {
                channel_id: "ch-1".to_string()
            })
        );
    }

    #[test]
    fn selected_row_is_readable_against_the_background() {
        let theme = Theme::dark();
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let rows = build_rows(&view, 2, 30, &theme);
        let selected = rows
            .iter()
            .find(|row| row.message_index == 2)
            .expect("selected row");
        let style = selected.line.spans[2].style;
        // The regression: this used to be `fg(theme.selection)` — a
        // background colour used as a foreground, i.e. near-invisible text.
        assert_eq!(style.fg, Some(theme.foreground));
        assert_eq!(style.bg, Some(theme.selection));
        assert!(
            selected.line.width() >= 30,
            "selection must fill the row: {}",
            selected.line.width()
        );
    }

    #[test]
    fn unselected_author_is_readable_too() {
        let theme = Theme::dark();
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let rows = build_rows(&view, 2, 30, &theme);
        let other = rows.iter().find(|row| row.message_index == 0).expect("row");
        assert_eq!(other.line.spans[2].style.fg, Some(theme.foreground));
        assert_eq!(other.line.spans[1].style.fg, Some(theme.primary));
    }

    #[test]
    fn selection_contrast_holds_in_the_light_theme_too() {
        let theme = Theme::light();
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let rows = build_rows(&view, 0, 30, &theme);
        let selected = rows.iter().find(|row| row.message_index == 0).expect("row");
        // Light themes define `selection` as a near-white wash; using it as a
        // foreground on a light background is the same bug in reverse.
        assert_eq!(selected.line.spans[2].style.fg, Some(theme.foreground));
        assert_eq!(selected.line.spans[2].style.bg, Some(theme.selection));
    }

    #[test]
    fn window_keeps_the_selected_message_visible_when_it_wraps() {
        let long = "word ".repeat(400);
        let messages = VecDeque::from(vec![
            message("m1", 1, "alice", "one line"),
            message("m2", 2, "bob", &long),
            message("m3", 3, "alice", "three line"),
        ]);
        let channels = seed().1;
        let view = view(&messages, &channels);
        let rows = build_rows(&view, 1, 30, &Theme::dark());
        let selected_end = selected_row_end(&rows, 1);
        let window = window(rows.len(), selected_end, 20);
        assert_eq!(window.end - window.start, 20);
        assert!(
            window.contains(&selected_end),
            "wrapped selection scrolled out of view: {window:?} of {selected_end}"
        );
    }

    #[test]
    fn window_shows_context_on_both_sides_of_the_selection() {
        // Regression: the viewport used to *end* on the selected message, so
        // scrolling up left blank rows below it and made everything after the
        // selection unreachable.
        let window = window(100, 50, 20);
        assert!(window.start < 50, "nothing above: {window:?}");
        assert!(window.end > 51, "nothing below: {window:?}");
        assert_eq!(window.end - window.start, 20);
    }

    #[test]
    fn window_clamps_at_both_edges_so_tail_still_lands_at_the_bottom() {
        // Following the newest message must still leave it at the bottom edge,
        // or every stream would appear to end one screen early.
        assert_eq!(window(100, 99, 20), 80..100);
        assert_eq!(window(100, 0, 20), 0..20);
    }

    #[test]
    fn window_wider_than_the_content_returns_everything() {
        assert_eq!(window(3, 2, 20), 0..3);
    }

    #[test]
    fn window_on_no_rows_is_empty_rather_than_panicking() {
        assert_eq!(window(0, 0, 10), 0..0);
        assert_eq!(window(5, 0, 0), 0..0);
        assert_eq!(selected_row_end(&[], 3), 0);
    }

    #[test]
    fn unread_separator_sits_before_the_first_unread_message() {
        let messages = VecDeque::from(vec![
            message("m1", 1, "alice", "read"),
            message("m2", 2, "bob", "unread one"),
            message("m3", 3, "alice", "unread two"),
        ]);
        let channels = seed().1;
        let view = ChatView {
            unread: 2,
            ..view(&messages, &channels)
        };
        let rows = build_rows(&view, 2, 30, &Theme::dark());
        let separator = rows
            .iter()
            .position(|r| r.line.spans[0].content.contains("unread"))
            .expect("unread separator");
        assert_eq!(rows[separator].message_index, 1);
    }

    #[test]
    fn hints_cover_every_normal_mode_verb() {
        let short = normal_hints(false).render(0);
        for verb in [
            "jk move", "r reply", "e edit", "X redact", "m read", "u older",
        ] {
            assert!(short.contains(verb), "short hints missing {verb}");
        }
        let long = normal_hints(true).render(0);
        assert!(long.contains("R refresh"));
        assert!(long.contains("hide help"));
        assert!(!short.contains("compose"), "short hints gained an item");
        assert!(!short.contains("R refresh"), "short hints gained an item");
    }

    #[test]
    fn input_hints_describe_an_armed_reply_but_not_a_plain_say() {
        let reply = ChatComposeSeed::reply("bob", "m1".to_string());
        let rendered = input_hints(&reply).render(0);
        assert!(rendered.contains("reply"));
        let plain = input_hints(&ChatComposeSeed::say());
        assert!(plain.render(0).contains("send"));
        assert!(!plain.render(0).contains("cancel say"));
        assert_eq!(
            plain.items().next().map(|item| item.key()),
            Some("Esc"),
            "the exit key must be the row's first item: {}",
            plain.render(0)
        );
    }

    #[test]
    fn seed_labels_say_which_editor_is_armed() {
        assert_eq!(ChatComposeSeed::say().label(), "say");
        assert_eq!(
            ChatComposeSeed::reply("bob", "m1".to_string()).label(),
            "reply"
        );
        assert_eq!(
            ChatComposeSeed::edit("m1".to_string(), 1, "x".to_string()).label(),
            "edit"
        );
        assert!(!ChatComposeSeed::say().is_contextual());
        assert!(ChatComposeSeed::reply("bob", "m1".to_string()).is_contextual());
    }

    #[test]
    fn reset_returns_to_the_tail() {
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        browser.handle_key(key(KeyCode::Char('g')), &view);
        assert_eq!(browser.cursor(), 0);
        browser.reset();
        browser.sync(&view);
        assert_eq!(browser.cursor(), 2);
    }

    #[test]
    fn a_narrow_panel_does_not_panic() {
        let theme = Theme::dark();
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        // A 1-column viewport must not panic or produce an empty window.
        let rows = build_rows(&view, 0, 1, &theme);
        let end = selected_row_end(&rows, 0);
        let _ = window(rows.len(), end, 1);
    }

    // ── M003: the row model is built once per content change ──────────

    /// What one cached read cost, on this thread.
    fn rows_cost(work: impl FnOnce()) -> (u64, u64) {
        let before = render_cost::snapshot();
        work();
        let after = render_cost::snapshot().since(&before);
        (after.chat_row_builds, after.chat_wrap_passes)
    }

    /// A revised message leaves room on its last row for the marker.
    ///
    /// The marker is appended to the message's last row, so a row that already
    /// filled the width pushed it past the drawn columns and ratatui clipped it
    /// — an edited message then looked unedited in the side panel. The body now
    /// wraps at the width minus the marker, which shortens every row of a
    /// revised message and is the cost of always showing it.
    #[test]
    fn a_revised_message_leaves_room_for_its_marker() {
        const WIDTH: u16 = 26;
        let theme = Arc::new(Theme::dark());
        let mut revised = message("m1", 2, "alice", "a body long enough to wrap at this width");
        revised.revision = 2;
        let messages = VecDeque::from(vec![revised]);
        let channels = vec![channel()];
        let revised_view = view(&messages, &channels);
        let rows = build_rows(&revised_view, 0, WIDTH, &theme);

        let last = rows.last().expect("the message draws at least one row");
        let drawn: usize = last
            .line
            .spans
            .iter()
            .map(|span| display_width(&span.content))
            .sum();
        assert!(
            drawn <= usize::from(WIDTH),
            "the last row fits the width so the marker is not clipped: {drawn} \
             columns in {WIDTH}"
        );
        assert!(
            last.line
                .spans
                .iter()
                .any(|span| span.content.contains("(r2)")),
            "and the marker is on it: {:?}",
            last.line.spans
        );
        // The narrowing is paid only by a revised message: the same body at
        // revision 1 keeps the wider wrap.
        let plain = message("m1", 2, "alice", "a body long enough to wrap at this width");
        let messages = VecDeque::from(vec![plain]);
        let plain_view = view(&messages, &channels);
        let plain_rows = build_rows(&plain_view, 0, WIDTH, &theme);
        assert!(
            plain_rows.len() <= rows.len(),
            "an unrevised message wraps no narrower than a revised one: {} vs {}",
            plain_rows.len(),
            rows.len()
        );
    }

    fn rows_request<'a>(sequence: u64, width: u16, theme: &'a Arc<Theme>) -> ChatRowsRequest<'a> {
        ChatRowsRequest {
            sequence,
            width,
            theme,
        }
    }

    fn model_text(model: ChatRowModel<'_>) -> String {
        model
            .rows
            .iter()
            .flat_map(|row| row.line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// A second read with an unchanged key rebuilds nothing at all.
    #[test]
    fn a_second_read_with_an_unchanged_key_rebuilds_nothing() {
        let theme = Arc::new(Theme::dark());
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);

        let _ = browser.rows(&view, rows_request(7, 30, &theme));
        let (builds, wraps) = rows_cost(|| {
            let _ = browser.rows(&view, rows_request(7, 30, &theme));
        });

        assert_eq!(builds, 0, "an unchanged key rebuilt the whole row model");
        assert_eq!(wraps, 0, "an unchanged key wrapped every body again");
    }

    /// A new message moves the reducer's sequence, so the next read rebuilds
    /// — once — and wraps every retained body.
    #[test]
    fn a_new_message_rebuilds_the_rows_once() {
        let theme = Arc::new(Theme::dark());
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        let _ = browser.rows(&view, rows_request(7, 30, &theme));

        let (builds, wraps) = rows_cost(|| {
            let _ = browser.rows(&view, rows_request(8, 30, &theme));
        });

        assert_eq!(builds, 1, "an applied event must rebuild the model once");
        assert_eq!(
            wraps, 3,
            "a rebuild wraps every retained body, which is why the key is coarse"
        );
    }

    /// A read-marker advance moves the unread divider without moving the
    /// sequence, so the count is part of the key.
    #[test]
    fn a_read_marker_advance_rebuilds_the_unread_divider() {
        let theme = Arc::new(Theme::dark());
        let (messages, channels) = seed();
        let current = ChatView {
            unread: 2,
            ..view(&messages, &channels)
        };
        let mut browser = ChatBrowser::default();
        browser.sync(&current);
        let before = model_text(browser.rows(&current, rows_request(7, 40, &theme)));
        assert!(
            before.contains("2 unread"),
            "the premise: the divider names the unread count, got {before:?}"
        );

        // The marker reached the tail: the same window, the same sequence,
        // the same cursor — only the count the divider draws moved.
        let advanced = ChatView {
            unread: 0,
            ..view(&messages, &channels)
        };
        let (builds, _) = rows_cost(|| {
            let _ = browser.rows(&advanced, rows_request(7, 40, &theme));
        });
        assert_eq!(
            builds, 1,
            "the read marker moved the divider and the sequence did not"
        );
        let after = model_text(browser.rows(&advanced, rows_request(7, 40, &theme)));
        assert!(
            !after.contains("unread"),
            "the divider must be gone once the marker reached the tail, got {after:?}"
        );
    }

    /// The wrap width is an input: a resize rebuilds, the same width does not.
    #[test]
    fn a_width_change_rebuilds_and_the_same_width_does_not() {
        let theme = Arc::new(Theme::dark());
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        let _ = browser.rows(&view, rows_request(7, 30, &theme));

        let (same, _) = rows_cost(|| {
            let _ = browser.rows(&view, rows_request(7, 30, &theme));
        });
        assert_eq!(same, 0, "the same width must not rebuild");

        let (narrower, _) = rows_cost(|| {
            let _ = browser.rows(&view, rows_request(7, 24, &theme));
        });
        assert_eq!(narrower, 1, "a narrower body column re-wraps the bodies");
    }

    /// The cursor styles the selected message's rows, so it is part of the key.
    #[test]
    fn a_cursor_move_rebuilds_the_styled_rows() {
        let theme = Arc::new(Theme::dark());
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        let _ = browser.rows(&view, rows_request(7, 30, &theme));

        browser.handle_key(key(KeyCode::Char('k')), &view);
        let (builds, _) = rows_cost(|| {
            let _ = browser.rows(&view, rows_request(7, 30, &theme));
        });

        assert_eq!(builds, 1, "the selection marker moved to another message");
    }

    /// The theme styles every row, so it is part of the key too.
    #[test]
    fn a_theme_change_rebuilds_the_rows() {
        let dark = Arc::new(Theme::dark());
        let light = Arc::new(Theme::light());
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        let _ = browser.rows(&view, rows_request(7, 30, &dark));

        let (builds, _) = rows_cost(|| {
            let _ = browser.rows(&view, rows_request(7, 30, &light));
        });

        assert_eq!(
            builds, 1,
            "the rows carry the palette they were styled with"
        );
    }

    /// The load-bearing redaction direction: a body that becomes redacted is
    /// drawn as `[redacted]` on the next read, never served from the rows the
    /// previous read produced.
    ///
    /// The reducer's `sequence` is what carries this: a redaction applies to
    /// the window without changing its length, the read marker, the width, the
    /// cursor or the theme. Dropping `sequence` from `ChatRowsKey::covers`
    /// makes this test fail; that perturbation is recorded in the milestone's
    /// evidence and was reverted.
    #[test]
    fn a_body_that_becomes_redacted_is_re_rendered_on_the_next_read() {
        let theme = Arc::new(Theme::dark());
        let (mut messages, channels) = seed();
        // One browser, as a surface has: a second browser would start with an
        // empty cache and prove nothing about the key.
        let mut browser = ChatBrowser::default();
        {
            let view = view(&messages, &channels);
            browser.sync(&view);
            let text = model_text(browser.rows(&view, rows_request(7, 40, &theme)));
            assert!(text.contains("second"), "the premise: the body is drawn");
        }

        // Same message, same id, same length, same cursor, same marker — only
        // the reducer's sequence moved, because a redaction was applied.
        messages[1].redacted = true;
        let view = view(&messages, &channels);
        browser.sync(&view);
        let text = model_text(browser.rows(&view, rows_request(8, 40, &theme)));

        assert!(
            !text.contains("second"),
            "a redacted body was served from the pre-redaction rows: {text:?}"
        );
        assert!(
            text.contains("[redacted]"),
            "the redaction must be visible on the next read: {text:?}"
        );
    }

    /// An edited body (`revision` bumped, same `message_id`) re-renders.
    #[test]
    fn an_edited_body_is_re_rendered_on_the_next_read() {
        let theme = Arc::new(Theme::dark());
        let (mut messages, channels) = seed();
        // One browser: the edit has to be observed *by the cache*, not by a
        // freshly built one.
        let mut browser = ChatBrowser::default();
        {
            let view = view(&messages, &channels);
            browser.sync(&view);
            let text = model_text(browser.rows(&view, rows_request(7, 40, &theme)));
            assert!(text.contains("second"), "the premise: the body is drawn");
        }

        messages[1].body = "revised wording".to_string();
        messages[1].revision = 2;
        let view = view(&messages, &channels);
        browser.sync(&view);
        let text = model_text(browser.rows(&view, rows_request(8, 40, &theme)));

        assert!(
            text.contains("revised wording"),
            "the edit must reach the screen: {text:?}"
        );
        assert!(
            !text.contains("second"),
            "the replaced body must be gone: {text:?}"
        );
        assert!(
            text.contains("(r2)"),
            "the revision marker follows the new body: {text:?}"
        );
    }

    /// One channel's rows are never served to another: the channel is part of
    /// the key, so even a caller that kept the sequence would rebuild.
    #[test]
    fn another_channels_rows_are_never_served_from_the_cache() {
        let theme = Arc::new(Theme::dark());
        let (messages, channels) = seed();
        let current = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&current);
        let first = model_text(browser.rows(&current, rows_request(7, 40, &theme)));
        assert!(first.contains("third"), "the premise: channel one is drawn");

        // The same window presented as another channel's: the sequence, the
        // width, the unread count, the cursor and the theme are all unchanged,
        // so only the channel can force the rebuild.
        let other = ChatView {
            active_channel_id: Some("ch-2"),
            ..view(&messages, &channels)
        };
        let (builds, _) = rows_cost(|| {
            let _ = browser.rows(&other, rows_request(7, 40, &theme));
        });

        assert_eq!(
            builds, 1,
            "a model built for one channel was read under another: the channel \
             is not in the key"
        );
    }

    /// A reset (project switch, revocation, session teardown) drops the model
    /// with everything else the browser holds.
    #[test]
    fn a_reset_drops_the_cached_rows() {
        let theme = Arc::new(Theme::dark());
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let mut browser = ChatBrowser::default();
        browser.sync(&view);
        let _ = browser.rows(&view, rows_request(7, 30, &theme));

        browser.reset();
        browser.sync(&view);
        let (builds, _) = rows_cost(|| {
            let _ = browser.rows(&view, rows_request(7, 30, &theme));
        });

        assert_eq!(builds, 1, "a reset must drop the rows it was holding");
    }

    /// The cache is a memo of the builder, not a second producer: for every
    /// fixture the cached model is exactly what the uncached build produces.
    #[test]
    fn the_cached_rows_are_the_rows_the_builder_produces() {
        let theme = Arc::new(Theme::dark());
        let mut fixtures: Vec<(VecDeque<ChatMessageDto>, Vec<ChatChannelDto>, usize, usize)> =
            Vec::new();
        let (messages, channels) = seed();
        fixtures.push((messages.clone(), channels.clone(), 2, 0));
        fixtures.push((VecDeque::new(), channels.clone(), 0, 0));
        fixtures.push((
            VecDeque::from(vec![message("m1", 1, "alice", &"word ".repeat(80))]),
            channels.clone(),
            0,
            0,
        ));
        fixtures.push((
            VecDeque::from(vec![message("m1", 1, "bob", "héllo 日本語 wörld")]),
            channels.clone(),
            0,
            0,
        ));
        let mut redacted = message("m1", 1, "alice", "withdrawn");
        redacted.redacted = true;
        fixtures.push((VecDeque::from(vec![redacted]), channels.clone(), 0, 0));

        for (messages, channels, cursor, unread) in fixtures {
            let view = ChatView {
                unread,
                ..view(&messages, &channels)
            };
            let mut browser = ChatBrowser {
                cursor,
                ..ChatBrowser::default()
            };
            let cached = browser.rows(&view, rows_request(3, 26, &theme));
            let expected = build_rows(&view, cursor, 26, &theme);
            assert_eq!(
                row_signature(cached.rows),
                row_signature(&expected),
                "the cache's rows differ from the builder's"
            );
            assert_eq!(
                cached.selected_end,
                selected_row_end(&expected, cursor),
                "the cached viewport anchor differs from the uncached scan"
            );
        }
    }

    /// The borrowed window draws exactly what cloning each row drew: every
    /// span's text and style, in order.
    #[test]
    fn the_borrowed_window_draws_what_cloning_the_rows_drew() {
        let theme = Theme::dark();
        let (messages, channels) = seed();
        let view = view(&messages, &channels);
        let rows = build_rows(&view, 2, 30, &theme);
        let range = window(rows.len(), selected_row_end(&rows, 2), 4);

        let cloned: Vec<Vec<(String, Style)>> = range
            .clone()
            .map(|index| {
                rows[index]
                    .line
                    .clone()
                    .spans
                    .iter()
                    .map(|span| (span.content.to_string(), span.style))
                    .collect()
            })
            .collect();
        let borrowed: Vec<Vec<(String, Style)>> = window_lines(&rows, range)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| (span.content.to_string(), span.style))
                    .collect()
            })
            .collect();

        assert_eq!(
            cloned, borrowed,
            "the borrowed window must draw the rows the clone drew"
        );
    }

    fn row_signature(rows: &[ChatRow]) -> Vec<(usize, Vec<(String, Style)>)> {
        rows.iter()
            .map(|row| {
                (
                    row.message_index,
                    row.line
                        .spans
                        .iter()
                        .map(|span| (span.content.to_string(), span.style))
                        .collect(),
                )
            })
            .collect()
    }
}
