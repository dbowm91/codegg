use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::prelude::Widget;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewMode {
    FileList,
    DiffView,
}

#[derive(Debug, Clone)]
pub struct ReviewItem {
    pub path: String,
    pub kind: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Clone)]
pub struct ReviewDialog {
    pub items: Vec<ReviewItem>,
    pub selected: usize,
    pub scroll: usize,
    pub mode: ReviewMode,
    pub diff_content: Option<(String, String)>,
    pub diff_scroll: usize,
    pub theme: Arc<Theme>,
}

impl ReviewDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            scroll: 0,
            mode: ReviewMode::FileList,
            diff_content: None,
            diff_scroll: 0,
            theme,
        }
    }

    pub fn set_items(&mut self, items: Vec<ReviewItem>) {
        self.items = items;
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_diff(&mut self, old: String, new: String) {
        self.diff_content = Some((old, new));
        self.mode = ReviewMode::DiffView;
        self.diff_scroll = 0;
    }

    pub fn selected_item(&self) -> Option<&ReviewItem> {
        self.items.get(self.selected)
    }

    fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll {
                self.scroll = self.selected;
            }
        }
    }

    fn select_down(&mut self) {
        if self.selected + 1 < self.items.len() {
            self.selected += 1;
            self.scroll = self.scroll.saturating_add(1);
        }
    }
}

impl Component for ReviewDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match self.mode {
            ReviewMode::FileList => match key.code {
                crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                    if key.modifiers.is_empty() =>
                {
                    self.select_up();
                    None
                }
                crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                    if key.modifiers.is_empty() =>
                {
                    self.select_down();
                    None
                }
                crossterm::event::KeyCode::Enter => {
                    if let Some(item) = self.items.get(self.selected) {
                        let path = item.path.clone();
                        Some(TuiMsg::ReviewOpenDiff { path })
                    } else {
                        None
                    }
                }
                crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
                _ => None,
            },
            ReviewMode::DiffView => match key.code {
                crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                    if key.modifiers.is_empty() =>
                {
                    self.diff_scroll = self.diff_scroll.saturating_add(1);
                    None
                }
                crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                    if key.modifiers.is_empty() =>
                {
                    self.diff_scroll = self.diff_scroll.saturating_add(1);
                    None
                }
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Backspace => {
                    self.mode = ReviewMode::FileList;
                    self.diff_content = None;
                    self.diff_scroll = 0;
                    None
                }
                _ => None,
            },
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _theme: &Arc<Theme>) {
        if area.height < 5 {
            return;
        }

        match self.mode {
            ReviewMode::FileList => self.render_file_list(frame, area),
            ReviewMode::DiffView => self.render_diff_view(frame, area),
        }
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Review
    }
}

impl ReviewDialog {
    fn render_file_list(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(format!(" Changed Files ({}) ", self.items.len()));

        let inner = block.inner(area);
        block.render(area, frame.buffer_mut());

        if self.items.is_empty() {
            let empty = Line::from(Span::styled(
                "No changed files",
                Style::default().fg(self.theme.muted),
            ));
            let empty_area = Rect::new(inner.x, inner.y, inner.width, 1);
            frame.render_widget(empty, empty_area);
            return;
        }

        let items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .skip(self.scroll)
            .map(|(i, item)| {
                let is_selected = i == self.selected;
                let style = if is_selected {
                    Style::default()
                        .bg(self.theme.selection)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.foreground)
                };

                let kind_char = match item.kind.as_str() {
                    "Created" | "Added" => "A",
                    "Deleted" => "D",
                    "Renamed" => "R",
                    _ => "M",
                };

                let kind_style = match item.kind.as_str() {
                    "Created" | "Added" => Style::default().fg(self.theme.success),
                    "Deleted" => Style::default().fg(self.theme.error),
                    "Renamed" => Style::default().fg(self.theme.warning),
                    _ => Style::default().fg(self.theme.foreground),
                };

                let stats = if item.additions > 0 || item.deletions > 0 {
                    format!(" +{}/-{}", item.additions, item.deletions)
                } else {
                    String::new()
                };

                let line = Line::from(vec![
                    Span::styled(format!("  {} ", kind_char), kind_style),
                    Span::styled(&item.path, style),
                    Span::styled(
                        stats,
                        Style::default().fg(self.theme.muted),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }

    fn render_diff_view(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(" Diff ");

        let inner = block.inner(area);
        block.render(area, frame.buffer_mut());

        let Some((old, new)) = &self.diff_content else {
            let msg = Line::from(Span::styled(
                "No diff content",
                Style::default().fg(self.theme.muted),
            ));
            let msg_area = Rect::new(inner.x, inner.y, inner.width, 1);
            frame.render_widget(msg, msg_area);
            return;
        };

        let diff = similar::TextDiff::from_lines(old, new);
        let mut y = 0u16;
        let visible_lines = inner.height as usize;

        for change in diff.iter_all_changes() {
            if y as usize >= visible_lines {
                break;
            }

            let (prefix, color) = match change.tag() {
                similar::ChangeTag::Delete => ("-", self.theme.error),
                similar::ChangeTag::Insert => ("+", self.theme.success),
                similar::ChangeTag::Equal => (" ", self.theme.foreground),
            };

            let text = change.value();
            let line_text = format!("{}{}", prefix, text);
            let display_line = if line_text.ends_with('\n') {
                &line_text[..line_text.len() - 1]
            } else {
                &line_text
            };

            let line = Line::from(Span::styled(
                display_line.to_string(),
                Style::default().fg(color),
            ));

            let line_area = Rect::new(inner.x, inner.y + y, inner.width, 1);
            frame.render_widget(line, line_area);
            y += 1;
        }

        let footer = Line::from(Span::styled(
            "Esc/Backspace: back to file list",
            Style::default().fg(self.theme.muted),
        ));
        let footer_area = Rect::new(
            inner.x,
            inner.y + inner.height.saturating_sub(1),
            inner.width,
            1,
        );
        frame.render_widget(footer, footer_area);
    }
}
