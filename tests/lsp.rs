use codegg::error::ToolError;
use codegg::lsp::client::parse_publish_diagnostics;
use codegg::lsp::diagnostics::DiagnosticsOutput;
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
        "renamePreview",
        "formatPreview",
        "sourceActionPreview",
        "semanticCheckPreview",
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

#[tokio::test]
#[allow(non_snake_case)]
async fn lsp_schema_includes_renamePreview_and_formatPreview() {
    let tool = make_tool();
    let params = tool.parameters();
    let ops = params["properties"]["operation"]["enum"]
        .as_array()
        .expect("operation.enum should be an array");
    assert!(ops.iter().any(|v| v.as_str() == Some("renamePreview")));
    assert!(ops.iter().any(|v| v.as_str() == Some("formatPreview")));
}

#[tokio::test]
#[allow(non_snake_case)]
async fn lsp_schema_includes_sourceActionPreview() {
    let tool = make_tool();
    let params = tool.parameters();
    let ops = params["properties"]["operation"]["enum"]
        .as_array()
        .expect("operation.enum should be an array");
    assert!(ops
        .iter()
        .any(|v| v.as_str() == Some("sourceActionPreview")));
    let action_param = params["properties"]["action"]
        .as_object()
        .expect("action property should be an object");
    assert!(action_param.get("description").is_some());
}

#[tokio::test]
#[allow(non_snake_case)]
async fn renamePreview_requires_new_name() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "renamePreview",
            "file_path": "src/main.rs",
            "line": 1,
            "column": 1
        }))
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::Execution(ref m) if m.contains("new_name")));
}

#[tokio::test]
#[allow(non_snake_case)]
async fn renamePreview_requires_file_path_line_column() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "renamePreview",
            "new_name": "foo"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("file_path") || m.contains("line") || m.contains("column"))
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn formatPreview_requires_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "formatPreview"
        }))
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::Execution(ref m) if m.contains("file_path")));
}

// ── 2. LSP tool is ReadOnly ───────────────────────────────────────────

#[test]
fn lsp_tool_category_is_read_only() {
    let tool = make_tool();
    assert_eq!(tool.category(), ToolCategory::ReadOnly);
}

#[test]
fn lsp_tool_remains_read_only() {
    let tool = make_tool();
    assert_eq!(tool.category(), ToolCategory::ReadOnly);
}

#[test]
#[allow(non_snake_case)]
fn renamePreview_is_read_only_tool_category() {
    let tool = make_tool();
    assert_eq!(tool.category(), ToolCategory::ReadOnly);
}

#[test]
#[allow(non_snake_case)]
fn codeLens_still_not_exposed() {
    let tool = make_tool();
    let params = tool.parameters();
    let ops = params["properties"]["operation"]["enum"]
        .as_array()
        .expect("operation.enum");
    assert!(!ops.iter().any(|v| v.as_str() == Some("codeLens")));
    assert!(!tool.description().contains("codeLens"));
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
        matches!(err, ToolError::Execution(ref msg) if msg.contains("line") || msg.contains("permission denied") || msg.contains("symlink")),
        "expected line error (or path permission on symlinked tempdir), got: {err:?}"
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
        matches!(err, ToolError::Execution(ref msg) if msg.contains("column") || msg.contains("permission denied") || msg.contains("symlink")),
        "expected column error (or path permission on symlinked tempdir), got: {err:?}"
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
        matches!(err, ToolError::Execution(ref msg) if msg.contains("line") || msg.contains("permission denied") || msg.contains("symlink")),
        "expected line error for findReferences (or path permission on symlinked tempdir), got: {err:?}"
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
        matches!(err, ToolError::Execution(ref msg) if msg.contains("line") || msg.contains("permission denied") || msg.contains("symlink")),
        "expected line error for hover (or path permission on symlinked tempdir), got: {err:?}"
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

// ── 10. DiagnosticsOutput warming field ────────────────────────────────

#[test]
fn diagnostics_output_has_warming_field() {
    let output = DiagnosticsOutput {
        diagnostics_may_still_be_warming: true,
        diagnostics: Vec::new(),
    };
    assert!(output.diagnostics_may_still_be_warming);
    assert!(output.diagnostics.is_empty());
}

#[test]
fn diagnostics_output_clean_not_warming() {
    let output = DiagnosticsOutput {
        diagnostics_may_still_be_warming: false,
        diagnostics: Vec::new(),
    };
    assert!(!output.diagnostics_may_still_be_warming);
    assert!(output.diagnostics.is_empty());
}

// ── 11. Stale launch read helpers removed ──────────────────────────────

#[test]
fn stale_launch_read_helpers_removed() {
    // read_response and read_notification were removed in the hardening pass.
    // This test ensures they don't exist as public items in the launch module.
    let _ = std::any::type_name::<codegg::lsp::launch::LspProcess>(); // still exists
}

// ── 12. sourceActionPreview tests ─────────────────────────────────────

#[test]
#[allow(non_snake_case)]
fn source_action_kind_accepts_source_organize_imports() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    let kind = SourceActionPreviewKind::parse("source.organizeImports").unwrap();
    assert_eq!(kind, SourceActionPreviewKind::OrganizeImports);
}

