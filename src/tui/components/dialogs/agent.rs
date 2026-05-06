use std::cell::RefCell;
use std::sync::Arc;

use crate::agent::Agent;
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

#[allow(unused_macros)]
#[cfg(feature = "debug-logging")]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("codegg_debug.log")
            .and_then(|mut file| {
                std::io::Write::write_all(&mut file, format!($($arg)*).as_bytes())
            });
    };
}

#[allow(unused_macros)]
#[cfg(not(feature = "debug-logging"))]
macro_rules! debug_log {
    ($($arg:tt)*) => {};
}

pub struct AgentDialog {
    pub theme: Arc<Theme>,
    pub agents: Vec<Agent>,
    pub current: String,
    pub selected: usize,
    pub scroll: CenteredScroll,
    pub filter: String,
    visible_height: usize,
    filtered_cache: RefCell<Option<(String, Vec<usize>)>>,
}

impl Clone for AgentDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            agents: self.agents.clone(),
            current: self.current.clone(),
            selected: self.selected,
            scroll: self.scroll.clone(),
            filter: self.filter.clone(),
            visible_height: self.visible_height,
            filtered_cache: RefCell::new(self.filtered_cache.borrow().clone()),
        }
    }
}

impl AgentDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            agents: Vec::new(),
            current: String::new(),
            selected: 0,
            scroll: CenteredScroll::new(),
            filter: String::new(),
            visible_height: 10,
            filtered_cache: RefCell::new(None),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_agents(&mut self, agents: Vec<&Agent>) {
        self.agents = agents.into_iter().cloned().collect();
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn set_current(&mut self, current: &str) {
        self.current = current.to_string();
    }

    pub fn selected(&self) -> Option<String> {
        self.filtered().get(self.selected).map(|a| a.name.clone())
    }

    pub fn select_up(&mut self) {
        let filtered_len = self.filtered().len();
        if self.selected > 0 {
            self.selected -= 1;
        }
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll
                .clamp(self.selected, filtered_len, self.visible_height);
        }
    }

    pub fn select_down(&mut self) {
        let filtered_len = self.filtered().len();
        let max = filtered_len.saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll
                .clamp(self.selected, filtered_len, self.visible_height);
        }
    }

    pub fn set_filter(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.scroll.reset();
        self.update_cache();
    }

    pub fn backspace_filter(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.scroll.reset();
        self.update_cache();
    }

    fn update_cache(&mut self) {
        if self.filter.is_empty() {
            self.filtered_cache.borrow_mut().take();
            return;
        }
        let indices: Vec<usize> = self
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| a.name.to_lowercase().contains(&self.filter.to_lowercase()))
            .map(|(i, _)| i)
            .collect();
        self.filtered_cache
            .borrow_mut()
            .replace((self.filter.clone(), indices));
    }

    pub fn initialize_selection(&mut self, current: &str) {
        if let Some(idx) = self.agents.iter().position(|a| a.name == current) {
            self.selected = idx;
        }
        let filtered_len = self.filtered().len();
        if self.selected < self.scroll.get()
            || self.selected >= self.scroll.get() + self.visible_height
        {
            self.scroll
                .clamp(self.selected, filtered_len, self.visible_height);
        }
    }

    fn filtered(&self) -> Vec<&Agent> {
        if self.filter.is_empty() {
            self.agents.iter().collect()
        } else {
            if let Some((ref cache_filter, ref indices)) = self.filtered_cache.borrow().as_ref() {
                if cache_filter == &self.filter {
                    return indices.iter().map(|&i| &self.agents[i]).collect();
                }
            }
            self.agents
                .iter()
                .filter(|a| a.name.to_lowercase().contains(&self.filter.to_lowercase()))
                .collect()
        }
    }
}

impl Default for AgentDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Component for AgentDialog {
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
            crossterm::event::KeyCode::Enter => self
                .selected()
                .map(|agent_name| TuiMsg::SelectAgent { agent_name }),
            crossterm::event::KeyCode::Char(c) => {
                self.set_filter(c);
                None
            }
            crossterm::event::KeyCode::Backspace => {
                self.backspace_filter();
                None
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

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.set_theme(theme);
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Select Agent ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if !self.filter.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("filter: ", Style::default().fg(theme.muted)),
                Span::raw(&self.filter),
            ]));
            lines.push(Line::from(""));
        }

        let filtered = self.filtered();
        let scroll = self.scroll.get();
        for (i, agent) in filtered.iter().enumerate() {
            if i < scroll {
                continue;
            }
            let is_current = agent.name == self.current;
            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.primary)
                    .bg(theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.foreground)
            };
            let marker = if is_current { "✓ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(theme.muted)),
                Span::styled(&agent.name, style),
                Span::styled(
                    format!(" — {}", agent.description),
                    Style::default().fg(theme.muted),
                ),
            ]));
        }

        if filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                "no agents match filter",
                Style::default().fg(theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑/↓ navigate  Enter select  Esc cancel",
            Style::default().fg(theme.muted),
        )));

        let block = Block::default()
            .title(" Agents ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Agent
    }
}
