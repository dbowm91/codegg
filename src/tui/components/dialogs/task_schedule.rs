//! Project Work Orders M003: Task scheduling sheet dialog.
//!
//! FocusManager-owned modal opened by Enter in Task composer mode.
//! The component owns its editable buffers (seeded from
//! [`TaskScheduleDraft`]); Enter on Confirm emits
//! [`TuiMsg::TaskScheduleConfirm`] with edited values for App-side
//! validation. Validation failures keep the sheet open with an
//! actionable error and leave the editable prompt untouched.
//!
//! Tab/Shift+Tab move sheet focus and never reach the agent selector:
//! the top modal consumes all keys, so there is no Tab collision by
//! construction. Bare `j`/`k` are deliberately unbound here (Vim
//! navigation belongs to the Task view, not the sheet).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::sync::Arc;

use crate::tui::app::state::{prompt_preview, TaskScheduleDraft, TaskSheetField};
use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

/// Bounded seed data snapshotted when the sheet opens. Display only;
/// the daemon re-validates everything at `WorkOrderCreate`.
#[derive(Debug, Clone)]
pub struct TaskScheduleSeed {
    pub prompt_preview: String,
    pub lanes: Vec<(String, Option<String>)>,
    pub lane_index: usize,
    pub models: Vec<String>,
    pub model_index: usize,
    pub policy_summary: String,
    pub workspace_summary: String,
    pub queue_summary: String,
    pub capabilities_supported: bool,
}

#[derive(Clone)]
pub struct TaskScheduleDialog {
    seed: TaskScheduleSeed,
    focus: TaskSheetField,
    delay_text: String,
    not_before_text: String,
    repeat_text: String,
    sequential: bool,
    gate_join_all: bool,
    error: Option<String>,
}

impl TaskScheduleDialog {
    pub fn new(seed: TaskScheduleSeed, draft: &TaskScheduleDraft) -> Self {
        Self {
            seed,
            focus: TaskSheetField::Delay,
            delay_text: draft.delay_text.clone(),
            not_before_text: draft.not_before_text.clone(),
            repeat_text: draft.repeat_text.clone(),
            sequential: draft.sequential,
            gate_join_all: matches!(draft.gate_join, crate::tui::app::state::GateJoin::All),
            error: None,
        }
    }

    pub fn move_focus(&mut self, delta: isize) {
        if delta >= 0 {
            for _ in 0..delta {
                self.focus = self.focus.advance();
            }
        } else {
            for _ in 0..-delta {
                self.focus = self.focus.retreat();
            }
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }

    fn selected_lane_id(&self) -> Option<String> {
        if !self.sequential {
            return None;
        }
        self.seed
            .lanes
            .get(self.seed.lane_index)
            .map(|(id, _)| id.clone())
    }

    fn selected_model(&self) -> Option<String> {
        self.seed.models.get(self.seed.model_index).cloned()
    }

    fn cycle_lane(&mut self, delta: isize) {
        if self.seed.lanes.is_empty() {
            return;
        }
        let len = self.seed.lanes.len() as isize;
        let next = (self.seed.lane_index as isize + delta).rem_euclid(len) as usize;
        self.seed.lane_index = next;
    }

    fn cycle_model(&mut self, delta: isize) {
        if self.seed.models.is_empty() {
            return;
        }
        let len = self.seed.models.len() as isize;
        let next = (self.seed.model_index as isize + delta).rem_euclid(len) as usize;
        self.seed.model_index = next;
    }

    fn confirm_msg(&self) -> TuiMsg {
        TuiMsg::TaskScheduleConfirm {
            delay_text: self.delay_text.clone(),
            not_before_text: self.not_before_text.clone(),
            repeat_text: self.repeat_text.clone(),
            sequential: self.sequential,
            lane_id: self.selected_lane_id(),
            gate_join_all: self.gate_join_all,
            model: self.selected_model(),
        }
    }

    fn focused_buffer_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            TaskSheetField::Delay => Some(&mut self.delay_text),
            TaskSheetField::NotBefore => Some(&mut self.not_before_text),
            TaskSheetField::Repeat => Some(&mut self.repeat_text),
            _ => None,
        }
    }
}

fn truncate_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    if width <= 1 {
        return chars.into_iter().take(width).collect();
    }
    chars.into_iter().take(width - 1).collect::<String>() + "…"
}

