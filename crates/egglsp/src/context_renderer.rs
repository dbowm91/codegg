//! Agent context renderer.
//!
//! Renders [`LspContextPacket`] into concise, readable text blocks
//! suitable for agent prompts. No raw JSON is ever dumped.

use crate::context::{
    AgentContextSource, LspContextItem, LspContextItemKind, LspContextPacket, LspPreviewArtifact,
};

// ---------------------------------------------------------------------------
// Model tier
// ---------------------------------------------------------------------------

/// Controls how much LSP context is rendered into agent prompts.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ModelTier {
    /// Minimal: diagnostics + hunk-local definitions only.
    Small,
    /// Standard: diagnostics + references + hover.
    #[default]
    Workhorse,
    /// Broader: all available evidence.
    Frontier,
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Small => write!(f, "small"),
            Self::Workhorse => write!(f, "workhorse"),
            Self::Frontier => write!(f, "frontier"),
        }
    }
}

/// Render a model tier as a human-readable string.
pub fn render_model_tier(tier: ModelTier) -> String {
    tier.to_string()
}

/// Map a prompt-profile family or name into an [`ModelTier`].
///
/// This is a best-effort classification that callers can override by
/// passing an explicit `ModelTier` to the renderer. The mapping is
/// deliberately conservative:
///
/// - `FrontierReasoning`, `FrontierExecutor`, `LongContextPlanner`,
///   `Default` → `Frontier`
/// - `FastExecutor`, `LocalStrict`, `ToolFragile` → `Small`
/// - everything else (including unknown names) → `Workhorse`
pub fn model_tier_for_profile(family: &str) -> ModelTier {
    let f = family.to_ascii_lowercase();
    match f.as_str() {
        "frontierreasoning"
        | "frontier_executor"
        | "frontiere executor"
        | "frontier"
        | "longcontextplanner"
        | "long_context_planner"
        | "default" => ModelTier::Frontier,
        "fastexecutor" | "fast_executor" | "localstrict" | "local_strict" | "toolfragile"
        | "tool_fragile" => ModelTier::Small,
        _ => ModelTier::Workhorse,
    }
}

// ---------------------------------------------------------------------------
// Render config
// ---------------------------------------------------------------------------

/// Configuration for agent context rendering.
#[derive(Debug, Clone)]
pub struct LspContextRenderConfig {
    /// Maximum diagnostic items to render.
    pub max_diagnostics: usize,
    /// Maximum reference items to render.
    pub max_references: usize,
    /// Maximum symbol items to render.
    pub max_symbols: usize,
    /// Byte budget per section.
    pub max_bytes_per_section: usize,
    /// Whether to include preview artifact notes.
    pub include_previews: bool,
    /// Whether to include truncation notes.
    pub include_truncation_notes: bool,
    /// Model tier controlling content breadth.
    pub model_tier: ModelTier,
}

impl Default for LspContextRenderConfig {
    fn default() -> Self {
        Self {
            max_diagnostics: 10,
            max_references: 15,
            max_symbols: 15,
            max_bytes_per_section: 2000,
            include_previews: true,
            include_truncation_notes: true,
            model_tier: ModelTier::Workhorse,
        }
    }
}

// ---------------------------------------------------------------------------
// Item rendering
// ---------------------------------------------------------------------------

fn render_item_line(item: &LspContextItem) -> String {
    let kind_label = match item.kind {
        LspContextItemKind::Diagnostic => "diagnostic",
        LspContextItemKind::Definition => "definition",
        LspContextItemKind::Declaration => "declaration",
        LspContextItemKind::Reference => "reference",
        LspContextItemKind::Implementation => "implementation",
        LspContextItemKind::DocumentHighlight => "highlight",
        LspContextItemKind::Hover => "hover",
        LspContextItemKind::SignatureHelp => "signature",
        LspContextItemKind::CompletionSummary => "completion",
        LspContextItemKind::SemanticTokenSummary => "semantic",
        LspContextItemKind::WorkspaceSymbol => "symbol",
        LspContextItemKind::OperationalNote => "note",
    };

    let freshness_label = match item.provenance.freshness {
        crate::context::LspEvidenceFreshness::Fresh => "fresh",
        crate::context::LspEvidenceFreshness::PossiblyStale => "possibly_stale",
        crate::context::LspEvidenceFreshness::Stale => "stale",
        crate::context::LspEvidenceFreshness::RetainedAfterRestart => "retained",
        crate::context::LspEvidenceFreshness::StaleAfterEdit => "stale_after_edit",
        crate::context::LspEvidenceFreshness::ServerGenerationMismatch => "gen_mismatch",
        crate::context::LspEvidenceFreshness::Unknown => "unknown",
    };

    let gen_label = item
        .provenance
        .server_generation
        .map(|g| format!(" gen={g}"))
        .unwrap_or_default();

    let loc = match (item.line, item.column) {
        (Some(l), Some(c)) => format!(":{}-{}", l + 1, c + 1),
        (Some(l), None) => format!(":{}", l + 1),
        _ => String::new(),
    };

    format!(
        "{} {}{} [{}, {}, {}{}]{}",
        item.file.display(),
        loc,
        item.symbol
            .as_ref()
            .map(|s| format!(" `{s}`"))
            .unwrap_or_default(),
        kind_label,
        freshness_label,
        item.provenance.server_id,
        gen_label,
        if item.message.len() > 80 {
            format!(" | {}", &item.message[..77])
        } else {
            format!(" | {}", item.message)
        },
    )
}

