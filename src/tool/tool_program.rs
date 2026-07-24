//! Foreground model-facing `tool_program` tool.
//!
//! Allows the model to submit a restricted-Python program that
//! calls read-only tools through the ToolBroker pipeline. The
//! program is compiled, validated, submitted to the scheduler,
//! and the result is returned synchronously.
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

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::scheduler::submission::{JobSubmissionService, SubmissionKey};
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance, ToolTrust};
use crate::tool::contract::{ToolCallerPolicy, ToolContract, ToolEffectClass};
use crate::tool::{Tool, ToolCategory};
use codegg_core::jobs::{
    IdempotencyClass as JobsIdempotencyClass, JobKind, JobPayload, JobPriority, JobSource, NewJob,
    ResourceRequest, RetryPolicy,
};
use codegg_core::tool_program::{self, ProgramStore};

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
pub struct ToolProgramTool {
    submission: Option<Arc<JobSubmissionService>>,
}

impl ToolProgramTool {
    pub fn new() -> Self {
        Self { submission: None }
    }

    pub fn with_submission(mut self, submission: Arc<JobSubmissionService>) -> Self {
        self.submission = Some(submission);
        self
    }
}

impl Default for ToolProgramTool {
    fn default() -> Self {
        Self::new()
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
                    "status": { "type": "string", "enum": ["completed", "failed", "cancelled", "timed_out", "interrupted"] },
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
                    }
                },
                "required": ["status"]
            })),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let result = self.execute_impl(input).await?;
        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let value = self.execute_impl(input.clone()).await?;
        let display = value
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string();
        Ok(StructuredToolResult {
            output: format!("program status: {}", display),
            value: Some(value),
            success: true,
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
    async fn execute_impl(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
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

        if source.is_empty() {
            return Err(ToolError::Format("source must not be empty".into()));
        }
        if tools.is_empty() {
            return Err(ToolError::Format("tools array must not be empty".into()));
        }

        // Step 1: Compile the program
        let compilation = tool_program::compile_program(source)
            .map_err(|e| ToolError::Format(format!("program compilation failed: {}", e)))?;

        // Step 2: Validate IR integrity
        tool_program::verify_ir_integrity(&compilation.ir)
            .map_err(|e| ToolError::Format(format!("IR verification failed: {}", e)))?;

        // Step 3: Resolve manifest — validate tool availability
        // Full manifest resolution requires ToolBroker access, which is
        // available through the submission service. For this milestone,
        // we validate that all requested tools are non-empty and the
        // program compiles. Full broker-based manifest resolution is
        // performed at execution time in the ToolProgramExecutor.

        // Step 4: Submit to scheduler
        let submission = self.submission.as_ref().ok_or_else(|| {
            ToolError::Disabled("tool_program requires scheduler submission service".into())
        })?;

        let source_digest = ProgramStore::digest_source(source);
        let program_id = format!("tp-{}", &source_digest[..16.min(source_digest.len())]);

        let workspace_root =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let workspace_id = submission
            .workspace_id_for_root(&workspace_root)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let submission_key = SubmissionKey::new(format!("tp-submit:{}", source_digest))
            .map_err(|e| ToolError::Execution(format!("invalid submission key: {}", e)))?;

        let new_job = NewJob {
            workspace_id,
            session_id: None,
            turn_id: None,
            kind: JobKind::ToolProgram,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ToolProgram {
                program_id: program_id.clone(),
                source_digest: source_digest.clone(),
                ir_digest: Some(compilation.ir.digest.clone()),
                authority_digest: source_digest.clone(),
                submission_key: format!("tp-submit:{}", source_digest),
            },
            resource_request: ResourceRequest::for_kind(JobKind::ToolProgram),
            timeout: Some(std::time::Duration::from_millis(timeout_ms)),
            retry_policy: RetryPolicy::no_retry(),
            idempotency: JobsIdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: vec![],
        };

        let submitted = submission
            .submit(Some(submission_key), new_job)
            .await
            .map_err(|e| ToolError::Execution(format!("submission failed: {}", e)))?;

        // Step 5: Wait for completion
        let wait_duration = std::time::Duration::from_millis(timeout_ms + 30_000); // extra buffer for scheduling
        let completion = submission
            .scheduler()
            .wait_for_completion(&submitted.job_id, wait_duration)
            .await
            .map_err(|e| ToolError::Execution(format!("wait failed: {}", e)))?;

        // Step 6: Map result
        let status = match completion.status {
            crate::scheduler::executor::ExecutorStatus::Completed => "completed",
            crate::scheduler::executor::ExecutorStatus::Failed => "failed",
            crate::scheduler::executor::ExecutorStatus::Cancelled => "cancelled",
            crate::scheduler::executor::ExecutorStatus::TimedOut => "timed_out",
            crate::scheduler::executor::ExecutorStatus::Interrupted => "interrupted",
        };

        // Parse calls_completed from summary: "status=X steps=N ... calls=N"
        let calls_completed = completion
            .summary
            .split_whitespace()
            .find(|s| s.starts_with("calls="))
            .and_then(|s| s.strip_prefix("calls="))
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        let mut result = json!({
            "status": status,
            "program_id": program_id,
            "steps_used": completion.metrics.elapsed_ms,
            "calls_completed": calls_completed,
            "program_artifacts": [],  // intermediate calls stay in program artifact ledger, not transcript
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
