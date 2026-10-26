//! Model-facing `job` control tool (async-background-jobs M001).
//!
//! One narrowly authorized adapter over the existing daemon
//! `CoreRequest::JobGet/JobWait/JobCancel` surface for background jobs
//! submitted via `test.execution_mode=background` (and later `bash`).
//! `wait` timeout means still-running, never a job failure. All actions
//! enforce exact-owner scope: the caller's session must equal the job's
//! owning session; knowledge of a `JobId` alone confers no rights.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::scheduler::job_notifications::{JobNotificationService, RecoveredJobTerminal};
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance, ToolTrust};
use crate::tool::{Tool, ToolCategory};

pub struct JobTool {
    submission: Option<Arc<crate::scheduler::JobSubmissionService>>,
    notifications: Option<Arc<JobNotificationService>>,
}

impl JobTool {
    pub fn new() -> Self {
        Self {
            submission: None,
            notifications: None,
        }
    }

    pub fn with_submission(
        mut self,
        submission: Arc<crate::scheduler::JobSubmissionService>,
    ) -> Self {
        self.submission = Some(submission);
        self
    }

    pub fn with_notifications(mut self, service: Arc<JobNotificationService>) -> Self {
        self.notifications = Some(service);
        self
    }
}

impl Default for JobTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for JobTool {
    fn name(&self) -> &str {
        "job"
    }

    fn description(&self) -> &str {
        "Inspect and control your own background jobs. Actions: status|wait|cancel|output. wait timeout means still-running, not failure; wait is a bounded manual observation, not a wakeup. Only jobs owned by your session are visible; other sessions' jobs are denied without data leakage. Do not poll in a loop; background completion arrives as one terminal notification automatically. Never batch a wait with other work: a mixed batch rejects the wait so it must be re-issued on its own. A solo wait holds no batch execution slot but is still bounded by timeout_ms. A resume_owner wait records a next-turn continuation marker and, only with an explicit host-trusted continuation grant, may start one bounded idle owner turn; without that opt-in, completion is notify-only and never spends model tokens by itself."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "wait", "cancel", "output"],
                    "description": "Control action. status returns stable state; wait observes up to timeout_ms then reports still-running; cancel terminates the exact job and descendants; output returns the bounded terminal summary or artifact reference."
                },
                "job_id": {
                    "type": "string",
                    "description": "Durable job id from a background JobHandle (inspect_ref)."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 300000,
                    "description": "Bounded observation timeout for wait only (default 10000). A wait timeout means still-running, never job failure. Whole-valued floats (e.g. 10000.0) are coerced to that integer; fractional values fail validation."
                }
            },
            "required": ["action", "job_id"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let value = self.execute_impl(input, None).await?;
        Ok(serde_json::to_string_pretty(&value).unwrap_or_default())
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let value = self.execute_impl(input, ctx).await?;
        let status = value
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string();
        let success = !matches!(status.as_str(), "failed" | "timed_out" | "denied" | "error");
        // The pre-change output was a bare `job <status>` with the summary
        // only in the structured value, so background-job stdout was not
        // retrievable from the transcript. Surface the bounded terminal
        // summary (or the still-running hint) inline.
        let mut output = format!("job {status}");
        if let Some(summary) = value.get("summary").and_then(|s| s.as_str()) {
            if !summary.is_empty() {
                output.push_str(&format!("\n{summary}"));
            }
        } else if let Some(message) = value.get("message").and_then(|s| s.as_str()) {
            output.push_str(&format!("\n{message}"));
        }
        Ok(StructuredToolResult {
            output,
            value: Some(value),
            success,
            provenance: Some(ToolProvenance {
                backend: "native".to_string(),
                implementation: "codegg/job".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                elapsed_ms: None,
                truncated: false,
                trust: ToolTrust::LocalTrusted,
            }),
        })
    }
}

