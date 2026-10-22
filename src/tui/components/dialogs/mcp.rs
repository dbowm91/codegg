use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::sync::Arc;

use crossterm::event::KeyEvent;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::util::truncate::truncate_prefix;

/// Who owns this render's chrome.
///
/// The two paths differ only in that. [`Chrome::Owned`] is the modal box, which
/// draws its own border on each pane plus a per-pane footer; [`Chrome::Hosted`]
/// is a column in a side panel, where the host has already painted the border,
/// the title and the `●` focus marker into the same rect — twice over, once per
/// pane.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chrome {
    Owned,
    Hosted,
}

/// The verbs this box owns, for the panel host to reuse verbatim.
///
/// It varies by mode, because the same key means two different things in the two
/// states and only one of them can be right at a time. `handle_key` below is the
/// implementation; this is the advertisement.
///
/// `↑/↓` are absent even though `handle_key` answers them: a focused panel
/// never sees them, because `SidePanelHost::handle_key` resolves the arrow keys
/// as panel directions before the widget is consulted. `Esc` is absent for a
/// different reason — it is the *mode* switch, and it means one thing in the
/// modal box (close) and another in a column (leave the widget's own mode), so
/// a hosted hint cannot promise it. So the footer these two strings replaced —
/// and it appeared twice, once per pane — was false in both panes the moment the
/// box docked.
///
/// The consequence worth stating plainly: `Esc` is the *only* way out of action
/// mode, so a docked `/mcp` can enter that mode and cannot leave it from inside
/// the column. That is the host-level exit gap milestone M002a recorded, not
/// something this widget can fix by choosing different words.
const HINT_BROWSE: &str = "j/k move · enter actions";
const HINT_ACTION: &str = "j/k move · enter run";

#[derive(Debug, Clone)]
pub struct McpServerInfo {
    pub name: String,
    pub status: String,
    pub status_error: Option<String>,
    pub server_type: String,
    pub tools: Vec<McpToolInfo>,
    pub resources: Vec<McpResourceInfo>,
    pub has_oauth: bool,
}

#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct McpResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone)]
pub struct McpDialog {
    pub servers: Vec<McpServerInfo>,
    pub selected: usize,
    pub action_mode: bool,
    pub selected_action: usize,
    pub browse_mode: BrowseMode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BrowseMode {
    None,
    Resources { selected: usize },
}

impl McpDialog {
    pub fn new() -> Self {
        Self {
            servers: Vec::new(),
            selected: 0,
            action_mode: false,
            selected_action: 0,
            browse_mode: BrowseMode::None,
        }
    }

    pub fn set_servers(&mut self, servers: Vec<McpServerInfo>) {
        let old_selected = self.selected_server().map(|s| s.name.clone());
        self.servers = servers;

        if let Some(selected_name) = old_selected {
            if let Some(new_idx) = self.servers.iter().position(|s| s.name == selected_name) {
                self.selected = new_idx;
            } else {
                self.selected = self.selected.min(self.servers.len().saturating_sub(1));
                self.action_mode = false;
                self.selected_action = 0;
            }
        } else {
            self.selected = 0;
            self.action_mode = false;
            self.selected_action = 0;
        }
        self.browse_mode = BrowseMode::None;
    }

    pub fn selected_server(&self) -> Option<&McpServerInfo> {
        self.servers.get(self.selected)
    }

    pub fn select_up(&mut self) {
        match &mut self.browse_mode {
            BrowseMode::Resources { selected } if self.action_mode => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            _ => {
                if self.action_mode {
                    if self.selected_action > 0 {
                        self.selected_action -= 1;
                    }
                } else if self.selected > 0 {
                    self.selected -= 1;
                }
            }
        }
    }

    pub fn select_down(&mut self) {
        if matches!(self.browse_mode, BrowseMode::Resources { .. }) {
            let max = self
                .servers
                .get(self.selected)
                .map(|s| s.resources.len())
                .unwrap_or(0);
            if let BrowseMode::Resources { selected } = &mut self.browse_mode {
                if *selected + 1 < max {
                    *selected += 1;
                }
            }
        } else if self.action_mode {
            let actions = self.available_actions();
            if self.selected_action + 1 < actions.len() {
                self.selected_action += 1;
            }
        } else {
            let count = self.servers.len();
            if count > 0 && self.selected + 1 < count {
                self.selected += 1;
            }
        }
    }

    pub fn enter_action_mode(&mut self) {
        if !self.servers.is_empty() {
            self.action_mode = true;
            self.selected_action = 0;
        }
    }

    pub fn exit_action_mode(&mut self) {
        self.action_mode = false;
        self.selected_action = 0;
        self.browse_mode = BrowseMode::None;
    }

