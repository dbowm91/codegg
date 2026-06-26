use std::collections::HashMap;

use crate::context::LspContextBudget;
use crate::context_renderer::{LspContextRenderConfig, ModelTier};
use crate::workflow_recipes::{LspWorkflowRecipe, RecipeSettings};

// ---------------------------------------------------------------------------
// Stale evidence handling policy
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum StaleEvidencePolicy {
    #[default]
    IncludeWithWarning,
    OmitStale,
    RequireFresh,
}

// ---------------------------------------------------------------------------
// Unavailable LSP server handling policy
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum LspUnavailablePolicy {
    #[default]
    NoteOnly,
    Omit,
    FailWhenRequired,
}

// ---------------------------------------------------------------------------
// Task risk classification
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum LspTaskRisk {
    #[default]
    Normal,
    Low,
    High,
    SecuritySensitive,
}

// ---------------------------------------------------------------------------
// Operational lifecycle state (mirrors LspOperationalState concepts)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LspOperationalState {
    Ready,
    Starting,
    Initializing,
    Indexing,
    Degraded,
    RestartScheduled,
    Restarting,
    Failed,
    Stopping,
    Stopped,
}

impl std::fmt::Display for LspOperationalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => write!(f, "ready"),
            Self::Starting => write!(f, "starting"),
            Self::Initializing => write!(f, "initializing"),
            Self::Indexing => write!(f, "indexing"),
            Self::Degraded => write!(f, "degraded"),
            Self::RestartScheduled => write!(f, "restart_scheduled"),
            Self::Restarting => write!(f, "restarting"),
            Self::Failed => write!(f, "failed"),
            Self::Stopping => write!(f, "stopping"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

impl LspOperationalState {
    pub fn from_health_state(label: &str) -> Option<Self> {
        match label {
            "ready" => Some(Self::Ready),
            "starting" => Some(Self::Starting),
            "initializing" => Some(Self::Initializing),
            "indexing" => Some(Self::Indexing),
            "degraded" => Some(Self::Degraded),
            "restart_scheduled" => Some(Self::RestartScheduled),
            "restarting" => Some(Self::Restarting),
            "failed" => Some(Self::Failed),
            "stopping" => Some(Self::Stopping),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }

    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Ready | Self::Indexing | Self::Degraded)
    }
}

// ---------------------------------------------------------------------------
// Where the tier came from
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum TierSource {
    #[default]
    Default,
    ExplicitOverride,
    ConfigOverride,
    ModelFamily,
    WorkflowDefault,
}

impl std::fmt::Display for TierSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::ExplicitOverride => write!(f, "explicit_override"),
            Self::ConfigOverride => write!(f, "config_override"),
            Self::ModelFamily => write!(f, "model_family"),
            Self::WorkflowDefault => write!(f, "workflow_default"),
        }
    }
}

impl TierSource {
    /// Human-readable label for the tier source.
    pub fn render(&self) -> &'static str {
        match self {
            TierSource::Default => "default",
            TierSource::ExplicitOverride => "explicit override",
            TierSource::ConfigOverride => "config override",
            TierSource::ModelFamily => "model family heuristic",
            TierSource::WorkflowDefault => "workflow default",
        }
    }
}

// ---------------------------------------------------------------------------
// Tier resolution result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TierResolution {
    pub tier: ModelTier,
    pub source: TierSource,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Central policy struct
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LspContextPolicy {
    pub model_tier: ModelTier,
    pub workflow: LspWorkflowRecipe,
    pub task_risk: LspTaskRisk,
    pub lifecycle_state: Option<LspOperationalState>,
    pub token_budget_hint: Option<usize>,
    pub max_context_bytes: usize,
    pub include_cross_file: bool,
    pub include_hierarchy: bool,
    pub include_previews: bool,
    pub stale_evidence_policy: StaleEvidencePolicy,
    pub unavailable_policy: LspUnavailablePolicy,
    pub tier_source: TierSource,
}

impl Default for LspContextPolicy {
    fn default() -> Self {
        Self {
            model_tier: ModelTier::Workhorse,
            workflow: LspWorkflowRecipe::RepairLocal,
            task_risk: LspTaskRisk::Normal,
            lifecycle_state: None,
            token_budget_hint: None,
            max_context_bytes: 32_768,
            include_cross_file: false,
            include_hierarchy: false,
            include_previews: false,
            stale_evidence_policy: StaleEvidencePolicy::default(),
            unavailable_policy: LspUnavailablePolicy::default(),
            tier_source: TierSource::Default,
        }
    }
}

impl LspContextPolicy {
    /// Constructor that sets sensible defaults for the given tier/workflow
    /// and applies overrides from additional parameters.
    pub fn resolve(
        tier: ModelTier,
        workflow: LspWorkflowRecipe,
        task_risk: LspTaskRisk,
        lifecycle_state: Option<LspOperationalState>,
        stale_policy: Option<StaleEvidencePolicy>,
        unavailable_policy: Option<LspUnavailablePolicy>,
        token_budget_hint: Option<usize>,
        max_context_bytes: Option<usize>,
    ) -> Self {
        let base = Self::workflow_tier_defaults(workflow, tier);
        Self {
            task_risk,
            lifecycle_state,
            token_budget_hint,
            max_context_bytes: max_context_bytes.unwrap_or(base.max_context_bytes),
            stale_evidence_policy: stale_policy.unwrap_or(base.stale_evidence_policy),
            unavailable_policy: unavailable_policy.unwrap_or(base.unavailable_policy),
            ..base
        }
    }

