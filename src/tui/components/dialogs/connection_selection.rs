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
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// State for the connection selection dialog.
#[derive(Clone)]
pub struct ConnectionSelectionDialog {
    pub session_id: String,
    pub selection: Option<SessionSelectionDto>,
    pub connections: Vec<ProviderConnectionSummaryDto>,
    pub models: Vec<SelectedModelDto>,
    pub connection_idx: usize,
    pub model_idx: usize,
    /// Which pane `j`/`k` and the arrows currently move inside.
    ///
    /// Defaults to [`ConnectionColumn::Connections`] so the pre-existing
    /// `j`/`k` connection navigation is unchanged until the user presses `l`.
    pub column: ConnectionColumn,
    pub last_error: Option<String>,
    pub loading: bool,
}

/// The pane that owns vertical navigation in the connection dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionColumn {
    Connections,
    Models,
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
            column: ConnectionColumn::Connections,
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

    /// The model pane is per-connection, so moving the highlight must
    /// trigger a refetch for the newly highlighted row. Returning `None`
    /// here left the previous connection's models on screen under a
    /// different highlighted row, and `pending_update` pairs that list with
    /// the highlighted connection — so Enter could bind one connection's
    /// model to another connection.
    fn focus_changed(&self) -> Option<crate::tui::app::TuiMsg> {
        self.connections.get(self.connection_idx).map(|connection| {
            crate::tui::app::TuiMsg::ConnectionFocusChanged {
                connection_id: connection.id.clone(),
            }
        })
    }

    fn move_model(&mut self, delta: i64) {
        if self.models.is_empty() {
            return;
        }
        let len = self.models.len() as i64;
        let next = (self.model_idx as i64 + delta).rem_euclid(len);
        self.model_idx = next as usize;
    }

    /// Move vertically inside the column that currently owns the focus.
    ///
    /// A connection move also refetches, because the model pane is scoped to
    /// one connection; a model move needs no refetch, so it returns no message
    /// and the rendered highlight is the only thing that changes.
    fn move_within_column(&mut self, delta: i64) -> Option<crate::tui::app::TuiMsg> {
        match self.column {
            ConnectionColumn::Connections => {
                self.move_connection(delta);
                self.focus_changed()
            }
            ConnectionColumn::Models => {
                self.move_model(delta);
                None
            }
        }
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

/// Number of leading connection-id characters shown on a row.
///
/// Eight is enough to separate two rows that are otherwise identical while
/// staying short enough to leave room for the credential hint.
const CONNECTION_ID_PREVIEW_LEN: usize = 8;

/// Short, stable discriminator for one connection id.
fn short_connection_id(id: &str) -> String {
    id.chars().take(CONNECTION_ID_PREVIEW_LEN).collect()
}

/// Title for one pane, marking which column owns `j`/`k` right now.
///
/// Both panes keep their own `▶` highlight row, so without a title-level
/// marker two highlighted rows are on screen at once and the user cannot tell
/// which one `j` will move.
fn column_title(
    label: &str,
    active: ConnectionColumn,
    column: ConnectionColumn,
    theme: &Arc<Theme>,
) -> Line<'static> {
    if active == column {
        Line::from(Span::styled(
            format!("▸ {label} ◂"),
            ratatui::style::Style::default().fg(theme.primary),
        ))
    } else {
        Line::from(Span::raw(format!("  {label}  ")))
    }
}

