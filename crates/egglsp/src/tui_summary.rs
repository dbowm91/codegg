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
    /// Whether any item was truncated by budget enforcement.
    pub truncated: bool,
    /// Whether any evidence item is stale.
    pub stale: bool,
    /// Number of registered preview artifacts.
    pub preview_count: usize,
    /// Whether any registered preview artifact has a stale base.
    pub preview_stale: bool,
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

    for item in &packet.items {
        match item.kind {
            LspContextItemKind::Diagnostic => diagnostics_count += 1,
            LspContextItemKind::Reference => references_count += 1,
            LspContextItemKind::Definition | LspContextItemKind::Declaration => {
                definitions_count += 1
            }
            _ => {}
        }

        if matches!(
            item.provenance.freshness,
            LspEvidenceFreshness::Stale | LspEvidenceFreshness::PossiblyStale
        ) {
            stale = true;
        }

        if server_id.is_none() && !item.provenance.server_id.is_empty() {
            server_id = Some(item.provenance.server_id.clone());
        }
        if server_generation.is_none() {
            server_generation = item.provenance.server_generation;
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
        truncated,
        stale,
        preview_count: registry.len(),
        preview_stale,
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

    let counts = format!(
        "{}d {}r {}def",
        summary.diagnostics_count, summary.references_count, summary.definitions_count
    );

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
pub fn render_tui_summary_detail(summary: &LspTuiSummary) -> String {
    let mut lines = Vec::new();

    lines.push(format!("LSP Status: {}", summary.server_status));

    let server_line = match (&summary.server_id, summary.server_generation) {
        (Some(id), Some(gen)) => format!("Server: {id} gen={gen}"),
        (Some(id), None) => format!("Server: {id}"),
        (None, Some(gen)) => format!("Server: gen={gen}"),
        (None, None) => "Server: unknown".to_string(),
    };
    lines.push(server_line);

    lines.push(format!(
        "Context: {} diagnostics, {} refs, {} definitions",
        summary.diagnostics_count, summary.references_count, summary.definitions_count
    ));

    if summary.preview_count > 0 {
        lines.push(format!(
            "Preview: {} pending (stale={})",
            summary.preview_count, summary.preview_stale
        ));
    }

    if summary.truncated {
        lines.push("Truncated: yes".to_string());
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

        assert!(detail.contains("LSP Status: ready"));
        assert!(detail.contains("Server: rust-analyzer gen=3"));
        assert!(detail.contains("1 diagnostics, 1 refs, 0 definitions"));
        assert!(detail.contains("Preview: 1 pending"));
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
}
