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
//!
//! # Indexing conventions
//!
//! All line and column values in these DTOs are **1-indexed** for
//! consistency with the presentation layer. The tool adapter may
//! convert to/from LSP-native 0-indexed positions as needed.

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
    /// Include overlay diagnostics/symbols when proposed content is available.
    pub include_overlay: bool,
    /// Include safe source-action preview hints (e.g. organize imports).
    pub include_source_actions: bool,
    /// Include definition results when a position is provided.
    pub include_definitions: bool,
    /// Include reference results when a position is provided.
    pub include_references: bool,
    /// Radius (lines above/below target) for the source excerpt.
    pub excerpt_radius: u32,
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
            include_overlay: false,
            include_source_actions: false,
            include_definitions: true,
            include_references: true,
            excerpt_radius: 40,
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

    pub fn with_overlay(mut self, include: bool) -> Self {
        self.include_overlay = include;
        self
    }

    pub fn with_source_actions(mut self, include: bool) -> Self {
        self.include_source_actions = include;
        self
    }

    pub fn with_excerpt_radius(mut self, radius: u32) -> Self {
        self.excerpt_radius = radius;
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

/// A source text excerpt around the target location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSourceExcerpt {
    /// 1-indexed start line of the excerpt.
    pub start_line: u32,
    /// 1-indexed end line of the excerpt (inclusive).
    pub end_line: u32,
    /// The excerpt text.
    pub text: String,
    /// Whether the excerpt was truncated due to byte limits.
    pub truncated: bool,
}

/// Diagnostic freshness metadata carried alongside diagnostic results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticDiagnosticEvidence {
    /// Freshness classification of the diagnostics.
    pub freshness: String,
    /// Source of the diagnostics (pushed/pulled/unknown).
    pub source: String,
    /// Age in milliseconds since diagnostics were received.
    pub age_ms: i64,
    /// Whether the diagnostics are usable as evidence.
    pub usable_evidence: bool,
}

/// Overlay diagnostics/symbols from proposed content preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticOverlay {
    /// Whether an overlay was used.
    pub used: bool,
    /// Whether overlay diagnostics may still be warming.
    pub diagnostics_may_still_be_warming: bool,
    /// Diagnostics from the proposed content.
    pub diagnostics: Vec<FileDiagnostic>,
    /// Error fetching overlay diagnostics, if any.
    pub diagnostics_error: Option<String>,
    /// Symbols from the proposed content.
    pub symbols: Vec<SemanticOverlaySymbol>,
    /// Error fetching overlay symbols, if any.
    pub symbols_error: Option<String>,
    /// Whether the disk view was restored after overlay.
    pub restored_disk_view: bool,
    /// Error restoring disk view, if any.
    pub restore_error: Option<String>,
}

/// A compact symbol from an overlay preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticOverlaySymbol {
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// A source-action preview hint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSourceActionHint {
    /// The action identifier (e.g. "source.organizeImports").
    pub action: String,
    /// Whether the action is available for this file.
    pub available: bool,
    /// Human-readable error if the action could not be resolved.
    pub error: Option<String>,
}

/// Per-section truncation metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSectionTruncation {
    /// Which section was truncated (e.g. "diagnostics", "symbols", "references").
    pub section: String,
    /// Original count before truncation.
    pub original_count: Option<usize>,
    /// Number of items emitted after truncation.
    pub emitted_count: usize,
    /// The limit that was applied.
    pub limit: usize,
}

/// Truncation limits applied during collection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticContextLimits {
    pub diagnostics_truncated: bool,
    pub symbols_truncated: bool,
    pub references_truncated: bool,
    pub overlay_diagnostics_truncated: bool,
    pub excerpt_truncated: bool,
}

