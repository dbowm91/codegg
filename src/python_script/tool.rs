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
#[derive(Default)]
pub struct PythonScriptTool;

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
            timeout_secs: timeout,
            session_id: None,
            intent,
        };

        let result = execute_python_script(&request).await;
        let projected = project_python_run(&result);

        Ok(projected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_and_category() {
        let tool = PythonScriptTool;
        assert_eq!(tool.name(), "python_script");
        assert_eq!(tool.category(), ToolCategory::ShellExec);
    }

    #[test]
    fn parameters_schema() {
        let tool = PythonScriptTool;
        let params = tool.parameters();
        let props = params.get("properties").unwrap();
        assert!(props.get("code").is_some());
        assert!(props.get("mode").is_some());
        let required = params.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("code")));
        assert!(required.contains(&json!("mode")));
    }
}
