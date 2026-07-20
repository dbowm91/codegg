//! Bounded session projection snapshot.
//!
//! A snapshot is the canonical, deterministic projection of a session
//! at one point in time. Two compliant reducers given the same
//! snapshot and ordered event stream MUST reach equivalent
//! [`SessionProjectionSnapshot`] values.

use serde::{Deserialize, Serialize};

use crate::projection::caps::PROJECTION_PROTOCOL_VERSION;
use crate::projection::dto::{
    JobProjection, RunProjection, SessionSummaryProjection, WorkspaceSummaryProjection,
};
use crate::projection::limits::{
    MAX_PROJECTION_DIAGNOSTICS, MAX_PROJECTION_JOBS, MAX_PROJECTION_RUNS,
};

/// Bounded projection of one session plus its surrounding context.
///
/// The snapshot is what a client receives on connect, on resync, and
/// after a daemon restart. Subsequent updates arrive as ordered
/// projection events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionProjectionSnapshot {
    /// Negotiated projection protocol version that produced this
    /// snapshot.
    pub protocol_version: u32,
    /// Last `event_seq` value embedded in this snapshot.
    pub event_seq: u64,
    /// Wall-clock timestamp at which the snapshot was generated.
    pub generated_at_ms: i64,
    /// Id of the primary session this snapshot represents.
    pub primary_session_id: String,
    /// Project the primary session belongs to.
    pub project_id: String,
    /// Workspace the primary session belongs to.
    pub workspace_id: String,
    /// Bounded summary of the primary session.
    pub primary_session: SessionSummaryProjection,
    /// Bounded summaries of other sessions in the same project, kept
    /// for cross-session visibility in the TUI sidebar.
    pub secondary_sessions: Vec<SessionSummaryProjection>,
    /// Bounded summary of the active workspace.
    pub workspace: WorkspaceSummaryProjection,
    /// Active turn projection. `None` when no turn is in flight.
    pub active_turn: Option<crate::projection::dto::TurnProjection>,
    /// Most recent completed turns (newest first). Bounded by
    /// [`crate::projection::limits::MAX_PROJECTION_RECENT_TOOLS`] of
    /// turn slots — older turns collapse into
    /// [`SessionSummaryProjection::recent_summary`].
    pub recent_turns: Vec<crate::projection::dto::TurnProjection>,
    /// Active and recently completed runs.
    pub runs: Vec<RunProjection>,
    /// Active and recently observed durable jobs.
    pub jobs: Vec<JobProjection>,
    /// Bounded diagnostic list emitted by the reducer.
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

impl SessionProjectionSnapshot {
    /// Build a minimal snapshot used as a starting point for reducers
    /// and adapters. The `diagnostics`, `recent_turns`, `runs`, `jobs`,
    /// and `secondary_sessions` collections are empty.
    pub fn empty(session_id: &str, project_id: &str, workspace_id: &str) -> Self {
        Self {
            protocol_version: PROJECTION_PROTOCOL_VERSION,
            event_seq: 0,
            generated_at_ms: 0,
            primary_session_id: session_id.to_string(),
            project_id: project_id.to_string(),
            workspace_id: workspace_id.to_string(),
            primary_session: SessionSummaryProjection {
                session_id: session_id.to_string(),
                project_id: project_id.to_string(),
                workspace_id: workspace_id.to_string(),
                title: String::new(),
                status: "idle".to_string(),
                selected_model: None,
                selected_agent: None,
                has_active_turn: false,
                pending_permission_count: 0,
                pending_question_count: 0,
                input_tokens: None,
                output_tokens: None,
                active_subagents: 0,
                time_created_at: None,
                time_updated_at: None,
                recent_summary: None,
            },
            secondary_sessions: Vec::new(),
            workspace: WorkspaceSummaryProjection {
                workspace_id: workspace_id.to_string(),
                canonical_root: String::new(),
                display_name: String::new(),
                created_at: 0,
                last_opened_at: 0,
                archived_at: None,
                active_sessions: 0,
                services_active: false,
                active_leases: 0,
                config_revision: 0,
                health: Default::default(),
            },
            active_turn: None,
            recent_turns: Vec::new(),
            runs: Vec::new(),
            jobs: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Push a diagnostic, dropping the oldest if the cap is reached.
    pub fn push_diagnostic(&mut self, diag: ProjectionDiagnostic) {
        if self.diagnostics.len() >= MAX_PROJECTION_DIAGNOSTICS {
            self.diagnostics.remove(0);
        }
        self.diagnostics.push(diag);
    }

    /// Push a run summary, dropping the oldest if the cap is reached.
    pub fn push_run(&mut self, run: RunProjection) {
        if self.runs.len() >= MAX_PROJECTION_RUNS {
            self.runs.remove(0);
        }
        self.runs.push(run);
    }

    /// Upsert a job, dropping the oldest if the cap is reached.
    pub fn upsert_job(&mut self, job: JobProjection) {
        if let Some(slot) = self.jobs.iter_mut().find(|j| j.job_id == job.job_id) {
            *slot = job;
            return;
        }
        if self.jobs.len() >= MAX_PROJECTION_JOBS {
            self.jobs.remove(0);
        }
        self.jobs.push(job);
    }
}

/// Diagnostic entry recorded by the reducer. Diagnostics are
/// strictly informational and must not affect reducer logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionDiagnostic {
    pub code: String,
    pub message: String,
    pub at_ms: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
}

impl ProjectionDiagnostic {
    pub fn new(code: impl Into<String>, message: impl Into<String>, at_ms: i64) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            at_ms,
            session_id: None,
            turn_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot_has_default_fields() {
        let snap = SessionProjectionSnapshot::empty("s", "p", "w");
        assert_eq!(snap.primary_session_id, "s");
        assert_eq!(snap.project_id, "p");
        assert_eq!(snap.workspace_id, "w");
        assert!(snap.active_turn.is_none());
        assert!(snap.recent_turns.is_empty());
        assert!(snap.runs.is_empty());
        assert!(snap.jobs.is_empty());
        assert!(snap.diagnostics.is_empty());
    }

    #[test]
    fn push_diagnostic_caps_at_max() {
        let mut snap = SessionProjectionSnapshot::empty("s", "p", "w");
        for i in 0..(MAX_PROJECTION_DIAGNOSTICS * 2) {
            snap.push_diagnostic(ProjectionDiagnostic::new("c", format!("{i}"), i as i64));
        }
        assert_eq!(snap.diagnostics.len(), MAX_PROJECTION_DIAGNOSTICS);
    }
}
