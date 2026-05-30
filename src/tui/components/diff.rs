use std::sync::Arc;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;
use similar::{ChangeTag, TextDiff};

use crate::tui::components::scroll::CenteredScroll;
use crate::tui::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub enum DiffMode {
    Inline,
    SideBySide,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub line_number_old: Option<usize>,
    pub line_number_new: Option<usize>,
    pub content: String,
    pub tag: ChangeTag,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone)]
pub struct DiffViewer {
    pub theme: Arc<Theme>,
    pub old_content: String,
    pub new_content: String,
    pub hunks: Vec<DiffHunk>,
    pub scroll: CenteredScroll,
    pub mode: DiffMode,
    pub title: String,
}

impl DiffViewer {
    pub fn new(old_content: Box<str>, new_content: Box<str>, title: Box<str>) -> Self {
        let hunks = Self::compute_diff(&old_content, &new_content);
        Self {
            theme: Arc::new(Theme::default()),
            old_content: old_content.to_string(),
            new_content: new_content.to_string(),
            hunks,
            scroll: CenteredScroll::new(),
            mode: DiffMode::Inline,
            title: title.to_string(),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            DiffMode::Inline => DiffMode::SideBySide,
            DiffMode::SideBySide => DiffMode::Inline,
        };
        self.scroll.reset();
    }

    fn compute_diff(old_content: &str, new_content: &str) -> Vec<DiffHunk> {
        let diff = TextDiff::from_lines(old_content, new_content);
        let mut hunks = Vec::new();
        let mut current_hunk: Option<DiffHunk> = None;
        let mut old_line = 0;
        let mut new_line = 0;

        for change in diff.iter_all_changes() {
            let tag = change.tag();
            match tag {
                ChangeTag::Delete => {
                    old_line += 1;
                    if current_hunk.is_none() {
                        current_hunk = Some(DiffHunk {
                            old_start: old_line,
                            old_count: 0,
                            new_start: new_line + 1,
                            new_count: 0,
                            lines: Vec::new(),
                        });
                    }
                    if let Some(ref mut hunk) = current_hunk {
                        hunk.old_count += 1;
                        hunk.lines.push(DiffLine {
                            line_number_old: Some(old_line),
                            line_number_new: None,
                            content: change.value().to_string(),
                            tag,
                        });
                    }
                }
                ChangeTag::Insert => {
                    new_line += 1;
                    if current_hunk.is_none() {
                        current_hunk = Some(DiffHunk {
                            old_start: old_line + 1,
                            old_count: 0,
                            new_start: new_line,
                            new_count: 0,
                            lines: Vec::new(),
                        });
                    }
                    if let Some(ref mut hunk) = current_hunk {
                        hunk.new_count += 1;
                        hunk.lines.push(DiffLine {
                            line_number_old: None,
                            line_number_new: Some(new_line),
                            content: change.value().to_string(),
                            tag,
                        });
                    }
                }
                ChangeTag::Equal => {
                    old_line += 1;
                    new_line += 1;
                    if let Some(hunk) = current_hunk.take() {
                        hunks.push(hunk);
                    }
                }
            }
        }

        if let Some(hunk) = current_hunk {
            hunks.push(hunk);
        }

        hunks
    }

    pub fn total_lines(&self) -> usize {
        self.hunks.iter().map(|h| h.lines.len()).sum()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

        self.render_header(frame, chunks[0]);
        self.render_diff(frame, chunks[1]);
        self.render_footer(frame, chunks[2]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let mode_str = match self.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(self.theme.primary)),
            Span::styled(&self.title, Style::default().fg(self.theme.foreground)),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(self.theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Diff Viewer ")
                .border_style(Style::default().fg(self.theme.border)),
        );

        frame.render_widget(header, area);
    }

    fn render_diff(&self, frame: &mut Frame, area: Rect) {
        if area.height < 1 {
            return;
        }

        let inner = area.intersection(Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        ));

        let visible_lines = inner.height as usize;
        let scroll_offset = self.scroll.get();

        let mut y = 0u16;

        for hunk in &self.hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    ChangeTag::Delete => self.theme.error,
                    ChangeTag::Insert => self.theme.success,
                    ChangeTag::Equal => self.theme.foreground,
                };

                let old_num = line
                    .line_number_old
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());
                let new_num = line
                    .line_number_new
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());

                let prefix = match line.tag {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(inner.x, inner.y + y, inner.width, 1);
                frame.render_widget(styled_line, line_area);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let mode_toggle = match self.mode {
            DiffMode::Inline => "Press 's' for Side-by-Side",
            DiffMode::SideBySide => "Press 's' for Inline",
        };

        let info = format!("Scroll: ↑↓ or j/k | Mode: {} | Close: Esc", mode_toggle);

        let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            info,
            Style::default().fg(self.theme.muted),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.border)),
        );

        frame.render_widget(footer, area);
    }

    pub fn handle_scroll(&mut self, delta: isize) {
        let current = self.scroll.get() as isize;
        let new_scroll = current.saturating_add(delta).max(0) as usize;
        self.scroll.set(new_scroll);
    }

    pub fn scroll_to_bottom(&mut self) {
        let total = self.total_lines();
        self.scroll.set(total.saturating_sub(1));
    }
}

impl Widget for &DiffViewer {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        if area.height < 7 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(" Diff Viewer ");

        let inner = block.inner(area);
        block.render(area, buf);

        let header_area = Rect::new(inner.x, inner.y, inner.width, 1);
        let mode_str = match self.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(self.theme.primary)),
            Span::styled(&self.title, Style::default().fg(self.theme.foreground)),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(self.theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line);
        header.render(header_area, buf);

        let footer_area = Rect::new(
            inner.x,
            inner.y.saturating_add(inner.height).saturating_sub(1),
            inner.width,
            1,
        );

        let mode_toggle = match self.mode {
            DiffMode::Inline => "Press 's' for Side-by-Side",
            DiffMode::SideBySide => "Press 's' for Inline",
        };

        let info = format!("Scroll: ↑↓ or j/k | Mode: {} | Close: Esc", mode_toggle);

        let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            info,
            Style::default().fg(self.theme.muted),
        )));

        footer.render(footer_area, buf);

        let content_height = inner.height.saturating_sub(2);
        if content_height == 0 {
            return;
        }

        let visible_lines = content_height as usize;
        let scroll_offset = self.scroll.get();
        let mut y = 0u16;

        for hunk in &self.hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    ChangeTag::Delete => self.theme.error,
                    ChangeTag::Insert => self.theme.success,
                    ChangeTag::Equal => self.theme.foreground,
                };

                let old_num = line
                    .line_number_old
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());
                let new_num = line
                    .line_number_new
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());

                let prefix = match line.tag {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(inner.x, inner.y + 1 + y, inner.width, 1);
                styled_line.render(line_area, buf);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }
    }
}
