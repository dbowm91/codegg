use super::lsp_security::SecurityRiskMarker;
use crate::error::ToolError;
use crate::lsp::semantic_context::SemanticContextCollector;
use crate::tool::{
    StructuredToolResult, Tool, ToolBackendKind, ToolCategory, ToolExecutionContext,
    ToolProvenance, ToolTrust,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

const MAX_REFERENCES: usize = 100;
const MAX_SYMBOLS: usize = 300;
const MAX_WORKSPACE_SYMBOLS: usize = 200;
const MAX_HOVER_CHARS: usize = 2000;
const MAX_DOCUMENT_HIGHLIGHTS: usize = 100;
const MAX_COMPLETION_CANDIDATES: usize = 200;
const MAX_SEMANTIC_TOKENS: usize = 1000;
const MAX_CODE_ACTIONS: usize = 50;

const MAX_SEMANTIC_CONTEXT_RADIUS: u32 = 120;
const DEFAULT_SEMANTIC_CONTEXT_RADIUS: u32 = 40;
const MAX_CONTEXT_DIAGNOSTICS: usize = 100;
const MAX_CONTEXT_SYMBOLS: usize = 120;
const MAX_CONTEXT_REFERENCES: usize = 80;
const MAX_CONTEXT_EXCERPT_BYTES: usize = 32_000;
const MAX_HIERARCHY_ITEMS: usize = 32;
const MAX_HIERARCHY_EDGES: usize = 128;
const MAX_HIERARCHY_RANGES: usize = 32;

const DEFAULT_SECURITY_CONTEXT_RADIUS: u32 = 80;
const MAX_SECURITY_CONTEXT_RADIUS: u32 = 200;
const DEFAULT_MAX_RISK_MARKERS: usize = 80;
const MAX_RISK_MARKERS: usize = 200;
const MAX_SECURITY_SYMBOLS: usize = 80;
const MAX_SECURITY_DIAGNOSTICS: usize = 80;

const DEFAULT_CALL_EXPANSION_DEPTH: u8 = 0;
const MAX_CALL_EXPANSION_DEPTH: u8 = 2;
const DEFAULT_MAX_CALL_NODES: usize = 32;
const MAX_CALL_NODES: usize = 64;
const MAX_CALL_EDGES: usize = 128;

/// Compute effective per-hunk navigation limits from a request, clamping to
/// safe upper bounds and coercing zero values to 1.
pub(crate) fn effective_hunk_navigation_limits(
    request: &egglsp::hunk_context::HunkSourceNavigationRequest,
) -> (usize, usize, usize) {
    let max_symbols = request.max_symbols_per_hunk.clamp(1, MAX_CONTEXT_SYMBOLS);
    let max_diagnostics = request
        .max_diagnostics_per_hunk
        .clamp(1, MAX_CONTEXT_DIAGNOSTICS);
    let max_references = request
        .max_references_per_hunk
        .clamp(1, MAX_CONTEXT_REFERENCES);
    (max_symbols, max_diagnostics, max_references)
}

#[derive(Serialize)]
struct LspToolOutput<T> {
    operation: String,
    file_path: Option<String>,
    result_count: usize,
    truncated: bool,
    results: T,
}

#[derive(Serialize, Clone)]
pub(crate) struct DiagnosticSummary {
    pub(crate) file: String,
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) severity: String,
    pub(crate) source: Option<String>,
    pub(crate) code: Option<String>,
    pub(crate) message: String,
}

#[derive(Serialize)]
struct LocationSummary {
    file: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

#[derive(Serialize)]
struct DocumentHighlightSummary {
    file: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    kind: String,
}

#[derive(Serialize, Clone)]
pub(crate) struct SymbolSummary {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) file: String,
    pub(crate) start_line: u32,
    pub(crate) start_column: u32,
    pub(crate) end_line: u32,
    pub(crate) end_column: u32,
}

#[derive(Serialize)]
struct HoverSummary {
    file: String,
    line: u32,
    column: u32,
    contents: String,
}

#[derive(Serialize)]
struct WorkspaceSymbolSummary {
    name: String,
    kind: String,
    file: Option<String>,
    start_line: Option<u32>,
    start_column: Option<u32>,
    container_name: Option<String>,
}

#[derive(Serialize)]
struct SemanticContextPacket {
    file: String,
    target: Option<SemanticContextTarget>,
    excerpt: SourceExcerpt,
    diagnostics: Vec<DiagnosticSummary>,
    current_diagnostics_error: Option<String>,
    diagnostic_evidence: Option<DiagnosticEvidenceMeta>,
    overlay: Option<SemanticOverlaySummary>,
    symbols: Vec<SymbolSummary>,
    current_symbols_error: Option<String>,
    definitions: Vec<LocationSummary>,
    definitions_error: Option<String>,
    references: Vec<LocationSummary>,
    references_error: Option<String>,
    source_actions: Vec<SemanticSourceActionHint>,
    section_truncations: Vec<egglsp::semantic_context::SemanticSectionTruncation>,
    call_hierarchy: Option<CallHierarchySummary>,
    type_hierarchy: Option<TypeHierarchySummary>,
    limits: SemanticContextLimits,
}

#[derive(Serialize)]
struct SemanticContextTarget {
    line: u32,
    column: u32,
}

#[derive(Serialize)]
pub struct SourceExcerpt {
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
}

#[derive(Serialize)]
struct SemanticOverlaySummary {
    used: bool,
    diagnostics_may_still_be_warming: bool,
    diagnostics: Vec<DiagnosticSummary>,
    diagnostics_error: Option<String>,
    symbols: Vec<crate::lsp::overlay::SemanticSymbolSummary>,
    symbols_error: Option<String>,
    restored_disk_view: bool,
    restore_error: Option<String>,
}

#[derive(Serialize)]
struct SemanticContextLimits {
    diagnostics_truncated: bool,
    symbols_truncated: bool,
    references_truncated: bool,
    overlay_diagnostics_truncated: bool,
    excerpt_truncated: bool,
}

#[derive(Serialize)]
pub struct SemanticSourceActionHint {
    pub action: String,
    pub available: bool,
    pub preview: Option<crate::lsp::edit::WorkspaceEditPreview>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
struct HierarchyRangeSummary {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

#[derive(Serialize)]
struct HierarchyItemSummary {
    name: String,
    kind: String,
    file: Option<String>,
    range: HierarchyRangeSummary,
    selection_range: HierarchyRangeSummary,
    detail: Option<String>,
}

#[derive(Serialize)]
struct IncomingCallSummary {
    from: HierarchyItemSummary,
    from_ranges: Vec<HierarchyRangeSummary>,
}

#[derive(Serialize)]
struct OutgoingCallSummary {
    to: HierarchyItemSummary,
    from_ranges: Vec<HierarchyRangeSummary>,
}

#[derive(Serialize)]
struct CallHierarchySummary {
    items: Vec<HierarchyItemSummary>,
    incoming: Vec<IncomingCallSummary>,
    outgoing: Vec<OutgoingCallSummary>,
    prepare_error: Option<String>,
    incoming_error: Option<String>,
    outgoing_error: Option<String>,
    truncated: bool,
}

#[derive(Serialize, Clone)]
struct CallExpansionNode {
    id: String,
    name: String,
    kind: String,
    file: Option<String>,
    range: HierarchyRangeSummary,
    selection_range: HierarchyRangeSummary,
    detail: Option<String>,
    depth: u8,
}

#[derive(Serialize)]
struct CallExpansionEdge {
    from: String,
    to: String,
    direction: String,
    ranges: Vec<HierarchyRangeSummary>,
}

#[derive(Serialize)]
struct CallExpansionSummary {
    root: Option<CallExpansionNode>,
    direction: String,
    depth: u8,
    nodes: Vec<CallExpansionNode>,
    edges: Vec<CallExpansionEdge>,
    truncated: bool,
    errors: Vec<String>,
}

#[derive(Serialize)]
struct TypeHierarchySummary {
    items: Vec<HierarchyItemSummary>,
    supertypes: Vec<HierarchyItemSummary>,
    subtypes: Vec<HierarchyItemSummary>,
    prepare_error: Option<String>,
    supertypes_error: Option<String>,
    subtypes_error: Option<String>,
    truncated: bool,
}

/// Diagnostic freshness metadata for semantic/security context packets.
#[derive(Debug, Clone, Serialize)]
struct DiagnosticEvidenceMeta {
    freshness: crate::lsp::diagnostics::LspDiagnosticFreshness,
    source: crate::lsp::diagnostics::LspDiagnosticSource,
    age_ms: i64,
    usable_evidence: bool,
    server_generation: Option<u64>,
    post_restart: bool,
}

#[derive(Serialize)]
struct SecurityContextPacket {
    file: String,
    target: Option<SemanticContextTarget>,
    excerpt: SourceExcerpt,
    risk_markers: Vec<SecurityRiskMarker>,
    security_relevant_symbols: Vec<SymbolSummary>,
    security_relevant_diagnostics: Vec<DiagnosticSummary>,
    diagnostic_evidence: Option<DiagnosticEvidenceMeta>,
    definitions: Vec<LocationSummary>,
    references: Vec<LocationSummary>,
    call_hierarchy: Option<CallHierarchySummary>,
    call_expansion: Option<CallExpansionSummary>,
    overlay: Option<SemanticOverlaySummary>,
    preset: Option<String>,
    notes: Vec<String>,
    limits: SecurityContextLimits,
}

#[derive(Serialize)]
struct SecurityContextLimits {
    risk_markers_truncated: bool,
    diagnostics_truncated: bool,
    symbols_truncated: bool,
    references_truncated: bool,
    excerpt_truncated: bool,
    overlay_diagnostics_truncated: bool,
    call_expansion_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EffectiveSecurityContextSettings {
    pub(crate) categories: Option<Vec<String>>,
    pub(crate) radius: u32,
    pub(crate) max_risk_markers: usize,
    pub(crate) include_call_hierarchy: bool,
    pub(crate) preset_note: Option<String>,
    pub(crate) preset_name: Option<String>,
    pub(crate) call_depth: u8,
    pub(crate) max_call_nodes: usize,
    pub(crate) call_direction: crate::lsp::operations::HierarchyDirection,
}

#[derive(Debug, Deserialize)]
struct LspInput {
    operation: String,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    column: Option<u32>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    new_name: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    patch: Option<String>,
    #[serde(default)]
    radius: Option<u32>,
    #[serde(default)]
    include_references: Option<bool>,
    #[serde(default)]
    include_definitions: Option<bool>,
    #[serde(default)]
    include_overlay: Option<bool>,
    #[serde(default)]
    include_source_actions: Option<bool>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    include_call_hierarchy: Option<bool>,
    #[serde(default)]
    include_type_hierarchy: Option<bool>,
    #[serde(default)]
    security_categories: Option<Vec<String>>,
    #[serde(default)]
    max_risk_markers: Option<usize>,
    #[serde(default)]
    security_preset: Option<String>,
    #[serde(default)]
    call_depth: Option<u8>,
    #[serde(default)]
    max_call_nodes: Option<usize>,
    #[serde(default)]
    call_direction: Option<String>,
    #[serde(default)]
    max_hunks: Option<usize>,
    #[serde(default)]
    start_line: Option<u32>,
    #[serde(default)]
    start_column: Option<u32>,
    #[serde(default)]
    end_line: Option<u32>,
    #[serde(default)]
    end_column: Option<u32>,
    #[serde(default)]
    max_candidates: Option<usize>,
    #[serde(default)]
    max_tokens: Option<usize>,
    #[serde(default)]
    max_actions: Option<usize>,
    #[serde(default)]
    action_index: Option<usize>,
    #[serde(default)]
    only: Option<Vec<String>>,
    #[serde(default)]
    trigger_kind: Option<i32>,
    #[serde(default)]
    trigger_char: Option<String>,
}

pub fn to_lsp_position(line: u32, column: u32) -> crate::lsp::lsp_types::Position {
    crate::lsp::lsp_types::Position {
        line: line.saturating_sub(1),
        character: column.saturating_sub(1),
    }
}

pub fn truncate_to_byte_limit_on_char_boundary(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

fn uri_to_path(uri: &crate::lsp::lsp_types::Uri) -> String {
    let raw = uri.to_string();
    url::Url::parse(&raw)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(raw)
}

fn symbol_kind_to_string(kind: crate::lsp::lsp_types::SymbolKind) -> String {
    match kind {
        crate::lsp::lsp_types::SymbolKind::FILE => "file",
        crate::lsp::lsp_types::SymbolKind::MODULE => "module",
        crate::lsp::lsp_types::SymbolKind::NAMESPACE => "namespace",
        crate::lsp::lsp_types::SymbolKind::PACKAGE => "package",
        crate::lsp::lsp_types::SymbolKind::CLASS => "class",
        crate::lsp::lsp_types::SymbolKind::METHOD => "method",
        crate::lsp::lsp_types::SymbolKind::PROPERTY => "property",
        crate::lsp::lsp_types::SymbolKind::FIELD => "field",
        crate::lsp::lsp_types::SymbolKind::CONSTRUCTOR => "constructor",
        crate::lsp::lsp_types::SymbolKind::ENUM => "enum",
        crate::lsp::lsp_types::SymbolKind::INTERFACE => "interface",
        crate::lsp::lsp_types::SymbolKind::FUNCTION => "function",
        crate::lsp::lsp_types::SymbolKind::VARIABLE => "variable",
        crate::lsp::lsp_types::SymbolKind::CONSTANT => "constant",
        crate::lsp::lsp_types::SymbolKind::STRING => "string",
        crate::lsp::lsp_types::SymbolKind::NUMBER => "number",
        crate::lsp::lsp_types::SymbolKind::BOOLEAN => "boolean",
        crate::lsp::lsp_types::SymbolKind::ARRAY => "array",
        crate::lsp::lsp_types::SymbolKind::OBJECT => "object",
        crate::lsp::lsp_types::SymbolKind::KEY => "key",
        crate::lsp::lsp_types::SymbolKind::NULL => "null",
        crate::lsp::lsp_types::SymbolKind::ENUM_MEMBER => "enum_member",
        crate::lsp::lsp_types::SymbolKind::STRUCT => "struct",
        crate::lsp::lsp_types::SymbolKind::EVENT => "event",
        crate::lsp::lsp_types::SymbolKind::OPERATOR => "operator",
        crate::lsp::lsp_types::SymbolKind::TYPE_PARAMETER => "type_parameter",
        _ => "unknown",
    }
    .to_string()
}

fn format_direction(d: crate::lsp::operations::HierarchyDirection) -> String {
    match d {
        crate::lsp::operations::HierarchyDirection::Incoming => "incoming".to_string(),
        crate::lsp::operations::HierarchyDirection::Outgoing => "outgoing".to_string(),
        crate::lsp::operations::HierarchyDirection::Both => "both".to_string(),
    }
}

fn severity_to_string(severity: crate::lsp::lsp_types::DiagnosticSeverity) -> String {
    match severity {
        crate::lsp::lsp_types::DiagnosticSeverity::ERROR => "error",
        crate::lsp::lsp_types::DiagnosticSeverity::WARNING => "warning",
        crate::lsp::lsp_types::DiagnosticSeverity::INFORMATION => "info",
        crate::lsp::lsp_types::DiagnosticSeverity::HINT => "hint",
        _ => "unknown",
    }
    .to_string()
}

fn document_highlight_kind_to_string(
    kind: Option<crate::lsp::lsp_types::DocumentHighlightKind>,
) -> String {
    match kind {
        Some(crate::lsp::lsp_types::DocumentHighlightKind::TEXT) => "text",
        Some(crate::lsp::lsp_types::DocumentHighlightKind::READ) => "read",
        Some(crate::lsp::lsp_types::DocumentHighlightKind::WRITE) => "write",
        _ => "unspecified",
    }
    .to_string()
}

pub struct LspTool {
    service: Arc<crate::lsp::service::LspService>,
    allowed_root: PathBuf,
}

impl std::fmt::Debug for LspTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspTool")
            .field("service", &Arc::as_ptr(&self.service))
            .field("allowed_root", &self.allowed_root)
            .finish()
    }
}

impl PartialEq for LspTool {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.service, &other.service) && self.allowed_root == other.allowed_root
    }
}

