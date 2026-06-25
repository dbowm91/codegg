//! TUI-facing summary model for LSP context packets.
//!
//! Builds compact, structured summaries from [`LspContextPacket`] and
//! [`PreviewArtifactRegistry`] for display in the terminal UI status
//! bar and detail panels.

use crate::context::{
    LspContextItemKind, LspContextPacket, LspContextPacketMode, LspEvidenceFreshness,
};
use crate::preview_registry::PreviewArtifactRegistry;

// ---------------------------------------------------------------------------
// Summary struct
// ---------------------------------------------------------------------------

/// Compact summary of LSP context state for TUI display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LspTuiSummary {
    /// High-level server status string.
    pub server_status: String,
    /// Server identifier (e.g. "rust-analyzer").
    pub server_id: Option<String>,
    /// Server generation at time of summary.
    pub server_generation: Option<u64>,
    /// Number of diagnostic items in the packet.
    pub diagnostics_count: usize,
    /// Number of reference items.
    pub references_count: usize,
    /// Number of definition items (definitions + declarations).
    pub definitions_count: usize,
    /// Total item count.
    pub total_items: usize,
    /// Whether any item was truncated by budget enforcement.
    pub truncated: bool,
    /// Whether any evidence item is stale.
    pub stale: bool,
    /// Number of items with Stale freshness (vs PossiblyStale).
    pub stale_freshness_count: usize,
    /// Number of items with PossiblyStale freshness.
    pub possibly_stale_count: usize,
    /// Number of items with Fresh freshness.
    pub fresh_count: usize,
    /// Number of registered preview artifacts.
    pub preview_count: usize,
    /// Whether any registered preview artifact has a stale base.
    pub preview_stale: bool,
    /// IDs of registered preview artifacts (truncated to the most
    /// recent for display).
    pub preview_ids: Vec<String>,
    /// Operations the server reports as unsupported (e.g.
    /// "implementation unsupported by basedpyright"). Derived from
    /// capability-decision notes on items.
    pub unsupported_operations: Vec<String>,
    /// Whether item counts came from an actual context packet (true)
    /// or are placeholder zeros from the live-service status path (false).
    pub counts_from_packet: bool,
    /// General notes from packet assembly.
    pub notes: Vec<String>,
    /// Operational state notes (e.g. "LSP state: indexing").
    pub operational_notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Build an [`LspTuiSummary`] from a context packet and preview registry.
