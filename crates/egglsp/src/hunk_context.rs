use serde::{Deserialize, Serialize};

use crate::capability::LspUnavailable;
use crate::diagnostics::FileDiagnostic;
use crate::semantic_context::{
    SemanticCallGraphSummary, SemanticDiagnosticEvidence, SemanticLocation,
    SemanticSectionTruncation, SemanticSourceExcerpt, SemanticSymbolSummary,
    SemanticTypeGraphSummary,
};

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_excerpt_radius() -> u32 {
    40
}

fn default_max_hunks() -> usize {
    20
}

fn default_max_symbols() -> usize {
    10
}

fn default_max_diagnostics() -> usize {
    10
}

fn default_max_references() -> usize {
    10
}

/// A 1-indexed line range within a file.
///
/// All line values are **1-indexed** to match existing semantic-context
/// conventions. `start_line` is inclusive, `end_line` is inclusive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HunkLineRange {
    pub start_line: u32,
    pub end_line: u32,
}

/// A parsed hunk descriptor from a unified diff.
///
/// IDs are deterministic: `file_path:hunk_index:new_start-new_end`.
/// All line values are 1-indexed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HunkDescriptor {
    pub id: String,
    pub file_path: String,
    pub old_range: Option<HunkLineRange>,
    pub new_range: Option<HunkLineRange>,
    pub header: Option<String>,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub context_lines: usize,
}

/// Request for hunk-aware source navigation.
///
/// All line values are 1-indexed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HunkSourceNavigationRequest {
    pub file_path: String,
    /// Pre-parsed hunk descriptors. Internal use only; the model-facing
    /// tool schema exposes only `patch` for unified diff input.
    #[serde(default)]
    pub hunks: Vec<HunkDescriptor>,
    /// Optional raw unified diff text to parse into hunks.
    #[serde(default)]
    pub patch: Option<String>,
    pub intent: String,
    #[serde(default = "default_true")]
    pub include_definitions: bool,
    #[serde(default = "default_true")]
    pub include_references: bool,
    #[serde(default = "default_false")]
    pub include_call_hierarchy: bool,
    #[serde(default = "default_false")]
    pub include_type_hierarchy: bool,
    #[serde(default = "default_excerpt_radius")]
    pub excerpt_radius: u32,
    #[serde(default = "default_max_hunks")]
    pub max_hunks: usize,
    #[serde(default = "default_max_symbols")]
    pub max_symbols_per_hunk: usize,
    #[serde(default = "default_max_diagnostics")]
    pub max_diagnostics_per_hunk: usize,
    #[serde(default = "default_max_references")]
    pub max_references_per_hunk: usize,
}

/// Per-hunk evidence with enclosing symbol, related symbols,
/// diagnostics, definitions, references, call/type hierarchy,
/// source excerpt, freshness, and notes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HunkEvidence {
    pub hunk: HunkDescriptor,
    pub focus_range: Option<HunkLineRange>,
    pub enclosing_symbol: Option<SemanticSymbolSummary>,
    #[serde(default)]
    pub related_symbols: Vec<SemanticSymbolSummary>,
    #[serde(default)]
    pub diagnostics: Vec<FileDiagnostic>,
    #[serde(default)]
    pub nearby_diagnostics: Vec<FileDiagnostic>,
    #[serde(default)]
    pub definitions: Vec<SemanticLocation>,
    #[serde(default)]
    pub references: Vec<SemanticLocation>,
    pub call_hierarchy: Option<SemanticCallGraphSummary>,
    pub type_hierarchy: Option<SemanticTypeGraphSummary>,
    pub source_excerpt: Option<SemanticSourceExcerpt>,
    pub diagnostic_evidence: Option<SemanticDiagnosticEvidence>,
    #[serde(default)]
    pub section_truncations: Vec<SemanticSectionTruncation>,
    #[serde(default)]
    pub unavailable: Vec<LspUnavailable>,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Truncation limits applied during hunk navigation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HunkSourceNavigationLimits {
    pub hunks_truncated: bool,
    pub symbols_truncated: bool,
    pub diagnostics_truncated: bool,
    pub references_truncated: bool,
    pub excerpt_truncated: bool,
}

