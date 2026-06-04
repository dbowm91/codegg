use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use std::sync::Arc;

use super::super::theme::Theme;

pub struct StatusBarWidget {
    pub theme: Arc<Theme>,
    pub status: String,
    pub token_str: String,
    pub thinking: bool,
    pub thinking_label: Option<String>,
    pub loading: bool,
    pub loading_label: Option<String>,
    pub subagent_count: usize,
    pub undo_message: Option<String>,
}

impl StatusBarWidget {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            status: "idle".to_string(),
            token_str: String::new(),
            thinking: false,
            thinking_label: None,
            loading: false,
            loading_label: None,
            subagent_count: 0,
            undo_message: None,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_status(&mut self, status: String) {
        self.status = status;
    }

    pub fn set_tokens(&mut self, token_str: String) {
        self.token_str = token_str;
    }

    pub fn set_thinking(&mut self, thinking: bool, label: Option<String>) {
        self.thinking = thinking;
        self.thinking_label = label;
    }

    pub fn set_loading(&mut self, loading: bool, label: Option<String>) {
        self.loading = loading;
        self.loading_label = label;
    }

    pub fn set_subagent_count(&mut self, count: usize) {
        self.subagent_count = count;
    }

    pub fn set_undo_message(&mut self, msg: &str) {
        self.undo_message = Some(msg.to_string());
    }

    pub fn clear_undo_message(&mut self) {
        self.undo_message = None;
    }
}

impl Default for StatusBarWidget {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &StatusBarWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let total_width = area.width as usize;
        if total_width == 0 {
            return;
        }

        let mut left_spans: Vec<Span<'_>> = Vec::new();
        let mut middle_spans: Vec<Span<'_>> = Vec::new();

        let (status_label, status_color) = match self.status.as_str() {
            "working" => ("● working", self.theme.warning),
            "error" => ("✗ error", self.theme.error),
            _ => ("❯ idle", self.theme.primary),
        };
        left_spans.push(Span::styled(
            format!(" {} ", status_label),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        ));

        if let Some(ref msg) = self.undo_message {
            middle_spans.push(Span::styled("  ", Style::default()));
            middle_spans.push(Span::styled(
                format!("{} (press U to undo)", msg),
                Style::default().fg(self.theme.warning).add_modifier(Modifier::BOLD),
            ));
        }

        if self.thinking {
            let label = self.thinking_label.as_deref().unwrap_or("Thinking...");
            middle_spans.push(Span::styled("  ", Style::default()));
            middle_spans.push(Span::styled(
                format!("◌ {label}"),
                Style::default().fg(self.theme.warning).add_modifier(Modifier::BOLD),
            ));
        }

        if self.loading {
            let label = self.loading_label.as_deref().unwrap_or("loading");
            middle_spans.push(Span::styled("  ", Style::default()));
            middle_spans.push(Span::styled(
                format!("⟳ {label}"),
                Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD),
            ));
        }

        if self.subagent_count > 0 {
            let label = if self.subagent_count == 1 {
                "subagent"
            } else {
                "subagents"
            };
            middle_spans.push(Span::styled("  ", Style::default()));
            middle_spans.push(Span::styled(
                format!("{} {}", self.subagent_count, label),
                Style::default().fg(self.theme.secondary),
            ));
        }

        let mut right_spans: Vec<Span<'_>> = Vec::new();
        if !self.token_str.is_empty() {
            right_spans.push(Span::styled(
                format!(" {} ", self.token_str),
                Style::default().fg(self.theme.foreground),
            ));
        }

        let left_width: usize = left_spans.iter().map(|s| s.width()).sum();
        let middle_width: usize = middle_spans.iter().map(|s| s.width()).sum();
        let right_width: usize = right_spans.iter().map(|s| s.width()).sum();

        let used = left_width + middle_width + right_width;
        if used > total_width {
            middle_spans.clear();
            let used = left_width + right_width;
            if used >= total_width {
                right_spans.clear();
            }
        }

        let left_width: usize = left_spans.iter().map(|s| s.width()).sum();
        let middle_width: usize = middle_spans.iter().map(|s| s.width()).sum();
        let right_width: usize = right_spans.iter().map(|s| s.width()).sum();

        let pad1 = total_width.saturating_sub(left_width + middle_width + right_width);

        let mut all_spans: Vec<Span<'_>> = Vec::with_capacity(
            left_spans.len() + middle_spans.len() + right_spans.len() + 2,
        );
        all_spans.extend(left_spans);
        all_spans.extend(middle_spans);
        all_spans.push(Span::raw(" ".repeat(pad1)));
        all_spans.extend(right_spans);

        let line = Line::from(all_spans);
        if area.height <= 1 {
            let paragraph =
                Paragraph::new(line).style(Style::default().bg(self.theme.background));
            paragraph.render(area, buf);
            return;
        }

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(line).block(block);
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    fn rendered_line(buf: &Buffer, width: u16) -> String {
        (0..width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>()
    }

    #[test]
    fn one_row_footer_renders_status_and_tokens() {
        let mut widget = StatusBarWidget::default();
        widget.set_status("working".to_string());
        widget.set_tokens("↓10 ↑5 (15) / 20 75%".to_string());

        let area = Rect::new(0, 0, 60, 1);
        let mut buf = Buffer::empty(area);
        (&widget).render(area, &mut buf);

        let line = rendered_line(&buf, area.width);
        assert!(line.contains("working"));
        assert!(line.contains("↓10 ↑5"));
    }

    #[test]
    fn taller_footer_keeps_top_border() {
        let widget = StatusBarWidget::default();
        let area = Rect::new(0, 0, 20, 2);
        let mut buf = Buffer::empty(area);
        (&widget).render(area, &mut buf);

        let line = rendered_line(&buf, area.width);
        assert!(line.contains("─"));
    }
}
