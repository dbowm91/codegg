use crate::lsp::hunk_nav_ranges::{
    diagnostics_in_range, diagnostics_near_range, expand_range, find_enclosing_symbol,
    find_related_symbols, locations_in_range,
};
use egglsp::hunk_context::{
    HunkDescriptor, HunkEvidence, HunkLineRange, HunkSourceNavigationLimits,
    HunkSourceNavigationResponse,
};
use egglsp::semantic_context::SemanticContextResponse;

pub struct HunkSourceNavigator {
    limits: HunkSourceNavigationLimits,
    max_symbols_per_hunk: usize,
    max_diagnostics_per_hunk: usize,
    max_references_per_hunk: usize,
    excerpt_radius: u32,
}

impl HunkSourceNavigator {
    pub fn new() -> Self {
        Self {
            limits: HunkSourceNavigationLimits::default(),
            max_symbols_per_hunk: 10,
            max_diagnostics_per_hunk: 10,
            max_references_per_hunk: 10,
            excerpt_radius: 40,
        }
    }

    pub fn with_limits(mut self, limits: HunkSourceNavigationLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_max_symbols_per_hunk(mut self, max: usize) -> Self {
        self.max_symbols_per_hunk = max;
        self
    }

    pub fn with_max_diagnostics_per_hunk(mut self, max: usize) -> Self {
        self.max_diagnostics_per_hunk = max;
        self
    }

    pub fn with_max_references_per_hunk(mut self, max: usize) -> Self {
        self.max_references_per_hunk = max;
        self
    }

    pub fn with_excerpt_radius(mut self, radius: u32) -> Self {
        self.excerpt_radius = radius;
        self
    }

    pub fn build(
        &self,
        semantic: SemanticContextResponse,
        hunks: Vec<HunkDescriptor>,
    ) -> HunkSourceNavigationResponse {
        let mut response = HunkSourceNavigationResponse::new(&semantic.file_path);
        let mut limits = HunkSourceNavigationLimits::default();

        let total_lines = semantic
            .source_excerpt
            .as_ref()
            .map(|e| e.end_line.max(e.start_line))
            .unwrap_or(0);

        for hunk in hunks {
            if let Some(evidence) = self.build_evidence(&hunk, &semantic, total_lines, &mut limits)
            {
                response.hunks.push(evidence);
            }
        }

        response.limits = limits;
        response.truncated = response.limits.hunks_truncated
            || response.limits.symbols_truncated
            || response.limits.diagnostics_truncated
            || response.limits.references_truncated
            || response.limits.excerpt_truncated;

        response
    }

    fn build_evidence(
        &self,
        hunk: &HunkDescriptor,
        semantic: &SemanticContextResponse,
        file_line_count: u32,
        limits: &mut HunkSourceNavigationLimits,
    ) -> Option<HunkEvidence> {
        let new_range = hunk.new_range.as_ref()?;
        let focus_range = expand_range(new_range, self.excerpt_radius, file_line_count);

        let enclosing_symbol = find_enclosing_symbol(new_range, &semantic.all_symbols).cloned();

        let related =
            find_related_symbols(new_range, &semantic.all_symbols, self.max_symbols_per_hunk);
        if related.len() >= self.max_symbols_per_hunk {
            limits.symbols_truncated = true;
        }
        let related_symbols: Vec<_> = related.into_iter().cloned().collect();

        let intersecting_diags: Vec<_> = diagnostics_in_range(new_range, &semantic.diagnostics)
            .into_iter()
            .take(self.max_diagnostics_per_hunk)
            .cloned()
            .collect();
        if intersecting_diags.len() >= self.max_diagnostics_per_hunk {
            limits.diagnostics_truncated = true;
        }

        let nearby_diags: Vec<_> = diagnostics_near_range(new_range, &semantic.diagnostics, 5)
            .into_iter()
            .take(
                self.max_diagnostics_per_hunk
                    .saturating_sub(intersecting_diags.len()),
            )
            .cloned()
            .collect();

        let defs: Vec<_> = locations_in_range(new_range, &semantic.definitions)
            .into_iter()
            .cloned()
            .collect();

        let refs: Vec<_> = locations_in_range(new_range, &semantic.references)
            .into_iter()
            .take(self.max_references_per_hunk)
            .cloned()
            .collect();
        if refs.len() >= self.max_references_per_hunk {
            limits.references_truncated = true;
        }

        let call_hierarchy = semantic.call_hierarchy.clone();
        let type_hierarchy = semantic.type_hierarchy.clone();
        let source_excerpt = semantic.source_excerpt.clone();
        let diagnostic_evidence = semantic.diagnostic_evidence.clone();

        let section_truncations = semantic.section_truncations.clone();
        let unavailable = semantic.unavailable.clone();

        let mut notes = Vec::new();
        if enclosing_symbol.is_none() {
            notes.push("no enclosing symbol found".to_string());
        }

        Some(HunkEvidence {
            hunk: hunk.clone(),
            focus_range: Some(HunkLineRange {
                start_line: focus_range.start_line,
                end_line: focus_range.end_line,
            }),
            enclosing_symbol,
            related_symbols,
            diagnostics: intersecting_diags,
            nearby_diagnostics: nearby_diags,
            definitions: defs,
            references: refs,
            call_hierarchy,
            type_hierarchy,
            source_excerpt,
            diagnostic_evidence,
            section_truncations,
            unavailable,
            notes,
        })
    }
}

impl Default for HunkSourceNavigator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egglsp::diagnostics::FileDiagnostic;
    use egglsp::hunk_context::HunkDescriptor;
    use egglsp::lsp_types::DiagnosticSeverity;
    use egglsp::semantic_context::{
        SemanticContextResponse, SemanticLocation, SemanticSourceExcerpt, SemanticSymbolSummary,
    };