#[test]
#[allow(non_snake_case)]
fn source_action_kind_accepts_aliases() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    assert_eq!(
        SourceActionPreviewKind::parse("organizeImports").unwrap(),
        SourceActionPreviewKind::OrganizeImports
    );
    assert_eq!(
        SourceActionPreviewKind::parse("organize_imports").unwrap(),
        SourceActionPreviewKind::OrganizeImports
    );
}

#[test]
#[allow(non_snake_case)]
fn source_action_kind_rejects_fix_all() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    let err = SourceActionPreviewKind::parse("source.fixAll").unwrap_err();
    assert!(
        matches!(err, codegg::lsp::LspError::UnsupportedSourceAction(ref m) if m == "source.fixAll"),
        "expected UnsupportedSourceAction for source.fixAll, got: {err:?}"
    );
}

#[test]
#[allow(non_snake_case)]
fn source_action_kind_rejects_quickfix() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    let err = SourceActionPreviewKind::parse("quickfix").unwrap_err();
    assert!(
        matches!(err, codegg::lsp::LspError::UnsupportedSourceAction(ref m) if m == "quickfix"),
        "expected UnsupportedSourceAction for quickfix, got: {err:?}"
    );
}

#[test]
#[allow(non_snake_case)]
fn source_action_kind_rejects_unknown() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    let err = SourceActionPreviewKind::parse("source.someFutureThing").unwrap_err();
    assert!(
        matches!(err, codegg::lsp::LspError::UnsupportedSourceAction(_)),
        "expected UnsupportedSourceAction, got: {err:?}"
    );
}

#[test]
#[allow(non_snake_case)]
fn select_source_action_rejects_command_only() {
    use codegg::lsp::lsp_types::{CodeActionOrCommand, Command};
    use codegg::lsp::operations::{select_source_action_edit, SourceActionPreviewKind};
    let actions = vec![CodeActionOrCommand::Command(Command {
        title: "organize imports".into(),
        command: "source.organizeImports".into(),
        arguments: None,
    })];
    let err =
        select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions).unwrap_err();
    assert!(
        matches!(err, codegg::lsp::LspError::CommandOnlySourceAction(_)),
        "expected CommandOnlySourceAction, got: {err:?}"
    );
}

#[test]
#[allow(non_snake_case)]
fn select_source_action_rejects_no_edit() {
    use codegg::lsp::lsp_types::{CodeAction, CodeActionKind, CodeActionOrCommand};
    use codegg::lsp::operations::{select_source_action_edit, SourceActionPreviewKind};
    let actions = vec![CodeActionOrCommand::CodeAction(CodeAction {
        title: "organize imports".into(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        edit: None,
        ..Default::default()
    })];
    let err =
        select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions).unwrap_err();
    assert!(
        matches!(err, codegg::lsp::LspError::NoEditForSourceAction(_)),
        "expected NoEditForSourceAction, got: {err:?}"
    );
}

#[test]
#[allow(non_snake_case, clippy::mutable_key_type)]
fn select_source_action_selects_single_edit_bearing() {
    use codegg::lsp::lsp_types::{
        CodeAction, CodeActionKind, CodeActionOrCommand, TextEdit, WorkspaceEdit,
    };
    use codegg::lsp::operations::{select_source_action_edit, SourceActionPreviewKind};
    use std::collections::HashMap;
    let mut changes = HashMap::new();
    changes.insert(
        "file:///tmp/test.rs".parse().unwrap(),
        vec![TextEdit {
            range: codegg::lsp::lsp_types::Range::default(),
            new_text: "// organized".into(),
        }],
    );
    let actions = vec![CodeActionOrCommand::CodeAction(CodeAction {
        title: "organize imports".into(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })];
    let ws_edit =
        select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions).unwrap();
    assert!(ws_edit.changes.as_ref().unwrap().len() == 1);
}

