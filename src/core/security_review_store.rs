//! The daemon's own registry of the security-review runs it started. The
//! bound is a count: the oldest run is dropped when it is reached.

use std::sync::Arc;

use codegg_protocol::security_review::{SecurityReviewRunDto, SecurityReviewRunStatusDto};
use tokio::sync::Notify;

/// How many runs the daemon keeps.
const MAX_RUNS: usize = 16;

/// The daemon's registry of the security-review runs it started.
///
/// The run's state lives here, not in the frontend: the read answers from
/// this registry, so a run nobody can read back is not a run.
#[derive(Default)]
pub struct SecurityReviewStore {
    runs: parking_lot::Mutex<Vec<SecurityReviewRun>>,
}

/// One run, as the daemon holds it.
pub struct SecurityReviewRun {
    /// The run as the protocol carries it.
    pub run: SecurityReviewRunDto,
    /// The session the run belongs to.
    pub session_id: String,
    /// The workspace the run belongs to.
    pub workspace_id: String,
    /// When the daemon started the run, in epoch milliseconds.
    pub started_at_ms: i64,
    pub cancel: Arc<Notify>,
}

impl SecurityReviewStore {
    /// Record a run the daemon just started for this session.
    pub(crate) fn start(
        &self,
        run: SecurityReviewRunDto,
        session_id: String,
        workspace_id: String,
        started_at_ms: i64,
    ) -> Arc<Notify> {
        let cancel = Arc::new(Notify::new());
        let mut runs = self.runs.lock();
        runs.push(SecurityReviewRun {
            run,
            session_id,
            workspace_id,
            started_at_ms,
            cancel: Arc::clone(&cancel),
        });
        while runs.len() > MAX_RUNS {
            runs.remove(0);
        }
        cancel
    }

    /// Apply the outcome to a run the daemon holds.
    pub(crate) fn finish(&self, run_id: &str, apply: impl FnOnce(&mut SecurityReviewRunDto)) {
        let mut runs = self.runs.lock();
        if let Some(entry) = runs.iter_mut().find(|entry| entry.run.run_id == run_id) {
            apply(&mut entry.run);
        }
    }

    /// The run this session named, if the daemon still holds it.
    pub(crate) fn get(&self, session_id: &str, run_id: &str) -> Option<SecurityReviewRunDto> {
        self.runs
            .lock()
            .iter()
            .find(|entry| entry.session_id == session_id && entry.run.run_id == run_id)
            .map(|entry| entry.run.clone())
    }

    /// The session's newest run, if the daemon holds one.
    pub(crate) fn newest(&self, session_id: &str) -> Option<SecurityReviewRunDto> {
        self.runs
            .lock()
            .iter()
            .filter(|entry| entry.session_id == session_id)
            .max_by_key(|entry| entry.started_at_ms)
            .map(|entry| entry.run.clone())
    }

    pub(crate) fn running_turn_id(&self, session_id: &str) -> Option<String> {
        self.runs
            .lock()
            .iter()
            .filter(|entry| {
                entry.session_id == session_id
                    && entry.run.status == SecurityReviewRunStatusDto::Running
            })
            .max_by_key(|entry| entry.started_at_ms)
            .map(|entry| entry.run.turn_id.clone())
    }

    /// Cancel the running review this turn names.
    pub(crate) fn cancel_by_turn(&self, session_id: &str, turn_id: &str) -> bool {
        let mut runs = self.runs.lock();
        let Some(entry) = runs.iter_mut().find(|entry| {
            entry.session_id == session_id
                && entry.run.turn_id == turn_id
                && entry.run.status == SecurityReviewRunStatusDto::Running
        }) else {
            return false;
        };
        entry.run.status = SecurityReviewRunStatusDto::Cancelled;
        entry.run.completed_at_ms = Some(chrono::Utc::now().timestamp_millis());
        entry.cancel.notify_one();
        true
    }
}

