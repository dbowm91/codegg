//! Unified LSP context packet types, budgeting, dedup/ranking, and
//! the evidence collector API.
//!
//! This module provides a single abstraction for gathering structured
//! LSP evidence (diagnostics, definitions, references, symbols,
//! semantic tokens, etc.) into an [`LspContextPacket`] that callers
//! can budget, dedup, rank, and serialize.
//!
//! # Design
//!
//! - [`LspContextRequest`] describes *what* evidence to collect.
//! - [`LspContextItem`] is a single piece of evidence with provenance
//!   and scoring metadata.
//! - [`LspContextPacket`] is the assembled collection ready for
//!   budget enforcement and truncation.
//! - [`enforce_context_budget`] deterministically truncates items that
//!   exceed configurable limits.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use lsp_types::Position;
use serde::{Deserialize, Serialize};

use crate::hunk_context::HunkDescriptor;

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

/// Configurable limits for context packet assembly.
///
/// Defaults are conservative and tuned for agent-facing prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspContextBudget {
    /// Maximum number of distinct files to include.
    pub max_files: usize,
    /// Maximum number of line ranges (items) per file.
    pub max_ranges_per_file: usize,
    /// Maximum diagnostics across all files.
    pub max_diagnostics: usize,
    /// Maximum reference items across all files.
    pub max_references: usize,
    /// Maximum symbol items across all files.
    pub max_symbols: usize,
    /// Maximum completion summary items.
    pub max_completion_items: usize,
    /// Maximum semantic tokens (individual tokens, not summaries).
    pub max_semantic_tokens: usize,
    /// Total byte budget for the serialized packet.
    pub max_bytes: usize,
}

impl Default for LspContextBudget {
    fn default() -> Self {
        Self {
            max_files: 10,
            max_ranges_per_file: 5,
            max_diagnostics: 20,
            max_references: 30,
            max_symbols: 30,
            max_completion_items: 10,
            max_semantic_tokens: 200,
            max_bytes: 32_768,
        }
    }
}

/// Returns conservative default budget values.
pub fn default_budget() -> LspContextBudget {
    LspContextBudget::default()
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// A 0-indexed line range within a file. `start` is inclusive, `end` is
/// exclusive (Rust half-open convention).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    /// Number of lines in this range.
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Returns `true` if the range contains no lines.
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// A hunk range with optional original (old-file) side.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HunkRange {
    pub start: u32,
    pub end: u32,
    pub original_start: Option<u32>,
    pub original_end: Option<u32>,
}

/// Risk classification mode for review-context requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LspRiskMode {
    /// Minimal evidence gathering; only diagnostics and definitions.
    Conservative,
    /// Standard evidence with references and call hierarchy.
    Standard,
    /// Aggressive evidence including type hierarchy and completions.
    Aggressive,
}

impl Default for LspRiskMode {
    fn default() -> Self {
        Self::Standard
    }
}

/// Describes what evidence to collect.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum LspContextRequest {
    /// Collect evidence for a specific file and line ranges.
    File {
        file: PathBuf,
        line_ranges: Vec<LineRange>,
        include_symbols: bool,
        include_diagnostics: bool,
    },
    /// Collect hunk-aware evidence (definitions, references, diagnostics
    /// near changed lines).
    Hunk {
        file: PathBuf,
        hunks: Vec<HunkRange>,
        include_references: bool,
        include_definitions: bool,
        include_implementations: bool,
        include_semantic_tokens: bool,
        include_security_evidence: bool,
    },
    /// Collect symbol-centric evidence (references, implementations,
    /// call hierarchy).
    Symbol {
        file: PathBuf,
        position: Position,
        include_references: bool,
        include_implementations: bool,
        include_call_like_context: bool,
    },
    /// Collect review-mode evidence across multiple changed files.
    Review {
        changed_files: Vec<PathBuf>,
        hunks: Vec<HunkDescriptor>,
        risk_mode: LspRiskMode,
    },
}

// ---------------------------------------------------------------------------
// Evidence freshness
// ---------------------------------------------------------------------------

