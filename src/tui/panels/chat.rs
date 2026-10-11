//! Project chat as a swappable side panel.
//!
//! The Workspace view already projected project chat into the side column,
//! but it could only *scroll*: composing, sending, editing, redacting, and
//! paging all had to be typed as `/chat-*` commands into the main input
//! while watching the panel. That split one workflow across two surfaces.
//! This panel puts the whole interaction in the column.
//!
//! ## Relationship to `/chat`
//!
//! This is a peer of [`ChatDialog`](crate::tui::components::dialogs::chat::ChatDialog),
//! not a replacement — the slash commands stay available so the two can be
//! compared. Both share:
//!
//! * the same bounded projection in `ChatState` — this panel reads it every
//!   frame and never caches a second copy;
//! * the same two-mode Normal/Compose/Edit keymap and the same key meanings
//!   (`j`/`k` move, `i` compose, `r` reply, `e` edit, `X` redact, `m` mark
//!   read, `u` load older, `R` refresh, `[`/`]` switch channel);
//! * the same intent boundary. Every mutation emits a [`TuiMsg`] that
//!   `App::process_msg` routes into the existing `commands/chat.rs`
//!   `start_chat_*` seams, so daemon round-trips, authorization, idempotency
//!   keys, and the request-id / reconnect-epoch stale guards all stay where
//!   they already are.
//!
//! The difference is presentation: this column is 30 columns wide, so there
//! is a single composer row instead of a bordered three-row editor, message
//! bodies hard-wrap, and the keybar is one line of hints.
//!
//! ## Authority
//!
//! The panel never picks a project. `PanelCtx::chat_project` is resolved by
//! the command layer from the active tab or the Workspace selection, and a
//! panel with no bound project refuses to take focus at all — the host
//! routes every key to whichever panel is active, and a chat panel that
//! cannot act would otherwise swallow the composer's keystrokes.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::protocol::core::ChatMessageDto;
use crate::tui::app::state::chat::{ChatStatus, ProjectChat};
use crate::tui::app::TuiMsg;
use crate::tui::input::InputAction;
use crate::tui::theme::Theme;
use crate::tui::wrap::{ellipsize_to_width, wrap_to_strings};

use super::{PanelCtx, PanelKey, SidePanel, SidePanelId};

/// Rows one page-key moves the message cursor.
///
/// Key handling runs before render and has no frame, so this is a fixed step
/// rather than the true viewport height. Render re-anchors on the cursor, so
/// a mis-sized page still lands on a visible row.
const PANEL_PAGE_STEP: usize = 10;

/// Maximum composer characters. Matches the reducer's draft bound so the
/// panel never holds text the daemon would reject on send.
const MAX_COMPOSER_CHARS: usize = 4_000;

/// Interaction mode of the chat panel.
///
/// Mirrors `ChatDialog`'s `ChatMode` so both surfaces speak the same
/// vocabulary, but owned locally: the panel must be able to sit in Compose
/// mode while the `/chat` window sits in Normal, and they are separate
/// views of the same projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChatMode {
    /// Browsing: single keys drive actions, no text is captured.
    #[default]
    Normal,
    /// Composing a new message.
    Compose,
    /// Editing an existing message's body.
    Edit,
}

impl ChatMode {
    /// Whether printable keys are captured as text.
    pub fn is_input(self) -> bool {
        matches!(self, ChatMode::Compose | ChatMode::Edit)
    }
}

/// Interactive project-chat side panel.
///
/// Holds only per-view state — mode, cursor, channel highlight, and the
/// composer buffer. Message data lives in `ChatState`; the buffer is
/// mirrored into the per-project draft on every edit so a failed send keeps
/// the author's text and `/chat-send` from the command line sees the same
/// thing.
#[derive(Debug, Default)]
pub struct ChatPanel {
    mode: ChatMode,
    /// Index into the active channel's message window.
    cursor: usize,
    /// Index into the channel list.
    channel_cursor: usize,
    buffer: String,
    caret: usize,
    edit_target: Option<String>,
    reply_target: Option<String>,
    /// First `X` arms a destructive redact; the second confirms.
    redact_armed: Option<String>,
    /// Mirrors `App::chat_panel_project` — true when a project is bound.
    /// Kept in step by the same command functions that set the App field,
    /// because `SidePanelHost::focus` has no access to app state.
    bound: bool,
}

impl ChatPanel {
    pub fn mode(&self) -> ChatMode {
        self.mode
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn channel_cursor(&self) -> usize {
        self.channel_cursor
    }

    pub fn is_unbound(&self) -> bool {
        !self.bound
    }

    /// Record whether a project is bound. Called by the command layer in
    /// lockstep with `App::chat_panel_project`.
    pub fn set_bound(&mut self, bound: bool) {
        self.bound = bound;
        if !bound {
            self.reset();
        }
    }

    /// Drop every piece of per-view state.
    ///
    /// Called on revocation and project change so a composer buffer cannot
    /// outlive the projection it was composed against.
    pub fn reset(&mut self) {
        self.mode = ChatMode::Normal;
        self.cursor = 0;
        self.channel_cursor = 0;
        self.buffer.clear();
        self.caret = 0;
        self.edit_target = None;
        self.reply_target = None;
        self.redact_armed = None;
    }

    // ── Reading state ─────────────────────────────────────────────────

    fn entry<'a>(&self, ctx: &'a PanelCtx<'_>) -> Option<&'a ProjectChat> {
        ctx.chat_project.and_then(|p| ctx.chat.get(p))
    }

    fn message_count(&self, ctx: &PanelCtx<'_>) -> usize {
        self.entry(ctx).map_or(0, |e| e.messages.len())
    }

    fn channel_count(&self, ctx: &PanelCtx<'_>) -> usize {
        self.entry(ctx).map_or(0, |e| e.channels.len())
    }

    /// Sending into a project the daemon never authorized would produce a
    /// guaranteed `project_not_found` round-trip, so the composer stays
    /// closed until state arrives.
    fn interactive(&self, ctx: &PanelCtx<'_>) -> bool {
        matches!(
            self.entry(ctx).map(|e| e.status),
            Some(ChatStatus::Ready) | Some(ChatStatus::Error)
        )
    }

    fn selected<'a>(&self, ctx: &'a PanelCtx<'_>) -> Option<&'a ChatMessageDto> {
        self.entry(ctx).and_then(|e| e.messages.get(self.cursor))
    }

    fn channel_name(&self, ctx: &PanelCtx<'_>) -> String {
        let Some(entry) = self.entry(ctx) else {
            return "—".to_string();
        };
        entry
            .channels
            .iter()
            .find(|c| Some(c.channel_id.as_str()) == entry.active_channel_id.as_deref())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "general".to_string())
    }

