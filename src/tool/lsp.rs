use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct LspInput {
    operation: String,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    column: Option<u32>,
    #[serde(default)]
    end_line: Option<u32>,
    #[serde(default)]
    end_column: Option<u32>,
    #[serde(default)]
    symbol: Option<String>,
}

pub struct LspTool {
    service: Arc<crate::lsp::service::LspService>,
    allowed_root: PathBuf,
}

impl LspTool {
    pub fn new(service: Arc<crate::lsp::service::LspService>) -> Self {
        Self {
            service,
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self
    }

    fn resolve_file(&self, path: &Option<String>) -> Result<PathBuf, ToolError> {
        let p = path
            .as_ref()
            .ok_or_else(|| ToolError::Execution("file_path required".to_string()))?;
        let original = if p.starts_with('/') {
            PathBuf::from(p)
        } else {
            std::env::current_dir().unwrap_or_default().join(p)
        };
        crate::tool::util::validate_path(&original, &self.allowed_root)
            .map_err(|e| ToolError::Execution(e.to_string()))
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Experimental: Query LSP server for code intelligence. Operations: goToDefinition, findReferences, hover, documentSymbol, workspaceSymbol, goToImplementation, prepareCallHierarchy, incomingCalls, outgoingCalls, codeAction, codeLens."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": [
                        "goToDefinition", "findReferences", "hover",
                        "documentSymbol", "workspaceSymbol", "goToImplementation",
                        "prepareCallHierarchy", "incomingCalls", "outgoingCalls",
                        "codeAction", "codeLens"
                    ],
                    "description": "LSP operation to perform"
                },
                "file_path": {
                    "type": "string",
                    "description": "File path for the operation"
                },
                "line": {
                    "type": "number",
                    "description": "Line number (1-indexed)"
                },
                "column": {
                    "type": "number",
                    "description": "Column number"
                },
                "end_line": {
                    "type": "number",
                    "description": "End line number for codeAction range (1-indexed)"
                },
                "end_column": {
                    "type": "number",
                    "description": "End column number for codeAction range"
                },
                "symbol": {
                    "type": "string",
                    "description": "Symbol name for symbol-based operations"
                }
            },
            "required": ["operation"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: LspInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid lsp input: {e}")))?;

        let ops = crate::lsp::operations::LspOperations::new(self.service.clone());

        let result = match parsed.operation.as_str() {
            "goToDefinition" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let line = parsed.line.unwrap_or(0);
                let col = parsed.column.unwrap_or(0);
                let locs = ops
                    .go_to_definition(&file, line, col)
                    .await
                    .map_err(|e| ToolError::Execution(format!("goToDefinition: {e}")))?;
                serde_json::to_string_pretty(&locs)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "findReferences" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let line = parsed.line.unwrap_or(0);
                let col = parsed.column.unwrap_or(0);
                let refs = ops
                    .find_references(&file, line, col)
                    .await
                    .map_err(|e| ToolError::Execution(format!("findReferences: {e}")))?;
                serde_json::to_string_pretty(&refs)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "hover" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let line = parsed.line.unwrap_or(0);
                let col = parsed.column.unwrap_or(0);
                let hover = ops
                    .hover(&file, line, col)
                    .await
                    .map_err(|e| ToolError::Execution(format!("hover: {e}")))?;
                serde_json::to_string_pretty(&hover)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "documentSymbol" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let syms = ops
                    .document_symbols(&file)
                    .await
                    .map_err(|e| ToolError::Execution(format!("documentSymbol: {e}")))?;
                serde_json::to_string_pretty(&syms)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "workspaceSymbol" => {
                let sym = parsed.symbol.as_ref().ok_or_else(|| {
                    ToolError::Execution("symbol required for workspaceSymbol".to_string())
                })?;
                let params = serde_json::json!({
                    "query": sym,
                    "workDoneToken": null,
                    "partialResultToken": null,
                });
                let keys = self.service.client_keys().await;
                let key = keys
                    .first()
                    .ok_or_else(|| ToolError::Execution("no LSP client available".to_string()))?;
                let resp = self
                    .service
                    .send_request(key, "workspace/symbol", params)
                    .await
                    .map_err(|e| ToolError::Execution(format!("workspaceSymbol: {e}")))?;
                serde_json::to_string_pretty(&resp)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "goToImplementation" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let uri_str = url::Url::from_file_path(&file)
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                let params = serde_json::json!({
                    "textDocument": { "uri": uri_str },
                    "position": {
                        "line": parsed.line.unwrap_or(0),
                        "character": parsed.column.unwrap_or(0),
                    },
                });
                let keys = self.service.client_keys().await;
                let key = keys
                    .first()
                    .ok_or_else(|| ToolError::Execution("no LSP client available".to_string()))?;
                let resp = self
                    .service
                    .send_request(key, "textDocument/implementation", params)
                    .await
                    .map_err(|e| ToolError::Execution(format!("goToImplementation: {e}")))?;
                serde_json::to_string_pretty(&resp)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "prepareCallHierarchy" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let uri_str = url::Url::from_file_path(&file)
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                let params = serde_json::json!({
                    "textDocument": { "uri": uri_str },
                    "position": {
                        "line": parsed.line.unwrap_or(0),
                        "character": parsed.column.unwrap_or(0),
                    },
                    "workDoneToken": null,
                    "partialResultToken": null,
                });
                let keys = self.service.client_keys().await;
                let key = keys
                    .first()
                    .ok_or_else(|| ToolError::Execution("no LSP client available".to_string()))?;
                let resp = self
                    .service
                    .send_request(key, "textDocument/prepareCallHierarchy", params)
                    .await
                    .map_err(|e| ToolError::Execution(format!("prepareCallHierarchy: {e}")))?;
                serde_json::to_string_pretty(&resp)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "incomingCalls" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let uri_str = url::Url::from_file_path(&file)
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                let params = serde_json::json!({
                    "item": {
                        "uri": uri_str,
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 0 },
                        },
                    },
                    "workDoneToken": null,
                    "partialResultToken": null,
                });
                let keys = self.service.client_keys().await;
                let key = keys
                    .first()
                    .ok_or_else(|| ToolError::Execution("no LSP client available".to_string()))?;
                let resp = self
                    .service
                    .send_request(key, "callHierarchy/incomingCalls", params)
                    .await
                    .map_err(|e| ToolError::Execution(format!("incomingCalls: {e}")))?;
                serde_json::to_string_pretty(&resp)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "outgoingCalls" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let uri_str = url::Url::from_file_path(&file)
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                let params = serde_json::json!({
                    "item": {
                        "uri": uri_str,
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 0 },
                        },
                    },
                    "workDoneToken": null,
                    "partialResultToken": null,
                });
                let keys = self.service.client_keys().await;
                let key = keys
                    .first()
                    .ok_or_else(|| ToolError::Execution("no LSP client available".to_string()))?;
                let resp = self
                    .service
                    .send_request(key, "callHierarchy/outgoingCalls", params)
                    .await
                    .map_err(|e| ToolError::Execution(format!("outgoingCalls: {e}")))?;
                serde_json::to_string_pretty(&resp)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "codeAction" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let start_line = parsed.line.unwrap_or(0);
                let start_col = parsed.column.unwrap_or(0);
                let end_line = parsed.end_line.unwrap_or(start_line);
                let end_col = parsed.end_column.unwrap_or(0);
                let actions = ops
                    .code_actions(
                        &file,
                        start_line,
                        start_col,
                        end_line,
                        end_col,
                        Vec::new(),
                        None,
                    )
                    .await
                    .map_err(|e| ToolError::Execution(format!("codeAction: {e}")))?;
                serde_json::to_string_pretty(&actions)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "codeLens" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let lenses = ops
                    .code_lens(&file)
                    .await
                    .map_err(|e| ToolError::Execution(format!("codeLens: {e}")))?;
                serde_json::to_string_pretty(&lenses)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            op => return Err(ToolError::Execution(format!("unknown LSP operation: {op}"))),
        };

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lsp_tool_name() {
        let tool = LspTool::new(std::sync::Arc::new(
            crate::lsp::service::LspService::new(crate::config::schema::LspConfig::default().into()),
        ));
        assert_eq!(tool.name(), "lsp");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn lsp_parameters_schema_snapshot() {
        let tool = LspTool::new(std::sync::Arc::new(
            crate::lsp::service::LspService::new(crate::config::schema::LspConfig::default().into()),
        ));
        let params = tool.parameters();
        let expected = json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": [
                        "goToDefinition", "findReferences", "hover",
                        "documentSymbol", "workspaceSymbol", "goToImplementation",
                        "prepareCallHierarchy", "incomingCalls", "outgoingCalls",
                        "codeAction", "codeLens"
                    ],
                    "description": "LSP operation to perform"
                },
                "file_path": {
                    "type": "string",
                    "description": "File path for the operation"
                },
                "line": {
                    "type": "number",
                    "description": "Line number (1-indexed)"
                },
                "column": {
                    "type": "number",
                    "description": "Column number"
                },
                "end_line": {
                    "type": "number",
                    "description": "End line number for codeAction range (1-indexed)"
                },
                "end_column": {
                    "type": "number",
                    "description": "End column number for codeAction range"
                },
                "symbol": {
                    "type": "string",
                    "description": "Symbol name for symbol-based operations"
                }
            },
            "required": ["operation"]
        });
        assert_eq!(params, expected);
    }
}
