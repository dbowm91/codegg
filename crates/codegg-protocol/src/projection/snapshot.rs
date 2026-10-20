//! Bounded session projection snapshot.
//!
//! A snapshot is the canonical, deterministic projection of a session
//! at one point in time. Two compliant reducers given the same
//! snapshot and ordered event stream MUST reach equivalent
//! [`SessionProjectionSnapshot`] values.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::projection::caps::PROJECTION_PROTOCOL_VERSION;
use crate::projection::dto::{
    AgentRunGroupSummaryProjection, AgentRunSummary, ConvergenceSummaryProjection, JobProjection,
    RunProjection, SessionSummaryProjection, ToolProgramSummary, WorkspaceSummaryProjection,
    WorktreeSummaryProjection,
};
use crate::projection::limits::{
    MAX_PROJECTION_AGENT_RUNS, MAX_PROJECTION_DIAGNOSTICS, MAX_PROJECTION_JOBS,
    MAX_PROJECTION_RUNS, MAX_PROJECTION_RUN_GROUPS, MAX_PROJECTION_SESSION_SUBAGENTS,
    MAX_PROJECTION_TOOL_PROGRAMS, MAX_PROJECTION_WORKTREES,
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
    pub secondary_sessions: VecDeque<SessionSummaryProjection>,
    /// Bounded summary of the active workspace.
    pub workspace: WorkspaceSummaryProjection,
    /// Active turn projection. `None` when no turn is in flight.
    pub active_turn: Option<crate::projection::dto::TurnProjection>,
    /// Session-owned child instances. Unlike the conversational agent tree,
    /// these survive parent-turn replacement and are the liveness authority.
    #[serde(default)]
    pub subagents: VecDeque<crate::projection::dto::AgentTreeNodeProjection>,
    /// Most recent completed turns (newest first). Bounded by
    /// [`crate::projection::limits::MAX_PROJECTION_RECENT_TOOLS`] of
    /// turn slots — older turns collapse into
    /// [`SessionSummaryProjection::recent_summary`].
    pub recent_turns: VecDeque<crate::projection::dto::TurnProjection>,
    /// Active and recently completed runs.
    pub runs: VecDeque<RunProjection>,
    /// Active and recently observed durable jobs.
    pub jobs: VecDeque<JobProjection>,
    /// Active and recently completed background tool programs.
    pub tool_programs: VecDeque<ToolProgramSummary>,
    /// Durable delegated agent runs, including detached/background runs.
    #[serde(default)]
    pub agent_runs: VecDeque<AgentRunSummary>,
    /// Managed worktrees referenced by projected agent runs.
    #[serde(default)]
    pub worktrees: VecDeque<WorktreeSummaryProjection>,
    /// Durable run-group summaries.
    #[serde(default)]
    pub run_groups: VecDeque<AgentRunGroupSummaryProjection>,
    /// Durable bounded convergence summaries.
    #[serde(default)]
    pub convergences: VecDeque<ConvergenceSummaryProjection>,
    /// Bounded diagnostic list emitted by the reducer.
    pub diagnostics: VecDeque<ProjectionDiagnostic>,
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
            secondary_sessions: VecDeque::new(),
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
            subagents: VecDeque::new(),
            recent_turns: VecDeque::new(),
            runs: VecDeque::new(),
            jobs: VecDeque::new(),
            tool_programs: VecDeque::new(),
            agent_runs: VecDeque::new(),
            worktrees: VecDeque::new(),
            run_groups: VecDeque::new(),
            convergences: VecDeque::new(),
            diagnostics: VecDeque::new(),
        }
    }

    /// Upsert one session-owned child instance, evicting the oldest terminal
    /// instance first when the independent session cap is reached.
    pub fn upsert_subagent(
        &mut self,
        node: crate::projection::dto::AgentTreeNodeProjection,
    ) -> bool {
        if let Some(slot) = self
            .subagents
            .iter_mut()
            .find(|item| item.child_instance_id == node.child_instance_id)
        {
            if matches!(
                slot.status,
                crate::projection::dto::AgentTreeStatus::Running
            ) {
                let mut node = node;
                // These fields describe the admitted generation, not mutable
                // activity. Duplicate starts cannot rebind its lineage.
                node.originating_turn_id
                    .clone_from(&slot.originating_turn_id);
                node.task_id = slot.task_id;
                node.created_at = slot.created_at;
                *slot = node;
            } else if !matches!(
                node.status,
                crate::projection::dto::AgentTreeStatus::Running
            ) && node.completed_at.unwrap_or_default()
                >= slot.completed_at.unwrap_or_default()
            {
                *slot = node;
            }
            return true;
        }
        if self.subagents.len() >= MAX_PROJECTION_SESSION_SUBAGENTS {
            if let Some(index) = self.subagents.iter().position(|item| {
                !matches!(
                    item.status,
                    crate::projection::dto::AgentTreeStatus::Running
                )
            }) {
                self.subagents.remove(index);
            } else {
                return false;
            }
        }
        self.subagents.push_back(node);
        true
    }

    /// Push a diagnostic, dropping the oldest if the cap is reached.
    pub fn push_diagnostic(&mut self, diag: ProjectionDiagnostic) {
        if self.diagnostics.len() >= MAX_PROJECTION_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(diag);
    }

    /// Push a run summary, dropping the oldest if the cap is reached.
    pub fn push_run(&mut self, run: RunProjection) {
        if self.runs.len() >= MAX_PROJECTION_RUNS {
            self.runs.pop_front();
        }
        self.runs.push_back(run);
    }

    /// Upsert a job, dropping the oldest if the cap is reached.
    pub fn upsert_job(&mut self, job: JobProjection) {
        if let Some(slot) = self.jobs.iter_mut().find(|j| j.job_id == job.job_id) {
            let prior_terminal = terminal_job_state(&slot.state);
            let incoming_terminal = terminal_job_state(&job.state);
            let different_attempt = matches!(
                (&slot.active_attempt_id, &job.active_attempt_id),
                (Some(old), Some(new)) if old != new
            );
            let stale_attempt = different_attempt && job.updated_at < slot.updated_at;
            let newer_attempt = different_attempt && !stale_attempt;
            let stale_lifecycle_hint =
                !newer_attempt && job_state_rank(&job.state) < job_state_rank(&slot.state);
            // A late terminal event from an earlier attempt must not finish a
            // retry that is already running. Likewise, a delayed create/start
            // hint cannot resurrect a terminal job unless it names a new
            // attempt (the retry transition).
            if stale_attempt || (newer_attempt && incoming_terminal && !prior_terminal) {
                return;
            }
            if prior_terminal && !incoming_terminal && !newer_attempt {
                return;
            }
            let mut merged = job;
            if stale_lifecycle_hint {
                merged.state.clone_from(&slot.state);
                merged.active_attempt_id.clone_from(&slot.active_attempt_id);
            }
            if merged.workspace_id.is_empty() {
                merged.workspace_id.clone_from(&slot.workspace_id);
            }
            if merged.kind.is_empty() {
                merged.kind.clone_from(&slot.kind);
            }
            if merged.summary.is_empty() {
                merged.summary.clone_from(&slot.summary);
            }
            if merged.session_id.is_none() {
                merged.session_id.clone_from(&slot.session_id);
            }
            if merged.turn_id.is_none() {
                merged.turn_id.clone_from(&slot.turn_id);
            }
            if merged.active_attempt_id.is_none() {
                merged.active_attempt_id.clone_from(&slot.active_attempt_id);
            }
            if merged.error_class.is_none() {
                merged.error_class.clone_from(&slot.error_class);
            }
            *slot = merged;
            return;
        }
        if self.jobs.len() >= MAX_PROJECTION_JOBS {
            self.jobs.pop_front();
        }
        self.jobs.push_back(job);
    }

    /// Upsert a tool program summary, dropping the oldest if the cap
    /// is reached.
    pub fn upsert_tool_program(&mut self, program: ToolProgramSummary) {
        if let Some(slot) = self
            .tool_programs
            .iter_mut()
            .find(|p| p.program_id == program.program_id)
        {
            *slot = program;
            return;
        }
        if self.tool_programs.len() >= MAX_PROJECTION_TOOL_PROGRAMS {
            self.tool_programs.pop_front();
        }
        self.tool_programs.push_back(program);
    }

    pub fn upsert_agent_run(&mut self, mut run: AgentRunSummary) {
        run.normalise();
        if let Some(slot) = self.agent_runs.iter_mut().find(|r| r.run_id == run.run_id) {
            *slot = run;
            return;
        }
        if self.agent_runs.len() >= MAX_PROJECTION_AGENT_RUNS {
            self.agent_runs.pop_front();
        }
        self.agent_runs.push_back(run);
    }

    pub fn upsert_worktree(&mut self, mut worktree: WorktreeSummaryProjection) {
        worktree.normalise();
        if let Some(slot) = self
            .worktrees
            .iter_mut()
            .find(|w| w.worktree_id == worktree.worktree_id)
        {
            *slot = worktree;
            return;
        }
        if self.worktrees.len() >= MAX_PROJECTION_WORKTREES {
            self.worktrees.pop_front();
        }
        self.worktrees.push_back(worktree);
    }

    pub fn upsert_run_group(&mut self, mut group: AgentRunGroupSummaryProjection) {
        group.normalise();
        if let Some(slot) = self
            .run_groups
            .iter_mut()
            .find(|g| g.group_id == group.group_id)
        {
            if group.workstream_claims_updated_at.is_none() {
                group.workstream_claims = std::mem::take(&mut slot.workstream_claims);
                group.workstream_dependencies = std::mem::take(&mut slot.workstream_dependencies);
                group.workstream_claims_updated_at = slot.workstream_claims_updated_at;
            }
            *slot = group;
            return;
        }
        if self.run_groups.len() >= MAX_PROJECTION_RUN_GROUPS {
            self.run_groups.pop_front();
        }
        self.run_groups.push_back(group);
    }

    pub fn upsert_convergence(&mut self, mut convergence: ConvergenceSummaryProjection) {
        convergence.normalise();
        if let Some(slot) = self
            .convergences
            .iter_mut()
            .find(|item| item.convergence_id == convergence.convergence_id)
        {
            *slot = convergence;
            return;
        }
        if self.convergences.len() >= crate::projection::limits::MAX_PROJECTION_CONVERGENCES {
            self.convergences.pop_front();
        }
        self.convergences.push_back(convergence);
    }
}

