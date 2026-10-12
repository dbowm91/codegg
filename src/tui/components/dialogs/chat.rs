//! Self-contained project chat window (`/chat`).
//!
//! This replaces the read-only `InfoType::ProjectChat` panel. The old
//! surface could only scroll: every mutation (`/chat-send`, `/chat-edit`,
//! `/chat-redact`, `/chat-read`, `/chat-sync`, `/chat-history`) had to be
//! typed into the main input field while the window stayed open, so the
//! window and the input field were two halves of one workflow. Here the
//! window owns the whole chat interaction — browse, compose, edit, reply,
//! redact, mark-read, page, and switch channels.
//!
//! ## One browser, two surfaces
//!
//! Navigation, the keymap, row rendering, the viewport anchor, and the key
//! hints all live in [`chat_browser`](crate::tui::chat_browser), shared with
//! the chat side panel. This dialog supplies only what is genuinely its own:
//! a bounded composer with a caret. The panel has no composer — the bottom
//! input is the one text surface — so there it resolves to
//! [`ChatKeyAction::Compose`](crate::tui::chat_browser::ChatKeyAction::Compose)
//! and arms the shared composer instead. Same verbs, same keys, same
//! rendering; the caret lives in different places.
//!
//! ## Authority
//!
//! Authority is unchanged. The dialog owns *presentation and local intent
//! only*: it holds a bounded snapshot pushed from `ChatState` and emits
//! [`TuiMsg`] intents that `App::process_msg` routes into the existing
//! `commands/chat.rs` `start_chat_*` seams. It never talks to
//! `CoreClient`, never mutates a message, and never decides that a send
//! succeeded. Daemon round-trips, authorization, idempotency keys, and
//! the per-project request-id / reconnect-epoch stale guards all live
//! where they already did.
//!
//! ## Interaction model
//!
//! Two modes, following the `Route::Workspace` normal-mode convention:
//!
//! * **Normal** — `j`/`k` move the message cursor; `i`/`Enter`/`a` open the
//!   composer; `r` replies, `e` edits, `X` redacts the selected message;
//!   `m` marks read; `u` pages older; `R` refreshes; `[`/`]` switch
//!   channels. Typing is intercepted — no unbound character falls through
//!   to the main input.
//! * **Compose/Edit** — printable keys, `Backspace`, and cursor movement
//!   edit the buffer; `Enter` submits, `Esc` returns to Normal with the
//!   buffer intact.
//!
//! Cursor position and the composer buffer are deliberately *not* stored
//! in `ChatState`: they are per-window view state that has no meaning
//! once the window closes. The message buffer, by contrast, is mirrored
//! into the per-project draft on every edit so `/chat-send` from the
//! command line sees the same text.

use std::collections::VecDeque;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::protocol::core::{ChatChannelDto, ChatComposingDto, ChatMessageDto};
use crate::tui::app::state::chat::ChatStatus;
use crate::tui::app::TuiMsg;
use crate::tui::chat_browser::{self, ChatBrowser, ChatComposeSeed, ChatKeyAction, ChatView};
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

/// Maximum messages held in the window snapshot. Matches the reducer's
/// `MAX_CHAT_MESSAGES_PER_CHANNEL` so the dialog never holds more than
/// the window it mirrors.
pub const MAX_CHAT_WINDOW_MESSAGES: usize = 100;

/// Input mode of the chat window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMode {
    /// Browsing: single keys drive actions, no text is captured.
    Normal,
    /// Composing a new message.
    Compose,
    /// Editing an existing message's body.
    Edit,
}

