use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::scheduler::submission::{JobSubmissionService, SubmissionKey};
use crate::tool::{Tool, ToolCategory};

use super::projection::project_python_run;
use super::types::{PythonExecutionMode, PythonRunResult, PythonScriptRequest};

/// Result of a delegated Python run. The `run_id` is `Some` iff a `RunStore`
/// record was successfully begun — it is the proof of delegated ownership.
#[derive(Debug, Clone)]
pub struct DelegatedPythonRun {
    pub result: PythonRunResult,
    pub run_id: Option<codegg_core::run_store::RunId>,
}

impl DelegatedPythonRun {
    pub fn into_result(self) -> PythonRunResult {
        self.result
    }
    pub fn result(&self) -> &PythonRunResult {
        &self.result
    }
}

/// Execute a Python script and persist the run to the canonical RunStore.
///
/// This is the single entry point for canonical Python delegation — both
/// the model-facing `PythonScriptTool` and `BashTool`'s active routing
/// dispatcher must use this function so that Python ownership is real,
/// not a label applied after direct `python3 -c` execution.
///
/// Returns the run result plus an optional `RunId` proving that the
/// delegated record was begun. `run_id` is `None` only when `run_store`
/// is `None` or when `begin_run` failed (logged but non-fatal).
#[cfg(test)]
pub async fn execute_and_persist_python_script(
    request: &PythonScriptRequest,
    run_store: Option<&Arc<dyn codegg_core::run_store::RunStore>>,
) -> DelegatedPythonRun {
    let result = super::executor::execute_python_script(request).await;
    let run_id = if let Some(store) = run_store {
        persist_python_run(store, request, &result).await
    } else {
        None
    };
    DelegatedPythonRun { result, run_id }
}

/// Build a `RunDraft` from a Python request and result. Used by both the
/// executor (which calls `begin_run` before execution and `complete_run`
/// after) and the legacy `persist_python_run` helper.
fn build_python_run_draft(
    request: &PythonScriptRequest,
    result: &PythonRunResult,
) -> codegg_core::run_store::RunDraft {
    use codegg_core::run_store::*;

    let cwd = request.cwd.clone();
    let workspace_root = request
        .workspace_root
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| cwd.clone());

    RunDraft {
        kind: RunKind::Python,
        invocation: RunInvocation {
            command: "python3".to_string(),
            argv: Some(vec!["python3".to_string(), "<script>".to_string()]),
            script_hash: result.script_body_hash.clone(),
        },
        session_id: request.session_id.clone(),
        parent_run_id: None,
        workspace_root,
        cwd,
        backend: BackendRecord {
            family: "python_script".to_string(),
            detail: Some(format!("{:?}", request.mode)),
        },
        risk: RiskRecord {
            level: format!("{:?}", result.risk.level),
            has_subprocess: result.capabilities.subprocess,
            has_git_mutation: false,
            has_destructive_mutation: result.capabilities.destructive_fs,
        },
        planned_backend: Some(PlannedBackend::PythonScript),
        actual_backend: Some(ActualBackend::PythonScript),
        ownership: RunOwnership::DelegatedBackend,
        asset_provenance: None,
    }
}

/// Write artifacts (stdout, stderr, diff) to an active RunStore record.
pub(crate) async fn write_python_run_artifacts(
    store: &Arc<dyn codegg_core::run_store::RunStore>,
    handle: &codegg_core::run_store::RunHandle,
    result: &PythonRunResult,
) {
    use codegg_core::run_store::*;

    if !result.stdout.is_empty() {
        let _ = store
            .write_artifact(
                handle,
                ArtifactInput {
                    kind: ArtifactKind::Stdout,
                    data: result.stdout.as_bytes().to_vec(),
                    mime_type: "text/plain".to_string(),
                    safe_for_model: false,
                },
            )
            .await;
    }

    if !result.stderr.is_empty() {
        let _ = store
            .write_artifact(
                handle,
                ArtifactInput {
                    kind: ArtifactKind::Stderr,
                    data: result.stderr.as_bytes().to_vec(),
                    mime_type: "text/plain".to_string(),
                    safe_for_model: false,
                },
            )
            .await;
    }

    if let Some(ref diff) = result.diff {
        let _ = store
            .write_artifact(
                handle,
                ArtifactInput {
                    kind: ArtifactKind::UnifiedDiff,
                    data: diff.as_bytes().to_vec(),
                    mime_type: "text/plain".to_string(),
                    safe_for_model: false,
                },
            )
            .await;
    }
}

