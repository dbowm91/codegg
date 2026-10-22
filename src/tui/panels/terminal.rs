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
//!   [`map_key`] and gated by `classify_focused`; `Esc` leaves, `Ctrl+]` sends
//!   a literal escape to the child.
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
use crate::tui::input::{InputAction, InputMode};
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
        match ctx
            .terminal
            .render_view(&handle, body_height, ctx.caret_blink_on, highlight)
        {
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
            // No `Esc` arm: in NOR there is no mode to leave, and the host
            // no longer treats `Esc` as a panel exit, so `Esc` lands here as
            // an ordinary key and is handled by the navigation seam below
            // (or consumed). The panel keeps the keyboard and the selection;
            // `Shift+Q` is how the user leaves it.
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
        // the two cannot disagree about what is a control sequence. `Esc`
        // leaves insert here, exactly as it does in the box.
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

    /// Decline the host's directional gesture while the PTY owns the keyboard.
    ///
    /// In INS every key is program input. `H`/`J`/`K`/`L` are letters the user
    /// types — in commands, paths and environment variables — so letting the
    /// host read them as "move this panel" drops the character and relocates
    /// the column. The gesture belongs to reading, where there is no keyboard
    /// to protect.
    fn claims_panel_directions(&self, ctx: &PanelCtx<'_>) -> bool {
        !self
            .handle
            .as_deref()
            .is_some_and(|handle| ctx.terminal.is_focused(handle))
    }

    /// Claim the host's exit key (`Shift+Q`) while the PTY is focused (INS).
    ///
    /// The host drops panel focus *without consulting the controller*, so a
    /// focused terminal that declined the key left the two focus flags
    /// disagreeing: the panel was no longer focused, but the view still read
    /// `TerminalFocus::Focused`. Claiming the key routes it through
    /// [`Self::handle_key`], which owns the mode rule and ends in
    /// `TuiMsg::TerminalPanelBlur` — the one call that clears the view's focus.
    ///
    /// `Shift+Q` only, and **never a bare `q`**. In INS the panel is a
    /// terminal, and `q` is a character the user is typing — claiming it here
    /// meant the shell could never receive a `q`, which is the most common
    /// letter in most commands. `Shift+Q` is the host's exit, so it is the one
    /// key that can mean "leave", and in INS it is program input like any
    /// other: it reaches the shell and the panel keeps the keyboard.
    ///
    /// Only in INS: while reading there is no PTY keyboard to protect, so the
    /// host's exit is the correct answer and keeps the key cheap. In NOR the
    /// host's own blur is right — nothing is typing — and `Esc` is the mode
    /// switch that leaves INS, not the key that leaves the panel.
    fn owns_exit_key(&self, key: KeyEvent, ctx: &PanelCtx<'_>) -> bool {
        key.modifiers.contains(KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::Char('Q'))
            && self
                .handle
                .as_deref()
                .is_some_and(|handle| ctx.terminal.is_focused(handle))
    }

    /// Take the keyboard in Insert mode whenever this panel is showing a
    /// live PTY.
    ///
    /// This panel renders a process, not a text field, so Insert belongs to
    /// the process. Without the opt-in, docking the terminal with `Shift+L`
    /// and typing sent every keystroke to a composer the user was not looking
    /// at — the panel looked frozen and the characters "disappeared".
    ///
    /// `is_focused` is the controller's own verdict, so a terminal that is
    /// still attaching or has exited does **not** claim the keyboard: those
    /// keys fall through to the composer, where they are at least visible and
    /// recoverable instead of vanishing.
    fn claims_insert_keys(&self, ctx: &PanelCtx<'_>) -> bool {
        self.handle
            .as_deref()
            .is_some_and(|handle| ctx.terminal.is_focused(handle))
    }

    fn composer_hints(&self, hints: &PanelHints<'_>) -> Option<String> {
        // Unfocused: `None`.
        //
        // This used to answer `"TERM · focus to scroll, select and type"`,
        // keeping the row alive above a composer that had the keyboard. The
        // row exists to name whoever holds the keyboard, and every verb in it
        // — including the gesture that would reclaim them — needs the keyboard
        // it had just given up. So the line sat under the LLM buffer describing
        // a terminal nobody was in, and the `Shift+←/→` gesture that would have
        // brought it back was itself the thing the line was advertising.
        //
        // Nothing is lost. The panel's own chrome still says a terminal is
        // docked and which one, the `[TERM]` badge names the destination, and
        // the row comes back the moment the panel takes the keyboard again.
        // See `SidePanelHost::composer_hints` for why this panel — a *bespoke*
        // one — declines while a hosted `/files` panel still advertises.
        if !hints.focused {
            return None;
        }
        Some(if self.handle.is_none() {
            "TERM · i start a shell · v chars · x lines · d delete · space+y copy · C clear"
                .to_string()
        } else if hints.input_mode == InputMode::Insert {
            "TERM · INS typing to the shell · Esc read · Ctrl+] literal esc".to_string()
        } else {
            "TERM · i type · hjkl move · v chars · x lines · space+y copy · C clear".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::panels::PanelPlacement;
    use std::sync::Arc;

    fn hints(focused: bool) -> PanelHints<'static> {
        PanelHints {
            focused,
            placement: PanelPlacement::Right,
            chat_project: None,
            input_mode: InputMode::Normal,
        }
    }

    /// The reported defect, at the seam: the row above the composer must
    /// describe whoever holds the keyboard, and after a `Shift+←/→` swap the
    /// terminal no longer holds it. Every branch of the old answer named
    /// *terminal* verbs — `focus to scroll, select and type` — at a user whose
    /// keys were going to the LLM buffer, which is the one thing that line
    /// exists to prevent.
    #[test]
    fn the_panel_declines_the_row_when_another_surface_holds_the_keyboard() {
        let mut panel = TerminalPanel::default();
        assert_eq!(
            panel.composer_hints(&hints(false)),
            None,
            "an unfocused panel with no terminal must not claim the row either"
        );
        panel.show(Some("t-1"));
        assert_eq!(
            panel.composer_hints(&hints(false)),
            None,
            "a hint describing the terminal's own verbs must not survive losing the keyboard"
        );
    }

    /// The other half of the same rule, so the fix cannot be "always `None`".
    /// Both modes are claimed from here, and neither depends on the view's own
    /// focus flag — the derived mode is the answer, not `is_focused`.
    #[test]
    fn the_panel_describes_its_verbs_while_it_holds_the_keyboard() {
        let mut panel = TerminalPanel::default();
        assert!(
            panel.composer_hints(&hints(true)).is_some(),
            "a focused panel with no terminal must still name `i`"
        );
        panel.show(Some("t-1"));

        let mut insert = hints(true);
        insert.input_mode = InputMode::Insert;
        let typing = panel
            .composer_hints(&insert)
            .expect("focused panel advertises");
        assert!(
            typing.contains("INS typing to the shell"),
            "Insert must describe the shell, got {typing:?}"
        );

        let reading = panel
            .composer_hints(&hints(true))
            .expect("focused panel advertises");
        assert!(
            reading.contains("i type"),
            "Normal must describe the reader verbs, got {reading:?}"
        );
    }

    // ── Caret blink ───────────────────────────────────────────────────

    /// Cells carrying `REVERSED` in a rendered frame.
    ///
    /// The caret is the only thing here that paints it: the fixture's text is
    /// plain, so a reversed cell is the caret and nothing else.
    fn reversed_cells(buffer: &Buffer) -> usize {
        buffer
            .content
            .iter()
            .filter(|cell| cell.modifier.contains(Modifier::REVERSED))
            .count()
    }

    /// A live PTY behind `handle`, in Insert, with one line of output.
    fn live_focused_terminal(
        handle: &str,
    ) -> crate::tui::interactive_terminal::InteractiveTerminalController {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

        let mut terminal = crate::tui::interactive_terminal::InteractiveTerminalController::new();
        assert!(terminal.apply_created(
            handle.to_string(),
            "ws-1".to_string(),
            "sh".to_string(),
            40,
            4
        ));
        let body = crate::protocol::interactive_process::InteractiveOutputChunk {
            handle: handle.to_string(),
            from_seq: 0,
            next_seq: 5,
            gap: false,
            data_b64: B64.encode(b"alpha"),
        };
        assert!(terminal
            .apply_attached(handle, "a-1".to_string(), &body, None)
            .expect("attach")
            .is_none());
        assert!(
            terminal.focus(handle),
            "a live attached view takes the keyboard"
        );
        terminal
    }

    /// Render the panel into a fresh buffer at a pinned blink phase.
    ///
    /// Pinned, not sampled: the phase arrives as a grant on [`PanelCtx`], which
    /// is the only reason this can be asserted without racing a live clock — and
    /// the reason the panel cannot quietly read a clock of its own.
    fn rendered_with_phase(handle: &str, caret_blink_on: bool) -> Buffer {
        use crate::tui::components::sidebar::SidebarWidget;

        let theme = Arc::new(Theme::default());
        let mut session = SidebarWidget::new(Arc::clone(&theme));
        let mut chat = crate::tui::app::state::ChatState::default();
        let mut terminal = live_focused_terminal(handle);
        let mut panel = TerminalPanel::default();
        panel.show(Some(handle));
        let mut ctx = PanelCtx {
            theme,
            now_ms: 0,
            session: &mut session,
            chat: &mut chat,
            terminal: &mut terminal,
            vim_mode: false,
            chat_project: None,
            area: Rect::new(0, 0, 40, 10),
            focused: true,
            input_mode: InputMode::Insert,
            caret_blink_on,
        };
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 10));
        let area = ctx.area;
        panel.render(&mut ctx, &mut buffer, area);
        buffer
    }

    /// The reported defect at the surface the user is actually looking at:
    /// the docked caret was painted once and never changed, so a user could
    /// not tell from the column that the PTY was live.
    #[test]
    fn the_docked_caret_blinks_on_the_phase_the_host_grants() {
        assert_eq!(
            reversed_cells(&rendered_with_phase("t-blink", true)),
            1,
            "the visible phase paints the caret"
        );
        assert_eq!(
            reversed_cells(&rendered_with_phase("t-blink", false)),
            0,
            "the hidden phase paints it dark"
        );
    }
}
