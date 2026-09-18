use crossterm::event::KeyEvent;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Widget, Wrap};
use std::sync::Arc;

use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::tui::theme::Theme;
use codegg_protocol::provider::{EggpoolTlsPolicy, ProviderCredentialKind, ProviderSetupEntryDto};

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
    /// Setup-catalog endpoint policy (`fixed`, `optional_override`,
    /// `required`, `proxy_preset`). Drives the typed form; never a
    /// TUI-maintained provider-name allowlist.
    pub endpoint_policy: String,
    /// Accepted credential kinds from the setup catalog
    /// (`api_key` and/or `bearer`).
    pub credential_kinds: Vec<String>,
    /// Whether the catalog requires an endpoint for this provider.
    pub requires_endpoint: bool,
    /// Non-secret default endpoint/origin when the catalog pins one.
    pub default_endpoint: Option<String>,
    /// Whether the catalog marks this provider as onboardable with the
    /// current safe form. Non-connectable rows render disabled.
    pub connectable: bool,
}

/// Typed `/connect` form selected from the daemon-owned setup catalog.
/// Exhaustive rendering/validation branches on this enum; there is no
/// remote JSON-schema form engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectFormKind {
    /// Fixed-endpoint provider: secret (+ credential-kind choice when the
    /// catalog admits both) plus optional display label. No endpoint fields.
    Fixed,
    /// Provider works without an endpoint; caller may supply an override.
    OptionalOverride,
    /// Caller must supply a full base URL (custom/compatible, Azure).
    RequiredEndpoint,
    /// Eggpool-style host + port + TLS preset plus secret and scope.
    EggpoolProxy,
}

impl ProviderInfo {
    /// Convert one secret-free setup-catalog DTO into presentation metadata.
    /// The DTO is the authority; the TUI never hard-codes provider names.
    pub fn from_setup_dto(dto: &ProviderSetupEntryDto) -> Self {
        let requires_api_key = dto.credential_kinds.iter().any(|kind| kind == "api_key");
        let auth_modes = if dto.credential_kinds.is_empty() || requires_api_key {
            vec![ProviderAuthMode::ApiKey]
        } else {
            // Credential kinds without API key (e.g. bearer-only): the typed
            // credential-kind flow carries the secret, so legacy metadata
            // stays consistent with `requires_api_key == false`.
            vec![ProviderAuthMode::None]
        };
        Self {
            id: dto.id.clone(),
            name: dto.display_name.clone(),
            description: dto.description.clone(),
            requires_api_key,
            auth_modes,
            env_var_name: dto.env_var.clone(),
            base_url_example: dto.default_endpoint.clone(),
            endpoint_policy: dto.endpoint_policy.clone(),
            credential_kinds: dto.credential_kinds.clone(),
            requires_endpoint: dto.requires_endpoint,
            default_endpoint: dto.default_endpoint.clone(),
            connectable: dto.connectable,
        }
    }

    /// Typed form for this provider, derived from the catalog endpoint
    /// policy. Unknown policies fall back to the fixed secret-only form so
    /// a new catalog policy cannot silently enable an endpoint free-for-all.
    pub fn form_kind(&self) -> ConnectFormKind {
        match self.endpoint_policy.as_str() {
            "proxy_preset" => ConnectFormKind::EggpoolProxy,
            "required" => ConnectFormKind::RequiredEndpoint,
            "optional_override" => ConnectFormKind::OptionalOverride,
            _ => ConnectFormKind::Fixed,
        }
    }

    /// Whether the row is selectable in `/connect`.
    pub fn is_selectable(&self) -> bool {
        self.connectable
    }

    /// Whether the provider admits both API-key and bearer credentials and
    /// therefore needs an explicit credential-kind choice.
    pub fn needs_credential_choice(&self) -> bool {
        let has_api_key = self.credential_kinds.iter().any(|k| k == "api_key");
        let has_bearer = self.credential_kinds.iter().any(|k| k == "bearer");
        has_api_key && has_bearer
    }

