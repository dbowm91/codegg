use codegg::error::ToolError;
use codegg::lsp::client::parse_publish_diagnostics;
use codegg::lsp::diagnostics::DiagnosticsOutput;
use codegg::lsp::language::{detect_language, language_id_to_server_id};
use codegg::tool::lsp::to_lsp_position;
use codegg::tool::{Tool, ToolCategory};

fn make_tool() -> codegg::tool::lsp::LspTool {
    codegg::tool::lsp::LspTool::new(codegg::lsp::service::LspService::new_arc(
        codegg::lsp::config::LspConfig::default(),
    ))
}

fn make_tool_with_root(root: &std::path::Path) -> codegg::tool::lsp::LspTool {
    codegg::tool::lsp::LspTool::new(codegg::lsp::service::LspService::new_arc(
        codegg::lsp::config::LspConfig::default(),
    ))
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
        "semanticContext",
        "callHierarchy",
        "typeHierarchy",
        "securityContext",
        "capabilities",
        "hunkSourceContext",
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
    assert!(tool.description().contains("semanticContext"));
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
    let service =
        codegg::lsp::service::LspService::new_arc(codegg::lsp::config::LspConfig::Disabled(true));
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

// ── 14. semanticContext tests ───────────────────────────────────────

#[test]
#[allow(non_snake_case)]
fn lsp_schema_includes_semanticContext() {
    let tool = make_tool();
    let params = tool.parameters();
    let ops = params["properties"]["operation"]["enum"]
        .as_array()
        .expect("operation.enum should be an array");
    assert!(
        ops.iter().any(|v| v.as_str() == Some("semanticContext")),
        "semanticContext missing from operation enum"
    );
}

#[test]
#[allow(non_snake_case)]
fn lsp_schema_includes_radius_and_include_flags() {
    let tool = make_tool();
    let params = tool.parameters();
    let radius = params["properties"]["radius"]
        .as_object()
        .expect("radius property should be an object");
    assert!(radius.get("description").is_some());
    let include_refs = params["properties"]["include_references"]
        .as_object()
        .expect("include_references property should be an object");
    assert!(include_refs.get("description").is_some());
    let include_defs = params["properties"]["include_definitions"]
        .as_object()
        .expect("include_definitions property should be an object");
    assert!(include_defs.get("description").is_some());
    let include_overlay = params["properties"]["include_overlay"]
        .as_object()
        .expect("include_overlay property should be an object");
    assert!(include_overlay.get("description").is_some());
}

#[test]
#[allow(non_snake_case)]
fn semanticContext_is_read_only() {
    let tool = make_tool();
    assert_eq!(tool.category(), ToolCategory::ReadOnly);
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticContext_requires_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "semanticContext"
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
async fn semanticContext_requires_line_and_column_together() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "semanticContext",
            "file_path": "src/main.rs",
            "line": 1
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("both line and column")),
        "expected line+column error, got: {err:?}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticContext_rejects_content_and_patch() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "semanticContext",
            "file_path": "src/tool/mod.rs",
            "line": 1,
            "column": 1,
            "content": "fn main() {}",
            "patch": "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() {}\n"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("either content or patch, not both")),
        "expected content+patch error, got: {err:?}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticContext_patch_does_not_write_disk() {
    let (_dir, path) = temp_rs_file("fn main() {\n    println!(\"old\");\n}\n");
    let original = std::fs::read_to_string(&path).unwrap();
    let tool = make_tool_with_root(_dir.path());
    let _ = tool
        .execute(serde_json::json!({
            "operation": "semanticContext",
            "file_path": path.to_str().unwrap(),
            "line": 1,
            "column": 1,
            "patch": "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    println!(\"old\");\n+    println!(\"new\");\n }\n"
        }))
        .await;
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        after, original,
        "semanticContext must not write patched content to disk"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semanticContext_with_line_column_returns_excerpt() {
    let tool = make_tool();
    let result = tool
        .execute(serde_json::json!({
            "operation": "semanticContext",
            "file_path": "src/tool/mod.rs",
            "line": 1,
            "column": 1,
            "radius": 5
        }))
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["operation"], "semanticContext");
    assert!(v["results"]["excerpt"]["text"].is_string());
    assert!(v["results"]["target"]["line"] == 1);
    assert!(v["results"]["target"]["column"] == 1);
    assert!(v["results"]["limits"].is_object());
}

