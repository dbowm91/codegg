use crossterm::event::KeyEvent;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Widget, Wrap};
use std::sync::Arc;

use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::tui::theme::Theme;

#[derive(Clone)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub requires_api_key: bool,
    pub env_var_name: Option<String>,
    pub base_url_example: Option<String>,
}

#[derive(Clone)]
pub struct ConnectDialog {
    pub providers: Vec<ProviderInfo>,
    pub selected: usize,
    pub scroll: usize,
    pub theme: Arc<Theme>,
    pub step: ConnectStep,
    pub api_key_input: String,
    pub cursor_pos: usize,
    pub error_message: Option<String>,
    pub list_state: ListState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectStep {
    SelectProvider,
    EnterApiKey,
}

impl ConnectDialog {
    pub fn new(providers: Vec<ProviderInfo>, theme: Arc<Theme>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            providers,
            selected: 0,
            scroll: 0,
            theme,
            step: ConnectStep::SelectProvider,
            api_key_input: String::new(),
            cursor_pos: 0,
            error_message: None,
            list_state,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn cursor_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.list_state.select(Some(self.selected));
            self.clamp_scroll();
        }
    }

    pub fn cursor_down(&mut self) {
        if self.selected < self.providers.len().saturating_sub(1) {
            self.selected += 1;
            self.list_state.select(Some(self.selected));
            self.clamp_scroll();
        }
    }

    fn clamp_scroll(&mut self) {
        let max_visible = 10usize;
        if self.selected >= self.scroll + max_visible {
            self.scroll = self.selected.saturating_sub(max_visible.saturating_sub(1));
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
    }

    pub fn select_provider(&mut self) -> Option<&ProviderInfo> {
        if self.selected < self.providers.len() {
            Some(&self.providers[self.selected])
        } else {
            None
        }
    }

    pub fn move_to_api_key_step(&mut self) {
        self.step = ConnectStep::EnterApiKey;
        self.api_key_input.clear();
        self.cursor_pos = 0;
        self.error_message = None;
    }

    pub fn back_to_provider_selection(&mut self) {
        self.step = ConnectStep::SelectProvider;
        self.api_key_input.clear();
        self.cursor_pos = 0;
        self.error_message = None;
    }

    pub fn insert_char(&mut self, c: char) {
        self.api_key_input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            let before = &self.api_key_input[..self.cursor_pos];
            let ch_len = before
                .chars()
                .next_back()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            let new_cursor = self.cursor_pos - ch_len;
            self.api_key_input.drain(new_cursor..self.cursor_pos);
            self.cursor_pos = new_cursor;
        }
    }

    pub fn get_api_key(&self) -> String {
        self.api_key_input.clone()
    }

    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
    }

    pub fn clear_error(&mut self) {
        self.error_message = None;
    }
}

