//! Fast-select runtime-policy selector for `/approval` and `/sandbox`.
//!
//! Bare `/approval` used to print a status toast and nothing else, which
//! left `yolo` reachable only by remembering the argument spelling
//! (`/approval yolo`). A selector is the only affordance that keeps the
//! dangerous option discoverable *and* makes it a deliberate act.
//!
//! Selection is deliberately one keystroke deep:
//!
//! - `j` / `k` / `↓` / `↑` move the cursor, `Enter` commits it.
//! - `1`–`3` commit the matching row immediately — no Enter, matching the
//!   one-key answering the permission dialog already does for `y`/`p`/`n`.
//!
//! The component is a *view* over the same `request_policy_change` entry
//! point the argument form uses, so the warning matrix, the FullHost /
//! Yolo double confirmation, and the daemon CAS all still apply on this
//! path. Picking a row here never bypasses them.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::sync::Arc;

use codegg_core::approval::{ApprovalMode, SandboxProfile};

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

/// Which axis the selector is editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicySelectorKind {
    Approval,
    Sandbox,
}

/// One selectable row: a wire value plus the sentence that tells the
/// operator what they are actually turning on.
struct PolicyOption {
    /// The value handed to `request_policy_change`. Exactly one axis is
    /// populated.
    approval_mode: Option<ApprovalMode>,
    sandbox_profile: Option<SandboxProfile>,
    label: &'static str,
    detail: &'static str,
}

/// Fixed, ordered option tables. Order is the render order *and* the digit
/// order, so it must stay stable — a digit is an absolute index, not an
/// offset.
const APPROVAL_OPTIONS: [PolicyOption; 3] = [
    PolicyOption {
        approval_mode: Some(ApprovalMode::Interactive),
        sandbox_profile: None,
        label: "interactive",
        detail: "Ask before escalating — every approval signal becomes a prompt",
    },
    PolicyOption {
        approval_mode: Some(ApprovalMode::Automatic),
        sandbox_profile: None,
        label: "automatic",
        detail: "Bounded reviewer decides escalations without asking you",
    },
    PolicyOption {
        approval_mode: Some(ApprovalMode::Yolo),
        sandbox_profile: None,
        label: "yolo",
        detail: "Accept escalations automatically (never overrides a hard deny)",
    },
];

const SANDBOX_OPTIONS: [PolicyOption; 3] = [
    PolicyOption {
        approval_mode: None,
        sandbox_profile: Some(SandboxProfile::ReadOnly),
        label: "read_only",
        detail: "No writes anywhere; builds and tests still run",
    },
    PolicyOption {
        approval_mode: None,
        sandbox_profile: Some(SandboxProfile::WorkspaceWrite),
        label: "workspace_write",
        detail: "Writes confined to the project root (default)",
    },
    PolicyOption {
        approval_mode: None,
        sandbox_profile: Some(SandboxProfile::FullHost),
        label: "full_host",
        detail: "No filesystem containment; command risk signals are advisory only",
    },
];

#[derive(Clone)]
pub struct PolicySelectorDialog {
    pub kind: PolicySelectorKind,
    pub selected: usize,
    pub theme: Arc<Theme>,
}

impl PolicySelectorDialog {
    pub fn new(kind: PolicySelectorKind, current: Option<&str>, theme: Arc<Theme>) -> Self {
        let options = Self::options_for(kind);
        // Pre-position the cursor on the active value so Enter is a no-op
        // re-affirmation rather than an accidental change, and so the
        // operator can see what is in force without reading `/policy`.
        let selected = current
            .and_then(|c| options.iter().position(|o| o.label == c))
            .unwrap_or(0);
        Self {
            kind,
            selected,
            theme,
        }
    }

    fn options_for(kind: PolicySelectorKind) -> &'static [PolicyOption] {
        match kind {
            PolicySelectorKind::Approval => &APPROVAL_OPTIONS,
            PolicySelectorKind::Sandbox => &SANDBOX_OPTIONS,
        }
    }

    fn options(&self) -> &'static [PolicyOption] {
        Self::options_for(self.kind)
    }

    fn len(&self) -> usize {
        self.options().len()
    }

    fn title(&self) -> &'static str {
        match self.kind {
            PolicySelectorKind::Approval => " Approval Mode ",
            PolicySelectorKind::Sandbox => " Sandbox Profile ",
        }
    }

    pub fn cursor_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn cursor_down(&mut self) {
        if self.selected + 1 < self.len() {
            self.selected += 1;
        }
    }

    /// Commit row `index`. Returns the message the App turns into a
    /// `request_policy_change` call.
    fn commit(&self, index: usize) -> Option<TuiMsg> {
        let option = self.options().get(index)?;
        Some(TuiMsg::PolicyOptionSelected {
            approval_mode: option.approval_mode.map(|m| m.as_str().to_string()),
            sandbox_profile: option.sandbox_profile.map(|p| p.as_str().to_string()),
        })
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }
}