/// Freshness of a single context evidence item.
///
/// Mirrors [`crate::diagnostics::LspDiagnosticFreshness`] but
/// generalized for non-diagnostic evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LspEvidenceFreshness {
    /// Evidence reflects the latest file content.
    Fresh,
    /// File content changed since evidence was collected; may be stale.
    PossiblyStale,
    /// Server restarted or workspace root changed; evidence is stale.
    Stale,
    /// Evidence retained from a previous server generation after restart.
    RetainedAfterRestart,
    /// File was edited after this evidence was collected.
    StaleAfterEdit,
    /// Server generation does not match the expected generation.
    ServerGenerationMismatch,
    /// Freshness unknown.
    Unknown,
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// Describes where a context item came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspEvidenceProvenance {
    /// Server that produced this evidence (e.g. "rust-analyzer").
    pub server_id: String,
    /// Server generation at time of collection.
    pub server_generation: Option<u64>,
    /// LSP operation that produced the evidence (e.g. "textDocument/definition").
    pub operation: String,
    /// Freshness classification.
    pub freshness: LspEvidenceFreshness,
    /// Capability decision at the time of collection (e.g. "supported",
    /// "unsupported", "unknown").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_decision: Option<String>,
    /// Document version or file hash at time of collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_version: Option<String>,
    /// Age in milliseconds since evidence was received from the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_ms: Option<u64>,
    /// Whether this evidence was produced by a post-restart server.
    pub post_restart: bool,
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// Context-item scoring metadata used for ranking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspContextScore {
    /// Base priority (higher = more important).
    pub priority: u32,
    /// Whether this item is adjacent to a changed hunk.
    pub is_hunk_local: bool,
    /// Whether this item represents an error-level diagnostic.
    pub is_error: bool,
    /// Whether the item's file matches the primary file of the request.
    pub is_same_file: bool,
    /// Freshness rank (0 = freshest, higher = less fresh).
    pub freshness_rank: u32,
}

impl LspContextScore {
    /// Compute a weighted score. Higher is more relevant.
    pub fn score(&self) -> i64 {
        let base = self.priority as i64;
        let hunk_bonus: i64 = if self.is_hunk_local { 50 } else { 0 };
        let error_bonus: i64 = if self.is_error { 30 } else { 0 };
        let same_file_bonus: i64 = if self.is_same_file { 20 } else { 0 };
        let freshness_penalty = -(self.freshness_rank as i64) * 5;
        base + hunk_bonus + error_bonus + same_file_bonus + freshness_penalty
    }
}

// ---------------------------------------------------------------------------
// Item kinds
// ---------------------------------------------------------------------------

/// The type of evidence an [`LspContextItem`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LspContextItemKind {
    Diagnostic,
    Definition,
    Declaration,
    Reference,
    Implementation,
    DocumentHighlight,
    Hover,
    SignatureHelp,
    CompletionSummary,
    SemanticTokenSummary,
    WorkspaceSymbol,
    OperationalNote,
}

// ---------------------------------------------------------------------------
// Context item
// ---------------------------------------------------------------------------

/// A single piece of LSP evidence within a context packet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspContextItem {
    /// What kind of evidence this is.
    pub kind: LspContextItemKind,
    /// File this evidence pertains to.
    pub file: PathBuf,
    /// Optional line range (start inclusive, end exclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<LineRange>,
    /// Optional line number (0-indexed). Kept for backward compatibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Optional column number (0-indexed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Human-readable description of the evidence.
    pub message: String,
    /// Optional symbol name associated with this item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Where this evidence originated in the agent workflow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<AgentContextSource>,
    /// Provenance metadata.
    pub provenance: LspEvidenceProvenance,
    /// Scoring metadata for ranking.
    pub score: LspContextScore,
    /// Optional structured payload (JSON) for complex evidence types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl LspContextItem {
    /// Compute a dedup key: kind + file + range + symbol + message hash.
    pub fn dedup_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.kind.hash(&mut hasher);
        self.file.hash(&mut hasher);
        self.line.hash(&mut hasher);
        self.column.hash(&mut hasher);
        if let Some(ref r) = self.range {
            r.start.hash(&mut hasher);
            r.end.hash(&mut hasher);
        }
        self.symbol.hash(&mut hasher);
        self.message.hash(&mut hasher);
        hasher.finish()
    }
}