fn truncate_section(text: &str, max_bytes: usize, remaining: usize) -> String {
    let bytes = text.len();
    if bytes <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    // Don't cut in the middle of a UTF-8 char.
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = text[..end].to_string();
    result.push_str(&format!("... ({remaining} more items)"));
    result
}

// ---------------------------------------------------------------------------
// Section builders
// ---------------------------------------------------------------------------

fn build_diagnostics_section(
    items: &[LspContextItem],
    config: &LspContextRenderConfig,
) -> Option<String> {
    let mut diags: Vec<&LspContextItem> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Diagnostic)
        .collect();
    diags.truncate(config.max_diagnostics);

    if diags.is_empty() {
        return None;
    }

    let total = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Diagnostic)
        .count();

    let mut lines = Vec::new();
    for item in &diags {
        lines.push(render_item_line(item));
    }

    let body = lines.join("\n");
    let trunc_note = if total > config.max_diagnostics {
        format!("\n({} diagnostics omitted)", total - config.max_diagnostics)
    } else {
        String::new()
    };

    let section = format!("## Diagnostics\n{body}{trunc_note}");
    Some(truncate_section(
        &section,
        config.max_bytes_per_section,
        total.saturating_sub(config.max_diagnostics),
    ))
}

fn build_definitions_section(
    items: &[LspContextItem],
    config: &LspContextRenderConfig,
    tier: ModelTier,
) -> Option<String> {
    if matches!(tier, ModelTier::Small) {
        // Small tier only gets hunk-local definitions.
        let hunk_defs: Vec<&LspContextItem> = items
            .iter()
            .filter(|i| {
                matches!(
                    i.kind,
                    LspContextItemKind::Definition | LspContextItemKind::Declaration
                ) && i.score.is_hunk_local
            })
            .collect();
        if hunk_defs.is_empty() {
            return None;
        }
        let lines: Vec<String> = hunk_defs.into_iter().map(render_item_line).collect();
        return Some(format!("## Definitions (hunk-local)\n{}", lines.join("\n")));
    }

    let defs: Vec<&LspContextItem> = items
        .iter()
        .filter(|i| {
            matches!(
                i.kind,
                LspContextItemKind::Definition | LspContextItemKind::Declaration
            )
        })
        .collect();
    if defs.is_empty() {
        return None;
    }
    let total = defs.len();
    let mut lines: Vec<String> = defs
        .into_iter()
        .take(config.max_symbols)
        .map(render_item_line)
        .collect();
    if total > config.max_symbols {
        lines.push(format!("({} more definitions)", total - config.max_symbols));
    }
    Some(truncate_section(
        &format!("## Definitions\n{}", lines.join("\n")),
        config.max_bytes_per_section,
        total.saturating_sub(config.max_symbols),
    ))
}

fn build_references_section(
    items: &[LspContextItem],
    config: &LspContextRenderConfig,
    tier: ModelTier,
) -> Option<String> {
    if matches!(tier, ModelTier::Small) {
        return None;
    }

    let refs: Vec<&LspContextItem> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Reference)
        .collect();
    if refs.is_empty() {
        return None;
    }
    let total = refs.len();
    let mut lines: Vec<String> = refs
        .into_iter()
        .take(config.max_references)
        .map(render_item_line)
        .collect();
    if total > config.max_references {
        lines.push(format!(
            "({} more references)",
            total - config.max_references
        ));
    }
    Some(truncate_section(
        &format!("## References\n{}", lines.join("\n")),
        config.max_bytes_per_section,
        total.saturating_sub(config.max_references),
    ))
}

fn build_hover_section(
    items: &[LspContextItem],
    config: &LspContextRenderConfig,
    tier: ModelTier,
) -> Option<String> {
    if matches!(tier, ModelTier::Small) {
        return None;
    }

    let hovers: Vec<&LspContextItem> = items
        .iter()
        .filter(|i| {
            matches!(
                i.kind,
                LspContextItemKind::Hover | LspContextItemKind::SignatureHelp
            )
        })
        .collect();
    if hovers.is_empty() {
        return None;
    }
    let total = hovers.len();
    let lines: Vec<String> = hovers.into_iter().take(5).map(render_item_line).collect();
    Some(truncate_section(
        &format!("## Hover/Signature\n{}", lines.join("\n")),
        config.max_bytes_per_section,
        total.saturating_sub(5),
    ))
}

