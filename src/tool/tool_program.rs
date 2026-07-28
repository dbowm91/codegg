//! Model-facing `tool_program` tool.
//!
//! Allows the model to submit a restricted-Python program that
//! calls read-only tools through the ToolBroker pipeline. The
//! program is compiled, validated, submitted to the scheduler,
//! and the result is returned synchronously (foreground) or
//! asynchronously with a durable handle (background).
//!
//! # Artifact isolation
//!
//! Intermediate tool call outputs stay inside the program's artifact
//! ledger and do NOT enter the parent model transcript. Only the
//! final program result (status, output, metrics) is projected into
//! the transcript. Callers can inspect `program_artifacts` in the
//! structured result to see intermediate call metadata, but these
//! are opaque handles — the full content is stored in the program's
//! own artifact store and must be expanded via `context_read` if
//! needed.
//!
//! # Background mode
//!
//! When `execution_mode` is `"background"`, the tool returns a
//! compact [`ProgramHandle`] immediately and the parent agent
//! continues. When the program reaches a terminal state, exactly
//! one notification is delivered to the parent session's inbox
//! via the [`ToolProgramNotificationService`].

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::scheduler::submission::{JobSubmissionService, SubmissionKey};
use crate::scheduler::tool_program_notifications::{ProgramHandle, ToolProgramNotificationService};
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance, ToolTrust};
use crate::tool::contract::{ToolCallerPolicy, ToolContract, ToolEffectClass};
use crate::tool::{Tool, ToolCategory};
use codegg_core::jobs::{
    IdempotencyClass as JobsIdempotencyClass, JobKind, JobPayload, JobPriority, JobSource, NewJob,
    ResourceRequest, RetryPolicy,
};
use codegg_core::tool_program::{self, ProgramStore};

const MAX_MODEL_TIMEOUT_MS: u64 = 10 * 60 * 1_000;

/// Metadata for one intermediate tool call inside a program.
///
/// These are included in the `program_artifacts` array of the final
/// result. They do NOT enter the parent transcript — only the final
/// program result is projected. The full call content is stored in
/// the program's artifact store and can be expanded via
/// `context_read` using the `artifact_handle`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgramCallArtifact {
    /// Tool name that was called (e.g. "read", "grep").
    pub tool_name: String,
    /// Input arguments passed to the tool.
    pub input: serde_json::Value,
    /// Whether the call succeeded.
    pub success: bool,
    /// Artifact handle for the full output content (ctx:// URI).
    /// The caller can use `context_read` to expand this.
    pub artifact_handle: Option<String>,
    /// Truncated display preview (first ~200 chars).
    pub preview: String,
}

/// Foreground tool for submitting read-only tool programs.
///
/// Programs execute through the scheduler and return only the final
/// result to the parent transcript. Intermediate tool call outputs
/// stay in the program's artifact ledger (see [`ProgramCallArtifact`])
/// and do NOT enter the transcript by default.
///
/// When `execution_mode` is `"background"`, the tool returns a
/// compact handle immediately and the parent continues. Terminal
/// notifications are delivered via the notification service.
pub struct ToolProgramTool {
    submission: Option<Arc<JobSubmissionService>>,
    notification_service: Option<Arc<ToolProgramNotificationService>>,
}

impl ToolProgramTool {
    pub fn new() -> Self {
        Self {
            submission: None,
            notification_service: None,
        }
    }

    pub fn with_submission(mut self, submission: Arc<JobSubmissionService>) -> Self {
        self.submission = Some(submission);
        self
    }

    pub fn with_notification_service(
        mut self,
        service: Arc<ToolProgramNotificationService>,
    ) -> Self {
        self.notification_service = Some(service);
        self
    }

