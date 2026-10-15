//! The agents side panel: the agent management surface.
//!
//! Replaces the read-only `/agents` info dump with a selectable list and
//! per-agent actions. The panel owns **presentation only** — cursor,
//! viewport, and which action is armed. It resolves no project and performs
//! no agent mutation itself; every action becomes a `TuiMsg` routed into an
//! existing command seam, which is the contract every panel follows.
//!
//! ## Actions differ by provenance, and the panel says so
//!
//! A **project** agent (`.codegg/agents/`) and a **global** agent
//! (`~/.config/codegg/agents/`) are both user-authored, so both can be
//! edited, deactivated, or deleted — but they are *not* the same thing, and
//! the badge says which one each row is. A global agent applies to every
//! project on the machine; a project agent applies to this one. A
//! **default** agent (`build`, `explore`, …) is compiled into the binary, so
//! it can be overridden or deactivated but *not* deleted — deleting would
//! only remove the override and leave the agent present, which is worse than
//! offering no delete at all. Restoring drops the override, which is exact
//! precisely because the default has no saved copy to go stale.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::tui::app::TuiMsg;
use crate::tui::commands::agent_editor::AgentOrigin;
pub use crate::tui::commands::agent_editor::AgentRow;
use crate::tui::input::InputAction;

use super::{PanelCtx, PanelHints, PanelKey, SidePanel, SidePanelId};

/// Maximum rows the list holds before the viewport scrolls.
const MAX_AGENT_ROWS: usize = 512;

/// What the user asked the panel to do with the selected agent.
///
/// Deliberately a *selection*, not an instruction: the command layer decides
/// which editor path, override, or refresh each one implies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSelection {
    /// Open the agent's config in the editor.
    Edit,
    /// Flip suppression for the agent.
    ToggleDisabled,
    /// Remove a custom agent's config.
    Delete,
    /// Drop a default agent's override.
    Restore,
    /// Create a new agent from the template.
    New,
}

/// Agent management panel. Holds the rows the command layer loaded and the
/// user's cursor within them.
///
/// `armed` is the one action state the panel owns: a delete is the only verb
/// here that removes work, so it is armed rather than immediate — `d` then
/// `d` deletes, and anything else cancels.
#[derive(Default)]
pub struct AgentsPanel {
    rows: Vec<AgentRow>,
    cursor: usize,
    scroll: usize,
    armed: Option<AgentSelection>,
}

impl AgentsPanel {
    /// Replace the visible rows, preserving the cursor by name.
    ///
    /// Preserving by name rather than index is what makes a save-then-refresh
    /// feel stable: `:wq` on an agent reloads the registry, and the row the
    /// user was looking at must still be the row under the cursor even though
    /// the list behind it changed.
    pub fn set_rows(&mut self, rows: Vec<AgentRow>) {
        let previous = self.selected().map(|row| row.name.clone());
        self.rows = rows;
        self.rows.truncate(MAX_AGENT_ROWS);
        self.cursor = previous
            .and_then(|name| self.rows.iter().position(|row| row.name == name))
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1));
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
        self.clamp_scroll(1);
    }

    /// Replace the rows while keeping the armed action honest.
    ///
    /// An armed delete must not survive a reload: if the list changed, the
    /// row the user armed against may no longer be the row under the cursor,
    /// and `d` then `d` would delete something they never selected.
    pub(crate) fn reload_from(&mut self, rows: Vec<AgentRow>) {
        self.armed = None;
        self.set_rows(rows);
    }

    /// The row under the cursor, if any.
    pub fn selected(&self) -> Option<&AgentRow> {
        self.rows.get(self.cursor)
    }

    /// Keep `scroll` such that the cursor is visible in a `height`-row
    /// viewport. `height` is at least one so a tiny terminal cannot divide
    /// the offset by zero or produce an inverted range.
    fn clamp_scroll(&mut self, height: usize) {
        let height = height.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
        if self.cursor >= self.scroll + height {
            self.scroll = self.cursor + 1 - height;
        }
        let max_scroll = self.rows.len().saturating_sub(height);
        self.scroll = self.scroll.min(max_scroll);
    }

    fn move_cursor(&mut self, delta: isize, height: usize) {
        if self.rows.is_empty() {
            return;
        }
        self.cursor = if delta.is_negative() {
            self.cursor.saturating_sub(delta.unsigned_abs())
        } else {
            self.cursor
                .saturating_add(delta.unsigned_abs())
                .min(self.rows.len() - 1)
        };
        self.clamp_scroll(height);
    }

    /// Whether the selected row may be deleted.
    ///
    /// Enforced here *and* in the command layer: the panel hides an
    /// unavailable verb, and the command layer refuses one that arrives by
    /// another route.
    fn can(&self, selection: AgentSelection) -> bool {
        let Some(row) = self.selected() else {
            return false;
        };
        match selection {
            AgentSelection::Edit => row.path.is_some(),
            AgentSelection::Delete => row.origin.can_delete(),
            AgentSelection::Restore => crate::tui::commands::agent_editor::can_restore(row),
            AgentSelection::ToggleDisabled => true,
            AgentSelection::New => true,
        }
    }

    /// Turn a chosen verb into a command-layer intent.
    ///
    /// `New` is answered even with no selected row, because
    /// `panel_is_empty` deliberately keeps this panel focusable on the
    /// promise that `n` always works — with zero rows loaded, refusing here
    /// would make that promise false and strand the user on the one key
    /// that could fill the list. The empty name reaches the command layer,
    /// which answers with the `:new <name>` instruction rather than
    /// guessing a name.
    fn emit(&mut self, selection: AgentSelection) -> PanelKey {
        let name = match selection {
            AgentSelection::New => self
                .selected()
                .map(|row| row.name.clone())
                .unwrap_or_default(),
            _ => match self.selected() {
                Some(row) => row.name.clone(),
                None => return PanelKey::Consumed,
            },
        };
        self.armed = None;
        PanelKey::Intent(TuiMsg::AgentPanelAction { selection, name })
    }
}

