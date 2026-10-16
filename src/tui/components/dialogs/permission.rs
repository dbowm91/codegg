use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::permission::PermissionRequest;
use crate::session::events::ToolRisk;
use crate::tool::risk::classify_tool_risk;
use crate::tui::app::TuiMsg;

/// Option index for `DenyOnce` in the dialog's fixed row order
/// (`AllowOnce`, `AlwaysAllow`, `DenyOnce`, `AlwaysDeny`). Dismissing the
/// dialog with `Esc` commits this row.
const DENY_ONCE_CHOICE: usize = 2;

#[derive(Clone)]
pub struct PermissionDialog {
    pub request: PermissionRequest,
    pub selected_option: usize,
    pub theme: Arc<Theme>,
    pub confirm_persistent: bool,
    /// Option index the interstitial is confirming (`1` = Always Allow,
    /// `3` = Always Deny). Read before the interstitial state is cleared so a
    /// confirmed standing grant still reports the grant it was granting —
    /// the previous code read and cleared one field, silently degrading an
    /// "always" answer to "allow once".
    pub pending_choice: Option<usize>,
    pub risk: ToolRisk,
}

impl PermissionDialog {
    pub fn new(request: PermissionRequest, theme: Arc<Theme>) -> Self {
        let risk = classify_tool_risk(
            &request.tool,
            request.args.as_ref().unwrap_or(&serde_json::Value::Null),
        );
        Self {
            request,
            selected_option: 0,
            theme,
            confirm_persistent: false,
            pending_choice: None,
            risk,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn options(&self) -> Vec<&'static str> {
        if self.confirm_persistent {
            vec!["⚠ Confirm & Persist", "← Cancel"]
        } else {
            vec!["Allow Once", "Always Allow", "Deny Once", "Always Deny"]
        }
    }

    /// Codex-style direct keys, mapped to the same option indices Enter
    /// selects. `p` deliberately arms the same persistence confirmation as
    /// selecting "Always Allow", so a standing grant is still an explicit
    /// second keypress.
    pub fn option_for_key(code: KeyCode) -> Option<usize> {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(0),
            KeyCode::Char('p') | KeyCode::Char('P') => Some(1),
            KeyCode::Char('n') | KeyCode::Char('N') => Some(2),
            _ => None,
        }
    }

    pub fn selected_option(&self) -> usize {
        self.selected_option
    }

    pub fn cursor_down(&mut self) {
        self.selected_option = if self.confirm_persistent {
            1
        } else {
            (self.selected_option + 1).min(3)
        };
    }

    pub fn cursor_up(&mut self) {
        if self.selected_option > 0 {
            self.selected_option -= 1;
        }
    }

    fn tool_icon(&self) -> &str {
        match self.request.tool.as_str() {
            "bash" => ">$",
            "read" => "[R]",
            "edit" => "[E]",
            "write" => "[W]",
            "glob" => "[G]",
            "grep" => "[/]",
            "list" => "[L]",
            "task" => "[T]",
            "webfetch" => "[W]",
            "websearch" => "[S]",
            _ => "[?]",
        }
    }

    fn risk_color(&self, theme: &Theme) -> ratatui::style::Color {
        match self.risk {
            ToolRisk::Read => theme.primary,
            ToolRisk::Write => theme.warning,
            ToolRisk::GitMutation => theme.warning,
            ToolRisk::DependencyMutation => theme.secondary,
            ToolRisk::Network => theme.primary,
            ToolRisk::Destructive => theme.error,
            ToolRisk::CredentialAdjacent => theme.warning,
            ToolRisk::Unknown => theme.muted,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        // Single outer frame in `theme.border` — the same frame the other
        // dialogs use — carrying the request title so the box reads as one
        // surface instead of a warning-coloured frame nested inside another.
        // Risk still shouts, but through the title/label text rather than a
        // second border.
        let risk_label = format!("{}", self.risk);
        let risk_color = self.risk_color(theme);
        let outer_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background))
            .title(Span::styled(
                format!(" Permission Request — {} ", self.request.tool),
                Style::default().fg(theme.foreground),
            ))
            .title_top(
                Line::from(vec![
                    Span::styled(
                        format!("{} ", self.tool_icon()),
                        Style::default().fg(theme.warning),
                    ),
                    Span::styled(
                        format!("[{}] ", risk_label),
                        Style::default().fg(risk_color).add_modifier(Modifier::BOLD),
                    ),
                ])
                .right_aligned(),
            );
        let inner = outer_block.inner(area);
        frame.render_widget(Paragraph::new("").block(outer_block), area);