    /// Whether this provider's form collects an endpoint value (required or
    /// optional override). Fixed providers never do; proxy presets use the
    /// host/port/TLS fields instead.
    pub fn collects_endpoint(&self) -> bool {
        matches!(
            self.form_kind(),
            ConnectFormKind::RequiredEndpoint | ConnectFormKind::OptionalOverride
        )
    }
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
            endpoint_policy: "fixed".to_string(),
            credential_kinds: vec!["api_key".to_string()],
            requires_endpoint: false,
            default_endpoint: None,
            connectable: true,
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
            endpoint_policy: "fixed".to_string(),
            credential_kinds: Vec::new(),
            requires_endpoint: false,
            default_endpoint: None,
            connectable: true,
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
    /// Generic endpoint value for `required` / `optional_override` forms.
    /// Proxy presets keep using `host`/`port`/`tls_policy` instead.
    pub endpoint: String,
    /// Credential kind for the generic create request. Selected via the
    /// `SelectCredentialKind` step when the catalog admits both.
    pub credential_kind: ProviderCredentialKind,
    pub display_name: String,
    pub tls_policy: EggpoolTlsPolicy,
    pub scope_personal: bool,
    pub operation_id: Option<String>,
    /// When true the dialog is waiting for the daemon-owned setup catalog.
    /// The provider list stays empty until the response populates it; the
    /// dialog never falls back to an Eggpool-only list.
    pub loading: bool,
    /// Bounded actionable error when the setup catalog fails to load.
    pub load_error: Option<String>,
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
            endpoint: self.endpoint.clone(),
            credential_kind: self.credential_kind,
            display_name: self.display_name.clone(),
            tls_policy: self.tls_policy,
            scope_personal: self.scope_personal,
            operation_id: self.operation_id.clone(),
            loading: self.loading,
            load_error: self.load_error.clone(),
            rotation_target: self.rotation_target.clone(),
            cursor_pos: self.cursor_pos,
            error_message: self.error_message.clone(),
            list_state: self.list_state,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectStep {
    SelectProvider,
    EnterEndpoint,
    SelectCredentialKind,
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
            endpoint: String::new(),
            credential_kind: ProviderCredentialKind::ApiKey,
            display_name: String::new(),
            tls_policy: EggpoolTlsPolicy::Optional,
            scope_personal: true,
            operation_id: None,
            loading: false,
            load_error: None,
            rotation_target: None,
            cursor_pos: 0,
            error_message: None,
            list_state,
        }
    }

    /// Loading-state dialog for `/connect`. The provider list stays empty
    /// until the daemon-owned setup catalog populates it; there is no
    /// Eggpool-only fallback.
    pub fn new_loading(theme: Arc<Theme>) -> Self {
        let mut dialog = Self::new(Vec::new(), theme);
        dialog.loading = true;
        dialog.load_error = None;
        dialog.list_state.select(None);
        dialog
    }

    /// Populate the provider list from the secret-free setup catalog.
    /// Catalog order is preserved; selection resets to the first row.
    pub fn set_setup_entries(&mut self, dtos: &[ProviderSetupEntryDto]) {
        self.providers = dtos.iter().map(ProviderInfo::from_setup_dto).collect();
        self.loading = false;
        self.load_error = None;
        self.selected = 0;
        self.scroll = 0;
        if self.providers.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
        self.error_message = None;
    }

    /// Record a bounded actionable catalog-load failure. The list stays
    /// empty; callers must not substitute an Eggpool-only list.
    pub fn set_setup_error(&mut self, error: String) {
        const MAX_ERROR: usize = 300;
        let mut bounded = error;
        if bounded.len() > MAX_ERROR {
            bounded.truncate(MAX_ERROR);
        }
        bounded.retain(|c| !c.is_control() || c == '\n');
        self.loading = false;
        self.load_error = Some(bounded);
        self.error_message = None;
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn selected_entry(&self) -> Option<&ProviderInfo> {
        self.providers.get(self.selected)
    }

    pub fn selected_form_kind(&self) -> Option<ConnectFormKind> {
        self.selected_entry().map(|entry| entry.form_kind())
    }

    pub fn credential_kind_label(&self) -> &'static str {
        match self.credential_kind {
            ProviderCredentialKind::ApiKey => "API key",
            ProviderCredentialKind::Bearer => "Bearer token",
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

    pub fn move_to_endpoint_step(&mut self) {
        self.step = ConnectStep::EnterEndpoint;
        self.form_input = self.endpoint.clone();
        self.cursor_pos = self.form_input.len();
        self.error_message = None;
    }

    pub fn move_to_credential_kind_step(&mut self) {
        // Default to the first catalog-admitted kind so a stale selection
        // cannot leak across providers.
        let entry = self.selected_entry().cloned();
        if let Some(entry) = entry {
            if entry.credential_kinds.iter().any(|k| k == "api_key") {
                self.credential_kind = ProviderCredentialKind::ApiKey;
            } else if entry.credential_kinds.iter().any(|k| k == "bearer") {
                self.credential_kind = ProviderCredentialKind::Bearer;
            }
        }
        self.step = ConnectStep::SelectCredentialKind;
        self.error_message = None;
    }

    /// Advance from provider selection into the typed form for the chosen
    /// catalog entry. Ordinary providers never traverse Eggpool host/port/TLS
    /// steps. Returns false when selection cannot advance (loading, load
    /// error, empty list, or disabled row) after recording a bounded error.
    pub fn advance_from_provider_selection(&mut self) -> bool {
        if self.loading {
            return false;
        }
        if let Some(error) = self.load_error.clone() {
            self.error_message = Some(format!("Providers unavailable: {error}"));
            return false;
        }
        let entry = match self.selected_entry().cloned() {
            Some(entry) => entry,
            None => {
                self.error_message = Some("Select a provider".to_string());
                return false;
            }
        };
        if !entry.is_selectable() {
            self.error_message = Some(format!("{} is not yet supported in /connect", entry.name));
            return false;
        }
        self.error_message = None;
        match entry.form_kind() {
            ConnectFormKind::EggpoolProxy => {
                self.move_to_host_step();
            }
            ConnectFormKind::RequiredEndpoint | ConnectFormKind::OptionalOverride => {
                self.move_to_endpoint_step();
            }
            ConnectFormKind::Fixed => {
                if entry.needs_credential_choice() {
                    self.move_to_credential_kind_step();
                } else {
                    // Pin the single admitted kind so the create request
                    // cannot inherit a stale bearer/api-key selection.
                    if entry.credential_kinds.iter().any(|k| k == "bearer") {
                        self.credential_kind = ProviderCredentialKind::Bearer;
                    } else {
                        self.credential_kind = ProviderCredentialKind::ApiKey;
                    }
                    self.move_to_api_key_step();
                }
            }
        }
        true
    }

    /// Advance from the endpoint step into credential-kind selection (when
    /// the catalog admits both) or directly into secret entry.
    pub fn next_after_endpoint(&mut self) -> bool {
        let entry = match self.selected_entry().cloned() {
            Some(entry) => entry,
            None => {
                self.error_message = Some("Select a provider".to_string());
                return false;
            }
        };
        let trimmed = self.form_input.trim().to_owned();
        if entry.requires_endpoint && trimmed.is_empty() {
            self.error_message = Some("Endpoint is required".to_string());
            return false;
        }
        self.endpoint = trimmed;
        self.error_message = None;
        if entry.needs_credential_choice() {
            self.move_to_credential_kind_step();
        } else {
            if entry.credential_kinds.iter().any(|k| k == "bearer") {
                self.credential_kind = ProviderCredentialKind::Bearer;
            } else {
                self.credential_kind = ProviderCredentialKind::ApiKey;
            }
            self.move_to_api_key_step();
        }
        true
    }

    pub fn next_after_credential_kind(&mut self) {
        self.move_to_api_key_step();
    }

    pub fn is_text_input_step(&self) -> bool {
        matches!(
            self.step,
            ConnectStep::EnterHost
                | ConnectStep::EnterPort
                | ConnectStep::EnterEndpoint
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

    pub fn cycle_credential_kind(&mut self, forward: bool) {
        if self.step != ConnectStep::SelectCredentialKind {
            return;
        }
        let entry = match self.selected_entry() {
            Some(entry) => entry,
            None => return,
        };
        if !entry.needs_credential_choice() {
            return;
        }
        // Two options only; direction just toggles.
        let _ = forward;
        self.credential_kind = match self.credential_kind {
            ProviderCredentialKind::ApiKey => ProviderCredentialKind::Bearer,
            ProviderCredentialKind::Bearer => ProviderCredentialKind::ApiKey,
        };
    }

    /// Step back through the typed form. The path is derived from the
    /// selected catalog entry so skipped steps (e.g. endpoint for fixed
    /// providers) are never revisited. Rotation editors close from secret
    /// entry instead of entering the provisioning form.
    pub fn back_step(&mut self) -> bool {
        if self.rotation_target.is_some() && self.step == ConnectStep::EnterApiKey {
            self.clear_secret();
            self.form_input.clear();
            return false;
        }
        match self.step {
            ConnectStep::SelectProvider => false,
            ConnectStep::EnterEndpoint => {
                self.step = ConnectStep::SelectProvider;
                self.form_input.clear();
                true
            }
            ConnectStep::SelectCredentialKind => {
                let collects_endpoint = self
                    .selected_entry()
                    .is_some_and(|entry| entry.collects_endpoint());
                if collects_endpoint {
                    self.step = ConnectStep::EnterEndpoint;
                    self.form_input = self.endpoint.clone();
                    self.cursor_pos = self.form_input.len();
                } else {
                    self.step = ConnectStep::SelectProvider;
                    self.form_input.clear();
                }
                true
            }
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
                let kind = self.selected_form_kind();
                let needs_choice = self
                    .selected_entry()
                    .is_some_and(|entry| entry.needs_credential_choice());
                if needs_choice {
                    self.step = ConnectStep::SelectCredentialKind;
                    self.form_input.clear();
                    self.cursor_pos = 0;
                } else if kind == Some(ConnectFormKind::EggpoolProxy) {
                    self.step = ConnectStep::SelectTls;
                    self.form_input.clear();
                    self.cursor_pos = 0;
                } else if kind == Some(ConnectFormKind::RequiredEndpoint)
                    || kind == Some(ConnectFormKind::OptionalOverride)
                {
                    self.step = ConnectStep::EnterEndpoint;
                    self.form_input = self.endpoint.clone();
                    self.cursor_pos = self.form_input.len();
                } else {
                    self.step = ConnectStep::SelectProvider;
                    self.form_input.clear();
                    self.cursor_pos = 0;
                }
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
        self.endpoint.clear();
        self.credential_kind = ProviderCredentialKind::ApiKey;
        self.display_name.clear();
        self.api_key_input.clear();
        self.cursor_pos = 0;
        self.error_message = None;
    }

    /// Mouse hit-test row for one provider entry. Each entry renders three
    /// content rows (name, description, credential status); content row 0 is
    /// the first entry's name line. Returns the provider index honoring the
    /// current scroll offset.
    pub fn hit_test_provider_row(&self, rel_y: usize) -> Option<usize> {
        if self.step != ConnectStep::SelectProvider || self.loading {
            return None;
        }
        if self.load_error.is_some() || self.providers.is_empty() {
            return None;
        }
        // rel_y is dialog-relative including the top border.
        if rel_y < 1 {
            return None;
        }
        let content_row = rel_y - 1;
        let idx = self.scroll + content_row / 3;
        if idx < self.providers.len() {
            Some(idx)
        } else {
            None
        }
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
            ConnectStep::EnterEndpoint => self.endpoint = self.form_input.trim().to_owned(),
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

                if self.loading {
                    let loading = Paragraph::new(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            "Loading providers from daemon…",
                            Style::default().fg(self.theme.foreground),
                        )),
                    ])
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: true });
                    loading.render(inner_area, buf);
                    let footer = Paragraph::new(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            " Esc close ",
                            Style::default().fg(self.theme.muted),
                        )),
                    ])
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                    footer.render(area, buf);
                    return;
                }

                if let Some(error) = self.load_error.as_ref() {
                    let failed = Paragraph::new(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            format!("Failed to load providers: {error}"),
                            Style::default().fg(self.theme.error),
                        )),
                        Line::from(""),
                        Line::from(Span::styled(
                            "Press Esc and retry /connect.",
                            Style::default().fg(self.theme.muted),
                        )),
                    ])
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: true });
                    failed.render(inner_area, buf);
                    let footer = Paragraph::new(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            " Esc close ",
                            Style::default().fg(self.theme.muted),
                        )),
                    ])
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                    footer.render(area, buf);
                    return;
                }

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

                    let name = if provider.is_selectable() {
                        provider.name.clone()
                    } else {
                        format!("{} (not supported)", provider.name)
                    };
                    let mut lines = vec![Line::from(Span::styled(name, style))];

                    if !provider.description.is_empty() {
                        lines.push(Line::from(Span::styled(
                            &provider.description,
                            Style::default().fg(self.theme.muted),
                        )));
                    }

                    let api_key_status = if !provider.is_selectable() {
                        "Not yet supported in /connect".to_string()
                    } else if provider.credential_kinds.iter().any(|k| k == "bearer")
                        && provider.credential_kinds.iter().any(|k| k == "api_key")
                    {
                        if let Some(env_var) = &provider.env_var_name {
                            format!("Credential: API key or bearer (or {env_var})")
                        } else {
                            "Credential: API key or bearer".to_string()
                        }
                    } else if provider.requires_api_key {
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
            ConnectStep::EnterEndpoint
            | ConnectStep::SelectCredentialKind
            | ConnectStep::EnterHost
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
                    ConnectStep::EnterEndpoint => {
                        let label = if provider.requires_endpoint {
                            "Endpoint (required, full base URL):"
                        } else {
                            "Endpoint override (optional, blank for default):"
                        };
                        (label, self.form_input.as_str())
                    }
                    ConnectStep::SelectCredentialKind => {
                        ("Credential kind: choose with ↑/↓, press Enter", "")
                    }
                    ConnectStep::EnterApiKey => {
                        let label = match self.credential_kind {
                            ProviderCredentialKind::Bearer => "Bearer token (masked):",
                            ProviderCredentialKind::ApiKey => "API key (masked):",
                        };
                        (label, self.api_key_input.as_str())
                    }
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
                    ConnectStep::SelectProvider => ("Select a provider:", ""),
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

                if self.step == ConnectStep::EnterEndpoint {
                    if let Some(default) = provider.default_endpoint.as_ref() {
                        if !default.is_empty() {
                            lines.push(Line::from(""));
                            lines.push(Line::from(Span::styled(
                                format!("Default: {default}"),
                                Style::default().fg(self.theme.muted),
                            )));
                        }
                    }
                }

                if self.step == ConnectStep::Review {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("Provider: {}", provider.name),
                        Style::default().fg(self.theme.muted),
                    )));
                    match provider.form_kind() {
                        ConnectFormKind::EggpoolProxy => {
                            lines.push(Line::from(Span::styled(
                                format!("Host: {} Port: {}", self.host, self.port),
                                Style::default().fg(self.theme.muted),
                            )));
                        }
                        ConnectFormKind::RequiredEndpoint | ConnectFormKind::OptionalOverride => {
                            let endpoint = if self.endpoint.trim().is_empty() {
                                provider.default_endpoint.clone().unwrap_or_default()
                            } else {
                                self.endpoint.clone()
                            };
                            if !endpoint.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    format!("Endpoint: {endpoint}"),
                                    Style::default().fg(self.theme.muted),
                                )));
                            }
                        }
                        ConnectFormKind::Fixed => {}
                    }
                    lines.push(Line::from(Span::styled(
                        format!("Credential: {}", self.credential_kind_label()),
                        Style::default().fg(self.theme.muted),
                    )));
                    if !self.display_name.trim().is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("Display name: {}", self.display_name.trim()),
                            Style::default().fg(self.theme.muted),
                        )));
                    }
                }

                lines.push(Line::from(""));

                let input_text = if matches!(
                    self.step,
                    ConnectStep::SelectTls
                        | ConnectStep::SelectCredentialKind
                        | ConnectStep::SelectScope
                        | ConnectStep::Review
                ) {
                    format!(
                        "> {}",
                        match self.step {
                            ConnectStep::SelectTls => match self.tls_policy {
                                EggpoolTlsPolicy::Required => "Required TLS; press Enter",
                                EggpoolTlsPolicy::Optional => "Optional TLS; press Enter",
                                EggpoolTlsPolicy::Disabled => "TLS disabled; press Enter",
                            },
                            ConnectStep::SelectCredentialKind => match self.credential_kind {
                                ProviderCredentialKind::ApiKey => "API key; press Enter",
                                ProviderCredentialKind::Bearer => "Bearer token; press Enter",
                            },
                            ConnectStep::SelectScope => "Personal; press Enter",
                            ConnectStep::Review => "submit",
                            _ => "continue",
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
        // While the setup catalog loads, only Esc dismisses. The list stays
        // empty; there is no Eggpool-only fallback to interact with.
        if self.loading && !matches!(key.code, crossterm::event::KeyCode::Esc) {
            return None;
        }
        match key.code {
            crossterm::event::KeyCode::Esc => {
                if self.back_step() {
                    None
                } else {
                    // Dismissing from selection or secret entry forgets any
                    // typed secret so cancellation leaves no credential behind.
                    self.clear_secret();
                    Some(TuiMsg::CloseDialog)
                }
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                if self.step == ConnectStep::SelectProvider {
                    self.cursor_up();
                } else if self.step == ConnectStep::SelectTls {
                    self.cycle_tls_policy(false);
                } else if self.step == ConnectStep::SelectCredentialKind {
                    self.cycle_credential_kind(false);
                }
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                if self.step == ConnectStep::SelectProvider {
                    self.cursor_down();
                } else if self.step == ConnectStep::SelectTls {
                    self.cycle_tls_policy(true);
                } else if self.step == ConnectStep::SelectCredentialKind {
                    self.cycle_credential_kind(true);
                }
                None
            }
            crossterm::event::KeyCode::Enter => match self.step {
                ConnectStep::SelectProvider => {
                    if self.load_error.is_some() {
                        None
                    } else {
                        self.advance_from_provider_selection();
                        None
                    }
                }
                ConnectStep::EnterEndpoint => {
                    self.next_after_endpoint();
                    None
                }
                ConnectStep::SelectCredentialKind => {
                    self.next_after_credential_kind();
                    None
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
                    // Eggpool-class presets with both credential kinds choose
                    // explicitly; single-kind presets continue directly.
                    let needs_choice = self
                        .selected_entry()
                        .is_some_and(|entry| entry.needs_credential_choice());
                    if needs_choice {
                        self.move_to_credential_kind_step();
                    } else {
                        self.move_to_api_key_step();
                    }
                    None
                }
                ConnectStep::EnterApiKey => {
                    let api_key = self.get_api_key();
                    if api_key.trim().is_empty() {
                        let label = match self.credential_kind {
                            ProviderCredentialKind::Bearer => "Bearer token",
                            ProviderCredentialKind::ApiKey => "API key",
                        };
                        self.set_error(format!("{label} cannot be empty"));
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

    fn hit_test(&self, rel_y: usize) -> Option<usize> {
        self.hit_test_provider_row(rel_y)
    }

    fn set_selected(&mut self, idx: usize) {
        if idx < self.providers.len() {
            self.selected = idx;
            self.list_state.select(Some(idx));
            self.clamp_scroll();
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

                if self.loading {
                    let loading = Paragraph::new(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            "Loading providers from daemon…",
                            Style::default().fg(theme.foreground),
                        )),
                    ])
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: true });
                    frame.render_widget(loading, inner_area);
                    let footer = Paragraph::new(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            " Esc close ",
                            Style::default().fg(theme.muted),
                        )),
                    ])
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                    frame.render_widget(footer, chunks[1]);
                    return;
                }

                if let Some(error) = self.load_error.as_ref() {
                    let failed = Paragraph::new(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            format!("Failed to load providers: {error}"),
                            Style::default().fg(theme.error),
                        )),
                        Line::from(""),
                        Line::from(Span::styled(
                            "Press Esc and retry /connect.",
                            Style::default().fg(theme.muted),
                        )),
                    ])
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: true });
                    frame.render_widget(failed, inner_area);
                    let footer = Paragraph::new(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            " Esc close ",
                            Style::default().fg(theme.muted),
                        )),
                    ])
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true });
                    frame.render_widget(footer, chunks[1]);
                    return;
                }

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

                    let name = if provider.is_selectable() {
                        provider.name.clone()
                    } else {
                        format!("{} (not supported)", provider.name)
                    };
                    let mut lines = vec![Line::from(Span::styled(name, style))];

                    if !provider.description.is_empty() {
                        lines.push(Line::from(Span::styled(
                            &provider.description,
                            Style::default().fg(theme.muted),
                        )));
                    }

                    let api_key_status = if !provider.is_selectable() {
                        "Not yet supported in /connect".to_string()
                    } else if provider.credential_kinds.iter().any(|k| k == "bearer")
                        && provider.credential_kinds.iter().any(|k| k == "api_key")
                    {
                        if let Some(env_var) = &provider.env_var_name {
                            format!("Credential: API key or bearer (or {env_var})")
                        } else {
                            "Credential: API key or bearer".to_string()
                        }
                    } else if provider.requires_api_key {
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
            ConnectStep::EnterEndpoint
            | ConnectStep::SelectCredentialKind
            | ConnectStep::EnterHost
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
                    ConnectStep::EnterEndpoint => {
                        let label = if provider.requires_endpoint {
                            "Endpoint (required, full base URL):"
                        } else {
                            "Endpoint override (optional, blank for default):"
                        };
                        (label, self.form_input.as_str())
                    }
                    ConnectStep::SelectCredentialKind => {
                        ("Credential kind: choose with ↑/↓, press Enter", "")
                    }
                    ConnectStep::EnterApiKey => {
                        let label = match self.credential_kind {
                            codegg_protocol::provider::ProviderCredentialKind::Bearer => {
                                "Bearer token (masked):"
                            }
                            codegg_protocol::provider::ProviderCredentialKind::ApiKey => {
                                "API key (masked):"
                            }
                        };
                        (label, self.api_key_input.as_str())
                    }
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
                    ConnectStep::SelectProvider => ("Select a provider:", ""),
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

                if self.step == ConnectStep::EnterEndpoint {
                    if let Some(default) = provider.default_endpoint.as_ref() {
                        if !default.is_empty() {
                            lines.push(Line::from(""));
                            lines.push(Line::from(Span::styled(
                                format!("Default: {default}"),
                                Style::default().fg(theme.muted),
                            )));
                        }
                    }
                }

                if self.step == ConnectStep::Review {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("Provider: {}", provider.name),
                        Style::default().fg(theme.muted),
                    )));
                    match provider.form_kind() {
                        ConnectFormKind::EggpoolProxy => {
                            lines.push(Line::from(Span::styled(
                                format!("Host: {} Port: {}", self.host, self.port),
                                Style::default().fg(theme.muted),
                            )));
                        }
                        ConnectFormKind::RequiredEndpoint | ConnectFormKind::OptionalOverride => {
                            let endpoint = if self.endpoint.trim().is_empty() {
                                provider.default_endpoint.clone().unwrap_or_default()
                            } else {
                                self.endpoint.clone()
                            };
                            if !endpoint.is_empty() {
                                lines.push(Line::from(Span::styled(
                                    format!("Endpoint: {endpoint}"),
                                    Style::default().fg(theme.muted),
                                )));
                            }
                        }
                        ConnectFormKind::Fixed => {}
                    }
                    lines.push(Line::from(Span::styled(
                        format!("Credential: {}", self.credential_kind_label()),
                        Style::default().fg(theme.muted),
                    )));
                    if !self.display_name.trim().is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("Display name: {}", self.display_name.trim()),
                            Style::default().fg(theme.muted),
                        )));
                    }
                }

                lines.push(Line::from(""));

                let input_text = if matches!(
                    self.step,
                    ConnectStep::SelectTls
                        | ConnectStep::SelectCredentialKind
                        | ConnectStep::SelectScope
                        | ConnectStep::Review
                ) {
                    format!(
                        "> {}",
                        match self.step {
                            ConnectStep::SelectTls => match self.tls_policy {
                                EggpoolTlsPolicy::Required => "Required TLS; press Enter",
                                EggpoolTlsPolicy::Optional => "Optional TLS; press Enter",
                                EggpoolTlsPolicy::Disabled => "TLS disabled; press Enter",
                            },
                            ConnectStep::SelectCredentialKind => match self.credential_kind {
                                codegg_protocol::provider::ProviderCredentialKind::ApiKey => {
                                    "API key; press Enter"
                                }
                                codegg_protocol::provider::ProviderCredentialKind::Bearer => {
                                    "Bearer token; press Enter"
                                }
                            },
                            ConnectStep::SelectScope => "Personal; press Enter",
                            ConnectStep::Review => "submit",
                            _ => "continue",
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

#[cfg(test)]
mod connect_restoration_tests {
    use super::*;
    use crate::tui::components::component::Component;
    use codegg_protocol::provider::ProviderSetupEntryDto;

    fn dto(
        id: &str,
        endpoint_policy: &str,
        credential_kinds: &[&str],
        requires_endpoint: bool,
    ) -> ProviderSetupEntryDto {
        ProviderSetupEntryDto {
            id: id.to_string(),
            display_name: format!("{id} display"),
            description: format!("{id} description"),
            connectable: true,
            credential_kinds: credential_kinds.iter().map(|k| k.to_string()).collect(),
            endpoint_policy: endpoint_policy.to_string(),
            default_endpoint: None,
            requires_endpoint,
            env_var: None,
        }
    }

    fn dialog_with(dtos: &[ProviderSetupEntryDto]) -> ConnectDialog {
        let theme = Arc::new(Theme::dark());
        let mut dialog = ConnectDialog::new_loading(Arc::clone(&theme));
        dialog.set_setup_entries(dtos);
        dialog
    }

    #[test]
    fn loading_state_has_no_eggpool_fallback() {
        let dialog = ConnectDialog::new_loading(Arc::new(Theme::dark()));
        assert!(dialog.is_loading());
        assert!(dialog.providers.is_empty());
        assert!(dialog.load_error.is_none());
        assert!(dialog.selected_entry().is_none());
    }

    #[test]
    fn setup_entries_come_from_the_catalog() {
        let dtos = vec![
            dto("openai", "optional_override", &["api_key"], false),
            dto("eggpool", "proxy_preset", &["api_key", "bearer"], true),
        ];
        let dialog = dialog_with(&dtos);
        assert!(!dialog.is_loading());
        assert_eq!(dialog.providers.len(), 2);
        assert_eq!(dialog.providers[0].id, "openai");
        assert_eq!(dialog.providers[1].id, "eggpool");
        assert_eq!(
            dialog.providers[0].form_kind(),
            ConnectFormKind::OptionalOverride
        );
        assert_eq!(
            dialog.providers[1].form_kind(),
            ConnectFormKind::EggpoolProxy
        );
    }

    #[test]
    fn ordinary_fixed_provider_skips_eggpool_fields() {
        let dtos = vec![dto("openai", "fixed", &["api_key"], false)];
        let mut dialog = dialog_with(&dtos);
        assert!(dialog.advance_from_provider_selection());
        // Fixed single-kind providers go straight to secret entry: no host,
        // port, TLS, or endpoint steps.
        assert_eq!(dialog.step, ConnectStep::EnterApiKey);
        assert_eq!(dialog.credential_kind, ProviderCredentialKind::ApiKey);
    }

    #[test]
    fn fixed_dual_kind_provider_requires_credential_choice() {
        let dtos = vec![dto("mistral", "fixed", &["api_key", "bearer"], false)];
        let mut dialog = dialog_with(&dtos);
        assert!(dialog.advance_from_provider_selection());
        assert_eq!(dialog.step, ConnectStep::SelectCredentialKind);
        dialog.cycle_credential_kind(true);
        assert_eq!(dialog.credential_kind, ProviderCredentialKind::Bearer);
        dialog.next_after_credential_kind();
        assert_eq!(dialog.step, ConnectStep::EnterApiKey);
        // Back from secret returns to the kind choice, never to TLS.
        assert!(dialog.back_step());
        assert_eq!(dialog.step, ConnectStep::SelectCredentialKind);
    }

    #[test]
    fn required_endpoint_provider_validates_without_preset_port() {
        let dtos = vec![dto("custom", "required", &["api_key", "bearer"], true)];
        let mut dialog = dialog_with(&dtos);
        assert!(dialog.advance_from_provider_selection());
        assert_eq!(dialog.step, ConnectStep::EnterEndpoint);
        // Empty endpoint is rejected for required providers.
        assert!(!dialog.next_after_endpoint());
        assert_eq!(dialog.step, ConnectStep::EnterEndpoint);
        dialog.form_input = "https://models.example/v1".to_string();
        dialog.cursor_pos = dialog.form_input.len();
        assert!(dialog.next_after_endpoint());
        assert_eq!(dialog.step, ConnectStep::SelectCredentialKind);
    }

    #[test]
    fn optional_override_accepts_blank_endpoint() {
        let dtos = vec![dto("openai", "optional_override", &["api_key"], false)];
        let mut dialog = dialog_with(&dtos);
        assert!(dialog.advance_from_provider_selection());
        assert_eq!(dialog.step, ConnectStep::EnterEndpoint);
        assert!(dialog.next_after_endpoint());
        assert_eq!(dialog.step, ConnectStep::EnterApiKey);
        assert!(dialog.endpoint.is_empty());
    }

    #[test]
    fn eggpool_preset_keeps_host_port_tls_flow() {
        let dtos = vec![dto("eggpool", "proxy_preset", &["api_key", "bearer"], true)];
        let mut dialog = dialog_with(&dtos);
        assert!(dialog.advance_from_provider_selection());
        assert_eq!(dialog.step, ConnectStep::EnterHost);
        dialog.form_input = "127.0.0.1".to_string();
        dialog.cursor_pos = dialog.form_input.len();
        dialog.commit_form_input();
        dialog.form_input = "11300".to_string();
        dialog.cursor_pos = dialog.form_input.len();
        dialog.step = ConnectStep::EnterPort;
        dialog.commit_form_input();
        assert_eq!(dialog.host, "127.0.0.1");
        assert_eq!(dialog.port, "11300");
        // Back from host returns to provider selection.
        dialog.step = ConnectStep::EnterHost;
        assert!(dialog.back_step());
        assert_eq!(dialog.step, ConnectStep::SelectProvider);
    }

    #[test]
    fn disabled_rows_are_visible_but_not_selectable() {
        let mut disabled = dto("future", "fixed", &["api_key"], false);
        disabled.connectable = false;
        let mut dialog = dialog_with(&[disabled]);
        assert!(!dialog.providers[0].is_selectable());
        assert!(!dialog.advance_from_provider_selection());
        assert_eq!(dialog.step, ConnectStep::SelectProvider);
        assert!(dialog.error_message.is_some());
    }

    #[test]
    fn secret_is_masked_rendered_and_forgotten_on_clone() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let dtos = vec![dto("openai", "fixed", &["api_key"], false)];
        let mut dialog = dialog_with(&dtos);
        assert!(dialog.advance_from_provider_selection());
        let secret = "sk-connect-restoration-secret-1";
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        let _ = key;
        for ch in secret.chars() {
            dialog.insert_char(ch);
        }
        assert_eq!(dialog.get_api_key(), secret);
        // Focus/render clones never receive the plaintext.
        let clone = dialog.clone();
        assert!(clone.api_key_input.is_empty());
        // Rendered output masks the secret and never contains it.
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                use ratatui::widgets::Widget;
                (&dialog).render(frame.area(), frame.buffer_mut());
            })
            .expect("draw connect dialog");
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect();
        assert!(
            !content.contains(secret),
            "rendered dialog must not contain the secret"
        );
        // Cancellation forgets the secret.
        dialog.clear_secret();
        assert!(dialog.get_api_key().is_empty());
    }

    #[test]
    fn provider_rows_hit_test_with_scroll() {
        let dtos = vec![
            dto("openai", "fixed", &["api_key"], false),
            dto("anthropic", "fixed", &["api_key"], false),
            dto("eggpool", "proxy_preset", &["api_key", "bearer"], true),
        ];
        let dialog = dialog_with(&dtos);
        // Three content rows per entry: rows 1-3 map to entry 0.
        assert_eq!(dialog.hit_test_provider_row(1), Some(0));
        assert_eq!(dialog.hit_test_provider_row(3), Some(0));
        assert_eq!(dialog.hit_test_provider_row(4), Some(1));
        assert_eq!(dialog.hit_test_provider_row(7), Some(2));
        assert_eq!(dialog.hit_test_provider_row(0), None);
        assert_eq!(dialog.hit_test_provider_row(100), None);
        // Component-trait path agrees.
        assert_eq!(Component::hit_test(&dialog, 4), Some(1));
    }

    #[test]
    fn rotation_editor_closes_from_secret_entry() {
        let theme = Arc::new(Theme::dark());
        let mut dialog = ConnectDialog::new(
            vec![ProviderInfo::api_key("eggpool", "Eggpool", "Rotate", None)],
            theme,
        );
        dialog.set_rotation_target("connection-1".to_string(), 3);
        assert_eq!(dialog.step, ConnectStep::EnterApiKey);
        // Esc from the rotation secret entry closes instead of entering the
        // provisioning TLS flow.
        assert!(!dialog.back_step());
        assert!(dialog.get_api_key().is_empty());
    }
}
