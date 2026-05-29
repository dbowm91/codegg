use crate::session::events::ToolRisk;
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

fn risk_color(risk: &ToolRisk) -> ratatui::style::Color {
    match risk {
        ToolRisk::Read => ratatui::style::Color::Rgb(100, 180, 255),
        ToolRisk::Write => ratatui::style::Color::Rgb(255, 200, 60),
        ToolRisk::GitMutation => ratatui::style::Color::Rgb(255, 150, 50),
        ToolRisk::DependencyMutation => ratatui::style::Color::Rgb(200, 130, 255),
        ToolRisk::Network => ratatui::style::Color::Rgb(130, 200, 255),
        ToolRisk::Destructive => ratatui::style::Color::Rgb(255, 80, 80),
        ToolRisk::CredentialAdjacent => ratatui::style::Color::Rgb(255, 120, 120),
        ToolRisk::Unknown => ratatui::style::Color::Rgb(140, 140, 150),
    }
}

fn risk_label(risk: &ToolRisk) -> &'static str {
    match risk {
        ToolRisk::Read => "read-only",
        ToolRisk::Write => "write",
        ToolRisk::GitMutation => "git",
        ToolRisk::DependencyMutation => "deps",
        ToolRisk::Network => "network",
        ToolRisk::Destructive => "destructive",
        ToolRisk::CredentialAdjacent => "secrets",
        ToolRisk::Unknown => "?",
    }
}

fn format_duration(duration_ms: u64) -> String {
    if duration_ms < 1000 {
        format!("{}ms", duration_ms)
    } else {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    }
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
    pub risk: ToolRisk,
    pub summary: Option<String>,
    pub cwd: Option<String>,
}

impl ToolCallEntry {
    pub fn new(name: String, input: String, status: ToolStatus) -> Self {
        Self {
            name,
            input,
            output: String::new(),
            status,
            duration_ms: None,
            exit_code: None,
            output_lines: None,
            risk: ToolRisk::Unknown,
            summary: None,
            cwd: None,
        }
    }
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

    pub fn set_entry_risk(&mut self, idx: usize, risk: ToolRisk) {
        if let Some(entry) = self.entries.get_mut(idx) {
            entry.risk = risk;
        }
    }

    pub fn set_entry_summary(&mut self, idx: usize, summary: Option<String>) {
        if let Some(entry) = self.entries.get_mut(idx) {
            entry.summary = summary;
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
                ToolStatus::Error => "failed",
            };
            let icon = if self.expanded.get(i).copied().unwrap_or(false) {
                "▼"
            } else {
                "▶"
            };

            // Header line: icon + tool name + status + risk badge + duration
            let mut header_spans = vec![
                Span::styled(format!(" {icon} "), Style::default()),
                Span::styled(
                    entry.name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(status_label, status_style),
            ];

            // Risk badge
            let rc = risk_color(&entry.risk);
            header_spans.push(Span::styled(
                format!(" [{}]", risk_label(&entry.risk)),
                Style::default().fg(rc),
            ));

            // Duration
            if let Some(dur) = entry.duration_ms {
                header_spans.push(Span::styled(
                    format!(" {}", format_duration(dur)),
                    Style::default()
                        .fg(ratatui::style::Color::Rgb(120, 120, 130))
                        .add_modifier(Modifier::DIM),
                ));
            }

            // Exit code for failed commands
            if entry.status == ToolStatus::Error {
                if let Some(code) = entry.exit_code {
                    header_spans.push(Span::styled(
                        format!(" exit:{}", code),
                        Style::default().fg(ratatui::style::Color::Rgb(255, 100, 100)),
                    ));
                }
            }

            lines.push(Line::from(header_spans));

            // Summary line (if available, shown when collapsed)
            if !self.expanded.get(i).copied().unwrap_or(false) {
                if let Some(ref summary) = entry.summary {
                    lines.push(Line::from(Span::styled(
                        format!("    {summary}"),
                        Style::default()
                            .fg(ratatui::style::Color::Rgb(160, 160, 170))
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
            }

            // Expanded view: raw input + output
            if self.expanded.get(i).copied().unwrap_or(false) {
                // Input/command preview
                if !entry.input.is_empty() {
                    let input_lines: Vec<&str> = entry.input.lines().take(5).collect();
                    for l in &input_lines {
                        lines.push(Line::from(Span::styled(
                            format!("  {l}"),
                            Style::default().fg(ratatui::style::Color::Rgb(100, 100, 110)),
                        )));
                    }
                    if entry.input.lines().count() > 5 {
                        lines.push(Line::from(Span::styled(
                            format!("  ... (+{} more lines)", entry.input.lines().count() - 5),
                            Style::default()
                                .fg(ratatui::style::Color::Rgb(100, 100, 110))
                                .add_modifier(Modifier::DIM),
                        )));
                    }
                }

                // Cwd line
                if let Some(ref cwd) = entry.cwd {
                    lines.push(Line::from(Span::styled(
                        format!("  cwd: {cwd}"),
                        Style::default().fg(ratatui::style::Color::Rgb(100, 100, 110)),
                    )));
                }

                // Output
                if !entry.output.is_empty() {
                    let is_diff = is_diff_output(&entry.output);
                    let output_limit = 20;
                    for l in entry.output.lines().take(output_limit) {
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
                    let total_lines = entry.output.lines().count();
                    if total_lines > output_limit {
                        lines.push(Line::from(Span::styled(
                            format!("  ... (+{} more lines)", total_lines - output_limit),
                            Style::default()
                                .fg(ratatui::style::Color::Rgb(100, 100, 110))
                                .add_modifier(Modifier::DIM),
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
