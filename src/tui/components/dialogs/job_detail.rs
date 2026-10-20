use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::sync::Arc;

use crate::protocol::dto::{JobAttemptDto, JobRecordDto};
use crate::tui::{
    app::TuiMsg,
    components::component::{Component, DialogType},
    theme::Theme,
};

#[derive(Debug, Clone)]
pub struct JobDetailDialog {
    pub job: Option<JobRecordDto>,
    pub attempts: Vec<JobAttemptDto>,
    pub job_id: String,
    pub loading: bool,
    pub error: Option<String>,
    pub cancel_pending: bool,
    pub can_cancel: bool,
}

impl JobDetailDialog {
    pub fn loading(job_id: String) -> Self {
        Self {
            job: None,
            attempts: Vec::new(),
            job_id,
            loading: true,
            error: None,
            cancel_pending: false,
            can_cancel: true,
        }
    }

    pub fn loaded(job_id: String, job: Option<JobRecordDto>, attempts: Vec<JobAttemptDto>) -> Self {
        Self {
            job,
            attempts,
            job_id,
            loading: false,
            error: None,
            cancel_pending: false,
            can_cancel: true,
        }
    }

    fn live(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| crate::tui::app::state::tasks::is_live_state(&job.state))
    }
}

impl Component for JobDetailDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Some(TuiMsg::CloseDialog),
            KeyCode::Char('r') => Some(TuiMsg::OpenJobDetail {
                job_id: self.job_id.clone(),
            }),
            KeyCode::Char('x') if self.can_cancel && self.live() && !self.cancel_pending => {
                self.cancel_pending = true;
                Some(TuiMsg::CancelJob {
                    job_id: self.job_id.clone(),
                })
            }
            _ => None,
        }
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let area = centered(area, 76, 24);
        frame.render_widget(Clear, area);
        let outer = Block::default()
            .title(" Job detail ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        let mut lines = vec![Line::from(vec![
            Span::styled("Job: ", Style::default().fg(theme.muted)),
            Span::raw(&self.job_id),
        ])];
        if self.loading {
            lines.push(Line::from("Loading job details…"));
        }
        if let Some(error) = &self.error {
            lines.push(Line::from(Span::styled(
                error,
                Style::default().fg(Color::Red),
            )));
        }
        if let Some(job) = &self.job {
            lines.extend([
                Line::from(format!(
                    "State: {} · kind: {} · priority: {}",
                    job.state, job.kind, job.priority
                )),
                Line::from(format!(
                    "Session: {}",
                    job.session_id.as_deref().unwrap_or("unbound")
                )),
                Line::from(format!(
                    "Attempts: {} · current: {}",
                    job.attempt_count,
                    job.current_attempt_id.as_deref().unwrap_or("none")
                )),
                Line::from(format!(
                    "Created: {} · updated: {}",
                    job.created_at_ms, job.updated_at_ms
                )),
            ]);
            if let Some(reason) = &job.cancel_reason {
                lines.push(Line::from(format!("Cancel request: {reason}")));
            }
            if !self.attempts.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Attempt history",
                    Style::default().fg(theme.primary),
                )));
                for attempt in self.attempts.iter().rev().take(8).rev() {
                    let detail = attempt.error_class.as_deref().unwrap_or_else(|| {
                        attempt
                            .executor
                            .as_deref()
                            .unwrap_or("executor unavailable")
                    });
                    lines.push(Line::from(format!(
                        "#{} {} · {} · {}",
                        attempt.sequence, attempt.state, attempt.attempt_id, detail
                    )));
                }
            }
        }
        let footer = if self.cancel_pending {
            "Cancel requested… · r refresh · Esc close"
        } else if self.live() && self.can_cancel {
            "x request cancel · r refresh · Esc close"
        } else if self.live() {
            "read only · r refresh · Esc close"
        } else {
            "r refresh · Esc close"
        };
        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), areas[0]);
        frame.render_widget(
            Paragraph::new(footer).style(Style::default().fg(theme.muted)),
            areas[1],
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::JobDetail
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn job(state: &str) -> JobRecordDto {
        JobRecordDto {
            job_id: "job-stable-1".into(),
            workspace_id: "workspace-1".into(),
            session_id: Some("session-1".into()),
            turn_id: None,
            kind: "test".into(),
            source: serde_json::Value::Null,
            priority: "normal".into(),
            payload: serde_json::Value::Null,
            state: state.into(),
            current_attempt_id: None,
            attempt_count: 0,
            retry_policy: serde_json::Value::Null,
            idempotency: "none".into(),
            not_before: None,
            deadline: None,
            schedule_id: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            terminal_at_ms: None,
            cancel_requested_at: None,
            cancel_reason: None,
            labels: Default::default(),
            // Remote-target identity (Eggwork M001). `None` for a local job.
            target_node_id: None,
        }
    }

    #[test]
    fn cancel_is_identity_keyed_and_offered_only_for_a_live_job() {
        let mut loading = JobDetailDialog::loading("job-stable-1".into());
        assert_eq!(
            loading.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            None,
            "no action is offered before the daemon confirms the job is live"
        );

        let mut live = JobDetailDialog::loaded("job-stable-1".into(), Some(job("running")), vec![]);
        assert_eq!(
            live.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            Some(TuiMsg::CancelJob {
                job_id: "job-stable-1".into()
            })
        );
        assert!(live.cancel_pending);
        assert_eq!(
            live.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            None,
            "a second keypress cannot retarget or duplicate an in-flight cancellation"
        );

        let mut observer =
            JobDetailDialog::loaded("job-stable-1".into(), Some(job("running")), vec![]);
        observer.can_cancel = false;
        assert_eq!(
            observer.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            None,
            "read-only observers see details without an enabled cancel affordance"
        );

        let mut terminal =
            JobDetailDialog::loaded("job-stable-1".into(), Some(job("succeeded")), vec![]);
        assert_eq!(
            terminal.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            None
        );
    }
}