fn build_previews_section(
    packet: &LspContextPacket,
    config: &LspContextRenderConfig,
) -> Option<String> {
    if !config.include_previews || packet.previews.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    for (i, preview) in packet.previews.iter().enumerate() {
        let preview_id = packet.preview_ids.get(i).filter(|id| !id.is_empty());

        let label = match preview {
            LspPreviewArtifact::Rename { description, .. } => format!("Rename: {description}"),
            LspPreviewArtifact::Formatting { description, .. } => {
                format!("Formatting: {description}")
            }
            LspPreviewArtifact::CodeAction { description, .. } => {
                format!("CodeAction: {description}")
            }
        };

        let mut line = format!("{label} — Preview only; not applied.");
        if let Some(id) = preview_id {
            line.push_str(&format!(" ID: {id}."));
        }
        line.push_str(" User approval is required before applying.");
        lines.push(line);
    }
    Some(format!("## Previews\n{}", lines.join("\n")))
}

/// Render lifecycle-state warnings for agent context.
///
/// When the LSP server is not in Ready state, agents should not
/// over-trust the evidence. This function adds explicit notes.
fn render_lifecycle_notes(packet: &LspContextPacket) -> Vec<String> {
    let mut notes = Vec::new();

    if let Some(ref state) = packet.operational_state {
        match state.as_str() {
            "starting" => {
                notes.push(
                    "LSP server is starting — no semantic evidence available yet.".to_string(),
                );
            }
            "initializing" => {
                notes.push(
                    "LSP server is initializing — no semantic evidence available yet.".to_string(),
                );
            }
            "indexing" => {
                notes.push(
                    "LSP server is indexing — diagnostics and symbols may be incomplete."
                        .to_string(),
                );
            }
            "degraded" => {
                notes.push("LSP server is degraded — evidence may be partial.".to_string());
            }
            "restart_scheduled" => {
                notes.push(
                    "LSP server restart is scheduled — current evidence may become stale."
                        .to_string(),
                );
            }
            "restarting" => {
                notes.push(
                    "LSP server is restarting — current evidence should be treated as stale."
                        .to_string(),
                );
            }
            "failed" => {
                notes.push(
                    "LSP server has failed — no fresh semantic evidence available.".to_string(),
                );
            }
            "stopping" => {
                notes.push(
                    "LSP server is stopping — no fresh semantic evidence available.".to_string(),
                );
            }
            "stopped" => {
                notes.push("LSP server is stopped — no semantic evidence available.".to_string());
            }
            "ready" => {
                // No note needed for ready state.
            }
            _ => {
                notes.push(format!(
                    "LSP server is in unknown state ({state}) — evidence may be unreliable."
                ));
            }
        }
    }

    notes
}

fn build_notes_section(
    packet: &LspContextPacket,
    config: &LspContextRenderConfig,
) -> Option<String> {
    let mut notes = packet.notes.clone();

    notes.extend(render_lifecycle_notes(packet));

    if config.include_truncation_notes {
        for note in &packet.truncation.notes {
            notes.push(note.clone());
        }
    }

    if notes.is_empty() {
        return None;
    }

    let lines: Vec<String> = notes.into_iter().map(|n| format!("- {n}")).collect();
    Some(format!("## Notes\n{}", lines.join("\n")))
}

// ---------------------------------------------------------------------------
// Phase 10 section builders
// ---------------------------------------------------------------------------

fn build_impact_analysis_section(
    items: &[LspContextItem],
    config: &LspContextRenderConfig,
    tier: ModelTier,
) -> Option<String> {
    let defs: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Definition)
        .collect();
    let refs: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Reference)
        .collect();
    let impls: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Implementation)
        .collect();
    let diag_in_changed: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == LspContextItemKind::Diagnostic
                && i.source == Some(AgentContextSource::Diagnostics)
        })
        .collect();

    if defs.is_empty() && refs.is_empty() && impls.is_empty() && diag_in_changed.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    if !defs.is_empty() {
        lines.push("### Target Definition".to_string());
        for item in defs.iter().take(3) {
            lines.push(render_item_line(item));
        }
    }
    if !refs.is_empty() {
        let max = match tier {
            ModelTier::Small => 5,
            ModelTier::Workhorse => config.max_references.min(15),
            ModelTier::Frontier => config.max_references,
        };
        lines.push(format!(
            "### Affected References ({}/{})",
            refs.len().min(max),
            refs.len()
        ));
        for item in refs.iter().take(max) {
            lines.push(render_item_line(item));
        }
        if refs.len() > max {
            lines.push(format!("({} more references truncated)", refs.len() - max));
        }
    }
    if !impls.is_empty() && matches!(tier, ModelTier::Frontier) {
        lines.push(format!("### Implementations ({})", impls.len()));
        for item in impls.iter().take(5) {
            lines.push(render_item_line(item));
        }
    }
    if !diag_in_changed.is_empty() {
        lines.push(format!(
            "### Diagnostics in Changed Files ({})",
            diag_in_changed.len()
        ));
        for item in diag_in_changed.iter().take(config.max_diagnostics) {
            lines.push(render_item_line(item));
        }
    }

    if lines.is_empty() {
        return None;
    }
    Some(truncate_section(
        &format!("## Impact Analysis\n{}", lines.join("\n")),
        config.max_bytes_per_section,
        0,
    ))
}