    /// Build defaults based on the workflow/tier matrix.
    pub fn workflow_tier_defaults(workflow: LspWorkflowRecipe, tier: ModelTier) -> Self {
        let (include_cross_file, include_hierarchy, include_previews) =
            Self::feature_flags_for_workflow_tier(workflow, tier);

        let stale_evidence_policy = match tier {
            ModelTier::Small => StaleEvidencePolicy::OmitStale,
            ModelTier::Workhorse => StaleEvidencePolicy::IncludeWithWarning,
            ModelTier::Frontier => StaleEvidencePolicy::IncludeWithWarning,
        };

        Self {
            model_tier: tier,
            workflow,
            task_risk: LspTaskRisk::Normal,
            lifecycle_state: None,
            token_budget_hint: None,
            max_context_bytes: Self::max_context_bytes_for_tier(tier),
            include_cross_file,
            include_hierarchy,
            include_previews,
            stale_evidence_policy,
            unavailable_policy: LspUnavailablePolicy::NoteOnly,
            tier_source: TierSource::WorkflowDefault,
        }
    }

    fn feature_flags_for_workflow_tier(
        workflow: LspWorkflowRecipe,
        tier: ModelTier,
    ) -> (bool, bool, bool) {
        match workflow {
            LspWorkflowRecipe::RepairLocal => {
                let cross_file = matches!(tier, ModelTier::Workhorse | ModelTier::Frontier);
                let hierarchy = matches!(tier, ModelTier::Frontier);
                let previews = matches!(tier, ModelTier::Frontier);
                (cross_file, hierarchy, previews)
            }
            LspWorkflowRecipe::RepairHunk => {
                let cross_file = matches!(tier, ModelTier::Frontier);
                let hierarchy = matches!(tier, ModelTier::Frontier);
                (cross_file, hierarchy, false)
            }
            LspWorkflowRecipe::ReviewDiff => {
                let cross_file = matches!(tier, ModelTier::Workhorse | ModelTier::Frontier);
                let hierarchy = matches!(tier, ModelTier::Frontier);
                (cross_file, hierarchy, false)
            }
            LspWorkflowRecipe::SecurityReviewEnriched => {
                let cross_file = matches!(tier, ModelTier::Workhorse | ModelTier::Frontier);
                let hierarchy = matches!(tier, ModelTier::Frontier);
                (cross_file, hierarchy, false)
            }
            LspWorkflowRecipe::PreviewSuggestion => {
                let previews = !matches!(tier, ModelTier::Small);
                (false, false, previews)
            }
            LspWorkflowRecipe::ImpactAnalysis => {
                let cross_file = !matches!(tier, ModelTier::Small);
                let hierarchy = matches!(tier, ModelTier::Frontier);
                (cross_file, hierarchy, false)
            }
            LspWorkflowRecipe::TestFailureRepair => {
                let cross_file = matches!(tier, ModelTier::Workhorse | ModelTier::Frontier);
                let hierarchy = matches!(tier, ModelTier::Frontier);
                (cross_file, hierarchy, false)
            }
            LspWorkflowRecipe::ReviewFile => {
                let cross_file = false;
                let hierarchy = matches!(tier, ModelTier::Frontier);
                (cross_file, hierarchy, false)
            }
            LspWorkflowRecipe::HunkSourceNavigation => {
                let cross_file = matches!(tier, ModelTier::Frontier);
                let hierarchy = matches!(tier, ModelTier::Frontier);
                (cross_file, hierarchy, false)
            }
            LspWorkflowRecipe::InterfaceBoundary => {
                let cross_file = false;
                let hierarchy = !matches!(tier, ModelTier::Small);
                (cross_file, hierarchy, false)
            }
            LspWorkflowRecipe::CrossFileRepair => {
                let cross_file = !matches!(tier, ModelTier::Small);
                let hierarchy = matches!(tier, ModelTier::Frontier);
                (cross_file, hierarchy, false)
            }
            LspWorkflowRecipe::CallNeighborhood => {
                let hierarchy = !matches!(tier, ModelTier::Small);
                (false, hierarchy, false)
            }
        }
    }

    fn max_context_bytes_for_tier(tier: ModelTier) -> usize {
        match tier {
            ModelTier::Small => 16_384,
            ModelTier::Workhorse => 32_768,
            ModelTier::Frontier => 65_536,
        }
    }

    /// Convert to a render config for the agent-facing renderer.
    pub fn to_render_config(&self) -> LspContextRenderConfig {
        let (max_diagnostics, max_references, max_symbols, max_bytes_per_section) =
            match self.model_tier {
                ModelTier::Small => (10, 0, 15, 1000),
                ModelTier::Workhorse => (15, 15, 20, 2000),
                ModelTier::Frontier => (20, 30, 30, 4000),
            };

        LspContextRenderConfig {
            max_diagnostics,
            max_references,
            max_symbols,
            max_bytes_per_section,
            include_previews: self.include_previews,
            include_truncation_notes: true,
            include_cross_file: self.include_cross_file,
            include_hierarchy: self.include_hierarchy,
            model_tier: self.model_tier,
        }
    }

