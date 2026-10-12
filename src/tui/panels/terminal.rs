//! The interactive terminal as a swappable side panel.
//!
//! ## Relationship to `/terminal`
//!
//! A peer of the terminal *box*, built on the same
//! [`InteractiveTerminalController`] rather than a second implementation.
//! Both surfaces read the same views, forward through the same
//! [`classify_key`] gate, and send through the same
//! [`send_terminal_input`](crate::tui::commands::interactive_terminal::send_terminal_input)
//! command seam, so a key cannot mean one thing in the box and another in the
//! panel.
//!
//! ## Mode
//!
//! The box's `Viewing`/`Focused` pair is exactly the app's Normal/Insert
//! split, and it is reused rather than re-invented:
//!
//! * **NOR** — the panel has the keyboard but the PTY does not. Arrow and
//!   page keys scroll; `i` takes the PTY; the directional gesture is claimed
//!   by the host before it reaches here.
//! * **INS** — the PTY owns the keyboard. Every key is mapped by
//!   [`map_key`] and gated by [`classify_key`]; `Ctrl-]` leaves.
//!
//! Focus lives on the *controller's* view rather than on the panel, so the
//! box and the panel cannot disagree about whether the PTY is live.
//!
//! ## Width
//!
//! The box renders in a 60×50 centred rect; the column is about 30 wide.
//! The emulator's own screen state is unchanged by that, and `Paragraph`
//! clips what does not fit — so a narrow panel shows the left edge of the
//! emulator screen, wrapped by the program rather than by us. Reflowing
//! would mean re-feeding the parser at a different width and losing columns
//! the program believes it has.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::tui::app::TuiMsg;
use crate::tui::commands::interactive_terminal::map_key;
use crate::tui::input::InputAction;
use crate::tui::interactive_terminal::{classify_key, TerminalKeyAction};
use crate::tui::theme::Theme;

use super::{PanelCtx, PanelHints, PanelKey, SidePanel, SidePanelId};

/// Interactive terminal side panel.
///
/// Holds only per-view state — which handle this panel shows. Every byte of
/// output, every focus flag, and the whole VT parser live on the controller
/// in `PanelCtx`, because the box needs them too and two parsers for one PTY
/// would disagree about what is on screen.
#[derive(Debug, Default)]
pub struct TerminalPanel {
    handle: Option<String>,
}

impl TerminalPanel {
    /// Handle this panel is showing, if any.
    pub fn handle(&self) -> Option<&str> {
        self.handle.as_deref()
    }

    /// Show `handle`, or the controller's active view when it is already
    /// gone.
    ///
    /// Adopted rather than invented: the panel never resolves a terminal,
    /// the same way the chat panel never resolves a project.
    pub fn show(&mut self, handle: Option<&str>) {
        self.handle = handle.map(str::to_string);
    }

    /// Forget the handle. Called when the view it names disappears, so the
    /// panel degrades to "no terminal" instead of rendering an error for a
    /// handle that can never come back.
    pub fn forget(&mut self, handle: &str) {
        if self.handle.as_deref() == Some(handle) {
            self.handle = None;
        }
    }

    /// True when there is nothing to show. The host refuses focus rather
    /// than stranding keys in a panel that cannot act on them.
    pub fn is_empty(&self) -> bool {
        self.handle.is_none()
    }
}

// ── Rendering ─────────────────────────────────────────────────────────