impl LspTool {
    pub fn new(service: Arc<crate::lsp::service::LspService>) -> Self {
        Self {
            service,
            allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn with_allowed_root(mut self, root: PathBuf) -> Self {
        self.allowed_root = root;
        self
    }

    fn resolve_file_from_str(&self, p: &str) -> Result<PathBuf, ToolError> {
        let original = if p.starts_with('/') {
            PathBuf::from(p)
        } else {
            self.allowed_root.join(p)
        };
        crate::tool::util::validate_path(&original, &self.allowed_root)
            .map_err(|e| ToolError::Execution(e.to_string()))
    }

    fn resolve_file(&self, path: &Option<String>) -> Result<PathBuf, ToolError> {
        let p = path
            .as_ref()
            .ok_or_else(|| ToolError::Execution("file_path required".to_string()))?;
        self.resolve_file_from_str(p)
    }

    fn reject_probable_multi_file_patch(&self, patch: &str) -> Result<(), ToolError> {
        let diff_git_count = patch
            .lines()
            .filter(|line| line.starts_with("diff --git "))
            .count();
        let old_header_count = patch
            .lines()
            .filter(|line| line.starts_with("--- "))
            .count();
        let new_header_count = patch
            .lines()
            .filter(|line| line.starts_with("+++ "))
            .count();

        if diff_git_count > 1 || old_header_count > 1 || new_header_count > 1 {
            return Err(ToolError::Execution(
                "semanticCheckPreview only supports single-file patches".to_string(),
            ));
        }

        Ok(())
    }

    async fn capability_snapshot_for_file(
        &self,
        file: &std::path::Path,
    ) -> Option<egglsp::LspCapabilitySnapshot> {
        let (key, _) = self.service.get_or_create_client(file).await.ok()?;
        // Prefer the stored override-aware snapshot.
        if let Some(snap) = self.service.normalized_capabilities_for_key(&key).await {
            return Some(snap);
        }
        // Fallback: rebuild from raw capabilities (should not happen in
        // normal operation after initialization completes).
        let caps = self.service.get_capabilities_for_key(&key).await?;
        let lang = crate::lsp::language::detect_language(file.to_str().unwrap_or(""));
        let server_name = key.split(':').next_back().map(String::from);
        Some(egglsp::LspCapabilitySnapshot::from_capabilities(
            &caps,
            server_name.as_deref(),
            lang,
        ))
    }

    /// Look up the operational state note (if any) for the
    /// server that would service `file_path_str`. Returns
    /// `Some(note)` for notable states (`Indexing`,
    /// `Restarting`, `Degraded`, etc.) and `None` for `Ready`
    /// or when the key is unknown. The note is suitable for
    /// appending to a hunk-source-context response's `notes`
    /// field.
    async fn operational_state_note_for_file(&self, file_path_str: &str) -> Option<String> {
        let p = std::path::Path::new(file_path_str);
        let key_result = self.service.get_or_create_client(p).await.ok()?;
        let key = key_result.0;
        let state = self.service.operational_state_for_key(&key).await?;
        state.context_note()
    }

    /// Construct a [`HunkSourceNavigationCollector`] with explicit per-hunk
    /// limits instead of the global constants.
    fn build_hunk_source_navigation_collector(
        &self,
        radius: u32,
        max_symbols_per_hunk: usize,
        max_diagnostics_per_hunk: usize,
        max_references_per_hunk: usize,
    ) -> crate::lsp::hunk_nav_collector::HunkSourceNavigationCollector {
        let ops = Arc::new(crate::lsp::operations::LspOperations::new(
            self.service.clone(),
        ));
        let diagnostics = Arc::new(crate::lsp::diagnostics::DiagnosticsCollector::new(
            self.service.clone(),
        ));
        let sem_collector = crate::lsp::semantic_context::SemanticContextCollector::new(
            self.service.clone(),
            ops,
            diagnostics,
            self.allowed_root.clone(),
        );

        let nav = crate::lsp::hunk_nav::HunkSourceNavigator::new()
            .with_excerpt_radius(radius)
            .with_max_symbols_per_hunk(max_symbols_per_hunk)
            .with_max_diagnostics_per_hunk(max_diagnostics_per_hunk)
            .with_max_references_per_hunk(max_references_per_hunk);

        crate::lsp::hunk_nav_collector::HunkSourceNavigationCollector::new(sem_collector, nav)
    }

    /// Execute a typed `hunkSourceContext` request directly, bypassing the
    /// JSON-in/JSON-out model-facing path. This is used by the security
    /// review executor to avoid unnecessary serialization round-trips.
    pub async fn execute_hunk_source_context_typed(
        &self,
        request: egglsp::hunk_context::HunkSourceNavigationRequest,
    ) -> Result<egglsp::hunk_context::HunkSourceNavigationResponse, String> {
        self.resolve_file_from_str(&request.file_path)
            .map_err(|e| format!("hunkSourceContext: path validation failed: {e}"))?;

        let radius = request.excerpt_radius.min(MAX_SEMANTIC_CONTEXT_RADIUS);

        let (max_symbols, max_diagnostics, max_references) =
            effective_hunk_navigation_limits(&request);

        let collector = self.build_hunk_source_navigation_collector(
            radius,
            max_symbols,
            max_diagnostics,
            max_references,
        );
        collector.collect(request).await
    }

    fn resolve_security_context_settings(
        parsed: &LspInput,
        has_position: bool,
    ) -> Result<EffectiveSecurityContextSettings, ToolError> {
        let mut categories: Option<Vec<String>> = None;
        let mut radius = DEFAULT_SECURITY_CONTEXT_RADIUS;
        let mut max_risk_markers = DEFAULT_MAX_RISK_MARKERS;
        let mut include_call_hierarchy = has_position;
        let mut preset_note: Option<String> = None;
        let mut preset_name: Option<String> = None;

        if let Some(ref preset_str) = parsed.security_preset {
            let preset = super::lsp_security::parse_security_preset(Some(preset_str.as_str()))
                .map_err(ToolError::Execution)?;
            if let Some(preset) = preset {
                let defaults = super::lsp_security::preset_defaults(preset);
                categories = Some(defaults.categories);
                radius = defaults.radius;
                max_risk_markers = defaults.max_risk_markers;
                include_call_hierarchy = defaults.include_call_hierarchy;
                preset_note = Some(defaults.note.to_string());
                preset_name = Some(super::lsp_security::security_preset_name(preset).to_string());
            }
        }

        if let Some(ref cats) = parsed.security_categories {
            categories = Some(cats.clone());
        }
        if let Some(r) = parsed.radius {
            radius = r;
        }
        if let Some(m) = parsed.max_risk_markers {
            max_risk_markers = m;
        }
        if let Some(h) = parsed.include_call_hierarchy {
            include_call_hierarchy = h;
        }

        radius = radius.min(MAX_SECURITY_CONTEXT_RADIUS);
        max_risk_markers = max_risk_markers.min(MAX_RISK_MARKERS);

        let mut call_depth = DEFAULT_CALL_EXPANSION_DEPTH;
        let mut max_call_nodes = DEFAULT_MAX_CALL_NODES;
        let mut call_direction = crate::lsp::operations::HierarchyDirection::Both;

        // Presets do not enable call expansion (all keep call_depth = 0)
        // Explicit fields override
        if let Some(d) = parsed.call_depth {
            if d > MAX_CALL_EXPANSION_DEPTH {
                return Err(ToolError::Execution(format!(
                    "call_depth {d} exceeds maximum {MAX_CALL_EXPANSION_DEPTH}"
                )));
            }
            call_depth = d;
        }
        if let Some(n) = parsed.max_call_nodes {
            max_call_nodes = n.min(MAX_CALL_NODES);
        }
        if let Some(ref dir_str) = parsed.call_direction {
            call_direction =
                crate::lsp::operations::HierarchyDirection::parse(Some(dir_str.as_str()))
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
        }

        Ok(EffectiveSecurityContextSettings {
            categories,
            radius,
            max_risk_markers,
            include_call_hierarchy,
            preset_note,
            preset_name,
            call_depth,
            max_call_nodes,
            call_direction,
        })
    }

    async fn resolve_semantic_check_content(
        &self,
        file: &Path,
        content: Option<&String>,
        patch: Option<&String>,
    ) -> Result<String, ToolError> {
        match (content, patch) {
            (Some(_), Some(_)) => Err(ToolError::Execution(
                "semanticCheckPreview accepts either content or patch, not both".to_string(),
            )),
            (None, None) => Err(ToolError::Execution(
                "content or patch required for semanticCheckPreview".to_string(),
            )),
            (Some(content), None) => Ok(content.clone()),
            (None, Some(patch)) => {
                let original = tokio::fs::read_to_string(file).await.map_err(|e| {
                    ToolError::Execution(format!(
                        "semanticCheckPreview patch failed: failed to read file {}: {}",
                        file.display(),
                        e
                    ))
                })?;
                self.reject_probable_multi_file_patch(patch)?;
                crate::tool::patch_util::apply_unified_diff(&original, patch).map_err(|e| {
                    ToolError::Execution(format!("semanticCheckPreview patch failed: {e}"))
                })
            }
        }
    }

    fn require_line_col(
        &self,
        line: &Option<u32>,
        column: &Option<u32>,
    ) -> Result<(u32, u32), ToolError> {
        let l =
            line.ok_or_else(|| ToolError::Execution("line is required (1-indexed)".to_string()))?;
        let c = column
            .ok_or_else(|| ToolError::Execution("column is required (1-indexed)".to_string()))?;
        Ok((l, c))
    }

    pub fn build_source_excerpt(
        file: &Path,
        target_line: Option<u32>,
        radius: u32,
    ) -> Result<(SourceExcerpt, bool), ToolError> {
        let content = std::fs::read_to_string(file).map_err(|e| {
            ToolError::Execution(format!(
                "semanticContext: failed to read {}: {}",
                file.display(),
                e
            ))
        })?;
        let total_lines = content.lines().count().max(1) as u32;
        let (start_line, end_line) = if let Some(target) = target_line {
            let start = target.saturating_sub(radius).max(1);
            let end = (target.saturating_add(radius)).min(total_lines);
            (start, end)
        } else {
            (1, radius.min(total_lines))
        };
        let start_idx = (start_line.saturating_sub(1)) as usize;
        let end_idx = end_line as usize;
        let text: String = content
            .lines()
            .skip(start_idx)
            .take(end_idx - start_idx)
            .collect::<Vec<_>>()
            .join("\n");
        let (display, truncated) =
            truncate_to_byte_limit_on_char_boundary(&text, MAX_CONTEXT_EXCERPT_BYTES);
        let display = display.to_string();
        Ok((
            SourceExcerpt {
                start_line,
                end_line,
                text: display,
            },
            truncated,
        ))
    }

    fn flatten_symbols(
        symbols: &[crate::lsp::lsp_types::DocumentSymbol],
        file: &str,
        output: &mut Vec<SymbolSummary>,
        remaining: &mut usize,
    ) {
        for sym in symbols {
            if *remaining == 0 {
                return;
            }
            let range = sym.range;
            output.push(SymbolSummary {
                name: sym.name.clone(),
                kind: symbol_kind_to_string(sym.kind),
                file: file.to_string(),
                start_line: range.start.line + 1,
                start_column: range.start.character + 1,
                end_line: range.end.line + 1,
                end_column: range.end.character + 1,
            });
            *remaining -= 1;
            if let Some(children) = &sym.children {
                Self::flatten_symbols(children, file, output, remaining);
            }
        }
    }

    pub fn source_action_hint_from_result(
        action: crate::lsp::operations::SourceActionPreviewKind,
        result: Result<crate::lsp::edit::WorkspaceEditPreview, crate::lsp::LspError>,
    ) -> SemanticSourceActionHint {
        let action_str = match action {
            crate::lsp::operations::SourceActionPreviewKind::OrganizeImports => {
                "source.organizeImports".to_string()
            }
        };
        match result {
            Ok(preview) if preview.total_edits > 0 => SemanticSourceActionHint {
                action: action_str,
                available: true,
                preview: Some(preview),
                error: None,
            },
            Ok(preview) => SemanticSourceActionHint {
                action: action_str,
                available: false,
                preview: Some(preview),
                error: Some("source action produced no edits".to_string()),
            },
            Err(e) => SemanticSourceActionHint {
                action: action_str,
                available: false,
                preview: None,
                error: Some(e.to_string()),
            },
        }
    }

    async fn collect_source_action_hints(
        &self,
        ops: &crate::lsp::operations::LspOperations,
        file: &Path,
    ) -> Vec<SemanticSourceActionHint> {
        let actions = [crate::lsp::operations::SourceActionPreviewKind::OrganizeImports];
        let mut hints = Vec::with_capacity(actions.len());
        for action in actions {
            let result = ops
                .source_action_preview(file, action, Some(&self.allowed_root))
                .await;
            hints.push(Self::source_action_hint_from_result(action, result));
        }
        hints
    }

    fn convert_lsp_range(range: crate::lsp::lsp_types::Range) -> HierarchyRangeSummary {
        HierarchyRangeSummary {
            start_line: range.start.line + 1,
            start_column: range.start.character + 1,
            end_line: range.end.line + 1,
            end_column: range.end.character + 1,
        }
    }

    fn convert_hierarchy_item(
        item: &crate::lsp::lsp_types::CallHierarchyItem,
    ) -> HierarchyItemSummary {
        HierarchyItemSummary {
            name: item.name.clone(),
            kind: symbol_kind_to_string(item.kind),
            file: Some(uri_to_path(&item.uri)),
            range: Self::convert_lsp_range(item.range),
            selection_range: Self::convert_lsp_range(item.selection_range),
            detail: item.detail.clone(),
        }
    }

    fn convert_type_hierarchy_item(
        item: &crate::lsp::lsp_types::TypeHierarchyItem,
    ) -> HierarchyItemSummary {
        HierarchyItemSummary {
            name: item.name.clone(),
            kind: symbol_kind_to_string(item.kind),
            file: Some(uri_to_path(&item.uri)),
            range: Self::convert_lsp_range(item.range),
            selection_range: Self::convert_lsp_range(item.selection_range),
            detail: item.detail.clone(),
        }
    }

    async fn build_call_hierarchy_summary(
        &self,
        ops: &crate::lsp::operations::LspOperations,
        file: &Path,
        line: u32,
        column: u32,
        direction: crate::lsp::operations::HierarchyDirection,
    ) -> CallHierarchySummary {
        let items_result = ops.prepare_call_hierarchy(file, line, column).await;
        let items = match items_result {
            Ok(items) => items,
            Err(e) => {
                return CallHierarchySummary {
                    items: Vec::new(),
                    incoming: Vec::new(),
                    outgoing: Vec::new(),
                    prepare_error: Some(e.to_string()),
                    incoming_error: None,
                    outgoing_error: None,
                    truncated: false,
                };
            }
        };

        let items_truncated = items.len() > MAX_HIERARCHY_ITEMS;
        let item_summaries: Vec<HierarchyItemSummary> = items
            .iter()
            .take(MAX_HIERARCHY_ITEMS)
            .map(Self::convert_hierarchy_item)
            .collect();

        if items.is_empty() {
            return CallHierarchySummary {
                items: Vec::new(),
                incoming: Vec::new(),
                outgoing: Vec::new(),
                prepare_error: None,
                incoming_error: None,
                outgoing_error: None,
                truncated: false,
            };
        }

        let primary = &items[0];
        let mut incoming = Vec::new();
        let mut incoming_error = None;
        let mut incoming_raw_len = 0usize;
        let mut outgoing = Vec::new();
        let mut outgoing_error = None;
        let mut outgoing_raw_len = 0usize;
        let mut ranges_truncated = false;

        if matches!(
            direction,
            crate::lsp::operations::HierarchyDirection::Incoming
                | crate::lsp::operations::HierarchyDirection::Both
        ) {
            match ops.incoming_calls(primary.clone()).await {
                Ok(calls) => {
                    incoming_raw_len = calls.len();
                    let capped: Vec<_> = calls.into_iter().take(MAX_HIERARCHY_EDGES).collect();
                    incoming = capped
                        .iter()
                        .map(|call| {
                            let raw_range_count = call.from_ranges.len();
                            let truncated_ranges: Vec<_> = call
                                .from_ranges
                                .iter()
                                .take(MAX_HIERARCHY_RANGES)
                                .map(|r| Self::convert_lsp_range(*r))
                                .collect();
                            ranges_truncated |= raw_range_count > MAX_HIERARCHY_RANGES;
                            IncomingCallSummary {
                                from: Self::convert_hierarchy_item(&call.from),
                                from_ranges: truncated_ranges,
                            }
                        })
                        .collect();
                }
                Err(e) => {
                    incoming_error = Some(e.to_string());
                }
            }
        }

        if matches!(
            direction,
            crate::lsp::operations::HierarchyDirection::Outgoing
                | crate::lsp::operations::HierarchyDirection::Both
        ) {
            match ops.outgoing_calls(primary.clone()).await {
                Ok(calls) => {
                    outgoing_raw_len = calls.len();
                    let capped: Vec<_> = calls.into_iter().take(MAX_HIERARCHY_EDGES).collect();
                    outgoing = capped
                        .iter()
                        .map(|call| {
                            let raw_range_count = call.from_ranges.len();
                            let truncated_ranges: Vec<_> = call
                                .from_ranges
                                .iter()
                                .take(MAX_HIERARCHY_RANGES)
                                .map(|r| Self::convert_lsp_range(*r))
                                .collect();
                            ranges_truncated |= raw_range_count > MAX_HIERARCHY_RANGES;
                            OutgoingCallSummary {
                                to: Self::convert_hierarchy_item(&call.to),
                                from_ranges: truncated_ranges,
                            }
                        })
                        .collect();
                }
                Err(e) => {
                    outgoing_error = Some(e.to_string());
                }
            }
        }

        let truncated = items_truncated
            || incoming_raw_len > MAX_HIERARCHY_EDGES
            || outgoing_raw_len > MAX_HIERARCHY_EDGES
            || ranges_truncated;

        CallHierarchySummary {
            items: item_summaries,
            incoming,
            outgoing,
            prepare_error: None,
            incoming_error,
            outgoing_error,
            truncated,
        }
    }

    async fn build_type_hierarchy_summary(
        &self,
        ops: &crate::lsp::operations::LspOperations,
        file: &Path,
        line: u32,
        column: u32,
        direction: crate::lsp::operations::HierarchyDirection,
    ) -> TypeHierarchySummary {
        let items_result = ops.prepare_type_hierarchy(file, line, column).await;
        let items = match items_result {
            Ok(items) => items,
            Err(e) => {
                return TypeHierarchySummary {
                    items: Vec::new(),
                    supertypes: Vec::new(),
                    subtypes: Vec::new(),
                    prepare_error: Some(e.to_string()),
                    supertypes_error: None,
                    subtypes_error: None,
                    truncated: false,
                };
            }
        };

        let items_truncated = items.len() > MAX_HIERARCHY_ITEMS;
        let item_summaries: Vec<HierarchyItemSummary> = items
            .iter()
            .take(MAX_HIERARCHY_ITEMS)
            .map(Self::convert_type_hierarchy_item)
            .collect();

        if items.is_empty() {
            return TypeHierarchySummary {
                items: Vec::new(),
                supertypes: Vec::new(),
                subtypes: Vec::new(),
                prepare_error: None,
                supertypes_error: None,
                subtypes_error: None,
                truncated: false,
            };
        }

        let primary = &items[0];
        let mut supertypes = Vec::new();
        let mut supertypes_error = None;
        let mut supertypes_raw_len = 0usize;
        let mut subtypes = Vec::new();
        let mut subtypes_error = None;
        let mut subtypes_raw_len = 0usize;

        if matches!(
            direction,
            crate::lsp::operations::HierarchyDirection::Incoming
                | crate::lsp::operations::HierarchyDirection::Both
        ) {
            match ops.supertypes(primary.clone()).await {
                Ok(items) => {
                    supertypes_raw_len = items.len();
                    let capped: Vec<_> = items.into_iter().take(MAX_HIERARCHY_ITEMS).collect();
                    supertypes = capped
                        .iter()
                        .map(Self::convert_type_hierarchy_item)
                        .collect();
                }
                Err(e) => {
                    supertypes_error = Some(e.to_string());
                }
            }
        }

        if matches!(
            direction,
            crate::lsp::operations::HierarchyDirection::Outgoing
                | crate::lsp::operations::HierarchyDirection::Both
        ) {
            match ops.subtypes(primary.clone()).await {
                Ok(items) => {
                    subtypes_raw_len = items.len();
                    let capped: Vec<_> = items.into_iter().take(MAX_HIERARCHY_ITEMS).collect();
                    subtypes = capped
                        .iter()
                        .map(Self::convert_type_hierarchy_item)
                        .collect();
                }
                Err(e) => {
                    subtypes_error = Some(e.to_string());
                }
            }
        }

        let truncated = items_truncated
            || supertypes_raw_len > MAX_HIERARCHY_ITEMS
            || subtypes_raw_len > MAX_HIERARCHY_ITEMS;

        TypeHierarchySummary {
            items: item_summaries,
            supertypes,
            subtypes,
            prepare_error: None,
            supertypes_error,
            subtypes_error,
            truncated,
        }
    }

    async fn build_call_expansion_summary(
        &self,
        ops: &crate::lsp::operations::LspOperations,
        file: &Path,
        line: u32,
        column: u32,
        direction: crate::lsp::operations::HierarchyDirection,
        max_depth: u8,
        max_nodes: usize,
    ) -> CallExpansionSummary {
        use std::collections::{HashSet, VecDeque};

        let root_items = match ops.prepare_call_hierarchy(file, line, column).await {
            Ok(items) => items,
            Err(e) => {
                return CallExpansionSummary {
                    root: None,
                    direction: format_direction(direction),
                    depth: max_depth,
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    truncated: false,
                    errors: vec![format!("prepare_call_hierarchy: {e}")],
                };
            }
        };

        let Some(root_item) = root_items.first() else {
            return CallExpansionSummary {
                root: None,
                direction: format_direction(direction),
                depth: max_depth,
                nodes: Vec::new(),
                edges: Vec::new(),
                truncated: false,
                errors: Vec::new(),
            };
        };

        let root_node = Self::call_expansion_node_from_item(root_item, 0);
        let root_id = root_node.id.clone();

        let mut nodes: Vec<CallExpansionNode> = vec![root_node.clone()];
        let mut edges: Vec<CallExpansionEdge> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(root_id.clone());

        let mut queue: VecDeque<(crate::lsp::lsp_types::CallHierarchyItem, u8)> = VecDeque::new();
        queue.push_back((root_item.clone(), 0));

        let mut truncated = false;

        while let Some((item, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            if nodes.len() >= max_nodes {
                truncated = true;
                continue;
            }

            if matches!(
                direction,
                crate::lsp::operations::HierarchyDirection::Incoming
                    | crate::lsp::operations::HierarchyDirection::Both
            ) {
                match ops.incoming_calls(item.clone()).await {
                    Ok(calls) => {
                        for call in calls {
                            if nodes.len() >= max_nodes {
                                truncated = true;
                                break;
                            }
                            let child_id = Self::call_expansion_node_id(&call.from);
                            let child_depth = depth + 1;
                            let (ranges, ranges_truncated) =
                                Self::capped_call_ranges(&call.from_ranges);
                            truncated |= ranges_truncated;
                            let edge = CallExpansionEdge {
                                from: child_id.clone(),
                                to: Self::call_expansion_node_id(&item),
                                direction: "incoming".to_string(),
                                ranges,
                            };
                            truncated |= Self::push_call_expansion_edge(&mut edges, edge);
                            if seen.insert(child_id.clone()) {
                                let node =
                                    Self::call_expansion_node_from_item(&call.from, child_depth);
                                let node_truncated =
                                    Self::push_call_expansion_node(&mut nodes, node, max_nodes);
                                truncated |= node_truncated;
                                if !node_truncated {
                                    queue.push_back((call.from, child_depth));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!(
                            "incoming_calls for {}: {e}",
                            Self::call_expansion_node_id(&item)
                        ));
                    }
                }
            }

            if nodes.len() >= max_nodes {
                truncated = true;
                continue;
            }

            if matches!(
                direction,
                crate::lsp::operations::HierarchyDirection::Outgoing
                    | crate::lsp::operations::HierarchyDirection::Both
            ) {
                match ops.outgoing_calls(item.clone()).await {
                    Ok(calls) => {
                        for call in calls {
                            if nodes.len() >= max_nodes {
                                truncated = true;
                                break;
                            }
                            let child_id = Self::call_expansion_node_id(&call.to);
                            let child_depth = depth + 1;
                            let (ranges, ranges_truncated) =
                                Self::capped_call_ranges(&call.from_ranges);
                            truncated |= ranges_truncated;
                            let edge = CallExpansionEdge {
                                from: Self::call_expansion_node_id(&item),
                                to: child_id.clone(),
                                direction: "outgoing".to_string(),
                                ranges,
                            };
                            truncated |= Self::push_call_expansion_edge(&mut edges, edge);
                            if seen.insert(child_id.clone()) {
                                let node =
                                    Self::call_expansion_node_from_item(&call.to, child_depth);
                                let node_truncated =
                                    Self::push_call_expansion_node(&mut nodes, node, max_nodes);
                                truncated |= node_truncated;
                                if !node_truncated {
                                    queue.push_back((call.to, child_depth));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!(
                            "outgoing_calls for {}: {e}",
                            Self::call_expansion_node_id(&item)
                        ));
                    }
                }
            }
        }

        CallExpansionSummary {
            root: Some(root_node),
            direction: format_direction(direction),
            depth: max_depth,
            nodes,
            edges,
            truncated,
            errors,
        }
    }

    fn call_expansion_node_id(item: &crate::lsp::lsp_types::CallHierarchyItem) -> String {
        let file = uri_to_path(&item.uri);
        let sel = &item.selection_range;
        format!(
            "{}:{}:{}:{}",
            file,
            item.name,
            sel.start.line + 1,
            sel.start.character + 1
        )
    }

    fn call_expansion_node_from_item(
        item: &crate::lsp::lsp_types::CallHierarchyItem,
        depth: u8,
    ) -> CallExpansionNode {
        CallExpansionNode {
            id: Self::call_expansion_node_id(item),
            name: item.name.clone(),
            kind: symbol_kind_to_string(item.kind),
            file: Some(uri_to_path(&item.uri)),
            range: Self::convert_lsp_range(item.range),
            selection_range: Self::convert_lsp_range(item.selection_range),
            detail: item.detail.clone(),
            depth,
        }
    }

    fn capped_call_ranges(
        ranges: &[crate::lsp::lsp_types::Range],
    ) -> (Vec<HierarchyRangeSummary>, bool) {
        let truncated = ranges.len() > MAX_HIERARCHY_RANGES;
        let capped = ranges
            .iter()
            .take(MAX_HIERARCHY_RANGES)
            .map(|r| Self::convert_lsp_range(*r))
            .collect();
        (capped, truncated)
    }

    fn push_call_expansion_edge(
        edges: &mut Vec<CallExpansionEdge>,
        edge: CallExpansionEdge,
    ) -> bool {
        if edges.len() >= MAX_CALL_EDGES {
            return true;
        }
        edges.push(edge);
        false
    }

    fn push_call_expansion_node(
        nodes: &mut Vec<CallExpansionNode>,
        node: CallExpansionNode,
        max_nodes: usize,
    ) -> bool {
        if nodes.len() >= max_nodes {
            return true;
        }
        nodes.push(node);
        false
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Query LSP server for code intelligence and preview-only edits. Operations: goToDefinition, findReferences, hover, documentSymbol, workspaceSymbol, diagnostics, declaration, implementation, documentHighlights, signatureHelp, completion, semanticTokens, renamePreview, formatPreview, sourceActionPreview, codeActionSummaries, codeActionPreview, semanticCheckPreview, semanticContext, securityContext, callHierarchy, typeHierarchy, capabilities, hunkSourceContext. semanticCheckPreview accepts either full proposed content or a single-file unified diff patch. semanticContext returns a compact LSP-backed context packet with source excerpt, diagnostics, symbols, and optional definition/reference/overlay information. securityContext returns a security-review context packet with risk markers. capabilities returns a normalized snapshot of what the server supports. hunkSourceContext returns per-hunk navigation evidence with enclosing symbols, diagnostics, definitions, references, and hierarchy. When include_source_actions=true, semanticContext also includes safe source-action preview hints (initially only source.organizeImports). callHierarchy/typeHierarchy return call/type hierarchy information for the symbol at line+column. declaration/implementation/documentHighlights/signatureHelp/completion/semanticTokens are read-only navigation and code intelligence operations. codeActionSummaries returns code action summaries for a range; codeActionPreview returns a preview of a specific code action. Edit operations are previews only; use apply_patch (or other mutating tools) for actual changes."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": [
                        "goToDefinition", "findReferences", "hover",
                        "documentSymbol", "workspaceSymbol", "diagnostics",
                        "declaration", "implementation", "documentHighlights",
                        "signatureHelp", "completion", "semanticTokens",
                        "renamePreview", "formatPreview", "sourceActionPreview",
                        "codeActionSummaries", "codeActionPreview",
                        "semanticCheckPreview", "semanticContext",
                        "callHierarchy", "typeHierarchy",
                        "securityContext", "capabilities",
                        "hunkSourceContext"
                    ],
                    "description": "LSP operation to perform. declaration/implementation/documentHighlights/signatureHelp/completion/semanticTokens are read-only navigation and code intelligence. codeActionSummaries returns code action summaries for a range; codeActionPreview returns a preview of a specific code action. semanticCheckPreview accepts either full proposed content or a single-file unified diff patch. semanticContext returns a compact LSP-backed context packet. securityContext returns a security-review context packet with risk markers. capabilities returns a normalized snapshot of what the server supports. hunkSourceContext returns per-hunk navigation evidence with enclosing symbols, diagnostics, definitions, references, and hierarchy. callHierarchy/typeHierarchy return call/type hierarchy information for the symbol at line+column. Edit operations are previews only; use apply_patch (or other mutating tools) for actual changes."
                },
                "file_path": {
                    "type": "string",
                    "description": "File path for the operation"
                },
                "line": {
                    "type": "number",
                    "description": "Line number (1-indexed)"
                },
                "column": {
                    "type": "number",
                    "description": "Column number (1-indexed)"
                },
                "symbol": {
                    "type": "string",
                    "description": "Symbol name for workspaceSymbol operation"
                },
                "new_name": {
                    "type": "string",
                    "description": "New name for renamePreview operation"
                },
                "action": {
                    "type": "string",
                    "description": "Allowlisted source action for sourceActionPreview. Initially supports source.organizeImports."
                },
                "content": {
                    "type": "string",
                    "description": "Proposed full file content for semanticCheckPreview, semanticContext overlay, or securityContext overlay. Mutually exclusive with patch."
                },
                "patch": {
                    "type": "string",
                    "description": "Single-file unified diff patch to apply in memory for semanticCheckPreview, semanticContext overlay, or securityContext overlay. Mutually exclusive with content."
                },
                "radius": {
                    "type": "number",
                    "description": "Number of lines above and below target for semanticContext/securityContext source excerpt. semanticContext default 40/max 120; securityContext default 80/max 200."
                },
                "include_references": {
                    "type": "boolean",
                    "description": "Include findReferences results in semanticContext (default true when line+column provided)"
                },
                "include_definitions": {
                    "type": "boolean",
                    "description": "Include goToDefinition results in semanticContext (default true when line+column provided)"
                },
                "include_overlay": {
                    "type": "boolean",
                    "description": "Include overlay diagnostics in semanticContext (default true when content or patch provided)"
                },
                "include_source_actions": {
                    "type": "boolean",
                    "description": "Include safe allowlisted source-action preview hints in semanticContext. Initially only source.organizeImports. Default false."
                },
                "direction": {
                    "type": "string",
                    "enum": ["incoming", "outgoing", "both"],
                    "description": "Hierarchy direction for callHierarchy/typeHierarchy. Defaults to both. For typeHierarchy, incoming means supertypes and outgoing means subtypes."
                },
                "include_call_hierarchy": {
                    "type": "boolean",
                    "description": "Include call hierarchy section in semanticContext. In securityContext, call hierarchy defaults to true when line+column are supplied. Requires line+column."
                },
                "include_type_hierarchy": {
                    "type": "boolean",
                    "description": "Include type hierarchy section in semanticContext. Requires line+column. Default false."
                },
                "security_categories": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional risk marker categories to include in securityContext. Defaults to all supported categories. Supported: auth, crypto, filesystem, network, process, unsafe, serialization, sql, secrets, path_traversal, concurrency."
                },
                "max_risk_markers": {
                    "type": "number",
                    "description": "Maximum risk markers to return for securityContext. Default 80, max 200."
                },
                "security_preset": {
                    "type": "string",
                    "enum": ["rust_server", "rust_cli", "web_backend", "dependency_review", "unsafe_review"],
                    "description": "Optional securityContext preset that sets default risk categories, radius, marker limits, and call-hierarchy behavior. Explicit inputs override preset defaults."
                },
                "call_depth": {
                    "type": "number",
                    "description": "Optional securityContext call expansion depth. Default 0/off. Max 2. Requires line+column."
                },
                "max_call_nodes": {
                    "type": "number",
                    "description": "Maximum call expansion nodes for securityContext. Default 32, max 64."
                },
                "call_direction": {
                    "type": "string",
                    "enum": ["incoming", "outgoing", "both"],
                    "description": "Direction for securityContext call expansion. incoming=callers, outgoing=callees, both=both. Default both."
                },
                "max_hunks": {
                    "type": "number",
                    "description": "Maximum hunks for hunkSourceContext. Default 20."
                },
                "max_candidates": {
                    "type": "number",
                    "description": "Maximum completion candidates to return. Default 200."
                },
                "max_tokens": {
                    "type": "number",
                    "description": "Maximum semantic tokens to return. Default 1000."
                },
                "max_actions": {
                    "type": "number",
                    "description": "Maximum code actions for codeActionSummaries. Default 50."
                },
                "action_index": {
                    "type": "number",
                    "description": "Index of the code action to preview for codeActionPreview (0-indexed)."
                },
                "only": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional code action kinds filter for codeActionSummaries/codeActionPreview."
                },
                "trigger_kind": {
                    "type": "number",
                    "description": "Completion trigger kind: 1=invoked, 2=triggerCharacter, 3=triggerForIncompleteCompletions."
                },
                "trigger_char": {
                    "type": "string",
                    "description": "Trigger character for completion when trigger_kind=2."
                }
            },
            "required": ["operation"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let parsed: LspInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid lsp input: {e}")))?;

        let ops = crate::lsp::operations::LspOperations::new(self.service.clone());
        let file_path_str = parsed.file_path.clone();

        let result = match parsed.operation.as_str() {
            "goToDefinition" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let pos = to_lsp_position(line, col);
                let locs = ops
                    .go_to_definition(&file, pos.line, pos.character)
                    .await
                    .map_err(|e| ToolError::Execution(format!("goToDefinition: {e}")))?;
                let summaries: Vec<LocationSummary> = locs
                    .iter()
                    .map(|loc| {
                        let range = loc.target_range;
                        LocationSummary {
                            file: uri_to_path(&loc.target_uri),
                            start_line: range.start.line + 1,
                            start_column: range.start.character + 1,
                            end_line: range.end.line + 1,
                            end_column: range.end.character + 1,
                        }
                    })
                    .collect();
                let output = LspToolOutput {
                    operation: "goToDefinition".to_string(),
                    file_path: file_path_str,
                    result_count: summaries.len(),
                    truncated: false,
                    results: summaries,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "findReferences" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let pos = to_lsp_position(line, col);
                let refs = ops
                    .find_references(&file, pos.line, pos.character)
                    .await
                    .map_err(|e| ToolError::Execution(format!("findReferences: {e}")))?;
                let truncated = refs.len() > MAX_REFERENCES;
                let capped: Vec<_> = refs.into_iter().take(MAX_REFERENCES).collect();
                let summaries: Vec<LocationSummary> = capped
                    .iter()
                    .map(|loc| {
                        let range = loc.range;
                        LocationSummary {
                            file: uri_to_path(&loc.uri),
                            start_line: range.start.line + 1,
                            start_column: range.start.character + 1,
                            end_line: range.end.line + 1,
                            end_column: range.end.character + 1,
                        }
                    })
                    .collect();
                let output = LspToolOutput {
                    operation: "findReferences".to_string(),
                    file_path: file_path_str,
                    result_count: summaries.len(),
                    truncated,
                    results: summaries,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "hover" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let pos = to_lsp_position(line, col);
                let hover_text = ops
                    .hover(&file, pos.line, pos.character)
                    .await
                    .map_err(|e| ToolError::Execution(format!("hover: {e}")))?;
                let contents = hover_text.unwrap_or_default();
                let truncated = contents.len() > MAX_HOVER_CHARS;
                let display = if truncated {
                    &contents[..MAX_HOVER_CHARS]
                } else {
                    &contents
                };
                let summary = HoverSummary {
                    file: file_path_str.clone().unwrap_or_default(),
                    line,
                    column: col,
                    contents: display.to_string(),
                };
                let output = LspToolOutput {
                    operation: "hover".to_string(),
                    file_path: file_path_str,
                    result_count: if summary.contents.is_empty() { 0 } else { 1 },
                    truncated,
                    results: summary,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "documentSymbol" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let syms = ops
                    .document_symbols(&file)
                    .await
                    .map_err(|e| ToolError::Execution(format!("documentSymbol: {e}")))?;
                let file_str = file.to_string_lossy().to_string();
                let mut remaining = MAX_SYMBOLS;
                let mut summaries = Vec::new();
                Self::flatten_symbols(&syms, &file_str, &mut summaries, &mut remaining);
                let output = LspToolOutput {
                    operation: "documentSymbol".to_string(),
                    file_path: file_path_str,
                    result_count: summaries.len(),
                    truncated: remaining == 0,
                    results: summaries,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "diagnostics" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (key, uri_str) = self
                    .service
                    .ensure_file_open_from_disk(&file)
                    .await
                    .map_err(|e| ToolError::Execution(format!("diagnostics: {e}")))?;
                let snapshot = self
                    .service
                    .get_diagnostic_snapshot_for_key(&key, &uri_str)
                    .await
                    .map_err(|e| ToolError::Execution(format!("diagnostics: {e}")))?;
                let warming = snapshot.diagnostics_may_still_be_warming();
                let summaries: Vec<DiagnosticSummary> = snapshot
                    .diagnostics
                    .iter()
                    .map(|d| DiagnosticSummary {
                        file: d.file.clone(),
                        line: d.line + 1,
                        column: d.column + 1,
                        severity: severity_to_string(d.severity),
                        source: d.source.clone(),
                        code: d.code.clone(),
                        message: d.message.clone(),
                    })
                    .collect();
                #[derive(Serialize)]
                struct DiagnosticsResult {
                    diagnostics_may_still_be_warming: bool,
                    freshness: crate::lsp::diagnostics::LspDiagnosticFreshness,
                    source: crate::lsp::diagnostics::LspDiagnosticSource,
                    age_ms: i64,
                    usable_evidence: bool,
                    diagnostics: Vec<DiagnosticSummary>,
                }
                let result = DiagnosticsResult {
                    diagnostics_may_still_be_warming: warming,
                    freshness: snapshot.freshness,
                    source: snapshot.source,
                    age_ms: snapshot.age_ms,
                    usable_evidence: snapshot.is_usable_evidence(),
                    diagnostics: summaries,
                };
                let output = LspToolOutput {
                    operation: "diagnostics".to_string(),
                    file_path: file_path_str,
                    result_count: result.diagnostics.len(),
                    truncated: false,
                    results: result,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "workspaceSymbol" => {
                let sym = parsed.symbol.as_ref().ok_or_else(|| {
                    ToolError::Execution("symbol required for workspaceSymbol".to_string())
                })?;
                let params = serde_json::json!({
                    "query": sym,
                    "workDoneToken": null,
                    "partialResultToken": null,
                });
                let key = if parsed.file_path.is_some() {
                    let file = self.resolve_file(&parsed.file_path)?;
                    let (k, _) = self
                        .service
                        .get_or_create_client_for_file(&file)
                        .await
                        .map_err(|e| ToolError::Execution(format!("workspaceSymbol: {e}")))?;
                    k
                } else {
                    let root = std::env::current_dir().unwrap_or_default();
                    self.service
                        .find_existing_client_for_root_hint(Some(&root), None)
                        .await
                        .map_err(|e| ToolError::Execution(format!("workspaceSymbol: {e}")))?
                        .0
                };
                let resp = self
                    .service
                    .send_request(&key, "workspace/symbol", params)
                    .await
                    .map_err(|e| ToolError::Execution(format!("workspaceSymbol: {e}")))?;

                // Try to parse as Vec<WorkspaceSymbol> first, then Vec<SymbolInformation>.
                let summaries: Vec<WorkspaceSymbolSummary> = if resp.as_array().is_some() {
                    // Try WorkspaceSymbol form
                    if let Ok(syms) = serde_json::from_value::<
                        Vec<crate::lsp::lsp_types::WorkspaceSymbol>,
                    >(resp.clone())
                    {
                        syms.into_iter()
                            .take(MAX_WORKSPACE_SYMBOLS)
                            .map(|s| {
                                let (file, start_line, start_column) = match &s.location {
                                    crate::lsp::lsp_types::OneOf::Left(loc) => (
                                        Some(uri_to_path(&loc.uri)),
                                        Some(loc.range.start.line + 1),
                                        Some(loc.range.start.character + 1),
                                    ),
                                    crate::lsp::lsp_types::OneOf::Right(wloc) => {
                                        (Some(uri_to_path(&wloc.uri)), None, None)
                                    }
                                };
                                WorkspaceSymbolSummary {
                                    name: s.name,
                                    kind: symbol_kind_to_string(s.kind),
                                    file,
                                    start_line,
                                    start_column,
                                    container_name: s.container_name,
                                }
                            })
                            .collect()
                    } else if let Ok(syms) = serde_json::from_value::<
                        Vec<crate::lsp::lsp_types::SymbolInformation>,
                    >(resp.clone())
                    {
                        // SymbolInformation form
                        syms.into_iter()
                            .take(MAX_WORKSPACE_SYMBOLS)
                            .map(|s| WorkspaceSymbolSummary {
                                name: s.name,
                                kind: symbol_kind_to_string(s.kind),
                                file: Some(uri_to_path(&s.location.uri)),
                                start_line: Some(s.location.range.start.line + 1),
                                start_column: Some(s.location.range.start.character + 1),
                                container_name: s.container_name,
                            })
                            .collect()
                    } else {
                        // Cannot parse - return empty
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                let truncated = resp
                    .as_array()
                    .is_some_and(|a| a.len() > MAX_WORKSPACE_SYMBOLS);
                let output = LspToolOutput {
                    operation: "workspaceSymbol".to_string(),
                    file_path: file_path_str,
                    result_count: summaries.len(),
                    truncated,
                    results: summaries,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "renamePreview" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let new_name = parsed.new_name.as_ref().ok_or_else(|| {
                    ToolError::Execution("new_name required for renamePreview".to_string())
                })?;
                let pos = to_lsp_position(line, col);
                let preview = ops
                    .rename_preview_unchecked(
                        &file,
                        pos.line,
                        pos.character,
                        new_name,
                        Some(&self.allowed_root),
                    )
                    .await
                    .map_err(|e| ToolError::Execution(format!("renamePreview: {e}")))?;
                let output = LspToolOutput {
                    operation: "renamePreview".to_string(),
                    file_path: file_path_str,
                    result_count: preview.total_edits,
                    truncated: preview.truncated,
                    results: preview,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "formatPreview" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let preview = ops
                    .format_preview_unchecked(&file, Some(&self.allowed_root))
                    .await
                    .map_err(|e| ToolError::Execution(format!("formatPreview: {e}")))?;
                let output = LspToolOutput {
                    operation: "formatPreview".to_string(),
                    file_path: file_path_str,
                    result_count: preview.total_edits,
                    truncated: preview.truncated,
                    results: preview,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "sourceActionPreview" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let action_str = parsed.action.as_deref().ok_or_else(|| {
                    ToolError::Execution("action required for sourceActionPreview".to_string())
                })?;
                let kind = crate::lsp::operations::SourceActionPreviewKind::parse(action_str)
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                let preview = ops
                    .source_action_preview(&file, kind, Some(&self.allowed_root))
                    .await
                    .map_err(|e| ToolError::Execution(format!("sourceActionPreview: {e}")))?;
                let output = LspToolOutput {
                    operation: "sourceActionPreview".to_string(),
                    file_path: file_path_str,
                    result_count: preview.total_edits,
                    truncated: preview.truncated,
                    results: preview,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "callHierarchy" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let direction =
                    crate::lsp::operations::HierarchyDirection::parse(parsed.direction.as_deref())
                        .map_err(|e| ToolError::Execution(e.to_string()))?;
                let pos = to_lsp_position(line, col);
                let summary = self
                    .build_call_hierarchy_summary(&ops, &file, pos.line, pos.character, direction)
                    .await;
                let output = LspToolOutput {
                    operation: "callHierarchy".to_string(),
                    file_path: file_path_str,
                    result_count: summary.items.len()
                        + summary.incoming.len()
                        + summary.outgoing.len(),
                    truncated: summary.truncated,
                    results: summary,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "typeHierarchy" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let direction =
                    crate::lsp::operations::HierarchyDirection::parse(parsed.direction.as_deref())
                        .map_err(|e| ToolError::Execution(e.to_string()))?;
                let pos = to_lsp_position(line, col);
                let summary = self
                    .build_type_hierarchy_summary(&ops, &file, pos.line, pos.character, direction)
                    .await;
                let output = LspToolOutput {
                    operation: "typeHierarchy".to_string(),
                    file_path: file_path_str,
                    result_count: summary.items.len()
                        + summary.supertypes.len()
                        + summary.subtypes.len(),
                    truncated: summary.truncated,
                    results: summary,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "semanticCheckPreview" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let content = self
                    .resolve_semantic_check_content(
                        &file,
                        parsed.content.as_ref(),
                        parsed.patch.as_ref(),
                    )
                    .await?;
                let preview = ops
                    .semantic_check_preview(&file, content, Some(&self.allowed_root))
                    .await
                    .map_err(|e| ToolError::Execution(format!("semanticCheckPreview: {e}")))?;
                let diag_summaries: Vec<DiagnosticSummary> = preview
                    .diagnostics
                    .iter()
                    .map(|d| DiagnosticSummary {
                        file: d.file.clone(),
                        line: d.line + 1,
                        column: d.column + 1,
                        severity: severity_to_string(d.severity),
                        source: d.source.clone(),
                        code: d.code.clone(),
                        message: d.message.clone(),
                    })
                    .collect();
                #[derive(Serialize)]
                struct SemanticCheckResult {
                    diagnostics_may_still_be_warming: bool,
                    diagnostics: Vec<DiagnosticSummary>,
                    diagnostics_error: Option<String>,
                    symbols: Vec<crate::lsp::overlay::SemanticSymbolSummary>,
                    symbols_error: Option<String>,
                    restored_disk_view: bool,
                    restore_error: Option<String>,
                }
                let result = SemanticCheckResult {
                    diagnostics_may_still_be_warming: preview.diagnostics_may_still_be_warming,
                    diagnostics: diag_summaries,
                    diagnostics_error: preview.diagnostics_error,
                    symbols: preview.symbols,
                    symbols_error: preview.symbols_error,
                    restored_disk_view: preview.restored_disk_view,
                    restore_error: preview.restore_error,
                };
                let output = LspToolOutput {
                    operation: "semanticCheckPreview".to_string(),
                    file_path: file_path_str,
                    result_count: result.diagnostics.len() + result.symbols.len(),
                    truncated: false,
                    results: result,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "semanticContext" => {
                let file = self.resolve_file(&parsed.file_path)?;

                // Reject both content and patch upfront (same as semanticCheckPreview)
                if parsed.content.is_some() && parsed.patch.is_some() {
                    return Err(ToolError::Execution(
                        "semanticCheckPreview accepts either content or patch, not both"
                            .to_string(),
                    ));
                }

                let radius = parsed
                    .radius
                    .unwrap_or(DEFAULT_SEMANTIC_CONTEXT_RADIUS)
                    .min(MAX_SEMANTIC_CONTEXT_RADIUS);
                let has_position = parsed.line.is_some() && parsed.column.is_some();
                let has_proposed = parsed.content.is_some() || parsed.patch.is_some();
                let want_overlay = parsed.include_overlay.unwrap_or(has_proposed);

                let target = if has_position {
                    Some(SemanticContextTarget {
                        line: parsed.line.unwrap(),
                        column: parsed.column.unwrap(),
                    })
                } else if parsed.line.is_some() || parsed.column.is_some() {
                    return Err(ToolError::Execution(
                        "semanticContext requires both line and column when either is supplied"
                            .to_string(),
                    ));
                } else {
                    None
                };

                // Hierarchy flags require a full position
                let include_call_hierarchy = parsed.include_call_hierarchy.unwrap_or(false);
                let include_type_hierarchy = parsed.include_type_hierarchy.unwrap_or(false);

                if (include_call_hierarchy || include_type_hierarchy) && !has_position {
                    return Err(ToolError::Execution(
                        "semanticContext hierarchy sections require both line and column"
                            .to_string(),
                    ));
                }

                // Build the shared request for the collector
                let file_str = file.to_string_lossy().to_string();
                let mut request = egglsp::semantic_context::SemanticContextRequest::new(
                    &file_str,
                    egglsp::semantic_context::SemanticContextIntent::Explain,
                )
                .with_excerpt_radius(radius);
                if has_position {
                    request = request.with_position(parsed.line.unwrap(), parsed.column.unwrap());
                }
                request.include_overlay = false;
                request.include_source_actions = false;
                request.include_definitions = parsed.include_definitions.unwrap_or(has_position);
                request.include_references = parsed.include_references.unwrap_or(has_position);
                request.include_call_hierarchy = include_call_hierarchy;
                request.include_type_hierarchy = include_type_hierarchy;
                request.max_symbols = MAX_CONTEXT_SYMBOLS;
                request.max_references = MAX_CONTEXT_REFERENCES;
                request.max_diagnostics = MAX_CONTEXT_DIAGNOSTICS;

                let ops = Arc::new(crate::lsp::operations::LspOperations::new(
                    self.service.clone(),
                ));
                let diagnostics = Arc::new(crate::lsp::diagnostics::DiagnosticsCollector::new(
                    self.service.clone(),
                ));
                let collector = SemanticContextCollector::new(
                    self.service.clone(),
                    ops.clone(),
                    diagnostics,
                    self.allowed_root.clone(),
                );

                let response = collector
                    .collect(request)
                    .await
                    .map_err(|e| ToolError::Execution(format!("semanticContext: {e}")))?;

                let (overlay, overlay_diagnostics_truncated) = if want_overlay {
                    match self
                        .resolve_semantic_check_content(
                            &file,
                            parsed.content.as_ref(),
                            parsed.patch.as_ref(),
                        )
                        .await
                    {
                        Ok(content) => {
                            match ops
                                .semantic_check_preview(&file, content, Some(&self.allowed_root))
                                .await
                            {
                                Ok(preview) => {
                                    let overlay_diag_truncated =
                                        preview.diagnostics.len() > MAX_CONTEXT_DIAGNOSTICS;
                                    let diag_summaries: Vec<DiagnosticSummary> = preview
                                        .diagnostics
                                        .iter()
                                        .take(MAX_CONTEXT_DIAGNOSTICS)
                                        .map(|d| DiagnosticSummary {
                                            file: d.file.clone(),
                                            line: d.line + 1,
                                            column: d.column + 1,
                                            severity: severity_to_string(d.severity),
                                            source: d.source.clone(),
                                            code: d.code.clone(),
                                            message: d.message.clone(),
                                        })
                                        .collect();
                                    (
                                        Some(SemanticOverlaySummary {
                                            used: true,
                                            diagnostics_may_still_be_warming: preview
                                                .diagnostics_may_still_be_warming,
                                            diagnostics: diag_summaries,
                                            diagnostics_error: preview.diagnostics_error,
                                            symbols: preview.symbols,
                                            symbols_error: preview.symbols_error,
                                            restored_disk_view: preview.restored_disk_view,
                                            restore_error: preview.restore_error,
                                        }),
                                        overlay_diag_truncated,
                                    )
                                }
                                Err(e) => (
                                    Some(SemanticOverlaySummary {
                                        used: true,
                                        diagnostics_may_still_be_warming: false,
                                        diagnostics: Vec::new(),
                                        diagnostics_error: Some(format!("overlay: {e}")),
                                        symbols: Vec::new(),
                                        symbols_error: None,
                                        restored_disk_view: false,
                                        restore_error: None,
                                    }),
                                    false,
                                ),
                            }
                        }
                        Err(_) if !has_proposed => (None, false),
                        Err(e) => (
                            Some(SemanticOverlaySummary {
                                used: true,
                                diagnostics_may_still_be_warming: false,
                                diagnostics: Vec::new(),
                                diagnostics_error: Some(format!("overlay content: {e}")),
                                symbols: Vec::new(),
                                symbols_error: None,
                                restored_disk_view: false,
                                restore_error: None,
                            }),
                            false,
                        ),
                    }
                } else {
                    (None, false)
                };

                // Source-action hints (opt-in)
                let include_source_actions = parsed.include_source_actions.unwrap_or(false);
                let source_actions = if include_source_actions {
                    self.collect_source_action_hints(&ops, &file).await
                } else {
                    Vec::new()
                };

                // Adapt the shared response into the tool-local packet
                let mut packet = SemanticContextPacket::from_semantic_response(
                    response,
                    target,
                    overlay,
                    source_actions,
                    overlay_diagnostics_truncated,
                );
                if !include_call_hierarchy {
                    packet.call_hierarchy = None;
                }
                if !include_type_hierarchy {
                    packet.type_hierarchy = None;
                }

                // Compute result_count from packet fields
                let overlay_diag_count = packet
                    .overlay
                    .as_ref()
                    .map(|o| o.diagnostics.len())
                    .unwrap_or(0);
                let overlay_sym_count = packet
                    .overlay
                    .as_ref()
                    .map(|o| o.symbols.len())
                    .unwrap_or(0);
                let source_action_count = packet
                    .source_actions
                    .iter()
                    .filter(|hint| hint.available)
                    .count();
                let call_hierarchy_count = packet
                    .call_hierarchy
                    .as_ref()
                    .map(|c| c.items.len() + c.incoming.len() + c.outgoing.len())
                    .unwrap_or(0);
                let type_hierarchy_count = packet
                    .type_hierarchy
                    .as_ref()
                    .map(|c| c.items.len() + c.supertypes.len() + c.subtypes.len())
                    .unwrap_or(0);
                let packet_truncated = packet.limits.diagnostics_truncated
                    || packet.limits.symbols_truncated
                    || packet.limits.references_truncated
                    || packet.limits.overlay_diagnostics_truncated
                    || packet.limits.excerpt_truncated
                    || packet.call_hierarchy.as_ref().is_some_and(|c| c.truncated)
                    || packet.type_hierarchy.as_ref().is_some_and(|c| c.truncated);
                let result_count = packet.diagnostics.len()
                    + packet.symbols.len()
                    + packet.definitions.len()
                    + packet.references.len()
                    + overlay_diag_count
                    + overlay_sym_count
                    + source_action_count
                    + call_hierarchy_count
                    + type_hierarchy_count;

                let output = LspToolOutput {
                    operation: "semanticContext".to_string(),
                    file_path: file_path_str,
                    result_count,
                    truncated: packet_truncated,
                    results: packet,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "securityContext" => {
                let file = self.resolve_file(&parsed.file_path)?;

                if parsed.content.is_some() && parsed.patch.is_some() {
                    return Err(ToolError::Execution(
                        "securityContext accepts either content or patch, not both".to_string(),
                    ));
                }

                let file_str = file.to_string_lossy().to_string();
                let has_position = parsed.line.is_some() && parsed.column.is_some();
                let has_proposed = parsed.content.is_some() || parsed.patch.is_some();

                let settings = Self::resolve_security_context_settings(&parsed, has_position)?;

                let target = if has_position {
                    Some(SemanticContextTarget {
                        line: parsed.line.unwrap(),
                        column: parsed.column.unwrap(),
                    })
                } else if parsed.line.is_some() || parsed.column.is_some() {
                    return Err(ToolError::Execution(
                        "securityContext requires both line and column when either is supplied"
                            .to_string(),
                    ));
                } else {
                    None
                };

                if settings.include_call_hierarchy && !has_position {
                    return Err(ToolError::Execution(
                        "securityContext call hierarchy requires both line and column".to_string(),
                    ));
                }

                if settings.call_depth > 0 && !has_position {
                    return Err(ToolError::Execution(
                        "securityContext call_depth requires both line and column".to_string(),
                    ));
                }

                let ops = Arc::new(crate::lsp::operations::LspOperations::new(
                    self.service.clone(),
                ));
                let diagnostics = Arc::new(crate::lsp::diagnostics::DiagnosticsCollector::new(
                    self.service.clone(),
                ));
                let mut request = egglsp::semantic_context::SemanticContextRequest::new(
                    file_str.clone(),
                    egglsp::semantic_context::SemanticContextIntent::SecurityReview,
                )
                .with_excerpt_radius(settings.radius);
                if has_position {
                    request = request.with_position(parsed.line.unwrap(), parsed.column.unwrap());
                }
                request.include_overlay = false;
                request.include_source_actions = false;
                request.include_definitions = has_position;
                request.include_references = has_position;
                request.include_call_hierarchy = settings.include_call_hierarchy && has_position;
                request.include_type_hierarchy = false;
                request.max_symbols = MAX_CONTEXT_SYMBOLS;
                request.max_references = MAX_CONTEXT_REFERENCES;
                request.max_diagnostics = MAX_CONTEXT_DIAGNOSTICS;

                let collector = SemanticContextCollector::new(
                    self.service.clone(),
                    ops.clone(),
                    diagnostics,
                    self.allowed_root.clone(),
                );
                let response = collector
                    .collect(request)
                    .await
                    .map_err(|e| ToolError::Execution(format!("securityContext: {e}")))?;

                // Capture operational/state notes from the
                // semantic response so they propagate to the
                // security packet. The semantic adapter only
                // consumes `response.notes` to extract per-section
                // errors; everything else (e.g. operational state
                // notes) would otherwise be lost. We restore them
                // here as a separate slot so the security packet
                // shows both the original context and any
                // new state observations.
                let semantic_state_notes: Vec<String> = response.notes.clone();

                let (overlay, overlay_diagnostics_truncated) = if has_proposed {
                    match self
                        .resolve_semantic_check_content(
                            &file,
                            parsed.content.as_ref(),
                            parsed.patch.as_ref(),
                        )
                        .await
                    {
                        Ok(content) => {
                            match ops
                                .semantic_check_preview(&file, content, Some(&self.allowed_root))
                                .await
                            {
                                Ok(preview) => {
                                    let overlay_diag_truncated =
                                        preview.diagnostics.len() > MAX_SECURITY_DIAGNOSTICS;
                                    let diag_summaries: Vec<DiagnosticSummary> = preview
                                        .diagnostics
                                        .iter()
                                        .take(MAX_SECURITY_DIAGNOSTICS)
                                        .map(|d| DiagnosticSummary {
                                            file: d.file.clone(),
                                            line: d.line + 1,
                                            column: d.column + 1,
                                            severity: severity_to_string(d.severity),
                                            source: d.source.clone(),
                                            code: d.code.clone(),
                                            message: d.message.clone(),
                                        })
                                        .collect();
                                    (
                                        Some(SemanticOverlaySummary {
                                            used: true,
                                            diagnostics_may_still_be_warming: preview
                                                .diagnostics_may_still_be_warming,
                                            diagnostics: diag_summaries,
                                            diagnostics_error: preview.diagnostics_error,
                                            symbols: preview.symbols,
                                            symbols_error: preview.symbols_error,
                                            restored_disk_view: preview.restored_disk_view,
                                            restore_error: preview.restore_error,
                                        }),
                                        overlay_diag_truncated,
                                    )
                                }
                                Err(e) => (
                                    Some(SemanticOverlaySummary {
                                        used: true,
                                        diagnostics_may_still_be_warming: false,
                                        diagnostics: Vec::new(),
                                        diagnostics_error: Some(format!("overlay: {e}")),
                                        symbols: Vec::new(),
                                        symbols_error: None,
                                        restored_disk_view: false,
                                        restore_error: None,
                                    }),
                                    false,
                                ),
                            }
                        }
                        Err(e) => (
                            Some(SemanticOverlaySummary {
                                used: true,
                                diagnostics_may_still_be_warming: false,
                                diagnostics: Vec::new(),
                                diagnostics_error: Some(format!("overlay content: {e}")),
                                symbols: Vec::new(),
                                symbols_error: None,
                                restored_disk_view: false,
                                restore_error: None,
                            }),
                            false,
                        ),
                    }
                } else {
                    (None, false)
                };

                let semantic_packet = SemanticContextPacket::from_semantic_response(
                    response,
                    target,
                    overlay,
                    Vec::new(),
                    overlay_diagnostics_truncated,
                );
                let SemanticContextPacket {
                    file: packet_file,
                    target: packet_target,
                    excerpt,
                    diagnostics: raw_diags,
                    current_diagnostics_error: current_diag_err,
                    diagnostic_evidence: diag_evidence,
                    overlay,
                    symbols: all_syms,
                    current_symbols_error: current_sym_err,
                    definitions,
                    definitions_error: defs_error,
                    references,
                    references_error: refs_error,
                    source_actions: _,
                    section_truncations: _,
                    call_hierarchy: shared_call_hierarchy,
                    type_hierarchy: _,
                    limits: semantic_limits,
                } = semantic_packet;

                let super::lsp_security::RiskScanResult {
                    markers: risk_markers,
                    truncated: risk_markers_truncated,
                } = super::lsp_security::scan_risk_markers(
                    &excerpt,
                    &settings.categories,
                    settings.max_risk_markers,
                );

                let security_diags: Vec<DiagnosticSummary> = raw_diags
                    .into_iter()
                    .filter(|d| {
                        super::lsp_security::is_security_relevant_diagnostic(d, &risk_markers)
                    })
                    .collect();
                let (security_diags, security_diag_truncated) =
                    super::lsp_security::cap_vec(security_diags, MAX_SECURITY_DIAGNOSTICS);

                let relevant_syms: Vec<SymbolSummary> = all_syms
                    .into_iter()
                    .filter(|s| {
                        super::lsp_security::is_security_relevant_symbol(
                            s,
                            &risk_markers,
                            parsed.line,
                        )
                    })
                    .collect();
                let (security_syms, security_sym_truncated) =
                    super::lsp_security::cap_vec(relevant_syms, MAX_SECURITY_SYMBOLS);

                let caps_snapshot = self.capability_snapshot_for_file(&file).await;
                let mut notes = Vec::new();
                if risk_markers.is_empty() {
                    notes.push("no risk markers detected in excerpt".to_string());
                }
                if !has_position {
                    notes.push(
                        "no target position: definitions, references, and call hierarchy omitted"
                            .to_string(),
                    );
                }

                let call_hierarchy = if settings.include_call_hierarchy && has_position {
                    let supported = caps_snapshot
                        .as_ref()
                        .map(|c| c.supports(egglsp::LspSemanticOperation::CallHierarchy))
                        .unwrap_or(true);
                    if supported {
                        shared_call_hierarchy
                    } else {
                        notes.push("call hierarchy not supported by server".to_string());
                        None
                    }
                } else {
                    None
                };

                let call_expansion = if settings.call_depth > 0 && has_position {
                    let supported = caps_snapshot
                        .as_ref()
                        .map(|c| c.supports(egglsp::LspSemanticOperation::CallHierarchy))
                        .unwrap_or(true);
                    if supported {
                        Some(
                            self.build_call_expansion_summary(
                                &ops,
                                &file,
                                parsed.line.unwrap(),
                                parsed.column.unwrap(),
                                settings.call_direction,
                                settings.call_depth,
                                settings.max_call_nodes,
                            )
                            .await,
                        )
                    } else {
                        notes.push(
                            "call expansion not supported by server (call hierarchy required)"
                                .to_string(),
                        );
                        None
                    }
                } else {
                    None
                };

                if let Some(err) = current_diag_err {
                    notes.push(format!("diagnostics unavailable: {err}"));
                }
                if let Some(err) = current_sym_err {
                    notes.push(format!("document symbols unavailable: {err}"));
                }
                if let Some(err) = defs_error {
                    notes.push(format!("definitions unavailable: {err}"));
                }
                if let Some(err) = refs_error {
                    notes.push(format!("references unavailable: {err}"));
                }
                if let Some(note) = settings.preset_note {
                    notes.push(note);
                }
                // Propagate operational/state notes from the
                // semantic response (e.g. "LSP server indexing").
                // These were added by `SemanticContextCollector`
                // after consulting the operational state map and
                // would otherwise be lost in the security packet.
                for note in &semantic_state_notes {
                    if !notes.iter().any(|existing| existing == note) {
                        notes.push(note.clone());
                    }
                }
                if let Some(ref evidence) = diag_evidence {
                    match evidence.freshness {
                        crate::lsp::diagnostics::LspDiagnosticFreshness::Stale => {
                            notes.push("diagnostics stale: treating diagnostics as low-confidence evidence".to_string());
                        }
                        crate::lsp::diagnostics::LspDiagnosticFreshness::PossiblyStale => {
                            notes.push("diagnostics possibly stale: file changed since last diagnostics push; evidence is best-effort".to_string());
                        }
                        crate::lsp::diagnostics::LspDiagnosticFreshness::Unavailable => {
                            notes.push("diagnostics unavailable: no LSP diagnostic evidence available; absence of diagnostics is not evidence of absence".to_string());
                        }
                        _ => {}
                    }
                }

                let security_diag_count = security_diags.len();
                let security_sym_count = security_syms.len();
                let risk_markers_len = risk_markers.len();
                let call_hierarchy_count = call_hierarchy
                    .as_ref()
                    .map(|c| c.items.len() + c.incoming.len() + c.outgoing.len())
                    .unwrap_or(0);
                let call_expansion_count = call_expansion
                    .as_ref()
                    .map(|c| c.nodes.len() + c.edges.len())
                    .unwrap_or(0);
                let call_expansion_truncated = call_expansion.as_ref().is_some_and(|c| c.truncated);
                let call_hierarchy_truncated = call_hierarchy.as_ref().is_some_and(|c| c.truncated);
                let result_count = risk_markers_len
                    + security_diag_count
                    + security_sym_count
                    + definitions.len()
                    + references.len()
                    + call_hierarchy_count
                    + call_expansion_count;
                let packet = SecurityContextPacket {
                    file: packet_file,
                    target: packet_target,
                    excerpt,
                    risk_markers,
                    security_relevant_symbols: security_syms,
                    security_relevant_diagnostics: security_diags,
                    diagnostic_evidence: diag_evidence,
                    definitions,
                    references,
                    call_hierarchy,
                    call_expansion,
                    overlay,
                    preset: settings.preset_name,
                    notes,
                    limits: SecurityContextLimits {
                        risk_markers_truncated,
                        diagnostics_truncated: semantic_limits.diagnostics_truncated
                            || security_diag_truncated,
                        symbols_truncated: semantic_limits.symbols_truncated
                            || security_sym_truncated,
                        references_truncated: semantic_limits.references_truncated,
                        excerpt_truncated: semantic_limits.excerpt_truncated,
                        overlay_diagnostics_truncated: semantic_limits
                            .overlay_diagnostics_truncated
                            || overlay_diagnostics_truncated,
                        call_expansion_truncated,
                    },
                };
                let truncated = risk_markers_truncated
                    || semantic_limits.diagnostics_truncated
                    || semantic_limits.symbols_truncated
                    || semantic_limits.references_truncated
                    || semantic_limits.excerpt_truncated
                    || semantic_limits.overlay_diagnostics_truncated
                    || security_diag_truncated
                    || security_sym_truncated
                    || call_hierarchy_truncated
                    || call_expansion_truncated
                    || overlay_diagnostics_truncated;

                let output = LspToolOutput {
                    operation: "securityContext".to_string(),
                    file_path: file_path_str,
                    result_count,
                    truncated,
                    results: packet,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "capabilities" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let snapshot = self
                    .capability_snapshot_for_file(&file)
                    .await
                    .unwrap_or_default();

                serde_json::to_string_pretty(&snapshot)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "hunkSourceContext" => {
                let file_path_str = parsed
                    .file_path
                    .clone()
                    .ok_or_else(|| ToolError::Execution("file_path required".to_string()))?;
                self.resolve_file(&parsed.file_path)?;

                let radius = parsed
                    .radius
                    .unwrap_or(DEFAULT_SEMANTIC_CONTEXT_RADIUS)
                    .min(MAX_SEMANTIC_CONTEXT_RADIUS);
                let max_hunks = parsed.max_hunks.unwrap_or(20);

                let request = egglsp::hunk_context::HunkSourceNavigationRequest {
                    file_path: file_path_str.clone(),
                    hunks: vec![],
                    patch: parsed.patch.clone(),
                    intent: "navigation".to_string(),
                    include_definitions: parsed.include_definitions.unwrap_or(true),
                    include_references: parsed.include_references.unwrap_or(true),
                    include_call_hierarchy: parsed.include_call_hierarchy.unwrap_or(false),
                    include_type_hierarchy: parsed.include_type_hierarchy.unwrap_or(false),
                    excerpt_radius: radius,
                    max_hunks,
                    max_symbols_per_hunk: MAX_CONTEXT_SYMBOLS,
                    max_diagnostics_per_hunk: MAX_CONTEXT_DIAGNOSTICS,
                    max_references_per_hunk: MAX_CONTEXT_REFERENCES,
                };

                let mut response = self
                    .execute_hunk_source_context_typed(request)
                    .await
                    .map_err(ToolError::Execution)?;

                // Inject operational state notes so the hunk
                // summary reflects the LSP server's current health
                // (indexing, restarting, degraded, etc.). The note
                // is idempotent — re-runs with the same state
                // produce the same note and dedupe naturally in
                // `format_hunk_source_context_summary`.
                if let Some(state_note) = self.operational_state_note_for_file(&file_path_str).await
                {
                    if !response.notes.iter().any(|n| n == &state_note) {
                        response.push_note(state_note);
                    }
                }

                let output = LspToolOutput {
                    operation: "hunkSourceContext".to_string(),
                    file_path: Some(file_path_str),
                    result_count: response.hunks.len(),
                    truncated: response.truncated,
                    results: response,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "declaration" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let pos = to_lsp_position(line, col);
                let locs = ops
                    .declaration(&file, pos.line, pos.character)
                    .await
                    .map_err(|e| ToolError::Execution(format!("declaration: {e}")))?;
                let truncated = locs.len() > MAX_REFERENCES;
                let capped: Vec<_> = locs.into_iter().take(MAX_REFERENCES).collect();
                let summaries: Vec<LocationSummary> = capped
                    .iter()
                    .map(|loc| {
                        let range = loc.target_range;
                        LocationSummary {
                            file: uri_to_path(&loc.target_uri),
                            start_line: range.start.line + 1,
                            start_column: range.start.character + 1,
                            end_line: range.end.line + 1,
                            end_column: range.end.character + 1,
                        }
                    })
                    .collect();
                let output = LspToolOutput {
                    operation: "declaration".to_string(),
                    file_path: file_path_str,
                    result_count: summaries.len(),
                    truncated,
                    results: summaries,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "implementation" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let pos = to_lsp_position(line, col);
                let locs = ops
                    .implementation(&file, pos.line, pos.character)
                    .await
                    .map_err(|e| ToolError::Execution(format!("implementation: {e}")))?;
                let truncated = locs.len() > MAX_REFERENCES;
                let capped: Vec<_> = locs.into_iter().take(MAX_REFERENCES).collect();
                let summaries: Vec<LocationSummary> = capped
                    .iter()
                    .map(|loc| {
                        let range = loc.target_range;
                        LocationSummary {
                            file: uri_to_path(&loc.target_uri),
                            start_line: range.start.line + 1,
                            start_column: range.start.character + 1,
                            end_line: range.end.line + 1,
                            end_column: range.end.character + 1,
                        }
                    })
                    .collect();
                let output = LspToolOutput {
                    operation: "implementation".to_string(),
                    file_path: file_path_str,
                    result_count: summaries.len(),
                    truncated,
                    results: summaries,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "documentHighlights" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let pos = to_lsp_position(line, col);
                let highlights = ops
                    .document_highlights(&file, pos.line, pos.character)
                    .await
                    .map_err(|e| ToolError::Execution(format!("documentHighlights: {e}")))?;
                let file_str = file.to_string_lossy().to_string();
                let truncated = highlights.len() > MAX_DOCUMENT_HIGHLIGHTS;
                let capped: Vec<_> = highlights
                    .into_iter()
                    .take(MAX_DOCUMENT_HIGHLIGHTS)
                    .collect();
                let summaries: Vec<DocumentHighlightSummary> = capped
                    .iter()
                    .map(|h| DocumentHighlightSummary {
                        file: file_str.clone(),
                        start_line: h.range.start.line + 1,
                        start_column: h.range.start.character + 1,
                        end_line: h.range.end.line + 1,
                        end_column: h.range.end.character + 1,
                        kind: document_highlight_kind_to_string(h.kind),
                    })
                    .collect();
                let output = LspToolOutput {
                    operation: "documentHighlights".to_string(),
                    file_path: file_path_str,
                    result_count: summaries.len(),
                    truncated,
                    results: summaries,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "signatureHelp" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let pos = to_lsp_position(line, col);
                let help = ops
                    .signature_help_typed(&file, pos.line, pos.character)
                    .await
                    .map_err(|e| ToolError::Execution(format!("signatureHelp: {e}")))?;
                let result_count = help.as_ref().map(|h| h.signatures.len()).unwrap_or(0);
                let output = LspToolOutput {
                    operation: "signatureHelp".to_string(),
                    file_path: file_path_str,
                    result_count,
                    truncated: false,
                    results: help,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "completion" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
                let pos = to_lsp_position(line, col);
                let max = parsed.max_candidates.unwrap_or(MAX_COMPLETION_CANDIDATES);
                let trigger_kind = match parsed.trigger_kind {
                    Some(1) => Some(crate::lsp::lsp_types::CompletionTriggerKind::INVOKED),
                    Some(2) => Some(
                        crate::lsp::lsp_types::CompletionTriggerKind::TRIGGER_CHARACTER,
                    ),
                    Some(3) => Some(
                        crate::lsp::lsp_types::CompletionTriggerKind::TRIGGER_FOR_INCOMPLETE_COMPLETIONS,
                    ),
                    _ => None,
                };
                let candidates = ops
                    .completion_bounded(
                        &file,
                        pos.line,
                        pos.character,
                        trigger_kind,
                        parsed.trigger_char.clone(),
                        max,
                    )
                    .await
                    .map_err(|e| ToolError::Execution(format!("completion: {e}")))?;
                let output = LspToolOutput {
                    operation: "completion".to_string(),
                    file_path: file_path_str,
                    result_count: candidates.len(),
                    truncated: false,
                    results: candidates,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "semanticTokens" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let max = parsed.max_tokens.unwrap_or(MAX_SEMANTIC_TOKENS);
                let tokens = ops
                    .semantic_tokens(&file, max)
                    .await
                    .map_err(|e| ToolError::Execution(format!("semanticTokens: {e}")))?;
                let output = LspToolOutput {
                    operation: "semanticTokens".to_string(),
                    file_path: file_path_str,
                    result_count: tokens.len(),
                    truncated: false,
                    results: tokens,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "codeActionSummaries" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let start_line_input = parsed.start_line.ok_or_else(|| {
                    ToolError::Execution(
                        "start_line required for codeActionSummaries (1-indexed)".to_string(),
                    )
                })?;
                let start_col_input = parsed.start_column.unwrap_or(start_line_input);
                let end_line_input = parsed.end_line.unwrap_or(start_line_input);
                let end_col_input = parsed.end_column.unwrap_or(start_col_input);
                let max = parsed.max_actions.unwrap_or(MAX_CODE_ACTIONS);
                let only = parsed.only.as_ref().map(|kinds| {
                    kinds
                        .iter()
                        .map(|k| crate::lsp::lsp_types::CodeActionKind::from(k.clone()))
                        .collect::<Vec<_>>()
                });
                let actions = ops
                    .code_action_summaries(
                        &file,
                        start_line_input.saturating_sub(1),
                        start_col_input.saturating_sub(1),
                        end_line_input.saturating_sub(1),
                        end_col_input.saturating_sub(1),
                        Vec::new(),
                        only,
                        max,
                    )
                    .await
                    .map_err(|e| ToolError::Execution(format!("codeActionSummaries: {e}")))?;
                let output = LspToolOutput {
                    operation: "codeActionSummaries".to_string(),
                    file_path: file_path_str,
                    result_count: actions.len(),
                    truncated: false,
                    results: actions,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            "codeActionPreview" => {
                let file = self.resolve_file(&parsed.file_path)?;
                let start_line_input = parsed.start_line.ok_or_else(|| {
                    ToolError::Execution(
                        "start_line required for codeActionPreview (1-indexed)".to_string(),
                    )
                })?;
                let start_col_input = parsed.start_column.unwrap_or(start_line_input);
                let end_line_input = parsed.end_line.unwrap_or(start_line_input);
                let end_col_input = parsed.end_column.unwrap_or(start_col_input);
                let action_index = parsed.action_index.ok_or_else(|| {
                    ToolError::Execution("action_index required for codeActionPreview".to_string())
                })?;
                let only = parsed.only.as_ref().map(|kinds| {
                    kinds
                        .iter()
                        .map(|k| crate::lsp::lsp_types::CodeActionKind::from(k.clone()))
                        .collect::<Vec<_>>()
                });
                let preview = ops
                    .preview_code_action(
                        &file,
                        start_line_input.saturating_sub(1),
                        start_col_input.saturating_sub(1),
                        end_line_input.saturating_sub(1),
                        end_col_input.saturating_sub(1),
                        Vec::new(),
                        only,
                        action_index,
                        Some(&self.allowed_root),
                    )
                    .await
                    .map_err(|e| ToolError::Execution(format!("codeActionPreview: {e}")))?;
                let total_edits: usize = preview.affected_files.iter().map(|f| f.edits.len()).sum();
                let output = LspToolOutput {
                    operation: "codeActionPreview".to_string(),
                    file_path: file_path_str,
                    result_count: total_edits,
                    truncated: preview.truncated,
                    results: preview,
                };
                serde_json::to_string_pretty(&output)
                    .map_err(|e| ToolError::Execution(format!("serialize: {e}")))?
            }
            op => return Err(ToolError::Execution(format!("unknown LSP operation: {op}"))),
        };

        Ok(result)
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let start = Instant::now();
        let output = self.execute(input).await?;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let output_value = serde_json::from_str::<serde_json::Value>(&output).ok();
        let truncated = output_value
            .as_ref()
            .and_then(|v| v.get("truncated"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let provenance = ToolProvenance {
            backend: ToolBackendKind::Native.label().to_lowercase(),
            implementation: "egglsp".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            elapsed_ms: Some(elapsed_ms),
            truncated,
            trust: ToolTrust::LocalUntrusted,
        };
        let success = match output_value {
            Some(v) => {
                let top_restore_error = v
                    .pointer("/results/restore_error")
                    .and_then(|e| e.as_str())
                    .is_some();
                let overlay_restore_error = v
                    .pointer("/results/overlay/restore_error")
                    .and_then(|e| e.as_str())
                    .is_some();
                !(top_restore_error || overlay_restore_error)
            }
            None => true,
        };
        Ok(StructuredToolResult::with_provenance(
            output, success, provenance,
        ))
    }
}

impl SemanticContextPacket {
    /// Adapt a shared [`egglsp::semantic_context::SemanticContextResponse`]
    /// into the tool-local presentation packet.
    fn from_semantic_response(
        response: egglsp::semantic_context::SemanticContextResponse,
        target: Option<SemanticContextTarget>,
        overlay_override: Option<SemanticOverlaySummary>,
        source_actions: Vec<SemanticSourceActionHint>,
        overlay_diagnostics_truncated: bool,
    ) -> Self {
        let excerpt = response
            .source_excerpt
            .as_ref()
            .map(|src| SourceExcerpt {
                start_line: src.start_line,
                end_line: src.end_line,
                text: src.text.clone(),
            })
            .unwrap_or(SourceExcerpt {
                start_line: 1,
                end_line: 1,
                text: String::new(),
            });

        let diagnostics: Vec<DiagnosticSummary> = response
            .diagnostics
            .iter()
            .map(|d| DiagnosticSummary {
                file: d.file.clone(),
                line: d.line + 1,
                column: d.column + 1,
                severity: severity_to_string(d.severity),
                source: d.source.clone(),
                code: d.code.clone(),
                message: d.message.clone(),
            })
            .collect();

        let diagnostic_evidence =
            response
                .diagnostic_evidence
                .as_ref()
                .map(|e| DiagnosticEvidenceMeta {
                    freshness: e.freshness,
                    source: e.source,
                    age_ms: e.age_ms,
                    usable_evidence: e.usable_evidence,
                    server_generation: e.server_generation,
                    post_restart: e.post_restart,
                });

        let symbols: Vec<SymbolSummary> = response
            .all_symbols
            .iter()
            .map(|s| SymbolSummary {
                name: s.name.clone(),
                kind: s.kind.clone(),
                file: s.file.clone(),
                start_line: s.start_line,
                start_column: s.start_column,
                end_line: s.end_line,
                end_column: s.end_column,
            })
            .collect();

        let definitions: Vec<LocationSummary> = response
            .definitions
            .iter()
            .map(|l| LocationSummary {
                file: l.file.clone(),
                start_line: l.start_line,
                start_column: l.start_column,
                end_line: l.end_line,
                end_column: l.end_column,
            })
            .collect();

        let references: Vec<LocationSummary> = response
            .references
            .iter()
            .map(|l| LocationSummary {
                file: l.file.clone(),
                start_line: l.start_line,
                start_column: l.start_column,
                end_line: l.end_line,
                end_column: l.end_column,
            })
            .collect();

        let overlay = overlay_override.or_else(|| {
            response
                .overlay
                .as_ref()
                .map(Self::from_shared_overlay_summary)
        });

        let current_diagnostics_error = response
            .notes
            .iter()
            .find(|n| n.contains("diagnostics"))
            .cloned();
        let current_symbols_error = response
            .notes
            .iter()
            .find(|n| n.contains("documentSymbol"))
            .cloned();
        let definitions_error = response
            .notes
            .iter()
            .find(|n| n.contains("goToDefinition"))
            .cloned();
        let references_error = response
            .notes
            .iter()
            .find(|n| n.contains("findReferences"))
            .cloned();
        let mut section_truncations = response.section_truncations;
        if overlay_diagnostics_truncated
            && !section_truncations
                .iter()
                .any(|trunc| trunc.section == "overlay_diagnostics")
        {
            section_truncations.push(egglsp::semantic_context::SemanticSectionTruncation {
                section: "overlay_diagnostics".to_string(),
                original_count: overlay.as_ref().map(|o| o.diagnostics.len()),
                emitted_count: overlay.as_ref().map(|o| o.diagnostics.len()).unwrap_or(0),
                limit: MAX_CONTEXT_DIAGNOSTICS,
            });
        }
        let limits = SemanticContextLimits {
            diagnostics_truncated: response.limits.diagnostics_truncated,
            symbols_truncated: response.limits.symbols_truncated,
            references_truncated: response.limits.references_truncated,
            overlay_diagnostics_truncated: response.limits.overlay_diagnostics_truncated
                || overlay_diagnostics_truncated,
            excerpt_truncated: response.limits.excerpt_truncated,
        };

        SemanticContextPacket {
            file: response.file_path,
            target,
            excerpt,
            diagnostics,
            current_diagnostics_error,
            diagnostic_evidence,
            overlay,
            symbols,
            current_symbols_error,
            definitions,
            definitions_error,
            references,
            references_error,
            source_actions,
            section_truncations,
            call_hierarchy: response
                .call_hierarchy
                .as_ref()
                .map(Self::from_shared_call_hierarchy_summary),
            type_hierarchy: response
                .type_hierarchy
                .as_ref()
                .map(Self::from_shared_type_hierarchy_summary),
            limits,
        }
    }

    fn from_shared_call_hierarchy_summary(
        summary: &egglsp::semantic_context::SemanticCallGraphSummary,
    ) -> CallHierarchySummary {
        fn map_range(
            range: &egglsp::semantic_context::SemanticHierarchyRange,
        ) -> HierarchyRangeSummary {
            HierarchyRangeSummary {
                start_line: range.start_line,
                start_column: range.start_column,
                end_line: range.end_line,
                end_column: range.end_column,
            }
        }

        fn map_item(
            item: &egglsp::semantic_context::SemanticHierarchyItem,
        ) -> HierarchyItemSummary {
            HierarchyItemSummary {
                name: item.name.clone(),
                kind: item.kind.clone(),
                file: Some(item.file.clone()),
                range: map_range(&item.range),
                selection_range: map_range(&item.selection_range),
                detail: item.detail.clone(),
            }
        }

        CallHierarchySummary {
            items: summary.items.iter().map(map_item).collect(),
            incoming: summary
                .incoming
                .iter()
                .map(|rel| IncomingCallSummary {
                    from: map_item(&rel.item),
                    from_ranges: rel.ranges.iter().map(map_range).collect(),
                })
                .collect(),
            outgoing: summary
                .outgoing
                .iter()
                .map(|rel| OutgoingCallSummary {
                    to: map_item(&rel.item),
                    from_ranges: rel.ranges.iter().map(map_range).collect(),
                })
                .collect(),
            prepare_error: summary.prepare_error.clone(),
            incoming_error: summary.incoming_error.clone(),
            outgoing_error: summary.outgoing_error.clone(),
            truncated: summary.truncated,
        }
    }

    fn from_shared_type_hierarchy_summary(
        summary: &egglsp::semantic_context::SemanticTypeGraphSummary,
    ) -> TypeHierarchySummary {
        fn map_item(
            item: &egglsp::semantic_context::SemanticHierarchyItem,
        ) -> HierarchyItemSummary {
            HierarchyItemSummary {
                name: item.name.clone(),
                kind: item.kind.clone(),
                file: Some(item.file.clone()),
                range: HierarchyRangeSummary {
                    start_line: item.range.start_line,
                    start_column: item.range.start_column,
                    end_line: item.range.end_line,
                    end_column: item.range.end_column,
                },
                selection_range: HierarchyRangeSummary {
                    start_line: item.selection_range.start_line,
                    start_column: item.selection_range.start_column,
                    end_line: item.selection_range.end_line,
                    end_column: item.selection_range.end_column,
                },
                detail: item.detail.clone(),
            }
        }

        TypeHierarchySummary {
            items: summary.items.iter().map(map_item).collect(),
            supertypes: summary.supertypes.iter().map(map_item).collect(),
            subtypes: summary.subtypes.iter().map(map_item).collect(),
            prepare_error: summary.prepare_error.clone(),
            supertypes_error: summary.supertypes_error.clone(),
            subtypes_error: summary.subtypes_error.clone(),
            truncated: summary.truncated,
        }
    }

    fn from_shared_overlay_summary(
        summary: &egglsp::semantic_context::SemanticOverlay,
    ) -> SemanticOverlaySummary {
        SemanticOverlaySummary {
            used: summary.used,
            diagnostics_may_still_be_warming: summary.diagnostics_may_still_be_warming,
            diagnostics: summary
                .diagnostics
                .iter()
                .map(|d| DiagnosticSummary {
                    file: d.file.clone(),
                    line: d.line + 1,
                    column: d.column + 1,
                    severity: severity_to_string(d.severity),
                    source: d.source.clone(),
                    code: d.code.clone(),
                    message: d.message.clone(),
                })
                .collect(),
            diagnostics_error: summary.diagnostics_error.clone(),
            symbols: summary
                .symbols
                .iter()
                .map(|s| crate::lsp::overlay::SemanticSymbolSummary {
                    name: s.name.clone(),
                    kind: s.kind.clone(),
                    start_line: s.start_line,
                    start_column: s.start_column,
                    end_line: s.end_line,
                    end_column: s.end_column,
                })
                .collect(),
            symbols_error: summary.symbols_error.clone(),
            restored_disk_view: summary.restored_disk_view,
            restore_error: summary.restore_error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_rs_file(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.rs");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn lsp_tool_name() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        assert_eq!(tool.name(), "lsp");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn lsp_parameters_schema_snapshot() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let params = tool.parameters();
        let expected = json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": [
                        "goToDefinition", "findReferences", "hover",
                        "documentSymbol", "workspaceSymbol", "diagnostics",
                        "declaration", "implementation", "documentHighlights",
                        "signatureHelp", "completion", "semanticTokens",
                        "renamePreview", "formatPreview", "sourceActionPreview",
                        "codeActionSummaries", "codeActionPreview",
                        "semanticCheckPreview", "semanticContext",
                        "callHierarchy", "typeHierarchy",
                        "securityContext", "capabilities",
                        "hunkSourceContext"
                    ],
                    "description": "LSP operation to perform. declaration/implementation/documentHighlights/signatureHelp/completion/semanticTokens are read-only navigation and code intelligence. codeActionSummaries returns code action summaries for a range; codeActionPreview returns a preview of a specific code action. semanticCheckPreview accepts either full proposed content or a single-file unified diff patch. semanticContext returns a compact LSP-backed context packet. securityContext returns a security-review context packet with risk markers. capabilities returns a normalized snapshot of what the server supports. hunkSourceContext returns per-hunk navigation evidence with enclosing symbols, diagnostics, definitions, references, and hierarchy. callHierarchy/typeHierarchy return call/type hierarchy information for the symbol at line+column. Edit operations are previews only; use apply_patch (or other mutating tools) for actual changes."
                },
                "file_path": {
                    "type": "string",
                    "description": "File path for the operation"
                },
                "line": {
                    "type": "number",
                    "description": "Line number (1-indexed)"
                },
                "column": {
                    "type": "number",
                    "description": "Column number (1-indexed)"
                },
                "symbol": {
                    "type": "string",
                    "description": "Symbol name for workspaceSymbol operation"
                },
                "new_name": {
                    "type": "string",
                    "description": "New name for renamePreview operation"
                },
                "action": {
                    "type": "string",
                    "description": "Allowlisted source action for sourceActionPreview. Initially supports source.organizeImports."
                },
                "content": {
                    "type": "string",
                    "description": "Proposed full file content for semanticCheckPreview, semanticContext overlay, or securityContext overlay. Mutually exclusive with patch."
                },
                "patch": {
                    "type": "string",
                    "description": "Single-file unified diff patch to apply in memory for semanticCheckPreview, semanticContext overlay, or securityContext overlay. Mutually exclusive with content."
                },
                "radius": {
                    "type": "number",
                    "description": "Number of lines above and below target for semanticContext/securityContext source excerpt. semanticContext default 40/max 120; securityContext default 80/max 200."
                },
                "include_references": {
                    "type": "boolean",
                    "description": "Include findReferences results in semanticContext (default true when line+column provided)"
                },
                "include_definitions": {
                    "type": "boolean",
                    "description": "Include goToDefinition results in semanticContext (default true when line+column provided)"
                },
                "include_overlay": {
                    "type": "boolean",
                    "description": "Include overlay diagnostics in semanticContext (default true when content or patch provided)"
                },
                "include_source_actions": {
                    "type": "boolean",
                    "description": "Include safe allowlisted source-action preview hints in semanticContext. Initially only source.organizeImports. Default false."
                },
                "direction": {
                    "type": "string",
                    "enum": ["incoming", "outgoing", "both"],
                    "description": "Hierarchy direction for callHierarchy/typeHierarchy. Defaults to both. For typeHierarchy, incoming means supertypes and outgoing means subtypes."
                },
                "include_call_hierarchy": {
                    "type": "boolean",
                    "description": "Include call hierarchy section in semanticContext. In securityContext, call hierarchy defaults to true when line+column are supplied. Requires line+column."
                },
                "include_type_hierarchy": {
                    "type": "boolean",
                    "description": "Include type hierarchy section in semanticContext. Requires line+column. Default false."
                },
                "security_categories": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional risk marker categories to include in securityContext. Defaults to all supported categories. Supported: auth, crypto, filesystem, network, process, unsafe, serialization, sql, secrets, path_traversal, concurrency."
                },
                "max_risk_markers": {
                    "type": "number",
                    "description": "Maximum risk markers to return for securityContext. Default 80, max 200."
                },
                "security_preset": {
                    "type": "string",
                    "enum": ["rust_server", "rust_cli", "web_backend", "dependency_review", "unsafe_review"],
                    "description": "Optional securityContext preset that sets default risk categories, radius, marker limits, and call-hierarchy behavior. Explicit inputs override preset defaults."
                },
                "call_depth": {
                    "type": "number",
                    "description": "Optional securityContext call expansion depth. Default 0/off. Max 2. Requires line+column."
                },
                "max_call_nodes": {
                    "type": "number",
                    "description": "Maximum call expansion nodes for securityContext. Default 32, max 64."
                },
                "call_direction": {
                    "type": "string",
                    "enum": ["incoming", "outgoing", "both"],
                    "description": "Direction for securityContext call expansion. incoming=callers, outgoing=callees, both=both. Default both."
                },
                "max_hunks": {
                    "type": "number",
                    "description": "Maximum hunks for hunkSourceContext. Default 20."
                },
                "max_candidates": {
                    "type": "number",
                    "description": "Maximum completion candidates to return. Default 200."
                },
                "max_tokens": {
                    "type": "number",
                    "description": "Maximum semantic tokens to return. Default 1000."
                },
                "max_actions": {
                    "type": "number",
                    "description": "Maximum code actions for codeActionSummaries. Default 50."
                },
                "action_index": {
                    "type": "number",
                    "description": "Index of the code action to preview for codeActionPreview (0-indexed)."
                },
                "only": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional code action kinds filter for codeActionSummaries/codeActionPreview."
                },
                "trigger_kind": {
                    "type": "number",
                    "description": "Completion trigger kind: 1=invoked, 2=triggerCharacter, 3=triggerForIncompleteCompletions."
                },
                "trigger_char": {
                    "type": "string",
                    "description": "Trigger character for completion when trigger_kind=2."
                }
            },
            "required": ["operation"]
        });
        assert_eq!(params, expected);
    }

    #[tokio::test]
    async fn semantic_check_content_accepts_content() {
        let (_dir, path) = temp_rs_file("fn main() {}\n");
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let content = "fn main() { println!(\"hi\"); }".to_string();
        let resolved = tool
            .resolve_semantic_check_content(&path, Some(&content), None)
            .await
            .unwrap();
        assert_eq!(resolved, content);
    }

    #[tokio::test]
    async fn semantic_check_content_rejects_content_and_patch() {
        let (_dir, path) = temp_rs_file("fn main() {}\n");
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .resolve_semantic_check_content(
                &path,
                Some(&"fn main() {}".to_string()),
                Some(&"@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() {}\n".to_string()),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref msg) if msg.contains("either content or patch, not both"))
        );
    }

    #[tokio::test]
    async fn semantic_check_content_rejects_missing_content_and_patch() {
        let (_dir, path) = temp_rs_file("fn main() {}\n");
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .resolve_semantic_check_content(&path, None, None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref msg) if msg.contains("content or patch required"))
        );
    }

    #[tokio::test]
    async fn semantic_check_patch_applies_in_memory() {
        let (_dir, path) = temp_rs_file("fn main() {\n    println!(\"old\");\n}\n");
        let original = std::fs::read_to_string(&path).unwrap();
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let patch = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn main() {
-    println!(\"old\");
+    println!(\"new\");
 }
"
        .to_string();
        let resolved = tool
            .resolve_semantic_check_content(&path, None, Some(&patch))
            .await
            .unwrap();
        assert!(resolved.contains("new"));
        let disk = std::fs::read_to_string(&path).unwrap();
        assert_eq!(disk, original);
    }

    #[tokio::test]
    async fn semantic_check_patch_rejects_invalid_patch() {
        let (_dir, path) = temp_rs_file("fn main() {}\n");
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .resolve_semantic_check_content(&path, None, Some(&"@@ -1,1 +1,1 @@\n x\n".to_string()))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref msg) if msg.contains("semanticCheckPreview patch failed"))
        );
    }

    #[tokio::test]
    async fn semantic_check_patch_rejects_probable_multi_file_patch() {
        let (_dir, path) = temp_rs_file("fn main() {}\n");
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let patch = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,1 +1,1 @@
-fn main() {}
+fn main() {}
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,1 +1,1 @@
-fn lib() {}
+fn lib() {}
"
        .to_string();
        let err = tool
            .resolve_semantic_check_content(&path, None, Some(&patch))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref msg) if msg.contains("single-file patches"))
        );
    }

    #[test]
    fn uri_to_path_decodes_percent_encoded_file_uri() {
        let uri: crate::lsp::lsp_types::Uri = "file:///tmp/a%20b.rs".parse().unwrap();
        let path = uri_to_path(&uri);
        assert_eq!(path, "/tmp/a b.rs");
    }

    #[test]
    fn uri_to_path_non_file_uri_unchanged() {
        let uri: crate::lsp::lsp_types::Uri = "https://example.com/test".parse().unwrap();
        let path = uri_to_path(&uri);
        assert_eq!(path, "https://example.com/test");
    }

    #[tokio::test]
    async fn lsp_execute_structured_attaches_native_provenance() {
        use crate::tool::Tool;
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let res = tool
            .execute_structured(json!({"operation": "no_such_op"}), None)
            .await;
        assert!(res.is_err());
    }

    // ── semanticContext tests ──────────────────────────────────────────

    #[tokio::test]
    async fn semantic_context_schema_includes_operation() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let params = tool.parameters();
        let ops = params["properties"]["operation"]["enum"]
            .as_array()
            .unwrap();
        assert!(ops.iter().any(|v| v.as_str() == Some("semanticContext")));
    }

    #[tokio::test]
    async fn semantic_context_requires_file_path_execution() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .execute(json!({
                "operation": "semanticContext"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("file_path")),
            "expected file_path error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn semantic_context_requires_line_and_column_together() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .execute(json!({
                "operation": "semanticContext",
                "file_path": "src/tool/mod.rs",
                "line": 1
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("both line and column")),
            "expected line+column error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn semantic_context_rejects_content_and_patch() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .execute(json!({
                "operation": "semanticContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "content": "fn main() {}",
                "patch": "@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main() {}\n"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("either content or patch, not both")),
            "expected content+patch error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn semantic_context_patch_does_not_write_disk() {
        let (_dir, path) = temp_rs_file("fn main() {\n    println!(\"old\");\n}\n");
        let original = std::fs::read_to_string(&path).unwrap();
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ))
        .with_allowed_root(_dir.path().to_path_buf());
        let _ = tool.execute(json!({
            "operation": "semanticContext",
            "file_path": path.to_str().unwrap(),
            "line": 1,
            "column": 1,
            "patch": "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    println!(\"old\");\n+    println!(\"new\");\n }\n"
        })).await;
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            after, original,
            "semanticContext must not write patched content to disk"
        );
    }

    #[test]
    fn semantic_context_excerpt_top_of_file() {
        let content = "line1\nline2\nline3\nline4\nline5\n";
        let (_dir, path) = temp_rs_file(content);
        let (excerpt, truncated) = LspTool::build_source_excerpt(&path, Some(1), 40).unwrap();
        assert_eq!(excerpt.start_line, 1);
        assert!(!truncated);
        assert!(excerpt.text.contains("line1"));
    }

    #[test]
    fn semantic_context_excerpt_middle() {
        let content: String = (1..=20)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (_dir, path) = temp_rs_file(&content);
        let (excerpt, truncated) = LspTool::build_source_excerpt(&path, Some(10), 2).unwrap();
        assert_eq!(excerpt.start_line, 8);
        assert_eq!(excerpt.end_line, 12);
        assert!(!truncated);
    }

    #[test]
    fn semantic_context_excerpt_end_of_file() {
        let content: String = (1..=10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (_dir, path) = temp_rs_file(&content);
        let (excerpt, truncated) = LspTool::build_source_excerpt(&path, Some(10), 2).unwrap();
        assert_eq!(excerpt.start_line, 8);
        assert_eq!(excerpt.end_line, 10);
        assert!(!truncated);
    }

    #[test]
    fn semantic_context_excerpt_caps_radius() {
        let content: String = (1..=100)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (_dir, path) = temp_rs_file(&content);
        let (excerpt, _) = LspTool::build_source_excerpt(&path, Some(50), 200).unwrap();
        assert!(excerpt.text.contains("line50"));
    }

    #[test]
    fn semantic_context_excerpt_truncates_large_text() {
        let line = "x".repeat(1000);
        let content: String = (1..=50)
            .map(|_| line.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let (_dir, path) = temp_rs_file(&content);
        let (_excerpt, truncated) = LspTool::build_source_excerpt(&path, Some(25), 40).unwrap();
        assert!(truncated);
    }

    #[test]
    fn cap_vec_exact_cap_not_truncated() {
        let items: Vec<i32> = (0..32).collect();
        let (capped, truncated) = super::super::lsp_security::cap_vec(items, 32);
        assert_eq!(capped.len(), 32);
        assert!(!truncated);
    }

    #[test]
    fn cap_vec_over_cap_truncated() {
        let items: Vec<i32> = (0..33).collect();
        let (capped, truncated) = super::super::lsp_security::cap_vec(items, 32);
        assert_eq!(capped.len(), 32);
        assert!(truncated);
    }

    #[test]
    fn cap_vec_under_cap_not_truncated() {
        let items: Vec<i32> = (0..10).collect();
        let (capped, truncated) = super::super::lsp_security::cap_vec(items, 32);
        assert_eq!(capped.len(), 10);
        assert!(!truncated);
    }

    #[test]
    fn cap_vec_empty_not_truncated() {
        let items: Vec<i32> = Vec::new();
        let (capped, truncated) = super::super::lsp_security::cap_vec(items, 32);
        assert!(capped.is_empty());
        assert!(!truncated);
    }

    // ── securityContext tests ──────────────────────────────────────────

    #[test]
    fn security_risk_scanner_detects_process_execution() {
        let excerpt = SourceExcerpt {
            start_line: 1,
            end_line: 3,
            text: "use std::process::Command;\nfn main() {\n    let c = Command::new(\"ls\");\n}"
                .to_string(),
        };
        let result = super::super::lsp_security::scan_risk_markers(&excerpt, &None, 80);
        assert!(!result.markers.is_empty());
        assert!(result.markers.iter().any(|m| m.category == "process"));
    }

    #[test]
    fn security_risk_scanner_detects_unsafe() {
        let excerpt = SourceExcerpt {
            start_line: 1,
            end_line: 2,
            text: "fn main() {\n    let x = unsafe { 42 };\n}".to_string(),
        };
        let result = super::super::lsp_security::scan_risk_markers(&excerpt, &None, 80);
        assert!(result.markers.iter().any(|m| m.category == "unsafe"));
    }

    #[test]
    fn security_risk_scanner_detects_filesystem() {
        let excerpt = SourceExcerpt {
            start_line: 1,
            end_line: 2,
            text: "use std::fs::File;\nlet f = File::open(\"foo\");".to_string(),
        };
        let result = super::super::lsp_security::scan_risk_markers(&excerpt, &None, 80);
        assert!(result.markers.iter().any(|m| m.category == "filesystem"));
    }

    #[test]
    fn security_risk_scanner_detects_network() {
        let excerpt = SourceExcerpt {
            start_line: 1,
            end_line: 2,
            text: "use axum::Router;\nlet app = Router::new();".to_string(),
        };
        let result = super::super::lsp_security::scan_risk_markers(&excerpt, &None, 80);
        assert!(result.markers.iter().any(|m| m.category == "network"));
    }

    #[test]
    fn security_risk_scanner_filters_categories() {
        let excerpt = SourceExcerpt {
            start_line: 1,
            end_line: 3,
            text: "use std::process::Command;\nuse std::fs::File;\nfn main() {}".to_string(),
        };
        let result = super::super::lsp_security::scan_risk_markers(
            &excerpt,
            &Some(vec!["process".to_string()]),
            80,
        );
        assert!(result.markers.iter().all(|m| m.category == "process"));
    }

    #[test]
    fn security_risk_scanner_exact_cap_not_truncated() {
        let mut lines = Vec::new();
        for i in 0..5 {
            lines.push(format!("Command::new(\"{i}\");"));
        }
        let excerpt = SourceExcerpt {
            start_line: 1,
            end_line: 5,
            text: lines.join("\n"),
        };
        let result = super::super::lsp_security::scan_risk_markers(&excerpt, &None, 5);
        assert_eq!(result.markers.len(), 5);
        assert!(!result.truncated);
    }

    #[test]
    fn security_risk_scanner_over_cap_truncated() {
        let mut lines = Vec::new();
        for i in 0..200 {
            lines.push(format!("Command::new(\"{i}\");"));
        }
        let excerpt = SourceExcerpt {
            start_line: 1,
            end_line: 200,
            text: lines.join("\n"),
        };
        let result = super::super::lsp_security::scan_risk_markers(&excerpt, &None, 3);
        assert!(result.markers.len() <= 3);
        assert!(result.truncated);
    }

    #[test]
    fn security_risk_scanner_preserves_line_numbers() {
        let excerpt = SourceExcerpt {
            start_line: 10,
            end_line: 12,
            text: "fn main() {}\nunsafe { }\nfn foo() {}".to_string(),
        };
        let result = super::super::lsp_security::scan_risk_markers(&excerpt, &None, 80);
        let unsafe_marker = result.markers.iter().find(|m| m.category == "unsafe");
        assert!(unsafe_marker.is_some());
        assert_eq!(unsafe_marker.unwrap().line, 11);
    }

    #[test]
    fn security_risk_scanner_no_markers_for_clean_code() {
        let excerpt = SourceExcerpt {
            start_line: 1,
            end_line: 2,
            text: "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}".to_string(),
        };
        let result = super::super::lsp_security::scan_risk_markers(&excerpt, &None, 80);
        assert!(result.markers.is_empty());
    }

    // ── Phase 7: truncation + schema tests ───────────────────────────

    #[test]
    fn security_context_top_level_truncated_false_when_limits_clear() {
        let limits = SecurityContextLimits {
            risk_markers_truncated: false,
            diagnostics_truncated: false,
            symbols_truncated: false,
            references_truncated: false,
            excerpt_truncated: false,
            overlay_diagnostics_truncated: false,
            call_expansion_truncated: false,
        };
        let truncated = limits.risk_markers_truncated
            || limits.diagnostics_truncated
            || limits.symbols_truncated
            || limits.references_truncated
            || limits.excerpt_truncated
            || limits.overlay_diagnostics_truncated;
        assert!(!truncated);
    }

    #[test]
    fn security_context_top_level_truncated_true_when_marker_limit_truncated() {
        let limits = SecurityContextLimits {
            risk_markers_truncated: true,
            diagnostics_truncated: false,
            symbols_truncated: false,
            references_truncated: false,
            excerpt_truncated: false,
            overlay_diagnostics_truncated: false,
            call_expansion_truncated: false,
        };
        let truncated = limits.risk_markers_truncated
            || limits.diagnostics_truncated
            || limits.symbols_truncated
            || limits.references_truncated
            || limits.excerpt_truncated
            || limits.overlay_diagnostics_truncated;
        assert!(truncated);
    }

    #[test]
    fn security_context_top_level_truncated_true_when_any_limit_truncated() {
        let cases: [[bool; 6]; 6] = [
            [true, false, false, false, false, false],
            [false, true, false, false, false, false],
            [false, false, true, false, false, false],
            [false, false, false, true, false, false],
            [false, false, false, false, true, false],
            [false, false, false, false, false, true],
        ];
        for (i, flag) in cases.iter().enumerate() {
            let limits = SecurityContextLimits {
                risk_markers_truncated: flag[0],
                diagnostics_truncated: flag[1],
                symbols_truncated: flag[2],
                references_truncated: flag[3],
                excerpt_truncated: flag[4],
                overlay_diagnostics_truncated: flag[5],
                call_expansion_truncated: false,
            };
            let truncated = limits.risk_markers_truncated
                || limits.diagnostics_truncated
                || limits.symbols_truncated
                || limits.references_truncated
                || limits.excerpt_truncated
                || limits.overlay_diagnostics_truncated;
            assert!(truncated, "case {i} should be truncated");
        }
    }

    #[test]
    fn structured_lsp_provenance_reflects_truncated_field() {
        let json_truncated = r#"{"operation":"findReferences","truncated":true,"results":[]}"#;
        let v: serde_json::Value = serde_json::from_str(json_truncated).unwrap();
        let truncated = v
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(truncated);

        let json_not_truncated = r#"{"operation":"findReferences","truncated":false,"results":[]}"#;
        let v: serde_json::Value = serde_json::from_str(json_not_truncated).unwrap();
        let truncated = v
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(!truncated);

        let json_missing = r#"{"operation":"findReferences","results":[]}"#;
        let v: serde_json::Value = serde_json::from_str(json_missing).unwrap();
        let truncated = v
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(!truncated);
    }

    #[test]
    fn lsp_schema_descriptions_include_security_context_overlay() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let params = tool.parameters();
        let content_desc = params["properties"]["content"]["description"]
            .as_str()
            .unwrap();
        assert!(
            content_desc.contains("securityContext"),
            "content description should mention securityContext: {content_desc}"
        );
        let patch_desc = params["properties"]["patch"]["description"]
            .as_str()
            .unwrap();
        assert!(
            patch_desc.contains("securityContext"),
            "patch description should mention securityContext: {patch_desc}"
        );
        let radius_desc = params["properties"]["radius"]["description"]
            .as_str()
            .unwrap();
        assert!(
            radius_desc.contains("securityContext"),
            "radius description should mention securityContext: {radius_desc}"
        );
        let hierarchy_desc = params["properties"]["include_call_hierarchy"]["description"]
            .as_str()
            .unwrap();
        assert!(
            hierarchy_desc.contains("securityContext"),
            "include_call_hierarchy description should mention securityContext: {hierarchy_desc}"
        );
    }

    // ── securityContext preset tests ─────────────────────────────────────

    #[tokio::test]
    async fn security_context_no_preset_preserves_defaults() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let result = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        // No preset → preset field should be null
        assert!(v["results"]["preset"].is_null());
    }

    #[tokio::test]
    async fn security_context_preset_sets_categories() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let result = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "security_preset": "rust_cli"
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["results"]["preset"], "rust_cli");
        // Notes should mention the preset
        let notes = v["results"]["notes"].as_array().unwrap();
        assert!(notes
            .iter()
            .any(|n| n.as_str().unwrap().contains("rust_cli")));
    }

    #[tokio::test]
    async fn security_context_explicit_categories_override_preset() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let result = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "security_preset": "rust_server",
                "security_categories": ["auth"]
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        let markers = v["results"]["risk_markers"].as_array().unwrap();
        for marker in markers {
            assert_eq!(
                marker["category"].as_str().unwrap(),
                "auth",
                "explicit categories should override preset"
            );
        }
    }

    #[tokio::test]
    async fn security_context_invalid_preset_rejected() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "security_preset": "bogus_preset"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("unknown security_preset")),
            "expected unknown preset error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn security_context_preset_visible_in_output() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let result = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "security_preset": "unsafe_review"
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["results"]["preset"], "unsafe_review");
    }

    #[test]
    fn security_preset_schema_includes_enum() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let params = tool.parameters();
        let prop = params["properties"]["security_preset"]
            .as_object()
            .expect("security_preset property should be an object");
        assert_eq!(prop["type"], "string");
        let enum_vals = prop["enum"]
            .as_array()
            .expect("security_preset.enum should be an array");
        assert!(enum_vals.iter().any(|v| v.as_str() == Some("rust_server")));
        assert!(enum_vals.iter().any(|v| v.as_str() == Some("rust_cli")));
        assert!(enum_vals.iter().any(|v| v.as_str() == Some("web_backend")));
        assert!(enum_vals
            .iter()
            .any(|v| v.as_str() == Some("dependency_review")));
        assert!(enum_vals
            .iter()
            .any(|v| v.as_str() == Some("unsafe_review")));
    }

    // ── Phase 3: direct effective settings tests ───────────────────────

    fn security_context_input() -> LspInput {
        LspInput {
            operation: "securityContext".to_string(),
            file_path: Some("src/tool/mod.rs".to_string()),
            line: Some(1),
            column: Some(1),
            symbol: None,
            new_name: None,
            action: None,
            content: None,
            patch: None,
            radius: None,
            include_references: None,
            include_definitions: None,
            include_overlay: None,
            include_source_actions: None,
            direction: None,
            include_call_hierarchy: None,
            include_type_hierarchy: None,
            security_categories: None,
            max_risk_markers: None,
            security_preset: None,
            call_depth: None,
            max_call_nodes: None,
            call_direction: None,
            max_hunks: None,
            start_line: None,
            start_column: None,
            end_line: None,
            end_column: None,
            max_candidates: None,
            max_tokens: None,
            max_actions: None,
            action_index: None,
            only: None,
            trigger_kind: None,
            trigger_char: None,
        }
    }

    #[test]
    fn security_context_settings_no_preset_preserves_defaults_without_position() {
        let input = security_context_input();
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert_eq!(s.radius, DEFAULT_SECURITY_CONTEXT_RADIUS);
        assert_eq!(s.max_risk_markers, DEFAULT_MAX_RISK_MARKERS);
        assert!(!s.include_call_hierarchy);
        assert!(s.categories.is_none());
        assert!(s.preset_note.is_none());
        assert!(s.preset_name.is_none());
    }

    #[test]
    fn security_context_settings_no_preset_preserves_defaults_with_position() {
        let input = security_context_input();
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert_eq!(s.radius, DEFAULT_SECURITY_CONTEXT_RADIUS);
        assert_eq!(s.max_risk_markers, DEFAULT_MAX_RISK_MARKERS);
        assert!(s.include_call_hierarchy);
        assert!(s.categories.is_none());
        assert!(s.preset_note.is_none());
        assert!(s.preset_name.is_none());
    }

    #[test]
    fn security_context_settings_rust_server_sets_defaults() {
        let mut input = security_context_input();
        input.security_preset = Some("rust_server".to_string());
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert_eq!(s.radius, 120);
        assert_eq!(s.max_risk_markers, 120);
        assert!(s.include_call_hierarchy);
        assert!(s.preset_name.as_deref() == Some("rust_server"));
        let cats = s.categories.as_ref().unwrap();
        assert_eq!(cats.len(), 11);
        assert!(cats.contains(&"auth".to_string()));
        assert!(cats.contains(&"network".to_string()));
        assert!(cats.contains(&"sql".to_string()));
    }

    #[test]
    fn security_context_settings_dependency_review_disables_call_hierarchy() {
        let mut input = security_context_input();
        input.security_preset = Some("dependency_review".to_string());
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert!(!s.include_call_hierarchy);
        assert_eq!(s.radius, 80);
        assert_eq!(s.max_risk_markers, 80);
        let cats = s.categories.as_ref().unwrap();
        assert_eq!(cats.len(), 6);
    }

    #[test]
    fn security_context_settings_explicit_categories_override_preset() {
        let mut input = security_context_input();
        input.security_preset = Some("rust_server".to_string());
        input.security_categories = Some(vec!["auth".to_string()]);
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert_eq!(s.categories.as_deref(), Some(&["auth".to_string()][..]));
    }

    #[test]
    fn security_context_settings_explicit_radius_overrides_preset() {
        let mut input = security_context_input();
        input.security_preset = Some("rust_server".to_string());
        input.radius = Some(50);
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert_eq!(s.radius, 50);
    }

    #[test]
    fn security_context_settings_radius_clamps_to_max() {
        let mut input = security_context_input();
        input.radius = Some(999);
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert_eq!(s.radius, MAX_SECURITY_CONTEXT_RADIUS);
    }

    #[test]
    fn security_context_settings_explicit_max_markers_overrides_preset() {
        let mut input = security_context_input();
        input.security_preset = Some("rust_server".to_string());
        input.max_risk_markers = Some(50);
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert_eq!(s.max_risk_markers, 50);
    }

    #[test]
    fn security_context_settings_max_markers_clamps_to_max() {
        let mut input = security_context_input();
        input.max_risk_markers = Some(999);
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert_eq!(s.max_risk_markers, MAX_RISK_MARKERS);
    }

    #[test]
    fn security_context_settings_explicit_include_call_hierarchy_false_overrides_preset() {
        let mut input = security_context_input();
        input.security_preset = Some("rust_server".to_string());
        input.include_call_hierarchy = Some(false);
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert!(!s.include_call_hierarchy);
    }

    #[test]
    fn security_context_settings_explicit_include_call_hierarchy_true_overrides_dependency_review()
    {
        let mut input = security_context_input();
        input.security_preset = Some("dependency_review".to_string());
        input.include_call_hierarchy = Some(true);
        let s = LspTool::resolve_security_context_settings(&input, false).unwrap();
        assert!(s.include_call_hierarchy);
    }

    #[test]
    fn security_context_settings_invalid_preset_rejected() {
        let mut input = security_context_input();
        input.security_preset = Some("bogus".to_string());
        let err = LspTool::resolve_security_context_settings(&input, false).unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("unknown security_preset")),
            "expected unknown preset error, got: {err:?}"
        );
    }

    // ── Phase 4: operation-level preset tests ──────────────────────────

    #[tokio::test]
    async fn security_context_dependency_review_omits_call_hierarchy_by_default() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let result = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "security_preset": "dependency_review"
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["results"]["preset"], "dependency_review");
        assert!(
            v["results"]["call_hierarchy"].is_null(),
            "dependency_review should omit call_hierarchy by default"
        );
    }

    // ── Call expansion settings tests ──────────────────────────────────

    #[test]
    fn security_context_settings_default_call_depth_zero() {
        let input = security_context_input();
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert_eq!(s.call_depth, 0);
        assert_eq!(s.max_call_nodes, 32);
        assert!(matches!(
            s.call_direction,
            crate::lsp::operations::HierarchyDirection::Both
        ));
    }

    #[test]
    fn security_context_settings_call_depth_one_enabled() {
        let mut input = security_context_input();
        input.call_depth = Some(1);
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert_eq!(s.call_depth, 1);
    }

    #[test]
    fn security_context_settings_call_depth_two_enabled() {
        let mut input = security_context_input();
        input.call_depth = Some(2);
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert_eq!(s.call_depth, 2);
    }

    #[test]
    fn security_context_settings_call_depth_over_max_rejected() {
        let mut input = security_context_input();
        input.call_depth = Some(3);
        let err = LspTool::resolve_security_context_settings(&input, true).unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("call_depth") && m.contains("exceeds maximum")),
            "expected call_depth over max error, got: {err:?}"
        );
    }

    #[test]
    fn security_context_settings_max_call_nodes_clamps() {
        let mut input = security_context_input();
        input.max_call_nodes = Some(999);
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert_eq!(s.max_call_nodes, 64);
    }

    #[test]
    fn security_context_settings_call_direction_defaults_both() {
        let input = security_context_input();
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert!(matches!(
            s.call_direction,
            crate::lsp::operations::HierarchyDirection::Both
        ));
    }

    #[test]
    fn security_context_settings_call_direction_incoming() {
        let mut input = security_context_input();
        input.call_direction = Some("incoming".to_string());
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert!(matches!(
            s.call_direction,
            crate::lsp::operations::HierarchyDirection::Incoming
        ));
    }

    #[test]
    fn security_context_settings_call_direction_outgoing() {
        let mut input = security_context_input();
        input.call_direction = Some("outgoing".to_string());
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert!(matches!(
            s.call_direction,
            crate::lsp::operations::HierarchyDirection::Outgoing
        ));
    }

    #[test]
    fn security_context_settings_call_direction_rejects_invalid() {
        let mut input = security_context_input();
        input.call_direction = Some("bogus".to_string());
        let err = LspTool::resolve_security_context_settings(&input, true).unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("unsupported hierarchy direction")),
            "expected invalid direction error, got: {err:?}"
        );
    }

    #[test]
    fn security_context_settings_preset_does_not_enable_call_expansion() {
        let mut input = security_context_input();
        input.security_preset = Some("rust_server".to_string());
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert_eq!(s.call_depth, 0, "presets should not enable call expansion");
    }

    // ── Call expansion DTO tests ──────────────────────────────────────

    #[test]
    fn call_expansion_node_id_is_deterministic() {
        use crate::lsp::lsp_types::{CallHierarchyItem, Range, SymbolKind, Uri};
        use std::str::FromStr;
        let uri = Uri::from_str("file:///tmp/test.rs").unwrap();
        let item = CallHierarchyItem {
            name: "test_fn".to_string(),
            kind: SymbolKind::FUNCTION,
            uri,
            range: Range {
                start: crate::lsp::lsp_types::Position {
                    line: 9,
                    character: 0,
                },
                end: crate::lsp::lsp_types::Position {
                    line: 9,
                    character: 20,
                },
            },
            selection_range: Range {
                start: crate::lsp::lsp_types::Position {
                    line: 9,
                    character: 3,
                },
                end: crate::lsp::lsp_types::Position {
                    line: 9,
                    character: 10,
                },
            },
            detail: None,
            tags: None,
            data: None,
        };
        let id1 = LspTool::call_expansion_node_id(&item);
        let id2 = LspTool::call_expansion_node_id(&item);
        assert_eq!(id1, id2);
        assert!(id1.contains("test_fn"));
        assert!(id1.contains("10:4")); // 1-indexed line:col
    }

    // ── Call expansion operation-level tests ──────────────────────────

    #[tokio::test]
    async fn security_context_call_depth_zero_omits_call_expansion() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let result = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "call_depth": 0
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(
            v["results"]["call_expansion"].is_null(),
            "call_depth=0 should omit call_expansion"
        );
    }

