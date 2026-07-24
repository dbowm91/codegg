//! Integration tests for background tool program submission.
//!
//! Tests that the `tool_program` tool supports both foreground and
//! background execution modes, returns the correct handle type for
//! background mode, and that the notification service is properly
//! wired.

use serde_json::json;

use codegg::tool::Tool;

#[test]
fn tool_program_execution_mode_parameter() {
    let tool = codegg::tool::tool_program::ToolProgramTool::new();
    let params = tool.parameters();
    let props = params.get("properties").unwrap();
    let exec_mode = props.get("execution_mode").unwrap();
    let enum_vals: Vec<_> = exec_mode
        .get("enum")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(enum_vals.contains(&"foreground"));
    assert!(enum_vals.contains(&"background"));
}

#[test]
fn tool_program_output_schema_includes_submitted_status() {
    let tool = codegg::tool::tool_program::ToolProgramTool::new();
    let contract = tool.contract("tool_program", tool.parameters());
    let schema = contract.output_schema.unwrap();
    let status_enum: Vec<_> = schema["properties"]["status"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(status_enum.contains(&"submitted"));
}

#[test]
fn tool_program_output_schema_includes_handle() {
    let tool = codegg::tool::tool_program::ToolProgramTool::new();
    let contract = tool.contract("tool_program", tool.parameters());
    let schema = contract.output_schema.unwrap();
    assert!(schema["properties"]["handle"].is_object());
    let handle_props = &schema["properties"]["handle"]["properties"];
    assert!(handle_props.get("program_id").is_some());
    assert!(handle_props.get("job_id").is_some());
    assert!(handle_props.get("status").is_some());
    assert!(handle_props.get("submitted_at").is_some());
    assert!(handle_props.get("timeout_ms").is_some());
    assert!(handle_props.get("inspect_ref").is_some());
    assert!(handle_props.get("cancel_ref").is_some());
}

#[test]
fn execution_mode_foreground_is_default() {
    use codegg::tool::tool_program::ExecutionMode;
    assert_eq!(
        ExecutionMode::from_str("foreground"),
        ExecutionMode::Foreground
    );
    assert_eq!(ExecutionMode::from_str(""), ExecutionMode::Foreground);
    assert_eq!(
        ExecutionMode::from_str("unknown"),
        ExecutionMode::Foreground
    );
}

#[test]
fn execution_mode_background_recognized() {
    use codegg::tool::tool_program::ExecutionMode;
    assert_eq!(
        ExecutionMode::from_str("background"),
        ExecutionMode::Background
    );
    assert_eq!(ExecutionMode::from_str("bg"), ExecutionMode::Background);
    assert_eq!(
        ExecutionMode::from_str("BACKGROUND"),
        ExecutionMode::Background
    );
}

#[test]
fn tool_program_background_mode_requires_submission() {
    let tool = codegg::tool::tool_program::ToolProgramTool::new(); // no submission
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(json!({
                "source": "emit(1)\n",
                "tools": ["read"],
                "execution_mode": "background"
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("scheduler"));
    });
}

#[test]
fn tool_program_foreground_invalid_source_fails() {
    let tool = codegg::tool::tool_program::ToolProgramTool::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(json!({
                "source": "import os\n",
                "tools": ["read"],
                "execution_mode": "foreground"
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("compilation"));
    });
}

#[test]
fn tool_program_background_invalid_source_fails() {
    let tool = codegg::tool::tool_program::ToolProgramTool::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(json!({
                "source": "import os\n",
                "tools": ["read"],
                "execution_mode": "background"
            }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("compilation"));
    });
}

#[test]
fn tool_program_cancel_without_submission_fails() {
    let tool = codegg::tool::tool_program::ToolProgramTool::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool.cancel("j-1").await.unwrap_err();
        assert!(err.to_string().contains("scheduler"));
    });
}