fn build_test_failure_section(
    items: &[LspContextItem],
    config: &LspContextRenderConfig,
    _tier: ModelTier,
) -> Option<String> {
    let test_diags: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == LspContextItemKind::Diagnostic
                && i.source == Some(AgentContextSource::Diagnostics)
        })
        .collect();
    let ws_symbols: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::WorkspaceSymbol)
        .collect();
    let failure_defs: Vec<_> = items
        .iter()
        .filter(|i| {
            matches!(
                i.kind,
                LspContextItemKind::Definition | LspContextItemKind::Reference
            )
        })
        .collect();

    if test_diags.is_empty() && ws_symbols.is_empty() && failure_defs.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    if !test_diags.is_empty() {
        lines.push(format!("### Test Diagnostics ({})", test_diags.len()));
        for item in test_diags.iter().take(config.max_diagnostics) {
            lines.push(render_item_line(item));
        }
    }
    if !ws_symbols.is_empty() {
        lines.push(format!("### Test Symbols ({})", ws_symbols.len()));
        for item in ws_symbols.iter().take(config.max_symbols) {
            lines.push(render_item_line(item));
        }
    }
    if !failure_defs.is_empty() {
        lines.push(format!(
            "### Failure-linked Definitions/Refs ({})",
            failure_defs.len()
        ));
        for item in failure_defs.iter().take(config.max_references) {
            lines.push(render_item_line(item));
        }
    }

    if lines.is_empty() {
        return None;
    }
    Some(truncate_section(
        &format!("## Failure-linked Evidence\n{}", lines.join("\n")),
        config.max_bytes_per_section,
        0,
    ))
}

fn build_boundary_section(
    items: &[LspContextItem],
    config: &LspContextRenderConfig,
    tier: ModelTier,
) -> Option<String> {
    let boundary_syms: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::WorkspaceSymbol)
        .collect();
    let impls: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Implementation)
        .collect();
    let ext_refs: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Reference && !i.score.is_same_file)
        .collect();

    if boundary_syms.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    lines.push(format!("### Boundary Symbols ({})", boundary_syms.len()));
    for item in boundary_syms.iter().take(config.max_symbols) {
        lines.push(render_item_line(item));
    }
    if !ext_refs.is_empty() {
        let max = match tier {
            ModelTier::Small => 0,
            _ => config.max_references,
        };
        if max > 0 {
            lines.push(format!("### External References ({})", ext_refs.len()));
            for item in ext_refs.iter().take(max) {
                lines.push(render_item_line(item));
            }
        }
    }
    if !impls.is_empty() && matches!(tier, ModelTier::Frontier) {
        lines.push(format!("### Implementations ({})", impls.len()));
        for item in impls.iter().take(5) {
            lines.push(render_item_line(item));
        }
    }

    if lines.is_empty() {
        return None;
    }
    Some(truncate_section(
        &format!("## Interface Boundary\n{}", lines.join("\n")),
        config.max_bytes_per_section,
        0,
    ))
}

fn build_cross_file_section(
    items: &[LspContextItem],
    config: &LspContextRenderConfig,
    _tier: ModelTier,
) -> Option<String> {
    let primary_items: Vec<_> = items.iter().filter(|i| i.score.is_same_file).collect();
    let related_items: Vec<_> = items.iter().filter(|i| !i.score.is_same_file).collect();

    if primary_items.is_empty() && related_items.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    if !primary_items.is_empty() {
        lines.push(format!(
            "### Primary File Evidence ({})",
            primary_items.len()
        ));
        for item in primary_items
            .iter()
            .take(config.max_symbols + config.max_diagnostics)
        {
            lines.push(render_item_line(item));
        }
    }
    if !related_items.is_empty() {
        lines.push(format!(
            "### Related File Evidence ({})",
            related_items.len()
        ));
        for item in related_items
            .iter()
            .take(config.max_symbols + config.max_diagnostics)
        {
            lines.push(render_item_line(item));
        }
    }

    if lines.is_empty() {
        return None;
    }
    Some(truncate_section(
        &format!("## Cross-File Repair\n{}", lines.join("\n")),
        config.max_bytes_per_section,
        0,
    ))
}

