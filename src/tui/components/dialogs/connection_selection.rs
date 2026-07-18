//! Provider Connections Milestone 3: connection + model selection dialog.
//!
//! This dialog is intentionally read-only on the connection list (the
//! daemon owns the connection catalog) and write-only on the session
//! selection (the daemon's selection service writes back). It never
//! constructs a provider, never stores a credential, and never resolves
//! a secret locally.

use std::sync::Arc;

use crate::protocol::provider::{
    ProviderConnectionSummaryDto, SelectedModelDto, SessionSelectionDto,
};
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// State for the connection selection dialog.
pub struct ConnectionSelectionDialog {
    pub session_id: String,
    pub selection: Option<SessionSelectionDto>,
    pub connections: Vec<ProviderConnectionSummaryDto>,
    pub models: Vec<SelectedModelDto>,
    pub connection_idx: usize,
    pub model_idx: usize,
    pub last_error: Option<String>,
    pub loading: bool,
}

impl ConnectionSelectionDialog {
    #[allow(dead_code)]
    pub fn new(session_id: String, _theme: Arc<Theme>) -> Self {
        Self {
            session_id,
            selection: None,
            connections: Vec::new(),
            models: Vec::new(),
            connection_idx: 0,
            model_idx: 0,
            last_error: None,
            loading: true,
        }
    }

    pub fn set_selection(&mut self, selection: SessionSelectionDto) {
        self.selection = Some(selection);
    }

    pub fn set_connections(&mut self, connections: Vec<ProviderConnectionSummaryDto>) {
        self.connections = connections;
        if self.connection_idx >= self.connections.len() {
            self.connection_idx = 0;
        }
    }

    pub fn set_models(&mut self, models: Vec<SelectedModelDto>) {
        self.models = models;
        if self.model_idx >= self.models.len() {
            self.model_idx = 0;
        }
    }

    pub fn set_error(&mut self, message: String) {
        self.last_error = Some(message);
        self.loading = false;
    }

    pub fn finish_loading(&mut self) {
        self.loading = false;
    }

    fn move_connection(&mut self, delta: i64) {
        if self.connections.is_empty() {
            return;
        }
        let len = self.connections.len() as i64;
        let next = (self.connection_idx as i64 + delta).rem_euclid(len);
        self.connection_idx = next as usize;
        self.model_idx = 0;
    }

    fn move_model(&mut self, delta: i64) {
        if self.models.is_empty() {
            return;
        }
        let len = self.models.len() as i64;
        let next = (self.model_idx as i64 + delta).rem_euclid(len);
        self.model_idx = next as usize;
    }

    /// Build the optimistic-revision update request for the currently
    /// highlighted connection + model pair. The dialog does not know
    /// the current revisions; the daemon will detect any conflict.
    pub fn pending_update(&self) -> Option<PendingSelectionUpdate> {
        let connection = self.connections.get(self.connection_idx)?;
        let model = self.models.get(self.model_idx)?;
        Some(PendingSelectionUpdate {
            session_id: self.session_id.clone(),
            connection_id: connection.id.clone(),
            connection_revision: connection.revision,
            model_id: model.model_id.clone(),
            catalog_revision: model.catalog_revision.clone(),
        })
    }
}

/// Snapshot of the user's pending selection. Returned by
/// [`ConnectionSelectionDialog::pending_update`] so the TUI command
/// layer can build a typed `CoreRequest::SessionSelectionUpdate`.
#[derive(Debug, Clone)]
pub struct PendingSelectionUpdate {
    pub session_id: String,
    pub connection_id: String,
    pub connection_revision: u64,
    pub model_id: String,
    pub catalog_revision: String,
}

