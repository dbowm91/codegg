//! Scheduler executor for Tool Programs (M005).
//!
//! Loads verified IR, creates a [`MeteredInterpreter`], and runs it
//! through the scheduler's admission-controlled execution path with
//! cancellation, heartbeat, and typed terminal results.

use std::sync::Arc;

use async_trait::async_trait;
use codegg_core::jobs::{JobKind, JobPayload, JobRecord};
use codegg_core::tool_program::{
    BrokerCallback, BudgetSnapshot, CallRequest, CallResult, InterpreterError, MeteredInterpreter,
    ProgramStatus, ProgramValue, RunConfig, RuntimeLimits,
};
use sha2::{Digest, Sha256};

use crate::scheduler::executor::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, ExecutorValidationError,
    JobExecutionContext, JobExecutor, JobProgressSink,
};
use crate::tool::broker::{BrokerInvocationContext, ToolBroker};
use crate::tool::ToolRegistry;

/// In-process fixture broker — retained for backward-compat tests only.
///
/// Returns a deterministic `ToolResult` for any tool call. Production
/// code should use [`BrokerAdapter`] with the real [`ToolBroker`].
#[cfg(test)]
pub struct FixtureBroker {
    heartbeat_count: std::sync::atomic::AtomicU32,
}

#[cfg(test)]
impl FixtureBroker {
    pub fn new() -> Self {
        Self {
            heartbeat_count: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub fn heartbeat_count(&self) -> u32 {
        self.heartbeat_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
impl Default for FixtureBroker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[async_trait]
impl BrokerCallback for FixtureBroker {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        // Return a deterministic result based on the tool name
        let output = serde_json::json!({
            "tool": request.tool_name,
            "status": "ok",
            "input": request.input,
        });
        Ok(CallResult {
            output: ProgramValue::ToolResult(output),
            artifacts: vec![],
            success: true,
        })
    }

    async fn submit_child_job(
        &self,
        request: &codegg_core::tool_program::child_job::ChildJobRequest,
    ) -> Result<codegg_core::tool_program::child_job::ChildJobResult, InterpreterError> {
        use codegg_core::tool_program::child_job::*;
        let details = match request.op {
            ChildJobOp::Test => ChildJobDetails::Test(TestJobResult {
                status: "passed".into(),
                framework: Some("fixture".into()),
                total: Some(1),
                passed: Some(1),
                failed: Some(0),
                skipped: Some(0),
                ..Default::default()
            }),
            ChildJobOp::Build => ChildJobDetails::Build(BuildJobResult {
                status: "success".into(),
                ..Default::default()
            }),
            ChildJobOp::Lint => ChildJobDetails::Lint(LintJobResult {
                status: "clean".into(),
                ..Default::default()
            }),
            ChildJobOp::Format => ChildJobDetails::Format(FormatJobResult {
                status: "clean".into(),
                ..Default::default()
            }),
        };
        Ok(ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 100,
            details,
            artifacts: vec![],
            error: None,
        })
    }

    async fn heartbeat(&self, _budget: &BudgetSnapshot) {
        self.heartbeat_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Scheduler executor for `JobKind::ToolProgram`.
///
/// Validates the program payload, loads and verifies IR, creates a
/// [`MeteredInterpreter`], and runs it with cancellation support.
pub struct ToolProgramExecutor {
    broker: Arc<ToolBroker>,
    registry: Arc<ToolRegistry>,
    submission: Option<Arc<crate::scheduler::submission::JobSubmissionService>>,
    notification_service:
        Option<Arc<crate::scheduler::tool_program_notifications::ToolProgramNotificationService>>,
    artifact_store: Option<Arc<dyn crate::context::ContextArtifactStore>>,
}

impl ToolProgramExecutor {
    pub fn new(broker: Arc<ToolBroker>, registry: Arc<ToolRegistry>) -> Self {
        Self {
            broker,
            registry,
            submission: None,
            notification_service: None,
            artifact_store: None,
        }
    }

    pub fn with_submission(
        mut self,
        submission: Arc<crate::scheduler::submission::JobSubmissionService>,
    ) -> Self {
        self.submission = Some(submission);
        self
    }

    pub fn with_notification_service(
        mut self,
        service: Arc<crate::scheduler::tool_program_notifications::ToolProgramNotificationService>,
    ) -> Self {
        self.notification_service = Some(service);
        self
    }

    pub fn with_artifact_store(
        mut self,
        store: Arc<dyn crate::context::ContextArtifactStore>,
    ) -> Self {
        self.artifact_store = Some(store);
        self
    }
}

impl Default for ToolProgramExecutor {
    fn default() -> Self {
        // Default creates with a minimal setup - only used in tests
        let registry = Arc::new(ToolRegistry::with_defaults());
        let broker = Arc::new(ToolBroker::new(&registry));
        Self {
            broker,
            registry,
            submission: None,
            notification_service: None,
            artifact_store: None,
        }
    }
}

/// Adapter that bridges the interpreter's `BrokerCallback` to the
/// real `ToolBroker` pipeline for programmatic tool calls.
pub struct BrokerAdapter {
    broker: Arc<ToolBroker>,
    registry: Arc<ToolRegistry>,
    program_id: String,
    submission: Option<Arc<crate::scheduler::submission::JobSubmissionService>>,
    workspace_id: Option<codegg_core::workspace::WorkspaceId>,
    allowed_tools: Option<std::collections::HashSet<String>>,
    cwd: std::path::PathBuf,
    ledger: Option<crate::tool::tool_program_ledger::ToolProgramLedger>,
    cancellation: Option<tokio_util::sync::CancellationToken>,
    progress: Option<Arc<dyn JobProgressSink>>,
    job_id: Option<codegg_core::jobs::JobId>,
    session_id: Option<String>,
    turn_id: Option<String>,
    agent_id: Option<String>,
    attempt_id: Option<String>,
    grant: Option<codegg_core::jobs::ToolAuthorityGrant>,
    deadline: Option<tokio::time::Instant>,
    artifact_store: Option<Arc<dyn crate::context::ContextArtifactStore>>,
    /// M013-C-34: Track submitted child job results for artifact handles.
    child_results: std::sync::Mutex<Vec<ChildJobTracking>>,
}

/// M013-C-34: Tracking record for child jobs submitted during Tool Program
/// execution. Used to build `ChildArtifactHandle` records after execution.
#[derive(Debug, Clone)]
struct ChildJobTracking {
    job_id: String,
    attempt_id: Option<String>,
    run_id: Option<String>,
    #[allow(dead_code)]
    sequence: u32,
    status: String,
    #[allow(dead_code)]
    success: bool,
    /// M014-G1: SHA-256 digest of the child's terminal result for artifact
    /// integrity verification. Computed from the child's completion summary
    /// and status.
    artifact_id: Option<String>,
    artifact_digest: Option<String>,
    absence_reason: Option<String>,
}

impl BrokerAdapter {
    pub fn new(broker: Arc<ToolBroker>, registry: Arc<ToolRegistry>, program_id: String) -> Self {
        Self {
            broker,
            registry,
            program_id,
            submission: None,
            workspace_id: None,
            allowed_tools: None,
            // No process-CWD fallback: production construction must set
            // the workspace root explicitly via `with_cwd`. Reading the
            // process CWD here would silently bind execution to whatever
            // directory the daemon was launched from.
            cwd: std::path::PathBuf::from("."),
            ledger: None,
            cancellation: None,
            progress: None,
            job_id: None,
            session_id: None,
            turn_id: None,
            agent_id: None,
            attempt_id: None,
            grant: None,
            deadline: None,
            artifact_store: None,
            child_results: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// M013-C-34: Return the tracked child job results for artifact handle
    /// construction. Consumes the tracking vector.
    #[allow(private_interfaces)]
    pub fn take_child_results(&self) -> Vec<ChildJobTracking> {
        self.child_results
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }

    pub fn with_submission(
        mut self,
        submission: Arc<crate::scheduler::submission::JobSubmissionService>,
    ) -> Self {
        self.submission = Some(submission);
        self
    }

    pub fn with_workspace_id(mut self, workspace_id: codegg_core::workspace::WorkspaceId) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    pub fn with_cwd(mut self, cwd: std::path::PathBuf) -> Self {
        self.cwd = cwd;
        self
    }

    pub fn with_allowed_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = Some(tools.into_iter().collect());
        self
    }

    pub fn with_ledger(
        mut self,
        ledger: crate::tool::tool_program_ledger::ToolProgramLedger,
    ) -> Self {
        self.ledger = Some(ledger);
        self
    }

    pub fn with_cancellation(mut self, cancellation: tokio_util::sync::CancellationToken) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    pub fn with_progress(
        mut self,
        progress: Arc<dyn JobProgressSink>,
        job_id: codegg_core::jobs::JobId,
    ) -> Self {
        self.progress = Some(progress);
        self.job_id = Some(job_id);
        self
    }

    pub fn with_grant(mut self, grant: codegg_core::jobs::ToolAuthorityGrant) -> Self {
        self.grant = Some(grant);
        self
    }

    pub fn with_context(
        mut self,
        session_id: Option<String>,
        turn_id: Option<String>,
        agent_id: Option<String>,
        attempt_id: Option<String>,
        grant: Option<codegg_core::jobs::ToolAuthorityGrant>,
    ) -> Self {
        self.session_id = session_id;
        self.turn_id = turn_id;
        self.agent_id = agent_id;
        self.attempt_id = attempt_id;
        self.grant = grant;
        self
    }

    pub fn with_deadline(mut self, deadline: Option<tokio::time::Instant>) -> Self {
        self.deadline = deadline;
        self
    }

    pub fn with_artifact_store(
        mut self,
        store: Arc<dyn crate::context::ContextArtifactStore>,
    ) -> Self {
        self.artifact_store = Some(store);
        self
    }
}

#[async_trait]
impl BrokerCallback for BrokerAdapter {
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError> {
        if let Some(allowed_tools) = &self.allowed_tools {
            if !allowed_tools.contains(&request.tool_name) {
                return Err(InterpreterError::BrokerError(format!(
                    "tool '{}' is not in the frozen Tool Program manifest",
                    request.tool_name
                )));
            }
        }
        let remaining_ms = self.deadline.map(|deadline| {
            deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .as_millis()
                .min(u64::MAX as u128) as u64
        });
        if remaining_ms == Some(0) {
            return Err(InterpreterError::Cancelled);
        }
        let ctx = BrokerInvocationContext {
            caller: crate::tool::contract::ToolCaller::Program {
                program_id: self.program_id.clone(),
            },
            cwd: self.cwd.clone(),
            session_id: self.session_id.clone(),
            workspace_id: self.workspace_id.as_ref().map(|w| w.to_string()),
            agent_id: self.agent_id.clone(),
            turn_id: self.turn_id.clone(),
            job_id: self.job_id.as_ref().map(ToString::to_string),
            attempt_id: self.attempt_id.clone(),
            permission_mode: None,
            timeout_ms: Some(remaining_ms.unwrap_or(30_000).min(30_000)),
            submission_key: None,
            authority: match &self.grant {
                Some(grant) => crate::tool::broker::BrokerAuthority::from_grant(grant.clone()),
                None => crate::tool::broker::BrokerAuthority::Unverified,
            },
            cancellation: self.cancellation.clone(),
            deadline: remaining_ms.map(|millis| {
                chrono::Utc::now()
                    + chrono::Duration::from_std(std::time::Duration::from_millis(millis))
                        .unwrap_or_else(|_| chrono::Duration::zero())
            }),
            principal_ref: self.grant.as_ref().map(|g| g.principal_ref.clone()),
            workspace_path_policy_id: self
                .grant
                .as_ref()
                .map(|g| g.workspace_path_policy_id.clone()),
            allowed_tools: self
                .allowed_tools
                .as_ref()
                .map(|s| s.iter().cloned().collect()),
            current_policy_revision: self.grant.as_ref().map(|g| g.policy_revision.clone()),
        };

        if self
            .cancellation
            .as_ref()
            .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
        {
            return Err(InterpreterError::Cancelled);
        }

        match self
            .broker
            .execute(
                &self.registry,
                &request.tool_name,
                request.input.clone(),
                ctx,
            )
            .await
        {
            Ok(result) => {
                // M012-F02: Classify the terminal status for programmatic consumers.
                // Only Success becomes a completed call; all other statuses are errors.
                match result.into_programmatic_outcome() {
                    Ok(value) => {
                        let program_value = match value.value {
                            Some(v) => ProgramValue::ToolResult(v),
                            None => ProgramValue::ToolResult(
                                serde_json::json!({"display": value.display}),
                            ),
                        };
                        Ok(CallResult {
                            output: program_value,
                            artifacts: value.artifacts.into_iter().map(|a| a.artifact_id).collect(),
                            success: true,
                        })
                    }
                    Err(outcome) => {
                        let msg = format!("tool '{}' failed: {:?}", request.tool_name, outcome);
                        Err(InterpreterError::BrokerError(msg))
                    }
                }
            }
            Err(e) => Err(InterpreterError::BrokerError(e.to_string())),
        }
    }

    async fn submit_child_job_with_checkpoint(
        &self,
        request: &codegg_core::tool_program::child_job::ChildJobRequest,
        checkpoint: &codegg_core::tool_program::InterpreterCheckpoint,
    ) -> Result<codegg_core::tool_program::child_job::ChildJobResult, InterpreterError> {
        use crate::scheduler::submission::SubmissionKey;
        use codegg_core::tool_program::child_job::*;

        let submission = self.submission.as_ref().ok_or_else(|| {
            InterpreterError::BrokerError("child job submission requires scheduler service".into())
        })?;

        let workspace_id = self.workspace_id.as_ref().ok_or_else(|| {
            InterpreterError::BrokerError("child job requires workspace_id".into())
        })?;

        request
            .config
            .validate()
            .map_err(InterpreterError::BrokerError)?;
        let resolved_cwd = request
            .config
            .resolve_cwd(&self.cwd)
            .map_err(InterpreterError::BrokerError)?;

        // Build the NewJob based on operation type
        let (kind, payload, timeout) = match &request.config {
            ChildJobConfig::Test(cfg) => {
                let argv = vec!["cargo".into(), "test".into()];
                let timeout = cfg.timeout_secs.map(std::time::Duration::from_secs);
                (
                    codegg_core::jobs::JobKind::Test,
                    codegg_core::jobs::JobPayload::Test {
                        command: "cargo test".into(),
                        argv,
                        cwd: resolved_cwd.clone(),
                        scope: cfg.scope.clone(),
                    },
                    timeout,
                )
            }
            ChildJobConfig::Build(cfg) => {
                let argv = cfg
                    .argv
                    .clone()
                    .unwrap_or_else(|| vec!["cargo".into(), "build".into()]);
                let timeout = cfg.timeout_secs.map(std::time::Duration::from_secs);
                (
                    codegg_core::jobs::JobKind::Build,
                    codegg_core::jobs::JobPayload::ManagedArgv {
                        argv,
                        cwd: resolved_cwd.clone(),
                    },
                    timeout,
                )
            }
            ChildJobConfig::Lint(cfg) => {
                let argv = cfg.argv.clone().unwrap_or_else(|| {
                    vec![
                        "cargo".into(),
                        "clippy".into(),
                        "--".into(),
                        "-D".into(),
                        "warnings".into(),
                    ]
                });
                let timeout = cfg.timeout_secs.map(std::time::Duration::from_secs);
                (
                    codegg_core::jobs::JobKind::Lint,
                    codegg_core::jobs::JobPayload::ManagedArgv {
                        argv,
                        cwd: resolved_cwd.clone(),
                    },
                    timeout,
                )
            }
            ChildJobConfig::Format(cfg) => {
                let argv = cfg.argv.clone().unwrap_or_else(|| {
                    vec!["cargo".into(), "fmt".into(), "--".into(), "--check".into()]
                });
                let timeout = cfg.timeout_secs.map(std::time::Duration::from_secs);
                (
                    codegg_core::jobs::JobKind::Format,
                    codegg_core::jobs::JobPayload::ManagedArgv {
                        argv,
                        cwd: resolved_cwd.clone(),
                    },
                    timeout,
                )
            }
        };

        // Create submission key for idempotency
        let config_hash = {
            let mut hasher = Sha256::new();
            hasher.update(format!("{:?}", request.op).as_bytes());
            hasher.update(format!("{:?}", request.config).as_bytes());
            format!("{:x}", hasher.finalize())
        };
        let submission_key = SubmissionKey::new(format!(
            "child-job:{}:{}:{}",
            self.program_id, request.sequence, config_hash
        ))
        .map_err(|e| InterpreterError::BrokerError(format!("invalid submission key: {}", e)))?;

        let requested_child_timeout =
            timeout.unwrap_or_else(|| std::time::Duration::from_secs(300));
        let requested_child_deadline = tokio::time::Instant::now() + requested_child_timeout;
        let effective_deadline = self
            .deadline
            .map(|parent| parent.min(requested_child_deadline))
            .unwrap_or(requested_child_deadline);
        let effective_timeout =
            effective_deadline.saturating_duration_since(tokio::time::Instant::now());
        let new_job = codegg_core::jobs::NewJob {
            workspace_id: workspace_id.clone(),
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
            kind,
            source: codegg_core::jobs::JobSource::AgentDelegated,
            priority: codegg_core::jobs::JobPriority::Normal,
            payload,
            resource_request: codegg_core::jobs::ResourceRequest::for_kind(kind),
            timeout: Some(effective_timeout),
            retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
            idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: Some(
                chrono::Utc::now()
                    + chrono::Duration::from_std(effective_timeout)
                        .unwrap_or_else(|_| chrono::Duration::zero()),
            ),
            schedule_id: None,
            depends_on: vec![],
            parent_job_id: self.job_id.clone(),
            parent_attempt_id: self
                .attempt_id
                .as_ref()
                .map(codegg_core::jobs::AttemptId::new_unchecked),
            parent_call_id: Some(format!("call:{}:{}", self.program_id, request.sequence)),
            parent_program_id: Some(self.program_id.clone()),
            parent_instruction_sequence: Some(request.sequence),
            relation_kind: Some("child_job".to_string()),
        };

        // On restart the pending checkpoint is authoritative: reattach by
        // durable child identity instead of rebuilding a submission whose
        // parent attempt necessarily belongs to the new daemon generation.
        let submitted = if let Some(pending) = &checkpoint.pending_child_wait {
            let child_id = codegg_core::jobs::JobId::new_unchecked(&pending.child_job_id);
            let child = submission
                .scheduler()
                .store()
                .get_job(&child_id)
                .await
                .map_err(|error| InterpreterError::BrokerError(error.to_string()))?
                .ok_or_else(|| {
                    InterpreterError::ReplayDivergence(
                        "pending child job is missing from the durable store".into(),
                    )
                })?;
            if child.parent_program_id.as_deref() != Some(self.program_id.as_str())
                || child.parent_job_id.as_ref() != self.job_id.as_ref()
                || child
                    .parent_attempt_id
                    .as_ref()
                    .map(ToString::to_string)
                    .as_deref()
                    != Some(pending.parent_attempt_id.as_str())
                || child.parent_call_id.as_deref() != Some(pending.canonical_call_id.as_str())
                || child
                    .parent_instruction_sequence
                    .is_some_and(|sequence| sequence != request.sequence)
            {
                return Err(InterpreterError::ReplayDivergence(format!(
                    "pending child durable lineage does not match checkpoint \
                     (program={:?}/{:?}, parent_job={:?}/{:?}, parent_attempt={:?}/{:?}, sequence={:?}/{})",
                    child.parent_program_id,
                    pending.parent_program_id,
                    child.parent_job_id,
                    self.job_id,
                    child.parent_attempt_id,
                    pending.parent_attempt_id,
                    child.parent_instruction_sequence,
                    request.sequence,
                )));
            }
            crate::scheduler::submission::SubmittedJob {
                job_id: child.job_id,
                state: child.state,
                workspace_id: child.workspace_id,
                priority: child.priority,
            }
        } else {
            submission
                .submit(Some(submission_key), new_job)
                .await
                .map_err(|e| {
                    InterpreterError::BrokerError(format!("child job submission failed: {}", e))
                })?
        };

        let parent_job_id = self
            .job_id
            .as_ref()
            .map(ToString::to_string)
            .ok_or_else(|| {
                InterpreterError::BrokerError(
                    "child wait checkpoint requires parent job identity".into(),
                )
            })?;
        let current_parent_attempt_id = self.attempt_id.clone().ok_or_else(|| {
            InterpreterError::BrokerError(
                "child wait checkpoint requires parent attempt identity".into(),
            )
        })?;
        if let Some(pending) = &checkpoint.pending_child_wait {
            if pending.child_job_id != submitted.job_id.to_string()
                || pending.parent_program_id != self.program_id
                || pending.parent_job_id != parent_job_id
                || pending.instruction_sequence != request.sequence
                || pending.operation_config_digest != format!("sha256:{config_hash}")
            {
                return Err(InterpreterError::ReplayDivergence(
                    "reattached child identity or lineage mismatch".into(),
                ));
            }
        }
        let parent_attempt_id = checkpoint
            .pending_child_wait
            .as_ref()
            .map(|pending| pending.parent_attempt_id.clone())
            .unwrap_or(current_parent_attempt_id);
        let mut pending_checkpoint = checkpoint.clone();
        pending_checkpoint.pending_child_wait = Some(codegg_core::tool_program::PendingChildWait {
            child_job_id: submitted.job_id.to_string(),
            expected_result_slot: request.sequence,
            child_op: request.op.to_string(),
            parent_program_id: self.program_id.clone(),
            parent_job_id,
            parent_attempt_id,
            canonical_call_id: format!("call:{}:{}", self.program_id, request.sequence),
            instruction_sequence: request.sequence,
            operation_config_digest: format!("sha256:{config_hash}"),
            operation_value: Some(codegg_core::tool_program::ProgramValue::String(
                request.op.to_string(),
            )),
            config_value: Some(codegg_core::tool_program::ProgramValue::from_json(
                match &request.config {
                    codegg_core::tool_program::child_job::ChildJobConfig::Test(config) => {
                        serde_json::to_value(config)
                    }
                    codegg_core::tool_program::child_job::ChildJobConfig::Build(config) => {
                        serde_json::to_value(config)
                    }
                    codegg_core::tool_program::child_job::ChildJobConfig::Lint(config) => {
                        serde_json::to_value(config)
                    }
                    codegg_core::tool_program::child_job::ChildJobConfig::Format(config) => {
                        serde_json::to_value(config)
                    }
                }
                .unwrap_or_default(),
            )),
        });
        pending_checkpoint.refresh_semantic_digest();
        self.checkpoint(&pending_checkpoint).await?;
        crate::test_failpoint::hit("tool_program_after_child_wait_checkpoint");

        let mut wait_timeout = effective_timeout + std::time::Duration::from_secs(30);

        let completion = loop {
            if self
                .cancellation
                .as_ref()
                .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
            {
                let _ = submission
                    .scheduler()
                    .request_cancel(&submitted.job_id, "parent_tool_program_cancelled")
                    .await;
                return Err(InterpreterError::Cancelled);
            }
            match submission
                .scheduler()
                .wait_for_completion(&submitted.job_id, std::time::Duration::from_millis(100))
                .await
            {
                Ok(completion) => break completion,
                Err(error) if wait_timeout > std::time::Duration::ZERO => {
                    wait_timeout =
                        wait_timeout.saturating_sub(std::time::Duration::from_millis(100));
                    if wait_timeout.is_zero() {
                        let _ = submission
                            .scheduler()
                            .request_cancel(&submitted.job_id, "child_job_wait_timeout")
                            .await;
                        return Err(InterpreterError::BrokerError(format!(
                            "child job wait failed: {}",
                            error
                        )));
                    }
                }
                Err(error) => {
                    return Err(InterpreterError::BrokerError(format!(
                        "child job wait failed: {}",
                        error
                    )))
                }
            }
        };

        // Map ExecutorStatus to ChildJobResult
        let success = matches!(
            completion.status,
            crate::scheduler::executor::ExecutorStatus::Completed
        );
        let exit_code = match completion.status {
            crate::scheduler::executor::ExecutorStatus::Completed => Some(0),
            crate::scheduler::executor::ExecutorStatus::Failed => Some(1),
            crate::scheduler::executor::ExecutorStatus::TimedOut => Some(124),
            crate::scheduler::executor::ExecutorStatus::Cancelled => Some(130),
            crate::scheduler::executor::ExecutorStatus::Interrupted => Some(1),
        };

        let is_cancelled = matches!(
            completion.status,
            crate::scheduler::executor::ExecutorStatus::Cancelled
        );
        let is_timed_out = matches!(
            completion.status,
            crate::scheduler::executor::ExecutorStatus::TimedOut
        );

        // Extract command string from config for result metadata
        let command_str = match &request.config {
            ChildJobConfig::Test(_) => Some("cargo test".into()),
            ChildJobConfig::Build(cfg) => cfg
                .argv
                .as_ref()
                .map(|a| a.join(" "))
                .or_else(|| Some("cargo build".into())),
            ChildJobConfig::Lint(cfg) => cfg
                .argv
                .as_ref()
                .map(|a| a.join(" "))
                .or_else(|| Some("cargo clippy".into())),
            ChildJobConfig::Format(cfg) => cfg
                .argv
                .as_ref()
                .map(|a| a.join(" "))
                .or_else(|| Some("cargo fmt".into())),
        };

        // Infer framework from command for test operations
        let framework = command_str.as_ref().and_then(|cmd| {
            if cmd.starts_with("cargo") {
                Some("cargo".into())
            } else if cmd.starts_with("pytest") || cmd.starts_with("python") {
                Some("pytest".into())
            } else if cmd.starts_with("npm") || cmd.starts_with("npx") {
                Some("npm".into())
            } else if cmd.starts_with("make") {
                Some("make".into())
            } else {
                None
            }
        });

        let details = match &request.config {
            ChildJobConfig::Test(_) => ChildJobDetails::Test(TestJobResult {
                status: if success {
                    "passed"
                } else if is_cancelled {
                    "cancelled"
                } else if is_timed_out {
                    "timed_out"
                } else {
                    "failed"
                }
                .into(),
                framework,
                cancelled: is_cancelled,
                timed_out: is_timed_out,
                ..Default::default()
            }),
            ChildJobConfig::Build(_) => ChildJobDetails::Build(BuildJobResult {
                status: if success {
                    "success"
                } else if is_cancelled {
                    "cancelled"
                } else if is_timed_out {
                    "timed_out"
                } else {
                    "failure"
                }
                .into(),
                command: command_str,
                ..Default::default()
            }),
            ChildJobConfig::Lint(_) => ChildJobDetails::Lint(LintJobResult {
                status: if success {
                    "clean"
                } else if is_cancelled {
                    "cancelled"
                } else if is_timed_out {
                    "timed_out"
                } else {
                    "warnings"
                }
                .into(),
                command: command_str,
                ..Default::default()
            }),
            ChildJobConfig::Format(_) => ChildJobDetails::Format(FormatJobResult {
                status: if success {
                    "clean"
                } else if is_cancelled {
                    "cancelled"
                } else if is_timed_out {
                    "timed_out"
                } else {
                    "needs_formatting"
                }
                .into(),
                command: command_str,
                would_change: !success && !is_cancelled && !is_timed_out,
            }),
        };

        // M013-C-34: Track the child job result for artifact handle construction.
        let status_str = if success {
            "completed"
        } else if is_cancelled {
            "cancelled"
        } else if is_timed_out {
            "timed_out"
        } else {
            "failed"
        };
        let attempts = submission
            .scheduler()
            .store()
            .list_attempts(&submitted.job_id)
            .await
            .map_err(|error| InterpreterError::BrokerError(error.to_string()))?;
        let attempt = attempts.into_iter().max_by_key(|attempt| attempt.sequence);
        // Keep opaque scheduler metadata separate from the canonical context
        // artifact. Call-result consumers resolve the first handle through the
        // context-artifact store, so the durable summary must precede
        // job/run identity handles.
        let mut canonical_artifacts = Vec::new();
        let mut metadata_artifacts = Vec::new();
        if let Some(run_id) = completion.run_id.as_ref() {
            metadata_artifacts.push(format!("run://{run_id}"));
        }
        if let Some(attempt) = attempt.as_ref() {
            metadata_artifacts.push(format!(
                "job://{}/attempt/{}",
                submitted.job_id, attempt.attempt_id
            ));
        }
        // The scheduler's completion summary is the bounded display projection;
        // the executor-owned RunStore remains authoritative for full output.
        // Persisting this summary gives callers a durable expansion handle even
        // when a lightweight executor has no RunStore run id.
        let mut canonical_artifact_id = None;
        let mut canonical_artifact_digest = None;
        if let Some(store) = &self.artifact_store {
            let handle = format!("child-job://{}/summary", submitted.job_id);
            let session_id = self
                .session_id
                .clone()
                .unwrap_or_else(|| "tool-program".to_string());
            let summary_artifact = crate::context::ContextArtifact {
                handle: handle.clone(),
                session_id,
                turn_index: 0,
                tool_call_id: Some(format!("call:{}:{}", self.program_id, request.sequence)),
                tool_name: Some(format!("child_job/{}", request.op)),
                kind: crate::context::ArtifactKind::CommandOutput,
                created_at_ms: chrono::Utc::now().timestamp_millis(),
                content_hash: crate::context::compute_content_hash(&completion.summary),
                raw_bytes_len: completion.summary.len(),
                estimated_tokens: completion.summary.len().div_ceil(4),
                redacted_content: completion.summary.clone(),
            };
            let content_hash = summary_artifact.content_hash.clone();
            if let Err(error) = store.put(summary_artifact).await {
                tracing::warn!(%error, child_job = %submitted.job_id, "failed to persist child-job summary artifact");
            } else {
                canonical_artifacts.push(handle.clone());
                canonical_artifact_id = Some(handle);
                canonical_artifact_digest = Some(content_hash);
            }
        }
        canonical_artifacts.extend(metadata_artifacts);

        if let Ok(mut results) = self.child_results.lock() {
            results.push(ChildJobTracking {
                job_id: submitted.job_id.to_string(),
                attempt_id: attempt
                    .as_ref()
                    .map(|attempt| attempt.attempt_id.to_string()),
                run_id: attempt
                    .as_ref()
                    .and_then(|attempt| attempt.run_id.as_ref())
                    .map(ToString::to_string),
                sequence: request.sequence,
                status: status_str.to_string(),
                success,
                artifact_id: canonical_artifact_id,
                artifact_digest: canonical_artifact_digest,
                absence_reason: if canonical_artifacts.is_empty() {
                    Some("child executor completed without a canonical result artifact".into())
                } else {
                    None
                },
            });
        }

        Ok(ChildJobResult {
            success,
            exit_code,
            duration_ms: completion.metrics.elapsed_ms,
            details,
            artifacts: canonical_artifacts,
            error: if !success {
                Some(completion.summary)
            } else {
                None
            },
        })
    }

    async fn heartbeat(&self, budget: &BudgetSnapshot) {
        if let (Some(progress), Some(job_id)) = (&self.progress, &self.job_id) {
            let _ = progress
                .progress(
                    job_id,
                    &format!(
                        "tool_program heartbeat steps={} calls={} iterations={}",
                        budget.steps, budget.calls, budget.iterations
                    ),
                )
                .await;
        }
    }

    async fn call_reserved(
        &self,
        sequence: u32,
        request: &CallRequest,
    ) -> Result<(), InterpreterError> {
        if let Some(ledger) = &self.ledger {
            ledger
                .reserve_call(&self.program_id, sequence, request)
                .map_err(|error| InterpreterError::BrokerError(error.to_string()))?;
        }
        Ok(())
    }

    async fn call_completed(
        &self,
        completed: &codegg_core::tool_program::CompletedCall,
    ) -> Result<(), InterpreterError> {
        if let Some(ledger) = &self.ledger {
            ledger
                .persist_call_completion(&self.program_id, completed)
                .map_err(|error| InterpreterError::BrokerError(error.to_string()))?;
        }
        Ok(())
    }

    async fn checkpoint(
        &self,
        checkpoint: &codegg_core::tool_program::InterpreterCheckpoint,
    ) -> Result<(), InterpreterError> {
        if let Some(ledger) = &self.ledger {
            ledger
                .persist_checkpoint(&self.program_id, checkpoint)
                .map_err(|error| InterpreterError::BrokerError(error.to_string()))?;
        }
        Ok(())
    }
}

#[async_trait]
impl JobExecutor for ToolProgramExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ToolProgram
    }

    fn supports(&self, kind: JobKind) -> bool {
        matches!(kind, JobKind::ToolProgram)
    }

    fn validate(&self, job: &JobRecord) -> Result<(), ExecutorValidationError> {
        match &job.payload {
            JobPayload::ToolProgram {
                program_id,
                invocation_key,
                source_digest,
                authority_digest,
                execution_context_json,
                source_ref,
                source_length,
                allowed_tools,
                authority_grant_json,
                ..
            } => {
                if program_id.is_empty() {
                    return Err(ExecutorValidationError::MissingField("program_id".into()));
                }
                if invocation_key.is_empty() {
                    return Err(ExecutorValidationError::MissingField(
                        "invocation_key".into(),
                    ));
                }
                if source_digest.is_empty() {
                    return Err(ExecutorValidationError::MissingField(
                        "source_digest".into(),
                    ));
                }
                if authority_digest.is_empty() {
                    return Err(ExecutorValidationError::MissingField(
                        "authority_digest".into(),
                    ));
                }
                if execution_context_json.is_none() {
                    return Err(ExecutorValidationError::MissingField(
                        "execution_context_json".into(),
                    ));
                }
                if source_ref.is_none() || source_length.is_none() {
                    return Err(ExecutorValidationError::MissingField(
                        "source_ref/source_length".into(),
                    ));
                }
                if authority_grant_json.is_none() {
                    return Err(ExecutorValidationError::MissingField(
                        "authority_grant_json".into(),
                    ));
                }
                let manifest =
                    crate::tool::program_manifest::resolve_manifest(&self.broker, allowed_tools);
                if !crate::tool::program_manifest::manifest_is_valid(&manifest) {
                    return Err(ExecutorValidationError::InvalidPayload(format!(
                        "tool-program manifest rejected: {}",
                        manifest
                            .rejected
                            .iter()
                            .map(|rejection| {
                                format!("{} ({})", rejection.tool_name, rejection.reason)
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
                Ok(())
            }
            _ => Err(ExecutorValidationError::UnsupportedKind {
                executor: "tool_program".into(),
                kind: format!("{:?}", job.kind),
            }),
        }
    }

    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let started = std::time::Instant::now();

        // Emit progress: starting
        let _ = ctx
            .progress
            .progress(ctx.job_id(), "tool_program: starting")
            .await;

        // M012: Early cancellation check before validation.
        if ctx.cancellation.is_cancelled() {
            return ExecutorCompletion {
                status: ExecutorStatus::Cancelled,
                summary: "cancelled before execution".into(),
                run_id: None,
                metrics: ExecutorMetrics {
                    cpu_time_ms: None,
                    peak_memory_mb: None,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                },
            };
        }

        // Extract payload
        let (
            program_id,
            invocation_key,
            source_digest,
            ir_digest,
            authority_digest,
            execution_context_json,
            source_ref,
            source_length,
            allowed_tools,
            execution_mode,
            authority_grant_json,
        ) = match &ctx.job.payload {
            JobPayload::ToolProgram {
                program_id,
                invocation_key,
                source_digest,
                ir_digest,
                authority_digest,
                execution_context_json,
                source_ref,
                source_length,
                allowed_tools,
                execution_mode,
                authority_grant_json,
                ..
            } => (
                program_id.clone(),
                invocation_key.clone(),
                source_digest.clone(),
                ir_digest.clone(),
                authority_digest.clone(),
                execution_context_json.clone(),
                source_ref.clone(),
                *source_length,
                allowed_tools.clone(),
                execution_mode.clone(),
                authority_grant_json.clone(),
            ),
            _ => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: "invalid payload".into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };

        // Emit progress: validating
        let _ = ctx
            .progress
            .progress(ctx.job_id(), "tool_program: validating")
            .await;

        let execution_context = match execution_context_json.as_deref().and_then(|json| {
            serde_json::from_str::<codegg_core::jobs::ToolProgramExecutionContext>(json).ok()
        }) {
            Some(context) if context.validate().is_ok() => context,
            _ => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: "tool-program execution context is missing or invalid".into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };
        let expected_authority = crate::tool::tool_program_context::authority_digest(
            &execution_context,
            &allowed_tools,
            &source_digest,
        );
        if expected_authority != authority_digest {
            tracing::warn!(
                expected = %expected_authority,
                got = %authority_digest,
                "tool-program authority digest mismatch (context redacted)"
            );
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: "tool-program authority context digest mismatch".into(),
                run_id: None,
                metrics: ExecutorMetrics {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                },
            };
        }
        if invocation_key.is_empty() {
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: "tool-program invocation key is empty".into(),
                run_id: None,
                metrics: ExecutorMetrics {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                },
            };
        }

        // Emit progress: loading IR
        let _ = ctx
            .progress
            .progress(ctx.job_id(), "tool_program: loading IR")
            .await;

        let source_ref = match (source_ref, source_length) {
            (Some(relative_path), Some(length)) => {
                crate::tool::tool_program_source::ToolProgramSourceRef {
                    digest: source_digest.clone(),
                    length,
                    relative_path,
                }
            }
            (_rel, _len) => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: "missing durable tool-program source reference".into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };
        let source = match crate::tool::tool_program_source::ToolProgramSourceStore::new(
            &ctx.workspace_root,
        )
        .retrieve(&source_ref)
        {
            Ok(source) => source,
            Err(error) => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: format!("source verification failed: {}", error),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };
        if codegg_core::tool_program::ProgramStore::digest_source(&source) != source_digest {
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: "source verification failed: digest mismatch".into(),
                run_id: None,
                metrics: ExecutorMetrics {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                },
            };
        }

        let compilation = match codegg_core::tool_program::compile_program(&source) {
            Ok(c) => c,
            Err(e) => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: format!("compilation failed: {}", e),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };

        // Verify IR integrity
        if let Err(e) = codegg_core::tool_program::verify_ir_integrity(&compilation.ir) {
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: format!("IR verification failed: {}", e),
                run_id: None,
                metrics: ExecutorMetrics {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                },
            };
        }

        if let Some(expected_ir_digest) = ir_digest {
            if compilation.ir.digest != expected_ir_digest {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: "IR verification failed: digest mismatch".into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        }

        // Emit progress: executing
        let _ = ctx
            .progress
            .progress(ctx.job_id(), "tool_program: executing")
            .await;

        // Create runtime limits from IR bounds
        let mut limits = RuntimeLimits::from(&compilation.ir.bounds);
        // Set sensible defaults for M005
        limits.max_stall_time_ms = 60_000; // 60s stall timeout
        limits.max_per_call_time_ms = 30_000; // 30s per-call timeout
        limits.max_retries = 2; // Up to 2 retries for transient errors
        limits.max_bytes = 16 * 1024 * 1024; // 16 MiB value growth for spill tests
        limits.max_value_growth = 16 * 1024 * 1024;

        // Save per-call timeout before moving limits
        let per_call_timeout_ms = limits.max_per_call_time_ms;

        // Compute wall deadline from job timeout or program limits
        let wall_deadline = ctx
            .job
            .deadline
            .and_then(|d| {
                let dur = d.signed_duration_since(chrono::Utc::now());
                if dur.num_milliseconds() > 0 {
                    Some(
                        tokio::time::Instant::now()
                            + tokio::time::Duration::from_millis(dur.num_milliseconds() as u64),
                    )
                } else {
                    None
                }
            })
            .or_else(|| {
                if limits.max_wall_time_ms > 0 {
                    Some(
                        tokio::time::Instant::now()
                            + tokio::time::Duration::from_millis(limits.max_wall_time_ms),
                    )
                } else {
                    None
                }
            });

        // Create interpreter and restore durable completed calls before the
        // first instruction. Replay then returns the stored typed result
        // without invoking the broker again.
        let ledger = crate::tool::tool_program_ledger::ToolProgramLedger::new(&ctx.workspace_root);
        let mut interpreter = MeteredInterpreter::new(compilation.ir.clone(), limits);
        match ledger.load_completed_calls(&program_id) {
            Ok(completed_calls) => interpreter.load_completed_calls(completed_calls),
            Err(error) => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: format!("durable call journal is corrupt: {error}"),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        }

        // M014-C11/C-12: Load the latest valid checkpoint and restore
        // interpreter state before resumed execution. This includes
        // locals, stack, budgets, next call sequence, and pending child
        // wait identity.
        let checkpoint = match ledger.load_latest_checkpoint_checked(&program_id) {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: format!("durable checkpoint journal is corrupt: {error}"),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };
        if let Some(checkpoint) = checkpoint {
            let durable_deadline = ctx.job.deadline.map(|deadline| deadline.timestamp_millis());
            if checkpoint.original_deadline_millis != durable_deadline {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: "checkpoint original deadline diverges from durable job deadline"
                        .into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
            match interpreter.restore_checkpoint(checkpoint) {
                Ok(()) => {
                    let _ = ctx
                        .progress
                        .progress(ctx.job_id(), "tool_program: checkpoint restored")
                        .await;
                }
                Err(error) => {
                    return ExecutorCompletion {
                        status: ExecutorStatus::Failed,
                        summary: format!("checkpoint restore divergence: {}", error),
                        run_id: None,
                        metrics: ExecutorMetrics {
                            elapsed_ms: started.elapsed().as_millis() as u64,
                            ..Default::default()
                        },
                    };
                }
            }
        }

        // M013 C-01/C-03: The authority grant is pre-computed at submission
        // time and carried in the job payload. Deserialize and verify it.
        // The executor must NOT fabricate a replacement grant.
        let grant: codegg_core::jobs::ToolAuthorityGrant = match authority_grant_json
            .as_ref()
            .and_then(|json| serde_json::from_str(json).ok())
        {
            Some(g) => g,
            None => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: "missing or invalid authority_grant_json in payload".into(),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };
        if !grant.verify_integrity() {
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: "authority grant integrity verification failed".into(),
                run_id: None,
                metrics: ExecutorMetrics {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                },
            };
        }
        if !grant.is_valid(chrono::Utc::now().timestamp_millis()) {
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: "authority grant is expired or revoked".into(),
                run_id: None,
                metrics: ExecutorMetrics {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                },
            };
        }
        let frozen_contracts: Vec<crate::tool::tool_program_context::ContractEntry> =
            match serde_json::from_str::<serde_json::Value>(&grant.contract_snapshot_json)
                .ok()
                .and_then(|value| value.get("contracts").cloned())
                .and_then(|contracts| serde_json::from_value(contracts).ok())
            {
                Some(contracts) => contracts,
                None => {
                    return ExecutorCompletion {
                        status: ExecutorStatus::Failed,
                        summary: "missing or invalid frozen contract snapshot".into(),
                        run_id: None,
                        metrics: ExecutorMetrics {
                            elapsed_ms: started.elapsed().as_millis() as u64,
                            ..Default::default()
                        },
                    };
                }
            };
        let current_contracts = match crate::tool::tool_program_context::resolve_contract_snapshot(
            &self.broker,
            &allowed_tools,
        ) {
            Ok(contracts) => contracts,
            Err(error) => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: format!("runtime contract resolution failed: {error}"),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };
        let frozen_digest =
            crate::tool::tool_program_context::canonical_contract_digest(&frozen_contracts);
        let current_digest =
            crate::tool::tool_program_context::canonical_contract_digest(&current_contracts);
        if frozen_contracts != current_contracts
            || frozen_digest.as_deref() != Ok(grant.contract_digest.as_str())
            || current_digest.as_deref() != Ok(grant.contract_digest.as_str())
        {
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: format!(
                    "runtime contract snapshot drift detected (grant={}, frozen={:?}, current={:?})",
                    grant.contract_digest, frozen_digest, current_digest
                ),
                run_id: None,
                metrics: ExecutorMetrics {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                },
            };
        }
        let contract_digest = grant.contract_digest.clone();
        let manifest_digest = crate::tool::tool_program_context::stable_digest(
            &serde_json::to_string(&serde_json::json!({
                "allowed_tools": allowed_tools,
                "source_digest": source_digest,
                "ir_digest": compilation.ir.digest,
                "contract_digest": contract_digest,
            }))
            .unwrap_or_default(),
        );
        interpreter.set_replay_fingerprint(codegg_core::tool_program::ReplayFingerprint {
            schema_version: 2,
            program_id: program_id.clone(),
            authority_digest: grant.compute_digest(),
            execution_context_digest: execution_context.compute_digest(),
            source_digest: source_digest.clone(),
            ir_digest: compilation.ir.digest.clone(),
            workspace_id: ctx.workspace_id.to_string(),
            workspace_path_policy_id: execution_context.workspace_path_policy_id.clone(),
            policy_revision: execution_context
                .policy_revision
                .clone()
                .unwrap_or_default(),
            session_id: execution_context.session_id.clone(),
            agent_id: execution_context.agent_id.clone(),
            manifest_digest,
            contract_digest: contract_digest.clone(),
            backend_selection: "native_only".to_string(),
            original_deadline_millis: ctx.job.deadline.map(|d| d.timestamp_millis()),
        });

        // Bind the immutable contract snapshot to this workspace's durable
        // artifact store for the lifetime of the attempt.
        let canonical_artifact_store: Arc<dyn crate::context::ContextArtifactStore> =
            self.artifact_store.clone().unwrap_or_else(|| {
                Arc::new(crate::context::FileArtifactStore::new(&ctx.workspace_root))
            });
        let workspace_broker = self
            .broker
            .for_workspace_artifacts(canonical_artifact_store.clone());

        // Create real broker adapter
        let mut broker_adapter = BrokerAdapter::new(
            Arc::new(workspace_broker),
            self.registry.clone(),
            program_id.clone(),
        );
        if let Some(ref submission) = self.submission {
            broker_adapter = broker_adapter.with_submission(submission.clone());
        }
        broker_adapter = broker_adapter.with_workspace_id(ctx.workspace_id.clone());
        broker_adapter = broker_adapter.with_cwd(ctx.workspace_root.clone());
        broker_adapter = broker_adapter.with_artifact_store(canonical_artifact_store.clone());
        broker_adapter = broker_adapter.with_allowed_tools(allowed_tools.clone());
        broker_adapter = broker_adapter
            .with_ledger(ledger.clone())
            .with_cancellation(ctx.cancellation.clone())
            .with_progress(ctx.progress.clone(), ctx.job.job_id.clone())
            .with_context(
                execution_context.session_id.clone(),
                execution_context.turn_id.clone(),
                execution_context.agent_id.clone(),
                Some(ctx.attempt_id.to_string()),
                None,
            )
            .with_grant(grant)
            .with_deadline(wall_deadline);

        // A terminal result is the commit point. If the daemon died after
        // that commit but before the attempt/notification transition, a new
        // generation must converge from the verified record without running
        // the interpreter or nested calls again.
        let result_store =
            crate::tool::tool_program_result::ToolProgramResultStore::new(&ctx.workspace_root);
        match result_store.load(&program_id) {
            Ok(Some(record)) => {
                if execution_mode == "background" {
                    if let Some(service) = &self.notification_service {
                        if let Err(error) = service
                            .record_terminal_result(
                                &program_id,
                                ctx.job_id().as_str(),
                                execution_context.session_id.as_deref(),
                                execution_context.agent_id.as_deref(),
                                execution_context.turn_id.as_deref(),
                                &record,
                            )
                            .await
                        {
                            return ExecutorCompletion {
                                status: ExecutorStatus::Failed,
                                summary: format!(
                                    "terminal result recovered but durable notification failed: {error}"
                                ),
                                run_id: None,
                                metrics: ExecutorMetrics {
                                    elapsed_ms: started.elapsed().as_millis() as u64,
                                    ..Default::default()
                                },
                            };
                        }
                    }
                }
                let status = match record.result.status {
                    ProgramStatus::Completed => ExecutorStatus::Completed,
                    ProgramStatus::Cancelled => ExecutorStatus::Cancelled,
                    ProgramStatus::TimedOut | ProgramStatus::Stalled => ExecutorStatus::TimedOut,
                    ProgramStatus::Failed
                    | ProgramStatus::Incomplete
                    | ProgramStatus::Recoverable => ExecutorStatus::Failed,
                };
                return ExecutorCompletion {
                    status,
                    summary: format!(
                        "recovered committed Tool Program result {}",
                        record.result_digest
                    ),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
            Ok(None) => {}
            Err(error) => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: format!("committed Tool Program result is corrupt: {error}"),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        }

        // Build run configuration
        let run_config = RunConfig {
            wall_deadline,
            per_call_timeout_ms: Some(per_call_timeout_ms),
            result_schema: None, // No schema for M005 fixture programs
        };

        // Run the interpreter with cancellation support and config
        let result = interpreter
            .run_with_config(&broker_adapter, Some(&ctx.cancellation), &run_config)
            .await;

        // The public result must not become terminal before the redacted
        // completion ledger has been durably updated.
        if let Err(error) =
            ledger.persist_completed_calls(&program_id, interpreter.completed_calls())
        {
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: format!("call ledger persistence failed: {error}"),
                run_id: None,
                metrics: ExecutorMetrics {
                    cpu_time_ms: None,
                    peak_memory_mb: None,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                },
            };
        }
        // M013-C-38: Only native backend is supported. The admission
        // gate in tool_program.rs rejects hosted policies, but we
        // enforce the invariant defensively here too.
        let selected_backend = if execution_context.backend_policy == "native_only" {
            "native"
        } else {
            return ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: format!(
                    "non-native backend policy '{}' is not supported",
                    execution_context.backend_policy
                ),
                run_id: None,
                metrics: ExecutorMetrics {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    ..Default::default()
                },
            };
        };

        // M012-G / M013-C-33: Collect call artifacts from completed calls,
        // including output digests from the ledger for integrity verification.
        let mut call_artifacts = Vec::new();
        for call in interpreter.completed_calls().values() {
            let preview = match &call.result.output {
                codegg_core::tool_program::ProgramValue::String(s) => {
                    s.chars().take(200).collect::<String>()
                }
                codegg_core::tool_program::ProgramValue::ToolResult(v) => {
                    v.to_string().chars().take(200).collect::<String>()
                }
                other => format!("{:?}", other).chars().take(200).collect::<String>(),
            };
            let artifact_id = call.result.artifacts.first().cloned();
            let (digest, absence_reason) = if let Some(handle) = artifact_id.as_deref() {
                let artifact = match canonical_artifact_store.get(handle).await {
                    Ok(Some(artifact)) => artifact,
                    Ok(None) => {
                        return ExecutorCompletion {
                            status: ExecutorStatus::Failed,
                            summary: format!("canonical call artifact is missing: {handle}"),
                            run_id: None,
                            metrics: ExecutorMetrics {
                                elapsed_ms: started.elapsed().as_millis() as u64,
                                ..Default::default()
                            },
                        };
                    }
                    Err(error) => {
                        return ExecutorCompletion {
                            status: ExecutorStatus::Failed,
                            summary: format!("canonical call artifact read failed: {error}"),
                            run_id: None,
                            metrics: ExecutorMetrics {
                                elapsed_ms: started.elapsed().as_millis() as u64,
                                ..Default::default()
                            },
                        };
                    }
                };
                let computed = crate::context::compute_content_hash(&artifact.redacted_content);
                if computed != artifact.content_hash {
                    return ExecutorCompletion {
                        status: ExecutorStatus::Failed,
                        summary: format!("canonical call artifact digest mismatch: {handle}"),
                        run_id: None,
                        metrics: ExecutorMetrics {
                            elapsed_ms: started.elapsed().as_millis() as u64,
                            ..Default::default()
                        },
                    };
                }
                (Some(format!("sha256:{computed}")), None)
            } else {
                (
                    None,
                    Some("nested call output remained below artifact threshold".into()),
                )
            };
            call_artifacts.push(crate::tool::tool_program_result::ProgramArtifactHandle {
                tool_name: Some(call.request.tool_name.clone()),
                preview,
                success: call.result.success,
                artifact_id,
                digest,
                absence_reason,
            });
        }

        // M013-C-35: Output artifact spill. When the serialized output exceeds
        // a threshold, write it to a file and replace the inline output with
        // a bounded preview. The output_artifact handle points to the file.
        let mut output_artifact: Option<String> = None;
        let mut result = result;
        if let Some(ref output) = result.output {
            let output_json = serde_json::to_vec(output).unwrap_or_default();
            const OUTPUT_SPILL_THRESHOLD: usize = 256 * 1024; // 256 KiB
            if output_json.len() > OUTPUT_SPILL_THRESHOLD {
                let session_id = execution_context
                    .session_id
                    .as_deref()
                    .unwrap_or("tool-program");
                let reference = match crate::tool::tool_program_result::persist_program_artifact(
                    canonical_artifact_store.clone(),
                    session_id,
                    &format!("{program_id}-output"),
                    "tool_program",
                    &output_json,
                )
                .await
                {
                    Ok(reference) => reference,
                    Err(error) => {
                        return ExecutorCompletion {
                            status: ExecutorStatus::Failed,
                            summary: format!(
                                "canonical output artifact persistence failed: {error}"
                            ),
                            run_id: None,
                            metrics: ExecutorMetrics {
                                elapsed_ms: started.elapsed().as_millis() as u64,
                                ..Default::default()
                            },
                        };
                    }
                };
                output_artifact = Some(reference.handle);
                let preview: String = String::from_utf8_lossy(
                    &output_json[..OUTPUT_SPILL_THRESHOLD.min(output_json.len())],
                )
                .chars()
                .take(1024)
                .collect();
                result.output = Some(codegg_core::tool_program::ProgramValue::String(format!(
                    "[output spilled to artifact — {} bytes] preview: {}",
                    output_json.len(),
                    preview
                )));
            }
        }

        // M013-C-34: Collect child artifacts from tracked child job results.
        let child_artifacts: Vec<crate::tool::tool_program_result::ChildArtifactHandle> =
            broker_adapter
                .take_child_results()
                .into_iter()
                .map(
                    |child| crate::tool::tool_program_result::ChildArtifactHandle {
                        job_id: child.job_id,
                        attempt_id: child.attempt_id,
                        run_id: child.run_id,
                        status: child.status,
                        artifact_id: child.artifact_id,
                        digest: child.artifact_digest,
                        absence_reason: child.absence_reason,
                    },
                )
                .collect();

        let result_record = match crate::tool::tool_program_result::ToolProgramResultStore::new(
            &ctx.workspace_root,
        )
        .persist(
            &program_id,
            ctx.attempt_id.as_str(),
            selected_backend,
            result.clone(),
            call_artifacts,
            child_artifacts,
            output_artifact,
        ) {
            Ok(record) => Some(record),
            Err(error) => {
                return ExecutorCompletion {
                    status: ExecutorStatus::Failed,
                    summary: format!("typed result persistence failed: {error}"),
                    run_id: None,
                    metrics: ExecutorMetrics {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        ..Default::default()
                    },
                };
            }
        };

        // Emit progress: completed
        let _ = ctx
            .progress
            .progress(ctx.job_id(), &format!("tool_program: {:?}", result.status))
            .await;

        // Map program result to executor completion
        let status = match result.status {
            ProgramStatus::Completed => ExecutorStatus::Completed,
            ProgramStatus::Failed => ExecutorStatus::Failed,
            ProgramStatus::Cancelled => ExecutorStatus::Cancelled,
            ProgramStatus::TimedOut => ExecutorStatus::TimedOut,
            ProgramStatus::Stalled => ExecutorStatus::TimedOut,
            ProgramStatus::Incomplete => ExecutorStatus::Failed,
            ProgramStatus::Recoverable => ExecutorStatus::Failed,
        };

        // Build summary
        let mut summary = format!(
            "status={:?} steps={} iterations={} bytes={} calls={}",
            result.status,
            result.steps_used,
            result.iterations_used,
            result.bytes_used,
            result.calls_completed
        );
        if let Some(ref output) = result.output {
            let _ = output;
            summary.push_str("\noutput_present=true");
        }
        if result.error_message.is_some() {
            summary.push_str("\nerror_present=true");
        }
        if let Some(ref class) = result.failure_class {
            summary.push_str(&format!("\nfailure_class: {}", class));
        }

        if execution_mode == "background" {
            if let (Some(service), Some(record)) = (&self.notification_service, result_record) {
                crate::test_failpoint::hit("tool_program_after_result_persist");
                if let Err(error) = service
                    .record_terminal_result(
                        &program_id,
                        ctx.job_id().as_str(),
                        execution_context.session_id.as_deref(),
                        execution_context.agent_id.as_deref(),
                        execution_context.turn_id.as_deref(),
                        &record,
                    )
                    .await
                {
                    return ExecutorCompletion {
                        status: ExecutorStatus::Failed,
                        summary: format!(
                            "terminal result committed but durable notification failed: {error}"
                        ),
                        run_id: None,
                        metrics: ExecutorMetrics {
                            elapsed_ms: started.elapsed().as_millis() as u64,
                            ..Default::default()
                        },
                    };
                }
            }
        }

        ExecutorCompletion {
            status,
            summary,
            run_id: None,
            metrics: ExecutorMetrics {
                cpu_time_ms: None,
                peak_memory_mb: None,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        IdempotencyClass, JobId, JobPayload, JobPriority, JobSource, JobState, ResourceRequest,
        RetryPolicy,
    };
    use codegg_core::workspace::WorkspaceId;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn sample_tool_program_job(program_id: &str, source_digest: &str) -> JobRecord {
        let now = chrono::Utc::now();
        let registry = crate::tool::ToolRegistry::with_defaults();
        let broker = crate::tool::ToolBroker::new(&registry);
        let tools = vec!["read".to_string()];
        let contracts =
            crate::tool::tool_program_context::resolve_contract_snapshot(&broker, &tools).unwrap();
        let contract_json =
            crate::tool::tool_program_context::canonical_contract_json(&contracts).unwrap();
        let contract_digest =
            crate::tool::tool_program_context::canonical_contract_digest(&contracts).unwrap();
        let execution_context = codegg_core::jobs::ToolProgramExecutionContext {
            workspace_path_policy_id: "ws-1".into(),
            principal_ref: Some("test-principal".into()),
            authority_ref: Some("test-decision".into()),
            policy_revision: Some("test-policy-v1".into()),
            path_policy_revision: Some("test-path-v1".into()),
            decision_outcome: Some("allowed".into()),
            caller_class: Some("agent".into()),
            maximum_effect_class: Some("read_only".into()),
            decision_issued_at: Some(chrono::Utc::now().timestamp_millis()),
            contract_snapshot_json: contract_json,
            ..codegg_core::jobs::ToolProgramExecutionContext::for_workspace("ws-1", "test")
        };
        let authority_digest = crate::tool::tool_program_context::authority_digest(
            &execution_context,
            &tools,
            source_digest,
        );
        let authority_grant = crate::tool::tool_program_context::build_authority_grant(
            Some(&execution_context),
            "ws-1",
            program_id,
            &tools,
            source_digest,
            "",
            &contract_digest,
        )
        .unwrap();
        let authority_grant_json = serde_json::to_string(&authority_grant).unwrap();
        JobRecord {
            job_id: JobId::new_unchecked("j-tp"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            session_id: None,
            turn_id: None,
            kind: JobKind::ToolProgram,
            source: JobSource::Interactive,
            priority: JobPriority::Normal,
            payload: JobPayload::ToolProgram {
                program_id: program_id.to_string(),
                invocation_key: "test-invocation".to_string(),
                source_digest: source_digest.to_string(),
                ir_digest: None,
                authority_digest,
                execution_context_json: Some(serde_json::to_string(&execution_context).unwrap()),
                submission_key: "key_123".to_string(),
                execution_mode: "foreground".to_string(),
                source_ref: Some(format!("{}.py", source_digest)),
                source_length: Some(0),
                allowed_tools: tools,
                authority_grant_json: Some(authority_grant_json),
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            state: JobState::Queued,
            current_attempt_id: None,
            attempt_count: 0,
            not_before: None,
            deadline: None,
            schedule_id: None,
            created_at: now,
            updated_at: now,
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            depends_on: vec![],
            labels: HashMap::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
        }
    }

    #[test]
    fn executor_kind_is_tool_program() {
        let exec = ToolProgramExecutor::default();
        assert_eq!(exec.kind(), ExecutorKind::ToolProgram);
    }

    #[test]
    fn supports_tool_program_kind() {
        let exec = ToolProgramExecutor::default();
        assert!(exec.supports(JobKind::ToolProgram));
        assert!(!exec.supports(JobKind::Python));
        assert!(!exec.supports(JobKind::Test));
    }

    #[test]
    fn validate_rejects_empty_program_id() {
        let exec = ToolProgramExecutor::default();
        let job = sample_tool_program_job("", "digest");
        assert!(exec.validate(&job).is_err());
    }

    #[test]
    fn validate_rejects_empty_source_digest() {
        let exec = ToolProgramExecutor::default();
        let job = sample_tool_program_job("prog_1", "");
        assert!(exec.validate(&job).is_err());
    }

    #[test]
    fn validate_rejects_empty_authority_digest() {
        let exec = ToolProgramExecutor::default();
        let mut job = sample_tool_program_job("prog_1", "digest");
        if let JobPayload::ToolProgram {
            ref mut authority_digest,
            ..
        } = job.payload
        {
            authority_digest.clear();
        }
        assert!(exec.validate(&job).is_err());
    }

    #[test]
    fn validate_accepts_valid_job() {
        let exec = ToolProgramExecutor::default();
        let job = sample_tool_program_job("prog_1", "digest_abc");
        assert!(exec.validate(&job).is_ok());
    }

    #[test]
    fn validate_rejects_wrong_payload() {
        let exec = ToolProgramExecutor::default();
        let now = chrono::Utc::now();
        let job = JobRecord {
            job_id: JobId::new_unchecked("j-test"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Normal,
            payload: JobPayload::Test {
                command: "echo ok".into(),
                argv: vec!["echo".into(), "ok".into()],
                cwd: None,
                scope: None,
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            state: JobState::Queued,
            current_attempt_id: None,
            attempt_count: 0,
            not_before: None,
            deadline: None,
            schedule_id: None,
            created_at: now,
            updated_at: now,
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            depends_on: vec![],
            labels: HashMap::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
        };
        assert!(exec.validate(&job).is_err());
    }

    #[tokio::test]
    async fn execute_fixture_program() {
        use crate::scheduler::executor::{JobExecutionContext, NoopProgressSink};
        use crate::scheduler::permit::ResourcePermitGuard;
        use codegg_core::jobs::AttemptId;
        use codegg_core::workspace::WorkspaceId;
        let workspace = tempfile::tempdir().unwrap();
        let fixture_source = "emit({\"status\": \"ok\", \"program_id\": \"test_prog\"})\n";
        let source_ref =
            crate::tool::tool_program_source::ToolProgramSourceStore::new(workspace.path())
                .persist(fixture_source)
                .unwrap();

        let exec = ToolProgramExecutor::default();
        let source = codegg_core::tool_program::ProgramStore::digest_source(fixture_source);

        let mut job = sample_tool_program_job("test_prog", &source);
        if let JobPayload::ToolProgram {
            source_ref: ref mut job_source_ref,
            source_length: ref mut job_source_length,
            ..
        } = job.payload
        {
            *job_source_ref = Some(source_ref.relative_path);
            *job_source_length = Some(source_ref.length);
        }

        let ctx = JobExecutionContext {
            job,
            attempt_id: AttemptId::new_unchecked("att-1"),
            daemon_generation: codegg_core::jobs::DaemonGeneration::new_unchecked("gen-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            workspace_root: workspace.path().to_path_buf(),
            cancellation: tokio_util::sync::CancellationToken::new(),
            progress: Arc::new(NoopProgressSink),
            resources: ResourcePermitGuard::new_orphan(Default::default()),
        };

        let result = exec.execute(ctx).await;
        assert_eq!(
            result.status,
            ExecutorStatus::Completed,
            "{}",
            result.summary
        );
        assert!(result.summary.contains("Completed"));
    }

    #[tokio::test]
    async fn execute_cancelled_program() {
        use crate::scheduler::executor::{JobExecutionContext, NoopProgressSink};
        use crate::scheduler::permit::ResourcePermitGuard;
        use codegg_core::jobs::AttemptId;
        use codegg_core::workspace::WorkspaceId;

        let exec = ToolProgramExecutor::default();
        let source = codegg_core::tool_program::ProgramStore::digest_source(
            "emit({\"status\": \"ok\", \"program_id\": \"test_prog\"})\n",
        );

        let job = sample_tool_program_job("test_prog", &source);

        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();

        let ctx = JobExecutionContext {
            job,
            attempt_id: AttemptId::new_unchecked("att-2"),
            daemon_generation: codegg_core::jobs::DaemonGeneration::new_unchecked("gen-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            workspace_root: std::env::current_dir().unwrap(),
            cancellation: token,
            progress: Arc::new(NoopProgressSink),
            resources: ResourcePermitGuard::new_orphan(Default::default()),
        };

        let result = exec.execute(ctx).await;
        assert_eq!(result.status, ExecutorStatus::Cancelled);
    }
}
