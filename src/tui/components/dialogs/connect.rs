use crossterm::event::KeyEvent;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Widget, Wrap};
use std::sync::Arc;

use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::tui::theme::Theme;
use codegg_protocol::provider::EggpoolTlsPolicy;

/// Auth modes a provider can support. The first pass keeps all providers
/// in `ApiKey` mode; `OAuthDevice` and `ExternalCommand` are reserved for
/// future, officially-supported flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAuthMode {
    ApiKey,
    OAuthDevice,
    ExternalCommand,
    None,
}

#[derive(Clone, Default)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Kept for backward compatibility. New code should consult
    /// `auth_modes` instead.
    pub requires_api_key: bool,
    /// Auth modes the provider supports. The first pass defaults this to
    /// `[ApiKey]` (or `[None]`) to match `requires_api_key`.
    pub auth_modes: Vec<ProviderAuthMode>,
    pub env_var_name: Option<String>,
    pub base_url_example: Option<String>,
}

impl ProviderInfo {
    /// Build a provider that supports the API-key flow.
    pub fn api_key(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        env_var_name: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            requires_api_key: true,
            auth_modes: vec![ProviderAuthMode::ApiKey],
            env_var_name,
            base_url_example: None,
        }
    }

    /// Build a provider that does not require auth (e.g. local).
    pub fn no_auth(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            requires_api_key: false,
            auth_modes: vec![ProviderAuthMode::None],
            env_var_name: None,
            base_url_example: None,
        }
    }

    /// True if the provider's auth modes include API-key entry.
    pub fn supports_api_key(&self) -> bool {
        self.auth_modes
            .iter()
            .any(|m| matches!(m, ProviderAuthMode::ApiKey))
    }
}

pub struct ConnectDialog {
    pub providers: Vec<ProviderInfo>,
    pub selected: usize,
    pub scroll: usize,
    pub theme: Arc<Theme>,
    pub step: ConnectStep,
    pub api_key_input: String,
    /// Non-secret form input. The API key remains isolated in
    /// `api_key_input` and is never placed in a generic TUI message.
    pub form_input: String,
    pub host: String,
    pub port: String,
    pub display_name: String,
    pub tls_policy: EggpoolTlsPolicy,
    pub scope_personal: bool,
    pub operation_id: Option<String>,
    /// When set, the dialog is the masked credential editor for an existing
    /// connection rather than a new provisioning flow.
    pub rotation_target: Option<(String, u64)>,
    pub cursor_pos: usize,
    pub error_message: Option<String>,
    pub list_state: ListState,
}

