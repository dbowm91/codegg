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
/// Wraps per-hunk evidence alongside a base semantic response,
/// global truncation limits, and informational notes.
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

#[cfg(test)]
mod tests {
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
