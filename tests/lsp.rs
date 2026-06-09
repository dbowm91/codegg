use codegg::error::ToolError;
use codegg::lsp::client::parse_publish_diagnostics;
use codegg::lsp::language::{detect_language, language_id_to_server_id};
use codegg::tool::lsp::to_lsp_position;
use codegg::tool::{Tool, ToolCategory};

fn make_tool() -> codegg::tool::lsp::LspTool {
    codegg::tool::lsp::LspTool::new(std::sync::Arc::new(codegg::lsp::service::LspService::new(
        codegg::lsp::config::LspConfig::default(),
    )))
}

fn make_tool_with_root(root: &std::path::Path) -> codegg::tool::lsp::LspTool {
    codegg::tool::lsp::LspTool::new(std::sync::Arc::new(codegg::lsp::service::LspService::new(
        codegg::lsp::config::LspConfig::default(),
    )))
    .with_allowed_root(root.to_path_buf())
}

/// Create a temporary .rs file in a temp dir and return (dir, path).
fn temp_rs_file(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("test.rs");
    std::fs::write(&path, content).expect("write temp file");
    (dir, path)
}

// ── Language detection (existing tests preserved) ──────────────────────

#[test]
fn test_detect_language_rust() {
    let lang = detect_language("test.rs");
    assert!(lang.is_some());
    assert_eq!(lang.unwrap(), "rust");
}

#[test]
fn test_detect_language_python() {
    let lang = detect_language("test.py");
    assert!(lang.is_some());
    assert_eq!(lang.unwrap(), "python");
}

#[test]
fn test_detect_language_typescript() {
    let lang = detect_language("test.ts");
    assert!(lang.is_some());
    assert_eq!(lang.unwrap(), "typescript");
}

#[test]
fn test_detect_language_unknown() {
    let lang = detect_language("test.unknown");
    assert!(lang.is_none());
}

#[test]
fn test_language_id_to_server_id_rust() {
    let server_id = language_id_to_server_id("rust");
    assert!(server_id.is_some());
}

#[test]
fn test_language_id_to_server_id_unknown() {
    let server_id = language_id_to_server_id("nonexistent");
    assert!(server_id.is_none());
}

// ── 1. LSP tool schema ────────────────────────────────────────────────

#[test]
fn lsp_tool_schema_operation_enum() {
    let tool = make_tool();
    let params = tool.parameters();
    let ops = params["properties"]["operation"]["enum"]
        .as_array()
        .expect("operation.enum should be an array");
    let expected = [
        "goToDefinition",
        "findReferences",
        "hover",
        "documentSymbol",
        "workspaceSymbol",
        "diagnostics",
    ];
    assert_eq!(ops.len(), expected.len());
    for name in &expected {
        assert!(
            ops.iter().any(|v| v.as_str() == Some(name)),
            "missing operation: {name}"
        );
    }
}

#[test]
fn lsp_tool_schema_requires_operation() {
    let tool = make_tool();
    let params = tool.parameters();
    let required = params["required"].as_array().expect("required array");
    assert_eq!(required.len(), 1);
    assert_eq!(required[0].as_str(), Some("operation"));
}

// ── 2. LSP tool is ReadOnly ───────────────────────────────────────────

#[test]
fn lsp_tool_category_is_read_only() {
    let tool = make_tool();
    assert_eq!(tool.category(), ToolCategory::ReadOnly);
}

#[test]
fn lsp_tool_name_and_description() {
    let tool = make_tool();
    assert_eq!(tool.name(), "lsp");
    assert!(tool.description().contains("goToDefinition"));
    assert!(tool.description().contains("diagnostics"));
    assert!(!tool.description().contains("codeLens"));
}

// ── 3. Line/column conversion ─────────────────────────────────────────

#[test]
fn to_lsp_position_one_indexed_to_zero_indexed() {
    let pos = to_lsp_position(1, 1);
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 0);
}

#[test]
fn to_lsp_position_nontrivial_values() {
    let pos = to_lsp_position(10, 5);
    assert_eq!(pos.line, 9);
    assert_eq!(pos.character, 4);
}