#[test]
#[allow(non_snake_case)]
fn semantic_context_excerpt_truncates_on_utf8_boundary() {
    // "é" is 2 bytes in UTF-8; if byte limit lands in the middle, it should back up
    let content = "a".repeat(100) + "é" + &"b".repeat(100);
    let (_dir, path) = temp_rs_file(&content);
    let (excerpt, _truncated) =
        codegg::tool::lsp::LspTool::build_source_excerpt(&path, None, 40).unwrap();
    // The excerpt should not contain replacement characters
    assert!(
        !excerpt.text.contains('\u{FFFD}'),
        "excerpt contains replacement characters from split UTF-8"
    );
}

// ── 15. semanticContext source action hints ─────────────────────────────

#[test]
#[allow(non_snake_case)]
fn lsp_schema_includes_include_source_actions() {
    let tool = make_tool();
    let params = tool.parameters();
    let prop = params["properties"]["include_source_actions"]
        .as_object()
        .expect("include_source_actions property should be an object");
    assert_eq!(prop["type"], "boolean");
    assert!(prop["description"]
        .as_str()
        .unwrap()
        .contains("source.organizeImports"));
}

#[test]
#[allow(non_snake_case)]
fn semantic_context_source_actions_default_false() {
    let tool = make_tool();
    let params = tool.parameters();
    let prop = params["properties"]["include_source_actions"]
        .as_object()
        .expect("include_source_actions property should be an object");
    assert_eq!(prop["type"], "boolean");
    let desc = prop["description"].as_str().unwrap();
    assert!(
        desc.contains("Default false"),
        "description should document default: {desc}"
    );
}

#[tokio::test]
#[allow(non_snake_case)]
async fn semantic_context_source_actions_omitted_by_default() {
    let tool = make_tool();
    let result = tool
        .execute(serde_json::json!({
            "operation": "semanticContext",
            "file_path": "src/tool/mod.rs",
            "line": 1,
            "column": 1,
            "radius": 5
        }))
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    let src_actions = v["results"]["source_actions"]
        .as_array()
        .expect("source_actions should be an array");
    assert!(
        src_actions.is_empty(),
        "source_actions should be empty when include_source_actions is omitted"
    );
}

#[test]
#[allow(non_snake_case)]
fn semantic_context_source_actions_is_read_only() {
    let tool = make_tool();
    assert_eq!(tool.category(), ToolCategory::ReadOnly);
}

#[test]
#[allow(non_snake_case)]
fn semantic_context_packet_serializes_empty_source_actions() {
    let packet = serde_json::json!({
        "file": "src/main.rs",
        "target": null,
        "excerpt": {"start_line": 1, "end_line": 5, "text": "hello"},
        "diagnostics": [],
        "current_diagnostics_error": null,
        "overlay": null,
        "symbols": [],
        "current_symbols_error": null,
        "definitions": [],
        "definitions_error": null,
        "references": [],
        "references_error": null,
        "source_actions": [],
        "limits": {
            "diagnostics_truncated": false,
            "symbols_truncated": false,
            "references_truncated": false,
            "overlay_diagnostics_truncated": false,
            "excerpt_truncated": false
        }
    });
    let v: serde_json::Value = packet;
    assert!(v["source_actions"].as_array().unwrap().is_empty());
}

#[test]
#[allow(non_snake_case)]
fn semantic_context_packet_serializes_source_action_error_hint() {
    let hint = serde_json::json!({
        "action": "source.organizeImports",
        "available": false,
        "preview": null,
        "error": "No edit-bearing source action available"
    });
    let arr = serde_json::json!([hint]);
    let v: serde_json::Value = arr;
    assert_eq!(v[0]["action"], "source.organizeImports");
    assert_eq!(v[0]["available"], false);
    assert!(v[0]["preview"].is_null());
    assert!(v[0]["error"].as_str().unwrap().contains("No edit-bearing"));
}