fn terminal_job_state(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "completed"
            | "succeeded"
            | "failed"
            | "cancelled"
            | "timed_out"
            | "interrupted"
            | "expired"
    )
}

fn job_state_rank(state: &str) -> u8 {
    match state.trim().to_ascii_lowercase().as_str() {
        "created" | "scheduled" => 0,
        "queued" | "blocked" => 1,
        "started" | "running" => 2,
        "completed" | "succeeded" | "failed" | "cancelled" | "timed_out" | "interrupted"
        | "expired" => 3,
        _ => 1,
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
    fn job_upsert_preserves_lifecycle_order_and_retry_attempts() {
        let mut snapshot = SessionProjectionSnapshot::empty("s", "p", "w");
        let job = |state: &str, attempt: Option<&str>| JobProjection {
            job_id: "job-1".into(),
            workspace_id: "w".into(),
            kind: "test".into(),
            state: state.into(),
            summary: String::new(),
            session_id: Some("s".into()),
            turn_id: None,
            active_attempt_id: attempt.map(str::to_string),
            error_class: None,
            updated_at: 1,
        };
        snapshot.upsert_job(job("running", Some("attempt-1")));
        snapshot.upsert_job(job("created", None));
        assert_eq!(snapshot.jobs[0].state, "running");
        snapshot.upsert_job(job("completed", Some("attempt-1")));
        snapshot.upsert_job(job("running", Some("attempt-1")));
        assert_eq!(snapshot.jobs[0].state, "completed");
        snapshot.upsert_job(job("queued", Some("attempt-2")));
        assert_eq!(snapshot.jobs[0].state, "queued");
        snapshot.upsert_job(job("failed", Some("attempt-1")));
        assert_eq!(snapshot.jobs[0].state, "queued");
        assert_eq!(
            snapshot.jobs[0].active_attempt_id.as_deref(),
            Some("attempt-2")
        );
    }

    #[test]
    fn push_diagnostic_caps_at_max() {
        let mut snap = SessionProjectionSnapshot::empty("s", "p", "w");
        for i in 0..(MAX_PROJECTION_DIAGNOSTICS * 2) {
            snap.push_diagnostic(ProjectionDiagnostic::new("c", format!("{i}"), i as i64));
        }
        assert_eq!(snap.diagnostics.len(), MAX_PROJECTION_DIAGNOSTICS);
    }

    #[test]
    fn group_lifecycle_refresh_preserves_board_until_complete_claim_refresh() {
        let claim = crate::projection::workstream::WorkstreamClaimProjection {
            claim_id: "claim-1".into(),
            project_id: "p".into(),
            workspace_id: "w".into(),
            group_id: "g".into(),
            kind: "path".into(),
            target: "src/lib.rs".into(),
            state: "claimed".into(),
            owner_kind: "run".into(),
            owner_id: "run-1".into(),
            version: 1,
            lease_generation: 1,
            lease_expires_at: None,
            updated_at: 10,
        };
        let mut snapshot = SessionProjectionSnapshot::empty("s", "p", "w");
        let mut board = AgentRunGroupSummaryProjection {
            group_id: "g".into(),
            workstream_claims: vec![claim],
            workstream_claims_updated_at: Some(10),
            ..Default::default()
        };
        snapshot.upsert_run_group(board.clone());

        board.status = "completed".into();
        board.workstream_claims.clear();
        board.workstream_claims_updated_at = None;
        snapshot.upsert_run_group(board.clone());
        assert_eq!(snapshot.run_groups[0].workstream_claims.len(), 1);

        board.workstream_claims_updated_at = Some(11);
        snapshot.upsert_run_group(board);
        assert!(snapshot.run_groups[0].workstream_claims.is_empty());
    }
}