#[test]
fn to_lsp_position_saturates_at_zero() {
    let pos = to_lsp_position(0, 0);
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 0);
}

#[test]
fn to_lsp_position_saturates_line_only() {
    let pos = to_lsp_position(0, 5);
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 4);
}

#[test]
fn to_lsp_position_saturates_column_only() {
    let pos = to_lsp_position(5, 0);
    assert_eq!(pos.line, 4);
    assert_eq!(pos.character, 0);
}

// ── 4. Position validation ────────────────────────────────────────────
//
// The execute() flow is: parse input → resolve_file → require_line_col → LSP op.
// resolve_file runs first, so tests for missing line/column must supply a
// valid file_path.  Tests for missing file_path omit it entirely.

#[tokio::test]
async fn lsp_execute_missing_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": "goToDefinition", "line": 1, "column": 1}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("file_path")),
        "expected file_path error, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_missing_line_and_column_no_file() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": "goToDefinition"}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("file_path")),
        "expected file_path error when all missing, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_find_references_requires_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": "findReferences"}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("file_path")),
        "expected file_path error for findReferences, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_hover_requires_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": "hover"}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("file_path")),
        "expected file_path error for hover, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_document_symbol_requires_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": "documentSymbol"}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("file_path")),
        "expected file_path error for documentSymbol, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_diagnostics_requires_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": "diagnostics"}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("file_path")),
        "expected file_path error for diagnostics, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_workspace_symbol_requires_symbol() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": "workspaceSymbol"}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("symbol")),
        "expected symbol error for workspaceSymbol, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_code_lens_removed_from_schema() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": "codeLens"}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("unknown LSP operation")),
        "expected unknown operation error for removed codeLens, got: {err:?}"
    );
}

// ── 4b. Position validation with a real file ──────────────────────────
// These tests supply a real file so resolve_file passes, then verify the
// line/column errors.

#[tokio::test]
async fn lsp_execute_missing_line_with_file() {
    let (_dir, path) = temp_rs_file("fn main() {}");
    let tool = make_tool_with_root(_dir.path());
    let err = tool
        .execute(serde_json::json!({
            "operation": "goToDefinition",
            "file_path": path.to_str().unwrap(),
            "column": 1
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("line")),
        "expected line error, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_missing_column_with_file() {
    let (_dir, path) = temp_rs_file("fn main() {}");
    let tool = make_tool_with_root(_dir.path());
    let err = tool
        .execute(serde_json::json!({
            "operation": "goToDefinition",
            "file_path": path.to_str().unwrap(),
            "line": 1
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("column")),
        "expected column error, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_find_references_missing_line_with_file() {
    let (_dir, path) = temp_rs_file("fn main() {}");
    let tool = make_tool_with_root(_dir.path());
    let err = tool
        .execute(serde_json::json!({
            "operation": "findReferences",
            "file_path": path.to_str().unwrap(),
            "column": 1
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("line")),
        "expected line error for findReferences, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_hover_missing_line_with_file() {
    let (_dir, path) = temp_rs_file("fn main() {}");
    let tool = make_tool_with_root(_dir.path());
    let err = tool
        .execute(serde_json::json!({
            "operation": "hover",
            "file_path": path.to_str().unwrap(),
            "column": 1
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("line")),
        "expected line error for hover, got: {err:?}"
    );
}

// ── 5. Unknown operation ──────────────────────────────────────────────

#[tokio::test]
async fn lsp_execute_unknown_operation() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": "nonExistentOp"}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("unknown LSP operation")),
        "expected unknown operation error, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_execute_empty_operation_string() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({"operation": ""}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("unknown LSP operation")),
        "expected unknown operation error for empty string, got: {err:?}"
    );
}

// ── 6. Diagnostics parser ─────────────────────────────────────────────

#[test]
fn parse_publish_diagnostics_valid_json() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": "file:///src/main.rs",
            "diagnostics": [
                {
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 10 }
                    },
                    "message": "unused variable",
                    "severity": 2
                }
            ]
        }
    });
    let result = parse_publish_diagnostics(&json.to_string());
    assert!(result.is_some());
    let (uri, diags) = result.unwrap();
    assert_eq!(uri, "file:///src/main.rs");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].message, "unused variable");
}

