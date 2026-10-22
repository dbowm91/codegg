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
//! This is a peer of [`ChatDialog`](crate::tui::components::dialogs::chat::ChatDialog)
//! — the same workflow, in a more convenient place — and it is deliberately
//! built on the *same implementation* rather than a parallel one. Both
//! surfaces own a [`ChatBrowser`](crate::tui::chat_browser::ChatBrowser) and
//! nothing else: the cursor, the keymap, the row rendering, the viewport
//! anchor, and the key hints all come from that one place. A verb added to
//! `/chat` is therefore present here on the same commit, which is the whole
//! point — the previous peer implementation drifted until the panel could
//! neither reply nor edit.
//!
//! The single difference is where the caret lives. The dialog has its own
//! bounded composer; this panel has none, because the bottom composer is the
//! one input surface for every destination. So a verb that needs typing
//! resolves to a [`ChatComposeSeed`](crate::tui::chat_browser::ChatComposeSeed)
//! that arms the shared composer — including the key hints above the input,
//! which come from [`ChatPanel::composer_hints`].
//!
//! ## Authority
//!
//! The panel never picks a project. `PanelCtx::chat_project` is resolved by
//! the command layer from the active tab or the Workspace selection, and a
//! panel with no bound project refuses to take focus at all — the host
//! routes every key to whichever panel is active, and a chat panel that
//! cannot act would otherwise swallow the composer's keystrokes.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::tui::app::state::chat::ProjectChat;
use crate::tui::app::TuiMsg;
use crate::tui::chat_browser::{self, ChatBrowser, ChatKeyAction, ChatView};
use crate::tui::input::InputAction;
use crate::tui::theme::Theme;
use crate::tui::wrap::ellipsize_to_width;

use super::{PanelCtx, PanelHints, PanelKey, SidePanel, SidePanelId};

/// Read-only project-chat side panel.
///
/// Holds only per-view state — the shared [`ChatBrowser`] plus the mirror of
/// `App::chat_panel_project`. Message data lives in `ChatState`, read fresh
/// every frame. There is deliberately **no** composer buffer here: the bottom
/// composer is the single input surface for both send targets, and a second
/// live text buffer would put two cursors on screen with nothing to say which
/// one Enter would use.
#[derive(Debug, Default)]
pub struct ChatPanel {
    browser: ChatBrowser,
    /// Mirrors `App::chat_panel_project` — true when a project is bound.
    /// Kept in step by the same command functions that set the App field,
    /// because `SidePanelHost::focus` has no access to app state.
    bound: bool,
}

impl ChatPanel {
    pub fn cursor(&self) -> usize {
        self.browser.cursor()
    }

    pub fn channel_cursor(&self) -> usize {
        self.browser.channel_cursor()
    }

