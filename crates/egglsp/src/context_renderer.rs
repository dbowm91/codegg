//! Agent context renderer.
//!
//! Renders [`LspContextPacket`] into concise, readable text blocks
//! suitable for agent prompts. No raw JSON is ever dumped.

use crate::context::{LspContextItem, LspContextItemKind, LspContextPacket, LspPreviewArtifact};

// ---------------------------------------------------------------------------
// Model tier
// ---------------------------------------------------------------------------

/// Controls how much LSP context is rendered into agent prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ModelTier {
    /// Minimal: diagnostics + hunk-local definitions only.
    Small,
    /// Standard: diagnostics + references + hover.
    Workhorse,
    /// Broader: all available evidence.
    Frontier,
}

impl Default for ModelTier {
    fn default() -> Self {
        Self::Workhorse
    }
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
    for preview in &packet.previews {
        let label = match preview {
            LspPreviewArtifact::Rename(desc) => format!("Rename: {desc}"),
            LspPreviewArtifact::Formatting(desc) => format!("Formatting: {desc}"),
            LspPreviewArtifact::CodeAction(desc) => format!("CodeAction: {desc}"),
        };
        lines.push(format!("{label} — Preview only; not applied."));
    }
    Some(format!("## Previews\n{}", lines.join("\n")))
}

fn build_notes_section(
    packet: &LspContextPacket,
    config: &LspContextRenderConfig,
) -> Option<String> {
    let mut notes = packet.notes.clone();

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
        LspContextPacketMode, LspContextRequest, LspContextScore, LspEvidenceFreshness,
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
            line,
            column: None,
            message: message.to_string(),
            symbol: None,
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
            mode: LspContextPacketMode::Opportunistic,
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
}
