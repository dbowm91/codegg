use std::cell::RefCell;

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::tui::components::hint_line::HintLine;
use crate::tui::components::scroll::ListCursor;
use crate::tui::theme::Theme;
use crate::util::fuzzy::fuzzy_score;

#[derive(Debug, Clone, PartialEq)]
pub enum CompletionType {
    Slash,
    File,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum CompletionItemKind {
    #[default]
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub description: Option<String>,
    pub kind: CompletionItemKind,
}

impl Default for CompletionItem {
    fn default() -> Self {
        Self {
            label: String::new(),
            description: None,
            kind: CompletionItemKind::File,
        }
    }
}

impl CompletionItem {
    pub fn new(label: String, description: Option<String>) -> Self {
        Self {
            label,
            description,
            kind: CompletionItemKind::default(),
        }
    }

    pub fn new_file(label: String, description: Option<String>) -> Self {
        Self {
            label,
            description,
            kind: CompletionItemKind::File,
        }
    }

    pub fn new_directory(label: String, description: Option<String>) -> Self {
        Self {
            label,
            description,
            kind: CompletionItemKind::Directory,
        }
    }

    pub fn icon(&self) -> &'static str {
        match self.kind {
            CompletionItemKind::File => "📄",
            CompletionItemKind::Directory => "📁",
            CompletionItemKind::Symlink => "🔗",
        }
    }
}

pub struct CompletionOverlay {
    pub items: Vec<CompletionItem>,
    pub cursor: ListCursor,
    pub ctype: CompletionType,
    pub filter: String,
    pub visible: bool,
    pub trigger_x: u16,
    filtered_cache: RefCell<Option<(String, CompletionType, Vec<usize>)>>,
}

