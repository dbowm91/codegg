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
// Phase 9: Lifecycle status DTOs
// ---------------------------------------------------------------------------

/// Detailed per-server status for lifecycle debugging.
/// Built from `LspOperationalHealthSnapshot` and capability snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LspServerStatusDetail {
    /// Client key (e.g. "/path/to/root:rust-analyzer").
    pub key: String,
    /// Server identifier (e.g. "rust-analyzer").
    pub server_id: String,
    /// Workspace root path.
    pub root: String,
    /// Operational state label.
    pub state: String,
    /// Operational state detail (e.g. reason for degraded/failed).
    pub state_detail: Option<String>,
    /// Server generation.
    pub generation: u64,
    /// Number of pending requests.
    pub pending_requests: usize,
    /// Number of open documents.
    pub open_documents: usize,
    /// Restart attempts.
    pub restart_attempts: u32,
    /// Last error message.
    pub last_error: Option<String>,
    /// Bounded stderr tail.
    pub stderr_tail: Vec<String>,
    /// Whether the server is usable.
    pub usable: bool,
    /// Effective capability snapshot (if initialized).
    pub capabilities: Option<ServerCapabilitySummary>,
}

/// Compact summary of effective capabilities for display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServerCapabilitySummary {
    pub supports_diagnostics: bool,
    pub supports_rename: bool,
    pub supports_code_actions: bool,
    pub supports_document_formatting: bool,
    pub supports_range_formatting: bool,
    pub supports_declaration: bool,
    pub supports_implementation: bool,
    pub supports_document_highlight: bool,
    pub supports_signature_help: bool,
    pub supports_inlay_hints: bool,
    pub supports_folding_ranges: bool,
    pub supports_selection_ranges: bool,
    pub supports_document_links: bool,
    pub supports_completion: bool,
    pub supports_hover: bool,
    pub supports_semantic_tokens: bool,
    pub supports_push_diagnostics: bool,
    pub supports_pull_diagnostics: bool,
}

impl From<&crate::capability::LspCapabilitySnapshot> for ServerCapabilitySummary {
    fn from(snap: &crate::capability::LspCapabilitySnapshot) -> Self {
        Self {
            supports_diagnostics: snap.supports_diagnostics,
            supports_rename: snap.supports_rename,
            supports_code_actions: snap.supports_code_actions,
            supports_document_formatting: snap.supports_document_formatting,
            supports_range_formatting: snap.supports_range_formatting,
            supports_declaration: snap.supports_declaration,
            supports_implementation: snap.supports_implementation,
            supports_document_highlight: snap.supports_document_highlight,
            supports_signature_help: snap.supports_signature_help,
            supports_inlay_hints: snap.supports_inlay_hints,
            supports_folding_ranges: snap.supports_folding_ranges,
            supports_selection_ranges: snap.supports_selection_ranges,
            supports_document_links: snap.supports_document_links,
            supports_completion: snap.supports_completion,
            supports_hover: snap.supports_hover,
            supports_semantic_tokens: snap.supports_semantic_tokens,
            supports_push_diagnostics: snap.supports_push_diagnostics,
            supports_pull_diagnostics: snap.supports_pull_diagnostics,
        }
    }
}

/// Render a compact one-line status for a single server.
///
/// Format: `"rust-analyzer [ready] gen=4 | root=/path | 2 pending, 5 open"`
pub fn render_server_status_line(detail: &LspServerStatusDetail) -> String {
    let mut line = format!(
        "{} [{}] gen={} | root={}",
        detail.server_id, detail.state, detail.generation, detail.root
    );
    line.push_str(&format!(
        " | {} pending, {} open",
        detail.pending_requests, detail.open_documents
    ));
    if detail.restart_attempts > 0 {
        line.push_str(&format!(", {} restarts", detail.restart_attempts));
    }
    if let Some(err) = &detail.last_error {
        let preview: String = err.chars().take(80).collect();
        line.push_str(&format!(", error: {preview}"));
    }
    line
}

/// Render detailed multi-line status for a single server.
pub fn render_server_detail(detail: &LspServerStatusDetail) -> String {
    let mut lines = Vec::new();
    lines.push(render_server_status_line(detail));

    if let Some(ref state_detail) = detail.state_detail {
        lines.push(format!("  State detail: {state_detail}"));
    }

    if let Some(ref caps) = detail.capabilities {
        let supported: Vec<&str> = [
            ("diagnostics", caps.supports_diagnostics),
            ("rename", caps.supports_rename),
            ("code_actions", caps.supports_code_actions),
            ("formatting", caps.supports_document_formatting),
            ("range_formatting", caps.supports_range_formatting),
            ("declaration", caps.supports_declaration),
            ("implementation", caps.supports_implementation),
            ("document_highlight", caps.supports_document_highlight),
            ("signature_help", caps.supports_signature_help),
            ("inlay_hints", caps.supports_inlay_hints),
            ("completion", caps.supports_completion),
            ("semantic_tokens", caps.supports_semantic_tokens),
        ]
        .iter()
        .filter(|(_, v)| *v)
        .map(|(name, _)| *name)
        .collect();
        if supported.is_empty() {
            lines.push("  Capabilities: none advertised".to_string());
        } else {
            lines.push(format!("  Capabilities: {}", supported.join(", ")));
        }
        let diag = if caps.supports_pull_diagnostics {
            "pull"
        } else if caps.supports_push_diagnostics {
            "push"
        } else {
            "none"
        };
        lines.push(format!("  Diagnostics mode: {diag}"));
    } else {
        lines.push("  Capabilities: not yet initialized".to_string());
    }

    if !detail.stderr_tail.is_empty() {
        let tail_preview: Vec<&String> = detail.stderr_tail.iter().rev().take(5).rev().collect();
        lines.push(format!("  Stderr (last {}):", tail_preview.len()));
        for line in &tail_preview {
            lines.push(format!("    {line}"));
        }
    }

    lines.join("\n")
}

/// Render a list of server status details.
pub fn render_servers_list(details: &[LspServerStatusDetail]) -> String {
    if details.is_empty() {
        return "No active LSP servers.".to_string();
    }
    let mut lines = Vec::new();
    lines.push(format!("Active LSP servers: {}", details.len()));
    lines.push(String::new());
    for detail in details {
        lines.push(render_server_status_line(detail));
    }
    lines.join("\n")
}

/// Render a capability snapshot for a single server.
pub fn render_capabilities(detail: &LspServerStatusDetail) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Capabilities for {} ({})",
        detail.server_id, detail.key
    ));
    if let Some(ref caps) = detail.capabilities {
        let all_caps = [
            ("diagnostics (push)", caps.supports_push_diagnostics),
            ("diagnostics (pull)", caps.supports_pull_diagnostics),
            ("completion", caps.supports_completion),
            ("hover", caps.supports_hover),
            ("rename", caps.supports_rename),
            ("code_actions", caps.supports_code_actions),
            ("document_formatting", caps.supports_document_formatting),
            ("range_formatting", caps.supports_range_formatting),
            ("declaration", caps.supports_declaration),
            ("implementation", caps.supports_implementation),
            ("document_highlight", caps.supports_document_highlight),
            ("signature_help", caps.supports_signature_help),
            ("inlay_hints", caps.supports_inlay_hints),
            ("folding_ranges", caps.supports_folding_ranges),
            ("selection_ranges", caps.supports_selection_ranges),
            ("document_links", caps.supports_document_links),
            ("semantic_tokens", caps.supports_semantic_tokens),
        ];
        for (name, supported) in &all_caps {
            let tag = if *supported { "yes" } else { "no" };
            lines.push(format!("  {name}: {tag}"));
        }
    } else {
        lines.push("  Server not yet initialized — capabilities unknown.".to_string());
    }
    lines.join("\n")
}

/// Render the error/health info for a single server.
pub fn render_server_errors(detail: &LspServerStatusDetail) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Health for {} ({})", detail.server_id, detail.key));
    lines.push(format!("  State: {}", detail.state));
    if let Some(ref state_detail) = detail.state_detail {
        lines.push(format!("  Detail: {state_detail}"));
    }
    if let Some(ref err) = detail.last_error {
        lines.push(format!("  Last error: {err}"));
    } else {
        lines.push("  Last error: (none)".to_string());
    }
    if detail.restart_attempts > 0 {
        lines.push(format!("  Restart attempts: {}", detail.restart_attempts));
    }
    if detail.stderr_tail.is_empty() {
        lines.push("  Stderr: (empty)".to_string());
    } else {
        let tail_preview: Vec<&String> = detail.stderr_tail.iter().rev().take(10).rev().collect();
        lines.push(format!("  Stderr tail ({} lines):", tail_preview.len()));
        for line in &tail_preview {
            lines.push(format!("    {line}"));
        }
    }
    lines.join("\n")
}

/// Build a `LspServerStatusDetail` from a health snapshot and optional capability snapshot.
pub fn build_server_status_detail(
    key: &str,
    snapshot: &crate::health::LspOperationalHealthSnapshot,
    caps: Option<&crate::capability::LspCapabilitySnapshot>,
) -> LspServerStatusDetail {
    let state_detail = match &snapshot.state {
        crate::health::LspOperationalState::Degraded { reason } => Some(reason.clone()),
        crate::health::LspOperationalState::Failed { reason } => Some(reason.clone()),
        crate::health::LspOperationalState::RestartScheduled { attempt, delay_ms } => {
            Some(format!("attempt {attempt}, delay {delay_ms}ms"))
        }
        crate::health::LspOperationalState::Restarting { attempt } => {
            Some(format!("attempt {attempt}"))
        }
        _ => None,
    };

    LspServerStatusDetail {
        key: key.to_string(),
        server_id: snapshot.server_id.clone(),
        root: snapshot.root.display().to_string(),
        state: snapshot.state.label().to_string(),
        state_detail,
        generation: snapshot.generation,
        pending_requests: snapshot.pending_requests,
        open_documents: snapshot.open_documents,
        restart_attempts: snapshot.restart_attempts,
        last_error: snapshot.last_error.clone(),
        stderr_tail: snapshot.stderr_tail.clone(),
        usable: snapshot.state.is_usable(),
        capabilities: caps.map(ServerCapabilitySummary::from),
    }
}

