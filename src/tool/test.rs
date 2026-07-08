use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

use crate::error::ToolError;
use crate::test_runner::{
    custom::{is_allowed_custom_command, CUSTOM_COMMAND_ALLOWLIST},
    format_test_report, resolve_and_run_test, TestRunRequest, TestScope, TestStatus,
};
use crate::tool::backend::{
    StructuredToolResult, ToolBackendKind, ToolExecutionContext, ToolProvenance, ToolTrust,
};
use crate::tool::{Tool, ToolCategory};

/// Run project tests through codegg's supervised test runner. Prefer this
/// over bash for cargo test, cargo nextest, pytest, uv run pytest, go test,
/// make test, and similar test commands. The tool streams full stdout/stderr
/// to logs, classifies timeouts/failures, and returns a compact report
/// instead of dumping full output into context.
#[derive(Default)]
pub struct TestTool;

#[async_trait]
impl Tool for TestTool {
    fn name(&self) -> &str {
        "test"
    }

    fn description(&self) -> &str {
        "Run project tests through codegg's supervised test runner. Prefer this over bash for cargo test, cargo nextest, pytest, uv run pytest, go test, make test, and similar test commands. The tool streams full stdout/stderr to logs, classifies timeouts/failures, and returns a compact report instead of dumping full output into context."
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
                    "description": "Working directory. Defaults to current working directory."
                },
                "timeout": {
                    "type": "number",
                    "description": "Wall-clock timeout in seconds. Default 300."
                },
                "stall_timeout": {
                    "type": "number",
                    "description": "No-output timeout in seconds. Default 120; set 0 to disable."
                }
            },
            "required": ["scope"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let request = parse_test_request(&input)?;
        let report = resolve_and_run_test(request, None)
            .await
            .map_err(|e| ToolError::Execution(format!("test runner error: {e}")))?;
        Ok(format_test_report(&report))
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let start = Instant::now();
        let request = parse_test_request(&input)?;
        let report = resolve_and_run_test(request, None)
            .await
            .map_err(|e| ToolError::Execution(format!("test runner error: {e}")))?;
        let output = format_test_report(&report);
        let elapsed_ms = start.elapsed().as_millis() as u64;

        let success = matches!(report.status, TestStatus::Passed);
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

/// Parse JSON input into a `TestRunRequest`.
fn parse_test_request(input: &serde_json::Value) -> Result<TestRunRequest, ToolError> {
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
            if cmd.trim().is_empty() {
                return Err(ToolError::Execution(
                    "command parameter must not be empty".to_string(),
                ));
            }
            if !is_allowed_custom_command(cmd) {
                return Err(ToolError::Execution(format!(
                    "custom command not in allowlist: '{cmd}'. Allowed prefixes: {}",
                    CUSTOM_COMMAND_ALLOWLIST.join(", ")
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

    let workdir = input["workdir"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let timeout_secs = input["timeout"].as_u64();
    let stall_timeout_secs = input["stall_timeout"].as_u64();

    Ok(TestRunRequest {
        scope,
        workdir,
        timeout_secs,
        stall_timeout_secs,
        max_report_bytes: None,
        session_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_tool_rejects_missing_scope() {
        let tool = TestTool;
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
        let tool = TestTool;
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
        let tool = TestTool;
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
        let tool = TestTool;
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
        let tool = TestTool;
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
        let tool = TestTool;
        let input = json!({"scope": "custom", "command": "rm -rf /"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err}").contains("not in allowlist"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_tool_accepts_custom_command_in_allowlist() {
        let tool = TestTool;
        let input = json!({"scope": "custom", "command": "cargo test --lib", "workdir": "/tmp"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            !format!("{err}").contains("not in allowlist"),
            "should not be an allowlist error: {err}"
        );
    }

    #[tokio::test]
    async fn test_tool_rejects_empty_package() {
        let tool = TestTool;
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
        let tool = TestTool;
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
        let tool = TestTool;
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
        let tool = TestTool;
        assert_eq!(tool.name(), "test");
        assert!(tool.description().contains("supervised test runner"));
    }

    #[test]
    fn test_tool_parameters_schema() {
        let tool = TestTool;
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

    #[test]
    fn test_parse_request_custom_workdir() {
        let input = json!({"scope": "auto", "workdir": "/some/dir"});
        let req = parse_test_request(&input).unwrap();
        assert_eq!(req.workdir, PathBuf::from("/some/dir"));
    }

    #[test]
    fn test_parse_request_timeout() {
        let input = json!({"scope": "auto", "timeout": 60, "stall_timeout": 30});
        let req = parse_test_request(&input).unwrap();
        assert_eq!(req.timeout_secs, Some(60));
        assert_eq!(req.stall_timeout_secs, Some(30));
    }

    #[test]
    fn test_tool_exposes_in_definitions() {
        let tool = TestTool;
        assert!(tool.expose_in_definitions());
    }
}
