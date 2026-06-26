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
            ("hover", true), // always available
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
}
