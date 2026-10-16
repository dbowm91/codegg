use async_trait::async_trait;
use codegg_core::jobs::ExecutionTarget;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::error::ToolError;
use crate::test_runner::{custom::validate_custom_command, TestRunRequest, TestScope};
use crate::tool::backend::{
    StructuredToolResult, ToolBackendKind, ToolExecutionContext, ToolProvenance, ToolTrust,
};
use crate::tool::{Tool, ToolCategory};

/// Run project tests through codegg's supervised test runner. Prefer this
/// over bash for cargo test, cargo nextest, pytest, uv run pytest, go test,
/// make test, and similar test commands. The tool streams full stdout/stderr
/// to logs, classifies timeouts/failures, and returns a compact report
/// instead of dumping full output into context.
pub struct TestTool {
    #[allow(dead_code)]
    run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
    submission: Option<Arc<crate::scheduler::JobSubmissionService>>,
    session_id: Option<String>,
    job_notifications: Option<Arc<crate::scheduler::job_notifications::JobNotificationService>>,
    /// The session's workspace root. This is the default test `workdir`, and
    /// the base a relative `workdir` resolves against.
    ///
    /// Without it the tool fell back to `std::env::current_dir()`, which in a
    /// daemon is the directory the daemon was launched from — so a session
    /// rooted at one repository ran that repository's tests inside whatever
    /// unrelated tree the user happened to start the daemon from.
    workspace_root: Option<PathBuf>,
}

impl TestTool {
    pub fn new() -> Self {
        Self {
            run_store: None,
            submission: None,
            session_id: None,
            job_notifications: None,
            workspace_root: None,
        }
    }

    pub fn with_run_store(store: Arc<dyn codegg_core::run_store::RunStore>) -> Self {
        Self {
            run_store: Some(store),
            submission: None,
            session_id: None,
            job_notifications: None,
            workspace_root: None,
        }
    }

    pub fn with_workspace_root(mut self, root: PathBuf) -> Self {
        self.workspace_root = Some(root);
        self
    }

    pub fn set_workspace_root(&mut self, root: PathBuf) {
        self.workspace_root = Some(root);
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_submission(
        mut self,
        submission: Arc<crate::scheduler::JobSubmissionService>,
    ) -> Self {
        self.submission = Some(submission);
        self
    }

    pub fn with_job_notifications(
        mut self,
        service: Arc<crate::scheduler::job_notifications::JobNotificationService>,
    ) -> Self {
        self.job_notifications = Some(service);
        self
    }
}

/// Execution mode for the `test` tool (M001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestExecutionMode {
    Foreground,
    Background,
}

impl TestExecutionMode {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "background" | "bg" => Self::Background,
            _ => Self::Foreground,
        }
    }
}

impl Default for TestTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TestTool {
    fn name(&self) -> &str {
        "test"
    }

    fn causal_contract(&self) -> Option<crate::tool_advisor::causal_frontier::ToolCausalContract> {
        crate::tool_advisor::causal_frontier::native_causal_contract("test")
    }