#[test]
#[allow(non_snake_case, clippy::mutable_key_type)]
fn select_source_action_rejects_ambiguous_multiple_edits() {
    use codegg::lsp::lsp_types::{
        CodeAction, CodeActionKind, CodeActionOrCommand, TextEdit, WorkspaceEdit,
    };
    use codegg::lsp::operations::{select_source_action_edit, SourceActionPreviewKind};
    use std::collections::HashMap;
    let make_edit = || {
        let mut changes = HashMap::new();
        changes.insert(
            "file:///tmp/test.rs".parse().unwrap(),
            vec![TextEdit {
                range: codegg::lsp::lsp_types::Range::default(),
                new_text: "// organized".into(),
            }],
        );
        WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }
    };
    let actions = vec![
        CodeActionOrCommand::CodeAction(CodeAction {
            title: "organize imports (rustfmt)".into(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            edit: Some(make_edit()),
            ..Default::default()
        }),
        CodeActionOrCommand::CodeAction(CodeAction {
            title: "organize imports (rust-analyzer)".into(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            edit: Some(make_edit()),
            ..Default::default()
        }),
    ];
    let err =
        select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions).unwrap_err();
    assert!(
        matches!(err, codegg::lsp::LspError::AmbiguousSourceAction(_, _)),
        "expected AmbiguousSourceAction, got: {err:?}"
    );
}

#[test]
#[allow(non_snake_case)]
fn select_source_action_rejects_code_action_command_only() {
    use codegg::lsp::lsp_types::{CodeAction, CodeActionKind, CodeActionOrCommand, Command};
    use codegg::lsp::operations::{select_source_action_edit, SourceActionPreviewKind};
    let actions = vec![CodeActionOrCommand::CodeAction(CodeAction {
        title: "organize imports".into(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        edit: None,
        command: Some(Command {
            title: "organize imports".into(),
            command: "source.organizeImports".into(),
            arguments: None,
        }),
        ..Default::default()
    })];
    let err =
        select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions).unwrap_err();
    assert!(
        matches!(err, codegg::lsp::LspError::CommandOnlySourceAction(_)),
        "expected CommandOnlySourceAction for CodeAction with command but no edit, got: {err:?}"
    );
}

#[test]
#[allow(non_snake_case, clippy::mutable_key_type)]
fn select_source_action_ignores_nonmatching_actions() {
    use codegg::lsp::lsp_types::{
        CodeAction, CodeActionKind, CodeActionOrCommand, TextEdit, WorkspaceEdit,
    };
    use codegg::lsp::operations::{select_source_action_edit, SourceActionPreviewKind};
    use std::collections::HashMap;
    let mut changes = HashMap::new();
    changes.insert(
        "file:///tmp/test.rs".parse().unwrap(),
        vec![TextEdit {
            range: codegg::lsp::lsp_types::Range::default(),
            new_text: "// organized".into(),
        }],
    );
    let actions = vec![
        CodeActionOrCommand::CodeAction(CodeAction {
            title: "quickfix something".into(),
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(WorkspaceEdit {
                changes: Some(changes.clone()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        CodeActionOrCommand::CodeAction(CodeAction {
            title: "organize imports".into(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        }),
    ];
    let ws_edit =
        select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions).unwrap();
    assert!(ws_edit.changes.as_ref().unwrap().len() == 1);
}

#[test]
#[allow(non_snake_case, clippy::mutable_key_type)]
fn select_source_action_accepts_child_kind() {
    use codegg::lsp::lsp_types::{
        CodeAction, CodeActionKind, CodeActionOrCommand, TextEdit, WorkspaceEdit,
    };
    use codegg::lsp::operations::{select_source_action_edit, SourceActionPreviewKind};
    use std::collections::HashMap;
    let mut changes = HashMap::new();
    changes.insert(
        "file:///tmp/test.rs".parse().unwrap(),
        vec![TextEdit {
            range: codegg::lsp::lsp_types::Range::default(),
            new_text: "// organized".into(),
        }],
    );
    let actions = vec![CodeActionOrCommand::CodeAction(CodeAction {
        title: "organize imports (rustfmt)".into(),
        kind: Some(CodeActionKind::new("source.organizeImports.rustfmt")),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })];
    let ws_edit =
        select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions).unwrap();
    assert!(ws_edit.changes.as_ref().unwrap().len() == 1);
}

#[test]
#[allow(non_snake_case)]
fn document_end_position_utf16_empty() {
    use codegg::lsp::operations::document_end_position_utf16;
    let pos = document_end_position_utf16("");
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 0);
}

#[test]
#[allow(non_snake_case)]
fn document_end_position_utf16_single_line_ascii() {
    use codegg::lsp::operations::document_end_position_utf16;
    let pos = document_end_position_utf16("hello");
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 5);
}

#[test]
#[allow(non_snake_case)]
fn document_end_position_utf16_ends_with_newline() {
    use codegg::lsp::operations::document_end_position_utf16;
    let pos = document_end_position_utf16("hello\n");
    assert_eq!(pos.line, 1);
    assert_eq!(pos.character, 0);
}

#[test]
#[allow(non_snake_case)]
fn document_end_position_utf16_multiline() {
    use codegg::lsp::operations::document_end_position_utf16;
    let pos = document_end_position_utf16("line1\nline2\nline3");
    assert_eq!(pos.line, 2);
    assert_eq!(pos.character, 5);
}

#[test]
#[allow(non_snake_case)]
fn document_end_position_utf16_crlf() {
    use codegg::lsp::operations::document_end_position_utf16;
    // \r is counted as a character before \n resets the line.
    // "a\r\n" → line 1, character 0
    let pos = document_end_position_utf16("a\r\n");
    assert_eq!(pos.line, 1);
    assert_eq!(pos.character, 0);
    // "\r\n" alone → line 1, character 0
    let pos = document_end_position_utf16("\r\n");
    assert_eq!(pos.line, 1);
    assert_eq!(pos.character, 0);
}

#[test]
#[allow(non_snake_case)]
fn document_end_position_utf16_unicode() {
    use codegg::lsp::operations::document_end_position_utf16;
    // emoji is 2 UTF-16 code units, 'a' is 1
    let pos = document_end_position_utf16("a");
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 1);
    let pos = document_end_position_utf16("\u{1F600}");
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 2);
}

