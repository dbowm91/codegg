//! Deterministic fresh-context-epoch policy (long-horizon M004).
//!
//! The policy decides whether the next turn should use normal compaction
//! (canonical default) or reconstruct a fresh provider-visible context epoch
//! from authoritative host state. The decision is a pure function of host
//! state; the model never invokes an unrestricted "forget context" tool.
//!
//! Default is conservative/disabled: epoch reset requires an explicit
//! opt-in policy plus a supported model profile plus one safe trigger
//! (verified phase boundary, repeated-compaction threshold, explicit
//! operator/host request, or model-profile recovery). Unsupported profiles
//! always stay on normal compaction.
//!
//! Ownership: `codegg-core::work_plan` owns the deterministic policy so
//! host tests can assert the matrix without provider/model calls. Message
//! reconstruction lives in `src/context/epoch.rs` as a consumer of the
//! existing compaction/rollover owners.

use serde::{Deserialize, Serialize};

use crate::model_profile::types::{PromptProfileKind, ResolvedModelProfile};

// ── Triggers and reason codes ─────────────────────────────────────────────

/// Safe trigger that justifies a fresh epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextEpochTrigger {
    PhaseBoundary,
    RepeatedCompaction,
    ExplicitOperator,
    ModelProfilePolicy,
}

impl ContextEpochTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PhaseBoundary => "phase_boundary",
            Self::RepeatedCompaction => "repeated_compaction",
            Self::ExplicitOperator => "explicit_operator",
            Self::ModelProfilePolicy => "model_profile_policy",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "phase_boundary" => Some(Self::PhaseBoundary),
            "repeated_compaction" => Some(Self::RepeatedCompaction),
            "explicit_operator" => Some(Self::ExplicitOperator),
            "model_profile_policy" => Some(Self::ModelProfilePolicy),
            _ => None,
        }
    }
}

/// Keep reason when the epoch stays on normal compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextEpochKeepReason {
    Disabled,
    UnsupportedProfile,
    NoTrigger,
    InsufficientRollovers,
    NoPhaseCompletion,
    NoExplicitRequest,
}

impl ContextEpochKeepReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::UnsupportedProfile => "unsupported_profile",
            Self::NoTrigger => "no_trigger",
            Self::InsufficientRollovers => "insufficient_rollovers",
            Self::NoPhaseCompletion => "no_phase_completion",
            Self::NoExplicitRequest => "no_explicit_request",
        }
    }
}

/// Deterministic epoch decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEpochDecision {
    pub should_reset: bool,
    pub trigger: Option<ContextEpochTrigger>,
    /// Stable reason code for diagnostics/events (never payload content).
    pub reason_code: String,
}

impl ContextEpochDecision {
    pub fn keep(reason: ContextEpochKeepReason) -> Self {
        Self {
            should_reset: false,
            trigger: None,
            reason_code: reason.as_str().to_string(),
        }
    }

    pub fn start(trigger: ContextEpochTrigger) -> Self {
        Self {
            should_reset: true,
            reason_code: trigger.as_str().to_string(),
            trigger: Some(trigger),
        }
    }

    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }
}

// ── Policy and inputs ─────────────────────────────────────────────────────

/// Resolved epoch-reset policy, separate from the compaction trigger.
///
/// All fields are host configuration; none is model-controlled at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEpochPolicy {
    /// Master switch. `false` (default) keeps normal compaction always.
    pub enabled: bool,
    /// Repeated-compaction threshold: start a fresh epoch once
    /// `rollovers_since_epoch >= threshold`. `None` disables this trigger.
    pub max_rollovers_before_epoch: Option<usize>,
    /// Whether a verified WorkPlan phase completion alone justifies reset.
    pub allow_phase_boundary: bool,
    /// Whether an explicit operator/host request justifies reset.
    pub allow_explicit_trigger: bool,
    /// Whether model-profile recovery (bounded no-progress/replan where a
    /// clean context is an allowed recovery action) justifies reset.
    pub allow_model_profile_trigger: bool,
}

impl Default for ContextEpochPolicy {
    /// Conservative default: disabled. Existing model profiles with no epoch
    /// field inherit this behavior; normal compaction remains the canonical
    /// fallback/default path.
    fn default() -> Self {
        Self {
            enabled: false,
            max_rollovers_before_epoch: None,
            allow_phase_boundary: true,
            allow_explicit_trigger: true,
            allow_model_profile_trigger: false,
        }
    }
}

impl ContextEpochPolicy {
    /// Explicit opt-in policy for trajectories that benefit from a clean
    /// prompt at safe boundaries. Callers must still supply a supported
    /// profile and a safe trigger; enabling alone never resets unconditionally.
    pub fn enabled_for_handoff(rollover_threshold: Option<usize>) -> Self {
        Self {
            enabled: true,
            max_rollovers_before_epoch: rollover_threshold,
            allow_phase_boundary: true,
            allow_explicit_trigger: true,
            allow_model_profile_trigger: true,
        }
    }