    fn description(&self) -> &str {
        "Run project tests through codegg's supervised test runner. Prefer this over bash for cargo test, cargo nextest, pytest, uv run pytest, go test, make test, and similar test commands. The tool streams full stdout/stderr to logs, classifies timeouts/failures, and returns a compact report instead of dumping full output into context. Set execution_mode=background to receive a stable JobHandle immediately and a terminal notification on completion without blocking; foreground (default) preserves the current synchronous report."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "scope": {
                    "type": "string",
                    "enum": ["auto", "workspace", "changed", "package", "file", "previous_failures", "custom"],
                    "description": "Test scope to run. Use auto for the default project test command."
                },
                "package": {
                    "type": "string",
                    "description": "Package/crate name for package scope."
                },
                "path": {
                    "type": "string",
                    "description": "File path for file scope."
                },
                "command": {
                    "type": "string",
                    "description": "Custom test command. Requires custom scope and must pass test command safety checks."
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for the test command. Defaults to the session workspace root. A relative path resolves against that root."
                },
                "timeout": {
                    "type": "number",
                    "description": "Wall-clock timeout in seconds. Default 300."
                },
                "stall_timeout": {
                    "type": "number",
                    "description": "No-output timeout in seconds. Default 120; set 0 to disable."
                },
                "max_report_bytes": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Hard ceiling for the formatted report returned to the model, in bytes. Default 20000. Useful for keeping model-visible output bounded when failure sets are large."
                },
                "execution_mode": {
                    "type": "string",
                    "enum": ["foreground", "background"],
                    "description": "Execution mode. 'foreground' (default) blocks until completion and returns the compact report. 'background' returns a stable JobHandle immediately; one terminal notification is delivered when the job finishes. No polling is needed."
                },
                "on_complete": {
                    "type": "string",
                    "enum": ["notify_only", "resume_owner"],
                    "description": "Background completion disposition (default notify_only). notify_only persists one durable notice for the next turn without spending model tokens. resume_owner additionally records a next-turn continuation marker and, only with an explicit host-trusted continuation grant, may start one bounded idle owner turn; without a grant idle dispatch stays notify-only. User cancel, expiry, or revocation wins before any invocation. Applies to background only."
                }
            },
            "required": ["scope"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let mode = input
            .get("execution_mode")
            .and_then(|v| v.as_str())
            .map(TestExecutionMode::from_str)
            .unwrap_or(TestExecutionMode::Foreground);
        if mode == TestExecutionMode::Background {
            // Legacy string path has no turn context; use the tool's bound
            // session if present. Fail without spawning otherwise.
            let handle = self.submit_background(input, None).await?;
            return Ok(serde_json::to_string_pretty(&handle).unwrap_or_default());
        }
        let request = parse_test_request_in(&input, self.workspace_root.as_deref())?;
        let max_report_bytes = request.max_report_bytes;
        let (summary, _) = self.run_scheduled_test(request).await?;
        Ok(match max_report_bytes {
            Some(max_report_bytes) => truncate_report_text(&summary, max_report_bytes),
            None => summary,
        })
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        use crate::live_execution_audit as live;
        let start = Instant::now();
        let mode = input
            .get("execution_mode")
            .and_then(|v| v.as_str())
            .map(TestExecutionMode::from_str)
            .unwrap_or(TestExecutionMode::Foreground);
        if mode == TestExecutionMode::Background {
            let value = self.submit_background(input.clone(), ctx.clone()).await?;
            let digest = live::command_digest_str(
                &serde_json::to_vec(&input)
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .unwrap_or_default(),
            );
            emit_test_audit(ctx.as_ref(), &digest, "submitted").await;
            let output = format!(
                "background test submitted as job {}; terminal notification will arrive without polling",
                value
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            );
            let provenance = ToolProvenance {
                backend: ToolBackendKind::Native.label().to_lowercase(),
                implementation: "test_runner".to_string(),
                version: None,
                elapsed_ms: Some(start.elapsed().as_millis() as u64),
                truncated: false,
                trust: ToolTrust::LocalTrusted,
            };
            return Ok(StructuredToolResult {
                output,
                value: Some(value),
                success: true,
                provenance: Some(provenance),
            });
        }
        let mut request = parse_test_request_in(&input, self.workspace_root.as_deref())?;
        request.session_id = request
            .session_id
            .or_else(|| ctx.as_ref().and_then(|context| context.session_id.clone()))
            .or_else(|| self.session_id.clone());
        let max_report_bytes = request.max_report_bytes;
        // Digest the normalized input: test selection (scope/package/
        // command) only, never output bodies.
        let digest = live::command_digest_str(
            &serde_json::to_vec(&input)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default(),
        );
        let (output, status) = match self.run_scheduled_test(request).await {
            Ok(pair) => pair,
            Err(ToolError::Timeout(message)) => {
                // M002: dispatched then reaped by the deadline — emit
                // the timeout outcome, then propagate.
                emit_test_audit(ctx.as_ref(), &digest, live::OUTCOME_TIMEOUT).await;
                return Err(ToolError::Timeout(message));
            }
            Err(other) => return Err(other),
        };
        let output = max_report_bytes
            .map(|cap| truncate_report_text(&output, cap))
            .unwrap_or(output);
        let elapsed_ms = start.elapsed().as_millis() as u64;

        // M002: one structural `command_execute` event (family `test`)
        // for the real scheduler-owned dispatch. Denied/pre-dispatch
        // failures return above without emitting.
        let terminal = match status {
            crate::scheduler::ExecutorStatus::Completed => live::OUTCOME_SUCCESS,
            crate::scheduler::ExecutorStatus::Failed => live::OUTCOME_FAILURE,
            crate::scheduler::ExecutorStatus::Cancelled => live::OUTCOME_CANCELLED,
            crate::scheduler::ExecutorStatus::TimedOut => live::OUTCOME_TIMEOUT,
            crate::scheduler::ExecutorStatus::Interrupted => live::OUTCOME_UNCERTAIN,
        };
        emit_test_audit(ctx.as_ref(), &digest, terminal).await;

        let success = !output.contains("FAILED") && !output.contains("error");
        let provenance = ToolProvenance {
            backend: ToolBackendKind::Native.label().to_lowercase(),
            implementation: "test_runner".to_string(),
            version: None,
            elapsed_ms: Some(elapsed_ms),
            truncated: false,
            trust: ToolTrust::LocalTrusted,
        };

        Ok(StructuredToolResult::with_provenance(
            output, success, provenance,
        ))
    }
}

