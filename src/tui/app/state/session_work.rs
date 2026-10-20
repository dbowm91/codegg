//! Canonical bounded work association for one authorized session projection.
//!
//! A child may also have a durable AgentRun and scheduler Job, and a tool
//! program has a Job. This selector folds those records by real IDs so UI
//! surfaces do not count one execution multiple times.

use crate::protocol::projection::snapshot::SessionProjectionSnapshot;
use crate::tui::app::state::subagents::SubagentRow;
use crate::tui::app::state::tasks::{is_live_state, JobLike};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionWorkKind {
    Child,
    AgentRun,
    Job,
    ToolProgram,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWorkRow {
    /// Stable logical identity used for selection and focus retention.
    pub key: String,
    pub kind: SessionWorkKind,
    pub session_id: String,
    pub child_instance_id: Option<String>,
    pub run_id: Option<String>,
    pub job_id: Option<String>,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub subagent: Option<SubagentRow>,
    pub job: Option<JobLike>,
}

impl SessionWorkRow {
    pub fn is_live(&self) -> bool {
        is_live_state(&self.status)
    }
}

/// Derive one canonical set of session work rows. Durable run status wins
/// over a related child hint; tool-program progress enriches its job; a job
/// already attached to a run/child is not emitted a second time.
pub fn select_session_work(snapshot: &SessionProjectionSnapshot) -> Vec<SessionWorkRow> {
    let session_id = snapshot.primary_session_id.clone();
    let mut rows = Vec::<SessionWorkRow>::new();
    let mut source_jobs = HashSet::<String>::new();
    let mut source_runs = HashSet::<String>::new();

    let mut child_nodes = Vec::new();
    let mut seen_child_nodes = HashSet::new();
    for node in &snapshot.subagents {
        let key = if node.child_instance_id.is_empty() {
            format!("legacy:{}", node.task_id)
        } else {
            format!("child:{}", node.child_instance_id)
        };
        if seen_child_nodes.insert(key) {
            child_nodes.push(node);
        }
    }
    for turn in snapshot
        .active_turn
        .iter()
        .chain(snapshot.recent_turns.iter().take(1))
    {
        for node in &turn.agent_tree {
            let key = if node.child_instance_id.is_empty() {
                format!("legacy:{}:{}", turn.turn_id, node.task_id)
            } else {
                format!("child:{}", node.child_instance_id)
            };
            if seen_child_nodes.insert(key) {
                child_nodes.push(node);
            }
        }
    }
    for node in child_nodes {
        let child_instance_id =
            (!node.child_instance_id.is_empty()).then(|| node.child_instance_id.clone());
        let key = child_instance_id
            .as_ref()
            .map(|id| format!("child:{id}"))
            .unwrap_or_else(|| format!("task:{}", node.task_id));
        let status = match node.status {
            crate::protocol::projection::dto::AgentTreeStatus::Running => "running",
            crate::protocol::projection::dto::AgentTreeStatus::Completed => "completed",
            crate::protocol::projection::dto::AgentTreeStatus::Failed => "failed",
        }
        .to_string();
        let row = SubagentRow {
            child_instance_id: child_instance_id.clone(),
            task_id: node.task_id.to_string(),
            run_id: node.run_id.clone(),
            display_name: node.display_name.clone(),
            profile: node.profile.clone(),
            usefulness: node.usefulness.clone(),
            agent: node.agent.clone(),
            description: node.description.clone(),
            status: status.clone(),
        };
        if let Some(run_id) = node.run_id.as_ref() {
            source_runs.insert(run_id.clone());
        }
        rows.push(SessionWorkRow {
            key,
            kind: SessionWorkKind::Child,
            session_id: session_id.clone(),
            child_instance_id,
            run_id: node.run_id.clone(),
            job_id: None,
            status,
            title: row.label().to_string(),
            summary: row.topic().to_string(),
            subagent: Some(row),
            job: None,
        });
    }

    for run in &snapshot.agent_runs {
        if let Some(job_id) = run.job_id.as_ref() {
            source_jobs.insert(job_id.clone());
        }
        if let Some(existing) = rows
            .iter_mut()
            .find(|row| row.run_id.as_deref() == Some(run.run_id.as_str()))
        {
            existing.status = run.status.clone();
            existing.job_id = run.job_id.clone();
            existing.summary = run
                .progress
                .clone()
                .or_else(|| run.terminal_summary.clone())
                .unwrap_or_else(|| existing.summary.clone());
            if let Some(child) = existing.subagent.as_mut() {
                child.status.clone_from(&run.status);
                if let Some(progress) = run.progress.as_ref() {
                    child.description.clone_from(progress);
                }
            }
            continue;
        }
        if !source_runs.insert(run.run_id.clone()) {
            continue;
        }
        let summary = run
            .progress
            .clone()
            .or_else(|| run.terminal_summary.clone())
            .unwrap_or_else(|| format!("{} run", run.status));
        let child = SubagentRow {
            child_instance_id: None,
            task_id: run.run_id.clone(),
            run_id: Some(run.run_id.clone()),
            display_name: None,
            profile: None,
            usefulness: None,
            agent: run.agent.clone(),
            description: summary.clone(),
            status: run.status.clone(),
        };
        rows.push(SessionWorkRow {
            key: format!("run:{}", run.run_id),
            kind: SessionWorkKind::AgentRun,
            session_id: session_id.clone(),
            child_instance_id: None,
            run_id: Some(run.run_id.clone()),
            job_id: run.job_id.clone(),
            status: run.status.clone(),
            title: run.agent.clone(),
            summary,
            subagent: Some(child),
            job: None,
        });
    }

    for job in &snapshot.jobs {
        if source_jobs.contains(&job.job_id) {
            continue;
        }
        let program = snapshot
            .tool_programs
            .iter()
            .find(|program| program.job_id == job.job_id);
        let (kind, status, summary, error_class, key_kind) = if let Some(program) = program {
            (
                format!("{} tool program", program.language),
                program.state.clone(),
                program
                    .last_progress
                    .clone()
                    .or_else(|| program.phase.clone())
                    .unwrap_or_else(|| job.summary.clone()),
                program
                    .failure_class
                    .clone()
                    .or_else(|| job.error_class.clone()),
                SessionWorkKind::ToolProgram,
            )
        } else {
            (
                job.kind.clone(),
                job.state.clone(),
                job.summary.clone(),
                job.error_class.clone(),
                SessionWorkKind::Job,
            )
        };
        rows.push(job_row(
            &session_id,
            &job.job_id,
            key_kind,
            kind,
            status,
            summary,
            error_class,
        ));
    }
    for program in &snapshot.tool_programs {
        if source_jobs.contains(&program.job_id)
            || snapshot.jobs.iter().any(|job| job.job_id == program.job_id)
        {
            continue;
        }
        rows.push(job_row(
            &session_id,
            &program.job_id,
            SessionWorkKind::ToolProgram,
            format!("{} tool program", program.language),
            program.state.clone(),
            program
                .last_progress
                .clone()
                .or_else(|| program.phase.clone())
                .unwrap_or_else(|| "tool program running".into()),
            program.failure_class.clone(),
        ));
    }
    rows
}

fn job_row(
    session_id: &str,
    job_id: &str,
    kind: SessionWorkKind,
    title: String,
    status: String,
    summary: String,
    error_class: Option<String>,
) -> SessionWorkRow {
    SessionWorkRow {
        key: format!("job:{job_id}"),
        kind,
        session_id: session_id.to_string(),
        child_instance_id: None,
        run_id: None,
        job_id: Some(job_id.to_string()),
        status: status.clone(),
        title: title.clone(),
        summary: summary.clone(),
        subagent: None,
        job: Some(JobLike {
            job_id: job_id.to_string(),
            kind: title,
            summary,
            state: status,
            error_class,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::projection::dto::{
        AgentRunSummary, AgentTreeNodeProjection, AgentTreeStatus, JobProjection,
        ToolProgramSummary,
    };
    use crate::protocol::projection::fixtures::idle_snapshot;

    #[test]
    fn related_child_run_job_is_one_row_and_tool_program_enriches_its_job() {
        let mut snapshot = idle_snapshot();
        snapshot.subagents.push_back(AgentTreeNodeProjection {
            child_instance_id: "child-1".into(),
            originating_turn_id: Some("turn-1".into()),
            task_id: 7,
            agent: "explore".into(),
            description: "child progress".into(),
            status: AgentTreeStatus::Running,
            parent_task_id: None,
            created_at: 1,
            completed_at: None,
            result_summary: None,
            display_name: Some("Sally".into()),
            profile: None,
            usefulness: None,
            run_id: Some("run-1".into()),
        });
        snapshot.agent_runs.push_back(AgentRunSummary {
            run_id: "run-1".into(),
            task_id: "task-uuid".into(),
            project_id: "project-fixture".into(),
            repository_id: None,
            workspace_id: "workspace-fixture".into(),
            job_id: Some("job-child".into()),
            parent_run_id: None,
            agent: "explore".into(),
            status: "running".into(),
            depth: 1,
            worktree_id: None,
            branch: None,
            base_commit: None,
            result_commit: None,
            validation_summary: None,
            group_id: None,
            attention_required: false,
            terminal_summary: None,
            control_status: "available".into(),
            progress: Some("run progress".into()),
            failure_class: None,
            updated_at: 2,
        });
        snapshot.jobs.push_back(JobProjection {
            job_id: "job-child".into(),
            workspace_id: "workspace-fixture".into(),
            kind: "agent".into(),
            state: "running".into(),
            summary: "child job".into(),
            session_id: Some("session-fixture".into()),
            turn_id: Some("turn-1".into()),
            active_attempt_id: None,
            error_class: None,
            updated_at: 2,
        });
        snapshot.jobs.push_back(JobProjection {
            job_id: "job-program".into(),
            workspace_id: "workspace-fixture".into(),
            kind: "tool_program".into(),
            state: "running".into(),
            summary: "generic job summary".into(),
            session_id: Some("session-fixture".into()),
            turn_id: Some("turn-1".into()),
            active_attempt_id: None,
            error_class: None,
            updated_at: 3,
        });
        snapshot.tool_programs.push_back(ToolProgramSummary {
            program_id: "program-1".into(),
            job_id: "job-program".into(),
            state: "running".into(),
            phase: Some("compiling".into()),
            language: "rust".into(),
            parent_turn_id: Some("turn-1".into()),
            parent_agent_id: None,
            calls_completed: 1,
            child_jobs_running: 0,
            submitted_at: 1,
            started_at: Some(2),
            completed_at: None,
            failure_class: None,
            terminal_handle: None,
            last_progress: Some("crate foo".into()),
        });
        let rows = select_session_work(&snapshot);
        assert_eq!(
            rows.len(),
            2,
            "run and child merge; tool program and job merge"
        );
        let child = rows
            .iter()
            .find(|row| row.child_instance_id.as_deref() == Some("child-1"))
            .unwrap();
        assert_eq!(child.status, "running");
        assert_eq!(child.run_id.as_deref(), Some("run-1"));
        assert_eq!(child.job_id.as_deref(), Some("job-child"));
        assert!(child.job.is_none());
        let program = rows
            .iter()
            .find(|row| row.job_id.as_deref() == Some("job-program"))
            .unwrap();
        assert_eq!(program.kind, SessionWorkKind::ToolProgram);
        assert_eq!(program.summary, "crate foo");
        assert_eq!(program.job.as_ref().unwrap().kind, "rust tool program");
    }

    #[test]
    fn session_owned_children_do_not_hide_legacy_turn_local_children() {
        let mut snapshot = idle_snapshot();
        snapshot.subagents.push_back(AgentTreeNodeProjection {
            child_instance_id: "child-current".into(),
            originating_turn_id: None,
            task_id: 7,
            agent: "explore".into(),
            description: "new lifecycle".into(),
            status: AgentTreeStatus::Running,
            parent_task_id: None,
            created_at: 1,
            completed_at: None,
            result_summary: None,
            display_name: None,
            profile: None,
            usefulness: None,
            run_id: None,
        });
        let mut turn = crate::protocol::projection::fixtures::active_turn_snapshot()
            .active_turn
            .unwrap();
        turn.agent_tree.push_back(AgentTreeNodeProjection {
            child_instance_id: String::new(),
            originating_turn_id: None,
            task_id: 7,
            agent: "legacy".into(),
            description: "turn-local compatibility row".into(),
            status: AgentTreeStatus::Running,
            parent_task_id: None,
            created_at: 1,
            completed_at: None,
            result_summary: None,
            display_name: None,
            profile: None,
            usefulness: None,
            run_id: None,
        });
        snapshot.active_turn = Some(turn);

        let rows = select_session_work(&snapshot);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.key == "child:child-current"));
        assert!(rows.iter().any(|row| row
            .subagent
            .as_ref()
            .is_some_and(|child| { child.agent == "legacy" })));
    }
}