impl Component for TaskScheduleDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        // Ctrl+Enter confirms from any field.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter {
            return Some(self.confirm_msg());
        }
        match key.code {
            KeyCode::Esc => Some(TuiMsg::CloseDialog),
            KeyCode::Tab => {
                self.clear_error();
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.focus = self.focus.retreat();
                } else {
                    self.focus = self.focus.advance();
                }
                None
            }
            KeyCode::Up => {
                self.clear_error();
                self.focus = self.focus.retreat();
                None
            }
            KeyCode::Down => {
                self.clear_error();
                self.focus = self.focus.advance();
                None
            }
            KeyCode::Enter => {
                if self.focus == TaskSheetField::Confirm {
                    Some(self.confirm_msg())
                } else {
                    self.clear_error();
                    self.focus = self.focus.advance();
                    None
                }
            }
            KeyCode::Backspace => {
                if let Some(buffer) = self.focused_buffer_mut() {
                    buffer.pop();
                    self.clear_error();
                }
                None
            }
            KeyCode::Left | KeyCode::Right => {
                let delta = if key.code == KeyCode::Left { -1 } else { 1 };
                match self.focus {
                    TaskSheetField::Sequential => self.cycle_lane(delta),
                    TaskSheetField::Model => self.cycle_model(delta),
                    _ => {}
                }
                None
            }
            KeyCode::Char(' ') => {
                match self.focus {
                    TaskSheetField::Sequential => {
                        self.sequential = !self.sequential;
                        self.clear_error();
                    }
                    TaskSheetField::GateJoin => {
                        self.gate_join_all = !self.gate_join_all;
                        self.clear_error();
                    }
                    TaskSheetField::Model => self.cycle_model(1),
                    TaskSheetField::Confirm => return Some(self.confirm_msg()),
                    _ => {
                        if let Some(buffer) = self.focused_buffer_mut() {
                            buffer.push(' ');
                            self.clear_error();
                        }
                    }
                }
                None
            }
            KeyCode::Char('[') => {
                if self.focus == TaskSheetField::Model {
                    self.cycle_model(-1);
                } else if let Some(buffer) = self.focused_buffer_mut() {
                    buffer.push('[');
                }
                None
            }
            KeyCode::Char(']') => {
                if self.focus == TaskSheetField::Model {
                    self.cycle_model(1);
                } else if let Some(buffer) = self.focused_buffer_mut() {
                    buffer.push(']');
                }
                None
            }
            KeyCode::Char(',') => {
                if self.focus == TaskSheetField::Sequential {
                    self.cycle_lane(-1);
                } else if let Some(buffer) = self.focused_buffer_mut() {
                    buffer.push(',');
                }
                None
            }
            KeyCode::Char('.') => {
                if self.focus == TaskSheetField::Sequential {
                    self.cycle_lane(1);
                } else if let Some(buffer) = self.focused_buffer_mut() {
                    buffer.push('.');
                }
                None
            }
            KeyCode::Char(c) => {
                if let Some(buffer) = self.focused_buffer_mut() {
                    if buffer.len() < 128 {
                        buffer.push(c);
                        self.clear_error();
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        if let Some(buffer) = self.focused_buffer_mut() {
            let room = 128usize.saturating_sub(buffer.len());
            buffer.push_str(&text.chars().take(room).collect::<String>());
            self.clear_error();
        }
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        render_sheet(
            frame,
            area,
            theme,
            &self.seed,
            self.focus,
            &self.delay_text,
            &self.not_before_text,
            &self.repeat_text,
            self.sequential,
            self.gate_join_all,
            self.error.as_deref(),
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::TaskSchedule
    }
}

#[allow(clippy::too_many_arguments)]
fn render_sheet(
    frame: &mut Frame,
    area: Rect,
    theme: &Arc<Theme>,
    seed: &TaskScheduleSeed,
    focus: TaskSheetField,
    delay_text: &str,
    not_before_text: &str,
    repeat_text: &str,
    sequential: bool,
    gate_join_all: bool,
    error: Option<&str>,
) {
    if area.height < 8 || area.width < 30 {
        return;
    }
    let width = area.width as usize;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Schedule Task ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width < 10 {
        return;
    }
    let marker = |field: TaskSheetField| {
        if focus == field {
            Span::styled(
                "▸ ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("  ")
        }
    };
    let field_value = |field: TaskSheetField, text: &str| -> String {
        if focus == field {
            format!("{text}▌")
        } else {
            text.to_string()
        }
    };
    let lane_label = seed
        .lanes
        .get(seed.lane_index)
        .map(|(id, label)| {
            let short: String = id.chars().take(8).collect();
            match label {
                Some(label) if !label.trim().is_empty() => format!("{label} ({short})"),
                _ => short,
            }
        })
        .unwrap_or_else(|| "no lane".to_string());
    let model_label = seed
        .models
        .get(seed.model_index)
        .cloned()
        .unwrap_or_else(|| "current model".to_string());
    let mut lines = vec![
        Line::from(Span::styled(
            truncate_to_width(
                &format!("Task: {}", prompt_preview(&seed.prompt_preview)),
                width,
            ),
            Style::default().fg(theme.foreground),
        )),
        Line::from(vec![
            marker(TaskSheetField::Delay),
            Span::raw("Delay (0, 20m, 1h30m): "),
            Span::styled(
                field_value(TaskSheetField::Delay, delay_text),
                Style::default().fg(theme.secondary),
            ),
        ]),
        Line::from(vec![
            marker(TaskSheetField::NotBefore),
            Span::raw("Not-before RFC3339 (blank=none): "),
            Span::styled(
                field_value(TaskSheetField::NotBefore, not_before_text),
                Style::default().fg(theme.secondary),
            ),
        ]),
        Line::from(vec![
            marker(TaskSheetField::Repeat),
            Span::raw("Repeat count (1-100): "),
            Span::styled(
                field_value(TaskSheetField::Repeat, repeat_text),
                Style::default().fg(theme.secondary),
            ),
        ]),
        Line::from(vec![
            marker(TaskSheetField::Sequential),
            Span::raw(format!(
                "Sequential [Space]: {}  lane ,/. : {lane_label}",
                if sequential { "on" } else { "off" }
            )),
        ]),
        Line::from(vec![
            marker(TaskSheetField::GateJoin),
            Span::raw(format!(
                "Join [Space]: {}",
                if gate_join_all {
                    "ALL gates"
                } else {
                    "ANY gate"
                }
            )),
        ]),
        Line::from(vec![
            marker(TaskSheetField::Model),
            Span::raw(format!("Model [/]: {model_label}")),
        ]),
        Line::from(Span::styled(
            truncate_to_width(
                &format!(
                    "Policy: {} · workspace: {}",
                    seed.policy_summary, seed.workspace_summary
                ),
                width,
            ),
            Style::default().fg(theme.muted),
        )),
        Line::from(Span::styled(
            truncate_to_width(&format!("Queue: {}", seed.queue_summary), width),
            Style::default().fg(theme.muted),
        )),
        Line::from(Span::styled(
            "External trigger: unavailable (server capability M005 not enabled)",
            Style::default().fg(theme.muted),
        )),
    ];
    if let Some(error) = error {
        lines.push(Line::from(Span::styled(
            truncate_to_width(&format!("Error: {error}"), width),
            Style::default().fg(theme.error),
        )));
    }
    lines.push(Line::from(vec![
        marker(TaskSheetField::Confirm),
        Span::styled(
            "Confirm",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  (Enter confirms · Esc cancels · default is immediate/zero delay)"),
    ]));
    let paragraph = Paragraph::new(lines).style(Style::default().fg(theme.foreground));
    frame.render_widget(paragraph, inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> TaskScheduleSeed {
        TaskScheduleSeed {
            prompt_preview: "do the thing".to_string(),
            lanes: vec![("lane-1".to_string(), Some("main".to_string()))],
            lane_index: 0,
            models: vec!["conn-a/model-1".to_string(), "conn-a/model-2".to_string()],
            model_index: 0,
            policy_summary: "approval:interactive sandbox:workspace_write".to_string(),
            workspace_summary: "auto (isolated worktree for Git mutation)".to_string(),
            queue_summary: "0 running · 2 waiting".to_string(),
            capabilities_supported: true,
        }
    }

    #[test]
    fn tab_cycles_fields_and_never_emits_agent_msgs() {
        let mut dialog = TaskScheduleDialog::new(seed(), &TaskScheduleDraft::default());
        assert_eq!(dialog.focus, TaskSheetField::Delay);
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(dialog.handle_key(tab), None);
        assert_eq!(dialog.focus, TaskSheetField::NotBefore);
        // Tab never produces SwitchAgent/CycleAgent traffic.
        for _ in 0..10 {
            let msg = dialog.handle_key(tab);
            assert!(msg.is_none() || matches!(msg, Some(TuiMsg::TaskScheduleConfirm { .. })));
        }
    }

    #[test]
    fn confirm_emits_edited_values() {
        let mut dialog = TaskScheduleDialog::new(seed(), &TaskScheduleDraft::default());
        dialog.delay_text = "20m".to_string();
        dialog.focus = TaskSheetField::Confirm;
        match dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Some(TuiMsg::TaskScheduleConfirm { delay_text, .. }) => {
                assert_eq!(delay_text, "20m");
            }
            other => panic!("expected confirm, got {other:?}"),
        }
    }

    #[test]
    fn space_toggles_sequential_and_join() {
        let mut dialog = TaskScheduleDialog::new(seed(), &TaskScheduleDraft::default());
        dialog.focus = TaskSheetField::Sequential;
        dialog.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(dialog.sequential);
        dialog.focus = TaskSheetField::GateJoin;
        dialog.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!dialog.gate_join_all);
    }

    fn render_to_text(dialog: &mut TaskScheduleDialog, width: u16, height: u16) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Arc::new(Theme::default());
        terminal
            .draw(|frame| {
                dialog.render(frame, frame.area(), &theme);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push(buffer[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn sheet_renders_across_terminal_sizes_without_panic() {
        let mut dialog = TaskScheduleDialog::new(seed(), &TaskScheduleDraft::default());
        let normal = render_to_text(&mut dialog, 80, 24);
        assert!(normal.contains("Schedule Task"));
        assert!(normal.contains("Delay"));
        assert!(normal.contains("Confirm"));
        // Narrow terminals degrade (truncated, still bounded) and tiny
        // areas are a guarded no-op rather than a panic.
        let narrow = render_to_text(&mut dialog, 40, 20);
        assert!(narrow.contains("Schedule Task"));
        let _tiny = render_to_text(&mut dialog, 20, 5);
    }
}