impl TestTool {
    /// M001 background path: validate, submit durably, subscribe for terminal
    /// delivery, and return a stable `JobHandle` without blocking.
    ///
    /// The handle is returned only after both durable acceptance and durable
    /// subscription. Crash between submit and subscribe is healed by retry
    /// with the same submission key (same invocation returns the same job,
    /// then subscribes). Background requires a real host-originated session
    /// context; otherwise it fails without spawning.
    async fn submit_background(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let Some(submission) = self.submission.clone() else {
            return Err(ToolError::Execution(
                "test execution requires the daemon scheduler; standalone callers must provide a harness scheduler".into(),
            ));
        };
        let Some(notifications) = self.job_notifications.clone() else {
            return Err(ToolError::Execution(
                "background test requires the daemon job notification service".into(),
            ));
        };
        let session_id = ctx
            .as_ref()
            .and_then(|c| c.session_id.clone())
            .or_else(|| self.session_id.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ToolError::Execution(
                    "background test requires a real host-originated session context".into(),
                )
            })?;
        let turn_id = ctx.as_ref().and_then(|c| c.turn_id.clone());
        let invocation_key = ctx
            .as_ref()
            .and_then(|c| c.invocation_key.clone())
            .unwrap_or_else(|| format!("test-bg:{}", uuid::Uuid::new_v4()));
        let disposition = input
            .get("on_complete")
            .and_then(|v| v.as_str())
            .map(crate::scheduler::job_notifications::JobCompletionDisposition::from_str)
            .unwrap_or(crate::scheduler::job_notifications::JobCompletionDisposition::NotifyOnly);
        let owner_principal = ctx.as_ref().and_then(|c| {
            c.origin_principal
                .clone()
                .or_else(|| c.principal_identity.clone())
        });
        let mut request = parse_test_request_in(&input, self.workspace_root.as_deref())?;
        request.session_id = Some(session_id.clone());
        let resolved = crate::test_runner::resolve_test_command(&request)
            .map_err(|e| ToolError::Execution(format!("test command resolution failed: {e}")))?;
        let workspace_id = submission
            .workspace_id_for_root(&request.workdir)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let key_digest = crate::tool::tool_program_context::stable_digest(&invocation_key);
        let submission_key =
            crate::scheduler::submission::SubmissionKey::new(format!("test-bg:{key_digest}"))
                .map_err(|e| ToolError::Execution(format!("invalid submission key: {e}")))?;
        let submitted = submission
            .submit(
                Some(submission_key),
                codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: Some(session_id.clone()),
                    turn_id: turn_id.clone(),
                    kind: codegg_core::jobs::JobKind::Test,
                    source: codegg_core::jobs::JobSource::Interactive,
                    priority: codegg_core::jobs::JobPriority::Interactive,
                    payload: codegg_core::jobs::JobPayload::Test {
                        command: resolved.argv.join(" "),
                        argv: resolved.argv,
                        cwd: Some(resolved.cwd.to_string_lossy().into_owned()),
                        scope: Some(resolved.scope_label),
                        parent_run_id: None,
                    },
                    resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                        codegg_core::jobs::JobKind::Test,
                    ),
                    timeout: request.timeout_secs.map(std::time::Duration::from_secs),
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
                    not_before: None,
                    deadline: None,
                    schedule_id: None,
                    depends_on: Vec::new(),
                    parent_job_id: None,
                    parent_attempt_id: None,
                    parent_call_id: None,
                    parent_program_id: None,
                    parent_instruction_sequence: None,
                    relation_kind: None,
                    target: ExecutionTarget::default(),
                },
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        // Durable subscription before reporting acceptance. If the daemon
        // restarts after submit but before subscribe, retry with the same
        // invocation key returns the same job and completes the subscribe.
        notifications
            .subscribe_with_options(
                &submitted.job_id,
                "test",
                &session_id,
                turn_id.as_deref(),
                &invocation_key,
                disposition,
                owner_principal.as_deref(),
                Some(submitted.workspace_id.as_str()),
                None,
            )
            .await
            .map_err(|e| ToolError::Execution(format!("background subscribe failed: {e}")))?;
        let now = chrono::Utc::now().timestamp_millis();
        let handle = crate::scheduler::job_notifications::job_handle_for(
            &submitted.job_id,
            "test",
            "submitted",
            now,
            None,
        );
        Ok(serde_json::json!({
            "job_id": handle.job_id,
            "kind": handle.kind,
            "status": handle.status,
            "submitted_at": handle.submitted_at,
            "inspect_ref": handle.inspect_ref,
            "cancel_ref": handle.cancel_ref,
            "on_complete": disposition.as_str(),
            "message": "background test accepted; one terminal notification will arrive without polling",
        }))
    }

    async fn run_scheduled_test(
        &self,
        request: TestRunRequest,
    ) -> Result<(String, crate::scheduler::ExecutorStatus), ToolError> {
        let Some(submission) = self.submission.clone() else {
            return Err(ToolError::Execution(
                "test execution requires the daemon scheduler; standalone callers must provide a harness scheduler".into(),
            ));
        };
        let resolved = crate::test_runner::resolve_test_command(&request)
            .map_err(|e| ToolError::Execution(format!("test command resolution failed: {e}")))?;
        let workspace_id = submission
            .workspace_id_for_root(&request.workdir)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let job = submission
            .submit(
                None,
                codegg_core::jobs::NewJob {
                    workspace_id,
                    session_id: request
                        .session_id
                        .clone()
                        .or_else(|| self.session_id.clone()),
                    turn_id: None,
                    kind: codegg_core::jobs::JobKind::Test,
                    source: codegg_core::jobs::JobSource::Interactive,
                    priority: codegg_core::jobs::JobPriority::Interactive,
                    payload: codegg_core::jobs::JobPayload::Test {
                        command: resolved.argv.join(" "),
                        argv: resolved.argv,
                        cwd: Some(resolved.cwd.to_string_lossy().into_owned()),
                        scope: Some(resolved.scope_label),
                        parent_run_id: None,
                    },
                    resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                        codegg_core::jobs::JobKind::Test,
                    ),
                    timeout: request.timeout_secs.map(std::time::Duration::from_secs),
                    retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                    idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
                    not_before: None,
                    deadline: None,
                    schedule_id: None,
                    depends_on: Vec::new(),
                    parent_job_id: None,
                    parent_attempt_id: None,
                    parent_call_id: None,
                    parent_program_id: None,
                    parent_instruction_sequence: None,
                    relation_kind: None,
                    target: ExecutionTarget::default(),
                },
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let wait_for = request
            .timeout_secs
            .map(|seconds| std::time::Duration::from_secs(seconds.saturating_add(5)))
            .unwrap_or_else(|| std::time::Duration::from_secs(900));
        let completion = submission
            .scheduler()
            .wait_for_completion(&job.job_id, wait_for)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let status = completion.status;
        let summary = completion.summary;
        let completion_status = match completion.status {
            crate::scheduler::ExecutorStatus::Completed => "passed",
            crate::scheduler::ExecutorStatus::Failed => "failed",
            crate::scheduler::ExecutorStatus::Cancelled => "cancelled",
            crate::scheduler::ExecutorStatus::TimedOut => "timed_out",
            crate::scheduler::ExecutorStatus::Interrupted => "unknown",
        };
        let marker = serde_json::json!({
            "job_id": job.job_id.as_str(),
            "kind": "test",
            "status": completion_status,
            "summary": summary,
        });
        Ok((
            format!(
                "{}\nCODEGG_VALIDATION_RESULT:{}",
                marker["summary"].as_str().unwrap_or_default(),
                marker
            ),
            status,
        ))
    }
}

