use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::Widget;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::security::workflow::{
    filter_panel_items, project_receipt_to_panel_items, resolve_security_review_item_path,
    SecurityReviewFilter, SecurityReviewHunkLineKind, SecurityReviewPanelItem,
    SecurityReviewPanelItemKind, SecurityReviewReceipt,
};
use crate::tui::app::TuiMsg;

/// Which section of the detail pane is currently focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityReviewDetailSection {
    Summary,
    Hunk,
}

#[derive(Clone)]
pub struct SecurityReviewDialog {
    pub receipt: Box<Option<SecurityReviewReceipt>>,
    pub all_items: Vec<SecurityReviewPanelItem>,
    pub visible_items: Vec<SecurityReviewPanelItem>,
    pub selected_index: usize,
    pub detail_scroll: u16,
    pub detail_section: SecurityReviewDetailSection,
    pub filter: SecurityReviewFilter,
    pub theme: Arc<Theme>,
}

impl SecurityReviewDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            receipt: Box::new(None),
            all_items: Vec::new(),
            visible_items: Vec::new(),
            selected_index: 0,
            detail_scroll: 0,
            detail_section: SecurityReviewDetailSection::Summary,
            filter: SecurityReviewFilter::All,
            theme,
        }
    }

    pub fn with_receipt(theme: Arc<Theme>, receipt: SecurityReviewReceipt) -> Self {
        let all_items = project_receipt_to_panel_items(&receipt);
        let visible_items = filter_panel_items(&all_items, SecurityReviewFilter::All);
        Self {
            receipt: Box::new(Some(receipt)),
            all_items,
            visible_items,
            selected_index: 0,
            detail_scroll: 0,
            detail_section: SecurityReviewDetailSection::Summary,
            filter: SecurityReviewFilter::All,
            theme,
        }
    }

    pub fn update_receipt(&mut self, receipt: SecurityReviewReceipt) {
        self.all_items = project_receipt_to_panel_items(&receipt);
        self.recompute_visible();
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_receipt(&mut self, receipt: Option<SecurityReviewReceipt>) {
        *self.receipt = receipt;
        self.all_items = match *self.receipt {
            Some(ref receipt) => project_receipt_to_panel_items(receipt),
            None => Vec::new(),
        };
        self.recompute_visible();
    }

    fn recompute_visible(&mut self) {
        self.visible_items = filter_panel_items(&self.all_items, self.filter);
        if self.selected_index >= self.visible_items.len() {
            self.selected_index = self.visible_items.len().saturating_sub(1);
        }
        self.detail_scroll = 0;
    }

    pub fn selected_item(&self) -> Option<&SecurityReviewPanelItem> {
        self.visible_items.get(self.selected_index)
    }

    fn select_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            self.detail_scroll = 0;
        }
    }

    fn select_down(&mut self) {
        if self.selected_index + 1 < self.visible_items.len() {
            self.selected_index += 1;
            self.detail_scroll = 0;
        }
    }

    fn cycle_filter(&mut self) {
        self.filter = self.filter.next();
        self.recompute_visible();
    }

    /// Jump detail scroll to the first hunk line in the current detail rendering.
    fn jump_to_hunk(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        if item.hunk.is_none() {
            return;
        }
        // The hunk header appears after summary lines. Count lines to reach it.
        // Summary: prompt marker (0-1) + title + blank + location + summary + blank + details...
        // Hunk starts after a blank line following the last detail line.
        let mut line_count: u16 = 0;
        // prompt marker
        if item.kind == SecurityReviewPanelItemKind::Prompt {
            line_count += 1;
        }
        // title
        line_count += 1;
        // blank
        line_count += 1;
        // location (if present)
        if item.file_path.is_some() {
            line_count += 1;
        }
        // summary
        if !item.summary.is_empty() {
            line_count += 1;
        }
        // blank before details
        if !item.detail.is_empty() {
            line_count += 1;
        }
        // detail lines
        line_count += item.detail.len() as u16;
        // blank before hunk
        line_count += 1;
        // hunk header
        line_count += 1;
        // scroll to hunk header (first hunk line is at line_count - 1)
        self.detail_scroll = line_count.saturating_sub(1);
        self.detail_section = SecurityReviewDetailSection::Hunk;
    }

    /// Navigate to the next hunk-backed item (wraps around).
    fn select_next_hunk(&mut self) {
        if self.visible_items.is_empty() {
            return;
        }
        let start = self.selected_index;
        let mut idx = start;
        loop {
            idx = (idx + 1) % self.visible_items.len();
            if self.visible_items[idx].hunk.is_some() {
                self.selected_index = idx;
                self.detail_scroll = 0;
                self.detail_section = SecurityReviewDetailSection::Summary;
                return;
            }
            if idx == start {
                return; // wrapped all the way around
            }
        }
    }

    /// Navigate to the previous hunk-backed item (wraps around).
    fn select_prev_hunk(&mut self) {
        if self.visible_items.is_empty() {
            return;
        }
        let start = self.selected_index;
        let mut idx = start;
        loop {
            idx = if idx == 0 {
                self.visible_items.len() - 1
            } else {
                idx - 1
            };
            if self.visible_items[idx].hunk.is_some() {
                self.selected_index = idx;
                self.detail_scroll = 0;
                self.detail_section = SecurityReviewDetailSection::Summary;
                return;
            }
            if idx == start {
                return; // wrapped all the way around
            }
        }
    }

    fn counts(receipt: &SecurityReviewReceipt) -> (usize, usize, usize) {
        (
            receipt.output.findings.len(),
            receipt.output.review_prompts.len(),
            receipt.output.notes.len() + receipt.output.preflight_results.len(),
        )
    }
}