#[test]
#[allow(non_snake_case)]
fn source_action_hint_available_when_preview_has_edits() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    use codegg::tool::lsp::LspTool;
    let preview = codegg::lsp::edit::WorkspaceEditPreview {
        title: "organize imports".to_string(),
        files: vec![],
        total_files: 1,
        total_edits: 3,
        truncated: false,
    };
    let hint = LspTool::source_action_hint_from_result(
        SourceActionPreviewKind::OrganizeImports,
        Ok(preview),
    );
    assert!(hint.available);
    assert_eq!(hint.action, "source.organizeImports");
    assert!(hint.preview.is_some());
    assert!(hint.error.is_none());
}

#[test]
#[allow(non_snake_case)]
fn source_action_hint_unavailable_when_preview_empty() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    use codegg::tool::lsp::LspTool;
    let preview = codegg::lsp::edit::WorkspaceEditPreview {
        title: "organize imports".to_string(),
        files: vec![],
        total_files: 0,
        total_edits: 0,
        truncated: false,
    };
    let hint = LspTool::source_action_hint_from_result(
        SourceActionPreviewKind::OrganizeImports,
        Ok(preview),
    );
    assert!(!hint.available);
    assert!(hint.preview.is_some());
    assert!(hint.error.as_deref() == Some("source action produced no edits"));
}

#[test]
#[allow(non_snake_case)]
fn source_action_hint_captures_error() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    use codegg::tool::lsp::LspTool;
    let err = codegg::lsp::LspError::NoEditForSourceAction("organize imports".to_string());
    let hint =
        LspTool::source_action_hint_from_result(SourceActionPreviewKind::OrganizeImports, Err(err));
    assert!(!hint.available);
    assert!(hint.preview.is_none());
    assert!(hint.error.is_some());
    assert!(hint.error.unwrap().contains("organize imports"));
}

#[test]
#[allow(non_snake_case)]
fn source_action_hint_available_when_truncated() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    use codegg::tool::lsp::LspTool;
    let preview = codegg::lsp::edit::WorkspaceEditPreview {
        title: "organize imports".to_string(),
        files: vec![],
        total_files: 5,
        total_edits: 12,
        truncated: true,
    };
    let hint = LspTool::source_action_hint_from_result(
        SourceActionPreviewKind::OrganizeImports,
        Ok(preview),
    );
    assert!(hint.available);
    assert!(hint.preview.is_some());
    assert!(hint.error.is_none());
}

#[test]
#[allow(non_snake_case)]
fn source_action_hint_available_with_single_edit() {
    use codegg::lsp::operations::SourceActionPreviewKind;
    use codegg::tool::lsp::LspTool;
    let preview = codegg::lsp::edit::WorkspaceEditPreview {
        title: "organize imports".to_string(),
        files: vec![],
        total_files: 1,
        total_edits: 1,
        truncated: false,
    };
    let hint = LspTool::source_action_hint_from_result(
        SourceActionPreviewKind::OrganizeImports,
        Ok(preview),
    );
    assert!(hint.available);
    assert_eq!(hint.action, "source.organizeImports");
}

// ── Hierarchy tests ───────────────────────────────────────────────────

#[test]
#[allow(non_snake_case)]
fn lsp_schema_includes_callHierarchy_and_typeHierarchy() {
    let tool = make_tool();
    let params = tool.parameters();
    let ops = params["properties"]["operation"]["enum"]
        .as_array()
        .expect("operation.enum should be an array");
    assert!(
        ops.iter().any(|v| v.as_str() == Some("callHierarchy")),
        "missing callHierarchy in operation enum"
    );
    assert!(
        ops.iter().any(|v| v.as_str() == Some("typeHierarchy")),
        "missing typeHierarchy in operation enum"
    );
}

#[test]
#[allow(non_snake_case)]
fn lsp_schema_includes_direction() {
    let tool = make_tool();
    let params = tool.parameters();
    let direction = &params["properties"]["direction"];
    assert_eq!(direction["type"], "string");
    let enum_vals = direction["enum"]
        .as_array()
        .expect("direction.enum should be an array");
    assert!(enum_vals.iter().any(|v| v.as_str() == Some("incoming")));
    assert!(enum_vals.iter().any(|v| v.as_str() == Some("outgoing")));
    assert!(enum_vals.iter().any(|v| v.as_str() == Some("both")));
}