/// M002: emit one structural `command_execute` event (family `test`)
/// for a real scheduler-owned test dispatch, or stay silent without
/// the threaded audit pair.
async fn emit_test_audit(ctx: Option<&ToolExecutionContext>, digest_hex: &str, outcome: &str) {
    let Some(ctx) = ctx else { return };
    let Some((audit, emitter)) = ctx.live_audit_hook() else {
        return;
    };
    let scope = crate::live_execution_audit::invocation_scope(
        ctx.invocation_key.as_deref(),
        digest_hex,
        outcome,
    );
    crate::live_execution_audit::emit_command_execute(
        emitter,
        audit,
        digest_hex,
        "test",
        outcome,
        &scope,
        None,
        Some("test"),
        None,
        None,
        None,
    )
    .await;
}

fn truncate_report_text(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_string();
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[report truncated]", &text[..end])
}

/// Parse JSON input into a `TestRunRequest` with a synthetic workspace root.
///
/// Production callers must use [`parse_test_request_in`] with the real root.
/// This wrapper gives the scope/timeout parser tests a root so they exercise
/// the production shape; the rootless refusal is pinned separately by
/// `a_rootless_parse_refuses_rather_than_inventing_a_workdir`.
#[cfg(test)]
fn parse_test_request(input: &serde_json::Value) -> Result<TestRunRequest, ToolError> {
    static FIXTURE_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let root = FIXTURE_ROOT.get_or_init(std::env::temp_dir);
    parse_test_request_in(input, Some(root.as_path()))
}

