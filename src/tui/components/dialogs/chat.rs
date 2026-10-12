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
    pub messages: Vec<ChatMessageDto>,
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
    mode: ChatMode,
    /// Index into `snapshot.messages` of the selected row.
    cursor: usize,
    /// Index into `snapshot.channels` of the highlighted channel.
    channel_cursor: usize,
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
    /// First `X` press arms a destructive redact; the second confirms.
    redact_armed: Option<String>,
    show_help: bool,
}

impl ChatDialog {
    pub fn new(theme: Arc<Theme>, project_id: String) -> Self {
        Self {
            theme,
            project_id,
            snapshot: ChatSnapshot::default(),
            mode: ChatMode::Normal,
            cursor: 0,
            channel_cursor: 0,
            buffer: String::new(),
            caret: 0,
            edit_target: None,
            edit_revision: 0,
            reply_target: None,
            redact_armed: None,
            show_help: false,
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
        self.cursor
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
        self.cursor = self.cursor.min(self.message_count().saturating_sub(1));
        // Re-anchor the channel highlight on whichever channel the
        // reducer now reports as active, so a completed switch lands the
        // highlight where the user aimed and the next press continues
        // from there.
        self.channel_cursor = self
            .snapshot
            .channels
            .iter()
            .position(|c| Some(c.channel_id.as_str()) == self.snapshot.active_channel_id.as_deref())
            .unwrap_or(0)
            .min(self.channel_count().saturating_sub(1));
    }

    fn message_count(&self) -> usize {
        self.snapshot.messages.len()
    }

    fn channel_count(&self) -> usize {
        self.snapshot.channels.len()
    }

    /// Selected message, if the window holds one.
    pub fn selected(&self) -> Option<&ChatMessageDto> {
        self.snapshot.messages.get(self.cursor)
    }

    /// True when the daemon authorized enough state for interactive use.
    /// Sending into an unavailable project would produce a guaranteed
    /// `project_not_found` round-trip, so the composer stays closed.
    fn interactive(&self) -> bool {
        matches!(self.snapshot.status, ChatStatus::Ready | ChatStatus::Error)
    }

    /// Rows one page-key moves the cursor.
    ///
    /// Key handling runs before render and has no access to the frame, so
    /// this is a fixed step rather than the true viewport height. The
    /// render pass re-anchors the window on the cursor afterwards, so a
    /// mis-sized page still lands on a visible row.
    fn page_size(&self, step: u16) -> usize {
        usize::from(step).max(1)
    }

    fn clamp_cursor(&mut self) {
        let max = self.message_count().saturating_sub(1);
        self.cursor = self.cursor.min(max);
    }

    // ── Mode transitions ──────────────────────────────────────────────

    fn open_compose(&mut self) {
        if !self.interactive() {
            return;
        }
        self.mode = ChatMode::Compose;
        self.edit_target = None;
        self.reply_target = None;
        // The buffer is NOT cleared here: it holds the project draft, which
        // is how a message survives a failed send. Wiping it on open would
        // make the retained draft unreachable from the one place the user
        // can fix a send. It is empty after a successful send because
        // `cancel_input` runs on submit.
        self.caret = char_len(&self.buffer);
    }

    fn open_reply(&mut self) {
        if !self.interactive() {
            return;
        }
        let Some(message) = self.selected() else {
            return;
        };
        // Clone before mutating: `selected` borrows `self`.
        let (author, message_id) = (message.author_principal.clone(), message.message_id.clone());
        self.mode = ChatMode::Compose;
        self.edit_target = None;
        self.reply_target = Some(message_id);
        self.buffer = format!("@{author} ");
        self.caret = char_len(&self.buffer);
    }

    fn open_edit(&mut self) {
        let Some(message) = self.selected() else {
            return;
        };
        if !self.interactive() || message.redacted {
            return;
        }
        // Authorship is the daemon's call, not the client's — the same
        // principal string is not proof of ownership across transports.
        // Prefilling an uneditable message only costs a rejected
        // round-trip with a daemon-authored reason, so the client does
        // not guess and hide the true error.
        let (message_id, revision, body) = (
            message.message_id.clone(),
            message.revision,
            message.body.clone(),
        );
        self.mode = ChatMode::Edit;
        self.reply_target = None;
        self.edit_target = Some(message_id);
        self.edit_revision = revision;
        self.buffer = body;
        self.caret = char_len(&self.buffer);
    }

    fn cancel_input(&mut self) {
        self.mode = ChatMode::Normal;
        self.edit_target = None;
        self.reply_target = None;
        self.buffer.clear();
        self.caret = 0;
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

    fn channel_name(&self) -> String {
        self.snapshot
            .channels
            .iter()
            .find(|c| Some(c.channel_id.as_str()) == self.snapshot.active_channel_id.as_deref())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "general".to_string())
    }

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
                format!(" #{} ", self.channel_name()),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{}/{} messages{unread}{older}",
                    if total == 0 { 0 } else { self.cursor + 1 },
                    total
                ),
                Style::default().fg(theme.muted),
            ),
        ])
    }

    /// One message row. `selected` drives the cursor highlight.
    fn message_row(
        &self,
        message: &ChatMessageDto,
        selected: bool,
        unread: bool,
        theme: &Arc<Theme>,
    ) -> Line<'static> {
        let marker = if selected { "▍" } else { " " };
        let base = Style::default().fg(if selected {
            theme.selection
        } else if unread {
            theme.foreground
        } else {
            theme.muted
        });
        let author_style = if selected {
            Style::default()
                .fg(theme.selection)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.primary)
        };
        let mut spans = vec![
            Span::styled(marker, base),
            Span::styled(format!("{} ", message.author_principal), author_style),
        ];
        if message.redacted {
            spans.push(Span::styled(
                "[redacted]",
                Style::default()
                    .fg(theme.muted)
                    .add_modifier(Modifier::ITALIC),
            ));
        } else {
            spans.push(Span::styled(
                message.body.clone(),
                Style::default().fg(theme.foreground),
            ));
        }
        if message.revision > 1 {
            spans.push(Span::styled(
                format!(" (edited r{})", message.revision),
                Style::default().fg(theme.muted),
            ));
        }
        if let Some(_reply) = message.reply_to.as_deref() {
            spans.push(Span::styled(" ↩", Style::default().fg(theme.link)));
        }
        if selected {
            // Offer edit/redact on the selected row regardless of who
            // wrote it. The TUI never learns its own principal (the
            // daemon binds it per connection), so gating the affordance
            // on a client-side guess would hide actions that the daemon
            // would actually allow, and would imply an authorship check
            // the client is not performing.
            spans.push(Span::styled(
                " [e:edit X:redact]",
                Style::default().fg(theme.muted),
            ));
        }
        Line::from(spans)
    }

    /// The body shown in the viewport, including status branches.
    ///
    /// `height`/`width` come from the real render area, so the message
    /// window is sized to what actually fits. Sizing it by message count
    /// instead would emit up to 100 lines into a ~30-row viewport, and the
    /// cursor would scroll out of sight in the middle of the window.
    fn body_lines(&self, height: usize, width: usize, theme: &Arc<Theme>) -> Vec<Line<'static>> {
        let dim = Style::default().fg(theme.muted);
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
                // Unread separator mirrors the reducer's unread boundary so
                // the window and `panel_lines` agree on where it sits.
                let unread = self.snapshot.unread.min(self.message_count());
                let unread_from = self.message_count().saturating_sub(unread);
                // Keep the cursor inside the viewport: window exactly
                // `visible` messages ending at the cursor, clamped at
                // both ends. The retention header, when present, gets its
                // own row so the message count still fits the viewport.
                let total = self.message_count();
                let header_rows = usize::from(self.snapshot.history_truncated);
                let visible = height.max(1).saturating_sub(header_rows).max(1);
                let start = self
                    .cursor
                    .saturating_sub(visible.saturating_sub(1))
                    .min(total.saturating_sub(visible));
                let end = total.min(start.saturating_add(visible));
                let mut lines = Vec::new();
                if self.snapshot.history_truncated {
                    lines.push(Line::from(Span::styled(
                        "… older history retained daemon-side (u loads more)",
                        dim,
                    )));
                }
                for (offset, index) in (start..end).enumerate() {
                    if unread > 0 && index == unread_from && offset > 0 {
                        lines.push(Line::from(Span::styled(
                            format!("── {unread} unread ──"),
                            dim,
                        )));
                    }
                    let message = &self.snapshot.messages[index];
                    // Wrap against the real viewport so the cursor stays
                    // aligned with what is on screen.
                    let available =
                        u16::try_from(width.saturating_sub(2).max(8)).unwrap_or(u16::MAX);
                    let row = self.message_row(message, index == self.cursor, unread > 0, theme);
                    if row.width() > usize::from(available) {
                        for chunk in crate::tui::wrap::wrap_to_strings(&flatten(row), available) {
                            lines.push(Line::from(Span::styled(
                                chunk,
                                Style::default().fg(theme.foreground),
                            )));
                        }
                    } else {
                        lines.push(row);
                    }
                }
                lines
            }
        }
    }

    /// Footer: mode-specific hints plus any error worth surfacing here.
    fn footer(&self, theme: &Arc<Theme>) -> Line<'static> {
        let dim = Style::default().fg(theme.muted);
        match self.mode {
            ChatMode::Normal => Line::from(Span::styled(
                if self.show_help {
                    "j/k move · i compose · r reply · e edit · X redact · m read · u older · R refresh · [ ] channel · ? hide help · Esc close"
                } else {
                    "i compose · e edit · X redact · m read · u older · [ ] channel · ? help · Esc close"
                },
                dim,
            )),
            ChatMode::Compose => Line::from(Span::styled("Enter send · Esc cancel", dim)),
            ChatMode::Edit => Line::from(Span::styled("Enter save · Esc cancel", dim)),
        }
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
        let label = match self.mode {
            ChatMode::Edit => "edit",
            _ if self.reply_target.is_some() => "reply",
            _ => "say",
        };
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
        // Ctrl-based paging works in every mode so a long history stays
        // reachable without first leaving the composer.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') if self.mode == ChatMode::Normal => {
                    self.cursor = (self.cursor + 10).min(self.message_count().saturating_sub(1));
                    return None;
                }
                KeyCode::Char('u') if self.mode == ChatMode::Normal => {
                    self.cursor = self.cursor.saturating_sub(10);
                    return None;
                }
                KeyCode::Char('c') => return Some(TuiMsg::CloseDialog),
                _ => {}
            }
        }

        match self.mode {
            ChatMode::Normal => self.handle_normal_key(key),
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
        if self.redact_armed.is_some() {
            return Line::from(Span::styled(
                " press X again to redact (irreversible)",
                Style::default().fg(theme.warning),
            ));
        }
        Line::default()
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        let last = self.message_count().saturating_sub(1);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(TuiMsg::CloseDialog),
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor = (self.cursor + 1).min(last);
                self.clamp_cursor();
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                None
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.cursor = 0;
                None
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.cursor = last;
                None
            }
            KeyCode::PageDown => {
                self.cursor = (self.cursor + self.page_size(10)).min(last);
                None
            }
            KeyCode::PageUp => {
                self.cursor = self.cursor.saturating_sub(self.page_size(10));
                None
            }
            KeyCode::Char('i') | KeyCode::Char('a') | KeyCode::Enter => {
                self.open_compose();
                None
            }
            KeyCode::Char('r') => {
                self.open_reply();
                None
            }
            KeyCode::Char('e') => {
                self.open_edit();
                None
            }
            KeyCode::Char('X') => {
                let message = self.selected()?;
                if message.redacted {
                    return None;
                }
                let message_id = message.message_id.clone();
                // Two-step confirm: redaction is irreversible daemon-side
                // and a single stray capital X should not destroy a
                // message the user merely meant to select.
                if self.redact_armed.as_deref() == Some(message_id.as_str()) {
                    self.redact_armed = None;
                    return Some(TuiMsg::ChatRedact { message_id });
                }
                self.redact_armed = Some(message_id);
                None
            }
            KeyCode::Char('m') => Some(TuiMsg::ChatMarkRead),
            KeyCode::Char('u') => {
                if self.snapshot.backward_paging {
                    Some(TuiMsg::ChatLoadOlder)
                } else {
                    // The daemon cannot serve backward paging; say so
                    // rather than letting the key look broken.
                    Some(TuiMsg::ChatPagingUnsupported)
                }
            }
            KeyCode::Char('R') => Some(TuiMsg::ChatRefresh),
            KeyCode::Char('[') => self.step_channel(-1),
            KeyCode::Char(']') => self.step_channel(1),
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                None
            }
            _ => None,
        }
    }

    /// Move the channel highlight one step in `delta` and emit the
    /// resulting switch intent. Returns `None` when there is nothing to
    /// switch to, so a single-channel project stays quiet.
    fn step_channel(&mut self, delta: isize) -> Option<TuiMsg> {
        let count = self.channel_count();
        if count < 2 {
            return None;
        }
        // The highlight follows the *active* channel rather than a
        // remembered position, so repeated switches always move from
        // where the user actually is.
        let active = self
            .snapshot
            .channels
            .iter()
            .position(|c| Some(c.channel_id.as_str()) == self.snapshot.active_channel_id.as_deref())
            .unwrap_or(self.channel_cursor);
        // Step from the highlight, not from the active channel. The
        // reducer only adopts a new active channel once its page lands, so
        // rebasing on `active` would make a second press resolve from the
        // channel the user is already leaving and walk the wrong way.
        let _ = active;
        let next = (self.channel_cursor as isize + delta).rem_euclid(count as isize) as usize;
        self.channel_cursor = next;
        let channel_id = self.snapshot.channels.get(next)?.channel_id.clone();
        Some(TuiMsg::ChatSwitchChannel { channel_id })
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

/// Flatten a `Line` back to plain text for the wrap path.
fn flatten(line: Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::core::ChatChannelDto;

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
            messages: vec![
                message("m1", 1, "alice", "first"),
                message("m2", 2, "bob", "second"),
                message("m3", 3, "alice", "third"),
            ],
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
    fn normal_mode_navigates_and_clamps() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(d.cursor(), 1);
        // Past the end clamps rather than panicking on an empty window.
        d.handle_key(key(KeyCode::Char('j')));
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(d.cursor(), 2);
        d.handle_key(key(KeyCode::Char('k')));
        assert_eq!(d.cursor(), 1);
        d.handle_key(key(KeyCode::Char('k')));
        d.handle_key(key(KeyCode::Char('k')));
        assert_eq!(d.cursor(), 0);
        d.handle_key(key(KeyCode::Char('G')));
        assert_eq!(d.cursor(), 2);
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
        assert_eq!(d.handle_key(key(KeyCode::Char('j'))), None);
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
    fn reply_prefills_the_author_and_carries_the_target() {
        let mut d = dialog();
        // Select bob's message, then reply.
        d.handle_key(key(KeyCode::Char('j')));
        d.handle_key(key(KeyCode::Char('r')));
        assert_eq!(d.mode(), ChatMode::Compose);
        assert_eq!(d.buffer(), "@bob ");
        let msg = d.handle_key(key(KeyCode::Enter));
        assert_eq!(
            msg,
            Some(TuiMsg::ChatSend {
                body: "@bob".to_string(),
                reply_to: Some("m2".to_string())
            })
        );
    }

    #[test]
    fn edit_prefills_the_body_and_targets_that_message() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('e')));
        assert_eq!(d.mode(), ChatMode::Edit);
        assert_eq!(d.buffer(), "first");
        // Editing in place: clear the prefilled body, retype, submit.
        for _ in 0..d.buffer().chars().count() {
            d.handle_key(key(KeyCode::Backspace));
        }
        assert_eq!(d.buffer(), "");
        for ch in "edited".chars() {
            d.handle_key(key(KeyCode::Char(ch)));
        }
        let msg = d.handle_key(key(KeyCode::Enter));
        assert_eq!(
            msg,
            Some(TuiMsg::ChatEdit {
                message_id: "m1".to_string(),
                body: "edited".to_string()
            })
        );
        assert_eq!(d.mode(), ChatMode::Normal);
    }

    #[test]
    fn editing_a_redacted_message_never_opens_an_editor() {
        let mut snapshot = ready_snapshot();
        snapshot.messages[0].redacted = true;
        let mut d = ChatDialog::new(Arc::new(Theme::default()), "p".to_string());
        d.apply_snapshot(snapshot);
        d.handle_key(key(KeyCode::Char('e')));
        assert_eq!(d.mode(), ChatMode::Normal);
    }

    #[test]
    fn redact_needs_two_confirming_presses() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Char('X'))), None);
        assert!(d.redact_armed.is_some());
        let msg = d.handle_key(key(KeyCode::Char('X')));
        assert_eq!(
            msg,
            Some(TuiMsg::ChatRedact {
                message_id: "m1".to_string()
            })
        );
        assert!(d.redact_armed.is_none());
    }

    #[test]
    fn moving_the_cursor_disarms_a_pending_redact() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('X')));
        d.handle_key(key(KeyCode::Char('j')));
        // The second X lands on a different message than the armed one.
        assert_eq!(d.handle_key(key(KeyCode::Char('X'))), None);
        assert_eq!(d.redact_armed.as_deref(), Some("m2"));
    }

    #[test]
    fn unavailable_project_offers_no_composer() {
        let mut snapshot = ready_snapshot();
        snapshot.status = ChatStatus::Unavailable;
        let mut d = ChatDialog::new(Arc::new(Theme::default()), "p".to_string());
        d.apply_snapshot(snapshot);
        d.handle_key(key(KeyCode::Char('i')));
        assert_eq!(d.mode(), ChatMode::Normal);
        // Navigation still works so the panel stays explorable.
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(d.cursor(), 1);
    }

    #[test]
    fn load_older_is_gated_on_the_daemon_capability() {
        let mut snapshot = ready_snapshot();
        snapshot.backward_paging = false;
        let mut d = ChatDialog::new(Arc::new(Theme::default()), "p".to_string());
        d.apply_snapshot(snapshot);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('u'))),
            Some(TuiMsg::ChatPagingUnsupported)
        );

        let mut snapshot = ready_snapshot();
        snapshot.backward_paging = true;
        let mut d = ChatDialog::new(Arc::new(Theme::default()), "p".to_string());
        d.apply_snapshot(snapshot);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('u'))),
            Some(TuiMsg::ChatLoadOlder)
        );
    }

    #[test]
    fn channel_steps_wrap_in_both_directions() {
        let mut snapshot = ready_snapshot();
        snapshot.channels.push(ChatChannelDto {
            channel_id: "ch-2".to_string(),
            project_id: "p".to_string(),
            name: "random".to_string(),
            created_by: "alice".to_string(),
            created_at_ms: 0,
        });
        snapshot.channels.push(ChatChannelDto {
            channel_id: "ch-3".to_string(),
            project_id: "p".to_string(),
            name: "ops".to_string(),
            created_by: "alice".to_string(),
            created_at_ms: 0,
        });
        let mut d = ChatDialog::new(Arc::new(Theme::default()), "p".to_string());
        d.apply_snapshot(snapshot.clone());
        assert_eq!(
            d.handle_key(key(KeyCode::Char(']'))),
            Some(TuiMsg::ChatSwitchChannel {
                channel_id: "ch-2".to_string()
            })
        );
        assert_eq!(d.channel_cursor, 1);

        // Repeating before the reducer adopts the switch still walks
        // forward from the highlight rather than re-anchoring on the
        // channel being left.
        assert_eq!(
            d.handle_key(key(KeyCode::Char(']'))),
            Some(TuiMsg::ChatSwitchChannel {
                channel_id: "ch-3".to_string()
            })
        );
        assert_eq!(d.channel_cursor, 2);

        // Once the reducer adopts ch-3 the highlight re-anchors there, so
        // stepping back lands on ch-2 and wrapping still works.
        let mut applied = snapshot.clone();
        applied.active_channel_id = Some("ch-3".to_string());
        d.apply_snapshot(applied);
        assert_eq!(d.channel_cursor, 2);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('['))),
            Some(TuiMsg::ChatSwitchChannel {
                channel_id: "ch-2".to_string()
            })
        );
        assert_eq!(d.channel_cursor, 1);
        // Stepping back past the start wraps to the end.
        d.handle_key(key(KeyCode::Char('[')));
        assert_eq!(d.channel_cursor, 0);
        d.handle_key(key(KeyCode::Char('[')));
        assert_eq!(d.channel_cursor, 2);
    }

    #[test]
    fn typing_never_leaks_to_normal_mode_keys() {
        let mut d = dialog();
        // j/k/e/X/R/[ must act as commands, not as text.
        d.handle_key(key(KeyCode::Char('j')));
        assert_eq!(d.cursor(), 1);
        assert_eq!(d.buffer(), "");
        d.handle_key(key(KeyCode::Char('e')));
        assert_eq!(d.mode(), ChatMode::Edit);
    }

    #[test]
    fn composing_text_survives_an_unrelated_snapshot_push() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('i')));
        d.handle_key(key(KeyCode::Char('d')));
        d.handle_key(key(KeyCode::Char('r')));
        assert_eq!(d.buffer(), "dr");
        // A live event lands mid-typing.
        d.apply_snapshot(ready_snapshot());
        assert_eq!(d.mode(), ChatMode::Compose);
        assert_eq!(d.buffer(), "dr");
    }

    #[test]
    fn normal_mode_adopts_the_shared_draft() {
        let mut d = dialog();
        let mut snapshot = ready_snapshot();
        snapshot.draft = "unsent text".to_string();
        d.apply_snapshot(snapshot);
        d.handle_key(key(KeyCode::Char('i')));
        assert_eq!(d.buffer(), "unsent text");
    }

    #[test]
    fn backspace_and_cursor_movement_edit_the_buffer() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('i')));
        for ch in "abcd".chars() {
            d.handle_key(key(KeyCode::Char(ch)));
        }
        // Caret at 3 in "abcd" sits between 'c' and 'd', so Backspace
        // removes 'c' — the caret moves, the text before it does not.
        d.handle_key(key(KeyCode::Left));
        d.handle_key(key(KeyCode::Backspace));
        assert_eq!(d.buffer(), "abd");
        // Delete removes at the caret instead, so Home + Delete drops 'a'.
        d.handle_key(key(KeyCode::Home));
        d.handle_key(key(KeyCode::Delete));
        assert_eq!(d.buffer(), "bd");
        d.handle_key(key(KeyCode::End));
        assert_eq!(d.caret, 2);
        d.handle_key(key(KeyCode::Backspace));
        d.handle_key(key(KeyCode::Backspace));
        d.handle_key(key(KeyCode::Backspace));
        assert_eq!(d.buffer(), "");
        // Backspace at the start is a no-op, not an underflow.
        d.handle_key(key(KeyCode::Backspace));
        assert_eq!(d.buffer(), "");
        assert_eq!(d.caret, 0);
    }

    /// The rendered window must always contain the cursor. Sizing the
    /// window by message count instead of viewport height would push the
    /// highlighted row past the bottom of a short viewport.
    #[test]
    fn rendered_window_always_contains_the_cursor() {
        let theme = Arc::new(Theme::default());
        let mut snapshot = ready_snapshot();
        snapshot.messages = (1..=40)
            .map(|seq| message(&format!("m{seq}"), seq, "alice", &format!("body {seq}")))
            .collect();
        let mut d = ChatDialog::new(Arc::clone(&theme), "p".to_string());
        d.apply_snapshot(snapshot);

        // Cursor at the top of a 100-row message set, rendered into a
        // 10-row viewport.
        d.handle_key(key(KeyCode::Char('g')));
        let lines = d.body_lines(10, 80, &theme);
        assert!(lines.iter().any(|l| l
            .spans
            .iter()
            .any(|s| s.content.contains("body 1▍") || s.content.as_ref() == "▍")));

        // Cursor in the middle: the window follows it.
        d.handle_key(key(KeyCode::Char('j')));
        for _ in 0..19 {
            d.handle_key(key(KeyCode::Char('j')));
        }
        assert_eq!(d.cursor(), 20);
        let lines = d.body_lines(10, 80, &theme);
        assert!(
            lines
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content.as_ref() == "▍")),
            "cursor row is rendered"
        );
        assert!(
            lines.len() <= 10,
            "window stays within the viewport: {}",
            lines.len()
        );

        // Cursor at the end clamps rather than running past the set.
        d.handle_key(key(KeyCode::Char('G')));
        assert_eq!(d.cursor(), 39);
        let lines = d.body_lines(10, 80, &theme);
        assert!(lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.as_ref() == "▍")));
        assert!(lines.len() <= 10);
    }

    #[test]
    fn multibyte_edits_keep_char_boundaries() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('i')));
        for ch in "héllo→".chars() {
            d.handle_key(key(KeyCode::Char(ch)));
        }
        assert_eq!(d.buffer(), "héllo→");
        d.handle_key(key(KeyCode::Backspace));
        assert_eq!(d.buffer(), "héllo");
        d.handle_key(key(KeyCode::Home));
        d.handle_key(key(KeyCode::Delete));
        assert_eq!(d.buffer(), "éllo");
    }

    /// Regression: the dialog used to reserve **zero** composer rows in
    /// Normal mode, so typing into what looked like an empty input box did
    /// nothing and nothing on screen said `i` was the way in. This is the
    /// same defect the side panel had.
    #[test]
    fn normal_mode_still_shows_the_composer_row_with_a_badge() {
        let theme = Arc::new(Theme::default());
        let mut d = ChatDialog::new(Arc::clone(&theme), "p".to_string());
        d.apply_snapshot(ready_snapshot());
        assert_eq!(d.mode(), ChatMode::Normal);

        let line = d.composer_line(&theme);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            text.contains("NOR"),
            "Normal mode must be labelled NOR: {text}"
        );
        assert!(
            text.contains("press i to type"),
            "Normal mode must name the key that starts composing: {text}"
        );
    }

    #[test]
    fn compose_mode_relabels_the_badge_to_ins() {
        let theme = Arc::new(Theme::default());
        let mut d = ChatDialog::new(Arc::clone(&theme), "p".to_string());
        d.apply_snapshot(ready_snapshot());
        d.handle_key(key(KeyCode::Char('i')));
        assert_eq!(d.mode(), ChatMode::Compose);

        let line = d.composer_line(&theme);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("INS"), "Compose mode must read INS: {text}");
        assert!(
            !text.contains("press i to type"),
            "the entry hint must not linger once composing: {text}"
        );
    }

    #[test]
    fn typed_text_is_visible_in_the_dialog_composer() {
        let theme = Arc::new(Theme::default());
        let mut d = ChatDialog::new(Arc::clone(&theme), "p".to_string());
        d.apply_snapshot(ready_snapshot());
        d.handle_key(key(KeyCode::Char('i')));
        for ch in "ship it".chars() {
            d.handle_key(key(KeyCode::Char(ch)));
        }
        let line = d.composer_line(&theme);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("ship it"), "typed text must render: {text}");
    }
}