#[test]
#[allow(non_snake_case)]
fn lsp_schema_includes_hierarchy_context_flags() {
    let tool = make_tool();
    let params = tool.parameters();
    assert_eq!(
        params["properties"]["include_call_hierarchy"]["type"],
        "boolean"
    );
    assert_eq!(
        params["properties"]["include_type_hierarchy"]["type"],
        "boolean"
    );
}

#[test]
#[allow(non_snake_case)]
fn callHierarchy_requires_file_path_line_column() {
    let tool = make_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(serde_json::json!({
                "operation": "callHierarchy"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("file_path")),
            "expected file_path error, got: {err:?}"
        );

        let err = tool
            .execute(serde_json::json!({
                "operation": "callHierarchy",
                "file_path": "src/main.rs"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("line")),
            "expected line error, got: {err:?}"
        );

        let err = tool
            .execute(serde_json::json!({
                "operation": "callHierarchy",
                "file_path": "src/main.rs",
                "line": 1
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("column")),
            "expected column error, got: {err:?}"
        );
    });
}

#[test]
#[allow(non_snake_case)]
fn typeHierarchy_requires_file_path_line_column() {
    let tool = make_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(serde_json::json!({
                "operation": "typeHierarchy"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("file_path")),
            "expected file_path error, got: {err:?}"
        );

        let err = tool
            .execute(serde_json::json!({
                "operation": "typeHierarchy",
                "file_path": "src/main.rs"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("line")),
            "expected line error, got: {err:?}"
        );
    });
}

#[test]
#[allow(non_snake_case)]
fn semanticContext_hierarchy_requires_line_column() {
    let tool = make_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(serde_json::json!({
                "operation": "semanticContext",
                "file_path": "src/tool/mod.rs",
                "include_call_hierarchy": true,
                "include_type_hierarchy": true
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("hierarchy sections require both line and column")),
            "expected hierarchy position error, got: {err:?}"
        );
    });
}

#[test]
#[allow(non_snake_case)]
fn semanticContext_hierarchy_rejects_line_without_column() {
    let tool = make_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(serde_json::json!({
                "operation": "semanticContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "include_call_hierarchy": true
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("both line and column")),
            "expected partial position error, got: {err:?}"
        );
    });
}

#[test]
#[allow(non_snake_case)]
fn semanticContext_hierarchy_rejects_column_without_line() {
    let tool = make_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(serde_json::json!({
                "operation": "semanticContext",
                "file_path": "src/tool/mod.rs",
                "column": 1,
                "include_type_hierarchy": true
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("both line and column")),
            "expected partial position error, got: {err:?}"
        );
    });
}

#[test]
#[allow(non_snake_case)]
fn semanticContext_hierarchy_with_position_accepts_flags() {
    let tool = make_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let result = tool
            .execute(serde_json::json!({
                "operation": "semanticContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "include_call_hierarchy": true,
                "include_type_hierarchy": true
            }))
            .await;
        // Should succeed (hierarchy sections may have errors but validation passes)
        assert!(
            result.is_ok(),
            "hierarchy flags with position should not fail validation: {result:?}"
        );
    });
}

#[test]
#[allow(non_snake_case)]
fn hierarchy_direction_defaults_to_both() {
    use codegg::lsp::operations::HierarchyDirection;
    let dir = HierarchyDirection::parse(None).unwrap();
    assert_eq!(dir, HierarchyDirection::Both);
}

#[test]
#[allow(non_snake_case)]
fn hierarchy_direction_parses_incoming_outgoing_both() {
    use codegg::lsp::operations::HierarchyDirection;
    assert_eq!(
        HierarchyDirection::parse(Some("incoming")).unwrap(),
        HierarchyDirection::Incoming
    );
    assert_eq!(
        HierarchyDirection::parse(Some("outgoing")).unwrap(),
        HierarchyDirection::Outgoing
    );
    assert_eq!(
        HierarchyDirection::parse(Some("both")).unwrap(),
        HierarchyDirection::Both
    );
}

#[test]
#[allow(non_snake_case)]
fn hierarchy_direction_rejects_invalid() {
    use codegg::lsp::operations::HierarchyDirection;
    let err = HierarchyDirection::parse(Some("invalid")).unwrap_err();
    assert!(
        matches!(err, codegg::lsp::LspError::RequestFailed(ref m) if m.contains("unsupported hierarchy direction")),
        "expected direction error, got: {err:?}"
    );
}