/// Parse JSON input, resolving `workdir` against `workspace_root`.
///
/// An omitted `workdir` defaults to the workspace root, and a relative
/// `workdir` resolves against it. Neither falls back to the process working
/// directory, which in a daemon is unrelated to the session.
fn parse_test_request_in(
    input: &serde_json::Value,
    workspace_root: Option<&std::path::Path>,
) -> Result<TestRunRequest, ToolError> {
    let scope_str = input["scope"]
        .as_str()
        .ok_or_else(|| ToolError::Execution("missing required 'scope' parameter".to_string()))?;

    let scope = match scope_str {
        "auto" => TestScope::Auto,
        "workspace" => TestScope::Workspace,
        "changed" => TestScope::Changed,
        "package" => {
            let pkg = input["package"].as_str().ok_or_else(|| {
                ToolError::Execution("package scope requires 'package' parameter".to_string())
            })?;
            if pkg.trim().is_empty() {
                return Err(ToolError::Execution(
                    "package parameter must not be empty".to_string(),
                ));
            }
            TestScope::Package(pkg.to_string())
        }
        "file" => {
            let path = input["path"].as_str().ok_or_else(|| {
                ToolError::Execution("file scope requires 'path' parameter".to_string())
            })?;
            if path.trim().is_empty() {
                return Err(ToolError::Execution(
                    "path parameter must not be empty".to_string(),
                ));
            }
            TestScope::File(PathBuf::from(path))
        }
        "previous_failures" => TestScope::PreviousFailures,
        "custom" => {
            let cmd = input["command"].as_str().ok_or_else(|| {
                ToolError::Execution("custom scope requires 'command' parameter".to_string())
            })?;
            if validate_custom_command(cmd).is_err() {
                return Err(ToolError::Execution(format!(
                    "custom command rejected by safety validator: '{cmd}'. \
                     Allowed argv prefixes: cargo test, cargo nextest, pytest, \
                     uv run pytest, go test, zig build test, make test, make check, \
                     npm test, pnpm test, yarn test, bun test. \
                     Shell metacharacters, redirection, pipes, command substitution, \
                     and newlines are not allowed."
                )));
            }
            TestScope::CustomCommand(cmd.to_string())
        }
        other => {
            return Err(ToolError::Execution(format!(
                "unknown scope '{other}'. Valid scopes: auto, workspace, changed, package, file, previous_failures, custom"
            )));
        }
    };

    // Default to the session's workspace root, never the process cwd. A
    // relative `workdir` resolves against that same root under the same
    // containment rule every file path obeys, because `Command::current_dir`
    // would otherwise have the OS resolve it against the daemon's cwd.
    //
    // Both branches go through the shared resolver so an omitted and an
    // explicit root produce the *same* canonical spelling. Two spellings of
    // one directory would let the job's derived `workspace_id` differ between
    // two calls that mean the same thing.
    let workdir = match (input["workdir"].as_str(), workspace_root) {
        (Some(raw), Some(root)) => crate::tool::util::resolve_workdir(Path::new(raw), root)?,
        (Some(raw), None) => PathBuf::from(raw),
        (None, Some(root)) => crate::tool::util::resolve_workdir(root, root)?,
        // No workspace authority: refuse rather than guess a directory.
        // Guessing here is what put tests in the daemon's launch directory.
        (None, None) => {
            return Err(ToolError::Execution(
                "no workspace root is bound to this tool; pass 'workdir' explicitly".to_string(),
            ))
        }
    };

    let timeout_secs = parse_positive_seconds(input.get("timeout"), "timeout")?;
    let stall_timeout_secs = parse_positive_seconds(input.get("stall_timeout"), "stall_timeout")?;

    let max_report_bytes = match input.get("max_report_bytes") {
        Some(v) => {
            let n = v.as_u64().ok_or_else(|| {
                ToolError::Execution("max_report_bytes must be a positive integer".to_string())
            })?;
            if n == 0 {
                return Err(ToolError::Execution(
                    "max_report_bytes must be greater than 0".to_string(),
                ));
            }
            Some(n as usize)
        }
        None => None,
    };

    Ok(TestRunRequest {
        scope,
        workdir,
        execution_cwd: None,
        timeout_secs,
        stall_timeout_secs,
        max_report_bytes,
        session_id: None,
        parent_run_id: None,
        cancellation: None,
    })
}

