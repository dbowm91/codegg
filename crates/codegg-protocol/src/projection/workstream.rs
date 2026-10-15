//! Pure bounded workstream view derived from one authorized session snapshot.
//!
//! The session projection stream remains the durable revision/replay authority;
//! this module joins its canonical run, group, job, and worktree facts without
//! introducing another store or execution authority.

use serde::{Deserialize, Serialize};

use super::dto::{AgentRunGroupSummaryProjection, AgentRunSummary, WorktreeSummaryProjection};
use super::limits::{truncate_str, MAX_PROJECTION_STRING_BYTES};
use super::snapshot::SessionProjectionSnapshot;

pub const MAX_WORKSTREAM_MEMBERS: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWorkstreamSnapshot {
    pub project_id: String,
    pub workspace_id: String,
    pub source_session_id: String,
    /// Cursor from the authorized session projection stream.
    pub revision: u64,
    pub group: AgentRunGroupSummaryProjection,
    pub members: Vec<AgentWorkstreamMember>,
    /// Member IDs whose authoritative run facts were absent or outside this
    /// snapshot's project/workspace. Consumers must show an incomplete view.
    pub unavailable_member_run_ids: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWorkstreamMember {
    pub run_id: String,
    pub task_id: String,
    pub agent: String,
    pub status: String,
    pub depth: u32,
    pub worktree: WorkstreamWorktreeFact,
    pub job: WorkstreamJobFact,
    pub result_commit: Option<String>,
    pub validation_summary: Option<String>,
    pub failure_class: Option<String>,
    pub attention_required: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum WorkstreamJobFact {
    NotLinked,
    Observed { job_id: String, status: String },
    Unavailable { job_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum WorkstreamWorktreeFact {
    NotLinked,
    Observed { worktree: WorktreeSummaryProjection },
    Unavailable { worktree_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkstreamProjectionError {
    #[error("run group is not present in this authorized session snapshot")]
    GroupNotFound,
}

/// Join the requested group to bounded canonical facts already in a session
/// projection snapshot. It never reads storage, infers completion, or exposes
/// prompts, full task descriptions, paths, logs, or natural-language results.
pub fn project_workstream(
    snapshot: &SessionProjectionSnapshot,
    group_id: &str,
) -> Result<AgentWorkstreamSnapshot, WorkstreamProjectionError> {
    let mut group = snapshot
        .run_groups
        .iter()
        .find(|group| group.group_id == group_id)
        .cloned()
        .ok_or(WorkstreamProjectionError::GroupNotFound)?;

    let truncated = group.member_run_ids.len() > MAX_WORKSTREAM_MEMBERS;
    group.member_run_ids.truncate(MAX_WORKSTREAM_MEMBERS);
    group.normalise();
    let mut members = Vec::with_capacity(group.member_run_ids.len().min(MAX_WORKSTREAM_MEMBERS));
    let mut unavailable = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for run_id in group.member_run_ids.iter().take(MAX_WORKSTREAM_MEMBERS) {
        if !seen.insert(run_id) {
            unavailable.push(run_id.clone());
            continue;
        }
        let Some(run) = snapshot.agent_runs.iter().find(|run| run.run_id == *run_id) else {
            unavailable.push(run_id.clone());
            continue;
        };
        if (!run.project_id.is_empty() && run.project_id != snapshot.project_id)
            || (!run.workspace_id.is_empty() && run.workspace_id != snapshot.workspace_id)
        {
            unavailable.push(run_id.clone());
            continue;
        }
        members.push(project_member(snapshot, run));
    }

    Ok(AgentWorkstreamSnapshot {
        project_id: snapshot.project_id.clone(),
        workspace_id: snapshot.workspace_id.clone(),
        source_session_id: snapshot.primary_session_id.clone(),
        revision: snapshot.event_seq,
        group,
        members,
        unavailable_member_run_ids: unavailable,
        truncated,
    })
}

fn project_member(
    snapshot: &SessionProjectionSnapshot,
    run: &AgentRunSummary,
) -> AgentWorkstreamMember {
    let worktree = match run.worktree_id.as_deref() {
        None => WorkstreamWorktreeFact::NotLinked,
        Some(worktree_id) => match snapshot.worktrees.iter().find(|worktree| {
            worktree.worktree_id == worktree_id
                && worktree
                    .owner_run_id
                    .as_deref()
                    .is_some_and(|owner| owner == run.run_id)
        }) {
            Some(worktree) => {
                let mut worktree = worktree.clone();
                worktree.normalise();
                WorkstreamWorktreeFact::Observed { worktree }
            }
            None => WorkstreamWorktreeFact::Unavailable {
                worktree_id: bounded(worktree_id),
            },
        },
    };
    let job = match run.job_id.as_deref() {
        None => WorkstreamJobFact::NotLinked,
        Some(job_id) => match snapshot
            .jobs
            .iter()
            .find(|job| job.job_id == job_id && job.workspace_id == snapshot.workspace_id)
        {
            Some(job) => WorkstreamJobFact::Observed {
                job_id: bounded(job_id),
                status: bounded(&job.state),
            },
            None => WorkstreamJobFact::Unavailable {
                job_id: bounded(job_id),
            },
        },
    };
    AgentWorkstreamMember {
        run_id: bounded(&run.run_id),
        task_id: bounded(&run.task_id),
        agent: bounded(&run.agent),
        status: bounded(&run.status),
        depth: run.depth,
        worktree,
        job,
        result_commit: run.result_commit.as_deref().map(bounded),
        validation_summary: run.validation_summary.as_deref().map(bounded),
        failure_class: run.failure_class.as_deref().map(bounded),
        attention_required: run.attention_required,
        updated_at: run.updated_at,
    }
}

fn bounded(value: &str) -> String {
    truncate_str(value, MAX_PROJECTION_STRING_BYTES).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::dto::{AgentRunGroupSummaryProjection, JobProjection};

    fn fixture() -> SessionProjectionSnapshot {
        let mut snapshot = SessionProjectionSnapshot::empty("session-a", "project-a", "ws-a");
        snapshot.event_seq = 42;
        snapshot
            .run_groups
            .push_back(AgentRunGroupSummaryProjection {
                group_id: "group-a".into(),
                owner_run_id: "owner-a".into(),
                owner_kind: "turn".into(),
                owner_session_id: Some("session-a".into()),
                status: "running".into(),
                member_run_ids: vec!["run-a".into(), "run-b".into()],
                ..AgentRunGroupSummaryProjection::default()
            });
        snapshot.agent_runs.push_back(AgentRunSummary {
            run_id: "run-a".into(),
            task_id: "task-a".into(),
            project_id: "project-a".into(),
            workspace_id: "ws-a".into(),
            job_id: Some("job-a".into()),
            worktree_id: Some("worktree-a".into()),
            agent: "worker".into(),
            status: "running".into(),
            ..AgentRunSummary::default()
        });
        snapshot.worktrees.push_back(WorktreeSummaryProjection {
            worktree_id: "worktree-a".into(),
            owner_run_id: Some("run-a".into()),
            branch: Some("work/worker".into()),
            base_commit: Some("base".into()),
            state: "ready".into(),
            health: "clean".into(),
            dirty: false,
            conflicted: false,
            retained_for_attention: false,
            updated_at: 3,
        });
        snapshot.jobs.push_back(JobProjection {
            job_id: "job-a".into(),
            workspace_id: "ws-a".into(),
            kind: "agent_turn".into(),
            state: "running".into(),
            summary: "private job output omitted".into(),
            session_id: Some("session-a".into()),
            turn_id: None,
            active_attempt_id: None,
            error_class: None,
            updated_at: 3,
        });
        snapshot
    }

    #[test]
    fn joins_bounded_facts_and_preserves_the_session_cursor() {
        let snapshot = fixture();
        let view = project_workstream(&snapshot, "group-a").unwrap();
        assert_eq!(view.revision, 42);
        assert_eq!(view.members.len(), 1);
        assert_eq!(view.unavailable_member_run_ids, ["run-b"]);
        assert_eq!(
            view.members[0].job,
            WorkstreamJobFact::Observed {
                job_id: "job-a".into(),
                status: "running".into()
            }
        );
        assert!(matches!(
            view.members[0].worktree,
            WorkstreamWorktreeFact::Observed { .. }
        ));
        let encoded = serde_json::to_string(&view).unwrap();
        assert!(!encoded.contains("private job output omitted"));
        assert!(!encoded.contains("terminal_summary"));
    }

    #[test]
    fn linked_but_missing_worktree_is_explicitly_unavailable() {
        let mut snapshot = fixture();
        snapshot.worktrees.clear();
        let view = project_workstream(&snapshot, "group-a").unwrap();
        assert_eq!(
            view.members[0].worktree,
            WorkstreamWorktreeFact::Unavailable {
                worktree_id: "worktree-a".into()
            }
        );
    }

    #[test]
    fn cross_workspace_job_is_not_disclosed() {
        let mut snapshot = fixture();
        snapshot.jobs[0].workspace_id = "workspace-other".into();
        let view = project_workstream(&snapshot, "group-a").unwrap();
        assert_eq!(
            view.members[0].job,
            WorkstreamJobFact::Unavailable {
                job_id: "job-a".into()
            }
        );
    }

    #[test]
    fn mismatched_project_and_workspace_facts_are_not_disclosed() {
        let mut snapshot = fixture();
        let mut other = snapshot.agent_runs[0].clone();
        other.run_id = "run-b".into();
        other.project_id = "project-b".into();
        snapshot.agent_runs.push_back(other);
        let view = project_workstream(&snapshot, "group-a").unwrap();
        assert_eq!(view.members.len(), 1);
        assert_eq!(view.unavailable_member_run_ids, ["run-b"]);
    }

    #[test]
    fn group_lookup_is_exact_and_member_bound_is_enforced() {
        let snapshot = fixture();
        assert_eq!(
            project_workstream(&snapshot, "other").unwrap_err(),
            WorkstreamProjectionError::GroupNotFound
        );
        let mut oversized = snapshot;
        oversized.run_groups[0].member_run_ids = (0..MAX_WORKSTREAM_MEMBERS + 1)
            .map(|index| format!("run-{index}"))
            .collect();
        let view = project_workstream(&oversized, "group-a").unwrap();
        assert!(view.truncated);
        assert_eq!(view.group.member_run_ids.len(), MAX_WORKSTREAM_MEMBERS);
        assert!(view.members.len() <= MAX_WORKSTREAM_MEMBERS);
        assert!(view.unavailable_member_run_ids.len() <= MAX_WORKSTREAM_MEMBERS);
    }
}