    fn range(s: u32, e: u32) -> HunkLineRange {
        HunkLineRange {
            start_line: s,
            end_line: e,
        }
    }

    fn hunk(id: &str, file: &str, new_start: u32, new_end: u32) -> HunkDescriptor {
        HunkDescriptor {
            id: id.to_string(),
            file_path: file.to_string(),
            old_range: None,
            new_range: Some(HunkLineRange {
                start_line: new_start,
                end_line: new_end,
            }),
            header: None,
            added_lines: 0,
            removed_lines: 0,
            context_lines: 0,
        }
    }

    fn sym(name: &str, start: u32, end: u32) -> SemanticSymbolSummary {
        SemanticSymbolSummary {
            name: name.to_string(),
            kind: "function".to_string(),
            file: "test.rs".to_string(),
            start_line: start,
            start_column: 1,
            end_line: end,
            end_column: 1,
        }
    }

    fn loc(file: &str, start: u32, end: u32) -> SemanticLocation {
        SemanticLocation {
            file: file.to_string(),
            start_line: start,
            start_column: 1,
            end_line: end,
            end_column: 1,
        }
    }

    fn diag(line: u32) -> FileDiagnostic {
        FileDiagnostic {
            file: "test.rs".to_string(),
            line,
            column: 1,
            message: "test".to_string(),
            severity: DiagnosticSeverity::ERROR,
            source: None,
            code: None,
        }
    }

    fn base_semantic(file_path: &str) -> SemanticContextResponse {
        SemanticContextResponse {
            file_path: file_path.to_string(),
            symbol: None,
            all_symbols: Vec::new(),
            diagnostics: Vec::new(),
            definitions: Vec::new(),
            references: Vec::new(),
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: Some(SemanticSourceExcerpt {
                start_line: 1,
                end_line: 50,
                text: "code".to_string(),
                truncated: false,
            }),
            diagnostic_evidence: None,
            overlay: None,
            source_actions: vec![],
            section_truncations: vec![],
            limits: Default::default(),
            notes: vec![],
            truncated: false,
            unavailable: vec![],
        }
    }