/// The assembled hunk source navigation response.
///
/// Wraps per-hunk evidence derived from a single semantic collection
/// (centered on the first hunk), global truncation limits, and
/// informational notes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HunkSourceNavigationResponse {
    pub file_path: String,
    #[serde(default)]
    pub hunks: Vec<HunkEvidence>,
    pub limits: HunkSourceNavigationLimits,
    #[serde(default)]
    pub notes: Vec<String>,
    pub truncated: bool,
}

impl HunkSourceNavigationResponse {
    pub fn new(file_path: impl Into<String>) -> Self {
        Self {
            file_path: file_path.into(),
            hunks: Vec::new(),
            limits: HunkSourceNavigationLimits::default(),
            notes: Vec::new(),
            truncated: false,
        }
    }

    pub fn push_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    pub fn mark_truncated(&mut self) {
        self.truncated = true;
    }
}

// ---------------------------------------------------------------------------
// Bridge to canonical LspContextPacket (Pass 6)
// ---------------------------------------------------------------------------

/// Convert a [`HunkSourceNavigationResponse`] into a flat
/// list of [`crate::context::LspContextItem`]s tagged with
/// [`crate::context::AgentContextSource::Hunk`].
///
/// One item is produced per (hunk, kind) pair:
/// - one `Definition` per definition location
/// - one `Reference` per reference location
/// - one `Diagnostic` per diagnostic in the changed range
/// - one `Symbol` per enclosing or related symbol
///
/// All items carry the response's freshness (PossiblyStale when
/// `truncated` is set, Fresh otherwise) and the response's notes
/// are NOT propagated here — callers add them at the packet level
/// to avoid duplication.
pub fn hunk_response_to_context_items(
    response: &HunkSourceNavigationResponse,
) -> Vec<crate::context::LspContextItem> {
    use crate::context::{
        AgentContextSource, LspContextItem, LspContextItemKind, LspEvidenceFreshness,
        LspEvidenceProvenance, LspContextScore, LineRange,
    };
    use std::path::PathBuf;

    let freshness = if response.truncated {
        LspEvidenceFreshness::PossiblyStale
    } else {
        LspEvidenceFreshness::Fresh
    };
    let file = PathBuf::from(&response.file_path);

    let mut items = Vec::new();
    for evidence in &response.hunks {
        // Enclosing symbol → one WorkspaceSymbol item.
        if let Some(sym) = &evidence.enclosing_symbol {
            let line_range = if sym.end_line >= sym.start_line {
                Some(LineRange {
                    start: sym.start_line,
                    end: sym.end_line,
                })
            } else {
                None
            };
            items.push(LspContextItem {
                kind: LspContextItemKind::WorkspaceSymbol,
                file: file.clone(),
                range: line_range,
                line: Some(sym.start_line),
                column: Some(sym.start_column),
                message: format!("enclosing: {}", sym.name),
                symbol: Some(sym.name.clone()),
                source: Some(AgentContextSource::Hunk),
                provenance: LspEvidenceProvenance {
                    server_id: String::new(),
                    server_generation: None,
                    operation: "hunkSourceContext".to_string(),
                    freshness,
                    capability_decision: None,
                    document_version: None,
                    age_ms: None,
                    post_restart: false,
                },
                score: LspContextScore {
                    priority: 20,
                    is_hunk_local: true,
                    is_error: false,
                    is_same_file: true,
                    freshness_rank: 0,
                },
                payload: None,
            });
        }

        // Diagnostics in the hunk range → one Diagnostic item each.
        for d in &evidence.diagnostics {
            items.push(LspContextItem {
                kind: LspContextItemKind::Diagnostic,
                file: file.clone(),
                range: None,
                line: Some(d.line),
                column: None,
                message: d.message.clone(),
                symbol: None,
                source: Some(AgentContextSource::Hunk),
                provenance: LspEvidenceProvenance {
                    server_id: String::new(),
                    server_generation: None,
                    operation: "hunkSourceContext".to_string(),
                    freshness,
                    capability_decision: None,
                    document_version: None,
                    age_ms: None,
                    post_restart: false,
                },
                score: LspContextScore {
                    priority: 30,
                    is_hunk_local: true,
                    is_error: matches!(d.severity, lsp_types::DiagnosticSeverity::ERROR),
                    is_same_file: true,
                    freshness_rank: 0,
                },
                payload: None,
            });
        }

        // Definitions → one Definition item per location.
        for loc in &evidence.definitions {
            let loc_file = PathBuf::from(&loc.file);
            items.push(LspContextItem {
                kind: LspContextItemKind::Definition,
                file: loc_file.clone(),
                range: None,
                line: Some(loc.start_line),
                column: None,
                message: format!("definition at {}:{}", loc_file.display(), loc.start_line),
                symbol: None,
                source: Some(AgentContextSource::Hunk),
                provenance: LspEvidenceProvenance {
                    server_id: String::new(),
                    server_generation: None,
                    operation: "hunkSourceContext".to_string(),
                    freshness,
                    capability_decision: None,
                    document_version: None,
                    age_ms: None,
                    post_restart: false,
                },
                score: LspContextScore {
                    priority: 25,
                    is_hunk_local: false,
                    is_error: false,
                    is_same_file: loc_file == file,
                    freshness_rank: 0,
                },
                payload: None,
            });
        }

        // References → one Reference item per location.
        for loc in &evidence.references {
            let loc_file = PathBuf::from(&loc.file);
            items.push(LspContextItem {
                kind: LspContextItemKind::Reference,
                file: loc_file.clone(),
                range: None,
                line: Some(loc.start_line),
                column: None,
                message: format!("reference at {}:{}", loc_file.display(), loc.start_line),
                symbol: None,
                source: Some(AgentContextSource::Hunk),
                provenance: LspEvidenceProvenance {
                    server_id: String::new(),
                    server_generation: None,
                    operation: "hunkSourceContext".to_string(),
                    freshness,
                    capability_decision: None,
                    document_version: None,
                    age_ms: None,
                    post_restart: false,
                },
                score: LspContextScore {
                    priority: 15,
                    is_hunk_local: false,
                    is_error: false,
                    is_same_file: loc_file == file,
                    freshness_rank: 0,
                },
                payload: None,
            });
        }
    }

    items
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    use crate::context::{AgentContextSource, LspContextItemKind, LspEvidenceFreshness};
    use crate::diagnostics::FileDiagnostic;
    use crate::semantic_context::SemanticLocation;

    fn make_descriptor(file_path: &str) -> HunkDescriptor {
        HunkDescriptor {
            id: format!("{file_path}:0:1-3"),
            file_path: file_path.to_string(),
            old_range: None,
            new_range: None,
            header: Some("@@ -1,3 +1,3 @@".to_string()),
            added_lines: 1,
            removed_lines: 1,
            context_lines: 2,
        }
    }

    fn make_evidence(file: &str) -> HunkEvidence {
        HunkEvidence {
            hunk: make_descriptor(file),
            focus_range: None,
            enclosing_symbol: None,
            related_symbols: vec![],
            diagnostics: vec![],
            nearby_diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            section_truncations: vec![],
            unavailable: vec![],
            notes: vec![],
        }
    }

    #[test]
    fn empty_response_yields_no_items() {
        let response = HunkSourceNavigationResponse::new("src/lib.rs");
        let items = hunk_response_to_context_items(&response);
        assert!(items.is_empty());
    }

    #[test]
    fn enclosing_symbol_produces_workspace_symbol_item() {
        let mut response = HunkSourceNavigationResponse::new("src/lib.rs");
        let mut ev = make_evidence("src/lib.rs");
        ev.enclosing_symbol = Some(crate::semantic_context::SemanticSymbolSummary {
            name: "foo".to_string(),
            kind: "function".to_string(),
            file: "src/lib.rs".to_string(),
            start_line: 5,
            start_column: 0,
            end_line: 15,
            end_column: 1,
        });
        response.hunks.push(ev);
        let items = hunk_response_to_context_items(&response);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, LspContextItemKind::WorkspaceSymbol);
        assert_eq!(items[0].source, Some(AgentContextSource::Hunk));
        assert_eq!(items[0].symbol.as_deref(), Some("foo"));
        assert_eq!(items[0].line, Some(5));
    }

    #[test]
    fn diagnostic_in_hunk_carries_error_severity() {
        let mut response = HunkSourceNavigationResponse::new("src/lib.rs");
        let mut ev = make_evidence("src/lib.rs");
        ev.diagnostics.push(FileDiagnostic {
            file: "src/lib.rs".to_string(),
            line: 10,
            column: 0,
            severity: lsp_types::DiagnosticSeverity::ERROR,
            code: None,
            source: None,
            message: "unused variable".to_string(),
        });
        response.hunks.push(ev);
        let items = hunk_response_to_context_items(&response);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, LspContextItemKind::Diagnostic);
        assert!(items[0].score.is_error);
    }

    #[test]
    fn diagnostic_warning_not_flagged_as_error() {
        let mut response = HunkSourceNavigationResponse::new("src/lib.rs");
        let mut ev = make_evidence("src/lib.rs");
        ev.diagnostics.push(FileDiagnostic {
            file: "src/lib.rs".to_string(),
            line: 10,
            column: 0,
            severity: lsp_types::DiagnosticSeverity::WARNING,
            code: None,
            source: None,
            message: "unused variable".to_string(),
        });
        response.hunks.push(ev);
        let items = hunk_response_to_context_items(&response);
        assert!(!items[0].score.is_error);
    }

    #[test]
    fn definition_produces_definition_item() {
        let mut response = HunkSourceNavigationResponse::new("src/lib.rs");
        let mut ev = make_evidence("src/lib.rs");
        ev.definitions.push(SemanticLocation {
            file: "src/lib.rs".to_string(),
            start_line: 5,
            start_column: 0,
            end_line: 5,
            end_column: 10,
        });
        response.hunks.push(ev);
        let items = hunk_response_to_context_items(&response);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, LspContextItemKind::Definition);
        assert_eq!(items[0].line, Some(5));
        assert!(items[0].score.is_same_file);
    }

    #[test]
    fn cross_file_reference_marks_is_same_file_false() {
        let mut response = HunkSourceNavigationResponse::new("src/lib.rs");
        let mut ev = make_evidence("src/lib.rs");
        ev.references.push(SemanticLocation {
            file: "src/other.rs".to_string(),
            start_line: 42,
            start_column: 0,
            end_line: 42,
            end_column: 8,
        });
        response.hunks.push(ev);
        let items = hunk_response_to_context_items(&response);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, LspContextItemKind::Reference);
        assert!(!items[0].score.is_same_file);
    }

    #[test]
    fn all_items_are_tagged_with_hunk_source() {
        let mut response = HunkSourceNavigationResponse::new("src/lib.rs");
        let mut ev = make_evidence("src/lib.rs");
        ev.diagnostics.push(FileDiagnostic {
            file: "src/lib.rs".to_string(),
            line: 5,
            column: 0,
            severity: lsp_types::DiagnosticSeverity::ERROR,
            code: None,
            source: None,
            message: "err".to_string(),
        });
        ev.definitions.push(SemanticLocation {
            file: "src/lib.rs".to_string(),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 4,
        });
        ev.references.push(SemanticLocation {
            file: "src/lib.rs".to_string(),
            start_line: 2,
            start_column: 0,
            end_line: 2,
            end_column: 4,
        });
        response.hunks.push(ev);
        let items = hunk_response_to_context_items(&response);
        assert_eq!(items.len(), 3);
        for item in &items {
            assert_eq!(item.source, Some(AgentContextSource::Hunk));
        }
    }

    #[test]
    fn truncated_response_marks_freshness_possibly_stale() {
        let mut response = HunkSourceNavigationResponse::new("src/lib.rs");
        response.truncated = true;
        let mut ev = make_evidence("src/lib.rs");
        ev.diagnostics.push(FileDiagnostic {
            file: "src/lib.rs".to_string(),
            line: 1,
            column: 0,
            severity: lsp_types::DiagnosticSeverity::ERROR,
            code: None,
            source: None,
            message: "x".to_string(),
        });
        response.hunks.push(ev);
        let items = hunk_response_to_context_items(&response);
        assert_eq!(
            items[0].provenance.freshness,
            LspEvidenceFreshness::PossiblyStale
        );
    }

    #[test]
    fn non_truncated_response_marks_freshness_fresh() {
        let mut response = HunkSourceNavigationResponse::new("src/lib.rs");
        let mut ev = make_evidence("src/lib.rs");
        ev.diagnostics.push(FileDiagnostic {
            file: "src/lib.rs".to_string(),
            line: 1,
            column: 0,
            severity: lsp_types::DiagnosticSeverity::ERROR,
            code: None,
            source: None,
            message: "x".to_string(),
        });
        response.hunks.push(ev);
        let items = hunk_response_to_context_items(&response);
        assert_eq!(items[0].provenance.freshness, LspEvidenceFreshness::Fresh);
    }
}

