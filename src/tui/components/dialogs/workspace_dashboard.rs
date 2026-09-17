//! Project Work Orders M004: global Workspace dashboard dialog.
//!
//! FocusManager-owned modal rendering the bounded global dashboard
//! projection: one coarse row per authorized project with running,
//! future, attention, permission, and question indicators. The
//! component renders a [`WorkspaceDashboardSnapshot`] synced from
//! `WorkspaceDashboardState` after every mutation; it owns no daemon
//! truth. Navigation uses `j`/`k` and arrows with `g`/`G` top/bottom,
//! matching existing list views; `Enter` descends to the project Task
//! view (or the materialized session for an expanded running task);
//! `Tab` expands/collapses the selected project's running tasks (one
//! bounded lazy fetch, never N+1); `Ctrl+R` refreshes; `Esc` returns to
//! the exact prior project/session view. Other bare characters are
//! filter text. Closing or switching views never cancels daemon-owned
//! work.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::sync::Arc;

use crate::tui::app::state::{
    dashboard_row_badge, WorkspaceDashboardState, MAX_DASHBOARD_VISIBLE_ROWS,
};
use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

/// One rendered dashboard row (bounded, secret-free).
#[derive(Debug, Clone)]
pub struct WorkspaceDashboardLine {
    pub project_id: String,
    pub display_name: String,
    pub lifecycle: String,
    pub badge: String,
    pub status: String,
    pub stale: bool,
}

/// Render snapshot synced from `WorkspaceDashboardState`.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceDashboardSnapshot {
    pub lines: Vec<WorkspaceDashboardLine>,
    pub selected_id: Option<String>,
    pub summary_line: String,
    pub filter_query: String,
    pub loading: bool,
    pub dirty: bool,
    pub truncated: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub expanded_project_id: Option<String>,
    pub expanded_titles: Vec<String>,
    pub expanded_loading: bool,
}

impl WorkspaceDashboardSnapshot {
    pub fn from_state(state: &WorkspaceDashboardState) -> Self {
        let indices = state.filtered_indices();
        let total = indices.len();
        let center = state.selected_row.min(total.saturating_sub(1));
        let visible = MAX_DASHBOARD_VISIBLE_ROWS.min(total.max(1));
        let half = visible / 2;
        let start = if center < half || total <= visible {
            0
        } else if center + half >= total {
            total.saturating_sub(visible)
        } else {
            center - half
        };
        let mut lines = Vec::new();
        for &i in indices[start..(start + visible).min(total)].iter() {
            let Some(row) = state.rows.get(i) else {
                continue;
            };
            lines.push(WorkspaceDashboardLine {
                project_id: row.summary.project_id.clone(),
                display_name: row.summary.display_name.clone(),
                lifecycle: row.summary.lifecycle.clone(),
                badge: dashboard_row_badge(row),
                status: row.summary.coarse_status_code.clone(),
                stale: row.stale,
            });
        }
        let attention: u64 = state.rows.iter().map(|row| row.attention_count()).sum();
        let running: u64 = state.rows.iter().map(|row| row.running_count()).sum();
        let mut summary_line = format!(
            "{} projects · {} running · {} need attention",
            state.rows.len(),
            running,
            attention
        );
        if state.truncated {
            summary_line.push_str(" · more on daemon (refine filter)");
        }
        if state.dirty {
            summary_line.push_str(" · updating…");
        }
        Self {
            lines,
            selected_id: state.selected().map(|row| row.summary.project_id.clone()),
            summary_line,
            filter_query: state.query.clone(),
            loading: state.loading,
            dirty: state.dirty,
            truncated: state.truncated,
            error: state.error.clone(),
            notice: state.notice.clone(),
            expanded_project_id: state.expanded_project_id.clone(),
            expanded_titles: state
                .expanded_tasks
                .iter()
                .map(|task| {
                    let title = task
                        .title
                        .as_deref()
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .unwrap_or("(untitled task)");
                    let short: String = task.work_order_id.chars().take(8).collect();
                    format!("{short} · {}", title.chars().take(48).collect::<String>())
                })
                .collect(),
            expanded_loading: state.expanded_loading,
        }
    }
}

#[derive(Clone)]
pub struct WorkspaceDashboardDialog {
    snapshot: WorkspaceDashboardSnapshot,
}

impl WorkspaceDashboardDialog {
    pub fn new() -> Self {
        Self {
            snapshot: WorkspaceDashboardSnapshot {
                loading: true,
                ..WorkspaceDashboardSnapshot::default()
            },
        }
    }