    #[test]
    fn empty_hunks_produces_empty_response() {
        let nav = HunkSourceNavigator::new();
        let semantic = base_semantic("test.rs");
        let resp = nav.build(semantic, vec![]);
        assert!(resp.hunks.is_empty());
        assert!(!resp.truncated);
    }

    #[test]
    fn enclosing_symbol_found() {
        let nav = HunkSourceNavigator::new();
        let mut semantic = base_semantic("test.rs");
        semantic.all_symbols = vec![sym("outer", 1, 100), sym("inner", 5, 20)];
        let hunks = vec![hunk("h0", "test.rs", 8, 12)];
        let resp = nav.build(semantic, hunks);
        assert_eq!(resp.hunks.len(), 1);
        let ev = &resp.hunks[0];
        assert_eq!(ev.enclosing_symbol.as_ref().unwrap().name, "inner");
    }

    #[test]
    fn related_symbols_exclude_enclosing() {
        let nav = HunkSourceNavigator::new();
        let mut semantic = base_semantic("test.rs");
        semantic.all_symbols = vec![sym("here", 10, 20), sym("nearby", 25, 30)];
        let hunks = vec![hunk("h0", "test.rs", 12, 15)];
        let resp = nav.build(semantic, hunks);
        let ev = &resp.hunks[0];
        assert_eq!(ev.related_symbols.len(), 1);
        assert_eq!(ev.related_symbols[0].name, "nearby");
    }

    #[test]
    fn diagnostics_intersecting_hunk() {
        let nav = HunkSourceNavigator::new();
        let mut semantic = base_semantic("test.rs");
        semantic.diagnostics = vec![diag(3), diag(10), diag(50)];
        let hunks = vec![hunk("h0", "test.rs", 8, 12)];
        let resp = nav.build(semantic, hunks);
        let ev = &resp.hunks[0];
        assert_eq!(ev.diagnostics.len(), 1);
        assert_eq!(ev.diagnostics[0].line, 10);
    }

    #[test]
    fn nearby_diagnostics_outside_hunk() {
        let nav = HunkSourceNavigator::new();
        let mut semantic = base_semantic("test.rs");
        semantic.diagnostics = vec![diag(3), diag(7), diag(50)];
        let hunks = vec![hunk("h0", "test.rs", 8, 12)];
        let resp = nav.build(semantic, hunks);
        let ev = &resp.hunks[0];
        assert!(ev.diagnostics.is_empty());
        let lines: Vec<u32> = ev.nearby_diagnostics.iter().map(|d| d.line).collect();
        assert!(lines.contains(&3));
        assert!(lines.contains(&7));
        assert!(!lines.contains(&50));
    }

    #[test]
    fn definitions_and_references_in_range() {
        let nav = HunkSourceNavigator::new();
        let mut semantic = base_semantic("test.rs");
        semantic.definitions = vec![loc("test.rs", 9, 9), loc("other.rs", 5, 5)];
        semantic.references = vec![loc("test.rs", 10, 10), loc("test.rs", 11, 11)];
        let hunks = vec![hunk("h0", "test.rs", 8, 12)];
        let resp = nav.build(semantic, hunks);
        let ev = &resp.hunks[0];
        assert_eq!(ev.definitions.len(), 1);
        assert_eq!(ev.references.len(), 2);
    }

    #[test]
    fn caps_truncate_symbols() {
        let nav = HunkSourceNavigator::new().with_max_symbols_per_hunk(2);
        let mut semantic = base_semantic("test.rs");
        semantic.all_symbols = (0..5)
            .map(|i| sym(&format!("s{i}"), 20 + i * 5, 23 + i * 5))
            .collect();
        let hunks = vec![hunk("h0", "test.rs", 25, 28)];
        let resp = nav.build(semantic, hunks);
        assert!(resp.limits.symbols_truncated);
    }