    /// Cancel a background tool program by its job_id. This is
    /// idempotent: cancelling an already-completed or already-cancelled
    /// program is a no-op.
    pub async fn cancel(&self, job_id: &str) -> Result<(), ToolError> {
        let submission = self.submission.as_ref().ok_or_else(|| {
            ToolError::Disabled("tool_program requires scheduler submission service".into())
        })?;
        submission
            .scheduler()
            .request_cancel(
                &codegg_core::jobs::JobId::new_unchecked(job_id),
                "user_cancelled_via_tool_program",
            )
            .await
            .map_err(|e| ToolError::Execution(format!("cancel failed: {}", e)))?;
        Ok(())
    }
}

impl Default for ToolProgramTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Execution mode for tool programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Block until completion and return the result (default).
    Foreground,
    /// Return a handle immediately; notification on completion.
    Background,
}

impl ExecutionMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "background" | "bg" => Self::Background,
            _ => Self::Foreground,
        }
    }
}

#[async_trait]
impl Tool for ToolProgramTool {
    fn name(&self) -> &str {
        "tool_program"
    }

    fn description(&self) -> &str {
        "Submit a read-only program that calls tools. The program is compiled to a safe IR, \
         validated against the tool manifest, and executed in a sandboxed interpreter. \
         Only read-only and deterministic tools may be called. Intermediate tool call outputs \
         stay in the program artifact ledger and do not enter the parent transcript."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Restricted Python source code. Supports: variables, \
                        assignments, if/elif/else, for loops (with range()), while loops, \
                        function calls (call()), parallel groups (parallel()), emit(), \
                        fail(), basic arithmetic, string operations, list/dict literals, \
                        and indexing."
                },
                "tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tool names this program may call. All tools must be \
                        in the read-only palette and have output schemas."
                },
                "description": {
                    "type": "string",
                    "description": "Human-readable description of what the program does."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds (default: 120000)."
                },
                "execution_mode": {
                    "type": "string",
                    "enum": ["foreground", "background"],
                    "description": "Execution mode. 'foreground' (default) blocks until \
                        completion and returns the result. 'background' returns a \
                        program handle immediately and the parent continues; a \
                        terminal notification is delivered when the program finishes."
                },
                "backend_policy": {
                    "type": "string",
                    "enum": ["native_only"],
                    "description": "Execution backend policy. Only native execution is supported."
                }
            },
            "required": ["source", "tools"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOnly,
            effect_class: ToolEffectClass::ReadOnly,
            idempotency: crate::tool::contract::IdempotencyClass::Idempotent,
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": ["completed", "failed", "cancelled", "timed_out", "interrupted", "submitted"] },
                    "output": {},
                    "steps_used": { "type": "integer" },
                    "calls_completed": { "type": "integer" },
                    "program_id": { "type": "string" },
                    "error": { "type": "string" },
                    "program_artifacts": {
                        "type": "array",
                        "description": "Intermediate tool call metadata. These do NOT enter the parent transcript.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "tool_name": { "type": "string" },
                                "success": { "type": "boolean" },
                                "artifact_handle": { "type": "string" },
                                "preview": { "type": "string" }
                            }
                        }
                    },
                    "handle": {
                        "type": "object",
                        "description": "Program handle (background mode only).",
                        "properties": {
                            "program_id": { "type": "string" },
                            "job_id": { "type": "string" },
                            "status": { "type": "string" },
                            "submitted_at": { "type": "integer" },
                            "timeout_ms": { "type": "integer" },
                            "inspect_ref": { "type": "string" },
                            "cancel_ref": { "type": "string" }
                        }
                    }
                },
                "required": ["status"]
            })),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let result = self.execute_impl(input, None).await?;
        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let value = self.execute_impl(input.clone(), ctx).await?;
        let display = value
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string();
        let success = matches!(display.as_str(), "completed" | "submitted");
        Ok(StructuredToolResult {
            output: format!("program status: {}", display),
            value: Some(value),
            success,
            provenance: Some(ToolProvenance {
                backend: "native".to_string(),
                implementation: "codegg/tool_program".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                elapsed_ms: None,
                truncated: false,
                trust: ToolTrust::LocalTrusted,
            }),
        })
    }
}