#[test]
fn parse_publish_diagnostics_unknown_notification() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/completion",
        "params": {}
    });
    assert!(parse_publish_diagnostics(&json.to_string()).is_none());
}

#[test]
fn parse_publish_diagnostics_malformed_json() {
    assert!(parse_publish_diagnostics("not json at all").is_none());
}

#[test]
fn parse_publish_diagnostics_empty_string() {
    assert!(parse_publish_diagnostics("").is_none());
}

#[test]
fn parse_publish_diagnostics_empty_diagnostics_array() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": "file:///src/main.rs",
            "diagnostics": []
        }
    });
    let result = parse_publish_diagnostics(&json.to_string());
    assert!(result.is_some());
    let (_, diags) = result.unwrap();
    assert!(diags.is_empty());
}

#[test]
fn parse_publish_diagnostics_missing_params() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics"
    });
    assert!(parse_publish_diagnostics(&json.to_string()).is_none());
}

#[test]
fn parse_publish_diagnostics_missing_uri() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "diagnostics": []
        }
    });
    assert!(parse_publish_diagnostics(&json.to_string()).is_none());
}

#[test]
fn parse_publish_diagnostics_multiple_diagnostics() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": "file:///src/lib.rs",
            "diagnostics": [
                {
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 5 }
                    },
                    "message": "error one",
                    "severity": 1
                },
                {
                    "range": {
                        "start": { "line": 2, "character": 4 },
                        "end": { "line": 2, "character": 12 }
                    },
                    "message": "warning two",
                    "severity": 2
                }
            ]
        }
    });
    let result = parse_publish_diagnostics(&json.to_string());
    assert!(result.is_some());
    let (uri, diags) = result.unwrap();
    assert_eq!(uri, "file:///src/lib.rs");
    assert_eq!(diags.len(), 2);
    assert_eq!(diags[0].message, "error one");
    assert_eq!(diags[1].message, "warning two");
}

#[test]
fn parse_publish_diagnostics_json_array_not_object() {
    assert!(parse_publish_diagnostics("[1, 2, 3]").is_none());
}

#[test]
fn parse_publish_diagnostics_number_not_string() {
    assert!(parse_publish_diagnostics("42").is_none());
}

// ── 7. Disabled LSP ───────────────────────────────────────────────────

#[tokio::test]
async fn lsp_disabled_config_rejects_clients() {
    let service = std::sync::Arc::new(codegg::lsp::service::LspService::new(
        codegg::lsp::config::LspConfig::Disabled(true),
    ));
    let tool = codegg::tool::lsp::LspTool::new(service);
    // No file_path → "file_path required" error (before LSP is even invoked)
    let err = tool
        .execute(serde_json::json!({"operation": "goToDefinition"}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref msg) if msg.contains("file_path")),
        "expected file_path error with disabled config, got: {err:?}"
    );
}

#[tokio::test]
async fn lsp_unsupported_language_path_error() {
    let tool = make_tool();
    // .xyz123 has no language mapping; path validation also fails (file doesn't exist)
    let err = tool
        .execute(serde_json::json!({
            "operation": "goToDefinition",
            "file_path": "noext",
            "line": 1,
            "column": 1
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(_)),
        "expected Execution error, got: {err:?}"
    );
}

// ── 8. Structured execution provenance ────────────────────────────────

#[tokio::test]
async fn lsp_execute_structured_returns_error_for_unknown_op() {
    let tool = make_tool();
    let result = tool
        .execute_structured(serde_json::json!({"operation": "nonExistentOp"}), None)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn lsp_execute_structured_missing_operation() {
    let tool = make_tool();
    let result = tool.execute_structured(serde_json::json!({}), None).await;
    assert!(result.is_err());
}

// ── 9. Tool trait default behavior ────────────────────────────────────

#[test]
fn lsp_tool_expose_in_definitions() {
    let tool = make_tool();
    assert!(tool.expose_in_definitions());
}

#[test]
fn lsp_tool_defer_loading_is_false() {
    let tool = make_tool();
    assert!(!tool.defer_loading());
}