impl TerminalPanel {
    fn header(&self, ctx: &PanelCtx<'_>, theme: &Theme) -> Line<'static> {
        let Some(handle) = self.handle.as_deref() else {
            return Line::default();
        };
        let typing = ctx.terminal.is_focused(handle);
        Line::from(vec![
            Span::styled(
                format!(" {handle}"),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if typing { "  INS" } else { "  NOR" },
                Style::default()
                    .fg(if typing { theme.primary } else { theme.muted })
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    }

    fn status(&self, ctx: &PanelCtx<'_>, theme: &Theme) -> Line<'static> {
        let Some(handle) = self.handle.as_deref() else {
            return Line::default();
        };
        let Some(view) = ctx.terminal.view(handle) else {
            return Line::default();
        };
        Line::from(Span::styled(
            format!(
                " {}",
                crate::tui::wrap::ellipsize_to_width(&view.status_line(), 28)
            ),
            Style::default().fg(theme.muted),
        ))
    }
}

impl SidePanel for TerminalPanel {
    fn id(&self) -> SidePanelId {
        SidePanelId::Terminal
    }

    fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
        let theme = ctx.theme.clone();
        if area.width < 12 || area.height < 4 {
            return;
        }
        let title = if ctx.focused {
            " Terminal ● "
        } else {
            " Terminal "
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
        let rows = Layout::vertical([
            Constraint::Length(1), // header
            Constraint::Min(1),    // screen
            Constraint::Length(1), // status
        ])
        .split(inner);

        let Some(handle) = self.handle.clone() else {
            Paragraph::new(Line::from(Span::styled(
                " No terminal — /terminal",
                Style::default().fg(theme.muted),
            )))
            .render(rows[1], buf);
            return;
        };
        Paragraph::new(self.header(ctx, &theme)).render(rows[0], buf);

        let body_height = usize::from(rows[1].height);
        match ctx.terminal.render_styled_lines(&handle, body_height) {
            Some(lines) if !lines.is_empty() => Paragraph::new(lines).render(rows[1], buf),
            _ => Paragraph::new(Line::from(Span::styled(
                " no output — press i to type",
                Style::default().fg(theme.muted),
            )))
            .render(rows[1], buf),
        }
        Paragraph::new(self.status(ctx, &theme)).render(rows[2], buf);
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        _action: Option<InputAction>,
        ctx: &mut PanelCtx<'_>,
    ) -> PanelKey {
        let Some(handle) = self.handle.clone() else {
            return PanelKey::Consumed;
        };
        if ctx.terminal.view(&handle).is_none() {
            // The view went away underneath us. Forget it rather than
            // reporting an error the user cannot cause or clear.
            self.forget(&handle);
            return PanelKey::Consumed;
        }

        if !ctx.terminal.is_focused(&handle) {
            // NOR. `i` is the way in; the directional gesture was already
            // claimed by the host, so anything else here is inert rather
            // than forwarded — a stray `q` must never become PTY input.
            return match key.code {
                KeyCode::Char('i') if key.modifiers == KeyModifiers::NONE => {
                    PanelKey::Intent(TuiMsg::TerminalPanelFocus { handle })
                }
                KeyCode::Enter => PanelKey::Intent(TuiMsg::TerminalPanelFocus { handle }),
                _ => PanelKey::Consumed,
            };
        }

        // INS. Every key is classified by the same gate the box uses, so
        // the two cannot disagree about what is a control sequence.
        match classify_key(true, map_key(key)) {
            TerminalKeyAction::EscapeFocus => {
                PanelKey::Intent(TuiMsg::TerminalPanelBlur { handle })
            }
            TerminalKeyAction::Forward(bytes) => {
                PanelKey::Intent(TuiMsg::TerminalPanelInput { handle, bytes })
            }
            TerminalKeyAction::Ignored => PanelKey::Consumed,
        }
    }

    fn composer_hints(&self, hints: &PanelHints<'_>) -> Option<String> {
        if !hints.focused || self.handle.is_none() {
            return None;
        }
        Some("TERM · i type · ^-] stop · Shift+← back".to_string())
    }

    fn on_activate(&mut self, ctx: &mut PanelCtx<'_>) {
        // Adopt whatever the controller considers active, so selecting the
        // panel shows the terminal the user was just looking at rather than
        // an empty column. An empty controller leaves the panel empty, which
        // the host reads as "cannot act" and refuses to focus.
        if self.handle.is_none() {
            self.show(ctx.terminal.active_handle());
        }
    }
}
