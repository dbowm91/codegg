use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;
use codegg_core::run_store::RunDetailView;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDetailTab {
    Summary,
    Invocation,
    Output,
    Artifacts,
    Changes,
    Policy,
    Context,
}

impl RunDetailTab {
    pub fn all() -> &'static [RunDetailTab] {
        &[
            Self::Summary,
            Self::Invocation,
            Self::Output,
            Self::Artifacts,
            Self::Changes,
            Self::Policy,
            Self::Context,
        ]
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Summary => "Summary",
            Self::Invocation => "Invocation",
            Self::Output => "Output",
            Self::Artifacts => "Artifacts",
            Self::Changes => "Changes",
            Self::Policy => "Policy",
            Self::Context => "Context",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunDetailDialog {
    pub detail: RunDetailView,
    pub tab_index: usize,
    pub scroll: usize,
    pub selected_artifact: Option<usize>,
    pub theme: Arc<Theme>,
}

impl RunDetailDialog {
    pub fn new(detail: RunDetailView, theme: Arc<Theme>) -> Self {
        Self {
            detail,
            tab_index: 0,
            scroll: 0,
            selected_artifact: None,
            theme,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    fn current_tab(&self) -> &RunDetailTab {
        &RunDetailTab::all()[self.tab_index]
    }

    fn render_summary(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let cell = &self.detail.cell;
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Run ID: ", Style::default().fg(Color::DarkGray)),
                Span::raw(cell.run_id.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Title: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&cell.title),
            ]),
            Line::from(vec![
                Span::styled("Kind: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{:?}", cell.kind)),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:?}", cell.status),
                    Self::status_style(&cell.status),
                ),
            ]),
            Line::from(vec![
                Span::styled("Backend: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&cell.backend_label),
            ]),
        ];

        if let Some(duration) = cell.duration {
            let secs = duration.num_seconds();
            lines.push(Line::from(vec![
                Span::styled("Duration: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{}s", secs)),
            ]));
        }

        lines.push(Line::from(vec![
            Span::styled("Risk: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&cell.risk_label),
        ]));

        if let Some(ref sandbox) = cell.sandbox_label {
            lines.push(Line::from(vec![
                Span::styled("Sandbox: ", Style::default().fg(Color::DarkGray)),
                Span::raw(sandbox),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            &cell.summary,
            Style::default().fg(Color::White),
        )));

        if cell.changed_file_count > 0 {
            lines.push(Line::from(vec![
                Span::styled("Changed files: ", Style::default().fg(Color::DarkGray)),
                Span::raw(cell.changed_file_count.to_string()),
            ]));
        }

        // Context promotion state
        let ctx_color = match &cell.context_state {
            codegg_core::run_store::ContextPromotionState::LocalOnly => Color::DarkGray,
            codegg_core::run_store::ContextPromotionState::ProjectionIncluded => Color::Green,
            codegg_core::run_store::ContextPromotionState::ArtifactRangeIncluded { .. } => {
                Color::Cyan
            }
            codegg_core::run_store::ContextPromotionState::Pinned => Color::Yellow,
            codegg_core::run_store::ContextPromotionState::Excluded => Color::Red,
        };
        let ctx_label = match &cell.context_state {
            codegg_core::run_store::ContextPromotionState::LocalOnly => "Local only",
            codegg_core::run_store::ContextPromotionState::ProjectionIncluded => {
                "Projection included"
            }
            codegg_core::run_store::ContextPromotionState::ArtifactRangeIncluded { .. } => {
                "Artifact range"
            }
            codegg_core::run_store::ContextPromotionState::Pinned => "Pinned",
            codegg_core::run_store::ContextPromotionState::Excluded => "Excluded",
        };
        lines.push(Line::from(vec![
            Span::styled("Context: ", Style::default().fg(Color::DarkGray)),
            Span::styled(ctx_label, Style::default().fg(ctx_color)),
        ]));

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Summary"))
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }

    fn render_invocation(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let inv = &self.detail.invocation;
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Command: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&inv.command),
            ]),
            Line::from(vec![
                Span::styled("CWD: ", Style::default().fg(Color::DarkGray)),
                Span::raw(inv.cwd.display().to_string()),
            ]),
            Line::from(vec![
                Span::styled("Workspace: ", Style::default().fg(Color::DarkGray)),
                Span::raw(inv.workspace_root.display().to_string()),
            ]),
            Line::from(vec![
                Span::styled("Backend: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&inv.backend_family),
            ]),
        ];

        if let Some(ref detail) = inv.backend_detail {
            lines.push(Line::from(vec![
                Span::styled("Backend detail: ", Style::default().fg(Color::DarkGray)),
                Span::raw(detail),
            ]));
        }

        if let Some(ref argv) = inv.argv {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "argv:",
                Style::default().fg(Color::DarkGray),
            )));
            for arg in argv {
                lines.push(Line::from(format!("  {}", arg)));
            }
        }

        if let Some(ref hash) = inv.script_hash {
            lines.push(Line::from(vec![
                Span::styled("Script hash: ", Style::default().fg(Color::DarkGray)),
                Span::raw(hash),
            ]));
        }

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Invocation"))
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }

    fn render_artifacts(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let artifacts = &self.detail.artifacts;
        if artifacts.is_empty() {
            let paragraph = Paragraph::new("No artifacts.")
                .block(Block::default().borders(Borders::ALL).title("Artifacts"));
            paragraph.render(area, buf);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();
        for (i, artifact) in artifacts.iter().enumerate() {
            let style = if self.selected_artifact == Some(i) {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:?}", artifact.kind), style),
                Span::styled(
                    format!(
                        "  {} ({})",
                        artifact.relative_path,
                        Self::format_bytes(artifact.byte_length)
                    ),
                    style,
                ),
            ]));
            if artifact.truncated {
                lines.push(Line::from(Span::styled(
                    "    (truncated)",
                    Style::default().fg(Color::Yellow),
                )));
            }
            if artifact.redacted {
                lines.push(Line::from(Span::styled(
                    "    (redacted)",
                    Style::default().fg(Color::Yellow),
                )));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Artifacts"))
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }

    fn render_changes(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let changes = &self.detail.changes;
        if changes.is_empty() {
            let paragraph = Paragraph::new("No changed files.")
                .block(Block::default().borders(Borders::ALL).title("Changes"));
            paragraph.render(area, buf);
            return;
        }

        let lines: Vec<Line> = changes
            .iter()
            .map(|c| {
                Line::from(vec![
                    Span::styled(
                        format!("[{}] ", c.kind),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(c.path.display().to_string()),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Changes"))
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }

    fn render_policy(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let policy = &self.detail.policy;
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Risk: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&policy.risk_level),
            ]),
            Line::from(vec![
                Span::styled("Subprocess: ", Style::default().fg(Color::DarkGray)),
                Span::raw(if policy.has_subprocess { "yes" } else { "no" }),
            ]),
            Line::from(vec![
                Span::styled("Git mutation: ", Style::default().fg(Color::DarkGray)),
                Span::raw(if policy.has_git_mutation { "yes" } else { "no" }),
            ]),
            Line::from(vec![
                Span::styled("Destructive: ", Style::default().fg(Color::DarkGray)),
                Span::raw(if policy.has_destructive_mutation {
                    "yes"
                } else {
                    "no"
                }),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("OS isolation: ", Style::default().fg(Color::DarkGray)),
                Span::raw(if policy.os_isolation { "yes" } else { "no" }),
            ]),
            Line::from(vec![
                Span::styled("Network isolation: ", Style::default().fg(Color::DarkGray)),
                Span::raw(if policy.network_isolation {
                    "yes"
                } else {
                    "no"
                }),
            ]),
        ];

        if !policy.read_roots.is_empty() {
            lines.push(Line::from(Span::styled(
                "Read roots:",
                Style::default().fg(Color::DarkGray),
            )));
            for root in &policy.read_roots {
                lines.push(Line::from(format!("  {}", root.display())));
            }
        }

        if !policy.write_roots.is_empty() {
            lines.push(Line::from(Span::styled(
                "Write roots:",
                Style::default().fg(Color::DarkGray),
            )));
            for root in &policy.write_roots {
                lines.push(Line::from(format!("  {}", root.display())));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Policy"))
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }

    fn render_context(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let state = &self.detail.cell.context_state;
        let (state_label, state_color) = match state {
            codegg_core::run_store::ContextPromotionState::LocalOnly => {
                ("Local only (not in context)", Color::DarkGray)
            }
            codegg_core::run_store::ContextPromotionState::ProjectionIncluded => {
                ("Projection included in context", Color::Green)
            }
            codegg_core::run_store::ContextPromotionState::ArtifactRangeIncluded { .. } => {
                ("Artifact range included in context", Color::Cyan)
            }
            codegg_core::run_store::ContextPromotionState::Pinned => {
                ("Pinned for future context", Color::Yellow)
            }
            codegg_core::run_store::ContextPromotionState::Excluded => {
                ("Excluded from context", Color::Red)
            }
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled("State: ", Style::default().fg(Color::DarkGray)),
                Span::styled(state_label, Style::default().fg(state_color)),
            ]),
            Line::from(""),
        ];

        // Estimate token contribution
        let total_artifact_bytes: u64 = self.detail.artifacts.iter().map(|a| a.byte_length).sum();
        let est_tokens = total_artifact_bytes / 4; // rough estimate
        lines.push(Line::from(vec![
            Span::styled("Estimated tokens: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("~{}", est_tokens)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Artifact bytes: ", Style::default().fg(Color::DarkGray)),
            Span::raw(Self::format_bytes(total_artifact_bytes)),
        ]));

        lines.push(Line::from(""));
        match state {
            codegg_core::run_store::ContextPromotionState::LocalOnly => {
                lines.push(Line::from(Span::styled(
                    "Press [p] to promote to context",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            codegg_core::run_store::ContextPromotionState::Pinned => {
                lines.push(Line::from(Span::styled(
                    "Press [u] to unpin from context",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            _ => {
                lines.push(Line::from(Span::styled(
                    "Press [p] to change promotion state",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Context"))
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }

    fn render_output(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let proj = &self.detail.projection;
        let artifacts = &self.detail.artifacts;

        let mut lines: Vec<Line> = Vec::new();

        if let Some(proj) = proj {
            lines.push(Line::from(vec![
                Span::styled("Projector: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&proj.projector),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Exactness: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&proj.exactness),
            ]));
            if !proj.omitted_ranges.is_empty() {
                lines.push(Line::from(format!(
                    "Omitted ranges: {}",
                    proj.omitted_ranges.len()
                )));
            }
            lines.push(Line::from(""));
        }

        let stdout = artifacts
            .iter()
            .find(|a| matches!(a.kind, codegg_core::run_store::ArtifactKind::Stdout));
        let stderr = artifacts
            .iter()
            .find(|a| matches!(a.kind, codegg_core::run_store::ArtifactKind::Stderr));

        if let Some(stdout) = stdout {
            lines.push(Line::from(Span::styled(
                "stdout:",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(format!(
                "  {} ({})",
                stdout.relative_path,
                Self::format_bytes(stdout.byte_length)
            )));
        }
        if let Some(stderr) = stderr {
            lines.push(Line::from(Span::styled(
                "stderr:",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(format!(
                "  {} ({})",
                stderr.relative_path,
                Self::format_bytes(stderr.byte_length)
            )));
        }

        if stdout.is_none() && stderr.is_none() {
            lines.push(Line::from("No stdout/stderr artifacts."));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press [Enter] to view artifact content",
            Style::default().fg(Color::DarkGray),
        )));

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Output"))
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }

    fn status_style(status: &codegg_core::run_store::RunStatus) -> Style {
        match status {
            codegg_core::run_store::RunStatus::Running => Style::default().fg(Color::Yellow),
            codegg_core::run_store::RunStatus::Complete => Style::default().fg(Color::Green),
            codegg_core::run_store::RunStatus::Failed => Style::default().fg(Color::Red),
            codegg_core::run_store::RunStatus::TimedOut => Style::default().fg(Color::Red),
            codegg_core::run_store::RunStatus::Cancelled => Style::default().fg(Color::DarkGray),
            codegg_core::run_store::RunStatus::Incomplete => Style::default().fg(Color::Yellow),
        }
    }

    fn format_bytes(bytes: u64) -> String {
        if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else {
            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
        }
    }
}

impl Component for RunDetailDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Some(TuiMsg::CloseDialog),
            KeyCode::Left | KeyCode::Char('h') => {
                if self.tab_index > 0 {
                    self.tab_index -= 1;
                    self.scroll = 0;
                }
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.tab_index < RunDetailTab::all().len() - 1 {
                    self.tab_index += 1;
                    self.scroll = 0;
                }
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll += 1;
                None
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(20);
                None
            }
            KeyCode::PageDown => {
                self.scroll += 20;
                None
            }
            KeyCode::Home => {
                self.scroll = 0;
                None
            }
            KeyCode::Char('r') if self.detail.cell.can_rerun => Some(TuiMsg::RunRerun {
                run_id: self.detail.cell.run_id.to_string(),
            }),
            KeyCode::Char('p') if self.detail.cell.can_promote => Some(TuiMsg::RunPromote {
                run_id: self.detail.cell.run_id.to_string(),
            }),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(TuiMsg::RunCopyId {
                    run_id: self.detail.cell.run_id.to_string(),
                })
            }
            _ => None,
        }
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _theme: &Arc<Theme>) {
        let area = centered_rect(80, 80, area);
        frame.render_widget(Clear, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title("Run Detail")
            .padding(Padding::new(1, 1, 0, 0));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let tab_titles: Vec<Line> = RunDetailTab::all()
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let style = if i == self.tab_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                Line::from(Span::styled(tab.label(), style))
            })
            .collect();

        let tabs = Tabs::new(tab_titles)
            .select(self.tab_index)
            .highlight_style(Style::default().fg(Color::Yellow));

        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        frame.render_widget(tabs, chunks[0]);

        match self.current_tab() {
            RunDetailTab::Summary => self.render_summary(chunks[1], frame.buffer_mut()),
            RunDetailTab::Invocation => self.render_invocation(chunks[1], frame.buffer_mut()),
            RunDetailTab::Output => self.render_output(chunks[1], frame.buffer_mut()),
            RunDetailTab::Artifacts => self.render_artifacts(chunks[1], frame.buffer_mut()),
            RunDetailTab::Changes => self.render_changes(chunks[1], frame.buffer_mut()),
            RunDetailTab::Policy => self.render_policy(chunks[1], frame.buffer_mut()),
            RunDetailTab::Context => self.render_context(chunks[1], frame.buffer_mut()),
        }
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::RunDetail
    }

    fn is_modal(&self) -> bool {
        true
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