/// Complete a Python run in the RunStore with the final status and metadata.
pub(crate) async fn complete_python_run(
    store: &Arc<dyn codegg_core::run_store::RunStore>,
    handle: codegg_core::run_store::RunHandle,
    result: &PythonRunResult,
) -> Option<codegg_core::run_store::RunId> {
    use chrono::Utc;
    use codegg_core::run_store::*;

    let status = match &result.status {
        super::types::PythonRunStatus::Success => RunStatus::Complete,
        super::types::PythonRunStatus::Failed(_) => RunStatus::Failed,
        super::types::PythonRunStatus::TimedOut => RunStatus::TimedOut,
        super::types::PythonRunStatus::SpawnError => RunStatus::Failed,
    };

    let _ = store
        .complete_run(
            handle.clone(),
            RunCompletion {
                status,
                completed_at: Utc::now(),
                permissions: vec![],
                sandbox: Some(SandboxRecord {
                    os_isolation: result.os_filesystem_isolation,
                    network_isolation: result.os_network_isolation,
                    read_roots: result.effective_read_roots.clone(),
                    write_roots: result.effective_write_roots.clone(),
                }),
                projection: None,
                changes: result
                    .changed_files
                    .iter()
                    .map(|p| ChangedPathRecord {
                        path: p.clone(),
                        kind: "modified".to_string(),
                    })
                    .collect(),
                rerun: None,
                actual_backend: Some(ActualBackend::PythonScript),
                fallback: None,
            },
        )
        .await;

    Some(handle.run_id)
}

/// Begin a Python run in the RunStore. Returns a handle for subsequent
/// artifact writes and completion. The run is visible as "active" immediately.
pub async fn begin_python_run(
    store: &Arc<dyn codegg_core::run_store::RunStore>,
    request: &PythonScriptRequest,
    result: &PythonRunResult,
) -> Option<codegg_core::run_store::RunHandle> {
    let draft = build_python_run_draft(request, result);
    match store.begin_run(draft).await {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::warn!("python script: failed to begin RunStore run: {e}");
            None
        }
    }
}

/// Persist a completed Python run to the RunStore. Best-effort; errors
/// are logged but do not propagate. Returns the `RunId` if the run was
/// successfully begun — callers use this as proof of delegated ownership.
///
/// This is the legacy combined helper. For executor-owned runs, prefer
/// `begin_python_run` + `write_python_run_artifacts` + `complete_python_run`.
pub async fn persist_python_run(
    store: &Arc<dyn codegg_core::run_store::RunStore>,
    request: &PythonScriptRequest,
    result: &PythonRunResult,
) -> Option<codegg_core::run_store::RunId> {
    let handle = begin_python_run(store, request, result).await?;
    write_python_run_artifacts(store, &handle, result).await;
    complete_python_run(store, handle, result).await
}

/// Model-facing tool for executing Python scripts with safety analysis.
///
/// PythonScriptTool materializes scripts to temp files, runs static risk
/// analysis, captures changed files for Transform mode, and projects
/// results safely. Use this instead of bash for Python one-off scripts,
/// analysis, bulk transforms, and custom verification.
pub struct PythonScriptTool {
    run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
    submission_service: Option<Arc<JobSubmissionService>>,
}

impl PythonScriptTool {
    pub fn new() -> Self {
        Self {
            run_store: None,
            submission_service: None,
        }
    }

    pub fn with_run_store(store: Arc<dyn codegg_core::run_store::RunStore>) -> Self {
        Self {
            run_store: Some(store),
            submission_service: None,
        }
    }

