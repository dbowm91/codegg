use crate::session::Session;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use std::collections::HashMap;
use std::sync::Arc;

use super::super::app::TodoEntry;
use super::super::theme::Theme;

#[derive(Debug, Clone)]
struct Section {
    expanded: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HoveredElement {
    Section(String),
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
    sections: HashMap<String, Section>,
    section_order: Vec<String>,
    focused_idx: usize,
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
        let mut sections = HashMap::new();
        let section_order = vec![
            "session".to_string(),
            "git".to_string(),
            "config".to_string(),
            "tokens".to_string(),
            "todos".to_string(),
            "mcp".to_string(),
            "files".to_string(),
        ];
        for name in &section_order {
            sections.insert(name.clone(), Section { expanded: true });
        }
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
            sections,
            section_order,
            focused_idx: 0,
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

    pub fn toggle_section(&mut self, name: &str) {
        if let Some(section) = self.sections.get_mut(name) {
            section.expanded = !section.expanded;
        }
    }

    pub fn toggle_focused(&mut self) {
        let name = self.section_order.get(self.focused_idx).cloned();
        if let Some(name) = name {
            self.toggle_section(&name);
        }
    }

    pub fn focus_next(&mut self) {
        if !self.section_order.is_empty() {
            self.focused_idx = (self.focused_idx + 1) % self.section_order.len();
        }
    }

    pub fn focus_prev(&mut self) {
        if !self.section_order.is_empty() {
            self.focused_idx = if self.focused_idx == 0 {
                self.section_order.len() - 1
            } else {
                self.focused_idx - 1
            };
        }
    }

    pub fn focused_name(&self) -> Option<&str> {
        self.section_order.get(self.focused_idx).map(|s| s.as_str())
    }

    fn is_expanded(&self, name: &str) -> bool {
        self.sections.get(name).map(|s| s.expanded).unwrap_or(true)
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

        line_num += 1;
        if self.session.is_some() {
            line_num += 1;
            if self.is_expanded("session") {
                line_num += 4;
            }
        }

        line_num += 2;
        if self.is_expanded("config") {
            line_num += 2;
        }

        line_num += 2;
        if self.is_expanded("tokens") {
            line_num += 3;
        }

        if !self.todos.is_empty() {
            line_num += 2;
            if self.is_expanded("todos") {
                line_num += 1;
                for i in 0..self.todos.len() {
                    if y == line_num {
                        return HoveredElement::Todo(i);
                    }
                    line_num += 1;
                }
            }
        }

        if !self.mcp_servers.is_empty() {
            line_num += 2;
            if self.is_expanded("mcp") {
                line_num += 1;
                for i in 0..self.mcp_servers.len() {
                    if y == line_num {
                        return HoveredElement::McpServer(i);
                    }
                    line_num += 1;
                }
            }
        }

        if !self.file_changes.is_empty() {
            line_num += 2;
            if self.is_expanded("files") {
                line_num += 1;
                for i in 0..self.file_changes.len() {
                    if y == line_num {
                        return HoveredElement::FileChange(i);
                    }
                    line_num += 1;
                }
            }
        }

        if y <= 3 {
            for (i, name) in self.section_order.iter().enumerate() {
                let section_line = if i == 0 {
                    1
                } else {
                    let mut sum = 2u16;
                    for j in 0..i {
                        sum += 1;
                        if self.is_expanded(&self.section_order[j]) {
                            sum += self.expanded_height(&self.section_order[j]);
                        }
                    }
                    sum
                };
                if y == section_line {
                    return HoveredElement::Section(name.clone());
                }
            }
        }

        HoveredElement::None
    }

    fn expanded_height(&self, section: &str) -> u16 {
        match section {
            "session" => 5,
            "git" => 2,
            "config" => 3,
            "tokens" => 4,
            "todos" => 1 + self.todos.len() as u16,
            "mcp" => 1 + self.mcp_servers.len() as u16,
            "files" => 1 + self.file_changes.len() as u16,
            _ => 0,
        }
    }

    fn get_tooltip_for_hover(&self) -> String {
        match &self.hovered_element {
            HoveredElement::Section(name) => {
                format!("Click: Toggle {} section", name)
            }
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

    fn section_header(&self, label: &str, key: &str, focused: bool) -> Line<'_> {
        let arrow = if self.is_expanded(key) {
            "▼ "
        } else {
            "▶ "
        };
        let base_style = if focused {
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
                .fg(self.theme.muted)
                .add_modifier(Modifier::BOLD)
        };
        Line::from(Span::styled(format!("{}{label}", arrow), base_style))
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

        lines.push(self.section_header(
            " Session ",
            "session",
            self.focused_name() == Some("session"),
        ));
        if self.is_expanded("session") {
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
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(" Git ", "git", self.focused_name() == Some("git")));
        if self.is_expanded("git") {
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
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(
            " Config ",
            "config",
            self.focused_name() == Some("config"),
        ));
        if self.is_expanded("config") {
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
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(
            " Tokens ",
            "tokens",
            self.focused_name() == Some("tokens"),
        ));
        if self.is_expanded("tokens") {
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
        }

        if !self.todos.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.section_header(
                " Todos ",
                "todos",
                self.focused_name() == Some("todos"),
            ));
            if self.is_expanded("todos") {
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
        }

        if !self.mcp_servers.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.section_header(
                " MCP Servers ",
                "mcp",
                self.focused_name() == Some("mcp"),
            ));
            if self.is_expanded("mcp") {
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
        }

        if !self.file_changes.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.section_header(
                " File Changes ",
                "files",
                self.focused_name() == Some("files"),
            ));
            if self.is_expanded("files") {
                lines.push(Line::from(""));
                for path in &self.file_changes {
                    lines.push(Line::from(vec![
                        Span::styled("  M ", Style::default().fg(self.theme.warning)),
                        Span::raw(path),
                    ]));
                }
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
