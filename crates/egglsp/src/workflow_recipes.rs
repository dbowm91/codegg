//! Named workflow recipes for common LSP-assisted tasks.
//!
//! Recipes are the Phase 7 orchestration layer: given a task type,
//! they gather a bounded set of evidence, render it for the model
//! tier, preserve freshness notes, and degrade gracefully when the
//! LSP server is unavailable.
//!
//! # Design
//!
//! Recipes are **not** a new framework — they are thin helper
//! functions that assemble existing [`LspContextRequest`] values,
//! collect evidence, and produce a rendered outcome. Each recipe
//! follows the pattern:
//!
//! ```text
//! inputs → evidence requests → budget → fallback → render policy
//! ```
//!
//! # Recipe taxonomy
//!
//! | Recipe | Purpose |
//! |--------|---------|
//! | `repair_local` | Repair a localized issue around a target line/diagnostic |
//! | `repair_hunk` | Repair code around changed diff hunks |
//! | `review_file` | Semantic review of a single file without a diff |
//! | `review_diff` | Semantic review of changed files/hunks |
//! | `security_review_enriched` | Enrich deterministic security review with LSP evidence |
//! | `hunk_source_navigation` | Collect semantic context around changed hunks |
//! | `preview_suggestion` | Include safe preview-only edit suggestions |

use std::path::PathBuf;

use crate::context::{
    AgentContextSource, HunkRange, LineRange, LspContextBudget, LspContextMode, LspContextPacket,
    LspContextPacketMode, LspContextRequest, LspContextTruncation, LspRiskMode,
};
use crate::context_renderer::{render_lsp_context_for_agent, LspContextRenderConfig, ModelTier};
use crate::evidence_collector::{
    collect_context, collect_hunk_context, LspContextError, LspEvidenceProvider,
};
use crate::hunk_context::HunkDescriptor;

// ---------------------------------------------------------------------------
// Recipe enum
// ---------------------------------------------------------------------------

/// Named workflow recipes that agents can invoke.
///
/// Each variant maps to a specific evidence-gathering strategy,
/// budget policy, and render configuration. The recipe names are
/// internal; public UI may use different command names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LspWorkflowRecipe {
    /// Repair a localized issue in one file around a target line/column
    /// or diagnostic.
    RepairLocal,
    /// Repair code around changed diff hunks.
    RepairHunk,
    /// Semantic review of a single file without a diff.
    ReviewFile,
    /// Semantic review of changed files/hunks.
    ReviewDiff,
    /// Enrich deterministic security review with LSP-backed evidence.
    SecurityReviewEnriched,
    /// Collect semantic context around changed hunks for source
    /// navigation and review.
    HunkSourceNavigation,
    /// Include safe preview-only semantic edit suggestions in
    /// repair/review context.
    PreviewSuggestion,
    /// Impact analysis for a symbol change across files.
    ImpactAnalysis,
    /// Test failure repair with heuristic symbol extraction.
    TestFailureRepair,
    /// API/interface boundary review with implementations.
    InterfaceBoundary,
    /// Cross-file repair evidence gathering.
    CrossFileRepair,
    /// Shallow call neighborhood around a symbol.
    CallNeighborhood,
}

impl std::fmt::Display for LspWorkflowRecipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepairLocal => write!(f, "repair_local"),
            Self::RepairHunk => write!(f, "repair_hunk"),
            Self::ReviewFile => write!(f, "review_file"),
            Self::ReviewDiff => write!(f, "review_diff"),
            Self::SecurityReviewEnriched => write!(f, "security_review_enriched"),
            Self::HunkSourceNavigation => write!(f, "hunk_source_navigation"),
            Self::PreviewSuggestion => write!(f, "preview_suggestion"),
            Self::ImpactAnalysis => write!(f, "impact_analysis"),
            Self::TestFailureRepair => write!(f, "test_failure_repair"),
            Self::InterfaceBoundary => write!(f, "interface_boundary"),
            Self::CrossFileRepair => write!(f, "cross_file_repair"),
            Self::CallNeighborhood => write!(f, "call_neighborhood"),
        }
    }
}

// ---------------------------------------------------------------------------
// Recipe settings
// ---------------------------------------------------------------------------

/// Shared settings that control how a recipe gathers and renders evidence.
#[derive(Debug, Clone)]
pub struct RecipeSettings {
    /// Model tier controlling content breadth.
    pub model_tier: ModelTier,
    /// Whether LSP context is required or opportunistic.
    pub mode: LspContextMode,
    /// Risk classification mode.
    pub risk_mode: LspRiskMode,
    /// Maximum number of files to include.
    pub max_files: usize,
    /// Maximum ranges per file.
    pub max_ranges_per_file: usize,
    /// Maximum diagnostics across all files.
    pub max_diagnostics: usize,
    /// Maximum references across all files.
    pub max_references: usize,
    /// Maximum symbols across all files.
    pub max_symbols: usize,
    /// Whether to include definitions in the evidence.
    pub include_definitions: bool,
    /// Whether to include references in the evidence.
    pub include_references: bool,
    /// Whether to include preview hints (source actions).
    pub include_preview_hints: bool,
    /// Freshness tolerance: whether to include stale evidence.
    pub allow_stale_evidence: bool,
}

impl Default for RecipeSettings {
    fn default() -> Self {
        Self {
            model_tier: ModelTier::Workhorse,
            mode: LspContextMode::Opportunistic,
            risk_mode: LspRiskMode::Standard,
            max_files: 10,
            max_ranges_per_file: 5,
            max_diagnostics: 20,
            max_references: 30,
            max_symbols: 30,
            include_definitions: true,
            include_references: true,
            include_preview_hints: false,
            allow_stale_evidence: true,
        }
    }
}

impl RecipeSettings {
    /// Settings tuned for a small/fast model tier.
    pub fn small_tier() -> Self {
        Self {
            model_tier: ModelTier::Small,
            include_definitions: true,
            include_references: false,
            include_preview_hints: false,
            max_references: 0,
            ..Default::default()
        }
    }

    /// Settings tuned for a workhorse model tier.
    pub fn workhorse_tier() -> Self {
        Self {
            model_tier: ModelTier::Workhorse,
            include_definitions: true,
            include_references: true,
            include_preview_hints: false,
            ..Default::default()
        }
    }

    /// Settings tuned for a frontier model tier.
    pub fn frontier_tier() -> Self {
        Self {
            model_tier: ModelTier::Frontier,
            include_definitions: true,
            include_references: true,
            include_preview_hints: true,
            max_references: 50,
            max_symbols: 50,
            ..Default::default()
        }
    }

    /// Derive settings from a model tier with sensible defaults.
    pub fn for_tier(tier: ModelTier) -> Self {
        match tier {
            ModelTier::Small => Self::small_tier(),
            ModelTier::Workhorse => Self::workhorse_tier(),
            ModelTier::Frontier => Self::frontier_tier(),
        }
    }