/// Root diagnosis result for a given file path.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RootDiagnosis {
    /// The input path that was diagnosed.
    pub input_path: String,
    /// Detected language.
    pub detected_language: Option<String>,
    /// Candidate root markers found walking up from the file.
    pub root_markers_found: Vec<String>,
    /// Selected project root.
    pub selected_root: Option<String>,
    /// Server profile that would be used.
    pub server_profile: Option<String>,
    /// Whether the file is inside the allowed root.
    pub inside_allowed_root: bool,
    /// Reasons why no LSP context is available (if any).
    pub issues: Vec<String>,
}

/// Render root diagnosis as human-readable text.
pub fn render_root_diagnosis(diag: &RootDiagnosis) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Root diagnosis for: {}", diag.input_path));

    if let Some(ref lang) = diag.detected_language {
        lines.push(format!("  Language: {lang}"));
    } else {
        lines.push("  Language: (unrecognized)".to_string());
    }

    if diag.root_markers_found.is_empty() {
        lines.push("  Root markers found: (none)".to_string());
    } else {
        lines.push(format!(
            "  Root markers found: {}",
            diag.root_markers_found.join(", ")
        ));
    }

    if let Some(ref root) = diag.selected_root {
        lines.push(format!("  Selected root: {root}"));
    } else {
        lines.push("  Selected root: (none)".to_string());
    }

    if let Some(ref profile) = diag.server_profile {
        lines.push(format!("  Server profile: {profile}"));
    } else {
        lines.push("  Server profile: (none available for this language)".to_string());
    }

    lines.push(format!(
        "  Inside allowed root: {}",
        if diag.inside_allowed_root {
            "yes"
        } else {
            "no"
        }
    ));

    if diag.issues.is_empty() {
        lines.push("  Issues: (none)".to_string());
    } else {
        for issue in &diag.issues {
            lines.push(format!("  Issue: {issue}"));
        }
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Preview list / detail helpers (Phase 8)
// ---------------------------------------------------------------------------

/// Compact one-line summary of a preview entry for list views.
///
/// Format: `preview-1-12345 rename | foo -> bar | 3 files, 5 edits | stale | 2m ago`
pub fn render_preview_list_entry(entry: &crate::preview_registry::PreviewArtifactEntry) -> String {
    let kind = crate::preview_registry::PreviewArtifactRegistry::preview_kind(entry);
    let title = crate::preview_registry::PreviewArtifactRegistry::preview_title(entry);
    let edit_count = crate::preview_registry::PreviewArtifactRegistry::preview_edit_count(entry);

    let file_count = entry.file_edits.len();
    let stale_tag = if entry.stale_base { " | STALE" } else { "" };
    let applied_tag = if entry.applied { " | applied" } else { "" };

    let age = format_age(entry.created_at);

    format!(
        "{} {} | {} | {} files, {} edits{}{} | {}",
        entry.id, kind, title, file_count, edit_count, stale_tag, applied_tag, age
    )
}

/// Multi-line detail view of a single preview entry.
///
/// Shows full metadata, affected files, hashes, and patch status.
pub fn render_preview_detail(entry: &crate::preview_registry::PreviewArtifactEntry) -> String {
    let mut lines = Vec::new();

    let kind = crate::preview_registry::PreviewArtifactRegistry::preview_kind(entry);
    let title = crate::preview_registry::PreviewArtifactRegistry::preview_title(entry);
    let edit_count = crate::preview_registry::PreviewArtifactRegistry::preview_edit_count(entry);

    lines.push(format!("Preview: {}", entry.id));
    lines.push(format!("Kind: {kind}"));
    lines.push(format!("Title: {title}"));
    lines.push(format!("Provenance: {}", entry.capability_provenance));
    lines.push(format!("Created: {}", format_age(entry.created_at)));
    lines.push(format!("Edit count: {edit_count}"));
    lines.push(format!("Affected files: {}", entry.file_edits.len()));

    if !entry.file_edits.is_empty() {
        for file in &entry.file_edits {
            let hash = entry
                .original_hashes
                .get(file)
                .map(|h| h.as_str())
                .unwrap_or("unknown");
            lines.push(format!("  {file} (hash: {hash})"));
        }
    }

    if entry.stale_base {
        lines.push("Status: STALE — base content has changed since creation.".to_string());
        if !entry.stale_files.is_empty() {
            for sf in &entry.stale_files {
                lines.push(format!(
                    "  {} expected={} actual={}",
                    sf.file, sf.expected_hash, sf.actual_hash
                ));
            }
        }
        lines.push(
            "To refresh: re-run the original LSP preview command to generate a new preview."
                .to_string(),
        );
    } else {
        lines.push("Status: FRESH".to_string());
    }

    if entry.applied {
        lines.push("Applied: yes".to_string());
    } else {
        lines.push("Applied: no — this preview has not been applied.".to_string());
    }

    lines.push(
        "To apply: use the separate mutating apply path (e.g. apply_patch) with user approval."
            .to_string(),
    );
    lines.push("Export does not mark as applied — it is a read-only handoff.".to_string());

    lines.join("\n")
}

/// Render a list of all preview entries (newest first).
pub fn render_preview_list(registry: &crate::preview_registry::PreviewArtifactRegistry) -> String {
    if registry.is_empty() {
        return "No preview artifacts registered.".to_string();
    }

    let entries = registry.recent(registry.len());
    let mut lines = Vec::new();
    lines.push(format!(
        "Preview artifacts: {} total, {} stale, {} applied",
        registry.len(),
        registry.stale_count(),
        registry.applied_count()
    ));
    lines.push(String::new());
    for entry in entries.iter().rev() {
        lines.push(render_preview_list_entry(entry));
    }
    lines.join("\n")
}

/// Apply candidate: read-only export of a preview for the mutating apply path.
///
/// This struct carries all the metadata a mutating apply tool needs to
/// apply a preview while preserving the read-only boundary of `LspTool`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviewApplyCandidate {
    /// Preview artifact ID.
    pub preview_id: String,
    /// Operation kind (rename, formatting, code_action).
    pub kind: String,
    /// Human-readable title.
    pub title: String,
    /// Affected file paths.
    pub affected_files: Vec<String>,
    /// Original file hashes by path.
    pub original_hashes: std::collections::HashMap<String, String>,
    /// Total edit count.
    pub edit_count: usize,
    /// Whether the base is stale (should warn/block apply).
    pub stale_base: bool,
    /// Provenance string.
    pub provenance: String,
    /// Whether already applied.
    pub applied: bool,
    /// Unified diff patches for each affected file.
    pub patches: Vec<crate::context::PreviewFilePatch>,
}

/// Export a preview entry as an [`PreviewApplyCandidate`] for the mutating
/// apply path. Returns `None` if the entry was not found.
///
/// This is a **read-only** operation: it reads from the registry without
/// calling `mark_applied` or any other mutating method. The caller is
/// responsible for the actual apply via a separate mutating path (e.g.
/// `apply_patch` with user approval).
pub fn export_preview_apply_candidate(
    registry: &crate::preview_registry::PreviewArtifactRegistry,
    preview_id: &str,
) -> Option<PreviewApplyCandidate> {
    let entry = registry.get(preview_id)?;

    let patches = match &entry.artifact {
        crate::context::LspPreviewArtifact::Rename { patches, .. }
        | crate::context::LspPreviewArtifact::Formatting { patches, .. }
        | crate::context::LspPreviewArtifact::CodeAction { patches, .. } => patches.clone(),
    };

    Some(PreviewApplyCandidate {
        preview_id: entry.id.clone(),
        kind: crate::preview_registry::PreviewArtifactRegistry::preview_kind(entry).to_string(),
        title: crate::preview_registry::PreviewArtifactRegistry::preview_title(entry).to_string(),
        affected_files: entry.file_edits.clone(),
        original_hashes: entry.original_hashes.clone(),
        edit_count: crate::preview_registry::PreviewArtifactRegistry::preview_edit_count(entry),
        stale_base: entry.stale_base,
        provenance: entry.capability_provenance.clone(),
        applied: entry.applied,
        patches,
    })
}

// ---------------------------------------------------------------------------
// Preview apply validation (read-only, testable boundary)
// ---------------------------------------------------------------------------

/// Errors that can occur while validating a preview for application.
///
/// All errors are non-fatal: callers should surface them as user-visible
/// messages and skip the apply. None of these errors mutates the registry
/// or the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewApplyError {
    /// The requested preview ID was not found in the registry.
    NotFound {
        /// The preview ID that was requested.
        preview_id: String,
    },
    /// The preview's base content has changed since it was created.
    Stale {
        /// Preview ID.
        preview_id: String,
        /// Files whose content diverged from the original hash, if known.
        stale_files: Vec<crate::preview_registry::StaleFileInfo>,
        /// Human-readable detail produced by the renderer.
        detail: String,
    },
    /// The preview has no patches to apply.
    NoPatches {
        /// Preview ID.
        preview_id: String,
    },
    /// The preview has already been applied.
    AlreadyApplied {
        /// Preview ID.
        preview_id: String,
    },
    /// Reading the file from disk failed.
    FileReadError {
        /// Affected file path.
        path: String,
        /// Underlying error message.
        error: String,
    },
    /// The current on-disk content does not match the preview's original hash.
    HashMismatch {
        /// Affected file path.
        path: String,
        /// Hash recorded when the preview was created.
        expected_hash: String,
        /// Current SHA-256 hex of the file.
        actual_hash: String,
    },
    /// The patch failed to apply (context or delete mismatch, etc.).
    PatchError {
        /// Affected file path.
        path: String,
        /// Underlying error message.
        error: String,
    },
}

impl std::fmt::Display for PreviewApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { preview_id } => {
                write!(f, "Preview not found: {preview_id}")
            }
            Self::Stale {
                preview_id, detail, ..
            } => {
                write!(
                    f,
                    "Preview {preview_id} is STALE — base content changed.\n{detail}"
                )
            }
            Self::NoPatches { preview_id } => {
                write!(f, "Preview {preview_id} has no patches to apply")
            }
            Self::AlreadyApplied { preview_id } => {
                write!(
                    f,
                    "Preview {preview_id} has already been applied; re-apply requires an explicit confirmation flow"
                )
            }
            Self::FileReadError { path, error } => {
                write!(f, "Failed to read {path}: {error}")
            }
            Self::HashMismatch {
                path,
                expected_hash,
                actual_hash,
            } => {
                write!(
                    f,
                    "{path} changed since preview was created (expected {expected_hash}, got {actual_hash})"
                )
            }
            Self::PatchError { path, error } => {
                write!(f, "Patch failed for {path}: {error}")
            }
        }
    }
}