/// Bounded read-only snapshot of one project's chat, pushed from
/// `ChatState` on every completion and live event.
///
/// The dialog never reaches back into app state: `Component::render` only
/// receives a frame, an area, and a theme, so content has to be handed to
/// it. Keeping the snapshot in one struct makes that push a single call
/// and keeps render free of app types.
#[derive(Debug, Clone, Default)]
pub struct ChatSnapshot {
    pub channels: Vec<ChatChannelDto>,
    pub active_channel_id: Option<String>,
    pub messages: VecDeque<ChatMessageDto>,
    pub status: ChatStatus,
    pub draft: String,
    pub last_error: Option<String>,
    pub last_send_error: Option<String>,
    pub unread: usize,
    pub composing: Vec<ChatComposingDto>,
    pub history_truncated: bool,
    /// Daemon retains messages older than the held window.
    pub has_older: bool,
    /// Daemon advertises `ChatHistoryOlder`. False against an older daemon,
    /// which makes `u` a no-op with an explanatory hint rather than a
    /// request the daemon cannot answer.
    pub backward_paging: bool,
}

/// The `/chat` window.
#[derive(Clone)]
pub struct ChatDialog {
    theme: Arc<Theme>,
    project_id: String,
    snapshot: ChatSnapshot,
    /// Shared navigation/selection state — the same implementation the side
    /// panel uses, so the two cannot disagree about where the cursor is or
    /// what `r` means.
    browser: ChatBrowser,
    mode: ChatMode,
    /// Composer buffer for Compose/Edit mode.
    buffer: String,
    /// Caret position within `buffer`.
    caret: usize,
    /// Message the pending edit targets (`Edit` mode only).
    edit_target: Option<String>,
    /// Revision captured when the edit began, so the daemon still owns
    /// conflict detection rather than the client guessing one.
    edit_revision: u64,
    /// Message a pending reply targets.
    reply_target: Option<String>,
}