    pub fn available_actions(&self) -> Vec<&'static str> {
        if let Some(server) = self.selected_server() {
            let mut actions = match server.status.as_str() {
                "connected" => vec![
                    "Browse Resources",
                    "Configure OAuth",
                    "Disconnect",
                    "Remove",
                ],
                "connecting" => vec!["Wait"],
                "error" => vec!["Configure OAuth", "Reconnect", "Remove"],
                _ => vec!["Configure OAuth", "Connect", "Remove"],
            };
            if !server.has_oauth {
                actions.retain(|a| *a != "Configure OAuth");
            }
            actions
        } else {
            vec![]
        }
    }

    pub fn selected_action_name(&self) -> Option<&'static str> {
        let actions = self.available_actions();
        actions.get(self.selected_action).copied()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    /// One routine for both render paths, so the two panes cannot disagree
    /// about who owns the chrome.
    ///
    /// `mcp` is the one box in this milestone that paints **two** bordered
    /// panes rather than one, which is what makes a partial fix so easy to
    /// write: a hosted `mcp` that dropped the outer box and kept the inner one
    /// still overwrote the host's title. Both blocks and both footers are
    /// behind `Chrome::Owned`, and `Chrome::Hosted` draws neither.
    fn render_body(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: Chrome) {
        let chunks = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        self.render_server_list(chunks[0], frame, theme, chrome);
        self.render_server_details(chunks[1], frame, theme, chrome);
    }

    fn render_server_list(
        &self,
        area: Rect,
        frame: &mut Frame,
        theme: &Arc<Theme>,
        chrome: Chrome,
    ) {
        let mut lines: Vec<Line> = Vec::new();

        if chrome == Chrome::Owned {
            lines.push(Line::from(Span::styled(
                " MCP Servers ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }

        if self.servers.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no servers configured)",
                Style::default().fg(theme.muted),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Use /mcps add to configure",
                Style::default().fg(theme.muted),
            )));
        } else {
            for (i, server) in self.servers.iter().enumerate() {
                let is_selected = i == self.selected && !self.action_mode;
                let status_icon = match server.status.as_str() {
                    "connected" => "●",
                    "connecting" => "◐",
                    "error" => "✗",
                    _ => "○",
                };
                let status_color = match server.status.as_str() {
                    "connected" => theme.success,
                    "connecting" => theme.warning,
                    "error" => theme.error,
                    _ => theme.muted,
                };

                let style = if is_selected {
                    Style::default()
                        .fg(theme.primary)
                        .bg(theme.selection)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground)
                };

                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(status_icon, Style::default().fg(status_color)),
                    Span::styled(" ", Style::default()),
                    Span::styled(&server.name, style),
                ]));
            }
        }

        if chrome == Chrome::Owned {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "↑/↓ navigate  |  Enter select  |  Esc close",
                Style::default().fg(theme.muted),
            )));

            let block = Block::default()
                .title(" Servers ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.background));

            let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, area);
            return;
        }

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme.background))
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn render_server_details(
        &self,
        area: Rect,
        frame: &mut Frame,
        theme: &Arc<Theme>,
        chrome: Chrome,
    ) {
        let mut lines: Vec<Line> = Vec::new();

        if chrome == Chrome::Owned {
            lines.push(Line::from(Span::styled(
                " Server Details ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }

        if let Some(server) = self.selected_server() {
            let status_icon = match server.status.as_str() {
                "connected" => "●",
                "connecting" => "◐",
                "error" => "✗",
                _ => "○",
            };
            let status_color = match server.status.as_str() {
                "connected" => theme.success,
                "connecting" => theme.warning,
                "error" => theme.error,
                _ => theme.muted,
            };
            let status_text = match server.status.as_str() {
                "connected" => "Connected",
                "connecting" => "Connecting",
                "error" => "Error",
                _ => "Disconnected",
            };

            lines.push(Line::from(vec![
                Span::styled("name:   ", Style::default().fg(theme.muted)),
                Span::raw(&server.name),
            ]));
            lines.push(Line::from(vec![
                Span::styled("type:   ", Style::default().fg(theme.muted)),
                Span::raw(&server.server_type),
            ]));
            lines.push(Line::from(vec![
                Span::styled("status: ", Style::default().fg(theme.muted)),
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::raw(format!(" {}", status_text)),
            ]));

            if let Some(ref error) = server.status_error {
                lines.push(Line::from(vec![
                    Span::styled("error:  ", Style::default().fg(theme.error)),
                    Span::raw(error),
                ]));
            }

            if server.has_oauth {
                lines.push(Line::from(vec![
                    Span::styled("oauth:  ", Style::default().fg(theme.muted)),
                    Span::styled("configured", Style::default().fg(theme.success)),
                ]));
            }

            lines.push(Line::from(""));

            if matches!(self.browse_mode, BrowseMode::Resources { .. }) {
                lines.push(Line::from(Span::styled(
                    " Resources ",
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));

                if server.resources.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "  (no resources available)",
                        Style::default().fg(theme.muted),
                    )));
                } else {
                    let selected_idx = match self.browse_mode {
                        BrowseMode::Resources { selected } => selected,
                        _ => 0,
                    };
                    for (i, resource) in server.resources.iter().enumerate() {
                        let is_selected = i == selected_idx;
                        let prefix = if is_selected { "> " } else { "  " };
                        let style = if is_selected {
                            Style::default()
                                .fg(theme.selection_text)
                                .bg(theme.selection)
                        } else {
                            Style::default().fg(theme.foreground)
                        };
                        lines.push(Line::from(vec![
                            Span::styled(prefix, style),
                            Span::styled(&resource.name, style),
                        ]));
                        if let Some(ref desc) = resource.description {
                            let d = if desc.len() > 50 {
                                format!("      {}...", truncate_prefix(desc, 50))
                            } else {
                                format!("      {}", desc)
                            };
                            lines.push(Line::from(Span::styled(
                                d,
                                Style::default().fg(theme.muted),
                            )));
                        }
                    }
                }
            } else {
                lines.push(Line::from(Span::styled(
                    " Tools ",
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));

                if server.tools.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "  (no tools available)",
                        Style::default().fg(theme.muted),
                    )));
                } else {
                    for tool in server.tools.iter().take(10) {
                        lines.push(Line::from(vec![
                            Span::styled("  ", Style::default()),
                            Span::styled(&tool.name, Style::default().fg(theme.foreground)),
                        ]));
                        if !tool.description.is_empty() {
                            let desc = if tool.description.len() > 60 {
                                format!("{}...", truncate_prefix(&tool.description, 60))
                            } else {
                                tool.description.clone()
                            };
                            lines.push(Line::from(vec![
                                Span::styled("      ", Style::default()),
                                Span::styled(desc, Style::default().fg(theme.muted)),
                            ]));
                        }
                    }
                    if server.tools.len() > 10 {
                        lines.push(Line::from(Span::styled(
                            format!("  ... and {} more", server.tools.len() - 10),
                            Style::default().fg(theme.muted),
                        )));
                    }
                }

                if !server.resources.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!(
                            "  {} resources available (Enter to browse)",
                            server.resources.len()
                        ),
                        Style::default().fg(theme.primary),
                    )));
                }
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Actions ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));

            let actions = self.available_actions();
            for (i, action) in actions.iter().enumerate() {
                let is_selected = self.action_mode && i == self.selected_action;
                let prefix = if is_selected { "> " } else { "  " };
                let style = if is_selected {
                    Style::default().fg(theme.accent_text).bg(theme.primary)
                } else {
                    Style::default().fg(theme.foreground)
                };
                lines.push(Line::from(Span::styled(
                    format!("{}{}", prefix, action),
                    style,
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  (select a server to view details)",
                Style::default().fg(theme.muted),
            )));
        }

        if chrome == Chrome::Owned {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "↑/↓ navigate  |  Enter select  |  Esc close",
                Style::default().fg(theme.muted),
            )));

            let block = Block::default()
                .title(" Details ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.background));

            let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, area);
            return;
        }

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(theme.background))
                .wrap(Wrap { trim: true }),
            area,
        );
    }
}