    pub fn set_snapshot(&mut self, snapshot: WorkspaceDashboardSnapshot) {
        self.snapshot = snapshot;
    }
}

impl Default for WorkspaceDashboardDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for WorkspaceDashboardDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        // Bare `j`/`k`/`g`/`G` navigate (Vim convention, same as the
        // picker/Task view); every other bare printable character is
        // filter text handled by the App. `Tab` expands, `Ctrl+R`
        // refreshes, so `e`/`r` remain typeable in the filter.
        if key.modifiers == crossterm::event::KeyModifiers::CONTROL
            && key.code == KeyCode::Char('r')
        {
            return Some(TuiMsg::WorkspaceDashboardRefresh);
        }
        match key.code {
            KeyCode::Esc => Some(TuiMsg::CloseDialog),
            KeyCode::Up | KeyCode::Char('k') => Some(TuiMsg::WorkspaceDashboardMove { delta: -1 }),
            KeyCode::Down | KeyCode::Char('j') => Some(TuiMsg::WorkspaceDashboardMove { delta: 1 }),
            KeyCode::PageUp => Some(TuiMsg::WorkspaceDashboardMove { delta: -10 }),
            KeyCode::PageDown => Some(TuiMsg::WorkspaceDashboardMove { delta: 10 }),
            KeyCode::Home => Some(TuiMsg::WorkspaceDashboardMove { delta: -10_000 }),
            KeyCode::End => Some(TuiMsg::WorkspaceDashboardMove { delta: 10_000 }),
            KeyCode::Char('g') => Some(TuiMsg::WorkspaceDashboardMove { delta: -10_000 }),
            KeyCode::Char('G') => Some(TuiMsg::WorkspaceDashboardMove { delta: 10_000 }),
            KeyCode::Enter => Some(TuiMsg::WorkspaceDashboardOpen),
            KeyCode::Tab => Some(TuiMsg::WorkspaceDashboardToggleExpand),
            // Filter text is handled by the App (bare-char routing while
            // the dashboard is open); the dialog consumes Tab so focus
            // never leaks to SwitchAgent.
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
        render_dashboard(frame, area, theme, &self.snapshot);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::WorkspaceDashboard
    }
}