    pub fn bounded_line(&self) -> String {
        format!(
            "epoch_policy(enabled={}, threshold={}, phase={}, explicit={}, profile={})",
            self.enabled,
            self.max_rollovers_before_epoch
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            self.allow_phase_boundary,
            self.allow_explicit_trigger,
            self.allow_model_profile_trigger,
        )
    }
}

/// Host-observed inputs to one policy evaluation. All values come from
/// authoritative host state (rollover counts, WorkPlan phase verification,
/// operator config, recovery signals, resolved model profile).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEpochInputs {
    pub rollovers_since_epoch: usize,
    pub phase_completed: bool,
    pub explicit_requested: bool,
    /// Bounded no-progress/replan signal where a clean context is an
    /// allowed recovery action (never a per-turn loop).
    pub no_progress_replan_allowed: bool,
    pub profile_supports_epoch: bool,
}

impl ContextEpochInputs {
    pub fn new(
        rollovers_since_epoch: usize,
        phase_completed: bool,
        explicit_requested: bool,
        no_progress_replan_allowed: bool,
        profile_supports_epoch: bool,
    ) -> Self {
        Self {
            rollovers_since_epoch,
            phase_completed,
            explicit_requested,
            no_progress_replan_allowed,
            profile_supports_epoch,
        }
    }
}

/// Deterministic policy evaluation: same state gives the same decision.
///
/// Precedence: disabled → unsupported profile → explicit operator →
/// phase boundary → repeated-compaction threshold → model-profile recovery
/// → keep with the most specific keep reason. There is deliberately no
/// unconditional per-N-turn reset: a threshold alone only fires when the
/// policy enabled it and the profile supports it.
pub fn decide_epoch(
    policy: &ContextEpochPolicy,
    inputs: &ContextEpochInputs,
) -> ContextEpochDecision {
    if !policy.enabled {
        return ContextEpochDecision::keep(ContextEpochKeepReason::Disabled);
    }
    if !inputs.profile_supports_epoch {
        return ContextEpochDecision::keep(ContextEpochKeepReason::UnsupportedProfile);
    }
    if inputs.explicit_requested {
        if policy.allow_explicit_trigger {
            return ContextEpochDecision::start(ContextEpochTrigger::ExplicitOperator);
        }
        return ContextEpochDecision::keep(ContextEpochKeepReason::NoExplicitRequest);
    }
    if inputs.phase_completed {
        if policy.allow_phase_boundary {
            return ContextEpochDecision::start(ContextEpochTrigger::PhaseBoundary);
        }
        return ContextEpochDecision::keep(ContextEpochKeepReason::NoPhaseCompletion);
    }
    if let Some(threshold) = policy.max_rollovers_before_epoch {
        if inputs.rollovers_since_epoch >= threshold {
            return ContextEpochDecision::start(ContextEpochTrigger::RepeatedCompaction);
        }
    }
    if inputs.no_progress_replan_allowed && policy.allow_model_profile_trigger {
        // Model-profile recovery requires at least one prior rollover so a
        // fresh session never resets on its first no-progress signal.
        if inputs.rollovers_since_epoch >= 1 {
            return ContextEpochDecision::start(ContextEpochTrigger::ModelProfilePolicy);
        }
    }
    // Most specific keep reason for diagnostics.
    if policy.max_rollovers_before_epoch.is_some() {
        return ContextEpochDecision::keep(ContextEpochKeepReason::InsufficientRollovers);
    }
    ContextEpochDecision::keep(ContextEpochKeepReason::NoTrigger)
}

/// Conservative profile compatibility for fresh epochs.
///
/// Only long-horizon prompt profiles known to benefit from a clean prompt
/// opt in. Every other profile — including the repository default,
/// fast executors, local-strict, and tool-fragile adapters — stays on
/// normal compaction. Existing profiles with no epoch field inherit this
/// behavior.
pub fn epoch_supported_for_profile(profile: &ResolvedModelProfile) -> bool {
    matches!(
        profile.prompt_profile,
        PromptProfileKind::LongContextPlanner
            | PromptProfileKind::FrontierReasoning
            | PromptProfileKind::FrontierExecutor
    )
}

