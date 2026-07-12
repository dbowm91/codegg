use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::util::truncate::truncate_prefix;

use crate::research::service::ResearchRunSummary;
use crate::research::types::{Confidence, ResearchBundle, RunStatus};

#[derive(Debug, Clone, PartialEq)]
pub enum ResearchBrowserMode {
    RunsList,
    RunDetail,
    ReportView,
}

#[derive(Debug, Clone)]
pub enum ReportSection {
    Report,
    Brief,
    AgentAnswer,
    Handoff,
}

#[derive(Clone)]
pub struct ResearchBrowserDialog {
    pub theme: Arc<Theme>,
    pub runs: Vec<ResearchRunSummary>,
    pub selected: usize,
    pub mode: ResearchBrowserMode,
    pub detail: Option<ResearchBundle>,
    pub detail_selected: usize,
    pub content_scroll: usize,
    pub report_content: Option<(ReportSection, String)>,
    pub loading: bool,
}

impl ResearchBrowserDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            runs: Vec::new(),
            selected: 0,
            mode: ResearchBrowserMode::RunsList,
            detail: None,
            detail_selected: 0,
            content_scroll: 0,
            report_content: None,
            loading: false,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_runs(&mut self, runs: Vec<ResearchRunSummary>) {
        self.runs = runs;
        self.selected = 0;
        self.mode = ResearchBrowserMode::RunsList;
    }

    pub fn set_bundle(&mut self, bundle: ResearchBundle) {
        self.detail = Some(bundle);
        self.detail_selected = 0;
        self.mode = ResearchBrowserMode::RunDetail;
    }

    pub fn set_report_content(&mut self, section: ReportSection, content: String) {
        self.report_content = Some((section, content));
        self.content_scroll = 0;
        self.mode = ResearchBrowserMode::ReportView;
    }

    fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn select_down(&mut self) {
        if self.selected + 1 < self.runs.len() {
            self.selected += 1;
        }
    }

    fn detail_select_up(&mut self) {
        if self.detail_selected > 0 {
            self.detail_selected -= 1;
        }
    }

    fn detail_select_down(&mut self, max: usize) {
        if max > 0 && self.detail_selected + 1 < max {
            self.detail_selected += 1;
        }
    }

    fn selected_run(&self) -> Option<&ResearchRunSummary> {
        self.runs.get(self.selected)
    }

    fn available_report_sections(&self) -> Vec<(&'static str, ReportSection)> {
        let mut sections = Vec::new();
        if let Some(ref bundle) = self.detail {
            if let Some(ref plan) = bundle.plan {
                if !plan.scope.is_empty() {
                    sections.push(("Research Plan", ReportSection::Report));
                }
            }
            sections.push(("Sources", ReportSection::Brief));
            sections.push(("Claims", ReportSection::AgentAnswer));
            sections.push(("Contradictions", ReportSection::Handoff));
        }
        sections
    }
}

impl Component for ResearchBrowserDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match self.mode {
            ResearchBrowserMode::RunsList => match key.code {
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
                    if let Some(run) = self.selected_run() {
                        let run_id = run.run_id.clone();
                        self.loading = true;
                        Some(TuiMsg::ResearchOpenRun { run_id })
                    } else {
                        None
                    }
                }
                crossterm::event::KeyCode::Char('r') if key.modifiers.is_empty() => {
                    self.loading = true;
                    Some(TuiMsg::ResearchRefreshRuns)
                }
                crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
                _ => None,
            },
            ResearchBrowserMode::RunDetail => match key.code {
                crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                    if key.modifiers.is_empty() =>
                {
                    self.detail_select_up();
                    None
                }
                crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                    if key.modifiers.is_empty() =>
                {
                    let max = self.available_report_sections().len();
                    self.detail_select_down(max);
                    None
                }
                crossterm::event::KeyCode::Enter => {
                    let sections = self.available_report_sections();
                    if let Some((label, _section)) = sections.get(self.detail_selected) {
                        let section_label = label.to_string();
                        let bundle_run_id = self
                            .detail
                            .as_ref()
                            .map(|b| b.request.id.clone())
                            .unwrap_or_default();
                        Some(TuiMsg::ResearchLoadSection {
                            run_id: bundle_run_id,
                            section: section_label,
                        })
                    } else {
                        None
                    }
                }
                crossterm::event::KeyCode::Esc => {
                    self.mode = ResearchBrowserMode::RunsList;
                    self.detail = None;
                    None
                }
                _ => None,
            },
            ResearchBrowserMode::ReportView => match key.code {
                crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                    if key.modifiers.is_empty() =>
                {
                    self.content_scroll = self.content_scroll.saturating_add(1);
                    None
                }
                crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                    if key.modifiers.is_empty() =>
                {
                    self.content_scroll = self.content_scroll.saturating_sub(1);
                    None
                }
                crossterm::event::KeyCode::PageUp => {
                    self.content_scroll = self.content_scroll.saturating_add(10);
                    None
                }
                crossterm::event::KeyCode::PageDown => {
                    self.content_scroll = self.content_scroll.saturating_sub(10);
                    None
                }
                crossterm::event::KeyCode::Home => {
                    self.content_scroll = usize::MAX;
                    None
                }
                crossterm::event::KeyCode::End => {
                    self.content_scroll = 0;
                    None
                }
                crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Backspace => {
                    self.report_content = None;
                    self.mode = ResearchBrowserMode::RunDetail;
                    self.content_scroll = 0;
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

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        if area.height < 5 {
            return;
        }
        self.theme = Arc::clone(theme);

        match self.mode {
            ResearchBrowserMode::RunsList => self.render_runs_list(frame, area),
            ResearchBrowserMode::RunDetail => self.render_run_detail(frame, area),
            ResearchBrowserMode::ReportView => self.render_report_view(frame, area),
        }
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::ResearchBrowser
    }
}