impl JobTool {
    async fn execute_impl(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Execution("missing required 'action'".into()))?;
        let job_id_str = input
            .get("job_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::Execution("missing required 'job_id'".into()))?;
        let submission = self.submission.as_ref().ok_or_else(|| {
            ToolError::Execution("job control requires the daemon scheduler".into())
        })?;
        let caller_session = ctx.as_ref().and_then(|c| c.session_id.clone());
        let job_id = codegg_core::jobs::JobId::new_unchecked(job_id_str);
        // Exact-owner enforcement happens after loading the job/notice so
        // forged ids fail closed without distinguishing absence from
        // foreign ownership beyond a generic denied message.
        match action {
            "status" => {
                let job = submission
                    .scheduler()
                    .store()
                    .get_job(&job_id)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?
                    .ok_or_else(|| ToolError::Execution("job not found".into()))?;
                enforce_job_owner(&job.session_id, caller_session.as_deref())?;
                let mut value = json!({
                    "job_id": job.job_id.as_str(),
                    "kind": job.kind.as_str(),
                    "status": job_state_str(&job.state),
                    "session_id": job.session_id,
                    "turn_id": job.turn_id,
                });
                // F3: terminal states carry the verify-style structured
                // envelope; pending states carry the pending envelope.
                // Both derive from the typed `JobState`, never from text.
                if let Some((machine_code, exit_code, reason, recommendation, next_action)) =
                    job_state_terminal_envelope(&job.state)
                {
                    value["machine_code"] = json!(machine_code);
                    value["exit_code"] = json!(exit_code);
                    value["reason"] = json!(reason);
                    value["recommendation"] = json!(recommendation);
                    value["next_action"] = json!(next_action);
                } else {
                    let (machine_code, reason, recommendation, next_action) =
                        job_state_pending_envelope(&job.state);
                    value["machine_code"] = json!(machine_code);
                    value["exit_code"] = json!(None::<u32>);
                    value["reason"] = json!(reason);
                    value["recommendation"] = json!(recommendation);
                    value["next_action"] = json!(next_action);
                }
                Ok(value)
            }
            "wait" => {
                // F1: whole-valued floats coerce (10000.0 -> 10000) via the
                // shared tolerant parser; absent or unparseable values keep
                // the default bound.
                let timeout_ms = input
                    .get("timeout_ms")
                    .and_then(crate::tool::util::json_usize)
                    .map(|n| n as u64)
                    .unwrap_or(10_000)
                    .clamp(1, 300_000);
                // Owner check before waiting so a second principal cannot
                // observe foreign progress or hold a wait slot.
                let job = submission
                    .scheduler()
                    .store()
                    .get_job(&job_id)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?
                    .ok_or_else(|| ToolError::Execution("job not found".into()))?;
                enforce_job_owner(&job.session_id, caller_session.as_deref())?;
                match submission
                    .scheduler()
                    .wait_for_completion(&job_id, Duration::from_millis(timeout_ms))
                    .await
                {
                    // F3: verify-style envelope from the typed
                    // `ExecutorStatus`; agents branch on `machine_code`,
                    // not on the summary prose.
                    Ok(completion) => {
                        let (machine_code, exit_code, reason, recommendation, next_action) =
                            executor_terminal_envelope(&completion.status);
                        Ok(json!({
                            "job_id": job_id_str,
                            "status": executor_status_str(&completion.status),
                            "machine_code": machine_code,
                            "exit_code": exit_code,
                            "reason": reason,
                            "recommendation": recommendation,
                            "next_action": next_action,
                            "summary": truncate_bounded(&completion.summary, 2048),
                            "run_id": completion.run_id.map(|r| r.to_string()),
                        }))
                    }
                    Err(_) => Ok(json!({
                        "job_id": job_id_str,
                        "status": "still-running",
                        "machine_code": "JOB_STILL_RUNNING",
                        "exit_code": None::<u32>,
                        "reason": "bounded wait elapsed; the job continues",
                        "recommendation": "wait again with a fresh bound or rely on the terminal notification; do not poll in a loop",
                        "next_action": "wait",
                        "message": "bounded wait elapsed; job continues; terminal notification will arrive without polling",
                    })),
                }
            }
            "cancel" => {
                let job = submission
                    .scheduler()
                    .store()
                    .get_job(&job_id)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?
                    .ok_or_else(|| ToolError::Execution("job not found".into()))?;
                enforce_job_owner(&job.session_id, caller_session.as_deref())?;
                let result = submission
                    .scheduler()
                    .request_cancel(&job_id, "user_cancelled_via_job_tool")
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                // Q6: distinct envelope per outcome; never one shared message.
                // `Cancelled` is terminal now, `Requested` is draining, and
                // `AlreadyTerminal` re-reads the terminal state.
                match result.state {
                    codegg_core::jobs::CancelOutcome::Cancelled => {
                        let current = submission
                            .scheduler()
                            .store()
                            .get_job(&job_id)
                            .await
                            .map_err(|e| ToolError::Execution(e.to_string()))?
                            .map(|job| job_state_str(&job.state).to_string())
                            .unwrap_or_else(|| "cancelled".to_string());
                        Ok(json!({
                            "job_id": job_id_str,
                            "status": current,
                            "machine_code": "JOB_CANCELLED",
                            "cancelled": true,
                            "already_terminal": false,
                            "message": "job cancelled immediately; terminal state reached and descendants cancelled",
                            "next_action": "inspect diagnostics and revise",
                        }))
                    }
                    codegg_core::jobs::CancelOutcome::Requested => {
                        let current = submission
                            .scheduler()
                            .store()
                            .get_job(&job_id)
                            .await
                            .map_err(|e| ToolError::Execution(e.to_string()))?
                            .map(|job| job_state_str(&job.state).to_string())
                            .unwrap_or_else(|| "running".to_string());
                        Ok(json!({
                            "job_id": job_id_str,
                            "status": current,
                            "machine_code": "JOB_CANCEL_REQUESTED",
                            "cancelled": true,
                            "already_terminal": false,
                            "message": "cancellation requested; draining the running attempt and its descendants",
                            "next_action": "wait",
                        }))
                    }
                    codegg_core::jobs::CancelOutcome::AlreadyTerminal => {
                        let current = submission
                            .scheduler()
                            .store()
                            .get_job(&job_id)
                            .await
                            .map_err(|e| ToolError::Execution(e.to_string()))?
                            .ok_or_else(|| ToolError::Execution("job not found".into()))?;
                        let status = job_state_str(&current.state).to_string();
                        let (machine_code, _exit_code, _reason, _recommendation, next_action) =
                            job_state_terminal_envelope(&current.state).unwrap_or((
                                "JOB_UNKNOWN",
                                None,
                                "job reached an unrecognized terminal status",
                                "re-issue status to re-read the current state",
                                "inspect diagnostics and revise",
                            ));
                        Ok(json!({
                            "job_id": job_id_str,
                            "status": status,
                            "machine_code": machine_code,
                            "cancelled": false,
                            "already_terminal": true,
                            "message": "job already terminal; no cancellation applied",
                            "next_action": next_action,
                        }))
                    }
                }
            }
            "output" => {
                // Bounded terminal summary or artifact reference; never an
                // unbounded log dump. Prefers the durable notice (persisted
                // final outcome) so output retrieval does not re-execute.
                if let Some(service) = self.notifications.as_ref() {
                    if let Ok(Some(notice)) = service.get(job_id_str).await {
                        JobNotificationService::check_owner(&notice, caller_session.as_deref())
                            .map_err(|_| ToolError::Execution("job not found".into()))?;
                        // F3: the notice status is a stored typed
                        // projection, matched literally — never parsed out
                        // of the summary.
                        let (machine_code, exit_code, reason, recommendation, next_action) =
                            notice_terminal_envelope(&notice.status);
                        return Ok(json!({
                            "job_id": notice.job_id,
                            "status": notice.status,
                            "machine_code": machine_code,
                            "exit_code": exit_code,
                            "reason": reason,
                            "recommendation": recommendation,
                            "next_action": next_action,
                            "summary": notice.summary,
                            "run_id": notice.run_id,
                        }));
                    }
                }
                let job = submission
                    .scheduler()
                    .store()
                    .get_job(&job_id)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?
                    .ok_or_else(|| ToolError::Execution("job not found".into()))?;
                enforce_job_owner(&job.session_id, caller_session.as_deref())?;
                // Non-terminal jobs have no final output yet.
                let (machine_code, reason, recommendation, next_action) =
                    job_state_pending_envelope(&job.state);
                Ok(json!({
                    "job_id": job_id_str,
                    "status": job_state_str(&job.state),
                    "machine_code": machine_code,
                    "exit_code": None::<u32>,
                    "reason": reason,
                    "recommendation": recommendation,
                    "next_action": next_action,
                    "message": "no terminal output yet; use wait for bounded observation",
                }))
            }
            _ => Err(ToolError::Execution(format!(
                "unknown action '{action}'. Use status|wait|cancel|output"
            ))),
        }
    }
}

