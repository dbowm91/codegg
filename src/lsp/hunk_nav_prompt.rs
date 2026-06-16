use egglsp::hunk_context::HunkSourceNavigationResponse;
use std::fmt::Write;

const MAX_DIAGNOSTICS_SHOWN: usize = 5;
const MAX_SYMBOLS_SHOWN: usize = 5;
const MAX_REFERENCES_SHOWN: usize = 5;

/// Format a [`HunkSourceNavigationResponse`] into a compact,
/// agent-facing summary suitable for review/edit-planning prompts.
///
/// The output is deterministic, bounded in size, and preserves
/// freshness/truncation metadata. It does not dump raw JSON.
///
/// `extra_notes` carries caller-supplied notes (e.g. operational
/// state notes from the LSP service) that are appended to the
/// global notes section before per-hunk notes. Pass `&[]` when
/// no extra notes are available.
pub fn format_hunk_source_context_summary(
    response: &HunkSourceNavigationResponse,
    extra_notes: &[String],
) -> String {
    let mut out = String::with_capacity(512);

    writeln!(&mut out, "Hunk Source Context").unwrap();
    writeln!(&mut out, "File: {}", response.file_path).unwrap();

    // Diagnostic freshness from first hunk's evidence.
    if let Some(first) = response.hunks.first() {
        if let Some(de) = &first.diagnostic_evidence {
            writeln!(
                &mut out,
                "Diagnostic evidence: {:?}, age_ms={}",
                de.freshness, de.age_ms,
            )
            .unwrap();
        }
    }

    if response.truncated {
        writeln!(&mut out, "Note: output truncated").unwrap();
    }

    // Global notes.
    for note in &response.notes {
        writeln!(&mut out, "Note: {note}").unwrap();
    }

    // Caller-supplied notes (operational state, etc.).
    for note in extra_notes {
        writeln!(&mut out, "Note: {note}").unwrap();
    }

    for ev in response.hunks.iter() {
        writeln!(&mut out).unwrap();

        let hunk_id = &ev.hunk.id;
        writeln!(&mut out, "Hunk {hunk_id}").unwrap();

        // Focus range.
        if let Some(focus) = &ev.focus_range {
            writeln!(
                &mut out,
                "Focus: lines {}-{}",
                focus.start_line, focus.end_line,
            )
            .unwrap();
        }

        // Enclosing symbol.
        if let Some(sym) = &ev.enclosing_symbol {
            writeln!(
                &mut out,
                "Enclosing symbol: {} {} lines {}-{}",
                sym.kind, sym.name, sym.start_line, sym.end_line,
            )
            .unwrap();
        } else {
            writeln!(&mut out, "Enclosing symbol: none").unwrap();
        }

        // Related symbols (bounded).
        if !ev.related_symbols.is_empty() {
            let names: Vec<&str> = ev
                .related_symbols
                .iter()
                .take(MAX_SYMBOLS_SHOWN)
                .map(|s| s.name.as_str())
                .collect();
            write!(&mut out, "Related symbols: {}", names.join(", ")).unwrap();
            if ev.related_symbols.len() > MAX_SYMBOLS_SHOWN {
                write!(
                    &mut out,
                    " ({} more)",
                    ev.related_symbols.len() - MAX_SYMBOLS_SHOWN,
                )
                .unwrap();
            }
            writeln!(&mut out).unwrap();
        }

        // Diagnostics in hunk (bounded).
        if !ev.diagnostics.is_empty() {
            write!(&mut out, "Diagnostics: {} in hunk", ev.diagnostics.len(),).unwrap();
            let shown: Vec<&str> = ev
                .diagnostics
                .iter()
                .take(MAX_DIAGNOSTICS_SHOWN)
                .map(|d| d.message.as_str())
                .collect();
            if !shown.is_empty() {
                write!(&mut out, " ({})", shown.join("; ")).unwrap();
            }
            writeln!(&mut out).unwrap();
        }

        // Nearby diagnostics.
        if !ev.nearby_diagnostics.is_empty() {
            writeln!(
                &mut out,
                "Nearby diagnostics: {}",
                ev.nearby_diagnostics.len(),
            )
            .unwrap();
        }

        // Definitions.
        if !ev.definitions.is_empty() {
            writeln!(
                &mut out,
                "Definitions: {} intersecting",
                ev.definitions.len(),
            )
            .unwrap();
        }

        // References (bounded).
        if !ev.references.is_empty() {
            write!(&mut out, "References: {} intersecting", ev.references.len(),).unwrap();
            if ev.references.len() > MAX_REFERENCES_SHOWN {
                write!(
                    &mut out,
                    " ({} more)",
                    ev.references.len() - MAX_REFERENCES_SHOWN,
                )
                .unwrap();
            }
            writeln!(&mut out).unwrap();
        }

        // Call hierarchy summary.
        if let Some(ch) = &ev.call_hierarchy {
            writeln!(
                &mut out,
                "Call hierarchy: {} incoming, {} outgoing",
                ch.incoming_count, ch.outgoing_count,
            )
            .unwrap();
        }

        // Type hierarchy summary.
        if let Some(th) = &ev.type_hierarchy {
            writeln!(
                &mut out,
                "Type hierarchy: {} supertypes, {} subtypes",
                th.supertypes_count, th.subtypes_count,
            )
            .unwrap();
        }

        // Truncation flags (show once for the first hunk).
        if response
            .hunks
            .first()
            .is_some_and(|h| h.hunk.id == ev.hunk.id)
        {
            if response.limits.symbols_truncated {
                writeln!(&mut out, "Truncation: symbols truncated").unwrap();
            }
            if response.limits.diagnostics_truncated {
                writeln!(&mut out, "Truncation: diagnostics truncated").unwrap();
            }
            if response.limits.references_truncated {
                writeln!(&mut out, "Truncation: references truncated").unwrap();
            }
        }

        // Per-hunk notes.
        for note in &ev.notes {
            writeln!(&mut out, "Note: {note}").unwrap();
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use egglsp::capability::LspUnavailable;
    use egglsp::diagnostics::{FileDiagnostic, LspDiagnosticFreshness, LspDiagnosticSource};
    use egglsp::hunk_context::{
        HunkDescriptor, HunkEvidence, HunkLineRange, HunkSourceNavigationLimits,
        HunkSourceNavigationResponse,
    };
    use egglsp::lsp_types::DiagnosticSeverity;
    use egglsp::semantic_context::{SemanticDiagnosticEvidence, SemanticSymbolSummary};

    fn test_hunk_descriptor(id: &str, start: u32, end: u32) -> HunkDescriptor {
        HunkDescriptor {
            id: id.to_string(),
            file_path: "src/foo.rs".to_string(),
            old_range: None,
            new_range: Some(HunkLineRange {
                start_line: start,
                end_line: end,
            }),
            header: Some(format!(
                "@@ -{},0 +{},{} @@",
                start,
                end - start + 1,
                end - start + 1
            )),
            added_lines: (end - start + 1) as usize,
            removed_lines: 0,
            context_lines: 0,
        }
    }

    fn test_evidence(id: &str, start: u32, end: u32) -> HunkEvidence {
        HunkEvidence {
            hunk: test_hunk_descriptor(id, start, end),
            focus_range: Some(HunkLineRange {
                start_line: start.saturating_sub(5),
                end_line: end + 5,
            }),
            enclosing_symbol: Some(SemanticSymbolSummary {
                name: "parse_request".to_string(),
                kind: "function".to_string(),
                file: "src/foo.rs".to_string(),
                start_line: 31,
                start_column: 1,
                end_line: 70,
                end_column: 1,
            }),
            related_symbols: vec![
                SemanticSymbolSummary {
                    name: "validate_header".to_string(),
                    kind: "function".to_string(),
                    file: "src/foo.rs".to_string(),
                    start_line: 20,
                    start_column: 1,
                    end_line: 28,
                    end_column: 1,
                },
                SemanticSymbolSummary {
                    name: "parse_body".to_string(),
                    kind: "function".to_string(),
                    file: "src/foo.rs".to_string(),
                    start_line: 75,
                    start_column: 1,
                    end_line: 90,
                    end_column: 1,
                },
            ],
            diagnostics: vec![FileDiagnostic {
                file: "src/foo.rs".to_string(),
                line: 10,
                column: 1,
                message: "unused variable: x".to_string(),
                severity: DiagnosticSeverity::WARNING,
                source: None,
                code: None,
            }],
            nearby_diagnostics: vec![FileDiagnostic {
                file: "src/foo.rs".to_string(),
                line: 13,
                column: 1,
                message: "possible inflection point".to_string(),
                severity: DiagnosticSeverity::WARNING,
                source: None,
                code: None,
            }],
            definitions: vec![egglsp::semantic_context::SemanticLocation {
                file: "src/foo.rs".to_string(),
                start_line: 9,
                start_column: 1,
                end_line: 9,
                end_column: 10,
            }],
            references: vec![
                egglsp::semantic_context::SemanticLocation {
                    file: "src/foo.rs".to_string(),
                    start_line: 10,
                    start_column: 1,
                    end_line: 10,
                    end_column: 5,
                },
                egglsp::semantic_context::SemanticLocation {
                    file: "src/foo.rs".to_string(),
                    start_line: 11,
                    start_column: 1,
                    end_line: 11,
                    end_column: 5,
                },
                egglsp::semantic_context::SemanticLocation {
                    file: "src/foo.rs".to_string(),
                    start_line: 12,
                    start_column: 1,
                    end_line: 12,
                    end_column: 5,
                },
            ],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: Some(SemanticDiagnosticEvidence {
                freshness: LspDiagnosticFreshness::Fresh,
                source: LspDiagnosticSource::Unknown,
                age_ms: 1200,
                usable_evidence: true,
                server_generation: None,
                post_restart: false,
            }),
            section_truncations: vec![],
            unavailable: vec![],
            notes: vec![],
        }
    }

    fn base_response() -> HunkSourceNavigationResponse {
        let mut resp = HunkSourceNavigationResponse::new("src/foo.rs");
        resp.hunks.push(test_evidence("h0", 42, 48));
        resp.limits = HunkSourceNavigationLimits::default();
        resp.truncated = false;
        resp
    }

    #[test]
    fn includes_file_path() {
        let resp = base_response();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("File: src/foo.rs"));
    }

    #[test]
    fn includes_hunk_id_and_focus() {
        let resp = base_response();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Hunk h0"));
        assert!(summary.contains("Focus: lines"));
    }

    #[test]
    fn includes_enclosing_symbol() {
        let resp = base_response();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Enclosing symbol: function parse_request"));
    }

    #[test]
    fn includes_related_symbols() {
        let resp = base_response();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Related symbols: validate_header, parse_body"));
    }

    #[test]
    fn includes_diagnostics() {
        let resp = base_response();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Diagnostics: 1 in hunk"));
        assert!(summary.contains("unused variable: x"));
    }

    #[test]
    fn includes_nearby_diagnostics() {
        let resp = base_response();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Nearby diagnostics: 1"));
    }

    #[test]
    fn includes_definitions() {
        let resp = base_response();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Definitions: 1 intersecting"));
    }

    #[test]
    fn includes_references() {
        let resp = base_response();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("References: 3 intersecting"));
    }

    #[test]
    fn includes_diagnostic_freshness() {
        let resp = base_response();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Diagnostic evidence:"));
        assert!(summary.contains("Fresh"));
        assert!(summary.contains("age_ms=1200"));
    }

    #[test]
    fn truncation_flags_appear() {
        let mut resp = base_response();
        resp.limits.references_truncated = true;
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Truncation: references truncated"));
    }

    #[test]
    fn no_enclosing_symbol_shows_none() {
        let mut resp = base_response();
        resp.hunks[0].enclosing_symbol = None;
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Enclosing symbol: none"));
    }

    #[test]
    fn empty_response_produces_header_only() {
        let resp = HunkSourceNavigationResponse::new("src/empty.rs");
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Hunk Source Context"));
        assert!(summary.contains("File: src/empty.rs"));
    }

    #[test]
    fn truncated_flag_appears() {
        let mut resp = base_response();
        resp.truncated = true;
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Note: output truncated"));
    }

    #[test]
    fn truncation_bounded_symbols() {
        let mut resp = base_response();
        resp.hunks[0].related_symbols = (0..10)
            .map(|i| SemanticSymbolSummary {
                name: format!("sym{i}"),
                kind: "function".to_string(),
                file: "src/foo.rs".to_string(),
                start_line: i * 10,
                start_column: 1,
                end_line: i * 10 + 5,
                end_column: 1,
            })
            .collect();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("5 more"));
    }

    #[test]
    fn truncation_bounded_diagnostics() {
        let mut resp = base_response();
        resp.hunks[0].diagnostics = (0..10)
            .map(|i| FileDiagnostic {
                file: "src/foo.rs".to_string(),
                line: i,
                column: 1,
                message: format!("diag{i}"),
                severity: DiagnosticSeverity::ERROR,
                source: None,
                code: None,
            })
            .collect();
        let summary = format_hunk_source_context_summary(&resp, &[]);
        assert!(summary.contains("Diagnostics: 10 in hunk"));
        // Only first 5 messages shown
        assert!(summary.contains("diag0"));
        assert!(summary.contains("diag4"));
        assert!(!summary.contains("diag5"));
    }

    #[test]
    fn extra_notes_are_appended() {
        let resp = base_response();
        let extra = vec!["server indexing".to_string(), "warmup pending".to_string()];
        let summary = format_hunk_source_context_summary(&resp, &extra);
        assert!(summary.contains("Note: server indexing"));
        assert!(summary.contains("Note: warmup pending"));
    }
}
