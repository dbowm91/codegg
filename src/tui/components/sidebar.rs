use crate::session::events::AgentPlan;
use crate::session::Session;
use crate::tui::app::state::session::DiffStatsState;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
    Widget,
};
use std::sync::Arc;

use super::super::app::TodoEntry;
use super::super::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Goal,
    Plan,
    Todos,
    FileChanges,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HoveredElement {
    Section(SidebarSection),
    Todo(usize),
    McpServer(usize),
    FileChange(usize),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarFileChange {
    pub path: String,
    pub action: String,
    pub diff_preview: Vec<String>,
    pub diff_state: DiffStatsState,
}

pub struct SidebarWidget {
    pub theme: Arc<Theme>,
    pub session: Option<Session>,
    pub agent: String,
    pub provider: String,
    pub model: String,
    pub todos: Vec<TodoEntry>,
    pub mcp_servers: Vec<(String, String)>,
    pub file_changes: Vec<SidebarFileChange>,
    pub git_branch: Option<String>,
    pub git_dirty: bool,
    pub project_root: Option<String>,
    pub goal: Option<String>,
    pub plan: Option<AgentPlan>,
    scroll_offset: usize,
    goal_collapsed: bool,
    plan_collapsed: bool,
    todos_collapsed: bool,
    file_changes_collapsed: bool,
    hovered_element: HoveredElement,
    tooltip_text: String,
}

impl SidebarWidget {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            session: None,
            agent: String::new(),
            provider: String::new(),
            model: String::new(),
            todos: Vec::new(),
            mcp_servers: Vec::new(),
            file_changes: Vec::new(),
            git_branch: None,
            git_dirty: false,
            project_root: None,
            goal: None,
            plan: None,
            scroll_offset: 0,
            goal_collapsed: false,
            plan_collapsed: false,
            todos_collapsed: false,
            file_changes_collapsed: false,
            hovered_element: HoveredElement::None,
            tooltip_text: String::new(),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_session(&mut self, session: &Session) {
        self.session = Some(session.clone());
    }

    pub fn set_agent(&mut self, agent: &str) {
        self.agent = agent.to_string();
    }

    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
    }

    pub fn set_provider(&mut self, provider: &str) {
        self.provider = provider.to_string();
    }

    pub fn set_todos(&mut self, todos: Vec<TodoEntry>) {
        self.todos = todos;
    }

    pub fn set_mcp_servers(&mut self, servers: Vec<(String, String)>) {
        self.mcp_servers = servers;
    }

    pub fn set_file_changes(&mut self, changes: Vec<SidebarFileChange>) {
        self.file_changes = changes;
    }

    pub fn set_git_info(&mut self, branch: Option<String>, dirty: bool, root: Option<String>) {
        self.git_branch = branch;
        self.git_dirty = dirty;
        self.project_root = root;
    }

    pub fn set_goal(&mut self, goal: Option<String>) {
        self.goal = goal;
    }

    pub fn set_plan(&mut self, plan: Option<AgentPlan>) {
        self.plan = plan;
    }

    pub fn toggle_focused(&mut self) {}

    pub fn focus_next(&mut self) {}

    pub fn focus_prev(&mut self) {}

    pub fn focused_name(&self) -> Option<&str> {
        None
    }

    pub fn scroll_up(&mut self, area: Rect) {
        self.scroll_offset = self.scroll_offset.saturating_sub(scroll_step(area));
    }

    pub fn scroll_down(&mut self, area: Rect) {
        self.scroll_offset = (self.scroll_offset + scroll_step(area)).min(self.max_scroll(area));
    }

    pub fn toggle_hovered_section(&mut self) -> bool {
        let HoveredElement::Section(section) = self.hovered_element else {
            return false;
        };

        match section {
            SidebarSection::Goal => self.goal_collapsed = !self.goal_collapsed,
            SidebarSection::Plan => self.plan_collapsed = !self.plan_collapsed,
            SidebarSection::Todos => self.todos_collapsed = !self.todos_collapsed,
            SidebarSection::FileChanges => {
                self.file_changes_collapsed = !self.file_changes_collapsed;
            }
        }
        true
    }

    pub fn max_scroll(&self, area: Rect) -> usize {
        let viewport_height = sidebar_content_height(area) as usize;
        self.content_lines(area.width)
            .len()
            .saturating_sub(viewport_height)
    }

    pub fn set_hover_position(&mut self, x: u16, y: u16, area: Option<Rect>) {
        if let Some(area) = area {
            let rel_y = y.saturating_sub(area.y);
            let rel_x = x.saturating_sub(area.x);

            if rel_x > 0 && rel_x < area.width && rel_y > 0 && rel_y < area.height {
                self.hovered_element = self.element_at(rel_x, rel_y);
                self.tooltip_text = self.get_tooltip_for_hover();
            } else {
                self.hovered_element = HoveredElement::None;
                self.tooltip_text.clear();
            }
        }
    }

    pub fn clear_hover(&mut self) {
        self.hovered_element = HoveredElement::None;
        self.tooltip_text.clear();
    }

    pub fn get_tooltip(&self) -> Option<&str> {
        if self.tooltip_text.is_empty() {
            None
        } else {
            Some(&self.tooltip_text)
        }
    }

    fn element_at(&self, _x: u16, y: u16) -> HoveredElement {
        let visible_y = y.saturating_sub(1) as usize;
        self.line_targets()
            .get(self.scroll_offset + visible_y)
            .cloned()
            .unwrap_or(HoveredElement::None)
    }

    fn line_targets(&self) -> Vec<HoveredElement> {
        let mut targets = Vec::new();

        targets.push(HoveredElement::None);
        if self.session.is_some() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::None);
            if self
                .session
                .as_ref()
                .and_then(|s| s.share_url.as_ref())
                .is_some()
            {
                targets.push(HoveredElement::None);
            }
        } else {
            targets.push(HoveredElement::None);
        }

        targets.push(HoveredElement::None);
        targets.push(HoveredElement::None);
        targets.push(HoveredElement::None);
        if self.git_branch.is_some() && self.git_dirty {
            targets.push(HoveredElement::None);
        }

        targets.push(HoveredElement::None);
        targets.push(HoveredElement::None);
        targets.push(HoveredElement::None);
        if !self.provider.is_empty() {
            targets.push(HoveredElement::None);
        }
        targets.push(HoveredElement::None);

        if self.goal.is_some() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::Section(SidebarSection::Goal));
            if !self.goal_collapsed {
                targets.push(HoveredElement::None);
            }
        }

        if let Some(ref plan) = self.plan {
            if !plan.items.is_empty() {
                targets.push(HoveredElement::None);
                targets.push(HoveredElement::Section(SidebarSection::Plan));
                if !self.plan_collapsed {
                    for _ in &plan.items {
                        targets.push(HoveredElement::None);
                    }
                }
            }
        }

        if !self.todos.is_empty() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::Section(SidebarSection::Todos));
            if !self.todos_collapsed {
                for i in 0..self.todos.len() {
                    targets.push(HoveredElement::Todo(i));
                }
            }
        }

        if !self.mcp_servers.is_empty() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::None);
            for i in 0..self.mcp_servers.len() {
                targets.push(HoveredElement::McpServer(i));
            }
        }

        if !self.file_changes.is_empty() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::Section(SidebarSection::FileChanges));
            if !self.file_changes_collapsed {
                for i in 0..self.file_changes.len() {
                    targets.push(HoveredElement::FileChange(i));
                }
            }
        }

        targets
    }

    fn content_lines(&self, area_width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let width = area_width as usize;

        if let Some(sess) = &self.session {
            let title = clean_inline_text(&sess.title, width);
            lines.push(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(vec![
                Span::styled("id: ", Style::default().fg(self.theme.muted)),
                Span::raw(sess.id.chars().take(8).collect::<String>()),
            ]));
            if let Some(ref url) = sess.share_url {
                lines.push(Line::from(vec![
                    Span::styled("shared: ", Style::default().fg(self.theme.success)),
                    Span::raw(clean_inline_text(url, width.saturating_sub(10))),
                ]));
            }
        } else {
            lines.push(self.section_header(" Session "));
            lines.push(Line::from(Span::styled(
                "no session",
                Style::default().fg(self.theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(" Git "));
        if let Some(ref branch) = self.git_branch {
            lines.push(Line::from(vec![
                Span::styled("branch: ", Style::default().fg(self.theme.muted)),
                Span::raw(clean_inline_text(branch, width.saturating_sub(10))),
            ]));
            if self.git_dirty {
                lines.push(Line::from(vec![
                    Span::styled("status: ", Style::default().fg(self.theme.warning)),
                    Span::raw("dirty"),
                ]));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "not a git repo",
                Style::default().fg(self.theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(" Config "));
        lines.push(Line::from(vec![
            Span::styled("agent: ", Style::default().fg(self.theme.muted)),
            Span::raw(clean_inline_text(&self.agent, width.saturating_sub(9))),
        ]));
        if !self.provider.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("provider: ", Style::default().fg(self.theme.muted)),
                Span::raw(clean_inline_text(&self.provider, width.saturating_sub(12))),
            ]));
        }
        let model_short = self.model.split('/').next_back().unwrap_or(&self.model);
        lines.push(Line::from(vec![
            Span::styled("model: ", Style::default().fg(self.theme.muted)),
            Span::raw(clean_inline_text(model_short, width.saturating_sub(9))),
        ]));

        if self.goal.is_some() {
            lines.push(Line::from(""));
            lines.push(self.collapsible_header(" Goal ", self.goal_collapsed));
            if let Some(ref goal) = self.goal {
                if !self.goal_collapsed {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", clean_inline_text(goal, width.saturating_sub(4))),
                        Style::default().fg(self.theme.foreground),
                    )));
                }
            }
        }

        if let Some(ref plan) = self.plan {
            if !plan.items.is_empty() {
                lines.push(Line::from(""));
                lines.push(self.collapsible_header(
                    &format!(" Plan ({}) ", plan.items.len()),
                    self.plan_collapsed,
                ));
                if !self.plan_collapsed {
                    for item in &plan.items {
                        let (icon, style) = match item.status {
                            crate::session::events::PlanItemStatus::Done => {
                                ("[x]", Style::default().fg(self.theme.success))
                            }
                            crate::session::events::PlanItemStatus::InProgress => {
                                ("[>]", Style::default().fg(self.theme.warning))
                            }
                            crate::session::events::PlanItemStatus::Skipped => {
                                ("[-]", Style::default().fg(self.theme.muted))
                            }
                            crate::session::events::PlanItemStatus::Blocked => {
                                ("[?]", Style::default().fg(self.theme.error))
                            }
                            crate::session::events::PlanItemStatus::Pending => {
                                ("[ ]", Style::default().fg(self.theme.muted))
                            }
                        };
                        lines.push(Line::from(vec![
                            Span::styled(format!("  {} ", icon), style),
                            Span::styled(
                                clean_inline_text(&item.text, width.saturating_sub(8)),
                                Style::default().fg(self.theme.foreground),
                            ),
                        ]));
                    }
                }
            }
        }

        if !self.todos.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.collapsible_header(
                &format!(" Todos ({}) ", self.todos.len()),
                self.todos_collapsed,
            ));
            if !self.todos_collapsed {
                for todo in &self.todos {
                    let status_icon = match todo.status.as_str() {
                        "completed" => "[x]",
                        "in_progress" => "[>]",
                        _ => "[ ]",
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{status_icon} "),
                            Style::default().fg(self.theme.muted),
                        ),
                        Span::raw(clean_inline_text(&todo.content, width.saturating_sub(6))),
                    ]));
                }
            }
        }

        if !self.mcp_servers.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.section_header(" MCP Servers "));
            for (name, status) in &self.mcp_servers {
                let dot = match status.as_str() {
                    "connected" => "*",
                    "connecting" => "~",
                    "error" => "!",
                    _ => "-",
                };
                let dot_style = match status.as_str() {
                    "connected" => Style::default().fg(self.theme.success),
                    "connecting" => Style::default().fg(self.theme.warning),
                    "error" => Style::default().fg(self.theme.error),
                    _ => Style::default().fg(self.theme.muted),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{dot} "), dot_style),
                    Span::raw(clean_inline_text(name, width.saturating_sub(4))),
                ]));
            }
        }

        if !self.file_changes.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.collapsible_header(
                &format!(" Modified Files ({}) ", self.file_changes.len()),
                self.file_changes_collapsed,
            ));
            if !self.file_changes_collapsed {
                for change in &self.file_changes {
                    let (stats_text, stats_style) = match &change.diff_state {
                        DiffStatsState::Ready {
                            additions,
                            deletions,
                            ..
                        } => (format!("+{additions} -{deletions}"), Style::default()),
                        DiffStatsState::Pending { .. } => {
                            ("diff...".to_string(), Style::default().fg(self.theme.muted))
                        }
                        DiffStatsState::Skipped { reason, .. } => {
                            (reason.to_string(), Style::default().fg(self.theme.muted))
                        }
                        DiffStatsState::Error { .. } => (
                            "diff err".to_string(),
                            Style::default().fg(self.theme.error),
                        ),
                    };
                    let stats_width = stats_text.len() + 1;
                    let mut spans = vec![
                        Span::styled(
                            format!("  {} ", clean_inline_text(&change.action, 1)),
                            Style::default().fg(self.theme.warning),
                        ),
                        Span::raw(clean_inline_text(
                            &change.path,
                            width.saturating_sub(6 + stats_width),
                        )),
                        Span::raw(" "),
                    ];
                    // For Ready state, split into colored +/- spans.
                    if let DiffStatsState::Ready {
                        additions,
                        deletions,
                        ..
                    } = &change.diff_state
                    {
                        spans.push(Span::styled(
                            format!("+{additions}"),
                            Style::default().fg(self.theme.success),
                        ));
                        spans.push(Span::raw(" "));
                        spans.push(Span::styled(
                            format!("-{deletions}"),
                            Style::default().fg(self.theme.error),
                        ));
                    } else {
                        spans.push(Span::styled(stats_text, stats_style));
                    }
                    lines.push(Line::from(spans));
                }
            }
        }

        lines
    }

    fn get_tooltip_for_hover(&self) -> String {
        match &self.hovered_element {
            HoveredElement::Section(section) => match section {
                SidebarSection::Goal => "Click to collapse/expand goal".to_string(),
                SidebarSection::Plan => "Click to collapse/expand plan".to_string(),
                SidebarSection::Todos => "Click to collapse/expand todos".to_string(),
                SidebarSection::FileChanges => {
                    "Click to collapse/expand modified files".to_string()
                }
            },
            HoveredElement::Todo(idx) => self
                .todos
                .get(*idx)
                .map(|todo| format!("Todo: {} [{}]", todo.content, todo.status))
                .unwrap_or_default(),
            HoveredElement::McpServer(idx) => self
                .mcp_servers
                .get(*idx)
                .map(|(name, status)| format!("MCP Server: {} ({})", name, status))
                .unwrap_or_default(),
            HoveredElement::FileChange(idx) => self
                .file_changes
                .get(*idx)
                .map(|change| {
                    let stats_str = match &change.diff_state {
                        DiffStatsState::Ready {
                            additions,
                            deletions,
                            ..
                        } => {
                            format!("+{additions} -{deletions}")
                        }
                        DiffStatsState::Pending { .. } => "computing diff...".to_string(),
                        DiffStatsState::Skipped { reason, .. } => format!("skipped: {reason}"),
                        DiffStatsState::Error { message, .. } => format!("error: {message}"),
                    };
                    format!(
                        "Modified: {} ({}, {})",
                        change.path, change.action, stats_str
                    )
                })
                .unwrap_or_default(),
            HoveredElement::None => String::new(),
        }
    }

    fn section_header(&self, label: &str) -> Line<'static> {
        // Section titles share the muted (placeholder) text color so the
        // sidebar reads with a single text hue. `theme.primary` is often
        // a near-background accent in Halloy themes (e.g. Cyber Red's
        // #230202) which renders the title invisible.
        let style = Style::default()
            .fg(self.theme.muted)
            .add_modifier(Modifier::BOLD);
        Line::from(Span::styled(label.to_string(), style))
    }

    fn collapsible_header(&self, label: &str, collapsed: bool) -> Line<'static> {
        let marker = if collapsed { "[+]" } else { "[-]" };
        let style = Style::default()
            .fg(self.theme.muted)
            .add_modifier(Modifier::BOLD);
        Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(self.theme.muted)),
            Span::styled(label.to_string(), style),
        ])
    }
}

