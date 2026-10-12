//! The panel manager: pick what the side column shows, and bind a hotkey to it.
//!
//! One box replaces the old two-button grid. The grid was two unlabelled
//! buttons in a 30-column overlay; this is a filtered list that names each
//! panel, says what it is for, and shows which digit is already bound to it.
//!
//! ## Interaction
//!
//! * **type** — filter, same as `/help`.
//! * **`j`/`k`, `↑`/`↓`** — move the highlight.
//! * **`Enter`** — load the highlighted panel and take focus.
//! * **`1`-`9`, `0`** — bind the highlighted panel to that digit, persisting
//!   it to `codegg.json`.
//! * **`Esc`** — clear the filter, then close.
//!
//! The cursor stays on a real panel rather than a filtered position, so
//! typing narrows what is *visible* without renumbering what `Enter` and the
//! digits act on. A user who filtered to `chat` and pressed `Enter` expects
//! chat, not "the third of whatever matched".

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::state::panel_hotkeys::PanelHotkeys;
use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::panels::SidePanelId;
use crate::tui::theme::Theme;

/// Upper bound on the filter text, so a held key cannot grow this without
/// limit.
const MAX_QUERY_LEN: usize = 48;

pub struct PanelManagerDialog {
    theme: Arc<Theme>,
    query: String,
    /// Index into [`SidePanelId::ALL`], not into the filtered view.
    cursor: usize,
    /// The user's saved digit bindings, read-only here: saving happens in
    /// the command layer so persistence lives in one place.
    hotkeys: PanelHotkeys,
}

impl Clone for PanelManagerDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            query: self.query.clone(),
            cursor: self.cursor,
            hotkeys: self.hotkeys.clone(),
        }
    }
}

impl std::fmt::Debug for PanelManagerDialog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PanelManagerDialog(<filtered panel list>)")
    }
}