// ---------------------------------------------------------------------------
// Preview artifacts
// ---------------------------------------------------------------------------

/// Preview-only mutation artifacts (rename, formatting, code action).
/// These are never written to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LspPreviewArtifact {
    Rename {
        /// Human-readable description of the rename.
        description: String,
        /// Number of file edits in the preview.
        edit_count: usize,
    },
    Formatting {
        /// Human-readable description of the formatting change.
        description: String,
        /// SHA-256 hash of the formatted content.
        content_hash: Option<String>,
    },
    CodeAction {
        /// Human-readable description of the code action.
        description: String,
        /// Action kind (e.g. "quickfix", "refactor").
        kind: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Truncation
// ---------------------------------------------------------------------------

/// Records what was truncated during budget enforcement.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LspContextTruncation {
    pub files_truncated: bool,
    pub diagnostics_truncated: bool,
    pub references_truncated: bool,
    pub symbols_truncated: bool,
    pub bytes_truncated: bool,
    pub total_bytes: usize,
    pub max_bytes: usize,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Context mode
// ---------------------------------------------------------------------------

/// Controls whether context gathering is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LspContextPacketMode {
    /// Context gathering is disabled.
    Disabled,
    /// Context is gathered opportunistically when the server is available.
    Opportunistic,
    /// Context is required; a missing server is an error.
    Required,
}

impl Default for LspContextPacketMode {
    fn default() -> Self {
        Self::Opportunistic
    }
}

/// Alias for module-level re-export clarity.
pub type LspContextMode = LspContextPacketMode;

// ---------------------------------------------------------------------------
// Agent context source
// ---------------------------------------------------------------------------

/// Identifies where a piece of evidence originated in the agent workflow.
///
/// Used by prompt assemblers and TUI summaries to label evidence provenance
/// beyond the raw LSP operation name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentContextSource {
    /// Evidence from repository search or file browsing.
    RepositorySearch,
    /// Evidence from a diff or patch.
    Diff,
    /// Evidence from hunk/source navigation.
    Hunk,
    /// Evidence from diagnostics (errors, warnings).
    Diagnostics,
    /// Evidence assembled by the LSP context pipeline.
    LspContext,
    /// Evidence from the security review workflow.
    SecurityContext,
    /// Evidence provided directly by the user.
    UserProvided,
}

impl std::fmt::Display for AgentContextSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepositorySearch => write!(f, "repository_search"),
            Self::Diff => write!(f, "diff"),
            Self::Hunk => write!(f, "hunk"),
            Self::Diagnostics => write!(f, "diagnostics"),
            Self::LspContext => write!(f, "lsp_context"),
            Self::SecurityContext => write!(f, "security_context"),
            Self::UserProvided => write!(f, "user_provided"),
        }
    }
}

// ---------------------------------------------------------------------------
// Packet
// ---------------------------------------------------------------------------

/// The assembled collection of LSP evidence ready for budget enforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspContextPacket {
    /// Request that produced this packet.
    pub request: LspContextRequest,
    /// Evidence items (may be truncated by budget enforcement).
    pub items: Vec<LspContextItem>,
    /// Preview artifacts (rename/format/code-action previews).
    pub previews: Vec<LspPreviewArtifact>,
    /// Registry IDs for preview artifacts, parallel to `previews`.
    #[serde(default)]
    pub preview_ids: Vec<String>,
    /// Mode used during collection.
    pub mode: LspContextPacketMode,
    /// Workspace root this packet was collected against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<PathBuf>,
    /// When this packet was generated (millis since epoch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<u64>,
    /// Server that produced the evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    /// Server generation at time of collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_generation: Option<u64>,
    /// Operational state summary at time of collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operational_state: Option<String>,
    /// Budget used during collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<LspContextBudget>,
    /// Operational notes (e.g. "LSP state: indexing").
    #[serde(default)]
    pub notes: Vec<String>,
    /// Truncation record populated by budget enforcement.
    #[serde(default)]
    pub truncation: LspContextTruncation,
}

