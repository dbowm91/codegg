use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_types::NumberOrString;
use serde::{Deserialize, Serialize};

use crate::diagnostics::FileDiagnostic;
use crate::error::LspError;
use crate::service::LspService;

pub struct OverlayRestoreToken {
    pub(crate) original_text: String,
    pub(crate) file_path: PathBuf,
    pub(crate) key: String,
    pub(crate) uri: String,
}

pub struct OverlaySession {
    service: Arc<LspService>,
}

impl OverlaySession {
    pub fn new(service: Arc<LspService>) -> Self {
        Self { service }
    }

    pub async fn apply_overlay(
        &self,
        file_path: &Path,
        proposed_text: &str,
    ) -> Result<OverlayRestoreToken, LspError> {
        let original_text = tokio::fs::read_to_string(file_path).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })?;

        let (key, uri) = self.service.ensure_file_open_from_disk(file_path).await?;
        self.service.update_file(file_path, proposed_text).await?;

        Ok(OverlayRestoreToken {
            original_text,
            file_path: file_path.to_path_buf(),
            key,
            uri,
        })
    }

    pub async fn restore(&self, token: &OverlayRestoreToken) -> Result<(), LspError> {
        self.service
            .update_file(&token.file_path, &token.original_text)
            .await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCheckPreview {
    pub file: String,
    pub diagnostics_may_still_be_warming: bool,
    pub diagnostics: Vec<FileDiagnostic>,
    pub diagnostics_error: Option<String>,
    pub symbols: Vec<SemanticSymbolSummary>,
    pub symbols_error: Option<String>,
    pub restored_disk_view: bool,
    pub restore_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSymbolSummary {
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

fn symbol_kind_to_string(kind: lsp_types::SymbolKind) -> String {
    match kind {
        lsp_types::SymbolKind::FILE => "file",
        lsp_types::SymbolKind::MODULE => "module",
        lsp_types::SymbolKind::NAMESPACE => "namespace",
        lsp_types::SymbolKind::PACKAGE => "package",
        lsp_types::SymbolKind::CLASS => "class",
        lsp_types::SymbolKind::METHOD => "method",
        lsp_types::SymbolKind::PROPERTY => "property",
        lsp_types::SymbolKind::FIELD => "field",
        lsp_types::SymbolKind::CONSTRUCTOR => "constructor",
        lsp_types::SymbolKind::ENUM => "enum",
        lsp_types::SymbolKind::INTERFACE => "interface",
        lsp_types::SymbolKind::FUNCTION => "function",
        lsp_types::SymbolKind::VARIABLE => "variable",
        lsp_types::SymbolKind::CONSTANT => "constant",
        lsp_types::SymbolKind::STRING => "string",
        lsp_types::SymbolKind::NUMBER => "number",
        lsp_types::SymbolKind::BOOLEAN => "boolean",
        lsp_types::SymbolKind::ARRAY => "array",
        lsp_types::SymbolKind::OBJECT => "object",
        lsp_types::SymbolKind::KEY => "key",
        lsp_types::SymbolKind::NULL => "null",
        lsp_types::SymbolKind::ENUM_MEMBER => "enum_member",
        lsp_types::SymbolKind::STRUCT => "struct",
        lsp_types::SymbolKind::EVENT => "event",
        lsp_types::SymbolKind::OPERATOR => "operator",
        lsp_types::SymbolKind::TYPE_PARAMETER => "type_parameter",
        _ => "unknown",
    }
    .to_string()
}

pub(crate) fn flatten_symbols(
    symbols: &[lsp_types::DocumentSymbol],
    output: &mut Vec<SemanticSymbolSummary>,
    remaining: &mut usize,
) {
    for sym in symbols {
        if *remaining == 0 {
            return;
        }
        let range = sym.range;
        output.push(SemanticSymbolSummary {
            name: sym.name.clone(),
            kind: symbol_kind_to_string(sym.kind),
            start_line: range.start.line + 1,
            start_column: range.start.character + 1,
            end_line: range.end.line + 1,
            end_column: range.end.character + 1,
        });
        *remaining -= 1;
        if let Some(children) = &sym.children {
            flatten_symbols(children, output, remaining);
        }
    }
}

pub(crate) fn diagnostic_to_file_diagnostic(
    uri_str: &str,
    d: lsp_types::Diagnostic,
) -> FileDiagnostic {
    FileDiagnostic {
        file: uri_str.to_string(),
        line: d.range.start.line,
        column: d.range.start.character,
        message: d.message,
        severity: d.severity.unwrap_or(lsp_types::DiagnosticSeverity::ERROR),
        source: d.source,
        code: d.code.as_ref().map(|c| match c {
            NumberOrString::Number(n) => n.to_string(),
            NumberOrString::String(s) => s.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_restore_token_creation() {
        let token = OverlayRestoreToken {
            original_text: "fn main() {}".to_string(),
            file_path: PathBuf::from("/tmp/test.rs"),
            key: "test-key".to_string(),
            uri: "file:///tmp/test.rs".to_string(),
        };
        assert_eq!(token.original_text, "fn main() {}");
        assert_eq!(token.file_path, PathBuf::from("/tmp/test.rs"));
        assert_eq!(token.key, "test-key");
        assert_eq!(token.uri, "file:///tmp/test.rs");
    }

    #[test]
    fn test_semantic_check_preview_serializes() {
        let preview = SemanticCheckPreview {
            file: "/tmp/test.rs".to_string(),
            diagnostics_may_still_be_warming: false,
            diagnostics: vec![FileDiagnostic {
                file: "/tmp/test.rs".to_string(),
                line: 1,
                column: 0,
                message: "error message".to_string(),
                severity: lsp_types::DiagnosticSeverity::ERROR,
                source: Some("rustc".to_string()),
                code: Some("E0001".to_string()),
            }],
            diagnostics_error: None,
            symbols: vec![SemanticSymbolSummary {
                name: "main".to_string(),
                kind: "function".to_string(),
                start_line: 1,
                start_column: 1,
                end_line: 3,
                end_column: 1,
            }],
            symbols_error: None,
            restored_disk_view: true,
            restore_error: None,
        };

        let json = serde_json::to_string(&preview).unwrap();
        let deserialized: SemanticCheckPreview = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.file, "/tmp/test.rs");
        assert_eq!(deserialized.diagnostics.len(), 1);
        assert_eq!(deserialized.symbols.len(), 1);
        assert!(deserialized.restored_disk_view);
        assert!(deserialized.diagnostics_error.is_none());
        assert!(deserialized.symbols_error.is_none());
        assert!(deserialized.restore_error.is_none());
    }

    #[test]
    fn test_semantic_check_preview_serializes_error_fields() {
        let preview = SemanticCheckPreview {
            file: "/tmp/test.rs".to_string(),
            diagnostics_may_still_be_warming: true,
            diagnostics: vec![],
            diagnostics_error: Some("diagnostics request failed".to_string()),
            symbols: vec![],
            symbols_error: Some("symbol request failed".to_string()),
            restored_disk_view: false,
            restore_error: Some("restore failed: connection lost".to_string()),
        };

        let json = serde_json::to_string(&preview).unwrap();
        let deserialized: SemanticCheckPreview = serde_json::from_str(&json).unwrap();
        assert!(deserialized.diagnostics.is_empty());
        assert!(deserialized.symbols.is_empty());
        assert!(!deserialized.restored_disk_view);
        assert_eq!(
            deserialized.diagnostics_error.as_deref(),
            Some("diagnostics request failed")
        );
        assert_eq!(
            deserialized.symbols_error.as_deref(),
            Some("symbol request failed")
        );
        assert_eq!(
            deserialized.restore_error.as_deref(),
            Some("restore failed: connection lost")
        );
    }

    #[test]
    fn test_symbol_summary_bounds() {
        let summary = SemanticSymbolSummary {
            name: "func".to_string(),
            kind: "function".to_string(),
            start_line: 10,
            start_column: 5,
            end_line: 12,
            end_column: 1,
        };
        assert_eq!(summary.start_line, 10);
        assert_eq!(summary.end_column, 1);
        assert_eq!(summary.kind, "function");
    }

    #[test]
    fn test_semantic_check_preview_error_fields_serialize() {
        let preview = SemanticCheckPreview {
            file: "/tmp/test.rs".to_string(),
            diagnostics_may_still_be_warming: true,
            diagnostics: vec![],
            diagnostics_error: Some("connection lost".to_string()),
            symbols: vec![],
            symbols_error: None,
            restored_disk_view: false,
            restore_error: Some("restore failed".to_string()),
        };
        let json = serde_json::to_string(&preview).unwrap();
        assert!(json.contains("diagnostics_error"));
        assert!(json.contains("symbols_error"));
        assert!(json.contains("restore_error"));
        let deserialized: SemanticCheckPreview = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.diagnostics_error.as_deref(),
            Some("connection lost")
        );
        assert!(deserialized.symbols_error.is_none());
        assert_eq!(
            deserialized.restore_error.as_deref(),
            Some("restore failed")
        );
    }

    #[test]
    fn test_overlay_restore_token_carries_key_uri() {
        let token = OverlayRestoreToken {
            original_text: "let x = 1;".to_string(),
            file_path: PathBuf::from("/workspace/src/main.rs"),
            key: "rust-_/workspace/src/main.rs".to_string(),
            uri: "file:///workspace/src/main.rs".to_string(),
        };
        assert_eq!(token.key, "rust-_/workspace/src/main.rs");
        assert_eq!(token.uri, "file:///workspace/src/main.rs");
        assert_eq!(token.file_path, PathBuf::from("/workspace/src/main.rs"));
    }

    #[test]
    fn test_flatten_symbols_empty() {
        let mut output = Vec::new();
        let mut remaining = 100;
        flatten_symbols(&[], &mut output, &mut remaining);
        assert!(output.is_empty());
        assert_eq!(remaining, 100);
    }

    #[test]
    #[allow(deprecated)]
    fn test_flatten_symbols_respects_remaining() {
        use lsp_types::{DocumentSymbol, Position, Range, SymbolKind};
        let symbols = vec![
            DocumentSymbol {
                name: "a".to_string(),
                kind: SymbolKind::FUNCTION,
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 0,
                    },
                },
                selection_range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                },
                detail: None,
                children: None,
                deprecated: None,
                tags: None,
            },
            DocumentSymbol {
                name: "b".to_string(),
                kind: SymbolKind::FUNCTION,
                range: Range {
                    start: Position {
                        line: 2,
                        character: 0,
                    },
                    end: Position {
                        line: 3,
                        character: 0,
                    },
                },
                selection_range: Range {
                    start: Position {
                        line: 2,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 5,
                    },
                },
                detail: None,
                children: None,
                deprecated: None,
                tags: None,
            },
        ];
        let mut output = Vec::new();
        let mut remaining = 1;
        flatten_symbols(&symbols, &mut output, &mut remaining);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].name, "a");
        assert_eq!(remaining, 0);
    }
}