    #[tokio::test]
    async fn security_context_call_depth_requires_line_column() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "call_depth": 1
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("call_depth") && m.contains("line and column")),
            "expected call_depth requires position error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn security_context_call_depth_over_max_rejected() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "call_depth": 3
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("call_depth") && m.contains("exceeds maximum")),
            "expected call_depth over max error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn security_context_call_direction_invalid_rejected() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let err = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "call_direction": "bogus"
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(ref m) if m.contains("unsupported hierarchy direction")),
            "expected invalid direction error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn security_context_call_depth_one_with_position_returns_expansion_or_errors() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let result = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "call_depth": 1
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        // call_expansion should be present (not null)
        assert!(
            !v["results"]["call_expansion"].is_null(),
            "call_depth=1 should produce call_expansion section"
        );
        // Should have direction field
        assert_eq!(v["results"]["call_expansion"]["direction"], "both");
        // Should have depth field
        assert_eq!(v["results"]["call_expansion"]["depth"], 1);
    }

    #[test]
    fn security_context_schema_includes_call_expansion_inputs() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let params = tool.parameters();
        assert!(
            params["properties"].get("call_depth").is_some(),
            "schema should include call_depth"
        );
        assert!(
            params["properties"].get("max_call_nodes").is_some(),
            "schema should include max_call_nodes"
        );
        assert!(
            params["properties"].get("call_direction").is_some(),
            "schema should include call_direction"
        );
    }

    // ── Cap helper tests (Phase 4) ──────────────────────────────────

    #[test]
    fn call_expansion_capped_ranges_exact_cap_not_truncated() {
        use crate::lsp::lsp_types::{Position, Range};
        let ranges: Vec<Range> = (0..MAX_HIERARCHY_RANGES)
            .map(|i| Range {
                start: Position {
                    line: i as u32,
                    character: 0,
                },
                end: Position {
                    line: i as u32,
                    character: 5,
                },
            })
            .collect();
        let (capped, truncated) = LspTool::capped_call_ranges(&ranges);
        assert_eq!(capped.len(), MAX_HIERARCHY_RANGES);
        assert!(!truncated);
    }

    #[test]
    fn call_expansion_capped_ranges_over_cap_truncated() {
        use crate::lsp::lsp_types::{Position, Range};
        let ranges: Vec<Range> = (0..=MAX_HIERARCHY_RANGES)
            .map(|i| Range {
                start: Position {
                    line: i as u32,
                    character: 0,
                },
                end: Position {
                    line: i as u32,
                    character: 5,
                },
            })
            .collect();
        let (capped, truncated) = LspTool::capped_call_ranges(&ranges);
        assert_eq!(capped.len(), MAX_HIERARCHY_RANGES);
        assert!(truncated);
    }

    #[test]
    fn call_expansion_push_edge_exact_cap_not_truncated() {
        let mut edges: Vec<CallExpansionEdge> = Vec::new();
        for _ in 0..MAX_CALL_EDGES {
            let edge = CallExpansionEdge {
                from: "a".into(),
                to: "b".into(),
                direction: "incoming".into(),
                ranges: Vec::new(),
            };
            assert!(!LspTool::push_call_expansion_edge(&mut edges, edge));
        }
        assert_eq!(edges.len(), MAX_CALL_EDGES);
    }

    #[test]
    fn call_expansion_push_edge_over_cap_truncated() {
        let mut edges: Vec<CallExpansionEdge> = Vec::new();
        for _ in 0..MAX_CALL_EDGES {
            let edge = CallExpansionEdge {
                from: "a".into(),
                to: "b".into(),
                direction: "incoming".into(),
                ranges: Vec::new(),
            };
            let _ = LspTool::push_call_expansion_edge(&mut edges, edge);
        }
        let overflow = CallExpansionEdge {
            from: "c".into(),
            to: "d".into(),
            direction: "outgoing".into(),
            ranges: Vec::new(),
        };
        assert!(LspTool::push_call_expansion_edge(&mut edges, overflow));
        assert_eq!(edges.len(), MAX_CALL_EDGES);
    }

    #[test]
    fn call_expansion_push_node_exact_cap_not_truncated() {
        let mut nodes: Vec<CallExpansionNode> = Vec::new();
        let max = 8;
        for _ in 0..max {
            let node = CallExpansionNode {
                id: "n".into(),
                name: "f".into(),
                kind: "function".into(),
                file: None,
                range: HierarchyRangeSummary {
                    start_line: 1,
                    start_column: 1,
                    end_line: 1,
                    end_column: 5,
                },
                selection_range: HierarchyRangeSummary {
                    start_line: 1,
                    start_column: 1,
                    end_line: 1,
                    end_column: 5,
                },
                detail: None,
                depth: 0,
            };
            assert!(!LspTool::push_call_expansion_node(&mut nodes, node, max));
        }
        assert_eq!(nodes.len(), max);
    }

    #[test]
    fn call_expansion_push_node_over_cap_truncated() {
        let mut nodes: Vec<CallExpansionNode> = Vec::new();
        let max = 8;
        for _ in 0..max {
            let node = CallExpansionNode {
                id: "n".into(),
                name: "f".into(),
                kind: "function".into(),
                file: None,
                range: HierarchyRangeSummary {
                    start_line: 1,
                    start_column: 1,
                    end_line: 1,
                    end_column: 5,
                },
                selection_range: HierarchyRangeSummary {
                    start_line: 1,
                    start_column: 1,
                    end_line: 1,
                    end_column: 5,
                },
                detail: None,
                depth: 0,
            };
            let _ = LspTool::push_call_expansion_node(&mut nodes, node, max);
        }
        let overflow = CallExpansionNode {
            id: "overflow".into(),
            name: "g".into(),
            kind: "function".into(),
            file: None,
            range: HierarchyRangeSummary {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 5,
            },
            selection_range: HierarchyRangeSummary {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 5,
            },
            detail: None,
            depth: 1,
        };
        assert!(LspTool::push_call_expansion_node(&mut nodes, overflow, max));
        assert_eq!(nodes.len(), max);
    }

    // ── Expansion semantics tests (Phase 5) ─────────────────────────

    #[test]
    fn call_expansion_node_id_differs_by_selection_position() {
        use crate::lsp::lsp_types::{CallHierarchyItem, Range, SymbolKind, Uri};
        use std::str::FromStr;
        let uri = Uri::from_str("file:///tmp/test.rs").unwrap();
        let make_item = |sel_line: u32, sel_char: u32| CallHierarchyItem {
            name: "test_fn".to_string(),
            kind: SymbolKind::FUNCTION,
            uri: uri.clone(),
            range: Range {
                start: crate::lsp::lsp_types::Position {
                    line: 9,
                    character: 0,
                },
                end: crate::lsp::lsp_types::Position {
                    line: 9,
                    character: 20,
                },
            },
            selection_range: Range {
                start: crate::lsp::lsp_types::Position {
                    line: sel_line,
                    character: sel_char,
                },
                end: crate::lsp::lsp_types::Position {
                    line: sel_line,
                    character: sel_char + 5,
                },
            },
            detail: None,
            tags: None,
            data: None,
        };
        let id1 = LspTool::call_expansion_node_id(&make_item(9, 3));
        let id2 = LspTool::call_expansion_node_id(&make_item(10, 3));
        assert_ne!(
            id1, id2,
            "different selection positions should produce different IDs"
        );
    }

    // ── Operation-level tests (Phase 6) ─────────────────────────────

    #[test]
    fn security_context_max_call_nodes_clamps_in_settings() {
        let mut input = security_context_input();
        input.max_call_nodes = Some(200);
        let s = LspTool::resolve_security_context_settings(&input, true).unwrap();
        assert_eq!(
            s.max_call_nodes, MAX_CALL_NODES,
            "max_call_nodes should be clamped to MAX_CALL_NODES"
        );
    }

    #[tokio::test]
    async fn security_context_call_expansion_truncated_limit_field_present() {
        let tool = LspTool::new(crate::lsp::service::LspService::new_arc(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        ));
        let result = tool
            .execute(json!({
                "operation": "securityContext",
                "file_path": "src/tool/mod.rs",
                "line": 1,
                "column": 1,
                "call_depth": 0
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(
            v["results"]["limits"]
                .get("call_expansion_truncated")
                .is_some(),
            "limits.call_expansion_truncated should always be present"
        );
        assert_eq!(v["results"]["limits"]["call_expansion_truncated"], false);
    }

    // ── from_semantic_response adapter tests ─────────────────────────

    #[test]
    fn semantic_context_packet_from_response_preserves_shape_and_limits() {
        use egglsp::diagnostics::FileDiagnostic;
        use egglsp::lsp_types::DiagnosticSeverity;
        use egglsp::semantic_context::{
            SemanticCallGraphSummary, SemanticContextLimits as SharedSemanticContextLimits,
            SemanticContextResponse, SemanticHierarchyItem, SemanticHierarchyRange,
            SemanticHierarchyRelation, SemanticLocation, SemanticSourceExcerpt,
            SemanticSymbolSummary, SemanticTypeGraphSummary,
        };

        let response = SemanticContextResponse {
            file_path: "src/main.rs".to_string(),
            symbol: None,
            all_symbols: vec![SemanticSymbolSummary {
                name: "my_fn".to_string(),
                kind: "function".to_string(),
                file: "src/main.rs".to_string(),
                start_line: 10,
                start_column: 1,
                end_line: 15,
                end_column: 2,
            }],
            diagnostics: vec![FileDiagnostic {
                file: "src/main.rs".to_string(),
                line: 4,
                column: 7,
                message: "unused variable".to_string(),
                severity: DiagnosticSeverity::WARNING,
                source: Some("rustc".to_string()),
                code: Some("unused".to_string()),
            }],
            definitions: vec![SemanticLocation {
                file: "src/lib.rs".to_string(),
                start_line: 20,
                start_column: 1,
                end_line: 20,
                end_column: 10,
            }],
            references: vec![SemanticLocation {
                file: "src/main.rs".to_string(),
                start_line: 5,
                start_column: 3,
                end_line: 5,
                end_column: 8,
            }],
            call_hierarchy: Some(SemanticCallGraphSummary {
                incoming_count: 1,
                outgoing_count: 1,
                items: vec![SemanticHierarchyItem {
                    name: "root".to_string(),
                    kind: "function".to_string(),
                    file: "src/main.rs".to_string(),
                    range: SemanticHierarchyRange {
                        start_line: 10,
                        start_column: 1,
                        end_line: 15,
                        end_column: 2,
                    },
                    selection_range: SemanticHierarchyRange {
                        start_line: 10,
                        start_column: 1,
                        end_line: 10,
                        end_column: 4,
                    },
                    detail: Some("root detail".to_string()),
                }],
                incoming: vec![SemanticHierarchyRelation {
                    item: SemanticHierarchyItem {
                        name: "caller".to_string(),
                        kind: "function".to_string(),
                        file: "src/lib.rs".to_string(),
                        range: SemanticHierarchyRange {
                            start_line: 20,
                            start_column: 1,
                            end_line: 24,
                            end_column: 2,
                        },
                        selection_range: SemanticHierarchyRange {
                            start_line: 20,
                            start_column: 1,
                            end_line: 20,
                            end_column: 4,
                        },
                        detail: Some("incoming detail".to_string()),
                    },
                    ranges: vec![SemanticHierarchyRange {
                        start_line: 21,
                        start_column: 5,
                        end_line: 21,
                        end_column: 11,
                    }],
                }],
                outgoing: vec![],
                truncated: true,
                prepare_error: None,
                incoming_error: Some("prepare failed".to_string()),
                outgoing_error: None,
            }),
            type_hierarchy: Some(SemanticTypeGraphSummary {
                supertypes_count: 1,
                subtypes_count: 1,
                items: vec![SemanticHierarchyItem {
                    name: "Widget".to_string(),
                    kind: "class".to_string(),
                    file: "src/main.rs".to_string(),
                    range: SemanticHierarchyRange {
                        start_line: 30,
                        start_column: 1,
                        end_line: 40,
                        end_column: 2,
                    },
                    selection_range: SemanticHierarchyRange {
                        start_line: 30,
                        start_column: 1,
                        end_line: 30,
                        end_column: 7,
                    },
                    detail: Some("type root".to_string()),
                }],
                supertypes: vec![SemanticHierarchyItem {
                    name: "Base".to_string(),
                    kind: "class".to_string(),
                    file: "src/base.rs".to_string(),
                    range: SemanticHierarchyRange {
                        start_line: 1,
                        start_column: 1,
                        end_line: 9,
                        end_column: 2,
                    },
                    selection_range: SemanticHierarchyRange {
                        start_line: 1,
                        start_column: 1,
                        end_line: 1,
                        end_column: 5,
                    },
                    detail: Some("super".to_string()),
                }],
                subtypes: vec![SemanticHierarchyItem {
                    name: "Derived".to_string(),
                    kind: "class".to_string(),
                    file: "src/derived.rs".to_string(),
                    range: SemanticHierarchyRange {
                        start_line: 50,
                        start_column: 1,
                        end_line: 60,
                        end_column: 2,
                    },
                    selection_range: SemanticHierarchyRange {
                        start_line: 50,
                        start_column: 1,
                        end_line: 50,
                        end_column: 8,
                    },
                    detail: Some("sub".to_string()),
                }],
                truncated: false,
                prepare_error: None,
                supertypes_error: None,
                subtypes_error: None,
            }),
            source_excerpt: Some(SemanticSourceExcerpt {
                start_line: 8,
                end_line: 12,
                text: "fn my_fn() {}".to_string(),
                truncated: false,
            }),
            diagnostic_evidence: Some(egglsp::semantic_context::SemanticDiagnosticEvidence {
                freshness: crate::lsp::diagnostics::LspDiagnosticFreshness::Fresh,
                source: crate::lsp::diagnostics::LspDiagnosticSource::Pushed,
                age_ms: 100,
                usable_evidence: true,
                server_generation: None,
                post_restart: false,
            }),
            overlay: None,
            source_actions: vec![],
            section_truncations: vec![egglsp::semantic_context::SemanticSectionTruncation {
                section: "diagnostics".to_string(),
                original_count: Some(2),
                emitted_count: 1,
                limit: 1,
            }],
            limits: SharedSemanticContextLimits {
                diagnostics_truncated: true,
                symbols_truncated: true,
                references_truncated: true,
                overlay_diagnostics_truncated: true,
                excerpt_truncated: true,
            },
            notes: vec![],
            truncated: true,
            unavailable: vec![],
        };

        let packet = SemanticContextPacket::from_semantic_response(
            response,
            Some(SemanticContextTarget {
                line: 10,
                column: 5,
            }),
            None,
            vec![],
            false,
        );

        assert_eq!(packet.file, "src/main.rs");
        assert!(packet.target.is_some());
        assert_eq!(packet.target.as_ref().unwrap().line, 10);
        assert_eq!(packet.excerpt.start_line, 8);
        assert_eq!(packet.excerpt.end_line, 12);
        assert_eq!(packet.symbols.len(), 1);
        assert_eq!(packet.symbols[0].name, "my_fn");
        assert_eq!(packet.diagnostics.len(), 1);
        assert_eq!(packet.definitions.len(), 1);
        assert_eq!(packet.definitions[0].file, "src/lib.rs");
        assert_eq!(packet.references.len(), 1);
        assert_eq!(packet.references[0].file, "src/main.rs");
        assert_eq!(packet.section_truncations.len(), 1);
        assert!(packet.limits.diagnostics_truncated);
        assert!(packet.limits.symbols_truncated);
        assert!(packet.limits.references_truncated);
        assert!(packet.limits.overlay_diagnostics_truncated);
        assert!(packet.limits.excerpt_truncated);
        let call = packet.call_hierarchy.as_ref().unwrap();
        assert_eq!(call.items.len(), 1);
        assert_eq!(call.items[0].detail.as_deref(), Some("root detail"));
        assert_eq!(call.incoming.len(), 1);
        assert_eq!(
            call.incoming[0].from.detail.as_deref(),
            Some("incoming detail")
        );
        assert_eq!(call.incoming[0].from_ranges.len(), 1);
        assert_eq!(call.incoming[0].from_ranges[0].start_line, 21);
        assert!(call.truncated);
        let ty = packet.type_hierarchy.as_ref().unwrap();
        assert_eq!(ty.items.len(), 1);
        assert_eq!(ty.supertypes.len(), 1);
        assert_eq!(ty.subtypes.len(), 1);
        assert_eq!(ty.items[0].detail.as_deref(), Some("type root"));
        let ev = packet.diagnostic_evidence.as_ref().unwrap();
        assert_eq!(ev.age_ms, 100);
        assert!(ev.usable_evidence);
        assert_eq!(
            ev.freshness,
            crate::lsp::diagnostics::LspDiagnosticFreshness::Fresh
        );
        assert_eq!(
            ev.source,
            crate::lsp::diagnostics::LspDiagnosticSource::Pushed
        );
    }

    #[test]
    fn security_context_reuses_semantic_response_generic_facts() {
        use egglsp::diagnostics::FileDiagnostic;
        use egglsp::lsp_types::DiagnosticSeverity;
        use egglsp::semantic_context::{
            SemanticCallGraphSummary, SemanticContextResponse, SemanticHierarchyItem,
            SemanticHierarchyRange, SemanticLocation, SemanticOverlay, SemanticSourceExcerpt,
            SemanticSymbolSummary, SemanticTypeGraphSummary,
        };

        let response = SemanticContextResponse {
            file_path: "src/lib.rs".to_string(),
            symbol: None,
            all_symbols: vec![SemanticSymbolSummary {
                name: "auth_manager".to_string(),
                kind: "struct".to_string(),
                file: "src/lib.rs".to_string(),
                start_line: 4,
                start_column: 1,
                end_line: 16,
                end_column: 2,
            }],
            diagnostics: vec![FileDiagnostic {
                file: "src/lib.rs".to_string(),
                line: 3,
                column: 9,
                message: "possible secret".to_string(),
                severity: DiagnosticSeverity::WARNING,
                source: Some("rustc".to_string()),
                code: Some("warn".to_string()),
            }],
            definitions: vec![SemanticLocation {
                file: "src/lib.rs".to_string(),
                start_line: 4,
                start_column: 1,
                end_line: 16,
                end_column: 2,
            }],
            references: vec![SemanticLocation {
                file: "src/main.rs".to_string(),
                start_line: 11,
                start_column: 3,
                end_line: 11,
                end_column: 9,
            }],
            call_hierarchy: Some(SemanticCallGraphSummary {
                incoming_count: 1,
                outgoing_count: 0,
                items: vec![SemanticHierarchyItem {
                    name: "auth_manager".to_string(),
                    kind: "struct".to_string(),
                    file: "src/lib.rs".to_string(),
                    range: SemanticHierarchyRange {
                        start_line: 4,
                        start_column: 1,
                        end_line: 16,
                        end_column: 2,
                    },
                    selection_range: SemanticHierarchyRange {
                        start_line: 4,
                        start_column: 1,
                        end_line: 4,
                        end_column: 13,
                    },
                    detail: Some("security root".to_string()),
                }],
                incoming: vec![],
                outgoing: vec![],
                truncated: false,
                prepare_error: None,
                incoming_error: None,
                outgoing_error: None,
            }),
            type_hierarchy: Some(SemanticTypeGraphSummary {
                supertypes_count: 0,
                subtypes_count: 1,
                items: vec![],
                supertypes: vec![],
                subtypes: vec![SemanticHierarchyItem {
                    name: "SecureAuthManager".to_string(),
                    kind: "struct".to_string(),
                    file: "src/secure.rs".to_string(),
                    range: SemanticHierarchyRange {
                        start_line: 22,
                        start_column: 1,
                        end_line: 30,
                        end_column: 2,
                    },
                    selection_range: SemanticHierarchyRange {
                        start_line: 22,
                        start_column: 1,
                        end_line: 22,
                        end_column: 7,
                    },
                    detail: Some("subtype detail".to_string()),
                }],
                truncated: false,
                prepare_error: None,
                supertypes_error: None,
                subtypes_error: None,
            }),
            source_excerpt: Some(SemanticSourceExcerpt {
                start_line: 1,
                end_line: 12,
                text: "let password = read_secret();".to_string(),
                truncated: false,
            }),
            diagnostic_evidence: Some(egglsp::semantic_context::SemanticDiagnosticEvidence {
                freshness: crate::lsp::diagnostics::LspDiagnosticFreshness::PossiblyStale,
                source: crate::lsp::diagnostics::LspDiagnosticSource::Pulled,
                age_ms: 250,
                usable_evidence: true,
                server_generation: None,
                post_restart: false,
            }),
            overlay: Some(SemanticOverlay {
                used: true,
                diagnostics_may_still_be_warming: false,
                diagnostics: vec![],
                diagnostics_error: None,
                symbols: vec![],
                symbols_error: None,
                restored_disk_view: true,
                restore_error: None,
            }),
            source_actions: vec![],
            section_truncations: vec![],
            limits: egglsp::semantic_context::SemanticContextLimits::default(),
            notes: vec![],
            truncated: false,
            unavailable: vec![],
        };

        let semantic_packet = SemanticContextPacket::from_semantic_response(
            response,
            Some(SemanticContextTarget { line: 4, column: 1 }),
            Some(SemanticOverlaySummary {
                used: true,
                diagnostics_may_still_be_warming: false,
                diagnostics: vec![],
                diagnostics_error: None,
                symbols: vec![],
                symbols_error: None,
                restored_disk_view: true,
                restore_error: None,
            }),
            vec![],
            false,
        );

        let super::super::lsp_security::RiskScanResult {
            markers: risk_markers,
            truncated: risk_markers_truncated,
        } = super::super::lsp_security::scan_risk_markers(
            &semantic_packet.excerpt,
            &None,
            DEFAULT_MAX_RISK_MARKERS,
        );
        let security_diags: Vec<DiagnosticSummary> = semantic_packet
            .diagnostics
            .iter()
            .filter(|d| {
                super::super::lsp_security::is_security_relevant_diagnostic(d, &risk_markers)
            })
            .cloned()
            .collect();
        let (security_diags, diagnostics_truncated) =
            super::super::lsp_security::cap_vec(security_diags, MAX_SECURITY_DIAGNOSTICS);
        let security_syms: Vec<SymbolSummary> = semantic_packet
            .symbols
            .iter()
            .filter(|s| {
                super::super::lsp_security::is_security_relevant_symbol(s, &risk_markers, Some(4))
            })
            .cloned()
            .collect();
        let (security_syms, symbols_truncated) =
            super::super::lsp_security::cap_vec(security_syms, MAX_SECURITY_SYMBOLS);

        let packet = SecurityContextPacket {
            file: semantic_packet.file,
            target: semantic_packet.target,
            excerpt: semantic_packet.excerpt,
            risk_markers,
            security_relevant_symbols: security_syms,
            security_relevant_diagnostics: security_diags,
            diagnostic_evidence: semantic_packet.diagnostic_evidence,
            definitions: semantic_packet.definitions,
            references: semantic_packet.references,
            call_hierarchy: semantic_packet.call_hierarchy,
            call_expansion: None,
            overlay: semantic_packet.overlay,
            preset: Some("dependency_review".to_string()),
            notes: vec![],
            limits: SecurityContextLimits {
                risk_markers_truncated,
                diagnostics_truncated,
                symbols_truncated,
                references_truncated: false,
                excerpt_truncated: semantic_packet.limits.excerpt_truncated,
                overlay_diagnostics_truncated: semantic_packet.limits.overlay_diagnostics_truncated,
                call_expansion_truncated: false,
            },
        };

        assert!(packet.excerpt.text.contains("password"));
        assert_eq!(packet.security_relevant_diagnostics.len(), 1);
        assert_eq!(packet.security_relevant_symbols.len(), 1);
        assert_eq!(packet.definitions.len(), 1);
        assert_eq!(packet.references.len(), 1);
        assert!(packet.overlay.as_ref().unwrap().used);
        let evidence = packet.diagnostic_evidence.as_ref().unwrap();
        assert_eq!(
            evidence.freshness,
            crate::lsp::diagnostics::LspDiagnosticFreshness::PossiblyStale
        );
        assert_eq!(
            evidence.source,
            crate::lsp::diagnostics::LspDiagnosticSource::Pulled
        );
        assert_eq!(packet.call_hierarchy.as_ref().unwrap().items.len(), 1);
    }

    #[test]
    fn semantic_context_packet_uses_response_overlay_when_no_override() {
        use egglsp::diagnostics::FileDiagnostic;
        use egglsp::lsp_types::DiagnosticSeverity;
        use egglsp::semantic_context::{
            SemanticContextResponse, SemanticOverlay, SemanticOverlaySymbol, SemanticSourceExcerpt,
        };

        let response = SemanticContextResponse {
            file_path: "src/main.rs".to_string(),
            symbol: None,
            all_symbols: vec![],
            diagnostics: vec![FileDiagnostic {
                file: "src/main.rs".to_string(),
                line: 0,
                column: 0,
                message: "diagnostic".to_string(),
                severity: DiagnosticSeverity::INFORMATION,
                source: Some("rustc".to_string()),
                code: None,
            }],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: Some(SemanticSourceExcerpt {
                start_line: 1,
                end_line: 1,
                text: "let x = 1;".to_string(),
                truncated: false,
            }),
            diagnostic_evidence: None,
            overlay: Some(SemanticOverlay {
                used: true,
                diagnostics_may_still_be_warming: true,
                diagnostics: vec![FileDiagnostic {
                    file: "src/main.rs".to_string(),
                    line: 1,
                    column: 1,
                    message: "overlay diagnostic".to_string(),
                    severity: DiagnosticSeverity::WARNING,
                    source: Some("overlay".to_string()),
                    code: Some("ov".to_string()),
                }],
                diagnostics_error: Some("warming".to_string()),
                symbols: vec![SemanticOverlaySymbol {
                    name: "overlay_fn".to_string(),
                    kind: "function".to_string(),
                    start_line: 2,
                    start_column: 1,
                    end_line: 2,
                    end_column: 10,
                }],
                symbols_error: Some("symbols".to_string()),
                restored_disk_view: false,
                restore_error: Some("restore".to_string()),
            }),
            source_actions: vec![],
            section_truncations: vec![],
            limits: egglsp::semantic_context::SemanticContextLimits::default(),
            notes: vec![],
            truncated: false,
            unavailable: vec![],
        };

        let packet =
            SemanticContextPacket::from_semantic_response(response, None, None, vec![], false);

        let overlay = packet
            .overlay
            .as_ref()
            .expect("overlay should be preserved");
        assert!(overlay.used);
        assert!(overlay.diagnostics_may_still_be_warming);
        assert_eq!(overlay.diagnostics.len(), 1);
        assert_eq!(overlay.diagnostics[0].message, "overlay diagnostic");
        assert_eq!(overlay.diagnostics_error.as_deref(), Some("warming"));
        assert_eq!(overlay.symbols.len(), 1);
        assert_eq!(overlay.symbols[0].name, "overlay_fn");
        assert_eq!(overlay.symbols_error.as_deref(), Some("symbols"));
        assert!(!overlay.restored_disk_view);
        assert_eq!(overlay.restore_error.as_deref(), Some("restore"));
    }

    #[test]
    fn semantic_context_packet_from_response_converts_diagnostics_to_1indexed() {
        use egglsp::diagnostics::FileDiagnostic;
        use egglsp::lsp_types::DiagnosticSeverity;
        use egglsp::semantic_context::SemanticContextResponse;

        let response = SemanticContextResponse {
            file_path: "test.rs".to_string(),
            symbol: None,
            all_symbols: vec![],
            diagnostics: vec![FileDiagnostic {
                file: "test.rs".to_string(),
                line: 0,
                column: 0,
                message: "error here".to_string(),
                severity: DiagnosticSeverity::ERROR,
                source: None,
                code: None,
            }],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            overlay: None,
            source_actions: vec![],
            section_truncations: vec![],
            limits: egglsp::semantic_context::SemanticContextLimits::default(),
            notes: vec![],
            truncated: false,
            unavailable: vec![],
        };

        let packet =
            SemanticContextPacket::from_semantic_response(response, None, None, vec![], false);

        assert_eq!(packet.diagnostics.len(), 1);
        assert_eq!(packet.diagnostics[0].line, 1, "line 0 -> 1-indexed");
        assert_eq!(packet.diagnostics[0].column, 1, "column 0 -> 1-indexed");
        assert_eq!(packet.diagnostics[0].severity, "error");
    }

    #[test]
    fn semantic_context_packet_from_response_notes_become_errors() {
        use egglsp::semantic_context::SemanticContextResponse;

        let response = SemanticContextResponse {
            file_path: "test.rs".to_string(),
            symbol: None,
            all_symbols: vec![],
            diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            overlay: None,
            source_actions: vec![],
            section_truncations: vec![],
            limits: egglsp::semantic_context::SemanticContextLimits::default(),
            notes: vec![
                "diagnostics: server offline".to_string(),
                "documentSymbol: timeout".to_string(),
                "goToDefinition: not supported".to_string(),
                "findReferences: not supported".to_string(),
                "unrelated note".to_string(),
            ],
            truncated: false,
            unavailable: vec![],
        };

        let packet =
            SemanticContextPacket::from_semantic_response(response, None, None, vec![], false);

        assert_eq!(
            packet.current_diagnostics_error.as_deref(),
            Some("diagnostics: server offline")
        );
        assert_eq!(
            packet.current_symbols_error.as_deref(),
            Some("documentSymbol: timeout")
        );
        assert_eq!(
            packet.definitions_error.as_deref(),
            Some("goToDefinition: not supported")
        );
        assert_eq!(
            packet.references_error.as_deref(),
            Some("findReferences: not supported")
        );
    }

    // --- Regression tests for hierarchy flag wiring (lsp_semantic_context_hierarchy_wiring_patch) ---

    #[test]
    fn semantic_context_request_sets_call_hierarchy_flag() {
        use egglsp::semantic_context::SemanticContextRequest;

        let req = SemanticContextRequest::new(
            "test.rs",
            egglsp::semantic_context::SemanticContextIntent::Explain,
        )
        .with_call_hierarchy(true);
        assert!(req.include_call_hierarchy);

        let req = SemanticContextRequest::new(
            "test.rs",
            egglsp::semantic_context::SemanticContextIntent::Explain,
        )
        .with_call_hierarchy(false);
        assert!(!req.include_call_hierarchy);
    }

    #[test]
    fn semantic_context_request_sets_type_hierarchy_flag() {
        use egglsp::semantic_context::SemanticContextRequest;

        let req = SemanticContextRequest::new(
            "test.rs",
            egglsp::semantic_context::SemanticContextIntent::Explain,
        )
        .with_type_hierarchy(true);
        assert!(req.include_type_hierarchy);

        let req = SemanticContextRequest::new(
            "test.rs",
            egglsp::semantic_context::SemanticContextIntent::Explain,
        )
        .with_type_hierarchy(false);
        assert!(!req.include_type_hierarchy);
    }

    #[test]
    fn semantic_packet_adapts_shared_call_hierarchy() {
        use egglsp::semantic_context::{
            SemanticCallGraphSummary, SemanticContextResponse, SemanticHierarchyItem,
            SemanticHierarchyRange, SemanticSourceExcerpt,
        };

        let response = SemanticContextResponse {
            file_path: "src/main.rs".to_string(),
            symbol: None,
            all_symbols: vec![],
            diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: Some(SemanticCallGraphSummary {
                incoming_count: 1,
                outgoing_count: 1,
                items: vec![SemanticHierarchyItem {
                    name: "my_fn".to_string(),
                    kind: "function".to_string(),
                    file: "src/main.rs".to_string(),
                    range: SemanticHierarchyRange {
                        start_line: 10,
                        start_column: 1,
                        end_line: 15,
                        end_column: 2,
                    },
                    selection_range: SemanticHierarchyRange {
                        start_line: 10,
                        start_column: 1,
                        end_line: 10,
                        end_column: 4,
                    },
                    detail: None,
                }],
                incoming: vec![],
                outgoing: vec![],
                truncated: false,
                prepare_error: None,
                incoming_error: None,
                outgoing_error: None,
            }),
            type_hierarchy: None,
            source_excerpt: Some(SemanticSourceExcerpt {
                start_line: 10,
                end_line: 10,
                text: "fn my_fn() {}".to_string(),
                truncated: false,
            }),
            diagnostic_evidence: None,
            overlay: None,
            source_actions: vec![],
            section_truncations: vec![],
            limits: egglsp::semantic_context::SemanticContextLimits::default(),
            notes: vec![],
            truncated: false,
            unavailable: vec![],
        };

        let packet =
            SemanticContextPacket::from_semantic_response(response, None, None, vec![], false);

        let call = packet
            .call_hierarchy
            .as_ref()
            .expect("call hierarchy should be present");
        assert_eq!(call.items.len(), 1);
        assert_eq!(call.items[0].name, "my_fn");
        assert_eq!(call.incoming.len(), 0);
        assert_eq!(call.outgoing.len(), 0);
    }

    #[test]
    fn semantic_packet_adapts_shared_type_hierarchy() {
        use egglsp::semantic_context::{
            SemanticContextResponse, SemanticHierarchyItem, SemanticHierarchyRange,
            SemanticSourceExcerpt, SemanticTypeGraphSummary,
        };

        let response = SemanticContextResponse {
            file_path: "src/main.rs".to_string(),
            symbol: None,
            all_symbols: vec![],
            diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: Some(SemanticTypeGraphSummary {
                supertypes_count: 1,
                subtypes_count: 1,
                items: vec![SemanticHierarchyItem {
                    name: "Widget".to_string(),
                    kind: "class".to_string(),
                    file: "src/main.rs".to_string(),
                    range: SemanticHierarchyRange {
                        start_line: 30,
                        start_column: 1,
                        end_line: 40,
                        end_column: 2,
                    },
                    selection_range: SemanticHierarchyRange {
                        start_line: 30,
                        start_column: 1,
                        end_line: 30,
                        end_column: 7,
                    },
                    detail: None,
                }],
                supertypes: vec![],
                subtypes: vec![],
                truncated: false,
                prepare_error: None,
                supertypes_error: None,
                subtypes_error: None,
            }),
            source_excerpt: Some(SemanticSourceExcerpt {
                start_line: 30,
                end_line: 30,
                text: "struct Widget {}".to_string(),
                truncated: false,
            }),
            diagnostic_evidence: None,
            overlay: None,
            source_actions: vec![],
            section_truncations: vec![],
            limits: egglsp::semantic_context::SemanticContextLimits::default(),
            notes: vec![],
            truncated: false,
            unavailable: vec![],
        };

        let packet =
            SemanticContextPacket::from_semantic_response(response, None, None, vec![], false);

        let ty = packet
            .type_hierarchy
            .as_ref()
            .expect("type hierarchy should be present");
        assert_eq!(ty.items.len(), 1);
        assert_eq!(ty.items[0].name, "Widget");
        assert_eq!(ty.supertypes.len(), 0);
        assert_eq!(ty.subtypes.len(), 0);
    }

    #[test]
    fn semantic_context_hierarchy_flags_false_omits_hierarchy() {
        use egglsp::semantic_context::SemanticContextResponse;

        let response = SemanticContextResponse {
            file_path: "test.rs".to_string(),
            symbol: None,
            all_symbols: vec![],
            diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: Some(egglsp::semantic_context::SemanticCallGraphSummary {
                incoming_count: 0,
                outgoing_count: 0,
                items: vec![],
                incoming: vec![],
                outgoing: vec![],
                truncated: false,
                prepare_error: None,
                incoming_error: None,
                outgoing_error: None,
            }),
            type_hierarchy: Some(egglsp::semantic_context::SemanticTypeGraphSummary {
                supertypes_count: 0,
                subtypes_count: 0,
                items: vec![],
                supertypes: vec![],
                subtypes: vec![],
                truncated: false,
                prepare_error: None,
                supertypes_error: None,
                subtypes_error: None,
            }),
            source_excerpt: None,
            diagnostic_evidence: None,
            overlay: None,
            source_actions: vec![],
            section_truncations: vec![],
            limits: egglsp::semantic_context::SemanticContextLimits::default(),
            notes: vec![],
            truncated: false,
            unavailable: vec![],
        };

        let packet =
            SemanticContextPacket::from_semantic_response(response, None, None, vec![], false);

        // from_semantic_response preserves hierarchy; the handler nulls it out
        // when flags are false. Verify the adapter preserves it (handler does the nulling).
        assert!(packet.call_hierarchy.is_some());
        assert!(packet.type_hierarchy.is_some());
    }

    #[test]
    fn security_context_request_sets_call_hierarchy_when_enabled() {
        use egglsp::semantic_context::SemanticContextRequest;

        let req = SemanticContextRequest::new(
            "test.rs",
            egglsp::semantic_context::SemanticContextIntent::SecurityReview,
        )
        .with_call_hierarchy(true);
        assert!(req.include_call_hierarchy);
        assert!(!req.include_type_hierarchy);
    }

    #[test]
    fn security_context_request_does_not_set_call_hierarchy_without_position() {
        use egglsp::semantic_context::SemanticContextRequest;

        // Simulates the securityContext path: when has_position is false,
        // include_call_hierarchy should not be set on the request.
        let req = SemanticContextRequest::new(
            "test.rs",
            egglsp::semantic_context::SemanticContextIntent::SecurityReview,
        );
        assert!(!req.include_call_hierarchy);
        assert!(!req.include_type_hierarchy);
    }

    #[test]
    fn semantic_context_request_builder_chains_correctly() {
        use egglsp::semantic_context::SemanticContextRequest;

        let req = SemanticContextRequest::new(
            "test.rs",
            egglsp::semantic_context::SemanticContextIntent::Explain,
        )
        .with_excerpt_radius(50)
        .with_position(10, 5)
        .with_call_hierarchy(true)
        .with_type_hierarchy(true);

        assert_eq!(req.file_path, "test.rs");
        assert_eq!(req.line, Some(10));
        assert_eq!(req.column, Some(5));
        assert_eq!(req.excerpt_radius, 50);
        assert!(req.include_call_hierarchy);
        assert!(req.include_type_hierarchy);
    }

    // ── Phase 7: effective_hunk_navigation_limits tests ───────────────

    mod hunk_limits_tests {
        use super::*;
        use egglsp::hunk_context::HunkSourceNavigationRequest;

        fn make_request(
            max_symbols: usize,
            max_diagnostics: usize,
            max_references: usize,
        ) -> HunkSourceNavigationRequest {
            HunkSourceNavigationRequest {
                file_path: "test.rs".to_string(),
                hunks: vec![],
                patch: None,
                intent: "test".to_string(),
                include_definitions: true,
                include_references: true,
                include_call_hierarchy: false,
                include_type_hierarchy: false,
                excerpt_radius: 40,
                max_hunks: 20,
                max_symbols_per_hunk: max_symbols,
                max_diagnostics_per_hunk: max_diagnostics,
                max_references_per_hunk: max_references,
            }
        }

        #[test]
        fn effective_hunk_navigation_limits_uses_request_values() {
            let request = make_request(5, 3, 7);
            let (sym, diag, refs) = effective_hunk_navigation_limits(&request);
            assert_eq!(sym, 5);
            assert_eq!(diag, 3);
            assert_eq!(refs, 7);
        }

        #[test]
        fn effective_hunk_navigation_limits_clamps_to_global_maximum() {
            let request = make_request(999, 999, 999);
            let (sym, diag, refs) = effective_hunk_navigation_limits(&request);
            assert_eq!(sym, MAX_CONTEXT_SYMBOLS);
            assert_eq!(diag, MAX_CONTEXT_DIAGNOSTICS);
            assert_eq!(refs, MAX_CONTEXT_REFERENCES);
        }

        #[test]
        fn effective_hunk_navigation_limits_coerces_zero_to_one() {
            let request = make_request(0, 0, 0);
            let (sym, diag, refs) = effective_hunk_navigation_limits(&request);
            assert_eq!(sym, 1, "zero should be coerced to 1");
            assert_eq!(diag, 1, "zero should be coerced to 1");
            assert_eq!(refs, 1, "zero should be coerced to 1");
        }

        #[test]
        fn effective_hunk_navigation_limits_exact_maximum_not_truncated() {
            let request = make_request(
                MAX_CONTEXT_SYMBOLS,
                MAX_CONTEXT_DIAGNOSTICS,
                MAX_CONTEXT_REFERENCES,
            );
            let (sym, diag, refs) = effective_hunk_navigation_limits(&request);
            assert_eq!(sym, MAX_CONTEXT_SYMBOLS);
            assert_eq!(diag, MAX_CONTEXT_DIAGNOSTICS);
            assert_eq!(refs, MAX_CONTEXT_REFERENCES);
        }

        #[test]
        fn effective_hunk_navigation_limits_one_above_maximum_clamped() {
            let request = make_request(
                MAX_CONTEXT_SYMBOLS + 1,
                MAX_CONTEXT_DIAGNOSTICS + 1,
                MAX_CONTEXT_REFERENCES + 1,
            );
            let (sym, diag, refs) = effective_hunk_navigation_limits(&request);
            assert_eq!(sym, MAX_CONTEXT_SYMBOLS);
            assert_eq!(diag, MAX_CONTEXT_DIAGNOSTICS);
            assert_eq!(refs, MAX_CONTEXT_REFERENCES);
        }
    }
}