/// Render one connection row.
///
/// A display name is not an identity: two connections can legitimately share
/// one (`eggstackmain` twice on the same provider, differing only by id and
/// bound key), and `R`/`d`/`e` act on whichever row is highlighted. The row
/// therefore carries three discriminators — a short id, the masked credential
/// tail when one is bound, and the current state — so the user can always say
/// which key they are about to replace.
///
/// They come *before* the descriptive tail on purpose. The connections pane is
/// half a narrow terminal, and a discriminator pushed behind the provider kind
/// and model count is exactly the part that gets truncated away at 120 columns
/// — reintroducing the ambiguity this row exists to remove.
///
/// The hint is exactly the four trailing characters the daemon computed; the
/// dialog never sees, derives, or widens it. Connections with no binding simply
/// omit it and fall back to the short id.
fn connection_row_label(connection: &ProviderConnectionSummaryDto) -> String {
    let mut label = format!(
        "{} · {}",
        connection.display_name,
        short_connection_id(&connection.id),
    );
    if let Some(hint) = connection.credential_hint.as_deref() {
        label.push_str(" · ");
        label.push_str(hint);
    }
    label.push_str(&format!(
        " · {} · ({}) · {} models",
        connection.state, connection.provider_kind, connection.model_count
    ));
    label
}

