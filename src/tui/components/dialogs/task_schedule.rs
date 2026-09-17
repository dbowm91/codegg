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
    /// C001: per-lane queue preview for the focused queue editor.
    /// Parallel to `lanes`: member order, revision, and pinned head.
    pub queue_orders: Vec<Vec<String>>,
    pub queue_revisions: Vec<u64>,
    pub queue_pinned_heads: Vec<Option<String>>,
    /// C001: whether the server supports M005 trigger management.
    /// Current daemons advertise this via WorkOrder capability; older
    /// servers leave the external-trigger row disabled.
    pub trigger_capable: bool,
}

#[derive(Clone)]
pub struct TaskScheduleDialog {
    seed: TaskScheduleSeed,
    focus: TaskSheetField,
    delay_text: String,
    not_before_text: String,
    repeat_text: String,
    sequential: bool,
    external_trigger: bool,
    queue_insert_position: usize,
    gate_join_all: bool,
    error: Option<String>,
}

impl TaskScheduleDialog {
    pub fn new(seed: TaskScheduleSeed, draft: &TaskScheduleDraft) -> Self {
        let queue_insert_position = draft.queue_insert_position.unwrap_or_else(|| {
            seed.queue_orders
                .get(seed.lane_index)
                .map(Vec::len)
                .unwrap_or(0)
        });
        Self {
            seed,
            focus: TaskSheetField::Delay,
            delay_text: draft.delay_text.clone(),
            not_before_text: draft.not_before_text.clone(),
            repeat_text: draft.repeat_text.clone(),
            sequential: draft.sequential,
            external_trigger: draft.external_trigger,
            queue_insert_position,
            gate_join_all: matches!(draft.gate_join, crate::tui::app::state::GateJoin::All),
            error: None,
        }
    }

    pub fn focus(&self) -> TaskSheetField {
        self.focus
    }

    pub fn queue_insert_position(&self) -> usize {
        self.queue_insert_position
    }

    pub fn external_trigger(&self) -> bool {
        self.external_trigger
    }