pub fn build_tui_summary(
    packet: &LspContextPacket,
    registry: &PreviewArtifactRegistry,
) -> LspTuiSummary {
    let mut diagnostics_count = 0usize;
    let mut references_count = 0usize;
    let mut definitions_count = 0usize;
    let mut stale = false;
    let mut server_id: Option<String> = None;
    let mut server_generation: Option<u64> = None;
    let mut stale_freshness_count = 0usize;
    let mut possibly_stale_count = 0usize;
    let mut fresh_count = 0usize;
    let mut unsupported_operations: Vec<String> = Vec::new();

    for item in &packet.items {
        match item.kind {
            LspContextItemKind::Diagnostic => diagnostics_count += 1,
            LspContextItemKind::Reference => references_count += 1,
            LspContextItemKind::Definition | LspContextItemKind::Declaration => {
                definitions_count += 1
            }
            _ => {}
        }

        match item.provenance.freshness {
            LspEvidenceFreshness::Stale
            | LspEvidenceFreshness::RetainedAfterRestart
            | LspEvidenceFreshness::ServerGenerationMismatch => {
                stale = true;
                stale_freshness_count += 1;
            }
            LspEvidenceFreshness::PossiblyStale | LspEvidenceFreshness::StaleAfterEdit => {
                stale = true;
                possibly_stale_count += 1;
            }
            LspEvidenceFreshness::Fresh | LspEvidenceFreshness::Unknown => {
                fresh_count += 1;
            }
        }

        if server_id.is_none() && !item.provenance.server_id.is_empty() {
            server_id = Some(item.provenance.server_id.clone());
        }
        if server_generation.is_none() {
            server_generation = item.provenance.server_generation;
        }

        // Surface "unsupported" capability decisions.
        if let Some(decision) = &item.provenance.capability_decision {
            let tag = format!("{}: {}", item.provenance.operation, decision);
            if !unsupported_operations.contains(&tag) {
                unsupported_operations.push(tag);
            }
        }
    }

    let server_status = match packet.mode {
        LspContextPacketMode::Disabled => "unavailable".to_string(),
        LspContextPacketMode::Opportunistic => {
            if stale {
                "degraded".to_string()
            } else {
                "ready".to_string()
            }
        }
        LspContextPacketMode::Required => {
            if stale {
                "degraded".to_string()
            } else {
                "ready".to_string()
            }
        }
    };

    let truncated = packet.truncation.bytes_truncated
        || packet.truncation.files_truncated
        || packet.truncation.diagnostics_truncated
        || packet.truncation.references_truncated;

    let preview_stale = registry.recent(registry.len()).iter().any(|e| e.stale_base);

    let preview_ids: Vec<String> = registry.recent(8).iter().map(|e| e.id.clone()).collect();

    let operational_notes: Vec<String> = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::OperationalNote)
        .map(|i| i.message.clone())
        .collect();

    LspTuiSummary {
        server_status,
        server_id,
        server_generation,
        diagnostics_count,
        references_count,
        definitions_count,
        total_items: packet.items.len(),
        truncated,
        stale,
        stale_freshness_count,
        possibly_stale_count,
        fresh_count,
        preview_count: registry.len(),
        preview_stale,
        preview_ids,
        unsupported_operations,
        counts_from_packet: true,
        notes: packet.notes.clone(),
        operational_notes,
    }
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

/// Render a compact one-liner status string for the TUI status bar.
///
/// Example: `"LSP: ready | rust-analyzer gen=3 | 4d 2r 1def | truncated"`
pub fn render_tui_status_line(summary: &LspTuiSummary) -> String {
    let server_info = match (&summary.server_id, summary.server_generation) {
        (Some(id), Some(gen)) => format!("{} gen={gen}", id),
        (Some(id), None) => id.clone(),
        (None, Some(gen)) => format!("gen={gen}"),
        (None, None) => "unknown".to_string(),
    };

    let counts = if summary.counts_from_packet {
        format!(
            "{}d {}r {}def",
            summary.diagnostics_count, summary.references_count, summary.definitions_count
        )
    } else {
        "—".to_string()
    };

    let mut parts = vec![
        "LSP:".to_string(),
        summary.server_status.clone(),
        "|".to_string(),
        server_info,
        "|".to_string(),
        counts,
    ];

    if summary.truncated {
        parts.push("|".to_string());
        parts.push("truncated".to_string());
    }

    if summary.preview_count > 0 {
        parts.push("|".to_string());
        parts.push(format!("{}p", summary.preview_count));
    }

    parts.join(" ")
}

