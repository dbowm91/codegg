use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::tui::wrap::wrap_count;
use crate::util::truncate::truncate_prefix;

use crate::research::service::ResearchRunSummary;
use crate::research::types::{Confidence, ResearchBundle, RunStatus};

/// Who owns this render's chrome.
///
/// The two paths differ only in that. [`Chrome::Owned`] is the modal box, which
/// draws its own border, heading and keymap footer per mode; [`Chrome::Hosted`]
/// is a column in a side panel, where the host has already painted all three
/// into the same rect.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chrome {
    Owned,
    Hosted,
}

/// The verbs `ResearchBrowserMode::RunsList` delivers, for the panel host.
///
/// `handle_key` also answers `↑/↓` and `Esc`, and neither belongs in a hosted
/// hint: the arrows are claimed by `insert_panel_direction_bindings` as panel
/// directions, and `Esc` is the mode switch — it means "close" in the modal
/// box and something else in a column. The old footer's `Esc close` was
/// therefore dead the moment this box docked.
const RUNS_LIST_HINT: &str = "j/k move · enter open · r refresh";

/// The verbs `ResearchBrowserMode::RunDetail` delivers.
///
/// No `Esc`: this mode's `Esc` steps back to the run list, and in a column it
/// is the host's exit instead — so a docked detail box has no way back but
/// `Shift+←`, which the host says so itself.
const RUN_DETAIL_HINT: &str = "j/k move · enter section";