    pub fn browser(&self) -> &ChatBrowser {
        &self.browser
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
    /// Called on revocation and project change so nothing composed against
    /// one projection survives into another.
    pub fn reset(&mut self) {
        self.browser.reset();
    }

    // ── Reading state ─────────────────────────────────────────────────

    fn entry<'a>(&self, ctx: &'a PanelCtx<'_>) -> Option<&'a ProjectChat> {
        ctx.chat_project.and_then(|p| ctx.chat.get(p))
    }

    /// The projection the browser renders, in the one shape both surfaces
    /// share.
    fn view<'a>(&self, ctx: &'a PanelCtx<'a>) -> Option<ChatView<'a>> {
        let entry = self.entry(ctx)?;
        Some(ChatView {
            messages: &entry.messages,
            channels: &entry.channels,
            active_channel_id: entry.active_channel_id.as_deref(),
            status: entry.status,
            unread: entry.unread_count(),
            backward_paging: ctx.chat.backward_paging,
        })
    }

    // ── Key routing ───────────────────────────────────────────────────

    /// Map a browser action onto the host's key contract.
    ///
    /// The one interesting case is `Compose`. A bare `i`/`a`/`Enter` carries
    /// no text and no target — it means "let me type" — so it returns
    /// [`PanelKey::PassThrough`] and the shared composer's own `FocusPrompt`
    /// binding opens Insert mode. That is what makes "type anywhere, land in
    /// the armed target" true: the same key that enters the agent prompt
    /// enters chat compose.
    ///
    /// `r`/`e` are different: they arrive pre-seeded with an author mention,
    /// a reply target, or the body being replaced. Falling through would
    /// strand that context, so they emit an intent that arms the composer
    /// *and* switches it to Insert, which is the dialog's behaviour reached
    /// by a different route.
    fn route(&self, action: ChatKeyAction, ctx: &PanelCtx<'_>) -> PanelKey {
        match action {
            ChatKeyAction::Consumed => PanelKey::Consumed,
            // The browser's `Exit` is the surface's own "leave" gesture: it
            // closes the `/chat` window. Inside the panel it cannot mean
            // that — `Esc` is the mode switch and never releases the panel —
            // so it means the one thing `Esc` still means here: back out of
            // the armed reply/edit context, keeping the typed text.
            //
            // The panel names the intent rather than reaching into the app's
            // prompt state, so the browser stays the single owner of the
            // keymap and the app stays the single owner of the context.
            ChatKeyAction::Exit => PanelKey::Intent(TuiMsg::CancelChatComposer),
            ChatKeyAction::Intent(msg) => PanelKey::Intent(msg),
            ChatKeyAction::Compose(seed) => {
                if seed.prefill.is_none() && !seed.is_contextual() {
                    return PanelKey::PassThrough;
                }
                if ctx.chat_project.is_none() {
                    return PanelKey::Consumed;
                }
                PanelKey::Intent(TuiMsg::ChatComposerSeed(seed))
            }
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────

impl ChatPanel {
    fn header(&self, view: &ChatView<'_>, theme: &Theme) -> Line<'static> {
        let total = view.message_count();
        let position = if total == 0 { 0 } else { self.cursor() + 1 };
        Line::from(vec![
            Span::styled(
                format!(" #{} ", view.channel_name()),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{position}/{total}{}",
                    if view.unread > 0 {
                        format!(" ·{}", view.unread)
                    } else {
                        String::new()
                    }
                ),
                Style::default().fg(theme.muted),
            ),
        ])
    }

    /// Status row: armed redact first (it is the thing waiting on the user),
    /// then send errors, then typing leases. Errors win over the typing
    /// indicator so a failed send is never hidden behind it.
    fn status_line(&self, ctx: &PanelCtx<'_>, theme: &Theme) -> Line<'static> {
        if self.browser.redact_armed().is_some() {
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

    /// Which destination the bottom composer is currently armed for.
    ///
    /// The panel has no composer of its own, so this row answers the question
    /// its old composer row used to answer — "where does what I type go?" —
    /// plus the one the badge alone cannot: *which panel has the keyboard*.
    /// A focused panel is drawn with a `▸` marker and says so in words,
    /// because mistaking the focused region for the send target is how a
    /// prompt silently lands in the team chat.
    fn target_line(&self, ctx: &PanelCtx<'_>, theme: &Theme) -> Line<'static> {
        let armed_chat = ctx.chat_project.is_some();
        let (marker, name, hint) = if !ctx.focused {
            (" ", "", "Shift+→ focus · ⇧S switch")
        } else if armed_chat {
            ("▸", " CHAT ", "↵ sends here · Shift+← back")
        } else {
            ("▸", " SESSIONS ", "↵ sends to agent · i to type")
        };
        let marker_style = if ctx.focused {
            Style::default()
                .fg(theme.background)
                .bg(theme.warning)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        let name_style = if ctx.focused {
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        Line::from(vec![
            Span::styled(marker, marker_style),
            Span::styled(name, name_style),
            Span::styled(format!(" {hint}"), Style::default().fg(theme.muted)),
        ])
    }
}

impl SidePanel for ChatPanel {
    fn id(&self) -> SidePanelId {
        SidePanelId::Chat
    }

    fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
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

        // The destination row is unconditional. Collapsing it whenever the
        // panel was unfocused made "which panel has the keyboard" invisible,
        // which is how a prompt reaches the agent while the user believes
        // they are typing into chat.
        //
        // There is no in-panel keybar: the keys for the panel that owns the
        // keyboard are rendered on the shared hint line above the composer,
        // so they sit next to the input they modify rather than inside the
        // panel they apply to.
        let rows = Layout::vertical([
            Constraint::Length(1), // header
            Constraint::Min(1),    // body
            Constraint::Length(1), // destination / focus marker
            Constraint::Length(1), // status
        ])
        .split(inner);

        // Nothing bound yet: say so rather than showing an empty box the
        // user cannot act in. The panel's own recorded binding is consulted
        // as well as the context's project, because a panel that has been
        // told it is unbound must not render an armed `CHAT` destination —
        // the two are set in lockstep by `sync_chat_panel_project`, so
        // either one alone would be a second source of truth.
        if !self.bound || self.entry(ctx).is_none() {
            let message = if !self.bound || ctx.chat_project.is_none() {
                "No project bound"
            } else {
                "Chat unavailable"
            };
            Paragraph::new(Line::from(Span::styled(
                format!(" {message}"),
                Style::default().fg(theme.muted),
            )))
            .render(rows[1], buf);
            return;
        }

        let Some(view) = self.view(ctx) else {
            return;
        };
        Paragraph::new(self.header(&view, &theme)).render(rows[0], buf);

        // Reconcile the cursor with the freshly read window *before*
        // rendering: the reducer may have applied a page since the last key.
        // On the very first frame this is what puts the cursor on the newest
        // message — the panel used to open on index 0, so a channel with
        // history looked empty because only the oldest rows were on screen.
        self.browser.sync(&view);

        let body_height = usize::from(rows[1].height);
        let lines = chat_browser::rows(&view, self.cursor(), rows[1].width, &theme);
        if lines.is_empty() {
            Paragraph::new(Line::from(Span::styled(
                match view.status {
                    crate::tui::app::state::chat::ChatStatus::Unavailable => " chat unavailable",
                    crate::tui::app::state::chat::ChatStatus::Loading
                    | crate::tui::app::state::chat::ChatStatus::Unknown => " loading…",
                    _ => " no messages — press i to write",
                },
                Style::default().fg(theme.muted),
            )))
            .render(rows[1], buf);
        } else {
            let selected_end = chat_browser::selected_row_end(&lines, self.cursor());
            let range = chat_browser::window(lines.len(), selected_end, body_height);
            let window: Vec<Line> = range.map(|index| lines[index].line.clone()).collect();
            Paragraph::new(window).render(rows[1], buf);
        }

        Paragraph::new(self.target_line(ctx, &theme)).render(rows[2], buf);
        Paragraph::new(self.status_line(ctx, &theme)).render(rows[3], buf);
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        _action: Option<InputAction>,
        ctx: &mut PanelCtx<'_>,
    ) -> PanelKey {
        // The panel is a viewer, so there is no input mode to dispatch on:
        // text entry belongs to the composer and reaches it either by
        // `PassThrough` or by arming it with a seed.
        let Some(view) = self.view(ctx) else {
            return PanelKey::Consumed;
        };
        // A bare `q` is the program's quit key and `Shift+Q` is the panel
        // exit. A focused panel owns the keyboard, so `q` here must neither
        // quit the program nor release the panel: it is inert, and the hint
        // row names `Shift+Q` for leaving.
        //
        // The browser maps `q` to its own `Exit` so that the `/chat` window
        // can be closed with it, which is why the panel filters the key out
        // rather than inheriting a gesture meant for a different surface.
        if key.code == KeyCode::Char('q') && key.modifiers.is_empty() {
            return PanelKey::Consumed;
        }
        let action = self.browser.handle_key(key, &view);
        let routed = self.route(action, ctx);
        // Reconcile after the move so a clamped cursor is what the caller
        // observes, and so the next frame does not re-derive it.
        self.browser.sync(&view);
        routed
    }

    fn composer_hints(&self, hints: &PanelHints<'_>) -> Option<String> {
        if !hints.focused || !self.bound || hints.chat_project.is_none() {
            return None;
        }
        // The panel names itself here so the line answers both questions at
        // once: *which region owns the keyboard*, and *what its keys do*.
        // The `[BUILD]`/`[CHAT]` badge says where text will be delivered,
        // which is not the same question — focusing Sessions does not retarget
        // the composer, and a line that implied it would would be a lie.
        let normal = chat_browser::normal_hints(self.browser.show_help());
        Some(format!("CHAT · {normal} · ⇧S panel"))
    }

    fn on_activate(&mut self, ctx: &mut PanelCtx<'_>) {
        // Re-anchor on whatever the reducer reports as active so a completed
        // switch lands where the user aimed and the next press continues
        // from there, and so reopening the panel starts at live.
        if let Some(view) = self.view(ctx) {
            self.browser.sync(&view);
        }
    }

    fn on_deactivate(&mut self, _ctx: &mut PanelCtx<'_>) {
        // Leaving must not strand a destructive action armed.
        self.browser.clear_redact();
    }
}
