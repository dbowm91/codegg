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
pub const MAX_WORKSTREAM_CLAIMS: usize = 128;
pub const MAX_WORKSTREAM_DEPENDENCIES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWorkstreamSnapshot {
    pub project_id: String,
    pub workspace_id: String,
    pub source_session_id: String,
    /// Cursor from the authorized session projection stream.
    pub revision: u64,
    pub group: AgentRunGroupSummaryProjection,
    pub members: Vec<AgentWorkstreamMember>,
    #[serde(default)]
    pub claims: Vec<WorkstreamClaimProjection>,
    #[serde(default)]
    pub dependencies: Vec<WorkstreamDependencyProjection>,
    #[serde(default)]
    pub unavailable_claim_ids: Vec<String>,
    /// Member IDs whose authoritative run facts were absent or outside this
    /// snapshot's project/workspace. Consumers must show an incomplete view.
    pub unavailable_member_run_ids: Vec<String>,
    pub truncated: bool,
    #[serde(default)]
    pub claims_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkstreamClaimProjection {
    pub claim_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub group_id: String,
    pub kind: String,
    pub target: String,
    pub state: String,
    pub owner_kind: String,
    pub owner_id: String,
    pub version: u64,
    pub lease_generation: u64,
    pub lease_expires_at: Option<i64>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkstreamDependencyProjection {
    pub claim_id: String,
    pub prerequisite_kind: String,
    pub prerequisite_id: String,
    pub created_at: i64,
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
    project_workstream_with_claims(snapshot, group_id, &[], &[])
}

/// Extend one authorized session snapshot with claim data read through the
/// same group/project authorization boundary. Foreign or mismatched records
/// are omitted and reported as unavailable; claim prose is never inferred.
pub fn project_workstream_with_claims(
    snapshot: &SessionProjectionSnapshot,
    group_id: &str,
    claims: &[WorkstreamClaimProjection],
    dependencies: &[WorkstreamDependencyProjection],
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

    let mut projected_claims = Vec::new();
    let mut unavailable_claim_ids = Vec::new();
    let claims_truncated =
        claims.len() > MAX_WORKSTREAM_CLAIMS || dependencies.len() > MAX_WORKSTREAM_DEPENDENCIES;
    for claim in claims.iter().take(MAX_WORKSTREAM_CLAIMS) {
        if claim.claim_id.is_empty()
            || claim.project_id != snapshot.project_id
            || claim.workspace_id != snapshot.workspace_id
            || claim.group_id != group_id
            || claim.target.len() > 256
            || claim.owner_id.is_empty()
            || claim.kind.len() > 32
            || claim.state.len() > 32
        {
            unavailable_claim_ids.push(bounded(&claim.claim_id));
            continue;
        }
        projected_claims.push(WorkstreamClaimProjection {
            claim_id: bounded(&claim.claim_id),
            project_id: snapshot.project_id.clone(),
            workspace_id: snapshot.workspace_id.clone(),
            group_id: group_id.to_owned(),
            kind: bounded(&claim.kind),
            target: truncate_str(&claim.target, 256).into_owned(),
            state: bounded(&claim.state),
            owner_kind: bounded(&claim.owner_kind),
            owner_id: bounded(&claim.owner_id),
            version: claim.version,
            lease_generation: claim.lease_generation,
            lease_expires_at: claim.lease_expires_at,
            updated_at: claim.updated_at,
        });
    }
    let projected_claim_ids: std::collections::HashSet<_> = projected_claims
        .iter()
        .map(|claim| claim.claim_id.as_str())
        .collect();
    let projected_dependencies = dependencies
        .iter()
        .take(MAX_WORKSTREAM_DEPENDENCIES)
        .filter(|edge| {
            projected_claim_ids.contains(edge.claim_id.as_str())
                && !edge.prerequisite_id.is_empty()
                && edge.prerequisite_id.len() <= 128
        })
        .map(|edge| WorkstreamDependencyProjection {
            claim_id: bounded(&edge.claim_id),
            prerequisite_kind: bounded(&edge.prerequisite_kind),
            prerequisite_id: bounded(&edge.prerequisite_id),
            created_at: edge.created_at,
        })
        .collect();

    Ok(AgentWorkstreamSnapshot {
        project_id: snapshot.project_id.clone(),
        workspace_id: snapshot.workspace_id.clone(),
        source_session_id: snapshot.primary_session_id.clone(),
        revision: snapshot.event_seq,
        group,
        members,
        claims: projected_claims,
        dependencies: projected_dependencies,
        unavailable_claim_ids,
        unavailable_member_run_ids: unavailable,
        truncated,
        claims_truncated,
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
    fn workstream_projection_filters_claim_scope_and_bounds_dependency_edges() {
        let claims = [
            WorkstreamClaimProjection {
                claim_id: "claim-a".into(),
                project_id: "project-a".into(),
                workspace_id: "ws-a".into(),
                group_id: "group-a".into(),
                kind: "path".into(),
                target: "src/lib.rs".into(),
                state: "claimed".into(),
                owner_kind: "run".into(),
                owner_id: "run-a".into(),
                version: 2,
                lease_generation: 1,
                lease_expires_at: None,
                updated_at: 4,
            },
            WorkstreamClaimProjection {
                claim_id: "claim-foreign".into(),
                project_id: "project-b".into(),
                workspace_id: "ws-a".into(),
                group_id: "group-a".into(),
                kind: "path".into(),
                target: "secret.rs".into(),
                state: "claimed".into(),
                owner_kind: "run".into(),
                owner_id: "run-b".into(),
                version: 1,
                lease_generation: 1,
                lease_expires_at: None,
                updated_at: 4,
            },
        ];
        let dependencies = [WorkstreamDependencyProjection {
            claim_id: "claim-a".into(),
            prerequisite_kind: "run".into(),
            prerequisite_id: "run-b".into(),
            created_at: 4,
        }];
        let view =
            project_workstream_with_claims(&fixture(), "group-a", &claims, &dependencies).unwrap();
        assert_eq!(view.claims.len(), 1);
        assert_eq!(view.unavailable_claim_ids, ["claim-foreign"]);
        assert_eq!(view.dependencies.len(), 1);
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