/// The refusal a security-review request earns before it starts.
pub(crate) async fn validate_security_review_scope(
    pool: &sqlx::SqlitePool,
    protocol_version: u32,
    project_id: &str,
    workspace_id: &str,
    session_id: &str,
) -> Result<(), (&'static str, String)> {
    if protocol_version != codegg_protocol::security_review::SECURITY_REVIEW_PROTOCOL_VERSION {
        return Err((
            "invalid_security_review_request",
            "unsupported security review protocol version".to_string(),
        ));
    }
    if project_id.is_empty() || workspace_id.is_empty() || session_id.is_empty() {
        return Err((
            "invalid_security_review_request",
            "project, workspace and session are required".to_string(),
        ));
    }
    if let Err(error) =
        crate::lsp::mutation::validate_session_workspace_binding(pool, session_id, workspace_id)
            .await
    {
        let code = match &error {
            crate::lsp::mutation::LspSessionWorkspaceBindingError::Mismatch => {
                "session_workspace_mismatch"
            }
            crate::lsp::mutation::LspSessionWorkspaceBindingError::Missing => "session_not_found",
            crate::lsp::mutation::LspSessionWorkspaceBindingError::Invalid(_) => {
                "invalid_security_review_request"
            }
            crate::lsp::mutation::LspSessionWorkspaceBindingError::Storage(_) => {
                "session_workspace_binding_failed"
            }
        };
        return Err((code, error.to_string()));
    }
    Ok(())
}

/// The report a run carries, bounded by the protocol's own bound.
pub(crate) fn bound_security_review_report(report: &str) -> String {
    let limit = codegg_protocol::security_review::SECURITY_REVIEW_MAX_REPORT_BYTES;
    if report.len() <= limit {
        return report.to_string();
    }
    let mut end = limit;
    while end > 0 && !report.is_char_boundary(end) {
        end -= 1;
    }
    report[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_run(run_id: &str, turn_id: &str) -> SecurityReviewRunDto {
        SecurityReviewRunDto {
            run_id: run_id.to_owned(),
            turn_id: turn_id.to_owned(),
            session_id: "session-1".to_owned(),
            status: SecurityReviewRunStatusDto::Running,
            report: None,
            error: None,
            finding_count: 0,
            prompt_count: 0,
            note_count: 0,
            enriched: false,
            lsp_available: false,
            completed_at_ms: None,
        }
    }

    #[test]
    fn the_cancel_stops_only_the_running_review_the_turn_names() {
        let store = SecurityReviewStore::default();
        store.start(
            running_run("run-1", "turn-1"),
            "session-1".to_owned(),
            "workspace-1".to_owned(),
            1,
        );
        assert_eq!(
            store.running_turn_id("session-1").as_deref(),
            Some("turn-1"),
            "a running review names the turn its cancel carries"
        );
        assert!(
            !store.cancel_by_turn("other-session", "turn-1"),
            "another session cannot cancel this run"
        );
        assert!(
            store.cancel_by_turn("session-1", "turn-1"),
            "the session that owns the run cancels it"
        );
        assert_eq!(
            store
                .get("session-1", "run-1")
                .map(|run| run.status)
                .expect("the run is still held"),
            SecurityReviewRunStatusDto::Cancelled,
            "the cancelled run reads back as cancelled"
        );
        assert!(
            store.running_turn_id("session-1").is_none(),
            "a cancelled run is no longer the session's running review"
        );
        assert!(
            !store.cancel_by_turn("session-1", "turn-1"),
            "a cancelled run is not cancelled twice"
        );
    }

    #[tokio::test]
    async fn the_cancel_signals_the_task_that_runs_the_review() {
        let store = SecurityReviewStore::default();
        let cancel = store.start(
            running_run("run-1", "turn-1"),
            "session-1".to_owned(),
            "workspace-1".to_owned(),
            1,
        );
        let signalled = cancel.notified();
        tokio::pin!(signalled);
        assert!(store.cancel_by_turn("session-1", "turn-1"));
        tokio::time::timeout(std::time::Duration::from_millis(200), signalled)
            .await
            .expect("the cancel must reach the task that runs the review");
    }
}