impl std::error::Error for PreviewApplyError {}

/// A single per-file write plan produced by [`validate_preview_apply`].
///
/// The plan is **read-only**: it computes the new content in memory and
/// the caller is responsible for actually writing it to disk and for
/// marking the preview applied via [`PreviewArtifactRegistry::mark_applied`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewApplyFilePlan {
    /// Absolute or relative file path to write.
    pub path: String,
    /// SHA-256 hex of the original on-disk content (from the preview).
    pub original_hash: String,
    /// SHA-256 hex of the file's content as read during validation.
    pub validated_hash: String,
    /// New file content after applying the patch in memory.
    pub new_content: String,
}

/// A validated, non-mutating apply plan for a preview.
///
/// Contains everything the caller needs to perform the actual file
/// mutations: per-file paths, validated hashes, and the new content
/// computed in memory. The caller MUST NOT call
/// [`PreviewArtifactRegistry::mark_applied`] until after every file in
/// the plan has been written successfully.
#[derive(Debug, Clone)]
pub struct PreviewApplyPlan {
    /// Preview artifact ID.
    pub preview_id: String,
    /// Kind label (`rename` | `formatting` | `code_action`).
    pub kind: String,
    /// Human-readable title.
    pub title: String,
    /// Provenance string from the registry.
    pub provenance: String,
    /// Per-file write plan.
    pub files: Vec<PreviewApplyFilePlan>,
}

/// Validate a preview for application without mutating any state.
///
/// This function performs all gating checks and content recomputation in
/// memory, returning a typed plan that callers can use to drive the
/// actual file writes. It does **not** write to disk and does **not**
/// call `mark_applied`. The caller is responsible for:
///
/// 1. Writing each `file.new_content` to `file.path`.
/// 2. Calling `registry.mark_applied(plan.preview_id)` only after every
///    write has succeeded.
/// 3. Calling `registry.refresh_staleness(plan.preview_id)` after apply
///    to mark the entry as fresh relative to the new file content.
///
/// # Gating rules
///
/// 1. Missing preview ID → `PreviewApplyError::NotFound`.
/// 2. Stale base content (file hashes diverge) → `PreviewApplyError::Stale`.
/// 3. No patches → `PreviewApplyError::NoPatches`.
/// 4. Already applied → `PreviewApplyError::AlreadyApplied` (caller must
///    bypass via an explicit reapply flow).
/// 5. Per-file: read failure, hash mismatch, or patch failure yields a
///    typed error and aborts the plan.
///
/// # `patch_applier`
///
/// The patch applier is injected so this function can be tested without
/// depending on `patch_util` (which lives in the root crate). The
/// production caller passes `patch_util::apply_unified_diff`.
///
/// `patch_util::apply_unified_diff(original, patch) -> Result<String, String>`
pub fn validate_preview_apply(
    registry: &crate::preview_registry::PreviewArtifactRegistry,
    preview_id: &str,
    patch_applier: &dyn Fn(&str, &str) -> Result<String, String>,
) -> Result<PreviewApplyPlan, PreviewApplyError> {
    let entry = registry
        .get(preview_id)
        .ok_or_else(|| PreviewApplyError::NotFound {
            preview_id: preview_id.to_string(),
        })?;

    if entry.stale_base {
        let detail = render_preview_detail(entry);
        let stale_files = entry.stale_files.clone();
        return Err(PreviewApplyError::Stale {
            preview_id: preview_id.to_string(),
            stale_files,
            detail,
        });
    }

    if entry.applied {
        return Err(PreviewApplyError::AlreadyApplied {
            preview_id: preview_id.to_string(),
        });
    }

    let patches = match &entry.artifact {
        crate::context::LspPreviewArtifact::Rename { patches, .. }
        | crate::context::LspPreviewArtifact::Formatting { patches, .. }
        | crate::context::LspPreviewArtifact::CodeAction { patches, .. } => patches.clone(),
    };

    if patches.is_empty() {
        return Err(PreviewApplyError::NoPatches {
            preview_id: preview_id.to_string(),
        });
    }

    let mut files = Vec::with_capacity(patches.len());
    for p in &patches {
        let file_content =
            std::fs::read_to_string(&p.path).map_err(|e| PreviewApplyError::FileReadError {
                path: p.path.clone(),
                error: e.to_string(),
            })?;

        let actual_hash = {
            use sha2::{Digest, Sha256};
            let digest = Sha256::digest(file_content.as_bytes());
            format!("{digest:x}")
        };
        if actual_hash != p.original_hash {
            return Err(PreviewApplyError::HashMismatch {
                path: p.path.clone(),
                expected_hash: p.original_hash.clone(),
                actual_hash,
            });
        }

        let new_content =
            patch_applier(&file_content, &p.patch).map_err(|e| PreviewApplyError::PatchError {
                path: p.path.clone(),
                error: e,
            })?;

        files.push(PreviewApplyFilePlan {
            path: p.path.clone(),
            original_hash: p.original_hash.clone(),
            validated_hash: actual_hash,
            new_content,
        });
    }

    let kind = crate::preview_registry::PreviewArtifactRegistry::preview_kind(entry).to_string();
    let title = crate::preview_registry::PreviewArtifactRegistry::preview_title(entry).to_string();

    Ok(PreviewApplyPlan {
        preview_id: preview_id.to_string(),
        kind,
        title,
        provenance: entry.capability_provenance.clone(),
        files,
    })
}

// ---------------------------------------------------------------------------
// Preview apply write-side (atomic-ish with hash recheck)
// ---------------------------------------------------------------------------

/// Error type for the write-side of preview application.
#[derive(Debug)]
pub enum PreviewApplyWriteError {
    /// File changed between validation and write (hash mismatch).
    StaleDuringWrite {
        /// Affected file path.
        path: String,
        /// Hash at validation time.
        expected_hash: String,
        /// Current on-disk hash at write time.
        actual_hash: String,
    },
    /// File could not be written (or re-read for hash check).
    WriteFailed {
        /// Affected file path.
        path: String,
        /// Underlying error message.
        error: String,
        /// Files that were successfully written before this failure.
        completed: Vec<String>,
    },
}

impl std::fmt::Display for PreviewApplyWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleDuringWrite {
                path,
                expected_hash,
                actual_hash,
            } => write!(
                f,
                "{path} changed since validation (expected {expected_hash}, got {actual_hash})"
            ),
            Self::WriteFailed {
                path,
                error,
                completed,
            } => write!(
                f,
                "write failed for {path}: {error} ({}/{} files completed)",
                completed.len(),
                completed.len() + 1
            ),
        }
    }
}

impl std::error::Error for PreviewApplyWriteError {}

/// Report of a preview-apply write attempt.
#[derive(Debug)]
pub struct PreviewApplyWriteReport {
    /// Files successfully written (absolute or relative paths as in the plan).
    pub written: Vec<String>,
    /// Whether all writes succeeded.
    pub all_succeeded: bool,
}

/// Write a validated preview-apply plan with hash rechecks.
///
/// Before writing each file, re-reads the file and confirms its
/// SHA-256 equals `file.validated_hash`. If the hash has changed
/// since validation, returns a [`PreviewApplyWriteError::StaleDuringWrite`]
/// error without writing that file.
///
/// Returns a [`PreviewApplyWriteReport`] indicating which files were
/// written and whether all writes succeeded. The caller **MUST NOT**
/// call `mark_applied` unless `all_succeeded` is true.
pub fn write_preview_apply_plan_atomically_enough(
    plan: &PreviewApplyPlan,
) -> Result<PreviewApplyWriteReport, PreviewApplyWriteError> {
    let mut written = Vec::new();
    for file in &plan.files {
        // Recheck hash before write.
        let current_content = std::fs::read_to_string(&file.path).map_err(|e| {
            PreviewApplyWriteError::WriteFailed {
                path: file.path.clone(),
                error: format!("read error: {e}"),
                completed: written.clone(),
            }
        })?;
        let current_hash = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(current_content.as_bytes()))
        };
        if current_hash != file.validated_hash {
            return Err(PreviewApplyWriteError::StaleDuringWrite {
                path: file.path.clone(),
                expected_hash: file.validated_hash.clone(),
                actual_hash: current_hash,
            });
        }
        // Write the new content.
        std::fs::write(&file.path, &file.new_content).map_err(|e| {
            PreviewApplyWriteError::WriteFailed {
                path: file.path.clone(),
                error: format!("write error: {e}"),
                completed: written.clone(),
            }
        })?;
        written.push(file.path.clone());
    }
    Ok(PreviewApplyWriteReport {
        written,
        all_succeeded: true,
    })
}