fn build_call_neighborhood_section(
    items: &[LspContextItem],
    config: &LspContextRenderConfig,
    _tier: ModelTier,
) -> Option<String> {
    let incoming: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == LspContextItemKind::DocumentHighlight
                || (i.kind == LspContextItemKind::Reference
                    && i.message.contains("caller highlight"))
        })
        .collect();
    let outgoing: Vec<_> = items
        .iter()
        .filter(|i| i.message.contains("callees (outgoing)"))
        .collect();

    if incoming.is_empty() && outgoing.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    if !outgoing.is_empty() {
        lines.push(format!("### Outgoing Calls ({})", outgoing.len()));
        for item in outgoing.iter().take(config.max_references) {
            lines.push(render_item_line(item));
        }
    }
    if !incoming.is_empty() {
        lines.push(format!("### Incoming Calls ({})", incoming.len()));
        for item in incoming.iter().take(config.max_references) {
            lines.push(render_item_line(item));
        }
    }

    if lines.is_empty() {
        return None;
    }
    Some(truncate_section(
        &format!("## Call Neighborhood\n{}", lines.join("\n")),
        config.max_bytes_per_section,
        0,
    ))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render an [`LspContextPacket`] into a concise text block for agent prompts.
///
/// Sections are rendered based on the model tier:
/// - **Small**: diagnostics + hunk-local definitions only
/// - **Workhorse**: diagnostics + references + hover + definitions
/// - **Frontier**: all available evidence
///
/// Never dumps raw JSON. Each section is omitted when empty.
pub fn render_lsp_context_for_agent(
    packet: &LspContextPacket,
    config: &LspContextRenderConfig,
) -> String {
    let tier = config.model_tier;
    let mut sections = Vec::new();

    // Status line always present.
    sections.push(render_lsp_status_line(packet));

    // Phase 10 request-specific sections.
    let has_phase10 = matches!(
        packet.request,
        crate::context::LspContextRequest::ImpactAnalysis { .. }
            | crate::context::LspContextRequest::TestFailureRepair { .. }
            | crate::context::LspContextRequest::InterfaceBoundary { .. }
            | crate::context::LspContextRequest::CrossFileRepair { .. }
            | crate::context::LspContextRequest::CallNeighborhood { .. }
    );

    if has_phase10 {
        // Render request-specific section first, then generic sections.
        match &packet.request {
            crate::context::LspContextRequest::ImpactAnalysis { .. } => {
                if let Some(s) = build_impact_analysis_section(&packet.items, config, tier) {
                    sections.push(s);
                }
            }
            crate::context::LspContextRequest::TestFailureRepair { .. } => {
                if let Some(s) = build_test_failure_section(&packet.items, config, tier) {
                    sections.push(s);
                }
            }
            crate::context::LspContextRequest::InterfaceBoundary { .. } => {
                if let Some(s) = build_boundary_section(&packet.items, config, tier) {
                    sections.push(s);
                }
            }
            crate::context::LspContextRequest::CrossFileRepair { .. } => {
                if let Some(s) = build_cross_file_section(&packet.items, config, tier) {
                    sections.push(s);
                }
            }
            crate::context::LspContextRequest::CallNeighborhood { .. } => {
                if let Some(s) = build_call_neighborhood_section(&packet.items, config, tier) {
                    sections.push(s);
                }
            }
            _ => {}
        }
    }

    // Generic sections.
    if let Some(s) = build_diagnostics_section(&packet.items, config) {
        sections.push(s);
    }
    if let Some(s) = build_definitions_section(&packet.items, config, tier) {
        sections.push(s);
    }
    if let Some(s) = build_references_section(&packet.items, config, tier) {
        sections.push(s);
    }
    if let Some(s) = build_hover_section(&packet.items, config, tier) {
        sections.push(s);
    }
    if let Some(s) = build_previews_section(packet, config) {
        sections.push(s);
    }
    if let Some(s) = build_notes_section(packet, config) {
        sections.push(s);
    }

    sections.join("\n\n")
}