impl Default for McpDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for McpDialog {
    /// A plain viewer box: no composer, no live filter, no mode of its own,
    /// so the panel host may resolve `Shift+←/→/↑/↓` as the docking gesture
    /// before this dialog sees the key.
    fn docks_as_viewer(&self) -> bool {
        {
            true
        }
    }
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                if self.action_mode {
                    self.exit_action_mode();
                } else {
                    return Some(TuiMsg::CloseDialog);
                }
                None
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.select_down();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if self.action_mode {
                    if let Some(server) = self.selected_server() {
                        if let Some(action) = self.selected_action_name() {
                            return Some(TuiMsg::McpAction {
                                server_name: server.name.clone(),
                                action: action.to_string(),
                            });
                        }
                    }
                    None
                } else if !self.servers.is_empty() {
                    self.enter_action_mode();
                    None
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    /// The verbs this box owns in the state it is currently in.
    fn keymap_hint(&self) -> Option<String> {
        Some(
            if self.action_mode {
                HINT_ACTION
            } else {
                HINT_BROWSE
            }
            .to_string(),
        )
    }

    /// Both panes, with neither pane's chrome.
    ///
    /// The host has already painted a border, a ` ● MCP ` title and the `●`
    /// focus marker into this same rect, and `render_in_panel` is handed the
    /// full rect rather than the interior. This box paints two panes, so it
    /// could overwrite that chrome twice over; the panes keep their contents and
    /// lose their frames, headings and footers, which is what the host's own
    /// title and the status line above the composer now say instead. `area` is
    /// the host's full rect, so inset by its border first.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, Chrome::Hosted);
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Mcp
    }
}