#[cfg(test)]
mod original_tests {
    use super::*;

    #[test]
    fn hunk_line_range_serializes() {
        let range = HunkLineRange {
            start_line: 10,
            end_line: 20,
        };
        let json = serde_json::to_value(&range).unwrap();
        assert_eq!(json["start_line"], 10);
        assert_eq!(json["end_line"], 20);

        let deserialized: HunkLineRange = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, range);
    }

    #[test]
    fn hunk_descriptor_roundtrip() {
        let desc = HunkDescriptor {
            id: "src/main.rs:0:10-20".to_string(),
            file_path: "src/main.rs".to_string(),
            old_range: Some(HunkLineRange {
                start_line: 5,
                end_line: 15,
            }),
            new_range: Some(HunkLineRange {
                start_line: 10,
                end_line: 20,
            }),
            header: Some("@@ -5,11 +10,11 @@".to_string()),
            added_lines: 3,
            removed_lines: 2,
            context_lines: 6,
        };
        let json = serde_json::to_string(&desc).unwrap();
        let deserialized: HunkDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, desc.id);
        assert_eq!(deserialized.file_path, desc.file_path);
        assert_eq!(deserialized.old_range, desc.old_range);
        assert_eq!(deserialized.new_range, desc.new_range);
        assert_eq!(deserialized.added_lines, 3);
        assert_eq!(deserialized.removed_lines, 2);
    }

    #[test]
    fn request_defaults() {
        let req: HunkSourceNavigationRequest =
            serde_json::from_str(r#"{"file_path":"a.rs","intent":"navigation","hunks":[]}"#)
                .unwrap();
        assert!(req.include_definitions);
        assert!(req.include_references);
        assert!(!req.include_call_hierarchy);
        assert!(!req.include_type_hierarchy);
        assert_eq!(req.excerpt_radius, 40);
        assert_eq!(req.max_hunks, 20);
        assert_eq!(req.max_symbols_per_hunk, 10);
        assert_eq!(req.max_diagnostics_per_hunk, 10);
        assert_eq!(req.max_references_per_hunk, 10);
        assert!(req.patch.is_none());
    }

    #[test]
    fn response_push_note() {
        let mut resp = HunkSourceNavigationResponse::new("a.rs");
        resp.push_note("test note");
        assert_eq!(resp.notes.len(), 1);
        assert_eq!(resp.notes[0], "test note");
    }

    #[test]
    fn response_mark_truncated() {
        let mut resp = HunkSourceNavigationResponse::new("a.rs");
        assert!(!resp.truncated);
        resp.mark_truncated();
        assert!(resp.truncated);
    }

    #[test]
    fn hunk_evidence_defaults() {
        let ev = HunkEvidence {
            hunk: HunkDescriptor {
                id: "f.rs:0:1-5".into(),
                file_path: "f.rs".into(),
                old_range: None,
                new_range: Some(HunkLineRange {
                    start_line: 1,
                    end_line: 5,
                }),
                header: None,
                added_lines: 0,
                removed_lines: 0,
                context_lines: 0,
            },
            focus_range: None,
            enclosing_symbol: None,
            related_symbols: Vec::new(),
            diagnostics: Vec::new(),
            nearby_diagnostics: Vec::new(),
            definitions: Vec::new(),
            references: Vec::new(),
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            section_truncations: Vec::new(),
            unavailable: Vec::new(),
            notes: Vec::new(),
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert!(json["related_symbols"].as_array().unwrap().is_empty());
        assert!(json["diagnostics"].as_array().unwrap().is_empty());
    }

    #[test]
    fn limits_default_all_false() {
        let limits = HunkSourceNavigationLimits::default();
        assert!(!limits.hunks_truncated);
        assert!(!limits.symbols_truncated);
        assert!(!limits.diagnostics_truncated);
        assert!(!limits.references_truncated);
        assert!(!limits.excerpt_truncated);
    }
}