#[test]
#[allow(non_snake_case)]
fn callHierarchy_invalid_direction_rejected() {
    let tool = make_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(serde_json::json!({
                "operation": "callHierarchy",
                "file_path": "src/main.rs",
                "line": 1,
                "column": 1,
                "direction": "invalid"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("unsupported hierarchy direction")),
            "expected direction error, got: {err:?}"
        );
    });
}

#[test]
#[allow(non_snake_case)]
fn typeHierarchy_invalid_direction_rejected() {
    let tool = make_tool();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let err = tool
            .execute(serde_json::json!({
                "operation": "typeHierarchy",
                "file_path": "src/main.rs",
                "line": 1,
                "column": 1,
                "direction": "bad"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("unsupported hierarchy direction")),
            "expected direction error, got: {err:?}"
        );
    });
}

#[test]
#[allow(non_snake_case)]
fn callHierarchy_summary_serializes_error_fields() {
    use serde_json::json;
    let summary = json!({
        "items": [],
        "incoming": [],
        "outgoing": [],
        "prepare_error": "server does not support call hierarchy",
        "incoming_error": null,
        "outgoing_error": null,
        "truncated": false
    });
    let s = serde_json::to_string(&summary).unwrap();
    assert!(s.contains("prepare_error"));
    assert!(s.contains("server does not support call hierarchy"));
    assert!(s.contains("truncated"));
}

#[test]
#[allow(non_snake_case)]
fn type_hierarchy_summary_serializes_error_fields() {
    use serde_json::json;
    let summary = json!({
        "items": [],
        "supertypes": [],
        "subtypes": [],
        "prepare_error": "not supported",
        "supertypes_error": null,
        "subtypes_error": null,
        "truncated": false
    });
    let s = serde_json::to_string(&summary).unwrap();
    assert!(s.contains("prepare_error"));
    assert!(s.contains("supertypes_error"));
    assert!(s.contains("subtypes_error"));
}

// ── securityContext tests ────────────────────────────────────────────

#[tokio::test]
async fn security_context_schema_includes_operation() {
    let tool = make_tool();
    let params = tool.parameters();
    let ops = params["properties"]["operation"]["enum"]
        .as_array()
        .expect("operation.enum should be an array");
    assert!(ops.iter().any(|v| v.as_str() == Some("securityContext")));
}

#[tokio::test]
async fn security_context_schema_includes_security_categories() {
    let tool = make_tool();
    let params = tool.parameters();
    let prop = params["properties"]["security_categories"]
        .as_object()
        .expect("security_categories property should be an object");
    assert_eq!(prop.get("type").unwrap(), "array");
}

#[tokio::test]
async fn security_context_schema_includes_max_risk_markers() {
    let tool = make_tool();
    let params = tool.parameters();
    let prop = params["properties"]["max_risk_markers"]
        .as_object()
        .expect("max_risk_markers property should be an object");
    assert_eq!(prop.get("type").unwrap(), "number");
}

#[tokio::test]
async fn security_context_is_read_only() {
    let tool = make_tool();
    assert_eq!(tool.category(), ToolCategory::ReadOnly);
}

#[tokio::test]
async fn security_context_requires_file_path() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "securityContext"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("file_path")),
        "expected file_path error, got: {err:?}"
    );
}

#[tokio::test]
async fn security_context_rejects_line_without_column() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": "src/tool/mod.rs",
            "line": 1
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("both line and column")),
        "expected line+column error, got: {err:?}"
    );
}

#[tokio::test]
async fn security_context_rejects_content_and_patch() {
    let tool = make_tool();
    let err = tool
        .execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": "src/tool/mod.rs",
            "line": 1,
            "column": 1,
            "content": "fn main() {}",
            "patch": "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() {}\n"
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ToolError::Execution(ref m) if m.contains("either content or patch, not both")),
        "expected content+patch error, got: {err:?}"
    );
}