    /// Convert to a budget for evidence collection.
    pub fn to_budget(&self) -> LspContextBudget {
        let base = RecipeSettings::for_tier(self.model_tier).to_budget();
        LspContextBudget {
            max_bytes: self.max_context_bytes,
            ..base
        }
    }

    /// Convert to recipe settings for evidence gathering.
    pub fn to_recipe_settings(&self) -> RecipeSettings {
        let mut settings = RecipeSettings::for_tier(self.model_tier);
        settings.include_references = !matches!(self.model_tier, ModelTier::Small);
        settings.include_preview_hints = self.include_previews;
        settings.include_definitions = true;
        settings.allow_stale_evidence = matches!(
            self.stale_evidence_policy,
            StaleEvidencePolicy::IncludeWithWarning
        );
        settings.include_cross_file = self.include_cross_file;
        settings.include_hierarchy = self.include_hierarchy;
        settings
    }

    /// One-line debug summary.
    pub fn policy_summary(&self) -> String {
        let lifecycle = self
            .lifecycle_state
            .map(|s| format!(" lifecycle={s}"))
            .unwrap_or_default();
        format!(
            "LSP policy: tier={} workflow={} risk={} stale={} tier_source={}{}",
            self.model_tier,
            self.workflow,
            self.task_risk,
            self.stale_evidence_policy,
            self.tier_source,
            lifecycle,
        )
    }
}

impl std::fmt::Display for StaleEvidencePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IncludeWithWarning => write!(f, "include_with_warning"),
            Self::OmitStale => write!(f, "omit_stale"),
            Self::RequireFresh => write!(f, "require_fresh"),
        }
    }
}

