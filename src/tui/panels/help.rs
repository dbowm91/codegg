//! Keybinding reference as a swappable side panel.
//!
//! ## Relationship to `/help`
//!
//! Not a peer implementation — a *host* for the very same [`HelpDialog`].
//! The panel holds a dialog and paints it with
//! [`HelpDialog::render_into`], so the filter, the NOR/INS badge, the
//! scrollbar, the row format, and the footer all come from one place. That
//! is the difference between "docking the box moves it" and "docking the box
//! gives you a similar panel": with a shared widget there is nothing to keep
//! in sync, so `/help` and the panel cannot drift apart the way the previous
//! chat pair did.
//!
//! The title is the only thing the panel supplies, because "is this region
//! focused" is a host fact the dialog cannot know about itself.

use crossterm::event::KeyEvent;
use ratatui::{buffer::Buffer, layout::Rect};

use crate::tui::components::component::Component;
use crate::tui::components::dialogs::help::HelpDialog;
use crate::tui::input::{InputAction, InputMode};
use crate::tui::theme::Theme;

use super::{PanelCtx, PanelHints, PanelKey, SidePanel, SidePanelId};

/// The docked form of `/help`.
#[derive(Debug)]
pub struct HelpPanel {
    dialog: HelpDialog,
    /// The vim setting the current rows were built for.
    ///
    /// Re-derived on every activate and render so a config reload changes
    /// what the panel lists, instead of leaving it describing a mode that is
    /// no longer on. A mismatch rebuilds the row set; the query is dropped
    /// with it, which is the honest trade — a stale filter over a rebuilt
    /// list would silently describe a different set than the rows shown.
    vim_mode: bool,
}

impl Default for HelpPanel {
    fn default() -> Self {
        Self {
            dialog: HelpDialog::new(std::sync::Arc::new(Theme::default()), Vec::new()),
            vim_mode: false,
        }
    }
}

impl HelpPanel {
    /// The filter and mode, exposed for the parity tests that cannot read
    /// what the render drew.
    pub fn dialog(&self) -> &HelpDialog {
        &self.dialog
    }
}

impl SidePanel for HelpPanel {
    fn id(&self) -> SidePanelId {
        SidePanelId::Help
    }

    fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
        // Built on first paint and rebuilt only when vim mode changes, so
        // the common path does no allocation at all.
        if ctx.vim_mode != self.vim_mode || self.dialog.rows_len() == 0 {
            self.vim_mode = ctx.vim_mode;
            self.dialog = HelpDialog::new_with_mode(
                std::sync::Arc::clone(&ctx.theme),
                self.vim_mode,
                InputMode::Insert,
            );
        }
        let title = if ctx.focused { " Help ● " } else { " Help " };
        self.dialog.render_into(buf, area, &ctx.theme, title);
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        _action: Option<InputAction>,
        _ctx: &mut PanelCtx<'_>,
    ) -> PanelKey {
        // The widget owns its keymap, including the directional that docks
        // it — which is why the widget tracks mode at all. Everything it does
        // not claim is still consumed: a stray `j` must never fall through to
        // the composer and start an agent turn.
        match self.dialog.handle_key(key) {
            Some(msg) => PanelKey::Intent(msg),
            None => PanelKey::Consumed,
        }
    }

    fn composer_hints(&self, hints: &PanelHints<'_>) -> Option<String> {
        if !hints.focused {
            return None;
        }
        let mode = if self.dialog.input_mode() == InputMode::Normal {
            "NOR"
        } else {
            "INS"
        };
        Some(format!(
            "HELP · {mode} · type to filter · Esc normal · Shift+← back"
        ))
    }
}