    pub fn with_submission_service(mut self, service: Arc<JobSubmissionService>) -> Self {
        self.submission_service = Some(service);
        self
    }

    pub fn with_scheduler(
        run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
        submission_service: Option<Arc<JobSubmissionService>>,
    ) -> Self {
        Self {
            run_store,
            submission_service,
        }
    }
}

impl Default for PythonScriptTool {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonScriptTool {
    /// Execute Python through the scheduler. Creates a durable job, waits
    /// for completion, and returns the projected result.
    async fn execute_via_scheduler(
        &self,
        request: &PythonScriptRequest,
        submission: &Arc<JobSubmissionService>,
    ) -> Result<String, ToolError> {
        use codegg_core::jobs::{
            DaemonGeneration, IdempotencyClass, JobKind, JobPayload, JobPriority, JobSource,
            NewJob, RetryPolicy,
        };

        let workspace_root = request
            .workspace_root
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| request.cwd.clone());

        let workspace_id = submission
            .workspace_id_for_root(&workspace_root)
            .await
            .map_err(|e| ToolError::Execution(format!("workspace registration failed: {e}")))?;

        let source_hash = crate::python_script::source_store::compute_digest(&request.code);

        let timeout = std::time::Duration::from_secs(request.timeout_secs.unwrap_or(60));

        // Idempotency: read-only modes are safe to repeat; transform is not
        let idempotency = match request.mode {
            PythonExecutionMode::Analyze | PythonExecutionMode::Verify => {
                IdempotencyClass::SafeRepeat
            }
            PythonExecutionMode::Transform => IdempotencyClass::NonIdempotent,
        };

        let payload = JobPayload::Python {
            script_path: String::new(),
            args: vec![],
            mode: request.mode.to_string(),
            source: Some(request.code.clone()),
            source_hash: Some(source_hash.clone()),
            cwd: Some(request.cwd.to_string_lossy().to_string()),
            timeout_secs: request.timeout_secs,
        };

        let mut labels = std::collections::HashMap::new();
        labels.insert(
            "workspace_root".to_string(),
            workspace_root.to_string_lossy().to_string(),
        );
        if let Some(ref intent) = request.intent {
            labels.insert("intent".to_string(), intent.clone());
        }

        let spec = NewJob {
            workspace_id: workspace_id.clone(),
            session_id: request.session_id.clone(),
            turn_id: None,
            kind: JobKind::Python,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload,
            resource_request: codegg_core::jobs::ResourceRequest::for_kind(JobKind::Python),
            timeout: Some(timeout),
            retry_policy: RetryPolicy::no_retry(),
            idempotency,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: vec![],
        };

        // Derive a submission key from source hash for idempotency
        let key = SubmissionKey::new(format!("python:{source_hash}"))
            .map_err(|e| ToolError::Execution(format!("invalid submission key: {e}")))?;

        let submitted = submission
            .submit(Some(key), spec)
            .await
            .map_err(|e| ToolError::Execution(format!("scheduler submission failed: {e}")))?;

        // Wait for completion
        let completion = submission
            .scheduler()
            .wait_for_completion(
                &submitted.job_id,
                timeout + std::time::Duration::from_secs(30),
            )
            .await
            .map_err(|e| ToolError::Execution(format!("scheduler wait failed: {e}")))?;

