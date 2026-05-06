use crate::session::message::ToolStatus;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

fn is_diff_output(output: &str) -> bool {
    output.starts_with("diff --")
        || output.contains("+++ ")
        || output.contains("--- ")
        || (output.contains("@@ ") && output.contains("+") && output.contains("-"))
}

#[derive(Debug, Clone)]
pub struct ToolCallEntry {
    pub name: String,
    pub input: String,
    pub output: String,
    pub status: ToolStatus,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub output_lines: Option<usize>,
}

pub struct ToolOutputWidget {
    pub entries: Vec<ToolCallEntry>,
    pub expanded: Vec<bool>,
}

impl ToolOutputWidget {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            expanded: Vec::new(),
        }
    }

    pub fn add_entry(&mut self, entry: ToolCallEntry) {
        self.entries.push(entry);
        self.expanded.push(false);
    }

    pub fn update_entry(&mut self, idx: usize, output: String, status: ToolStatus) {
        if let Some(entry) = self.entries.get_mut(idx) {
            entry.output = output;
            entry.status = status;
        }
    }

    pub fn toggle(&mut self, idx: usize) {
        if let Some(val) = self.expanded.get_mut(idx) {
            *val = !*val;
        }
    }
}

impl Default for ToolOutputWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for &ToolOutputWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        for (i, entry) in self.entries.iter().enumerate() {
            let status_style = match entry.status {
                ToolStatus::Pending => {
                    Style::default().fg(ratatui::style::Color::Rgb(100, 100, 110))
                }
                ToolStatus::Running => {
                    Style::default().fg(ratatui::style::Color::Rgb(255, 180, 60))
                }
                ToolStatus::Completed => {
                    Style::default().fg(ratatui::style::Color::Rgb(80, 200, 120))
                }
                ToolStatus::Error => Style::default().fg(ratatui::style::Color::Rgb(255, 80, 80)),
            };
            let status_label = match entry.status {
                ToolStatus::Pending => "pending",
                ToolStatus::Running => "running",
                ToolStatus::Completed => "done",
                ToolStatus::Error => "error",
            };
            let icon = if self.expanded.get(i).copied().unwrap_or(false) {
                "▼"
            } else {
                "▶"
            };

            lines.push(Line::from(vec![
                Span::styled(format!(" {icon} "), Style::default()),
                Span::styled(&entry.name, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::styled(status_label, status_style),
            ]));

            if self.expanded.get(i).copied().unwrap_or(false) {
                let is_diff = is_diff_output(&entry.output);
                if !entry.input.is_empty() {
                    for l in entry.input.lines().take(5) {
                        lines.push(Line::from(Span::styled(
                            format!("  {l}"),
                            Style::default().fg(ratatui::style::Color::Rgb(100, 100, 110)),
                        )));
                    }
                }
                if !entry.output.is_empty() {
                    for l in entry.output.lines().take(10) {
                        let color = if is_diff {
                            if l.starts_with('+') {
                                ratatui::style::Color::Rgb(80, 200, 120)
                            } else if l.starts_with('-') {
                                ratatui::style::Color::Rgb(255, 100, 100)
                            } else if l.starts_with('@') {
                                ratatui::style::Color::Rgb(100, 150, 255)
                            } else {
                                ratatui::style::Color::Rgb(140, 140, 150)
                            }
                        } else {
                            ratatui::style::Color::Rgb(140, 140, 150)
                        };
                        lines.push(Line::from(Span::styled(
                            format!("  {l}"),
                            Style::default().fg(color),
                        )));
                    }
                }
            }
        }

        let block = Block::default()
            .title(" Tools ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ratatui::style::Color::Rgb(50, 50, 60)));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}
