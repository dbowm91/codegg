//! Shared semantic context API independent of security review.
//!
//! [`SemanticContextRequest`] describes what the caller wants to know
//! about a code location. [`SemanticContextResponse`] carries the
//! assembled answer. Both are plain serializable DTOs — no live LSP
//! connection is required to construct or inspect them.
//!
//! The security review workflow can adapt these types into
//! security-specific evidence, but the API is intentionally
//! domain-agnostic.

use serde::{Deserialize, Serialize};

use crate::capability::LspUnavailable;
use crate::diagnostics::FileDiagnostic;

/// Describes what the caller wants to know.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticContextRequest {
    pub file_path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub intent: SemanticContextIntent,
    pub max_symbols: usize,
    pub max_references: usize,
    pub max_diagnostics: usize,
    pub call_depth: u8,
}

impl SemanticContextRequest {
    pub fn new(file_path: impl Into<String>, intent: SemanticContextIntent) -> Self {
        Self {
            file_path: file_path.into(),
            line: None,
            column: None,
            intent,
            max_symbols: 120,
            max_references: 80,
            max_diagnostics: 100,
            call_depth: 0,
        }
    }

    pub fn with_position(mut self, line: u32, column: u32) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    pub fn with_call_depth(mut self, depth: u8) -> Self {
        self.call_depth = depth.min(2);
        self
    }
}

/// High-level intent behind a semantic context request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticContextIntent {
    Explain,
    EditPlanning,
    Review,
    SecurityReview,
    TestPlanning,
    Navigation,
}

impl SemanticContextIntent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Explain => "explain",
            Self::EditPlanning => "edit_planning",
            Self::Review => "review",
            Self::SecurityReview => "security_review",
            Self::TestPlanning => "test_planning",
            Self::Navigation => "navigation",
        }
    }
}

/// A compact symbol summary returned as part of the semantic context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSymbolSummary {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// A compact location (definition or reference).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticLocation {
    pub file: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// Call graph summary for a single symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCallGraphSummary {
    pub incoming_count: usize,
    pub outgoing_count: usize,
    pub truncated: bool,
    pub prepare_error: Option<String>,
    pub incoming_error: Option<String>,
    pub outgoing_error: Option<String>,
}

/// Type graph summary for a single symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticTypeGraphSummary {
    pub supertypes_count: usize,
    pub subtypes_count: usize,
    pub truncated: bool,
    pub prepare_error: Option<String>,
    pub supertypes_error: Option<String>,
    pub subtypes_error: Option<String>,
}

/// The assembled semantic context response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticContextResponse {
    pub file_path: String,
    pub symbol: Option<SemanticSymbolSummary>,
    pub diagnostics: Vec<FileDiagnostic>,
    pub definitions: Vec<SemanticLocation>,
    pub references: Vec<SemanticLocation>,
    pub call_hierarchy: Option<SemanticCallGraphSummary>,
    pub type_hierarchy: Option<SemanticTypeGraphSummary>,
    pub notes: Vec<String>,
    pub truncated: bool,
    pub unavailable: Vec<LspUnavailable>,
}

impl SemanticContextResponse {
    pub fn new(file_path: impl Into<String>) -> Self {
        Self {
            file_path: file_path.into(),
            symbol: None,
            diagnostics: Vec::new(),
            definitions: Vec::new(),
            references: Vec::new(),
            call_hierarchy: None,
            type_hierarchy: None,
            notes: Vec::new(),
            truncated: false,
            unavailable: Vec::new(),
        }
    }

    pub fn push_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    pub fn push_unavailable(&mut self, u: LspUnavailable) {
        self.unavailable.push(u);
    }
}

/// Caps enforced on a semantic context request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticContextCaps {
    pub max_symbols: usize,
    pub max_references: usize,
    pub max_diagnostics: usize,
    pub max_call_depth: u8,
}

impl Default for SemanticContextCaps {
    fn default() -> Self {
        Self {
            max_symbols: 120,
            max_references: 80,
            max_diagnostics: 100,
            max_call_depth: 2,
        }
    }
}

impl SemanticContextCaps {
    pub fn enforce(&self, req: &mut SemanticContextRequest) {
        req.max_symbols = req.max_symbols.min(self.max_symbols);
        req.max_references = req.max_references.min(self.max_references);
        req.max_diagnostics = req.max_diagnostics.min(self.max_diagnostics);
        req.call_depth = req.call_depth.min(self.max_call_depth);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::LspSemanticOperation;

    #[test]
    fn semantic_context_caps_are_enforced() {
        let caps = SemanticContextCaps {
            max_symbols: 50,
            max_references: 20,
            max_diagnostics: 10,
            max_call_depth: 1,
        };
        let mut req = SemanticContextRequest::new("src/main.rs", SemanticContextIntent::Explain)
            .with_position(10, 5)
            .with_call_depth(5);
        req.max_symbols = 500;
        req.max_references = 500;
        req.max_diagnostics = 500;

        caps.enforce(&mut req);

        assert_eq!(req.max_symbols, 50);
        assert_eq!(req.max_references, 20);
        assert_eq!(req.max_diagnostics, 10);
        assert_eq!(req.call_depth, 1);
    }

    #[test]
    fn semantic_context_unsupported_capabilities_do_not_fail_request() {
        let mut resp = SemanticContextResponse::new("src/main.rs");
        resp.push_unavailable(LspUnavailable::new(
            LspSemanticOperation::CallHierarchy,
            "server lacks capability",
        ));
        resp.push_note("fallback to basic diagnostics");

        assert_eq!(resp.unavailable.len(), 1);
        assert_eq!(resp.notes.len(), 1);
        assert!(!resp.truncated);
    }

    #[test]
    fn semantic_context_security_intent_uses_security_caps() {
        let req = SemanticContextRequest::new("src/main.rs", SemanticContextIntent::SecurityReview)
            .with_position(10, 5)
            .with_call_depth(2);
        assert_eq!(req.intent, SemanticContextIntent::SecurityReview);
        assert_eq!(req.call_depth, 2);
    }

    #[test]
    fn semantic_context_request_defaults() {
        let req = SemanticContextRequest::new("test.rs", SemanticContextIntent::Navigation);
        assert_eq!(req.file_path, "test.rs");
        assert!(req.line.is_none());
        assert!(req.column.is_none());
        assert_eq!(req.max_symbols, 120);
        assert_eq!(req.max_references, 80);
        assert_eq!(req.max_diagnostics, 100);
        assert_eq!(req.call_depth, 0);
    }

    #[test]
    fn semantic_context_response_pushes_notes_and_unavailable() {
        let mut resp = SemanticContextResponse::new("a.rs");
        resp.push_note("note 1");
        resp.push_note("note 2");
        resp.push_unavailable(LspUnavailable::new(
            LspSemanticOperation::Hover,
            "not supported",
        ));
        assert_eq!(resp.notes.len(), 2);
        assert_eq!(resp.unavailable.len(), 1);
        assert_eq!(resp.unavailable[0].operation, "hover");
    }
}
