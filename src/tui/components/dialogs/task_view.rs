//! Project Work Orders M003: project Task view dialog.
//!
//! FocusManager-owned modal rendering the bounded project Task
//! projection: RUNNING / FUTURE-WAITING / NEEDS-ATTENTION / RECENT.
//! The component renders a [`TaskViewSnapshot`] synced from
//! `DialogState.task_view` after every mutation; it owns no daemon
//! truth. Navigation uses `j`/`k` and arrows; reorder uses Shift+J/K
//! (a focused move modifier, so bare `j`/`k` never collide with
//! navigation). Enter opens a materialized session through canonical
//! routing; future rows fetch detail instead of a fake session.
//! Closing or switching views never cancels daemon-owned work.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::sync::Arc;

use crate::tui::app::state::{group_task_rows, TaskViewSection, TaskViewState, MAX_SECTION_ROWS};
use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

/// One rendered Task-view row (bounded, secret-free).
#[derive(Debug, Clone)]
pub struct TaskViewLine {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub attention: Option<String>,
}

/// Render snapshot synced from `TaskViewState`.
#[derive(Debug, Clone, Default)]
pub struct TaskViewSnapshot {
    pub project_id: String,
    pub sections: Vec<(TaskViewSection, Vec<TaskViewLine>)>,
    pub selected_id: Option<String>,
    pub summary_line: String,
    pub loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// C001 trigger status lines per WorkOrder (`work_order_id ->
    /// human status`, e.g. `trigger active · hook-1 · 2/5 fires` or
    /// `trigger setup incomplete — press t to retry`). Metadata only,
    /// never secrets.
    pub trigger_labels: std::collections::HashMap<String, String>,
}

impl TaskViewSnapshot {
    pub fn from_state(state: &TaskViewState) -> Self {
        let groups = group_task_rows(&state.rows);
        let mut sections = Vec::with_capacity(groups.len());
        for (section, idxs) in groups {
            let mut lines = Vec::new();
            for idx in idxs.into_iter().take(MAX_SECTION_ROWS) {
                let Some(row) = state.rows.get(idx) else {
                    continue;
                };
                let short: String = row.work_order.work_order_id.chars().take(8).collect();
                let detail = if row.is_running() {
                    row.materialized_session_id()
                        .map(|s| {
                            let session_short: String = s.chars().take(8).collect();
                            format!("session {session_short}")
                        })
                        .unwrap_or_else(|| "starting".to_string())
                } else if row.is_terminal() {
                    row.work_order.state.clone()
                } else {
                    release_hint(row)
                };
                lines.push(TaskViewLine {
                    id: row.work_order.work_order_id.clone(),
                    title: row.title_or_preview(),
                    detail: format!("{short} · {detail}"),
                    attention: row.attention_reason(),
                });
            }
            sections.push((section, lines));
        }
        let summary_line = match state.summary.as_ref() {
            Some(summary) => format!(
                "{} active · {} waiting · {} attention · {} lanes",
                summary.active,
                summary.waiting_occurrences,
                summary.attention_occurrences,
                summary.lane_count
            ),
            None => "summary unavailable".to_string(),
        };
        Self {
            project_id: state
                .project_id
                .clone()
                .unwrap_or_else(|| "project".to_string()),
            sections,
            selected_id: state
                .selected_row()
                .map(|row| row.work_order.work_order_id.clone()),
            summary_line,
            loading: state.loading,
            error: state.error.clone(),
            notice: state.notice.clone(),
            trigger_labels: std::collections::HashMap::new(),
        }
    }

    /// Attach C001 trigger status lines (metadata only, never secrets).
    pub fn with_trigger_labels(
        mut self,
        labels: std::collections::HashMap<String, String>,
    ) -> Self {
        self.trigger_labels = labels;
        self
    }
}

fn release_hint(row: &crate::tui::app::state::TaskViewRow) -> String {
    let gates = &row.work_order.gates;
    if gates.is_empty() {
        return "immediate".to_string();
    }
    let kinds: Vec<&str> = gates.iter().map(|g| g.kind.as_str()).collect();
    if kinds == ["immediate"] {
        return "immediate".to_string();
    }
    let mut bits = Vec::new();
    for gate in gates {
        match gate.kind.as_str() {
            "delay" => bits.push(format!(
                "wait {}",
                crate::tui::app::state::format_delay_secs(gate.delay_secs.unwrap_or(0))
            )),
            "not_before" => bits.push("not-before".to_string()),
            "sequence_ready" => bits.push("sequenced".to_string()),
            "external_trigger" => bits.push("trigger".to_string()),
            other => bits.push(other.to_string()),
        }
    }
    if row.work_order.repeat_count > 1 {
        bits.push(format!("repeat {}x", row.work_order.repeat_count));
    }
    bits.join(" + ")
}

#[derive(Clone)]
pub struct TaskViewDialog {
    project_id: String,
    snapshot: TaskViewSnapshot,
}

impl TaskViewDialog {
    pub fn new(project_id: String) -> Self {
        Self {
            project_id: project_id.clone(),
            snapshot: TaskViewSnapshot {
                project_id,
                loading: true,
                ..TaskViewSnapshot::default()
            },
        }
    }