fn enforce_job_owner(job_session: &Option<String>, caller: Option<&str>) -> Result<(), ToolError> {
    // Q6: closed ownership — unknown job session and foreign session both
    // fail with the same `job not found`, so existence never leaks across
    // sessions.
    let Some(owner) = job_session.as_deref().filter(|s| !s.is_empty()) else {
        return Err(ToolError::Execution("job not found".into()));
    };
    let Some(caller) = caller.filter(|s| !s.is_empty()) else {
        return Err(ToolError::Execution("job not found".into()));
    };
    if owner != caller {
        // Same message as not-found to avoid leaking existence.
        return Err(ToolError::Execution("job not found".into()));
    }
    Ok(())
}

fn job_state_str(state: &codegg_core::jobs::JobState) -> &'static str {
    crate::scheduler::job_notifications::job_status_projection(state)
}

fn executor_status_str(status: &crate::scheduler::ExecutorStatus) -> &'static str {
    match status {
        crate::scheduler::ExecutorStatus::Completed => "completed",
        crate::scheduler::ExecutorStatus::Failed => "failed",
        crate::scheduler::ExecutorStatus::Cancelled => "cancelled",
        crate::scheduler::ExecutorStatus::TimedOut => "timed_out",
        crate::scheduler::ExecutorStatus::Interrupted => "interrupted",
    }
}

