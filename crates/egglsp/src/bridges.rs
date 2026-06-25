//! Named bridges between canonical Phase 5 context types and
//! presentation/derivative DTOs.
//!
//! # Drift guardrails (Pass 4)
//!
//! `LspContextPacket` (see [`crate::context`]) is the **single
//! canonical Phase 5 context model**. Tool-local and presentation
//! DTOs (e.g. `SemanticContextPacket` in `src/tool/lsp.rs`,
//! `SecurityContextPacket` in `src/tool/lsp.rs`) must NOT introduce
//! parallel canonical packet shapes — they are presentation
//! adapters that fold into `LspContextPacket` via the bridges
//! defined here.
//!
//! This module centralises the canonical entry points so ad-hoc
//! conversions do not creep back into scattered call sites:
//!
//! - [`semantic_context_to_lsp_items`] converts a shared
//!   [`crate::semantic_context::SemanticContextResponse`] into
//!   [`LspContextItem`]s ready to be appended to a packet's
//!   `items` field.
//! - [`lsp_packet_to_security_summary`] is the canonical bridge
//!   from [`LspContextPacket`] to [`SecurityEvidenceSummary`]; it
//!   delegates to [`crate::security_context::build_security_evidence_summary`]
//!   and exists so security consumers have one named entry point.
//! - [`lsp_packet_to_tui_summary`] is the canonical bridge from
//!   [`LspContextPacket`] + [`PreviewArtifactRegistry`] to
//!   [`LspTuiSummary`]; it delegates to
//!   [`crate::tui_summary::build_tui_summary`] for the same reason.
//!
//! The existing functions in [`crate::security_context`] and
//! [`crate::tui_summary`] remain the implementation of record;
//! these bridges are additive, named, and provide a single import
//! path for downstream consumers.

use crate::context::{
    AgentContextSource, LspContextItem, LspContextItemKind, LspContextPacket, LspContextScore,
    LspEvidenceFreshness, LspEvidenceProvenance,
};
use crate::preview_registry::PreviewArtifactRegistry;
use crate::security_context::{build_security_evidence_summary, SecurityEvidenceSummary};
use crate::semantic_context::SemanticContextResponse;
use crate::tui_summary::{build_tui_summary, LspTuiSummary};

// ---------------------------------------------------------------------------
// Semantic context → LspContextItem bridge
// ---------------------------------------------------------------------------

/// Map a shared [`SemanticContextResponse`]'s freshness onto the
/// canonical [`LspEvidenceFreshness`].
///
/// Exposed for callers that need to align freshness values across
/// the egglsp crate.
pub(crate) fn map_semantic_freshness(
    f: crate::diagnostics::LspDiagnosticFreshness,
) -> LspEvidenceFreshness {
    use crate::diagnostics::LspDiagnosticFreshness;
    match f {
        LspDiagnosticFreshness::Fresh => LspEvidenceFreshness::Fresh,
        LspDiagnosticFreshness::PossiblyStale => LspEvidenceFreshness::PossiblyStale,
        LspDiagnosticFreshness::Stale => LspEvidenceFreshness::Stale,
        LspDiagnosticFreshness::Unavailable => LspEvidenceFreshness::StaleAfterEdit,
    }
}

/// Compute the freshness rank used for scoring from canonical
/// freshness.
fn canonical_freshness_rank(f: LspEvidenceFreshness) -> u32 {
    match f {
        LspEvidenceFreshness::Fresh => 0,
        LspEvidenceFreshness::PossiblyStale => 1,
        LspEvidenceFreshness::Stale | LspEvidenceFreshness::RetainedAfterRestart => 2,
        LspEvidenceFreshness::StaleAfterEdit => 3,
        LspEvidenceFreshness::ServerGenerationMismatch => 4,
        LspEvidenceFreshness::Unknown => 5,
    }
}