fn render_dashboard(
    frame: &mut Frame,
    area: Rect,
    theme: &Arc<Theme>,
    snapshot: &WorkspaceDashboardSnapshot,
) {
    if area.height < 8 || area.width < 30 {
        return;
    }
    let width = area.width as usize;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Workspace ".to_string());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width < 10 {
        return;
    }
    let mut lines = vec![Line::from(Span::styled(
        truncate(&snapshot.summary_line, width),
        Style::default().fg(theme.muted),
    ))];
    if !snapshot.filter_query.is_empty() || snapshot.loading {
        let filter_line = if snapshot.filter_query.is_empty() {
            "Loading workspace…".to_string()
        } else {
            format!("Filter: {}", snapshot.filter_query)
        };
        lines.push(Line::from(Span::styled(
            truncate(&filter_line, width),
            Style::default().fg(theme.muted),
        )));
    }
    if snapshot.loading && snapshot.lines.is_empty() {
        lines.push(Line::from(Span::raw("Loading projects…")));
    }
    let detailed = width >= 60;
    if snapshot.lines.is_empty() && !snapshot.loading {
        lines.push(Line::from(Span::styled(
            "  (no matching projects)",
            Style::default().fg(theme.muted),
        )));
    }
    for line in &snapshot.lines {
        let selected = snapshot.selected_id.as_deref() == Some(line.project_id.as_str());
        let prefix = if selected { "▸ " } else { "  " };
        let mut spans = vec![Span::styled(
            prefix,
            Style::default()
                .fg(if selected { theme.primary } else { theme.muted })
                .add_modifier(Modifier::BOLD),
        )];
        spans.push(Span::styled(
            truncate(&line.display_name, width.saturating_sub(6)),
            Style::default().fg(if selected {
                theme.foreground
            } else {
                theme.muted
            }),
        ));
        if detailed {
            spans.push(Span::styled(
                format!("  {}", truncate(&line.badge, 40)),
                Style::default().fg(theme.muted),
            ));
        }
        lines.push(Line::from(spans));
        let mut meta = format!("    {} · {}", line.lifecycle, line.status);
        if line.stale {
            meta.push_str(" · stale");
        }
        lines.push(Line::from(Span::styled(
            truncate(&meta, width.saturating_sub(2)),
            Style::default().fg(if line.status == "idle" {
                theme.muted
            } else {
                theme.warning
            }),
        )));
        if snapshot.expanded_project_id.as_deref() == Some(line.project_id.as_str()) {
            if snapshot.expanded_loading {
                lines.push(Line::from(Span::styled(
                    "      loading tasks…",
                    Style::default().fg(theme.muted),
                )));
            } else if snapshot.expanded_titles.is_empty() {
                lines.push(Line::from(Span::styled(
                    "      (no running tasks)",
                    Style::default().fg(theme.muted),
                )));
            } else {
                for title in &snapshot.expanded_titles {
                    lines.push(Line::from(Span::styled(
                        truncate(&format!("      {title}"), width.saturating_sub(2)),
                        Style::default().fg(theme.foreground),
                    )));
                }
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
        "j/k move · g/G top/bottom · type to filter · Enter open · Tab tasks · Ctrl+R refresh · Esc back",
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
    use crate::protocol::work_order::ProjectActivitySummaryDto;

    fn test_row(id: &str, name: &str) -> crate::tui::app::state::WorkspaceDashboardRow {
        crate::tui::app::state::WorkspaceDashboardRow {
            summary: ProjectActivitySummaryDto {
                project_id: id.to_string(),
                display_name: name.to_string(),
                lifecycle: "active".to_string(),
                running_session_count: 1,
                running_work_order_count: 0,
                waiting_work_order_count: 2,
                future_work_order_count: 2,
                needs_attention_count: 0,
                pending_permission_count: 1,
                pending_question_count: 0,
                last_activity_at: Some(7),
                coarse_status_code: "permission".to_string(),
                counts_visible: true,
            },
            stale: false,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn vim_keys_map_to_dashboard_actions_without_collisions() {
        let mut dialog = WorkspaceDashboardDialog::new();
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('j'))),
            Some(TuiMsg::WorkspaceDashboardMove { delta: 1 })
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('k'))),
            Some(TuiMsg::WorkspaceDashboardMove { delta: -1 })
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('g'))),
            Some(TuiMsg::WorkspaceDashboardMove { delta: -10_000 })
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('G'))),
            Some(TuiMsg::WorkspaceDashboardMove { delta: 10_000 })
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::WorkspaceDashboardOpen)
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Tab)),
            Some(TuiMsg::WorkspaceDashboardToggleExpand)
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(
                KeyCode::Char('r'),
                crossterm::event::KeyModifiers::CONTROL
            )),
            Some(TuiMsg::WorkspaceDashboardRefresh)
        );
        // Bare `e`/`r`/`/` are filter text (None here; the App routes
        // them into the dashboard query), never actions.
        assert_eq!(dialog.handle_key(key(KeyCode::Char('e'))), None);
        assert_eq!(dialog.handle_key(key(KeyCode::Char('r'))), None);
        assert_eq!(dialog.handle_key(key(KeyCode::Char('/'))), None);
        assert_eq!(
            dialog.handle_key(key(KeyCode::Esc)),
            Some(TuiMsg::CloseDialog)
        );
    }

    #[test]
    fn snapshot_shows_badges_without_sensitive_content() {
        let mut state = WorkspaceDashboardState::new(None, 0);
        state.loading = false;
        state.rows = vec![test_row("p1", "Payments")];
        let snapshot = WorkspaceDashboardSnapshot::from_state(&state);
        assert_eq!(snapshot.lines.len(), 1);
        assert!(snapshot.lines[0].badge.contains("permission×1"));
        assert!(snapshot.lines[0].badge.contains("running×1"));
        assert!(!snapshot.lines[0].badge.contains("secret"));
        assert!(snapshot.summary_line.contains("1 projects"));
    }

    fn render_to_text(dialog: &mut WorkspaceDashboardDialog, width: u16, height: u16) -> String {
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
    fn dashboard_renders_across_terminal_sizes_without_panic() {
        let mut state = WorkspaceDashboardState::new(None, 0);
        state.loading = false;
        state.rows = vec![test_row("p1", "Payments"), test_row("p2", "Search")];
        let mut dialog = WorkspaceDashboardDialog::new();
        dialog.set_snapshot(WorkspaceDashboardSnapshot::from_state(&state));
        let normal = render_to_text(&mut dialog, 80, 24);
        assert!(normal.contains("Workspace"));
        assert!(normal.contains("Payments"));
        assert!(normal.contains("Search"));
        let narrow = render_to_text(&mut dialog, 40, 20);
        assert!(narrow.contains("Payments"));
        let _tiny = render_to_text(&mut dialog, 20, 5);
    }
}