        // Build a PythonRunResult from the completion summary for projection
        let result = PythonRunResult {
            status: match completion.status {
                crate::scheduler::executor::ExecutorStatus::Completed => {
                    crate::python_script::types::PythonRunStatus::Success
                }
                crate::scheduler::executor::ExecutorStatus::Cancelled => {
                    crate::python_script::types::PythonRunStatus::Failed(-4)
                }
                crate::scheduler::executor::ExecutorStatus::TimedOut => {
                    crate::python_script::types::PythonRunStatus::TimedOut
                }
                crate::scheduler::executor::ExecutorStatus::Failed
                | crate::scheduler::executor::ExecutorStatus::Interrupted => {
                    crate::python_script::types::PythonRunStatus::Failed(-1)
                }
            },
            stdout: completion.summary.clone(),
            stderr: String::new(),
            duration: std::time::Duration::from_millis(completion.metrics.elapsed_ms),
            mode: request.mode,
            script_length: request.code.len(),
            risk: crate::python_script::types::PythonRiskAssessment::safe(),
            capabilities: crate::python_script::types::PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".to_string(),
            diff: None,
            script_body_hash: Some(source_hash),
            stdout_label: completion
                .run_id
                .as_ref()
                .map(|rid| format!("run://{rid}/stdout")),
            stderr_label: completion
                .run_id
                .as_ref()
                .map(|rid| format!("run://{rid}/stderr")),
            diff_label: None,
            policy_decision: None,
            denied_capabilities: vec![],
            os_filesystem_isolation: false,
            os_network_isolation: false,
            effective_read_roots: vec![],
            effective_write_roots: vec![],
            allowed_subprocesses: vec![],
            enforcement_warnings: vec![],
        };

        Ok(project_python_run(&result))
    }
}

#[async_trait]
impl Tool for PythonScriptTool {
    fn name(&self) -> &str {
        "python_script"
    }

    fn description(&self) -> &str {
        "Execute Python scripts with static risk analysis, mode-based capability control, and safe projection. Use the least-powerful mode: 'analyze' for read-only analysis, 'transform' for workspace file changes, 'verify' for tests with subprocess. Prefer native tools for simple read/write/edit/test/git operations."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Python code to execute. Materialized to a temp file automatically."
                },
                "mode": {
                    "type": "string",
                    "enum": ["analyze", "transform", "verify"],
                    "description": "Execution mode. Use 'analyze' (default) for read-only. 'transform' for file changes. 'verify' for tests with subprocess."
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory. Defaults to current directory."
                },
                "timeout": {
                    "type": "number",
                    "description": "Wall-clock timeout in seconds. Default 60 for analyze/transform, 300 for verify."
                },
                "intent": {
                    "type": "string",
                    "description": "Optional description of what this script does."
                }
            },
            "required": ["code"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let request = build_python_request(&input)?;

        // Scheduler-owned path: submit through JobSubmissionService
        if let Some(ref submission) = self.submission_service {
            return self.execute_via_scheduler(&request, submission).await;
        }

        // No fallback: scheduler admission is required for production Python execution
        Err(ToolError::Disabled(
            "Python execution requires scheduler admission; scheduler is disabled".into(),
        ))
    }
}

/// Build a `PythonScriptRequest` from model-facing JSON input.
fn build_python_request(input: &serde_json::Value) -> Result<PythonScriptRequest, ToolError> {
    let code = input
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::Format("missing 'code' parameter".into()))?;

    let mode_str = input
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("analyze");

    let mode = match mode_str {
        "analyze" => PythonExecutionMode::Analyze,
        "transform" => PythonExecutionMode::Transform,
        "verify" => PythonExecutionMode::Verify,
        _ => {
            return Err(ToolError::Format(format!(
                "invalid mode '{mode_str}': expected analyze, transform, or verify"
            )))
        }
    };

    let workdir = input
        .get("workdir")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });

    let timeout = input
        .get("timeout")
        .and_then(|v| v.as_u64())
        .map(Some)
        .unwrap_or_else(|| {
            Some(match mode {
                PythonExecutionMode::Verify => 300,
                _ => 60,
            })
        });

    let intent = input
        .get("intent")
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok(PythonScriptRequest {
        code: code.to_string(),
        mode,
        cwd: workdir,
        workspace_root: None,
        timeout_secs: timeout,
        session_id: None,
        intent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_and_category() {
        let tool = PythonScriptTool::new();
        assert_eq!(tool.name(), "python_script");
        assert_eq!(tool.category(), ToolCategory::ShellExec);
    }

    #[test]
    fn parameters_schema() {
        let tool = PythonScriptTool::new();
        let params = tool.parameters();
        let props = params.get("properties").unwrap();
        assert!(props.get("code").is_some());
        assert!(props.get("mode").is_some());
        let required = params.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("code")));
        assert!(!required.contains(&json!("mode")));
    }
}