impl Component for PolicySelectorDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Esc => Some(TuiMsg::CloseDialog),
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor_up();
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor_down();
                None
            }
            KeyCode::Enter => self.commit(self.selected),
            // A digit is an absolute row index and answers immediately.
            // Guarded on no modifier so Ctrl+digit stays available to the
            // host for anything else.
            KeyCode::Char(d)
                if key.modifiers == KeyModifiers::NONE && d.is_ascii_digit() && d != '0' =>
            {
                let idx = d as usize - '1' as usize;
                self.commit(idx)
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
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background))
            .title(Span::styled(
                self.title(),
                Style::default().fg(theme.foreground),
            ));
        let inner = block.inner(area);
        frame.render_widget(Paragraph::new("").block(block), area);

        // Rows first and `Min` so a future fourth option is never silently
        // clipped, then a fixed-height bordered hint. `Length(1)` here would
        // hide everything but `interactive` — including `yolo`, which is the
        // whole reason the selector exists.
        let chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(inner);

        let mut rows: Vec<Line> = Vec::new();
        for (i, option) in self.options().iter().enumerate() {
            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.background)
                    .bg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };
            let detail_style = if is_selected {
                Style::default().fg(theme.background)
            } else {
                Style::default().fg(theme.muted)
            };
            // The digit is printed on the row so the shortcut is never a
            // discovery problem, and the cursor marker rides the same row.
            let marker = if is_selected { ">" } else { " " };
            rows.push(Line::from(vec![
                Span::styled(format!("{marker} {}  ", i + 1), style),
                Span::styled(option.label, style),
                Span::styled(format!("  —  {}", option.detail), detail_style),
            ]));
        }
        frame.render_widget(Paragraph::new(rows), chunks[0]);

        let hint = " ↑↓/j/k move  |  1-3 select now  |  Enter select  |  Esc cancel ";
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                hint,
                Style::default().fg(theme.muted),
            )))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .style(Style::default().bg(theme.background)),
            ),
            chunks[1],
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::PolicySelector
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(kind: PolicySelectorKind) -> PolicySelectorDialog {
        PolicySelectorDialog::new(kind, None, Arc::new(Theme::default()))
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn j_and_k_walk_the_list_without_committing() {
        let mut d = dialog(PolicySelectorKind::Approval);
        assert_eq!(d.handle_key(key(KeyCode::Char('j'))), None);
        assert_eq!(d.selected, 1);
        assert_eq!(d.handle_key(key(KeyCode::Down)), None);
        assert_eq!(d.selected, 2);
        assert_eq!(d.handle_key(key(KeyCode::Char('k'))), None);
        assert_eq!(d.selected, 1);
        assert_eq!(d.handle_key(key(KeyCode::Up)), None);
        assert_eq!(d.selected, 0);
    }

    /// The cursor must not wrap or run off either end.
    #[test]
    fn cursor_clamps_at_both_ends() {
        let mut d = dialog(PolicySelectorKind::Approval);
        assert_eq!(d.handle_key(key(KeyCode::Up)), None);
        assert_eq!(d.selected, 0);
        for _ in 0..10 {
            d.handle_key(key(KeyCode::Down));
        }
        assert_eq!(d.selected, 2);
    }

    /// A digit answers in one keypress — this is the whole point of the
    /// selector, so a second required Enter would be a regression.
    #[test]
    fn digit_commits_without_a_second_enter() {
        let mut d = dialog(PolicySelectorKind::Approval);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('3'))),
            Some(TuiMsg::PolicyOptionSelected {
                approval_mode: Some("yolo".to_string()),
                sandbox_profile: None,
            })
        );
    }

    #[test]
    fn every_digit_maps_to_its_own_row() {
        for (digit, expected) in [('1', "interactive"), ('2', "automatic"), ('3', "yolo")] {
            let mut d = dialog(PolicySelectorKind::Approval);
            assert_eq!(
                d.handle_key(key(KeyCode::Char(digit))),
                Some(TuiMsg::PolicyOptionSelected {
                    approval_mode: Some(expected.to_string()),
                    sandbox_profile: None,
                }),
                "digit {digit} should select {expected}"
            );
        }
    }

    #[test]
    fn enter_commits_the_highlighted_row() {
        let mut d = dialog(PolicySelectorKind::Approval);
        d.handle_key(key(KeyCode::Down));
        assert_eq!(
            d.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::PolicyOptionSelected {
                approval_mode: Some("automatic".to_string()),
                sandbox_profile: None,
            })
        );
    }

    #[test]
    fn sandbox_selector_targets_the_sandbox_axis() {
        let mut d = dialog(PolicySelectorKind::Sandbox);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('1'))),
            Some(TuiMsg::PolicyOptionSelected {
                approval_mode: None,
                sandbox_profile: Some("read_only".to_string()),
            })
        );
    }

    /// `0` is not a row. It must fall through to "unhandled", never to
    /// row 0 by underflow.
    #[test]
    fn zero_does_not_select_the_first_row() {
        let mut d = dialog(PolicySelectorKind::Approval);
        assert_eq!(d.handle_key(key(KeyCode::Char('0'))), None);
    }

    #[test]
    fn out_of_range_digit_is_ignored() {
        let mut d = dialog(PolicySelectorKind::Approval);
        assert_eq!(d.handle_key(key(KeyCode::Char('9'))), None);
    }

    #[test]
    fn modifier_digits_are_left_for_the_host() {
        let mut d = dialog(PolicySelectorKind::Approval);
        assert_eq!(
            d.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::CONTROL)),
            None
        );
    }

    #[test]
    fn esc_cancels() {
        let mut d = dialog(PolicySelectorKind::Approval);
        assert_eq!(d.handle_key(key(KeyCode::Esc)), Some(TuiMsg::CloseDialog));
    }

    /// Opening the selector must not silently change anything: the cursor
    /// starts on the value already in force.
    #[test]
    fn cursor_starts_on_the_active_value() {
        let d = PolicySelectorDialog::new(
            PolicySelectorKind::Approval,
            Some("yolo"),
            Arc::new(Theme::default()),
        );
        assert_eq!(d.selected, 2);
    }

    #[test]
    fn unknown_active_value_falls_back_to_the_first_row() {
        let d = PolicySelectorDialog::new(
            PolicySelectorKind::Approval,
            Some("nonsense"),
            Arc::new(Theme::default()),
        );
        assert_eq!(d.selected, 0);
    }
}