impl CompletionOverlay {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            cursor: ListCursor::new(),
            ctype: CompletionType::Slash,
            filter: String::new(),
            visible: false,
            trigger_x: 0,
            filtered_cache: RefCell::new(None),
        }
    }

    pub fn show(
        &mut self,
        ctype: CompletionType,
        items: Vec<CompletionItem>,
        filter: String,
        trigger_x: u16,
    ) {
        self.ctype = ctype;
        self.items = items;
        self.filter = filter;
        self.cursor.reset();
        self.visible = true;
        self.trigger_x = trigger_x;
        *self.filtered_cache.borrow_mut() = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn select_up(&mut self) {
        self.cursor.up(self.filtered_len(), 0);
    }

    pub fn select_down(&mut self) {
        self.cursor.down(self.filtered_len(), 0);
    }

    pub fn selected_index(&self) -> usize {
        self.cursor.index(self.filtered_len())
    }

    pub fn selected_item(&self) -> Option<CompletionItem> {
        self.filtered_items().get(self.selected_index()).cloned()
    }

    fn update_cache(&self) {
        let ctype = self.ctype.clone();
        let filter_str = match ctype {
            CompletionType::Slash => self.filter.trim_start_matches('/').to_lowercase(),
            CompletionType::File => self.filter.trim_start_matches('@').to_lowercase(),
            CompletionType::Agent => self.filter.trim_start_matches('@').to_lowercase(),
        };

        if filter_str.is_empty() {
            let indices: Vec<usize> = (0..self.items.len()).collect();
            *self.filtered_cache.borrow_mut() = Some((self.filter.clone(), ctype, indices));
            return;
        }

        let mut scored: Vec<(usize, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                let item_name = match ctype {
                    CompletionType::Slash => item.label.trim_start_matches('/'),
                    CompletionType::File => &item.label,
                    CompletionType::Agent => &item.label,
                };
                let score = fuzzy_score(&filter_str, item_name);
                if score > 0 {
                    Some((i, score))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by_key(|b| std::cmp::Reverse(b.1));

        let indices: Vec<usize> = scored.into_iter().map(|(i, _)| i).collect();
        *self.filtered_cache.borrow_mut() = Some((self.filter.clone(), ctype, indices));
    }

    pub fn filtered_items(&self) -> Vec<CompletionItem> {
        let cache_valid = self
            .filtered_cache
            .borrow()
            .as_ref()
            .map(|(f, c, _)| f == &self.filter && *c == self.ctype)
            .unwrap_or(false);

        if !cache_valid {
            self.update_cache();
        }

        if let Some((_, _, ref indices)) = *self.filtered_cache.borrow() {
            indices.iter().map(|&i| self.items[i].clone()).collect()
        } else if self.filter.is_empty() {
            self.items.clone()
        } else {
            Vec::new()
        }
    }

    pub fn filtered_len(&self) -> usize {
        let cache_valid = self
            .filtered_cache
            .borrow()
            .as_ref()
            .map(|(f, c, _)| f == &self.filter && *c == self.ctype)
            .unwrap_or(false);

        if !cache_valid {
            self.update_cache();
        }

        self.filtered_cache
            .borrow()
            .as_ref()
            .map(|(_, _, i)| i.len())
            .unwrap_or(self.items.len())
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.cursor.reset();
        *self.filtered_cache.borrow_mut() = None;
    }

    pub fn render(&self, frame: &mut Frame, prompt_area: Rect, theme: &Theme) {
        let items = self.filtered_items();
        if items.is_empty() {
            let compl_h = 3u16;
            let compl_w = 30.min(prompt_area.width.saturating_sub(2));
            let compl_area = Rect {
                x: self.trigger_x,
                y: prompt_area.y.saturating_sub(compl_h),
                width: compl_w,
                height: compl_h,
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.background));
            let hint = Span::styled(" No results ", Style::default().fg(theme.muted));
            frame.render_widget(
                ratatui::widgets::Paragraph::new(hint)
                    .block(block)
                    .alignment(ratatui::layout::Alignment::Center),
                compl_area,
            );
            return;
        }

        let max_h = 8.min(items.len() as u16);
        let compl_h = max_h + 2;
        let compl_w = 40.min(prompt_area.width.saturating_sub(2));

        if compl_w < 3 {
            return;
        }

        let x = self.trigger_x;

        let compl_area = Rect {
            x,
            y: prompt_area.y.saturating_sub(compl_h),
            width: compl_w,
            height: compl_h,
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));

        let inner_w = compl_w.saturating_sub(2);
        let list_area = Rect {
            x: compl_area.x + 1,
            y: compl_area.y + 1,
            width: inner_w,
            height: max_h,
        };

        let hint_area = Rect {
            x: compl_area.x + 1,
            y: compl_area.y + 1 + max_h,
            width: inner_w,
            height: 1,
        };

        let selected = self.cursor.index(items.len());
        let list_items: Vec<ListItem> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let style = if i == selected {
                    Style::default().bg(theme.selection).fg(theme.primary)
                } else {
                    Style::default().fg(theme.foreground)
                };
                let content = match (&self.ctype, &item.description) {
                    (CompletionType::Slash, Some(desc)) => Text::from(vec![Line::from(vec![
                        Span::styled(format!("{} ", item.label), style),
                        Span::styled(desc, Style::default().fg(theme.muted)),
                    ])]),
                    (CompletionType::Agent, Some(desc)) => Text::from(vec![Line::from(vec![
                        Span::styled(format!("{} ", item.label), style),
                        Span::styled(desc, Style::default().fg(theme.muted)),
                    ])]),
                    _ => Text::from(Span::styled(&item.label, style)),
                };
                ListItem::new(content)
            })
            .collect();

        // The popup border belongs to the full popup, not the list's inner
        // rectangle. Applying it to `list_area` consumed two completion rows
        // and clipped the lower entries whenever the list reached its limit.
        frame.render_widget(block, compl_area);
        let list = List::new(list_items);
        frame.render_widget(list, list_area);

        let hint = Span::styled(
            HintLine::from_items(&[("Esc", "close"), ("Tab", "select")]).render(0),
            Style::default().fg(theme.muted),
        );
        frame.render_widget(
            ratatui::widgets::Paragraph::new(hint).alignment(ratatui::layout::Alignment::Center),
            hint_area,
        );
    }
}

impl Default for CompletionOverlay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::components::scroll::ScrollPolicy;

    fn overlay_with(labels: &[&str]) -> CompletionOverlay {
        let mut overlay = CompletionOverlay::new();
        overlay.show(
            CompletionType::Slash,
            labels
                .iter()
                .map(|label| CompletionItem::new(label.to_string(), None))
                .collect(),
            String::new(),
            0,
        );
        overlay
    }

    #[test]
    fn completion_overlay_cursor_clamps_at_both_ends_and_never_wraps() {
        let mut overlay = overlay_with(&["/one", "/two", "/three"]);

        overlay.select_up();
        assert_eq!(
            overlay.selected_index(),
            0,
            "ScrollPolicy::Clamp must stop on the first completion, not wrap to the last"
        );

        overlay.select_down();
        overlay.select_down();
        overlay.select_down();
        assert_eq!(
            overlay.selected_index(),
            2,
            "ScrollPolicy::Clamp must stop on the last completion, not wrap to the first"
        );
        assert_eq!(overlay.cursor.policy(), ScrollPolicy::Clamp);
    }

    #[test]
    fn completion_overlay_clamps_a_cursor_left_stale_by_an_empty_filter_result() {
        let mut overlay = overlay_with(&["/one"]);
        overlay.cursor.set_index(7);
        overlay.set_filter("nothing-matches".to_string());
        assert_eq!(overlay.filtered_len(), 0);
        assert_eq!(
            overlay.selected_index(),
            0,
            "an empty result must resolve to index 0 rather than pointing past the end"
        );
        assert!(overlay.selected_item().is_none());
    }
}
