//! LSP evidence collector with trait-based dependency injection.
//!
//! [`LspEvidenceProvider`] abstracts LSP operations so the collector
//! can be tested without a live server. The core [`collect_context`]
//! function assembles an [`LspContextPacket`] from provider results,
//! enforcing budget and recording provenance for every item.
//!
//! # Design
//!
//! - [`LspEvidenceProvider`] is the trait that concrete adapters implement.
//! - [`collect_context`] is the main entry point for all request kinds.
//! - [`collect_hunk_context`] specializes in hunk-aware collection.
//! - [`make_provenance`] and [`item_kind_from_severity`] are helpers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tracing::debug;

use crate::context::{
    AgentContextSource, LspContextBudget, LspContextItem, LspContextItemKind, LspContextMode,
    LspContextPacket, LspContextPacketMode, LspContextRequest, LspContextScore,
    LspEvidenceFreshness, LspEvidenceProvenance, LspRiskMode,
};
use crate::error::LspError;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors specific to context collection.
#[derive(Debug, thiserror::Error)]
pub enum LspContextError {
    /// LSP server is not reachable or not initialized.
    #[error("LSP unavailable: {0}")]
    Unavailable(String),
    /// LSP server is reachable but degraded.
    #[error("LSP degraded: {0}")]
    Degraded(String),
    /// A required operation failed.
    #[error("Required operation failed: {0}")]
    RequiredFailed(String),
    /// Provider returned an LSP error.
    #[error("Provider error: {0}")]
    Provider(#[from] LspError),
}

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

/// Abstracts LSP operations for testable evidence collection.
///
/// Implementors return simplified tuple results so the collector
/// logic can be exercised with lightweight mocks.
///
/// # Sequential call contract (Pass 3 Phase 5)
///
/// The collector's [`collect_context`] and [`collect_hunk_context`]
/// functions are the only production callers of this trait. They
/// invoke provider methods **sequentially** (every `.await` is on a
/// single trait method, never combined with `join!` / `tokio::spawn`).
///
/// This contract matters for provenance-aware adapters like
/// [`crate::evidence_adapter::ServiceLspEvidenceProvider`], which
/// record per-call provenance in a shared side-channel slot. If two
/// trait methods are invoked concurrently on the same adapter
/// instance, the second call would clobber the first caller's
/// provenance before that caller could read it.
///
/// Mock providers (used in tests) are free to ignore this contract
/// — they have no side-channel — but production wiring must enforce
/// it. See
/// [`crate::evidence_adapter::ServiceLspEvidenceProvider`] for the
/// guarded accessor that detects contract violations.
#[async_trait]
pub trait LspEvidenceProvider: Send + Sync {
    /// Diagnostics for a file. Returns `(severity, message, range_text)`.
    async fn diagnostics_for_file(
        &self,
        file: &Path,
    ) -> Result<Vec<(String, String, String)>, LspError>;

    /// Document symbols. Returns `(name, kind, range_text)`.
    async fn document_symbols(
        &self,
        file: &Path,
    ) -> Result<Vec<(String, String, String)>, LspError>;

    /// Go-to-definition. Returns `(file_path, range_text)`.
    async fn go_to_definition(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String)>, LspError>;

    /// Find references. Returns `(file_path, range_text)`.
    async fn find_references(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String)>, LspError>;

    /// Implementations. Returns `(file_path, range_text)`.
    async fn implementations(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String)>, LspError>;

    /// Hover text at a position.
    async fn hover(&self, file: &Path, line: u32, column: u32) -> Result<Option<String>, LspError>;