    fn current_queue_order(&self) -> &[String] {
        self.seed
            .queue_orders
            .get(self.seed.lane_index)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn current_queue_revision(&self) -> u64 {
        self.seed
            .queue_revisions
            .get(self.seed.lane_index)
            .copied()
            .unwrap_or(0)
    }

    fn current_queue_pinned(&self) -> Option<&str> {
        self.seed
            .queue_pinned_heads
            .get(self.seed.lane_index)
            .and_then(|opt| opt.as_deref())
    }

    fn clamp_queue_position(&mut self) {
        let len = self.current_queue_order().len();
        let pinned_at_zero = self.current_queue_order().first().map(String::as_str)
            == self.current_queue_pinned()
            && self.current_queue_pinned().is_some();
        self.queue_insert_position = crate::tui::app::state::clamp_queue_insert_position(
            self.queue_insert_position,
            len,
            pinned_at_zero,
        );
    }

    fn move_queue(&mut self, delta: isize) {
        let len = self.current_queue_order().len();
        let pinned_at_zero = self.current_queue_order().first().map(String::as_str)
            == self.current_queue_pinned()
            && self.current_queue_pinned().is_some();
        self.queue_insert_position = crate::tui::app::state::move_queue_insert_position(
            self.queue_insert_position,
            delta,
            len,
            pinned_at_zero,
        );
        self.clear_error();
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
        // Reset the insertion marker to the end of the newly selected
        // lane's preview; the user then moves it directly with j/k.
        let order_len = self.current_queue_order().len();
        self.queue_insert_position = order_len;
        self.clamp_queue_position();
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
            queue_insert_position: if self.sequential {
                Some(self.queue_insert_position)
            } else {
                None
            },
            queue_expected_revision: if self.sequential {
                Some(self.current_queue_revision())
            } else {
                None
            },
            external_trigger: self.external_trigger,
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
        // C001 §6.2: when the queue editor owns focus, j/k and Up/Down
        // directly move the insertion marker for eligible future work.
        // Pinned running/claimed heads cannot be crossed; the move is a
        // local insertion intention until the post-create CAS placement.
        if self.focus == TaskSheetField::Queue
            && key.modifiers == KeyModifiers::NONE
            && matches!(
                key.code,
                KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k')
            )
        {
            let delta = match key.code {
                KeyCode::Up | KeyCode::Char('k') => -1,
                KeyCode::Down | KeyCode::Char('j') => 1,
                _ => 0,
            };
            self.move_queue(delta);
            return None;
        }
        // Shift+J/K also moves the queue marker (power-user alias; the
        // Task view keeps Shift+J/K as its reorder shortcut).
        if self.focus == TaskSheetField::Queue
            && key.modifiers == KeyModifiers::SHIFT
            && matches!(key.code, KeyCode::Char('J') | KeyCode::Char('K'))
        {
            let delta = match key.code {
                KeyCode::Char('K') => -1,
                _ => 1,
            };
            self.move_queue(delta);
            return None;
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
                    TaskSheetField::Queue => {
                        // Space on the queue editor is a no-op (movement
                        // is j/k/Up/Down); keep focus stable.
                        self.clear_error();
                    }
                    TaskSheetField::ExternalTrigger => {
                        if self.seed.trigger_capable {
                            self.external_trigger = !self.external_trigger;
                            self.clear_error();
                        } else {
                            self.set_error(
                                "External trigger is unavailable: the task-trigger capability (M005) is not enabled on this server"
                                    .to_string(),
                            );
                        }
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
        // Clamp the marker before render so lane cycling never leaves a
        // stale out-of-range insertion index on screen.
        self.clamp_queue_position();
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
            self.external_trigger,
            self.queue_insert_position,
            self.gate_join_all,
            self.error.as_deref(),
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::TaskSchedule
    }
}

fn queue_line(
    seed: &TaskScheduleSeed,
    focus: TaskSheetField,
    sequential: bool,
    queue_insert_position: usize,
    width: usize,
    theme: &Arc<Theme>,
) -> Line<'static> {
    let marker = if focus == TaskSheetField::Queue {
        "▸ "
    } else {
        "  "
    };
    if !sequential {
        return Line::from(vec![
            Span::raw(marker),
            Span::styled(
                truncate_to_width(
                    "Queue [j/k move insertion when focused]: off (sequential off)",
                    width,
                ),
                Style::default().fg(theme.muted),
            ),
        ]);
    }
    let order = seed
        .queue_orders
        .get(seed.lane_index)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let pinned = seed
        .queue_pinned_heads
        .get(seed.lane_index)
        .and_then(|opt| opt.as_deref());
    let pinned_at_zero = order.first().map(String::as_str) == pinned && pinned.is_some();
    let pos = crate::tui::app::state::clamp_queue_insert_position(
        queue_insert_position,
        order.len(),
        pinned_at_zero,
    );
    // Render a compact preview: pinned head marked, insertion marker as
    // `▸new`. Example: `[pinned wo-a | wo-b | ▸new | wo-c]`.
    let mut bits: Vec<String> = Vec::new();
    for (idx, id) in order.iter().enumerate() {
        if idx == pos {
            bits.push("▸new".to_string());
        }
        let short: String = id.chars().take(6).collect();
        if Some(id.as_str()) == pinned {
            bits.push(format!("pinned {short}"));
        } else {
            bits.push(short);
        }
    }
    if pos >= order.len() {
        bits.push("▸new".to_string());
    }
    let preview = if bits.is_empty() {
        "▸new (empty lane)".to_string()
    } else {
        bits.join(" | ")
    };
    Line::from(vec![
        Span::styled(
            marker,
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "Queue [j/k·↑/↓ move]: {}",
            truncate_to_width(&preview, width.saturating_sub(24))
        )),
    ])
}

fn external_trigger_line(
    seed: &TaskScheduleSeed,
    focus: TaskSheetField,
    external_trigger: bool,
    theme: &Arc<Theme>,
) -> Line<'static> {
    let marker = if focus == TaskSheetField::ExternalTrigger {
        "▸ "
    } else {
        "  "
    };
    if !seed.trigger_capable {
        return Line::from(vec![
            Span::raw(marker),
            Span::styled(
                "External trigger [Space]: unavailable (server capability M005 not enabled)",
                Style::default().fg(theme.muted),
            ),
        ]);
    }
    Line::from(vec![
        Span::styled(
            marker,
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "External trigger [Space]: {}",
            if external_trigger { "on" } else { "off" }
        )),
    ])
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
    external_trigger: bool,
    queue_insert_position: usize,
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
        queue_line(seed, focus, sequential, queue_insert_position, width, theme),
        external_trigger_line(seed, focus, external_trigger, theme),
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
            queue_orders: vec![vec![
                "wo-run".to_string(),
                "wo-a".to_string(),
                "wo-b".to_string(),
            ]],
            queue_revisions: vec![7],
            queue_pinned_heads: vec![Some("wo-run".to_string())],
            trigger_capable: true,
        }
    }

    fn seed_no_pinned() -> TaskScheduleSeed {
        TaskScheduleSeed {
            prompt_preview: "do the thing".to_string(),
            lanes: vec![("lane-1".to_string(), Some("main".to_string()))],
            lane_index: 0,
            models: vec!["conn-a/model-1".to_string()],
            model_index: 0,
            policy_summary: "policy".to_string(),
            workspace_summary: "workspace".to_string(),
            queue_summary: "queue".to_string(),
            capabilities_supported: true,
            queue_orders: vec![vec!["wo-a".to_string(), "wo-b".to_string()]],
            queue_revisions: vec![3],
            queue_pinned_heads: vec![None],
            trigger_capable: true,
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

    #[test]
    fn focused_queue_jk_moves_insertion_with_pinned_head() {
        let mut dialog = TaskScheduleDialog::new(seed_no_pinned(), &TaskScheduleDraft::default());
        dialog.sequential = true;
        dialog.focus = TaskSheetField::Queue;
        // Starts at end (2) for a 2-member lane.
        assert_eq!(dialog.queue_insert_position, 2);
        let k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(dialog.handle_key(k), None);
        assert_eq!(dialog.queue_insert_position, 1);
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(dialog.handle_key(up), None);
        assert_eq!(dialog.queue_insert_position, 0);
        // Boundary is a stable no-op.
        assert_eq!(dialog.handle_key(up), None);
        assert_eq!(dialog.queue_insert_position, 0);
        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(dialog.handle_key(j), None);
        assert_eq!(dialog.queue_insert_position, 1);
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(dialog.handle_key(down), None);
        assert_eq!(dialog.queue_insert_position, 2);
    }

    #[test]
    fn queue_marker_never_crosses_pinned_head() {
        let mut dialog = TaskScheduleDialog::new(seed(), &TaskScheduleDraft::default());
        dialog.sequential = true;
        dialog.focus = TaskSheetField::Queue;
        dialog.queue_insert_position = 1;
        let k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(dialog.handle_key(k), None);
        // Pinned wo-run at index 0 forbids position 0.
        assert_eq!(dialog.queue_insert_position, 1);
        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(dialog.handle_key(j), None);
        assert_eq!(dialog.queue_insert_position, 2);
    }

    #[test]
    fn queue_movement_does_not_change_focus_or_emit_agent_msgs() {
        let mut dialog = TaskScheduleDialog::new(seed_no_pinned(), &TaskScheduleDraft::default());
        dialog.focus = TaskSheetField::Queue;
        for key in [
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        ] {
            assert_eq!(dialog.handle_key(key), None);
            assert_eq!(dialog.focus, TaskSheetField::Queue);
        }
    }

    #[test]
    fn external_trigger_toggle_respects_capability() {
        let mut capable = TaskScheduleDialog::new(seed(), &TaskScheduleDraft::default());
        capable.focus = TaskSheetField::ExternalTrigger;
        capable.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(capable.external_trigger);
        let mut legacy_seed = seed();
        legacy_seed.trigger_capable = false;
        let mut legacy = TaskScheduleDialog::new(legacy_seed, &TaskScheduleDraft::default());
        legacy.focus = TaskSheetField::ExternalTrigger;
        legacy.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!legacy.external_trigger);
        assert!(legacy.error.as_deref().unwrap().contains("M005"));
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