impl SidePanel for AgentsPanel {
    fn id(&self) -> SidePanelId {
        SidePanelId::Agents
    }

    fn render(&mut self, ctx: &mut PanelCtx<'_>, buf: &mut Buffer, area: Rect) {
        let theme = Arc::clone(&ctx.theme);
        let title = if ctx.focused {
            " AGENTS ● "
        } else {
            " AGENTS "
        };
        let block = Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(area);
        buf.set_string(area.x, area.y, "", Style::default());
        block.render(area, buf);

        if self.rows.is_empty() {
            Paragraph::new(Line::from(Span::styled(
                "No agents loaded",
                Style::default().fg(theme.muted),
            )))
            .render(inner, buf);
            return;
        }

        let height = inner.height as usize;
        let mut lines: Vec<Line> = Vec::with_capacity(height);
        for (index, row) in self.rows.iter().enumerate().skip(self.scroll).take(height) {
            let selected = index == self.cursor;
            let base = if selected {
                Style::default().bg(theme.selection).fg(theme.foreground)
            } else {
                Style::default().fg(theme.foreground)
            };
            let marker = if selected { "▸" } else { " " };
            let badge = row.origin.badge();
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), base),
                Span::styled(
                    format!("{:<18}", truncate(&row.name, 18)),
                    base.add_modifier(Modifier::BOLD),
                ),
                // A global agent is highlighted distinctly: it is the one
                // row class whose `:wq` reaches outside this project.
                Span::styled(
                    format!("{:<8}", truncate(badge, 8)),
                    Style::default().fg(match row.origin {
                        AgentOrigin::Default => theme.muted,
                        AgentOrigin::Global => theme.warning,
                        AgentOrigin::Project => theme.secondary,
                    }),
                ),
                Span::styled(truncate(&row.description, 40), base),
            ]));
        }
        Paragraph::new(lines).render(inner, buf);
    }

    fn composer_hints(&self, hints: &PanelHints<'_>) -> Option<String> {
        hints.focused.then(|| {
            let Some(row) = self.selected() else {
                return "AGENTS · n new · ⇧H/⇧L panel · /agents help".to_string();
            };
            let mut verbs = vec!["jk move".to_string()];
            if self.can(AgentSelection::Edit) {
                verbs.push(format!(
                    "e {}",
                    crate::tui::commands::agent_editor::primary_action_label(row)
                ));
            }
            verbs.push("t deactivate".to_string());
            if row.origin.can_delete() {
                verbs.push("d delete".to_string());
            }
            if self.can(AgentSelection::Restore) {
                verbs.push("r restore".to_string());
            }
            // Spell out how to reach each scope rather than making the user
            // guess: `n` follows the row's own scope (and is project-scoped
            // for a row with no file yet), while global creation is explicit.
            verbs.push("n new".to_string());
            verbs.push(":new global <name>".to_string());
            format!("AGENTS · {}", verbs.join(" · "))
        })
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        action: Option<InputAction>,
        ctx: &mut PanelCtx<'_>,
    ) -> PanelKey {
        let height = ctx.area.height.saturating_sub(2).max(1) as usize;
        match action {
            Some(InputAction::NavigateDown) => {
                self.move_cursor(1, height);
                self.armed = None;
            }
            Some(InputAction::NavigateUp) => {
                self.move_cursor(-1, height);
                self.armed = None;
            }
            Some(InputAction::PageDown) => {
                self.move_cursor(10, height);
                self.armed = None;
            }
            Some(InputAction::PageUp) => {
                self.move_cursor(-10, height);
                self.armed = None;
            }
            _ => {
                // An armed action is answered before anything else, so `d`
                // then `d` deletes and `d` then `j` merely cancels.
                if self.armed == Some(AgentSelection::Delete) {
                    self.armed = None;
                    return match key.code {
                        KeyCode::Char('d') => self.emit(AgentSelection::Delete),
                        _ => PanelKey::Consumed,
                    };
                }
                match key.code {
                    // Selecting a row is the gesture everyone reaches for
                    // first, and it did nothing: `Enter` fell through to the
                    // catch-all and returned `Consumed` without emitting a
                    // message, so the panel looked inert. It now means the
                    // same thing as `e` — open this agent's config.
                    KeyCode::Enter => return self.emit(AgentSelection::Edit),
                    KeyCode::Char('e') => return self.emit(AgentSelection::Edit),
                    KeyCode::Char('t') => return self.emit(AgentSelection::ToggleDisabled),
                    KeyCode::Char('r') => return self.emit(AgentSelection::Restore),
                    KeyCode::Char('n') => return self.emit(AgentSelection::New),
                    KeyCode::Char('d') if self.can(AgentSelection::Delete) => {
                        self.armed = Some(AgentSelection::Delete);
                    }
                    _ => {}
                }
            }
        }
        PanelKey::Consumed
    }
}