/// The verbs `ResearchBrowserMode::ReportView` delivers.
///
/// `Home`/`End` are named because they are **not** panel directions and do
/// reach a widget from a column. `⌫` is named rather than `Esc` for the same
/// reason the review diff view names it: `Esc` is the host's, `⌫` is not.
const REPORT_VIEW_HINT: &str = "j/k scroll · ^d/^u page · Home/End jump · ⌫ back";

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
    pub fn new() -> Self {
        Self {
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
    /// A plain viewer box: no composer, no live filter, no mode of its own,
    /// so the panel host may resolve `Shift+←/→/↑/↓` as the docking gesture
    /// before this dialog sees the key.
    fn docks_as_viewer(&self) -> bool {
        {
            true
        }
    }
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

    /// The verbs this box owns **in the mode it is currently in**, spelled the
    /// way this box spells them.
    ///
    /// `handle_key` is mode-dispatched and the three modes share nothing: `r`
    /// refreshes the run list and means nothing in a report, `⌫` is the way
    /// back out of a report and means nothing anywhere else, and `Home`/`End`
    /// jump a report only. One string would have to describe keys that are
    /// dead in the mode on screen.
    fn keymap_hint(&self) -> Option<String> {
        Some(match self.mode {
            ResearchBrowserMode::RunsList => RUNS_LIST_HINT.to_string(),
            ResearchBrowserMode::RunDetail => RUN_DETAIL_HINT.to_string(),
            ResearchBrowserMode::ReportView => REPORT_VIEW_HINT.to_string(),
        })
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    /// The listing alone: no border, no title, no footer.
    ///
    /// The host has already painted a border, a title and the `●` focus marker
    /// into this same rect, and `render_in_panel` is handed the full rect
    /// rather than the interior, so a second border does not stack — it
    /// overwrites all three, in every mode, including the two early-return
    /// branches. `area` is the host's full rect, so inset by its border first.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, Chrome::Hosted);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::ResearchBrowser
    }
}

impl ResearchBrowserDialog {
    /// One body routine for all three modes and both render paths.
    ///
    /// The three footers are the reason it is one routine. Each mode had its
    /// own footer naming `Esc` and `↑/↓`, and a docked column destroyed all
    /// three — while the only copy a docked reader could see was the host's
    /// generic `"j/k scroll · i act"`, which is wrong for two of the three
    /// modes. The verbs moved to [`Component::keymap_hint`]; what stayed
    /// behind is a border and a title per mode, and both go with the host's.
    ///
    /// The report's scroll clamp is the other thing this routine exists to
    /// own. `content_scroll` was passed to `Paragraph::scroll` unclamped, so
    /// `Home` — which stores `usize::MAX` — reached ratatui as `u16::MAX` and
    /// rendered the report blank, with an offset that only grows, leaving
    /// `j`/`k` no way back. **That is a deliberate behaviour change**, and the
    /// only one: `Home` now lands on the last row the rect can show instead of
    /// past it. It is applied here rather than in `handle_key` because only
    /// this routine can see the rect, and doing it here is what stops the
    /// modal and the column reaching different last rows.
    fn render_body(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: Chrome) {
        if area.height < 5 {
            return;
        }

        match self.mode {
            ResearchBrowserMode::RunsList => {
                let title = if self.loading {
                    " Research Runs (loading...) "
                } else {
                    " Research Runs "
                };

                let block = match chrome {
                    Chrome::Owned => Some(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.border))
                            .title(title),
                    ),
                    Chrome::Hosted => None,
                };

                let inner = match &block {
                    Some(block) => block.inner(area),
                    None => area,
                };
                if let Some(block) = &block {
                    frame.render_widget(block, area);
                }

                if self.runs.is_empty() {
                    let empty_lines = vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            "  No research runs found",
                            Style::default().fg(theme.muted),
                        )),
                        Line::from(""),
                        Line::from(Span::styled(
                            "  Run /research <question> to start one",
                            Style::default().fg(theme.muted),
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
                                .bg(theme.selection)
                                .fg(theme.primary)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme.foreground)
                        };

                        let short_id: String = run.run_id.chars().take(8).collect();
                        let status_icon = match run.status {
                            RunStatus::Completed => ("done", theme.success),
                            RunStatus::Failed => ("FAIL", theme.error),
                            _ => ("run", theme.warning),
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
                            Span::styled(short_id.clone(), Style::default().fg(theme.muted)),
                            Span::styled("  ", Style::default()),
                            Span::styled(question_display, style),
                        ]);
                        ListItem::new(line)
                    })
                    .collect();

                let list = List::new(items);
                frame.render_widget(list, inner);

                if chrome == Chrome::Owned {
                    let footer = Line::from(Span::styled(
                        "j/k navigate  |  Enter: details  |  r: refresh  |  Esc close",
                        Style::default().fg(theme.muted),
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
            ResearchBrowserMode::RunDetail => {
                let block = match chrome {
                    Chrome::Owned => Some(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.border))
                            .title(" Research Run Detail "),
                    ),
                    Chrome::Hosted => None,
                };

                let inner = match &block {
                    Some(block) => block.inner(area),
                    None => area,
                };
                if let Some(block) = &block {
                    frame.render_widget(block, area);
                }

                let Some(bundle) = &self.detail else {
                    if self.loading {
                        let msg = Line::from(Span::styled(
                            "  Loading...",
                            Style::default().fg(theme.muted),
                        ));
                        let msg_area = Rect::new(inner.x, inner.y, inner.width, 1);
                        frame.render_widget(msg, msg_area);
                    } else {
                        let msg = Line::from(Span::styled(
                            "  No run selected",
                            Style::default().fg(theme.muted),
                        ));
                        let msg_area = Rect::new(inner.x, inner.y, inner.width, 1);
                        frame.render_widget(msg, msg_area);
                    }
                    return;
                };

                let mut lines: Vec<Line> = Vec::new();

                // Header info
                lines.push(Line::from(vec![
                    Span::styled("Question: ", Style::default().fg(theme.muted)),
                    Span::styled(
                        &bundle.request.question,
                        Style::default().fg(theme.foreground),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(theme.muted)),
                    Span::styled(
                        format!("{:?}", bundle.status.status),
                        Style::default().fg(match bundle.status.status {
                            RunStatus::Completed => theme.success,
                            RunStatus::Failed => theme.error,
                            _ => theme.warning,
                        }),
                    ),
                    Span::styled("  Mode: ", Style::default().fg(theme.muted)),
                    Span::styled(
                        format!("{:?}", bundle.request.mode),
                        Style::default().fg(theme.foreground),
                    ),
                    Span::styled("  Depth: ", Style::default().fg(theme.muted)),
                    Span::styled(
                        format!("{:?}", bundle.request.depth),
                        Style::default().fg(theme.foreground),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Counts: ", Style::default().fg(theme.muted)),
                    Span::styled(
                        format!(
                            "{} sources, {} evidence, {} claims, {} contradictions",
                            bundle.status.counts.sources,
                            bundle.status.counts.evidence_spans,
                            bundle.status.counts.claims,
                            bundle.status.counts.contradictions
                        ),
                        Style::default().fg(theme.foreground),
                    ),
                ]));

                if let Some(ref plan) = bundle.plan {
                    lines.push(Line::from(vec![
                        Span::styled("Plan: ", Style::default().fg(theme.muted)),
                        Span::styled(plan.scope.clone(), Style::default().fg(theme.foreground)),
                    ]));
                }

                lines.push(Line::from(""));

                // Sources summary
                if !bundle.sources.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("  Sources ({})", bundle.sources.len()),
                        Style::default()
                            .fg(theme.primary)
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
                            Style::default().fg(theme.foreground),
                        )));
                    }
                    if bundle.sources.len() > 3 {
                        lines.push(Line::from(Span::styled(
                            format!("    ... and {} more", bundle.sources.len() - 3),
                            Style::default().fg(theme.muted),
                        )));
                    }
                }

                // Claims summary
                if !bundle.claims.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("  Claims ({})", bundle.claims.len()),
                        Style::default()
                            .fg(theme.primary)
                            .add_modifier(Modifier::BOLD),
                    )));
                    for claim in bundle.claims.iter().take(3) {
                        let conf_color = match claim.confidence {
                            Confidence::High => theme.success,
                            Confidence::Medium => theme.warning,
                            Confidence::Low => theme.error,
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
                            Span::styled(display, Style::default().fg(theme.foreground)),
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
                            .fg(theme.warning)
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
                            Style::default().fg(theme.foreground),
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
                            .fg(theme.primary)
                            .add_modifier(Modifier::BOLD),
                    )));
                    for (i, (label, _)) in sections.iter().enumerate() {
                        let is_sel = i == self.detail_selected;
                        let prefix = if is_sel { "> " } else { "  " };
                        let style = if is_sel {
                            Style::default()
                                .fg(theme.primary)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme.foreground)
                        };
                        lines.push(Line::from(Span::styled(
                            format!("{}{}", prefix, label),
                            style,
                        )));
                    }
                }

                let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, inner);

                if chrome == Chrome::Owned {
                    let footer = Line::from(Span::styled(
                        " j/k navigate  |  Enter: open section  |  Esc: back to list",
                        Style::default().fg(theme.muted),
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
            ResearchBrowserMode::ReportView => {
                let title = match &self.report_content {
                    Some((section, _)) => match section {
                        ReportSection::Report => " Research Plan ",
                        ReportSection::Brief => " Sources ",
                        ReportSection::AgentAnswer => " Claims ",
                        ReportSection::Handoff => " Contradictions ",
                    },
                    None => " Content ",
                };

                let block = match chrome {
                    Chrome::Owned => Some(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(theme.border))
                            .title(title),
                    ),
                    Chrome::Hosted => None,
                };

                let inner = match &block {
                    Some(block) => block.inner(area),
                    None => area,
                };
                if let Some(block) = &block {
                    frame.render_widget(block, area);
                }

                let Some((_, content)) = &self.report_content else {
                    let msg = Line::from(Span::styled(
                        "  No content to display",
                        Style::default().fg(theme.muted),
                    ));
                    let msg_area = Rect::new(inner.x, inner.y, inner.width, 1);
                    frame.render_widget(msg, msg_area);
                    return;
                };

                // The rows the paragraph is about to be handed, counted with
                // the same wrapper the chat viewport and `InfoDialog` use, and
                // against the width of the rect actually being filled. A line
                // that wraps occupies several rows while `content_scroll`
                // still advances by one, so counting logical lines here would
                // clamp against the wrong quantity and leave the tail of a
                // wrapped report below the fold.
                let mut total_rows = 0usize;
                let lines: Vec<Line> = content
                    .lines()
                    .map(|line| {
                        total_rows += wrap_count(line, inner.width);
                        Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(theme.foreground),
                        ))
                    })
                    .collect();

                // The modal footer takes the last interior row, so only the
                // rows above it are ever visible content; a hosted column has
                // no footer row at all. Deriving this from `inner` in one
                // place is what stops the two paths reaching different last
                // rows from one shared clamp.
                let visible_rows = match chrome {
                    Chrome::Owned => (inner.height as usize).saturating_sub(1),
                    Chrome::Hosted => inner.height as usize,
                };
                // `.scroll()` consumes a `u16`, so the ceiling is capped there
                // too: a report of more than 65535 rows is unreachable rather
                // than wrapping the offset back to the top of the document.
                let max_scroll = total_rows
                    .saturating_sub(visible_rows)
                    .min(u16::MAX as usize);
                let scroll = self.content_scroll.min(max_scroll);

                let paragraph = Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll as u16, 0));
                frame.render_widget(paragraph, inner);

                if chrome == Chrome::Owned {
                    let footer = Line::from(Span::styled(
                        " j/k: scroll  PgUp/PgDn: page  Esc: back",
                        Style::default().fg(theme.muted),
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
        }
    }
}

/// These constructors lost their theme parameter when the render body stopped
/// caching one, which leaves `new()` shaped like `Default` — so it has one.
impl Default for ResearchBrowserDialog {
    fn default() -> Self {
        Self::new()
    }
}
