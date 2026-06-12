use egglsp::diagnostics::FileDiagnostic;
use egglsp::hunk_context::HunkLineRange;
use egglsp::semantic_context::{SemanticLocation, SemanticSourceExcerpt, SemanticSymbolSummary};

pub fn ranges_overlap(a: &HunkLineRange, b: &HunkLineRange) -> bool {
    a.start_line <= b.end_line && b.start_line <= a.end_line
}

pub fn range_contains(container: &HunkLineRange, inner: &HunkLineRange) -> bool {
    container.start_line <= inner.start_line && inner.end_line <= container.end_line
}

pub fn distance_between_ranges(a: &HunkLineRange, b: &HunkLineRange) -> i64 {
    if ranges_overlap(a, b) {
        return 0;
    }
    if a.end_line < b.start_line {
        (b.start_line as i64) - (a.end_line as i64) - 1
    } else {
        (a.start_line as i64) - (b.end_line as i64) - 1
    }
}

pub fn expand_range(range: &HunkLineRange, radius: u32, file_line_count: u32) -> HunkLineRange {
    HunkLineRange {
        start_line: range.start_line.saturating_sub(radius).max(1),
        end_line: range
            .end_line
            .saturating_add(radius)
            .min(file_line_count.max(1)),
    }
}

pub fn symbol_to_range(sym: &SemanticSymbolSummary) -> HunkLineRange {
    HunkLineRange {
        start_line: sym.start_line,
        end_line: sym.end_line,
    }
}

pub fn diagnostic_to_range(diag: &FileDiagnostic) -> HunkLineRange {
    HunkLineRange {
        start_line: diag.line,
        end_line: diag.line,
    }
}

pub fn location_to_range(loc: &SemanticLocation) -> HunkLineRange {
    HunkLineRange {
        start_line: loc.start_line,
        end_line: loc.end_line,
    }
}

pub fn excerpt_to_range(excerpt: &SemanticSourceExcerpt) -> HunkLineRange {
    HunkLineRange {
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
    }
}

pub fn find_enclosing_symbol<'a>(
    hunk_range: &HunkLineRange,
    symbols: &'a [SemanticSymbolSummary],
) -> Option<&'a SemanticSymbolSummary> {
    let mut full_contain: Vec<&SemanticSymbolSummary> = Vec::new();
    let mut overlapping: Vec<&SemanticSymbolSummary> = Vec::new();

    for sym in symbols {
        let sr = symbol_to_range(sym);
        if range_contains(&sr, hunk_range) {
            full_contain.push(sym);
        } else if ranges_overlap(&sr, hunk_range) {
            overlapping.push(sym);
        }
    }

    if !full_contain.is_empty() {
        full_contain.sort_by_key(|s| {
            let r = symbol_to_range(s);
            (r.end_line - r.start_line, r.start_line)
        });
        return full_contain.into_iter().next();
    }

    if !overlapping.is_empty() {
        overlapping.sort_by_key(|s| {
            let r = symbol_to_range(s);
            let dist = distance_between_ranges(&r, hunk_range).unsigned_abs();
            (dist, r.end_line - r.start_line)
        });
        return overlapping.into_iter().next();
    }

    let mut nearest: Vec<(&SemanticSymbolSummary, i64)> = symbols
        .iter()
        .map(|s| {
            let r = symbol_to_range(s);
            (
                s,
                distance_between_ranges(&r, hunk_range).unsigned_abs() as i64,
            )
        })
        .filter(|(_, d)| *d <= 20)
        .collect();

    if nearest.is_empty() {
        return None;
    }

    nearest.sort_by_key(|(s, d)| {
        let r = symbol_to_range(s);
        (*d, r.end_line - r.start_line)
    });

    Some(nearest[0].0)
}

pub fn find_related_symbols<'a>(
    hunk_range: &HunkLineRange,
    symbols: &'a [SemanticSymbolSummary],
    max: usize,
) -> Vec<&'a SemanticSymbolSummary> {
    let expanded = expand_range(hunk_range, 10, u32::MAX);
    let mut related: Vec<&SemanticSymbolSummary> = symbols
        .iter()
        .filter(|s| {
            let sr = symbol_to_range(s);
            !range_contains(&sr, hunk_range) && ranges_overlap(&sr, &expanded)
        })
        .collect();

    related.sort_by_key(|s| {
        let r = symbol_to_range(s);
        distance_between_ranges(&r, hunk_range).unsigned_abs()
    });

    related.truncate(max);
    related
}

