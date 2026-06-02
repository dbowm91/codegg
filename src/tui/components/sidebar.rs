use crate::session::events::AgentPlan;
use crate::session::Session;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use std::sync::Arc;

use super::super::app::TodoEntry;
use super::super::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub enum HoveredElement {
    Todo(usize),
    McpServer(usize),
    FileChange(usize),
    None,
}

pub struct SidebarWidget {
    pub theme: Arc<Theme>,
    pub session: Option<Session>,
    pub agent: String,
    pub model: String,
    pub token_in: u64,
    pub token_out: u64,
    pub status: String,
    pub todos: Vec<TodoEntry>,
    pub mcp_servers: Vec<(String, String)>,
    pub file_changes: Vec<String>,
    pub git_branch: Option<String>,
    pub git_dirty: bool,
    pub project_root: Option<String>,
    pub goal: Option<String>,
    pub plan: Option<AgentPlan>,
    pub context_pct: u64,
    hovered_element: HoveredElement,
    tooltip_text: String,
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1000 {
        format!("{}K", tokens / 1000)
    } else {
        tokens.to_string()
    }
}

impl SidebarWidget {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            session: None,
            agent: String::new(),
            model: String::new(),
            token_in: 0,
            token_out: 0,
            status: "idle".to_string(),
            todos: Vec::new(),
            mcp_servers: Vec::new(),
            file_changes: Vec::new(),
            git_branch: None,
            git_dirty: false,
            project_root: None,
            goal: None,
            plan: None,
            context_pct: 0,
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

    pub fn set_tokens(&mut self, input: u64, output: u64) {
        self.token_in = input;
        self.token_out = output;
    }

    pub fn set_status(&mut self, status: String) {
        self.status = status;
    }

    pub fn set_todos(&mut self, todos: Vec<TodoEntry>) {
        self.todos = todos;
    }

    pub fn set_mcp_servers(&mut self, servers: Vec<(String, String)>) {
        self.mcp_servers = servers;
    }

    pub fn set_file_changes(&mut self, paths: Vec<String>) {
        self.file_changes = paths;
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

    pub fn set_context_pct(&mut self, pct: u64) {
        self.context_pct = pct;
    }

    pub fn toggle_focused(&mut self) {}

    pub fn focus_next(&mut self) {}

    pub fn focus_prev(&mut self) {}

    pub fn focused_name(&self) -> Option<&str> {
        None
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
        let mut line_num = 1u16;

        // Session header + content
        line_num += 1; // blank
        if self.session.is_some() {
            line_num += 4; // title, id, shared, blank
        } else {
            line_num += 1; // "no session"
        }

        // Git header + content
        line_num += 1; // blank
        if self.git_branch.is_some() {
            line_num += if self.git_dirty { 2 } else { 1 };
        } else {
            line_num += 1;
        }

        // Config header + content
        line_num += 1; // blank
        line_num += 2; // agent, model

        // Goal header + content
        line_num += 1; // blank
        line_num += 1; // goal text or "(none)"

        // Plan header + content
        line_num += 1; // blank
        if let Some(ref plan) = self.plan {
            if plan.items.is_empty() {
                line_num += 1; // "(empty)"
            } else {
                for _item in &plan.items {
                    if y == line_num {
                        return HoveredElement::None;
                    }
                    line_num += 1;
                }
            }
        } else {
            line_num += 1; // "(no plan)"
        }

        // Tokens header + content
        line_num += 1; // blank
        line_num += 4; // in, out, total, context

        // Todos header + items
        if !self.todos.is_empty() {
            line_num += 1; // blank
            for i in 0..self.todos.len() {
                if y == line_num {
                    return HoveredElement::Todo(i);
                }
                line_num += 1;
            }
        }

        // MCP Servers header + items
        if !self.mcp_servers.is_empty() {
            line_num += 1; // blank
            for i in 0..self.mcp_servers.len() {
                if y == line_num {
                    return HoveredElement::McpServer(i);
                }
                line_num += 1;
            }
        }

        // File Changes header + items
        if !self.file_changes.is_empty() {
            line_num += 1; // blank
            for i in 0..self.file_changes.len() {
                if y == line_num {
                    return HoveredElement::FileChange(i);
                }
                line_num += 1;
            }
        }

        HoveredElement::None
    }

    fn get_tooltip_for_hover(&self) -> String {
        match &self.hovered_element {
            HoveredElement::Todo(idx) => {
                if let Some(todo) = self.todos.get(*idx) {
                    format!("Todo: {} [{}]", todo.content, todo.status)
                } else {
                    String::new()
                }
            }
            HoveredElement::McpServer(idx) => {
                if let Some((name, status)) = self.mcp_servers.get(*idx) {
                    format!("MCP Server: {} ({})", name, status)
                } else {
                    String::new()
                }
            }
            HoveredElement::FileChange(idx) => {
                if let Some(path) = self.file_changes.get(*idx) {
                    format!("Modified: {}", path)
                } else {
                    String::new()
                }
            }
            HoveredElement::None => String::new(),
        }
    }

    fn section_header<'a>(&self, label: &'a str) -> Line<'a> {
        let style = Style::default()
            .fg(self.theme.muted)
            .add_modifier(Modifier::BOLD);
        Line::from(Span::styled(label, style))
    }
}