        let chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(6)]).split(inner);

        let mut details = Vec::new();
        if let Some(ref path) = self.request.path {
            details.push(Line::from(vec![
                Span::styled("Path: ", Style::default().fg(theme.muted)),
                Span::styled(path, Style::default().fg(theme.foreground)),
            ]));
        }
        if let Some(ref args) = self.request.args {
            // Extract command preview for bash-like tools
            let command_preview = if self.request.tool == "bash" || self.request.tool == "terminal"
            {
                args.get("command")
                    .or_else(|| args.get("cmd"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            };

            if let Some(ref cmd) = command_preview {
                // Approving a command you cannot fully read is not a real
                // decision. The dialog is the one place the user is explicitly
                // asked to authorize an action, so show the whole command and
                // wrap it; only pathological input is elided, and then with an
                // explicit marker.
                let full = crate::tui::wrap::normalize_inline(cmd);
                let label = "Command: ";
                let hang = " ".repeat(label.chars().count());
                let body_width = inner.width.saturating_sub(hang.len() as u16 + 2);
                for (index, row) in crate::tui::wrap::wrap_to_strings(&full, body_width.max(16))
                    .into_iter()
                    .enumerate()
                {
                    let prefix = if index == 0 {
                        label.to_string()
                    } else {
                        hang.clone()
                    };
                    let style = Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD);
                    details.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(theme.muted)),
                        Span::styled(row, style),
                    ]));
                }
            } else {
                let args_str = if args.is_string() {
                    args.as_str().unwrap_or("").to_string()
                } else {
                    args.to_string()
                };
                if !args_str.is_empty() && args_str != "null" {
                    let label = "Args: ";
                    let hang = " ".repeat(label.chars().count());
                    let body_width = inner.width.saturating_sub(hang.len() as u16 + 2);
                    for row in crate::tui::wrap::wrap_to_strings(&args_str, body_width.max(16)) {
                        details.push(Line::from(vec![
                            Span::styled(label.to_string(), Style::default().fg(theme.muted)),
                            Span::styled(row, Style::default().fg(theme.foreground)),
                        ]));
                    }
                }
            }
        }

        // Risk warning for destructive/credential tools
        match self.risk {
            ToolRisk::Destructive => {
                details.push(Line::from(vec![
                    Span::styled(
                        "! ",
                        Style::default()
                            .fg(theme.error)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "This command may permanently delete files or modify system state",
                        Style::default().fg(theme.error),
                    ),
                ]));
            }
            ToolRisk::CredentialAdjacent => {
                details.push(Line::from(vec![
                    Span::styled(
                        "! ",
                        Style::default()
                            .fg(theme.warning)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "This command may access sensitive data (keys, tokens, credentials)",
                        Style::default().fg(theme.warning),
                    ),
                ]));
            }
            _ => {}
        }

        if details.is_empty() {
            details.push(Line::from(Span::styled(
                "No additional details",
                Style::default().fg(theme.muted),
            )));
        }

        let detail = Paragraph::new(details).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.background)),
        );
        frame.render_widget(detail, chunks[0]);

        let options: Vec<Line> = self
            .options()
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                let prefix = if i == self.selected_option {
                    "> "
                } else {
                    "  "
                };
                // Direct keys are the primary affordance, so each row names
                // its own key instead of leaving them to be discovered.
                // `1`-`4` are the positional equivalents of `y`/`p`/`n`
                // and the missing `4`; they answer in one keypress.
                let shortcut = match (self.confirm_persistent, i) {
                    (true, 0) => "y",
                    (true, _) => "n",
                    (false, 0) => "y",
                    (false, 1) => "p",
                    (false, 2) => "n",
                    (false, _) => "",
                };
                let keycap = format!("{shortcut}/{}", i + 1);
                let style = if self.confirm_persistent {
                    if i == 0 {
                        Style::default()
                            .fg(theme.error)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.foreground)
                    }
                } else if i == self.selected_option {
                    Style::default().fg(theme.background).bg(theme.primary)
                } else {
                    Style::default().fg(theme.foreground)
                };
                let label = format!("{prefix}{keycap})  {opt}");
                Line::from(Span::styled(label, style))
            })
            .collect();

        let hint = if self.confirm_persistent {
            " Options — y/1 confirm & persist  |  n/2 cancel "
        } else {
            " Options — y/1 yes  |  p/2 yes always  |  n/3 no  |  4 always deny  |  ↑↓/j/k navigate  |  Esc cancel "
        };

        let options_para = Paragraph::new(options).block(
            Block::default()
                .borders(Borders::ALL)
                .title(hint)
                .title_style(Style::default().fg(theme.muted))
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.background)),
        );
        frame.render_widget(options_para, chunks[1]);
    }

    /// Commit the option at `choice`. Persistent grants route through the
    /// `⚠ Confirm & Persist` interstitial instead of submitting immediately;
    /// this is the single path for both `Enter` and the `y`/`n`/`p` direct
    /// keys so they can never disagree about what a keypress means.
    fn commit(&mut self, choice: usize) -> Option<TuiMsg> {
        if self.confirm_persistent {
            let confirmed = self.pending_choice.take().unwrap_or(1);
            self.confirm_persistent = false;
            self.selected_option = 0;
            // Inside the interstitial, option 0 confirms and anything else
            // cancels — including the `n` direct key.
            return (choice == 0).then_some(TuiMsg::SubmitPermission {
                choice_index: confirmed,
            });
        }
        if choice == 1 || choice == 3 {
            self.confirm_persistent = true;
            self.pending_choice = Some(choice);
            self.selected_option = 0;
            return None;
        }
        Some(TuiMsg::SubmitPermission {
            choice_index: choice,
        })
    }

    fn cancel_confirm(&mut self) {
        self.confirm_persistent = false;
        self.pending_choice = None;
        self.selected_option = 0;
    }
}