/// F3 terminal envelope for a typed executor outcome:
/// `(machine_code, exit_code, reason, recommendation, next_action)`.
///
/// Derived from the [`ExecutorStatus`] enum via the [`JobState`] domain —
/// never from summary text — so agents branch on codes, mirroring the
/// `verify` tool's `status`/`summary`/`next_action` shape. `exit_code` is
/// `Some(0)` only for `Completed` (reported solely on success); every
/// other terminal outcome leaves it `null` because the exit is unknown or
/// executor-held (failed: read the summary).
fn executor_terminal_envelope(
    status: &crate::scheduler::ExecutorStatus,
) -> (
    &'static str,
    Option<u32>,
    &'static str,
    &'static str,
    &'static str,
) {
    use crate::scheduler::ExecutorStatus as Executor;
    use codegg_core::jobs::JobState as State;
    let state = match status {
        Executor::Completed => State::Completed,
        Executor::Failed => State::Failed,
        Executor::Cancelled => State::Cancelled,
        Executor::TimedOut => State::TimedOut,
        Executor::Interrupted => State::Interrupted,
    };
    job_state_terminal_envelope(&state).expect("every executor status is terminal")
}

/// F3 terminal envelope for a typed [`JobState`]; `None` while pending.
///
/// The single owner of the machine-code/reason/next-action vocabulary so
/// the `wait`, `output`, and `status` actions cannot drift apart.
fn job_state_terminal_envelope(
    state: &codegg_core::jobs::JobState,
) -> Option<(
    &'static str,
    Option<u32>,
    &'static str,
    &'static str,
    &'static str,
)> {
    use codegg_core::jobs::JobState as State;
    match state {
        State::Completed => Some((
            "JOB_COMPLETED",
            Some(0),
            "job completed successfully",
            "consume the summary and continue",
            "continue",
        )),
        State::Failed => Some((
            "JOB_FAILED",
            None,
            "job failed; the bounded summary carries the executor report",
            "inspect the summary, revise the payload or command, and resubmit; rule out detached children (a trailing `&` inside a background job holds the output pipe open and fails the run)",
            "inspect diagnostics and revise",
        )),
        State::Cancelled => Some((
            "JOB_CANCELLED",
            None,
            "job was cancelled before it could complete",
            "resubmit only if the work is still needed",
            "inspect diagnostics and revise",
        )),
        State::TimedOut => Some((
            "JOB_TIMED_OUT",
            None,
            "job exceeded its own execution bound",
            "narrow the work or raise the job's own timeout, then resubmit",
            "inspect diagnostics and revise",
        )),
        State::Interrupted => Some((
            "JOB_INTERRUPTED",
            None,
            "job was interrupted and did not finish",
            "resubmit the work; the previous attempt did not complete",
            "inspect diagnostics and revise",
        )),
        State::Expired => Some((
            "JOB_EXPIRED",
            None,
            "job expired before it could run",
            "resubmit the work",
            "inspect diagnostics and revise",
        )),
        _ => None,
    }
}