#[tokio::test]
#[allow(non_snake_case)]
async fn sourceActionPreview_requires_action() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "sourceActionPreview",
            "file_path": "src/main.rs"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("action")),
        "expected action error, got: {err:?}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn sourceActionPreview_requires_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "sourceActionPreview",
            "action": "source.organizeImports"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("file_path")),
        "expected file_path error, got: {err:?}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn sourceActionPreview_rejects_unsupported_action_without_lsp_request() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "sourceActionPreview",
            "file_path": "src/main.rs",
            "action": "source.fixAll"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("unsupported source action")),
        "expected unsupported action error, got: {err:?}"
    );
}

// ── 13. semanticCheckPreview tests ──────────────────────────────────

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticCheckPreview_requires_content_or_patch() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "semanticCheckPreview",
            "file_path": "src/main.rs"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("content or patch")),
        "expected content-or-patch error, got: {err:?}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticCheckPreview_requires_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "semanticCheckPreview",
            "patch": "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() {}\n"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("file_path")),
        "expected file_path error, got: {err:?}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticCheckPreview_rejects_content_and_patch() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "semanticCheckPreview",
            "file_path": "src/main.rs",
            "content": "fn main() {}",
            "patch": "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() {}\n"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("either content or patch, not both")),
        "expected content-and-patch error, got: {err:?}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticCheckPreview_rejects_invalid_patch() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "semanticCheckPreview",
            "file_path": "src/main.rs",
            "patch": "@@ -1,1 +1,1 @@\n x\n"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("semanticCheckPreview patch failed")),
        "expected invalid patch error, got: {err:?}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticCheckPreview_rejects_multi_file_patch() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "semanticCheckPreview",
            "file_path": "src/main.rs",
            "patch": "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,1 +1,1 @@
-fn main() {}
+fn main() {}
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,1 +1,1 @@
-fn lib() {}
+fn lib() {}
"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("single-file patches")),
        "expected single-file patch guardrail, got: {err:?}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticCheckPreview_does_not_mutate_disk() {
    let (_dir, path) = temp_rs_file("fn main() {\n    println!(\"old\");\n}\n");
    let original = std::fs::read_to_string(&path).unwrap();
    let tool = make_tool_with_root(_dir.path());
    let _ = tool
        .execute(serde_json::json!({
            "operation": "semanticCheckPreview",
            "file_path": path.to_str().unwrap(),
            "patch": "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn main() {
-    println!(\"old\");
+    println!(\"new\");
 }
"
        }))
        .await;
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        after, original,
        "semanticCheckPreview must not write patched content to disk"
    );
}

#[test]
#[allow(non_snake_case)]
fn semanticCheckPreview_is_read_only() {
    let tool = make_tool();
    assert_eq!(tool.category(), ToolCategory::ReadOnly);
}

#[test]
#[allow(non_snake_case)]
fn semanticCheckPreview_schema_includes_error_fields() {
    let tool = make_tool();
    let schema = tool.parameters();
    let op_enum = schema["properties"]["operation"]["enum"]
        .as_array()
        .unwrap();
    assert!(op_enum.iter().any(|v| v == "semanticCheckPreview"));
    assert!(schema["properties"]["content"].is_object());
    let patch = schema["properties"]["patch"].as_object().unwrap();
    assert!(patch
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap()
        .contains("Single-file unified diff"));
}
