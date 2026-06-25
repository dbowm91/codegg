//! Regression tests for LSP Phase 6: polish, docs, and status accuracy.

use egglsp::context::{
    LspContextItem, LspContextItemKind, LspContextPacket, LspContextPacketMode, LspContextScore,
    LspContextTruncation, LspEvidenceFreshness, LspEvidenceProvenance, LspPreviewArtifact,
};
use egglsp::preview_registry::PreviewArtifactRegistry;
use egglsp::server::server_definitions;
use egglsp::tui_summary::{
    build_tui_summary, render_tui_status_line, render_tui_summary_detail, LspTuiSummary,
};

/// Verify the documented server count matches the actual count.
/// If this test fails, update architecture/lsp.md, AGENTS.md,
/// architecture/overview.md, and .opencode/skills/lsp/SKILL.md.
#[test]
fn server_definition_count_matches_docs() {
    let defs = server_definitions();
    // If you change this count, update the docs:
    // - architecture/lsp.md (Supported Languages section)
    // - AGENTS.md (LSP gotcha and architecture index)
    // - architecture/overview.md (Verified Counts table)
    // - .opencode/skills/lsp/SKILL.md (server count mentions)
    // - README.md (Features section, if it mentions a count)
    assert_eq!(
        defs.len(),
        39,
        "Server count changed! Update all docs that reference this count."
    );
}

/// Verify that build_tui_summary sets counts_from_packet to true.
#[test]
fn summary_from_packet_has_counts_flag() {
    let packet = LspContextPacket {
        request: egglsp::context::LspContextRequest::File {
            file: std::path::PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: false,
        },
        items: vec![],
        previews: vec![],
        preview_ids: vec![],
        mode: LspContextPacketMode::Opportunistic,
        workspace_root: None,
        generated_at: None,
        server_id: Some("test-server".to_string()),
        server_generation: Some(1),
        operational_state: None,
        budget: None,
        notes: vec![],
        truncation: LspContextTruncation {
            files_truncated: false,
            diagnostics_truncated: false,
            references_truncated: false,
            symbols_truncated: false,
            bytes_truncated: false,
            total_bytes: 0,
            max_bytes: 0,
            notes: vec![],
        },
    };
    let registry = PreviewArtifactRegistry::new();
    let summary = build_tui_summary(&packet, &registry);
    assert!(
        summary.counts_from_packet,
        "build_tui_summary should set counts_from_packet=true"
    );
}

/// Verify live-service summary (no packet) shows unavailable counts.
#[test]
fn live_service_summary_shows_unavailable_counts() {
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
        preview_count: 0,
        preview_stale: false,
        preview_ids: vec![],
        unsupported_operations: vec![],
        notes: vec![],
        operational_notes: vec![],
        counts_from_packet: false,
    };

    let status_line = render_tui_status_line(&summary);
    // Should NOT contain "0d" or "0r" or "0def" — should contain "—" instead
    assert!(
        !status_line.contains("0d"),
        "Status line should not show 0d: {status_line}"
    );
    assert!(
        !status_line.contains("0r"),
        "Status line should not show 0r: {status_line}"
    );
    assert!(
        status_line.contains("—"),
        "Status line should show — for uncollected counts: {status_line}"
    );

    let detail = render_tui_summary_detail(&summary);
    assert!(
        detail.contains("not collected"),
        "Detail should say 'not collected': {detail}"
    );
}

/// Verify ready server with previews renders correctly.
#[test]
fn summary_ready_with_previews() {
    let item = LspContextItem {
        kind: LspContextItemKind::Diagnostic,
        file: std::path::PathBuf::from("test.rs"),
        range: None,
        line: None,
        column: None,
        message: "test diagnostic".to_string(),
        symbol: None,
        source: None,
        provenance: LspEvidenceProvenance {
            server_id: "rust-analyzer".to_string(),
            server_generation: Some(5),
            operation: "textDocument/publishDiagnostics".to_string(),
            freshness: LspEvidenceFreshness::Fresh,
            capability_decision: None,
            document_version: None,
            age_ms: None,
            post_restart: false,
        },
        score: LspContextScore {
            priority: 1,
            is_hunk_local: false,
            is_error: true,
            is_same_file: false,
            freshness_rank: 0,
        },
        payload: None,
    };
    let packet = LspContextPacket {
        request: egglsp::context::LspContextRequest::File {
            file: std::path::PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: false,
        },
        items: vec![item],
        previews: vec![],
        preview_ids: vec!["preview-1".to_string(), "preview-2".to_string()],
        mode: LspContextPacketMode::Opportunistic,
        workspace_root: Some(std::path::PathBuf::from("/workspace")),
        generated_at: None,
        server_id: Some("rust-analyzer".to_string()),
        server_generation: Some(5),
        operational_state: None,
        budget: None,
        notes: vec![],
        truncation: LspContextTruncation {
            files_truncated: false,
            diagnostics_truncated: false,
            references_truncated: false,
            symbols_truncated: false,
            bytes_truncated: false,
            total_bytes: 0,
            max_bytes: 0,
            notes: vec![],
        },
    };
    let mut registry = PreviewArtifactRegistry::new();
    registry.register(
        LspPreviewArtifact::Rename {
            description: "rename foo to bar".to_string(),
            edit_count: 2,
        },
        vec![],
        std::collections::HashMap::new(),
        "test".to_string(),
    );
    registry.register(
        LspPreviewArtifact::Formatting {
            description: "format file".to_string(),
            content_hash: None,
        },
        vec![],
        std::collections::HashMap::new(),
        "test".to_string(),
    );
    let summary = build_tui_summary(&packet, &registry);

    let status_line = render_tui_status_line(&summary);
    assert!(
        status_line.contains("rust-analyzer"),
        "Should contain server id: {status_line}"
    );
    assert!(
        status_line.contains("gen=5"),
        "Should contain generation: {status_line}"
    );
    assert!(
        status_line.contains("2p"),
        "Should show 2 previews: {status_line}"
    );
}

/// Verify degraded/restarting state renders correctly.
#[test]
fn summary_degraded_state() {
    let summary = LspTuiSummary {
        server_status: "degraded".to_string(),
        server_id: Some("gopls".to_string()),
        server_generation: Some(2),
        diagnostics_count: 0,
        references_count: 0,
        definitions_count: 0,
        total_items: 0,
        truncated: false,
        stale: true,
        stale_freshness_count: 0,
        possibly_stale_count: 0,
        fresh_count: 0,
        preview_count: 0,
        preview_stale: false,
        preview_ids: vec![],
        unsupported_operations: vec!["typeHierarchy".to_string()],
        notes: vec!["LSP state: indexing".to_string()],
        operational_notes: vec![],
        counts_from_packet: false,
    };

    let status_line = render_tui_status_line(&summary);
    assert!(
        status_line.contains("degraded"),
        "Should show degraded: {status_line}"
    );

    let detail = render_tui_summary_detail(&summary);
    assert!(
        detail.contains("typeHierarchy"),
        "Should list unsupported ops: {detail}"
    );
    assert!(
        detail.contains("indexing"),
        "Should show operational note: {detail}"
    );
}