impl ToolProgramTool {
    async fn execute_impl(
        &self,
        input: serde_json::Value,
        execution_context: Option<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let source = input
            .get("source")
            .and_then(|s| s.as_str())
            .ok_or_else(|| ToolError::Format("missing required field: source".into()))?;

        let tools: Vec<String> = input
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let _description = input
            .get("description")
            .and_then(|s| s.as_str())
            .unwrap_or("");

        let timeout_ms = input
            .get("timeout_ms")
            .and_then(|t| t.as_u64())
            .unwrap_or(120_000);
        if timeout_ms == 0 {
            return Err(ToolError::Format(
                "timeout_ms must be a finite positive duration".into(),
            ));
        }
        if timeout_ms > MAX_MODEL_TIMEOUT_MS {
            return Err(ToolError::Format(format!(
                "timeout_ms exceeds the {}ms Tool Program limit",
                MAX_MODEL_TIMEOUT_MS
            )));
        }

        let execution_mode = input
            .get("execution_mode")
            .and_then(|s| s.as_str())
            .map(ExecutionMode::from_str)
            .unwrap_or(ExecutionMode::Foreground);

        let backend_policy = input
            .get("backend_policy")
            .and_then(|value| value.as_str())
            .or_else(|| {
                execution_context
                    .as_ref()
                    .and_then(|context| context.backend_policy.as_deref())
            })
            .unwrap_or("native_only");
        let backend_policy = codegg_providers::HostedBackendPolicy::parse(backend_policy)
            .ok_or_else(|| ToolError::Format("backend_policy is not recognized".into()))?;
        let provider_name = execution_context
            .as_ref()
            .and_then(|context| context.provider_name.as_deref())
            .unwrap_or("unknown");
        let capabilities = codegg_providers::ProviderCapabilities::for_provider(provider_name);
        match backend_policy.resolve(&capabilities) {
            codegg_providers::ResolvedBackend::Failed { reason } => {
                return Err(ToolError::Disabled(reason));
            }
            codegg_providers::ResolvedBackend::Hosted => {
                // M013-C-38: Non-native backends are not supported for Tool
                // Programs. Reject hosted_required and hosted_preferred
                // instead of silently falling back to native.
                return Err(ToolError::Disabled(format!(
                    "hosted backend is not available for Tool Programs (policy={})",
                    backend_policy.as_str()
                )));
            }
            codegg_providers::ResolvedBackend::Native => {}
        }

        if source.is_empty() {
            return Err(ToolError::Format("source must not be empty".into()));
        }
        if tools.is_empty() {
            return Err(ToolError::Format("tools array must not be empty".into()));
        }
        if tools.len() > 64 {
            return Err(ToolError::Format(
                "tools array exceeds the 64-tool limit".into(),
            ));
        }

        // Step 1: Compile the program
        let compilation = tool_program::compile_program(source)
            .map_err(|e| ToolError::Format(format!("program compilation failed: {}", e)))?;

        // Step 2: Validate IR integrity
        tool_program::verify_ir_integrity(&compilation.ir)
            .map_err(|e| ToolError::Format(format!("IR verification failed: {}", e)))?;

        // Step 3: Submit to scheduler
        let submission = self.submission.as_ref().ok_or_else(|| {
            ToolError::Disabled("tool_program requires scheduler submission service".into())
        })?;

        if execution_mode == ExecutionMode::Background
            && execution_context
                .as_ref()
                .and_then(|context| context.session_id.as_ref())
                .is_none()
        {
            return Err(ToolError::Format(
                "background Tool Programs require a parent session context".into(),
            ));
        }

        let workspace_root = execution_context
            .as_ref()
            .map(|context| context.cwd.clone())
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });
        let workspace_id = submission
            .workspace_id_for_root(&workspace_root)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let invocation_key = execution_context
            .as_ref()
            .and_then(|context| context.invocation_key.clone())
            .unwrap_or_else(|| format!("tool-program:{}", uuid::Uuid::new_v4()));
        // The durable program identity is generated from the explicit
        // transport invocation key, never from source content. This keeps
        // retry deduplication stable while two distinct model calls with
        // identical source still receive distinct program identities.
        let invocation_digest = crate::tool::tool_program_context::stable_digest(&invocation_key);
        let program_id = format!("tp-{}", &invocation_digest[..32]);
        let submission_key = SubmissionKey::new(format!(
            "tp-submit:{}",
            crate::tool::tool_program_context::stable_digest(&invocation_key)
        ))
        .map_err(|e| ToolError::Execution(format!("invalid submission key: {}", e)))?;

        let mut context_record = crate::tool::tool_program_context::to_core_context(
            execution_context.as_ref(),
            workspace_id.as_str(),
            &program_id,
        )
        .map_err(ToolError::Permission)?;
        context_record.backend_policy = backend_policy.as_str().to_string();
        let contract_entries = execution_context
            .as_ref()
            .and_then(|context| context.program_contract_snapshot.clone())
            .ok_or_else(|| {
                ToolError::Permission(
                    "accepted invocation is missing the frozen runtime Broker catalog".into(),
                )
            })?;
        let requested: std::collections::BTreeSet<_> = tools.iter().cloned().collect();
        let frozen: std::collections::BTreeSet<_> = contract_entries
            .iter()
            .map(|entry| entry.tool_name.clone())
            .collect();
        if requested != frozen {
            return Err(ToolError::Permission(
                "requested tools do not match the accepted frozen runtime catalog".into(),
            ));
        }
        let contract_snapshot_json =
            crate::tool::tool_program_context::canonical_contract_json(&contract_entries)
                .map_err(ToolError::Permission)?;
        let contract_digest =
            crate::tool::tool_program_context::canonical_contract_digest(&contract_entries)
                .map_err(ToolError::Permission)?;
        context_record.contract_snapshot_json = contract_snapshot_json;
        let source_digest = ProgramStore::digest_source(source);
        let requested_deadline = chrono::Utc::now()
            + chrono::Duration::from_std(std::time::Duration::from_millis(timeout_ms))
                .map_err(|_| ToolError::Format("timeout_ms is too large".into()))?;
        let effective_deadline = execution_context
            .as_ref()
            .and_then(|context| context.deadline)
            .map(|parent| parent.min(requested_deadline))
            .unwrap_or(requested_deadline);
        let execution_context_json = serde_json::to_string(&context_record)
            .map_err(|e| ToolError::Execution(format!("context serialization failed: {}", e)))?;
        let authority_digest = crate::tool::tool_program_context::authority_digest(
            &context_record,
            &tools,
            &source_digest,
        );

        let authority_grant = crate::tool::tool_program_context::build_authority_grant(
            Some(&context_record),
            workspace_id.as_str(),
            &program_id,
            &tools,
            &source_digest,
            &compilation.ir.digest,
            &contract_digest,
        )
        .map_err(ToolError::Permission)?;
        let authority_grant_json = serde_json::to_string(&authority_grant)
            .map_err(|e| ToolError::Execution(format!("grant serialization failed: {}", e)))?;
        // Authorization and contract validation intentionally precede source
        // persistence, so rejected direct calls leave no executable residue.
        let source_ref =
            crate::tool::tool_program_source::ToolProgramSourceStore::new(&workspace_root)
                .persist(source)
                .map_err(|e| ToolError::Execution(format!("source persistence failed: {}", e)))?;

        let new_job = NewJob {
            workspace_id,
            session_id: context_record.session_id.clone(),
            turn_id: context_record.turn_id.clone(),
            kind: JobKind::ToolProgram,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ToolProgram {
                program_id: program_id.clone(),
                invocation_key,
                source_digest: source_digest.clone(),
                ir_digest: Some(compilation.ir.digest.clone()),
                authority_digest,
                execution_context_json: Some(execution_context_json),
                submission_key: submission_key.as_str().to_string(),
                execution_mode: match execution_mode {
                    ExecutionMode::Foreground => "foreground".into(),
                    ExecutionMode::Background => "background".into(),
                },
                source_ref: Some(source_ref.relative_path),
                source_length: Some(source_ref.length),
                allowed_tools: tools,
                authority_grant_json: Some(authority_grant_json),
            },
            resource_request: ResourceRequest::for_kind(JobKind::ToolProgram),
            timeout: Some(std::time::Duration::from_millis(timeout_ms)),
            retry_policy: RetryPolicy::no_retry(),
            idempotency: JobsIdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: Some(effective_deadline),
            schedule_id: None,
            depends_on: vec![],
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
        };

        let submitted = submission
            .submit(Some(submission_key), new_job)
            .await
            .map_err(|e| ToolError::Execution(format!("submission failed: {}", e)))?;

        match execution_mode {
            ExecutionMode::Background => {
                self.handle_background(submitted, program_id, timeout_ms)
                    .await
            }
            ExecutionMode::Foreground => {
                self.handle_foreground(submitted, program_id, timeout_ms, &workspace_root)
                    .await
            }
        }
    }

    /// Handle background submission: return a compact handle immediately
    /// and register a notification record for terminal delivery.
    async fn handle_background(
        &self,
        submitted: crate::scheduler::submission::SubmittedJob,
        program_id: String,
        timeout_ms: u64,
    ) -> Result<serde_json::Value, ToolError> {
        let now = chrono::Utc::now().timestamp_millis();
        let job_id = submitted.job_id.as_str().to_string();

        let handle = ProgramHandle {
            program_id: program_id.clone(),
            job_id: job_id.clone(),
            status: "submitted".to_string(),
            submitted_at: now,
            timeout_ms,
            inspect_ref: program_id.clone(),
            cancel_ref: job_id.clone(),
        };

        Ok(json!({
            "status": "submitted",
            "program_id": program_id,
            "handle": {
                "program_id": handle.program_id,
                "job_id": handle.job_id,
                "status": handle.status,
                "submitted_at": handle.submitted_at,
                "timeout_ms": handle.timeout_ms,
                "inspect_ref": handle.inspect_ref,
                "cancel_ref": handle.cancel_ref,
            }
        }))
    }

    /// Handle foreground submission: wait for completion and return the result.
    async fn handle_foreground(
        &self,
        submitted: crate::scheduler::submission::SubmittedJob,
        program_id: String,
        timeout_ms: u64,
        workspace_root: &std::path::Path,
    ) -> Result<serde_json::Value, ToolError> {
        let submission = self.submission.as_ref().ok_or_else(|| {
            ToolError::Disabled("tool_program requires scheduler submission service".into())
        })?;

        // Wait for completion
        let wait_duration = std::time::Duration::from_millis(timeout_ms + 30_000); // extra buffer for scheduling
        let completion = submission
            .scheduler()
            .wait_for_completion(&submitted.job_id, wait_duration)
            .await
            .map_err(|e| ToolError::Execution(format!("wait failed: {}", e)))?;

        if let Ok(Some(record)) =
            crate::tool::tool_program_result::ToolProgramResultStore::new(workspace_root)
                .load(&program_id)
        {
            return Ok(crate::tool::tool_program_result::result_to_json(&record));
        }

        // Compatibility fallback for terminal jobs written before the typed
        // result record existed. It intentionally exposes only the terminal
        // summary; semantic counters are not reconstructed from text.
        let status = match completion.status {
            crate::scheduler::executor::ExecutorStatus::Completed => "completed",
            crate::scheduler::executor::ExecutorStatus::Failed => "failed",
            crate::scheduler::executor::ExecutorStatus::Cancelled => "cancelled",
            crate::scheduler::executor::ExecutorStatus::TimedOut => "timed_out",
            crate::scheduler::executor::ExecutorStatus::Interrupted => "interrupted",
        };

        let mut result = json!({
            "status": status,
            "program_id": program_id,
            "steps_used": 0,
            "calls_completed": 0,
            "program_artifacts": [],
        });

        if !completion.summary.is_empty() {
            result["output"] = json!(completion.summary);
        }

        if status == "failed" || status == "timed_out" {
            result["error"] = json!(completion.summary);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_program_name() {
        let tool = ToolProgramTool::new();
        assert_eq!(tool.name(), "tool_program");
    }

    #[test]
    fn tool_program_category_is_readonly() {
        let tool = ToolProgramTool::new();
        assert_eq!(tool.category(), ToolCategory::ReadOnly);
    }

    #[test]
    fn tool_program_parameters_have_required_fields() {
        let tool = ToolProgramTool::new();
        let params = tool.parameters();
        let required = params.get("required").and_then(|r| r.as_array());
        assert!(required.is_some());
        let names: Vec<_> = required
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(names.contains(&"source"));
        assert!(names.contains(&"tools"));
    }

    #[test]
    fn tool_program_contract_allows_direct_only() {
        let tool = ToolProgramTool::new();
        let contract = tool.contract("tool_program", tool.parameters());
        assert_eq!(contract.caller_policy, ToolCallerPolicy::DirectOnly);
        assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
        assert!(contract.output_schema.is_some());
    }

    #[test]
    fn tool_program_missing_source_fails() {
        let tool = ToolProgramTool::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = tool.execute(json!({"tools": ["read"]})).await.unwrap_err();
            assert!(err.to_string().contains("source"));
        });
    }

    #[test]
    fn tool_program_missing_tools_fails() {
        let tool = ToolProgramTool::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = tool
                .execute(json!({"source": "emit(1)\n"}))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("tools"));
        });
    }

    #[test]
    fn tool_program_empty_source_fails() {
        let tool = ToolProgramTool::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = tool
                .execute(json!({"source": "", "tools": ["read"]}))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("empty"));
        });
    }

    #[test]
    fn tool_program_invalid_source_fails() {
        let tool = ToolProgramTool::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = tool
                .execute(json!({"source": "import os\n", "tools": ["read"]}))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("compilation"));
        });
    }

    #[test]
    fn tool_program_no_submission_fails() {
        let tool = ToolProgramTool::new(); // no submission service
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = tool
                .execute(json!({"source": "emit(1)\n", "tools": ["read"]}))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("scheduler"));
        });
    }

    #[test]
    fn program_call_artifact_serializes() {
        let artifact = ProgramCallArtifact {
            tool_name: "read".to_string(),
            input: json!({"path": "/tmp/a.txt"}),
            success: true,
            artifact_handle: Some("ctx://tool/s1/0/c1".to_string()),
            preview: "line 1: hello".to_string(),
        };
        let json = serde_json::to_value(&artifact).unwrap();
        assert_eq!(json["tool_name"], "read");
        assert_eq!(json["success"], true);
        assert_eq!(json["artifact_handle"], "ctx://tool/s1/0/c1");
    }

    #[test]
    fn program_call_artifact_roundtrip() {
        let artifact = ProgramCallArtifact {
            tool_name: "grep".to_string(),
            input: json!({"pattern": "TODO"}),
            success: false,
            artifact_handle: None,
            preview: String::new(),
        };
        let json_str = serde_json::to_string(&artifact).unwrap();
        let back: ProgramCallArtifact = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.tool_name, "grep");
        assert!(!back.success);
        assert!(back.artifact_handle.is_none());
    }

    #[test]
    fn tool_program_output_schema_includes_artifacts() {
        let tool = ToolProgramTool::new();
        let contract = tool.contract("tool_program", tool.parameters());
        let schema = contract.output_schema.unwrap();
        let props = schema.get("properties").unwrap();
        assert!(props.get("program_artifacts").is_some());
        assert!(props.get("calls_completed").is_some());
    }
}
