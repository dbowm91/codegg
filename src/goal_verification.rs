//! Application-layer assembly of host-owned goal evidence.
//!
//! This module reads durable owners and converts their records into the
//! narrow, store-independent context consumed by `codegg_core::goal`'s
//! deterministic verifier. It does not execute tools or infer provenance from
//! model prose.

use chrono::{DateTime, Utc};
use codegg_core::goal::{
    GoalEvidenceContext, GoalExecutionEvidence, GoalTodoEvidence, HostEvidenceStatus,
};
use codegg_core::jobs::store::JobStoreQuery;
use codegg_core::jobs::{JobKind, JobState, JobStore, SqliteJobStore};
use sqlx::SqlitePool;

const MAX_EVIDENCE_RECORDS: u32 = 128;

pub async fn assemble(
    pool: &SqlitePool,
    session_id: &str,
    goal_created_at: DateTime<Utc>,
) -> Result<GoalEvidenceContext, String> {
    let job_store = SqliteJobStore::new(pool.clone());
    let jobs = job_store
        .list_job_records(JobStoreQuery {
            kinds: vec![JobKind::Test, JobKind::Subagent],
            session_id: Some(session_id.to_string()),
            limit: Some(MAX_EVIDENCE_RECORDS),
            ..Default::default()
        })
        .await
        .map_err(|error| error.to_string())?;

    let mut executions = Vec::with_capacity(jobs.len());
    for job in jobs {
        // Session identity is host-owned provenance. The creation boundary
        // prevents a failed job from an earlier goal from poisoning a newer
        // goal after restart while remaining reconstructable from SQLite.
        if job.created_at < goal_created_at {
            continue;
        }
        let source = match job.kind {
            JobKind::Test => "test",
            JobKind::Subagent => "delegated_run",
            _ => continue,
        };
        executions.push(GoalExecutionEvidence {
            id: job.job_id.as_str().to_string(),
            source: source.to_string(),
            status: host_status(job.state),
        });
    }

    let todos = codegg_core::session::store::TodoStore::new(pool.clone())
        .list(session_id)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .take(MAX_EVIDENCE_RECORDS as usize)
        .map(|todo| GoalTodoEvidence {
            content: todo.content.chars().take(256).collect(),
            status: todo.status.chars().take(32).collect(),
        })
        .collect();

    Ok(GoalEvidenceContext { executions, todos })
}

fn host_status(state: JobState) -> HostEvidenceStatus {
    match state {
        JobState::Completed => HostEvidenceStatus::Passed,
        JobState::Failed
        | JobState::Cancelled
        | JobState::TimedOut
        | JobState::Interrupted
        | JobState::Expired => HostEvidenceStatus::Failed,
        JobState::Scheduled | JobState::Queued | JobState::Running | JobState::Blocked => {
            HostEvidenceStatus::InProgress
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_job_states_are_fail_closed() {
        assert_eq!(host_status(JobState::Completed), HostEvidenceStatus::Passed);
        assert_eq!(host_status(JobState::Failed), HostEvidenceStatus::Failed);
        assert_eq!(
            host_status(JobState::Interrupted),
            HostEvidenceStatus::Failed
        );
        assert_eq!(
            host_status(JobState::Running),
            HostEvidenceStatus::InProgress
        );
    }
}
