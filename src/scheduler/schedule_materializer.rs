//! The production occurrence materializer.
//!
//! A fired schedule occurrence becomes a durable job through
//! [`JobSubmissionService`](super::submission::JobSubmissionService) — the
//! single production admission path — so a scheduled job is admitted exactly
//! like a submitted one: the workspace is validated, the canonical resource
//! profile is applied, one durable record is created, and the scheduler is
//! woken. Nothing here creates a job behind the submission facade, and nothing
//! here invokes an executor.
//!
//! # The host boundary is re-applied at materialization time
//!
//! A schedule is authorized **once**, when it is created. By the time an
//! occurrence fires, the daemon may have been restarted with a different
//! configuration, and the administrator may have withdrawn the node's
//! approval. A materializer that trusted the creation-time decision would
//! keep dispatching to a node the operator has since de-approved, so the
//! host-boundary gates — the remotely-runnable shape, the administrator's
//! approval, and the operator's resource ceiling — are applied again here
//! and fail closed.
//!
//! The *principal* gate is deliberately **not** re-applied: a timer-driven
//! occurrence has no live principal. It was applied when the schedule was
//! created, and that decision is what authorized the schedule to exist at
//! all. Re-deriving a principal for a timer would invent authority rather
//! than check it.
//!
//! # The occurrence is idempotent across a crash
//!
//! The occurrence's job id is a pure function of `(schedule_id,
//! scheduled_for)`, so the `job.id` primary key is the durable uniqueness
//! constraint. A daemon that stopped between writing the job row and
//! writing the occurrence row replays into the *same* job rather than a
//! second one.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use codegg_core::jobs::{
    occurrence_job_id, ExecutionTarget, JobId, MaterializerError, OccurrenceMaterializer,
    ScheduleRecord,
};

use super::submission::{JobSubmissionError, JobSubmissionService, SubmissionKey};

/// The daemon's production occurrence materializer.
pub struct ScheduleMaterializer {
    submission: Arc<JobSubmissionService>,
    /// The administrator's remote-target policy, resolved once at daemon
    /// construction.
    ///
    /// `None` means the daemon approves no node at all, so every remote
    /// occurrence is refused. That is the fail-closed default, not an
    /// error.
    remote_policy: Option<Arc<crate::core::remote_targets::RemoteTargetPolicy>>,
}

impl ScheduleMaterializer {
    pub fn new(
        submission: Arc<JobSubmissionService>,
        remote_policy: Option<Arc<crate::core::remote_targets::RemoteTargetPolicy>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            submission,
            remote_policy,
        })
    }

    /// Re-apply the host-boundary gates to one occurrence.
    ///
    /// A local occurrence is unaffected by every remote gate.
    fn admit(&self, schedule: &ScheduleRecord) -> Result<(), MaterializerError> {
        let template = &schedule.job_template;
        let ExecutionTarget::EggworkNode { node_id } = &template.target else {
            return Ok(());
        };

        // Gate 1 — shape. A job that could never run remotely is refused
        // rather than becoming a durable job the executor would reject.
        if let Err(rejection) =
            crate::scheduler::eggwork::validate_remote_job(template.kind, &template.payload)
        {
            return Err(MaterializerError::Refused {
                code: rejection.code().to_owned(),
            });
        }

        // Gate 2 — administrator. The daemon's approval of the node is
        // configuration, and configuration can change between the
        // schedule's creation and the occurrence's firing. The check runs
        // before the resource ceiling so a caller learns nothing about a
        // node's limits unless the node itself is approved.
        let Some(policy) = self.remote_policy.as_deref() else {
            return Err(MaterializerError::Refused {
                code: "remote_target_not_approved".to_owned(),
            });
        };
        if let Err(rejection) = policy.check(node_id) {
            return Err(MaterializerError::Refused {
                code: rejection.code().to_owned(),
            });
        }

        // Gate 3 — the operator's resource ceiling for the node. It is
        // authoritative: a schedule may narrow a limit but never widen one
        // and never introduce an unconfigured dimension. The node's live
        // capabilities are checked later, in the executor's preflight.
        if let Err(rejection) =
            policy.check_resource_request(node_id, template.resource_request.remote.as_ref())
        {
            return Err(MaterializerError::Refused {
                code: rejection.code().to_owned(),
            });
        }

        Ok(())
    }
}