impl std::fmt::Display for LspTaskRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::Low => write!(f, "low"),
            Self::High => write!(f, "high"),
            Self::SecuritySensitive => write!(f, "security_sensitive"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tier resolution standalone function
// ---------------------------------------------------------------------------

/// Resolve the model tier following a precedence chain:
/// explicit override > config override > family heuristic > Workhorse default.
pub fn resolve_model_tier(
    override_tier: Option<ModelTier>,
    config_overrides: Option<&HashMap<String, ModelTier>>,
    model_family: &str,
) -> TierResolution {
    let mut notes = Vec::new();

    // 1. Explicit override wins.
    if let Some(tier) = override_tier {
        notes.push(format!("using explicit override: {tier}"));
        return TierResolution {
            tier,
            source: TierSource::ExplicitOverride,
            notes,
        };
    }

    // 2. Config overrides (keyed on model family).
    if let Some(config) = config_overrides {
        let family_lower = model_family.to_ascii_lowercase();
        if let Some(&tier) = config.get(&family_lower) {
            notes.push(format!(
                "resolved from config override for family '{family_lower}': {tier}"
            ));
            return TierResolution {
                tier,
                source: TierSource::ConfigOverride,
                notes,
            };
        }
    }

    // 3. Family heuristic via model_tier_for_profile.
    let tier = crate::context_renderer::model_tier_for_profile(model_family);
    notes.push(format!(
        "resolved from model family '{model_family}': {tier}"
    ));

    TierResolution {
        tier,
        source: TierSource::ModelFamily,
        notes,
    }
}

// ---------------------------------------------------------------------------
// Context diagnostics
// ---------------------------------------------------------------------------

/// Structured diagnostics explaining why LSP context was shaped as it was.
///
/// Generated from an `LspContextPacket` + `LspContextPolicy` without
/// mutating either. Intended for on-demand inspection, not normal prompts.
#[derive(Debug, Clone)]
pub struct LspContextDiagnostics {
    /// Resolved model tier.
    pub model_tier: ModelTier,
    /// How the tier was determined.
    pub tier_source: TierSource,
    /// Workflow recipe that produced the context.
    pub workflow: LspWorkflowRecipe,
    /// Task risk classification.
    pub task_risk: LspTaskRisk,
    /// Stale evidence policy applied.
    pub stale_policy: StaleEvidencePolicy,
    /// Unavailable LSP policy applied.
    pub unavailable_policy: LspUnavailablePolicy,
    /// Maximum context bytes budget.
    pub max_context_bytes: usize,
    /// Number of evidence items included in the rendered output.
    pub included_items: usize,
    /// Number of evidence items omitted (budget/dedup/filter).
    pub omitted_items: usize,
    /// Number of stale evidence items.
    pub stale_items: usize,
    /// Sections that were truncated with reason notes.
    pub truncated_sections: Vec<String>,
    /// Whether the packet came from the semantic cache.
    pub cache_hit: bool,
    /// Operational notes from collection.
    pub notes: Vec<String>,
}

impl LspContextDiagnostics {
    /// Build diagnostics from a context packet and the policy that produced it.
    ///
    /// Does not mutate either input. Cache-hit detection uses a heuristic
    /// (check notes for `[cache-hit]` prefix).
    pub fn from_packet_and_policy(
        packet: &crate::context::LspContextPacket,
        policy: &LspContextPolicy,
    ) -> Self {
        let included_items = packet.items.len();

        let total_possible = packet
            .budget
            .as_ref()
            .map(|b| b.max_diagnostics + b.max_references + b.max_symbols + b.max_files)
            .unwrap_or(included_items);
        let omitted_items = total_possible.saturating_sub(included_items);

        let stale_items = packet
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i.provenance.freshness,
                    crate::context::LspEvidenceFreshness::Stale
                        | crate::context::LspEvidenceFreshness::PossiblyStale
                )
            })
            .count();

        let mut truncated_sections = Vec::new();
        if packet.truncation.bytes_truncated {
            truncated_sections.push(format!(
                "total bytes truncated ({}/{} bytes)",
                packet.truncation.total_bytes, packet.truncation.max_bytes,
            ));
        }
        if packet.truncation.files_truncated {
            truncated_sections.push("files truncated by per-file range limit".to_string());
        }
        if packet.truncation.references_truncated {
            truncated_sections.push("references truncated by category limit".to_string());
        }
        if packet.truncation.diagnostics_truncated {
            truncated_sections.push("diagnostics truncated by category limit".to_string());
        }
        for note in &packet.notes {
            if note.contains("capped") || note.contains("truncated") || note.contains("omitted") {
                truncated_sections.push(note.clone());
            }
        }

        let cache_hit = packet.notes.iter().any(|n| n.starts_with("[cache-hit]"));

        Self {
            model_tier: policy.model_tier,
            tier_source: policy.tier_source,
            workflow: policy.workflow,
            task_risk: policy.task_risk,
            stale_policy: policy.stale_evidence_policy,
            unavailable_policy: policy.unavailable_policy,
            max_context_bytes: policy.max_context_bytes,
            included_items,
            omitted_items,
            stale_items,
            truncated_sections,
            cache_hit,
            notes: packet.notes.clone(),
        }
    }

    /// Render a compact human-readable summary for TUI display.
    pub fn render_compact(&self) -> String {
        let stale = if self.stale_items > 0 {
            format!(", {} stale", self.stale_items)
        } else {
            String::new()
        };
        let cache = if self.cache_hit { " [cache-hit]" } else { "" };
        let trunc = if self.truncated_sections.is_empty() {
            String::new()
        } else {
            format!(", {} truncated sections", self.truncated_sections.len())
        };
        format!(
            "Tier: {:?} ({}) | Workflow: {:?} | Risk: {:?} | Policy: stale={:?} unavailable={:?} | Items: {} included, {} omitted{}{}{}\nMax bytes: {}{}\nNotes: {}",
            self.model_tier,
            self.tier_source.render(),
            self.workflow,
            self.task_risk,
            self.stale_policy,
            self.unavailable_policy,
            self.included_items,
            self.omitted_items,
            stale,
            trunc,
            cache,
            self.max_context_bytes,
            if self.truncated_sections.is_empty() {
                String::new()
            } else {
                format!(
                    "\nTruncation: {}",
                    self.truncated_sections.join("; ")
                )
            },
            if self.notes.is_empty() {
                "(none)".to_string()
            } else {
                self.notes.join("; ")
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_tier_defaults_all_recipes_all_tiers() {
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
                let policy = LspContextPolicy::workflow_tier_defaults(recipe, tier);
                assert_eq!(policy.model_tier, tier);
                assert_eq!(policy.workflow, recipe);
            }
        }
    }

    #[test]
    fn resolve_model_tier_explicit_wins() {
        let config = HashMap::new();
        let resolution =
            resolve_model_tier(Some(ModelTier::Small), Some(&config), "frontierreasoning");
        assert_eq!(resolution.tier, ModelTier::Small);
        assert_eq!(resolution.source, TierSource::ExplicitOverride);
    }

    #[test]
    fn resolve_model_tier_config_wins() {
        let mut config = HashMap::new();
        config.insert("my-custom-model".to_string(), ModelTier::Frontier);
        let resolution = resolve_model_tier(None, Some(&config), "my-custom-model");
        assert_eq!(resolution.tier, ModelTier::Frontier);
        assert_eq!(resolution.source, TierSource::ConfigOverride);
    }

    #[test]
    fn resolve_model_tier_family_heuristic() {
        let resolution = resolve_model_tier(None, None, "frontierreasoning");
        assert_eq!(resolution.tier, ModelTier::Frontier);
        assert_eq!(resolution.source, TierSource::ModelFamily);
    }

    #[test]
    fn resolve_model_tier_unknown_family_defaults_workhorse() {
        let resolution = resolve_model_tier(None, None, "unknown-model");
        assert_eq!(resolution.tier, ModelTier::Workhorse);
        assert_eq!(resolution.source, TierSource::ModelFamily);
    }

    #[test]
    fn resolve_model_tier_no_config_no_override() {
        let resolution = resolve_model_tier(None, None, "");
        assert_eq!(resolution.tier, ModelTier::Workhorse);
        assert_eq!(resolution.source, TierSource::ModelFamily);
    }

    #[test]
    fn policy_summary_format() {
        let policy = LspContextPolicy {
            model_tier: ModelTier::Workhorse,
            workflow: LspWorkflowRecipe::RepairLocal,
            task_risk: LspTaskRisk::Normal,
            lifecycle_state: Some(LspOperationalState::Ready),
            tier_source: TierSource::ModelFamily,
            ..Default::default()
        };
        let summary = policy.policy_summary();
        assert!(summary.contains("tier=workhorse"));
        assert!(summary.contains("workflow=repair_local"));
        assert!(summary.contains("risk=normal"));
        assert!(summary.contains("lifecycle=ready"));
        assert!(summary.contains("tier_source=model_family"));
    }

    #[test]
    fn to_render_config_respects_tier() {
        let small = LspContextPolicy {
            model_tier: ModelTier::Small,
            include_previews: false,
            ..Default::default()
        };
        let config = small.to_render_config();
        assert_eq!(config.model_tier, ModelTier::Small);
        assert_eq!(config.max_bytes_per_section, 1000);
        assert!(!config.include_previews);

        let frontier = LspContextPolicy {
            model_tier: ModelTier::Frontier,
            include_previews: true,
            ..Default::default()
        };
        let config = frontier.to_render_config();
        assert_eq!(config.model_tier, ModelTier::Frontier);
        assert_eq!(config.max_bytes_per_section, 4000);
        assert!(config.include_previews);
    }

    #[test]
    fn to_budget_uses_max_context_bytes() {
        let policy = LspContextPolicy {
            model_tier: ModelTier::Frontier,
            max_context_bytes: 65_536,
            ..Default::default()
        };
        let budget = policy.to_budget();
        assert_eq!(budget.max_bytes, 65_536);
        assert_eq!(budget.max_references, 50);
        assert_eq!(budget.max_symbols, 50);
    }

    #[test]
    fn to_recipe_settings_preserves_tier() {
        let policy = LspContextPolicy {
            model_tier: ModelTier::Small,
            include_previews: false,
            stale_evidence_policy: StaleEvidencePolicy::OmitStale,
            ..Default::default()
        };
        let settings = policy.to_recipe_settings();
        assert_eq!(settings.model_tier, ModelTier::Small);
        assert!(!settings.include_references);
        assert!(!settings.include_preview_hints);
        assert!(!settings.allow_stale_evidence);
    }

    #[test]
    fn recipe_settings_for_tier_still_works() {
        let small = RecipeSettings::for_tier(ModelTier::Small);
        assert_eq!(small.model_tier, ModelTier::Small);
        assert!(!small.include_references);

        let workhorse = RecipeSettings::for_tier(ModelTier::Workhorse);
        assert_eq!(workhorse.model_tier, ModelTier::Workhorse);
        assert!(workhorse.include_references);

        let frontier = RecipeSettings::for_tier(ModelTier::Frontier);
        assert_eq!(frontier.model_tier, ModelTier::Frontier);
        assert!(frontier.include_preview_hints);
    }

    #[test]
    fn stale_evidence_policy_small_omits_stale() {
        let policy = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::RepairLocal,
            ModelTier::Small,
        );
        assert_eq!(policy.stale_evidence_policy, StaleEvidencePolicy::OmitStale);
    }

    #[test]
    fn stale_evidence_policy_workhorse_includes_with_warning() {
        let policy = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::RepairLocal,
            ModelTier::Workhorse,
        );
        assert_eq!(
            policy.stale_evidence_policy,
            StaleEvidencePolicy::IncludeWithWarning
        );
    }

    #[test]
    fn budget_constrained_policy_derivation() {
        let policy = LspContextPolicy::resolve(
            ModelTier::Workhorse,
            LspWorkflowRecipe::ImpactAnalysis,
            LspTaskRisk::Normal,
            None,
            None,
            None,
            None,
            Some(8_192),
        );
        assert_eq!(policy.max_context_bytes, 8_192);
        let budget = policy.to_budget();
        assert_eq!(budget.max_bytes, 8_192);
    }

    #[test]
    fn operational_state_from_health_state() {
        assert_eq!(
            LspOperationalState::from_health_state("ready"),
            Some(LspOperationalState::Ready)
        );
        assert_eq!(
            LspOperationalState::from_health_state("degraded"),
            Some(LspOperationalState::Degraded)
        );
        assert_eq!(LspOperationalState::from_health_state("bogus"), None);
    }

    #[test]
    fn operational_state_is_usable() {
        assert!(LspOperationalState::Ready.is_usable());
        assert!(LspOperationalState::Indexing.is_usable());
        assert!(LspOperationalState::Degraded.is_usable());
        assert!(!LspOperationalState::Starting.is_usable());
        assert!(!LspOperationalState::Failed.is_usable());
    }

    #[test]
    fn operational_state_display() {
        assert_eq!(LspOperationalState::Ready.to_string(), "ready");
        assert_eq!(LspOperationalState::Failed.to_string(), "failed");
    }

    #[test]
    fn policy_from_ref_into_render_config() {
        let policy = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::RepairLocal,
            ModelTier::Frontier,
        );
        let config: LspContextRenderConfig = (&policy).into();
        assert_eq!(config.model_tier, ModelTier::Frontier);
        assert!(config.include_previews);
    }

    #[test]
    fn policy_from_ref_into_recipe_settings() {
        let policy = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::ReviewDiff,
            ModelTier::Workhorse,
        );
        let settings: RecipeSettings = (&policy).into();
        assert_eq!(settings.model_tier, ModelTier::Workhorse);
        assert!(settings.include_references);
    }

    #[test]
    fn feature_flags_repair_local_tier_matrix() {
        let small = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::RepairLocal,
            ModelTier::Small,
        );
        assert!(!small.include_cross_file);
        assert!(!small.include_hierarchy);
        assert!(!small.include_previews);

        let workhorse = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::RepairLocal,
            ModelTier::Workhorse,
        );
        assert!(workhorse.include_cross_file);
        assert!(!workhorse.include_hierarchy);
        assert!(!workhorse.include_previews);

        let frontier = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::RepairLocal,
            ModelTier::Frontier,
        );
        assert!(frontier.include_cross_file);
        assert!(frontier.include_hierarchy);
        assert!(frontier.include_previews);
    }

    #[test]
    fn feature_flags_call_neighborhood() {
        let small = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::CallNeighborhood,
            ModelTier::Small,
        );
        assert!(!small.include_cross_file);
        assert!(!small.include_hierarchy);

        let workhorse = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::CallNeighborhood,
            ModelTier::Workhorse,
        );
        assert!(workhorse.include_hierarchy);

        let frontier = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::CallNeighborhood,
            ModelTier::Frontier,
        );
        assert!(frontier.include_hierarchy);
    }

    #[test]
    fn feature_flags_preview_suggestion() {
        let small = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::PreviewSuggestion,
            ModelTier::Small,
        );
        assert!(!small.include_previews);

        let workhorse = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::PreviewSuggestion,
            ModelTier::Workhorse,
        );
        assert!(workhorse.include_previews);
    }

    #[test]
    fn feature_flags_impact_analysis() {
        let small = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::ImpactAnalysis,
            ModelTier::Small,
        );
        assert!(!small.include_cross_file);

        let workhorse = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::ImpactAnalysis,
            ModelTier::Workhorse,
        );
        assert!(workhorse.include_cross_file);
        assert!(!workhorse.include_hierarchy);

        let frontier = LspContextPolicy::workflow_tier_defaults(
            LspWorkflowRecipe::ImpactAnalysis,
            ModelTier::Frontier,
        );
        assert!(frontier.include_cross_file);
        assert!(frontier.include_hierarchy);
    }

    #[test]
    fn tier_source_display() {
        assert_eq!(TierSource::Default.to_string(), "default");
        assert_eq!(
            TierSource::ExplicitOverride.to_string(),
            "explicit_override"
        );
        assert_eq!(TierSource::ModelFamily.to_string(), "model_family");
    }

    #[test]
    fn resolve_with_lifecycle_state() {
        let policy = LspContextPolicy::resolve(
            ModelTier::Workhorse,
            LspWorkflowRecipe::ReviewDiff,
            LspTaskRisk::High,
            Some(LspOperationalState::Degraded),
            None,
            None,
            None,
            None,
        );
        assert_eq!(policy.lifecycle_state, Some(LspOperationalState::Degraded));
        assert_eq!(policy.task_risk, LspTaskRisk::High);
    }

    // -----------------------------------------------------------------------
    // Workstream 5 hardening tests (Phase 11 policy)
    // -----------------------------------------------------------------------

    #[test]
    fn lsp_unavailable_policy_variants_distinguishable_on_policy() {
        // All three LspUnavailablePolicy variants must be set on
        // LspContextPolicy without panicking and remain distinguishable.
        for variant in [
            LspUnavailablePolicy::NoteOnly,
            LspUnavailablePolicy::Omit,
            LspUnavailablePolicy::FailWhenRequired,
        ] {
            let policy = LspContextPolicy {
                unavailable_policy: variant,
                ..Default::default()
            };
            assert_eq!(policy.unavailable_policy, variant);
        }

        // Ensure variants are pairwise distinct.
        assert_ne!(
            LspUnavailablePolicy::Omit,
            LspUnavailablePolicy::FailWhenRequired
        );
        assert_ne!(
            LspUnavailablePolicy::NoteOnly,
            LspUnavailablePolicy::FailWhenRequired
        );
        assert_ne!(LspUnavailablePolicy::NoteOnly, LspUnavailablePolicy::Omit);
    }

    #[test]
    fn policy_with_security_sensitive_risk_constructs_and_summarizes() {
        let policy = LspContextPolicy {
            task_risk: LspTaskRisk::SecuritySensitive,
            workflow: LspWorkflowRecipe::SecurityReviewEnriched,
            model_tier: ModelTier::Frontier,
            ..Default::default()
        };
        let summary = policy.policy_summary();
        assert!(
            summary.contains("risk=security_sensitive"),
            "policy_summary must include security-sensitive risk; got {summary}"
        );
        assert!(
            summary.contains("workflow=security_review_enriched"),
            "policy_summary must include workflow name; got {summary}"
        );
        assert!(
            summary.contains("tier=frontier"),
            "policy_summary must include tier; got {summary}"
        );
    }

    #[test]
    fn task_risk_all_variants_display() {
        // Every variant must round-trip through Display without panic.
        assert_eq!(LspTaskRisk::Normal.to_string(), "normal");
        assert_eq!(LspTaskRisk::Low.to_string(), "low");
        assert_eq!(LspTaskRisk::High.to_string(), "high");
        assert_eq!(
            LspTaskRisk::SecuritySensitive.to_string(),
            "security_sensitive"
        );
    }

    #[test]
    fn token_budget_hint_with_max_context_bytes_affects_budget() {
        // Setting token_budget_hint and max_context_bytes together:
        // token_budget_hint is recorded on the policy, and max_context_bytes
        // is propagated to the budget verbatim.
        let policy = LspContextPolicy::resolve(
            ModelTier::Workhorse,
            LspWorkflowRecipe::ImpactAnalysis,
            LspTaskRisk::Normal,
            None,
            None,
            None,
            Some(1024),  // token_budget_hint = "low" budget hint
            Some(8_192), // max_context_bytes override
        );
        assert_eq!(
            policy.token_budget_hint,
            Some(1024),
            "token_budget_hint must be stored verbatim"
        );
        let budget = policy.to_budget();
        assert_eq!(
            budget.max_bytes, 8_192,
            "max_context_bytes override must reach to_budget().max_bytes"
        );
    }

    #[test]
    fn token_budget_hint_does_not_override_max_context_bytes_when_none() {
        // When max_context_bytes is None, the workflow/tier baseline
        // (Workhorse = 32_768) must remain.
        let policy = LspContextPolicy::resolve(
            ModelTier::Workhorse,
            LspWorkflowRecipe::RepairLocal,
            LspTaskRisk::Normal,
            None,
            None,
            None,
            Some(512),
            None,
        );
        assert_eq!(policy.token_budget_hint, Some(512));
        let budget = policy.to_budget();
        assert_eq!(budget.max_bytes, 32_768);
    }

    #[test]
    fn render_config_from_policy_propagates_flags() {
        // Verify that include_previews, include_cross_file, and
        // include_hierarchy all propagate from policy to render config.
        let policy_on = LspContextPolicy {
            model_tier: ModelTier::Frontier,
            include_previews: true,
            include_cross_file: true,
            include_hierarchy: true,
            ..Default::default()
        };
        let config_on: LspContextRenderConfig = (&policy_on).into();
        assert!(config_on.include_previews);
        assert!(config_on.include_cross_file);
        assert!(config_on.include_hierarchy);

        let policy_off = LspContextPolicy {
            model_tier: ModelTier::Small,
            include_previews: false,
            include_cross_file: false,
            include_hierarchy: false,
            ..Default::default()
        };
        let config_off: LspContextRenderConfig = (&policy_off).into();
        assert!(!config_off.include_previews);
        assert!(!config_off.include_cross_file);
        assert!(!config_off.include_hierarchy);
    }

    #[test]
    fn recipe_settings_from_policy_allow_stale_matches_policy() {
        // IncludeWithWarning → allow_stale_evidence = true.
        let policy_warn = LspContextPolicy {
            stale_evidence_policy: StaleEvidencePolicy::IncludeWithWarning,
            ..Default::default()
        };
        let settings_warn: RecipeSettings = (&policy_warn).into();
        assert!(
            settings_warn.allow_stale_evidence,
            "IncludeWithWarning must translate to allow_stale_evidence=true"
        );

        // OmitStale → allow_stale_evidence = false.
        let policy_omit = LspContextPolicy {
            stale_evidence_policy: StaleEvidencePolicy::OmitStale,
            ..Default::default()
        };
        let settings_omit: RecipeSettings = (&policy_omit).into();
        assert!(
            !settings_omit.allow_stale_evidence,
            "OmitStale must translate to allow_stale_evidence=false"
        );

        // RequireFresh → allow_stale_evidence = false (only
        // IncludeWithWarning enables it).
        let policy_fresh = LspContextPolicy {
            stale_evidence_policy: StaleEvidencePolicy::RequireFresh,
            ..Default::default()
        };
        let settings_fresh: RecipeSettings = (&policy_fresh).into();
        assert!(
            !settings_fresh.allow_stale_evidence,
            "RequireFresh must translate to allow_stale_evidence=false"
        );
    }

    #[test]
    fn recipe_settings_from_policy_includes_references_depends_on_tier() {
        // Small tier → include_references = false (regardless of policy).
        let policy_small = LspContextPolicy {
            model_tier: ModelTier::Small,
            ..Default::default()
        };
        let settings_small: RecipeSettings = (&policy_small).into();
        assert!(!settings_small.include_references);

        // Workhorse tier → include_references = true.
        let policy_wh = LspContextPolicy {
            model_tier: ModelTier::Workhorse,
            ..Default::default()
        };
        let settings_wh: RecipeSettings = (&policy_wh).into();
        assert!(settings_wh.include_references);
    }

    #[test]
    fn policy_summary_includes_workflow_tier_and_risk() {
        let policy = LspContextPolicy {
            model_tier: ModelTier::Small,
            workflow: LspWorkflowRecipe::CrossFileRepair,
            task_risk: LspTaskRisk::High,
            lifecycle_state: None,
            ..Default::default()
        };
        let summary = policy.policy_summary();
        assert!(
            summary.contains("workflow=cross_file_repair"),
            "summary missing workflow name: {summary}"
        );
        assert!(
            summary.contains("tier=small"),
            "summary missing tier: {summary}"
        );
        assert!(
            summary.contains("risk=high"),
            "summary missing risk: {summary}"
        );
    }

    #[test]
    fn policy_summary_no_lifecycle_when_unset() {
        // Lifecycle token should not appear when lifecycle_state is None.
        let policy = LspContextPolicy {
            lifecycle_state: None,
            ..Default::default()
        };
        let summary = policy.policy_summary();
        assert!(
            !summary.contains("lifecycle="),
            "summary must omit lifecycle= when lifecycle_state is None; got {summary}"
        );
    }

    #[test]
    fn resolve_all_args_set_tier_source_to_workflow_default() {
        // LspContextPolicy::resolve currently takes 8 arguments and
        // always returns tier_source = WorkflowDefault because it
        // derives from workflow_tier_defaults. Verify all 8 are accepted
        // and the tier_source is WorkflowDefault.
        let policy = LspContextPolicy::resolve(
            ModelTier::Frontier,                          // tier (override)
            LspWorkflowRecipe::ImpactAnalysis,            // workflow
            LspTaskRisk::SecuritySensitive,               // task_risk
            Some(LspOperationalState::Indexing),          // lifecycle_state
            Some(StaleEvidencePolicy::OmitStale),         // stale_policy
            Some(LspUnavailablePolicy::FailWhenRequired), // unavailable_policy
            Some(2048),                                   // token_budget_hint
            Some(16_384),                                 // max_context_bytes
        );
        assert_eq!(policy.model_tier, ModelTier::Frontier);
        assert_eq!(policy.workflow, LspWorkflowRecipe::ImpactAnalysis);
        assert_eq!(policy.task_risk, LspTaskRisk::SecuritySensitive);
        assert_eq!(policy.lifecycle_state, Some(LspOperationalState::Indexing));
        assert_eq!(policy.stale_evidence_policy, StaleEvidencePolicy::OmitStale);
        assert_eq!(
            policy.unavailable_policy,
            LspUnavailablePolicy::FailWhenRequired
        );
        assert_eq!(policy.token_budget_hint, Some(2048));
        assert_eq!(policy.max_context_bytes, 16_384);
        // tier_source is WorkflowDefault because resolve builds from
        // workflow_tier_defaults.
        assert_eq!(policy.tier_source, TierSource::WorkflowDefault);
    }

    #[test]
    fn resolve_without_overrides_uses_baseline_for_tier() {
        // Without explicit overrides, stale/unavailable policies should
        // come from the tier baseline (Small = OmitStale).
        let policy = LspContextPolicy::resolve(
            ModelTier::Small,
            LspWorkflowRecipe::RepairLocal,
            LspTaskRisk::Normal,
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(policy.stale_evidence_policy, StaleEvidencePolicy::OmitStale);
        assert_eq!(policy.unavailable_policy, LspUnavailablePolicy::NoteOnly);
        // Small tier defaults to 16_384.
        assert_eq!(policy.max_context_bytes, 16_384);
    }

    #[test]
    fn stale_evidence_policy_all_variants_display() {
        assert_eq!(
            StaleEvidencePolicy::IncludeWithWarning.to_string(),
            "include_with_warning"
        );
        assert_eq!(StaleEvidencePolicy::OmitStale.to_string(), "omit_stale");
        assert_eq!(
            StaleEvidencePolicy::RequireFresh.to_string(),
            "require_fresh"
        );
    }

    #[test]
    fn diagnostics_from_packet_and_policy_basic() {
        use crate::context::{
            LineRange, LspContextPacket, LspContextPacketMode, LspContextRequest,
            LspContextTruncation,
        };
        use std::path::PathBuf;

        let policy = LspContextPolicy {
            model_tier: ModelTier::Small,
            workflow: LspWorkflowRecipe::RepairLocal,
            task_risk: LspTaskRisk::Normal,
            stale_evidence_policy: StaleEvidencePolicy::OmitStale,
            unavailable_policy: LspUnavailablePolicy::NoteOnly,
            ..Default::default()
        };

        let packet = LspContextPacket {
            request: LspContextRequest::File {
                file: PathBuf::from("test.rs"),
                line_ranges: vec![LineRange { start: 10, end: 20 }],
                include_symbols: true,
                include_diagnostics: false,
            },
            items: vec![],
            previews: vec![],
            preview_ids: vec![],
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: Some("rust-analyzer".to_string()),
            server_generation: Some(1),
            operational_state: Some("Ready".to_string()),
            budget: None,
            notes: vec!["test note".to_string()],
            truncation: LspContextTruncation::default(),
        };

        let diag = LspContextDiagnostics::from_packet_and_policy(&packet, &policy);
        assert_eq!(diag.model_tier, ModelTier::Small);
        assert_eq!(diag.workflow, LspWorkflowRecipe::RepairLocal);
        assert_eq!(diag.included_items, 0);
        assert!(!diag.cache_hit);

        let rendered = diag.render_compact();
        assert!(rendered.contains("Small"));
        assert!(rendered.contains("test note"));
    }
}