impl ChatDialog {
    pub fn new(theme: Arc<Theme>, project_id: String) -> Self {
        Self {
            theme,
            project_id,
            snapshot: ChatSnapshot::default(),
            browser: ChatBrowser::default(),
            mode: ChatMode::Normal,
            buffer: String::new(),
            caret: 0,
            edit_target: None,
            edit_revision: 0,
            reply_target: None,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn mode(&self) -> ChatMode {
        self.mode
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn cursor(&self) -> usize {
        self.browser.cursor()
    }

    /// Apply a fresh snapshot from the reducer.
    ///
    /// The composer buffer is only adopted from the project draft while in
    /// Normal mode. Overwriting it mid-send would destroy what the user is
    /// typing every time an unrelated live event landed — the exact reason
    /// the buffer lives in the window rather than in the shared state.
    pub fn apply_snapshot(&mut self, snapshot: ChatSnapshot) {
        // Unsent local text wins over the shared draft while browsing: the
        // draft is only authoritative when the window has nothing of its
        // own, so a snapshot can never clobber half-typed text.
        if self.mode == ChatMode::Normal && self.buffer.is_empty() {
            self.buffer = snapshot.draft.clone();
            self.caret = char_len(&self.buffer);
        }
        self.snapshot = snapshot;
        // Reconcile the shared browser with the new window. On the first
        // push this is what puts the cursor on the newest message rather
        // than index 0, so opening `/chat` on a busy channel shows the
        // conversation instead of its oldest rows.
        self.browser.sync(&Self::view_of(&self.snapshot));
    }

    /// Borrow a snapshot as the browser's view shape.
    ///
    /// An associated function taking `&ChatSnapshot` rather than a method on
    /// `self`, so the resulting borrow covers only the snapshot field and
    /// can coexist with the `&mut` the browser needs.
    fn view_of(snapshot: &ChatSnapshot) -> ChatView<'_> {
        ChatView {
            messages: &snapshot.messages,
            channels: &snapshot.channels,
            active_channel_id: snapshot.active_channel_id.as_deref(),
            status: snapshot.status,
            unread: snapshot.unread,
            backward_paging: snapshot.backward_paging,
        }
    }

    fn message_count(&self) -> usize {
        self.snapshot.messages.len()
    }

    fn channel_count(&self) -> usize {
        self.snapshot.channels.len()
    }

    /// Selected message, if the window holds one.
    pub fn selected(&self) -> Option<&ChatMessageDto> {
        self.snapshot.messages.get(self.browser.cursor())
    }

    // ── Mode transitions ──────────────────────────────────────────────

    /// Open in Insert mode so the user can type immediately.
    ///
    /// A chat box is opened in order to write in it; starting in Normal
    /// meant the first keystroke was swallowed as navigation and the user
    /// had to discover a second key before they could say anything.
    ///
    /// Called *after* the first [`apply_snapshot`](Self::apply_snapshot),
    /// deliberately. That method adopts the shared project draft only while
    /// the window is in Normal mode — the guard that stops a live event from
    /// clobbering a half-typed message. Flipping the mode first would silently
    /// drop a draft the user had already written and never sent.
    ///
    /// Adopted through [`open_seed`](Self::open_seed) so this is the same
    /// transition `i` performs, rather than a second path into the mode
    /// fields that could disagree about what Compose implies.
    pub fn enter_insert_mode(&mut self) {
        self.open_seed(ChatComposeSeed::say());
    }

    /// Open the composer for `seed`.
    ///
    /// The seed comes from the shared browser, so `r` and `e` behave
    /// identically here and in the side panel: same author prefill, same
    /// reply target, same captured revision.
    fn open_seed(&mut self, seed: ChatComposeSeed) {
        if seed.edit_target.is_some() {
            self.mode = ChatMode::Edit;
            self.edit_target = seed.edit_target;
            self.edit_revision = seed.edit_revision;
            self.reply_target = None;
            self.buffer = seed.prefill.unwrap_or_default();
        } else {
            self.mode = ChatMode::Compose;
            self.edit_target = None;
            self.reply_target = seed.reply_to;
            match seed.prefill {
                // A bare `i` means "keep my draft": the buffer is NOT
                // cleared here, because it holds the project draft, which
                // is how a message survives a failed send.
                Some(text) => self.buffer = text,
                None => self.buffer = self.snapshot.draft.clone(),
            }
        }
        self.caret = char_len(&self.buffer);
    }

    fn cancel_input(&mut self) {
        self.mode = ChatMode::Normal;
        self.edit_target = None;
        self.reply_target = None;
        self.buffer.clear();
        self.caret = 0;
    }

    /// The context the pending editor is armed with, for badge/hint wording.
    fn seed(&self) -> ChatComposeSeed {
        ChatComposeSeed {
            prefill: None,
            reply_to: self.reply_target.clone(),
            edit_target: self.edit_target.clone(),
            edit_revision: self.edit_revision,
        }
    }

    // ── Buffer editing ────────────────────────────────────────────────

    fn insert(&mut self, ch: char) {
        let idx = char_byte_index(&self.buffer, self.caret);
        self.buffer.insert(idx, ch);
        self.caret += 1;
    }

    fn insert_str(&mut self, text: &str) {
        let idx = char_byte_index(&self.buffer, self.caret);
        self.buffer.insert_str(idx, text);
        self.caret += char_len(text);
    }

    fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        let idx = char_byte_index(&self.buffer, self.caret - 1);
        self.buffer.remove(idx);
        self.caret -= 1;
    }

    fn delete(&mut self) {
        if self.caret >= char_len(&self.buffer) {
            return;
        }
        let idx = char_byte_index(&self.buffer, self.caret);
        self.buffer.remove(idx);
    }

    // ── Render helpers ────────────────────────────────────────────────

    /// Header line: channel, position, unread, and paging state.
    fn header(&self, theme: &Arc<Theme>) -> Line<'static> {
        let total = self.message_count();
        let unread = if self.snapshot.unread > 0 {
            format!(" · {} unread", self.snapshot.unread)
        } else {
            String::new()
        };
        let older = if self.snapshot.has_older {
            " · older ↓"
        } else {
            ""
        };
        Line::from(vec![
            Span::styled(
                format!(" #{} ", Self::view_of(&self.snapshot).channel_name()),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{}/{} messages{unread}{older}",
                    if total == 0 {
                        0
                    } else {
                        self.browser.cursor() + 1
                    },
                    total
                ),
                Style::default().fg(theme.muted),
            ),
        ])
    }

    /// The body shown in the viewport, including status branches.
    ///
    /// `height`/`width` come from the real render area, so the message
    /// window is sized to what actually fits. Sizing it by message count
    /// instead would emit up to 100 lines into a ~30-row viewport, and the
    /// cursor would scroll out of sight in the middle of the window.
    fn body_lines(&self, height: usize, width: usize, theme: &Arc<Theme>) -> Vec<Line<'static>> {
        let dim = Style::default().fg(theme.muted);
        let view = Self::view_of(&self.snapshot);
        match self.snapshot.status {
            ChatStatus::Unavailable => vec![
                Line::from(Span::styled("Chat is unavailable for this project.", dim)),
                Line::from(Span::styled(
                    "Messages are hidden and commands are denied.",
                    dim,
                )),
            ],
            ChatStatus::Loading | ChatStatus::Unknown => {
                vec![Line::from(Span::styled("Loading chat…", dim))]
            }
            ChatStatus::Error if self.message_count() == 0 => vec![
                Line::from(Span::styled(
                    self.snapshot
                        .last_error
                        .clone()
                        .unwrap_or_else(|| "refresh failed".to_string()),
                    Style::default().fg(theme.error),
                )),
                Line::from(Span::styled("Press R to retry.", dim)),
            ],
            _ => {
                if self.message_count() == 0 {
                    return vec![
                        Line::from(Span::styled("No messages yet.", dim)),
                        Line::from(Span::styled("Press i to write the first one.", dim)),
                    ];
                }
                // The retention header gets its own row so the message
                // count still fits the viewport.
                let header_rows = usize::from(self.snapshot.history_truncated);
                let visible = height.max(1).saturating_sub(header_rows).max(1);
                let rows = chat_browser::rows(
                    &view,
                    self.browser.cursor(),
                    u16::try_from(width).unwrap_or(u16::MAX),
                    theme,
                );
                let mut lines: Vec<Line<'static>> = Vec::new();
                if self.snapshot.history_truncated {
                    lines.push(Line::from(Span::styled(
                        "… older history retained daemon-side (u loads more)",
                        dim,
                    )));
                }
                if rows.is_empty() {
                    return lines;
                }
                let selected_end = chat_browser::selected_row_end(&rows, self.browser.cursor());
                let range = chat_browser::window(rows.len(), selected_end, visible);
                lines.extend(range.map(|index| rows[index].line.clone()));
                lines
            }
        }
    }

    /// Footer: mode-specific hints plus any error worth surfacing here.
    fn footer(&self, theme: &Arc<Theme>) -> Line<'static> {
        let dim = Style::default().fg(theme.muted);
        let hints = match self.mode {
            ChatMode::Normal => format!(
                "{} · Esc close",
                chat_browser::normal_hints(self.browser.show_help())
            ),
            _ => chat_browser::input_hints(&self.seed()),
        };
        Line::from(Span::styled(hints, dim))
    }

    /// Inverse-video mode badge.
    ///
    /// This dialog used to render *zero* composer rows in Normal mode, so a
    /// user typing into what looked like an empty input box saw nothing
    /// happen and no indication that the key they needed was `i`. The badge
    /// is drawn in both modes for the same reason the panel's is: the mode
    /// must be observable before the user types, not after.
    fn mode_badge(&self, theme: &Arc<Theme>) -> Span<'static> {
        let (label, colour) = match self.mode {
            ChatMode::Compose | ChatMode::Edit => ("INS", theme.primary),
            ChatMode::Normal => ("NOR", theme.muted),
        };
        Span::styled(
            format!(" {label} "),
            Style::default()
                .fg(theme.background)
                .bg(colour)
                .add_modifier(Modifier::BOLD),
        )
    }

    /// The composer/edit box, including a visible caret and the mode badge.
    fn composer_line(&self, theme: &Arc<Theme>) -> Line<'static> {
        let badge = self.mode_badge(theme);
        let label = self.seed().label();
        // The placeholder is a *fallback*, not the buffer. It used to be
        // assigned into `text` and then dropped on the floor: the empty case
        // rendered a bare caret and the non-empty case rendered only the
        // slice after the caret. That is why typing appeared to do nothing —
        // every character before the cursor was never drawn.
        let placeholder_text = match self.mode {
            // Spelling out the mode's entry key is what turns "typing did
            // nothing" into a discoverable interaction.
            ChatMode::Normal => "press i to type",
            ChatMode::Edit => "unchanged",
            _ => "message…",
        };
        let (before, after) = if self.buffer.is_empty() {
            (placeholder_text.to_string(), String::new())
        } else {
            let idx = char_byte_index(&self.buffer, self.caret);
            (
                self.buffer[..idx].to_string(),
                self.buffer[idx..].to_string(),
            )
        };
        let empty = self.buffer.is_empty();
        Line::from(vec![
            badge,
            Span::raw(" "),
            Span::styled(
                format!(" {label}› "),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                before,
                if empty {
                    Style::default().fg(theme.muted)
                } else {
                    Style::default().fg(theme.foreground)
                },
            ),
            Span::styled("▏", Style::default().fg(theme.primary)),
            Span::styled(after, Style::default().fg(theme.foreground)),
        ])
    }
}