impl Default for SidebarWidget {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &SidebarWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(self.section_header(" Session "));
        lines.push(Line::from(""));
        if let Some(sess) = &self.session {
            lines.push(Line::from(vec![
                Span::styled("title: ", Style::default().fg(self.theme.muted)),
                Span::raw(&sess.title),
            ]));
            lines.push(Line::from(vec![
                Span::styled("id: ", Style::default().fg(self.theme.muted)),
                Span::raw(&sess.id[..8.min(sess.id.len())]),
            ]));
            if let Some(ref url) = sess.share_url {
                lines.push(Line::from(vec![
                    Span::styled("shared: ", Style::default().fg(self.theme.success)),
                    Span::raw(url),
                ]));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "no session",
                Style::default().fg(self.theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(" Git "));
        lines.push(Line::from(""));
        if let Some(ref branch) = self.git_branch {
            lines.push(Line::from(vec![
                Span::styled("branch: ", Style::default().fg(self.theme.muted)),
                Span::raw(branch),
            ]));
            if self.git_dirty {
                lines.push(Line::from(vec![
                    Span::styled("status: ", Style::default().fg(self.theme.warning)),
                    Span::raw("✗ dirty"),
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
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("agent: ", Style::default().fg(self.theme.muted)),
            Span::raw(&self.agent),
        ]));
        let model_short = self.model.split('/').next_back().unwrap_or(&self.model);
        lines.push(Line::from(vec![
            Span::styled("model: ", Style::default().fg(self.theme.muted)),
            Span::raw(model_short),
        ]));

        lines.push(Line::from(""));
        lines.push(self.section_header(" Goal "));
        lines.push(Line::from(""));
        if let Some(ref goal) = self.goal {
            let display = if goal.len() > (area.width as usize).saturating_sub(4) {
                format!("{}…", &goal[..(area.width as usize).saturating_sub(5)])
            } else {
                goal.clone()
            };
            lines.push(Line::from(Span::styled(
                format!("  {}", display),
                Style::default().fg(self.theme.foreground),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(self.theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(" Plan "));
        lines.push(Line::from(""));
        if let Some(ref plan) = self.plan {
            if plan.items.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (empty)",
                    Style::default().fg(self.theme.muted),
                )));
            } else {
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
                    let text = if item.text.len() > (area.width as usize).saturating_sub(8) {
                        format!(
                            "{}…",
                            &item.text[..(area.width as usize).saturating_sub(9)]
                        )
                    } else {
                        item.text.clone()
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {} ", icon), style),
                        Span::styled(text, Style::default().fg(self.theme.foreground)),
                    ]));
                }
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  (no plan)",
                Style::default().fg(self.theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(" Tokens "));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("in:  ", Style::default().fg(self.theme.muted)),
            Span::raw(format_tokens(self.token_in)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("out: ", Style::default().fg(self.theme.muted)),
            Span::raw(format_tokens(self.token_out)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("total: ", Style::default().fg(self.theme.muted)),
            Span::raw(format_tokens(self.token_in + self.token_out)),
        ]));
        let ctx_style = if self.context_pct > 80 {
            Style::default().fg(self.theme.error)
        } else if self.context_pct > 60 {
            Style::default().fg(self.theme.warning)
        } else {
            Style::default().fg(self.theme.muted)
        };
        lines.push(Line::from(vec![
            Span::styled("ctx: ", Style::default().fg(self.theme.muted)),
            Span::styled(format!("{}%", self.context_pct), ctx_style),
        ]));

        if !self.todos.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.section_header(" Todos "));
            lines.push(Line::from(""));
            for todo in &self.todos {
                let status_icon = match todo.status.as_str() {
                    "completed" => "✓",
                    "pending" => "○",
                    _ => "○",
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{status_icon} "),
                        Style::default().fg(self.theme.muted),
                    ),
                    Span::raw(&todo.content),
                ]));
            }
        }

        if !self.mcp_servers.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.section_header(" MCP Servers "));
            lines.push(Line::from(""));
            for (name, status) in &self.mcp_servers {
                let dot = match status.as_str() {
                    "connected" => "●",
                    "connecting" => "◐",
                    "error" => "✗",
                    _ => "○",
                };
                let dot_style = match status.as_str() {
                    "connected" => Style::default().fg(self.theme.success),
                    "connecting" => Style::default().fg(self.theme.warning),
                    "error" => Style::default().fg(self.theme.error),
                    _ => Style::default().fg(self.theme.muted),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{dot} "), dot_style),
                    Span::raw(name),
                ]));
            }
        }

        if !self.file_changes.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.section_header(" File Changes "));
            lines.push(Line::from(""));
            for path in &self.file_changes {
                lines.push(Line::from(vec![
                    Span::styled("  M ", Style::default().fg(self.theme.warning)),
                    Span::raw(path),
                ]));
            }
        }

        let block = Block::default()
            .title(" Sidebar ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, buf);

        if let Some(tooltip) = self.get_tooltip() {
            let tooltip_width = (tooltip.len() as u16 + 4).min(area.width.saturating_sub(3));
            let tooltip_x = area.x + 1;
            let tooltip_y = area.y + area.height.saturating_sub(2);

            if tooltip_y > area.y {
                let tooltip_area = Rect::new(tooltip_x, tooltip_y, tooltip_width, 2);
                let tooltip_block = Block::default()
                    .border_style(Style::default().fg(self.theme.primary))
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
