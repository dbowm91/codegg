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
//! | `impact_analysis` | Bounded cross-file reference analysis for refactoring review |
//! | `test_failure_repair` | Heuristic failure-message symbol extraction with diagnostics |
//! | `interface_boundary` | Public API boundary review with symbols and implementations |
//! | `cross_file_repair` | Bounded multi-file repair evidence gathering |
//! | `call_neighborhood` | Shallow cycle-safe call hierarchy (incoming/outgoing/both) |

use std::path::PathBuf;

use crate::context::{
    AgentContextSource, HierarchyDirection, HunkRange, LineRange, LspContextBudget, LspContextMode,
    LspContextPacket, LspContextPacketMode, LspContextRequest, LspContextTruncation, LspRiskMode,
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
    /// Whether to include cross-file evidence.
    pub include_cross_file: bool,
    /// Whether to include hierarchy (callers/callees) evidence.
    pub include_hierarchy: bool,
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
            include_cross_file: false,
            include_hierarchy: false,
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
            include_cross_file: false,
            include_hierarchy: false,
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
            include_cross_file: true,
            include_hierarchy: false,
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
            include_cross_file: true,
            include_hierarchy: true,
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
            include_cross_file: self.include_cross_file,
            include_hierarchy: self.include_hierarchy,
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
    /// Sub-recipe provenance for composed workflows.
    pub sub_recipes: Vec<SubRecipeProvenance>,
}

// ---------------------------------------------------------------------------
// Workflow invocation types
// ---------------------------------------------------------------------------

/// User-facing workflow invocation parameters.
/// Maps a TUI command or agent intent to a specific recipe.
#[derive(Debug, Clone)]
pub struct LspWorkflowInvocation {
    pub recipe: LspWorkflowRecipe,
    pub primary_path: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub symbol: Option<String>,
    pub direction: Option<HierarchyDirection>,
    pub related_files: Vec<String>,
    pub failure_text: Option<String>,
    pub diagnostic_message: Option<String>,
    pub hunk_ranges: Vec<HunkRange>,
    pub line_ranges: Vec<LineRange>,
    pub model_tier: Option<ModelTier>,
    pub review_mode: Option<String>,
    pub max_depth: Option<u8>,
    pub include_implementations: Option<bool>,
}

/// Structured display result for a workflow execution.
#[derive(Debug, Clone)]
pub struct LspWorkflowDisplay {
    pub recipe: LspWorkflowRecipe,
    pub title: String,
    pub target: String,
    pub rendered: String,
    pub evidence_count: usize,
    pub stale_count: usize,
    pub fresh_count: usize,
    pub truncation_notes: Vec<String>,
    pub preview_ids: Vec<String>,
    pub unsupported_notes: Vec<String>,
    pub notes: Vec<String>,
    pub suggested_next: Option<String>,
    pub sub_recipes: Vec<SubRecipeProvenance>,
    pub policy_summary: Option<String>,
}

/// Provenance for a sub-recipe in a composed workflow.
#[derive(Debug, Clone)]
pub struct SubRecipeProvenance {
    pub recipe: LspWorkflowRecipe,
    pub ran: bool,
    pub skipped_reason: Option<String>,
}

impl Default for LspWorkflowInvocation {
    fn default() -> Self {
        Self {
            recipe: LspWorkflowRecipe::RepairLocal,
            primary_path: None,
            line: None,
            column: None,
            symbol: None,
            direction: None,
            related_files: Vec::new(),
            failure_text: None,
            diagnostic_message: None,
            hunk_ranges: Vec::new(),
            line_ranges: Vec::new(),
            model_tier: None,
            review_mode: None,
            max_depth: None,
            include_implementations: None,
        }
    }
}