impl Component for ConnectionSelectionDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<crate::tui::app::TuiMsg> {
        // While the catalog is loading the dialog renders only the
        // "Loading connections and models…" placeholder, so there is nothing
        // on screen for a keypress to refer to. Accepting lifecycle keys in
        // that window is how a connection gets disabled or purged without the
        // user ever seeing the row they acted on — `d` and `p` are
        // single-key destructive actions with no confirmation step.
        //
        // `selected()` would return `None` while `connections` is still
        // empty, which makes this inert today, but that is an accident of
        // initialization order rather than a contract: `set_connections`
        // does not clear `loading`, and the refresh response is delivered to
        // the *live* focus-manager clone, so any ordering that lands rows
        // before `finish_loading()` would make the destructive keys live
        // against an invisible list. Gate on `loading` so the placeholder
        // window is inert regardless of arrival order — dismissal is the
        // only action it accepts.
        if self.loading {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('q') => Some(crate::tui::app::TuiMsg::CloseDialog),
                _ => None,
            };
        }

        let selected = || {
            self.connections
                .get(self.connection_idx)
                .map(|connection| (connection.id.clone(), connection.revision))
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Some(crate::tui::app::TuiMsg::CloseDialog),
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_within_column(1);
                self.focus_changed()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_within_column(-1);
                self.focus_changed()
            }
            // `h`/`l` switch columns; `j`/`k` (and the arrows) move inside
            // whichever column owns the focus.
            //
            // `Tab` was the only model-navigation binding this dialog had, and
            // it was unreachable: `FocusManager::handle_key` consumes `Tab` and
            // `Shift+Tab` for dialog focus traversal before dispatching to the
            // component, so `move_model` never ran. The model pane could be
            // rendered and could never be selected from — Enter always bound
            // whatever sat at index 0. Column focus is bound to keys the focus
            // manager does not intercept.
            KeyCode::Char('h') | KeyCode::Left => {
                self.column = ConnectionColumn::Connections;
                None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.column = ConnectionColumn::Models;
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
            items.push(ListItem::new(connection_row_label(connection)));
        }
        let mut list_state = ListState::default();
        list_state.select(Some(self.connection_idx));
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(column_title(
                "Connections",
                self.column,
                ConnectionColumn::Connections,
                theme,
            )))
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
            .block(Block::default().borders(Borders::ALL).title(column_title(
                "Models",
                self.column,
                ConnectionColumn::Models,
                theme,
            )))
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
                y: inner.y + inner.height.saturating_sub(4),
                width: inner.width,
                height: 3,
            };
            frame.render_widget(footer_para, footer_area);
        }

        // The lifecycle verbs are the whole reason to open this dialog on a
        // connection you cannot use, and an unlabelled dialog cannot tell the
        // user they exist — pressing Enter on a row refreshes models, which
        // reads as "this dialog only picks models". Printed here so `e`, `d`,
        // `R`, `u` and `p` are discoverable without reading the source.
        let legend = Paragraph::new(Line::from(vec![
            Span::styled(
                "h/l column · j/k move · Enter use · ",
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                "e enable/disable · d delete · R rotate key · r refresh · u restore · p purge (blocked delete)",
                Style::default().fg(theme.muted),
            ),
        ]));
        let legend_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        frame.render_widget(legend, legend_area);

        if let Some(err) = &self.last_error {
            let error_para = Paragraph::new(err.as_str())
                .style(ratatui::style::Style::default().fg(theme.error));
            // Sits directly above the legend, not on top of it: an error the
            // user cannot read is the same dead end as a key they cannot find.
            let err_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(2),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn connection(
        id: &str,
        name: &str,
        state: &str,
        hint: Option<&str>,
    ) -> ProviderConnectionSummaryDto {
        ProviderConnectionSummaryDto {
            id: id.to_string(),
            provider_kind: "other:opencode_go".to_string(),
            display_name: name.to_string(),
            endpoint: "https://opencode.ai/zen/go/v1".to_string(),
            tls_policy: "required".to_string(),
            scope: "personal:local-user".to_string(),
            state: state.to_string(),
            revision: 3,
            model_count: 4,
            catalog_revision: None,
            health: None,
            credential_hint: hint.map(ToOwned::to_owned),
        }
    }

    fn model(id: &str) -> SelectedModelDto {
        SelectedModelDto {
            connection_id: "conn-1".to_string(),
            model_id: id.to_string(),
            model_name: id.to_string(),
            context_window: 200_000,
            max_output_tokens: Some(8_192),
            supports_tools: true,
            supports_vision: false,
            catalog_revision: "rev-19".to_string(),
        }
    }

    fn loaded_dialog() -> ConnectionSelectionDialog {
        let theme = Arc::new(Theme::default());
        let mut dialog =
            ConnectionSelectionDialog::new("session-1".to_string(), Arc::clone(&theme));
        dialog.loading = false;
        dialog.connections = vec![connection(
            "8b8622d5-9866-4b41-807e-837cc15b69a7",
            "opencode-go",
            "active",
            Some("…94e6"),
        )];
        dialog.set_models(vec![
            model("cat-jam-free"),
            model("mimo-v2.6-flash"),
            model("glm-5.3"),
        ]);
        dialog
    }

    /// The model pane must be reachable without `Tab`.
    ///
    /// `Tab` was the only model-navigation binding this dialog had and it never
    /// fired: `FocusManager::handle_key` consumes `Tab`/`Shift+Tab` for dialog
    /// focus traversal before dispatching to the component, so `move_model` was
    /// unreachable. The pane rendered 37 models and Enter bound index 0 every
    /// time, which happened to be `cat-jam-free` — a model with no wire
    /// mapping — so no model could ever actually be selected.
    #[test]
    fn the_model_pane_is_reachable_without_tab() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut dialog = loaded_dialog();
        assert_eq!(dialog.model_idx, 0);
        assert_eq!(dialog.pending_update().unwrap().model_id, "cat-jam-free");

        // `l` moves focus into the model column...
        dialog.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        assert_eq!(dialog.column, ConnectionColumn::Models);

        // ...and `j` then moves the highlight that Enter will commit.
        dialog.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(dialog.model_idx, 1);
        assert_eq!(dialog.pending_update().unwrap().model_id, "mimo-v2.6-flash");

        dialog.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(dialog.model_idx, 2);

        // `h` returns focus to the connections column, where `j` moves rows
        // again — the pre-existing behaviour, unchanged while that column owns
        // the focus.
        dialog.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        assert_eq!(dialog.column, ConnectionColumn::Connections);
        dialog.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(
            dialog.model_idx, 0,
            "a model move does not reset on its own"
        );
    }

    /// The dialog opens with the connections column focused, so `j`/`k` keep
    /// meaning what they always meant until the user presses `l`.
    #[test]
    fn vertical_navigation_starts_on_the_connections_column() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut dialog = loaded_dialog();
        assert_eq!(dialog.column, ConnectionColumn::Connections);
        dialog.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(dialog.model_idx, 0, "j must not move models before `l`");
    }

    fn render_rows(connections: Vec<ProviderConnectionSummaryDto>) -> String {
        let theme = Arc::new(Theme::default());
        let mut dialog =
            ConnectionSelectionDialog::new("session-1".to_string(), Arc::clone(&theme));
        dialog.loading = false;
        dialog.connections = connections;
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| dialog.render(frame, frame.area(), &theme))
            .expect("dialog renders");
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The live complaint: two connections both named `eggstackmain`, both
    /// `other:opencode_go`, differing only by id and bound credential. With
    /// only the display name rendered they are the same row twice, so the user
    /// cannot tell which key `R` is about to replace.
    #[test]
    fn connections_sharing_a_display_name_render_as_distinguishable_rows() {
        let rendered = render_rows(vec![
            connection(
                "0f3c9a11-7b2e-4c55-9d0e-1a2b3c4d5e6f",
                "eggstackmain",
                "active",
                Some("…LooA"),
            ),
            connection(
                "9a1d2e33-84ff-4a0b-9c7d-6e5f4a3b2c1d",
                "eggstackmain",
                "disabled",
                Some("…9xQb"),
            ),
        ]);

        let row_lines: Vec<&str> = rendered
            .lines()
            .filter(|line| line.contains("eggstackmain"))
            .collect();
        assert_eq!(row_lines.len(), 2, "both rows must render");
        assert_ne!(
            row_lines[0], row_lines[1],
            "two identical names must not produce identical rows"
        );
        assert!(
            rendered.contains("…LooA"),
            "the credential hint must render"
        );
        assert!(rendered.contains("…9xQb"));
        assert!(
            rendered.contains("0f3c9a11") && rendered.contains("9a1d2e33"),
            "the short connection id must render"
        );
        assert!(rendered.contains("active") && rendered.contains("disabled"));
    }

    /// Even with no credential bound, two same-named rows stay separable by
    /// their short id — the hint is a bonus, never the only discriminator.
    #[test]
    fn same_named_rows_without_a_hint_stay_distinguishable() {
        let rendered = render_rows(vec![
            connection("0f3c9a11-7b2e-4c55", "eggstackmain", "active", None),
            connection("9a1d2e33-84ff-4a0b", "eggstackmain", "active", None),
        ]);
        let row_lines: Vec<&str> = rendered
            .lines()
            .filter(|line| line.contains("eggstackmain"))
            .collect();
        assert_eq!(row_lines.len(), 2);
        assert_ne!(row_lines[0], row_lines[1]);
        assert!(!rendered.contains('…'), "no binding means no ellipsis row");
    }

    /// The lifecycle legend and the error row must not be displaced by the
    /// richer rows: an error the user cannot read is the same dead end as a
    /// key they cannot find.
    #[test]
    fn legend_and_error_rows_still_render() {
        let theme = Arc::new(Theme::default());
        let mut dialog =
            ConnectionSelectionDialog::new("session-1".to_string(), Arc::clone(&theme));
        dialog.loading = false;
        dialog.connections = vec![connection(
            "0f3c9a11-7b2e-4c55",
            "eggstackmain",
            "active",
            Some("…LooA"),
        )];
        dialog.set_error("rotation failed: credential rejected".to_string());
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| dialog.render(frame, frame.area(), &theme))
            .expect("dialog renders");
        let buffer = terminal.backend().buffer().clone();
        let rendered: String = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("credential rejected"));
        assert!(rendered.contains("p purge"));
        assert!(rendered.contains("…LooA"));
    }

    #[test]
    fn short_connection_id_is_bounded_to_eight_characters() {
        assert_eq!(short_connection_id("0f3c9a11-7b2e"), "0f3c9a11");
        assert_eq!(short_connection_id("abc"), "abc");
        assert_eq!(short_connection_id(""), "");
    }
}
