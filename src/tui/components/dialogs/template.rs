use crate::config::schema::SessionTemplate;
use crate::tui::app::TuiMsg;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use std::cell::RefCell;
use std::sync::Arc;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;

pub struct TemplateDialog {
    pub theme: Arc<Theme>,
    pub templates: Vec<(String, SessionTemplate)>,
    pub selected: usize,
    pub scroll: CenteredScroll,
    pub filter: String,
    visible_height: usize,
    filtered_cache: RefCell<Option<(String, Vec<usize>)>>,
}

impl Clone for TemplateDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            templates: self.templates.clone(),
            selected: self.selected,
            scroll: self.scroll.clone(),
            filter: self.filter.clone(),
            visible_height: self.visible_height,
            filtered_cache: RefCell::new(None),
        }
    }
}

impl TemplateDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            templates: Vec::new(),
            selected: 0,
            scroll: CenteredScroll::new(),
            filter: String::new(),
            visible_height: 10,
            filtered_cache: RefCell::new(None),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_templates(&mut self, templates: Vec<(String, SessionTemplate)>) {
        self.templates = templates;
        self.selected = 0;
        self.scroll.reset();
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn selected(&self) -> Option<(String, SessionTemplate)> {
        self.filtered()
            .get(self.selected)
            .map(|(key, t)| (key.clone(), t.clone()))
    }

    pub fn select_up(&mut self) {
        let filtered_len = self.filtered().len();
        if self.selected > 0 {
            self.selected -= 1;
        }
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll
                .clamp(self.selected, filtered_len, self.visible_height);
        }
    }

    pub fn select_down(&mut self) {
        let filtered_len = self.filtered().len();
        let max = filtered_len.saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll
                .clamp(self.selected, filtered_len, self.visible_height);
        }
    }

    pub fn set_filter(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.scroll.reset();
        self.update_cache();
    }

    pub fn backspace_filter(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.scroll.reset();
        self.update_cache();
    }

    fn update_cache(&mut self) {
        if self.filter.is_empty() {
            self.filtered_cache.borrow_mut().take();
            return;
        }
        let indices: Vec<usize> = self
            .templates
            .iter()
            .enumerate()
            .filter(|(_, (key, t))| {
                key.to_lowercase().contains(&self.filter.to_lowercase())
                    || t.name.to_lowercase().contains(&self.filter.to_lowercase())
            })
            .map(|(i, _)| i)
            .collect();
        self.filtered_cache
            .borrow_mut()
            .replace((self.filter.clone(), indices));
    }

    fn filtered(&self) -> Vec<&(String, SessionTemplate)> {
        if self.filter.is_empty() {
            self.templates.iter().collect()
        } else {
            if let Some((ref cache_filter, ref indices)) = self.filtered_cache.borrow().as_ref() {
                if cache_filter == &self.filter {
                    return indices.iter().map(|&i| &self.templates[i]).collect();
                }
            }
            self.templates
                .iter()
                .filter(|(key, t)| {
                    key.to_lowercase().contains(&self.filter.to_lowercase())
                        || t.name.to_lowercase().contains(&self.filter.to_lowercase())
                })
                .collect()
        }
    }
}

impl Default for TemplateDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &TemplateDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Select Template ",
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if !self.filter.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("filter: ", Style::default().fg(self.theme.muted)),
                Span::raw(&self.filter),
            ]));
            lines.push(Line::from(""));
        }

        let filtered = self.filtered();
        let scroll = self.scroll.get();
        for (i, (_key, template)) in filtered.iter().enumerate() {
            if i < scroll {
                continue;
            }
            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(self.theme.primary)
                    .bg(self.theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.foreground)
            };

            lines.push(Line::from(vec![
                Span::styled("  ", style),
                Span::styled(&template.name, style),
            ]));

            if let Some(desc) = &template.description {
                lines.push(Line::from(vec![
                    Span::styled("      ", Style::default()),
                    Span::styled(desc, Style::default().fg(self.theme.muted)),
                ]));
            }

            let details = [
                template.agent.as_deref().unwrap_or("-"),
                template.model.as_deref().unwrap_or("-"),
                template
                    .tags
                    .as_ref()
                    .map(|t| t.join(", "))
                    .unwrap_or_else(|| "-".to_string())
                    .as_str(),
            ]
            .join(" | ");

            lines.push(Line::from(vec![
                Span::styled("      ", Style::default()),
                Span::styled(details, Style::default().fg(self.theme.muted)),
            ]));
            lines.push(Line::from(""));
        }

        if filtered.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled("No templates found", Style::default().fg(self.theme.muted)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Enter: select, Esc: cancel",
            Style::default().fg(self.theme.muted),
        )]));

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border));
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true }).block(block);
        paragraph.render(area, buf);
    }
}

impl Component for TemplateDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.select_down();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if let Some((key, template)) = self.selected() {
                    Some(TuiMsg::SelectTemplate {
                        key,
                        template: Box::new(template),
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        self.filter.push_str(&text);
        self.update_cache();
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(
        &mut self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        theme: &Arc<Theme>,
    ) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Select Template ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if !self.filter.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("filter: ", Style::default().fg(theme.muted)),
                Span::raw(&self.filter),
            ]));
            lines.push(Line::from(""));
        }

        let filtered = self.filtered();
        let scroll = self.scroll.get();
        for (i, (_key, template)) in filtered.iter().enumerate() {
            if i < scroll {
                continue;
            }
            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.primary)
                    .bg(theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };

            lines.push(Line::from(vec![
                Span::styled("  ", style),
                Span::styled(&template.name, style),
            ]));

            if let Some(desc) = &template.description {
                lines.push(Line::from(vec![
                    Span::styled("      ", Style::default()),
                    Span::styled(desc, Style::default().fg(theme.muted)),
                ]));
            }

            let details = [
                template.agent.as_deref().unwrap_or("-"),
                template.model.as_deref().unwrap_or("-"),
                template
                    .tags
                    .as_ref()
                    .map(|t| t.join(", "))
                    .unwrap_or_else(|| "-".to_string())
                    .as_str(),
            ]
            .join(" | ");

            lines.push(Line::from(vec![
                Span::styled("      ", Style::default()),
                Span::styled(details, Style::default().fg(theme.muted)),
            ]));
            lines.push(Line::from(""));
        }

        if filtered.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled("No templates found", Style::default().fg(theme.muted)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Enter: select, Esc: cancel",
            Style::default().fg(theme.muted),
        )]));

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border));
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true }).block(block);
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Template
    }
}