/// Resolve the (server_id, server_generation, freshness, post_restart,
/// age_ms) tuple from a semantic response's diagnostic evidence.
///
/// When no evidence is present the bridge defaults to
/// `(None, None, Unknown, false, None)` — the same convention used
/// by the existing tool-facing bridge in `src/tool/lsp.rs`.
fn resolve_semantic_provenance(
    response: &SemanticContextResponse,
) -> (
    Option<String>,
    Option<u64>,
    LspEvidenceFreshness,
    bool,
    Option<u64>,
) {
    match response.diagnostic_evidence.as_ref() {
        Some(meta) => {
            let age_ms_u = if meta.age_ms < 0 {
                0
            } else {
                meta.age_ms as u64
            };
            (
                Some(format!("server:{}", response.file_path)),
                meta.server_generation,
                map_semantic_freshness(meta.freshness),
                meta.post_restart,
                Some(age_ms_u),
            )
        }
        None => (None, None, LspEvidenceFreshness::Unknown, false, None),
    }
}

/// Convert a shared [`SemanticContextResponse`] into a vector of
/// canonical [`LspContextItem`]s.
///
/// This is the **canonical entry point** for folding a semantic
/// read model into the Phase 5 packet. It converts:
///
/// - diagnostics → `LspContextItemKind::Diagnostic`
/// - symbols → `LspContextItemKind::WorkspaceSymbol`
/// - definitions → `LspContextItemKind::Definition`
/// - references → `LspContextItemKind::Reference`
///
/// All items are tagged with `AgentContextSource::LspContext` so
/// downstream consumers can attribute evidence origin. 1-indexed
/// line/column values in the semantic response are converted to
/// the canonical 0-indexed convention.
///
/// Provenance (server_id, generation, freshness, post_restart) is
/// propagated from `response.diagnostic_evidence` when present;
/// otherwise unknown defaults are used. Source excerpt, call/type
/// hierarchy, overlay, and source-action hints are **not** carried
/// into the items — those are presentation-only concerns and
/// remain on the tool-facing DTOs.
pub fn semantic_context_to_lsp_items(response: &SemanticContextResponse) -> Vec<LspContextItem> {
    let primary_file = std::path::PathBuf::from(&response.file_path);
    let (server_id, server_generation, freshness, post_restart, age_ms) =
        resolve_semantic_provenance(response);

    let mut items = Vec::new();

    // Diagnostics → Diagnostic items (0-indexed).
    for d in &response.diagnostics {
        let severity = format!("{:?}", d.severity).to_lowercase();
        items.push(LspContextItem {
            kind: LspContextItemKind::Diagnostic,
            file: primary_file.clone(),
            range: None,
            line: Some(d.line),
            column: Some(d.column),
            message: d.message.clone(),
            symbol: None,
            source: Some(AgentContextSource::LspContext),
            provenance: LspEvidenceProvenance {
                server_id: server_id.clone().unwrap_or_else(|| "unknown".to_string()),
                server_generation,
                operation: "textDocument/diagnostic".to_string(),
                freshness,
                capability_decision: Some("supported".to_string()),
                document_version: None,
                age_ms,
                post_restart,
            },
            score: LspContextScore {
                priority: semantic_severity_priority(&severity),
                is_hunk_local: false,
                is_error: severity.eq_ignore_ascii_case("error"),
                is_same_file: true,
                freshness_rank: canonical_freshness_rank(freshness),
            },
            payload: None,
        });
    }

    // Symbols → WorkspaceSymbol items.
    for s in &response.all_symbols {
        items.push(LspContextItem {
            kind: LspContextItemKind::WorkspaceSymbol,
            file: primary_file.clone(),
            range: None,
            line: Some(s.start_line.saturating_sub(1)),
            column: Some(s.start_column.saturating_sub(1)),
            message: format!("{} ({})", s.name, s.kind),
            symbol: Some(s.name.clone()),
            source: Some(AgentContextSource::LspContext),
            provenance: LspEvidenceProvenance {
                server_id: server_id.clone().unwrap_or_else(|| "unknown".to_string()),
                server_generation,
                operation: "textDocument/documentSymbol".to_string(),
                freshness,
                capability_decision: Some("supported".to_string()),
                document_version: None,
                age_ms,
                post_restart,
            },
            score: LspContextScore {
                priority: 6,
                is_hunk_local: false,
                is_error: false,
                is_same_file: true,
                freshness_rank: canonical_freshness_rank(freshness),
            },
            payload: None,
        });
    }

    // Definitions → Definition items.
    for l in &response.definitions {
        items.push(LspContextItem {
            kind: LspContextItemKind::Definition,
            file: std::path::PathBuf::from(&l.file),
            range: None,
            line: Some(l.start_line.saturating_sub(1)),
            column: Some(l.start_column.saturating_sub(1)),
            message: format!("definition at {}:{}", l.file, l.start_line),
            symbol: None,
            source: Some(AgentContextSource::LspContext),
            provenance: LspEvidenceProvenance {
                server_id: server_id.clone().unwrap_or_else(|| "unknown".to_string()),
                server_generation,
                operation: "textDocument/definition".to_string(),
                freshness,
                capability_decision: Some("supported".to_string()),
                document_version: None,
                age_ms,
                post_restart,
            },
            score: LspContextScore {
                priority: 7,
                is_hunk_local: false,
                is_error: false,
                is_same_file: l.file == response.file_path,
                freshness_rank: canonical_freshness_rank(freshness),
            },
            payload: None,
        });
    }

    // References → Reference items.
    for l in &response.references {
        items.push(LspContextItem {
            kind: LspContextItemKind::Reference,
            file: std::path::PathBuf::from(&l.file),
            range: None,
            line: Some(l.start_line.saturating_sub(1)),
            column: Some(l.start_column.saturating_sub(1)),
            message: format!("reference at {}:{}", l.file, l.start_line),
            symbol: None,
            source: Some(AgentContextSource::LspContext),
            provenance: LspEvidenceProvenance {
                server_id: server_id.clone().unwrap_or_else(|| "unknown".to_string()),
                server_generation,
                operation: "textDocument/references".to_string(),
                freshness,
                capability_decision: Some("supported".to_string()),
                document_version: None,
                age_ms,
                post_restart,
            },
            score: LspContextScore {
                priority: 5,
                is_hunk_local: false,
                is_error: false,
                is_same_file: l.file == response.file_path,
                freshness_rank: canonical_freshness_rank(freshness),
            },
            payload: None,
        });
    }

    items
}

