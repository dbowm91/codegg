//! Application-layer assembly of WorkPlan host evidence (long-horizon M003).
//!
//! Reads durable owners and converts their records into the narrow,
//! store-independent snapshot consumed by `codegg_core::work_plan`'s pure
//! assessment. It does not execute tools or infer provenance from model prose.
//! A ref whose target cannot be found is `Unavailable` — never satisfied.

use codegg_core::jobs::{
    ExecutionSubjectDisposition, ExecutionSubjectProvenance, JobAttempt, JobId, JobState, JobStore,
    SqliteJobStore,
};
use codegg_core::work_plan::{
    HostEvidenceStatus, WorkEvidenceKind, WorkItem, WorkPlanEvidenceSnapshot,
};
use sqlx::SqlitePool;

/// Host-resolved execution evidence keeps status separate from historical source identity.
#[derive(Debug, Clone)]
pub struct ResolvedWorkEvidence {
    pub kind: WorkEvidenceKind,
    pub ref_id: String,
    pub status: HostEvidenceStatus,
    pub source_subject: Option<ExecutionSubjectProvenance>,
    pub source_subject_disposition: ExecutionSubjectDisposition,
    pub native_job_id: Option<String>,
    pub native_attempt_id: Option<String>,
    pub native_run_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedWorkEvidenceSnapshot {
    pub entries: Vec<ResolvedWorkEvidence>,
}

async fn attempt_for_job(store: &SqliteJobStore, job_id: &JobId) -> Option<JobAttempt> {
    store
        .list_attempts(job_id)
        .await
        .ok()?
        .into_iter()
        .max_by_key(|a| a.sequence)
}

/// Resolve status and attempt-scoped subject for current native records. Legacy
/// attempts are left unavailable; this function never captures the worktree.
pub async fn assemble_resolved(
    pool: &SqlitePool,
    items: &[WorkItem],
) -> Result<ResolvedWorkEvidenceSnapshot, String> {
    const MAX_EVIDENCE_REFS: usize = 256;
    let store = SqliteJobStore::new(pool.clone());
    let mut entries = Vec::new();
    for evidence in items
        .iter()
        .flat_map(|item| item.evidence.iter())
        .take(MAX_EVIDENCE_REFS)
    {
        let mut resolved = ResolvedWorkEvidence {
            kind: evidence.kind,
            ref_id: evidence.ref_id.as_str().to_owned(),
            status: HostEvidenceStatus::Unavailable,
            source_subject: None,
            source_subject_disposition: ExecutionSubjectDisposition::Unavailable,
            native_job_id: None,
            native_attempt_id: None,
            native_run_id: None,
        };
        let job_id = JobId::new_unchecked(evidence.ref_id.as_str().to_owned());
        let job = match evidence.kind {
            WorkEvidenceKind::TestJob
            | WorkEvidenceKind::SchedulerJob
            | WorkEvidenceKind::DelegatedRun => store.get_job(&job_id).await.ok().flatten(),
            _ => None,
        };
        if let Some(job) = job {
            resolved.status = job_state_to_evidence(job.state);
            resolved.native_job_id = Some(job.job_id.to_string());
            if let Some(attempt) = attempt_for_job(&store, &job.job_id).await {
                resolved.native_attempt_id = Some(attempt.attempt_id.to_string());
                resolved.native_run_id = attempt.run_id.as_ref().map(ToString::to_string);
                resolved.source_subject_disposition = attempt
                    .source_subject
                    .as_ref()
                    .map(|p| p.disposition)
                    .unwrap_or(ExecutionSubjectDisposition::Unavailable);
                resolved.source_subject = attempt.source_subject;
            }
        } else if evidence.kind == WorkEvidenceKind::AgentRun {
            #[derive(sqlx::FromRow)]
            struct Link {
                status: String,
                job_id: Option<String>,
                attempt_id: Option<String>,
            }
            if let Ok(Some(link)) = sqlx::query_as::<_, Link>(
                "SELECT status, job_id, attempt_id FROM agent_run WHERE run_id = ?1",
            )
            .bind(evidence.ref_id.as_str())
            .fetch_optional(pool)
            .await
            {
                resolved.status = match link.status.as_str() {
                    "completed" => HostEvidenceStatus::Passed,
                    "failed" | "cancelled" | "timed_out" | "interrupted" | "expired" => {
                        HostEvidenceStatus::Failed
                    }
                    "running" | "scheduled" | "queued" | "blocked" | "pending" | "in_progress" => {
                        HostEvidenceStatus::InProgress
                    }
                    _ => HostEvidenceStatus::Unavailable,
                };
                resolved.native_job_id = link.job_id.clone();
                resolved.native_attempt_id = link.attempt_id.clone();
                if let (Some(job), Some(attempt)) = (link.job_id, link.attempt_id) {
                    let attempt_id = codegg_core::jobs::AttemptId::new_unchecked(attempt);
                    let job_id = codegg_core::jobs::JobId::new_unchecked(job);
                    if let Ok(attempts) = store.list_attempts(&job_id).await {
                        if let Some(record) =
                            attempts.into_iter().find(|a| a.attempt_id == attempt_id)
                        {
                            resolved.native_run_id =
                                record.run_id.as_ref().map(ToString::to_string);
                            resolved.source_subject_disposition = record
                                .source_subject
                                .as_ref()
                                .map(|p| p.disposition)
                                .unwrap_or(ExecutionSubjectDisposition::Unavailable);
                            resolved.source_subject = record.source_subject;
                        }
                    }
                }
            }
        }
        entries.push(resolved);
    }
    Ok(ResolvedWorkEvidenceSnapshot { entries })
}

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
    let row = sqlx::query_as::<_, StatusRow>("SELECT status FROM agent_run WHERE run_id = ?1")
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