    pub fn set_snapshot(&mut self, snapshot: TaskViewSnapshot) {
        self.snapshot = snapshot;
    }
}

impl Component for TaskViewDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Some(TuiMsg::CloseDialog),
            KeyCode::Up | KeyCode::Char('k') => Some(TuiMsg::TaskViewMove { delta: -1 }),
            KeyCode::Down | KeyCode::Char('j') => Some(TuiMsg::TaskViewMove { delta: 1 }),
            KeyCode::PageUp => Some(TuiMsg::TaskViewMove { delta: -10 }),
            KeyCode::PageDown => Some(TuiMsg::TaskViewMove { delta: 10 }),
            KeyCode::Home => Some(TuiMsg::TaskViewMove { delta: -10_000 }),
            KeyCode::End => Some(TuiMsg::TaskViewMove { delta: 10_000 }),
            // Shift+J/K reorder (focused move modifier; bare j/k navigate).
            KeyCode::Char('J') => Some(TuiMsg::TaskViewReorder { delta: 1 }),
            KeyCode::Char('K') => Some(TuiMsg::TaskViewReorder { delta: -1 }),
            KeyCode::Char('g') => Some(TuiMsg::TaskViewMove { delta: -10_000 }),
            KeyCode::Char('G') => Some(TuiMsg::TaskViewMove { delta: 10_000 }),
            KeyCode::Enter => Some(TuiMsg::TaskViewOpen),
            KeyCode::Char('r') => Some(TuiMsg::TaskViewRefresh),
            KeyCode::Char('d') => Some(TuiMsg::TaskViewDetail),
            KeyCode::Char('x') => Some(TuiMsg::TaskViewCancel),
            KeyCode::Char('u') => Some(TuiMsg::TaskViewResume),
            // C001 trigger management (metadata only; no secret-read path).
            KeyCode::Char('t') => Some(TuiMsg::TaskTriggerSetup),
            KeyCode::Char('T') => Some(TuiMsg::TaskTriggerRotate),
            KeyCode::Char('X') => Some(TuiMsg::TaskTriggerRevoke),
            KeyCode::Char('e') => Some(TuiMsg::TaskTriggerRefresh),
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
        render_view(frame, area, theme, &self.project_id, &self.snapshot);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::TaskView
    }
}

fn render_view(
    frame: &mut Frame,
    area: Rect,
    theme: &Arc<Theme>,
    project_id: &str,
    snapshot: &TaskViewSnapshot,
) {
    if area.height < 8 || area.width < 30 {
        return;
    }
    let width = area.width as usize;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Project Tasks ".to_string());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width < 10 {
        return;
    }
    let _ = project_id;
    let mut lines = vec![Line::from(Span::styled(
        truncate(&snapshot.summary_line, width),
        Style::default().fg(theme.muted),
    ))];
    if snapshot.loading {
        lines.push(Line::from(Span::raw("Loading tasks…")));
    }
    let detailed = width >= 60;
    for (section, rows) in &snapshot.sections {
        lines.push(Line::from(Span::styled(
            section.title(),
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        )));
        if rows.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(theme.muted),
            )));
            continue;
        }
        for row in rows {
            let selected = snapshot.selected_id.as_deref() == Some(row.id.as_str());
            let prefix = if selected { "▸ " } else { "  " };
            let mut spans = vec![Span::styled(
                prefix,
                Style::default()
                    .fg(if selected { theme.primary } else { theme.muted })
                    .add_modifier(Modifier::BOLD),
            )];
            spans.push(Span::styled(
                truncate(&row.title, width.saturating_sub(6)),
                Style::default().fg(if selected {
                    theme.foreground
                } else {
                    theme.muted
                }),
            ));
            if detailed {
                spans.push(Span::styled(
                    format!("  {}", truncate(&row.detail, 32)),
                    Style::default().fg(theme.muted),
                ));
            }
            lines.push(Line::from(spans));
            if let Some(attention) = row.attention.as_deref() {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        truncate(&format!("! {attention}"), width.saturating_sub(4)),
                        Style::default().fg(theme.warning),
                    ),
                ]));
            }
            if let Some(trigger_label) = snapshot.trigger_labels.get(&row.id) {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        truncate(trigger_label, width.saturating_sub(4)),
                        Style::default().fg(theme.secondary),
                    ),
                ]));
            }
        }
    }
    if let Some(error) = snapshot.error.as_deref() {
        lines.push(Line::from(Span::styled(
            truncate(&format!("Error: {error}"), width),
            Style::default().fg(theme.error),
        )));
    }
    if let Some(notice) = snapshot.notice.as_deref() {
        lines.push(Line::from(Span::styled(
            truncate(notice, width),
            Style::default().fg(theme.warning),
        )));
    }
    lines.push(Line::from(Span::styled(
        "j/k move · J/K reorder · Enter open · d detail · r refresh · x cancel · u resume · t trigger setup · T rotate · X revoke · e trigger refresh · Esc close",
        Style::default().fg(theme.muted),
    )));
    let paragraph = Paragraph::new(lines).style(Style::default().fg(theme.foreground));
    frame.render_widget(paragraph, inner);
}