/// The assembled semantic context response.
///
/// This is the internal semantic read model. Tool adapters convert
/// this into presentation-specific JSON shapes (e.g. `SemanticContextPacket`
/// for `semanticContext`, or security-filtered subsets for `securityContext`).
///
/// All line/column values are 1-indexed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticContextResponse {
    pub file_path: String,
    /// First symbol (backward-compatible shorthand).
    pub symbol: Option<SemanticSymbolSummary>,
    /// All document symbols (flattened, capped).
    #[serde(default)]
    pub all_symbols: Vec<SemanticSymbolSummary>,
    pub diagnostics: Vec<FileDiagnostic>,
    #[serde(default)]
    pub definitions: Vec<SemanticLocation>,
    #[serde(default)]
    pub references: Vec<SemanticLocation>,
    pub call_hierarchy: Option<SemanticCallGraphSummary>,
    pub type_hierarchy: Option<SemanticTypeGraphSummary>,
    /// Source text excerpt around the target.
    #[serde(default)]
    pub source_excerpt: Option<SemanticSourceExcerpt>,
    /// Diagnostic freshness metadata.
    #[serde(default)]
    pub diagnostic_evidence: Option<SemanticDiagnosticEvidence>,
    /// Overlay diagnostics/symbols from proposed content.
    #[serde(default)]
    pub overlay: Option<SemanticOverlay>,
    /// Source-action preview hints.
    #[serde(default)]
    pub source_actions: Vec<SemanticSourceActionHint>,
    /// Per-section truncation metadata.
    #[serde(default)]
    pub section_truncations: Vec<SemanticSectionTruncation>,
    /// Truncation limits applied during collection.
    #[serde(default)]
    pub limits: SemanticContextLimits,
    pub notes: Vec<String>,
    pub truncated: bool,
    pub unavailable: Vec<LspUnavailable>,
}

impl SemanticContextResponse {
    pub fn new(file_path: impl Into<String>) -> Self {
        Self {
            file_path: file_path.into(),
            symbol: None,
            all_symbols: Vec::new(),
            diagnostics: Vec::new(),
            definitions: Vec::new(),
            references: Vec::new(),
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            overlay: None,
            source_actions: Vec::new(),
            section_truncations: Vec::new(),
            limits: SemanticContextLimits::default(),
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

    /// Record a section truncation event.
    pub fn push_truncation(
        &mut self,
        section: impl Into<String>,
        original_count: Option<usize>,
        emitted_count: usize,
        limit: usize,
    ) {
        self.section_truncations.push(SemanticSectionTruncation {
            section: section.into(),
            original_count,
            emitted_count,
            limit,
        });
    }

    /// Mark the response as globally truncated.
    pub fn mark_truncated(&mut self) {
        self.truncated = true;
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
        assert!(!req.include_overlay);
        assert!(!req.include_source_actions);
        assert!(req.include_definitions);
        assert!(req.include_references);
        assert_eq!(req.excerpt_radius, 40);
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

    #[test]
    fn semantic_context_response_records_truncation() {
        let mut resp = SemanticContextResponse::new("a.rs");
        resp.push_truncation("diagnostics", Some(150), 100, 100);
        resp.push_truncation("references", Some(200), 80, 80);
        assert_eq!(resp.section_truncations.len(), 2);
        assert_eq!(resp.section_truncations[0].section, "diagnostics");
        assert_eq!(resp.section_truncations[0].original_count, Some(150));
        assert_eq!(resp.section_truncations[0].emitted_count, 100);
        assert_eq!(resp.section_truncations[1].section, "references");
    }

    #[test]
    fn semantic_context_response_new_fields_default_empty() {
        let resp = SemanticContextResponse::new("a.rs");
        assert!(resp.all_symbols.is_empty());
        assert!(resp.source_excerpt.is_none());
        assert!(resp.diagnostic_evidence.is_none());
        assert!(resp.overlay.is_none());
        assert!(resp.source_actions.is_empty());
        assert!(resp.section_truncations.is_empty());
        assert_eq!(resp.limits, SemanticContextLimits::default());
    }

    #[test]
    fn semantic_context_request_builder_chaining() {
        let req = SemanticContextRequest::new("f.rs", SemanticContextIntent::Review)
            .with_position(5, 10)
            .with_call_depth(1)
            .with_overlay(true)
            .with_source_actions(true)
            .with_excerpt_radius(80);
        assert!(req.include_overlay);
        assert!(req.include_source_actions);
        assert!(req.include_definitions);
        assert!(req.include_references);
        assert_eq!(req.excerpt_radius, 80);
        assert_eq!(req.line, Some(5));
        assert_eq!(req.column, Some(10));
        assert_eq!(req.call_depth, 1);
    }
}
