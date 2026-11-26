//! M009: the Workspace picker side panel.
//!
//! A bespoke surface in the column: the project catalog with its own
//! NOR/INS filter, and `Enter` opens or focuses the selected project's tab.
//! It reads the catalog cache the picker and the whole view already share —
//! no second catalog, no per-row request, no modal.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::TuiMsg;
use crate::components::component::{Component, DialogType};
use crate::components::hint_line::HintLine;

/// The project catalog as a side-panel list.
pub struct WorkspacePickerPanel {
    rows: Vec<(String, String)>,
    selected: usize,
    filter: String,
    insert: bool,
}

impl WorkspacePickerPanel {
    pub fn new(rows: Vec<(String, String)>) -> Self {
        Self {
            rows,
            selected: 0,
            filter: String::new(),
            insert: false,
        }
    }

    fn visible(&self) -> Vec<usize> {
        let query = self.filter.trim();
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, (id, name))| {
                query.is_empty()
                    || codegg::util::fuzzy::fuzzy_score(query, name) > 0
                    || codegg::util::fuzzy::fuzzy_score(query, id) > 0
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn hint(&self) -> HintLine {
        let mut hint = HintLine::new();
        if self.insert {
            hint.push("type", "filter")
                .push("Esc", "NOR")
                .push("^U", "clear");
        } else {
            hint.push("j/k", "move")
                .push("Enter", "open")
                .push("i", "filter");
        }
        hint
    }
}

impl Component for WorkspacePickerPanel {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Char('i') if key.modifiers.is_empty() && !self.insert => {
                self.insert = true;
            }
            KeyCode::Esc if self.insert => {
                if self.filter.is_empty() {
                    self.insert = false;
                } else {
                    self.filter.clear();
                    self.selected = 0;
                }
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                self.filter.clear();
                self.selected = 0;
            }
            KeyCode::Backspace if self.insert => {
                self.filter.pop();
                self.selected = 0;
            }
            KeyCode::Char(ch)
                if self.insert
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
            {
                if self.filter.chars().count() < 128 {
                    self.filter.push(ch);
                    self.selected = 0;
                }
            }
            KeyCode::Down | KeyCode::Char('j') if !self.insert && key.modifiers.is_empty() => {
                let len = self.visible().len();
                self.selected = (self.selected + 1).min(len.saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') if !self.insert && key.modifiers.is_empty() => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                let visible = self.visible();
                if let Some(index) = visible.get(self.selected) {
                    if let Some((project_id, _)) = self.rows.get(*index) {
                        return Some(TuiMsg::WorkspacePickerOpen {
                            project_id: project_id.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(
        &mut self,
        frame: &mut ratatui::Frame,
        area: Rect,
        theme: &std::sync::Arc<crate::theme::Theme>,
    ) {
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![
            Span::styled(
                if self.insert { " INS " } else { " NOR " },
                Style::default().fg(if self.insert {
                    theme.primary
                } else {
                    theme.muted
                }),
            ),
            Span::styled(
                if self.filter.is_empty() {
                    "type to filter".to_string()
                } else {
                    format!("/{}", self.filter)
                },
                Style::default().fg(theme.foreground),
            ),
        ]));
        if self.rows.is_empty() {
            lines.push(Line::from(Span::styled(
                "no projects in the catalog yet",
                Style::default().fg(theme.muted),
            )));
        }
        let visible = self.visible();
        for (position, index) in visible.iter().enumerate() {
            let Some((_, name)) = self.rows.get(*index) else {
                continue;
            };
            let selected = position == self.selected && !self.insert;
            lines.push(Line::from(Span::styled(
                format!("{}{name}", if selected { ">" } else { " " }),
                if selected {
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground)
                },
            )));
        }
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme.background)),
            area,
        );
    }

    fn keymap_hint_line(&self) -> Option<HintLine> {
        Some(self.hint())
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::None
    }

    fn docks_as_viewer(&self) -> bool {
        false
    }
}