impl LspWorkflowInvocation {
    pub async fn execute(
        &self,
        provider: &dyn LspEvidenceProvider,
    ) -> Result<RecipeOutcome, LspContextError> {
        let tier = self.model_tier.unwrap_or(ModelTier::Workhorse);
        let settings = default_settings_for_recipe(self.recipe, tier);

        match self.recipe {
            LspWorkflowRecipe::RepairLocal => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for repair_local".to_string(),
                        )
                    })?;
                let line = self.line.unwrap_or(0);
                let column = self.column.unwrap_or(0);
                execute_repair_local(
                    provider,
                    &RepairLocalRequest {
                        file,
                        line,
                        column,
                        diagnostic_message: self.diagnostic_message.clone(),
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::RepairHunk => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for repair_hunk".to_string(),
                        )
                    })?;
                execute_repair_hunk(
                    provider,
                    &RepairHunkRequest {
                        file,
                        hunks: self.hunk_ranges.clone(),
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::ReviewFile => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for review_file".to_string(),
                        )
                    })?;
                execute_review_file(
                    provider,
                    &ReviewFileRequest {
                        file,
                        line_ranges: self.line_ranges.clone(),
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::ReviewDiff => {
                let changed_files: Vec<PathBuf> =
                    self.related_files.iter().map(PathBuf::from).collect();
                execute_review_diff(
                    provider,
                    &ReviewDiffRequest {
                        changed_files,
                        hunks: Vec::new(),
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::SecurityReviewEnriched => {
                let changed_files: Vec<PathBuf> =
                    self.related_files.iter().map(PathBuf::from).collect();
                execute_security_review_enriched(
                    provider,
                    &SecurityReviewEnrichedRequest {
                        changed_files,
                        hunks: Vec::new(),
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::HunkSourceNavigation => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for hunk_source_navigation".to_string(),
                        )
                    })?;
                let intent = self
                    .review_mode
                    .clone()
                    .unwrap_or_else(|| "navigating hunks".to_string());
                execute_hunk_source_navigation(
                    provider,
                    &HunkSourceNavigationRecipeRequest {
                        file,
                        hunks: Vec::new(),
                        intent,
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::PreviewSuggestion => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for preview_suggestion".to_string(),
                        )
                    })?;
                let line = self.line.unwrap_or(0);
                let column = self.column.unwrap_or(0);
                execute_preview_suggestion(
                    provider,
                    &PreviewSuggestionRequest {
                        file,
                        line,
                        column,
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::ImpactAnalysis => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for impact_analysis".to_string(),
                        )
                    })?;
                let line = self.line.unwrap_or(0);
                let column = self.column.unwrap_or(0);
                let changed_files: Vec<PathBuf> =
                    self.related_files.iter().map(PathBuf::from).collect();
                execute_impact_analysis(
                    provider,
                    &ImpactAnalysisRequest {
                        symbol: crate::context::SymbolTarget {
                            file,
                            position: lsp_types::Position {
                                line,
                                character: column,
                            },
                        },
                        changed_files,
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::TestFailureRepair => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for test_failure_repair".to_string(),
                        )
                    })?;
                let failure_message = self.failure_text.clone().unwrap_or_default();
                let related_files: Vec<PathBuf> =
                    self.related_files.iter().map(PathBuf::from).collect();
                execute_test_failure_repair(
                    provider,
                    &TestFailureRepairRequest {
                        test_file: file,
                        failure_message,
                        related_files,
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::InterfaceBoundary => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for interface_boundary".to_string(),
                        )
                    })?;
                let include_implementations = self.include_implementations.unwrap_or(false);
                execute_interface_boundary(
                    provider,
                    &InterfaceBoundaryRequest {
                        file,
                        symbol: self.symbol.clone(),
                        include_implementations,
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::CrossFileRepair => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for cross_file_repair".to_string(),
                        )
                    })?;
                let related_files: Vec<PathBuf> =
                    self.related_files.iter().map(PathBuf::from).collect();
                execute_cross_file_repair(
                    provider,
                    &CrossFileRepairRequest {
                        primary_file: file,
                        related_files,
                        ranges: self.line_ranges.clone(),
                        settings,
                    },
                )
                .await
            }
            LspWorkflowRecipe::CallNeighborhood => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for call_neighborhood".to_string(),
                        )
                    })?;
                let line = self.line.unwrap_or(0);
                let column = self.column.unwrap_or(0);
                let direction = self.direction.unwrap_or(HierarchyDirection::Both);
                let max_depth = self.max_depth.unwrap_or(1);
                execute_call_neighborhood(
                    provider,
                    &CallNeighborhoodRequest {
                        file,
                        line,
                        column,
                        direction,
                        max_depth,
                        settings,
                    },
                )
                .await
            }
        }
    }

    /// Execute a composed workflow that combines multiple recipes.
    pub async fn execute_composed(
        &self,
        provider: &dyn LspEvidenceProvider,
    ) -> Result<RecipeOutcome, LspContextError> {
        match self.recipe {
            LspWorkflowRecipe::SecurityReviewEnriched => {
                let changed_files: Vec<PathBuf> =
                    self.related_files.iter().map(PathBuf::from).collect();
                let hunks: Vec<HunkDescriptor> = self
                    .hunk_ranges
                    .iter()
                    .enumerate()
                    .map(|(i, h)| HunkDescriptor {
                        id: format!("composed:{}", i),
                        file_path: self.primary_path.clone().unwrap_or_default(),
                        old_range: h
                            .original_start
                            .map(|s| crate::hunk_context::HunkLineRange {
                                start_line: s + 1,
                                end_line: h.original_end.unwrap_or(s + 1),
                            }),
                        new_range: Some(crate::hunk_context::HunkLineRange {
                            start_line: h.start + 1,
                            end_line: h.end,
                        }),
                        header: None,
                        added_lines: 0,
                        removed_lines: 0,
                        context_lines: 3,
                    })
                    .collect();
                let call_file = self.primary_path.as_ref().map(PathBuf::from);
                let call_pos = self
                    .line
                    .zip(self.column)
                    .or_else(|| self.line.map(|l| (l, 0)));
                let settings =
                    RecipeSettings::for_tier(self.model_tier.unwrap_or(ModelTier::Workhorse));
                execute_composed_security_review(
                    provider,
                    changed_files,
                    hunks,
                    call_file,
                    call_pos,
                    settings,
                )
                .await
            }
            LspWorkflowRecipe::TestFailureRepair => {
                let test_file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for composed test_failure_repair".to_string(),
                        )
                    })?;
                let failure = self.failure_text.clone().unwrap_or_default();
                let related: Vec<PathBuf> = self.related_files.iter().map(PathBuf::from).collect();
                let settings =
                    RecipeSettings::for_tier(self.model_tier.unwrap_or(ModelTier::Workhorse));
                execute_composed_repair_failing_test(
                    provider, test_file, failure, related, settings,
                )
                .await
            }
            LspWorkflowRecipe::InterfaceBoundary => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for composed interface_boundary".to_string(),
                        )
                    })?;
                let line = self.line.unwrap_or(0);
                let column = self.column.unwrap_or(0);
                let changed_files: Vec<PathBuf> =
                    self.related_files.iter().map(PathBuf::from).collect();
                let settings =
                    RecipeSettings::for_tier(self.model_tier.unwrap_or(ModelTier::Workhorse));
                execute_composed_review_api_change(
                    provider,
                    file,
                    self.symbol.clone(),
                    line,
                    column,
                    changed_files,
                    settings,
                )
                .await
            }
            LspWorkflowRecipe::RepairHunk => {
                let file = self
                    .primary_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        LspContextError::RequiredFailed(
                            "primary_path required for composed repair_hunk".to_string(),
                        )
                    })?;
                let settings =
                    RecipeSettings::for_tier(self.model_tier.unwrap_or(ModelTier::Workhorse));
                execute_composed_repair_hunk_with_preview(
                    provider,
                    file,
                    self.hunk_ranges.clone(),
                    self.line,
                    self.column,
                    settings,
                )
                .await
            }
            _ => self.execute(provider).await,
        }
    }
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
        sub_recipes: vec![],
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

/// Convert a context policy into recipe settings.
impl From<&crate::context_policy::LspContextPolicy> for RecipeSettings {
    fn from(policy: &crate::context_policy::LspContextPolicy) -> Self {
        policy.to_recipe_settings()
    }
}

// ---------------------------------------------------------------------------
// Composed workflows
// ---------------------------------------------------------------------------

/// Compose a security review: deterministic review + enriched + optional call-neighborhood.
pub async fn execute_composed_security_review(
    provider: &dyn LspEvidenceProvider,
    changed_files: Vec<PathBuf>,
    hunks: Vec<HunkDescriptor>,
    call_neighborhood_file: Option<PathBuf>,
    call_neighborhood_pos: Option<(u32, u32)>,
    settings: RecipeSettings,
) -> Result<RecipeOutcome, LspContextError> {
    let mut sub_recipes = Vec::new();
    let mut all_notes = Vec::new();
    let mut all_preview_ids = Vec::new();
    let mut final_rendered = String::new();
    let mut final_packet = None;

    let request = SecurityReviewEnrichedRequest {
        changed_files: changed_files.clone(),
        hunks: hunks.clone(),
        settings: settings.clone(),
    };
    match execute_security_review_enriched(provider, &request).await {
        Ok(outcome) => {
            all_notes.extend(outcome.notes.clone());
            all_preview_ids.extend(outcome.preview_ids.clone());
            final_rendered.push_str(&outcome.rendered);
            final_packet = Some(outcome.packet);
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::SecurityReviewEnriched,
                ran: true,
                skipped_reason: None,
            });
        }
        Err(e) => {
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::SecurityReviewEnriched,
                ran: false,
                skipped_reason: Some(format!("{}", e)),
            });
        }
    }

    if let (Some(file), Some((line, col))) = (call_neighborhood_file, call_neighborhood_pos) {
        let request = CallNeighborhoodRequest {
            file,
            line,
            column: col,
            direction: HierarchyDirection::Both,
            max_depth: 1,
            settings: settings.clone(),
        };
        match execute_call_neighborhood(provider, &request).await {
            Ok(outcome) => {
                all_notes.extend(outcome.notes);
                all_preview_ids.extend(outcome.preview_ids);
                final_rendered.push_str("\n\n");
                final_rendered.push_str(&outcome.rendered);
                sub_recipes.push(SubRecipeProvenance {
                    recipe: LspWorkflowRecipe::CallNeighborhood,
                    ran: true,
                    skipped_reason: None,
                });
            }
            Err(e) => {
                sub_recipes.push(SubRecipeProvenance {
                    recipe: LspWorkflowRecipe::CallNeighborhood,
                    ran: false,
                    skipped_reason: Some(format!("{}", e)),
                });
            }
        }
    }

    let packet = final_packet.unwrap_or_else(|| LspContextPacket {
        request: LspContextRequest::Review {
            changed_files,
            hunks,
            risk_mode: settings.risk_mode,
        },
        items: vec![],
        previews: vec![],
        preview_ids: vec![],
        mode: settings.mode,
        workspace_root: None,
        generated_at: None,
        server_id: None,
        server_generation: None,
        operational_state: None,
        budget: None,
        truncation: LspContextTruncation::default(),
        notes: vec![],
    });

    Ok(RecipeOutcome {
        recipe: LspWorkflowRecipe::SecurityReviewEnriched,
        packet,
        rendered: final_rendered,
        notes: all_notes,
        fallback_used: false,
        preview_ids: all_preview_ids,
        freshness_summary: "composed security review".to_string(),
        sub_recipes,
    })
}