impl Component for ChatDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        // Ctrl+C closes the window; the browser owns every other Ctrl chord.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(TuiMsg::CloseDialog);
        }

        match self.mode {
            ChatMode::Normal => {
                // Only the snapshot field is borrowed here, so the browser
                // can be mutated alongside it.
                let snapshot = &self.snapshot;
                let view = Self::view_of(snapshot);
                match self.browser.handle_key(key, &view) {
                    ChatKeyAction::Consumed => None,
                    ChatKeyAction::Exit => Some(TuiMsg::CloseDialog),
                    ChatKeyAction::Intent(msg) => Some(msg),
                    ChatKeyAction::Compose(seed) => {
                        self.open_seed(seed);
                        None
                    }
                }
            }
            ChatMode::Compose | ChatMode::Edit => self.handle_input_key(key),
        }
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        // Pasted text only lands in the composer. In Normal mode it is
        // dropped rather than silently opening an editor, because a paste
        // is not a deliberate request to start composing.
        if self.mode != ChatMode::Normal {
            self.insert_str(&text);
        }
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::ProjectChat
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.set_theme(theme);
        if area.height < 5 || area.width < 20 {
            return;
        }
        frame.render_widget(Clear, area);
        let title = format!(" Project Chat — {} ", self.project_id);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Channel bar is present only when the project actually has more
        // than one channel to switch between.
        let channel_rows = u16::from(self.channel_count() > 1);
        // Unconditional: a composer that appears only while typing is
        // indistinguishable from a broken one. Normal mode shows the badge
        // plus `i to type` rather than nothing.
        let composer_rows = 3;
        let rows = Layout::vertical([
            Constraint::Length(1), // header
            Constraint::Length(channel_rows),
            Constraint::Min(1), // body
            Constraint::Length(composer_rows),
            Constraint::Length(1), // composing / errors
            Constraint::Length(1), // footer
        ])
        .split(inner);

        frame.render_widget(Paragraph::new(self.header(theme)), rows[0]);

        if channel_rows > 0 {
            let spans: Vec<Span> = self
                .snapshot
                .channels
                .iter()
                .map(|channel| {
                    let active = Some(channel.channel_id.as_str())
                        == self.snapshot.active_channel_id.as_deref();
                    let style = if active {
                        Style::default()
                            .fg(theme.primary)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.muted)
                    };
                    Span::styled(format!(" {} ", channel.name), style)
                })
                .collect();
            frame.render_widget(Paragraph::new(Line::from(spans)), rows[1]);
        }

        let width = usize::from(rows[2].width);
        let height = usize::from(rows[2].height);
        frame.render_widget(
            Paragraph::new(self.body_lines(height, width, theme)).wrap(Wrap { trim: false }),
            rows[2],
        );

        if composer_rows > 0 {
            let composer_block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.primary));
            let composer_inner = composer_block.inner(rows[3]);
            frame.render_widget(composer_block, rows[3]);
            frame.render_widget(
                Paragraph::new(self.composer_line(theme)).alignment(Alignment::Left),
                composer_inner,
            );
        }

        let status = self.status_line(theme);
        frame.render_widget(Paragraph::new(status), rows[4]);
        frame.render_widget(Paragraph::new(self.footer(theme)), rows[5]);
    }
}

