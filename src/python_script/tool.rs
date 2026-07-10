use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};

use super::executor::execute_python_script;
use super::projection::project_python_run;
use super::types::{PythonExecutionMode, PythonScriptRequest};

/// Model-facing tool for executing Python scripts with safety analysis.
///
/// PythonScriptTool materializes scripts to temp files, runs static risk
/// analysis, captures changed files for Transform mode, and projects
/// results safely. Use this instead of bash for Python one-off scripts,
/// analysis, bulk transforms, and custom verification.
pub struct PythonScriptTool {
    run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
}

impl PythonScriptTool {
    pub fn new() -> Self {
        Self { run_store: None }
    }

    pub fn with_run_store(store: Arc<dyn codegg_core::run_store::RunStore>) -> Self {
        Self {
            run_store: Some(store),
        }
    }
}

impl Default for PythonScriptTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for PythonScriptTool {
    fn name(&self) -> &str {
        "python_script"
    }

    fn description(&self) -> &str {
        "Execute Python scripts with static risk analysis, mode-based capability control, and safe projection. Use the least-powerful mode: 'analyze' for read-only analysis, 'transform' for workspace file changes, 'verify' for test/verification with subprocess. Prefer native tools for simple read/write/edit/test/git operations."
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
            "required": ["code", "mode"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
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

        let request = PythonScriptRequest {
            code: code.to_string(),
            mode,
            cwd: workdir,
            workspace_root: None,
            timeout_secs: timeout,
            session_id: None,
            intent,
        };

        let result = execute_python_script(&request).await;

        // Persist to run store if available
        if let Some(ref store) = self.run_store {
            use chrono::Utc;
            use codegg_core::run_store::*;

            let cwd = request.cwd.clone();
            let workspace_root = request
                .workspace_root
                .clone()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| cwd.clone());

            let draft = RunDraft {
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
            };

            let status = match &result.status {
                super::types::PythonRunStatus::Success => RunStatus::Complete,
                super::types::PythonRunStatus::Failed(_) => RunStatus::Failed,
                super::types::PythonRunStatus::TimedOut => RunStatus::TimedOut,
                super::types::PythonRunStatus::SpawnError => RunStatus::Failed,
            };

            if let Ok(handle) = store.begin_run(draft).await {
                // Write stdout artifact
                if !result.stdout.is_empty() {
                    let _ = store
                        .write_artifact(
                            &handle,
                            ArtifactInput {
                                kind: ArtifactKind::Stdout,
                                data: result.stdout.as_bytes().to_vec(),
                                mime_type: "text/plain".to_string(),
                                safe_for_model: true,
                            },
                        )
                        .await;
                }

                // Write stderr artifact
                if !result.stderr.is_empty() {
                    let _ = store
                        .write_artifact(
                            &handle,
                            ArtifactInput {
                                kind: ArtifactKind::Stderr,
                                data: result.stderr.as_bytes().to_vec(),
                                mime_type: "text/plain".to_string(),
                                safe_for_model: true,
                            },
                        )
                        .await;
                }

                // Write diff artifact if present
                if let Some(ref diff) = result.diff {
                    let _ = store
                        .write_artifact(
                            &handle,
                            ArtifactInput {
                                kind: ArtifactKind::UnifiedDiff,
                                data: diff.as_bytes().to_vec(),
                                mime_type: "text/plain".to_string(),
                                safe_for_model: true,
                            },
                        )
                        .await;
                }

                let _ = store
                    .complete_run(
                        handle,
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
                        },
                    )
                    .await;
            }
        }

        let projected = project_python_run(&result);

        Ok(projected)
    }
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
        assert!(required.contains(&json!("mode")));
    }
}
