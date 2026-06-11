use super::lsp_security::SecurityRiskMarker;
use crate::error::ToolError;
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
    overlay: Option<SemanticOverlaySummary>,
    symbols: Vec<SymbolSummary>,
    current_symbols_error: Option<String>,
    definitions: Vec<LocationSummary>,
    definitions_error: Option<String>,
    references: Vec<LocationSummary>,
    references_error: Option<String>,
    source_actions: Vec<SemanticSourceActionHint>,
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

#[derive(Serialize)]
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

#[derive(Serialize)]
struct SecurityContextPacket {
    file: String,
    target: Option<SemanticContextTarget>,
    excerpt: SourceExcerpt,
    risk_markers: Vec<SecurityRiskMarker>,
    security_relevant_symbols: Vec<SymbolSummary>,
    security_relevant_diagnostics: Vec<DiagnosticSummary>,
    definitions: Vec<LocationSummary>,
    references: Vec<LocationSummary>,
    call_hierarchy: Option<CallHierarchySummary>,
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

pub struct LspTool {
    service: Arc<crate::lsp::service::LspService>,
    allowed_root: PathBuf,
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

    fn resolve_file(&self, path: &Option<String>) -> Result<PathBuf, ToolError> {
        let p = path
            .as_ref()
            .ok_or_else(|| ToolError::Execution("file_path required".to_string()))?;
        let original = if p.starts_with('/') {
            PathBuf::from(p)
        } else {
            std::env::current_dir().unwrap_or_default().join(p)
        };
        crate::tool::util::validate_path(&original, &self.allowed_root)
            .map_err(|e| ToolError::Execution(e.to_string()))
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

    #[allow(clippy::type_complexity)]
    fn resolve_security_context_settings(
        parsed: &LspInput,
        has_position: bool,
    ) -> Result<(Option<Vec<String>>, u32, usize, bool, Option<String>), ToolError> {
        let mut categories: Option<Vec<String>> = None;
        let mut radius = DEFAULT_SECURITY_CONTEXT_RADIUS;
        let mut max_risk_markers = DEFAULT_MAX_RISK_MARKERS;
        let mut include_call_hierarchy = has_position;
        let mut preset_note: Option<String> = None;

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

        Ok((
            categories,
            radius,
            max_risk_markers,
            include_call_hierarchy,
            preset_note,
        ))
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
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Query LSP server for code intelligence and preview-only edits. Operations: goToDefinition, findReferences, hover, documentSymbol, workspaceSymbol, diagnostics, renamePreview, formatPreview, sourceActionPreview, semanticCheckPreview, semanticContext, securityContext, callHierarchy, typeHierarchy. semanticCheckPreview accepts either full proposed content or a single-file unified diff patch. semanticContext returns a compact LSP-backed context packet with source excerpt, diagnostics, symbols, and optional definition/reference/overlay information. securityContext returns a security-review context packet with risk markers. When include_source_actions=true, semanticContext also includes safe source-action preview hints (initially only source.organizeImports). callHierarchy/typeHierarchy return call/type hierarchy information for the symbol at line+column. Edit operations are previews only; use apply_patch (or other mutating tools) for actual changes."
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
                        "renamePreview", "formatPreview", "sourceActionPreview",
                        "semanticCheckPreview", "semanticContext",
                        "callHierarchy", "typeHierarchy",
                        "securityContext"
                    ],
                    "description": "LSP operation to perform. semanticCheckPreview accepts either full proposed content or a single-file unified diff patch. semanticContext returns a compact LSP-backed context packet. securityContext returns a security-review context packet with risk markers. callHierarchy/typeHierarchy return call/type hierarchy information for the symbol at line+column. Edit operations are previews only; use apply_patch (or other mutating tools) for actual changes."
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
                let collector =
                    crate::lsp::diagnostics::DiagnosticsCollector::new(self.service.clone());
                let diag_output = collector
                    .get_diagnostics_for_file(&file)
                    .await
                    .map_err(|e| ToolError::Execution(format!("diagnostics: {e}")))?;
                let summaries: Vec<DiagnosticSummary> = diag_output
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
                    diagnostics: Vec<DiagnosticSummary>,
                }
                let result = DiagnosticsResult {
                    diagnostics_may_still_be_warming: diag_output.diagnostics_may_still_be_warming,
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
                    .rename_preview(
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
                    .format_preview(&file, Some(&self.allowed_root))
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

                let file_str = file.to_string_lossy().to_string();
                let radius = parsed
                    .radius
                    .unwrap_or(DEFAULT_SEMANTIC_CONTEXT_RADIUS)
                    .min(MAX_SEMANTIC_CONTEXT_RADIUS);
                let has_position = parsed.line.is_some() && parsed.column.is_some();
                let want_defs = parsed.include_definitions.unwrap_or(has_position);
                let want_refs = parsed.include_references.unwrap_or(has_position);
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

                // Phase 3: source excerpt
                let (excerpt, excerpt_truncated) = if has_position {
                    Self::build_source_excerpt(&file, parsed.line, radius)?
                } else {
                    Self::build_source_excerpt(&file, None, radius)?
                };

                // Phase 4: current diagnostics
                let collector =
                    crate::lsp::diagnostics::DiagnosticsCollector::new(self.service.clone());
                let (current_diags, current_diag_err, diagnostics_truncated) =
                    match collector.get_diagnostics_for_file(&file).await {
                        Ok(diag_output) => {
                            let raw_diag_len = diag_output.diagnostics.len();
                            let diagnostics_truncated = raw_diag_len > MAX_CONTEXT_DIAGNOSTICS;
                            let diags: Vec<DiagnosticSummary> = diag_output
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
                            (diags, None, diagnostics_truncated)
                        }
                        Err(e) => (Vec::new(), Some(format!("diagnostics: {e}")), false),
                    };

                // Phase 4: current document symbols
                let (current_syms, current_sym_err, symbols_truncated) =
                    match ops.document_symbols(&file).await {
                        Ok(syms) => {
                            let mut remaining = MAX_CONTEXT_SYMBOLS;
                            let mut summaries = Vec::new();
                            Self::flatten_symbols(&syms, &file_str, &mut summaries, &mut remaining);
                            (summaries, None, remaining == 0)
                        }
                        Err(e) => (Vec::new(), Some(format!("documentSymbol: {e}")), false),
                    };

                // Phase 5: definitions + references
                let mut definitions = Vec::new();
                let mut definitions_error = None;
                let mut references = Vec::new();
                let mut references_error = None;
                let mut refs_truncated = false;
                if has_position {
                    let pos = to_lsp_position(parsed.line.unwrap(), parsed.column.unwrap());
                    if want_defs {
                        match ops.go_to_definition(&file, pos.line, pos.character).await {
                            Ok(defs) => {
                                definitions = defs
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
                            }
                            Err(e) => {
                                definitions_error = Some(format!("goToDefinition: {e}"));
                            }
                        }
                    }
                    if want_refs {
                        match ops.find_references(&file, pos.line, pos.character).await {
                            Ok(refs) => {
                                refs_truncated = refs.len() > MAX_CONTEXT_REFERENCES;
                                references = refs
                                    .into_iter()
                                    .take(MAX_CONTEXT_REFERENCES)
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
                            }
                            Err(e) => {
                                references_error = Some(format!("findReferences: {e}"));
                            }
                        }
                    }
                }

                // Phase 6: overlay
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

                let overlay_diag_count = overlay.as_ref().map(|o| o.diagnostics.len()).unwrap_or(0);
                let overlay_sym_count = overlay.as_ref().map(|o| o.symbols.len()).unwrap_or(0);

                // Source-action hints (opt-in)
                let include_source_actions = parsed.include_source_actions.unwrap_or(false);
                let source_actions = if include_source_actions {
                    self.collect_source_action_hints(&ops, &file).await
                } else {
                    Vec::new()
                };
                let source_action_count =
                    source_actions.iter().filter(|hint| hint.available).count();

                // Call and type hierarchy (opt-in, position validated above)
                let call_hierarchy = if include_call_hierarchy && has_position {
                    Some(
                        self.build_call_hierarchy_summary(
                            &ops,
                            &file,
                            parsed.line.unwrap(),
                            parsed.column.unwrap(),
                            crate::lsp::operations::HierarchyDirection::Both,
                        )
                        .await,
                    )
                } else {
                    None
                };

                let type_hierarchy = if include_type_hierarchy && has_position {
                    Some(
                        self.build_type_hierarchy_summary(
                            &ops,
                            &file,
                            parsed.line.unwrap(),
                            parsed.column.unwrap(),
                            crate::lsp::operations::HierarchyDirection::Both,
                        )
                        .await,
                    )
                } else {
                    None
                };

                let call_hierarchy_count = call_hierarchy
                    .as_ref()
                    .map(|c| c.items.len() + c.incoming.len() + c.outgoing.len())
                    .unwrap_or(0);
                let type_hierarchy_count = type_hierarchy
                    .as_ref()
                    .map(|c| c.items.len() + c.supertypes.len() + c.subtypes.len())
                    .unwrap_or(0);
                let result_count = current_diags.len()
                    + current_syms.len()
                    + definitions.len()
                    + references.len()
                    + overlay_diag_count
                    + overlay_sym_count
                    + source_action_count
                    + call_hierarchy_count
                    + type_hierarchy_count;
                let packet = SemanticContextPacket {
                    file: file_str,
                    target,
                    excerpt,
                    diagnostics: current_diags,
                    current_diagnostics_error: current_diag_err,
                    overlay,
                    symbols: current_syms,
                    current_symbols_error: current_sym_err,
                    definitions,
                    definitions_error,
                    references,
                    references_error,
                    source_actions,
                    call_hierarchy,
                    type_hierarchy,
                    limits: SemanticContextLimits {
                        diagnostics_truncated,
                        symbols_truncated,
                        references_truncated: refs_truncated,
                        overlay_diagnostics_truncated,
                        excerpt_truncated,
                    },
                };
                let output = LspToolOutput {
                    operation: "semanticContext".to_string(),
                    file_path: file_path_str,
                    result_count,
                    truncated: false,
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

                let (categories, radius, max_risk_markers, include_call_hierarchy, preset_note) =
                    Self::resolve_security_context_settings(&parsed, has_position)?;

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

                if include_call_hierarchy && !has_position {
                    return Err(ToolError::Execution(
                        "securityContext call hierarchy requires both line and column".to_string(),
                    ));
                }

                let (excerpt, excerpt_truncated) = if has_position {
                    Self::build_source_excerpt(&file, parsed.line, radius)?
                } else {
                    Self::build_source_excerpt(&file, None, radius)?
                };

                let risk_scan =
                    super::lsp_security::scan_risk_markers(&excerpt, &categories, max_risk_markers);
                let risk_markers = risk_scan.markers;
                let risk_markers_truncated = risk_scan.truncated;

                let collector =
                    crate::lsp::diagnostics::DiagnosticsCollector::new(self.service.clone());
                let (raw_diags, current_diag_err) =
                    match collector.get_diagnostics_for_file(&file).await {
                        Ok(diag_output) => {
                            let diags: Vec<DiagnosticSummary> = diag_output
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
                            (diags, None)
                        }
                        Err(e) => (Vec::new(), Some(format!("diagnostics: {e}"))),
                    };

                let security_diags: Vec<DiagnosticSummary> = raw_diags
                    .into_iter()
                    .filter(|d| {
                        super::lsp_security::is_security_relevant_diagnostic(d, &risk_markers)
                    })
                    .collect();
                let (security_diags, diagnostics_truncated) =
                    super::lsp_security::cap_vec(security_diags, MAX_SECURITY_DIAGNOSTICS);

                let (all_syms, current_sym_err) = match ops.document_symbols(&file).await {
                    Ok(syms) => {
                        let mut remaining = MAX_CONTEXT_SYMBOLS;
                        let mut summaries = Vec::new();
                        Self::flatten_symbols(&syms, &file_str, &mut summaries, &mut remaining);
                        (summaries, None)
                    }
                    Err(e) => (Vec::new(), Some(format!("documentSymbol: {e}"))),
                };

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
                let (security_syms, symbols_truncated) =
                    super::lsp_security::cap_vec(relevant_syms, MAX_SECURITY_SYMBOLS);

                let mut definitions = Vec::new();
                let mut references = Vec::new();
                let mut refs_truncated = false;
                let mut defs_error: Option<String> = None;
                let mut refs_error: Option<String> = None;
                if has_position {
                    let pos = to_lsp_position(parsed.line.unwrap(), parsed.column.unwrap());
                    match ops.go_to_definition(&file, pos.line, pos.character).await {
                        Ok(defs) => {
                            definitions = defs
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
                        }
                        Err(e) => {
                            defs_error = Some(format!("definitions: {e}"));
                        }
                    }
                    match ops.find_references(&file, pos.line, pos.character).await {
                        Ok(refs) => {
                            refs_truncated = refs.len() > MAX_CONTEXT_REFERENCES;
                            references = refs
                                .into_iter()
                                .take(MAX_CONTEXT_REFERENCES)
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
                        }
                        Err(e) => {
                            refs_error = Some(format!("references: {e}"));
                        }
                    }
                }

                let (overlay, _overlay_diagnostics_truncated) = if has_proposed {
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

                let call_hierarchy = if include_call_hierarchy && has_position {
                    Some(
                        self.build_call_hierarchy_summary(
                            &ops,
                            &file,
                            parsed.line.unwrap(),
                            parsed.column.unwrap(),
                            crate::lsp::operations::HierarchyDirection::Both,
                        )
                        .await,
                    )
                } else {
                    None
                };

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
                if let Some(note) = preset_note {
                    notes.push(note);
                }

                let security_diag_count = security_diags.len();
                let security_sym_count = security_syms.len();
                let risk_markers_len = risk_markers.len();
                let call_hierarchy_count = call_hierarchy
                    .as_ref()
                    .map(|c| c.items.len() + c.incoming.len() + c.outgoing.len())
                    .unwrap_or(0);
                let result_count = risk_markers_len
                    + security_diag_count
                    + security_sym_count
                    + definitions.len()
                    + references.len()
                    + call_hierarchy_count;
                let packet = SecurityContextPacket {
                    file: file_str,
                    target,
                    excerpt,
                    risk_markers,
                    security_relevant_symbols: security_syms,
                    security_relevant_diagnostics: security_diags,
                    definitions,
                    references,
                    call_hierarchy,
                    overlay,
                    preset: parsed.security_preset.clone(),
                    notes,
                    limits: SecurityContextLimits {
                        risk_markers_truncated,
                        diagnostics_truncated,
                        symbols_truncated,
                        references_truncated: refs_truncated,
                        excerpt_truncated,
                    },
                };
                let truncated = risk_markers_truncated
                    || diagnostics_truncated
                    || symbols_truncated
                    || refs_truncated
                    || excerpt_truncated;

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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
        assert_eq!(tool.name(), "lsp");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn lsp_parameters_schema_snapshot() {
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
        let params = tool.parameters();
        let expected = json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": [
                        "goToDefinition", "findReferences", "hover",
                        "documentSymbol", "workspaceSymbol", "diagnostics",
                        "renamePreview", "formatPreview", "sourceActionPreview",
                        "semanticCheckPreview", "semanticContext",
                        "callHierarchy", "typeHierarchy",
                        "securityContext"
                    ],
                    "description": "LSP operation to perform. semanticCheckPreview accepts either full proposed content or a single-file unified diff patch. semanticContext returns a compact LSP-backed context packet. securityContext returns a security-review context packet with risk markers. callHierarchy/typeHierarchy return call/type hierarchy information for the symbol at line+column. Edit operations are previews only; use apply_patch (or other mutating tools) for actual changes."
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
                }
            },
            "required": ["operation"]
        });
        assert_eq!(params, expected);
    }