/// Render a multi-line detail summary for the TUI detail panel.
///
/// Format:
/// ```text
/// LSP: ready rust-analyzer gen=4
/// Context: 12 items, 3 diagnostics, 4 refs, 1 definitions
/// Freshness: 10 fresh, 2 stale, 0 possibly-stale
/// Truncated: yes
/// Previews: preview-1-1234567890, preview-2-1234567891 (2 pending, stale=true)
/// Unsupported: implementation (basedpyright)
/// Notes: LSP state: indexing
/// ```
pub fn render_tui_summary_detail(summary: &LspTuiSummary) -> String {
    let mut lines = Vec::new();

    let status = render_tui_status_line(summary);
    lines.push(status);

    if summary.counts_from_packet {
        lines.push(format!(
            "Context: {} items, {} diagnostics, {} refs, {} definitions",
            summary.total_items,
            summary.diagnostics_count,
            summary.references_count,
            summary.definitions_count
        ));

        let freshness_line = format!(
            "Freshness: {} fresh, {} stale, {} possibly-stale",
            summary.fresh_count, summary.stale_freshness_count, summary.possibly_stale_count
        );
        lines.push(freshness_line);
    } else {
        lines.push("Context: not collected in status snapshot".to_string());
    }

    if summary.truncated {
        lines.push("Truncated: yes".to_string());
    }

    if !summary.preview_ids.is_empty() {
        let preview_display = if summary.preview_ids.len() > 4 {
            format!(
                "{}, … ({} more)",
                summary.preview_ids[..4].join(", "),
                summary.preview_ids.len() - 4
            )
        } else {
            summary.preview_ids.join(", ")
        };
        lines.push(format!(
            "Previews: {} ({} pending, stale={})",
            preview_display, summary.preview_count, summary.preview_stale
        ));
    }

    if !summary.unsupported_operations.is_empty() {
        lines.push(format!(
            "Unsupported: {}",
            summary.unsupported_operations.join("; ")
        ));
    }

    let all_notes: Vec<&str> = summary
        .notes
        .iter()
        .chain(summary.operational_notes.iter())
        .map(|s| s.as_str())
        .collect();

    if all_notes.is_empty() {
        lines.push("Notes: (none)".to_string());
    } else {
        lines.push(format!("Notes: {}", all_notes.join("; ")));
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{
        LspContextRequest, LspContextScore, LspEvidenceProvenance, LspPreviewArtifact,
    };
    use std::path::PathBuf;

    fn make_item(
        kind: LspContextItemKind,
        file: &str,
        message: &str,
    ) -> crate::context::LspContextItem {
        crate::context::LspContextItem {
            kind,
            file: PathBuf::from(file),
            range: None,
            line: None,
            column: None,
            message: message.to_string(),
            symbol: None,
            source: None,
            provenance: LspEvidenceProvenance {
                server_id: "rust-analyzer".to_string(),
                server_generation: Some(3),
                operation: "test".to_string(),
                freshness: LspEvidenceFreshness::Fresh,
                capability_decision: None,
                document_version: None,
                age_ms: None,
                post_restart: false,
            },
            score: LspContextScore {
                priority: 10,
                is_hunk_local: false,
                is_error: false,
                is_same_file: false,
                freshness_rank: 0,
            },
            payload: None,
        }
    }

    fn make_packet(
        items: Vec<crate::context::LspContextItem>,
        mode: LspContextPacketMode,
    ) -> LspContextPacket {
        LspContextPacket {
            request: LspContextRequest::File {
                file: PathBuf::from("test.rs"),
                line_ranges: vec![],
                include_symbols: false,
                include_diagnostics: true,
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
            notes: Vec::new(),
            truncation: Default::default(),
        }
    }

    #[test]
    fn test_build_summary_from_packet() {
        let items = vec![
            make_item(LspContextItemKind::Diagnostic, "a.rs", "error: unused"),
            make_item(LspContextItemKind::Diagnostic, "a.rs", "warn: dead_code"),
            make_item(LspContextItemKind::Reference, "b.rs", "ref: foo"),
            make_item(LspContextItemKind::Definition, "c.rs", "def: bar"),
        ];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();

        let summary = build_tui_summary(&packet, &registry);
        assert_eq!(summary.server_status, "ready");
        assert_eq!(summary.server_id, Some("rust-analyzer".to_string()));
        assert_eq!(summary.server_generation, Some(3));
        assert_eq!(summary.diagnostics_count, 2);
        assert_eq!(summary.references_count, 1);
        assert_eq!(summary.definitions_count, 1);
        assert!(!summary.truncated);
        assert!(!summary.stale);
        assert_eq!(summary.preview_count, 0);
        assert!(!summary.preview_stale);
    }

    #[test]
    fn test_render_status_line_ready() {
        let items = vec![
            make_item(LspContextItemKind::Diagnostic, "a.rs", "err"),
            make_item(LspContextItemKind::Diagnostic, "a.rs", "warn"),
            make_item(LspContextItemKind::Reference, "b.rs", "ref"),
            make_item(LspContextItemKind::Definition, "c.rs", "def"),
        ];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();
        let summary = build_tui_summary(&packet, &registry);

        let line = render_tui_status_line(&summary);
        assert!(line.contains("LSP:"));
        assert!(line.contains("ready"));
        assert!(line.contains("rust-analyzer"));
        assert!(line.contains("gen=3"));
        assert!(line.contains("2d"));
        assert!(line.contains("1r"));
        assert!(line.contains("1def"));
        assert!(!line.contains("truncated"));
    }

    #[test]
    fn test_render_status_line_degraded() {
        let mut item = make_item(LspContextItemKind::Diagnostic, "a.rs", "err");
        item.provenance.freshness = LspEvidenceFreshness::Stale;
        let packet = make_packet(vec![item], LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();
        let summary = build_tui_summary(&packet, &registry);

        let line = render_tui_status_line(&summary);
        assert!(line.contains("degraded"));
    }

    #[test]
    fn test_render_detail_with_previews() {
        let items = vec![
            make_item(LspContextItemKind::Diagnostic, "a.rs", "err"),
            make_item(LspContextItemKind::Reference, "b.rs", "ref"),
        ];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let mut registry = PreviewArtifactRegistry::new();
        registry.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 1,
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );

        let summary = build_tui_summary(&packet, &registry);
        let detail = render_tui_summary_detail(&summary);

        assert!(detail.contains("LSP: ready"));
        assert!(detail.contains("rust-analyzer"));
        assert!(detail.contains("gen=3"));
        assert!(detail.contains("2 items, 1 diagnostics, 1 refs, 0 definitions"));
        assert!(detail.contains("Previews:"));
        assert!(detail.contains("1 pending"));
        assert!(detail.contains("Notes: (none)"));
    }

    #[test]
    fn test_summary_empty_packet() {
        let packet = make_packet(Vec::new(), LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();

        let summary = build_tui_summary(&packet, &registry);
        assert_eq!(summary.diagnostics_count, 0);
        assert_eq!(summary.references_count, 0);
        assert_eq!(summary.definitions_count, 0);
        assert!(!summary.truncated);
        assert!(!summary.stale);
        assert_eq!(summary.preview_count, 0);
        assert!(summary.notes.is_empty());
        assert!(summary.operational_notes.is_empty());
    }

    // -----------------------------------------------------------------------
    // Pass 8: Extended TUI summary tests
    // -----------------------------------------------------------------------

    #[test]
    fn summary_lists_context_counts() {
        let items = vec![
            make_item(LspContextItemKind::Diagnostic, "a.rs", "err1"),
            make_item(LspContextItemKind::Diagnostic, "a.rs", "err2"),
            make_item(LspContextItemKind::Reference, "b.rs", "ref1"),
            make_item(LspContextItemKind::Definition, "c.rs", "def1"),
        ];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();
        let summary = build_tui_summary(&packet, &registry);

        assert_eq!(summary.diagnostics_count, 2);
        assert_eq!(summary.references_count, 1);
        assert_eq!(summary.definitions_count, 1);
        assert_eq!(summary.total_items, 4);
    }

    #[test]
    fn summary_lists_freshness_counts() {
        let items = vec![
            make_item(LspContextItemKind::Diagnostic, "a.rs", "fresh"),
            {
                let mut i = make_item(LspContextItemKind::Diagnostic, "a.rs", "stale");
                i.provenance.freshness = LspEvidenceFreshness::Stale;
                i
            },
            {
                let mut i = make_item(LspContextItemKind::Diagnostic, "a.rs", "pos stale");
                i.provenance.freshness = LspEvidenceFreshness::PossiblyStale;
                i
            },
        ];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();
        let summary = build_tui_summary(&packet, &registry);

        assert_eq!(summary.fresh_count, 1);
        assert_eq!(summary.stale_freshness_count, 1);
        assert_eq!(summary.possibly_stale_count, 1);
        assert!(summary.stale);
    }

    #[test]
    fn summary_lists_preview_ids() {
        let items = vec![make_item(LspContextItemKind::Diagnostic, "a.rs", "err")];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let mut registry = PreviewArtifactRegistry::new();
        registry.register(
            LspPreviewArtifact::Formatting {
                description: "fmt 1".to_string(),
                content_hash: None,
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );
        registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 1,
            },
            vec!["b.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );
        let summary = build_tui_summary(&packet, &registry);

        assert_eq!(summary.preview_count, 2);
        assert_eq!(summary.preview_ids.len(), 2);
        for id in &summary.preview_ids {
            assert!(id.starts_with("preview-"));
        }
    }

    #[test]
    fn summary_lists_unsupported_operations() {
        let items = vec![
            {
                let mut i = make_item(LspContextItemKind::Reference, "a.rs", "ref");
                i.provenance.capability_decision =
                    Some("findReferences: implementation not supported".to_string());
                i
            },
            {
                let mut i = make_item(LspContextItemKind::Definition, "a.rs", "def");
                i.provenance.operation = "goToImplementation".to_string();
                i.provenance.capability_decision =
                    Some("goToImplementation: not supported by server".to_string());
                i
            },
        ];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();
        let summary = build_tui_summary(&packet, &registry);

        // Each unique operation+decision tag is captured.
        assert_eq!(summary.unsupported_operations.len(), 2);
        let detail = render_tui_summary_detail(&summary);
        assert!(detail.contains("Unsupported:"));
        assert!(detail.contains("goToImplementation"));
    }

    #[test]
    fn summary_handles_empty_context() {
        let packet = make_packet(Vec::new(), LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();
        let summary = build_tui_summary(&packet, &registry);
        let detail = render_tui_summary_detail(&summary);

        assert_eq!(summary.total_items, 0);
        assert_eq!(summary.diagnostics_count, 0);
        assert_eq!(summary.references_count, 0);
        assert_eq!(summary.definitions_count, 0);
        assert!(summary.preview_ids.is_empty());
        assert!(summary.unsupported_operations.is_empty());
        assert!(detail.contains("0 items"));
        assert!(detail.contains("0 fresh"));
        assert!(detail.contains("Notes: (none)"));
    }

    #[test]
    fn summary_stale_counts_in_detail() {
        let items = vec![
            {
                let mut i = make_item(LspContextItemKind::Diagnostic, "a.rs", "fresh");
                i.provenance.freshness = LspEvidenceFreshness::Fresh;
                i
            },
            {
                let mut i = make_item(LspContextItemKind::Diagnostic, "a.rs", "stale");
                i.provenance.freshness = LspEvidenceFreshness::Stale;
                i
            },
            {
                let mut i = make_item(LspContextItemKind::Diagnostic, "a.rs", "retained");
                i.provenance.freshness = LspEvidenceFreshness::RetainedAfterRestart;
                i
            },
            {
                let mut i = make_item(LspContextItemKind::Diagnostic, "a.rs", "pos stale");
                i.provenance.freshness = LspEvidenceFreshness::PossiblyStale;
                i
            },
            {
                let mut i = make_item(LspContextItemKind::Diagnostic, "a.rs", "stale after edit");
                i.provenance.freshness = LspEvidenceFreshness::StaleAfterEdit;
                i
            },
        ];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();
        let summary = build_tui_summary(&packet, &registry);
        let detail = render_tui_summary_detail(&summary);

        assert_eq!(summary.fresh_count, 1);
        // Stale + RetainedAfterRestart + ServerGenerationMismatch → stale_freshness_count
        assert_eq!(summary.stale_freshness_count, 2);
        // PossiblyStale + StaleAfterEdit → possibly_stale_count
        assert_eq!(summary.possibly_stale_count, 2);
        assert!(summary.stale);
        assert!(detail.contains("1 fresh, 2 stale, 2 possibly-stale"));
    }

    #[test]
    fn summary_preview_ids_appear_in_detail() {
        let items = vec![make_item(LspContextItemKind::Diagnostic, "a.rs", "err")];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let mut registry = PreviewArtifactRegistry::new();
        for i in 0..3 {
            registry.register(
                LspPreviewArtifact::Rename {
                    description: format!("rename {i}"),
                    edit_count: 1,
                },
                vec![format!("file{i}.rs")],
                std::collections::HashMap::new(),
                "rust-analyzer".to_string(),
            );
        }
        let summary = build_tui_summary(&packet, &registry);
        let detail = render_tui_summary_detail(&summary);

        assert_eq!(summary.preview_count, 3);
        assert_eq!(summary.preview_ids.len(), 3);
        assert!(detail.contains("Previews:"));
        assert!(detail.contains("3 pending"));
    }

    #[test]
    fn summary_unsupported_operations_appear_in_detail() {
        let items = vec![
            {
                let mut i = make_item(LspContextItemKind::Reference, "a.rs", "ref");
                i.provenance.operation = "findReferences".to_string();
                i.provenance.capability_decision = Some("implementation not supported".to_string());
                i
            },
            {
                let mut i = make_item(LspContextItemKind::Definition, "a.rs", "def");
                i.provenance.operation = "goToImplementation".to_string();
                i.provenance.capability_decision = Some("not supported by server".to_string());
                i
            },
        ];
        let packet = make_packet(items, LspContextPacketMode::Opportunistic);
        let registry = PreviewArtifactRegistry::new();
        let summary = build_tui_summary(&packet, &registry);
        let detail = render_tui_summary_detail(&summary);

        assert_eq!(summary.unsupported_operations.len(), 2);
        assert!(detail.contains("Unsupported:"));
        assert!(detail.contains("findReferences"));
        assert!(detail.contains("goToImplementation"));
    }

    #[test]
    fn test_render_status_line_no_packet() {
        let summary = LspTuiSummary {
            server_status: "ready".to_string(),
            server_id: Some("rust-analyzer".to_string()),
            server_generation: Some(3),
            diagnostics_count: 0,
            references_count: 0,
            definitions_count: 0,
            total_items: 0,
            truncated: false,
            stale: false,
            stale_freshness_count: 0,
            possibly_stale_count: 0,
            fresh_count: 0,
            preview_count: 2,
            preview_stale: false,
            preview_ids: vec!["preview-1".to_string()],
            unsupported_operations: Vec::new(),
            counts_from_packet: false,
            notes: Vec::new(),
            operational_notes: Vec::new(),
        };

        let line = render_tui_status_line(&summary);
        assert!(line.contains("LSP: ready"));
        assert!(line.contains("rust-analyzer gen=3"));
        assert!(line.contains("| — |"));
        assert!(line.contains("2p"));
        assert!(!line.contains("0d"));
        assert!(!line.contains("0r"));
        assert!(!line.contains("0def"));
    }

    #[test]
    fn test_render_detail_no_packet() {
        let summary = LspTuiSummary {
            server_status: "ready".to_string(),
            server_id: Some("rust-analyzer".to_string()),
            server_generation: Some(4),
            diagnostics_count: 0,
            references_count: 0,
            definitions_count: 0,
            total_items: 0,
            truncated: false,
            stale: false,
            stale_freshness_count: 0,
            possibly_stale_count: 0,
            fresh_count: 0,
            preview_count: 0,
            preview_stale: false,
            preview_ids: Vec::new(),
            unsupported_operations: Vec::new(),
            counts_from_packet: false,
            notes: Vec::new(),
            operational_notes: Vec::new(),
        };

        let detail = render_tui_summary_detail(&summary);
        assert!(detail.contains("Context: not collected in status snapshot"));
        assert!(!detail.contains("Freshness:"));
        assert!(!detail.contains("0 items"));
    }
}
