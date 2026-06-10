use std::collections::HashSet;
use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;

use crate::tui::app::TuiMsg;

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,
    pub session_id: String,
    pub label: String,
    pub time_updated: i64,
    pub message_count: Option<usize>,
    pub is_current: bool,
    pub is_archived: bool,
    pub children: Vec<TreeNode>,
    pub depth: usize,
}

#[derive(Clone)]
pub struct TreeDialog {
    pub theme: Arc<Theme>,
    pub nodes: Vec<TreeNode>,
    pub selected: usize,
    pub flat: Vec<(TreeNode, usize)>,
    pub expanded: HashSet<String>,
    pub scroll: CenteredScroll,
    visible_height: usize,
    current_session_id: Option<String>,
}

impl TreeDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            nodes: Vec::new(),
            selected: 0,
            flat: Vec::new(),
            expanded: HashSet::new(),
            scroll: CenteredScroll::new(),
            visible_height: 10,
            current_session_id: None,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn load_nodes(&mut self, nodes: Vec<TreeNode>, current_session_id: Option<String>) {
        self.nodes = nodes;
        self.current_session_id = current_session_id;
        if self.nodes.is_empty() {
            self.nodes.push(TreeNode {
                id: "root".to_string(),
                session_id: "root".to_string(),
                label: "no session".to_string(),
                time_updated: 0,
                message_count: None,
                is_current: false,
                is_archived: false,
                children: Vec::new(),
                depth: 0,
            });
        }
        self.expanded.clear();
        if let Some(first) = self.nodes.first() {
            self.expanded.insert(first.session_id.clone());
        }
        self.flatten();
        self.selected = 0;
        self.scroll.reset();
    }

    pub fn toggle_expand(&mut self) {
        if let Some((node, _)) = self.flat.get(self.selected) {
            if self.expanded.contains(&node.session_id) {
                self.expanded.remove(&node.session_id);
            } else {
                self.expanded.insert(node.session_id.clone());
            }
            self.flatten();
        }
    }

    pub fn fork_selected(&mut self) -> Option<String> {
        if let Some((node, _)) = self.flat.get(self.selected) {
            return Some(node.session_id.clone());
        }
        None
    }

    fn flatten(&mut self) {
        self.flat.clear();
        let nodes: &[TreeNode] = &self.nodes;
        Self::flatten_nodes_to_flat(nodes, &self.expanded, &mut self.flat);
    }

    fn flatten_nodes_to_flat(
        nodes: &[TreeNode],
        expanded: &std::collections::HashSet<String>,
        flat: &mut Vec<(TreeNode, usize)>,
    ) {
        for node in nodes {
            let indent = 0;
            flat.push((node.clone(), indent));

            if expanded.contains(&node.session_id) {
                Self::flatten_nodes_to_flat(&node.children, expanded, flat);
            }
        }
    }

    #[allow(dead_code)]
    fn flatten_nodes(&mut self, nodes: &[TreeNode], depth: usize) {
        for node in nodes {
            let indent = depth;
            self.flat.push((node.clone(), indent));

            if self.expanded.contains(&node.session_id) {
                self.flatten_nodes(&node.children, depth + 1);
            }
        }
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll
                .clamp(self.selected, self.flat.len(), self.visible_height);
        }
    }

    pub fn select_down(&mut self) {
        let max = self.flat.len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll
                .clamp(self.selected, self.flat.len(), self.visible_height);
        }
    }
}