impl Component for PermissionDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Esc => {
                if self.confirm_persistent {
                    self.cancel_confirm();
                    None
                } else {
                    // Esc is advertised as "cancel", so the tool must not
                    // run. Closing the dialog alone only tore down the UI:
                    // the daemon-side waiter stayed parked on an unbounded
                    // human wait with no dialog left to answer it, so the
                    // turn hung until it was cancelled. A dismissal is
                    // therefore the same explicit `DenyOnce` that `n`
                    // commits, routed through the one `commit` path so the
                    // keys can never disagree about what Esc means.
                    self.commit(DENY_ONCE_CHOICE)
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor_up();
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor_down();
                None
            }
            KeyCode::Enter => {
                let choice = self.selected_option;
                self.commit(choice)
            }
            // Digits answer in one keypress, the same "no second Enter"
            // contract `y`/`p`/`n` already have. Guarded on no modifier so
            // Ctrl+digit is left for the host. `0` is not a row: it falls
            // through as unhandled rather than underflowing to row 0.
            KeyCode::Char(d)
                if key.modifiers == KeyModifiers::NONE && d.is_ascii_digit() && d != '0' =>
            {
                let digit_index = d as usize - '1' as usize;
                // Bounds are read off `options()`, which is already the
                // two-row confirm sheet while the interstitial is armed —
                // so `1` confirms, `2` backs out, and a digit past the
                // visible rows is ignored instead of arming something the
                // operator cannot see.
                if digit_index >= self.options().len() {
                    return None;
                }
                self.commit(digit_index)
            }
            code => match Self::option_for_key(code) {
                // In the interstitial the direct keys answer the prompt
                // itself (`y` confirms, `n` backs out) rather than re-arming it.
                Some(choice) if self.confirm_persistent => {
                    self.commit(if choice == 0 { 0 } else { 1 })
                }
                Some(choice) => self.commit(choice),
                None => None,
            },
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render(frame, area, theme);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Permission
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> PermissionDialog {
        PermissionDialog::new(
            PermissionRequest {
                tool: "bash".to_string(),
                args: Some(serde_json::json!({ "command": "ls -la" })),
                path: None,
            },
            Arc::new(Theme::default()),
        )
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn direct_keys_map_to_codex_options() {
        let mut d = dialog();
        assert_eq!(
            d.handle_key(key(KeyCode::Char('y'))),
            Some(TuiMsg::SubmitPermission { choice_index: 0 })
        );
        let mut d = dialog();
        assert_eq!(
            d.handle_key(key(KeyCode::Char('n'))),
            Some(TuiMsg::SubmitPermission { choice_index: 2 })
        );
    }

    /// `p` must not grant standing permission on a single keypress; it arms
    /// the same confirmation the "Always Allow" row does.
    #[test]
    fn p_arms_persistence_confirmation_instead_of_granting_immediately() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Char('p'))), None);
        assert!(d.confirm_persistent);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('y'))),
            Some(TuiMsg::SubmitPermission { choice_index: 1 })
        );
    }

    #[test]
    fn n_cancels_out_of_the_persistence_confirmation() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('p')));
        assert!(d.confirm_persistent);
        assert_eq!(d.handle_key(key(KeyCode::Char('n'))), None);
        assert!(!d.confirm_persistent);
        assert_eq!(d.selected_option, 0);
    }

    /// Regression: the confirm branch used to read `pending_persistent` after
    /// clearing it, silently degrading an "always" answer to "allow once".
    #[test]
    fn always_allow_survives_confirmation_and_always_deny_is_reachable() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('p')));
        assert_eq!(
            d.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::SubmitPermission { choice_index: 1 })
        );

        let mut d = dialog();
        d.cursor_down();
        d.cursor_down();
        d.cursor_down(); // "Always Deny"
        assert_eq!(d.selected_option, 3);
        assert_eq!(d.handle_key(key(KeyCode::Enter)), None);
        assert!(d.confirm_persistent);
        assert_eq!(
            d.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::SubmitPermission { choice_index: 3 })
        );
    }

    #[test]
    fn esc_denies_when_no_confirmation_is_armed() {
        // Regression: Esc used to emit `CloseDialog`, which only tore down
        // the UI. The daemon-side waiter stayed parked on an unbounded human
        // wait with no dialog left to answer it, so the turn hung until it
        // was cancelled. Esc is advertised as "cancel", so it must answer the
        // request — identically to the `n` direct key.
        let mut d = dialog();
        assert_eq!(
            d.handle_key(key(KeyCode::Esc)),
            Some(TuiMsg::SubmitPermission {
                choice_index: DENY_ONCE_CHOICE
            })
        );
        // Esc must not arm the persistent-grant interstitial: a dismissal is
        // never a standing grant, and it must not need a second keypress.
        assert!(!d.confirm_persistent);
        assert_eq!(d.selected_option, 0);
    }

    /// The interstitial keeps its own Esc: inside it, Esc backs out of the
    /// "always" confirmation rather than answering the request at all.
    #[test]
    fn esc_backs_out_of_the_persistent_interstitial_without_answering() {
        let mut d = dialog();
        // `p` (Always Allow) is what arms the interstitial.
        assert_eq!(d.handle_key(key(KeyCode::Char('p'))), None);
        assert!(d.confirm_persistent);
        // Esc abandons the interstitial without submitting anything.
        assert_eq!(d.handle_key(key(KeyCode::Esc)), None);
        assert!(!d.confirm_persistent);
        assert_eq!(d.selected_option, 0);
    }

    /// Digits answer in one keypress, matching `y`/`p`/`n` — the whole
    /// point is that no second Enter is required for the once-only rows.
    /// The persistent rows (`2`, `4`) deliberately still arm the
    /// interstitial; that is covered separately.
    #[test]
    fn digits_answer_without_a_second_enter() {
        for (digit, expected) in [('1', 0), ('3', 2)] {
            let mut d = dialog();
            assert_eq!(
                d.handle_key(key(KeyCode::Char(digit))),
                Some(TuiMsg::SubmitPermission {
                    choice_index: expected
                }),
                "digit {digit} should answer immediately"
            );
        }
    }

    /// `4` is "Always Deny": one keypress to arm, never a standing grant.
    #[test]
    fn digit_four_arms_the_confirmation() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Char('4'))), None);
        assert!(d.confirm_persistent);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('1'))),
            Some(TuiMsg::SubmitPermission { choice_index: 3 })
        );
    }

    /// A persistent grant stays a two-step act through its digit too.
    #[test]
    fn digit_two_arms_then_one_commits() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Char('2'))), None);
        assert!(d.confirm_persistent);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('1'))),
            Some(TuiMsg::SubmitPermission { choice_index: 1 })
        );
    }

    /// Inside the interstitial the digit addresses the two-row confirm
    /// sheet, so `2` backs out instead of re-arming it.
    #[test]
    fn digit_two_cancels_the_interstitial() {
        let mut d = dialog();
        d.handle_key(key(KeyCode::Char('2')));
        assert!(d.confirm_persistent);
        assert_eq!(d.handle_key(key(KeyCode::Char('2'))), None);
        assert!(!d.confirm_persistent);
    }

    /// `0` is not a row. It must never underflow into "Allow Once".
    #[test]
    fn zero_is_not_a_row() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Char('0'))), None);
        assert!(!d.confirm_persistent);
    }

    /// Out-of-range digits are ignored rather than clamped onto a row the
    /// operator cannot see.
    #[test]
    fn out_of_range_digit_is_ignored() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Char('9'))), None);
    }

    #[test]
    fn modifier_digits_are_left_for_the_host() {
        let mut d = dialog();
        assert_eq!(
            d.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL)),
            None
        );
    }
}