fn truncate(text: &str, width: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_keys_map_to_view_actions_without_collisions() {
        let mut dialog = TaskViewDialog::new("project-1".to_string());
        let key = |code: KeyCode| KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('j'))),
            Some(TuiMsg::TaskViewMove { delta: 1 })
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('k'))),
            Some(TuiMsg::TaskViewMove { delta: -1 })
        );
        // Reorder needs the Shift modifier convention, never bare j/k.
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('J'))),
            Some(TuiMsg::TaskViewReorder { delta: 1 })
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('K'))),
            Some(TuiMsg::TaskViewReorder { delta: -1 })
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::TaskViewOpen)
        );
        // Tab is consumed (None) — never leaks to composer toggle.
        assert_eq!(dialog.handle_key(key(KeyCode::Tab)), None);
        // C001 trigger management keys (metadata only, no secret-read).
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('t'))),
            Some(TuiMsg::TaskTriggerSetup)
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('T'))),
            Some(TuiMsg::TaskTriggerRotate)
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('X'))),
            Some(TuiMsg::TaskTriggerRevoke)
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('e'))),
            Some(TuiMsg::TaskTriggerRefresh)
        );
    }

    #[test]
    fn snapshot_groups_attention_with_stable_labels() {
        use crate::protocol::work_order::{WorkOrderDto, WorkOrderOccurrenceDto};
        let row = crate::tui::app::state::TaskViewRow {
            work_order: WorkOrderDto {
                work_order_id: "wo-1".to_string(),
                revision: 1,
                project_id: "p".to_string(),
                creator_principal: "owner".to_string(),
                parent_session_id: None,
                parent_turn_id: None,
                parent_work_order_id: None,
                title: None,
                prompt: "fix the flaky test".to_string(),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: Vec::new(),
                gate_join: None,
                repeat_count: 1,
                sequence_lane_id: None,
                state: "active".to_string(),
                created_at_ms: 0,
                updated_at_ms: 0,
                cancelled_at_ms: None,
            },
            occurrence: Some(WorkOrderOccurrenceDto {
                occurrence_id: "occ-1".to_string(),
                work_order_id: "wo-1".to_string(),
                project_id: "p".to_string(),
                occurrence_index: 0,
                state: "needs_attention".to_string(),
                gate_latches: Vec::new(),
                not_before_ms: None,
                next_check_at_ms: None,
                session_id: None,
                job_id: None,
                workspace_id: None,
                worktree_id: None,
                attention_code: Some("model_unavailable".to_string()),
                diagnostic: None,
                created_at_ms: 0,
                updated_at_ms: 0,
                claimed_at_ms: None,
                started_at_ms: None,
                terminal_at_ms: None,
            }),
        };
        let state = TaskViewState {
            project_id: Some("p".to_string()),
            rows: vec![row],
            ..TaskViewState::default()
        };
        let snapshot = TaskViewSnapshot::from_state(&state);
        let attention = snapshot
            .sections
            .iter()
            .find(|(s, _)| *s == TaskViewSection::Attention)
            .expect("attention section");
        assert_eq!(attention.1.len(), 1);
        assert_eq!(
            attention.1[0].attention.as_deref(),
            Some("model unavailable")
        );
    }

    fn render_to_text(dialog: &mut TaskViewDialog, width: u16, height: u16) -> String {
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
    fn view_renders_sections_across_terminal_sizes_without_panic() {
        let mut dialog = TaskViewDialog::new("project-1".to_string());
        dialog.set_snapshot(TaskViewSnapshot {
            project_id: "project-1".to_string(),
            sections: vec![
                (TaskViewSection::Running, Vec::new()),
                (
                    TaskViewSection::Waiting,
                    vec![TaskViewLine {
                        id: "wo-1".to_string(),
                        title: "fix the flaky test".to_string(),
                        detail: "wo-1 · immediate".to_string(),
                        attention: None,
                    }],
                ),
                (TaskViewSection::Attention, Vec::new()),
                (TaskViewSection::Recent, Vec::new()),
            ],
            selected_id: Some("wo-1".to_string()),
            summary_line: "1 active · 1 waiting · 0 attention · 0 lanes".to_string(),
            loading: false,
            error: None,
            notice: None,
            trigger_labels: std::collections::HashMap::new(),
        });
        let normal = render_to_text(&mut dialog, 80, 24);
        assert!(normal.contains("Project Tasks"));
        assert!(normal.contains("RUNNING"));
        assert!(normal.contains("FUTURE / WAITING"));
        assert!(normal.contains("fix the flaky test"));
        // Narrow terminals hide the detail column but keep titles and
        // section headers; tiny areas are a guarded no-op.
        let narrow = render_to_text(&mut dialog, 40, 20);
        assert!(narrow.contains("FUTURE / WAITING"));
        assert!(narrow.contains("fix the flaky test"));
        let _tiny = render_to_text(&mut dialog, 20, 5);
    }
}