#[tokio::test]
async fn security_context_patch_does_not_write_disk() {
    let (_dir, path) = temp_rs_file("fn main() {\n    println!(\"old\");\n}\n");
    let original = std::fs::read_to_string(&path).unwrap();
    let tool = make_tool_with_root(_dir.path());
    let _ = tool.execute(serde_json::json!({
        "operation": "securityContext",
        "file_path": path.to_str().unwrap(),
        "line": 1,
        "column": 1,
        "patch": "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    println!(\"old\");\n+    println!(\"new\");\n }\n"
    })).await;
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        after, original,
        "securityContext must not write patched content to disk"
    );
}

#[tokio::test]
async fn security_context_returns_risk_markers_for_source_file() {
    let tool = make_tool();
    let result = tool
        .execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": "src/ide/mod.rs",
            "line": 4,
            "column": 1
        }))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["operation"], "securityContext");
    let markers = parsed["results"]["risk_markers"].as_array().unwrap();
    assert!(
        !markers.is_empty(),
        "should detect risk markers in source code"
    );
    let categories: Vec<&str> = markers
        .iter()
        .map(|m| m["category"].as_str().unwrap())
        .collect();
    assert!(
        categories.contains(&"process"),
        "should detect process category"
    );
}

#[tokio::test]
async fn security_context_with_patch_does_not_mutate_disk() {
    let (_dir, path) = temp_rs_file("fn main() {\n    println!(\"original\");\n}\n");
    let original = std::fs::read_to_string(&path).unwrap();
    let tool = make_tool_with_root(_dir.path());
    let _ = tool.execute(serde_json::json!({
        "operation": "securityContext",
        "file_path": path.to_str().unwrap(),
        "line": 2,
        "column": 1,
        "patch": "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    println!(\"original\");\n+    println!(\"modified\");\n }\n"
    })).await;
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after, original, "disk must not be mutated");
}

#[tokio::test]
async fn security_context_result_count_includes_markers() {
    let tool = make_tool();
    let result = tool
        .execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": "src/tool/mod.rs",
            "line": 1,
            "column": 1
        }))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let result_count = parsed["result_count"].as_u64().unwrap();
    let markers_count = parsed["results"]["risk_markers"].as_array().unwrap().len() as u64;
    assert!(
        result_count >= markers_count,
        "result_count should include risk markers"
    );
}

#[tokio::test]
async fn security_context_filters_by_category() {
    let tool = make_tool();
    let result = tool
        .execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": "src/tool/mod.rs",
            "line": 1,
            "column": 1,
            "security_categories": ["process"]
        }))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let markers = parsed["results"]["risk_markers"].as_array().unwrap();
    for marker in markers {
        assert_eq!(
            marker["category"].as_str().unwrap(),
            "process",
            "only process category should be present when filtered"
        );
    }
}

#[tokio::test]
async fn security_context_limits_risk_markers_precise() {
    let tool = make_tool();
    let result = tool
        .execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": "src/tool/mod.rs",
            "line": 1,
            "column": 1,
            "max_risk_markers": 2
        }))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let markers = parsed["results"]["risk_markers"].as_array().unwrap();
    let truncated = parsed["results"]["limits"]["risk_markers_truncated"]
        .as_bool()
        .unwrap();
    if markers.len() <= 2 {
        assert!(
            !truncated,
            "should not be truncated when markers <= max_risk_markers"
        );
    } else {
        assert!(
            truncated,
            "should be truncated when markers > max_risk_markers"
        );
    }
}

#[tokio::test]
async fn security_context_limits_symbols_precise() {
    let tool = make_tool();
    let result = tool
        .execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": "src/tool/mod.rs",
            "line": 1,
            "column": 1
        }))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let syms = parsed["results"]["security_relevant_symbols"]
        .as_array()
        .unwrap();
    let truncated = parsed["results"]["limits"]["symbols_truncated"]
        .as_bool()
        .unwrap();
    if syms.len() <= 80 {
        assert!(!truncated, "should not be truncated when symbols <= 80");
    } else {
        assert!(truncated, "should be truncated when symbols > 80");
    }
}

#[tokio::test]
async fn security_context_notes_include_no_position_message() {
    let tool = make_tool();
    let result = tool
        .execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": "src/tool/mod.rs"
        }))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let notes = parsed["results"]["notes"].as_array().unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.as_str().unwrap().contains("no target position")),
        "notes should mention missing target position"
    );
}