impl Component for SecurityReviewDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
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
            crossterm::event::KeyCode::PageDown => {
                self.detail_scroll = self.detail_scroll.saturating_add(5);
                None
            }
            crossterm::event::KeyCode::PageUp => {
                self.detail_scroll = self.detail_scroll.saturating_sub(5);
                None
            }
            crossterm::event::KeyCode::Char('f') if key.modifiers.is_empty() => {
                self.cycle_filter();
                None
            }
            crossterm::event::KeyCode::Char('n') if key.modifiers.is_empty() => {
                // Toggle notes-only filter for backwards compatibility.
                self.filter = if self.filter == SecurityReviewFilter::Notes {
                    SecurityReviewFilter::All
                } else {
                    SecurityReviewFilter::Notes
                };
                self.recompute_visible();
                None
            }
            crossterm::event::KeyCode::Char('p') if key.modifiers.is_empty() => {
                self.filter = if self.filter == SecurityReviewFilter::Prompts {
                    SecurityReviewFilter::All
                } else {
                    SecurityReviewFilter::Prompts
                };
                self.recompute_visible();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if let Some(item) = self.selected_item() {
                    if item.file_path.is_some() {
                        if let Some(ref receipt) = *self.receipt {
                            match resolve_security_review_item_path(receipt, item) {
                                Ok(resolved) => {
                                    let origin = match item.kind {
                                        SecurityReviewPanelItemKind::Finding => {
                                            Some("Security Review Finding".to_string())
                                        }
                                        SecurityReviewPanelItemKind::Prompt => {
                                            Some("Security Review Prompt".to_string())
                                        }
                                        _ => None,
                                    };
                                    return Some(TuiMsg::OpenSourcePreview {
                                        path: resolved,
                                        line: item.line,
                                        origin_label: origin,
                                    });
                                }
                                Err(_) => {
                                    return item.file_path.as_ref().map(|p| {
                                        TuiMsg::SecurityReviewJump {
                                            path: p.display().to_string(),
                                            line: item.line,
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
                None
            }
            // h: jump detail scroll to hunk section
            crossterm::event::KeyCode::Char('h') if key.modifiers.is_empty() => {
                self.jump_to_hunk();
                None
            }
            // H: copy hunk text to clipboard
            crossterm::event::KeyCode::Char('H') if key.modifiers.is_empty() => {
                if let Some(item) = self.selected_item() {
                    if let Some(ref hunk) = item.hunk {
                        let mut text = format!("{}\n", hunk.header);
                        for hl in &hunk.lines {
                            let prefix = match hl.kind {
                                SecurityReviewHunkLineKind::Added => "+",
                                SecurityReviewHunkLineKind::Removed => "-",
                                SecurityReviewHunkLineKind::Context => " ",
                            };
                            text.push_str(&format!("{}{}\n", prefix, hl.text));
                        }
                        if text.len() > 4096 {
                            text.truncate(4096);
                            text.push_str("\n... (truncated)");
                        }
                        let _ = crate::util::clipboard::copy_to_clipboard(&text);
                        return Some(TuiMsg::SecurityReviewJump {
                            path: String::new(),
                            line: None,
                        });
                    }
                }
                None
            }
            // ]: next hunk-backed item
            crossterm::event::KeyCode::Char(']') if key.modifiers.is_empty() => {
                self.select_next_hunk();
                None
            }
            // [: previous hunk-backed item
            crossterm::event::KeyCode::Char('[') if key.modifiers.is_empty() => {
                self.select_prev_hunk();
                None
            }
            crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                Some(TuiMsg::CloseDialog)
            }
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            TuiMsg::SecurityReviewJump { .. } => Some(msg),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _theme: &Arc<Theme>) {
        if area.height < 6 || area.width < 20 {
            return;
        }
        let Some(ref receipt) = *self.receipt else {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.border))
                .title(" Security Review ");
            let para = Paragraph::new(Line::from(Span::styled(
                "No security review result available.",
                Style::default().fg(self.theme.muted),
            )))
            .block(block);
            frame.render_widget(para, area);
            return;
        };

        let (findings, prompts, notes) = Self::counts(receipt);
        let enrichment = if receipt.enriched {
            "local-lsp"
        } else if !receipt.lsp_available {
            "unavailable"
        } else {
            "off"
        };
        let header_text = format!(
            "Security Review — {} | Findings: {} | Prompts: {} | Notes: {} | Enrichment: {}",
            receipt.root.display(),
            findings,
            prompts,
            notes,
            enrichment,
        );

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(3),
            ])
            .split(area);

        let header_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(" Security Review ");
        let header_para = Paragraph::new(Line::from(Span::styled(
            header_text,
            Style::default().fg(self.theme.foreground),
        )))
        .block(header_block);
        frame.render_widget(header_para, chunks[0]);

        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(chunks[1]);

        self.render_list(frame, split[0]);
        self.render_detail(frame, split[1]);

        let footer_text = format!(
            "j/k move | f filter ({}) | ]/[ hunk nav | h jump hunk | H copy hunk | Enter preview | Esc close",
            self.filter.label(),
        );
        let footer = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(self.theme.muted),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.border)),
        );
        frame.render_widget(footer, chunks[2]);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::SecurityReview
    }
}