impl Widget for &ConnectDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        match self.step {
            ConnectStep::SelectProvider => {
                let title = Line::from(vec![Span::styled(
                    " Connect to Provider ",
                    Style::default().add_modifier(Modifier::BOLD),
                )]);

                let block = Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.theme.primary))
                    .style(Style::default().bg(self.theme.background));

                let inner_area = block.inner(area);
                block.render(area, buf);

                let mut list_items: Vec<ListItem> = Vec::new();
                for (i, provider) in self.providers.iter().enumerate() {
                    let is_selected = i == self.selected;
                    let style = if is_selected {
                        Style::default()
                            .fg(self.theme.background)
                            .bg(self.theme.primary)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(self.theme.foreground)
                    };

                    let mut lines = vec![Line::from(Span::styled(&provider.name, style))];

                    if !provider.description.is_empty() {
                        lines.push(Line::from(Span::styled(
                            &provider.description,
                            Style::default().fg(self.theme.muted),
                        )));
                    }

                    let api_key_status = if provider.requires_api_key {
                        if let Some(env_var) = &provider.env_var_name {
                            format!("API Key: {} environment variable", env_var)
                        } else {
                            "API Key: Required".to_string()
                        }
                    } else {
                        "API Key: Not required".to_string()
                    };

                    lines.push(Line::from(Span::styled(
                        api_key_status,
                        Style::default().fg(self.theme.muted),
                    )));

                    list_items.push(ListItem::new(lines));
                }

                let list = List::new(list_items);
                list.render(inner_area, buf);

                let footer_text = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        " ↑/↓: Select | Enter: Choose | Esc: Cancel",
                        Style::default().fg(self.theme.muted),
                    )),
                ];

                let footer = Paragraph::new(footer_text)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                footer.render(area, buf);
            }
            ConnectStep::EnterApiKey => {
                let Some(provider) = self.providers.get(self.selected) else {
                    let block = Block::default()
                        .title(" Connect ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(self.theme.error))
                        .style(Style::default().bg(self.theme.background));
                    let inner_area = block.inner(area);
                    block.render(area, buf);
                    let msg = Paragraph::new("Selected provider is invalid. Press Esc to go back.")
                        .alignment(Alignment::Center)
                        .wrap(Wrap { trim: true });
                    msg.render(inner_area, buf);
                    return;
                };

                let title = Line::from(vec![Span::styled(
                    format!(" Connect to {} ", provider.name),
                    Style::default().add_modifier(Modifier::BOLD),
                )]);

                let block = Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.theme.primary))
                    .style(Style::default().bg(self.theme.background));

                let inner_area = block.inner(area);
                block.render(area, buf);

                let mut lines = vec![];

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Enter API key for {}:", provider.name),
                    Style::default().fg(self.theme.foreground),
                )));

                if let Some(env_var) = &provider.env_var_name {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("Or set {} environment variable", env_var),
                        Style::default().fg(self.theme.muted),
                    )));
                }

                lines.push(Line::from(""));

                let input_text = if self.api_key_input.is_empty() {
                    format!("> {}_", " ".repeat(60))
                } else {
                    let display_key = if self.api_key_input.len() > 60 {
                        format!("{}...{}", &self.api_key_input[..10], &self.api_key_input[self.api_key_input.len() - 47..])
                    } else {
                        self.api_key_input.clone()
                    };
                    format!(
                        "> {}{}",
                        display_key,
                        " ".repeat(60usize.saturating_sub(display_key.len()))
                    )
                };

                lines.push(Line::from(Span::styled(
                    input_text,
                    Style::default().fg(self.theme.foreground),
                )));

                lines.push(Line::from(""));

                if let Some(ref error) = self.error_message {
                    lines.push(Line::from(Span::styled(
                        format!("Error: {}", error),
                        Style::default().fg(self.theme.error),
                    )));
                }

                let paragraph = Paragraph::new(lines)
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: true });
                paragraph.render(inner_area, buf);

                let footer_text = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        " Enter: Save | Esc: Back",
                        Style::default().fg(self.theme.muted),
                    )),
                ];

                let footer = Paragraph::new(footer_text)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                footer.render(area, buf);
            }
        }
    }
}