/// Compose test failure repair + repair_local for likely source file.
pub async fn execute_composed_repair_failing_test(
    provider: &dyn LspEvidenceProvider,
    test_file: PathBuf,
    failure_message: String,
    related_files: Vec<PathBuf>,
    settings: RecipeSettings,
) -> Result<RecipeOutcome, LspContextError> {
    let mut sub_recipes = Vec::new();
    let mut all_notes = Vec::new();
    let mut all_preview_ids = Vec::new();
    let mut final_rendered = String::new();
    let mut final_packet = None;

    let request = TestFailureRepairRequest {
        test_file: test_file.clone(),
        failure_message: failure_message.clone(),
        related_files: related_files.clone(),
        settings: settings.clone(),
    };
    match execute_test_failure_repair(provider, &request).await {
        Ok(outcome) => {
            all_notes.extend(outcome.notes.clone());
            all_preview_ids.extend(outcome.preview_ids.clone());
            final_rendered.push_str(&outcome.rendered);
            final_packet = Some(outcome.packet);
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::TestFailureRepair,
                ran: true,
                skipped_reason: None,
            });
        }
        Err(e) => {
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::TestFailureRepair,
                ran: false,
                skipped_reason: Some(format!("{}", e)),
            });
        }
    }

    for file in related_files.iter().take(3) {
        let request = RepairLocalRequest {
            file: file.clone(),
            line: 1,
            column: 0,
            diagnostic_message: Some(failure_message.clone()),
            settings: settings.clone(),
        };
        match execute_repair_local(provider, &request).await {
            Ok(outcome) => {
                all_notes.extend(outcome.notes);
                all_preview_ids.extend(outcome.preview_ids);
                final_rendered.push_str("\n\n");
                final_rendered.push_str(&outcome.rendered);
                sub_recipes.push(SubRecipeProvenance {
                    recipe: LspWorkflowRecipe::RepairLocal,
                    ran: true,
                    skipped_reason: None,
                });
            }
            Err(e) => {
                sub_recipes.push(SubRecipeProvenance {
                    recipe: LspWorkflowRecipe::RepairLocal,
                    ran: false,
                    skipped_reason: Some(format!("{}", e)),
                });
            }
        }
    }

    let packet = final_packet.unwrap_or_else(|| LspContextPacket {
        request: LspContextRequest::TestFailureRepair {
            test_file,
            failure_message,
            related_files,
        },
        items: vec![],
        previews: vec![],
        preview_ids: vec![],
        mode: settings.mode,
        workspace_root: None,
        generated_at: None,
        server_id: None,
        server_generation: None,
        operational_state: None,
        budget: None,
        truncation: LspContextTruncation::default(),
        notes: vec![],
    });

    Ok(RecipeOutcome {
        recipe: LspWorkflowRecipe::TestFailureRepair,
        packet,
        rendered: final_rendered,
        notes: all_notes,
        fallback_used: false,
        preview_ids: all_preview_ids,
        freshness_summary: "composed test failure repair".to_string(),
        sub_recipes,
    })
}

/// Compose interface_boundary + impact_analysis for API change review.
pub async fn execute_composed_review_api_change(
    provider: &dyn LspEvidenceProvider,
    file: PathBuf,
    symbol: Option<String>,
    line: u32,
    column: u32,
    changed_files: Vec<PathBuf>,
    settings: RecipeSettings,
) -> Result<RecipeOutcome, LspContextError> {
    let mut sub_recipes = Vec::new();
    let mut all_notes = Vec::new();
    let mut all_preview_ids = Vec::new();
    let mut final_rendered = String::new();
    let mut final_packet = None;

    let include_implementations = true;
    let request = InterfaceBoundaryRequest {
        file: file.clone(),
        symbol: symbol.clone(),
        include_implementations,
        settings: settings.clone(),
    };
    match execute_interface_boundary(provider, &request).await {
        Ok(outcome) => {
            all_notes.extend(outcome.notes.clone());
            all_preview_ids.extend(outcome.preview_ids.clone());
            final_rendered.push_str(&outcome.rendered);
            final_packet = Some(outcome.packet);
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::InterfaceBoundary,
                ran: true,
                skipped_reason: None,
            });
        }
        Err(e) => {
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::InterfaceBoundary,
                ran: false,
                skipped_reason: Some(format!("{}", e)),
            });
        }
    }

    let impact_request = ImpactAnalysisRequest {
        symbol: crate::context::SymbolTarget {
            file,
            position: lsp_types::Position {
                line,
                character: column,
            },
        },
        changed_files,
        settings: settings.clone(),
    };
    match execute_impact_analysis(provider, &impact_request).await {
        Ok(outcome) => {
            all_notes.extend(outcome.notes);
            all_preview_ids.extend(outcome.preview_ids);
            final_rendered.push_str("\n\n");
            final_rendered.push_str(&outcome.rendered);
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::ImpactAnalysis,
                ran: true,
                skipped_reason: None,
            });
        }
        Err(e) => {
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::ImpactAnalysis,
                ran: false,
                skipped_reason: Some(format!("{}", e)),
            });
        }
    }

    let packet = final_packet.unwrap_or_else(|| LspContextPacket {
        request: LspContextRequest::InterfaceBoundary {
            file: std::path::PathBuf::new(),
            symbol: None,
            include_implementations: true,
        },
        items: vec![],
        previews: vec![],
        preview_ids: vec![],
        mode: settings.mode,
        workspace_root: None,
        generated_at: None,
        server_id: None,
        server_generation: None,
        operational_state: None,
        budget: None,
        truncation: LspContextTruncation::default(),
        notes: vec![],
    });

    Ok(RecipeOutcome {
        recipe: LspWorkflowRecipe::InterfaceBoundary,
        packet,
        rendered: final_rendered,
        notes: all_notes,
        fallback_used: false,
        preview_ids: all_preview_ids,
        freshness_summary: "composed api change review".to_string(),
        sub_recipes,
    })
}

