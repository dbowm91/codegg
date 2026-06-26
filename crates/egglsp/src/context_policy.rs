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
}