    #[test]
    fn caps_truncate_diagnostics() {
        let nav = HunkSourceNavigator::new().with_max_diagnostics_per_hunk(1);
        let mut semantic = base_semantic("test.rs");
        semantic.diagnostics = vec![diag(8), diag(10), diag(12)];
        let hunks = vec![hunk("h0", "test.rs", 8, 12)];
        let resp = nav.build(semantic, hunks);
        assert!(resp.limits.diagnostics_truncated);
    }

    #[test]
    fn caps_truncate_references() {
        let nav = HunkSourceNavigator::new().with_max_references_per_hunk(1);
        let mut semantic = base_semantic("test.rs");
        semantic.references = (0..5).map(|i| loc("test.rs", 8 + i, 8 + i)).collect();
        let hunks = vec![hunk("h0", "test.rs", 8, 12)];
        let resp = nav.build(semantic, hunks);
        assert!(resp.limits.references_truncated);
    }

    #[test]
    fn hierarchy_copied_from_semantic() {
        use egglsp::semantic_context::SemanticCallGraphSummary;
        let nav = HunkSourceNavigator::new();
        let mut semantic = base_semantic("test.rs");
        semantic.call_hierarchy = Some(SemanticCallGraphSummary {
            incoming_count: 1,
            outgoing_count: 0,
            items: vec![],
            incoming: vec![],
            outgoing: vec![],
            truncated: false,
            prepare_error: None,
            incoming_error: None,
            outgoing_error: None,
        });
        let hunks = vec![hunk("h0", "test.rs", 10, 15)];
        let resp = nav.build(semantic, hunks);
        assert!(resp.hunks[0].call_hierarchy.is_some());
    }

    #[test]
    fn source_excerpt_copied() {
        let nav = HunkSourceNavigator::new();
        let mut semantic = base_semantic("test.rs");
        semantic.source_excerpt = Some(SemanticSourceExcerpt {
            start_line: 1,
            end_line: 50,
            text: "fn main() {}".to_string(),
            truncated: false,
        });
        let hunks = vec![hunk("h0", "test.rs", 10, 15)];
        let resp = nav.build(semantic, hunks);
        assert!(resp.hunks[0].source_excerpt.is_some());
    }

    #[test]
    fn no_new_range_hunk_skipped() {
        let nav = HunkSourceNavigator::new();
        let mut hunk = hunk("h0", "test.rs", 10, 15);
        hunk.new_range = None;
        let semantic = base_semantic("test.rs");
        let resp = nav.build(semantic, vec![hunk]);
        assert!(resp.hunks.is_empty());
    }

    #[test]
    fn multiple_hunks() {
        let nav = HunkSourceNavigator::new();
        let mut semantic = base_semantic("test.rs");
        semantic.diagnostics = vec![diag(5), diag(50)];
        let hunks = vec![hunk("h0", "test.rs", 3, 7), hunk("h1", "test.rs", 48, 52)];
        let resp = nav.build(semantic, hunks);
        assert_eq!(resp.hunks.len(), 2);
        assert_eq!(resp.hunks[0].diagnostics.len(), 1);
        assert_eq!(resp.hunks[0].diagnostics[0].line, 5);
        assert_eq!(resp.hunks[1].diagnostics.len(), 1);
        assert_eq!(resp.hunks[1].diagnostics[0].line, 50);
    }

    #[test]
    fn no_enclosing_symbol_adds_note() {
        let nav = HunkSourceNavigator::new();
        let semantic = base_semantic("test.rs");
        let hunks = vec![hunk("h0", "test.rs", 10, 15)];
        let resp = nav.build(semantic, hunks);
        assert_eq!(resp.hunks[0].notes.len(), 1);
        assert!(resp.hunks[0].notes[0].contains("no enclosing symbol"));
    }
}