    fn clamp_cursor(&mut self, ctx: &PanelCtx<'_>) {
        self.cursor = self.cursor.min(self.message_count(ctx).saturating_sub(1));
    }

    // ── Buffer editing ────────────────────────────────────────────────

    fn insert(&mut self, ch: char) {
        if char_len(&self.buffer) >= MAX_COMPOSER_CHARS {
            return;
        }
        let idx = char_byte_index(&self.buffer, self.caret);
        self.buffer.insert(idx, ch);
        self.caret += 1;
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

    /// Copy the panel buffer into the per-project draft so `/chat-send` and
    /// `/chat-edit` from the command line stay in step with what is on
    /// screen, and so a failed send does not lose the text.
    fn mirror_draft(&self, ctx: &mut PanelCtx<'_>) {
        let Some(project) = ctx.chat_project else {
            return;
        };
        if ctx.chat.draft_for(project) != self.buffer {
            ctx.chat.set_draft(project, self.buffer.clone());
        }
    }

    /// Adopt the project draft into an empty buffer.
    ///
    /// Only while browsing: overwriting mid-typing would destroy what the
    /// user is composing every time an unrelated live event landed.
    fn sync_draft(&mut self, ctx: &PanelCtx<'_>) {
        if self.mode == ChatMode::Normal && self.buffer.is_empty() {
            if let Some(project) = ctx.chat_project {
                self.buffer = ctx.chat.draft_for(project).to_string();
                self.caret = char_len(&self.buffer);
            }
        }
    }

    fn open_compose(&mut self, ctx: &PanelCtx<'_>) {
        if !self.interactive(ctx) {
            return;
        }
        self.mode = ChatMode::Compose;
        self.edit_target = None;
        self.reply_target = None;
        // The buffer is NOT cleared: it holds the project draft, which is how
        // a message survives a failed send. Opening the composer must not
        // make that draft unreachable from the one place it can be fixed.
        self.caret = char_len(&self.buffer);
    }

    fn open_reply(&mut self, ctx: &PanelCtx<'_>) {
        if !self.interactive(ctx) {
            return;
        }
        let Some(message) = self.selected(ctx) else {
            return;
        };
        let (author, message_id) = (message.author_principal.clone(), message.message_id.clone());
        self.mode = ChatMode::Compose;
        self.edit_target = None;
        self.reply_target = Some(message_id);
        self.buffer = format!("@{author} ");
        self.caret = char_len(&self.buffer);
    }

    fn open_edit(&mut self, ctx: &PanelCtx<'_>) {
        let Some(message) = self.selected(ctx) else {
            return;
        };
        if !self.interactive(ctx) || message.redacted {
            return;
        }
        // Authorship is the daemon's call, not the client's. The same
        // principal string is not proof of ownership across transports, so
        // the panel neither hides the affordance nor pre-checks it — the
        // daemon answers with an authored reason either way.
        let (message_id, body) = (message.message_id.clone(), message.body.clone());
        self.mode = ChatMode::Edit;
        self.reply_target = None;
        self.edit_target = Some(message_id);
        self.buffer = body;
        self.caret = char_len(&self.buffer);
    }

    /// Leave input mode, returning to Normal with the buffer intact.
    ///
    /// `Esc` must never destroy typed text, so cancel does not clear the
    /// buffer. It is cleared on submit and by [`ChatPanel::reset`].
    fn cancel_input(&mut self) {
        self.mode = ChatMode::Normal;
        self.edit_target = None;
        self.reply_target = None;
        self.redact_armed = None;
    }

    // ── Key routing ───────────────────────────────────────────────────

    fn handle_input_key(&mut self, key: KeyEvent, ctx: &mut PanelCtx<'_>) -> PanelKey {
        match key.code {
            // `Esc` leaves input mode; the host drops focus afterwards only
            // if the panel has already returned to Normal.
            KeyCode::Esc => {
                self.cancel_input();
                PanelKey::Consumed
            }
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE => self.submit(ctx),
            KeyCode::Backspace => {
                self.backspace();
                self.mirror_draft(ctx);
                PanelKey::Consumed
            }
            KeyCode::Delete => {
                self.delete();
                self.mirror_draft(ctx);
                PanelKey::Consumed
            }
            KeyCode::Left => {
                self.caret = self.caret.saturating_sub(1);
                PanelKey::Consumed
            }
            KeyCode::Right => {
                self.caret = (self.caret + 1).min(char_len(&self.buffer));
                PanelKey::Consumed
            }
            KeyCode::Home => {
                self.caret = 0;
                PanelKey::Consumed
            }
            KeyCode::End => {
                self.caret = char_len(&self.buffer);
                PanelKey::Consumed
            }
            KeyCode::Char(ch)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.insert(ch);
                self.mirror_draft(ctx);
                PanelKey::Consumed
            }
            // Unclaimed keys are still consumed: while composing, a key must
            // never fall through and submit a turn.
            _ => PanelKey::Consumed,
        }
    }