/// Parse a positive-integer seconds value from an optional JSON parameter.
///
/// Returns `Ok(None)` when `value` is `None`, `Ok(Some(n))` when `n >= 1`,
/// and `Err` when the value is present but invalid (non-number, fractional,
/// negative, zero, or NaN). This matches the validation policy used for
/// `max_report_bytes` so the model gets a clear error rather than a silent
/// fall-through to defaults.
fn parse_positive_seconds(
    value: Option<&serde_json::Value>,
    field: &str,
) -> Result<Option<u64>, ToolError> {
    let Some(v) = value else {
        return Ok(None);
    };
    let n = v.as_f64().ok_or_else(|| {
        ToolError::Execution(format!(
            "{field} must be a positive integer number of seconds"
        ))
    })?;
    if !n.is_finite() || n <= 0.0 || n.fract() != 0.0 {
        return Err(ToolError::Execution(format!(
            "{field} must be a positive integer number of seconds"
        )));
    }
    Ok(Some(n as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_tool_rejects_missing_scope() {
        let tool = TestTool::default();
        let input = json!({});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err}").contains("missing required 'scope'"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_tool_rejects_package_scope_without_package() {
        let tool = TestTool::default();
        let input = json!({"scope": "package"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err}").contains("package scope requires 'package'"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_tool_rejects_file_scope_without_path() {
        let tool = TestTool::default();
        let input = json!({"scope": "file"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err}").contains("file scope requires 'path'"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_tool_rejects_custom_scope_without_command() {
        let tool = TestTool::default();
        let input = json!({"scope": "custom"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err}").contains("custom scope requires 'command'"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_tool_rejects_unknown_scope() {
        let tool = TestTool::default();
        let input = json!({"scope": "invalid"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err}").contains("unknown scope 'invalid'"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_tool_rejects_custom_command_not_in_allowlist() {
        let tool = TestTool::default();
        let input = json!({"scope": "custom", "command": "rm -rf /"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("rejected by safety validator"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn test_tool_accepts_custom_command_in_allowlist() {
        let tool = TestTool::default();
        let input = json!({"scope": "custom", "command": "cargo test --lib", "workdir": "/tmp"});
        let result = tool.execute(input).await;
        // Validation should pass (so the error is NOT a validator rejection).
        // Actual execution may or may not succeed depending on the test environment,
        // so we only assert that we got past the validator.
        if let Err(ref err) = result {
            let msg = format!("{err}");
            assert!(
                !msg.contains("rejected by safety validator"),
                "should not be a validator error: {msg}"
            );
        }
    }

    #[tokio::test]
    async fn test_tool_rejects_empty_package() {
        let tool = TestTool::default();
        let input = json!({"scope": "package", "package": "  "});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err}").contains("must not be empty"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_tool_rejects_empty_path() {
        let tool = TestTool::default();
        let input = json!({"scope": "file", "path": "  "});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err}").contains("must not be empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_tool_category_is_shell_exec() {
        let tool = TestTool::default();
        assert_eq!(tool.category(), ToolCategory::ShellExec);
    }

    #[test]
    fn test_tool_registered_in_default_registry() {
        let registry = crate::tool::ToolRegistry::with_defaults();
        assert!(
            registry.get("test").is_some(),
            "test tool not found in default registry"
        );
    }

    #[test]
    fn test_tool_name_and_description() {
        let tool = TestTool::default();
        assert_eq!(tool.name(), "test");
        assert!(tool.description().contains("supervised test runner"));
    }

    #[test]
    fn test_tool_parameters_schema() {
        let tool = TestTool::default();
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["scope"].is_object());
        assert!(params["required"]
            .as_array()
            .unwrap()
            .contains(&json!("scope")));
    }

    #[test]
    fn test_parse_request_auto_scope() {
        let input = json!({"scope": "auto"});
        let req = parse_test_request(&input).unwrap();
        assert_eq!(req.scope, TestScope::Auto);
    }

    #[test]
    fn test_parse_request_workspace_scope() {
        let input = json!({"scope": "workspace"});
        let req = parse_test_request(&input).unwrap();
        assert_eq!(req.scope, TestScope::Workspace);
    }

    #[test]
    fn test_parse_request_package_scope() {
        let input = json!({"scope": "package", "package": "my-crate"});
        let req = parse_test_request(&input).unwrap();
        assert_eq!(req.scope, TestScope::Package("my-crate".to_string()));
    }

    #[test]
    fn test_parse_request_file_scope() {
        let input = json!({"scope": "file", "path": "tests/foo.rs"});
        let req = parse_test_request(&input).unwrap();
        assert_eq!(req.scope, TestScope::File(PathBuf::from("tests/foo.rs")));
    }

    #[test]
    fn test_parse_request_custom_scope() {
        let input = json!({"scope": "custom", "command": "cargo test --lib"});
        let req = parse_test_request(&input).unwrap();
        assert_eq!(
            req.scope,
            TestScope::CustomCommand("cargo test --lib".to_string())
        );
    }

    #[test]
    fn test_parse_request_previous_failures() {
        let input = json!({"scope": "previous_failures"});
        let req = parse_test_request(&input).unwrap();
        assert_eq!(req.scope, TestScope::PreviousFailures);
    }

    /// An explicit absolute `workdir` is honored when it is inside the
    /// workspace.
    ///
    /// This test previously accepted `/some/dir` — a path outside any
    /// workspace — because the tool had no containment at all. It is now
    /// refused, which is the intended change: `bash` already rejects a
    /// `workdir` outside `allowed_paths`, and the job's `workspace_id` is
    /// derived from this very directory, so an out-of-workspace test run would
    /// register a workspace the session does not own.
    #[test]
    fn an_explicit_workdir_inside_the_workspace_is_honored() {
        let root = tempfile::tempdir().expect("workspace fixture");
        let nested = root.path().join("crates/inner");
        std::fs::create_dir_all(&nested).expect("nested fixture dir");

        let input = json!({"scope": "auto", "workdir": nested.to_string_lossy()});
        let req = parse_test_request_in(&input, Some(root.path()))
            .expect("an in-workspace workdir must be accepted");
        assert_eq!(
            req.workdir,
            nested.canonicalize().expect("canonical nested")
        );
    }

    #[test]
    fn an_explicit_workdir_outside_the_workspace_is_refused() {
        let root = tempfile::tempdir().expect("workspace fixture");
        let outside = tempfile::tempdir().expect("outside fixture");

        let input = json!({"scope": "auto", "workdir": outside.path().to_string_lossy()});
        let error = parse_test_request_in(&input, Some(root.path()))
            .expect_err("a workdir outside the workspace must be refused");
        assert!(
            matches!(error, ToolError::Permission(_)),
            "expected a permission error, got {error:?}"
        );
    }

    /// Regression: the `test` tool had no workspace authority at all, so an
    /// omitted `workdir` defaulted to the process cwd. Under the daemon that
    /// is the directory the daemon was launched from, so a session's tests ran
    /// in an unrelated tree.
    #[test]
    fn an_omitted_workdir_defaults_to_the_workspace_root() {
        let root = tempfile::tempdir().expect("workspace fixture");
        let input = json!({"scope": "auto"});
        let req = parse_test_request_in(&input, Some(root.path())).unwrap();
        assert_eq!(
            req.workdir,
            root.path().canonicalize().expect("canonical root"),
            "tests must default to the session workspace root"
        );
    }

    /// A relative `workdir` has to resolve against the same root. Left
    /// relative it would be handed to `Command::current_dir`, and the OS
    /// resolves that against the process cwd.
    ///
    /// The comparison is against the *canonicalized* root: on macOS a tempdir
    /// lives under `/var/...`, which is itself a symlink to `/private/var/...`,
    /// so a lexical comparison would fail on a correct resolution.
    #[test]
    fn a_relative_workdir_resolves_against_the_workspace_root() {
        let root = tempfile::tempdir().expect("workspace fixture");
        let input = json!({"scope": "auto", "workdir": "crates/inner"});
        let req = parse_test_request_in(&input, Some(root.path())).unwrap();
        assert_eq!(
            req.workdir,
            root.path()
                .canonicalize()
                .expect("canonical root")
                .join("crates/inner")
        );
    }

    /// A relative workdir must not be able to climb out of the workspace, and
    /// it must be caught by containment rather than by a lexical prefix test
    /// that a trailing `..` defeats.
    #[test]
    fn a_relative_workdir_cannot_traverse_out_of_the_workspace_root() {
        let root = tempfile::tempdir().expect("workspace fixture");
        let input = json!({"scope": "auto", "workdir": "../elsewhere"});

        let error = parse_test_request_in(&input, Some(root.path()))
            .expect_err("traversal out of the workspace must be refused");
        assert!(
            matches!(error, ToolError::Permission(_)),
            "a workdir outside the workspace is a permission error, not a path \
             error; got {error:?}"
        );

        // A legitimate nested directory still resolves inside the root.
        let nested = json!({"scope": "auto", "workdir": "crates/inner"});
        let req = parse_test_request_in(&nested, Some(root.path())).expect("nested workdir");
        assert!(
            req.workdir
                .starts_with(root.path().canonicalize().expect("canonical root")),
            "a nested workdir stays inside the workspace; got {}",
            req.workdir.display()
        );
    }

    /// With no workspace authority the tool refuses rather than inventing a
    /// directory. Guessing is precisely the behavior being removed.
    #[test]
    fn a_rootless_parse_refuses_rather_than_inventing_a_workdir() {
        let input = json!({"scope": "auto"});
        let error = parse_test_request_in(&input, None)
            .expect_err("a rootless parse must not pick a directory for the caller");
        assert!(
            error.to_string().contains("no workspace root"),
            "the error must say the workdir needs an explicit value; got {error}"
        );

        // An explicit workdir is still accepted without a root.
        let explicit = json!({"scope": "auto", "workdir": "/some/dir"});
        assert_eq!(
            parse_test_request_in(&explicit, None)
                .expect("an explicit workdir needs no workspace authority")
                .workdir,
            PathBuf::from("/some/dir")
        );
    }

    /// The registry must hand the tool its workspace root. Before this the
    /// tool was registered with run store, submission, and session id only,
    /// so it had no root to default to.
    #[test]
    fn the_registered_test_tool_carries_a_workspace_root() {
        let root = tempfile::tempdir().expect("workspace fixture");
        let registry = crate::tool::ToolRegistry::with_options(crate::tool::ToolRegistryOptions {
            git_read_only: None,
            workspace_root: Some(root.path().to_path_buf()),
            ..crate::tool::ToolRegistryOptions::default()
        });
        assert!(
            registry.get("test").is_some(),
            "test must be registered so the assertion is meaningful"
        );

        let tool =
            crate::tool::test::TestTool::new().with_workspace_root(root.path().to_path_buf());
        let req = parse_test_request_in(&json!({"scope": "auto"}), tool.workspace_root.as_deref())
            .expect("a rooted tool parses");
        assert_eq!(
            req.workdir,
            root.path().canonicalize().expect("canonical root")
        );
    }

    #[test]
    fn test_parse_request_timeout() {
        let input = json!({"scope": "auto", "timeout": 60, "stall_timeout": 30});
        let req = parse_test_request(&input).unwrap();
        assert_eq!(req.timeout_secs, Some(60));
        assert_eq!(req.stall_timeout_secs, Some(30));
    }

    #[test]
    fn test_parse_request_rejects_negative_timeout() {
        let input = json!({"scope": "auto", "timeout": -1});
        let err = parse_test_request(&input).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("timeout must be a positive integer"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_parse_request_rejects_zero_timeout() {
        let input = json!({"scope": "auto", "timeout": 0});
        let err = parse_test_request(&input).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("timeout must be a positive integer"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_parse_request_rejects_fractional_timeout() {
        let input = json!({"scope": "auto", "timeout": 0.5});
        let err = parse_test_request(&input).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("timeout must be a positive integer"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_parse_request_rejects_string_timeout() {
        let input = json!({"scope": "auto", "timeout": "60"});
        let err = parse_test_request(&input).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("timeout must be a positive integer"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_parse_request_rejects_invalid_stall_timeout() {
        let input = json!({"scope": "auto", "stall_timeout": -1});
        let err = parse_test_request(&input).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("stall_timeout must be a positive integer"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_tool_exposes_in_definitions() {
        let tool = TestTool::default();
        assert!(tool.expose_in_definitions());
    }

    #[test]
    fn test_tool_custom_rejects_semicolon_suffix() {
        // Bypass regression: an allowlisted prefix followed by `;` must NOT pass.
        let cmd = validate_custom_command_for_tool("cargo test; rm -rf target/tmp");
        assert!(cmd.is_err(), "semicolon suffix should be rejected");
    }

    #[test]
    fn test_tool_custom_rejects_pipe_suffix() {
        let cmd = validate_custom_command_for_tool("pytest | tee /tmp/out");
        assert!(cmd.is_err(), "pipe suffix should be rejected");
    }

    #[test]
    fn test_tool_custom_rejects_command_substitution() {
        let cmd = validate_custom_command_for_tool("pytest $(some-command)");
        assert!(cmd.is_err(), "command substitution should be rejected");
        let cmd = validate_custom_command_for_tool("pytest `some-command`");
        assert!(cmd.is_err(), "backtick substitution should be rejected");
    }

    #[test]
    fn test_tool_custom_rejects_newline_suffix() {
        let cmd = validate_custom_command_for_tool("cargo test\nrm -rf /");
        assert!(cmd.is_err(), "newline suffix should be rejected");
    }

    #[test]
    fn test_tool_custom_rejects_prefix_collision() {
        let cmd = validate_custom_command_for_tool("pytestevil");
        assert!(cmd.is_err(), "prefix collision should be rejected");
    }

    #[test]
    fn test_tool_custom_accepts_normal_cargo_test_args() {
        let cmd = validate_custom_command_for_tool("cargo test --lib");
        assert!(cmd.is_ok(), "normal cargo test args should be accepted");
        let cmd = validate_custom_command_for_tool("cargo test -p codegg-core");
        assert!(cmd.is_ok(), "package flag should be accepted");
    }

    /// Helper that runs the strict custom-command validator the same
    /// way `parse_test_request` does, but without spinning up a full
    /// test execution. Used by tool-layer bypass regression tests.
    fn validate_custom_command_for_tool(
        cmd: &str,
    ) -> Result<(), crate::test_runner::custom::CustomCommandValidationError> {
        crate::test_runner::custom::validate_custom_command(cmd).map(|_| ())
    }
}