pub fn diagnostics_in_range<'a>(
    range: &HunkLineRange,
    diags: &'a [FileDiagnostic],
) -> Vec<&'a FileDiagnostic> {
    diags
        .iter()
        .filter(|d| {
            let dr = diagnostic_to_range(d);
            ranges_overlap(&dr, range)
        })
        .collect()
}

pub fn diagnostics_near_range<'a>(
    range: &HunkLineRange,
    diags: &'a [FileDiagnostic],
    radius: u32,
) -> Vec<&'a FileDiagnostic> {
    let expanded = expand_range(range, radius, u32::MAX);
    diags
        .iter()
        .filter(|d| {
            let dr = diagnostic_to_range(d);
            ranges_overlap(&dr, &expanded) && !ranges_overlap(&dr, range)
        })
        .collect()
}

pub fn locations_in_range<'a>(
    range: &HunkLineRange,
    locs: &'a [SemanticLocation],
) -> Vec<&'a SemanticLocation> {
    locs.iter()
        .filter(|l| {
            let lr = location_to_range(l);
            ranges_overlap(&lr, range)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egglsp::lsp_types::DiagnosticSeverity;

    fn range(s: u32, e: u32) -> HunkLineRange {
        HunkLineRange {
            start_line: s,
            end_line: e,
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

    #[test]
    fn overlap_basic() {
        assert!(ranges_overlap(&range(1, 5), &range(3, 8)));
        assert!(ranges_overlap(&range(3, 8), &range(1, 5)));
        assert!(!ranges_overlap(&range(1, 3), &range(5, 8)));
        assert!(!ranges_overlap(&range(5, 8), &range(1, 3)));
    }

    #[test]
    fn overlap_adjacent() {
        assert!(ranges_overlap(&range(1, 5), &range(5, 8)));
        assert!(ranges_overlap(&range(5, 8), &range(1, 5)));
    }

    #[test]
    fn overlap_same_line() {
        assert!(ranges_overlap(&range(5, 5), &range(5, 5)));
    }

    #[test]
    fn contain_basic() {
        assert!(range_contains(&range(1, 10), &range(3, 7)));
        assert!(range_contains(&range(1, 10), &range(1, 10)));
        assert!(!range_contains(&range(3, 7), &range(1, 10)));
        assert!(!range_contains(&range(1, 5), &range(3, 8)));
    }

    #[test]
    fn contain_same_line() {
        assert!(range_contains(&range(5, 5), &range(5, 5)));
    }

    #[test]
    fn distance_non_overlapping() {
        assert_eq!(distance_between_ranges(&range(1, 3), &range(5, 8)), 1);
        assert_eq!(distance_between_ranges(&range(5, 8), &range(1, 3)), 1);
    }

    #[test]
    fn distance_overlapping() {
        assert_eq!(distance_between_ranges(&range(1, 5), &range(3, 8)), 0);
    }

    #[test]
    fn distance_adjacent() {
        assert_eq!(distance_between_ranges(&range(1, 3), &range(4, 6)), 0);
    }

    #[test]
    fn expand_respects_bounds() {
        let r = range(5, 10);
        let expanded = expand_range(&r, 2, 100);
        assert_eq!(expanded.start_line, 3);
        assert_eq!(expanded.end_line, 12);
    }

    #[test]
    fn expand_clamps_to_file_start() {
        let r = range(1, 3);
        let expanded = expand_range(&r, 10, 100);
        assert_eq!(expanded.start_line, 1);
    }

    #[test]
    fn expand_clamps_to_file_end() {
        let r = range(90, 98);
        let expanded = expand_range(&r, 10, 100);
        assert_eq!(expanded.end_line, 100);
    }

    #[test]
    fn expand_zero_file_lines() {
        let r = range(1, 1);
        let expanded = expand_range(&r, 5, 0);
        assert_eq!(expanded.end_line, 1);
    }

    #[test]
    fn symbol_to_range_conversion() {
        let s = sym("fn_a", 10, 20);
        let r = symbol_to_range(&s);
        assert_eq!(r.start_line, 10);
        assert_eq!(r.end_line, 20);
    }

    #[test]
    fn diagnostic_to_range_single_line() {
        let d = diag(42);
        let r = diagnostic_to_range(&d);
        assert_eq!(r.start_line, 42);
        assert_eq!(r.end_line, 42);
    }

    #[test]
    fn find_enclosing_smallest_containing() {
        let hunk = range(12, 15);
        let outer = sym("mod_outer", 1, 50);
        let inner = sym("fn_inner", 10, 20);
        let symbols = vec![outer, inner];

        let result = find_enclosing_symbol(&hunk, &symbols).unwrap();
        assert_eq!(result.name, "fn_inner");
    }

    #[test]
    fn find_enclosing_overlap_fallback() {
        let hunk = range(8, 15);
        let s1 = sym("fn_a", 1, 10);
        let s2 = sym("fn_b", 12, 20);
        let symbols = vec![s1, s2];

        let result = find_enclosing_symbol(&hunk, &symbols).unwrap();
        assert!(result.name == "fn_a" || result.name == "fn_b");
    }

    #[test]
    fn find_enclosing_nearest_fallback() {
        let hunk = range(50, 55);
        let s = sym("fn_far", 1, 5);
        let symbols = vec![s];

        let result = find_enclosing_symbol(&hunk, &symbols);
        assert!(result.is_none());
    }

    #[test]
    fn find_enclosing_no_symbols() {
        let hunk = range(10, 15);
        let symbols = vec![];
        assert!(find_enclosing_symbol(&hunk, &symbols).is_none());
    }

    #[test]
    fn find_related_symbols_excludes_enclosing() {
        let hunk = range(12, 15);
        let enclosing = sym("fn_here", 10, 20);
        let nearby = sym("fn_nearby", 25, 30);
        let far = sym("fn_far", 1, 1);
        let symbols = vec![enclosing, nearby, far];

        let result = find_related_symbols(&hunk, &symbols, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "fn_nearby");
    }

    #[test]
    fn find_related_symbols_respects_max() {
        let hunk = range(10, 15);
        let symbols: Vec<SemanticSymbolSummary> = (0..5)
            .map(|i| sym(&format!("fn_{i}"), 20 + i * 5, 23 + i * 5))
            .collect();

        let result = find_related_symbols(&hunk, &symbols, 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn diagnostics_in_range_basic() {
        let range = range(5, 10);
        let d1 = diag(3);
        let d2 = diag(7);
        let d3 = diag(12);
        let diags = vec![d1, d2, d3];

        let result = diagnostics_in_range(&range, &diags);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line, 7);
    }

    #[test]
    fn diagnostics_in_range_boundary() {
        let range = range(5, 10);
        let d1 = diag(5);
        let d2 = diag(10);
        let diags = vec![d1, d2];

        let result = diagnostics_in_range(&range, &diags);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_diagnostics_near_range() {
        let range = range(5, 10);
        let d1 = diag(3);
        let d2 = diag(7);
        let d3 = diag(15);
        let diags = vec![d1, d2, d3];

        let result = diagnostics_near_range(&range, &diags, 5);
        assert_eq!(result.len(), 2);
        let lines: Vec<u32> = result.iter().map(|d| d.line).collect();
        assert!(lines.contains(&3));
        assert!(lines.contains(&15));
    }

    #[test]
    fn locations_in_range_basic() {
        let range = range(5, 10);
        let l1 = SemanticLocation {
            file: "a.rs".into(),
            start_line: 3,
            start_column: 1,
            end_line: 7,
            end_column: 1,
        };
        let l2 = SemanticLocation {
            file: "a.rs".into(),
            start_line: 12,
            start_column: 1,
            end_line: 15,
            end_column: 1,
        };
        let locs = vec![l1, l2];

        let result = locations_in_range(&range, &locs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start_line, 3);
    }

    #[test]
    fn excerpt_to_range_conversion() {
        let excerpt = SemanticSourceExcerpt {
            start_line: 10,
            end_line: 20,
            text: "code".into(),
            truncated: false,
        };
        let r = excerpt_to_range(&excerpt);
        assert_eq!(r.start_line, 10);
        assert_eq!(r.end_line, 20);
    }
}