impl Default for SidebarWidget {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &SidebarWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let lines = self.content_lines(area.width);
        let scroll_offset = self.scroll_offset.min(self.max_scroll(area));

        let block = Block::default()
            .title(" Sidebar ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .scroll((scroll_offset as u16, 0));
        paragraph.render(area, buf);

        if self.max_scroll(area) > 0 {
            let mut state =
                ScrollbarState::new(self.content_lines(area.width).len()).position(scroll_offset);
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(self.theme.foreground))
                .track_style(Style::default().fg(self.theme.border))
                .begin_symbol(None)
                .end_symbol(None)
                .render(area, buf, &mut state);
        }

        if let Some(tooltip) = self.get_tooltip() {
            let tooltip_width = (tooltip.len() as u16 + 4).min(area.width.saturating_sub(3));
            let tooltip_x = area.x + 1;
            let tooltip_y = area.y + area.height.saturating_sub(2);

            if tooltip_y > area.y {
                let tooltip_area = Rect::new(tooltip_x, tooltip_y, tooltip_width, 2);
                let tooltip_block = Block::default()
                    .border_style(Style::default().fg(self.theme.border))
                    .borders(Borders::ALL)
                    .style(Style::default().bg(self.theme.background));

                let tooltip_text =
                    Paragraph::new(tooltip).style(Style::default().fg(self.theme.foreground));

                tooltip_block.render(tooltip_area, buf);
                tooltip_text.render(
                    Rect::new(
                        tooltip_area.x + 1,
                        tooltip_area.y + 1,
                        tooltip_area.width.saturating_sub(2),
                        1,
                    ),
                    buf,
                );
            }
        }
    }
}

fn scroll_step(area: Rect) -> usize {
    ((sidebar_content_height(area) as usize) / 3).max(1)
}

fn sidebar_content_height(area: Rect) -> u16 {
    area.height.saturating_sub(2)
}

pub fn clean_inline_text(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch == '\n' || ch == '\r' || ch.is_control() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        out.push(ch);
    }

    if max_chars == 0 {
        return String::new();
    }

    let count = out.chars().count();
    if count <= max_chars {
        return out;
    }

    if max_chars <= 1 {
        return "...".chars().take(max_chars).collect();
    }

    let keep = max_chars.saturating_sub(1);
    format!("{}…", out.chars().take(keep).collect::<String>())
}
