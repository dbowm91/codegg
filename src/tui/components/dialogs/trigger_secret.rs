//! C001: one-time external-trigger bearer display.
//!
//! Transient secret-safe surface for the M005 `WorkOrderTriggerCreate`
//! response. The bearer exists only in [`OneTimeTriggerSecret`] (TUI-local
//! opaque wrapper with redacted `Debug`) and is rendered here exactly
//! once. Closing the dialog drops the secret; metadata remains listable
//! but the bearer is never re-readable (rotation is revoke + create).
//!
//! Secret-safety: no `Serialize` on secret state, no bearer in prompt/
//! transcript/notifications/audit/events/WorkOrder summaries/dashboard
//! rows/model context, no SQLite persistence, and the dialog clears on
//! project/tab switch, reconnect, logout/authority loss, and shutdown.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

/// Render snapshot for the secret dialog. Carries display strings only;
/// the bearer itself is read via `expose()` at render time and never
/// cloned into logs or secondary state.
#[derive(Debug, Clone)]
pub struct TriggerSecretSnapshot {
    pub project_id: String,
    pub work_order_id: String,
    pub trigger_id: String,
    pub endpoint_path: String,
    pub curl_example: String,
    pub bearer_len: usize,
}

#[derive(Clone)]
pub struct TriggerSecretDialog {
    snapshot: TriggerSecretSnapshot,
    /// Bearer handle for the single render path. Held only while the
    /// dialog is mounted; dropped on close.
    bearer: Arc<crate::tui::app::state::OneTimeBearer>,
}

impl TriggerSecretDialog {
    pub fn from_secret(secret: &crate::tui::app::state::OneTimeTriggerSecret) -> Self {
        let bearer_len = secret.bearer.expose().len();
        // Clone the bearer string into an Arc-held wrapper for render;
        // the source `OneTimeTriggerSecret` in DialogState remains the
        // owner and is dropped on close. The dialog never logs it.
        let bearer = Arc::new(
            crate::tui::app::state::OneTimeBearer::new(secret.bearer.expose().to_string())
                .expect("bearer already validated"),
        );
        Self {
            snapshot: TriggerSecretSnapshot {
                project_id: secret.project_id.clone(),
                work_order_id: secret.work_order_id.clone(),
                trigger_id: secret.trigger_id.clone(),
                endpoint_path: secret.endpoint_path(),
                curl_example: secret.bearer.curl_example(&secret.trigger_id),
                bearer_len,
            },
            bearer,
        }
    }

    pub fn snapshot(&self) -> &TriggerSecretSnapshot {
        &self.snapshot
    }
}

// Custom Debug: never print the bearer.
impl std::fmt::Debug for TriggerSecretDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TriggerSecretDialog")
            .field("snapshot_trigger_id", &self.snapshot.trigger_id)
            .field("snapshot_work_order_id", &self.snapshot.work_order_id)
            .field("bearer", &"REDACTED")
            .finish()
    }
}

impl Component for TriggerSecretDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => Some(TuiMsg::TriggerSecretClose),
            // Tab is consumed (never leaks to composer toggle).
            KeyCode::Tab => None,
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog | TuiMsg::TriggerSecretClose => Some(TuiMsg::TriggerSecretClose),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        if area.height < 10 || area.width < 40 {
            return;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.warning))
            .title(" External trigger created — copy once ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height == 0 || inner.width < 10 {
            return;
        }
        let bearer = self.bearer.expose();
        let lines = vec![
            Line::from(Span::styled(
                format!("Trigger: {}", self.snapshot.trigger_id),
                Style::default().fg(theme.foreground),
            )),
            Line::from(Span::styled(
                format!("WorkOrder: {}", short_id(&self.snapshot.work_order_id)),
                Style::default().fg(theme.muted),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "Bearer (shown once — copy now):",
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                bearer.to_string(),
                Style::default().fg(theme.foreground),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                format!("POST {}", self.snapshot.endpoint_path),
                Style::default().fg(theme.secondary),
            )),
            Line::from(Span::styled(
                self.snapshot.curl_example.clone(),
                Style::default().fg(theme.muted),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "Warning: this token cannot be shown again. If lost, rotate (revoke + create) from the Task view.",
                Style::default().fg(theme.warning),
            )),
            Line::from(Span::styled(
                "Base URL: set ${CODEGG_BASE_URL} to your reachable daemon; the path above is authoritative.",
                Style::default().fg(theme.muted),
            )),
            Line::from(Span::styled(
                "Enter/Esc closes and forgets the bearer (metadata stays listable).",
                Style::default().fg(theme.muted),
            )),
        ];
        let paragraph = Paragraph::new(lines).style(Style::default().fg(theme.foreground));
        frame.render_widget(paragraph, inner);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::TriggerSecret
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::{OneTimeBearer, OneTimeTriggerSecret, UiRouteToken};

    fn secret() -> OneTimeTriggerSecret {
        let route = UiRouteToken::new(None, None, None, None, 0, 0, 1);
        OneTimeTriggerSecret {
            project_id: "project-1".to_string(),
            work_order_id: "wo-1".to_string(),
            trigger_id: "trigger-1".to_string(),
            bearer: OneTimeBearer::new("cggtr_trigger-1.secret-abc".to_string()).unwrap(),
            route,
        }
    }

    #[test]
    fn secret_dialog_never_debug_prints_bearer() {
        let secret = secret();
        let dialog = TriggerSecretDialog::from_secret(&secret);
        let debug = format!("{dialog:?}");
        assert!(!debug.contains("secret-abc"));
        assert!(!debug.contains("cggtr_"));
        let bearer_debug = format!("{:?}", secret.bearer);
        assert!(!bearer_debug.contains("secret-abc"));
        assert!(bearer_debug.contains("REDACTED"));
    }

    #[test]
    fn secret_dialog_keys_close_without_leaking() {
        let secret = secret();
        let mut dialog = TriggerSecretDialog::from_secret(&secret);
        let key = |code: KeyCode| KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        assert_eq!(
            dialog.handle_key(key(KeyCode::Esc)),
            Some(TuiMsg::TriggerSecretClose)
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::TriggerSecretClose)
        );
        // Tab consumed, never composer toggle.
        assert_eq!(dialog.handle_key(key(KeyCode::Tab)), None);
    }

    #[test]
    fn curl_example_uses_bearer_header_and_idempotency() {
        let secret = secret();
        let dialog = TriggerSecretDialog::from_secret(&secret);
        assert!(dialog
            .snapshot
            .curl_example
            .contains("Authorization: Bearer"));
        assert!(dialog.snapshot.curl_example.contains("Idempotency-Key"));
        assert!(dialog
            .snapshot
            .endpoint_path
            .contains("/api/v1/task-triggers/trigger-1/fire"));
    }
}