    /// Hand the buffer to the intent boundary and return to Normal.
    fn submit(&mut self, ctx: &mut PanelCtx<'_>) -> PanelKey {
        let body = self.buffer.trim().to_string();
        // An empty composer is a no-op, not a send. `start_chat_send`
        // rejects an empty body with a warning toast, so emitting the
        // intent here would make a stray Enter on an untouched composer
        // shout at the user about a message they never wrote.
        if body.is_empty() && self.mode == ChatMode::Compose {
            self.mode = ChatMode::Normal;
            self.edit_target = None;
            self.reply_target = None;
            return PanelKey::Consumed;
        }
        let intent = match self.mode {
            ChatMode::Edit => self
                .edit_target
                .take()
                .map(|message_id| TuiMsg::ChatEdit { message_id, body }),
            _ => Some(TuiMsg::ChatSend {
                body,
                reply_to: self.reply_target.take(),
            }),
        };
        self.mode = ChatMode::Normal;
        self.reply_target = None;
        self.edit_target = None;
        self.buffer.clear();
        self.caret = 0;
        if let Some(project) = ctx.chat_project {
            ctx.chat.set_draft(project, String::new());
        }
        // An empty submission produces no intent: the panel stays in Normal
        // rather than emitting a send the daemon would reject.
        match intent {
            Some(msg) => PanelKey::Intent(msg),
            None => PanelKey::Consumed,
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent, ctx: &mut PanelCtx<'_>) -> PanelKey {
        let last = self.message_count(ctx).saturating_sub(1);
        // Ctrl+U / Ctrl+D page alongside the dedicated page keys so the
        // muscle memory from the session panel carries over.
        let page = if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('u') => Some(-1),
                KeyCode::Char('d') => Some(1),
                _ => None,
            }
        } else {
            None
        };
        if let Some(delta) = page {
            if delta > 0 {
                self.cursor = (self.cursor + PANEL_PAGE_STEP).min(last);
            } else {
                self.cursor = self.cursor.saturating_sub(PANEL_PAGE_STEP);
            }
            self.clamp_cursor(ctx);
            return PanelKey::Consumed;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor = (self.cursor + 1).min(last);
                self.clamp_cursor(ctx);
                PanelKey::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                self.clamp_cursor(ctx);
                PanelKey::Consumed
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.cursor = 0;
                PanelKey::Consumed
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.cursor = last;
                PanelKey::Consumed
            }
            KeyCode::PageDown => {
                self.cursor = (self.cursor + PANEL_PAGE_STEP).min(last);
                self.clamp_cursor(ctx);
                PanelKey::Consumed
            }
            KeyCode::PageUp => {
                self.cursor = self.cursor.saturating_sub(PANEL_PAGE_STEP);
                self.clamp_cursor(ctx);
                PanelKey::Consumed
            }
            KeyCode::Char('i') | KeyCode::Char('a') | KeyCode::Enter => {
                self.open_compose(ctx);
                PanelKey::Consumed
            }
            KeyCode::Char('r') => {
                self.open_reply(ctx);
                PanelKey::Consumed
            }
            KeyCode::Char('e') => {
                self.open_edit(ctx);
                PanelKey::Consumed
            }
            KeyCode::Char('X') => self.arm_or_redact(ctx),
            KeyCode::Char('m') => PanelKey::Intent(TuiMsg::ChatMarkRead),
            KeyCode::Char('u') => PanelKey::Intent(if ctx.chat.backward_paging {
                TuiMsg::ChatLoadOlder
            } else {
                // The daemon cannot serve backward paging. Say so rather
                // than letting the key look broken.
                TuiMsg::ChatPagingUnsupported
            }),
            KeyCode::Char('R') => PanelKey::Intent(TuiMsg::ChatRefresh),
            KeyCode::Char('[') => self.step_channel(ctx, -1),
            KeyCode::Char(']') => self.step_channel(ctx, 1),
            // Normal mode deliberately swallows everything else. A leaked
            // `j` would submit a turn; a leaked `/` would open command mode
            // in the middle of browsing messages.
            _ => PanelKey::Consumed,
        }
    }

    /// First `X` arms, second on the same message redacts.
    ///
    /// Redaction is irreversible daemon-side, so it cannot be a single key.
    fn arm_or_redact(&mut self, ctx: &PanelCtx<'_>) -> PanelKey {
        let Some(message) = self.selected(ctx) else {
            self.redact_armed = None;
            return PanelKey::Consumed;
        };
        if message.redacted {
            self.redact_armed = None;
            return PanelKey::Consumed;
        }
        let message_id = message.message_id.clone();
        if self.redact_armed.as_deref() == Some(message_id.as_str()) {
            self.redact_armed = None;
            return PanelKey::Intent(TuiMsg::ChatRedact { message_id });
        }
        self.redact_armed = Some(message_id);
        PanelKey::Consumed
    }

    /// Move the channel highlight one step and emit the switch intent.
    ///
    /// Stepping from the highlight rather than the active channel is what
    /// makes repeated presses before a page lands walk the right way: the
    /// reducer only adopts a new active channel once its response applies,
    /// so rebasing on `active_channel_id` would resolve a second press from
    /// the channel the user is already leaving.
    fn step_channel(&mut self, ctx: &PanelCtx<'_>, delta: i8) -> PanelKey {
        let count = self.channel_count(ctx);
        if count < 2 {
            // A single-channel project has nothing to switch to; staying
            // quiet beats re-fetching the channel the user is already on.
            return PanelKey::Consumed;
        }
        let next =
            (self.channel_cursor as i64 + i64::from(delta)).rem_euclid(count as i64) as usize;
        self.channel_cursor = next;
        match self
            .entry(ctx)
            .and_then(|e| e.channels.get(next))
            .map(|c| c.channel_id.clone())
        {
            Some(channel_id) => PanelKey::Intent(TuiMsg::ChatSwitchChannel { channel_id }),
            None => PanelKey::Consumed,
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────

impl ChatPanel {
    fn header(&self, ctx: &PanelCtx<'_>, theme: &Theme) -> Line<'static> {
        let total = self.message_count(ctx);
        let unread = self.entry(ctx).map_or(0, |e| e.unread_count());
        let position = if total == 0 { 0 } else { self.cursor + 1 };
        Line::from(vec![
            Span::styled(
                format!(" #{} ", self.channel_name(ctx)),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{position}/{total}{}",
                    if unread > 0 {
                        format!(" ·{unread}")
                    } else {
                        String::new()
                    }
                ),
                Style::default().fg(theme.muted),
            ),
        ])
    }

    /// Flatten the message window into wrapped display lines, each tagged
    /// with the message it belongs to so the cursor highlights a whole
    /// message rather than one wrapped row.
    fn body_lines(
        &self,
        ctx: &PanelCtx<'_>,
        width: u16,
        theme: &Theme,
    ) -> Vec<(usize, Line<'static>)> {
        let Some(entry) = self.entry(ctx) else {
            return Vec::new();
        };
        let width = width.max(12);
        let mut out: Vec<(usize, Line<'static>)> = Vec::new();
        for (index, message) in entry.messages.iter().enumerate() {
            let selected = index == self.cursor;
            let marker = if selected { "▍" } else { " " };
            let author_style = if selected {
                Style::default()
                    .fg(theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.primary)
            };
            let body_style = if selected {
                Style::default().fg(theme.selection)
            } else {
                Style::default().fg(theme.foreground)
            };
            // Two columns go to the cursor marker and one space separates
            // the author from the body, so continuation lines are indented
            // under the first body column instead of the marker column.
            let prefix_cols = 2 + display_width(&message.author_principal);
            let body_width = width
                .saturating_sub(u16::try_from(prefix_cols).unwrap_or(u16::MAX))
                .max(8);
            let chunks = if message.redacted {
                vec!["[redacted]".to_string()]
            } else {
                wrap_to_strings(&message.body, body_width)
            };
            for (line_index, chunk) in chunks.iter().enumerate() {
                let spans = if line_index == 0 {
                    vec![
                        Span::styled(marker, body_style),
                        Span::styled(format!("{} ", message.author_principal), author_style),
                        Span::styled(chunk.clone(), body_style),
                    ]
                } else {
                    vec![Span::styled(format!("  {chunk}"), body_style)]
                };
                out.push((index, Line::from(spans)));
            }
        }
        out
    }

    /// Status row: armed redact first (it is the thing waiting on the user),
    /// then send errors, then typing leases. Errors win over the typing
    /// indicator so a failed send is never hidden behind it.
    fn status_line(&self, ctx: &PanelCtx<'_>, theme: &Theme) -> Line<'static> {
        if self.redact_armed.is_some() {
            return Line::from(Span::styled(
                " X = redact (final)",
                Style::default().fg(theme.warning),
            ));
        }
        let Some(entry) = self.entry(ctx) else {
            return Line::default();
        };
        if let Some(error) = entry.last_send_error.as_deref() {
            // The mark carries the meaning: at 28 inner columns a
            // "send failed:" prefix would push the reason — the part the
            // user needs — off the end of the row.
            return Line::from(Span::styled(
                format!(" ✗ {}", ellipsize_to_width(error, 26)),
                Style::default().fg(theme.error),
            ));
        }
        if entry.messages.is_empty() {
            if let Some(error) = entry.last_error.as_deref() {
                return Line::from(Span::styled(
                    format!(" {}", ellipsize_to_width(error, 28)),
                    Style::default().fg(theme.error),
                ));
            }
        }
        let composing = entry.active_composing(ctx.now_ms);
        if !entry.messages.is_empty() && !composing.is_empty() {
            let names: Vec<String> = composing.iter().map(|c| c.principal_id.clone()).collect();
            return Line::from(Span::styled(
                format!(" {} …", ellipsize_to_width(&names.join(", "), 22)),
                Style::default().fg(theme.muted),
            ));
        }
        Line::default()
    }

    /// Single-row composer with a visible caret.
    fn composer_line(&self, theme: &Theme, width: u16) -> Line<'static> {
        let width = usize::from(width).max(4);
        let body: String = self.buffer.chars().take(width.saturating_sub(3)).collect();
        let caret_offset = self.caret.min(char_len(&body));
        let before: String = body.chars().take(caret_offset).collect();
        let after: String = body.chars().skip(caret_offset).collect();
        Line::from(vec![
            Span::styled("▸ ", Style::default().fg(theme.primary)),
            Span::styled(before, Style::default().fg(theme.foreground)),
            Span::styled(
                "▌",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            Span::styled(after, Style::default().fg(theme.foreground)),
        ])
    }

    /// One-line keybar, shown only while the panel holds focus so an
    /// unfocused panel gives the row to content.
    fn keybar(&self, ctx: &PanelCtx<'_>, theme: &Theme) -> Line<'static> {
        if !ctx.focused {
            return Line::default();
        }
        let hint = if self.mode.is_input() {
            "↵ send · Esc cancel"
        } else {
            "jk move · i type · X del · ^⇧W panel"
        };
        Line::from(Span::styled(
            ellipsize_to_width(hint, usize::from(ctx.area.width).saturating_sub(2)),
            Style::default().fg(theme.muted),
        ))
    }
}

impl SidePanel for ChatPanel {
    fn id(&self) -> SidePanelId {
        SidePanelId::Chat
    }

    /// In input mode both of the host's exits belong to the panel: `Esc`
    /// and `q` must leave Compose without the buffer before the host
    /// returns keys to the composer. `q` in particular is a letter while
    /// composing, and letting the host treat it as a command would make it
    /// impossible to type a `q`.
    fn owns_exit_key(&self, _key: KeyEvent) -> bool {
        self.mode.is_input()
    }

    fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
        self.sync_draft(ctx);
        let theme = ctx.theme.clone();
        if area.width < 12 || area.height < 4 {
            return;
        }
        let label = SidePanelId::Chat.label();
        let title = if ctx.focused {
            format!(" {label} ● ")
        } else {
            format!(" {label} ")
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if ctx.focused {
                theme.primary
            } else {
                theme.border
            }))
            .title(title)
            .title_style(Style::default().fg(if ctx.focused {
                theme.primary
            } else {
                theme.muted
            }))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let composer_rows = u16::from(self.mode.is_input());
        let rows = Layout::vertical([
            Constraint::Length(1),             // header
            Constraint::Min(1),                // body
            Constraint::Length(composer_rows), // composer
            Constraint::Length(1),             // status
            Constraint::Length(1),             // keybar
        ])
        .split(inner);

        Paragraph::new(self.header(ctx, &theme)).render(rows[0], buf);

        // Nothing bound yet: say so rather than showing an empty box the
        // user cannot act in.
        if self.entry(ctx).is_none() {
            let message = if ctx.chat_project.is_none() {
                "No project bound"
            } else {
                "Chat unavailable"
            };
            Paragraph::new(Line::from(Span::styled(
                format!(" {message}"),
                Style::default().fg(theme.muted),
            )))
            .render(rows[1], buf);
            Paragraph::new(self.keybar(ctx, &theme)).render(rows[4], buf);
            return;
        }

        let body_height = usize::from(rows[1].height);
        let lines = self.body_lines(ctx, rows[1].width, &theme);
        if lines.is_empty() {
            Paragraph::new(Line::from(Span::styled(
                " no messages",
                Style::default().fg(theme.muted),
            )))
            .render(rows[1], buf);
        } else {
            // Anchor on the selected message's *last* wrapped line: a long
            // message can span more rows than the body has, and anchoring on
            // its first line would scroll it off-screen entirely.
            let selected_end = lines
                .iter()
                .rposition(|(index, _)| *index == self.cursor)
                .unwrap_or(lines.len() - 1);
            let start = selected_end.saturating_sub(body_height.saturating_sub(1));
            let window: Vec<Line> = lines[start..=selected_end]
                .iter()
                .map(|(_, line)| line.clone())
                .collect();
            Paragraph::new(window).render(rows[1], buf);
        }

        if composer_rows > 0 {
            Paragraph::new(self.composer_line(&theme, rows[2].width)).render(rows[2], buf);
        }
        Paragraph::new(self.status_line(ctx, &theme)).render(rows[3], buf);
        Paragraph::new(self.keybar(ctx, &theme)).render(rows[4], buf);
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        _action: Option<InputAction>,
        ctx: &mut PanelCtx<'_>,
    ) -> PanelKey {
        match self.mode {
            ChatMode::Normal => self.handle_normal_key(key, ctx),
            ChatMode::Compose | ChatMode::Edit => self.handle_input_key(key, ctx),
        }
    }

    fn on_activate(&mut self, ctx: &mut PanelCtx<'_>) {
        self.sync_draft(ctx);
        // Re-anchor the channel highlight on whatever the reducer reports as
        // active so a completed switch lands where the user aimed and the
        // next press continues from there.
        if let Some(entry) = self.entry(ctx) {
            self.channel_cursor = entry
                .channels
                .iter()
                .position(|c| Some(c.channel_id.as_str()) == entry.active_channel_id.as_deref())
                .unwrap_or(0);
            self.clamp_cursor(ctx);
        }
    }

    fn on_deactivate(&mut self, _ctx: &mut PanelCtx<'_>) {
        // Leaving must not strand a destructive action armed, nor leave the
        // user inside a half-typed edit they can no longer see.
        self.redact_armed = None;
        if self.mode.is_input() {
            self.cancel_input();
        }
    }
}

// ── Small helpers ─────────────────────────────────────────────────────

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn char_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

/// Terminal columns a string occupies. Used to reserve the author prefix so
/// wrapped bodies align; a char count would under-reserve for CJK/emoji.
fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    s.width()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::core::{ChatChannelDto, ChatMessageDto};
    use crate::tui::app::state::chat::ChatState;
    use crate::tui::components::sidebar::SidebarWidget;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use std::sync::Arc;

    const PROJECT: &str = "p1";
    const SIDEBAR: Rect = Rect::new(0, 0, 30, 20);

    /// The state a panel reads. Held separately from the panel under test so
    /// the two borrows stay disjoint — `PanelCtx` borrows the projections
    /// mutably, so the panel cannot live inside the same struct.
    struct World {
        chat: ChatState,
        sidebar: SidebarWidget,
        theme: Arc<Theme>,
    }

    impl World {
        fn new() -> Self {
            Self {
                chat: ChatState::new(),
                sidebar: SidebarWidget::default(),
                theme: Arc::new(Theme::dark()),
            }
        }

        /// One channel with a `Ready` window of messages.
        fn seeded(bodies: &[&str]) -> Self {
            let mut w = Self::new();
            w.seed_channels(&["ch1"], "general");
            w.seed_messages("ch1", bodies);
            w
        }

        fn seed_channels(&mut self, ids: &[&str], base_name: &str) {
            let epoch = self.chat.reconnect_epoch;
            for (i, id) in ids.iter().enumerate() {
                let name = if ids.len() == 1 {
                    base_name.to_string()
                } else {
                    format!("{base_name}{i}")
                };
                let request = self.chat.begin_ensure(PROJECT).expect("begin_ensure");
                assert!(self.chat.apply_channel_ensured(
                    request,
                    PROJECT,
                    &ChatChannelDto {
                        channel_id: (*id).to_string(),
                        project_id: PROJECT.to_string(),
                        name,
                        created_by: "alice".to_string(),
                        created_at_ms: 1,
                    },
                    epoch,
                ));
            }
        }

        fn seed_messages(&mut self, channel_id: &str, bodies: &[&str]) {
            let epoch = self.chat.reconnect_epoch;
            let request = self
                .chat
                .begin_history(PROJECT, channel_id)
                .expect("begin_history");
            let messages: Vec<ChatMessageDto> = bodies
                .iter()
                .enumerate()
                .map(|(i, body)| ChatMessageDto {
                    message_id: format!("m{}", i + 1),
                    channel_id: channel_id.to_string(),
                    project_id: PROJECT.to_string(),
                    seq: i as u64 + 1,
                    author_principal: "alice".to_string(),
                    author_agent: None,
                    body: (*body).to_string(),
                    reply_to: None,
                    thread_root: None,
                    mentions: Vec::new(),
                    references: Vec::new(),
                    revision: 1,
                    edited_at_ms: None,
                    redacted: false,
                    created_at_ms: 100 + i as i64,
                })
                .collect();
            assert!(self.chat.apply_history(
                request,
                PROJECT,
                channel_id,
                messages,
                bodies.len() as u64,
                false,
                0,
                epoch,
            ));
        }

        fn ctx(&mut self, focused: bool) -> PanelCtx<'_> {
            PanelCtx {
                theme: Arc::clone(&self.theme),
                now_ms: 100_000,
                session: &mut self.sidebar,
                chat: &mut self.chat,
                chat_project: Some(PROJECT),
                area: SIDEBAR,
                focused,
            }
        }
    }

    fn event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn press(panel: &mut ChatPanel, world: &mut World, ch: char) -> PanelKey {
        let mut ctx = world.ctx(true);
        panel.handle_key(event(KeyCode::Char(ch), KeyModifiers::NONE), None, &mut ctx)
    }

    fn tap(panel: &mut ChatPanel, world: &mut World, code: KeyCode) -> PanelKey {
        let mut ctx = world.ctx(true);
        panel.handle_key(event(code, KeyModifiers::NONE), None, &mut ctx)
    }

    fn paint(panel: &mut ChatPanel, world: &mut World, area: Rect, focused: bool) -> String {
        let mut buf = Buffer::empty(area);
        {
            let mut ctx = world.ctx(focused);
            panel.render(&mut ctx, &mut buf, area);
        }
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn paint_default(panel: &mut ChatPanel, world: &mut World, focused: bool) -> String {
        paint(panel, world, SIDEBAR, focused)
    }

    fn bound_panel() -> ChatPanel {
        let mut panel = ChatPanel::default();
        panel.set_bound(true);
        panel
    }

    #[test]
    fn unbound_panel_refuses_to_act() {
        let mut world = World::new();
        let mut panel = ChatPanel::default();
        assert!(panel.is_unbound());
        // No bound project: `i` cannot open a composer that has nowhere to
        // send, and every other key is still swallowed rather than leaking.
        assert_eq!(press(&mut panel, &mut world, 'i'), PanelKey::Consumed);
        assert_eq!(panel.mode(), ChatMode::Normal);
    }

    #[test]
    fn binding_drops_stale_view_state() {
        let mut world = World::seeded(&["one"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'i');
        press(&mut panel, &mut world, 'h');
        assert_eq!(panel.buffer(), "h");
        // Rebinding to another project must not carry the buffer across.
        panel.set_bound(false);
        assert_eq!(panel.buffer(), "");
        assert_eq!(panel.mode(), ChatMode::Normal);
        assert!(panel.is_unbound());
    }

    #[test]
    fn j_and_k_move_the_cursor_and_clamp_at_the_ends() {
        let mut world = World::seeded(&["a", "b", "c"]);
        let mut panel = bound_panel();
        assert_eq!(panel.cursor(), 0);
        press(&mut panel, &mut world, 'j');
        assert_eq!(panel.cursor(), 1);
        press(&mut panel, &mut world, 'k');
        press(&mut panel, &mut world, 'k');
        assert_eq!(panel.cursor(), 0, "must not underflow past the first");
        press(&mut panel, &mut world, 'G');
        assert_eq!(panel.cursor(), 2);
        press(&mut panel, &mut world, 'j');
        assert_eq!(panel.cursor(), 2, "must not run past the last");
        press(&mut panel, &mut world, 'g');
        assert_eq!(panel.cursor(), 0);
    }

    #[test]
    fn arrows_mirror_the_vim_keys() {
        let mut world = World::seeded(&["a", "b", "c"]);
        let mut panel = bound_panel();
        tap(&mut panel, &mut world, KeyCode::Down);
        assert_eq!(panel.cursor(), 1);
        tap(&mut panel, &mut world, KeyCode::Up);
        assert_eq!(panel.cursor(), 0);
        tap(&mut panel, &mut world, KeyCode::End);
        assert_eq!(panel.cursor(), 2);
        tap(&mut panel, &mut world, KeyCode::Home);
        assert_eq!(panel.cursor(), 0);
    }

    #[test]
    fn g_and_capital_g_jump_to_the_ends() {
        let mut world = World::seeded(&["a", "b", "c", "d", "e"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'G');
        assert_eq!(panel.cursor(), 4);
        tap(&mut panel, &mut world, KeyCode::Home);
        assert_eq!(panel.cursor(), 0);
        tap(&mut panel, &mut world, KeyCode::End);
        assert_eq!(panel.cursor(), 4);
    }

    #[test]
    fn page_keys_step_a_bounded_amount() {
        let bodies: Vec<String> = (0..40).map(|i| format!("m{i}")).collect();
        let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
        let mut world = World::seeded(&refs);
        let mut panel = bound_panel();
        tap(&mut panel, &mut world, KeyCode::PageDown);
        assert_eq!(panel.cursor(), PANEL_PAGE_STEP);
        tap(&mut panel, &mut world, KeyCode::PageUp);
        assert_eq!(panel.cursor(), 0);
    }

    #[test]
    fn ctrl_u_and_ctrl_d_page_like_the_page_keys() {
        let bodies: Vec<String> = (0..40).map(|i| format!("m{i}")).collect();
        let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
        let mut world = World::seeded(&refs);
        let mut panel = bound_panel();
        for (code, expect) in [
            (KeyCode::Char('d'), PANEL_PAGE_STEP),
            (KeyCode::Char('u'), 0),
        ] {
            let mut ctx = world.ctx(true);
            let outcome = panel.handle_key(event(code, KeyModifiers::CONTROL), None, &mut ctx);
            assert_eq!(outcome, PanelKey::Consumed);
            assert_eq!(panel.cursor(), expect);
        }
    }

    #[test]
    fn i_opens_the_composer_and_typing_is_captured() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'i');
        assert_eq!(panel.mode(), ChatMode::Compose);
        for ch in "hi".chars() {
            assert_eq!(press(&mut panel, &mut world, ch), PanelKey::Consumed);
        }
        assert_eq!(panel.buffer(), "hi");
    }

    #[test]
    fn enter_submits_a_send_intent_and_clears_the_buffer() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'i');
        press(&mut panel, &mut world, 'h');
        press(&mut panel, &mut world, 'i');
        assert_eq!(
            tap(&mut panel, &mut world, KeyCode::Enter),
            PanelKey::Intent(TuiMsg::ChatSend {
                body: "hi".to_string(),
                reply_to: None
            })
        );
        assert_eq!(panel.buffer(), "");
        assert_eq!(panel.mode(), ChatMode::Normal);
        assert_eq!(world.chat.draft_for(PROJECT), "");
    }

    #[test]
    fn enter_with_an_empty_composer_emits_nothing() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'i');
        assert_eq!(
            tap(&mut panel, &mut world, KeyCode::Enter),
            PanelKey::Consumed
        );
        assert_eq!(panel.mode(), ChatMode::Normal);
    }

    #[test]
    fn composing_mirrors_into_the_project_draft() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'i');
        press(&mut panel, &mut world, 'a');
        press(&mut panel, &mut world, 'b');
        assert_eq!(
            world.chat.draft_for(PROJECT),
            "ab",
            "the panel buffer must be visible to the /chat-* command path"
        );
    }

    #[test]
    fn escape_leaves_compose_mode_and_keeps_the_buffer() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'i');
        press(&mut panel, &mut world, 'd');
        assert_eq!(
            tap(&mut panel, &mut world, KeyCode::Esc),
            PanelKey::Consumed
        );
        assert_eq!(panel.mode(), ChatMode::Normal);
        assert_eq!(
            panel.buffer(),
            "d",
            "Esc must not destroy typed text; the author may want to fix and resend"
        );
    }

    #[test]
    fn exit_keys_belong_to_the_panel_only_while_composing() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        let esc = event(KeyCode::Esc, KeyModifiers::NONE);
        assert!(
            !panel.owns_exit_key(esc),
            "in Normal mode Esc must fall through to the host so it can drop focus"
        );
        press(&mut panel, &mut world, 'i');
        assert!(
            panel.owns_exit_key(esc),
            "while composing, Esc must leave Compose before focus leaves"
        );
        assert!(
            panel.owns_exit_key(event(KeyCode::Char('q'), KeyModifiers::NONE)),
            "`q` is a letter while composing, not a command"
        );
    }

    #[test]
    fn backspace_and_delete_edit_around_the_caret() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'i');
        press(&mut panel, &mut world, 'a');
        press(&mut panel, &mut world, 'b');
        tap(&mut panel, &mut world, KeyCode::Backspace);
        assert_eq!(panel.buffer(), "a");
        tap(&mut panel, &mut world, KeyCode::Home);
        tap(&mut panel, &mut world, KeyCode::Delete);
        assert_eq!(panel.buffer(), "");
    }

    #[test]
    fn reply_prefills_the_author_mention() {
        let mut world = World::seeded(&["first"]);
        let mut panel = bound_panel();
        assert_eq!(
            press(&mut panel, &mut world, 'r'),
            PanelKey::Consumed,
            "reply opens the composer rather than emitting an intent"
        );
        assert_eq!(panel.mode(), ChatMode::Compose);
        assert_eq!(panel.buffer(), "@alice ");
    }

    #[test]
    fn reply_is_not_available_before_chat_is_ready() {
        let mut world = World::new();
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'r');
        assert_eq!(
            panel.mode(),
            ChatMode::Normal,
            "no Ready window means no composer to open"
        );
    }

    #[test]
    fn edit_prefills_the_selected_body_and_emits_an_edit_intent() {
        let mut world = World::seeded(&["original"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'e');
        assert_eq!(panel.mode(), ChatMode::Edit);
        assert_eq!(panel.buffer(), "original");
        tap(&mut panel, &mut world, KeyCode::Backspace);
        assert_eq!(
            tap(&mut panel, &mut world, KeyCode::Enter),
            PanelKey::Intent(TuiMsg::ChatEdit {
                message_id: "m1".to_string(),
                body: "origina".to_string(),
            })
        );
    }

    #[test]
    fn redact_needs_two_presses_and_only_on_the_same_message() {
        let mut world = World::seeded(&["one", "two"]);
        let mut panel = bound_panel();
        // First press arms.
        assert_eq!(press(&mut panel, &mut world, 'X'), PanelKey::Consumed);
        // Moving the cursor re-arms onto the new target rather than
        // confirming a redact aimed at a different message.
        press(&mut panel, &mut world, 'j');
        assert_eq!(press(&mut panel, &mut world, 'X'), PanelKey::Consumed);
        assert_eq!(
            press(&mut panel, &mut world, 'X'),
            PanelKey::Intent(TuiMsg::ChatRedact {
                message_id: "m2".to_string()
            })
        );
    }

    #[test]
    fn redact_is_inert_on_an_already_redacted_message() {
        let mut world = World::seeded(&["one"]);
        let mut panel = bound_panel();
        world.seed_messages("ch1", &["one"]);
        {
            let entry = world.chat.get_mut(PROJECT).expect("entry");
            let msg = entry.messages.back_mut().expect("message");
            msg.redacted = true;
        }
        assert_eq!(press(&mut panel, &mut world, 'X'), PanelKey::Consumed);
        assert_eq!(press(&mut panel, &mut world, 'X'), PanelKey::Consumed);
    }

    #[test]
    fn mark_read_and_refresh_emit_their_intents() {
        let mut world = World::seeded(&["one"]);
        let mut panel = bound_panel();
        assert_eq!(
            press(&mut panel, &mut world, 'm'),
            PanelKey::Intent(TuiMsg::ChatMarkRead)
        );
        assert_eq!(
            press(&mut panel, &mut world, 'R'),
            PanelKey::Intent(TuiMsg::ChatRefresh)
        );
    }

    #[test]
    fn load_older_fails_closed_without_the_daemon_capability() {
        let mut world = World::seeded(&["one"]);
        let mut panel = bound_panel();
        assert!(!world.chat.backward_paging);
        assert_eq!(
            press(&mut panel, &mut world, 'u'),
            PanelKey::Intent(TuiMsg::ChatPagingUnsupported),
            "the daemon cannot decode a backward page; say so instead of breaking silently"
        );
        world.chat.backward_paging = true;
        assert_eq!(
            press(&mut panel, &mut world, 'u'),
            PanelKey::Intent(TuiMsg::ChatLoadOlder)
        );
    }

    #[test]
    fn channel_stepping_emits_the_switch_and_stays_quiet_for_one_channel() {
        let mut world = World::seeded(&["one"]);
        let mut panel = bound_panel();
        // Single channel: nothing to switch to.
        assert_eq!(press(&mut panel, &mut world, ']'), PanelKey::Consumed);

        world.seed_channels(&["ch2"], "room");
        assert!(matches!(
            press(&mut panel, &mut world, ']'),
            PanelKey::Intent(TuiMsg::ChatSwitchChannel { .. })
        ));
    }

    #[test]
    fn repeated_channel_steps_walk_forward_from_the_highlight() {
        let mut world = World::seeded(&["one"]);
        let mut panel = bound_panel();
        world.seed_channels(&["ch2", "ch3"], "room");
        // The reducer has not adopted ch2 yet, so a second press must
        // continue from the highlight rather than re-basing on the active
        // channel and wrapping back around to ch2.
        press(&mut panel, &mut world, ']');
        assert_eq!(panel.channel_cursor(), 1);
        assert!(
            matches!(
                press(&mut panel, &mut world, ']'),
                PanelKey::Intent(TuiMsg::ChatSwitchChannel { ref channel_id }) if channel_id == "ch3"
            ),
            "second press must advance, not wrap"
        );
        assert_eq!(panel.channel_cursor(), 2);
    }

    #[test]
    fn normal_mode_swallows_unbound_keys() {
        let mut world = World::seeded(&["one"]);
        let mut panel = bound_panel();
        // A leaked `j` here would submit an agent turn from the composer.
        for ch in [':', '/', '?', 'z', '1'] {
            assert_eq!(
                press(&mut panel, &mut world, ch),
                PanelKey::Consumed,
                "{ch} must not escape the focused panel"
            );
        }
        assert_eq!(panel.mode(), ChatMode::Normal);
    }

    #[test]
    fn deactivate_clears_an_armed_redact_and_leaves_input_mode() {
        let mut world = World::seeded(&["one", "two"]);
        let mut panel = bound_panel();
        press(&mut panel, &mut world, 'X');
        {
            let mut ctx = world.ctx(true);
            panel.on_deactivate(&mut ctx);
        }
        press(&mut panel, &mut world, 'i');
        {
            let mut ctx = world.ctx(true);
            panel.on_deactivate(&mut ctx);
        }
        assert_eq!(
            panel.mode(),
            ChatMode::Normal,
            "leaving the panel must not strand the user inside an invisible edit"
        );
        // Re-entering must start unarmed: X arms, and only a second X on the
        // same message confirms. A surviving arm would redact m1 on the
        // user's first attempt.
        assert_eq!(press(&mut panel, &mut world, 'X'), PanelKey::Consumed);
        assert_eq!(
            press(&mut panel, &mut world, 'X'),
            PanelKey::Intent(TuiMsg::ChatRedact {
                message_id: "m1".to_string()
            })
        );
    }

    #[test]
    fn render_shows_channel_position_and_messages() {
        let mut world = World::seeded(&["hello world"]);
        let mut panel = bound_panel();
        let text = paint_default(&mut panel, &mut world, true);
        assert!(text.contains("#general"), "{text}");
        assert!(text.contains("1/1"), "{text}");
        assert!(text.contains("hello world"), "{text}");
        assert!(text.contains("Chat"), "wrong panel label:\n{text}");
    }

    #[test]
    fn render_marks_focus_and_shows_a_keybar_only_when_focused() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        let focused = paint_default(&mut panel, &mut world, true);
        assert!(
            focused.contains('●'),
            "focused panel must be marked:\n{focused}"
        );
        assert!(focused.contains("i type"), "{focused}");

        let blurred = paint_default(&mut panel, &mut world, false);
        assert!(!blurred.contains('●'), "{blurred}");
        assert!(!blurred.contains("i type"), "{blurred}");
    }

    #[test]
    fn render_reports_an_unavailable_panel_instead_of_an_empty_box() {
        let mut world = World::new();
        let mut panel = bound_panel();
        let text = paint_default(&mut panel, &mut world, true);
        assert!(text.contains("Chat unavailable"), "{text}");
    }

    #[test]
    fn long_messages_wrap_and_stay_indented_under_the_body() {
        let long = "the quick brown fox jumps over the lazy dog again and again";
        let mut world = World::seeded(&[long]);
        let mut panel = bound_panel();
        let text = paint_default(&mut panel, &mut world, true);
        // The column is 30 wide; the message must wrap onto more than one
        // row rather than being clipped.
        let mut wrapped = 0;
        for row in text.lines() {
            // Every row is prefixed by the block's own border column.
            let inner = row.strip_prefix('│').unwrap_or(row);
            if inner.contains("quick brown") {
                assert!(
                    inner.starts_with('▍'),
                    "first line lost its cursor marker: {inner:?}"
                );
            }
            if inner.contains("jumps over") || inner.contains("dog again") {
                wrapped += 1;
                assert!(
                    inner.starts_with("  "),
                    "continuation line lost its indent: {inner:?}"
                );
            }
        }
        assert!(wrapped >= 2, "long message did not wrap:\n{text}");
    }

    #[test]
    fn a_narrow_terminal_renders_nothing_rather_than_panicking() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        for area in [
            Rect::new(0, 0, 4, 20),
            Rect::new(0, 0, 30, 2),
            Rect::new(0, 0, 0, 0),
        ] {
            paint(&mut panel, &mut world, area, true);
        }
    }

    #[test]
    fn composing_shows_the_caret_only_while_composing() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        assert!(!paint_default(&mut panel, &mut world, true).contains('▸'));
        press(&mut panel, &mut world, 'i');
        let text = paint_default(&mut panel, &mut world, true);
        assert!(text.contains('▸'), "{text}");
        assert!(text.contains('▌'), "composer must show a caret:\n{text}");
    }

    #[test]
    fn a_send_failure_is_surfaced_on_the_status_row() {
        let mut world = World::seeded(&["hello"]);
        let mut panel = bound_panel();
        world
            .chat
            .note_failed_send(PROJECT, "kept".to_string(), "denied by policy".to_string());
        let text = paint_default(&mut panel, &mut world, true);
        assert!(text.contains('✗'), "a failed send must be marked:\n{text}");
        assert!(text.contains("denied by policy"), "{text}");
    }
}