impl Clone for ConnectDialog {
    fn clone(&self) -> Self {
        Self {
            providers: self.providers.clone(),
            selected: self.selected,
            scroll: self.scroll,
            theme: Arc::clone(&self.theme),
            step: self.step.clone(),
            // Focus/render clones never receive the plaintext API key.
            api_key_input: String::new(),
            form_input: self.form_input.clone(),
            host: self.host.clone(),
            port: self.port.clone(),
            display_name: self.display_name.clone(),
            tls_policy: self.tls_policy,
            scope_personal: self.scope_personal,
            operation_id: self.operation_id.clone(),
            rotation_target: self.rotation_target.clone(),
            cursor_pos: self.cursor_pos,
            error_message: self.error_message.clone(),
            list_state: self.list_state.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectStep {
    SelectProvider,
    EnterHost,
    EnterPort,
    SelectTls,
    EnterApiKey,
    EnterDisplayName,
    SelectScope,
    Review,
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
            form_input: String::new(),
            host: String::new(),
            port: "11300".to_string(),
            display_name: String::new(),
            tls_policy: EggpoolTlsPolicy::Optional,
            scope_personal: true,
            operation_id: None,
            rotation_target: None,
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

    pub fn move_to_host_step(&mut self) {
        self.step = ConnectStep::EnterHost;
        self.form_input = self.host.clone();
        self.cursor_pos = self.form_input.len();
        self.error_message = None;
    }

    pub fn is_text_input_step(&self) -> bool {
        matches!(
            self.step,
            ConnectStep::EnterHost
                | ConnectStep::EnterPort
                | ConnectStep::EnterApiKey
                | ConnectStep::EnterDisplayName
        )
    }

    pub fn is_secret_input_step(&self) -> bool {
        self.step == ConnectStep::EnterApiKey
    }

    pub fn cycle_tls_policy(&mut self, forward: bool) {
        if self.step != ConnectStep::SelectTls {
            return;
        }
        self.tls_policy = match (self.tls_policy, forward) {
            (EggpoolTlsPolicy::Required, true) => EggpoolTlsPolicy::Optional,
            (EggpoolTlsPolicy::Optional, true) => EggpoolTlsPolicy::Disabled,
            (EggpoolTlsPolicy::Disabled, true) => EggpoolTlsPolicy::Required,
            (EggpoolTlsPolicy::Required, false) => EggpoolTlsPolicy::Disabled,
            (EggpoolTlsPolicy::Optional, false) => EggpoolTlsPolicy::Required,
            (EggpoolTlsPolicy::Disabled, false) => EggpoolTlsPolicy::Optional,
        };
    }

    pub fn back_step(&mut self) -> bool {
        match self.step {
            ConnectStep::SelectProvider => false,
            ConnectStep::EnterHost => {
                self.step = ConnectStep::SelectProvider;
                self.form_input.clear();
                true
            }
            ConnectStep::EnterPort => {
                self.step = ConnectStep::EnterHost;
                self.form_input = self.host.clone();
                self.cursor_pos = self.form_input.len();
                true
            }
            ConnectStep::SelectTls => {
                self.step = ConnectStep::EnterPort;
                self.form_input = self.port.clone();
                self.cursor_pos = self.form_input.len();
                true
            }
            ConnectStep::EnterApiKey => {
                self.step = ConnectStep::SelectTls;
                self.form_input.clear();
                self.cursor_pos = 0;
                self.clear_secret();
                true
            }
            ConnectStep::EnterDisplayName => {
                self.step = ConnectStep::EnterApiKey;
                self.form_input.clear();
                self.cursor_pos = 0;
                true
            }
            ConnectStep::SelectScope => {
                self.step = ConnectStep::EnterDisplayName;
                self.form_input = self.display_name.clone();
                self.cursor_pos = self.form_input.len();
                true
            }
            ConnectStep::Review => {
                self.step = ConnectStep::SelectScope;
                true
            }
        }
    }

    pub fn clear_secret(&mut self) {
        self.api_key_input.replace_range(.., "");
        self.api_key_input.shrink_to_fit();
    }

    pub fn set_rotation_target(&mut self, connection_id: String, expected_revision: u64) {
        self.rotation_target = Some((connection_id, expected_revision));
        self.step = ConnectStep::EnterApiKey;
        self.form_input.clear();
        self.cursor_pos = 0;
        self.error_message = None;
    }

    pub fn back_to_provider_selection(&mut self) {
        self.step = ConnectStep::SelectProvider;
        self.form_input.clear();
        self.host.clear();
        self.port = "11300".to_string();
        self.display_name.clear();
        self.api_key_input.clear();
        self.cursor_pos = 0;
        self.error_message = None;
    }

    pub fn insert_char(&mut self, c: char) {
        let input = if self.is_secret_input_step() {
            &mut self.api_key_input
        } else {
            &mut self.form_input
        };
        input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            let input = if self.is_secret_input_step() {
                &mut self.api_key_input
            } else {
                &mut self.form_input
            };
            let before = &input[..self.cursor_pos];
            let ch_len = before
                .chars()
                .next_back()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            let new_cursor = self.cursor_pos - ch_len;
            input.drain(new_cursor..self.cursor_pos);
            self.cursor_pos = new_cursor;
        }
    }

    pub fn get_api_key(&self) -> String {
        self.api_key_input.clone()
    }

    pub fn commit_form_input(&mut self) {
        match self.step {
            ConnectStep::EnterHost => self.host = self.form_input.trim().to_owned(),
            ConnectStep::EnterPort => self.port = self.form_input.trim().to_owned(),
            ConnectStep::EnterDisplayName => self.display_name = self.form_input.trim().to_owned(),
            _ => {}
        }
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
                    .border_style(Style::default().fg(self.theme.border))
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
                        " j/k/↑/↓ select  |  Enter choose  |  Esc close ",
                        Style::default().fg(self.theme.muted),
                    )),
                ];