impl Default for TreeDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &TreeDialog {
    #[allow(clippy::incompatible_msrv)]
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Session Tree ",
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if self.flat.is_empty() {
            lines.push(Line::from(Span::styled(
                "no forks",
                Style::default().fg(self.theme.muted),
            )));
        } else {
            let scroll = self.scroll.get();
            let mut rendered = 0;
            for (i, (node, indent)) in self.flat.iter().enumerate() {
                if i < scroll {
                    continue;
                }
                if rendered >= self.visible_height {
                    break;
                }
                let is_selected = i == self.selected;
                let prefix = "  ".repeat(*indent);
                let connector = if *indent > 0 { "├─ " } else { "" };

                let current_marker = if node.is_current { "● " } else { "  " };
                let has_children = !node.children.is_empty();
                let expand_indicator = if has_children {
                    if self.expanded.contains(&node.session_id) {
                        "▼ "
                    } else {
                        "▶ "
                    }
                } else {
                    "  "
                };
                let fork_suffix = if node.depth > 0 { " (fork)" } else { "" };
                let archived_suffix = if node.is_archived { " [archived]" } else { "" };

                let bg = if i.is_multiple_of(2) {
                    self.theme.background
                } else {
                    self.theme.alternate_bg
                };
                let style = if is_selected {
                    self.theme.selection_style()
                } else if node.is_archived {
                    Style::default().fg(self.theme.muted).bg(bg)
                } else if node.is_current {
                    Style::default()
                        .fg(self.theme.primary)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.foreground).bg(bg)
                };

                let _msg_count = node
                    .message_count
                    .map(|c| format!(" [{} msgs]", c))
                    .unwrap_or_default();

                lines.push(Line::from(Span::styled(
                    format!(
                        "{}{}{}{}{}{}{}",
                        prefix,
                        connector,
                        current_marker,
                        expand_indicator,
                        node.label,
                        fork_suffix,
                        archived_suffix
                    ),
                    style,
                )));
                rendered += 1;
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑/↓ navigate  Enter select  Esc cancel  e expand  f fork",
            Style::default().fg(self.theme.muted),
        )));

        let block = Block::default()
            .title(" Tree ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}

impl Component for TreeDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.select_down();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if let Some((node, _)) = self.flat.get(self.selected) {
                    Some(TuiMsg::SelectTreeSession {
                        session_id: node.session_id.clone(),
                    })
                } else {
                    None
                }
            }
            crossterm::event::KeyCode::Char('e') => {
                self.toggle_expand();
                None
            }
            crossterm::event::KeyCode::Char('f') => {
                if let Some((node, _)) = self.flat.get(self.selected) {
                    Some(TuiMsg::ForkTreeSession {
                        session_id: node.session_id.clone(),
                    })
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

    #[allow(clippy::incompatible_msrv)]
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Session Tree ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if self.flat.is_empty() {
            lines.push(Line::from(Span::styled(
                "no forks",
                Style::default().fg(theme.muted),
            )));
        } else {
            let scroll = self.scroll.get();
            let mut rendered = 0;
            for (i, (node, indent)) in self.flat.iter().enumerate() {
                if i < scroll {
                    continue;
                }
                if rendered >= self.visible_height {
                    break;
                }
                let is_selected = i == self.selected;
                let prefix = "  ".repeat(*indent);
                let connector = if *indent > 0 { "├─ " } else { "" };

                let current_marker = if node.is_current { "● " } else { "  " };
                let has_children = !node.children.is_empty();
                let expand_indicator = if has_children {
                    if self.expanded.contains(&node.session_id) {
                        "▼ "
                    } else {
                        "▶ "
                    }
                } else {
                    "  "
                };
                let fork_suffix = if node.depth > 0 { " (fork)" } else { "" };
                let archived_suffix = if node.is_archived { " [archived]" } else { "" };

                let bg = if i.is_multiple_of(2) {
                    theme.background
                } else {
                    theme.alternate_bg
                };
                let style = if is_selected {
                    theme.selection_style()
                } else if node.is_archived {
                    Style::default().fg(theme.muted).bg(bg)
                } else if node.is_current {
                    Style::default()
                        .fg(theme.primary)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground).bg(bg)
                };

                lines.push(Line::from(Span::styled(
                    format!(
                        "{}{}{}{}{}{}{}",
                        prefix,
                        connector,
                        current_marker,
                        expand_indicator,
                        node.label,
                        fork_suffix,
                        archived_suffix
                    ),
                    style,
                )));
                rendered += 1;
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑/↓ navigate  Enter select  Esc cancel  e expand  f fork",
            Style::default().fg(theme.muted),
        )));

        let block = Block::default()
            .title(" Tree ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Tree
    }
}