    /// Document highlights. Returns range_texts.
    async fn document_highlights(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<String>, LspError>;

    /// Signature help at a position. Returns `(label, documentation)`.
    async fn signature_help(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String)>, LspError>;

    /// Completion candidates at a position. Returns `(label, kind, detail)`.
    async fn completion(
        &self,
        file: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<(String, String, String)>, LspError>;

    /// Semantic tokens for a file range. Returns `(line, col, length, token_type)`.
    async fn semantic_tokens(
        &self,
        file: &Path,
        start_line: u32,
        end_line: u32,
    ) -> Result<Vec<(u32, u32, u32, String)>, LspError>;

    /// Workspace-wide symbol search. Returns `(name, kind, file_path, range_text)`.
    async fn workspace_symbols(
        &self,
        query: &str,
    ) -> Result<Vec<(String, String, String, String)>, LspError>;

    /// Operational state label (e.g. "ready", "initializing").
    async fn operational_state(&self) -> String;

    /// Returns `(server_id, generation)`.
    async fn server_info(&self) -> (Option<String>, Option<u64>);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build provenance metadata for a context item.
pub fn make_provenance(
    server_id: &str,
    generation: Option<u64>,
    operation: &str,
    freshness: LspEvidenceFreshness,
) -> LspEvidenceProvenance {
    LspEvidenceProvenance {
        server_id: server_id.to_string(),
        server_generation: generation,
        operation: operation.to_string(),
        freshness,
        capability_decision: None,
        document_version: None,
        age_ms: None,
        post_restart: generation.is_some_and(|g| g > 1),
    }
}

/// Map a diagnostic severity string to an [`LspContextItemKind`].
pub fn item_kind_from_severity(severity: &str) -> LspContextItemKind {
    match severity.to_lowercase().as_str() {
        "error" | "hint" | "information" | "warning" => LspContextItemKind::Diagnostic,
        _ => LspContextItemKind::Diagnostic,
    }
}

/// Map diagnostic severity to an `is_error` flag.
fn severity_is_error(severity: &str) -> bool {
    matches!(
        severity.to_lowercase().as_str(),
        "error" | "1" | "diagnosticseverity::error"
    )
}

/// Map freshness to a rank (lower = fresher).
fn freshness_rank(f: LspEvidenceFreshness) -> u32 {
    match f {
        LspEvidenceFreshness::Fresh => 0,
        LspEvidenceFreshness::PossiblyStale => 1,
        LspEvidenceFreshness::Stale | LspEvidenceFreshness::RetainedAfterRestart => 2,
        LspEvidenceFreshness::StaleAfterEdit => 3,
        LspEvidenceFreshness::ServerGenerationMismatch => 4,
        LspEvidenceFreshness::Unknown => 5,
    }
}

/// Determine evidence freshness from operational state.
fn freshness_for_state(state: &str) -> LspEvidenceFreshness {
    match state {
        "ready" => LspEvidenceFreshness::Fresh,
        "indexing" | "degraded" => LspEvidenceFreshness::PossiblyStale,
        "initializing" | "starting" => LspEvidenceFreshness::Unknown,
        _ => LspEvidenceFreshness::Stale,
    }
}

/// Create an operational note item.
fn operational_note(message: String, provenance: LspEvidenceProvenance) -> LspContextItem {
    LspContextItem {
        range: None,
        source: None,
        kind: LspContextItemKind::OperationalNote,
        file: PathBuf::new(),
        line: None,
        column: None,
        message,
        symbol: None,
        provenance,
        score: LspContextScore {
            priority: 0,
            is_hunk_local: false,
            is_error: false,
            is_same_file: false,
            freshness_rank: 0,
        },
        payload: None,
    }
}

/// Parse a range_text like "(1:5)-(3:10)" into (line, column) of the start.
///
/// Handles formats: `(line:col)-(line:col)`, `(line:col)-(line:col`, etc.
fn parse_range_start(range_text: &str) -> (Option<u32>, Option<u32>) {
    // Find the first parenthesized segment or the first "line:col" pair.
    let trimmed = range_text.trim();
    // Try to find `(` ... `:` ... `)` for the start position.
    if let Some(open) = trimmed.find('(') {
        let rest = &trimmed[open + 1..];
        if let Some(close) = rest.find(')') {
            let inner = &rest[..close];
            let nums: Vec<&str> = inner.split(':').collect();
            if nums.len() >= 2 {
                let line = nums[0].parse::<u32>().ok().map(|l| l.saturating_sub(1));
                let col = nums[1].parse::<u32>().ok().map(|c| c.saturating_sub(1));
                return (line, col);
            }
        }
    }
    // Fallback: no parens, try bare "line:col" or just "line".
    let nums: Vec<&str> = trimmed.split(':').collect();
    if nums.len() >= 2 {
        let line = nums[0].parse::<u32>().ok().map(|l| l.saturating_sub(1));
        let col = nums[1].parse::<u32>().ok().map(|c| c.saturating_sub(1));
        return (line, col);
    }
    if let Ok(n) = trimmed.parse::<u32>() {
        return (Some(n.saturating_sub(1)), None);
    }
    (None, None)
}

/// Check if a diagnostic line falls within a range.
fn line_in_range(line: u32, start: u32, end: u32) -> bool {
    line >= start && line < end
}

// ---------------------------------------------------------------------------
// Core collection
// ---------------------------------------------------------------------------

/// Main collection function that assembles an [`LspContextPacket`].
///
/// Dispatches on the request kind and delegates to the provider for
/// each capability-gated operation. Errors from the provider are
/// surfaced as operational notes (Opportunistic) or propagated
/// (Required).
pub async fn collect_context(
    provider: &dyn LspEvidenceProvider,
    request: &LspContextRequest,
    budget: &LspContextBudget,
    mode: &LspContextMode,
) -> Result<LspContextPacket, LspContextError> {
    // Disabled mode: return empty packet with note.
    if *mode == LspContextPacketMode::Disabled {
        let (_, gen) = provider.server_info().await;
        let prov = make_provenance("unknown", gen, "disabled", LspEvidenceFreshness::Unknown);
        return Ok(LspContextPacket {
            request: request.clone(),
            items: vec![operational_note(
                "LSP context collection is disabled".to_string(),
                prov,
            )],
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: *mode,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: vec!["disabled".to_string()],
            truncation: Default::default(),
        });
    }

    let state = provider.operational_state().await;
    let (server_id, generation) = provider.server_info().await;
    let sid = server_id.as_deref().unwrap_or("unknown");
    let freshness = freshness_for_state(&state);

    // If server is not usable and mode is Required, fail immediately.
    if !is_usable_state(&state) && *mode == LspContextPacketMode::Required {
        return Err(LspContextError::RequiredFailed(format!(
            "LSP server not usable in Required mode: {state}"
        )));
    }

    let mut items = Vec::new();
    let mut notes = Vec::new();

    // If server is not usable, return partial note (Opportunistic).
    if !is_usable_state(&state) {
        let prov = make_provenance(sid, generation, "operational_state", freshness);
        items.push(operational_note(format!("LSP state: {state}"), prov));
        return Ok(build_packet(request, mode, items, notes));
    }

    match request {
        LspContextRequest::File {
            file,
            line_ranges,
            include_symbols,
            include_diagnostics,
        } => {
            collect_file_context(
                provider,
                file,
                line_ranges,
                *include_symbols,
                *include_diagnostics,
                budget,
                sid,
                generation,
                freshness,
                &mut items,
                &mut notes,
            )
            .await;
        }
        LspContextRequest::Hunk {
            file,
            hunks,
            include_references,
            include_definitions,
            include_implementations,
            include_semantic_tokens,
            include_security_evidence: _,
        } => {
            let hunk_items = collect_hunk_context(
                provider,
                file,
                hunks,
                *include_references,
                *include_definitions,
                *include_implementations,
                *include_semantic_tokens,
                budget,
            )
            .await;
            match hunk_items {
                Ok(mut h) => items.append(&mut h),
                Err(e) => {
                    let prov = make_provenance(sid, generation, "hunk_context", freshness);
                    if *mode == LspContextPacketMode::Required {
                        return Err(LspContextError::RequiredFailed(e.to_string()));
                    }
                    notes.push(format!("hunk context degraded: {e}"));
                    items.push(operational_note(format!("hunk context error: {e}"), prov));
                }
            }
        }
        LspContextRequest::Symbol {
            file,
            position,
            include_references,
            include_implementations,
            include_call_like_context: _,
        } => {
            collect_symbol_context(
                provider,
                file,
                position.line,
                position.character,
                *include_references,
                *include_implementations,
                budget,
                sid,
                generation,
                freshness,
                &mut items,
                &mut notes,
            )
            .await;
        }
        LspContextRequest::Review {
            changed_files,
            hunks: _,
            risk_mode,
        } => {
            collect_review_context(
                provider,
                changed_files,
                *risk_mode,
                budget,
                sid,
                generation,
                freshness,
                &mut items,
                &mut notes,
            )
            .await;
        }
        LspContextRequest::ImpactAnalysis {
            symbol,
            changed_files,
            max_refs,
            max_files,
            max_depth,
        } => {
            collect_impact_analysis(
                provider,
                symbol,
                changed_files,
                *max_refs,
                *max_files,
                *max_depth,
                budget,
                sid,
                generation,
                freshness,
                &mut items,
                &mut notes,
            )
            .await;
        }
        LspContextRequest::TestFailureRepair {
            test_file,
            failure_message,
            related_files,
        } => {
            collect_test_failure_repair(
                provider,
                test_file,
                failure_message,
                related_files,
                budget,
                sid,
                generation,
                freshness,
                &mut items,
                &mut notes,
            )
            .await;
        }
        LspContextRequest::InterfaceBoundary {
            file,
            symbol,
            include_implementations,
        } => {
            collect_interface_boundary(
                provider,
                file,
                symbol.as_deref(),
                *include_implementations,
                budget,
                sid,
                generation,
                freshness,
                &mut items,
                &mut notes,
            )
            .await;
        }
        LspContextRequest::CrossFileRepair {
            primary_file,
            related_files,
            ranges,
        } => {
            collect_cross_file_repair(
                provider,
                primary_file,
                related_files,
                ranges,
                budget,
                sid,
                generation,
                freshness,
                &mut items,
                &mut notes,
            )
            .await;
        }
        LspContextRequest::CallNeighborhood {
            file,
            line,
            column,
            direction,
            max_depth,
            max_callers,
            max_callees,
        } => {
            collect_call_neighborhood(
                provider,
                file,
                *line,
                *column,
                *direction,
                *max_depth,
                *max_callers,
                *max_callees,
                budget,
                sid,
                generation,
                freshness,
                &mut items,
                &mut notes,
            )
            .await;
        }
    }

    // Dedup.
    items = crate::context::dedup_context_items(items);

    // Rank.
    crate::context::rank_context_items(&mut items, request);

    let mut packet = build_packet(request, mode, items, notes);

    // Enforce budget.
    crate::context::enforce_context_budget(&mut packet);

    Ok(packet)
}

fn is_usable_state(state: &str) -> bool {
    matches!(state, "ready" | "indexing" | "degraded")
}

fn build_packet(
    request: &LspContextRequest,
    mode: &LspContextMode,
    items: Vec<LspContextItem>,
    notes: Vec<String>,
) -> LspContextPacket {
    LspContextPacket {
        request: request.clone(),
        items,
        previews: Vec::new(),
        preview_ids: Vec::new(),
        mode: *mode,
        workspace_root: None,
        generated_at: None,
        server_id: None,
        server_generation: None,
        operational_state: None,
        budget: None,
        notes,
        truncation: Default::default(),
    }
}

// ---------------------------------------------------------------------------
// File context
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn collect_file_context(
    provider: &dyn LspEvidenceProvider,
    file: &Path,
    line_ranges: &[crate::context::LineRange],
    include_symbols: bool,
    include_diagnostics: bool,
    budget: &LspContextBudget,
    sid: &str,
    generation: Option<u64>,
    freshness: LspEvidenceFreshness,
    items: &mut Vec<LspContextItem>,
    notes: &mut Vec<String>,
) {
    // Diagnostics
    if include_diagnostics {
        match provider.diagnostics_for_file(file).await {
            Ok(diagnostics) => {
                let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
                let mut count = 0;
                for (severity, message, range_text) in &diagnostics {
                    if count >= budget.max_diagnostics {
                        notes.push(format!(
                            "diagnostics truncated at {}",
                            budget.max_diagnostics
                        ));
                        break;
                    }
                    let (line, column) = parse_range_start(range_text);
                    // Filter by line ranges if provided.
                    if !line_ranges.is_empty() {
                        let in_range = line_ranges
                            .iter()
                            .any(|r| line.is_some_and(|l| line_in_range(l, r.start, r.end)));
                        if !in_range {
                            continue;
                        }
                    }
                    items.push(LspContextItem {
                        range: None,
                        source: None,
                        kind: LspContextItemKind::Diagnostic,
                        file: file.to_path_buf(),
                        line,
                        column,
                        message: message.clone(),
                        symbol: None,
                        provenance: prov.clone(),
                        score: LspContextScore {
                            priority: if severity_is_error(severity) { 10 } else { 5 },
                            is_hunk_local: false,
                            is_error: severity_is_error(severity),
                            is_same_file: true,
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                    count += 1;
                }
            }
            Err(e) => {
                debug!("diagnostics_for_file failed: {e}");
                let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
                notes.push(format!("diagnostics: {e}"));
                items.push(operational_note(
                    format!("diagnostics unavailable: {e}"),
                    prov,
                ));
            }
        }
    }

    // Symbols
    if include_symbols {
        match provider.document_symbols(file).await {
            Ok(symbols) => {
                let prov =
                    make_provenance(sid, generation, "textDocument/documentSymbol", freshness);
                for (name, kind, range_text) in symbols.iter().take(budget.max_symbols) {
                    let (line, column) = parse_range_start(range_text);
                    items.push(LspContextItem {
                        range: None,
                        kind: LspContextItemKind::WorkspaceSymbol,
                        file: file.to_path_buf(),
                        line,
                        column,
                        message: format!("{name} ({kind})"),
                        symbol: Some(name.clone()),
                        source: None,
                        provenance: prov.clone(),
                        score: LspContextScore {
                            priority: 8,
                            is_hunk_local: false,
                            is_error: false,
                            is_same_file: true,
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
                if symbols.len() > budget.max_symbols {
                    notes.push(format!("symbols truncated at {}", budget.max_symbols));
                }
            }
            Err(e) => {
                debug!("document_symbols failed: {e}");
                let prov =
                    make_provenance(sid, generation, "textDocument/documentSymbol", freshness);
                notes.push(format!("symbols: {e}"));
                items.push(operational_note(format!("symbols unavailable: {e}"), prov));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Hunk context
// ---------------------------------------------------------------------------

/// Collect hunk-aware evidence for a single file.
///
/// For each hunk, collects diagnostics overlapping the range,
/// definitions/references/implementations at the center line,
/// and a semantic token summary.
pub async fn collect_hunk_context(
    provider: &dyn LspEvidenceProvider,
    file: &Path,
    hunks: &[crate::context::HunkRange],
    include_references: bool,
    include_definitions: bool,
    include_implementations: bool,
    include_semantic_tokens: bool,
    budget: &LspContextBudget,
) -> Result<Vec<LspContextItem>, LspContextError> {
    let (server_id, generation) = provider.server_info().await;
    let sid = server_id.as_deref().unwrap_or("unknown");
    let state = provider.operational_state().await;
    let freshness = freshness_for_state(&state);

    let mut items = Vec::new();

    // Collect all diagnostics once, then filter per hunk.
    let all_diagnostics = provider
        .diagnostics_for_file(file)
        .await
        .unwrap_or_default();

    let prov_diag = make_provenance(sid, generation, "textDocument/diagnostic", freshness);

    for hunk in hunks {
        // Diagnostics overlapping this hunk.
        for (severity, message, range_text) in &all_diagnostics {
            let (line, _col) = parse_range_start(range_text);
            if let Some(l) = line {
                if line_in_range(l, hunk.start, hunk.end) {
                    items.push(LspContextItem {
                        range: None,
                        source: None,
                        kind: LspContextItemKind::Diagnostic,
                        file: file.to_path_buf(),
                        line: Some(l),
                        column: None,
                        message: message.clone(),
                        symbol: None,
                        provenance: prov_diag.clone(),
                        score: LspContextScore {
                            priority: if severity_is_error(severity) { 15 } else { 10 },
                            is_hunk_local: true,
                            is_error: severity_is_error(severity),
                            is_same_file: true,
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
            }
        }

        // Center line for definitions/references.
        let center = hunk.start + (hunk.end.saturating_sub(hunk.start)) / 2;

        // Definitions at center.
        if include_definitions {
            match provider.go_to_definition(file, center, 0).await {
                Ok(defs) => {
                    let prov =
                        make_provenance(sid, generation, "textDocument/definition", freshness);
                    for (def_file, range_text) in &defs {
                        let (line, column) = parse_range_start(range_text);
                        items.push(LspContextItem {
                            range: None,
                            source: None,
                            kind: LspContextItemKind::Definition,
                            file: PathBuf::from(def_file),
                            line,
                            column,
                            message: format!("definition: {range_text}"),
                            symbol: None,
                            provenance: prov.clone(),
                            score: LspContextScore {
                                priority: 12,
                                is_hunk_local: def_file == file.to_str().unwrap_or(""),
                                is_error: false,
                                is_same_file: def_file == file.to_str().unwrap_or(""),
                                freshness_rank: freshness_rank(freshness),
                            },
                            payload: None,
                        });
                    }
                }
                Err(e) => {
                    debug!("go_to_definition at hunk center failed: {e}");
                }
            }
        }

        // References at center (capped per hunk).
        if include_references {
            match provider.find_references(file, center, 0).await {
                Ok(refs) => {
                    let prov =
                        make_provenance(sid, generation, "textDocument/references", freshness);
                    let cap = budget.max_references / hunks.len().max(1);
                    for (ref_file, range_text) in refs.iter().take(cap) {
                        let (line, column) = parse_range_start(range_text);
                        items.push(LspContextItem {
                            range: None,
                            source: None,
                            kind: LspContextItemKind::Reference,
                            file: PathBuf::from(ref_file),
                            line,
                            column,
                            message: format!("reference: {range_text}"),
                            symbol: None,
                            provenance: prov.clone(),
                            score: LspContextScore {
                                priority: 7,
                                is_hunk_local: ref_file == file.to_str().unwrap_or(""),
                                is_error: false,
                                is_same_file: ref_file == file.to_str().unwrap_or(""),
                                freshness_rank: freshness_rank(freshness),
                            },
                            payload: None,
                        });
                    }
                }
                Err(e) => {
                    debug!("find_references at hunk center failed: {e}");
                }
            }
        }

        // Implementations at center (capped per hunk).
        if include_implementations {
            match provider.implementations(file, center, 0).await {
                Ok(impls) => {
                    let prov =
                        make_provenance(sid, generation, "textDocument/implementation", freshness);
                    let cap = budget.max_references / hunks.len().max(1);
                    for (impl_file, range_text) in impls.iter().take(cap) {
                        let (line, column) = parse_range_start(range_text);
                        items.push(LspContextItem {
                            range: None,
                            source: None,
                            kind: LspContextItemKind::Implementation,
                            file: PathBuf::from(impl_file),
                            line,
                            column,
                            message: format!("implementation: {range_text}"),
                            symbol: None,
                            provenance: prov.clone(),
                            score: LspContextScore {
                                priority: 9,
                                is_hunk_local: impl_file == file.to_str().unwrap_or(""),
                                is_error: false,
                                is_same_file: impl_file == file.to_str().unwrap_or(""),
                                freshness_rank: freshness_rank(freshness),
                            },
                            payload: None,
                        });
                    }
                }
                Err(e) => {
                    debug!("implementations at hunk center failed: {e}");
                }
            }
        }
    }

    // Semantic token summary (file-level, not per-hunk).
    if include_semantic_tokens {
        match provider.semantic_tokens(file, 0, u32::MAX).await {
            Ok(tokens) => {
                let prov =
                    make_provenance(sid, generation, "textDocument/semanticTokens", freshness);
                // Summarize by token type counts.
                let mut type_counts: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for (_line, _col, _len, token_type) in &tokens {
                    *type_counts.entry(token_type.clone()).or_insert(0) += 1;
                }
                if !type_counts.is_empty() {
                    let summary: Vec<String> = type_counts
                        .iter()
                        .map(|(t, c)| format!("{t}: {c}"))
                        .collect();
                    items.push(LspContextItem {
                        range: None,
                        source: None,
                        kind: LspContextItemKind::SemanticTokenSummary,
                        file: file.to_path_buf(),
                        line: None,
                        column: None,
                        message: format!("semantic tokens: {}", summary.join(", ")),
                        symbol: None,
                        provenance: prov,
                        score: LspContextScore {
                            priority: 2,
                            is_hunk_local: false,
                            is_error: false,
                            is_same_file: true,
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
            }
            Err(e) => {
                debug!("semantic_tokens failed: {e}");
            }
        }
    }

    Ok(items)
}

// ---------------------------------------------------------------------------
// Symbol context
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn collect_symbol_context(
    provider: &dyn LspEvidenceProvider,
    file: &Path,
    line: u32,
    column: u32,
    include_references: bool,
    include_implementations: bool,
    budget: &LspContextBudget,
    sid: &str,
    generation: Option<u64>,
    freshness: LspEvidenceFreshness,
    items: &mut Vec<LspContextItem>,
    notes: &mut Vec<String>,
) {
    // Definition
    match provider.go_to_definition(file, line, column).await {
        Ok(defs) => {
            let prov = make_provenance(sid, generation, "textDocument/definition", freshness);
            for (def_file, range_text) in &defs {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: None,
                    kind: LspContextItemKind::Definition,
                    file: PathBuf::from(def_file),
                    line: l,
                    column: c,
                    message: format!("definition: {range_text}"),
                    symbol: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: 12,
                        is_hunk_local: false,
                        is_error: false,
                        is_same_file: def_file == file.to_str().unwrap_or(""),
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            notes.push(format!("definition: {e}"));
        }
    }

    // References
    if include_references {
        match provider.find_references(file, line, column).await {
            Ok(refs) => {
                let prov = make_provenance(sid, generation, "textDocument/references", freshness);
                for (ref_file, range_text) in refs.iter().take(budget.max_references) {
                    let (l, c) = parse_range_start(range_text);
                    items.push(LspContextItem {
                        range: None,
                        source: None,
                        kind: LspContextItemKind::Reference,
                        file: PathBuf::from(ref_file),
                        line: l,
                        column: c,
                        message: format!("reference: {range_text}"),
                        symbol: None,
                        provenance: prov.clone(),
                        score: LspContextScore {
                            priority: 7,
                            is_hunk_local: false,
                            is_error: false,
                            is_same_file: ref_file == file.to_str().unwrap_or(""),
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
            }
            Err(e) => {
                notes.push(format!("references: {e}"));
            }
        }
    }

    // Implementations
    if include_implementations {
        match provider.implementations(file, line, column).await {
            Ok(impls) => {
                let prov =
                    make_provenance(sid, generation, "textDocument/implementation", freshness);
                for (impl_file, range_text) in &impls {
                    let (l, c) = parse_range_start(range_text);
                    items.push(LspContextItem {
                        range: None,
                        source: None,
                        kind: LspContextItemKind::Implementation,
                        file: PathBuf::from(impl_file),
                        line: l,
                        column: c,
                        message: format!("implementation: {range_text}"),
                        symbol: None,
                        provenance: prov.clone(),
                        score: LspContextScore {
                            priority: 9,
                            is_hunk_local: false,
                            is_error: false,
                            is_same_file: impl_file == file.to_str().unwrap_or(""),
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
            }
            Err(e) => {
                notes.push(format!("implementations: {e}"));
            }
        }
    }

    // Hover
    match provider.hover(file, line, column).await {
        Ok(Some(text)) => {
            let prov = make_provenance(sid, generation, "textDocument/hover", freshness);
            items.push(LspContextItem {
                range: None,
                source: None,
                kind: LspContextItemKind::Hover,
                file: file.to_path_buf(),
                line: Some(line),
                column: Some(column),
                message: text,
                symbol: None,
                provenance: prov,
                score: LspContextScore {
                    priority: 6,
                    is_hunk_local: false,
                    is_error: false,
                    is_same_file: true,
                    freshness_rank: freshness_rank(freshness),
                },
                payload: None,
            });
        }
        Ok(None) => {}
        Err(e) => {
            notes.push(format!("hover: {e}"));
        }
    }

    // Document highlights
    match provider.document_highlights(file, line, column).await {
        Ok(highlights) => {
            let prov =
                make_provenance(sid, generation, "textDocument/documentHighlight", freshness);
            for range_text in &highlights {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: None,
                    kind: LspContextItemKind::DocumentHighlight,
                    file: file.to_path_buf(),
                    line: l,
                    column: c,
                    message: format!("highlight: {range_text}"),
                    symbol: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: 5,
                        is_hunk_local: false,
                        is_error: false,
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            notes.push(format!("highlights: {e}"));
        }
    }

    // Signature help
    match provider.signature_help(file, line, column).await {
        Ok(signatures) => {
            let prov = make_provenance(sid, generation, "textDocument/signatureHelp", freshness);
            for (label, documentation) in &signatures {
                items.push(LspContextItem {
                    range: None,
                    kind: LspContextItemKind::SignatureHelp,
                    file: file.to_path_buf(),
                    line: Some(line),
                    column: Some(column),
                    message: if documentation.is_empty() {
                        label.clone()
                    } else {
                        format!("{label}: {documentation}")
                    },
                    symbol: Some(label.clone()),
                    source: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: 6,
                        is_hunk_local: false,
                        is_error: false,
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            notes.push(format!("signatureHelp: {e}"));
        }
    }

    // Completion summary (top candidates only)
    match provider.completion(file, line, column).await {
        Ok(completions) => {
            let prov = make_provenance(sid, generation, "textDocument/completion", freshness);
            let cap = budget.max_completion_items;
            for (label, kind, detail) in completions.iter().take(cap) {
                let summary = if detail.is_empty() {
                    format!("{label} ({kind})")
                } else {
                    format!("{label} ({kind}): {detail}")
                };
                items.push(LspContextItem {
                    range: None,
                    kind: LspContextItemKind::CompletionSummary,
                    file: file.to_path_buf(),
                    line: Some(line),
                    column: Some(column),
                    message: summary,
                    symbol: Some(label.clone()),
                    source: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: 4,
                        is_hunk_local: false,
                        is_error: false,
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
            if completions.len() > cap {
                notes.push(format!(
                    "completions truncated at {cap} of {}",
                    completions.len()
                ));
            }
        }
        Err(e) => {
            notes.push(format!("completion: {e}"));
        }
    }

    // Semantic tokens (bounded summary, not raw tokens)
    match provider
        .semantic_tokens(file, line.saturating_sub(5), line + 10)
        .await
    {
        Ok(tokens) => {
            let prov = make_provenance(sid, generation, "textDocument/semanticTokens", freshness);
            // Summarize: count unique token types in the range.
            let mut type_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for (_l, _c, _len, token_type) in &tokens {
                *type_counts.entry(token_type.clone()).or_insert(0) += 1;
            }
            let summary: Vec<_> = type_counts
                .iter()
                .map(|(t, c)| format!("{t}({c})"))
                .collect();
            if !summary.is_empty() {
                items.push(LspContextItem {
                    range: None,
                    source: None,
                    kind: LspContextItemKind::SemanticTokenSummary,
                    file: file.to_path_buf(),
                    line: Some(line.saturating_sub(5)),
                    column: None,
                    message: format!("semantic tokens: {}", summary.join(", ")),
                    symbol: None,
                    provenance: prov,
                    score: LspContextScore {
                        priority: 3,
                        is_hunk_local: false,
                        is_error: false,
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            notes.push(format!("semanticTokens: {e}"));
        }
    }

    // Workspace symbols (bounded summary of matching symbols).
    // Only collected in review/standard+ risk modes for efficiency.
    if budget.max_symbols > 0 {
        match provider.workspace_symbols("").await {
            Ok(symbols) => {
                let prov = make_provenance(sid, generation, "workspace/symbol", freshness);
                let before = symbols.len();
                let capped: Vec<_> = symbols.iter().take(budget.max_symbols).collect();
                if capped.len() < before {
                    notes.push(format!(
                        "workspaceSymbols truncated at {} of {}",
                        capped.len(),
                        before
                    ));
                }
                for (name, kind, file_path, range_text) in &capped {
                    let (l, c) = parse_range_start(range_text);
                    items.push(LspContextItem {
                        range: None,
                        kind: LspContextItemKind::WorkspaceSymbol,
                        file: PathBuf::from(file_path),
                        line: l,
                        column: c,
                        message: format!("{kind}: {name}"),
                        symbol: Some(name.clone()),
                        source: None,
                        provenance: prov.clone(),
                        score: LspContextScore {
                            priority: 3,
                            is_hunk_local: false,
                            is_error: false,
                            is_same_file: file_path == file.to_str().unwrap_or(""),
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
            }
            Err(e) => {
                notes.push(format!("workspaceSymbols: {e}"));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Review context
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn collect_review_context(
    provider: &dyn LspEvidenceProvider,
    changed_files: &[PathBuf],
    risk_mode: LspRiskMode,
    budget: &LspContextBudget,
    sid: &str,
    generation: Option<u64>,
    freshness: LspEvidenceFreshness,
    items: &mut Vec<LspContextItem>,
    notes: &mut Vec<String>,
) {
    let files_cap = budget.max_files.min(changed_files.len());
    for file in changed_files.iter().take(files_cap) {
        // Diagnostics
        match provider.diagnostics_for_file(file).await {
            Ok(diagnostics) => {
                let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
                let before = diagnostics.len();
                let capped: Vec<_> = diagnostics.iter().take(budget.max_diagnostics).collect();
                if capped.len() < before {
                    notes.push(format!(
                        "diagnostics truncated at {} of {} for {}",
                        capped.len(),
                        before,
                        file.display()
                    ));
                }
                for (severity, message, range_text) in &capped {
                    let (line, column) = parse_range_start(range_text);
                    items.push(LspContextItem {
                        range: None,
                        source: None,
                        kind: LspContextItemKind::Diagnostic,
                        file: file.to_path_buf(),
                        line,
                        column,
                        message: message.clone(),
                        symbol: None,
                        provenance: prov.clone(),
                        score: LspContextScore {
                            priority: if severity_is_error(severity) { 10 } else { 5 },
                            is_hunk_local: false,
                            is_error: severity_is_error(severity),
                            is_same_file: true,
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
            }
            Err(e) => {
                notes.push(format!("diagnostics for {}: {e}", file.display()));
            }
        }

        // References are only collected in Standard/Aggressive risk modes.
        if matches!(risk_mode, LspRiskMode::Standard | LspRiskMode::Aggressive) {
            // Collect symbols first to find positions for references.
            if let Ok(symbols) = provider.document_symbols(file).await {
                let prov_ref =
                    make_provenance(sid, generation, "textDocument/references", freshness);
                let mut ref_count = 0;
                for (_name, _kind, range_text) in &symbols {
                    if ref_count >= budget.max_references {
                        break;
                    }
                    let (line, _col) = parse_range_start(range_text);
                    if let Some(l) = line {
                        if let Ok(refs) = provider.find_references(file, l, 0).await {
                            for (ref_file, ref_range) in &refs {
                                let (rl, rc) = parse_range_start(ref_range);
                                items.push(LspContextItem {
                                    range: None,
                                    source: None,
                                    kind: LspContextItemKind::Reference,
                                    file: PathBuf::from(ref_file),
                                    line: rl,
                                    column: rc,
                                    message: format!("reference: {ref_range}"),
                                    symbol: None,
                                    provenance: prov_ref.clone(),
                                    score: LspContextScore {
                                        priority: 7,
                                        is_hunk_local: false,
                                        is_error: false,
                                        is_same_file: ref_file == file.to_str().unwrap_or(""),
                                        freshness_rank: freshness_rank(freshness),
                                    },
                                    payload: None,
                                });
                                ref_count += 1;
                                if ref_count >= budget.max_references {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Definitions for first symbol in each file (always).
        if let Ok(symbols) = provider.document_symbols(file).await {
            if let Some((_name, _kind, range_text)) = symbols.first() {
                let (line, col) = parse_range_start(range_text);
                if let (Some(l), Some(c)) = (line, col) {
                    if let Ok(defs) = provider.go_to_definition(file, l, c).await {
                        let prov =
                            make_provenance(sid, generation, "textDocument/definition", freshness);
                        for (def_file, def_range) in &defs {
                            let (dl, dc) = parse_range_start(def_range);
                            items.push(LspContextItem {
                                range: None,
                                source: None,
                                kind: LspContextItemKind::Definition,
                                file: PathBuf::from(def_file),
                                line: dl,
                                column: dc,
                                message: format!("definition: {def_range}"),
                                symbol: None,
                                provenance: prov.clone(),
                                score: LspContextScore {
                                    priority: 12,
                                    is_hunk_local: false,
                                    is_error: false,
                                    is_same_file: def_file == file.to_str().unwrap_or(""),
                                    freshness_rank: freshness_rank(freshness),
                                },
                                payload: None,
                            });
                        }
                    }
                }
            }
        }

        // Implementations for first symbol in each file (Standard/Aggressive).
        if matches!(risk_mode, LspRiskMode::Standard | LspRiskMode::Aggressive) {
            if let Ok(symbols) = provider.document_symbols(file).await {
                if let Some((_name, _kind, range_text)) = symbols.first() {
                    let (line, col) = parse_range_start(range_text);
                    if let (Some(l), Some(c)) = (line, col) {
                        if let Ok(impls) = provider.implementations(file, l, c).await {
                            let prov = make_provenance(
                                sid,
                                generation,
                                "textDocument/implementation",
                                freshness,
                            );
                            for (impl_file, impl_range) in &impls {
                                let (il, ic) = parse_range_start(impl_range);
                                items.push(LspContextItem {
                                    range: None,
                                    source: None,
                                    kind: LspContextItemKind::Implementation,
                                    file: PathBuf::from(impl_file),
                                    line: il,
                                    column: ic,
                                    message: format!("implementation: {impl_range}"),
                                    symbol: None,
                                    provenance: prov.clone(),
                                    score: LspContextScore {
                                        priority: 9,
                                        is_hunk_local: false,
                                        is_error: false,
                                        is_same_file: impl_file == file.to_str().unwrap_or(""),
                                        freshness_rank: freshness_rank(freshness),
                                    },
                                    payload: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Hover for first symbol in each file (Standard/Aggressive).
        if matches!(risk_mode, LspRiskMode::Standard | LspRiskMode::Aggressive) {
            if let Ok(symbols) = provider.document_symbols(file).await {
                if let Some((_name, _kind, range_text)) = symbols.first() {
                    let (line, col) = parse_range_start(range_text);
                    if let (Some(l), Some(c)) = (line, col) {
                        if let Ok(Some(text)) = provider.hover(file, l, c).await {
                            let prov =
                                make_provenance(sid, generation, "textDocument/hover", freshness);
                            items.push(LspContextItem {
                                range: None,
                                source: None,
                                kind: LspContextItemKind::Hover,
                                file: file.to_path_buf(),
                                line: Some(l),
                                column: Some(c),
                                message: text,
                                symbol: None,
                                provenance: prov,
                                score: LspContextScore {
                                    priority: 6,
                                    is_hunk_local: false,
                                    is_error: false,
                                    is_same_file: true,
                                    freshness_rank: freshness_rank(freshness),
                                },
                                payload: None,
                            });
                        }
                    }
                }
            }
        }

        // Semantic token summary for each file (Aggressive only).
        if matches!(risk_mode, LspRiskMode::Aggressive) {
            match provider.semantic_tokens(file, 0, u32::MAX).await {
                Ok(tokens) => {
                    let prov =
                        make_provenance(sid, generation, "textDocument/semanticTokens", freshness);
                    let mut type_counts: std::collections::HashMap<String, usize> =
                        std::collections::HashMap::new();
                    for (_line, _col, _len, token_type) in &tokens {
                        *type_counts.entry(token_type.clone()).or_insert(0) += 1;
                    }
                    if !type_counts.is_empty() {
                        let summary: Vec<String> = type_counts
                            .iter()
                            .map(|(t, c)| format!("{t}: {c}"))
                            .collect();
                        items.push(LspContextItem {
                            range: None,
                            source: None,
                            kind: LspContextItemKind::SemanticTokenSummary,
                            file: file.to_path_buf(),
                            line: None,
                            column: None,
                            message: format!("semantic tokens: {}", summary.join(", ")),
                            symbol: None,
                            provenance: prov,
                            score: LspContextScore {
                                priority: 2,
                                is_hunk_local: false,
                                is_error: false,
                                is_same_file: true,
                                freshness_rank: freshness_rank(freshness),
                            },
                            payload: None,
                        });
                    }
                }
                Err(e) => {
                    debug!("semantic_tokens failed: {e}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Impact analysis context
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn collect_impact_analysis(
    provider: &dyn LspEvidenceProvider,
    symbol: &crate::context::SymbolTarget,
    changed_files: &[PathBuf],
    max_refs: usize,
    max_files: usize,
    max_depth: u8,
    budget: &LspContextBudget,
    sid: &str,
    generation: Option<u64>,
    freshness: LspEvidenceFreshness,
    items: &mut Vec<LspContextItem>,
    notes: &mut Vec<String>,
) {
    let file = &symbol.file;
    let line = symbol.position.line;
    let column = symbol.position.character;

    // 1. Definition of target symbol.
    match provider.go_to_definition(file, line, column).await {
        Ok(defs) => {
            let prov = make_provenance(sid, generation, "textDocument/definition", freshness);
            for (def_file, range_text) in &defs {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::LspContext),
                    kind: LspContextItemKind::Definition,
                    file: PathBuf::from(def_file),
                    line: l,
                    column: c,
                    message: format!("definition: {range_text}"),
                    symbol: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: 15,
                        is_hunk_local: false,
                        is_error: false,
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            debug!("impact analysis: go_to_definition failed: {e}");
            let prov = make_provenance(sid, generation, "textDocument/definition", freshness);
            notes.push(format!("impact analysis: definition unavailable: {e}"));
            items.push(operational_note(
                format!("definition unavailable: {e}"),
                prov,
            ));
        }
    }

    // 2. References (capped).
    let mut all_refs = match provider.find_references(file, line, column).await {
        Ok(refs) => refs,
        Err(e) => {
            debug!("impact analysis: find_references failed: {e}");
            let prov = make_provenance(sid, generation, "textDocument/reference", freshness);
            notes.push(format!("impact analysis: references unavailable: {e}"));
            items.push(operational_note(
                format!("references unavailable: {e}"),
                prov,
            ));
            Vec::new()
        }
    };

    // Rank: same-file and changed-file references first.
    let changed_set: std::collections::HashSet<PathBuf> = changed_files.iter().cloned().collect();
    let file_str = file.to_str().unwrap_or("");
    all_refs.sort_by(|a, b| {
        let a_same = a.0 == file_str;
        let b_same = b.0 == file_str;
        let a_changed = changed_set.contains(&PathBuf::from(&a.0));
        let b_changed = changed_set.contains(&PathBuf::from(&b.0));
        b_same
            .cmp(&a_same)
            .then(b_changed.cmp(&a_changed))
            .then(a.0.cmp(&b.0))
    });

    let capped_refs: Vec<_> = all_refs.into_iter().take(max_refs).collect();
    if capped_refs.len() < budget.max_references {
        notes.push(format!(
            "impact analysis: references capped at {}",
            max_refs
        ));
    }

    let prov_ref = make_provenance(sid, generation, "textDocument/reference", freshness);
    for (ref_file, range_text) in &capped_refs {
        let (l, c) = parse_range_start(range_text);
        let ref_path = PathBuf::from(ref_file);
        let is_same_file = ref_path == *file;
        let is_changed = changed_set.contains(&ref_path);
        items.push(LspContextItem {
            range: None,
            source: Some(if is_changed {
                AgentContextSource::Hunk
            } else {
                AgentContextSource::LspContext
            }),
            kind: LspContextItemKind::Reference,
            file: ref_path,
            line: l,
            column: c,
            message: format!("reference: {range_text}"),
            symbol: None,
            provenance: prov_ref.clone(),
            score: LspContextScore {
                priority: if is_same_file {
                    12
                } else if is_changed {
                    10
                } else {
                    5
                },
                is_hunk_local: is_changed,
                is_error: false,
                is_same_file,
                freshness_rank: freshness_rank(freshness),
            },
            payload: None,
        });
    }

    // 3. Implementations (if depth allows).
    if max_depth > 0 {
        match provider.implementations(file, line, column).await {
            Ok(impls) => {
                let prov =
                    make_provenance(sid, generation, "textDocument/implementation", freshness);
                for (impl_file, range_text) in &impls {
                    let (l, c) = parse_range_start(range_text);
                    items.push(LspContextItem {
                        range: None,
                        source: Some(AgentContextSource::LspContext),
                        kind: LspContextItemKind::Implementation,
                        file: PathBuf::from(impl_file),
                        line: l,
                        column: c,
                        message: format!("implementation: {range_text}"),
                        symbol: None,
                        provenance: prov.clone(),
                        score: LspContextScore {
                            priority: 8,
                            is_hunk_local: false,
                            is_error: false,
                            is_same_file: impl_file == file.to_str().unwrap_or(""),
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
            }
            Err(e) => {
                debug!("impact analysis: implementations failed: {e}");
            }
        }
    }

    // 4. Diagnostics in changed files (capped).
    let files_with_diags: Vec<PathBuf> = changed_files.iter().take(max_files).cloned().collect();
    for df in &files_with_diags {
        if let Ok(diagnostics) = provider.diagnostics_for_file(df).await {
            let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
            for (severity, message, range_text) in diagnostics.iter().take(budget.max_diagnostics) {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::Diagnostics),
                    kind: LspContextItemKind::Diagnostic,
                    file: df.clone(),
                    line: l,
                    column: c,
                    message: message.clone(),
                    symbol: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: if severity_is_error(severity) { 10 } else { 5 },
                        is_hunk_local: false,
                        is_error: severity_is_error(severity),
                        is_same_file: false,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Test failure repair context
// ---------------------------------------------------------------------------

/// Conservative symbol extractor for test failure messages.
///
/// Extracts only obvious identifiers, test names, file paths, and
/// line numbers. Never hallucinates; returns empty on ambiguous input.
fn extract_failure_symbols(failure_message: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for word in failure_message.split_whitespace() {
        // Strip trailing punctuation.
        let clean: String = word
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
            .collect();
        if clean.is_empty() {
            continue;
        }

        // Extract Rust-style identifiers: `foo::bar::baz` or `Foo::bar`.
        if clean.contains("::") {
            for part in clean.split("::") {
                let part = part.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                if !part.is_empty()
                    && part.len() >= 2
                    && !seen.contains(part)
                    && part
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_lowercase() || c == '_')
                {
                    seen.insert(part.to_string());
                    symbols.push(part.to_string());
                }
            }
        }

        // Extract identifiers that look like function/type names.
        if !clean.contains("::")
            && clean.len() >= 2
            && clean.chars().all(|c| c.is_alphanumeric() || c == '_')
            && clean
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_')
            && !matches!(
                clean.as_str(),
                "at" | "in"
                    | "on"
                    | "the"
                    | "a"
                    | "an"
                    | "is"
                    | "was"
                    | "thread"
                    | "panicked"
                    | "assertion"
                    | "failed"
                    | "error"
                    | "expected"
                    | "found"
                    | "called"
                    | "result"
                    | "note"
                    | "run"
                    | "test"
                    | "src"
                    | "lib"
                    | "main"
            )
            && !seen.contains(&clean)
        {
            seen.insert(clean.clone());
            symbols.push(clean);
        }
    }

    symbols
}

#[allow(clippy::too_many_arguments)]
async fn collect_test_failure_repair(
    provider: &dyn LspEvidenceProvider,
    test_file: &Path,
    failure_message: &str,
    related_files: &[PathBuf],
    budget: &LspContextBudget,
    sid: &str,
    generation: Option<u64>,
    freshness: LspEvidenceFreshness,
    items: &mut Vec<LspContextItem>,
    notes: &mut Vec<String>,
) {
    // 1. Diagnostics in the test file.
    match provider.diagnostics_for_file(test_file).await {
        Ok(diagnostics) => {
            let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
            for (severity, message, range_text) in diagnostics.iter().take(budget.max_diagnostics) {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::Diagnostics),
                    kind: LspContextItemKind::Diagnostic,
                    file: test_file.to_path_buf(),
                    line: l,
                    column: c,
                    message: message.clone(),
                    symbol: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: if severity_is_error(severity) { 12 } else { 7 },
                        is_hunk_local: false,
                        is_error: severity_is_error(severity),
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            debug!("test failure repair: diagnostics failed: {e}");
            let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
            items.push(operational_note(
                format!("test diagnostics unavailable: {e}"),
                prov,
            ));
        }
    }

    // 2. Symbols in the test file.
    match provider.document_symbols(test_file).await {
        Ok(symbols) => {
            let prov = make_provenance(sid, generation, "textDocument/documentSymbol", freshness);
            for (name, kind, range_text) in symbols.iter().take(budget.max_symbols) {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::LspContext),
                    kind: LspContextItemKind::WorkspaceSymbol,
                    file: test_file.to_path_buf(),
                    line: l,
                    column: c,
                    message: format!("{name} ({kind})"),
                    symbol: Some(name.clone()),
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: 8,
                        is_hunk_local: false,
                        is_error: false,
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            debug!("test failure repair: document_symbols failed: {e}");
        }
    }

    // 3. Extract symbols from failure message and find their definitions.
    let extracted = extract_failure_symbols(failure_message);
    if extracted.is_empty() {
        notes.push(
            "test failure repair: no symbols extracted from failure message (heuristic)"
                .to_string(),
        );
    } else {
        notes.push(format!(
            "test failure repair: extracted {} symbol(s) from failure message (heuristic)",
            extracted.len()
        ));
    }

    for sym_name in extracted.iter().take(5) {
        // Try workspace symbol search to find the symbol's location.
        if let Ok(ws_syms) = provider.workspace_symbols(sym_name).await {
            for (name, kind, file_path, range_text) in ws_syms.iter().take(3) {
                if name == sym_name || name.contains(sym_name.as_str()) {
                    let (l, c) = parse_range_start(range_text);
                    let sym_file = PathBuf::from(file_path);
                    let prov_sym = make_provenance(sid, generation, "workspace/symbol", freshness);

                    items.push(LspContextItem {
                        range: None,
                        source: Some(AgentContextSource::LspContext),
                        kind: LspContextItemKind::WorkspaceSymbol,
                        file: sym_file.clone(),
                        line: l,
                        column: c,
                        message: format!("{name} ({kind})"),
                        symbol: Some(name.clone()),
                        provenance: prov_sym.clone(),
                        score: LspContextScore {
                            priority: 10,
                            is_hunk_local: false,
                            is_error: false,
                            is_same_file: false,
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });

                    // Try to get definition for this symbol.
                    if let Some(line) = l {
                        if let Ok(defs) = provider
                            .go_to_definition(&sym_file, line, c.unwrap_or(0))
                            .await
                        {
                            let prov_def = make_provenance(
                                sid,
                                generation,
                                "textDocument/definition",
                                freshness,
                            );
                            for (def_file, def_range) in &defs {
                                let (dl, dc) = parse_range_start(def_range);
                                items.push(LspContextItem {
                                    range: None,
                                    source: Some(AgentContextSource::LspContext),
                                    kind: LspContextItemKind::Definition,
                                    file: PathBuf::from(def_file),
                                    line: dl,
                                    column: dc,
                                    message: format!("definition: {def_range}"),
                                    symbol: Some(name.clone()),
                                    provenance: prov_def.clone(),
                                    score: LspContextScore {
                                        priority: 12,
                                        is_hunk_local: false,
                                        is_error: false,
                                        is_same_file: false,
                                        freshness_rank: freshness_rank(freshness),
                                    },
                                    payload: None,
                                });
                            }
                        }
                    }
                    break;
                }
            }
        }
    }

    // 4. Diagnostics in related files.
    for rf in related_files.iter().take(budget.max_files) {
        if let Ok(diagnostics) = provider.diagnostics_for_file(rf).await {
            let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
            for (severity, message, range_text) in diagnostics.iter().take(budget.max_diagnostics) {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::Diagnostics),
                    kind: LspContextItemKind::Diagnostic,
                    file: rf.clone(),
                    line: l,
                    column: c,
                    message: message.clone(),
                    symbol: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: if severity_is_error(severity) { 10 } else { 5 },
                        is_hunk_local: false,
                        is_error: severity_is_error(severity),
                        is_same_file: false,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Interface boundary context
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn collect_interface_boundary(
    provider: &dyn LspEvidenceProvider,
    file: &Path,
    symbol: Option<&str>,
    include_implementations: bool,
    budget: &LspContextBudget,
    sid: &str,
    generation: Option<u64>,
    freshness: LspEvidenceFreshness,
    items: &mut Vec<LspContextItem>,
    notes: &mut Vec<String>,
) {
    // 1. Document symbols for the file (public/exported items).
    match provider.document_symbols(file).await {
        Ok(symbols) => {
            let prov = make_provenance(sid, generation, "textDocument/documentSymbol", freshness);
            let filtered: Vec<_> = if let Some(sym_filter) = symbol {
                symbols
                    .into_iter()
                    .filter(|(name, _, _)| name.contains(sym_filter))
                    .collect()
            } else {
                symbols
            };
            for (name, kind, range_text) in filtered.iter().take(budget.max_symbols) {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::LspContext),
                    kind: LspContextItemKind::WorkspaceSymbol,
                    file: file.to_path_buf(),
                    line: l,
                    column: c,
                    message: format!("{name} ({kind})"),
                    symbol: Some(name.clone()),
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: 10,
                        is_hunk_local: false,
                        is_error: false,
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            debug!("interface boundary: document_symbols failed: {e}");
            let prov = make_provenance(sid, generation, "textDocument/documentSymbol", freshness);
            items.push(operational_note(
                format!("document symbols unavailable: {e}"),
                prov,
            ));
        }
    }

    // 2. Diagnostics for the file.
    match provider.diagnostics_for_file(file).await {
        Ok(diagnostics) => {
            let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
            for (severity, message, range_text) in diagnostics.iter().take(budget.max_diagnostics) {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::Diagnostics),
                    kind: LspContextItemKind::Diagnostic,
                    file: file.to_path_buf(),
                    line: l,
                    column: c,
                    message: message.clone(),
                    symbol: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: if severity_is_error(severity) { 10 } else { 5 },
                        is_hunk_local: false,
                        is_error: severity_is_error(severity),
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            debug!("interface boundary: diagnostics failed: {e}");
        }
    }

    // 3. For each boundary symbol, get definition + implementations.
    let symbol_items: Vec<(String, u32, u32)> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::WorkspaceSymbol)
        .filter_map(|i| Some((i.symbol.clone()?, i.line?, i.column.unwrap_or(0))))
        .collect();

    for (sym_name, line, column) in symbol_items.iter().take(10) {
        // Definition.
        if let Ok(defs) = provider.go_to_definition(file, *line, *column).await {
            let prov = make_provenance(sid, generation, "textDocument/definition", freshness);
            for (def_file, range_text) in &defs {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::LspContext),
                    kind: LspContextItemKind::Definition,
                    file: PathBuf::from(def_file),
                    line: l,
                    column: c,
                    message: format!("definition: {range_text}"),
                    symbol: Some(sym_name.clone()),
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: 12,
                        is_hunk_local: false,
                        is_error: false,
                        is_same_file: def_file == file.to_str().unwrap_or(""),
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }

        // Implementations (if enabled).
        if include_implementations {
            match provider.implementations(file, *line, *column).await {
                Ok(impls) => {
                    let prov =
                        make_provenance(sid, generation, "textDocument/implementation", freshness);
                    for (impl_file, range_text) in &impls {
                        let (l, c) = parse_range_start(range_text);
                        items.push(LspContextItem {
                            range: None,
                            source: Some(AgentContextSource::LspContext),
                            kind: LspContextItemKind::Implementation,
                            file: PathBuf::from(impl_file),
                            line: l,
                            column: c,
                            message: format!("implementation: {range_text}"),
                            symbol: Some(sym_name.clone()),
                            provenance: prov.clone(),
                            score: LspContextScore {
                                priority: 8,
                                is_hunk_local: false,
                                is_error: false,
                                is_same_file: impl_file == file.to_str().unwrap_or(""),
                                freshness_rank: freshness_rank(freshness),
                            },
                            payload: None,
                        });
                    }
                }
                Err(e) => {
                    debug!("interface boundary: implementations failed for {sym_name}: {e}");
                    let prov =
                        make_provenance(sid, generation, "textDocument/implementation", freshness);
                    notes.push(format!("implementations unavailable for {sym_name}: {e}"));
                    items.push(operational_note(
                        format!("implementations unavailable for {sym_name}: {e}"),
                        prov,
                    ));
                }
            }
        }
    }

    // 4. Hover for boundary symbols.
    for (sym_name, line, column) in symbol_items.iter().take(5) {
        if let Ok(Some(hover)) = provider.hover(file, *line, *column).await {
            let prov = make_provenance(sid, generation, "textDocument/hover", freshness);
            items.push(LspContextItem {
                range: None,
                source: Some(AgentContextSource::LspContext),
                kind: LspContextItemKind::Hover,
                file: file.to_path_buf(),
                line: Some(*line),
                column: Some(*column),
                message: hover,
                symbol: Some(sym_name.clone()),
                provenance: prov,
                score: LspContextScore {
                    priority: 6,
                    is_hunk_local: false,
                    is_error: false,
                    is_same_file: true,
                    freshness_rank: freshness_rank(freshness),
                },
                payload: None,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Cross-file repair context
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn collect_cross_file_repair(
    provider: &dyn LspEvidenceProvider,
    primary_file: &Path,
    related_files: &[PathBuf],
    ranges: &[crate::context::LineRange],
    budget: &LspContextBudget,
    sid: &str,
    generation: Option<u64>,
    freshness: LspEvidenceFreshness,
    items: &mut Vec<LspContextItem>,
    _notes: &mut Vec<String>,
) {
    // 1. Diagnostics in primary file.
    match provider.diagnostics_for_file(primary_file).await {
        Ok(diagnostics) => {
            let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
            let capped: Vec<_> = diagnostics.iter().take(budget.max_diagnostics).collect();
            for (severity, message, range_text) in &capped {
                let (l, c) = parse_range_start(range_text);
                // Filter by ranges if provided.
                if !ranges.is_empty() {
                    let in_range = l.is_some_and(|line| {
                        ranges.iter().any(|r| line_in_range(line, r.start, r.end))
                    });
                    if !in_range {
                        continue;
                    }
                }
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::Diagnostics),
                    kind: LspContextItemKind::Diagnostic,
                    file: primary_file.to_path_buf(),
                    line: l,
                    column: c,
                    message: message.clone(),
                    symbol: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: if severity_is_error(severity) { 12 } else { 7 },
                        is_hunk_local: false,
                        is_error: severity_is_error(severity),
                        is_same_file: true,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
        Err(e) => {
            debug!("cross-file repair: primary diagnostics failed: {e}");
            let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
            items.push(operational_note(
                format!("primary diagnostics unavailable: {e}"),
                prov,
            ));
        }
    }

    // 2. Symbols in primary file ranges.
    if !ranges.is_empty() {
        match provider.document_symbols(primary_file).await {
            Ok(symbols) => {
                let prov =
                    make_provenance(sid, generation, "textDocument/documentSymbol", freshness);
                for (name, kind, range_text) in symbols.iter().take(budget.max_symbols) {
                    let (l, _c) = parse_range_start(range_text);
                    if let Some(line) = l {
                        let in_range = ranges.iter().any(|r| line_in_range(line, r.start, r.end));
                        if in_range {
                            let (sl, sc) = parse_range_start(range_text);
                            items.push(LspContextItem {
                                range: None,
                                source: Some(AgentContextSource::LspContext),
                                kind: LspContextItemKind::WorkspaceSymbol,
                                file: primary_file.to_path_buf(),
                                line: sl,
                                column: sc,
                                message: format!("{name} ({kind})"),
                                symbol: Some(name.clone()),
                                provenance: prov.clone(),
                                score: LspContextScore {
                                    priority: 10,
                                    is_hunk_local: true,
                                    is_error: false,
                                    is_same_file: true,
                                    freshness_rank: freshness_rank(freshness),
                                },
                                payload: None,
                            });
                        }
                    }
                }
            }
            Err(e) => {
                debug!("cross-file repair: primary symbols failed: {e}");
            }
        }
    }

    // 3. Diagnostics in related files (capped).
    let related_cap = budget.max_files.min(related_files.len());
    for rf in related_files.iter().take(related_cap) {
        if let Ok(diagnostics) = provider.diagnostics_for_file(rf).await {
            let prov = make_provenance(sid, generation, "textDocument/diagnostic", freshness);
            for (severity, message, range_text) in diagnostics.iter().take(budget.max_diagnostics) {
                let (l, c) = parse_range_start(range_text);
                items.push(LspContextItem {
                    range: None,
                    source: Some(AgentContextSource::Diagnostics),
                    kind: LspContextItemKind::Diagnostic,
                    file: rf.clone(),
                    line: l,
                    column: c,
                    message: message.clone(),
                    symbol: None,
                    provenance: prov.clone(),
                    score: LspContextScore {
                        priority: if severity_is_error(severity) { 8 } else { 4 },
                        is_hunk_local: false,
                        is_error: severity_is_error(severity),
                        is_same_file: false,
                        freshness_rank: freshness_rank(freshness),
                    },
                    payload: None,
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Call neighborhood context
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn collect_call_neighborhood(
    provider: &dyn LspEvidenceProvider,
    file: &Path,
    line: u32,
    column: u32,
    direction: crate::context::HierarchyDirection,
    max_depth: u8,
    max_callers: usize,
    max_callees: usize,
    _budget: &LspContextBudget,
    sid: &str,
    generation: Option<u64>,
    freshness: LspEvidenceFreshness,
    items: &mut Vec<LspContextItem>,
    notes: &mut Vec<String>,
) {
    // Depth guard: default max is 1, explicit >1 must be requested.
    let effective_depth = max_depth.min(3);
    if effective_depth == 0 {
        notes.push("call neighborhood: depth 0, no hierarchy collected".to_string());
        return;
    }

    // Collect outgoing callees via references at the symbol position.
    if matches!(
        direction,
        crate::context::HierarchyDirection::Outgoing | crate::context::HierarchyDirection::Both
    ) {
        match provider.find_references(file, line, column).await {
            Ok(refs) => {
                let prov = make_provenance(sid, generation, "call_hierarchy/outgoing", freshness);
                let capped: Vec<_> = refs.into_iter().take(max_callees).collect();
                if capped.len() < max_callees {
                    // This is fine, we just have fewer callees.
                }
                for (ref_file, range_text) in &capped {
                    let (l, c) = parse_range_start(range_text);
                    items.push(LspContextItem {
                        range: None,
                        source: Some(AgentContextSource::LspContext),
                        kind: LspContextItemKind::Reference,
                        file: PathBuf::from(ref_file),
                        line: l,
                        column: c,
                        message: format!("callees (outgoing): {range_text}"),
                        symbol: None,
                        provenance: prov.clone(),
                        score: LspContextScore {
                            priority: 7,
                            is_hunk_local: false,
                            is_error: false,
                            is_same_file: ref_file == file.to_str().unwrap_or(""),
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
            }
            Err(e) => {
                debug!("call neighborhood: outgoing references failed: {e}");
                let prov = make_provenance(sid, generation, "call_hierarchy/outgoing", freshness);
                notes.push(format!("outgoing calls unavailable: {e}"));
                items.push(operational_note(
                    format!("outgoing calls unavailable: {e}"),
                    prov,
                ));
            }
        }
    }

    // Collect incoming callers via document highlights (read/write).
    if matches!(
        direction,
        crate::context::HierarchyDirection::Incoming | crate::context::HierarchyDirection::Both
    ) {
        match provider.document_highlights(file, line, column).await {
            Ok(highlights) => {
                let prov = make_provenance(sid, generation, "call_hierarchy/incoming", freshness);
                let capped: Vec<_> = highlights.into_iter().take(max_callers).collect();
                for range_text in &capped {
                    let (l, c) = parse_range_start(range_text);
                    items.push(LspContextItem {
                        range: None,
                        source: Some(AgentContextSource::LspContext),
                        kind: LspContextItemKind::DocumentHighlight,
                        file: file.to_path_buf(),
                        line: l,
                        column: c,
                        message: format!("caller highlight: {range_text}"),
                        symbol: None,
                        provenance: prov.clone(),
                        score: LspContextScore {
                            priority: 7,
                            is_hunk_local: false,
                            is_error: false,
                            is_same_file: true,
                            freshness_rank: freshness_rank(freshness),
                        },
                        payload: None,
                    });
                }
            }
            Err(e) => {
                debug!("call neighborhood: incoming highlights failed: {e}");
                let prov = make_provenance(sid, generation, "call_hierarchy/incoming", freshness);
                notes.push(format!("incoming calls unavailable: {e}"));
                items.push(operational_note(
                    format!("incoming calls unavailable: {e}"),
                    prov,
                ));
            }
        }
    }

    // Note about depth limitation.
    if effective_depth > 1 {
        notes.push(format!(
            "call neighborhood: depth {effective_depth} requested, but recursive expansion is capped for safety"
        ));
    }
}

// ---------------------------------------------------------------------------
// Cached collection
// ---------------------------------------------------------------------------

/// Collect context with optional caching.
///
/// On cache hit, returns the cached packet with adjusted freshness notes.
/// On cache miss, calls `collect_context` and inserts the result if eligible.
///
/// The `file_hashes` parameter provides current file content hashes for
/// cache key construction and freshness validation.
///
/// Returns `(packet, cache_hit)` where `cache_hit` indicates whether the
/// result came from cache.
pub async fn collect_context_cached(
    provider: &dyn LspEvidenceProvider,
    request: &LspContextRequest,
    budget: &LspContextBudget,
    mode: &LspContextMode,
    workspace_root: &std::path::Path,
    file_hashes: &BTreeMap<PathBuf, String>,
    cache: Option<&mut crate::cache::LspSemanticCache>,
    capability_fingerprint: Option<&str>,
) -> Result<(LspContextPacket, bool), LspContextError> {
    use crate::cache::LspCacheKeyBuilder;

    // If cache is disabled or not provided, fall through to direct collection.
    let Some(cache) = cache else {
        let packet = collect_context(provider, request, budget, mode).await?;
        return Ok((packet, false));
    };

    if !cache.is_enabled() {
        let packet = collect_context(provider, request, budget, mode).await?;
        return Ok((packet, false));
    }

    // Build cache key.
    let (server_id, server_generation) = provider.server_info().await;
    let server_id_str = server_id.as_deref().unwrap_or("unknown");

    let operation = match request {
        LspContextRequest::File { .. } => "file",
        LspContextRequest::Hunk { .. } => "hunk",
        LspContextRequest::Symbol { .. } => "symbol",
        LspContextRequest::Review { .. } => "review",
        LspContextRequest::ImpactAnalysis { .. } => "impact_analysis",
        LspContextRequest::TestFailureRepair { .. } => "test_failure_repair",
        LspContextRequest::InterfaceBoundary { .. } => "interface_boundary",
        LspContextRequest::CrossFileRepair { .. } => "cross_file_repair",
        LspContextRequest::CallNeighborhood { .. } => "call_neighborhood",
    };

    let mut builder = LspCacheKeyBuilder::new(
        workspace_root.to_path_buf(),
        server_id_str.to_string(),
        operation.to_string(),
    )
    .with_request(request)
    .with_budget(budget);
    if let Some(fp) = capability_fingerprint {
        builder = builder.with_capability_fingerprint(fp.to_string());
    }
    let key = builder;

    // Add file hashes to key
    let mut key = key;
    for (path, hash) in file_hashes {
        key = key.with_file_hash(path.clone(), hash.clone());
    }
    let key = key.build();

    // Cache lookup
    if let Some(packet) = cache.get(&key, server_generation, file_hashes) {
        let mut packet = packet.clone();
        // Add cache hit note
        packet
            .notes
            .push("[cache-hit] Evidence served from semantic cache".to_string());
        tracing::debug!("LSP semantic cache hit for operation={}", operation);
        return Ok((packet, true));
    }

    // Cache miss: collect fresh evidence
    tracing::debug!("LSP semantic cache miss for operation={}", operation);
    let packet = collect_context(provider, request, budget, mode).await?;

    // Determine original freshness for cache entry
    let original_freshness = if packet.items.is_empty() {
        LspEvidenceFreshness::Unknown
    } else {
        // Use the most common freshness from items, or Fresh if all fresh
        let fresh_count = packet
            .items
            .iter()
            .filter(|i| i.provenance.freshness == LspEvidenceFreshness::Fresh)
            .count();
        if fresh_count == packet.items.len() {
            LspEvidenceFreshness::Fresh
        } else if fresh_count > 0 {
            LspEvidenceFreshness::PossiblyStale
        } else {
            packet
                .items
                .first()
                .map(|i| i.provenance.freshness)
                .unwrap_or(LspEvidenceFreshness::Unknown)
        }
    };

    // Insert into cache (cache handles eviction)
    cache.insert(key, packet.clone(), original_freshness, server_generation);

    Ok((packet, false))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::useless_format,
    clippy::unnecessary_cast,
    clippy::let_unit_value,
    clippy::field_reassign_with_default
)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockProvider {
        diagnostics: Mutex<Vec<(String, String, String)>>,
        symbols: Mutex<Vec<(String, String, String)>>,
        defs: Mutex<Vec<(String, String)>>,
        refs: Mutex<Vec<(String, String)>>,
        impls: Mutex<Vec<(String, String)>>,
        hover_text: Mutex<Option<String>>,
        highlights: Mutex<Vec<String>>,
        signatures: Mutex<Vec<(String, String)>>,
        completions: Mutex<Vec<(String, String, String)>>,
        sem_tokens: Mutex<Vec<(u32, u32, u32, String)>>,
        ws_symbols: Mutex<Vec<(String, String, String, String)>>,
        state: Mutex<String>,
        server_id: Mutex<Option<String>>,
        generation: Mutex<Option<u64>>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                diagnostics: Mutex::new(Vec::new()),
                symbols: Mutex::new(Vec::new()),
                defs: Mutex::new(Vec::new()),
                refs: Mutex::new(Vec::new()),
                impls: Mutex::new(Vec::new()),
                hover_text: Mutex::new(None),
                highlights: Mutex::new(Vec::new()),
                signatures: Mutex::new(Vec::new()),
                completions: Mutex::new(Vec::new()),
                sem_tokens: Mutex::new(Vec::new()),
                ws_symbols: Mutex::new(Vec::new()),
                state: Mutex::new("ready".to_string()),
                server_id: Mutex::new(Some("test-server".to_string())),
                generation: Mutex::new(Some(1)),
            }
        }
    }

    #[async_trait]
    impl LspEvidenceProvider for MockProvider {
        async fn diagnostics_for_file(
            &self,
            _file: &Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(self.diagnostics.lock().unwrap().clone())
        }

        async fn document_symbols(
            &self,
            _file: &Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(self.symbols.lock().unwrap().clone())
        }

        async fn go_to_definition(
            &self,
            _file: &Path,
            _line: u32,
            _column: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(self.defs.lock().unwrap().clone())
        }

        async fn find_references(
            &self,
            _file: &Path,
            _line: u32,
            _column: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(self.refs.lock().unwrap().clone())
        }

        async fn implementations(
            &self,
            _file: &Path,
            _line: u32,
            _column: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(self.impls.lock().unwrap().clone())
        }

        async fn hover(
            &self,
            _file: &Path,
            _line: u32,
            _column: u32,
        ) -> Result<Option<String>, LspError> {
            Ok(self.hover_text.lock().unwrap().clone())
        }

        async fn document_highlights(
            &self,
            _file: &Path,
            _line: u32,
            _column: u32,
        ) -> Result<Vec<String>, LspError> {
            Ok(self.highlights.lock().unwrap().clone())
        }

        async fn signature_help(
            &self,
            _file: &Path,
            _line: u32,
            _column: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(self.signatures.lock().unwrap().clone())
        }

        async fn completion(
            &self,
            _file: &Path,
            _line: u32,
            _column: u32,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(self.completions.lock().unwrap().clone())
        }

        async fn semantic_tokens(
            &self,
            _file: &Path,
            _start_line: u32,
            _end_line: u32,
        ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
            Ok(self.sem_tokens.lock().unwrap().clone())
        }

        async fn workspace_symbols(
            &self,
            _query: &str,
        ) -> Result<Vec<(String, String, String, String)>, LspError> {
            Ok(self.ws_symbols.lock().unwrap().clone())
        }

        async fn operational_state(&self) -> String {
            self.state.lock().unwrap().clone()
        }

        async fn server_info(&self) -> (Option<String>, Option<u64>) {
            (
                self.server_id.lock().unwrap().clone(),
                *self.generation.lock().unwrap(),
            )
        }
    }

    #[tokio::test]
    async fn test_collector_returns_diagnostics_with_provenance() {
        let provider = MockProvider::new();
        *provider.diagnostics.lock().unwrap() = vec![(
            "error".to_string(),
            "unused variable `x`".to_string(),
            "(3:5)-(3:10)".to_string(),
        )];

        let request = LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;

        let packet = collect_context(&provider, &request, &budget, &mode)
            .await
            .unwrap();

        assert_eq!(packet.items.len(), 1);
        let item = &packet.items[0];
        assert_eq!(item.kind, LspContextItemKind::Diagnostic);
        assert_eq!(item.message, "unused variable `x`");
        assert_eq!(item.line, Some(2)); // 0-indexed
        assert_eq!(item.column, Some(4));
        assert!(item.provenance.server_id.starts_with("test"));
        assert_eq!(item.provenance.operation, "textDocument/diagnostic");
    }

    #[tokio::test]
    async fn test_collector_returns_definition_excerpt() {
        let provider = MockProvider::new();
        *provider.defs.lock().unwrap() =
            vec![("src/lib.rs".to_string(), "(10:0)-(10:20)".to_string())];

        let request = LspContextRequest::Symbol {
            file: PathBuf::from("test.rs"),
            position: lsp_types::Position::new(5, 0),
            include_references: false,
            include_implementations: false,
            include_call_like_context: false,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;

        let packet = collect_context(&provider, &request, &budget, &mode)
            .await
            .unwrap();

        let defs: Vec<_> = packet
            .items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Definition)
            .collect();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].file, PathBuf::from("src/lib.rs"));
        assert_eq!(defs[0].line, Some(9)); // 0-indexed
    }

    #[tokio::test]
    async fn test_collector_limits_references() {
        let provider = MockProvider::new();
        *provider.refs.lock().unwrap() = (0..100)
            .map(|i| (format!("file_{i}.rs"), format!("(1:0)-(1:10)")))
            .collect();

        let request = LspContextRequest::Symbol {
            file: PathBuf::from("test.rs"),
            position: lsp_types::Position::new(5, 0),
            include_references: true,
            include_implementations: false,
            include_call_like_context: false,
        };
        let mut budget = LspContextBudget::default();
        budget.max_references = 5;
        let mode = LspContextMode::Opportunistic;

        let packet = collect_context(&provider, &request, &budget, &mode)
            .await
            .unwrap();

        let refs: Vec<_> = packet
            .items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Reference)
            .collect();
        assert!(
            refs.len() <= 5,
            "expected <= 5 references, got {}",
            refs.len()
        );
    }

    #[tokio::test]
    async fn test_collector_records_unsupported_notes() {
        // When a provider returns an error, we get an operational note.
        struct FailProvider;
        #[async_trait]
        impl LspEvidenceProvider for FailProvider {
            async fn diagnostics_for_file(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Err(LspError::NotInitialized("no server".into()))
            }
            async fn document_symbols(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn go_to_definition(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn find_references(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn implementations(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn hover(&self, _: &Path, _: u32, _: u32) -> Result<Option<String>, LspError> {
                Ok(None)
            }
            async fn document_highlights(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<String>, LspError> {
                Ok(vec![])
            }
            async fn signature_help(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn completion(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn semantic_tokens(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
                Ok(vec![])
            }
            async fn workspace_symbols(
                &self,
                _: &str,
            ) -> Result<Vec<(String, String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn operational_state(&self) -> String {
                "ready".to_string()
            }
            async fn server_info(&self) -> (Option<String>, Option<u64>) {
                (Some("fail".to_string()), Some(1))
            }
        }

        let request = LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;

        let packet = collect_context(&FailProvider, &request, &budget, &mode)
            .await
            .unwrap();

        // Should have an operational note about the failure.
        let notes: Vec<_> = packet
            .items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::OperationalNote)
            .collect();
        assert!(!notes.is_empty(), "expected operational note for failure");
    }

    #[tokio::test]
    async fn test_collector_handles_unknown_capability_fail_closed_when_required() {
        // Required mode + unavailable server → RequiredFailed error.
        struct UnavailRequiredProvider;
        #[async_trait]
        impl LspEvidenceProvider for UnavailRequiredProvider {
            async fn diagnostics_for_file(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn document_symbols(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn go_to_definition(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn find_references(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn implementations(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn hover(&self, _: &Path, _: u32, _: u32) -> Result<Option<String>, LspError> {
                Ok(None)
            }
            async fn document_highlights(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<String>, LspError> {
                Ok(vec![])
            }
            async fn signature_help(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn completion(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn semantic_tokens(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
                Ok(vec![])
            }
            async fn workspace_symbols(
                &self,
                _: &str,
            ) -> Result<Vec<(String, String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn operational_state(&self) -> String {
                "failed".to_string()
            }
            async fn server_info(&self) -> (Option<String>, Option<u64>) {
                (None, None)
            }
        }

        let request = LspContextRequest::Hunk {
            file: PathBuf::from("test.rs"),
            hunks: vec![crate::context::HunkRange {
                start: 0,
                end: 5,
                original_start: None,
                original_end: None,
            }],
            include_references: false,
            include_definitions: true,
            include_implementations: false,
            include_semantic_tokens: false,
            include_security_evidence: false,
        };
        let budget = LspContextBudget::default();

        let result = collect_context(
            &UnavailRequiredProvider,
            &request,
            &budget,
            &LspContextMode::Required,
        )
        .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            LspContextError::RequiredFailed(_) => {}
            other => panic!("expected RequiredFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_collector_does_not_execute_code_actions() {
        // Verify that code actions are never called — the provider has no
        // such method and we never invoke it.
        let provider = MockProvider::new();
        let request = LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: true,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;

        let packet = collect_context(&provider, &request, &budget, &mode)
            .await
            .unwrap();

        // No code action items.
        let code_actions: Vec<_> = packet
            .items
            .iter()
            .filter(|i| {
                i.provenance.operation.contains("codeAction")
                    || i.provenance.operation.contains("code_action")
            })
            .collect();
        assert!(code_actions.is_empty(), "should not execute code actions");
    }

    #[tokio::test]
    async fn test_collector_does_not_apply_rename_or_formatting() {
        let provider = MockProvider::new();
        let request = LspContextRequest::Review {
            changed_files: vec![PathBuf::from("test.rs")],
            hunks: vec![],
            risk_mode: LspRiskMode::Standard,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;

        let packet = collect_context(&provider, &request, &budget, &mode)
            .await
            .unwrap();

        // No rename/formatting operations.
        for item in &packet.items {
            assert!(
                !item.provenance.operation.contains("rename"),
                "should not perform rename"
            );
            assert!(
                !item.provenance.operation.contains("format"),
                "should not perform formatting"
            );
        }
    }

    #[tokio::test]
    async fn test_hunk_context_prefers_changed_range_diagnostics() {
        let provider = MockProvider::new();
        *provider.diagnostics.lock().unwrap() = vec![
            (
                "error".to_string(),
                "in hunk".to_string(),
                "(3:0)-(3:10)".to_string(),
            ),
            (
                "warning".to_string(),
                "outside hunk".to_string(),
                "(20:0)-(20:10)".to_string(),
            ),
        ];

        let hunks = vec![crate::context::HunkRange {
            start: 0,
            end: 5,
            original_start: None,
            original_end: None,
        }];

        let items = collect_hunk_context(
            &provider,
            PathBuf::from("test.rs").as_path(),
            &hunks,
            false,
            false,
            false,
            false,
            &LspContextBudget::default(),
        )
        .await
        .unwrap();

        // Only the in-hunk diagnostic should be present.
        let diagnostics: Vec<_> = items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Diagnostic)
            .collect();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "in hunk");
        assert!(diagnostics[0].score.is_hunk_local);
    }

    #[tokio::test]
    async fn test_hunk_context_includes_enclosing_symbol() {
        let provider = MockProvider::new();
        *provider.defs.lock().unwrap() = vec![("test.rs".to_string(), "(2:0)-(2:30)".to_string())];

        let hunks = vec![crate::context::HunkRange {
            start: 3,
            end: 6,
            original_start: None,
            original_end: None,
        }];

        let items = collect_hunk_context(
            &provider,
            PathBuf::from("test.rs").as_path(),
            &hunks,
            false,
            true,
            false,
            false,
            &LspContextBudget::default(),
        )
        .await
        .unwrap();

        let defs: Vec<_> = items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Definition)
            .collect();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].file, PathBuf::from("test.rs"));
    }

    #[tokio::test]
    async fn test_hunk_context_caps_references() {
        let provider = MockProvider::new();
        *provider.refs.lock().unwrap() = (0..50)
            .map(|i| (format!("file_{i}.rs"), format!("(1:0)-(1:10)")))
            .collect();

        let hunks = vec![crate::context::HunkRange {
            start: 0,
            end: 5,
            original_start: None,
            original_end: None,
        }];

        let mut budget = LspContextBudget::default();
        budget.max_references = 5;

        let items = collect_hunk_context(
            &provider,
            PathBuf::from("test.rs").as_path(),
            &hunks,
            true,
            false,
            false,
            false,
            &budget,
        )
        .await
        .unwrap();

        let refs: Vec<_> = items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Reference)
            .collect();
        assert!(refs.len() <= 5, "expected <= 5 refs, got {}", refs.len());
    }

    #[tokio::test]
    async fn test_hunk_context_degrades_without_lsp() {
        // Opportunistic mode should return empty items when server is unusable.
        struct UnavailProvider;
        #[async_trait]
        impl LspEvidenceProvider for UnavailProvider {
            async fn diagnostics_for_file(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn document_symbols(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn go_to_definition(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn find_references(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn implementations(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn hover(&self, _: &Path, _: u32, _: u32) -> Result<Option<String>, LspError> {
                Ok(None)
            }
            async fn document_highlights(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<String>, LspError> {
                Ok(vec![])
            }
            async fn signature_help(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn completion(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn semantic_tokens(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
                Ok(vec![])
            }
            async fn workspace_symbols(
                &self,
                _: &str,
            ) -> Result<Vec<(String, String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn operational_state(&self) -> String {
                "failed".to_string()
            }
            async fn server_info(&self) -> (Option<String>, Option<u64>) {
                (None, None)
            }
        }

        let request = LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;

        let packet = collect_context(&UnavailProvider, &request, &budget, &mode)
            .await
            .unwrap();

        // Should return a degraded note, not real diagnostics.
        let notes: Vec<_> = packet
            .items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::OperationalNote)
            .collect();
        assert!(!notes.is_empty(), "expected degraded note");
    }

    #[tokio::test]
    async fn test_required_mode_fails_when_server_unavailable() {
        struct UnavailProvider;
        #[async_trait]
        impl LspEvidenceProvider for UnavailProvider {
            async fn diagnostics_for_file(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn document_symbols(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn go_to_definition(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn find_references(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn implementations(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn hover(&self, _: &Path, _: u32, _: u32) -> Result<Option<String>, LspError> {
                Ok(None)
            }
            async fn document_highlights(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<String>, LspError> {
                Ok(vec![])
            }
            async fn signature_help(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn completion(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn semantic_tokens(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
                Ok(vec![])
            }
            async fn workspace_symbols(
                &self,
                _: &str,
            ) -> Result<Vec<(String, String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn operational_state(&self) -> String {
                "failed".to_string()
            }
            async fn server_info(&self) -> (Option<String>, Option<u64>) {
                (None, None)
            }
        }

        let request = LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();

        let result = collect_context(
            &UnavailProvider,
            &request,
            &budget,
            &LspContextMode::Required,
        )
        .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            LspContextError::RequiredFailed(_) => {}
            other => panic!("expected RequiredFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_opportunistic_returns_partial_when_server_unavailable() {
        struct PartialProvider;
        #[async_trait]
        impl LspEvidenceProvider for PartialProvider {
            async fn diagnostics_for_file(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![(
                    "error".to_string(),
                    "diag".to_string(),
                    "(1:0)-(1:5)".to_string(),
                )])
            }
            async fn document_symbols(
                &self,
                _: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Err(LspError::NotInitialized("not ready".into()))
            }
            async fn go_to_definition(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Err(LspError::NotInitialized("not ready".into()))
            }
            async fn find_references(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn implementations(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn hover(&self, _: &Path, _: u32, _: u32) -> Result<Option<String>, LspError> {
                Ok(None)
            }
            async fn document_highlights(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<String>, LspError> {
                Ok(vec![])
            }
            async fn signature_help(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Ok(vec![])
            }
            async fn completion(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn semantic_tokens(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
                Ok(vec![])
            }
            async fn workspace_symbols(
                &self,
                _: &str,
            ) -> Result<Vec<(String, String, String, String)>, LspError> {
                Ok(vec![])
            }
            async fn operational_state(&self) -> String {
                "ready".to_string()
            }
            async fn server_info(&self) -> (Option<String>, Option<u64>) {
                (Some("partial".to_string()), Some(1))
            }
        }

        let request = LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: true,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;

        let packet = collect_context(&PartialProvider, &request, &budget, &mode)
            .await
            .unwrap();

        // Should have diagnostic + operational notes for failed operations.
        let diags: Vec<_> = packet
            .items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Diagnostic)
            .collect();
        assert_eq!(diags.len(), 1, "diagnostic should survive");
    }

    #[tokio::test]
    async fn test_collect_context_cached_miss_then_hit() {
        let provider = MockProvider::new();
        *provider.diagnostics.lock().unwrap() = vec![(
            "error".to_string(),
            "test diagnostic".to_string(),
            "(1:0)-(1:10)".to_string(),
        )];

        let request = LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;
        let root = PathBuf::from("/tmp/test");
        let file_hashes = BTreeMap::new();

        let mut cache = crate::cache::LspSemanticCache::new(crate::cache::LspCacheConfig {
            mode: crate::cache::LspCacheMode::Memory,
            ..Default::default()
        });

        // First call: cache miss.
        let (packet1, hit1) = collect_context_cached(
            &provider,
            &request,
            &budget,
            &mode,
            &root,
            &file_hashes,
            Some(&mut cache),
            None,
        )
        .await
        .unwrap();
        assert!(!hit1, "first call should be a miss");
        assert_eq!(packet1.items.len(), 1);
        assert!(!packet1.notes.iter().any(|n| n.contains("cache-hit")));

        // Second call: cache hit.
        let (packet2, hit2) = collect_context_cached(
            &provider,
            &request,
            &budget,
            &mode,
            &root,
            &file_hashes,
            Some(&mut cache),
            None,
        )
        .await
        .unwrap();
        assert!(hit2, "second call should be a hit");
        assert_eq!(packet2.items.len(), 1);
        assert!(packet2.notes.iter().any(|n| n.contains("cache-hit")));
    }

    #[tokio::test]
    async fn test_collect_context_cached_disabled_passthrough() {
        let provider = MockProvider::new();
        *provider.diagnostics.lock().unwrap() = vec![(
            "warning".to_string(),
            "unused import".to_string(),
            "(2:0)-(2:15)".to_string(),
        )];

        let request = LspContextRequest::File {
            file: PathBuf::from("lib.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;
        let root = PathBuf::from("/tmp/test");
        let file_hashes = BTreeMap::new();

        let mut cache = crate::cache::LspSemanticCache::new(crate::cache::LspCacheConfig::default());

        // Disabled cache: always a miss, no caching.
        let (_, hit1) = collect_context_cached(
            &provider,
            &request,
            &budget,
            &mode,
            &root,
            &file_hashes,
            Some(&mut cache),
            None,
        )
        .await
        .unwrap();
        assert!(!hit1);

        let (_, hit2) = collect_context_cached(
            &provider,
            &request,
            &budget,
            &mode,
            &root,
            &file_hashes,
            Some(&mut cache),
            None,
        )
        .await
        .unwrap();
        assert!(!hit2, "disabled cache should never hit");
    }

    #[tokio::test]
    async fn test_collect_context_cached_no_cache_passthrough() {
        let provider = MockProvider::new();
        let request = LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: false,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;
        let root = PathBuf::from("/tmp/test");
        let file_hashes = BTreeMap::new();

        // None cache: always a miss.
        let (_, hit) = collect_context_cached(
            &provider,
            &request,
            &budget,
            &mode,
            &root,
            &file_hashes,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(!hit);
    }

    #[tokio::test]
    async fn test_collect_context_cached_file_hash_change_invalidates() {
        let provider = MockProvider::new();
        *provider.diagnostics.lock().unwrap() = vec![(
            "error".to_string(),
            "test".to_string(),
            "(1:0)-(1:5)".to_string(),
        )];

        let request = LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let mode = LspContextMode::Opportunistic;
        let root = PathBuf::from("/tmp/test");

        let mut file_hashes = BTreeMap::new();
        file_hashes.insert(PathBuf::from("test.rs"), "hash_v1".to_string());

        let mut cache = crate::cache::LspSemanticCache::new(crate::cache::LspCacheConfig {
            mode: crate::cache::LspCacheMode::Memory,
            ..Default::default()
        });

        // First call with hash_v1.
        let (_, hit1) = collect_context_cached(
            &provider,
            &request,
            &budget,
            &mode,
            &root,
            &file_hashes,
            Some(&mut cache),
            None,
        )
        .await
        .unwrap();
        assert!(!hit1);

        // Change file hash.
        file_hashes.insert(PathBuf::from("test.rs"), "hash_v2".to_string());

        // Second call with hash_v2: should miss (different key).
        let (_, hit2) = collect_context_cached(
            &provider,
            &request,
            &budget,
            &mode,
            &root,
            &file_hashes,
            Some(&mut cache),
            None,
        )
        .await
        .unwrap();
        assert!(!hit2, "changed file hash should produce a miss");
    }
}
