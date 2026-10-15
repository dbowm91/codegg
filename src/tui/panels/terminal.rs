//! The interactive terminal as a swappable side panel.
//!
//! ## Relationship to `/terminal`
//!
//! A peer of the terminal *box*, built on the same
//! [`InteractiveTerminalController`] rather than a second implementation.
//! Both surfaces read the same views, forward through the same
//! focus gate, and send through the same
//! [`send_terminal_input`](crate::tui::commands::interactive_terminal::send_terminal_input)
//! command seam, so a key cannot mean one thing in the box and another in the
//! panel.
//!
//! ## Mode
//!
//! The box's `Viewing`/`Focused` pair is exactly the app's Normal/Insert
//! split, and it is reused rather than re-invented:
//!
//! * **NOR** — the panel has the keyboard but the PTY does not, and it is a
//!   *reader*: `j/k/h/l` and arrows move the cursor and scroll at the edges,
//!   `Ctrl+u`/`Ctrl+d` page, `e`/`b` jump by word, `v` selects by character,
//!   `x` selects the cursor's whole line and grows one line per press, `d`
//!   deletes the selection, `space+y` yanks to the clipboard, and `C` clears.
//!   `i` takes the PTY — or starts a shell when there is none to
//!   take, since a terminal panel that refuses focus leaves nowhere to type
//!   and nothing to fix it with. Esc drops a selection before it leaves. The
//!   directional gesture is claimed by the host before it reaches here.
//!   Selection needs a history buffer to select *from*, so the emulator keeps
//!   `TERMINAL_SCROLLBACK_LINES` rows rather than only the live screen.
//!
//!   The `v`/`x` split is the shared state machine's rule, not this file's:
//!   `x` is an *action* (repeat to add a line) and `v` is a *mode* (movement
//!   extends). `d` is an *operator* and consumes what it acted on, so a bare
//!   `d` is a no-op rather than a fallback. `C` rather than `d` for clear is
//!   what frees `d` to mean delete.
//! * **INS** — the PTY owns the keyboard. Every key is mapped by
//!   [`map_key`] and gated by `classify_focused`; `Esc Esc` leaves.
//!
//! Focus lives on the *controller's* view rather than on the panel, so the
//! box and the panel cannot disagree about whether the PTY is live.
//!
//! ## Width
//!
//! The PTY is sized to the body rect this panel actually has, queued through
//! `queue_resize` and drained by `flush_terminal_resize`. Sizing it from the
//! outer terminal instead — which is what `terminal_viewport_size` does for the
//! initial box — leaves the program wrapping to columns this column cannot
//! show, so the reader sees the left edge of a much wider screen. Reflowing is
//! the cost: a resize re-wraps what the program believes it has, which is the
//! same trade every terminal makes and the only one that leaves the output
//! legible.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::tui::app::TuiMsg;
use crate::tui::commands::interactive_terminal::{apply_nav, map_key, NavOutcome};
use crate::tui::input::InputAction;
use crate::tui::interactive_terminal::TerminalKeyAction;
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

    /// Never refuses focus.
    ///
    /// Every other panel that refuses is refusing because it would swallow
    /// keys it cannot act on — an unbound Chat has no channel to show. This
    /// panel always has something to do: `i` takes the keyboard of a terminal
    /// that exists, or starts a shell when there is none. Refusing here was
    /// the actual dead end — an empty panel took no keys, so `i` never reached
    /// it, and the only way to get a terminal was a command typed elsewhere.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// What `i` should ask for.
    ///
    /// The dead case is routed to `TerminalPanelFocus` rather than straight to
    /// `StartTerminal` on purpose: only the command layer can tell an exited
    /// process (replace it) from a detached one (re-attach), and the panel is
    /// not allowed to know that difference.
    fn type_intent(&self) -> TuiMsg {
        match self.handle.as_deref() {
            Some(handle) => TuiMsg::TerminalPanelFocus {
                handle: handle.to_string(),
            },
            None => TuiMsg::StartTerminal,
        }
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

/// Queue a PTY resize matching the real body rect.
///
/// Last-wins and coalesced on the controller, so a column that resizes every
/// frame while a drag settles costs one daemon round trip rather than one per
/// frame. The queued size is drained by `flush_terminal_resize`, shared with
/// the input flush — the render path has no `App`, so this cannot send it.
///
/// Skipped when the size already matches. Not-live terminals are rejected by
/// `queue_resize`, whose typed error is deliberately discarded here: a panel
/// showing an exited process must keep rendering rather than toast on every
/// frame.
fn queue_viewport_resize(ctx: &mut PanelCtx<'_>, handle: &str, cols: u16, rows: u16) {
    if cols == 0 || rows == 0 {
        return;
    }
    if ctx.terminal.dimensions(handle) == Some((cols, rows)) {
        return;
    }
    let _ = ctx.terminal.queue_resize(handle, cols, rows);
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
                " press i for a shell",
                Style::default().fg(theme.muted),
            )))
            .render(rows[1], buf);
            return;
        };
        Paragraph::new(self.header(ctx, &theme)).render(rows[0], buf);

        // Match the PTY to the real body rect. The panel is typically far
        // narrower than the outer terminal the viewport size was derived from,
        // so without this the program wraps to columns nobody can see and the
        // column shows the left edge of a much wider screen.
        let body_height = usize::from(rows[1].height);
        let body_width = rows[1].width;
        queue_viewport_resize(
            ctx,
            &handle,
            body_width,
            u16::try_from(body_height).unwrap_or(1),
        );

        let highlight = Style::default().bg(theme.selection);
        match ctx.terminal.render_view(&handle, body_height, highlight) {
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
        // A handle whose view has gone is dropped rather than reported, but
        // the panel stays actionable: `i` then starts a shell, which is what
        // a terminal emulator does when you ask it for a new one.
        if let Some(handle) = self.handle.clone() {
            if ctx.terminal.view(&handle).is_none() {
                self.forget(&handle);
            }
        }
        let focused = self.handle.as_deref().is_some_and(|handle| {
            ctx.terminal.view(handle).is_some() && ctx.terminal.is_focused(handle)
        });

        if !focused {
            // NOR. `i` is the way in — to an existing PTY, or to a new one
            // when there is nothing live to type into. The directional
            // gesture was already claimed by the host.
            match key.code {
                KeyCode::Char('i') if key.modifiers == KeyModifiers::NONE => {
                    return PanelKey::Intent(self.type_intent());
                }
                KeyCode::Enter => return PanelKey::Intent(self.type_intent()),
                _ => {}
            }
            // No `Esc` arm: the host claims `Esc`/`q` as panel exits before
            // the panel is consulted, so an Esc arm here would be unreachable.
            // The selection survives the blur anyway — it lives on the
            // controller, not the panel — so stepping out and back keeps the
            // text the reader was about to copy.
            let Some(handle) = self.handle.clone() else {
                return PanelKey::Consumed;
            };
            // Helix navigation, selection, and `space+y` yank, through the
            // same seam the box uses. Unhandled keys stay consumed rather than
            // falling through to the composer: a stray `q` must never be
            // forwarded as PTY input, and the panel owns every key it shows.
            return match apply_nav(ctx.terminal, &handle, key) {
                NavOutcome::Unhandled => PanelKey::Consumed,
                NavOutcome::Handled => PanelKey::Consumed,
                NavOutcome::ClearRequested => {
                    PanelKey::Intent(TuiMsg::TerminalPanelClear { handle })
                }
            };
        }
        let Some(handle) = self.handle.clone() else {
            return PanelKey::Consumed;
        };

        // INS. Every key is classified by the same gate the box uses, so
        // the two cannot disagree about what is a control sequence — including
        // the `Esc Esc` arming, which lives on the view this panel reads
        // through `ctx` rather than in a private copy.
        match ctx.terminal.classify_focused(&handle, map_key(key)) {
            TerminalKeyAction::EscapeFocus => {
                PanelKey::Intent(TuiMsg::TerminalPanelBlur { handle })
            }
            TerminalKeyAction::Forward(bytes) => {
                PanelKey::Intent(TuiMsg::TerminalPanelInput { handle, bytes })
            }
            TerminalKeyAction::Ignored => PanelKey::Consumed,
        }
    }

    /// Claim `Esc`/`q` while the PTY is focused (INS).
    ///
    /// The host drops panel focus on either key *without consulting the
    /// controller*, so a focused terminal that declined them left the two focus
    /// flags disagreeing: the panel was no longer focused, but the view still
    /// read `TerminalFocus::Focused`. That is the reported "no key combo lets
    /// you leave" — `q` did drop panel focus while the PTY kept the keyboard,
    /// and refocusing the panel put the user straight back into INS with no
    /// route out. Claiming the key routes it through [`Self::handle_key`],
    /// which owns the `Esc Esc` rule and ends in
    /// `TuiMsg::TerminalPanelBlur` — the one call that clears the view's focus.
    ///
    /// Only in INS: while viewing there is no PTY keyboard to protect, so the
    /// host's exit is the correct answer and keeps `q`/`Esc` cheap.
    fn owns_exit_key(&self, key: KeyEvent, ctx: &PanelCtx<'_>) -> bool {
        matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
            && self
                .handle
                .as_deref()
                .is_some_and(|handle| ctx.terminal.is_focused(handle))
    }

    fn composer_hints(&self, hints: &PanelHints<'_>) -> Option<String> {
        Some(if !hints.focused {
            // Unfocused: navigation is still this panel's business, so the
            // line is not empty — but `i` only works with the keyboard, and
            // saying so is the difference between a dead key and a mode.
            match self.handle {
                None => "TERM · focus to start a shell".to_string(),
                Some(_) => "TERM · focus to scroll, select and type".to_string(),
            }
        } else if self.handle.is_none() {
            "TERM · i start a shell · v chars · x lines · d delete · space+y copy · C clear"
                .to_string()
        } else {
            "TERM · i type · Esc Esc stop · v chars · x lines · d delete · space+y copy · C clear"
                .to_string()
        })
    }

    fn on_activate(&mut self, ctx: &mut PanelCtx<'_>) {
        // Adopt whatever the controller considers active, so selecting the
        // panel shows the terminal the user was just looking at rather than
        // an empty column. An empty controller leaves the panel with nothing
        // to show, which `i` then fixes by starting a shell — the panel never
        // refuses focus, so it never strands the keyboard here.
        if self.handle.is_none() {
            self.show(ctx.terminal.active_handle());
        }
    }
}
