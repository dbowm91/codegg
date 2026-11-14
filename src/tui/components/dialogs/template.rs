use crate::config::schema::SessionTemplate;
use crate::tui::app::TuiMsg;
use crate::tui::components::hint_line::HintLine;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use std::cell::RefCell;
use std::sync::Arc;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::ListCursor;
use super::super::section::Section;

pub struct TemplateDialog {
    pub theme: Arc<Theme>,
    pub templates: Vec<(String, SessionTemplate)>,
    pub cursor: ListCursor,
    pub filter: String,
    visible_height: usize,
    filtered_cache: RefCell<Option<(String, Vec<usize>)>>,
}

impl Clone for TemplateDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            templates: self.templates.clone(),
            cursor: self.cursor.clone(),
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
            cursor: ListCursor::new(),
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
        self.cursor.reset();
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn selected(&self) -> Option<(String, SessionTemplate)> {
        let filtered = self.filtered();
        filtered
            .get(self.cursor.index(filtered.len()))
            .map(|(key, t)| (key.clone(), t.clone()))
    }

    pub fn select_up(&mut self) {
        let len = self.filtered().len();
        self.cursor.up(len, self.visible_height);
    }

    pub fn select_down(&mut self) {
        let len = self.filtered().len();
        self.cursor.down(len, self.visible_height);
    }

    pub fn set_filter(&mut self, c: char) {
        self.filter.push(c);
        self.cursor.reset();
        self.update_cache();
    }

    pub fn backspace_filter(&mut self) {
        self.filter.pop();
        self.cursor.reset();
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

        if !self.filter.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("filter: ", Style::default().fg(self.theme.muted)),
                Span::raw(&self.filter),
            ]));
            lines.push(Line::from(""));
        }

        let filtered = self.filtered();
        let scroll = self.cursor.offset();
        let selected = self.cursor.index(filtered.len());
        for (i, (_key, template)) in filtered.iter().enumerate() {
            if i < scroll {
                continue;
            }
            let is_selected = i == selected;
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
            HintLine::from_items(&[("Esc", "close"), ("↑/↓", "navigate"), ("Enter", "select")])
                .render(0),
            Style::default().fg(self.theme.muted),
        )]));

        let inner = Section::new("Select Template", &self.theme)
            .focused(true)
            .draw(area, buf);
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .render(inner, buf);
    }
}

impl Component for TemplateDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            // `j`/`k` navigate only while no query is being typed. This
            // dialog renders a filter box, so once it holds text they are
            // ordinary characters. Arrow keys always navigate.
            crossterm::event::KeyCode::Up => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Down => {
                self.select_down();
                None
            }
            crossterm::event::KeyCode::Char('k') if self.filter.is_empty() => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Char('j') if self.filter.is_empty() => {
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
            // Typing. Without this arm the filter could only ever be filled
            // by paste, so a template query was impossible to type.
            crossterm::event::KeyCode::Char(c) => {
                self.filter.push(c);
                self.update_cache();
                None
            }
            crossterm::event::KeyCode::Backspace => {
                self.filter.pop();
                self.update_cache();
                None
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

        if !self.filter.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("filter: ", Style::default().fg(theme.muted)),
                Span::raw(&self.filter),
            ]));
            lines.push(Line::from(""));
        }

        let filtered = self.filtered();
        let scroll = self.cursor.offset();
        let selected = self.cursor.index(filtered.len());
        for (i, (_key, template)) in filtered.iter().enumerate() {
            if i < scroll {
                continue;
            }
            let is_selected = i == selected;
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
            HintLine::from_items(&[("Esc", "close"), ("↑/↓", "navigate"), ("Enter", "select")])
                .render(0),
            Style::default().fg(theme.muted),
        )]));

        let inner = Section::new("Select Template", theme)
            .focused(true)
            .draw(area, frame.buffer_mut());
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .render(inner, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Template
    }
}

#[cfg(test)]
mod cursor_policy_tests {
    use super::*;
    use crate::tui::components::scroll::ScrollPolicy;

    fn template(name: &str) -> SessionTemplate {
        SessionTemplate {
            name: name.to_string(),
            description: None,
            agent: None,
            model: None,
            instructions: None,
            tags: None,
        }
    }

    fn dialog_with(names: &[&str], visible_height: usize) -> TemplateDialog {
        let mut dialog = TemplateDialog::new(Arc::new(Theme::default()));
        dialog.set_templates(
            names
                .iter()
                .map(|name| ((*name).to_string(), template(name)))
                .collect(),
        );
        dialog.set_visible_height(visible_height);
        dialog
    }

    #[test]
    fn template_cursor_clamps_at_both_ends_and_never_wraps() {
        let mut dialog = dialog_with(&["one", "two", "three"], 2);

        dialog.select_up();
        assert_eq!(
            dialog.cursor.index(3),
            0,
            "ScrollPolicy::Clamp must stop on the first template, not wrap to the last"
        );

        dialog.select_down();
        dialog.select_down();
        dialog.select_down();
        assert_eq!(
            dialog.cursor.index(3),
            2,
            "ScrollPolicy::Clamp must stop on the last template, not wrap to the first"
        );
        assert_eq!(dialog.cursor.policy(), ScrollPolicy::Clamp);
    }

    #[test]
    fn template_window_follows_the_cursor_past_the_last_visible_row() {
        let mut dialog = dialog_with(&["one", "two", "three"], 2);

        dialog.select_down();
        assert_eq!(
            dialog.cursor.offset(),
            0,
            "a step inside the window is free"
        );
        dialog.select_down();
        assert_eq!(
            dialog.cursor.offset(),
            1,
            "the window must follow once the cursor passes its last visible row"
        );
        assert_eq!(
            dialog.selected().map(|(key, _)| key),
            Some("three".to_string())
        );
    }

    #[test]
    fn template_set_templates_returns_the_cursor_to_the_top() {
        let mut dialog = dialog_with(&["one", "two"], 4);
        dialog.select_down();
        dialog.set_templates(vec![("only".to_string(), template("only"))]);
        assert_eq!(dialog.cursor.index(1), 0);
        assert_eq!(dialog.cursor.offset(), 0);
    }
}
