//! M003: one-time device-token credential display.
//!
//! Transient secret-safe surface for the `TeamTokenCreate` response. The
//! plaintext exists only in [`OneTimeDeviceToken`] (TUI-local opaque
//! wrapper with redacted `Debug`) and is rendered here exactly once.
//! Closing the dialog drops the credential; token metadata remains
//! listable but the plaintext is never re-readable (rotation is revoke +
//! create).
//!
//! Secret-safety: no `Serialize` on secret state, no credential in prompt/
//! transcript/notifications/audit/events/chat/dashboards/model context, no
//! SQLite persistence, and the dialog clears on project/tab switch,
//! reconnect, authority loss, and shutdown. The bearer is read via
//! `expose()` at render time and never cloned into logs or secondary
//! state.

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
/// the credential itself is read via `expose()` at render time and never
/// cloned into logs or secondary state.
#[derive(Debug, Clone)]
pub struct DeviceSecretSnapshot {
    pub principal_id: String,
    pub token_id: String,
    pub label: String,
    pub credential_len: usize,
}

#[derive(Clone)]
pub struct DeviceSecretDialog {
    snapshot: DeviceSecretSnapshot,
    /// Credential handle for the single render path. Held only while the
    /// dialog is mounted; dropped on close.
    credential: Arc<crate::tui::app::state::OneTimeBearer>,
}

impl DeviceSecretDialog {
    pub fn from_secret(secret: &crate::tui::app::state::OneTimeDeviceToken) -> Self {
        let credential_len = secret.token.expose().len();
        // Clone the credential into an Arc-held wrapper for render; the
        // source `OneTimeDeviceToken` in DialogState remains the owner
        // and is dropped on close. The dialog never logs it.
        let credential = Arc::new(
            crate::tui::app::state::OneTimeBearer::new_device_token(
                secret.token.expose().to_string(),
            )
            .expect("credential already validated"),
        );
        Self {
            snapshot: DeviceSecretSnapshot {
                principal_id: secret.principal_id.clone(),
                token_id: secret.token_id.clone(),
                label: secret.label.clone(),
                credential_len,
            },
            credential,
        }
    }

    pub fn snapshot(&self) -> &DeviceSecretSnapshot {
        &self.snapshot
    }
}

// Custom Debug: never print the credential.
impl std::fmt::Debug for DeviceSecretDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceSecretDialog")
            .field("snapshot_token_id", &self.snapshot.token_id)
            .field("snapshot_principal_id", &self.snapshot.principal_id)
            .field("credential", &"REDACTED")
            .finish()
    }
}

impl Component for DeviceSecretDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                Some(TuiMsg::TeamTokenSecretClose)
            }
            // Tab is consumed (never leaks to composer toggle).
            KeyCode::Tab => None,
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog | TuiMsg::TeamTokenSecretClose => {
                Some(TuiMsg::TeamTokenSecretClose)
            }
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
            .title(" Device token created — copy once ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height == 0 || inner.width < 10 {
            return;
        }
        let credential = self.credential.expose();
        let lines = vec![
            Line::from(Span::styled(
                format!("Token: {}", self.snapshot.token_id),
                Style::default().fg(theme.foreground),
            )),
            Line::from(Span::styled(
                format!(
                    "Principal: {}   Label: {}",
                    self.snapshot.principal_id, self.snapshot.label
                ),
                Style::default().fg(theme.muted),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "Credential (shown once — copy now):",
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                credential.to_string(),
                Style::default().fg(theme.foreground),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                format!("Credential length: {} chars", self.snapshot.credential_len),
                Style::default().fg(theme.secondary),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "Warning: this credential cannot be shown again. If lost, revoke it and issue a replacement.",
                Style::default().fg(theme.warning),
            )),
            Line::from(Span::styled(
                "Hand it to the device owner out of band; never paste it into chat.",
                Style::default().fg(theme.muted),
            )),
            Line::from(Span::styled(
                "Enter/Esc closes and forgets the credential (metadata stays listable).",
                Style::default().fg(theme.muted),
            )),
        ];
        let paragraph = Paragraph::new(lines).style(Style::default().fg(theme.foreground));
        frame.render_widget(paragraph, inner);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::TeamTokenSecret
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::{OneTimeBearer, OneTimeDeviceToken};

    fn secret() -> OneTimeDeviceToken {
        OneTimeDeviceToken {
            principal_id: "principal-1".to_string(),
            token_id: "token-1".to_string(),
            label: "device".to_string(),
            token: OneTimeBearer::new_device_token("cggt_token-1.secret-abc".to_string()).unwrap(),
            project_id: None,
        }
    }

    #[test]
    fn secret_dialog_never_debug_prints_credential() {
        let secret = secret();
        let dialog = DeviceSecretDialog::from_secret(&secret);
        let debug = format!("{dialog:?}");
        assert!(!debug.contains("secret-abc"));
        assert!(!debug.contains("cggt_"));
        let token_debug = format!("{:?}", secret.token);
        assert!(!token_debug.contains("secret-abc"));
        assert!(token_debug.contains("REDACTED"));
    }

    #[test]
    fn secret_dialog_keys_close_without_leaking() {
        let secret = secret();
        let mut dialog = DeviceSecretDialog::from_secret(&secret);
        let key = |code: KeyCode| KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        assert_eq!(
            dialog.handle_key(key(KeyCode::Esc)),
            Some(TuiMsg::TeamTokenSecretClose)
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::TeamTokenSecretClose)
        );
        // Tab consumed, never composer toggle.
        assert_eq!(dialog.handle_key(key(KeyCode::Tab)), None);
    }

    #[test]
    fn device_bearer_rejects_non_device_plaintext() {
        assert!(OneTimeBearer::new_device_token("".to_string()).is_err());
        assert!(OneTimeBearer::new_device_token("not-a-token".to_string()).is_err());
        assert!(OneTimeBearer::new_device_token("cggtr_id.secret".to_string()).is_err());
        assert!(OneTimeBearer::new_device_token("cggt_id.secret".to_string()).is_ok());
    }
}