    /// Convert settings into a budget for evidence collection.
    pub fn to_budget(&self) -> LspContextBudget {
        LspContextBudget {
            max_files: self.max_files,
            max_ranges_per_file: self.max_ranges_per_file,
            max_diagnostics: self.max_diagnostics,
            max_references: self.max_references,
            max_symbols: self.max_symbols,
            max_completion_items: 10,
            max_semantic_tokens: 200,
            max_bytes: 32_768,
        }
    }

    /// Convert settings into a render config.
    pub fn to_render_config(&self) -> LspContextRenderConfig {
        LspContextRenderConfig {
            max_diagnostics: self.max_diagnostics.min(20),
            max_references: self.max_references.min(30),
            max_symbols: self.max_symbols.min(30),
            max_bytes_per_section: match self.model_tier {
                ModelTier::Small => 1000,
                ModelTier::Workhorse => 2000,
                ModelTier::Frontier => 4000,
            },
            include_previews: self.include_preview_hints,
            include_truncation_notes: true,
            model_tier: self.model_tier,
        }
    }
}

// ---------------------------------------------------------------------------
// Recipe outcome
// ---------------------------------------------------------------------------

/// The result of executing a workflow recipe.
///
/// Contains the canonical packet, a rendered text representation,
/// structured notes, and metadata about fallback/truncation state.
#[derive(Debug, Clone)]
pub struct RecipeOutcome {
    /// The recipe that produced this outcome.
    pub recipe: LspWorkflowRecipe,
    /// The canonical context packet assembled by the recipe.
    pub packet: LspContextPacket,
    /// Rendered text suitable for agent prompts.
    pub rendered: String,
    /// Structured notes about fallback, truncation, or degraded state.
    pub notes: Vec<String>,
    /// Whether the LSP server was unavailable or degraded.
    pub fallback_used: bool,
    /// Preview IDs if any preview artifacts were generated.
    pub preview_ids: Vec<String>,
    /// Stale/freshness summary.
    pub freshness_summary: String,
}

// ---------------------------------------------------------------------------
// Recipe request types
// ---------------------------------------------------------------------------

