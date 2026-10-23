//! Application-layer assembly of host-owned goal evidence.
//!
//! This module reads durable owners and converts their records into the
//! narrow, store-independent context consumed by `codegg_core::goal`'s
//! deterministic verifier. It does not execute tools or infer provenance from
//! model prose.

use chrono::{DateTime, Utc};
use codegg_core::goal::{GoalEvidenceContext, GoalExecutionEvidence, GoalTodoEvidence};
use codegg_core::jobs::store::JobStoreQuery;
use codegg_core::jobs::{JobKind, JobStore, SqliteJobStore, GOAL_PROVENANCE_LABEL_KEY};
use sqlx::SqlitePool;

const MAX_EVIDENCE_RECORDS: u32 = 128;

pub async fn assemble(
    pool: &SqlitePool,
    session_id: &str,
    goal_id: &str,
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
        // Session identity and creation time are defense-in-depth bounds.
        // Exact ownership comes only from the host-written goal label.
        if job.created_at < goal_created_at {
            continue;
        }
        if job
            .labels
            .get(GOAL_PROVENANCE_LABEL_KEY)
            .map(String::as_str)
            != Some(goal_id)
        {
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
            status: codegg_core::jobs::evidence::job_success_evidence(job.state),
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

#[cfg(test)]
mod tests {
    use codegg_core::goal::HostEvidenceStatus;
    use codegg_core::jobs::JobState;

    // The shared authority answers "does this job count as evidence that the
    // work SUCCEEDED?" — a different question from whether the job is
    // finished. The pinned test names that projection `host_status`, and this
    // one-line delegation keeps the test body byte-identical to the one that
    // pinned the fail-closed verdicts.
    fn host_status(state: JobState) -> HostEvidenceStatus {
        codegg_core::jobs::evidence::job_success_evidence(state)
    }

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