impl ResearchBrowserDialog {
    fn render_runs_list(&self, frame: &mut Frame, area: Rect) {
        let title = if self.loading {
            " Research Runs (loading...) "
        } else {
            " Research Runs "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(title);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.runs.is_empty() {
            let empty_lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No research runs found",
                    Style::default().fg(self.theme.muted),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Run /research <question> to start one",
                    Style::default().fg(self.theme.muted),
                )),
            ];
            let paragraph = Paragraph::new(empty_lines);
            frame.render_widget(paragraph, inner);
            return;
        }

        let visible_height = inner.height as usize;
        let scroll = if self.selected >= visible_height.saturating_sub(1) {
            self.selected - visible_height + 2
        } else {
            0
        };

        let items: Vec<ListItem> = self
            .runs
            .iter()
            .enumerate()
            .skip(scroll)
            .map(|(i, run)| {
                let is_selected = i == self.selected;
                let style = if is_selected {
                    Style::default()
                        .bg(self.theme.selection)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.foreground)
                };

                let short_id: String = run.run_id.chars().take(8).collect();
                let status_icon = match run.status {
                    RunStatus::Completed => ("done", self.theme.success),
                    RunStatus::Failed => ("FAIL", self.theme.error),
                    _ => ("run", self.theme.warning),
                };

                let question_display = if run.question.len() > 50 {
                    format!("{}...", truncate_prefix(&run.question, 50))
                } else {
                    run.question.clone()
                };

                let line = Line::from(vec![
                    Span::styled(
                        format!(" {} ", status_icon.0),
                        Style::default().fg(status_icon.1),
                    ),
                    Span::styled(short_id.clone(), Style::default().fg(self.theme.muted)),
                    Span::styled("  ", Style::default()),
                    Span::styled(question_display, style),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);

        let footer = Line::from(Span::styled(
            "j/k navigate  |  Enter: details  |  r: refresh  |  Esc close",
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

    fn render_run_detail(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(" Research Run Detail ");

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(bundle) = &self.detail else {
            if self.loading {
                let msg = Line::from(Span::styled(
                    "  Loading...",
                    Style::default().fg(self.theme.muted),
                ));
                let msg_area = Rect::new(inner.x, inner.y, inner.width, 1);
                frame.render_widget(msg, msg_area);
            } else {
                let msg = Line::from(Span::styled(
                    "  No run selected",
                    Style::default().fg(self.theme.muted),
                ));
                let msg_area = Rect::new(inner.x, inner.y, inner.width, 1);
                frame.render_widget(msg, msg_area);
            }
            return;
        };

        let mut lines: Vec<Line> = Vec::new();

        // Header info
        lines.push(Line::from(vec![
            Span::styled("Question: ", Style::default().fg(self.theme.muted)),
            Span::styled(
                &bundle.request.question,
                Style::default().fg(self.theme.foreground),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().fg(self.theme.muted)),
            Span::styled(
                format!("{:?}", bundle.status.status),
                Style::default().fg(match bundle.status.status {
                    RunStatus::Completed => self.theme.success,
                    RunStatus::Failed => self.theme.error,
                    _ => self.theme.warning,
                }),
            ),
            Span::styled("  Mode: ", Style::default().fg(self.theme.muted)),
            Span::styled(
                format!("{:?}", bundle.request.mode),
                Style::default().fg(self.theme.foreground),
            ),
            Span::styled("  Depth: ", Style::default().fg(self.theme.muted)),
            Span::styled(
                format!("{:?}", bundle.request.depth),
                Style::default().fg(self.theme.foreground),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Counts: ", Style::default().fg(self.theme.muted)),
            Span::styled(
                format!(
                    "{} sources, {} evidence, {} claims, {} contradictions",
                    bundle.status.counts.sources,
                    bundle.status.counts.evidence_spans,
                    bundle.status.counts.claims,
                    bundle.status.counts.contradictions
                ),
                Style::default().fg(self.theme.foreground),
            ),
        ]));

        if let Some(ref plan) = bundle.plan {
            lines.push(Line::from(vec![
                Span::styled("Plan: ", Style::default().fg(self.theme.muted)),
                Span::styled(
                    plan.scope.clone(),
                    Style::default().fg(self.theme.foreground),
                ),
            ]));
        }

        lines.push(Line::from(""));

        // Sources summary
        if !bundle.sources.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  Sources ({})", bundle.sources.len()),
                Style::default()
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            for src in bundle.sources.iter().take(3) {
                let title = src.title.as_deref().unwrap_or(&src.uri);
                let display = if title.len() > 60 {
                    format!("    {}...", truncate_prefix(title, 60))
                } else {
                    format!("    {}", title)
                };
                lines.push(Line::from(Span::styled(
                    display,
                    Style::default().fg(self.theme.foreground),
                )));
            }
            if bundle.sources.len() > 3 {
                lines.push(Line::from(Span::styled(
                    format!("    ... and {} more", bundle.sources.len() - 3),
                    Style::default().fg(self.theme.muted),
                )));
            }
        }

        // Claims summary
        if !bundle.claims.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  Claims ({})", bundle.claims.len()),
                Style::default()
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            for claim in bundle.claims.iter().take(3) {
                let conf_color = match claim.confidence {
                    Confidence::High => self.theme.success,
                    Confidence::Medium => self.theme.warning,
                    Confidence::Low => self.theme.error,
                };
                let display = if claim.text.len() > 60 {
                    format!(
                        "    [{}] {}...",
                        claim.claim_type.as_str(),
                        truncate_prefix(&claim.text, 60)
                    )
                } else {
                    format!("    [{}] {}", claim.claim_type.as_str(), claim.text)
                };
                lines.push(Line::from(vec![
                    Span::styled(display, Style::default().fg(self.theme.foreground)),
                    Span::styled(
                        format!(" ({:?})", claim.confidence),
                        Style::default().fg(conf_color),
                    ),
                ]));
            }
        }

        // Contradictions
        if !bundle.contradictions.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  Contradictions ({})", bundle.contradictions.len()),
                Style::default()
                    .fg(self.theme.warning)
                    .add_modifier(Modifier::BOLD),
            )));
            for c in bundle.contradictions.iter().take(2) {
                let display = if c.description.len() > 60 {
                    format!("    {}...", truncate_prefix(&c.description, 60))
                } else {
                    format!("    {}", c.description)
                };
                lines.push(Line::from(Span::styled(
                    display,
                    Style::default().fg(self.theme.foreground),
                )));
            }
        }

        // Report sections (selectable)
        let sections = self.available_report_sections();
        if !sections.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Available Sections",
                Style::default()
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            for (i, (label, _)) in sections.iter().enumerate() {
                let is_sel = i == self.detail_selected;
                let prefix = if is_sel { "> " } else { "  " };
                let style = if is_sel {
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.foreground)
                };
                lines.push(Line::from(Span::styled(
                    format!("{}{}", prefix, label),
                    style,
                )));
            }
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);

        let footer = Line::from(Span::styled(
            " j/k navigate  |  Enter: open section  |  Esc: back to list",
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

    fn render_report_view(&self, frame: &mut Frame, area: Rect) {
        let title = match &self.report_content {
            Some((section, _)) => match section {
                ReportSection::Report => " Research Plan ",
                ReportSection::Brief => " Sources ",
                ReportSection::AgentAnswer => " Claims ",
                ReportSection::Handoff => " Contradictions ",
            },
            None => " Content ",
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(title);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some((_, content)) = &self.report_content else {
            let msg = Line::from(Span::styled(
                "  No content to display",
                Style::default().fg(self.theme.muted),
            ));
            let msg_area = Rect::new(inner.x, inner.y, inner.width, 1);
            frame.render_widget(msg, msg_area);
            return;
        };

        let lines: Vec<Line> = content
            .lines()
            .map(|line| {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(self.theme.foreground),
                ))
            })
            .collect();

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.content_scroll as u16, 0));
        frame.render_widget(paragraph, inner);

        let footer = Line::from(Span::styled(
            " j/k: scroll  PgUp/PgDn: page  Esc: back",
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