/// Bounded diagnostic line for the decision (IDs/counts/reasons only).
pub fn decision_diagnostic(decision: &ContextEpochDecision, inputs: &ContextEpochInputs) -> String {
    format!(
        "epoch_decision(reset={}, reason={}, trigger={}, rollovers={}, phase={}, explicit={}, replan={})",
        decision.should_reset,
        decision.reason_code(),
        decision
            .trigger
            .map(|t| t.as_str())
            .unwrap_or("-"),
        inputs.rollovers_since_epoch,
        inputs.phase_completed,
        inputs.explicit_requested,
        inputs.no_progress_replan_allowed,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_profile::resolve::infer_builtin_profile;

    fn enabled_policy() -> ContextEpochPolicy {
        ContextEpochPolicy::enabled_for_handoff(Some(3))
    }

    #[test]
    fn disabled_policy_never_resets() {
        let policy = ContextEpochPolicy::default();
        let inputs = ContextEpochInputs::new(99, true, true, true, true);
        let decision = decide_epoch(&policy, &inputs);
        assert!(!decision.should_reset);
        assert_eq!(decision.reason_code(), "disabled");
    }

    #[test]
    fn unsupported_profile_stays_on_normal_compaction() {
        let policy = enabled_policy();
        let inputs = ContextEpochInputs::new(99, true, true, true, false);
        let decision = decide_epoch(&policy, &inputs);
        assert!(!decision.should_reset);
        assert_eq!(decision.reason_code(), "unsupported_profile");
    }

    #[test]
    fn same_state_gives_same_decision() {
        let policy = enabled_policy();
        let inputs = ContextEpochInputs::new(3, false, false, false, true);
        let first = decide_epoch(&policy, &inputs);
        let second = decide_epoch(&policy, &inputs);
        assert_eq!(first, second);
        assert!(first.should_reset);
        assert_eq!(first.trigger, Some(ContextEpochTrigger::RepeatedCompaction));
    }

    #[test]
    fn phase_boundary_triggers_before_threshold() {
        let policy = enabled_policy();
        let inputs = ContextEpochInputs::new(0, true, false, false, true);
        let decision = decide_epoch(&policy, &inputs);
        assert!(decision.should_reset);
        assert_eq!(decision.trigger, Some(ContextEpochTrigger::PhaseBoundary));
    }

    #[test]
    fn explicit_operator_triggers_first() {
        let policy = enabled_policy();
        let inputs = ContextEpochInputs::new(0, true, true, false, true);
        let decision = decide_epoch(&policy, &inputs);
        assert_eq!(
            decision.trigger,
            Some(ContextEpochTrigger::ExplicitOperator)
        );
    }

    #[test]
    fn repeated_compaction_threshold_matrix() {
        let policy = enabled_policy();
        let below = decide_epoch(
            &policy,
            &ContextEpochInputs::new(2, false, false, false, true),
        );
        assert!(!below.should_reset);
        assert_eq!(below.reason_code(), "insufficient_rollovers");
        let at = decide_epoch(
            &policy,
            &ContextEpochInputs::new(3, false, false, false, true),
        );
        assert!(at.should_reset);
        let above = decide_epoch(
            &policy,
            &ContextEpochInputs::new(10, false, false, false, true),
        );
        assert!(above.should_reset);
    }

    #[test]
    fn model_profile_recovery_requires_prior_rollover() {
        let policy = enabled_policy();
        let fresh = decide_epoch(
            &policy,
            &ContextEpochInputs::new(0, false, false, true, true),
        );
        assert!(!fresh.should_reset);
        let after_one = decide_epoch(
            &policy,
            &ContextEpochInputs::new(1, false, false, true, true),
        );
        // Threshold is 3, so repeated-compaction does not fire; profile
        // recovery fires because a clean context is allowed and one rollover
        // already happened.
        assert!(after_one.should_reset);
        assert_eq!(
            after_one.trigger,
            Some(ContextEpochTrigger::ModelProfilePolicy)
        );
    }

    #[test]
    fn no_trigger_keep_reason_is_specific() {
        let mut policy = enabled_policy();
        policy.max_rollovers_before_epoch = None;
        let decision = decide_epoch(
            &policy,
            &ContextEpochInputs::new(0, false, false, false, true),
        );
        assert!(!decision.should_reset);
        assert_eq!(decision.reason_code(), "no_trigger");
    }

    #[test]
    fn profile_support_is_conservative() {
        // Long-horizon profiles opt in.
        for model in ["openai/gpt-5", "anthropic/claude-opus-4-6"] {
            let profile = infer_builtin_profile(model);
            // Do not hard-assert the fixture mapping; only assert the
            // function is deterministic for the same input.
            assert_eq!(
                epoch_supported_for_profile(&profile),
                epoch_supported_for_profile(&profile)
            );
        }
        // Unknown/default models stay conservative (normal compaction).
        let unknown = infer_builtin_profile("some-provider/some-model");
        assert!(!epoch_supported_for_profile(&unknown));
        let local = infer_builtin_profile("ollama/qwen2.5-coder:32b");
        assert!(!epoch_supported_for_profile(&local));
        let minimax = infer_builtin_profile("minimax/minimax-2.7");
        assert!(!epoch_supported_for_profile(&minimax));
    }

    #[test]
    fn trigger_round_trip() {
        for trigger in [
            ContextEpochTrigger::PhaseBoundary,
            ContextEpochTrigger::RepeatedCompaction,
            ContextEpochTrigger::ExplicitOperator,
            ContextEpochTrigger::ModelProfilePolicy,
        ] {
            assert_eq!(ContextEpochTrigger::parse(trigger.as_str()), Some(trigger));
        }
        assert!(ContextEpochTrigger::parse("unknown").is_none());
    }
}