/// Format a human-readable age from a millisecond timestamp.
fn format_age(created_at: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let age_ms = now.saturating_sub(created_at);
    let age_secs = age_ms / 1000;

    if age_secs < 60 {
        format!("{age_secs}s ago")
    } else if age_secs < 3600 {
        format!("{}m ago", age_secs / 60)
    } else if age_secs < 86400 {
        format!("{}h ago", age_secs / 3600)
    } else {
        format!("{}d ago", age_secs / 86400)
    }
}

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
                patches: Vec::new(),
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
                edit_count: 0,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );
        registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 1,
                patches: Vec::new(),
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
                    patches: Vec::new(),
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

    // -----------------------------------------------------------------------
    // Phase 8: preview list / detail / apply-candidate tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_preview_list_empty_registry() {
        let registry = PreviewArtifactRegistry::new();
        let output = render_preview_list(&registry);
        assert!(output.contains("No preview artifacts"));
    }

    #[test]
    fn test_render_preview_list_with_entries() {
        let mut registry = PreviewArtifactRegistry::new();
        registry.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 3,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string(), "b.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );
        registry.register(
            LspPreviewArtifact::Formatting {
                description: "format c.rs".to_string(),
                content_hash: None,
                edit_count: 0,
                patches: Vec::new(),
            },
            vec!["c.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );

        let output = render_preview_list(&registry);
        assert!(output.contains("2 total"));
        assert!(output.contains("rename"));
        assert!(output.contains("formatting"));
        assert!(output.contains("foo -> bar"));
        assert!(output.contains("format c.rs"));
    }

    #[test]
    fn test_render_preview_detail_fresh_entry() {
        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 5,
                patches: Vec::new(),
            },
            vec!["src/main.rs".to_string()],
            std::collections::HashMap::from([("src/main.rs".to_string(), "abc123".to_string())]),
            "rust-analyzer".to_string(),
        );

        let entry = registry.get(&id).unwrap();
        let detail = render_preview_detail(entry);
        assert!(detail.contains(&id));
        assert!(detail.contains("Kind: rename"));
        assert!(detail.contains("Title: foo -> bar"));
        assert!(detail.contains("Edit count: 5"));
        assert!(detail.contains("src/main.rs"));
        assert!(detail.contains("abc123"));
        assert!(detail.contains("Status: FRESH"));
        assert!(detail.contains("not been applied"));
        assert!(!detail.contains("STALE"));
    }

    #[test]
    fn test_render_preview_detail_stale_entry() {
        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 2,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );
        registry.mark_stale(&id);

        let entry = registry.get(&id).unwrap();
        let detail = render_preview_detail(entry);
        assert!(detail.contains("STALE"));
        assert!(detail.contains("base content has changed"));
    }

    #[test]
    fn test_render_preview_list_entry_line() {
        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::CodeAction {
                description: "organize imports".to_string(),
                kind: Some("source.organizeImports".to_string()),
                edit_count: 0,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );

        let entry = registry.get(&id).unwrap();
        let line = render_preview_list_entry(entry);
        assert!(line.contains(&id));
        assert!(line.contains("code_action"));
        assert!(line.contains("organize imports"));
        assert!(line.contains("1 files"));
        assert!(line.contains("ago"));
    }

    #[test]
    fn test_export_preview_apply_candidate_fresh() {
        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 3,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::from([("a.rs".to_string(), "hash1".to_string())]),
            "rust-analyzer".to_string(),
        );

        let candidate = export_preview_apply_candidate(&registry, &id).unwrap();
        assert_eq!(candidate.preview_id, id);
        assert_eq!(candidate.kind, "rename");
        assert!(!candidate.stale_base);
        assert!(!candidate.applied);
        assert_eq!(candidate.affected_files, vec!["a.rs"]);
        assert_eq!(candidate.edit_count, 3);
    }

    #[test]
    fn test_export_preview_apply_candidate_stale() {
        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Formatting {
                description: "format a.rs".to_string(),
                content_hash: None,
                edit_count: 0,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );
        registry.mark_stale(&id);

        let candidate = export_preview_apply_candidate(&registry, &id).unwrap();
        assert!(candidate.stale_base);
    }

    #[test]
    fn test_export_preview_apply_candidate_not_found() {
        let registry = PreviewArtifactRegistry::new();
        assert!(export_preview_apply_candidate(&registry, "nonexistent").is_none());
    }

    #[test]
    fn test_render_preview_list_shows_stale() {
        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 1,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );
        registry.mark_stale(&id);

        let output = render_preview_list(&registry);
        assert!(output.contains("1 total"));
        assert!(output.contains("1 stale"));
        assert!(output.contains("STALE"));
    }

    #[test]
    fn test_export_preview_apply_candidate_includes_patches() {
        use crate::context::PreviewFilePatch;

        let mut registry = PreviewArtifactRegistry::new();
        let patches = vec![PreviewFilePatch {
            path: "/tmp/a.rs".to_string(),
            patch: "@@ -1,3 +1,3 @@\n-old\n+new\n".to_string(),
            original_hash: "abc123".to_string(),
        }];
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 1,
                patches,
            },
            vec!["/tmp/a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );

        let candidate = export_preview_apply_candidate(&registry, &id).unwrap();
        assert_eq!(candidate.patches.len(), 1);
        assert_eq!(candidate.patches[0].path, "/tmp/a.rs");
        assert!(candidate.patches[0].patch.contains("+new"));
        assert_eq!(candidate.patches[0].original_hash, "abc123");
    }

    #[test]
    fn test_export_preview_apply_candidate_empty_patches() {
        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Formatting {
                description: "format a.rs".to_string(),
                content_hash: None,
                edit_count: 0,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );

        let candidate = export_preview_apply_candidate(&registry, &id).unwrap();
        assert!(candidate.patches.is_empty());
    }

    #[test]
    fn test_export_preview_apply_candidate_does_not_mark_applied() {
        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "foo -> bar".to_string(),
                edit_count: 1,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );

        let _candidate = export_preview_apply_candidate(&registry, &id).unwrap();
        // Export must not mark the entry as applied.
        assert!(!registry.get(&id).unwrap().applied);
    }

    // -----------------------------------------------------------------------
    // Phase 9: Lifecycle status DTO tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_server_status_line_ready() {
        use crate::health::{LspOperationalHealthSnapshot, LspOperationalState};
        use std::path::PathBuf;

        let snap = LspOperationalHealthSnapshot::from_operational_state(
            "rust-analyzer".to_string(),
            PathBuf::from("/tmp/project"),
            4,
            LspOperationalState::Ready,
            None,
            0,
            5,
            Some(10),
            Some(5),
            0,
            None,
            Vec::new(),
        );
        let detail = build_server_status_detail("/tmp/project:rust-analyzer", &snap, None);
        let line = render_server_status_line(&detail);
        assert!(line.contains("rust-analyzer"));
        assert!(line.contains("[ready]"));
        assert!(line.contains("gen=4"));
        assert!(line.contains("root=/tmp/project"));
    }

    #[test]
    fn test_render_server_detail_with_caps() {
        use crate::capability::LspCapabilitySnapshot;
        use crate::health::{LspOperationalHealthSnapshot, LspOperationalState};
        use std::path::PathBuf;

        let snap = LspOperationalHealthSnapshot::from_operational_state(
            "rust-analyzer".to_string(),
            PathBuf::from("/tmp"),
            1,
            LspOperationalState::Ready,
            None,
            0,
            0,
            None,
            None,
            0,
            None,
            Vec::new(),
        );
        let caps = LspCapabilitySnapshot::default();
        let detail = build_server_status_detail("/tmp:rust-analyzer", &snap, Some(&caps));
        let rendered = render_server_detail(&detail);
        assert!(rendered.contains("rust-analyzer"));
        assert!(rendered.contains("Capabilities:"));
    }

    #[test]
    fn test_render_server_errors_degraded() {
        use crate::health::{LspOperationalHealthSnapshot, LspOperationalState};
        use std::path::PathBuf;

        let snap = LspOperationalHealthSnapshot::from_operational_state(
            "pyright".to_string(),
            PathBuf::from("/tmp"),
            2,
            LspOperationalState::Degraded {
                reason: "diagnostics wait timed out".to_string(),
            },
            None,
            0,
            0,
            None,
            None,
            1,
            Some("timeout".to_string()),
            vec!["stderr line".to_string()],
        );
        let detail = build_server_status_detail("/tmp:pyright", &snap, None);
        let rendered = render_server_errors(&detail);
        assert!(rendered.contains("degraded"));
        assert!(rendered.contains("diagnostics wait timed out"));
        assert!(rendered.contains("timeout"));
    }

    #[test]
    fn test_render_servers_list_empty() {
        let rendered = render_servers_list(&[]);
        assert!(rendered.contains("No active LSP servers"));
    }

    #[test]
    fn test_render_root_diagnosis_no_root() {
        let diag = RootDiagnosis {
            input_path: "/tmp/src/main.py".to_string(),
            detected_language: Some("python".to_string()),
            root_markers_found: Vec::new(),
            selected_root: None,
            server_profile: None,
            inside_allowed_root: true,
            issues: vec!["No project root marker found".to_string()],
        };
        let rendered = render_root_diagnosis(&diag);
        assert!(rendered.contains("python"));
        assert!(rendered.contains("(none)"));
        assert!(rendered.contains("No project root marker found"));
    }

    #[test]
    fn test_render_root_diagnosis_with_root() {
        let diag = RootDiagnosis {
            input_path: "/tmp/project/src/main.rs".to_string(),
            detected_language: Some("rust".to_string()),
            root_markers_found: vec!["Cargo.toml".to_string(), ".git".to_string()],
            selected_root: Some("/tmp/project".to_string()),
            server_profile: Some("rust-analyzer".to_string()),
            inside_allowed_root: true,
            issues: Vec::new(),
        };
        let rendered = render_root_diagnosis(&diag);
        assert!(rendered.contains("rust"));
        assert!(rendered.contains("Cargo.toml"));
        assert!(rendered.contains("/tmp/project"));
        assert!(rendered.contains("rust-analyzer"));
        assert!(rendered.contains("(none)"));
    }

    #[test]
    fn test_server_capability_summary_from_snapshot() {
        use crate::capability::LspCapabilitySnapshot;
        let snap = LspCapabilitySnapshot::default();
        let summary = ServerCapabilitySummary::from(&snap);
        assert!(!summary.supports_rename);
        assert!(!summary.supports_code_actions);
    }

    // -----------------------------------------------------------------------
    // Phase 9 hardening: lifecycle/root/status correctness tests
    // (Workstream 3 of plans/lsp_phase_9_12_hardening_plan.md)
    //
    // Covers degraded/edge-state rendering for server status, capability
    // snapshots, error/health output, and root diagnosis. The pre-existing
    // tests cover only the happy path; these tests prove that the
    // renderers do not overclaim capabilities and correctly surface
    // non-ready operational states.
    // -----------------------------------------------------------------------

    use crate::capability::LspCapabilitySnapshot;
    use crate::health::{LspOperationalHealthSnapshot, LspOperationalState};

    /// Build an `LspServerStatusDetail` directly from scratch fields, so the
    /// tests can exercise every combination without depending on
    /// `build_server_status_detail`'s field-mapping logic.
    #[allow(clippy::too_many_arguments)]
    fn detail_from_fields(
        key: &str,
        server_id: &str,
        root: &str,
        state: LspOperationalState,
        state_detail: Option<String>,
        generation: u64,
        pending_requests: usize,
        open_documents: usize,
        restart_attempts: u32,
        last_error: Option<String>,
        stderr_tail: Vec<String>,
        caps: Option<&LspCapabilitySnapshot>,
    ) -> LspServerStatusDetail {
        let snapshot = LspOperationalHealthSnapshot::from_operational_state(
            server_id.to_string(),
            std::path::PathBuf::from(root),
            generation,
            state.clone(),
            None,
            pending_requests,
            open_documents,
            None,
            None,
            restart_attempts,
            last_error,
            stderr_tail,
        );
        let usable = state.is_usable();
        let label = state.label().to_string();
        LspServerStatusDetail {
            key: key.to_string(),
            server_id: server_id.to_string(),
            root: root.to_string(),
            state: label,
            state_detail,
            generation,
            pending_requests,
            open_documents,
            restart_attempts,
            last_error: snapshot.last_error,
            stderr_tail: snapshot.stderr_tail,
            usable,
            capabilities: caps.map(ServerCapabilitySummary::from),
        }
    }

    #[test]
    fn render_server_status_line_indexing_state() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Indexing,
            None,
            1,
            0,
            0,
            0,
            None,
            Vec::new(),
            None,
        );
        let line = render_server_status_line(&detail);
        assert!(
            line.contains("[indexing]"),
            "status line must label indexing state: {line}"
        );
        assert!(
            line.contains("gen=1"),
            "status line must show generation: {line}"
        );
        assert!(
            !line.contains("restarts"),
            "indexing with zero restart_attempts must not show restart suffix: {line}"
        );
        assert!(LspOperationalState::Indexing.is_usable());
    }

    #[test]
    fn render_server_status_line_degraded_state() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:pyright",
            "pyright",
            "/proj",
            LspOperationalState::Degraded {
                reason: "diagnostics wait timed out".to_string(),
            },
            Some("diagnostics wait timed out".to_string()),
            3,
            1,
            4,
            0,
            None,
            Vec::new(),
            None,
        );
        let line = render_server_status_line(&detail);
        assert!(line.contains("[degraded]"), "got: {line}");
        assert!(line.contains("gen=3"), "got: {line}");
        assert!(line.contains("pyright"), "got: {line}");
        assert!(line.contains("1 pending, 4 open"), "got: {line}");
    }

    #[test]
    fn render_server_status_line_failed_state() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:gopls",
            "gopls",
            "/proj",
            LspOperationalState::Failed {
                reason: "exited unexpectedly".to_string(),
            },
            Some("exited unexpectedly".to_string()),
            5,
            0,
            0,
            0,
            Some("exited unexpectedly".to_string()),
            Vec::new(),
            None,
        );
        let line = render_server_status_line(&detail);
        assert!(line.contains("[failed]"), "got: {line}");
        assert!(line.contains("gen=5"), "got: {line}");
        assert!(
            line.contains("error: exited unexpectedly"),
            "failed state with last_error must surface error preview: {line}"
        );
        assert!(!detail.usable, "failed servers must not be marked usable");
    }

    #[test]
    fn render_server_status_line_restarting_state_visible() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Restarting { attempt: 2 },
            Some("attempt 2".to_string()),
            6,
            0,
            0,
            2,
            None,
            Vec::new(),
            None,
        );
        let line = render_server_status_line(&detail);
        assert!(line.contains("[restarting]"), "got: {line}");
        assert!(
            line.contains("gen=6"),
            "restart state must show the new generation: {line}"
        );
        assert!(
            line.contains("2 restarts"),
            "restart_attempts > 0 must surface as ', N restarts': {line}"
        );

        let detail_lines = render_server_detail(&detail);
        assert!(
            detail_lines.contains("State detail: attempt 2"),
            "restarting detail must include attempt info: {detail_lines}"
        );
    }

    #[test]
    fn render_servers_list_with_indexing_server() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Indexing,
            None,
            1,
            0,
            0,
            0,
            None,
            Vec::new(),
            None,
        );
        let rendered = render_servers_list(&[detail]);
        assert!(rendered.contains("Active LSP servers: 1"));
        assert!(rendered.contains("[indexing]"));
        assert!(rendered.contains("rust-analyzer"));
    }

    #[test]
    fn render_servers_list_with_degraded_server() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:pyright",
            "pyright",
            "/proj",
            LspOperationalState::Degraded {
                reason: "diagnostics wait timed out".to_string(),
            },
            Some("diagnostics wait timed out".to_string()),
            2,
            0,
            0,
            0,
            Some("timeout".to_string()),
            vec!["stderr line".to_string()],
            None,
        );
        let rendered = render_servers_list(&[detail]);
        assert!(rendered.contains("Active LSP servers: 1"));
        assert!(rendered.contains("[degraded]"));
        assert!(rendered.contains("pyright"));
        assert!(rendered.contains("error: timeout"));
    }

    #[test]
    fn render_servers_list_with_failed_server() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:gopls",
            "gopls",
            "/proj",
            LspOperationalState::Failed {
                reason: "exited".to_string(),
            },
            Some("exited".to_string()),
            4,
            0,
            0,
            0,
            Some("exited".to_string()),
            Vec::new(),
            None,
        );
        let rendered = render_servers_list(&[detail]);
        assert!(rendered.contains("[failed]"));
        assert!(rendered.contains("gen=4"));
        assert!(rendered.contains("error: exited"));
    }

    #[test]
    fn render_servers_list_with_restarting_server() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Restarting { attempt: 1 },
            Some("attempt 1".to_string()),
            3,
            0,
            0,
            1,
            None,
            Vec::new(),
            None,
        );
        let rendered = render_servers_list(&[detail]);
        assert!(rendered.contains("[restarting]"));
        assert!(rendered.contains("gen=3"));
        assert!(rendered.contains("1 restarts"));
    }

    #[test]
    fn render_servers_list_with_multiple_servers() {
        use crate::health::LspOperationalState;

        let ready = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Ready,
            None,
            4,
            0,
            5,
            0,
            None,
            Vec::new(),
            None,
        );
        let degraded = detail_from_fields(
            "/proj:pyright",
            "pyright",
            "/proj",
            LspOperationalState::Degraded {
                reason: "slow".to_string(),
            },
            Some("slow".to_string()),
            2,
            0,
            0,
            0,
            None,
            Vec::new(),
            None,
        );
        let rendered = render_servers_list(&[ready, degraded]);
        assert!(rendered.contains("Active LSP servers: 2"));
        assert!(rendered.contains("[ready]"));
        assert!(rendered.contains("[degraded]"));
        // Both server ids must be present.
        assert!(rendered.contains("rust-analyzer"));
        assert!(rendered.contains("pyright"));
    }

    #[test]
    fn render_capabilities_uninitialized_snapshot() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Initializing,
            None,
            0,
            0,
            0,
            0,
            None,
            Vec::new(),
            None,
        );
        let rendered = render_capabilities(&detail);
        assert!(
            rendered.contains("Server not yet initialized"),
            "uninitialized snapshot must not enumerate capabilities: {rendered}"
        );
        assert!(
            !rendered.contains("hover:"),
            "uninitialized snapshot must not advertise hover: {rendered}"
        );
        assert!(
            !rendered.contains("rename:"),
            "uninitialized snapshot must not advertise rename: {rendered}"
        );
    }

    #[test]
    fn render_capabilities_default_snapshot_does_not_overclaim() {
        use crate::capability::LspCapabilitySnapshot;
        use crate::health::LspOperationalState;

        // Default snapshot has every bool false (including hover).
        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Ready,
            None,
            1,
            0,
            0,
            0,
            None,
            Vec::new(),
            Some(&LspCapabilitySnapshot::default()),
        );
        let rendered = render_capabilities(&detail);
        // Every capability line must read "no" for a default snapshot,
        // and critically must NOT overclaim hover support. We restrict
        // the assertion to indented lines (the body uses "  {name}: {tag}").
        for line in rendered.lines() {
            let trimmed = line.trim_start();
            if let Some((name, tag)) = trimmed.split_once(':') {
                // Skip the header line ("Capabilities for X (Y)").
                if name.starts_with("Capabilities for ") {
                    continue;
                }
                let tag = tag.trim();
                assert!(
                    tag == "yes" || tag == "no",
                    "unexpected tag in capability line {line:?}"
                );
                assert_eq!(
                    tag, "no",
                    "{name} must not be advertised as supported for a default snapshot: {line}"
                );
            }
        }
        assert!(
            rendered.contains("hover: no"),
            "hover must NOT be overclaimed when snapshot.supports_hover is false: {rendered}"
        );
        assert!(rendered.contains("completion: no"));
        assert!(rendered.contains("rename: no"));
        assert!(rendered.contains("diagnostics (push): no"));
        assert!(rendered.contains("diagnostics (pull): no"));
    }

    #[test]
    fn render_capabilities_advertises_hover_when_supported() {
        use crate::capability::LspCapabilitySnapshot;
        use crate::health::LspOperationalState;

        let snap = LspCapabilitySnapshot {
            supports_hover: true,
            supports_rename: true,
            supports_completion: true,
            ..Default::default()
        };
        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Ready,
            None,
            1,
            0,
            0,
            0,
            None,
            Vec::new(),
            Some(&snap),
        );
        let rendered = render_capabilities(&detail);
        assert!(
            rendered.contains("hover: yes"),
            "hover must be advertised when snapshot supports it: {rendered}"
        );
        assert!(rendered.contains("rename: yes"));
        assert!(rendered.contains("completion: yes"));
        assert!(
            rendered.contains("signature_help: no"),
            "unset capability must remain no: {rendered}"
        );
    }

    #[test]
    fn server_capability_summary_propagates_hover_field() {
        use crate::capability::LspCapabilitySnapshot;

        let snap_on = LspCapabilitySnapshot {
            supports_hover: true,
            ..Default::default()
        };
        let summary_on = ServerCapabilitySummary::from(&snap_on);
        assert!(summary_on.supports_hover);

        let snap_off = LspCapabilitySnapshot {
            supports_hover: false,
            ..Default::default()
        };
        let summary_off = ServerCapabilitySummary::from(&snap_off);
        assert!(!summary_off.supports_hover);
    }

    #[test]
    fn render_server_errors_without_stderr_tail() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Ready,
            None,
            1,
            0,
            0,
            0,
            None,
            Vec::new(),
            None,
        );
        let rendered = render_server_errors(&detail);
        assert!(
            rendered.contains("Stderr: (empty)"),
            "empty stderr tail must show '(empty)' placeholder: {rendered}"
        );
        assert!(
            rendered.contains("Last error: (none)"),
            "missing last_error must show '(none)' placeholder: {rendered}"
        );
        assert!(
            !rendered.contains("Restart attempts"),
            "zero restart_attempts must not emit a Restart attempts line: {rendered}"
        );
    }

    #[test]
    fn render_server_errors_with_stderr_tail() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:pyright",
            "pyright",
            "/proj",
            LspOperationalState::Degraded {
                reason: "timeout".to_string(),
            },
            Some("timeout".to_string()),
            2,
            0,
            0,
            1,
            Some("timeout".to_string()),
            vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string(),
            ],
            None,
        );
        let rendered = render_server_errors(&detail);
        assert!(rendered.contains("State: degraded"));
        assert!(rendered.contains("Detail: timeout"));
        assert!(rendered.contains("Last error: timeout"));
        assert!(rendered.contains("Restart attempts: 1"));
        assert!(
            rendered.contains("Stderr tail (3 lines)"),
            "stderr tail must include the line count: {rendered}"
        );
        assert!(rendered.contains("first"));
        assert!(rendered.contains("second"));
        assert!(rendered.contains("third"));
    }

    #[test]
    fn render_server_errors_stderr_tail_is_bounded_to_ten_lines() {
        use crate::health::LspOperationalState;

        let tail: Vec<String> = (0..25).map(|i| format!("line {i}")).collect();
        let detail = detail_from_fields(
            "/proj:gopls",
            "gopls",
            "/proj",
            LspOperationalState::Failed {
                reason: "boom".to_string(),
            },
            Some("boom".to_string()),
            1,
            0,
            0,
            0,
            Some("boom".to_string()),
            tail,
            None,
        );
        let rendered = render_server_errors(&detail);
        assert!(
            rendered.contains("Stderr tail (10 lines)"),
            "stderr tail preview must be capped at 10 lines: {rendered}"
        );
        assert!(
            !rendered.contains("line 0 "),
            "oldest stderr lines must be dropped from preview: {rendered}"
        );
        assert!(
            rendered.contains("line 24"),
            "most recent stderr lines must remain: {rendered}"
        );
    }

    #[test]
    fn render_server_detail_failed_includes_state_detail() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Failed {
                reason: "fatal: out of memory".to_string(),
            },
            Some("fatal: out of memory".to_string()),
            7,
            0,
            0,
            0,
            None,
            Vec::new(),
            None,
        );
        let rendered = render_server_detail(&detail);
        assert!(rendered.contains("[failed]"));
        assert!(
            rendered.contains("State detail: fatal: out of memory"),
            "failed state detail must surface reason: {rendered}"
        );
        assert!(rendered.contains("Capabilities: not yet initialized"));
    }

    #[test]
    fn render_server_detail_includes_stderr_preview() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Degraded {
                reason: "warn".to_string(),
            },
            Some("warn".to_string()),
            1,
            0,
            0,
            0,
            None,
            vec!["warn: deprecated flag".to_string()],
            None,
        );
        let rendered = render_server_detail(&detail);
        assert!(
            rendered.contains("Stderr (last 1):"),
            "stderr preview line must be emitted when tail non-empty: {rendered}"
        );
        assert!(rendered.contains("warn: deprecated flag"));
    }

    #[test]
    fn render_server_detail_uninitialized_capabilities() {
        use crate::health::LspOperationalState;

        let detail = detail_from_fields(
            "/proj:rust-analyzer",
            "rust-analyzer",
            "/proj",
            LspOperationalState::Initializing,
            None,
            0,
            0,
            0,
            0,
            None,
            Vec::new(),
            None,
        );
        let rendered = render_server_detail(&detail);
        assert!(
            rendered.contains("Capabilities: not yet initialized"),
            "uninitialized server must show 'not yet initialized' marker: {rendered}"
        );
        // Must not emit a populated capabilities list when uninitialized.
        // The "Diagnostics mode:" line is also conditional on capabilities,
        // so it must be absent as well.
        assert!(
            !rendered.contains("Diagnostics mode:"),
            "uninitialized server must not surface a diagnostics mode: {rendered}"
        );
        // The capability "list" line uses "  Capabilities: name, name" form.
        // When uninitialized, the only capabilities line is the
        // placeholder "not yet initialized" marker. There must not be a
        // line that lists individual capabilities.
        let has_capability_list = rendered.lines().any(|l| {
            l.trim_start().starts_with("Capabilities: ") && !l.contains("not yet initialized")
        });
        assert!(
            !has_capability_list,
            "must not emit a populated capabilities list when not initialized: {rendered}"
        );
    }

    #[test]
    fn render_root_diagnosis_outside_allowed_root() {
        let diag = RootDiagnosis {
            input_path: "/tmp/outside/src/main.rs".to_string(),
            detected_language: Some("rust".to_string()),
            root_markers_found: Vec::new(),
            selected_root: None,
            server_profile: None,
            inside_allowed_root: false,
            issues: vec!["File is outside allowed root".to_string()],
        };
        let rendered = render_root_diagnosis(&diag);
        assert!(
            rendered.contains("Inside allowed root: no"),
            "outside-allowed-root must render as 'no': {rendered}"
        );
        assert!(rendered.contains("File is outside allowed root"));
        assert!(rendered.contains("Selected root: (none)"));
        assert!(rendered.contains("Language: rust"));
    }

    #[test]
    fn render_root_diagnosis_unrecognized_language() {
        let diag = RootDiagnosis {
            input_path: "/tmp/data.xyz".to_string(),
            detected_language: None,
            root_markers_found: Vec::new(),
            selected_root: None,
            server_profile: None,
            inside_allowed_root: true,
            issues: vec!["Could not detect language from file extension".to_string()],
        };
        let rendered = render_root_diagnosis(&diag);
        assert!(
            rendered.contains("Language: (unrecognized)"),
            "missing language must render '(unrecognized)': {rendered}"
        );
        assert!(
            rendered.contains("Server profile: (none available for this language)"),
            "missing profile must render '(none available for this language)': {rendered}"
        );
        assert!(rendered.contains("Could not detect language from file extension"));
    }

    #[test]
    fn render_root_diagnosis_language_without_server_profile() {
        let diag = RootDiagnosis {
            input_path: "/tmp/src/main.xyz".to_string(),
            detected_language: Some("xyz".to_string()),
            root_markers_found: vec!["Cargo.toml".to_string()],
            selected_root: Some("/tmp".to_string()),
            server_profile: None,
            inside_allowed_root: true,
            issues: vec!["No LSP server profile configured for language: xyz".to_string()],
        };
        let rendered = render_root_diagnosis(&diag);
        assert!(
            rendered.contains("Language: xyz"),
            "known language label must render: {rendered}"
        );
        assert!(
            rendered.contains("Server profile: (none available for this language)"),
            "missing profile must show '(none available)': {rendered}"
        );
        assert!(
            rendered.contains("No LSP server profile configured for language: xyz"),
            "issue text must surface: {rendered}"
        );
    }

    #[test]
    fn render_root_diagnosis_no_issues_shows_placeholder() {
        let diag = RootDiagnosis {
            input_path: "/tmp/proj/src/main.rs".to_string(),
            detected_language: Some("rust".to_string()),
            root_markers_found: vec!["Cargo.toml".to_string()],
            selected_root: Some("/tmp/proj".to_string()),
            server_profile: Some("rust-analyzer".to_string()),
            inside_allowed_root: true,
            issues: Vec::new(),
        };
        let rendered = render_root_diagnosis(&diag);
        assert!(
            rendered.contains("Issues: (none)"),
            "empty issues must render '(none)' placeholder: {rendered}"
        );
        assert!(rendered.contains("Root markers found: Cargo.toml"));
        assert!(rendered.contains("Selected root: /tmp/proj"));
        assert!(rendered.contains("Server profile: rust-analyzer"));
    }

    #[test]
    fn render_root_diagnosis_nested_markers_listed() {
        let diag = RootDiagnosis {
            input_path: "/repo/services/payments/src/main.rs".to_string(),
            detected_language: Some("rust".to_string()),
            root_markers_found: vec![
                "Cargo.toml".to_string(),
                ".git".to_string(),
                ".github".to_string(),
            ],
            selected_root: Some("/repo".to_string()),
            server_profile: Some("rust-analyzer".to_string()),
            inside_allowed_root: true,
            issues: Vec::new(),
        };
        let rendered = render_root_diagnosis(&diag);
        assert!(
            rendered.contains("Root markers found: Cargo.toml, .git, .github"),
            "all collected root markers must be listed: {rendered}"
        );
        // The selected root is the nearest one found, not the file's
        // immediate parent.
        assert!(
            rendered.contains("Selected root: /repo"),
            "selected_root should reflect the actual nearest marker directory: {rendered}"
        );
    }

    #[test]
    fn root_diagnosis_constructor_does_not_start_servers() {
        // This test asserts the architectural invariant from the
        // hardening plan: constructing or rendering a RootDiagnosis
        // must not have any side effects on server lifecycle. We
        // verify by constructing a diagnosis from scratch and
        // rendering it; the absence of a service handle means there
        // is no path through which a server could be started.
        let diag = RootDiagnosis {
            input_path: "/tmp/proj/src/main.rs".to_string(),
            detected_language: Some("rust".to_string()),
            root_markers_found: vec!["Cargo.toml".to_string()],
            selected_root: Some("/tmp/proj".to_string()),
            server_profile: Some("rust-analyzer".to_string()),
            inside_allowed_root: true,
            issues: Vec::new(),
        };
        let rendered = render_root_diagnosis(&diag);
        // The render must succeed and contain the expected fields
        // without invoking any service or runtime.
        assert!(rendered.contains("Root diagnosis for:"));
        assert!(rendered.contains("Server profile: rust-analyzer"));
    }

    // -----------------------------------------------------------------------
    // Phase 9 / 12 hardening: validate_preview_apply (testable boundary)
    //
    // These tests prove the validation surface that the TUI handler uses
    // before any file mutation. They do NOT touch the filesystem; the
    // patch applier is injected.
    // -----------------------------------------------------------------------

    use crate::context::PreviewFilePatch;
    use crate::preview_registry::PreviewArtifactRegistry;

    fn sha256_hex_str(s: &str) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(s.as_bytes()))
    }

    fn identity_patch_applier(original: &str, _patch: &str) -> Result<String, String> {
        Ok(original.to_string())
    }

    fn dummy_failing_patch_applier(_original: &str, _patch: &str) -> Result<String, String> {
        Err("patch failed in injected applier".to_string())
    }

    #[test]
    fn validate_preview_apply_returns_not_found_for_missing_id() {
        let registry = PreviewArtifactRegistry::new();
        let result = validate_preview_apply(&registry, "nonexistent", &identity_patch_applier);
        assert!(
            matches!(result, Err(PreviewApplyError::NotFound { ref preview_id }) if preview_id == "nonexistent"),
            "expected NotFound, got {result:?}"
        );
    }

    #[test]
    fn validate_preview_apply_blocks_stale_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        std::fs::write(&file_path, "fn main() {}\n").expect("write");
        let original_hash = sha256_hex_str("fn main() {}\n");

        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Formatting {
                description: "fmt".to_string(),
                content_hash: None,
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: file_path.to_string_lossy().into_owned(),
                    patch: "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() { changed(); }\n"
                        .to_string(),
                    original_hash: original_hash.clone(),
                }],
            },
            vec![file_path.to_string_lossy().into_owned()],
            std::collections::HashMap::from([(
                file_path.to_string_lossy().into_owned(),
                original_hash.clone(),
            )]),
            "rust-analyzer".to_string(),
        );
        registry.mark_stale(&id);

        let result = validate_preview_apply(&registry, &id, &identity_patch_applier);
        match result {
            Err(PreviewApplyError::Stale { preview_id, .. }) => {
                assert_eq!(preview_id, id);
            }
            other => panic!("expected Stale, got {other:?}"),
        }
    }

    #[test]
    fn validate_preview_apply_blocks_no_patch_entries() {
        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 0,
                patches: Vec::new(),
            },
            vec!["a.rs".to_string()],
            std::collections::HashMap::new(),
            "server".to_string(),
        );

        let result = validate_preview_apply(&registry, &id, &identity_patch_applier);
        assert!(
            matches!(result, Err(PreviewApplyError::NoPatches { ref preview_id }) if preview_id == &id),
            "expected NoPatches, got {result:?}"
        );
    }

    #[test]
    fn validate_preview_apply_blocks_already_applied_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        std::fs::write(&file_path, "fn main() {}\n").expect("write");
        let original_hash = sha256_hex_str("fn main() {}\n");

        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Formatting {
                description: "fmt".to_string(),
                content_hash: None,
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: file_path.to_string_lossy().into_owned(),
                    patch: "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() { changed(); }\n"
                        .to_string(),
                    original_hash: original_hash.clone(),
                }],
            },
            vec![file_path.to_string_lossy().into_owned()],
            std::collections::HashMap::new(),
            "server".to_string(),
        );
        registry.mark_applied(&id);

        let result = validate_preview_apply(&registry, &id, &identity_patch_applier);
        match result {
            Err(PreviewApplyError::AlreadyApplied { preview_id }) => {
                assert_eq!(preview_id, id);
            }
            other => panic!("expected AlreadyApplied, got {other:?}"),
        }
    }

    #[test]
    fn validate_preview_apply_returns_plan_for_fresh_preview() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        let original = "fn main() {}\n";
        std::fs::write(&file_path, original).expect("write");
        let original_hash = sha256_hex_str(original);

        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: file_path.to_string_lossy().into_owned(),
                    patch: "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn renamed() {}\n".to_string(),
                    original_hash,
                }],
            },
            vec![file_path.to_string_lossy().into_owned()],
            std::collections::HashMap::new(),
            "server".to_string(),
        );

        let result = validate_preview_apply(&registry, &id, &identity_patch_applier);
        let plan = result.expect("plan");
        assert_eq!(plan.preview_id, id);
        assert_eq!(plan.kind, "rename");
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].path, file_path.to_string_lossy());
        assert_eq!(plan.files[0].original_hash, plan.files[0].validated_hash);
        // Identity applier returns the original content unchanged.
        assert_eq!(plan.files[0].new_content, original);
    }

    #[test]
    fn validate_preview_apply_returns_hash_mismatch_when_disk_changed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        std::fs::write(&file_path, "fn main() {}\n").expect("write");
        let stale_hash = sha256_hex_str("fn main() { ORIGINAL }\n"); // mismatched

        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: file_path.to_string_lossy().into_owned(),
                    patch: "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn renamed() {}\n".to_string(),
                    original_hash: stale_hash,
                }],
            },
            vec![file_path.to_string_lossy().into_owned()],
            std::collections::HashMap::new(),
            "server".to_string(),
        );

        let result = validate_preview_apply(&registry, &id, &identity_patch_applier);
        match result {
            Err(PreviewApplyError::HashMismatch {
                path,
                expected_hash,
                actual_hash,
            }) => {
                assert_eq!(path, file_path.to_string_lossy());
                assert_eq!(actual_hash, sha256_hex_str("fn main() {}\n"));
                assert_ne!(expected_hash, actual_hash);
            }
            other => panic!("expected HashMismatch, got {other:?}"),
        }
    }

    #[test]
    fn validate_preview_apply_propagates_patch_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        let original = "fn main() {}\n";
        std::fs::write(&file_path, original).expect("write");
        let original_hash = sha256_hex_str(original);

        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: file_path.to_string_lossy().into_owned(),
                    patch: "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn renamed() {}\n".to_string(),
                    original_hash,
                }],
            },
            vec![file_path.to_string_lossy().into_owned()],
            std::collections::HashMap::new(),
            "server".to_string(),
        );

        let result = validate_preview_apply(&registry, &id, &dummy_failing_patch_applier);
        match result {
            Err(PreviewApplyError::PatchError { path, error }) => {
                assert_eq!(path, file_path.to_string_lossy());
                assert!(error.contains("patch failed in injected applier"));
            }
            other => panic!("expected PatchError, got {other:?}"),
        }
    }

    #[test]
    fn validate_preview_apply_returns_file_read_error_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("does_not_exist.rs");
        let original_hash = sha256_hex_str("fn main() {}\n");

        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: file_path.to_string_lossy().into_owned(),
                    patch: "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn renamed() {}\n".to_string(),
                    original_hash,
                }],
            },
            vec![file_path.to_string_lossy().into_owned()],
            std::collections::HashMap::new(),
            "server".to_string(),
        );

        let result = validate_preview_apply(&registry, &id, &identity_patch_applier);
        match result {
            Err(PreviewApplyError::FileReadError { path, .. }) => {
                assert_eq!(path, file_path.to_string_lossy());
            }
            other => panic!("expected FileReadError, got {other:?}"),
        }
    }

    #[test]
    fn validate_preview_apply_does_not_mutate_registry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        let original = "fn main() {}\n";
        std::fs::write(&file_path, original).expect("write");
        let original_hash = sha256_hex_str(original);

        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: file_path.to_string_lossy().into_owned(),
                    patch: "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn renamed() {}\n".to_string(),
                    original_hash: original_hash.clone(),
                }],
            },
            vec![file_path.to_string_lossy().into_owned()],
            std::collections::HashMap::new(),
            "server".to_string(),
        );

        let before = (
            registry.len(),
            registry.applied_count(),
            registry.stale_count(),
        );
        let _ = validate_preview_apply(&registry, &id, &identity_patch_applier).expect("plan");
        let after = (
            registry.len(),
            registry.applied_count(),
            registry.stale_count(),
        );
        assert_eq!(
            before, after,
            "validate_preview_apply must not mutate registry"
        );
        assert!(
            !registry.get(&id).unwrap().applied,
            "applied flag must remain false until caller writes and explicitly marks applied"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 9 / 12 write-side: write_preview_apply_plan_atomically_enough
    //
    // These tests prove the write-side handler correctly handles partial
    // failures, race conditions, and normal success paths.
    // -----------------------------------------------------------------------

    fn make_write_plan(files: Vec<PreviewApplyFilePlan>) -> PreviewApplyPlan {
        PreviewApplyPlan {
            preview_id: "test-id".to_string(),
            kind: "rename".to_string(),
            title: "test preview".to_string(),
            provenance: "rust-analyzer".to_string(),
            files,
        }
    }

    #[test]
    fn write_plan_successful_single_file_apply() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        let original = "fn main() {}\n";
        let new_content = "fn main() { changed(); }\n";
        std::fs::write(&file_path, original).expect("write");

        let plan = make_write_plan(vec![PreviewApplyFilePlan {
            path: file_path.to_string_lossy().into_owned(),
            original_hash: sha256_hex_str(original),
            validated_hash: sha256_hex_str(original),
            new_content: new_content.to_string(),
        }]);

        let report = write_preview_apply_plan_atomically_enough(&plan).expect("write plan");
        assert!(report.all_succeeded);
        assert_eq!(report.written.len(), 1);
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), new_content);
    }

    #[test]
    fn write_plan_successful_multi_file_apply() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut plans = Vec::new();
        let mut expected_contents = Vec::new();

        for i in 0..3 {
            let path = dir.path().join(format!("file_{i}.rs"));
            let original = format!("fn a{i}() {{}}\n");
            let new_content = format!("fn a{i}() {{ changed(); }}\n");
            std::fs::write(&path, &original).expect("write");
            plans.push(PreviewApplyFilePlan {
                path: path.to_string_lossy().into_owned(),
                original_hash: sha256_hex_str(&original),
                validated_hash: sha256_hex_str(&original),
                new_content: new_content.clone(),
            });
            expected_contents.push((path, new_content));
        }

        let plan = make_write_plan(plans);
        let report = write_preview_apply_plan_atomically_enough(&plan).expect("write plan");
        assert!(report.all_succeeded);
        assert_eq!(report.written.len(), 3);

        for (path, expected) in expected_contents {
            assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        }
    }

    #[test]
    fn write_plan_stale_during_write_blocks_apply() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        let original = "fn main() {}\n";
        let new_content = "fn main() { changed(); }\n";
        std::fs::write(&file_path, original).expect("write");

        // Plan records the hash of the original content.
        let plan = make_write_plan(vec![PreviewApplyFilePlan {
            path: file_path.to_string_lossy().into_owned(),
            original_hash: sha256_hex_str(original),
            validated_hash: sha256_hex_str(original),
            new_content: new_content.to_string(),
        }]);

        // Modify the file AFTER plan creation but BEFORE calling the helper.
        std::fs::write(&file_path, "fn main() { external_edit(); }\n").expect("external edit");

        let err =
            write_preview_apply_plan_atomically_enough(&plan).expect_err("should fail on stale");
        match err {
            PreviewApplyWriteError::StaleDuringWrite {
                path,
                expected_hash,
                actual_hash,
            } => {
                assert_eq!(path, file_path.to_string_lossy());
                assert_eq!(expected_hash, sha256_hex_str(original));
                assert_eq!(
                    actual_hash,
                    sha256_hex_str("fn main() { external_edit(); }\n")
                );
            }
            other => panic!("expected StaleDuringWrite, got {other:?}"),
        }

        // File must NOT be modified by the helper.
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "fn main() { external_edit(); }\n"
        );
    }

    #[test]
    fn write_plan_failure_on_first_file_reports_empty_completed() {
        let plan = make_write_plan(vec![PreviewApplyFilePlan {
            path: "/nonexistent/dir/a.rs".to_string(),
            original_hash: sha256_hex_str("doesn't matter"),
            validated_hash: sha256_hex_str("doesn't matter"),
            new_content: "new content\n".to_string(),
        }]);

        let err =
            write_preview_apply_plan_atomically_enough(&plan).expect_err("should fail on read");
        match err {
            PreviewApplyWriteError::WriteFailed {
                path,
                error,
                completed,
            } => {
                assert_eq!(path, "/nonexistent/dir/a.rs");
                assert!(error.contains("read error"));
                assert!(
                    completed.is_empty(),
                    "no files should have been completed before first-file failure"
                );
            }
            other => panic!("expected WriteFailed, got {other:?}"),
        }
    }

    #[test]
    fn write_plan_failure_on_second_file_reports_partial_completed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file1 = dir.path().join("ok.rs");
        let original1 = "fn ok() {}\n";
        std::fs::write(&file1, original1).expect("write");

        let plan = make_write_plan(vec![
            PreviewApplyFilePlan {
                path: file1.to_string_lossy().into_owned(),
                original_hash: sha256_hex_str(original1),
                validated_hash: sha256_hex_str(original1),
                new_content: "fn ok() { changed(); }\n".to_string(),
            },
            PreviewApplyFilePlan {
                path: "/nonexistent/dir/fail.rs".to_string(),
                original_hash: sha256_hex_str("whatever"),
                validated_hash: sha256_hex_str("whatever"),
                new_content: "fail content\n".to_string(),
            },
        ]);

        let err =
            write_preview_apply_plan_atomically_enough(&plan).expect_err("should fail on 2nd");
        match err {
            PreviewApplyWriteError::WriteFailed {
                path,
                error,
                completed,
            } => {
                assert_eq!(path, "/nonexistent/dir/fail.rs");
                assert!(error.contains("read error"));
                assert_eq!(completed.len(), 1);
                assert_eq!(completed[0], file1.to_string_lossy());
            }
            other => panic!("expected WriteFailed, got {other:?}"),
        }

        // First file WAS written.
        assert_eq!(
            std::fs::read_to_string(&file1).unwrap(),
            "fn ok() { changed(); }\n"
        );
    }

    #[test]
    fn write_plan_stale_during_write_on_second_file_blocks_third() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file1 = dir.path().join("ok.rs");
        let file2 = dir.path().join("stale.rs");
        let file3 = dir.path().join("never.rs");
        let original1 = "fn ok() {}\n";
        let original2 = "fn stale() {}\n";
        let original3 = "fn never() {}\n";

        std::fs::write(&file1, original1).expect("write");
        std::fs::write(&file2, original2).expect("write");
        std::fs::write(&file3, original3).expect("write");

        let plan = make_write_plan(vec![
            PreviewApplyFilePlan {
                path: file1.to_string_lossy().into_owned(),
                original_hash: sha256_hex_str(original1),
                validated_hash: sha256_hex_str(original1),
                new_content: "fn ok() { changed(); }\n".to_string(),
            },
            PreviewApplyFilePlan {
                path: file2.to_string_lossy().into_owned(),
                original_hash: sha256_hex_str(original2),
                validated_hash: sha256_hex_str(original2),
                new_content: "fn stale() { changed(); }\n".to_string(),
            },
            PreviewApplyFilePlan {
                path: file3.to_string_lossy().into_owned(),
                original_hash: sha256_hex_str(original3),
                validated_hash: sha256_hex_str(original3),
                new_content: "fn never() { changed(); }\n".to_string(),
            },
        ]);

        // Modify file2 after plan creation.
        std::fs::write(&file2, "fn stale() { external(); }\n").expect("external edit");

        let err =
            write_preview_apply_plan_atomically_enough(&plan).expect_err("should fail on 2nd");
        match err {
            PreviewApplyWriteError::StaleDuringWrite {
                path,
                expected_hash,
                ..
            } => {
                assert_eq!(path, file2.to_string_lossy());
                assert_eq!(expected_hash, sha256_hex_str(original2));
            }
            other => panic!("expected StaleDuringWrite, got {other:?}"),
        }

        // file1 was written, file2 was NOT, file3 was NOT.
        assert_eq!(
            std::fs::read_to_string(&file1).unwrap(),
            "fn ok() { changed(); }\n"
        );
        assert_eq!(
            std::fs::read_to_string(&file2).unwrap(),
            "fn stale() { external(); }\n"
        );
        assert_eq!(std::fs::read_to_string(&file3).unwrap(), original3);
    }

    #[test]
    fn write_plan_success_then_stale_returns_only_written() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file1 = dir.path().join("first.rs");
        let file2 = dir.path().join("second.rs");
        let original1 = "fn first() {}\n";
        let original2 = "fn second() {}\n";

        std::fs::write(&file1, original1).expect("write");
        std::fs::write(&file2, original2).expect("write");

        let plan = make_write_plan(vec![
            PreviewApplyFilePlan {
                path: file1.to_string_lossy().into_owned(),
                original_hash: sha256_hex_str(original1),
                validated_hash: sha256_hex_str(original1),
                new_content: "fn first() { changed(); }\n".to_string(),
            },
            PreviewApplyFilePlan {
                path: file2.to_string_lossy().into_owned(),
                original_hash: sha256_hex_str(original2),
                validated_hash: sha256_hex_str(original2),
                new_content: "fn second() { changed(); }\n".to_string(),
            },
        ]);

        // Stale file2 after plan creation.
        std::fs::write(&file2, "fn second() { surprise(); }\n").expect("stale edit");

        let err = write_preview_apply_plan_atomically_enough(&plan).expect_err("should fail");
        match err {
            PreviewApplyWriteError::StaleDuringWrite { path, .. } => {
                assert_eq!(path, file2.to_string_lossy());
            }
            other => panic!("expected StaleDuringWrite, got {other:?}"),
        }

        // file1 was written successfully.
        assert_eq!(
            std::fs::read_to_string(&file1).unwrap(),
            "fn first() { changed(); }\n"
        );
    }

    /// Patch applier that replaces the first line of `original` with the
    /// first line after the `+` marker in the unified diff header.
    fn simple_replacement_applier(original: &str, patch: &str) -> Result<String, String> {
        // Extract the "+replacement" line from the patch.
        let replacement = patch
            .lines()
            .find(|l| l.starts_with('+'))
            .ok_or_else(|| "no + line in patch".to_string())?;
        let mut lines: Vec<&str> = original.lines().collect();
        if let Some(first) = lines.first_mut() {
            *first = &replacement[1..]; // strip the '+' prefix
        }
        let mut result = lines.join("\n");
        // Preserve trailing newline if original had one.
        if original.ends_with('\n') {
            result.push('\n');
        }
        Ok(result)
    }

    #[test]
    fn validate_then_write_end_to_end() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        let original = "fn main() {}\n";
        let new_content = "fn main() { renamed(); }\n";
        std::fs::write(&file_path, original).expect("write");
        let original_hash = sha256_hex_str(original);

        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: file_path.to_string_lossy().into_owned(),
                    patch: "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() { renamed(); }\n"
                        .to_string(),
                    original_hash,
                }],
            },
            vec![file_path.to_string_lossy().into_owned()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );

        // Validate with a real-ish patch applier.
        let plan = validate_preview_apply(&registry, &id, &simple_replacement_applier)
            .expect("validate should succeed");
        assert_eq!(plan.files[0].new_content, new_content);

        // Write.
        let report =
            write_preview_apply_plan_atomically_enough(&plan).expect("write should succeed");
        assert!(report.all_succeeded);

        // Mark applied only after all writes succeed.
        registry.mark_applied(&id);
        assert!(registry.get(&id).unwrap().applied);

        // File content is updated.
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), new_content);
    }

    #[test]
    fn validate_then_write_end_to_end_stale_file_blocks_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("a.rs");
        let original = "fn main() {}\n";
        std::fs::write(&file_path, original).expect("write");
        let original_hash = sha256_hex_str(original);

        let mut registry = PreviewArtifactRegistry::new();
        let id = registry.register(
            LspPreviewArtifact::Rename {
                description: "rename".to_string(),
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: file_path.to_string_lossy().into_owned(),
                    patch: "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() { renamed(); }\n"
                        .to_string(),
                    original_hash,
                }],
            },
            vec![file_path.to_string_lossy().into_owned()],
            std::collections::HashMap::new(),
            "rust-analyzer".to_string(),
        );

        // Validate first (succeeds — file unchanged).
        let plan = validate_preview_apply(&registry, &id, &simple_replacement_applier)
            .expect("validate should succeed");

        // External edit after validation.
        std::fs::write(&file_path, "fn main() { external(); }\n").expect("external edit");

        // Write-side detects stale.
        let err = write_preview_apply_plan_atomically_enough(&plan).expect_err("should fail");
        assert!(matches!(
            err,
            PreviewApplyWriteError::StaleDuringWrite { .. }
        ));

        // NOT marked applied.
        assert!(!registry.get(&id).unwrap().applied);

        // File content is the external edit, not the patch.
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "fn main() { external(); }\n"
        );
    }
}