impl Component for ConnectDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                if self.step == ConnectStep::EnterApiKey {
                    self.back_to_provider_selection();
                    None
                } else {
                    Some(TuiMsg::CloseDialog)
                }
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                if self.step == ConnectStep::SelectProvider {
                    self.cursor_up();
                }
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                if self.step == ConnectStep::SelectProvider {
                    self.cursor_down();
                }
                None
            }
            crossterm::event::KeyCode::Enter => match self.step {
                ConnectStep::SelectProvider => {
                    if let Some(provider) = self.select_provider().cloned() {
                        if provider.requires_api_key {
                            self.move_to_api_key_step();
                            None
                        } else {
                            Some(TuiMsg::ConnectConfigured {
                                provider_name: provider.name,
                                env_var: None,
                                api_key: None,
                            })
                        }
                    } else {
                        None
                    }
                }
                ConnectStep::EnterApiKey => {
                    let api_key = self.get_api_key();
                    if api_key.trim().is_empty() {
                        self.set_error("API key cannot be empty".to_string());
                        None
                    } else if let Some(provider) = self.select_provider().cloned() {
                        Some(TuiMsg::ConnectConfigured {
                            provider_name: provider.name,
                            env_var: provider.env_var_name,
                            api_key: Some(api_key),
                        })
                    } else {
                        None
                    }
                }
            },
            crossterm::event::KeyCode::Backspace => {
                if self.step == ConnectStep::EnterApiKey {
                    self.backspace();
                }
                None
            }
            crossterm::event::KeyCode::Char(c) => {
                if self.step == ConnectStep::EnterApiKey {
                    self.insert_char(c);
                }
                None
            }
            _ => None,
        }
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        if self.step == ConnectStep::EnterApiKey {
            self.api_key_input.insert_str(self.cursor_pos, &text);
            self.cursor_pos += text.len();
        }
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, theme: &Arc<Theme>) {
        use ratatui::layout::{Constraint, Layout};

        match self.step {
            ConnectStep::SelectProvider => {
                let chunks = Layout::default()
                    .direction(ratatui::layout::Direction::Vertical)
                    .constraints([Constraint::Min(0), Constraint::Length(3)])
                    .split(area);

                let title = Line::from(vec![Span::styled(
                    " Connect to Provider ",
                    Style::default().add_modifier(Modifier::BOLD),
                )]);

                let block = Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.primary))
                    .style(Style::default().bg(theme.background));

                let inner_area = block.inner(chunks[0]);
                frame.render_widget(block, chunks[0]);

                let mut list_items: Vec<ListItem> = Vec::new();
                for (i, provider) in self.providers.iter().enumerate() {
                    let is_selected = i == self.selected;
                    let style = if is_selected {
                        Style::default()
                            .fg(theme.background)
                            .bg(theme.primary)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.foreground)
                    };

                    let mut lines = vec![Line::from(Span::styled(&provider.name, style))];

                    if !provider.description.is_empty() {
                        lines.push(Line::from(Span::styled(
                            &provider.description,
                            Style::default().fg(theme.muted),
                        )));
                    }

                    let api_key_status = if provider.requires_api_key {
                        if let Some(env_var) = &provider.env_var_name {
                            format!("API Key: {} environment variable", env_var)
                        } else {
                            "API Key: Required".to_string()
                        }
                    } else {
                        "API Key: Not required".to_string()
                    };

                    lines.push(Line::from(Span::styled(
                        api_key_status,
                        Style::default().fg(theme.muted),
                    )));

                    list_items.push(ListItem::new(lines));
                }

                let list = List::new(list_items);
                frame.render_stateful_widget(list, inner_area, &mut self.list_state);

                let footer_text = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        " ↑/↓: Select | Enter: Choose | Esc: Cancel",
                        Style::default().fg(theme.muted),
                    )),
                ];

                let footer = Paragraph::new(footer_text)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                frame.render_widget(footer, chunks[1]);
            }
            ConnectStep::EnterApiKey => {
                let Some(provider) = self.providers.get(self.selected) else {
                    let block = Block::default()
                        .title(" Connect ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.error))
                        .style(Style::default().bg(theme.background));
                    frame.render_widget(block, area);
                    let msg = Paragraph::new("Selected provider is invalid. Press Esc to go back.")
                        .alignment(Alignment::Center)
                        .wrap(Wrap { trim: true });
                    frame.render_widget(msg, area);
                    return;
                };

                let title = Line::from(vec![Span::styled(
                    format!(" Connect to {} ", provider.name),
                    Style::default().add_modifier(Modifier::BOLD),
                )]);

                let block = Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.primary))
                    .style(Style::default().bg(theme.background));

                let inner_area = block.inner(area);
                frame.render_widget(block, area);

                let mut lines = vec![];

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Enter API key for {}:", provider.name),
                    Style::default().fg(theme.foreground),
                )));

                if let Some(env_var) = &provider.env_var_name {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("Or set {} environment variable", env_var),
                        Style::default().fg(theme.muted),
                    )));
                }

                lines.push(Line::from(""));

                let input_text = if self.api_key_input.is_empty() {
                    format!("> {}_", " ".repeat(60))
                } else {
                    let display_key = if self.api_key_input.len() > 60 {
                        format!("{}...{}", &self.api_key_input[..10], &self.api_key_input[self.api_key_input.len() - 47..])
                    } else {
                        self.api_key_input.clone()
                    };
                    format!(
                        "> {}{}",
                        display_key,
                        " ".repeat(60usize.saturating_sub(display_key.len()))
                    )
                };

                lines.push(Line::from(Span::styled(
                    input_text,
                    Style::default().fg(theme.foreground),
                )));

                lines.push(Line::from(""));

                if let Some(ref error) = self.error_message {
                    lines.push(Line::from(Span::styled(
                        format!("Error: {}", error),
                        Style::default().fg(theme.error),
                    )));
                }

                let paragraph = Paragraph::new(lines)
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: true });
                frame.render_widget(paragraph, inner_area);

                let footer_text = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        " Enter: Save | Esc: Back",
                        Style::default().fg(theme.muted),
                    )),
                ];

                let footer = Paragraph::new(footer_text)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                let footer_area = Rect::new(area.x, area.y + area.height - 2, area.width, 2);
                frame.render_widget(footer, footer_area);
            }
        }
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Connect
    }
}