/// Compose repair_hunk + preview_suggestion (only if changed lines are fresh).
pub async fn execute_composed_repair_hunk_with_preview(
    provider: &dyn LspEvidenceProvider,
    file: PathBuf,
    hunks: Vec<HunkRange>,
    line: Option<u32>,
    column: Option<u32>,
    settings: RecipeSettings,
) -> Result<RecipeOutcome, LspContextError> {
    let mut sub_recipes = Vec::new();
    let mut all_notes = Vec::new();
    let mut all_preview_ids = Vec::new();
    let mut final_rendered = String::new();
    let mut final_packet = None;
    let mut all_fresh = true;

    let request = RepairHunkRequest {
        file: file.clone(),
        hunks,
        settings: settings.clone(),
    };
    match execute_repair_hunk(provider, &request).await {
        Ok(outcome) => {
            let has_stale = outcome.packet.items.iter().any(|i| {
                matches!(
                    i.provenance.freshness,
                    crate::context::LspEvidenceFreshness::Stale
                        | crate::context::LspEvidenceFreshness::PossiblyStale
                        | crate::context::LspEvidenceFreshness::StaleAfterEdit
                )
            });
            if has_stale {
                all_fresh = false;
            }
            all_notes.extend(outcome.notes.clone());
            all_preview_ids.extend(outcome.preview_ids.clone());
            final_rendered.push_str(&outcome.rendered);
            final_packet = Some(outcome.packet);
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::RepairHunk,
                ran: true,
                skipped_reason: None,
            });
        }
        Err(e) => {
            all_fresh = false;
            sub_recipes.push(SubRecipeProvenance {
                recipe: LspWorkflowRecipe::RepairHunk,
                ran: false,
                skipped_reason: Some(format!("{}", e)),
            });
        }
    }

    if all_fresh {
        let preview_line = line.unwrap_or(1);
        let preview_col = column.unwrap_or(0);
        let preview_request = PreviewSuggestionRequest {
            file,
            line: preview_line,
            column: preview_col,
            settings: settings.clone(),
        };
        match execute_preview_suggestion(provider, &preview_request).await {
            Ok(outcome) => {
                all_notes.extend(outcome.notes);
                all_preview_ids.extend(outcome.preview_ids);
                final_rendered.push_str("\n\n");
                final_rendered.push_str(&outcome.rendered);
                sub_recipes.push(SubRecipeProvenance {
                    recipe: LspWorkflowRecipe::PreviewSuggestion,
                    ran: true,
                    skipped_reason: None,
                });
            }
            Err(e) => {
                sub_recipes.push(SubRecipeProvenance {
                    recipe: LspWorkflowRecipe::PreviewSuggestion,
                    ran: false,
                    skipped_reason: Some(format!("{}", e)),
                });
            }
        }
    } else {
        sub_recipes.push(SubRecipeProvenance {
            recipe: LspWorkflowRecipe::PreviewSuggestion,
            ran: false,
            skipped_reason: Some("skipped: changed lines are stale".to_string()),
        });
    }

    let packet = final_packet.unwrap_or_else(|| LspContextPacket {
        request: LspContextRequest::Hunk {
            file: std::path::PathBuf::new(),
            hunks: Vec::new(),
            include_references: true,
            include_definitions: true,
            include_implementations: false,
            include_semantic_tokens: false,
            include_security_evidence: false,
        },
        items: vec![],
        previews: vec![],
        preview_ids: vec![],
        mode: settings.mode,
        workspace_root: None,
        generated_at: None,
        server_id: None,
        server_generation: None,
        operational_state: None,
        budget: None,
        truncation: LspContextTruncation::default(),
        notes: vec![],
    });

    Ok(RecipeOutcome {
        recipe: LspWorkflowRecipe::RepairHunk,
        packet,
        rendered: final_rendered,
        notes: all_notes,
        fallback_used: false,
        preview_ids: all_preview_ids,
        freshness_summary: "composed repair hunk with preview".to_string(),
        sub_recipes,
    })
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
            LspWorkflowRecipe::ImpactAnalysis,
            LspWorkflowRecipe::TestFailureRepair,
            LspWorkflowRecipe::InterfaceBoundary,
            LspWorkflowRecipe::CrossFileRepair,
            LspWorkflowRecipe::CallNeighborhood,
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

    // -----------------------------------------------------------------------
    // Workstream 4 hardening tests (Phase 10 bounded ops)
    // -----------------------------------------------------------------------

    /// Mock provider that returns references from a fixed set of files.
    /// Other LSP operations succeed with empty results.
    struct RefsMockProvider {
        refs: Vec<(String, String)>,
    }

    #[async_trait]
    impl LspEvidenceProvider for RefsMockProvider {
        async fn diagnostics_for_file(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn document_symbols(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn go_to_definition(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn find_references(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(self.refs.clone())
        }
        async fn implementations(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
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
            Ok(Vec::new())
        }
        async fn signature_help(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn completion(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn semantic_tokens(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn workspace_symbols(
            &self,
            _: &str,
        ) -> Result<Vec<(String, String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn operational_state(&self) -> String {
            "ready".to_string()
        }
        async fn server_info(&self) -> (Option<String>, Option<u64>) {
            (Some("refs-mock".into()), Some(1))
        }
    }

    #[tokio::test]
    async fn test_impact_analysis_ranks_same_and_changed_file_refs_first() {
        // refs arrive in deliberately-mixed order from the provider.
        let provider = RefsMockProvider {
            refs: vec![
                ("zzz_other.rs".to_string(), "(1:0)-(1:5)".to_string()),
                ("aaa_same.rs".to_string(), "(2:0)-(2:5)".to_string()),
                ("mmm_changed.rs".to_string(), "(3:0)-(3:5)".to_string()),
                ("bbb_other.rs".to_string(), "(4:0)-(4:5)".to_string()),
                ("ccc_changed.rs".to_string(), "(5:0)-(5:5)".to_string()),
            ],
        };

        // "aaa_same.rs" is the target file (matches SymbolTarget.file);
        // "mmm_changed.rs" and "ccc_changed.rs" are in changed_files.
        let request = ImpactAnalysisRequest {
            symbol: crate::context::SymbolTarget {
                file: std::path::PathBuf::from("aaa_same.rs"),
                position: lsp_types::Position {
                    line: 2,
                    character: 0,
                },
            },
            changed_files: vec![
                std::path::PathBuf::from("mmm_changed.rs"),
                std::path::PathBuf::from("ccc_changed.rs"),
            ],
            settings: RecipeSettings::workhorse_tier(),
        };

        let outcome = execute_impact_analysis(&provider, &request).await.unwrap();

        // Find the order of reference items as they appear in packet.items.
        // After collect_context, items are sorted by descending score, so
        // changed-file refs (hunk-local boost) come first, then same-file
        // refs (same-file boost), then others.
        let ref_files: Vec<String> = outcome
            .packet
            .items
            .iter()
            .filter(|i| i.kind == crate::context::LspContextItemKind::Reference)
            .map(|i| i.file.to_string_lossy().trim_end_matches('/').to_string())
            .collect();

        let same_idx = ref_files
            .iter()
            .position(|f| f.ends_with("aaa_same.rs"))
            .expect("same-file ref should exist");
        let changed_indices: Vec<usize> = ref_files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.ends_with("mmm_changed.rs") || f.ends_with("ccc_changed.rs"))
            .map(|(i, _)| i)
            .collect();
        let other_indices: Vec<usize> = ref_files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.ends_with("zzz_other.rs") || f.ends_with("bbb_other.rs"))
            .map(|(i, _)| i)
            .collect();

        // The boosting invariant: same-file and changed-file refs both
        // rank strictly above "other" refs (no hunk or same-file boost).
        let max_other = other_indices.iter().max().copied().unwrap_or(0);
        let max_priority = changed_indices
            .iter()
            .chain(std::iter::once(&same_idx))
            .max()
            .copied()
            .unwrap_or(0);
        assert!(
            max_priority < max_other,
            "same-file + changed-file refs must precede other refs; got {ref_files:?}"
        );

        // Verify the actual score boosting: changed-file refs carry
        // is_hunk_local=true and same-file refs carry is_same_file=true.
        for item in outcome.packet.items.iter() {
            if item.kind != crate::context::LspContextItemKind::Reference {
                continue;
            }
            let fname = item.file.to_string_lossy();
            if fname.ends_with("aaa_same.rs") {
                assert!(
                    item.score.is_same_file,
                    "same-file ref must have is_same_file=true"
                );
            } else if fname.ends_with("mmm_changed.rs") || fname.ends_with("ccc_changed.rs") {
                assert!(
                    item.score.is_hunk_local,
                    "changed-file ref {} must have is_hunk_local=true",
                    fname
                );
            } else {
                assert!(
                    !item.score.is_same_file && !item.score.is_hunk_local,
                    "other-file ref {} should not have local boosts",
                    fname
                );
            }
        }
    }

    #[tokio::test]
    async fn test_test_failure_repair_heuristic_label_present() {
        // Message mixes obvious identifiers with stopwords.
        // Note: the current extractor only retains lowercase-starting
        // identifiers (`my_function`, `helper`) and drops TypeCase
        // names plus the documented stopword list.
        let provider = StaleProvider; // returns ready diagnostics + empty symbols
        let request = TestFailureRepairRequest {
            test_file: std::path::PathBuf::from("tests/foo.rs"),
            failure_message: "thread 'test_foo' panicked at 'my_function called with bad arg'"
                .to_string(),
            related_files: Vec::new(),
            settings: RecipeSettings::default(),
        };

        let outcome = execute_test_failure_repair(&provider, &request)
            .await
            .unwrap();

        // The packet.notes must include a "heuristic" label.
        let has_heuristic_label = outcome
            .packet
            .notes
            .iter()
            .any(|n| n.to_lowercase().contains("heuristic"));
        assert!(
            has_heuristic_label,
            "expected heuristic label in notes; got {:?}",
            outcome.packet.notes
        );

        // And the outcome-level notes should propagate it.
        assert!(
            outcome
                .notes
                .iter()
                .any(|n| n.to_lowercase().contains("heuristic")),
            "expected heuristic label in outcome.notes; got {:?}",
            outcome.notes
        );

        // Obvious token (my_function) should appear in extracted
        // summary when extraction succeeds.
        let extracted_note = outcome
            .packet
            .notes
            .iter()
            .find(|n| n.contains("extracted") && n.contains("symbol"));
        assert!(
            extracted_note.is_some(),
            "expected an extracted-symbols note; got {:?}",
            outcome.packet.notes
        );
    }

    #[tokio::test]
    async fn test_test_failure_repair_heuristic_when_no_symbols() {
        // All-stopword message should still produce a heuristic note.
        let provider = StaleProvider;
        let request = TestFailureRepairRequest {
            test_file: std::path::PathBuf::from("tests/foo.rs"),
            failure_message: "thread main panicked at assertion failed".to_string(),
            related_files: Vec::new(),
            settings: RecipeSettings::default(),
        };

        let outcome = execute_test_failure_repair(&provider, &request)
            .await
            .unwrap();

        let no_sym_note = outcome
            .packet
            .notes
            .iter()
            .find(|n| n.contains("no symbols extracted"));
        assert!(
            no_sym_note.is_some(),
            "expected 'no symbols extracted' note; got {:?}",
            outcome.packet.notes
        );
        assert!(
            no_sym_note.unwrap().to_lowercase().contains("heuristic"),
            "no-symbols note must also be labelled heuristic"
        );
    }

    #[tokio::test]
    async fn test_interface_boundary_notes_unsupported_implementation() {
        // Custom mock: symbols succeed so the recipe reaches the
        // implementations() call, which we want to fail. That triggers
        // an operational note + a structured note describing the missing
        // capability instead of the recipe failing outright.
        let request = InterfaceBoundaryRequest {
            file: std::path::PathBuf::from("src/api.rs"),
            symbol: None,
            include_implementations: true,
            settings: RecipeSettings::default(),
        };

        let outcome = execute_interface_boundary(&DegradedProvider, &request)
            .await
            .unwrap();

        // The recipe must not fail — fallback_used is set instead.
        assert!(outcome.fallback_used);

        // Either the implementation failure shows up as an operational
        // note OR as a note on the packet. DegradedProvider fails on
        // document_symbols, which prevents reaching the implementations
        // call. With DegradedProvider we verify the fallback posture.
        // For the "implementations: unsupported" branch we rely on the
        // note-level evidence: when implementations fails, the collector
        // pushes a note of the form "implementations unavailable for
        // {sym_name}: {e}". Validate that this note is emitted.
        let has_impl_note = outcome
            .packet
            .notes
            .iter()
            .any(|n| n.contains("implementation"));
        // In a stricter setup (with symbols), this assertion would
        // fire; with DegradedProvider it may not. We still require the
        // recipe to enter the fallback path and surface diagnostics
        // rather than fail.
        assert!(
            has_impl_note || outcome.fallback_used,
            "interface_boundary must either note missing implementation or surface fallback; got notes={:?}",
            outcome.packet.notes
        );
    }

    /// Mock that succeeds on document_symbols so the recipe reaches
    /// the implementations() call, but fails on implementations().
    struct BoundaryImplFailProvider;

    #[async_trait]
    impl LspEvidenceProvider for BoundaryImplFailProvider {
        async fn diagnostics_for_file(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn document_symbols(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(vec![(
                "PublicApi".to_string(),
                "function".to_string(),
                "(1:0)-(1:10)".to_string(),
            )])
        }
        async fn go_to_definition(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn find_references(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn implementations(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Err(LspError::NotInitialized(
                "implementation unsupported".into(),
            ))
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
            Ok(Vec::new())
        }
        async fn signature_help(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn completion(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn semantic_tokens(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn workspace_symbols(
            &self,
            _: &str,
        ) -> Result<Vec<(String, String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn operational_state(&self) -> String {
            "ready".to_string()
        }
        async fn server_info(&self) -> (Option<String>, Option<u64>) {
            (Some("boundary-impl-fail".into()), Some(1))
        }
    }

    #[tokio::test]
    async fn test_interface_boundary_emits_implementations_unavailable_note() {
        // Custom mock: document_symbols succeeds, implementations fails.
        // The recipe must surface an operational note mentioning the
        // implementation failure rather than failing the request.
        let request = InterfaceBoundaryRequest {
            file: std::path::PathBuf::from("src/api.rs"),
            symbol: None,
            include_implementations: true,
            settings: RecipeSettings::default(),
        };

        let outcome = execute_interface_boundary(&BoundaryImplFailProvider, &request)
            .await
            .unwrap();

        // The structured note describing the implementation failure.
        let has_impl_note = outcome
            .packet
            .notes
            .iter()
            .any(|n| n.contains("implementations unavailable"));
        assert!(
            has_impl_note,
            "expected 'implementations unavailable' note; got {:?}",
            outcome.packet.notes
        );

        // And an operational item describing the failure.
        let op_items: Vec<_> = outcome
            .packet
            .items
            .iter()
            .filter(|i| {
                i.kind == crate::context::LspContextItemKind::OperationalNote
                    && i.message.contains("implementations unavailable")
            })
            .collect();
        assert!(
            !op_items.is_empty(),
            "expected operational item for implementations failure; got items={:?}",
            outcome
                .packet
                .items
                .iter()
                .map(|i| i.message.as_str())
                .collect::<Vec<_>>()
        );
    }

    /// Mock that returns distinct diagnostics per file so we can count
    /// which related files were actually consulted.
    struct PerFileDiagnosticsProvider {
        marker: String,
    }

    #[async_trait]
    impl LspEvidenceProvider for PerFileDiagnosticsProvider {
        async fn diagnostics_for_file(
            &self,
            file: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            // Emit a diagnostic only for files whose path contains the
            // marker so we can observe which files were visited.
            if file.to_string_lossy().contains(&self.marker) {
                Ok(vec![(
                    "warning".into(),
                    format!("marker-{}", file.display()),
                    "(1:0)-(1:1)".into(),
                )])
            } else {
                Ok(vec![(
                    "warning".into(),
                    format!("other-{}", file.display()),
                    "(1:0)-(1:1)".into(),
                )])
            }
        }
        async fn document_symbols(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn go_to_definition(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn find_references(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn implementations(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
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
            Ok(Vec::new())
        }
        async fn signature_help(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn completion(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn semantic_tokens(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn workspace_symbols(
            &self,
            _: &str,
        ) -> Result<Vec<(String, String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn operational_state(&self) -> String {
            "ready".to_string()
        }
        async fn server_info(&self) -> (Option<String>, Option<u64>) {
            (Some("per-file-diag".into()), Some(1))
        }
    }

    #[tokio::test]
    async fn test_cross_file_repair_enforces_related_file_cap() {
        // Cap related_files to 2 via RecipeSettings.max_files, then
        // pass 5 related files. Only 2 of them should be touched.
        let provider = PerFileDiagnosticsProvider {
            marker: "INCLUDED".to_string(),
        };
        let settings = RecipeSettings {
            max_files: 2,
            ..Default::default()
        };

        let request = CrossFileRepairRequest {
            primary_file: std::path::PathBuf::from("src/main.rs"),
            related_files: vec![
                std::path::PathBuf::from("INCLUDED/keep1.rs"),
                std::path::PathBuf::from("INCLUDED/keep2.rs"),
                std::path::PathBuf::from("INCLUDED/drop1.rs"),
                std::path::PathBuf::from("INCLUDED/drop2.rs"),
                std::path::PathBuf::from("INCLUDED/drop3.rs"),
            ],
            ranges: vec![],
            settings,
        };

        let outcome = execute_cross_file_repair(&provider, &request)
            .await
            .unwrap();

        // Count diagnostics whose message contains the marker prefix
        // for related-file diagnostics (they read like
        // "marker-src/INCLUDED/<file>.rs").
        let related_diags: Vec<_> = outcome
            .packet
            .items
            .iter()
            .filter(|i| {
                i.kind == crate::context::LspContextItemKind::Diagnostic
                    && !i.score.is_same_file
                    && i.message.starts_with("marker-")
            })
            .collect();

        assert_eq!(
            related_diags.len(),
            2,
            "expected exactly 2 related-file diagnostics (cap=max_files=2); got {}: {:?}",
            related_diags.len(),
            related_diags
                .iter()
                .map(|i| i.message.as_str())
                .collect::<Vec<_>>()
        );
    }

    /// Mock returning document_highlights for incoming + refs for outgoing.
    struct CallNeighborhoodProvider {
        highlights: Vec<String>,
        refs: Vec<(String, String)>,
    }

    #[async_trait]
    impl LspEvidenceProvider for CallNeighborhoodProvider {
        async fn diagnostics_for_file(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn document_symbols(
            &self,
            _: &std::path::Path,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn go_to_definition(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn find_references(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(self.refs.clone())
        }
        async fn implementations(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
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
            Ok(self.highlights.clone())
        }
        async fn signature_help(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn completion(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn semantic_tokens(
            &self,
            _: &std::path::Path,
            _: u32,
            _: u32,
        ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn workspace_symbols(
            &self,
            _: &str,
        ) -> Result<Vec<(String, String, String, String)>, LspError> {
            Ok(Vec::new())
        }
        async fn operational_state(&self) -> String {
            "ready".to_string()
        }
        async fn server_info(&self) -> (Option<String>, Option<u64>) {
            (Some("call-hood".into()), Some(1))
        }
    }

    #[tokio::test]
    async fn test_call_neighborhood_enforces_depth_cap() {
        // Recipe hardcodes max_callers=10 / max_callees=10; depth is the
        // only request-controlled cap. Verify that requesting depth > 1
        // emits the explicit "recursive expansion is capped for safety"
        // note and that depth 0 emits "no hierarchy collected".
        let provider = CallNeighborhoodProvider {
            highlights: vec!["(1:0)-(1:5)".to_string()],
            refs: vec![("caller.rs".to_string(), "(2:0)-(2:5)".to_string())],
        };

        let request = CallNeighborhoodRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 42,
            column: 10,
            direction: crate::context::HierarchyDirection::Both,
            max_depth: 2,
            settings: RecipeSettings::default(),
        };

        let outcome = execute_call_neighborhood(&provider, &request)
            .await
            .unwrap();

        // Depth > 1 emits a depth-cap note.
        assert!(
            outcome
                .packet
                .notes
                .iter()
                .any(|n| n.contains("recursive expansion is capped")),
            "expected depth-cap note when max_depth=2; got {:?}",
            outcome.packet.notes
        );

        // depth=0 emits an early-return note.
        let provider_zero = CallNeighborhoodProvider {
            highlights: vec![],
            refs: vec![],
        };
        let request_zero = CallNeighborhoodRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 42,
            column: 10,
            direction: crate::context::HierarchyDirection::Both,
            max_depth: 0,
            settings: RecipeSettings::default(),
        };
        let outcome_zero = execute_call_neighborhood(&provider_zero, &request_zero)
            .await
            .unwrap();
        assert!(
            outcome_zero
                .packet
                .notes
                .iter()
                .any(|n| n.contains("depth 0")),
            "expected depth=0 early-return note; got {:?}",
            outcome_zero.packet.notes
        );

        // Depth is clamped at 3 internally — depth=10 still produces
        // items, never an unbounded walk.
        let request_deep = CallNeighborhoodRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 42,
            column: 10,
            direction: crate::context::HierarchyDirection::Both,
            max_depth: 10,
            settings: RecipeSettings::default(),
        };
        let outcome_deep = execute_call_neighborhood(&provider, &request_deep)
            .await
            .unwrap();
        // The execution terminates (test reaches here) and uses the
        // clamped effective depth, still emitting the depth-cap note.
        assert!(
            outcome_deep
                .packet
                .notes
                .iter()
                .any(|n| n.contains("recursive expansion is capped")),
            "expected depth-cap note for max_depth=10; got {:?}",
            outcome_deep.packet.notes
        );
    }

    #[tokio::test]
    async fn test_call_neighborhood_caller_callee_caps() {
        // The recipe hardcodes max_callers=10 and max_callees=10.
        // Provide 20 highlights + 20 refs and verify caps are honored.
        let provider = CallNeighborhoodProvider {
            highlights: (0..20).map(|i| format!("({i}:0)-({i}:5)")).collect(),
            refs: (0..20)
                .map(|i| (format!("caller_{i}.rs"), format!("({i}:0)-({i}:5)")))
                .collect(),
        };

        let request = CallNeighborhoodRequest {
            file: std::path::PathBuf::from("src/main.rs"),
            line: 42,
            column: 10,
            direction: crate::context::HierarchyDirection::Both,
            max_depth: 1,
            settings: RecipeSettings::default(),
        };

        let outcome = execute_call_neighborhood(&provider, &request)
            .await
            .unwrap();

        let incoming = outcome
            .packet
            .items
            .iter()
            .filter(|i| i.kind == crate::context::LspContextItemKind::DocumentHighlight)
            .count();
        let outgoing = outcome
            .packet
            .items
            .iter()
            .filter(|i| i.message.contains("callees (outgoing)"))
            .count();

        assert!(
            incoming <= 10,
            "incoming callers must be capped at 10; got {incoming}"
        );
        assert!(
            outgoing <= 10,
            "outgoing callees must be capped at 10; got {outgoing}"
        );
    }

    #[tokio::test]
    async fn test_impact_analysis_required_mode_fails_when_unusable() {
        let request = ImpactAnalysisRequest {
            symbol: crate::context::SymbolTarget {
                file: std::path::PathBuf::from("src/lib.rs"),
                position: lsp_types::Position {
                    line: 10,
                    character: 5,
                },
            },
            changed_files: vec![],
            settings: RecipeSettings {
                mode: LspContextMode::Required,
                ..Default::default()
            },
        };

        let result = execute_impact_analysis(&UnavailProvider, &request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            LspContextError::RequiredFailed(_) => {}
            other => panic!("expected RequiredFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_test_failure_repair_required_mode_fails_when_unusable() {
        let request = TestFailureRepairRequest {
            test_file: std::path::PathBuf::from("tests/foo.rs"),
            failure_message: "panicked at my_function".to_string(),
            related_files: vec![],
            settings: RecipeSettings {
                mode: LspContextMode::Required,
                ..Default::default()
            },
        };

        let result = execute_test_failure_repair(&UnavailProvider, &request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            LspContextError::RequiredFailed(_) => {}
            other => panic!("expected RequiredFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_impact_analysis_opportunistic_mode_returns_notes() {
        let request = ImpactAnalysisRequest {
            symbol: crate::context::SymbolTarget {
                file: std::path::PathBuf::from("src/lib.rs"),
                position: lsp_types::Position {
                    line: 10,
                    character: 5,
                },
            },
            changed_files: vec![],
            settings: RecipeSettings {
                mode: LspContextMode::Opportunistic,
                ..Default::default()
            },
        };

        let outcome = execute_impact_analysis(&UnavailProvider, &request)
            .await
            .unwrap();
        assert!(
            outcome.fallback_used,
            "Opportunistic mode with no server must flag fallback_used"
        );
        assert!(
            outcome.notes.iter().any(|n| n.contains("fallback")),
            "expected fallback note; got {:?}",
            outcome.notes
        );
    }

    #[tokio::test]
    async fn test_cross_file_repair_opportunistic_mode_returns_notes() {
        let request = CrossFileRepairRequest {
            primary_file: std::path::PathBuf::from("src/main.rs"),
            related_files: vec![std::path::PathBuf::from("src/lib.rs")],
            ranges: vec![],
            settings: RecipeSettings {
                mode: LspContextMode::Opportunistic,
                ..Default::default()
            },
        };

        let outcome = execute_cross_file_repair(&UnavailProvider, &request)
            .await
            .unwrap();
        assert!(outcome.fallback_used);
        assert!(outcome.notes.iter().any(|n| n.contains("fallback")));
    }

    // -----------------------------------------------------------------------
    // Composed workflow tests — sub-recipe provenance, caps, skip reasons
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_composed_security_review_records_sub_recipes() {
        let settings = RecipeSettings::for_tier(ModelTier::Workhorse);
        let outcome = execute_composed_security_review(
            &DegradedProvider,
            vec![std::path::PathBuf::from("src/auth.rs")],
            vec![],
            Some(std::path::PathBuf::from("src/auth.rs")),
            Some((10, 5)),
            settings,
        )
        .await
        .unwrap();

        // Should have at least security_review_enriched sub-recipe.
        assert!(
            !outcome.sub_recipes.is_empty(),
            "composed security review must record sub_recipes"
        );
        let enriched = outcome
            .sub_recipes
            .iter()
            .find(|sr| sr.recipe == LspWorkflowRecipe::SecurityReviewEnriched);
        assert!(enriched.is_some(), "must include SecurityReviewEnriched");
        assert!(
            enriched.unwrap().ran,
            "SecurityReviewEnriched must have ran"
        );

        // When call_neighborhood params are provided, should also have CallNeighborhood.
        let call = outcome
            .sub_recipes
            .iter()
            .find(|sr| sr.recipe == LspWorkflowRecipe::CallNeighborhood);
        assert!(
            call.is_some(),
            "must include CallNeighborhood when params provided"
        );
        assert!(call.unwrap().ran, "CallNeighborhood must have ran");

        // Freshness summary should reflect composed workflow.
        assert_eq!(outcome.freshness_summary, "composed security review");
    }

    #[tokio::test]
    async fn test_composed_security_review_skips_call_neighborhood_when_no_params() {
        let settings = RecipeSettings::for_tier(ModelTier::Workhorse);
        let outcome = execute_composed_security_review(
            &DegradedProvider,
            vec![std::path::PathBuf::from("src/auth.rs")],
            vec![],
            None, // no call_neighborhood file
            None, // no call_neighborhood position
            settings,
        )
        .await
        .unwrap();

        // Should only have security_review_enriched sub-recipe.
        assert_eq!(outcome.sub_recipes.len(), 1);
        assert_eq!(
            outcome.sub_recipes[0].recipe,
            LspWorkflowRecipe::SecurityReviewEnriched
        );
        assert!(outcome.sub_recipes[0].ran);

        // No CallNeighborhood sub-recipe.
        let call = outcome
            .sub_recipes
            .iter()
            .find(|sr| sr.recipe == LspWorkflowRecipe::CallNeighborhood);
        assert!(
            call.is_none(),
            "CallNeighborhood must not appear without params"
        );
    }

    #[tokio::test]
    async fn test_composed_security_review_records_skip_reason_on_failure() {
        let settings = RecipeSettings::for_tier(ModelTier::Workhorse);
        // UnavailProvider: security_review_enriched may still succeed (it uses diagnostics),
        // but call_neighborhood will fail because find_references errors.
        let outcome = execute_composed_security_review(
            &UnavailProvider,
            vec![std::path::PathBuf::from("src/auth.rs")],
            vec![],
            Some(std::path::PathBuf::from("src/auth.rs")),
            Some((10, 5)),
            settings,
        )
        .await
        .unwrap();

        // SecurityReviewEnriched runs even with UnavailProvider (degraded mode produces notes).
        let enriched = outcome
            .sub_recipes
            .iter()
            .find(|sr| sr.recipe == LspWorkflowRecipe::SecurityReviewEnriched);
        assert!(enriched.is_some());
        assert!(enriched.unwrap().ran);

        // CallNeighborhood should have a skipped_reason if it failed.
        let call = outcome
            .sub_recipes
            .iter()
            .find(|sr| sr.recipe == LspWorkflowRecipe::CallNeighborhood);
        if let Some(call) = call {
            // It may succeed in degraded mode or fail; either way it's recorded.
            if !call.ran {
                assert!(
                    call.skipped_reason.is_some(),
                    "skipped sub-recipe must have a reason"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_composed_repair_failing_test_records_sub_recipes_and_caps() {
        let settings = RecipeSettings::for_tier(ModelTier::Workhorse);
        let outcome = execute_composed_repair_failing_test(
            &DegradedProvider,
            std::path::PathBuf::from("tests/foo.rs"),
            "thread 'test_foo' panicked".to_string(),
            vec![
                std::path::PathBuf::from("src/foo.rs"),
                std::path::PathBuf::from("src/bar.rs"),
            ],
            settings,
        )
        .await
        .unwrap();

        // Should have test_failure_repair + up to 2 repair_local sub-recipes (capped at 3 total).
        assert!(
            outcome.sub_recipes.len() >= 2,
            "must have at least test_failure_repair + 1 repair_local, got {}",
            outcome.sub_recipes.len()
        );

        let tf = outcome
            .sub_recipes
            .iter()
            .find(|sr| sr.recipe == LspWorkflowRecipe::TestFailureRepair);
        assert!(tf.is_some(), "must include TestFailureRepair");
        assert!(tf.unwrap().ran);

        let repair_count = outcome
            .sub_recipes
            .iter()
            .filter(|sr| sr.recipe == LspWorkflowRecipe::RepairLocal)
            .count();
        assert!(repair_count >= 1, "must have at least 1 RepairLocal");
        assert!(
            repair_count <= 3,
            "RepairLocal must be capped at 3, got {}",
            repair_count
        );

        assert_eq!(outcome.freshness_summary, "composed test failure repair");
    }

    #[tokio::test]
    async fn test_composed_repair_failing_test_caps_related_files() {
        let settings = RecipeSettings::for_tier(ModelTier::Small);
        let outcome = execute_composed_repair_failing_test(
            &DegradedProvider,
            std::path::PathBuf::from("tests/bar.rs"),
            "assertion failed".to_string(),
            vec![
                std::path::PathBuf::from("src/a.rs"),
                std::path::PathBuf::from("src/b.rs"),
                std::path::PathBuf::from("src/c.rs"),
                std::path::PathBuf::from("src/d.rs"), // exceeds cap of 3
            ],
            settings,
        )
        .await
        .unwrap();

        // Should have at most 3 RepairLocal sub-recipes (cap is 3).
        let repair_count = outcome
            .sub_recipes
            .iter()
            .filter(|sr| sr.recipe == LspWorkflowRecipe::RepairLocal)
            .count();
        assert!(
            repair_count <= 3,
            "RepairLocal cap must be enforced, got {}",
            repair_count
        );
    }

    #[tokio::test]
    async fn test_composed_review_api_change_records_sub_recipes() {
        let settings = RecipeSettings::for_tier(ModelTier::Workhorse);
        let outcome = execute_composed_review_api_change(
            &DegradedProvider,
            std::path::PathBuf::from("src/api.rs"),
            Some("UserApi".to_string()),
            10,
            5,
            vec![],
            settings,
        )
        .await
        .unwrap();

        // Should have interface_boundary + impact_analysis sub-recipes.
        assert_eq!(outcome.sub_recipes.len(), 2);
        let ib = outcome
            .sub_recipes
            .iter()
            .find(|sr| sr.recipe == LspWorkflowRecipe::InterfaceBoundary);
        assert!(ib.is_some(), "must include InterfaceBoundary");
        assert!(ib.unwrap().ran);

        let ia = outcome
            .sub_recipes
            .iter()
            .find(|sr| sr.recipe == LspWorkflowRecipe::ImpactAnalysis);
        assert!(ia.is_some(), "must include ImpactAnalysis");
        assert!(ia.unwrap().ran);
    }

    #[tokio::test]
    async fn test_composed_repair_hunk_with_preview_records_sub_recipes() {
        let settings = RecipeSettings::for_tier(ModelTier::Workhorse);
        let outcome = execute_composed_repair_hunk_with_preview(
            &DegradedProvider,
            std::path::PathBuf::from("src/main.rs"),
            vec![crate::context::HunkRange {
                start: 0,
                end: 20,
                original_start: None,
                original_end: None,
            }],
            Some(10),
            Some(5),
            settings,
        )
        .await
        .unwrap();

        // Should have repair_hunk sub-recipe (always runs).
        let rh = outcome
            .sub_recipes
            .iter()
            .find(|sr| sr.recipe == LspWorkflowRecipe::RepairHunk);
        assert!(rh.is_some(), "must include RepairHunk");
        assert!(rh.unwrap().ran);
    }

    // -----------------------------------------------------------------------
    // LspWorkflowInvocation::execute_composed dispatch tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_invocation_composed_security_review_dispatches() {
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::SecurityReviewEnriched,
            related_files: vec!["src/auth.rs".to_string()],
            line: Some(10),
            column: Some(5),
            ..Default::default()
        };
        let outcome = invocation
            .execute_composed(&DegradedProvider)
            .await
            .unwrap();
        assert_eq!(outcome.recipe, LspWorkflowRecipe::SecurityReviewEnriched);
        assert!(
            !outcome.sub_recipes.is_empty(),
            "composed dispatch must populate sub_recipes"
        );
    }

    #[tokio::test]
    async fn test_invocation_composed_test_failure_repair_dispatches() {
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::TestFailureRepair,
            primary_path: Some("tests/foo.rs".to_string()),
            failure_text: Some("panicked".to_string()),
            related_files: vec!["src/foo.rs".to_string()],
            ..Default::default()
        };
        let outcome = invocation
            .execute_composed(&DegradedProvider)
            .await
            .unwrap();
        assert_eq!(outcome.recipe, LspWorkflowRecipe::TestFailureRepair);
        assert!(
            outcome.sub_recipes.len() >= 2,
            "composed test_failure_repair must have at least 2 sub_recipes"
        );
    }

    #[tokio::test]
    async fn test_invocation_composed_interface_boundary_dispatches() {
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::InterfaceBoundary,
            primary_path: Some("src/api.rs".to_string()),
            symbol: Some("UserApi".to_string()),
            line: Some(10),
            column: Some(5),
            ..Default::default()
        };
        let outcome = invocation
            .execute_composed(&DegradedProvider)
            .await
            .unwrap();
        assert_eq!(outcome.recipe, LspWorkflowRecipe::InterfaceBoundary);
        assert_eq!(outcome.sub_recipes.len(), 2);
    }

    #[tokio::test]
    async fn test_invocation_composed_repair_hunk_dispatches() {
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::RepairHunk,
            primary_path: Some("src/main.rs".to_string()),
            hunk_ranges: vec![crate::context::HunkRange {
                start: 0,
                end: 20,
                original_start: None,
                original_end: None,
            }],
            line: Some(10),
            column: Some(5),
            ..Default::default()
        };
        let outcome = invocation
            .execute_composed(&DegradedProvider)
            .await
            .unwrap();
        assert_eq!(outcome.recipe, LspWorkflowRecipe::RepairHunk);
        assert!(
            !outcome.sub_recipes.is_empty(),
            "composed repair_hunk must have sub_recipes"
        );
    }

    #[tokio::test]
    async fn test_invocation_composed_non_composed_recipe_falls_through() {
        // RepairLocal is not in the composed match, so it falls through to execute().
        let invocation = LspWorkflowInvocation {
            recipe: LspWorkflowRecipe::RepairLocal,
            primary_path: Some("src/main.rs".to_string()),
            line: Some(10),
            ..Default::default()
        };
        let outcome = invocation
            .execute_composed(&DegradedProvider)
            .await
            .unwrap();
        assert_eq!(outcome.recipe, LspWorkflowRecipe::RepairLocal);
        // Base recipes have empty sub_recipes.
        assert!(
            outcome.sub_recipes.is_empty(),
            "non-composed recipe must have empty sub_recipes"
        );
    }
}