impl PanelManagerDialog {
    pub fn new(theme: Arc<Theme>, hotkeys: PanelHotkeys, active: SidePanelId) -> Self {
        Self {
            theme,
            query: String::new(),
            // Opens on the live panel, so `Enter` with no movement is a no-op
            // rather than a silent jump to whatever happens to be first.
            cursor: active.index(),
            hotkeys,
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The highlighted panel.
    pub fn selected(&self) -> Option<SidePanelId> {
        SidePanelId::ALL.get(self.cursor).copied()
    }

    /// True when `panel` passes the current filter.
    fn matches(&self, panel: SidePanelId) -> bool {
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        query.split_whitespace().all(|term| {
            panel.label().to_lowercase().contains(term)
                || panel.title().to_lowercase().contains(term)
                || panel.hint().to_lowercase().contains(term)
        })
    }

    /// Move the highlight by `delta`, clamped to the registry.
    fn move_cursor(&mut self, delta: isize) {
        let len = SidePanelId::ALL.len() as isize;
        if len == 0 {
            return;
        }
        let next = (self.cursor as isize + delta).clamp(0, len - 1);
        self.cursor = next as usize;
    }

    fn push_query(&mut self, ch: char) {
        if self.query.chars().count() < MAX_QUERY_LEN {
            self.query.push(ch);
        }
    }
}

impl Component for PanelManagerDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        use crossterm::event::{KeyCode, KeyModifiers};
        match key.code {
            KeyCode::Esc => {
                if self.query.is_empty() {
                    return Some(TuiMsg::CloseDialog);
                }
                self.query.clear();
            }
            KeyCode::Enter => {
                let panel = self.selected()?;
                return Some(TuiMsg::SelectPanel { panel });
            }
            KeyCode::Up => self.move_cursor(-1),
            KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Backspace => {
                self.query.pop();
            }
            // A digit binds the *highlighted* panel. Guarded on no modifier
            // so a Ctrl+digit (which recalls a saved panel) is not swallowed
            // here and recorded as a fresh binding by accident.
            KeyCode::Char(d) if key.modifiers == KeyModifiers::NONE && d.is_ascii_digit() => {
                let panel = self.selected()?;
                return Some(TuiMsg::SavePanelHotkey { digit: d, panel });
            }
            KeyCode::Char(ch)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.push_query(ch);
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        // Purely local state: every change is the user's own key, so there is
        // nothing for a pushed message to carry.
        None
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let buf = frame.buffer_mut();
        let chunks = Layout::vertical([
            Constraint::Length(3), // title
            Constraint::Length(3), // filter
            Constraint::Min(1),    // list
            Constraint::Length(3), // footer
        ])
        .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Panels ")
            .title_style(
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background));
        Widget::render(block, chunks[0], buf);

        let filtering = !self.query.trim().is_empty();
        let filter_spans = if filtering {
            vec![
                Span::styled("search ", Style::default().fg(theme.muted)),
                Span::styled(
                    format!("{}_", self.query),
                    Style::default().fg(theme.primary),
                ),
            ]
        } else {
            vec![Span::styled(
                "type to filter panels…",
                Style::default().fg(theme.muted),
            )]
        };
        Widget::render(
            Paragraph::new(Line::from(filter_spans)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            ),
            chunks[1],
            buf,
        );

        // The list. A filtered-out row stays visible but dimmed rather than
        // disappearing: the user needs to see *why* the entry they expected
        // is not there.
        let mut lines: Vec<Line<'static>> = Vec::new();
        for (index, panel) in SidePanelId::ALL.iter().copied().enumerate() {
            let selected = index == self.cursor;
            let visible = self.matches(panel);
            let saved = self.hotkeys.digit_for(panel);
            let badge = match saved {
                Some(digit) => format!("  ^-{digit}"),
                None => String::new(),
            };
            let marker = if selected { "▸" } else { " " };
            let style = if !visible {
                Style::default().fg(theme.border)
            } else if selected {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(format!("{:<10}", panel.label()), style),
                Span::styled(
                    format!("{}  ", panel.hint()),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(badge, Style::default().fg(theme.warning)),
            ]));
        }
        Widget::render(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            ),
            chunks[2],
            buf,
        );

        let footer = if filtering {
            "Enter load  |  0-9 bind hotkey  |  Esc clear filter  |  Esc close"
        } else {
            "Enter load  |  0-9 bind hotkey  |  type to filter  |  Esc close"
        };
        Widget::render(
            Paragraph::new(Line::from(Span::styled(
                footer,
                Style::default().fg(theme.secondary),
            )))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            ),
            chunks[3],
            buf,
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::PanelManager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn dialog() -> PanelManagerDialog {
        PanelManagerDialog::new(
            Arc::new(Theme::default()),
            PanelHotkeys::default(),
            SidePanelId::Session,
        )
    }

    #[test]
    fn opens_on_the_live_panel_so_enter_is_a_no_op() {
        let d = PanelManagerDialog::new(
            Arc::new(Theme::default()),
            PanelHotkeys::default(),
            SidePanelId::Terminal,
        );
        assert_eq!(d.selected(), Some(SidePanelId::Terminal));
    }

    #[test]
    fn enter_selects_the_highlighted_panel() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            d.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(TuiMsg::SelectPanel {
                panel: SidePanelId::Chat
            })
        );
    }

    #[test]
    fn the_highlight_is_clamped_rather_than_wrapping() {
        let mut d = dialog(); // opens at index 0
        for _ in 0..10 {
            d.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
        }
        assert_eq!(d.cursor(), 0, "must not wrap past the first panel");
    }

    #[test]
    fn typing_narrows_what_is_visible_without_moving_the_highlight() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Down, KeyModifiers::NONE)); // highlight Chat
        for ch in "term".chars() {
            d.handle_key(key(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        // Terminal does not match "term"? It does. Chat is filtered out but
        // the highlight is still where it was.
        assert!(!d.matches(SidePanelId::Chat));
        assert!(d.matches(SidePanelId::Terminal));
        assert_eq!(d.cursor(), 1, "filtering must not renumber the cursor");
    }

    #[test]
    fn a_digit_binds_the_highlighted_panel() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            d.handle_key(key(KeyCode::Char('1'), KeyModifiers::NONE)),
            Some(TuiMsg::SavePanelHotkey {
                digit: '1',
                panel: SidePanelId::Chat
            })
        );
    }

    #[test]
    fn escape_clears_the_filter_before_it_closes() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(
            d.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            None,
            "one Esc must not discard a query mid-typing"
        );
        assert_eq!(
            d.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(TuiMsg::CloseDialog)
        );
    }
}