impl SecurityReviewDialog {
    fn render_list(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(format!(" Items ({}) ", self.visible_items.len()));

        let inner = block.inner(area);
        block.render(area, frame.buffer_mut());

        if self.visible_items.is_empty() {
            let empty = Line::from(Span::styled(
                "No items match the current filter",
                Style::default().fg(self.theme.muted),
            ));
            let empty_area = Rect::new(inner.x, inner.y, inner.width, 1);
            frame.render_widget(empty, empty_area);
            return;
        }

        let items: Vec<ListItem> = self
            .visible_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let is_selected = i == self.selected_index;
                let base_style = if is_selected {
                    Style::default()
                        .bg(self.theme.selection)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.foreground)
                };
                let marker = match item.kind {
                    SecurityReviewPanelItemKind::Finding => {
                        let color = match item.severity {
                            Some(crate::security::workflow::SecuritySeverity::Critical) => {
                                self.theme.error
                            }
                            Some(crate::security::workflow::SecuritySeverity::High) => {
                                self.theme.error
                            }
                            Some(crate::security::workflow::SecuritySeverity::Medium) => {
                                self.theme.warning
                            }
                            Some(crate::security::workflow::SecuritySeverity::Low) => {
                                self.theme.secondary
                            }
                            Some(crate::security::workflow::SecuritySeverity::Info) => {
                                self.theme.muted
                            }
                            None => self.theme.foreground,
                        };
                        Span::styled("[FINDING] ", Style::default().fg(color))
                    }
                    SecurityReviewPanelItemKind::Prompt => Span::styled(
                        "⚠ [PROMPT] ",
                        Style::default()
                            .fg(self.theme.warning)
                            .add_modifier(Modifier::BOLD),
                    ),
                    SecurityReviewPanelItemKind::Note => {
                        Span::styled("[NOTE] ", Style::default().fg(self.theme.muted))
                    }
                    SecurityReviewPanelItemKind::Preflight => {
                        Span::styled("[PREFLIGHT] ", Style::default().fg(self.theme.warning))
                    }
                };
                Line::from(vec![marker, Span::styled(item.title.clone(), base_style)])
            })
            .map(ListItem::new)
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }

    fn render_detail(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(" Detail ");
        let inner = block.inner(area);
        block.render(area, frame.buffer_mut());

        let Some(item) = self.selected_item() else {
            let empty = Line::from(Span::styled(
                "No item selected",
                Style::default().fg(self.theme.muted),
            ));
            let empty_area = Rect::new(inner.x, inner.y, inner.width, 1);
            frame.render_widget(empty, empty_area);
            return;
        };

        let mut lines: Vec<Line> = Vec::new();

        if item.kind == SecurityReviewPanelItemKind::Prompt {
            lines.push(Line::from(Span::styled(
                "⚠ Review prompt only — not a confirmed finding",
                Style::default()
                    .fg(self.theme.warning)
                    .add_modifier(Modifier::BOLD),
            )));
        }

        lines.push(Line::from(Span::styled(
            item.title.clone(),
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::BOLD),
        )));

        lines.push(Line::from(""));

        if let Some(path) = &item.file_path {
            let line_str = item.line.map(|l| format!(":{}", l)).unwrap_or_default();
            lines.push(Line::from(Span::styled(
                format!("Location: {}{}", path.display(), line_str),
                Style::default().fg(self.theme.muted),
            )));
        }
        if !item.summary.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("Summary: {}", item.summary),
                Style::default().fg(self.theme.foreground),
            )));
        }

        if !item.detail.is_empty() {
            lines.push(Line::from(""));
        }

        for d in &item.detail {
            let color = if d.starts_with("Severity:") || d.starts_with("Confidence:") {
                self.theme.secondary
            } else if d.starts_with("Category:") {
                self.theme.muted
            } else if d.starts_with("Recommendation:") {
                self.theme.success
            } else if d.starts_with("Evidence") {
                self.theme.secondary
            } else if d.starts_with("Not a confirmed finding") {
                self.theme.warning
            } else if d.starts_with("Preset:") || d.starts_with("Rationale:") {
                self.theme.muted
            } else {
                self.theme.foreground
            };
            lines.push(Line::from(Span::styled(
                d.clone(),
                Style::default().fg(color),
            )));
        }

        if let Some(hunk) = &item.hunk {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("Hunk: {}", hunk.header),
                Style::default()
                    .fg(self.theme.secondary)
                    .add_modifier(Modifier::BOLD),
            )));
            for hunk_line in &hunk.lines {
                let (prefix, color) = match hunk_line.kind {
                    SecurityReviewHunkLineKind::Added => ("+", self.theme.success),
                    SecurityReviewHunkLineKind::Removed => ("-", self.theme.error),
                    SecurityReviewHunkLineKind::Context => (" ", self.theme.muted),
                };
                let line_num = match (hunk_line.old_line, hunk_line.new_line) {
                    (Some(o), Some(n)) => format!("{:>4} {:>4}", o, n),
                    (Some(o), None) => format!("{:>4}     ", o),
                    (None, Some(n)) => format!("     {:>4}", n),
                    (None, None) => "         ".to_string(),
                };
                let is_focus = item
                    .line
                    .is_some_and(|line| hunk_line.new_line == Some(line));
                let style = if is_focus {
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{line_num} {prefix} "),
                        Style::default().fg(self.theme.muted),
                    ),
                    Span::styled(hunk_line.text.clone(), style),
                ]));
            }
        }

        let visible_height = inner.height as usize;
        let scroll = self.detail_scroll as usize;
        let start = scroll.min(lines.len());
        let end = (start + visible_height).min(lines.len());
        let visible = if start < lines.len() {
            lines[start..end].to_vec()
        } else {
            Vec::new()
        };

        let para = Paragraph::new(visible);
        frame.render_widget(para, inner);
    }
}