/// Truncate to a display width, with an ellipsis when it does not fit.
fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::Theme;
    use codegg_protocol::document::DocumentScope;
    use crossterm::event::KeyModifiers;

    fn row(origin: AgentOrigin, path: Option<&str>) -> AgentRow {
        row_named("build", origin, path)
    }

    fn row_named(name: &str, origin: AgentOrigin, path: Option<&str>) -> AgentRow {
        let scope = match origin {
            AgentOrigin::Global => DocumentScope::GlobalAgent,
            _ => DocumentScope::Workspace,
        };
        AgentRow {
            name: name.into(),
            description: "d".into(),
            mode: "Primary".into(),
            origin,
            path: path.map(str::to_string),
            scope,
            disabled: false,
        }
    }

    fn panel_with(rows: Vec<AgentRow>) -> AgentsPanel {
        let mut panel = AgentsPanel::default();
        panel.set_rows(rows);
        panel
    }

    /// A real `PanelCtx` so the key tests drive the actual `handle_key`
    /// rather than a reimplementation of it.
    fn with_ctx<T>(body: impl FnOnce(&mut PanelCtx<'_>) -> T) -> T {
        let theme = Arc::new(Theme::default());
        let mut session = crate::tui::components::sidebar::SidebarWidget::new(Arc::clone(&theme));
        let mut chat = crate::tui::app::state::ChatState::default();
        let mut terminal = crate::tui::interactive_terminal::InteractiveTerminalController::new();
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
            input_mode: crate::tui::input::InputMode::Normal,
        };
        body(&mut ctx)
    }

    fn action_for(panel: &mut AgentsPanel, key: char) -> PanelKey {
        with_ctx(|ctx| {
            panel.handle_key(
                KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE),
                None,
                ctx,
            )
        })
    }

    #[test]
    fn a_global_agent_offers_the_same_verbs_as_a_project_agent() {
        // Both are user-authored, so both are deletable. What differs is
        // which root the action reaches, not whether it is offered.
        for origin in [AgentOrigin::Global, AgentOrigin::Project] {
            let path = match origin {
                AgentOrigin::Global => Some("build.toml"),
                _ => Some(".codegg/agents/build.toml"),
            };
            let panel = panel_with(vec![row(origin, path)]);
            assert!(panel.can(AgentSelection::Delete), "{origin:?}");
            assert!(panel.can(AgentSelection::Edit), "{origin:?}");
        }
    }

    #[test]
    fn a_default_offers_no_delete_and_only_restore_when_overridden() {
        let panel = panel_with(vec![row(AgentOrigin::Default, None)]);
        assert!(!panel.can(AgentSelection::Delete));
        assert!(!panel.can(AgentSelection::Restore));
        assert!(!panel.can(AgentSelection::Edit));

        let panel = panel_with(vec![row(
            AgentOrigin::Default,
            Some(".codegg/agents/build.toml"),
        )]);
        assert!(panel.can(AgentSelection::Restore));
        assert!(!panel.can(AgentSelection::Delete));
    }

    #[test]
    fn each_origin_badge_says_which_root_it_is() {
        // The badge is the only thing distinguishing a global `build.toml`
        // from a project one in the list, so it must be exactly that.
        assert_eq!(AgentOrigin::Default.badge(), "default");
        assert_eq!(AgentOrigin::Project.badge(), "project");
        assert_eq!(AgentOrigin::Global.badge(), "global");
        assert_eq!(AgentOrigin::Global.scope(), DocumentScope::GlobalAgent);
        assert_eq!(AgentOrigin::Project.scope(), DocumentScope::Workspace);
    }

    #[test]
    fn delete_is_armed_so_one_d_never_removes_anything() {
        let mut panel = panel_with(vec![row(AgentOrigin::Global, Some("build.toml"))]);
        // First `d` arms only.
        assert!(matches!(action_for(&mut panel, 'd'), PanelKey::Consumed));
        // Any other key cancels rather than falling through to an action.
        assert!(matches!(action_for(&mut panel, 'j'), PanelKey::Consumed));
        // The second `d` must be re-armed first.
        assert!(matches!(action_for(&mut panel, 'd'), PanelKey::Consumed));
        match action_for(&mut panel, 'd') {
            PanelKey::Intent(TuiMsg::AgentPanelAction { selection, name }) => {
                assert_eq!(selection, AgentSelection::Delete);
                assert_eq!(name, "build");
            }
            other => panic!("expected an armed delete, got {other:?}"),
        }
    }

    #[test]
    fn a_reload_never_preserves_an_armed_delete() {
        // The list changing between arming and confirming must not let the
        // confirm land on the row the user had *already* looked away from.
        // Proved by the confirm needing a fresh `d`: if the arm had survived
        // the reload, the first press here would have deleted "build".
        let mut panel = panel_with(vec![row(AgentOrigin::Global, Some("build.toml"))]);
        assert!(matches!(action_for(&mut panel, 'd'), PanelKey::Consumed));
        panel.reload_from(vec![row_named(
            "other",
            AgentOrigin::Global,
            Some("other.toml"),
        )]);

        assert!(
            matches!(action_for(&mut panel, 'd'), PanelKey::Consumed),
            "the first d after a reload must only re-arm"
        );
        match action_for(&mut panel, 'd') {
            PanelKey::Intent(TuiMsg::AgentPanelAction { selection, name }) => {
                assert_eq!(selection, AgentSelection::Delete);
                assert_eq!(
                    name, "other",
                    "the confirm must act on the reloaded row, never the pre-reload one"
                );
            }
            other => panic!("expected an armed delete, got {other:?}"),
        }
    }

    #[test]
    fn new_works_even_with_no_row_loaded() {
        // `panel_is_empty` keeps this panel focusable on the promise that
        // `n` always works. With zero rows the intent carries an empty name
        // and the command layer answers with the `:new <name>` instruction.
        let mut panel = panel_with(Vec::new());
        match action_for(&mut panel, 'n') {
            PanelKey::Intent(TuiMsg::AgentPanelAction { selection, name }) => {
                assert_eq!(selection, AgentSelection::New);
                assert!(name.is_empty(), "no row means no name to carry");
            }
            other => panic!("expected a New intent, got {other:?}"),
        }
    }
}
