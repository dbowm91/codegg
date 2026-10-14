//! The session side panel.
//!
//! A thin adapter, not a rewrite: `SidebarWidget` already owns a bounded
//! projection, logical-row focus identities, collapse state, and its own
//! render. The panel exists so that widget can be swapped for another one
//! without moving its key handling, so its binding-resolved behavior is
//! exactly the `handle_sidebar_action` mapping it had before the host
//! existed.
//!
//! The only additions are the title bar, which makes the active panel legible
//! now that more than one panel can occupy this column, and the focus
//! highlight.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{buffer::Buffer, layout::Rect};

use crate::tui::app::TuiMsg;
use crate::tui::components::sidebar::SidebarActivation;
use crate::tui::input::InputAction;

use super::{PanelCtx, PanelHints, PanelKey, SidePanel, SidePanelId};

/// Session projection panel. Stateless — all view state lives in
/// `SidebarWidget`.
pub struct SessionPanel;

impl SidePanel for SessionPanel {
    fn id(&self) -> SidePanelId {
        SidePanelId::Session
    }

    /// `SidebarWidget::render_with_title` paints its own border, so the
    /// panel name goes into the top border row rather than a separate band
    /// that would steal a content row on a short terminal.
    fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
        let label = SidePanelId::Session.label();
        let title = if ctx.focused {
            format!(" {label} ● ")
        } else {
            format!(" {label} ")
        };
        ctx.session.set_theme(&ctx.theme);
        ctx.session
            .render_with_title(area, buf, &title, ctx.focused);
    }

    /// The session panel has no input mode of its own, so `Esc` and `q`
    /// always return to the composer.
    fn owns_exit_key(&self, _key: KeyEvent, _ctx: &PanelCtx<'_>) -> bool {
        false
    }

    fn composer_hints(&self, hints: &PanelHints<'_>) -> Option<String> {
        hints.focused.then(|| {
            "SESSIONS · jk move · ↵ open · space fold · hl collapse/expand · ⇧S panel".to_string()
        })
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        action: Option<InputAction>,
        ctx: &mut PanelCtx<'_>,
    ) -> PanelKey {
        match action {
            Some(InputAction::NavigateUp) => ctx.session.focus_prev(),
            Some(InputAction::NavigateDown) => ctx.session.focus_next(),
            Some(InputAction::PageUp) => ctx.session.focus_page_prev(ctx.area),
            Some(InputAction::PageDown) => ctx.session.focus_page_next(ctx.area),
            Some(InputAction::Left) => ctx.session.collapse_or_parent(),
            Some(InputAction::Right) => ctx.session.expand_focused(),
            // Normal-mode Space is the sidebar toggle row and Enter is
            // `Send`; both act on the focused row, which is why they are
            // matched separately rather than through one action.
            Some(InputAction::ToggleSection) | Some(InputAction::FocusSidebar)
                if key.code == KeyCode::Char(' ') =>
            {
                ctx.session.toggle_focused();
            }
            Some(InputAction::ToggleSection) => ctx.session.toggle_focused(),
            Some(InputAction::Send) => {
                return match ctx.session.activate_focused() {
                    SidebarActivation::ToggleSection | SidebarActivation::ToggleAgentNode(_) => {
                        ctx.session.toggle_focused();
                        PanelKey::Consumed
                    }
                    SidebarActivation::InspectRun(run_id) => {
                        PanelKey::Intent(TuiMsg::OpenRunDetail { run_id })
                    }
                    SidebarActivation::None => PanelKey::Consumed,
                };
            }
            // Unbound and text actions are consumed rather than leaked to
            // the prompt, so a focused sidebar can never half-type a turn.
            _ => {}
        }
        ctx.session.ensure_focused_visible(ctx.area);
        PanelKey::Consumed
    }
}
