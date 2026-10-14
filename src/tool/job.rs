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
        "Inspect and control your own background jobs. Actions: status|wait|cancel|output. wait timeout means still-running, not failure; wait is a bounded manual observation, not a wakeup. Only jobs owned by your session are visible; other sessions' jobs are denied without data leakage. Do not poll in a loop; background completion arrives as one terminal notification automatically. A resume_owner wait records a next-turn continuation marker and, only with an explicit host-trusted continuation grant, may start one bounded idle owner turn; without that opt-in, completion is notify-only and never spends model tokens by itself."
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
                    "description": "Bounded observation timeout for wait only (default 10000). A wait timeout means still-running, never job failure."
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
        Ok(StructuredToolResult {
            output: format!("job {status}"),
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
                Ok(json!({
                    "job_id": job.job_id.as_str(),
                    "kind": job.kind.as_str(),
                    "status": job_state_str(&job.state),
                    "session_id": job.session_id,
                    "turn_id": job.turn_id,
                }))
            }
            "wait" => {
                let timeout_ms = input
                    .get("timeout_ms")
                    .and_then(|v| v.as_u64())
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
                    Ok(completion) => Ok(json!({
                        "job_id": job_id_str,
                        "status": executor_status_str(&completion.status),
                        "summary": truncate_bounded(&completion.summary, 2048),
                        "run_id": completion.run_id.map(|r| r.to_string()),
                    })),
                    Err(_) => Ok(json!({
                        "job_id": job_id_str,
                        "status": "still-running",
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
                Ok(json!({
                    "job_id": job_id_str,
                    "cancelled": matches!(result.state, codegg_core::jobs::CancelOutcome::Cancelled | codegg_core::jobs::CancelOutcome::Requested),
                    "message": "cancellation targets the exact accepted job and its descendants only",
                }))
            }
            "output" => {
                // Bounded terminal summary or artifact reference; never an
                // unbounded log dump. Prefers the durable notice (persisted
                // final outcome) so output retrieval does not re-execute.
                if let Some(service) = self.notifications.as_ref() {
                    if let Ok(Some(notice)) = service.get(job_id_str).await {
                        JobNotificationService::check_owner(&notice, caller_session.as_deref())
                            .map_err(|_| ToolError::Execution("job not found".into()))?;
                        return Ok(json!({
                            "job_id": notice.job_id,
                            "status": notice.status,
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
                Ok(json!({
                    "job_id": job_id_str,
                    "status": job_state_str(&job.state),
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
    crate::scheduler::job_notifications::terminal_status_str(state)
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
        assert!(enforce_job_owner(&Some("s1".into()), Some("s1")).is_ok());
        assert!(enforce_job_owner(&Some("s1".into()), Some("s2")).is_err());
        assert!(enforce_job_owner(&Some("s1".into()), None).is_err());
        assert!(enforce_job_owner(&None, Some("s1")).is_err());
    }
}