/// Request for the `repair_local` recipe.
#[derive(Debug, Clone)]
pub struct RepairLocalRequest {
    /// Path to the file to repair.
    pub file: PathBuf,
    /// Target line (0-indexed) around which to gather context.
    pub line: u32,
    /// Target column (0-indexed).
    pub column: u32,
    /// Optional diagnostic message to focus on.
    pub diagnostic_message: Option<String>,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `repair_hunk` recipe.
#[derive(Debug, Clone)]
pub struct RepairHunkRequest {
    /// Path to the file containing the hunks.
    pub file: PathBuf,
    /// Hunks to gather context for.
    pub hunks: Vec<HunkRange>,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `review_file` recipe.
#[derive(Debug, Clone)]
pub struct ReviewFileRequest {
    /// Path to the file to review.
    pub file: PathBuf,
    /// Specific line ranges to review (empty = bounded symbol/diagnostic summary).
    pub line_ranges: Vec<LineRange>,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `review_diff` recipe.
#[derive(Debug, Clone)]
pub struct ReviewDiffRequest {
    /// Changed files.
    pub changed_files: Vec<PathBuf>,
    /// Hunk descriptors from the diff.
    pub hunks: Vec<HunkDescriptor>,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `security_review_enriched` recipe.
#[derive(Debug, Clone)]
pub struct SecurityReviewEnrichedRequest {
    /// Changed files.
    pub changed_files: Vec<PathBuf>,
    /// Hunk descriptors from the diff.
    pub hunks: Vec<HunkDescriptor>,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `hunk_source_navigation` recipe.
#[derive(Debug, Clone)]
pub struct HunkSourceNavigationRecipeRequest {
    /// Path to the file containing the hunks.
    pub file: PathBuf,
    /// Hunk descriptors.
    pub hunks: Vec<HunkDescriptor>,
    /// Intent description.
    pub intent: String,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `preview_suggestion` recipe.
#[derive(Debug, Clone)]
pub struct PreviewSuggestionRequest {
    /// Path to the file.
    pub file: PathBuf,
    /// Target line (0-indexed).
    pub line: u32,
    /// Target column (0-indexed).
    pub column: u32,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `impact_analysis` recipe.
#[derive(Debug, Clone)]
pub struct ImpactAnalysisRequest {
    /// Target symbol to analyze.
    pub symbol: crate::context::SymbolTarget,
    /// Files that have been changed.
    pub changed_files: Vec<PathBuf>,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `test_failure_repair` recipe.
#[derive(Debug, Clone)]
pub struct TestFailureRepairRequest {
    /// Path to the test file.
    pub test_file: PathBuf,
    /// Test failure output/message.
    pub failure_message: String,
    /// Related source files from the test runner or user.
    pub related_files: Vec<PathBuf>,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `interface_boundary` recipe.
#[derive(Debug, Clone)]
pub struct InterfaceBoundaryRequest {
    /// File containing the boundary symbols.
    pub file: PathBuf,
    /// Optional specific symbol to focus on.
    pub symbol: Option<String>,
    /// Whether to include trait/interface implementations.
    pub include_implementations: bool,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `cross_file_repair` recipe.
#[derive(Debug, Clone)]
pub struct CrossFileRepairRequest {
    /// Primary file being repaired.
    pub primary_file: PathBuf,
    /// Related files to gather evidence from.
    pub related_files: Vec<PathBuf>,
    /// Line ranges of interest in the primary file.
    pub ranges: Vec<LineRange>,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

/// Request for the `call_neighborhood` recipe.
#[derive(Debug, Clone)]
pub struct CallNeighborhoodRequest {
    /// File containing the target symbol.
    pub file: PathBuf,
    /// Line of the target symbol (0-indexed).
    pub line: u32,
    /// Column of the target symbol (0-indexed).
    pub column: u32,
    /// Direction of call traversal.
    pub direction: crate::context::HierarchyDirection,
    /// Maximum traversal depth (default 1).
    pub max_depth: u8,
    /// Recipe settings.
    pub settings: RecipeSettings,
}

// ---------------------------------------------------------------------------
// Recipe execution helpers
// ---------------------------------------------------------------------------

/// Execute the `repair_local` recipe.
///
/// Gathers source excerpt context around a target line, diagnostics,
/// enclosing symbols, and optionally definitions/references based
/// on the model tier.
pub async fn execute_repair_local(
    provider: &dyn LspEvidenceProvider,
    request: &RepairLocalRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();
    let range = LineRange {
        start: request.line.saturating_sub(20),
        end: request.line + 20,
    };

    let ctx_request = LspContextRequest::File {
        file: request.file.clone(),
        line_ranges: vec![range],
        include_symbols: true,
        include_diagnostics: true,
    };

    let mode = &request.settings.mode;
    let packet = collect_context(provider, &ctx_request, &budget, mode).await?;
    finish_recipe_outcome(LspWorkflowRecipe::RepairLocal, packet, &request.settings)
}

/// Execute the `repair_hunk` recipe.
///
/// Gathers hunk-local diagnostics, enclosing symbols, and
/// optionally definitions/references near changed lines.
pub async fn execute_repair_hunk(
    provider: &dyn LspEvidenceProvider,
    request: &RepairHunkRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    let items = collect_hunk_context(
        provider,
        &request.file,
        &request.hunks,
        request.settings.include_references,
        request.settings.include_definitions,
        false,
        false,
        &budget,
    )
    .await?;

    let mut packet = LspContextPacket {
        request: LspContextRequest::Hunk {
            file: request.file.clone(),
            hunks: request.hunks.clone(),
            include_references: request.settings.include_references,
            include_definitions: request.settings.include_definitions,
            include_implementations: false,
            include_semantic_tokens: false,
            include_security_evidence: false,
        },
        items,
        previews: Vec::new(),
        preview_ids: Vec::new(),
        mode: request.settings.mode,
        workspace_root: None,
        generated_at: None,
        server_id: None,
        server_generation: None,
        operational_state: None,
        budget: Some(budget),
        notes: Vec::new(),
        truncation: LspContextTruncation::default(),
    };

    let mut items = crate::context::dedup_context_items(std::mem::take(&mut packet.items));
    crate::context::rank_context_items(&mut items, &packet.request);
    packet.items = items;
    crate::context::enforce_context_budget(&mut packet);

    finish_recipe_outcome(LspWorkflowRecipe::RepairHunk, packet, &request.settings)
}

/// Execute the `review_file` recipe.
///
/// Gathers file diagnostics, document symbols, and bounded
/// evidence without scanning the entire file into context.
pub async fn execute_review_file(
    provider: &dyn LspEvidenceProvider,
    request: &ReviewFileRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    let ctx_request = LspContextRequest::File {
        file: request.file.clone(),
        line_ranges: request.line_ranges.clone(),
        include_symbols: true,
        include_diagnostics: true,
    };

    let mode = &request.settings.mode;
    let packet = collect_context(provider, &ctx_request, &budget, mode).await?;
    finish_recipe_outcome(LspWorkflowRecipe::ReviewFile, packet, &request.settings)
}

/// Execute the `review_diff` recipe.
///
/// Gathers diagnostics near changes, definitions/references for
/// changed symbols, and optional implementations under higher risk.
pub async fn execute_review_diff(
    provider: &dyn LspEvidenceProvider,
    request: &ReviewDiffRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    let ctx_request = LspContextRequest::Review {
        changed_files: request.changed_files.clone(),
        hunks: request.hunks.clone(),
        risk_mode: request.settings.risk_mode,
    };

    let mode = &request.settings.mode;
    let packet = collect_context(provider, &ctx_request, &budget, mode).await?;
    finish_recipe_outcome(LspWorkflowRecipe::ReviewDiff, packet, &request.settings)
}

/// Execute the `security_review_enriched` recipe.
///
/// Builds a security-focused review request with aggressive risk
/// mode and produces a recipe outcome with security-specific notes.
pub async fn execute_security_review_enriched(
    provider: &dyn LspEvidenceProvider,
    request: &SecurityReviewEnrichedRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    let ctx_request = LspContextRequest::Review {
        changed_files: request.changed_files.clone(),
        hunks: request.hunks.clone(),
        risk_mode: LspRiskMode::Aggressive,
    };

    let mode = &request.settings.mode;
    let mut packet = collect_context(provider, &ctx_request, &budget, mode).await?;

    // Add security-specific notes.
    packet
        .notes
        .push("recipe: security_review_enriched".to_string());
    if !request.hunks.is_empty() {
        packet.notes.push(format!(
            "reviewing {} hunks across {} files",
            request.hunks.len(),
            request.changed_files.len()
        ));
    }

    finish_recipe_outcome(
        LspWorkflowRecipe::SecurityReviewEnriched,
        packet,
        &request.settings,
    )
}

/// Execute the `hunk_source_navigation` recipe.
///
/// Gathers per-hunk source excerpts, enclosing symbols, hunk-local
/// diagnostics, and optional definitions/references.
pub async fn execute_hunk_source_navigation(
    provider: &dyn LspEvidenceProvider,
    request: &HunkSourceNavigationRecipeRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    // Build hunk ranges from descriptors.
    let hunk_ranges: Vec<HunkRange> = request
        .hunks
        .iter()
        .filter_map(|h| {
            h.new_range.as_ref().map(|r| HunkRange {
                start: r.start_line.saturating_sub(1),
                end: r.end_line,
                original_start: h.old_range.as_ref().map(|o| o.start_line.saturating_sub(1)),
                original_end: h.old_range.as_ref().map(|o| o.end_line),
            })
        })
        .collect();

    let items = collect_hunk_context(
        provider,
        &request.file,
        &hunk_ranges,
        request.settings.include_references,
        request.settings.include_definitions,
        false,
        false,
        &budget,
    )
    .await?;

    let mut packet = LspContextPacket {
        request: LspContextRequest::Hunk {
            file: request.file.clone(),
            hunks: hunk_ranges,
            include_references: request.settings.include_references,
            include_definitions: request.settings.include_definitions,
            include_implementations: false,
            include_semantic_tokens: false,
            include_security_evidence: false,
        },
        items,
        previews: Vec::new(),
        preview_ids: Vec::new(),
        mode: request.settings.mode,
        workspace_root: None,
        generated_at: None,
        server_id: None,
        server_generation: None,
        operational_state: None,
        budget: Some(budget),
        notes: vec![format!(
            "recipe: hunk_source_navigation ({})",
            request.intent
        )],
        truncation: LspContextTruncation::default(),
    };

    let mut items = crate::context::dedup_context_items(packet.items);
    crate::context::rank_context_items(&mut items, &packet.request);
    packet.items = items;
    crate::context::enforce_context_budget(&mut packet);

    // Tag all items with Hunk source.
    for item in &mut packet.items {
        item.source = Some(AgentContextSource::Hunk);
    }

    finish_recipe_outcome(
        LspWorkflowRecipe::HunkSourceNavigation,
        packet,
        &request.settings,
    )
}

/// Execute the `preview_suggestion` recipe.
///
/// Collects source-action hints where cheap, but does NOT apply
/// any previews. Returns preview metadata only.
pub async fn execute_preview_suggestion(
    provider: &dyn LspEvidenceProvider,
    request: &PreviewSuggestionRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    let ctx_request = LspContextRequest::File {
        file: request.file.clone(),
        line_ranges: vec![LineRange {
            start: request.line.saturating_sub(10),
            end: request.line + 10,
        }],
        include_symbols: false,
        include_diagnostics: true,
    };

    let mode = &request.settings.mode;
    let mut packet = collect_context(provider, &ctx_request, &budget, mode).await?;

    packet
        .notes
        .push("recipe: preview_suggestion — previews are not applied".to_string());

    finish_recipe_outcome(
        LspWorkflowRecipe::PreviewSuggestion,
        packet,
        &request.settings,
    )
}

/// Execute the `impact_analysis` recipe.
///
/// Gathers definition, capped cross-file references, diagnostics in
/// affected files, and document highlights near changed hunks.
pub async fn execute_impact_analysis(
    provider: &dyn LspEvidenceProvider,
    request: &ImpactAnalysisRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();
    let (max_refs, max_files, max_depth) = match request.settings.model_tier {
        ModelTier::Small => (5, 1, 0),
        ModelTier::Workhorse => (20, 5, 1),
        ModelTier::Frontier => (50, 10, 1),
    };

    let ctx_request = LspContextRequest::ImpactAnalysis {
        symbol: request.symbol.clone(),
        changed_files: request.changed_files.clone(),
        max_refs,
        max_files,
        max_depth,
    };

    let mode = &request.settings.mode;
    let packet = collect_context(provider, &ctx_request, &budget, mode).await?;
    finish_recipe_outcome(LspWorkflowRecipe::ImpactAnalysis, packet, &request.settings)
}

/// Execute the `test_failure_repair` recipe.
///
/// Extracts symbols from failure messages, gathers test-file
/// diagnostics and definitions for extracted symbols.
pub async fn execute_test_failure_repair(
    provider: &dyn LspEvidenceProvider,
    request: &TestFailureRepairRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    let ctx_request = LspContextRequest::TestFailureRepair {
        test_file: request.test_file.clone(),
        failure_message: request.failure_message.clone(),
        related_files: request.related_files.clone(),
    };

    let mode = &request.settings.mode;
    let packet = collect_context(provider, &ctx_request, &budget, mode).await?;
    finish_recipe_outcome(
        LspWorkflowRecipe::TestFailureRepair,
        packet,
        &request.settings,
    )
}

/// Execute the `interface_boundary` recipe.
///
/// Gathers document symbols, definitions, implementations, and
/// hover for public/exported API boundary symbols.
pub async fn execute_interface_boundary(
    provider: &dyn LspEvidenceProvider,
    request: &InterfaceBoundaryRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    let ctx_request = LspContextRequest::InterfaceBoundary {
        file: request.file.clone(),
        symbol: request.symbol.clone(),
        include_implementations: request.include_implementations,
    };

    let mode = &request.settings.mode;
    let packet = collect_context(provider, &ctx_request, &budget, mode).await?;
    finish_recipe_outcome(
        LspWorkflowRecipe::InterfaceBoundary,
        packet,
        &request.settings,
    )
}

/// Execute the `cross_file_repair` recipe.
///
/// Gathers diagnostics, symbols, and definitions/references across
/// a primary file and related files with explicit file caps.
pub async fn execute_cross_file_repair(
    provider: &dyn LspEvidenceProvider,
    request: &CrossFileRepairRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    let ctx_request = LspContextRequest::CrossFileRepair {
        primary_file: request.primary_file.clone(),
        related_files: request.related_files.clone(),
        ranges: request.ranges.clone(),
    };

    let mode = &request.settings.mode;
    let packet = collect_context(provider, &ctx_request, &budget, mode).await?;
    finish_recipe_outcome(
        LspWorkflowRecipe::CrossFileRepair,
        packet,
        &request.settings,
    )
}

/// Execute the `call_neighborhood` recipe.
///
/// Gathers shallow call hierarchy around a symbol with cycle-safe
/// dedup and incoming/outgoing caps.
pub async fn execute_call_neighborhood(
    provider: &dyn LspEvidenceProvider,
    request: &CallNeighborhoodRequest,
) -> Result<RecipeOutcome, LspContextError> {
    let budget = request.settings.to_budget();

    let ctx_request = LspContextRequest::CallNeighborhood {
        file: request.file.clone(),
        line: request.line,
        column: request.column,
        direction: request.direction,
        max_depth: request.max_depth,
        max_callers: 10,
        max_callees: 10,
    };

    let mode = &request.settings.mode;
    let packet = collect_context(provider, &ctx_request, &budget, mode).await?;
    finish_recipe_outcome(
        LspWorkflowRecipe::CallNeighborhood,
        packet,
        &request.settings,
    )
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Finish building a recipe outcome from a packet.
fn finish_recipe_outcome(
    recipe: LspWorkflowRecipe,
    packet: LspContextPacket,
    settings: &RecipeSettings,
) -> Result<RecipeOutcome, LspContextError> {
    let render_config = settings.to_render_config();
    let rendered = render_lsp_context_for_agent(&packet, &render_config);

    let fallback_used = matches!(packet.mode, LspContextPacketMode::Disabled)
        || packet
            .items
            .iter()
            .any(|i| i.kind == crate::context::LspContextItemKind::OperationalNote)
        || (matches!(packet.mode, LspContextPacketMode::Opportunistic)
            && !packet.items.is_empty()
            && packet
                .items
                .iter()
                .all(|i| i.kind == crate::context::LspContextItemKind::OperationalNote));

    let has_stale = packet.items.iter().any(|i| {
        matches!(
            i.provenance.freshness,
            crate::context::LspEvidenceFreshness::Stale
                | crate::context::LspEvidenceFreshness::PossiblyStale
                | crate::context::LspEvidenceFreshness::StaleAfterEdit
        )
    });

    let freshness_summary = if has_stale {
        "some evidence may be stale".to_string()
    } else {
        "evidence is fresh".to_string()
    };

    let mut notes = packet.notes.clone();
    if fallback_used {
        notes.push("LSP unavailable or degraded; using fallback".to_string());
    }

    let preview_ids = packet.preview_ids.clone();

    Ok(RecipeOutcome {
        recipe,
        packet,
        rendered,
        notes,
        fallback_used,
        preview_ids,
        freshness_summary,
    })
}

/// Get default recipe settings for a recipe and tier combination.
pub fn default_settings_for_recipe(recipe: LspWorkflowRecipe, tier: ModelTier) -> RecipeSettings {
    let mut settings = RecipeSettings::for_tier(tier);
    match recipe {
        LspWorkflowRecipe::RepairLocal => {
            settings.include_references = !matches!(tier, ModelTier::Small);
            settings.include_preview_hints = matches!(tier, ModelTier::Frontier);
        }
        LspWorkflowRecipe::RepairHunk => {
            settings.include_references = !matches!(tier, ModelTier::Small);
            settings.max_files = 1;
        }
        LspWorkflowRecipe::ReviewFile => {
            settings.max_files = 1;
            settings.include_references =
                matches!(tier, ModelTier::Workhorse | ModelTier::Frontier);
        }
        LspWorkflowRecipe::ReviewDiff => {
            settings.risk_mode = match tier {
                ModelTier::Small => LspRiskMode::Conservative,
                ModelTier::Workhorse => LspRiskMode::Standard,
                ModelTier::Frontier => LspRiskMode::Aggressive,
            };
        }
        LspWorkflowRecipe::SecurityReviewEnriched => {
            settings.risk_mode = LspRiskMode::Aggressive;
            settings.include_references = true;
            settings.include_preview_hints = false;
        }
        LspWorkflowRecipe::HunkSourceNavigation => {
            settings.max_files = 1;
            settings.include_references = !matches!(tier, ModelTier::Small);
        }
        LspWorkflowRecipe::PreviewSuggestion => {
            settings.max_files = 1;
            settings.include_references = false;
            settings.include_definitions = false;
            settings.include_preview_hints = true;
        }
        LspWorkflowRecipe::ImpactAnalysis => {
            settings.max_references = match tier {
                ModelTier::Small => 5,
                ModelTier::Workhorse => 20,
                ModelTier::Frontier => 50,
            };
            settings.max_files = match tier {
                ModelTier::Small => 1,
                ModelTier::Workhorse => 5,
                ModelTier::Frontier => 10,
            };
            settings.include_references = true;
        }
        LspWorkflowRecipe::TestFailureRepair => {
            settings.max_files = match tier {
                ModelTier::Small => 1,
                ModelTier::Workhorse => 3,
                ModelTier::Frontier => 5,
            };
            settings.include_references = false;
            settings.include_definitions = true;
        }
        LspWorkflowRecipe::InterfaceBoundary => {
            settings.max_files = 1;
            settings.include_references =
                matches!(tier, ModelTier::Workhorse | ModelTier::Frontier);
        }
        LspWorkflowRecipe::CrossFileRepair => {
            settings.max_files = match tier {
                ModelTier::Small => 1,
                ModelTier::Workhorse => 3,
                ModelTier::Frontier => 5,
            };
            settings.include_references = !matches!(tier, ModelTier::Small);
        }
        LspWorkflowRecipe::CallNeighborhood => {
            settings.max_files = 1;
            settings.include_references = !matches!(tier, ModelTier::Small);
        }
    }
    settings
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_display_names() {
        assert_eq!(LspWorkflowRecipe::RepairLocal.to_string(), "repair_local");
        assert_eq!(LspWorkflowRecipe::RepairHunk.to_string(), "repair_hunk");
        assert_eq!(LspWorkflowRecipe::ReviewFile.to_string(), "review_file");
        assert_eq!(LspWorkflowRecipe::ReviewDiff.to_string(), "review_diff");
        assert_eq!(
            LspWorkflowRecipe::SecurityReviewEnriched.to_string(),
            "security_review_enriched"
        );
        assert_eq!(
            LspWorkflowRecipe::HunkSourceNavigation.to_string(),
            "hunk_source_navigation"
        );
        assert_eq!(
            LspWorkflowRecipe::PreviewSuggestion.to_string(),
            "preview_suggestion"
        );
        assert_eq!(
            LspWorkflowRecipe::ImpactAnalysis.to_string(),
            "impact_analysis"
        );
        assert_eq!(
            LspWorkflowRecipe::TestFailureRepair.to_string(),
            "test_failure_repair"
        );
        assert_eq!(
            LspWorkflowRecipe::InterfaceBoundary.to_string(),
            "interface_boundary"
        );
        assert_eq!(
            LspWorkflowRecipe::CrossFileRepair.to_string(),
            "cross_file_repair"
        );
        assert_eq!(
            LspWorkflowRecipe::CallNeighborhood.to_string(),
            "call_neighborhood"
        );
    }

    #[test]
    fn settings_default_values() {
        let s = RecipeSettings::default();
        assert_eq!(s.model_tier, ModelTier::Workhorse);
        assert!(s.include_definitions);
        assert!(s.include_references);
        assert!(!s.include_preview_hints);
        assert!(s.allow_stale_evidence);
    }

    #[test]
    fn settings_small_tier_omits_references() {
        let s = RecipeSettings::small_tier();
        assert_eq!(s.model_tier, ModelTier::Small);
        assert!(!s.include_references);
        assert_eq!(s.max_references, 0);
    }

    #[test]
    fn settings_frontier_tier_includes_preview_hints() {
        let s = RecipeSettings::frontier_tier();
        assert_eq!(s.model_tier, ModelTier::Frontier);
        assert!(s.include_preview_hints);
        assert!(s.include_references);
        assert_eq!(s.max_references, 50);
    }

    #[test]
    fn impact_analysis_tier_defaults() {
        let small =
            default_settings_for_recipe(LspWorkflowRecipe::ImpactAnalysis, ModelTier::Small);
        assert_eq!(small.max_references, 5);
        assert_eq!(small.max_files, 1);
        assert!(small.include_references);

        let workhorse =
            default_settings_for_recipe(LspWorkflowRecipe::ImpactAnalysis, ModelTier::Workhorse);
        assert_eq!(workhorse.max_references, 20);
        assert_eq!(workhorse.max_files, 5);

        let frontier =
            default_settings_for_recipe(LspWorkflowRecipe::ImpactAnalysis, ModelTier::Frontier);
        assert_eq!(frontier.max_references, 50);
        assert_eq!(frontier.max_files, 10);
    }

    #[test]
    fn test_failure_repair_tier_defaults() {
        let small =
            default_settings_for_recipe(LspWorkflowRecipe::TestFailureRepair, ModelTier::Small);
        assert_eq!(small.max_files, 1);
        assert!(small.include_definitions);
        assert!(!small.include_references);

        let frontier =
            default_settings_for_recipe(LspWorkflowRecipe::TestFailureRepair, ModelTier::Frontier);
        assert_eq!(frontier.max_files, 5);
    }

    #[test]
    fn interface_boundary_tier_defaults() {
        let small =
            default_settings_for_recipe(LspWorkflowRecipe::InterfaceBoundary, ModelTier::Small);
        assert_eq!(small.max_files, 1);
        assert!(!small.include_references);

        let workhorse =
            default_settings_for_recipe(LspWorkflowRecipe::InterfaceBoundary, ModelTier::Workhorse);
        assert!(workhorse.include_references);
    }

    #[test]
    fn cross_file_repair_tier_defaults() {
        let small =
            default_settings_for_recipe(LspWorkflowRecipe::CrossFileRepair, ModelTier::Small);
        assert_eq!(small.max_files, 1);
        assert!(!small.include_references);

        let frontier =
            default_settings_for_recipe(LspWorkflowRecipe::CrossFileRepair, ModelTier::Frontier);
        assert_eq!(frontier.max_files, 5);
        assert!(frontier.include_references);
    }

    #[test]
    fn call_neighborhood_tier_defaults() {
        let small =
            default_settings_for_recipe(LspWorkflowRecipe::CallNeighborhood, ModelTier::Small);
        assert_eq!(small.max_files, 1);
        assert!(!small.include_references);

        let workhorse =
            default_settings_for_recipe(LspWorkflowRecipe::CallNeighborhood, ModelTier::Workhorse);
        assert!(workhorse.include_references);
    }

    #[test]
    fn settings_to_budget() {
        let s = RecipeSettings {
            max_files: 5,
            max_ranges_per_file: 3,
            max_diagnostics: 10,
            max_references: 15,
            max_symbols: 20,
            ..Default::default()
        };
        let b = s.to_budget();
        assert_eq!(b.max_files, 5);
        assert_eq!(b.max_ranges_per_file, 3);
        assert_eq!(b.max_diagnostics, 10);
        assert_eq!(b.max_references, 15);
        assert_eq!(b.max_symbols, 20);
    }

    #[test]
    fn settings_to_render_config() {
        let s = RecipeSettings::small_tier();
        let c = s.to_render_config();
        assert_eq!(c.model_tier, ModelTier::Small);
        assert_eq!(c.max_bytes_per_section, 1000);
        assert!(!c.include_previews);

        let f = RecipeSettings::frontier_tier();
        let cf = f.to_render_config();
        assert_eq!(cf.model_tier, ModelTier::Frontier);
        assert_eq!(cf.max_bytes_per_section, 4000);
        assert!(cf.include_previews);
    }

    #[test]
    fn default_settings_for_each_recipe() {
        for recipe in [
            LspWorkflowRecipe::RepairLocal,
            LspWorkflowRecipe::RepairHunk,
            LspWorkflowRecipe::ReviewFile,
            LspWorkflowRecipe::ReviewDiff,
            LspWorkflowRecipe::SecurityReviewEnriched,
            LspWorkflowRecipe::HunkSourceNavigation,
            LspWorkflowRecipe::PreviewSuggestion,
        ] {
            for tier in [ModelTier::Small, ModelTier::Workhorse, ModelTier::Frontier] {
                let settings = default_settings_for_recipe(recipe, tier);
                assert_eq!(settings.model_tier, tier);
            }
        }
    }

    #[test]
    fn security_review_enriched_uses_aggressive_risk() {
        let settings = default_settings_for_recipe(
            LspWorkflowRecipe::SecurityReviewEnriched,
            ModelTier::Workhorse,
        );
        assert_eq!(settings.risk_mode, LspRiskMode::Aggressive);
        assert!(!settings.include_preview_hints);
    }

    #[test]
    fn review_diff_scales_risk_with_tier() {
        let small = default_settings_for_recipe(LspWorkflowRecipe::ReviewDiff, ModelTier::Small);
        assert_eq!(small.risk_mode, LspRiskMode::Conservative);

        let workhorse =
            default_settings_for_recipe(LspWorkflowRecipe::ReviewDiff, ModelTier::Workhorse);
        assert_eq!(workhorse.risk_mode, LspRiskMode::Standard);

        let frontier =
            default_settings_for_recipe(LspWorkflowRecipe::ReviewDiff, ModelTier::Frontier);
        assert_eq!(frontier.risk_mode, LspRiskMode::Aggressive);
    }

    #[test]
    fn recipe_outcome_fields() {
        let packet = LspContextPacket {
            request: LspContextRequest::File {
                file: PathBuf::from("test.rs"),
                line_ranges: Vec::new(),
                include_symbols: false,
                include_diagnostics: true,
            },
            items: Vec::new(),
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
            truncation: LspContextTruncation::default(),
        };
        let settings = RecipeSettings::default();
        let outcome =
            finish_recipe_outcome(LspWorkflowRecipe::RepairLocal, packet, &settings).unwrap();
        assert_eq!(outcome.recipe, LspWorkflowRecipe::RepairLocal);
        assert!(!outcome.fallback_used);
        assert_eq!(outcome.freshness_summary, "evidence is fresh");
    }

    #[test]
    fn recipe_outcome_marks_fallback_when_disabled() {
        let packet = LspContextPacket {
            request: LspContextRequest::File {
                file: PathBuf::from("test.rs"),
                line_ranges: Vec::new(),
                include_symbols: false,
                include_diagnostics: true,
            },
            items: Vec::new(),
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Disabled,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: LspContextTruncation::default(),
        };
        let settings = RecipeSettings::default();
        let outcome =
            finish_recipe_outcome(LspWorkflowRecipe::ReviewFile, packet, &settings).unwrap();
        assert!(outcome.fallback_used);
    }

    // -----------------------------------------------------------------------
    // Async integration tests — exercise execute_* paths with mock providers
    // -----------------------------------------------------------------------

    use crate::evidence_collector::{LspContextError, LspEvidenceProvider};
    use crate::LspError;
    use async_trait::async_trait;

    /// Mock provider that returns diagnostics but fails on symbols/defs/refs
    /// (degraded LSP: some operations succeed, others error).
    struct DegradedProvider;

    #[async_trait]
    impl LspEvidenceProvider for DegradedProvider {
        async fn diagnostics_for_file(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(vec![(
                "warning".into(),
                "unused import".into(),
                "(2:0)-(2:10)".into(),
            )])
        }
        async fn document_symbols(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn go_to_definition(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn find_references(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn implementations(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn hover(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Option<String>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn document_highlights(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<String>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn signature_help(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn completion(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn semantic_tokens(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn workspace_symbols(
            &self,
            _: &str,
        ) -> Result<Vec<(String, String, String, String)>, LspError> {
            Err(LspError::NotInitialized("degraded".into()))
        }
        async fn operational_state(&self) -> String {
            "degraded".to_string()
        }
        async fn server_info(&self) -> (Option<String>, Option<u64>) {
            (Some("degraded-server".into()), Some(1))
        }
    }

    #[tokio::test]
    async fn test_repair_local_degraded_lsp() {
        let request = RepairLocalRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 10,
            column: 0,
            diagnostic_message: None,
            settings: RecipeSettings::default(),
        };

        let outcome = execute_repair_local(&DegradedProvider, &request)
            .await
            .unwrap();

        // Degraded provider still returns diagnostics, so items should exist.
        assert!(
            !outcome.packet.items.is_empty(),
            "degraded provider should still return diagnostic items"
        );

        // Fallback should be detected because operational notes are present
        // from failed symbol/def/ref operations.
        let op_notes: Vec<_> = outcome
            .packet
            .items
            .iter()
            .filter(|i| i.kind == crate::context::LspContextItemKind::OperationalNote)
            .collect();
        assert!(
            !op_notes.is_empty(),
            "expected operational notes for failed operations"
        );
        assert!(outcome.fallback_used);
        assert!(outcome.notes.iter().any(|n| n.contains("fallback")));
    }

    /// Mock provider that returns nothing — simulates LSP completely unavailable.
    struct UnavailProvider;

    #[async_trait]
    impl LspEvidenceProvider for UnavailProvider {
        async fn diagnostics_for_file(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn document_symbols(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn go_to_definition(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn find_references(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn implementations(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn hover(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Option<String>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn document_highlights(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<String>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn signature_help(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn completion(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn semantic_tokens(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn workspace_symbols(
            &self,
            _: &str,
        ) -> Result<Vec<(String, String, String, String)>, LspError> {
            Err(LspError::NotInitialized("no server".into()))
        }
        async fn operational_state(&self) -> String {
            "failed".to_string()
        }
        async fn server_info(&self) -> (Option<String>, Option<u64>) {
            (None, None)
        }
    }

    #[tokio::test]
    async fn test_repair_local_disabled_mode_fallback() {
        let request = RepairLocalRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 10,
            column: 0,
            diagnostic_message: None,
            settings: RecipeSettings {
                mode: LspContextMode::Disabled,
                ..Default::default()
            },
        };

        let outcome = execute_repair_local(&UnavailProvider, &request)
            .await
            .unwrap();

        // Disabled mode always produces fallback.
        assert!(outcome.fallback_used);
        assert!(outcome.notes.iter().any(|n| n.contains("fallback")));
    }

    #[tokio::test]
    async fn test_repair_local_required_mode_fails_when_unavailable() {
        let request = RepairLocalRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 10,
            column: 0,
            diagnostic_message: None,
            settings: RecipeSettings {
                mode: LspContextMode::Required,
                ..Default::default()
            },
        };

        let result = execute_repair_local(&UnavailProvider, &request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            LspContextError::RequiredFailed(_) => {}
            other => panic!("expected RequiredFailed, got {other:?}"),
        }
    }

    /// Mock provider with state="indexing" that yields PossiblyStale freshness.
    struct StaleProvider;

    #[async_trait]
    impl LspEvidenceProvider for StaleProvider {
        async fn diagnostics_for_file(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(vec![(
                "error".into(),
                "type mismatch".into(),
                "(5:0)-(5:20)".into(),
            )])
        }
        async fn document_symbols(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(vec![])
        }
        async fn go_to_definition(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(vec![])
        }
        async fn find_references(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(vec![])
        }
        async fn implementations(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(vec![])
        }
        async fn hover(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Option<String>, LspError> {
            Ok(None)
        }
        async fn document_highlights(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<String>, LspError> {
            Ok(vec![])
        }
        async fn signature_help(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(vec![])
        }
        async fn completion(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(vec![])
        }
        async fn semantic_tokens(
            &self,
            _: &std::path::Path,
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
            // "indexing" maps to PossiblyStale via freshness_for_state.
            "indexing".to_string()
        }
        async fn server_info(&self) -> (Option<String>, Option<u64>) {
            (Some("stale-server".into()), Some(2))
        }
    }

    #[tokio::test]
    async fn test_repair_hunk_stale_diagnostics() {
        let request = RepairHunkRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            hunks: vec![crate::context::HunkRange {
                start: 0,
                end: 20,
                original_start: None,
                original_end: None,
            }],
            settings: RecipeSettings {
                include_references: false,
                include_definitions: false,
                ..Default::default()
            },
        };

        let outcome = execute_repair_hunk(&StaleProvider, &request).await.unwrap();

        // The diagnostic from the stale provider should have PossiblyStale freshness.
        let diag = outcome
            .packet
            .items
            .iter()
            .find(|i| i.kind == crate::context::LspContextItemKind::Diagnostic)
            .expect("should have diagnostic");
        assert!(
            matches!(
                diag.provenance.freshness,
                crate::context::LspEvidenceFreshness::PossiblyStale
            ),
            "expected PossiblyStale, got {:?}",
            diag.provenance.freshness
        );

        // Freshness summary should reflect staleness.
        assert_eq!(outcome.freshness_summary, "some evidence may be stale");
    }

    #[tokio::test]
    async fn test_review_diff_tier_defaults() {
        let small = default_settings_for_recipe(LspWorkflowRecipe::ReviewDiff, ModelTier::Small);
        assert_eq!(small.risk_mode, LspRiskMode::Conservative);
        assert!(!small.include_references);

        let workhorse =
            default_settings_for_recipe(LspWorkflowRecipe::ReviewDiff, ModelTier::Workhorse);
        assert_eq!(workhorse.risk_mode, LspRiskMode::Standard);
        assert!(workhorse.include_references);

        let frontier =
            default_settings_for_recipe(LspWorkflowRecipe::ReviewDiff, ModelTier::Frontier);
        assert_eq!(frontier.risk_mode, LspRiskMode::Aggressive);
        assert!(frontier.include_references);
        assert!(frontier.include_preview_hints);
    }

    #[tokio::test]
    async fn test_security_review_enriched_always_aggressive() {
        let request = SecurityReviewEnrichedRequest {
            changed_files: vec![std::path::PathBuf::from("src/auth.rs")],
            hunks: vec![crate::hunk_context::HunkDescriptor {
                id: "h1".into(),
                file_path: "src/auth.rs".into(),
                old_range: Some(crate::hunk_context::HunkLineRange {
                    start_line: 10,
                    end_line: 20,
                }),
                new_range: Some(crate::hunk_context::HunkLineRange {
                    start_line: 10,
                    end_line: 25,
                }),
                header: None,
                added_lines: 5,
                removed_lines: 0,
                context_lines: 3,
            }],
            settings: RecipeSettings::small_tier(), // small tier, but recipe forces aggressive
        };

        let outcome = execute_security_review_enriched(&DegradedProvider, &request)
            .await
            .unwrap();

        // Verify security-specific note is present.
        assert!(outcome
            .notes
            .iter()
            .any(|n| n.contains("security_review_enriched")));

        // The recipe should have forced Aggressive risk mode internally
        // (verified via the packet's request having Aggressive risk).
        match &outcome.packet.request {
            LspContextRequest::Review { risk_mode, .. } => {
                assert_eq!(*risk_mode, LspRiskMode::Aggressive);
            }
            other => panic!("expected Review request, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_hunk_source_navigation_tags_items_with_hunk_source() {
        let request = HunkSourceNavigationRecipeRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            hunks: vec![crate::hunk_context::HunkDescriptor {
                id: "h1".into(),
                file_path: "src/main.rs".into(),
                old_range: Some(crate::hunk_context::HunkLineRange {
                    start_line: 1,
                    end_line: 5,
                }),
                new_range: Some(crate::hunk_context::HunkLineRange {
                    start_line: 1,
                    end_line: 8,
                }),
                header: None,
                added_lines: 3,
                removed_lines: 0,
                context_lines: 2,
            }],
            intent: "navigate hunk for review".to_string(),
            settings: RecipeSettings::default(),
        };

        let outcome = execute_hunk_source_navigation(&StaleProvider, &request)
            .await
            .unwrap();

        // All non-operational-note items should be tagged with Hunk source.
        for item in &outcome.packet.items {
            if item.kind != crate::context::LspContextItemKind::OperationalNote {
                assert_eq!(
                    item.source,
                    Some(AgentContextSource::Hunk),
                    "item {:?} should have Hunk source",
                    item.kind
                );
            }
        }

        // Recipe note should mention the intent.
        assert!(outcome
            .notes
            .iter()
            .any(|n| n.contains("hunk_source_navigation")));
    }

    #[tokio::test]
    async fn test_preview_suggestion_does_not_apply() {
        let request = PreviewSuggestionRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 15,
            column: 0,
            settings: RecipeSettings::default(),
        };

        let outcome = execute_preview_suggestion(&StaleProvider, &request)
            .await
            .unwrap();

        // Preview IDs should be empty — previews are never applied.
        assert!(
            outcome.preview_ids.is_empty(),
            "preview_suggestion must not apply previews"
        );

        // Should have a note stating previews are not applied.
        assert!(outcome.notes.iter().any(|n| n.contains("not applied")));
    }

    #[tokio::test]
    async fn test_review_file_captures_diagnostics() {
        let request = ReviewFileRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line_ranges: vec![],
            settings: RecipeSettings::default(),
        };

        let outcome = execute_review_file(&StaleProvider, &request).await.unwrap();

        // Should contain the diagnostic from StaleProvider.
        let diag = outcome
            .packet
            .items
            .iter()
            .find(|i| i.kind == crate::context::LspContextItemKind::Diagnostic);
        assert!(diag.is_some(), "review_file should capture diagnostics");
    }

    #[tokio::test]
    async fn test_repair_local_frontier_includes_preview_hints() {
        let settings =
            default_settings_for_recipe(LspWorkflowRecipe::RepairLocal, ModelTier::Frontier);
        assert!(settings.include_preview_hints);
        assert!(settings.include_references);
    }

    #[tokio::test]
    async fn test_repair_local_small_omits_references() {
        let settings =
            default_settings_for_recipe(LspWorkflowRecipe::RepairLocal, ModelTier::Small);
        assert!(!settings.include_references);
        assert!(!settings.include_preview_hints);
    }

    #[tokio::test]
    async fn test_impact_analysis_with_degraded_provider() {
        let request = ImpactAnalysisRequest {
            symbol: crate::context::SymbolTarget {
                file: std::path::PathBuf::from("src/lib.rs"),
                position: lsp_types::Position {
                    line: 10,
                    character: 5,
                },
            },
            changed_files: vec![std::path::PathBuf::from("src/a.rs")],
            settings: RecipeSettings::default(),
        };

        let outcome = execute_impact_analysis(&DegradedProvider, &request)
            .await
            .unwrap();

        // Degraded provider still returns diagnostics.
        assert!(!outcome.packet.items.is_empty());
        assert!(outcome.fallback_used);
    }

    #[tokio::test]
    async fn test_impact_analysis_with_unavail_provider() {
        let request = ImpactAnalysisRequest {
            symbol: crate::context::SymbolTarget {
                file: std::path::PathBuf::from("src/lib.rs"),
                position: lsp_types::Position {
                    line: 10,
                    character: 5,
                },
            },
            changed_files: vec![],
            settings: RecipeSettings::default(),
        };

        let outcome = execute_impact_analysis(&UnavailProvider, &request)
            .await
            .unwrap();

        // Unavail provider returns operational notes.
        assert!(outcome.fallback_used);
    }

    #[tokio::test]
    async fn test_test_failure_repair_with_degraded_provider() {
        let request = TestFailureRepairRequest {
            test_file: std::path::PathBuf::from("tests/foo.rs"),
            failure_message: "thread 'test_foo' panicked at 'assertion failed'".to_string(),
            related_files: vec![std::path::PathBuf::from("src/foo.rs")],
            settings: RecipeSettings::default(),
        };

        let outcome = execute_test_failure_repair(&DegradedProvider, &request)
            .await
            .unwrap();

        // Should have diagnostics from the test file.
        assert!(!outcome.packet.items.is_empty());
    }

    #[tokio::test]
    async fn test_interface_boundary_with_degraded_provider() {
        let request = InterfaceBoundaryRequest {
            file: std::path::PathBuf::from("src/api.rs"),
            symbol: None,
            include_implementations: false,
            settings: RecipeSettings::default(),
        };

        let outcome = execute_interface_boundary(&DegradedProvider, &request)
            .await
            .unwrap();

        // Should have operational notes for failed operations.
        assert!(outcome.fallback_used);
    }

    #[tokio::test]
    async fn test_cross_file_repair_with_degraded_provider() {
        let request = CrossFileRepairRequest {
            primary_file: std::path::PathBuf::from("src/main.rs"),
            related_files: vec![std::path::PathBuf::from("src/lib.rs")],
            ranges: vec![],
            settings: RecipeSettings::default(),
        };

        let outcome = execute_cross_file_repair(&DegradedProvider, &request)
            .await
            .unwrap();

        // Degraded provider returns diagnostics.
        assert!(!outcome.packet.items.is_empty());
    }

    #[tokio::test]
    async fn test_call_neighborhood_with_unavail_provider() {
        let request = CallNeighborhoodRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 42,
            column: 10,
            direction: crate::context::HierarchyDirection::Both,
            max_depth: 1,
            settings: RecipeSettings::default(),
        };

        let outcome = execute_call_neighborhood(&UnavailProvider, &request)
            .await
            .unwrap();

        // Unavail provider returns operational notes.
        assert!(outcome.fallback_used);
    }

    #[tokio::test]
    async fn test_impact_analysis_small_tier_caps() {
        let settings =
            default_settings_for_recipe(LspWorkflowRecipe::ImpactAnalysis, ModelTier::Small);
        assert_eq!(settings.max_references, 5);
        assert_eq!(settings.max_files, 1);
    }

    #[tokio::test]
    async fn test_call_neighborhood_direction_incoming() {
        let request = CallNeighborhoodRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 42,
            column: 10,
            direction: crate::context::HierarchyDirection::Incoming,
            max_depth: 1,
            settings: RecipeSettings::default(),
        };

        let outcome = execute_call_neighborhood(&DegradedProvider, &request)
            .await
            .unwrap();

        // Should have operational notes for failed incoming calls.
        assert!(outcome.fallback_used);
    }
}