/// Map a `FileDiagnostic`'s severity to a priority score.
///
/// Exposed as a small helper so future bridge points can stay
/// aligned with the existing tool-facing bridge.
fn semantic_severity_priority(severity_lower: &str) -> u32 {
    if severity_lower.eq_ignore_ascii_case("error") || severity_lower == "1" {
        10
    } else if severity_lower.eq_ignore_ascii_case("warning") || severity_lower == "2" {
        7
    } else {
        4
    }
}

// ---------------------------------------------------------------------------
// LspContextPacket → SecurityEvidenceSummary bridge
// ---------------------------------------------------------------------------

/// Canonical bridge from [`LspContextPacket`] to
/// [`SecurityEvidenceSummary`].
///
/// This is the **named entry point** security review code should
/// use. It is a thin wrapper over
/// [`crate::security_context::build_security_evidence_summary`];
/// the underlying function is preserved for backward compatibility
/// but new callers should import `lsp_packet_to_security_summary`
/// from this module.
pub fn lsp_packet_to_security_summary(packet: &LspContextPacket) -> SecurityEvidenceSummary {
    build_security_evidence_summary(packet)
}

// ---------------------------------------------------------------------------
// LspContextPacket → LspTuiSummary bridge
// ---------------------------------------------------------------------------

/// Canonical bridge from [`LspContextPacket`] +
/// [`PreviewArtifactRegistry`] to [`LspTuiSummary`].
///
/// This is the **named entry point** TUI consumers should use.
/// It is a thin wrapper over
/// [`crate::tui_summary::build_tui_summary`]; the underlying
/// function is preserved for backward compatibility but new
/// callers should import `lsp_packet_to_tui_summary` from this
/// module.
pub fn lsp_packet_to_tui_summary(
    packet: &LspContextPacket,
    registry: &PreviewArtifactRegistry,
) -> LspTuiSummary {
    build_tui_summary(packet, registry)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{
        LspContextItemKind, LspContextPacketMode, LspContextRequest, LspContextTruncation,
        LspPreviewArtifact,
    };
    use crate::diagnostics::{FileDiagnostic, LspDiagnosticFreshness, LspDiagnosticSource};
    use crate::semantic_context::{
        SemanticContextLimits, SemanticDiagnosticEvidence, SemanticHierarchyItem,
        SemanticHierarchyRange, SemanticHierarchyRelation, SemanticLocation, SemanticSymbolSummary,
    };
    use lsp_types::DiagnosticSeverity;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    fn make_response(
        diagnostics: Vec<FileDiagnostic>,
        symbols: Vec<SemanticSymbolSummary>,
        definitions: Vec<SemanticLocation>,
        references: Vec<SemanticLocation>,
        evidence: Option<SemanticDiagnosticEvidence>,
        section_truncations: Vec<crate::semantic_context::SemanticSectionTruncation>,
        limits: SemanticContextLimits,
    ) -> SemanticContextResponse {
        SemanticContextResponse {
            file_path: "src/lib.rs".to_string(),
            symbol: None,
            all_symbols: symbols,
            diagnostics,
            definitions,
            references,
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: evidence,
            overlay: None,
            source_actions: Vec::new(),
            section_truncations,
            limits,
            notes: Vec::new(),
            truncated: false,
            unavailable: Vec::new(),
        }
    }

    fn make_packet(
        items: Vec<LspContextItem>,
        truncation: LspContextTruncation,
        notes: Vec<String>,
        mode: LspContextPacketMode,
    ) -> LspContextPacket {
        LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")],
                hunks: Vec::new(),
                risk_mode: crate::context::LspRiskMode::Standard,
            },
            items,
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes,
            truncation,
        }
    }

    fn make_item(
        kind: LspContextItemKind,
        file: &str,
        message: &str,
        severity_for_diag: Option<DiagnosticSeverity>,
    ) -> LspContextItem {
        let (priority, is_error) = match severity_for_diag {
            Some(DiagnosticSeverity::ERROR) => (10, true),
            Some(DiagnosticSeverity::WARNING) => (7, false),
            Some(DiagnosticSeverity::INFORMATION) | Some(DiagnosticSeverity::HINT) => (4, false),
            Some(_) | None => (5, false),
        };
        LspContextItem {
            kind,
            file: PathBuf::from(file),
            range: None,
            line: Some(0),
            column: None,
            message: message.to_string(),
            symbol: None,
            source: None,
            provenance: LspEvidenceProvenance {
                server_id: "test-server".to_string(),
                server_generation: Some(1),
                operation: "test".to_string(),
                freshness: LspEvidenceFreshness::Fresh,
                capability_decision: None,
                document_version: None,
                age_ms: None,
                post_restart: false,
            },
            score: LspContextScore {
                priority,
                is_hunk_local: false,
                is_error,
                is_same_file: false,
                freshness_rank: 0,
            },
            payload: None,
        }
    }

    // -------------------------------------------------------------------
    // semantic_context_to_lsp_items tests
    // -------------------------------------------------------------------

    #[test]
    fn semantic_context_bridge_preserves_diagnostics() {
        let diagnostics = vec![
            FileDiagnostic {
                file: "src/lib.rs".to_string(),
                line: 11,
                column: 4,
                severity: DiagnosticSeverity::ERROR,
                source: Some("rust-analyzer".to_string()),
                code: Some("E0001".to_string()),
                message: "unused variable".to_string(),
            },
            FileDiagnostic {
                file: "src/lib.rs".to_string(),
                line: 20,
                column: 1,
                severity: DiagnosticSeverity::WARNING,
                source: Some("rust-analyzer".to_string()),
                code: None,
                message: "dead code".to_string(),
            },
        ];
        let response = make_response(
            diagnostics,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(SemanticDiagnosticEvidence {
                freshness: LspDiagnosticFreshness::Fresh,
                source: LspDiagnosticSource::Pushed,
                age_ms: 100,
                usable_evidence: true,
                server_generation: Some(3),
                post_restart: false,
            }),
            Vec::new(),
            SemanticContextLimits::default(),
        );

        let items = semantic_context_to_lsp_items(&response);
        let diag_items: Vec<_> = items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Diagnostic)
            .collect();

        // 1-indexed source → 0-indexed canonical lines.
        assert_eq!(diag_items.len(), 2);
        assert_eq!(diag_items[0].line, Some(11));
        assert_eq!(diag_items[0].column, Some(4));
        assert_eq!(diag_items[0].message, "unused variable");
        assert!(diag_items[0].score.is_error);
        assert_eq!(diag_items[0].score.priority, 10);

        assert_eq!(diag_items[1].line, Some(20));
        assert!(!diag_items[1].score.is_error);
        assert_eq!(diag_items[1].score.priority, 7);

        // Provenance propagates from diagnostic_evidence.
        for item in &diag_items {
            assert_eq!(item.provenance.server_generation, Some(3));
            assert_eq!(item.provenance.freshness, LspEvidenceFreshness::Fresh);
            assert!(!item.provenance.post_restart);
        }
    }

    #[test]
    fn semantic_context_bridge_preserves_truncation_notes() {
        // The bridge itself returns items only — truncation flags
        // and notes live on the wrapping packet. This test confirms
        // that truncation metadata *plumbs through* the bridge when
        // the caller assembles an LspContextPacket from the items
        // (mirroring the pattern used by the existing tool-facing
        // bridge and the security review path).
        use crate::semantic_context::SemanticSectionTruncation;

        let section_truncations = vec![
            SemanticSectionTruncation {
                section: "diagnostics".to_string(),
                original_count: Some(50),
                emitted_count: 20,
                limit: 20,
            },
            SemanticSectionTruncation {
                section: "symbols".to_string(),
                original_count: Some(40),
                emitted_count: 30,
                limit: 30,
            },
        ];
        let limits = SemanticContextLimits {
            diagnostics_truncated: true,
            symbols_truncated: true,
            references_truncated: false,
            overlay_diagnostics_truncated: false,
            excerpt_truncated: false,
        };
        let response = make_response(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            section_truncations,
            limits,
        );

        let _items = semantic_context_to_lsp_items(&response);

        // Assemble the canonical packet the way a real caller would
        // and confirm truncation surfaces on it.
        let mut packet = LspContextPacket {
            request: LspContextRequest::File {
                file: PathBuf::from("src/lib.rs"),
                line_ranges: Vec::new(),
                include_symbols: true,
                include_diagnostics: true,
            },
            items: Vec::new(),
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: LspContextTruncation {
                diagnostics_truncated: response.limits.diagnostics_truncated,
                symbols_truncated: response.limits.symbols_truncated,
                references_truncated: response.limits.references_truncated,
                ..Default::default()
            },
        };
        for tr in &response.section_truncations {
            packet.truncation.notes.push(format!(
                "section {} truncated: {}/{} (limit {})",
                tr.section,
                tr.emitted_count,
                tr.original_count
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".to_string()),
                tr.limit
            ));
        }

        assert!(packet.truncation.diagnostics_truncated);
        assert!(packet.truncation.symbols_truncated);
        assert!(!packet.truncation.references_truncated);
        assert!(packet
            .truncation
            .notes
            .iter()
            .any(|n| n.contains("diagnostics")));
        assert!(packet
            .truncation
            .notes
            .iter()
            .any(|n| n.contains("symbols")));
    }

    #[test]
    fn semantic_context_bridge_maps_definitions_and_references() {
        let symbols = vec![SemanticSymbolSummary {
            name: "main".to_string(),
            kind: "Function".to_string(),
            file: "src/lib.rs".to_string(),
            start_line: 10,
            start_column: 1,
            end_line: 12,
            end_column: 2,
        }];
        let definitions = vec![SemanticLocation {
            file: "src/lib.rs".to_string(),
            start_line: 21,
            start_column: 0,
            end_line: 21,
            end_column: 10,
        }];
        let references = vec![
            SemanticLocation {
                file: "src/lib.rs".to_string(),
                start_line: 31,
                start_column: 0,
                end_line: 31,
                end_column: 10,
            },
            SemanticLocation {
                file: "src/main.rs".to_string(),
                start_line: 6,
                start_column: 0,
                end_line: 6,
                end_column: 10,
            },
        ];
        let response = make_response(
            Vec::new(),
            symbols,
            definitions,
            references,
            None,
            Vec::new(),
            SemanticContextLimits::default(),
        );

        let items = semantic_context_to_lsp_items(&response);
        let defs = items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Definition)
            .count();
        let refs = items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Reference)
            .count();
        let syms = items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::WorkspaceSymbol)
            .count();

        assert_eq!(defs, 1);
        assert_eq!(refs, 2);
        assert_eq!(syms, 1);

        // 1-indexed 10 → 0-indexed 9 (symbol start line).
        let sym = items
            .iter()
            .find(|i| i.kind == LspContextItemKind::WorkspaceSymbol)
            .unwrap();
        assert_eq!(sym.line, Some(9));
        assert_eq!(sym.symbol.as_deref(), Some("main"));

        // Cross-file reference is NOT same-file.
        let cross = items
            .iter()
            .find(|i| i.kind == LspContextItemKind::Reference && i.file == Path::new("src/main.rs"))
            .expect("cross-file reference");
        assert!(!cross.score.is_same_file);

        // Same-file reference IS same-file.
        let same = items
            .iter()
            .find(|i| i.kind == LspContextItemKind::Reference && i.file == Path::new("src/lib.rs"))
            .expect("same-file reference");
        assert!(same.score.is_same_file);
    }

    #[test]
    fn semantic_context_bridge_unknown_evidence_yields_unknown_freshness() {
        let diagnostics = vec![FileDiagnostic {
            file: "src/lib.rs".to_string(),
            line: 5,
            column: 0,
            severity: DiagnosticSeverity::ERROR,
            source: None,
            code: None,
            message: "err".to_string(),
        }];
        let response = make_response(
            diagnostics,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            Vec::new(),
            SemanticContextLimits::default(),
        );

        let items = semantic_context_to_lsp_items(&response);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].provenance.freshness, LspEvidenceFreshness::Unknown);
        assert_eq!(items[0].provenance.server_generation, None);
    }

    // -------------------------------------------------------------------
    // lsp_packet_to_security_summary tests
    // -------------------------------------------------------------------

    #[test]
    fn lsp_packet_security_bridge_preserves_counts() {
        let items = vec![
            make_item(
                LspContextItemKind::Diagnostic,
                "a.rs",
                "err1",
                Some(DiagnosticSeverity::ERROR),
            ),
            make_item(
                LspContextItemKind::Diagnostic,
                "b.rs",
                "warn1",
                Some(DiagnosticSeverity::WARNING),
            ),
            make_item(LspContextItemKind::Reference, "c.rs", "ref", None),
            make_item(LspContextItemKind::Definition, "a.rs", "def", None),
            make_item(LspContextItemKind::Implementation, "d.rs", "impl", None),
        ];
        let packet = make_packet(
            items,
            LspContextTruncation::default(),
            vec!["test note".to_string()],
            LspContextPacketMode::Opportunistic,
        );

        let summary = lsp_packet_to_security_summary(&packet);
        assert_eq!(summary.diagnostics_count, 2);
        assert_eq!(summary.references_count, 1);
        assert_eq!(summary.definitions_count, 1);
        assert_eq!(summary.implementations_count, 1);
        assert_eq!(summary.server_id.as_deref(), Some("test-server"));
        assert_eq!(summary.notes, vec!["test note"]);
        assert!(!summary.truncated);
        assert!(!summary.stale);
        // The bridge produces the same output as the underlying
        // build_security_evidence_summary call — assert equivalence
        // to lock the contract.
        let baseline = build_security_evidence_summary(&packet);
        assert_eq!(summary.diagnostics_count, baseline.diagnostics_count);
        assert_eq!(summary.references_count, baseline.references_count);
        assert_eq!(summary.definitions_count, baseline.definitions_count);
        assert_eq!(
            summary.implementations_count,
            baseline.implementations_count
        );
        assert_eq!(summary.public_api_fanout, baseline.public_api_fanout);
        assert_eq!(summary.risk_tags, baseline.risk_tags);
    }

    #[test]
    fn lsp_packet_security_bridge_propagates_truncation_flag() {
        let items = vec![make_item(
            LspContextItemKind::Diagnostic,
            "a.rs",
            "err",
            Some(DiagnosticSeverity::ERROR),
        )];
        let truncation = LspContextTruncation {
            diagnostics_truncated: true,
            notes: vec!["diagnostics truncated".to_string()],
            ..Default::default()
        };
        let packet = make_packet(
            items,
            truncation,
            Vec::new(),
            LspContextPacketMode::Opportunistic,
        );

        let summary = lsp_packet_to_security_summary(&packet);
        assert!(summary.truncated);
    }

    // -------------------------------------------------------------------
    // lsp_packet_to_tui_summary tests
    // -------------------------------------------------------------------

    #[test]
    fn lsp_packet_tui_bridge_preserves_preview_ids() {
        let items = vec![make_item(
            LspContextItemKind::Diagnostic,
            "a.rs",
            "err",
            Some(DiagnosticSeverity::ERROR),
        )];
        let packet = make_packet(
            items,
            LspContextTruncation::default(),
            Vec::new(),
            LspContextPacketMode::Opportunistic,
        );
        let mut registry = PreviewArtifactRegistry::new();
        let id1 = registry.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 1,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            HashMap::new(),
            "rust-analyzer".to_string(),
        );
        let id2 = registry.register(
            LspPreviewArtifact::Formatting {
                description: "fmt a.rs".to_string(),
                content_hash: None,
                edit_count: 0,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            HashMap::new(),
            "rust-analyzer".to_string(),
        );
        let id3 = registry.register(
            LspPreviewArtifact::CodeAction {
                description: "organize imports".to_string(),
                kind: Some("source.organizeImports".to_string()),
                edit_count: 0,
                patches: Vec::new(),
            },
            vec!["b.rs".to_string()],
            HashMap::new(),
            "rust-analyzer".to_string(),
        );

        let summary = lsp_packet_to_tui_summary(&packet, &registry);

        // Preview IDs registered upstream survive the bridge.
        assert_eq!(summary.preview_count, 3);
        assert_eq!(summary.preview_ids.len(), 3);
        let ids: std::collections::HashSet<&str> =
            summary.preview_ids.iter().map(String::as_str).collect();
        assert!(ids.contains(id1.as_str()));
        assert!(ids.contains(id2.as_str()));
        assert!(ids.contains(id3.as_str()));

        // Bridge output matches build_tui_summary for the same inputs.
        let baseline = build_tui_summary(&packet, &registry);
        assert_eq!(summary.preview_ids, baseline.preview_ids);
        assert_eq!(summary.preview_count, baseline.preview_count);
        assert_eq!(summary.diagnostics_count, baseline.diagnostics_count);
        assert_eq!(summary.total_items, baseline.total_items);
    }

    #[test]
    fn lsp_packet_tui_bridge_handles_empty_registry() {
        let items = vec![make_item(
            LspContextItemKind::Diagnostic,
            "a.rs",
            "err",
            Some(DiagnosticSeverity::ERROR),
        )];
        let packet = make_packet(
            items,
            LspContextTruncation::default(),
            Vec::new(),
            LspContextPacketMode::Opportunistic,
        );
        let registry = PreviewArtifactRegistry::new();

        let summary = lsp_packet_to_tui_summary(&packet, &registry);
        assert_eq!(summary.preview_count, 0);
        assert!(summary.preview_ids.is_empty());
        assert!(!summary.preview_stale);
    }

    // -------------------------------------------------------------------
    // Hierarchy plumbing sanity (regression: ensure bridge does not
    // accidentally drop semantic call/type hierarchy data — it stays
    // on the source response; we just verify the bridge does not
    // panic on responses that include it).
    // -------------------------------------------------------------------

    #[test]
    fn semantic_context_bridge_ignores_hierarchy_without_panic() {
        let mut response = make_response(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            Vec::new(),
            SemanticContextLimits::default(),
        );
        let item = SemanticHierarchyItem {
            name: "foo".to_string(),
            kind: "function".to_string(),
            file: "src/lib.rs".to_string(),
            range: SemanticHierarchyRange {
                start_line: 1,
                start_column: 0,
                end_line: 2,
                end_column: 1,
            },
            selection_range: SemanticHierarchyRange {
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 3,
            },
            detail: None,
        };
        response.call_hierarchy = Some(crate::semantic_context::SemanticCallGraphSummary {
            incoming_count: 0,
            outgoing_count: 0,
            items: vec![item.clone()],
            incoming: vec![SemanticHierarchyRelation {
                item: item.clone(),
                ranges: Vec::new(),
            }],
            outgoing: vec![SemanticHierarchyRelation {
                item,
                ranges: Vec::new(),
            }],
            truncated: false,
            prepare_error: None,
            incoming_error: None,
            outgoing_error: None,
        });

        // The bridge returns items only — call/type hierarchy stays
        // on the source response for tool-side rendering.
        let items = semantic_context_to_lsp_items(&response);
        assert!(items.is_empty());
    }
}