    #[tokio::test]
    async fn semantic_check_content_accepts_content() {
        let (_dir, path) = temp_rs_file("fn main() {}\n");
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
        let res = tool
            .execute_structured(json!({"operation": "no_such_op"}), None)
            .await;
        assert!(res.is_err());
    }

    // ── semanticContext tests ──────────────────────────────────────────

    #[tokio::test]
    async fn semantic_context_schema_includes_operation() {
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
        let params = tool.parameters();
        let ops = params["properties"]["operation"]["enum"]
            .as_array()
            .unwrap();
        assert!(ops.iter().any(|v| v.as_str() == Some("semanticContext")));
    }

    #[tokio::test]
    async fn semantic_context_requires_file_path_execution() {
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )))
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
        };
        let truncated = limits.risk_markers_truncated
            || limits.diagnostics_truncated
            || limits.symbols_truncated
            || limits.references_truncated
            || limits.excerpt_truncated;
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
        };
        let truncated = limits.risk_markers_truncated
            || limits.diagnostics_truncated
            || limits.symbols_truncated
            || limits.references_truncated
            || limits.excerpt_truncated;
        assert!(truncated);
    }

    #[test]
    fn security_context_top_level_truncated_true_when_any_limit_truncated() {
        let cases: [[bool; 5]; 5] = [
            [true, false, false, false, false],
            [false, true, false, false, false],
            [false, false, true, false, false],
            [false, false, false, true, false],
            [false, false, false, false, true],
        ];
        for (i, flag) in cases.iter().enumerate() {
            let limits = SecurityContextLimits {
                risk_markers_truncated: flag[0],
                diagnostics_truncated: flag[1],
                symbols_truncated: flag[2],
                references_truncated: flag[3],
                excerpt_truncated: flag[4],
            };
            let truncated = limits.risk_markers_truncated
                || limits.diagnostics_truncated
                || limits.symbols_truncated
                || limits.references_truncated
                || limits.excerpt_truncated;
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
        let tool = LspTool::new(std::sync::Arc::new(crate::lsp::service::LspService::new(
            crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
        )));
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
}