#[async_trait]
impl OccurrenceMaterializer for ScheduleMaterializer {
    async fn materialize(
        &self,
        schedule: &ScheduleRecord,
        scheduled_for: DateTime<Utc>,
    ) -> Result<JobId, MaterializerError> {
        let occurrence_id = occurrence_job_id(&schedule.schedule_id, scheduled_for);

        // Crash-replay fast path. The daemon may have stopped between
        // writing this occurrence's job row and writing the occurrence
        // row; the job id is a pure function of the occurrence, so this
        // lookup is exact. Returning the existing job is what keeps the
        // occurrence to one job across the restart.
        if let Some(existing) = self
            .submission
            .existing_job(&occurrence_id)
            .await
            .map_err(|e| {
                MaterializerError::JobStore(codegg_core::jobs::JobStoreError::Storage(
                    codegg_core::error::StorageError::Database(e.to_string()),
                ))
            })?
        {
            return Ok(existing.job_id);
        }

        self.admit(schedule)?;

        let spec = schedule.job_template.new_job_for_occurrence(
            schedule.workspace_id.clone(),
            schedule.schedule_id.clone(),
            schedule.session_id.clone(),
            scheduled_for,
        );

        // The submission key carries the occurrence's identity through the
        // submission facade's own retry index, so two concurrent
        // materializations of the same occurrence converge on one job
        // instead of racing to create two.
        let key = SubmissionKey::new(format!(
            "schedule:{}:{}",
            schedule.schedule_id.as_str(),
            scheduled_for.timestamp_millis()
        ))
        .map_err(|_| MaterializerError::Refused {
            code: "invalid_submission_key".to_owned(),
        })?;

        match self.submission.submit(Some(key), spec).await {
            Ok(submitted) => Ok(submitted.job_id),
            Err(error) => {
                // A concurrent materialization of the same occurrence may
                // have won the race; converge on its job rather than
                // reporting a failure that would retry the occurrence.
                if let Some(existing) =
                    self.submission
                        .existing_job(&occurrence_id)
                        .await
                        .map_err(|e| {
                            MaterializerError::JobStore(codegg_core::jobs::JobStoreError::Storage(
                                codegg_core::error::StorageError::Database(e.to_string()),
                            ))
                        })?
                {
                    return Ok(existing.job_id);
                }
                Err(MaterializerError::Refused {
                    code: submission_error_code(&error),
                })
            }
        }
    }
}

/// A stable, secret-free reason code for a refused occurrence.
///
/// The submission error's own `Display` can carry a workspace path or a
/// payload fragment, so it never reaches a log line or a durable field
/// directly — only this slug does.
fn submission_error_code(error: &JobSubmissionError) -> String {
    match error {
        JobSubmissionError::SchedulerDisabled => "scheduler_disabled".to_owned(),
        JobSubmissionError::Workspace(_) => "workspace_unavailable".to_owned(),
        JobSubmissionError::Store(_) => "job_store_error".to_owned(),
        JobSubmissionError::Scheduler(_) => "scheduler_error".to_owned(),
        JobSubmissionError::InvalidSubmissionKey => "invalid_submission_key".to_owned(),
        JobSubmissionError::SubmissionKeyConflict => "submission_key_conflict".to_owned(),
        JobSubmissionError::PayloadTooLarge => "payload_too_large".to_owned(),
        JobSubmissionError::InvalidPayload(_) => "invalid_payload".to_owned(),
        JobSubmissionError::Goal(_) => "goal_lookup_failed".to_owned(),
        // M005 corrective C005: a process-class job was refused because the
        // target checkout is reserved by a live agent-run group. The
        // refusal's own message can name a path, so only the slug is
        // carried — and like every other refusal it is not recorded, so the
        // occurrence stays retryable once the group finishes.
        JobSubmissionError::ProcessDenied(_) => "process_denied".to_owned(),
    }
}