                let footer = Paragraph::new(footer_text)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                footer.render(area, buf);
            }
            ConnectStep::EnterHost
            | ConnectStep::EnterPort
            | ConnectStep::SelectTls
            | ConnectStep::EnterApiKey
            | ConnectStep::EnterDisplayName
            | ConnectStep::SelectScope
            | ConnectStep::Review => {
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
                    .border_style(Style::default().fg(self.theme.border))
                    .style(Style::default().bg(self.theme.background));

                let inner_area = block.inner(area);
                block.render(area, buf);

                let mut lines = vec![];

                let (label, value) = match self.step {
                    ConnectStep::EnterHost => {
                        ("Eggpool host or HTTP(S) origin:", self.form_input.as_str())
                    }
                    ConnectStep::EnterPort => ("Port (default 11300):", self.form_input.as_str()),
                    ConnectStep::EnterApiKey => ("API key (masked):", self.api_key_input.as_str()),
                    ConnectStep::EnterDisplayName => {
                        ("Display name (optional):", self.form_input.as_str())
                    }
                    ConnectStep::SelectTls => {
                        ("TLS policy: choose with ↑/↓, press Enter to continue", "")
                    }
                    ConnectStep::SelectScope => (
                        "Scope: Personal (project scope requires explicit context)",
                        "",
                    ),
                    ConnectStep::Review => ("Review and press Enter to connect:", ""),
                    ConnectStep::SelectProvider => unreachable!(),
                };

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    label.to_string(),
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

                let input_text = if matches!(
                    self.step,
                    ConnectStep::SelectTls | ConnectStep::SelectScope | ConnectStep::Review
                ) {
                    format!(
                        "> {}",
                        match self.step {
                            ConnectStep::SelectTls => match self.tls_policy {
                                EggpoolTlsPolicy::Required => "Required TLS; press Enter",
                                EggpoolTlsPolicy::Optional => "Optional TLS; press Enter",
                                EggpoolTlsPolicy::Disabled => "TLS disabled; press Enter",
                            },
                            ConnectStep::SelectScope => "Personal; press Enter",
                            ConnectStep::Review => "submit",
                            _ => unreachable!(),
                        }
                    )
                } else if value.is_empty() {
                    format!("> {}_", " ".repeat(60))
                } else {
                    // Never display the secret in plaintext. Render a fixed
                    // mask while typing, plus a non-secret length hint.
                    if self.step == ConnectStep::EnterApiKey {
                        let mask = crate::auth::mask_secret(value);
                        let length_hint = format!(" ({} chars)", value.chars().count());
                        format!("> {}{}{}", mask, length_hint, " ".repeat(8))
                    } else {
                        format!("> {}_", value)
                    }
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
                        " Enter: Continue  |  Backspace: delete  |  Esc: Back ",
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
                if self.back_step() {
                    None
                } else {
                    Some(TuiMsg::CloseDialog)
                }
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                if self.step == ConnectStep::SelectProvider {
                    self.cursor_up();
                } else if self.step == ConnectStep::SelectTls {
                    self.cycle_tls_policy(false);
                }
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                if self.step == ConnectStep::SelectProvider {
                    self.cursor_down();
                } else if self.step == ConnectStep::SelectTls {
                    self.cycle_tls_policy(true);
                }
                None
            }
            crossterm::event::KeyCode::Enter => match self.step {
                ConnectStep::SelectProvider => {
                    if let Some(provider) = self.select_provider().cloned() {
                        if provider.requires_api_key {
                            self.move_to_host_step();
                            None
                        } else {
                            Some(TuiMsg::CloseDialog)
                        }
                    } else {
                        None
                    }
                }
                ConnectStep::EnterHost => {
                    if self.form_input.trim().is_empty() {
                        self.set_error("Host cannot be empty".to_string());
                    } else {
                        self.commit_form_input();
                        self.form_input = self.port.clone();
                        self.cursor_pos = self.form_input.len();
                        self.step = ConnectStep::EnterPort;
                    }
                    None
                }
                ConnectStep::EnterPort => {
                    if self
                        .form_input
                        .trim()
                        .parse::<u16>()
                        .ok()
                        .filter(|port| *port > 0)
                        .is_none()
                    {
                        self.set_error("Port must be between 1 and 65535".to_string());
                    } else {
                        self.commit_form_input();
                        self.form_input.clear();
                        self.cursor_pos = 0;
                        self.step = ConnectStep::SelectTls;
                    }
                    None
                }
                ConnectStep::SelectTls => {
                    self.step = ConnectStep::EnterApiKey;
                    self.api_key_input.clear();
                    self.cursor_pos = 0;
                    None
                }
                ConnectStep::EnterApiKey => {
                    let api_key = self.get_api_key();
                    if api_key.trim().is_empty() {
                        self.set_error("API key cannot be empty".to_string());
                    } else if self.rotation_target.is_some() {
                        return Some(TuiMsg::SubmitConnect);
                    } else {
                        self.form_input = self.display_name.clone();
                        self.cursor_pos = self.form_input.len();
                        self.step = ConnectStep::EnterDisplayName;
                    }
                    None
                }
                ConnectStep::EnterDisplayName => {
                    self.commit_form_input();
                    self.step = ConnectStep::SelectScope;
                    None
                }
                ConnectStep::SelectScope => {
                    self.step = ConnectStep::Review;
                    None
                }
                ConnectStep::Review => Some(TuiMsg::SubmitConnect),
            },
            crossterm::event::KeyCode::Backspace => {
                if self.is_text_input_step() {
                    self.backspace();
                }
                None
            }
            crossterm::event::KeyCode::Char(c) => {
                if self.is_text_input_step() {
                    self.insert_char(c);
                }
                None
            }
            _ => None,
        }
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        if self.is_text_input_step() {
            let input = if self.is_secret_input_step() {
                &mut self.api_key_input
            } else {
                &mut self.form_input
            };
            input.insert_str(self.cursor_pos, &text);
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
                    .border_style(Style::default().fg(theme.border))
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
                        " j/k/↑/↓ select  |  Enter choose  |  Esc close ",
                        Style::default().fg(theme.muted),
                    )),
                ];

                let footer = Paragraph::new(footer_text)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                frame.render_widget(footer, chunks[1]);
            }
            ConnectStep::EnterHost
            | ConnectStep::EnterPort
            | ConnectStep::SelectTls
            | ConnectStep::EnterApiKey
            | ConnectStep::EnterDisplayName
            | ConnectStep::SelectScope
            | ConnectStep::Review => {
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
                    .border_style(Style::default().fg(theme.border))
                    .style(Style::default().bg(theme.background));

                let inner_area = block.inner(area);
                frame.render_widget(block, area);

                let mut lines = vec![];
                let (label, value) = match self.step {
                    ConnectStep::EnterHost => {
                        ("Eggpool host or HTTP(S) origin:", self.form_input.as_str())
                    }
                    ConnectStep::EnterPort => ("Port (default 11300):", self.form_input.as_str()),
                    ConnectStep::EnterApiKey => ("API key (masked):", self.api_key_input.as_str()),
                    ConnectStep::EnterDisplayName => {
                        ("Display name (optional):", self.form_input.as_str())
                    }
                    ConnectStep::SelectTls => {
                        ("TLS policy: choose with ↑/↓, press Enter to continue", "")
                    }
                    ConnectStep::SelectScope => (
                        "Scope: Personal (project scope requires explicit context)",
                        "",
                    ),
                    ConnectStep::Review => ("Review and press Enter to connect:", ""),
                    ConnectStep::SelectProvider => unreachable!(),
                };

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    label,
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

                let input_text = if matches!(
                    self.step,
                    ConnectStep::SelectTls | ConnectStep::SelectScope | ConnectStep::Review
                ) {
                    format!(
                        "> {}",
                        match self.step {
                            ConnectStep::SelectTls => match self.tls_policy {
                                EggpoolTlsPolicy::Required => "Required TLS; press Enter",
                                EggpoolTlsPolicy::Optional => "Optional TLS; press Enter",
                                EggpoolTlsPolicy::Disabled => "TLS disabled; press Enter",
                            },
                            ConnectStep::SelectScope => "Personal; press Enter",
                            ConnectStep::Review => "submit",
                            _ => unreachable!(),
                        }
                    )
                } else if value.is_empty() {
                    format!("> {}_", " ".repeat(60))
                } else {
                    if self.step == ConnectStep::EnterApiKey {
                        let mask = crate::auth::mask_secret(value);
                        let length_hint = format!(" ({} chars)", value.chars().count());
                        format!("> {}{}{}", mask, length_hint, " ".repeat(8))
                    } else {
                        format!("> {}_", value)
                    }
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
                        " Enter: Continue  |  Backspace: delete  |  Esc: Back ",
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
