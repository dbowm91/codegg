//! Application-layer assembly of WorkPlan host evidence (long-horizon M003).
//!
//! Reads durable owners and converts their records into the narrow,
//! store-independent snapshot consumed by `codegg_core::work_plan`'s pure
//! assessment. It does not execute tools or infer provenance from model prose.
//! A ref whose target cannot be found is `Unavailable` — never satisfied.

use codegg_core::jobs::{JobId, JobState, JobStore, SqliteJobStore};
use codegg_core::work_plan::{
    HostEvidenceStatus, WorkEvidenceKind, WorkItem, WorkPlanEvidenceSnapshot,
};
use sqlx::SqlitePool;

fn job_state_to_evidence(state: JobState) -> HostEvidenceStatus {
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

async fn job_evidence_status(pool: &SqlitePool, ref_id: &str) -> HostEvidenceStatus {
    let job_id = JobId::new_unchecked(ref_id.to_string());
    let store = SqliteJobStore::new(pool.clone());
    match store.get_job(&job_id).await {
        Ok(Some(record)) => job_state_to_evidence(record.state),
        Ok(None) => HostEvidenceStatus::Unavailable,
        Err(_) => HostEvidenceStatus::Unavailable,
    }
}

async fn agent_run_evidence_status(pool: &SqlitePool, ref_id: &str) -> HostEvidenceStatus {
    #[derive(sqlx::FromRow)]
    struct StatusRow {
        status: String,
    }
    let row = sqlx::query_as::<_, StatusRow>("SELECT status FROM agent_run WHERE id = ?1")
        .bind(ref_id)
        .fetch_optional(pool)
        .await;
    match row {
        Ok(Some(row)) => match row.status.as_str() {
            "completed" => HostEvidenceStatus::Passed,
            "failed" | "cancelled" | "timed_out" | "interrupted" | "expired" => {
                HostEvidenceStatus::Failed
            }
            "running" | "scheduled" | "queued" | "blocked" | "pending" | "in_progress" => {
                HostEvidenceStatus::InProgress
            }
            _ => HostEvidenceStatus::Unavailable,
        },
        _ => HostEvidenceStatus::Unavailable,
    }
}

/// Assemble a bounded evidence snapshot for the given items.
///
/// Test/Scheduler/Delegated refs resolve against the durable job store;
/// AgentRun refs resolve against the `agent_run` table; Artifact/Commit refs
/// are `Unavailable` (they require a host Satisfied acceptance through a
/// validated host path, never model assertion). failures to read a store
/// yield `Unavailable` for that ref, never `Passed`.
pub async fn assemble(
    pool: &SqlitePool,
    items: &[WorkItem],
) -> Result<WorkPlanEvidenceSnapshot, String> {
    let mut snapshot = WorkPlanEvidenceSnapshot::empty();
    // Bound the snapshot so a large plan cannot force unbounded store reads.
    const MAX_EVIDENCE_REFS: usize = 256;
    let mut seen = 0usize;
    for item in items {
        for evidence in &item.evidence {
            if seen >= MAX_EVIDENCE_REFS {
                break;
            }
            seen += 1;
            let status = match evidence.kind {
                WorkEvidenceKind::TestJob
                | WorkEvidenceKind::SchedulerJob
                | WorkEvidenceKind::DelegatedRun => {
                    job_evidence_status(pool, evidence.ref_id.as_str()).await
                }
                WorkEvidenceKind::AgentRun => {
                    // Prefer the agent_run table; fall back to the job store
                    // for delegated-run handles that share the id space.
                    let direct = agent_run_evidence_status(pool, evidence.ref_id.as_str()).await;
                    if direct == HostEvidenceStatus::Unavailable {
                        job_evidence_status(pool, evidence.ref_id.as_str()).await
                    } else {
                        direct
                    }
                }
                WorkEvidenceKind::Artifact | WorkEvidenceKind::Commit => {
                    HostEvidenceStatus::Unavailable
                }
            };
            snapshot.insert(evidence.kind, evidence.ref_id.as_str(), status);
        }
        if seen >= MAX_EVIDENCE_REFS {
            break;
        }
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_state_mapping_is_fail_closed() {
        assert_eq!(
            job_state_to_evidence(JobState::Completed),
            HostEvidenceStatus::Passed
        );
        assert_eq!(
            job_state_to_evidence(JobState::Failed),
            HostEvidenceStatus::Failed
        );
        assert_eq!(
            job_state_to_evidence(JobState::Running),
            HostEvidenceStatus::InProgress
        );
    }
}