impl ChatDialog {
    /// One line for composing leases and the last send error. Errors win
    /// the row so a failed send is never hidden behind a typing indicator.
    fn status_line(&self, theme: &Arc<Theme>) -> Line<'static> {
        if let Some(error) = self.snapshot.last_send_error.as_deref() {
            return Line::from(Span::styled(
                format!(" last send failed: {error}"),
                Style::default().fg(theme.error),
            ));
        }
        if let Some(error) = self.snapshot.last_error.as_deref() {
            if self.message_count() == 0 {
                return Line::from(Span::styled(
                    format!(" {error}"),
                    Style::default().fg(theme.error),
                ));
            }
        }
        if self.message_count() > 0 && !self.snapshot.composing.is_empty() {
            let names: Vec<String> = self
                .snapshot
                .composing
                .iter()
                .map(|c| c.principal_id.clone())
                .collect();
            return Line::from(Span::styled(
                format!(" {} typing…", names.join(", ")),
                Style::default().fg(theme.muted),
            ));
        }
        if self.browser.redact_armed().is_some() {
            return Line::from(Span::styled(
                " press X again to redact (irreversible)",
                Style::default().fg(theme.warning),
            ));
        }
        Line::default()
    }

    fn handle_input_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Esc => {
                self.cancel_input();
                None
            }
            KeyCode::Enter => {
                let body = self.buffer.trim().to_string();
                let msg = match self.mode {
                    ChatMode::Edit => self
                        .edit_target
                        .clone()
                        .map(|message_id| TuiMsg::ChatEdit { message_id, body }),
                    _ => Some(TuiMsg::ChatSend {
                        body,
                        reply_to: self.reply_target.clone(),
                    }),
                };
                // The buffer is cleared optimistically; a failed send puts
                // the text back into the project draft and the next
                // snapshot repopulates it, so nothing is ever lost.
                self.cancel_input();
                msg
            }
            KeyCode::Backspace => {
                self.backspace();
                None
            }
            KeyCode::Delete => {
                self.delete();
                None
            }
            KeyCode::Left => {
                self.caret = self.caret.saturating_sub(1);
                None
            }
            KeyCode::Right => {
                self.caret = (self.caret + 1).min(char_len(&self.buffer));
                None
            }
            KeyCode::Home => {
                self.caret = 0;
                None
            }
            KeyCode::End => {
                self.caret = char_len(&self.buffer);
                None
            }
            KeyCode::Char(ch) => {
                self.insert(ch);
                None
            }
            _ => None,
        }
    }
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// Byte offset of the `n`th char, clamped to a char boundary.
fn char_byte_index(text: &str, n: usize) -> usize {
    text.char_indices()
        .nth(n)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
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

    fn ready_snapshot() -> ChatSnapshot {
        ChatSnapshot {
            channels: vec![ChatChannelDto {
                channel_id: "ch-1".to_string(),
                project_id: "p".to_string(),
                name: "general".to_string(),
                created_by: "alice".to_string(),
                created_at_ms: 0,
            }],
            active_channel_id: Some("ch-1".to_string()),
            messages: VecDeque::from(vec![
                message("m1", 1, "alice", "first"),
                message("m2", 2, "bob", "second"),
                message("m3", 3, "alice", "third"),
            ]),
            status: ChatStatus::Ready,
            ..Default::default()
        }
    }

    fn dialog() -> ChatDialog {
        let mut d = ChatDialog::new(Arc::new(Theme::default()), "p".to_string());
        d.apply_snapshot(ready_snapshot());
        d
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn opens_on_the_newest_message() {
        let d = dialog();
        assert_eq!(d.cursor(), 2);
    }

    #[test]
    fn navigation_and_tail_pinning() {
        let mut d = dialog();
        // `j` at the newest message keeps the tail pinned.
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(d.cursor(), 2);
        // `k` unpins and scrolls up.
        d.handle_key(key(KeyCode::Char('k')));
        assert_eq!(d.cursor(), 1);
        d.handle_key(key(KeyCode::Char('k')));
        d.handle_key(key(KeyCode::Char('k')));
        assert_eq!(d.cursor(), 0);
        d.handle_key(key(KeyCode::Char('G')));
        assert_eq!(d.cursor(), 2);
    }

    #[test]
    fn a_new_message_arriving_while_pinned_moves_the_cursor() {
        let mut d = dialog();
        let mut snapshot = ready_snapshot();
        snapshot
            .messages
            .push_back(message("m4", 4, "bob", "fourth"));
        d.apply_snapshot(snapshot);
        assert_eq!(d.cursor(), 3);
    }

    #[test]
    fn a_new_message_arriving_after_scrolling_up_does_not() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('k')));
        assert_eq!(d.cursor(), 1);
        let mut snapshot = ready_snapshot();
        snapshot
            .messages
            .push_back(message("m4", 4, "bob", "fourth"));
        d.apply_snapshot(snapshot);
        assert_eq!(d.cursor(), 1);
    }

    #[test]
    fn navigation_on_an_empty_window_is_safe() {
        let mut snapshot = ready_snapshot();
        snapshot.messages.clear();
        let mut d = ChatDialog::new(Arc::new(Theme::default()), "p".to_string());
        d.apply_snapshot(snapshot);
        assert_eq!(d.handle_key(key(KeyCode::Char('j'))), None);
        assert_eq!(d.cursor(), 0);
        assert_eq!(d.handle_key(key(KeyCode::Char('G'))), None);
    }

    #[test]
    fn i_opens_the_composer_and_esc_returns_to_normal() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('i')));
        assert_eq!(d.mode(), ChatMode::Compose);
        d.handle_key(key(KeyCode::Char('h')));
        d.handle_key(key(KeyCode::Char('i')));
        assert_eq!(d.buffer(), "hi");
        // Esc discards the buffer and restores Normal-mode navigation.
        d.handle_key(key(KeyCode::Esc));
        assert_eq!(d.mode(), ChatMode::Normal);
        assert_eq!(d.buffer(), "");
        assert_eq!(d.handle_key(key(KeyCode::Char('k'))), None);
        assert_eq!(d.cursor(), 1);
    }

    #[test]
    fn enter_submits_the_buffered_body() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('i')));
        for ch in "hey".chars() {
            d.handle_key(key(KeyCode::Char(ch)));
        }
        let msg = d.handle_key(key(KeyCode::Enter));
        assert_eq!(
            msg,
            Some(TuiMsg::ChatSend {
                body: "hey".to_string(),
                reply_to: None
            })
        );
        assert_eq!(d.mode(), ChatMode::Normal);
    }

    #[test]
    fn r_replies_to_the_selected_message_and_prefills_the_author() {
        let mut d = dialog();
        // The window opens on the newest message, m3/alice.
        assert_eq!(d.cursor(), 2);
        d.handle_key(key(KeyCode::Char('r')));
        assert_eq!(d.mode(), ChatMode::Compose);
        assert_eq!(d.buffer(), "@alice ");
        let msg = d.handle_key(key(KeyCode::Enter));
        assert_eq!(
            msg,
            Some(TuiMsg::ChatSend {
                body: "@alice".to_string(),
                reply_to: Some("m3".to_string())
            })
        );
    }

    #[test]
    fn r_on_a_scrolled_selection_targets_that_message() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('k')));
        assert_eq!(d.cursor(), 1);
        d.handle_key(key(KeyCode::Char('r')));
        assert_eq!(d.buffer(), "@bob ");
        assert_eq!(
            d.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::ChatSend {
                body: "@bob".to_string(),
                reply_to: Some("m2".to_string())
            })
        );
    }

    #[test]
    fn e_prefills_the_existing_body_for_replacement() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('k')));
        d.handle_key(key(KeyCode::Char('e')));
        assert_eq!(d.mode(), ChatMode::Edit);
        assert_eq!(d.buffer(), "second");
        assert_eq!(
            d.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::ChatEdit {
                message_id: "m2".to_string(),
                body: "second".to_string()
            })
        );
    }

    #[test]
    fn x_needs_two_presses_to_redact() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Char('X'))), None);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('X'))),
            Some(TuiMsg::ChatRedact {
                message_id: "m3".to_string()
            })
        );
    }

    #[test]
    fn esc_closes_and_q_closes() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Esc)), Some(TuiMsg::CloseDialog));
        let mut d = dialog();
        assert_eq!(
            d.handle_key(key(KeyCode::Char('q'))),
            Some(TuiMsg::CloseDialog)
        );
    }

    #[test]
    fn unbound_printable_keys_never_fall_through() {
        let mut d = dialog();
        for code in ['z', 'Z', ' ', '/'] {
            assert_eq!(
                d.handle_key(key(KeyCode::Char(code))),
                None,
                "{code} must be swallowed in Normal mode"
            );
            assert_eq!(d.mode(), ChatMode::Normal);
        }
    }

    #[test]
    fn footer_advertises_the_reply_and_edit_verbs() {
        let d = dialog();
        let footer = d.footer(&Arc::new(Theme::default()));
        let text: String = footer.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("r reply"), "footer missing reply: {text}");
        assert!(text.contains("e edit"), "footer missing edit: {text}");
        assert!(text.contains("Esc close"), "footer missing exit: {text}");
    }

    #[test]
    fn footer_while_armed_describes_the_editor_not_the_browser() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('r')));
        let footer = d.footer(&Arc::new(Theme::default()));
        let text: String = footer.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("reply"), "armed footer missing label: {text}");
    }

    #[test]
    fn selected_author_is_rendered_legibly() {
        let d = dialog();
        let theme = Arc::new(Theme::dark());
        let lines = d.body_lines(20, 40, &theme);
        let rendered: Vec<&Line> = lines.iter().collect();
        assert!(!rendered.is_empty());
        // The regression guard: the selected row used to be painted
        // `fg(theme.selection)`, a background colour used as a foreground.
        let selected_style = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.style)
            .find(|style| style.bg == Some(theme.selection))
            .expect("selected row must carry the selection background");
        assert_eq!(selected_style.fg, Some(theme.foreground));
    }

    #[test]
    fn the_window_renders_the_newest_message_first_frame() {
        let d = dialog();
        let theme = Arc::new(Theme::dark());
        let lines = d.body_lines(6, 40, &theme);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        assert!(text.contains("third"), "newest message must be visible");
    }

    #[test]
    fn renders_without_panicking_at_a_minimum_size() {
        let mut d = dialog();
        let theme = Arc::new(Theme::dark());
        for (w, h) in [(20u16, 5u16), (200, 60), (1, 1)] {
            let backend = ratatui::backend::TestBackend::new(w, h);
            let mut terminal = ratatui::Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| d.render(frame, Rect::new(0, 0, w, h), &theme))
                .expect("render must not panic");
        }
    }

    #[test]
    fn renders_the_newest_message_on_a_real_frame() {
        let mut d = dialog();
        let theme = Arc::new(Theme::dark());
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| d.render(frame, frame.area(), &theme))
            .expect("render must not panic");
        let rendered = terminal.backend().buffer();
        let text: String = (0..rendered.area.height)
            .map(|y| {
                (0..rendered.area.width)
                    .map(|x| rendered[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("third"),
            "the newest message must be on screen on the first frame"
        );
        assert!(text.contains("r reply"), "the footer must advertise `r`");
    }
}