// ---------------------------------------------------------------------------
// Budget enforcement
// ---------------------------------------------------------------------------

/// Deterministically enforce budget limits on a context packet.
///
/// Truncation order:
/// 1. Per-file range limits
/// 2. Category limits (diagnostics, references, symbols, completions, tokens)
/// 3. File count limit
/// 4. Total byte limit
///
/// Returns a [`LspContextTruncation`] describing what was truncated.
pub fn enforce_context_budget(packet: &mut LspContextPacket) -> LspContextTruncation {
    let budget = default_budget();
    let mut truncation = LspContextTruncation::default();

    // 1. Per-file range limits: group items by file, keep best-scoring
    //    up to max_ranges_per_file.
    {
        let mut by_file: HashMap<PathBuf, Vec<usize>> = HashMap::new();
        for (i, item) in packet.items.iter().enumerate() {
            by_file.entry(item.file.clone()).or_default().push(i);
        }
        let mut indices_to_remove = std::collections::BTreeSet::new();
        for (_file, mut indices) in by_file {
            if indices.len() > budget.max_ranges_per_file {
                indices.sort_by_key(|&i| std::cmp::Reverse(packet.items[i].score.score()));
                for &idx in &indices[budget.max_ranges_per_file..] {
                    indices_to_remove.insert(idx);
                }
            }
        }
        if !indices_to_remove.is_empty() {
            let remove_vec: Vec<usize> = indices_to_remove.into_iter().collect();
            for &idx in remove_vec.iter().rev() {
                packet.items.remove(idx);
            }
            truncation.notes.push(format!(
                "truncated per-file ranges to {}/file",
                budget.max_ranges_per_file
            ));
        }
    }

    // 2. Category limits.
    fn truncate_by_kind(
        items: &mut Vec<LspContextItem>,
        kind: LspContextItemKind,
        limit: usize,
        label: &str,
        truncation: &mut LspContextTruncation,
    ) {
        let count = items.iter().filter(|i| i.kind == kind).count();
        if count > limit {
            let mut to_remove = Vec::new();
            let mut kept = 0;
            for (i, item) in items.iter().enumerate() {
                if item.kind == kind {
                    if kept >= limit {
                        to_remove.push(i);
                    } else {
                        kept += 1;
                    }
                }
            }
            for &idx in to_remove.iter().rev() {
                items.remove(idx);
            }
            truncation
                .notes
                .push(format!("truncated {label} from {count} to {limit}"));
        }
    }

    // Sort by score descending before category truncation so best items survive.
    packet
        .items
        .sort_by_key(|i| std::cmp::Reverse(i.score.score()));

    truncate_by_kind(
        &mut packet.items,
        LspContextItemKind::Diagnostic,
        budget.max_diagnostics,
        "diagnostics",
        &mut truncation,
    );
    truncation.diagnostics_truncated = truncation.notes.iter().any(|n| n.contains("diagnostics"));

    truncate_by_kind(
        &mut packet.items,
        LspContextItemKind::Reference,
        budget.max_references,
        "references",
        &mut truncation,
    );
    truncation.references_truncated = truncation.notes.iter().any(|n| n.contains("references"));

    truncate_by_kind(
        &mut packet.items,
        LspContextItemKind::WorkspaceSymbol,
        budget.max_symbols,
        "symbols",
        &mut truncation,
    );
    truncation.symbols_truncated = truncation.notes.iter().any(|n| n.contains("symbols"));

    truncate_by_kind(
        &mut packet.items,
        LspContextItemKind::CompletionSummary,
        budget.max_completion_items,
        "completions",
        &mut truncation,
    );

    // 3. File count limit.
    {
        let unique_files: Vec<PathBuf> = packet
            .items
            .iter()
            .map(|i| i.file.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        if unique_files.len() > budget.max_files {
            // Keep items from files that appear most often (best for relevance).
            let mut file_counts: HashMap<PathBuf, usize> = HashMap::new();
            for f in &unique_files {
                let count = packet.items.iter().filter(|i| &i.file == f).count();
                *file_counts.entry(f.clone()).or_insert(0) += count;
            }
            let mut sorted_files = unique_files;
            sorted_files
                .sort_by_key(|f| std::cmp::Reverse(file_counts.get(f).copied().unwrap_or(0)));
            let keep: std::collections::HashSet<PathBuf> =
                sorted_files.into_iter().take(budget.max_files).collect();
            let before = packet.items.len();
            packet.items.retain(|i| keep.contains(&i.file));
            if packet.items.len() < before {
                truncation.files_truncated = true;
                truncation.notes.push(format!(
                    "truncated files from {} to {}",
                    before, budget.max_files
                ));
            }
        }
    }

    // 4. Total byte limit.
    {
        let json = serde_json::to_vec(&packet.items).unwrap_or_default();
        truncation.total_bytes = json.len();
        truncation.max_bytes = budget.max_bytes;
        if json.len() > budget.max_bytes {
            // Binary-search the number of items to keep.
            let mut lo = 0usize;
            let mut hi = packet.items.len();
            while lo < hi {
                let mid = lo + (hi - lo + 1) / 2;
                let trimmed: Vec<&LspContextItem> = packet.items[..mid].iter().collect();
                let size = serde_json::to_vec(&trimmed).unwrap_or_default().len();
                if size <= budget.max_bytes {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            let removed = packet.items.len() - lo;
            if removed > 0 {
                packet.items.truncate(lo);
                truncation.bytes_truncated = true;
                truncation.notes.push(format!(
                    "truncated bytes: removed {removed} items to fit {budget_bytes} byte budget",
                    budget_bytes = budget.max_bytes
                ));
            }
        }
    }

    truncation
}

// ---------------------------------------------------------------------------
// Dedup
// ---------------------------------------------------------------------------

/// Deduplicate context items by kind + file + range + symbol + message hash.
///
/// When duplicates exist, the item with the higher score survives.
pub fn dedup_context_items(items: Vec<LspContextItem>) -> Vec<LspContextItem> {
    let mut best: HashMap<u64, LspContextItem> = HashMap::new();
    for item in items {
        let key = item.dedup_key();
        match best.get(&key) {
            Some(existing) if existing.score.score() >= item.score.score() => {}
            _ => {
                best.insert(key, item);
            }
        }
    }
    best.into_values().collect()
}

// ---------------------------------------------------------------------------
// Ranking
// ---------------------------------------------------------------------------

/// Sort context items by relevance score (descending) for the given request.
///
/// Items that are local to the request's primary file or hunk lines are
/// boosted. Freshness is factored into each item's score.
pub fn rank_context_items(items: &mut [LspContextItem], request: &LspContextRequest) {
    // Determine the primary file for the request.
    let primary_file = match request {
        LspContextRequest::File { file, .. }
        | LspContextRequest::Hunk { file, .. }
        | LspContextRequest::Symbol { file, .. } => Some(file.clone()),
        LspContextRequest::Review { changed_files, .. } => changed_files.first().cloned(),
    };

    // Determine the set of hunk lines for hunk-local boosting.
    let hunk_lines: std::collections::BTreeSet<u32> = match request {
        LspContextRequest::Hunk { hunks, .. } => {
            hunks.iter().flat_map(|h| h.start..h.end).collect()
        }
        LspContextRequest::Review { hunks, .. } => hunks
            .iter()
            .filter_map(|h| h.new_range.as_ref())
            .flat_map(|r| r.start_line..=r.end_line)
            .collect(),
        _ => std::collections::BTreeSet::new(),
    };

    for item in items.iter_mut() {
        // Update same_file flag.
        if let Some(ref pf) = primary_file {
            item.score.is_same_file = item.file == *pf;
        }
        // Update hunk_local flag.
        if !hunk_lines.is_empty() {
            if let Some(line) = item.line {
                item.score.is_hunk_local = hunk_lines.contains(&line);
            }
        }
    }

    items.sort_by_key(|i| std::cmp::Reverse(i.score.score()));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(
        kind: LspContextItemKind,
        file: &str,
        line: Option<u32>,
        message: &str,
        priority: u32,
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
                priority,
                is_hunk_local: false,
                is_error: false,
                is_same_file: false,
                freshness_rank: 0,
            },
            payload: None,
        }
    }

    #[test]
    fn test_default_budget_has_conservative_values() {
        let b = LspContextBudget::default();
        assert_eq!(b.max_files, 10);
        assert_eq!(b.max_ranges_per_file, 5);
        assert_eq!(b.max_diagnostics, 20);
        assert_eq!(b.max_references, 30);
        assert_eq!(b.max_symbols, 30);
        assert_eq!(b.max_completion_items, 10);
        assert_eq!(b.max_semantic_tokens, 200);
        assert_eq!(b.max_bytes, 32_768);
    }

    #[test]
    fn test_budget_limits_total_bytes() {
        // Spread across many files to avoid per-file range limits.
        let mut packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: (0..20)
                    .map(|i| PathBuf::from(format!("file_{i}.rs")))
                    .collect(),
                hunks: Vec::new(),
                risk_mode: LspRiskMode::default(),
            },
            items: (0..50)
                .map(|i| {
                    let file_idx = i % 20;
                    make_item(
                        LspContextItemKind::Diagnostic,
                        &format!("file_{file_idx}.rs"),
                        Some((i / 20) as u32),
                        &format!("error {i}: something went wrong in module {i} with long details to fill bytes"),
                        i,
                    )
                })
                .collect(),
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::default(),
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: LspContextTruncation::default(),
        };
        let truncation = enforce_context_budget(&mut packet);
        assert!(
            truncation.bytes_truncated || truncation.diagnostics_truncated,
            "expected some truncation; notes: {:?}",
            truncation.notes
        );
    }

    #[test]
    fn test_budget_limits_files() {
        let mut packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: (0..15)
                    .map(|i| PathBuf::from(format!("file_{i}.rs")))
                    .collect(),
                hunks: Vec::new(),
                risk_mode: LspRiskMode::default(),
            },
            items: (0..15)
                .map(|i| {
                    make_item(
                        LspContextItemKind::Diagnostic,
                        &format!("file_{i}.rs"),
                        Some(0),
                        "error",
                        10,
                    )
                })
                .collect(),
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::default(),
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: LspContextTruncation::default(),
        };
        let truncation = enforce_context_budget(&mut packet);
        assert!(truncation.files_truncated);
        let unique: std::collections::HashSet<_> =
            packet.items.iter().map(|i| i.file.clone()).collect();
        assert!(unique.len() <= 10);
    }

    #[test]
    fn test_budget_limits_diagnostics() {
        // Spread across many files so per-file range limits don't eat them first.
        let mut packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: (0..30)
                    .map(|i| PathBuf::from(format!("file_{i}.rs")))
                    .collect(),
                hunks: Vec::new(),
                risk_mode: LspRiskMode::default(),
            },
            items: (0..40)
                .map(|i| {
                    make_item(
                        LspContextItemKind::Diagnostic,
                        &format!("file_{}.rs", i % 30),
                        Some(i as u32),
                        &format!("diag {i}"),
                        10,
                    )
                })
                .collect(),
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::default(),
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: LspContextTruncation::default(),
        };
        let truncation = enforce_context_budget(&mut packet);
        assert!(
            truncation.diagnostics_truncated,
            "expected diagnostics truncation; notes: {:?}",
            truncation.notes
        );
        let diag_count = packet
            .items
            .iter()
            .filter(|i| i.kind == LspContextItemKind::Diagnostic)
            .count();
        assert!(diag_count <= 20);
    }

    #[test]
    fn test_budget_truncation_notes_recorded() {
        // Spread across many files so per-file range limits don't interfere.
        let mut packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: (0..20)
                    .map(|i| PathBuf::from(format!("file_{i}.rs")))
                    .collect(),
                hunks: Vec::new(),
                risk_mode: LspRiskMode::default(),
            },
            items: (0..50)
                .map(|i| {
                    let file_idx = i % 20;
                    make_item(
                        LspContextItemKind::Diagnostic,
                        &format!("file_{file_idx}.rs"),
                        Some((i / 20) as u32),
                        &format!("diag {i}"),
                        i,
                    )
                })
                .collect(),
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::default(),
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: LspContextTruncation::default(),
        };
        let truncation = enforce_context_budget(&mut packet);
        assert!(
            !truncation.notes.is_empty(),
            "expected truncation notes to be recorded"
        );
    }

    #[test]
    fn test_dedup_preserves_distinct_ranges() {
        let items = vec![
            make_item(LspContextItemKind::Diagnostic, "a.rs", Some(10), "msg", 10),
            make_item(LspContextItemKind::Diagnostic, "a.rs", Some(11), "msg", 10),
            make_item(LspContextItemKind::Diagnostic, "a.rs", Some(10), "msg", 10),
        ];
        let deduped = dedup_context_items(items);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn test_ranking_prefers_hunk_local_items() {
        let request = LspContextRequest::Hunk {
            file: PathBuf::from("a.rs"),
            hunks: vec![HunkRange {
                start: 10,
                end: 15,
                original_start: None,
                original_end: None,
            }],
            include_references: false,
            include_definitions: false,
            include_implementations: false,
            include_semantic_tokens: false,
            include_security_evidence: false,
        };

        let mut items = vec![
            make_item(LspContextItemKind::Diagnostic, "a.rs", Some(20), "far", 10),
            make_item(LspContextItemKind::Diagnostic, "a.rs", Some(12), "near", 10),
        ];

        rank_context_items(&mut items, &request);

        // The hunk-local item (line 12) should rank higher.
        let near_idx = items.iter().position(|i| i.message == "near").unwrap();
        let far_idx = items.iter().position(|i| i.message == "far").unwrap();
        assert!(near_idx < far_idx);
    }

    #[test]
    fn test_ranking_prefers_fresh_items() {
        let request = LspContextRequest::Review {
            changed_files: vec![PathBuf::from("a.rs")],
            hunks: Vec::new(),
            risk_mode: LspRiskMode::default(),
        };

        let mut fresh = make_item(LspContextItemKind::Diagnostic, "a.rs", Some(0), "fresh", 10);
        fresh.score.freshness_rank = 0;
        let mut stale = make_item(LspContextItemKind::Diagnostic, "a.rs", Some(0), "stale", 10);
        stale.score.freshness_rank = 3;

        let mut items = vec![stale, fresh];
        rank_context_items(&mut items, &request);

        let fresh_idx = items.iter().position(|i| i.message == "fresh").unwrap();
        let stale_idx = items.iter().position(|i| i.message == "stale").unwrap();
        assert!(fresh_idx < stale_idx);
    }

    #[test]
    fn test_line_range_len() {
        let r = LineRange { start: 5, end: 12 };
        assert_eq!(r.len(), 7);
        assert!(!r.is_empty());
        let empty = LineRange { start: 5, end: 5 };
        assert!(empty.is_empty());
    }

    #[test]
    fn test_score_calculation() {
        let score = LspContextScore {
            priority: 100,
            is_hunk_local: true,
            is_error: true,
            is_same_file: true,
            freshness_rank: 0,
        };
        // 100 + 50 + 30 + 20 + 0 = 200
        assert_eq!(score.score(), 200);
    }

    #[test]
    fn test_review_request_risk_mode_default() {
        assert_eq!(LspRiskMode::default(), LspRiskMode::Standard);
    }
}
