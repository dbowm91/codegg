use crate::lsp::diagnostics::DiagnosticsCollector;
use crate::lsp::lsp_types;
use crate::lsp::operations::LspOperations;
use crate::lsp::service::LspService;
use egglsp::capability::{LspCapabilitySnapshot, LspSemanticOperation, LspUnavailable};
use egglsp::semantic_context::{
    SemanticContextRequest, SemanticContextResponse, SemanticDiagnosticEvidence,
    SemanticHierarchyItem, SemanticHierarchyRange, SemanticHierarchyRelation, SemanticLocation,
    SemanticOverlay, SemanticOverlaySymbol, SemanticSourceExcerpt, SemanticSymbolSummary,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_CONTEXT_EXCERPT_BYTES: usize = 32_000;
const MAX_HIERARCHY_ITEMS: usize = 32;
const MAX_HIERARCHY_EDGES: usize = 128;
#[allow(dead_code)]
const MAX_HIERARCHY_RANGES: usize = 32;

pub struct SemanticContextCollector {
    service: Arc<LspService>,
    operations: Arc<LspOperations>,
    diagnostics: Arc<DiagnosticsCollector>,
    allowed_root: PathBuf,
}

impl SemanticContextCollector {
    pub fn new(
        service: Arc<LspService>,
        operations: Arc<LspOperations>,
        diagnostics: Arc<DiagnosticsCollector>,
        allowed_root: PathBuf,
    ) -> Self {
        Self {
            service,
            operations,
            diagnostics,
            allowed_root,
        }
    }

    pub async fn collect(
        &self,
        request: SemanticContextRequest,
    ) -> Result<SemanticContextResponse, String> {
        let file = resolve_file(&request.file_path, &self.allowed_root)?;
        let file_str = file.to_string_lossy().to_string();
        let has_position = request.line.is_some() && request.column.is_some();

        let mut response = SemanticContextResponse::new(&file_str);
        let mut limits = egglsp::semantic_context::SemanticContextLimits::default();

        // Phase 1: source excerpt
        let (excerpt, excerpt_truncated) = if has_position {
            build_source_excerpt(&file, request.line, request.excerpt_radius)?
        } else {
            build_source_excerpt(&file, None, request.excerpt_radius)?
        };
        limits.excerpt_truncated = excerpt_truncated;
        response.source_excerpt = Some(excerpt);

        // Phase 2: diagnostics
        match self
            .diagnostics
            .get_diagnostic_snapshot_for_file(&file)
            .await
        {
            Ok(snapshot) => {
                let raw_len = snapshot.diagnostics.len();
                let truncated = raw_len > request.max_diagnostics;
                limits.diagnostics_truncated = truncated;
                let age_ms = snapshot.age_ms;
                let usable = snapshot.is_usable_evidence();
                response.diagnostics = snapshot
                    .diagnostics
                    .into_iter()
                    .take(request.max_diagnostics)
                    .collect();
                if truncated {
                    response.push_truncation(
                        "diagnostics",
                        Some(raw_len),
                        response.diagnostics.len(),
                        request.max_diagnostics,
                    );
                }
                response.diagnostic_evidence = Some(SemanticDiagnosticEvidence {
                    freshness: snapshot.freshness,
                    source: snapshot.source,
                    age_ms,
                    usable_evidence: usable,
                });
            }
            Err(e) => {
                response.push_note(format!("diagnostics: {e}"));
            }
        }

        // Phase 3: document symbols
        match self.operations.document_symbols(&file).await {
            Ok(syms) => {
                let mut remaining = request.max_symbols;
                let mut summaries = Vec::new();
                flatten_symbols(&syms, &file_str, &mut summaries, &mut remaining);
                limits.symbols_truncated = remaining == 0;
                if remaining == 0 && !syms.is_empty() {
                    response.push_truncation(
                        "symbols",
                        Some(count_symbols_recursive(&syms)),
                        summaries.len(),
                        request.max_symbols,
                    );
                }
                response.all_symbols = summaries
                    .iter()
                    .map(|s| SemanticSymbolSummary {
                        name: s.name.clone(),
                        kind: s.kind.clone(),
                        file: s.file.clone(),
                        start_line: s.start_line,
                        start_column: s.start_column,
                        end_line: s.end_line,
                        end_column: s.end_column,
                    })
                    .collect();
                response.symbol = response.all_symbols.first().cloned();
            }
            Err(e) => {
                response.push_note(format!("documentSymbol: {e}"));
            }
        }

        // Phase 4: capability snapshot
        let caps_snapshot = self.capability_snapshot_for_file(&file).await;

        // Phase 5: definitions + references (capability-gated)
        if has_position {
            let line = request.line.unwrap();
            let column = request.column.unwrap();
            let pos = to_lsp_position(line, column);

            // Definitions
            if request.include_definitions {
                let defs_supported = caps_snapshot
                    .as_ref()
                    .map(|c| c.supports(LspSemanticOperation::Definition))
                    .unwrap_or(true);
                if defs_supported {
                    match self
                        .operations
                        .go_to_definition(&file, pos.line, pos.character)
                        .await
                    {
                        Ok(defs) => {
                            response.definitions = defs
                                .iter()
                                .map(|loc| {
                                    let range = loc.target_range;
                                    SemanticLocation {
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
                            response.push_note(format!("goToDefinition: {e}"));
                        }
                    }
                } else if let Some(u) = caps_snapshot
                    .as_ref()
                    .and_then(|c| c.unavailable(LspSemanticOperation::Definition))
                {
                    response.push_unavailable(u);
                }
            }

            // References
            if request.include_references {
                let refs_supported = caps_snapshot
                    .as_ref()
                    .map(|c| c.supports(LspSemanticOperation::References))
                    .unwrap_or(true);
                if refs_supported {
                    match self
                        .operations
                        .find_references(&file, pos.line, pos.character)
                        .await
                    {
                        Ok(refs) => {
                            let raw_len = refs.len();
                            let truncated = raw_len > request.max_references;
                            limits.references_truncated = truncated;
                            response.references = refs
                                .into_iter()
                                .take(request.max_references)
                                .map(|loc| {
                                    let range = loc.range;
                                    SemanticLocation {
                                        file: uri_to_path(&loc.uri),
                                        start_line: range.start.line + 1,
                                        start_column: range.start.character + 1,
                                        end_line: range.end.line + 1,
                                        end_column: range.end.character + 1,
                                    }
                                })
                                .collect();
                            if truncated {
                                response.push_truncation(
                                    "references",
                                    Some(raw_len),
                                    response.references.len(),
                                    request.max_references,
                                );
                            }
                        }
                        Err(e) => {
                            response.push_note(format!("findReferences: {e}"));
                        }
                    }
                } else if let Some(u) = caps_snapshot
                    .as_ref()
                    .and_then(|c| c.unavailable(LspSemanticOperation::References))
                {
                    response.push_unavailable(u);
                }
            }
        }

        // Phase 6: overlay
        if request.include_overlay || request.overlay_content.is_some() {
            match resolve_overlay_content(&request, &file, &self.allowed_root).await {
                Ok(content) => {
                    match self
                        .operations
                        .semantic_check_preview(&file, content, Some(&self.allowed_root))
                        .await
                    {
                        Ok(preview) => {
                            let raw_diag_len = preview.diagnostics.len();
                            let diag_truncated = raw_diag_len > request.max_diagnostics;
                            limits.overlay_diagnostics_truncated = diag_truncated;
                            let diags: Vec<_> = preview
                                .diagnostics
                                .into_iter()
                                .take(request.max_diagnostics)
                                .collect();
                            if diag_truncated {
                                response.push_truncation(
                                    "overlay_diagnostics",
                                    Some(raw_diag_len),
                                    diags.len(),
                                    request.max_diagnostics,
                                );
                            }
                            response.overlay = Some(SemanticOverlay {
                                used: true,
                                diagnostics_may_still_be_warming: preview
                                    .diagnostics_may_still_be_warming,
                                diagnostics: diags,
                                diagnostics_error: preview.diagnostics_error,
                                symbols: preview
                                    .symbols
                                    .into_iter()
                                    .map(|s| SemanticOverlaySymbol {
                                        name: s.name,
                                        kind: s.kind,
                                        start_line: s.start_line,
                                        start_column: s.start_column,
                                        end_line: s.end_line,
                                        end_column: s.end_column,
                                    })
                                    .collect(),
                                symbols_error: preview.symbols_error,
                                restored_disk_view: preview.restored_disk_view,
                                restore_error: preview.restore_error,
                            });
                        }
                        Err(e) => {
                            response.overlay = Some(SemanticOverlay {
                                used: true,
                                diagnostics_may_still_be_warming: false,
                                diagnostics: Vec::new(),
                                diagnostics_error: Some(format!("overlay: {e}")),
                                symbols: Vec::new(),
                                symbols_error: None,
                                restored_disk_view: false,
                                restore_error: None,
                            });
                        }
                    }
                }
                Err(e) => {
                    response.overlay = Some(SemanticOverlay {
                        used: true,
                        diagnostics_may_still_be_warming: false,
                        diagnostics: Vec::new(),
                        diagnostics_error: Some(format!("overlay content: {e}")),
                        symbols: Vec::new(),
                        symbols_error: None,
                        restored_disk_view: false,
                        restore_error: None,
                    });
                }
            }
        }

        // Phase 7: call/type hierarchy (opt-in, capability-gated)
        if request.include_call_hierarchy {
            if has_position {
                let line = request.line.unwrap();
                let column = request.column.unwrap();
                let ch_supported = caps_snapshot
                    .as_ref()
                    .map(|c| c.supports(LspSemanticOperation::CallHierarchy))
                    .unwrap_or(true);
                if ch_supported {
                    let summary = build_call_hierarchy_summary(
                        &self.operations,
                        &file,
                        line,
                        column,
                        egglsp::operations::HierarchyDirection::Both,
                    )
                    .await;
                    response.call_hierarchy = Some(summary);
                } else if let Some(u) = caps_snapshot
                    .as_ref()
                    .and_then(|c| c.unavailable(LspSemanticOperation::CallHierarchy))
                {
                    response.push_unavailable(u);
                }
            } else {
                response.push_note("call hierarchy requested but no position was provided");
                response.push_unavailable(LspUnavailable::new(
                    LspSemanticOperation::CallHierarchy,
                    "call hierarchy requires a position",
                ));
            }
        }

        if request.include_type_hierarchy {
            if has_position {
                let line = request.line.unwrap();
                let column = request.column.unwrap();
                let th_supported = caps_snapshot
                    .as_ref()
                    .map(|c| c.supports(LspSemanticOperation::TypeHierarchy))
                    .unwrap_or(true);
                if th_supported {
                    let summary = build_type_hierarchy_summary(
                        &self.operations,
                        &file,
                        line,
                        column,
                        egglsp::operations::HierarchyDirection::Both,
                    )
                    .await;
                    response.type_hierarchy = Some(summary);
                } else if let Some(u) = caps_snapshot
                    .as_ref()
                    .and_then(|c| c.unavailable(LspSemanticOperation::TypeHierarchy))
                {
                    response.push_unavailable(u);
                }
            } else {
                response.push_note("type hierarchy requested but no position was provided");
                response.push_unavailable(LspUnavailable::new(
                    LspSemanticOperation::TypeHierarchy,
                    "type hierarchy requires a position",
                ));
            }
        }

        response.limits = limits;
        response.truncated = response.limits.diagnostics_truncated
            || response.limits.symbols_truncated
            || response.limits.references_truncated
            || response.limits.overlay_diagnostics_truncated
            || response.limits.excerpt_truncated
            || !response.section_truncations.is_empty();

        Ok(response)
    }

    async fn capability_snapshot_for_file(&self, file: &Path) -> Option<LspCapabilitySnapshot> {
        let (key, _) = self.service.get_or_create_client(file).await.ok()?;
        let caps = self.service.get_capabilities_for_key(&key).await?;
        let lang = crate::lsp::language::detect_language(file.to_str().unwrap_or(""));
        let server_name = key.split(':').next_back().map(String::from);
        Some(LspCapabilitySnapshot::from_capabilities(
            &caps,
            server_name.as_deref(),
            lang,
        ))
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn resolve_file(file_path: &str, allowed_root: &Path) -> Result<PathBuf, String> {
    let p = PathBuf::from(file_path);
    let original = if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| allowed_root.to_path_buf())
            .join(p)
    };
    canonicalize_within_root(&original, allowed_root)
}

fn canonicalize_within_root(path: &Path, root: &Path) -> Result<PathBuf, String> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root_canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if canonical.starts_with(&root_canonical) || canonical.starts_with(root) {
        Ok(canonical)
    } else {
        Err(format!(
            "path {} is outside allowed root {}",
            path.display(),
            root.display()
        ))
    }
}

pub(crate) fn build_source_excerpt(
    file: &Path,
    target_line: Option<u32>,
    radius: u32,
) -> Result<(SemanticSourceExcerpt, bool), String> {
    let content = std::fs::read_to_string(file)
        .map_err(|e| format!("semanticContext: failed to read {}: {}", file.display(), e))?;
    let total_lines = content.lines().count().max(1) as u32;
    let (start_line, end_line) = if let Some(target) = target_line {
        let start = target.saturating_sub(radius).max(1);
        let end = target.saturating_add(radius).min(total_lines);
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
    let (display, truncated) = truncate_to_byte_limit(&text, MAX_CONTEXT_EXCERPT_BYTES);
    Ok((
        SemanticSourceExcerpt {
            start_line,
            end_line,
            text: display.to_string(),
            truncated,
        },
        truncated,
    ))
}

fn flatten_symbols(
    symbols: &[lsp_types::DocumentSymbol],
    file: &str,
    output: &mut Vec<SemanticSymbolSummary>,
    remaining: &mut usize,
) {
    for sym in symbols {
        if *remaining == 0 {
            return;
        }
        let range = sym.range;
        output.push(SemanticSymbolSummary {
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
            flatten_symbols(children, file, output, remaining);
        }
    }
}

fn symbol_kind_to_string(kind: lsp_types::SymbolKind) -> String {
    match kind {
        lsp_types::SymbolKind::FILE => "file",
        lsp_types::SymbolKind::MODULE => "module",
        lsp_types::SymbolKind::NAMESPACE => "namespace",
        lsp_types::SymbolKind::PACKAGE => "package",
        lsp_types::SymbolKind::CLASS => "class",
        lsp_types::SymbolKind::METHOD => "method",
        lsp_types::SymbolKind::PROPERTY => "property",
        lsp_types::SymbolKind::FIELD => "field",
        lsp_types::SymbolKind::CONSTRUCTOR => "constructor",
        lsp_types::SymbolKind::ENUM => "enum",
        lsp_types::SymbolKind::INTERFACE => "interface",
        lsp_types::SymbolKind::FUNCTION => "function",
        lsp_types::SymbolKind::VARIABLE => "variable",
        lsp_types::SymbolKind::CONSTANT => "constant",
        lsp_types::SymbolKind::STRING => "string",
        lsp_types::SymbolKind::NUMBER => "number",
        lsp_types::SymbolKind::BOOLEAN => "boolean",
        lsp_types::SymbolKind::ARRAY => "array",
        lsp_types::SymbolKind::OBJECT => "object",
        lsp_types::SymbolKind::KEY => "key",
        lsp_types::SymbolKind::NULL => "null",
        lsp_types::SymbolKind::ENUM_MEMBER => "enum_member",
        lsp_types::SymbolKind::STRUCT => "struct",
        lsp_types::SymbolKind::EVENT => "event",
        lsp_types::SymbolKind::OPERATOR => "operator",
        lsp_types::SymbolKind::TYPE_PARAMETER => "type_parameter",
        _ => "unknown",
    }
    .to_string()
}

fn uri_to_path(uri: &lsp_types::Uri) -> String {
    let raw = uri.to_string();
    url::Url::parse(&raw)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(raw)
}

fn truncate_to_byte_limit(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

fn to_lsp_position(line: u32, column: u32) -> lsp_types::Position {
    lsp_types::Position {
        line: line.saturating_sub(1),
        character: column.saturating_sub(1),
    }
}

#[allow(dead_code)]
fn severity_to_string(severity: lsp_types::DiagnosticSeverity) -> String {
    match severity {
        lsp_types::DiagnosticSeverity::ERROR => "error",
        lsp_types::DiagnosticSeverity::WARNING => "warning",
        lsp_types::DiagnosticSeverity::INFORMATION => "info",
        lsp_types::DiagnosticSeverity::HINT => "hint",
        _ => "unknown",
    }
    .to_string()
}

fn convert_hierarchy_range(range: lsp_types::Range) -> SemanticHierarchyRange {
    SemanticHierarchyRange {
        start_line: range.start.line + 1,
        start_column: range.start.character + 1,
        end_line: range.end.line + 1,
        end_column: range.end.character + 1,
    }
}

#[allow(dead_code)]
fn convert_hierarchy_item_to_symbol(item: &lsp_types::CallHierarchyItem) -> SemanticSymbolSummary {
    let range = convert_hierarchy_range(item.range);
    SemanticSymbolSummary {
        name: item.name.clone(),
        kind: symbol_kind_to_string(item.kind),
        file: uri_to_path(&item.uri),
        start_line: range.start_line,
        start_column: range.start_column,
        end_line: range.end_line,
        end_column: range.end_column,
    }
}

fn convert_hierarchy_item(item: &lsp_types::CallHierarchyItem) -> SemanticHierarchyItem {
    SemanticHierarchyItem {
        name: item.name.clone(),
        kind: symbol_kind_to_string(item.kind),
        file: uri_to_path(&item.uri),
        range: convert_hierarchy_range(item.range),
        selection_range: convert_hierarchy_range(item.selection_range),
        detail: item.detail.clone(),
    }
}

fn convert_type_hierarchy_item(item: &lsp_types::TypeHierarchyItem) -> SemanticHierarchyItem {
    SemanticHierarchyItem {
        name: item.name.clone(),
        kind: symbol_kind_to_string(item.kind),
        file: uri_to_path(&item.uri),
        range: convert_hierarchy_range(item.range),
        selection_range: convert_hierarchy_range(item.selection_range),
        detail: item.detail.clone(),
    }
}

fn truncate_ranges(ranges: &[lsp_types::Range]) -> (Vec<SemanticHierarchyRange>, bool) {
    let truncated = ranges.len() > MAX_HIERARCHY_RANGES;
    let output = ranges
        .iter()
        .take(MAX_HIERARCHY_RANGES)
        .cloned()
        .map(convert_hierarchy_range)
        .collect();
    (output, truncated)
}

fn count_symbols_recursive(symbols: &[lsp_types::DocumentSymbol]) -> usize {
    let mut count = 0;
    for sym in symbols {
        count += 1;
        if let Some(children) = &sym.children {
            count += count_symbols_recursive(children);
        }
    }
    count
}

async fn resolve_overlay_content(
    request: &SemanticContextRequest,
    file: &Path,
    _allowed_root: &Path,
) -> Result<String, String> {
    if let Some(content) = &request.overlay_content {
        return Ok(content.clone());
    }

    tokio::fs::read_to_string(file)
        .await
        .map_err(|e| format!("overlay content read: {}", e))
}

async fn build_call_hierarchy_summary(
    ops: &LspOperations,
    file: &Path,
    line: u32,
    column: u32,
    direction: egglsp::operations::HierarchyDirection,
) -> egglsp::semantic_context::SemanticCallGraphSummary {
    let items_result = ops.prepare_call_hierarchy(file, line, column).await;
    let items = match items_result {
        Ok(items) => items,
        Err(e) => {
            return egglsp::semantic_context::SemanticCallGraphSummary {
                incoming_count: 0,
                outgoing_count: 0,
                items: Vec::new(),
                incoming: Vec::new(),
                outgoing: Vec::new(),
                truncated: false,
                prepare_error: Some(e.to_string()),
                incoming_error: None,
                outgoing_error: None,
            };
        }
    };

    let items_truncated = items.len() > MAX_HIERARCHY_ITEMS;

    if items.is_empty() {
        return egglsp::semantic_context::SemanticCallGraphSummary {
            incoming_count: 0,
            outgoing_count: 0,
            items: Vec::new(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
            truncated: false,
            prepare_error: None,
            incoming_error: None,
            outgoing_error: None,
        };
    }

    let primary = items[0].clone();
    let items = items
        .into_iter()
        .take(MAX_HIERARCHY_ITEMS)
        .map(|item| convert_hierarchy_item(&item))
        .collect::<Vec<_>>();
    let mut incoming_count = 0usize;
    let mut incoming_error = None;
    let mut incoming_raw_len = 0usize;
    let mut incoming = Vec::new();
    let mut outgoing_count = 0usize;
    let mut outgoing_error = None;
    let mut outgoing_raw_len = 0usize;
    let mut outgoing = Vec::new();
    let mut incoming_ranges_truncated = false;
    let mut outgoing_ranges_truncated = false;

    if matches!(
        direction,
        egglsp::operations::HierarchyDirection::Incoming
            | egglsp::operations::HierarchyDirection::Both
    ) {
        match ops.incoming_calls(primary.clone()).await {
            Ok(calls) => {
                incoming_raw_len = calls.len();
                incoming_count = calls.len().min(MAX_HIERARCHY_EDGES);
                incoming = calls
                    .into_iter()
                    .take(MAX_HIERARCHY_EDGES)
                    .map(|call| {
                        let (ranges, truncated) = truncate_ranges(&call.from_ranges);
                        incoming_ranges_truncated |= truncated;
                        SemanticHierarchyRelation {
                            item: convert_hierarchy_item(&call.from),
                            ranges,
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
        egglsp::operations::HierarchyDirection::Outgoing
            | egglsp::operations::HierarchyDirection::Both
    ) {
        match ops.outgoing_calls(primary.clone()).await {
            Ok(calls) => {
                outgoing_raw_len = calls.len();
                outgoing_count = calls.len().min(MAX_HIERARCHY_EDGES);
                outgoing = calls
                    .into_iter()
                    .take(MAX_HIERARCHY_EDGES)
                    .map(|call| {
                        let (ranges, truncated) = truncate_ranges(&call.from_ranges);
                        outgoing_ranges_truncated |= truncated;
                        SemanticHierarchyRelation {
                            item: convert_hierarchy_item(&call.to),
                            ranges,
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
        || incoming_ranges_truncated
        || outgoing_ranges_truncated;

    egglsp::semantic_context::SemanticCallGraphSummary {
        incoming_count,
        outgoing_count,
        items,
        incoming,
        outgoing,
        truncated,
        prepare_error: None,
        incoming_error,
        outgoing_error,
    }
}

async fn build_type_hierarchy_summary(
    ops: &LspOperations,
    file: &Path,
    line: u32,
    column: u32,
    direction: egglsp::operations::HierarchyDirection,
) -> egglsp::semantic_context::SemanticTypeGraphSummary {
    let items_result = ops.prepare_type_hierarchy(file, line, column).await;
    let items = match items_result {
        Ok(items) => items,
        Err(e) => {
            return egglsp::semantic_context::SemanticTypeGraphSummary {
                supertypes_count: 0,
                subtypes_count: 0,
                items: Vec::new(),
                supertypes: Vec::new(),
                subtypes: Vec::new(),
                truncated: false,
                prepare_error: Some(e.to_string()),
                supertypes_error: None,
                subtypes_error: None,
            };
        }
    };

    let items_truncated = items.len() > MAX_HIERARCHY_ITEMS;

    if items.is_empty() {
        return egglsp::semantic_context::SemanticTypeGraphSummary {
            supertypes_count: 0,
            subtypes_count: 0,
            items: Vec::new(),
            supertypes: Vec::new(),
            subtypes: Vec::new(),
            truncated: false,
            prepare_error: None,
            supertypes_error: None,
            subtypes_error: None,
        };
    }

    let primary = items[0].clone();
    let items = items
        .into_iter()
        .take(MAX_HIERARCHY_ITEMS)
        .map(|item| convert_type_hierarchy_item(&item))
        .collect::<Vec<_>>();
    let mut supertypes_count = 0usize;
    let mut supertypes_error = None;
    let mut supertypes_raw_len = 0usize;
    let mut supertypes = Vec::new();
    let mut subtypes_count = 0usize;
    let mut subtypes_error = None;
    let mut subtypes_raw_len = 0usize;
    let mut subtypes = Vec::new();

    if matches!(
        direction,
        egglsp::operations::HierarchyDirection::Incoming
            | egglsp::operations::HierarchyDirection::Both
    ) {
        match ops.supertypes(primary.clone()).await {
            Ok(items) => {
                supertypes_raw_len = items.len();
                supertypes_count = items.len();
                supertypes = items
                    .into_iter()
                    .take(MAX_HIERARCHY_ITEMS)
                    .map(|item| convert_type_hierarchy_item(&item))
                    .collect();
            }
            Err(e) => {
                supertypes_error = Some(e.to_string());
            }
        }
    }

    if matches!(
        direction,
        egglsp::operations::HierarchyDirection::Outgoing
            | egglsp::operations::HierarchyDirection::Both
    ) {
        match ops.subtypes(primary.clone()).await {
            Ok(items) => {
                subtypes_raw_len = items.len();
                subtypes_count = items.len();
                subtypes = items
                    .into_iter()
                    .take(MAX_HIERARCHY_ITEMS)
                    .map(|item| convert_type_hierarchy_item(&item))
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

    egglsp::semantic_context::SemanticTypeGraphSummary {
        supertypes_count,
        subtypes_count,
        items,
        supertypes,
        subtypes,
        truncated,
        prepare_error: None,
        supertypes_error,
        subtypes_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_build_source_excerpt_basic() {
        let dir = std::env::temp_dir().join("semantic_context_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("excerpt_test.rs");

        let mut f = std::fs::File::create(&file).unwrap();
        for i in 1..=20 {
            writeln!(f, "line {i}").unwrap();
        }
        drop(f);

        let (excerpt, truncated) = build_source_excerpt(&file, Some(10), 2).unwrap();
        assert!(!truncated);
        assert_eq!(excerpt.start_line, 8);
        assert_eq!(excerpt.end_line, 12);
        assert!(excerpt.text.contains("line 8"));
        assert!(excerpt.text.contains("line 12"));
        assert!(!excerpt.text.contains("line 7"));
        assert!(!excerpt.text.contains("line 13"));

        let (excerpt_all, truncated_all) = build_source_excerpt(&file, None, 5).unwrap();
        assert!(!truncated_all);
        assert_eq!(excerpt_all.start_line, 1);
        assert_eq!(excerpt_all.end_line, 5);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_flatten_symbols_preserves_tree_order() {
        let sym_a = lsp_types::DocumentSymbol {
            name: "mod_a".to_string(),
            detail: None,
            kind: lsp_types::SymbolKind::MODULE,
            range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 0,
                    character: 0,
                },
                end: lsp_types::Position {
                    line: 10,
                    character: 0,
                },
            },
            selection_range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 0,
                    character: 4,
                },
                end: lsp_types::Position {
                    line: 0,
                    character: 9,
                },
            },
            children: Some(vec![lsp_types::DocumentSymbol {
                name: "fn_inner".to_string(),
                detail: None,
                kind: lsp_types::SymbolKind::FUNCTION,
                range: lsp_types::Range {
                    start: lsp_types::Position {
                        line: 2,
                        character: 0,
                    },
                    end: lsp_types::Position {
                        line: 5,
                        character: 0,
                    },
                },
                selection_range: lsp_types::Range {
                    start: lsp_types::Position {
                        line: 2,
                        character: 4,
                    },
                    end: lsp_types::Position {
                        line: 2,
                        character: 12,
                    },
                },
                children: None,
                tags: None,
                deprecated: None,
            }]),
            tags: None,
            deprecated: None,
        };
        let sym_b = lsp_types::DocumentSymbol {
            name: "struct_b".to_string(),
            detail: None,
            kind: lsp_types::SymbolKind::STRUCT,
            range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 12,
                    character: 0,
                },
                end: lsp_types::Position {
                    line: 15,
                    character: 0,
                },
            },
            selection_range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 12,
                    character: 7,
                },
                end: lsp_types::Position {
                    line: 12,
                    character: 15,
                },
            },
            children: None,
            tags: None,
            deprecated: None,
        };

        let mut output = Vec::new();
        let mut remaining = 10;
        flatten_symbols(&[sym_a, sym_b], "test.rs", &mut output, &mut remaining);

        assert_eq!(output.len(), 3);
        assert_eq!(output[0].name, "mod_a");
        assert_eq!(output[0].kind, "module");
        assert_eq!(output[1].name, "fn_inner");
        assert_eq!(output[1].kind, "function");
        assert_eq!(output[2].name, "struct_b");
        assert_eq!(output[2].kind, "struct");
    }

    #[test]
    fn test_uri_to_path_conversion() {
        use std::str::FromStr;
        let uri = lsp_types::Uri::from_str("file:///home/user/src/main.rs").unwrap();
        let path = uri_to_path(&uri);
        assert!(path.contains("main.rs"));
        assert!(!path.starts_with("file:"));

        let uri2 = lsp_types::Uri::from_str("file:///tmp/test.rs").unwrap();
        let path2 = uri_to_path(&uri2);
        assert!(path2.ends_with("test.rs"));
    }

    #[test]
    fn test_symbol_kind_to_string_coverage() {
        assert_eq!(
            symbol_kind_to_string(lsp_types::SymbolKind::FUNCTION),
            "function"
        );
        assert_eq!(symbol_kind_to_string(lsp_types::SymbolKind::CLASS), "class");
        assert_eq!(
            symbol_kind_to_string(lsp_types::SymbolKind::STRUCT),
            "struct"
        );
        assert_eq!(symbol_kind_to_string(lsp_types::SymbolKind::ENUM), "enum");
        assert_eq!(
            symbol_kind_to_string(lsp_types::SymbolKind::INTERFACE),
            "interface"
        );
        assert_eq!(
            symbol_kind_to_string(lsp_types::SymbolKind::VARIABLE),
            "variable"
        );
        assert_eq!(
            symbol_kind_to_string(lsp_types::SymbolKind::CONSTANT),
            "constant"
        );
        assert_eq!(
            symbol_kind_to_string(lsp_types::SymbolKind::METHOD),
            "method"
        );
        assert_eq!(symbol_kind_to_string(lsp_types::SymbolKind::FIELD), "field");
        assert_eq!(
            symbol_kind_to_string(lsp_types::SymbolKind::MODULE),
            "module"
        );
    }

    #[test]
    fn test_collector_new() {
        let service = Arc::new(LspService::new(egglsp::config::LspConfig::default()));
        let operations = Arc::new(LspOperations::new(service.clone()));
        let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));
        let _collector =
            SemanticContextCollector::new(service, operations, diagnostics, PathBuf::from("/tmp"));
    }

    #[test]
    fn test_truncate_to_byte_limit() {
        let short = "hello";
        let (result, truncated) = truncate_to_byte_limit(short, 100);
        assert!(!truncated);
        assert_eq!(result, "hello");

        let long = "a".repeat(500);
        let (result, truncated) = truncate_to_byte_limit(&long, 100);
        assert!(truncated);
        assert_eq!(result.len(), 100);

        let unicode = "héllo world 🌍";
        let (result, truncated) = truncate_to_byte_limit(&unicode, 12);
        assert!(truncated);
        assert!(std::str::from_utf8(result.as_bytes()).is_ok() || result.is_empty());
    }

    #[test]
    fn test_to_lsp_position_conversion() {
        let pos = to_lsp_position(1, 1);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);

        let pos2 = to_lsp_position(10, 5);
        assert_eq!(pos2.line, 9);
        assert_eq!(pos2.character, 4);

        let pos3 = to_lsp_position(0, 0);
        assert_eq!(pos3.line, 0);
        assert_eq!(pos3.character, 0);
    }

    #[test]
    fn test_severity_to_string() {
        assert_eq!(
            severity_to_string(lsp_types::DiagnosticSeverity::ERROR),
            "error"
        );
        assert_eq!(
            severity_to_string(lsp_types::DiagnosticSeverity::WARNING),
            "warning"
        );
        assert_eq!(
            severity_to_string(lsp_types::DiagnosticSeverity::INFORMATION),
            "info"
        );
        assert_eq!(
            severity_to_string(lsp_types::DiagnosticSeverity::HINT),
            "hint"
        );
    }

    #[test]
    fn test_convert_hierarchy_range() {
        let range = lsp_types::Range {
            start: lsp_types::Position {
                line: 4,
                character: 9,
            },
            end: lsp_types::Position {
                line: 7,
                character: 2,
            },
        };
        let converted = convert_hierarchy_range(range);
        assert_eq!(converted.start_line, 5);
        assert_eq!(converted.start_column, 10);
        assert_eq!(converted.end_line, 8);
        assert_eq!(converted.end_column, 3);
    }

    #[tokio::test]
    async fn test_resolve_overlay_content_prefers_request_content() {
        let request = SemanticContextRequest::new("test.rs", egglsp::SemanticContextIntent::Review)
            .with_overlay(true)
            .with_overlay_content("overlay from request");

        let content = resolve_overlay_content(
            &request,
            Path::new("/tmp/does-not-exist-for-overlay-test.rs"),
            Path::new("/tmp"),
        )
        .await
        .unwrap();

        assert_eq!(content, "overlay from request");
    }

    #[tokio::test]
    async fn test_resolve_overlay_content_reads_disk_when_request_missing() {
        let dir = std::env::temp_dir().join("semantic_context_overlay_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("overlay.rs");
        std::fs::write(&file, "disk overlay").unwrap();

        let request = SemanticContextRequest::new("test.rs", egglsp::SemanticContextIntent::Review)
            .with_overlay(true);

        let content = resolve_overlay_content(&request, &file, &dir)
            .await
            .unwrap();
        assert_eq!(content, "disk overlay");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_resolve_file_absolute() {
        let result = resolve_file("/tmp/test.rs", Path::new("/tmp")).unwrap();
        assert!(result.to_string_lossy().contains("test.rs"));
    }

    #[test]
    fn test_resolve_file_relative() {
        let cwd = std::env::current_dir().unwrap();
        let result = resolve_file("src/main.rs", &cwd).unwrap();
        assert!(result.to_string_lossy().contains("main.rs"));
    }

    #[test]
    fn test_build_source_excerpt_edge_cases() {
        let dir = std::env::temp_dir().join("semantic_context_edge_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("edge.rs");

        let mut f = std::fs::File::create(&file).unwrap();
        writeln!(f, "single line").unwrap();
        drop(f);

        let (excerpt, _) = build_source_excerpt(&file, Some(1), 100).unwrap();
        assert_eq!(excerpt.start_line, 1);
        assert_eq!(excerpt.end_line, 1);

        let (excerpt, _) = build_source_excerpt(&file, None, 100).unwrap();
        assert_eq!(excerpt.start_line, 1);
        assert_eq!(excerpt.end_line, 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_flatten_symbols_respects_remaining() {
        let sym = lsp_types::DocumentSymbol {
            name: "a".to_string(),
            detail: None,
            kind: lsp_types::SymbolKind::FUNCTION,
            range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 0,
                    character: 0,
                },
                end: lsp_types::Position {
                    line: 5,
                    character: 0,
                },
            },
            selection_range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 0,
                    character: 0,
                },
                end: lsp_types::Position {
                    line: 0,
                    character: 1,
                },
            },
            children: Some(vec![lsp_types::DocumentSymbol {
                name: "b".to_string(),
                detail: None,
                kind: lsp_types::SymbolKind::STRUCT,
                range: lsp_types::Range {
                    start: lsp_types::Position {
                        line: 1,
                        character: 0,
                    },
                    end: lsp_types::Position {
                        line: 2,
                        character: 0,
                    },
                },
                selection_range: lsp_types::Range {
                    start: lsp_types::Position {
                        line: 1,
                        character: 0,
                    },
                    end: lsp_types::Position {
                        line: 1,
                        character: 1,
                    },
                },
                children: None,
                tags: None,
                deprecated: None,
            }]),
            tags: None,
            deprecated: None,
        };

        let mut output = Vec::new();
        let mut remaining = 1;
        flatten_symbols(&[sym], "f.rs", &mut output, &mut remaining);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].name, "a");
        assert_eq!(remaining, 0);
    }
}