/// F3 envelope for a durable notice status string: `(machine_code,
/// exit_code, reason, recommendation, next_action)`.
///
/// The notice status is a stored typed projection (`completed`,
/// `failed`, ...), matched literally; an unrecognized value falls back
/// to `JOB_UNKNOWN` rather than guessing from the summary.
fn notice_terminal_envelope(
    status: &str,
) -> (
    &'static str,
    Option<u32>,
    &'static str,
    &'static str,
    &'static str,
) {
    use codegg_core::jobs::JobState as State;
    let state = match status {
        "completed" => State::Completed,
        "failed" => State::Failed,
        "cancelled" => State::Cancelled,
        "timed_out" => State::TimedOut,
        "interrupted" => State::Interrupted,
        "expired" => State::Expired,
        _ => {
            return (
                "JOB_UNKNOWN",
                None,
                "job reached an unrecognized terminal status",
                "re-issue status to re-read the current state",
                "inspect diagnostics and revise",
            );
        }
    };
    job_state_terminal_envelope(&state).expect("notice terminal states are terminal")
}

/// F3 envelope for a pending [`JobState`]: `(machine_code, reason,
/// recommendation, next_action)`. Pending jobs have no exit code.
fn job_state_pending_envelope(
    state: &codegg_core::jobs::JobState,
) -> (&'static str, &'static str, &'static str, &'static str) {
    use codegg_core::jobs::JobState as State;
    match state {
        State::Running => (
            "JOB_RUNNING",
            "job is running",
            "wait for the terminal notification rather than polling; do not poll in a loop",
            "wait",
        ),
        _ => (
            "JOB_PENDING",
            "job is admitted but not running yet",
            "wait for the terminal notification rather than polling",
            "wait",
        ),
    }
}

fn truncate_bounded(s: &str, cap: usize) -> String {
    s.chars().take(cap).collect()
}

#[allow(dead_code)]
fn _keep_recovered_import() {
    let _ = std::mem::size_of::<RecoveredJobTerminal>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_tool_name() {
        assert_eq!(JobTool::new().name(), "job");
    }

    #[test]
    fn job_tool_requires_action_and_id() {
        let tool = JobTool::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = tool
                .execute(serde_json::json!({"action": "status"}))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("job_id"));
        });
    }

    #[test]
    fn owner_check_fails_closed() {
        assert!(enforce_job_owner(&Some("s1".into()), Some("s2")).is_err());
        assert!(enforce_job_owner(&Some("s1".into()), None).is_err());
        assert!(enforce_job_owner(&None, Some("s1")).is_err());
    }

    /// F3: the envelope derives from the status enum, so two completions
    /// with the same status but different summaries share one machine code.
    #[test]
    fn terminal_envelope_derives_from_status_enum_not_summary_text() {
        use crate::scheduler::ExecutorStatus as Executor;
        use codegg_core::jobs::JobState as State;
        let via_wait = executor_terminal_envelope(&Executor::Completed);
        let via_state = job_state_terminal_envelope(&State::Completed).expect("terminal");
        assert_eq!(via_wait.0, via_state.0);
        assert_eq!(via_wait.0, "JOB_COMPLETED");
        assert_eq!(via_wait.1, Some(0));
        assert_eq!(via_wait.4, "continue");
        // A different status is a different code even before reading text.
        assert_eq!(
            executor_terminal_envelope(&Executor::Failed).0,
            "JOB_FAILED"
        );
    }

    /// F3: failure carries no invented exit code and points at the
    /// detached-child pipe hazard that fails background runs opaquely.
    #[test]
    fn failed_envelope_leaves_exit_code_null_with_actionable_guidance() {
        use codegg_core::jobs::JobState as State;
        let (_, exit_code, _, recommendation, next_action) =
            job_state_terminal_envelope(&State::Failed).expect("terminal");
        assert_eq!(exit_code, None);
        assert!(recommendation.contains("detached children"));
        assert_eq!(next_action, "inspect diagnostics and revise");
    }

    /// F3: pending states never claim a terminal code or an exit code.
    #[test]
    fn pending_states_stay_non_terminal_without_exit_code() {
        use codegg_core::jobs::JobState as State;
        assert!(job_state_terminal_envelope(&State::Running).is_none());
        assert!(job_state_terminal_envelope(&State::Queued).is_none());
        assert_eq!(job_state_pending_envelope(&State::Running).0, "JOB_RUNNING");
        assert_eq!(job_state_pending_envelope(&State::Queued).0, "JOB_PENDING");
        assert_eq!(notice_terminal_envelope("bogus-status").0, "JOB_UNKNOWN");
    }
}