/// Render a single-line LSP status summary.
pub fn render_lsp_status_line(packet: &LspContextPacket) -> String {
    let server_id = packet
        .items
        .iter()
        .find(|i| !i.provenance.server_id.is_empty())
        .map(|i| i.provenance.server_id.as_str())
        .unwrap_or("unknown");

    let gen = packet
        .items
        .iter()
        .find_map(|i| i.provenance.server_generation)
        .map(|g| format!(" gen={g}"))
        .unwrap_or_default();

    let diagnostics = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Diagnostic)
        .count();
    let refs = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Reference)
        .count();
    let defs = packet
        .items
        .iter()
        .filter(|i| {
            matches!(
                i.kind,
                LspContextItemKind::Definition | LspContextItemKind::Declaration
            )
        })
        .count();

    let truncated = if packet.truncation.bytes_truncated
        || packet.truncation.files_truncated
        || packet.truncation.references_truncated
    {
        ", truncated"
    } else {
        ""
    };

    format!(
        "LSP: {} | {}{} | {} diagnostics, {} refs, {} defs{}",
        match packet.mode {
            crate::context::LspContextPacketMode::Disabled => "disabled",
            crate::context::LspContextPacketMode::Opportunistic => "ready",
            crate::context::LspContextPacketMode::Required => "required",
        },
        server_id,
        gen,
        diagnostics,
        refs,
        defs,
        truncated,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{
        LineRange, LspContextPacketMode, LspContextRequest, LspContextScore, LspEvidenceFreshness,
        LspEvidenceProvenance, LspRiskMode,
    };
    use std::path::PathBuf;

    fn make_item(
        kind: LspContextItemKind,
        file: &str,
        line: Option<u32>,
        message: &str,
        hunk_local: bool,
    ) -> LspContextItem {
        LspContextItem {
            kind,
            file: PathBuf::from(file),
            range: line.map(|l| LineRange {
                start: l,
                end: l + 1,
            }),
            line,
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
                is_hunk_local: hunk_local,
                is_error: false,
                is_same_file: false,
                freshness_rank: 0,
            },
            payload: None,
        }
    }

    fn make_packet(
        items: Vec<LspContextItem>,
        previews: Vec<LspPreviewArtifact>,
    ) -> LspContextPacket {
        LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("a.rs")],
                hunks: Vec::new(),
                risk_mode: LspRiskMode::Standard,
            },
            items,
            previews,
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
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
    fn test_render_agent_context_sections() {
        let items = vec![
            make_item(
                LspContextItemKind::Diagnostic,
                "a.rs",
                Some(5),
                "error: unused",
                false,
            ),
            make_item(
                LspContextItemKind::Definition,
                "b.rs",
                Some(10),
                "def: foo",
                false,
            ),
            make_item(
                LspContextItemKind::Reference,
                "c.rs",
                Some(20),
                "ref: bar",
                false,
            ),
        ];
        let packet = make_packet(items, Vec::new());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);

        assert!(rendered.contains("## Diagnostics"));
        assert!(rendered.contains("## Definitions"));
        assert!(rendered.contains("## References"));
        assert!(rendered.contains("rust-analyzer"));
    }

    #[test]
    fn test_render_small_model_omits_references() {
        let items = vec![
            make_item(
                LspContextItemKind::Diagnostic,
                "a.rs",
                Some(0),
                "err",
                false,
            ),
            make_item(LspContextItemKind::Reference, "b.rs", Some(5), "ref", false),
            make_item(LspContextItemKind::Definition, "a.rs", Some(2), "def", true),
        ];
        let packet = make_packet(items, Vec::new());
        let config = LspContextRenderConfig {
            model_tier: ModelTier::Small,
            ..Default::default()
        };
        let rendered = render_lsp_context_for_agent(&packet, &config);

        assert!(rendered.contains("## Diagnostics"));
        assert!(rendered.contains("## Definitions (hunk-local)"));
        assert!(!rendered.contains("## References"));
        assert!(!rendered.contains("## Hover/Signature"));
    }

    #[test]
    fn test_render_status_line() {
        let items = vec![
            make_item(
                LspContextItemKind::Diagnostic,
                "a.rs",
                Some(0),
                "err",
                false,
            ),
            make_item(
                LspContextItemKind::Diagnostic,
                "a.rs",
                Some(1),
                "warn",
                false,
            ),
            make_item(LspContextItemKind::Reference, "b.rs", Some(0), "ref", false),
            make_item(
                LspContextItemKind::Definition,
                "c.rs",
                Some(0),
                "def",
                false,
            ),
        ];
        let packet = make_packet(items, Vec::new());
        let line = render_lsp_status_line(&packet);

        assert!(line.contains("LSP:"));
        assert!(line.contains("rust-analyzer"));
        assert!(line.contains("gen=3"));
        assert!(line.contains("2 diagnostics"));
        assert!(line.contains("1 refs"));
        assert!(line.contains("1 defs"));
    }

    #[test]
    fn test_render_empty_packet() {
        let packet = make_packet(Vec::new(), Vec::new());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);

        // Status line is always present.
        assert!(rendered.contains("LSP:"));
        // No other sections.
        assert!(!rendered.contains("## Diagnostics"));
        assert!(!rendered.contains("## Definitions"));
        assert!(!rendered.contains("## References"));
    }

    #[test]
    fn test_lifecycle_notes_starting() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("starting".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("LSP server is starting"));
    }

    #[test]
    fn test_lifecycle_notes_initializing() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("initializing".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("LSP server is initializing"));
    }

    #[test]
    fn test_lifecycle_notes_indexing() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("indexing".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("LSP server is indexing"));
        assert!(rendered.contains("may be incomplete"));
    }

    #[test]
    fn test_lifecycle_notes_degraded() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("degraded".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("LSP server is degraded"));
        assert!(rendered.contains("evidence may be partial"));
    }

    #[test]
    fn test_lifecycle_notes_restart_scheduled() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("restart_scheduled".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("restart is scheduled"));
    }

    #[test]
    fn test_lifecycle_notes_restarting() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("restarting".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("LSP server is restarting"));
        assert!(rendered.contains("treated as stale"));
    }

    #[test]
    fn test_lifecycle_notes_failed() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("failed".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("LSP server has failed"));
        assert!(rendered.contains("no fresh semantic evidence"));
    }

    #[test]
    fn test_lifecycle_notes_stopping() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("stopping".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("LSP server is stopping"));
    }

    #[test]
    fn test_lifecycle_notes_stopped() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("stopped".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("LSP server is stopped"));
    }

    #[test]
    fn test_lifecycle_notes_ready_absent() {
        let mut packet = make_packet(Vec::new(), Vec::new());
        packet.operational_state = Some("ready".to_string());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        // Ready state should not produce lifecycle notes.
        let notes_section = rendered.split("## Notes").nth(1).unwrap_or("");
        assert!(!notes_section.contains("LSP server is"));
    }

    #[test]
    fn test_lifecycle_notes_none_absent() {
        let packet = make_packet(Vec::new(), Vec::new());
        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        // No operational_state means no Notes section.
        assert!(!rendered.contains("## Notes"));
    }

    #[test]
    fn test_render_truncation_notes() {
        let items = vec![make_item(
            LspContextItemKind::Diagnostic,
            "a.rs",
            Some(0),
            "err",
            false,
        )];
        let mut packet = make_packet(items, Vec::new());
        packet
            .truncation
            .notes
            .push("truncated diagnostics from 20 to 10".to_string());

        let config = LspContextRenderConfig {
            include_truncation_notes: true,
            ..Default::default()
        };
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("truncated diagnostics"));

        let config_no_notes = LspContextRenderConfig {
            include_truncation_notes: false,
            ..Default::default()
        };
        let rendered_no_notes = render_lsp_context_for_agent(&packet, &config_no_notes);
        assert!(!rendered_no_notes.contains("truncated diagnostics"));
    }

    #[test]
    fn test_render_impact_analysis_section() {
        let items = vec![
            make_item(
                LspContextItemKind::Definition,
                "src/lib.rs",
                Some(10),
                "definition: fn foo",
                false,
            ),
            make_item(
                LspContextItemKind::Reference,
                "src/a.rs",
                Some(5),
                "reference: foo()",
                false,
            ),
            make_item(
                LspContextItemKind::Reference,
                "src/lib.rs",
                Some(15),
                "reference: foo()",
                false,
            ),
        ];
        let packet = LspContextPacket {
            request: LspContextRequest::ImpactAnalysis {
                symbol: crate::context::SymbolTarget {
                    file: PathBuf::from("src/lib.rs"),
                    position: lsp_types::Position {
                        line: 10,
                        character: 0,
                    },
                },
                changed_files: vec![PathBuf::from("src/a.rs")],
                max_refs: 20,
                max_files: 5,
                max_depth: 1,
            },
            items,
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
            truncation: Default::default(),
        };

        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("## Impact Analysis"));
        assert!(rendered.contains("Target Definition"));
        assert!(rendered.contains("Affected References"));
    }

    #[test]
    fn test_render_test_failure_section() {
        let items = vec![
            LspContextItem {
                kind: LspContextItemKind::Diagnostic,
                file: PathBuf::from("tests/foo.rs"),
                range: None,
                line: Some(20),
                column: None,
                message: "test_foo failed".to_string(),
                symbol: None,
                source: Some(AgentContextSource::Diagnostics),
                provenance: crate::context::LspEvidenceProvenance {
                    server_id: "test".to_string(),
                    server_generation: Some(1),
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
                    is_error: true,
                    is_same_file: true,
                    freshness_rank: 0,
                },
                payload: None,
            },
            LspContextItem {
                kind: LspContextItemKind::WorkspaceSymbol,
                file: PathBuf::from("tests/foo.rs"),
                range: None,
                line: Some(20),
                column: None,
                message: "test_foo (function)".to_string(),
                symbol: Some("test_foo".to_string()),
                source: Some(AgentContextSource::LspContext),
                provenance: crate::context::LspEvidenceProvenance {
                    server_id: "test".to_string(),
                    server_generation: Some(1),
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
                    is_same_file: true,
                    freshness_rank: 0,
                },
                payload: None,
            },
        ];
        let packet = LspContextPacket {
            request: LspContextRequest::TestFailureRepair {
                test_file: PathBuf::from("tests/foo.rs"),
                failure_message: "test_foo failed".to_string(),
                related_files: vec![PathBuf::from("src/foo.rs")],
            },
            items,
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
            truncation: Default::default(),
        };

        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("## Failure-linked Evidence"));
        assert!(rendered.contains("Test Diagnostics"));
        assert!(rendered.contains("Test Symbols"));
    }

    #[test]
    fn test_render_interface_boundary_section() {
        let items = vec![
            make_item(
                LspContextItemKind::WorkspaceSymbol,
                "src/api.rs",
                Some(5),
                "MyTrait (trait)",
                false,
            ),
            make_item(
                LspContextItemKind::Implementation,
                "src/impl.rs",
                Some(10),
                "implementation: (5:0)-(5:20)",
                false,
            ),
        ];
        let packet = LspContextPacket {
            request: LspContextRequest::InterfaceBoundary {
                file: PathBuf::from("src/api.rs"),
                symbol: None,
                include_implementations: true,
            },
            items,
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
            truncation: Default::default(),
        };

        let config = LspContextRenderConfig {
            model_tier: ModelTier::Frontier,
            ..Default::default()
        };
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("## Interface Boundary"));
        assert!(rendered.contains("Boundary Symbols"));
        assert!(rendered.contains("Implementations"));
    }

    #[test]
    fn test_render_cross_file_repair_section() {
        let items = vec![
            LspContextItem {
                kind: LspContextItemKind::Diagnostic,
                file: PathBuf::from("src/main.rs"),
                range: None,
                line: Some(5),
                column: None,
                message: "error in primary".to_string(),
                symbol: None,
                source: Some(AgentContextSource::Diagnostics),
                provenance: crate::context::LspEvidenceProvenance {
                    server_id: "test".to_string(),
                    server_generation: Some(1),
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
                    is_error: true,
                    is_same_file: true,
                    freshness_rank: 0,
                },
                payload: None,
            },
            LspContextItem {
                kind: LspContextItemKind::Diagnostic,
                file: PathBuf::from("src/lib.rs"),
                range: None,
                line: Some(10),
                column: None,
                message: "warning in related".to_string(),
                symbol: None,
                source: Some(AgentContextSource::Diagnostics),
                provenance: crate::context::LspEvidenceProvenance {
                    server_id: "test".to_string(),
                    server_generation: Some(1),
                    operation: "test".to_string(),
                    freshness: LspEvidenceFreshness::Fresh,
                    capability_decision: None,
                    document_version: None,
                    age_ms: None,
                    post_restart: false,
                },
                score: LspContextScore {
                    priority: 5,
                    is_hunk_local: false,
                    is_error: false,
                    is_same_file: false,
                    freshness_rank: 0,
                },
                payload: None,
            },
        ];
        let packet = LspContextPacket {
            request: LspContextRequest::CrossFileRepair {
                primary_file: PathBuf::from("src/main.rs"),
                related_files: vec![PathBuf::from("src/lib.rs")],
                ranges: vec![],
            },
            items,
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
            truncation: Default::default(),
        };

        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("## Cross-File Repair"));
        assert!(rendered.contains("Primary File Evidence"));
        assert!(rendered.contains("Related File Evidence"));
    }

    #[test]
    fn test_render_call_neighborhood_section() {
        let items = vec![
            make_item(
                LspContextItemKind::Reference,
                "src/caller.rs",
                Some(20),
                "callees (outgoing): (20:0)-(20:10)",
                false,
            ),
            make_item(
                LspContextItemKind::DocumentHighlight,
                "src/main.rs",
                Some(42),
                "caller highlight: (42:0)-(42:5)",
                false,
            ),
        ];
        let packet = LspContextPacket {
            request: LspContextRequest::CallNeighborhood {
                file: PathBuf::from("src/main.rs"),
                line: 42,
                column: 10,
                direction: crate::context::HierarchyDirection::Both,
                max_depth: 1,
                max_callers: 10,
                max_callees: 10,
            },
            items,
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
            truncation: Default::default(),
        };

        let config = LspContextRenderConfig::default();
        let rendered = render_lsp_context_for_agent(&packet, &config);
        assert!(rendered.contains("## Call Neighborhood"));
        assert!(rendered.contains("Outgoing Calls"));
        assert!(rendered.contains("Incoming Calls"));
    }
}