impl Component for ConnectionSelectionDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<crate::tui::app::TuiMsg> {
        let selected = || {
            self.connections
                .get(self.connection_idx)
                .map(|connection| (connection.id.clone(), connection.revision))
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Some(crate::tui::app::TuiMsg::CloseDialog),
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_connection(1);
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_connection(-1);
                None
            }
            KeyCode::Tab => {
                self.move_model(1);
                None
            }
            KeyCode::BackTab => {
                self.move_model(-1);
                None
            }
            KeyCode::Enter => {
                let pending = self.pending_update();
                pending.map(|p| crate::tui::app::TuiMsg::SubmitSelectionUpdate {
                    session_id: p.session_id,
                    connection_id: p.connection_id,
                    connection_revision: Some(p.connection_revision),
                    model_id: p.model_id,
                    catalog_revision: Some(p.catalog_revision),
                })
            }
            // `r` is a bounded manual catalog refresh; uppercase `R` opens
            // the masked local-only credential rotation flow.
            KeyCode::Char('r') => selected().map(|(connection_id, expected_revision)| {
                crate::tui::app::TuiMsg::ConnectionLifecycle {
                    action: crate::tui::app::ConnectionLifecycleAction::Refresh,
                    connection_id,
                    expected_revision,
                }
            }),
            KeyCode::Char('R') => selected().map(|(connection_id, expected_revision)| {
                crate::tui::app::TuiMsg::OpenConnectionRotation {
                    connection_id,
                    expected_revision,
                }
            }),
            KeyCode::Char('e') => selected().map(|(connection_id, expected_revision)| {
                let action = self
                    .connections
                    .get(self.connection_idx)
                    .map(|connection| connection.state.as_str())
                    .map(|state| {
                        if state == "active" {
                            crate::tui::app::ConnectionLifecycleAction::Disable
                        } else {
                            crate::tui::app::ConnectionLifecycleAction::Enable
                        }
                    })
                    .unwrap_or(crate::tui::app::ConnectionLifecycleAction::Enable);
                crate::tui::app::TuiMsg::ConnectionLifecycle {
                    action,
                    connection_id,
                    expected_revision,
                }
            }),
            KeyCode::Char('d') => selected().map(|(connection_id, expected_revision)| {
                crate::tui::app::TuiMsg::ConnectionLifecycle {
                    action: crate::tui::app::ConnectionLifecycleAction::Delete,
                    connection_id,
                    expected_revision,
                }
            }),
            KeyCode::Char('u') => selected().map(|(connection_id, expected_revision)| {
                crate::tui::app::TuiMsg::ConnectionLifecycle {
                    action: crate::tui::app::ConnectionLifecycleAction::Restore,
                    connection_id,
                    expected_revision,
                }
            }),
            KeyCode::Char('p') => selected().map(|(connection_id, expected_revision)| {
                crate::tui::app::TuiMsg::ConnectionLifecycle {
                    action: crate::tui::app::ConnectionLifecycleAction::Purge,
                    connection_id,
                    expected_revision,
                }
            }),
            _ => None,
        }
    }

    fn update(&mut self, msg: crate::tui::app::TuiMsg) -> Option<crate::tui::app::TuiMsg> {
        match msg {
            crate::tui::app::TuiMsg::SessionSelectionUpdated { selection, .. } => {
                self.set_selection(selection);
                None
            }
            crate::tui::app::TuiMsg::ProviderConnectionsUpdated { connections } => {
                self.set_connections(connections);
                None
            }
            crate::tui::app::TuiMsg::ProviderConnectionModelsUpdated { models, .. } => {
                self.set_models(models);
                None
            }
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Line::from(vec![
                Span::raw("Select Connection & Model — "),
                Span::styled(
                    self.session_id.as_str(),
                    ratatui::style::Style::default().fg(theme.primary),
                ),
            ]));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.loading {
            frame.render_widget(Paragraph::new("Loading connections and models…"), inner);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner);

        let mut items = Vec::with_capacity(self.connections.len());
        for connection in &self.connections {
            let label = format!(
                "{} ({}) — {} models — {}",
                connection.display_name,
                connection.provider_kind,
                connection.model_count,
                connection.state,
            );
            items.push(ListItem::new(label));
        }
        let mut list_state = ListState::default();
        list_state.select(Some(self.connection_idx));
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Connections"))
            .highlight_style(ratatui::style::Style::default().fg(theme.primary))
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, chunks[0], &mut list_state);

        let mut model_items = Vec::with_capacity(self.models.len());
        for model in &self.models {
            let label = format!(
                "{} — ctx={} tools={} vision={}",
                model.model_name, model.context_window, model.supports_tools, model.supports_vision
            );
            model_items.push(ListItem::new(label));
        }
        let mut model_state = ListState::default();
        model_state.select(Some(self.model_idx));
        let models_list = List::new(model_items)
            .block(Block::default().borders(Borders::ALL).title("Models"))
            .highlight_style(ratatui::style::Style::default().fg(theme.primary))
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(models_list, chunks[1], &mut model_state);

        if let Some(selection) = &self.selection {
            let footer = match selection {
                SessionSelectionDto::Selected {
                    connection,
                    model,
                    connection_revision,
                    catalog_revision,
                } => format!(
                    "Current: {} / {} (rev={} catalog={})",
                    connection.display_name,
                    model.model_name,
                    connection_revision,
                    catalog_revision
                ),
                SessionSelectionDto::LegacyUnresolved {
                    legacy_provider,
                    reason,
                    ..
                } => format!("Legacy unresolved ({}): {}", legacy_provider, reason),
                SessionSelectionDto::Unselected {} => {
                    "No active connection selected for this session.".to_string()
                }
            };
            let footer_para = Paragraph::new(footer).block(Block::default().borders(Borders::TOP));
            let footer_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(3),
                width: inner.width,
                height: 3,
            };
            frame.render_widget(footer_para, footer_area);
        }

        if let Some(err) = &self.last_error {
            let error_para = Paragraph::new(err.as_str())
                .style(ratatui::style::Style::default().fg(theme.error));
            let err_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(1),
                width: inner.width,
                height: 1,
            };
            frame.render_widget(error_para, err_area);
        }
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::ConnectionSelection
    }
}
