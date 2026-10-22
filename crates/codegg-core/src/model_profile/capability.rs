//! The per-model reasoning capability record, its resolution, and the
//! local admission policy that applies it (ADR-0049).
//!
//! The record itself is a schema type, so it lives in `codegg-config`
//! (the crate both the configuration surface and the core profile share)
//! and is re-exported here. This module owns the *policy* around it:
//! how two records merge, how the effective capability for a model is
//! resolved, and how a resolved selection is admitted or refused.
//!
//! Three properties are load-bearing here:
//!
//! * **Unknown is not Unsupported.** An undeclared dimension means
//!   "nobody has published a contract for this", not "the model refuses
//!   this". The kernel's fail-closed policy is what turns that
//!   distinction into a refusal at admission time, not a fabricated
//!   capability.
//! * **The accepted-label set is CodeGG's.** The kernel's neutral
//!   capability type is status-only — it carries no accepted-value list
//!   and never validates a label — so the set lives in CodeGG's record
//!   and the kernel supplies the per-dimension decision.
//! * **Resolution is read-only.** Nothing here mutates the adapter
//!   baseline, the configuration, or any in-flight turn state.

pub use codegg_config::schema::{
    ReasoningCapabilityRecord, ReasoningCapabilityStatus, MAX_REASONING_CAPABILITY_LABELS,
    MAX_REASONING_CAPABILITY_LABEL_BYTES,
};

use codegg_protocol::core::ReasoningOverride;
use codegg_providers::capability::{
    apply_reasoning_capability_policy, CapabilityDisposition, NeutralCapabilityStatus,
    NeutralThinkingCapability, ReasoningCapabilityPolicy, ReasoningCapabilitySelection,
    WireSurface, NOTICE_REASONING_DROPPED, NOTICE_REASONING_UNCERTAIN,
};

/// The reasoning dimensions CodeGG distinguishes.
///
/// A dimension name appears in a refusal and in a notice, so it is a
/// stable, spelled-out word rather than a kernel wire field.
pub mod dimension {
    /// The provider-scoped reasoning-effort label dimension.
    pub const EFFORT: &str = "effort";
    /// The fixed extended-thinking token budget dimension.
    pub const BUDGET: &str = "budget";
    /// The explicit-disable dimension.
    pub const DISABLE: &str = "disable";
}

/// A typed local refusal of a reasoning selection.
///
/// It names the model and the dimension, so the refusal is actionable
/// instead of a provider-side 400.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningCapabilityRefusal {
    /// The model the turn would have used.
    pub model: String,
    /// The dimension that was refused.
    pub dimension: &'static str,
    /// Why the dimension was refused.
    pub reason: String,
}

impl std::fmt::Display for ReasoningCapabilityRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "model '{}' does not support the reasoning {} dimension: {}",
            self.model, self.dimension, self.reason
        )
    }
}

impl std::error::Error for ReasoningCapabilityRefusal {}

/// The local admission decision for one reasoning selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningCapabilityDecision {
    /// The selection is supported; the request may proceed unchanged.
    Allow,
    /// The selection may proceed, with a recorded warning.
    AllowWithWarning {
        /// The dimension the warning is about.
        dimension: &'static str,
        /// What the warning says.
        notice: String,
    },
    /// The unsupported dimension was dropped; the request may proceed
    /// without it, with a recorded notice naming what was dropped.
    WarnDrop {
        /// The dimension that was dropped.
        dimension: &'static str,
        /// What was dropped, and why.
        notice: String,
    },
    /// The selection is not supported; refuse it locally.
    Reject(ReasoningCapabilityRefusal),
}

impl ReasoningCapabilityDecision {
    /// The selection the request should carry after this decision.
    ///
    /// A dropped dimension leaves no reasoning control behind, so the
    /// request falls back to inheriting the model's own default. A
    /// refusal has no effective selection: the turn does not proceed.
    pub fn effective_selection(
        &self,
        selection: Option<&ReasoningOverride>,
    ) -> Option<ReasoningOverride> {
        match self {
            ReasoningCapabilityDecision::Allow
            | ReasoningCapabilityDecision::AllowWithWarning { .. } => selection.cloned(),
            ReasoningCapabilityDecision::WarnDrop { .. } => None,
            ReasoningCapabilityDecision::Reject(_) => None,
        }
    }

    /// Whether the decision refuses the turn.
    pub fn is_rejection(&self) -> bool {
        matches!(self, ReasoningCapabilityDecision::Reject(_))
    }
}

/// Which source a refused capability declaration came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilitySource {
    /// The operator's configuration override.
    OperatorOverride,
    /// The discovery overlay from provider catalog metadata.
    DiscoveryOverlay,
}

impl std::fmt::Display for CapabilitySource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CapabilitySource::OperatorOverride => write!(formatter, "operator override"),
            CapabilitySource::DiscoveryOverlay => write!(formatter, "discovery overlay"),
        }
    }
}

/// One source that was refused during resolution, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedCapabilitySource {
    /// The source that was refused.
    pub source: CapabilitySource,
    /// Why it was refused.
    pub reason: String,
}

/// The outcome of resolving the effective capability for one model.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityResolution {
    /// The effective capability.
    pub effective: ReasoningCapabilityRecord,
    /// The sources that were refused, and why.
    ///
    /// A refused source is never silent: the effective record stays as
    /// it was, and the reason is reported here.
    pub rejected: Vec<RejectedCapabilitySource>,
}

/// The restrictiveness of a capability status.
///
/// `Supported` is the most permissive and `Unsupported` the most
/// restrictive. `Conflicting` is not on the ladder — two sources that
/// disagree produce it, and the kernel treats it exactly like `Unknown`.
fn restrictiveness(status: ReasoningCapabilityStatus) -> u8 {
    match status {
        ReasoningCapabilityStatus::Supported => 0,
        ReasoningCapabilityStatus::Mixed => 1,
        ReasoningCapabilityStatus::Unknown => 2,
        ReasoningCapabilityStatus::Unsupported => 3,
        ReasoningCapabilityStatus::Conflicting => 4,
    }
}

/// Combine two declarations of one dimension, keeping the narrower one.
///
/// An undeclared dimension imposes no constraint: `Unknown` on one side
/// leaves the other side's declaration standing. When both sides
/// declare, the more restrictive status wins — so a merge can never
/// widen a dimension.
fn narrow_status(
    base: ReasoningCapabilityStatus,
    narrowing: ReasoningCapabilityStatus,
) -> ReasoningCapabilityStatus {
    if narrowing == ReasoningCapabilityStatus::Unknown {
        return base;
    }
    if base == ReasoningCapabilityStatus::Unknown {
        return narrowing;
    }
    if base == ReasoningCapabilityStatus::Conflicting
        || narrowing == ReasoningCapabilityStatus::Conflicting
    {
        return ReasoningCapabilityStatus::Conflicting;
    }
    if restrictiveness(narrowing) >= restrictiveness(base) {
        narrowing
    } else {
        base
    }
}

/// Intersect two accepted-label sets.
///
/// An undeclared set imposes no constraint. Two declared sets intersect,
/// so the result can never be wider than either side.
fn intersect_labels(
    base: Option<&Vec<String>>,
    narrowing: Option<&Vec<String>>,
) -> Option<Vec<String>> {
    match (base, narrowing) {
        (None, None) => None,
        (None, Some(other)) => Some(other.clone()),
        (Some(base), None) => Some(base.clone()),
        (Some(base), Some(other)) => Some(
            base.iter()
                .filter(|label| other.contains(label))
                .cloned()
                .collect(),
        ),
    }
}

/// Intersect two thinking-budget ranges.
fn intersect_bounds(
    base_min: Option<usize>,
    base_max: Option<usize>,
    other_min: Option<usize>,
    other_max: Option<usize>,
) -> (Option<usize>, Option<usize>) {
    let min = match (base_min, other_min) {
        (None, other) => other,
        (base, None) => base,
        (Some(base), Some(other)) => Some(base.max(other)),
    };
    let max = match (base_max, other_max) {
        (None, other) => other,
        (base, None) => base,
        (Some(base), Some(other)) => Some(base.min(other)),
    };
    (min, max)
}

/// Combine two always-reasons declarations into a consistent pair.
///
/// The always-reasons status and its value are **one fact**, not two:
/// `supported` means the model always reasons, `unsupported` means it
/// does not, and `unknown`/`mixed`/`conflicting` leave it undeclared. A
/// merged record that carried a status and a value that disagreed would
/// be an unreadable record, so the value drives the status.
///
/// `Some(true)` wins: the always-reasons property removes the off switch,
/// so it is the narrower claim, and a source cannot un-declare it.
fn merge_always_reasons(
    base_status: ReasoningCapabilityStatus,
    base_value: Option<bool>,
    other_status: ReasoningCapabilityStatus,
    other_value: Option<bool>,
) -> (ReasoningCapabilityStatus, Option<bool>) {
    let value = match (base_value, other_value) {
        (None, other) => other,
        (base, None) => base,
        (Some(true), _) => Some(true),
        (Some(false), Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
    };
    let status = match value {
        Some(true) => ReasoningCapabilityStatus::Supported,
        Some(false) => ReasoningCapabilityStatus::Unsupported,
        None => {
            let statuses = [base_status, other_status];
            if statuses.contains(&ReasoningCapabilityStatus::Conflicting) {
                ReasoningCapabilityStatus::Conflicting
            } else if statuses.contains(&ReasoningCapabilityStatus::Mixed) {
                ReasoningCapabilityStatus::Mixed
            } else {
                ReasoningCapabilityStatus::Unknown
            }
        }
    };
    (status, value)
}

/// Merge a more specific capability record onto a baseline.
///
/// The merge only ever narrows:
///
/// * the merged status of a dimension is the more restrictive of the two
///   declared statuses;
/// * the merged accepted-label set is the **intersection** of the two
///   declared sets, so a merge can never widen a dimension's accepted
///   set beyond the baseline;
/// * the merged thinking-budget range is the intersection of the two
///   declared ranges;
/// * the always-reasons property is the narrower of the two claims.
///
/// A dimension that a side does not declare imposes no constraint, so
/// the generic baseline — which declares nothing — never erases what a
/// specific adapter declares.
pub fn merge_capability(
    baseline: &ReasoningCapabilityRecord,
    specific: &ReasoningCapabilityRecord,
) -> ReasoningCapabilityRecord {
    let mut merged = baseline.clone();

    merged.effort_status = narrow_status(baseline.effort_status, specific.effort_status);
    merged.accepted_effort_labels = intersect_labels(
        baseline.accepted_effort_labels.as_ref(),
        specific.accepted_effort_labels.as_ref(),
    );
    if merged
        .accepted_effort_labels
        .as_ref()
        .is_some_and(|labels| labels.is_empty())
    {
        // The two ladders do not overlap, so no label in the baseline's
        // set is accepted: the honest status is "refused", not
        // "supported with an empty set".
        merged.accepted_effort_labels = None;
        merged.effort_status = ReasoningCapabilityStatus::Unsupported;
    }
    if merged.effort_status == ReasoningCapabilityStatus::Unsupported {
        merged.accepted_effort_labels = None;
    }

    merged.disable_status = narrow_status(baseline.disable_status, specific.disable_status);

    merged.budget_status = narrow_status(baseline.budget_status, specific.budget_status);
    let (min, max) = intersect_bounds(
        baseline.min_thinking_budget,
        baseline.max_thinking_budget,
        specific.min_thinking_budget,
        specific.max_thinking_budget,
    );
    if min.zip(max).is_some_and(|(min, max)| min > max) {
        // The two ranges do not overlap. Two sources that disagree are
        // `Conflicting`, which the kernel treats like `Unknown`.
        merged.budget_status = ReasoningCapabilityStatus::Conflicting;
        merged.min_thinking_budget = None;
        merged.max_thinking_budget = None;
    } else {
        merged.min_thinking_budget = min;
        merged.max_thinking_budget = max;
    }
    if merged.budget_status == ReasoningCapabilityStatus::Unsupported {
        merged.min_thinking_budget = None;
        merged.max_thinking_budget = None;
    }

    let (status, value) = merge_always_reasons(
        baseline.always_reasons_status,
        baseline.always_reasons,
        specific.always_reasons_status,
        specific.always_reasons,
    );
    merged.always_reasons_status = status;
    merged.always_reasons = value;

    merged
}

/// Resolve the effective reasoning capability for one model.
///
/// ADR-0049 §1's authority order: operator/config override ▸ adapter
/// baseline ▸ discovery overlay ▸ unknown.
///
/// The resolution is **read-only**: it never mutates the baseline, the
/// override, the overlay, or any in-flight turn state.
///
/// The operator's override is authoritative for the dimensions it
/// declares, and it may only narrow the compiled baseline — the merged
/// accepted-label set is the intersection of the two declared sets, so
/// an override can never widen a dimension's accepted set beyond the
/// baseline. An override that contradicts the baseline is refused, and
/// the refusal is reported rather than applied.
///
/// The discovery overlay may only narrow. An overlay that declares a
/// label, a bound, or an always-reasons value the effective record does
/// not already carry is **rejected in full**: the effective record is
/// left as it was and the rejection is reported.
pub fn resolve_reasoning_capability(
    baseline: Option<&ReasoningCapabilityRecord>,
    operator_override: Option<&ReasoningCapabilityRecord>,
    overlay: Option<&ReasoningCapabilityRecord>,
) -> CapabilityResolution {
    let mut rejected = Vec::new();
    let mut effective = baseline.cloned().unwrap_or_default();

    if let Some(over) = operator_override {
        match apply_operator_override(&effective, over) {
            Ok(merged) => effective = merged,
            Err(reason) => rejected.push(RejectedCapabilitySource {
                source: CapabilitySource::OperatorOverride,
                reason,
            }),
        }
    }

    if let Some(over) = overlay {
        match apply_discovery_overlay(&effective, over) {
            Ok(narrowed) => effective = narrowed,
            Err(reason) => rejected.push(RejectedCapabilitySource {
                source: CapabilitySource::DiscoveryOverlay,
                reason,
            }),
        }
    }

    CapabilityResolution {
        effective,
        rejected,
    }
}

/// Apply the operator's override onto the effective record.
///
/// The override is authoritative, so it replaces the effective
/// dimension it declares. Its accepted-label set is still intersected
/// with the effective one: an override may narrow a dimension's
/// accepted set, never widen it beyond the compiled baseline.
fn apply_operator_override(
    effective: &ReasoningCapabilityRecord,
    over: &ReasoningCapabilityRecord,
) -> Result<ReasoningCapabilityRecord, String> {
    // A malformed override is refused rather than trusted verbatim.
    let problems = over.validate();
    if !problems.is_empty() {
        return Err(format!(
            "the override is malformed: {}",
            problems.join("; ")
        ));
    }

    let mut merged = effective.clone();

    merged.effort_status = narrow_status(effective.effort_status, over.effort_status);
    merged.accepted_effort_labels = intersect_labels(
        effective.accepted_effort_labels.as_ref(),
        over.accepted_effort_labels.as_ref(),
    );
    if merged
        .accepted_effort_labels
        .as_ref()
        .is_some_and(|labels| labels.is_empty())
    {
        merged.accepted_effort_labels = None;
        merged.effort_status = ReasoningCapabilityStatus::Unsupported;
    }
    if merged.effort_status == ReasoningCapabilityStatus::Unsupported {
        merged.accepted_effort_labels = None;
    }

    merged.disable_status = narrow_status(effective.disable_status, over.disable_status);

    merged.budget_status = narrow_status(effective.budget_status, over.budget_status);
    let (min, max) = intersect_bounds(
        effective.min_thinking_budget,
        effective.max_thinking_budget,
        over.min_thinking_budget,
        over.max_thinking_budget,
    );
    if let (Some(min), Some(max)) = (min, max) {
        if min > max {
            return Err(format!(
                "the override's thinking-budget range ({min}..={max}) does not overlap the effective range"
            ));
        }
    }
    merged.min_thinking_budget = min;
    merged.max_thinking_budget = max;
    if merged.budget_status == ReasoningCapabilityStatus::Unsupported {
        merged.min_thinking_budget = None;
        merged.max_thinking_budget = None;
    }

    let (status, value) = merge_always_reasons(
        effective.always_reasons_status,
        effective.always_reasons,
        over.always_reasons_status,
        over.always_reasons,
    );
    merged.always_reasons_status = status;
    merged.always_reasons = value;

    Ok(merged)
}

/// Apply the discovery overlay onto the effective record.
///
/// The overlay is untrusted input, so it may only **narrow**. Every
/// dimension it declares must already be declared by the effective
/// record, must not widen it, and must not introduce a label or a bound
/// the effective record does not carry. An overlay that breaks any of
/// those rules is rejected in full.
fn apply_discovery_overlay(
    effective: &ReasoningCapabilityRecord,
    overlay: &ReasoningCapabilityRecord,
) -> Result<ReasoningCapabilityRecord, String> {
    // Untrusted input is validated before it is compared, not after.
    let problems = overlay.validate();
    if !problems.is_empty() {
        return Err(format!("the overlay is malformed: {}", problems.join("; ")));
    }

    let mut narrowed = effective.clone();

    if overlay.effort_status != ReasoningCapabilityStatus::Unknown
        || overlay.accepted_effort_labels.is_some()
    {
        if effective.effort_status == ReasoningCapabilityStatus::Unknown {
            return Err(
                "the overlay declares the effort dimension, which the effective capability does not"
                    .to_string(),
            );
        }
        if restrictiveness(overlay.effort_status) < restrictiveness(effective.effort_status) {
            return Err(format!(
                "the overlay widens the effort dimension from {:?} to {:?}",
                effective.effort_status, overlay.effort_status
            ));
        }
        if let Some(labels) = &overlay.accepted_effort_labels {
            let accepted = effective.accepted_effort_labels.as_ref().ok_or_else(|| {
                "the overlay introduces accepted effort labels the effective capability does not carry"
                    .to_string()
            })?;
            for label in labels {
                if !accepted.contains(label) {
                    return Err(format!(
                        "the overlay introduces the effort label '{label}', which the effective capability does not accept"
                    ));
                }
            }
        }
        narrowed.effort_status = narrow_status(effective.effort_status, overlay.effort_status);
        narrowed.accepted_effort_labels = intersect_labels(
            effective.accepted_effort_labels.as_ref(),
            overlay.accepted_effort_labels.as_ref(),
        );
        if narrowed
            .accepted_effort_labels
            .as_ref()
            .is_some_and(|labels| labels.is_empty())
        {
            narrowed.accepted_effort_labels = None;
            narrowed.effort_status = ReasoningCapabilityStatus::Unsupported;
        }
        if narrowed.effort_status == ReasoningCapabilityStatus::Unsupported {
            narrowed.accepted_effort_labels = None;
        }
    }

    if overlay.disable_status != ReasoningCapabilityStatus::Unknown {
        if effective.disable_status == ReasoningCapabilityStatus::Unknown {
            return Err(
                "the overlay declares the disable dimension, which the effective capability does not"
                    .to_string(),
            );
        }
        if restrictiveness(overlay.disable_status) < restrictiveness(effective.disable_status) {
            return Err(format!(
                "the overlay widens the disable dimension from {:?} to {:?}",
                effective.disable_status, overlay.disable_status
            ));
        }
        narrowed.disable_status = narrow_status(effective.disable_status, overlay.disable_status);
    }

    if overlay.budget_status != ReasoningCapabilityStatus::Unknown
        || overlay.min_thinking_budget.is_some()
        || overlay.max_thinking_budget.is_some()
    {
        if effective.budget_status == ReasoningCapabilityStatus::Unknown {
            return Err(
                "the overlay declares the budget dimension, which the effective capability does not"
                    .to_string(),
            );
        }
        if restrictiveness(overlay.budget_status) < restrictiveness(effective.budget_status) {
            return Err(format!(
                "the overlay widens the budget dimension from {:?} to {:?}",
                effective.budget_status, overlay.budget_status
            ));
        }
        if (overlay.min_thinking_budget.is_some() || overlay.max_thinking_budget.is_some())
            && effective.min_thinking_budget.is_none()
            && effective.max_thinking_budget.is_none()
        {
            return Err(
                "the overlay introduces a thinking-budget bound the effective capability does not carry"
                    .to_string(),
            );
        }
        let (min, max) = intersect_bounds(
            effective.min_thinking_budget,
            effective.max_thinking_budget,
            overlay.min_thinking_budget,
            overlay.max_thinking_budget,
        );
        if let (Some(min), Some(max)) = (min, max) {
            if min > max {
                return Err(format!(
                    "the overlay's thinking-budget range ({min}..={max}) does not overlap the effective range"
                ));
            }
        }
        narrowed.budget_status = narrow_status(effective.budget_status, overlay.budget_status);
        narrowed.min_thinking_budget = min;
        narrowed.max_thinking_budget = max;
        if narrowed.budget_status == ReasoningCapabilityStatus::Unsupported {
            narrowed.min_thinking_budget = None;
            narrowed.max_thinking_budget = None;
        }
    }

    if (overlay.always_reasons_status != ReasoningCapabilityStatus::Unknown
        || overlay.always_reasons.is_some())
        && overlay.always_reasons != effective.always_reasons
    {
        return Err(format!(
            "the overlay changes the always-reasons property from {:?} to {:?}",
            effective.always_reasons, overlay.always_reasons
        ));
    }

    Ok(narrowed)
}

/// The kernel's neutral capability facts for one record.
///
/// The kernel's `status` is the overall "does this model reason at all"
/// fact; the three per-dimension statuses are the dimensions it
/// decides. The always-reasons property is CodeGG's own — the kernel
/// has no dimension for it — and it reaches the kernel through the
/// overall status.
pub fn to_neutral_thinking_capability(
    record: &ReasoningCapabilityRecord,
) -> NeutralThinkingCapability {
    NeutralThinkingCapability {
        status: neutral_status(overall_status(record)),
        toggle: neutral_status(record.disable_status),
        effort: neutral_status(record.effort_status),
        budget: neutral_status(record.budget_status),
    }
}

/// The overall status the kernel's decision function reads.
///
/// The kernel's overall status overrides every per-dimension status
/// unless it is `Supported`, so it has to answer "does this model
/// reason at all" rather than repeat one dimension.
///
/// The always-reasons property establishes that the model reasons. Its
/// *refutation* says nothing about whether reasoning exists, so it does
/// not contribute a restrictive status.
pub fn overall_status(record: &ReasoningCapabilityRecord) -> ReasoningCapabilityStatus {
    if record.always_reasons == Some(true) {
        return ReasoningCapabilityStatus::Supported;
    }
    let statuses = [
        record.effort_status,
        record.disable_status,
        record.budget_status,
    ];
    if statuses.contains(&ReasoningCapabilityStatus::Supported) {
        return ReasoningCapabilityStatus::Supported;
    }
    if statuses.contains(&ReasoningCapabilityStatus::Mixed) {
        return ReasoningCapabilityStatus::Mixed;
    }
    if statuses.contains(&ReasoningCapabilityStatus::Conflicting) {
        return ReasoningCapabilityStatus::Conflicting;
    }
    if statuses
        .iter()
        .all(|status| *status == ReasoningCapabilityStatus::Unsupported)
    {
        return ReasoningCapabilityStatus::Unsupported;
    }
    ReasoningCapabilityStatus::Unknown
}

/// Map one status onto the kernel's own vocabulary.
///
/// The kernel's status type is not serializable, so CodeGG's record
/// carries its own mirror of the same five statuses. This is the one
/// place the two vocabularies meet.
pub fn neutral_status(status: ReasoningCapabilityStatus) -> NeutralCapabilityStatus {
    match status {
        ReasoningCapabilityStatus::Supported => NeutralCapabilityStatus::Supported,
        ReasoningCapabilityStatus::Unsupported => NeutralCapabilityStatus::Unsupported,
        ReasoningCapabilityStatus::Unknown => NeutralCapabilityStatus::Unknown,
        ReasoningCapabilityStatus::Mixed => NeutralCapabilityStatus::Mixed,
        ReasoningCapabilityStatus::Conflicting => NeutralCapabilityStatus::Conflicting,
    }
}

/// Apply the effective capability to one resolved selection.
///
/// The kernel's decision function and its policy produce the
/// disposition; CodeGG owns the accepted-label set and the budget
/// bounds, which the kernel's status-only capability type cannot carry.
///
/// An absent selection inherits the model's default and is always
/// allowed: the check exists to refuse a *selection* the model does not
/// accept, not to gate ordinary turns.
///
/// `target` is the wire surface the request will be encoded on, when the
/// caller knows it. The kernel reads it only to label its notice, so an
/// unknown surface is passed as `None` and no surface is claimed.
pub fn evaluate_reasoning_selection(
    model: &str,
    selection: Option<&ReasoningOverride>,
    capability: &ReasoningCapabilityRecord,
    policy: &ReasoningCapabilityPolicy,
    target: Option<WireSurface>,
) -> ReasoningCapabilityDecision {
    let Some(selection) = selection else {
        return ReasoningCapabilityDecision::Allow;
    };

    // CodeGG's own accepted-set check first: the kernel never validates
    // a label, so an unsupported label has to be caught here.
    match selection {
        ReasoningOverride::Effort { level } => {
            if let Some(accepted) = &capability.accepted_effort_labels {
                if !accepted.iter().any(|label| label == level) {
                    return apply_disposition(
                        policy.unsupported,
                        model,
                        dimension::EFFORT,
                        format!("the model does not accept the reasoning-effort label '{level}'"),
                    );
                }
            }
        }
        ReasoningOverride::Budget { tokens } => {
            if let Some(min) = capability.min_thinking_budget {
                if *tokens < min {
                    return apply_disposition(
                        policy.unsupported,
                        model,
                        dimension::BUDGET,
                        format!("the model's thinking budget starts at {min} tokens"),
                    );
                }
            }
            if let Some(max) = capability.max_thinking_budget {
                if *tokens > max {
                    return apply_disposition(
                        policy.unsupported,
                        model,
                        dimension::BUDGET,
                        format!("the model's thinking budget ends at {max} tokens"),
                    );
                }
            }
        }
        ReasoningOverride::Off => {}
    }

    let kernel_selection = match selection {
        ReasoningOverride::Effort { level } => ReasoningCapabilitySelection::Effort(level),
        ReasoningOverride::Budget { tokens } => {
            ReasoningCapabilitySelection::Budget(*tokens as u64)
        }
        ReasoningOverride::Off => ReasoningCapabilitySelection::Off,
    };
    let dimension = match selection {
        ReasoningOverride::Effort { .. } => dimension::EFFORT,
        ReasoningOverride::Budget { .. } => dimension::BUDGET,
        ReasoningOverride::Off => dimension::DISABLE,
    };

    let facts = to_neutral_thinking_capability(capability);
    match apply_reasoning_capability_policy(model, kernel_selection, &facts, policy, target) {
        Ok(notices) => {
            if notices
                .iter()
                .any(|notice| notice.code.0 == NOTICE_REASONING_DROPPED)
            {
                return ReasoningCapabilityDecision::WarnDrop {
                    dimension,
                    notice: format!(
                        "model '{model}' does not support the reasoning {dimension} dimension; the {dimension} selection was dropped"
                    ),
                };
            }
            if notices
                .iter()
                .any(|notice| notice.code.0 == NOTICE_REASONING_UNCERTAIN)
            {
                return ReasoningCapabilityDecision::AllowWithWarning {
                    dimension,
                    notice: format!(
                        "the reasoning {dimension} capability of model '{model}' is uncertain; the selection proceeds with a warning"
                    ),
                };
            }
            ReasoningCapabilityDecision::Allow
        }
        Err(error) => ReasoningCapabilityDecision::Reject(ReasoningCapabilityRefusal {
            model: model.to_string(),
            dimension,
            reason: match error.field.as_deref() {
                Some(field) => format!(
                    "the {dimension} dimension is not supported (kernel field '{field}', reason {:?})",
                    error.reason
                ),
                None => format!(
                    "the {dimension} dimension is not supported (kernel reason {:?})",
                    error.reason
                ),
            },
        }),
    }
}

/// Turn a kernel disposition into a local decision.
///
/// The reason states the **fact**; the disposition states what the gate did
/// with it. They are separate because the same fact produces different
/// outcomes under different dispositions: a refused turn drops nothing, so a
/// refusal that said "the selection was dropped" would claim a forward that
/// never happened.
fn apply_disposition(
    disposition: CapabilityDisposition,
    model: &str,
    dimension: &'static str,
    reason: String,
) -> ReasoningCapabilityDecision {
    match disposition {
        CapabilityDisposition::Reject => {
            ReasoningCapabilityDecision::Reject(ReasoningCapabilityRefusal {
                model: model.to_string(),
                dimension,
                reason: format!("{reason}; the turn was refused"),
            })
        }
        CapabilityDisposition::WarnDrop => ReasoningCapabilityDecision::WarnDrop {
            dimension,
            notice: format!("{reason}; the {dimension} selection was dropped"),
        },
        CapabilityDisposition::AllowWithWarning => ReasoningCapabilityDecision::AllowWithWarning {
            dimension,
            notice: format!("{reason}; the selection proceeds with a warning"),
        },
        CapabilityDisposition::Allow => ReasoningCapabilityDecision::Allow,
    }
}

/// The source of this crate's build script.
///
/// The `build.rs` mirror is a second, hand-maintained declaration of the
/// profile's shape, so the two can drift. The tests below read it the
/// same way `scripts/check_model_profile_capability_mirror.py` does, so
/// the test and the check-time gate agree on what "the same field set"
/// means.
#[cfg(test)]
pub(crate) fn build_rs_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs");
    std::fs::read_to_string(&path).expect("codegg-core's build.rs is readable")
}

/// The declared field names of one Rust struct, read from source.
///
/// Only the shapes this crate's own mirror uses are supported: a struct
/// whose body is one field per line, with doc comments and attributes
/// allowed above each field.
#[cfg(test)]
pub(crate) fn struct_fields_from_source(source: &str, struct_name: &str) -> Vec<String> {
    let marker = format!("struct {struct_name} ");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("the source does not declare `struct {struct_name}`"));
    let rest = &source[start..];
    let open = rest
        .find('{')
        .unwrap_or_else(|| panic!("`struct {struct_name}` has no body"));
    let mut depth = 0usize;
    let mut close = rest.len();
    for (index, character) in rest[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = open + index;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &rest[open + 1..close];
    let mut fields = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("//")
            || line.starts_with("#[")
            || line.starts_with("/*")
        {
            continue;
        }
        // The runtime types declare `pub` fields; the build script's
        // mirror does not.
        let line = line.strip_prefix("pub ").unwrap_or(line);
        let Some((name, _)) = line.split_once(':') else {
            continue;
        };
        fields.push(name.trim().to_string());
    }
    fields
}

/// The field names a serializable value actually declares.
///
/// This reads the *type* rather than its source, so it cannot drift the
/// way a hand-written list can.
#[cfg(test)]
pub(crate) fn serialized_field_names<T: serde::Serialize>(value: &T) -> Vec<String> {
    let value = serde_json::to_value(value).expect("the value serializes");
    value
        .as_object()
        .expect("the value serializes to an object")
        .keys()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_profile::adapter::{AdapterDefinition, BUILTIN_ADAPTER_SOURCES};

    fn labels(values: &[&str]) -> Option<Vec<String>> {
        Some(values.iter().map(|value| (*value).to_string()).collect())
    }

    /// A record whose effort dimension accepts exactly `values`.
    fn effort_record(
        status: ReasoningCapabilityStatus,
        values: Option<&[&str]>,
    ) -> ReasoningCapabilityRecord {
        ReasoningCapabilityRecord {
            effort_status: status,
            accepted_effort_labels: values
                .map(|values| values.iter().map(|value| (*value).to_string()).collect()),
            ..ReasoningCapabilityRecord::default()
        }
    }

    fn evaluate(
        model: &str,
        selection: Option<&ReasoningOverride>,
        record: &ReasoningCapabilityRecord,
        policy: &ReasoningCapabilityPolicy,
    ) -> ReasoningCapabilityDecision {
        evaluate_reasoning_selection(
            model,
            selection,
            record,
            policy,
            Some(WireSurface::OpenaiChatCompletions),
        )
    }

    #[test]
    fn the_runtime_record_and_the_build_rs_mirror_declare_the_same_fields() {
        let mirror = struct_fields_from_source(&build_rs_source(), "ReasoningCapability");
        let runtime = serialized_field_names(&ReasoningCapabilityRecord::default());
        let mut mirror = mirror;
        let mut runtime = runtime;
        mirror.sort();
        runtime.sort();
        assert_eq!(
            mirror, runtime,
            "the runtime ReasoningCapabilityRecord and the build.rs mirror declare different \
             fields; a one-sided change must be caught here, not by a deny_unknown_fields error \
             on an asset"
        );
    }

    #[test]
    fn the_shipped_adapter_assets_declare_well_formed_capability_records() {
        let mut declared = 0usize;
        for (id, source) in BUILTIN_ADAPTER_SOURCES {
            let definition: AdapterDefinition =
                toml::from_str(source).expect("a built-in adapter definition parses");
            let Some(record) = &definition.profile.reasoning_capability else {
                continue;
            };
            declared += 1;
            let problems = record.validate();
            assert!(
                problems.is_empty(),
                "adapter {id} declares a malformed capability record: {problems:?}"
            );
            if let Some(accepted) = &record.accepted_effort_labels {
                assert!(
                    accepted.len() <= MAX_REASONING_CAPABILITY_LABELS,
                    "adapter {id} declares an unbounded label list"
                );
                assert!(
                    accepted.iter().all(|label| {
                        !label.trim().is_empty()
                            && label.len() <= MAX_REASONING_CAPABILITY_LABEL_BYTES
                    }),
                    "adapter {id} declares an empty or oversized label"
                );
                let mut unique = accepted.clone();
                unique.sort();
                unique.dedup();
                assert_eq!(
                    unique.len(),
                    accepted.len(),
                    "adapter {id} declares a duplicate label"
                );
            }
            if let (Some(min), Some(max)) = (record.min_thinking_budget, record.max_thinking_budget)
            {
                assert!(
                    min <= max,
                    "adapter {id} declares a thinking-budget range whose minimum exceeds its maximum"
                );
            }
        }
        assert!(
            declared > 0,
            "no shipped adapter asset declares a reasoning capability record"
        );
    }

    #[test]
    fn an_undeclared_dimension_resolves_as_unknown_not_unsupported() {
        let record = ReasoningCapabilityRecord::default();
        assert_eq!(record.effort_status, ReasoningCapabilityStatus::Unknown);
        assert_eq!(record.disable_status, ReasoningCapabilityStatus::Unknown);
        assert_eq!(record.budget_status, ReasoningCapabilityStatus::Unknown);
        assert_eq!(
            record.always_reasons_status,
            ReasoningCapabilityStatus::Unknown
        );

        let resolution = resolve_reasoning_capability(None, None, None);
        assert_eq!(
            resolution.effective.effort_status,
            ReasoningCapabilityStatus::Unknown,
            "an absent record must not read as `unsupported`"
        );
        assert_eq!(
            overall_status(&resolution.effective),
            ReasoningCapabilityStatus::Unknown
        );

        // The fail-closed policy is what turns "unknown" into a refusal
        // rather than an optimistic forward.
        let decision = evaluate(
            "some-model",
            Some(&ReasoningOverride::Effort {
                level: "high".to_string(),
            }),
            &resolution.effective,
            &ReasoningCapabilityPolicy::default(),
        );
        assert!(
            decision.is_rejection(),
            "an unknown dimension must be refused, not forwarded: {decision:?}"
        );
    }

    #[test]
    fn the_merge_rule_narrows_and_never_widens() {
        let baseline = effort_record(
            ReasoningCapabilityStatus::Supported,
            Some(&["low", "medium", "high"]),
        );

        // A more specific adapter's narrowing wins.
        let narrower = effort_record(
            ReasoningCapabilityStatus::Supported,
            Some(&["low", "medium"]),
        );
        let merged = merge_capability(&baseline, &narrower);
        assert_eq!(merged.accepted_effort_labels, labels(&["low", "medium"]));

        // A merge can never widen a dimension's accepted set beyond the
        // baseline, even when the specific adapter claims more.
        let wider = effort_record(
            ReasoningCapabilityStatus::Supported,
            Some(&["low", "medium", "high", "turbo"]),
        );
        let merged = merge_capability(&baseline, &wider);
        assert_eq!(
            merged.accepted_effort_labels,
            labels(&["low", "medium", "high"]),
            "a merge must not widen the baseline's accepted set"
        );

        // An undeclared dimension imposes no constraint: the generic
        // baseline declares nothing and must not erase a specific
        // adapter's record.
        let merged = merge_capability(
            &ReasoningCapabilityRecord::default(),
            &effort_record(ReasoningCapabilityStatus::Supported, Some(&["low"])),
        );
        assert_eq!(merged.accepted_effort_labels, labels(&["low"]));
        assert_eq!(merged.effort_status, ReasoningCapabilityStatus::Supported);

        // A more restrictive status wins.
        let refused = effort_record(ReasoningCapabilityStatus::Unsupported, None);
        let merged = merge_capability(&baseline, &refused);
        assert_eq!(merged.effort_status, ReasoningCapabilityStatus::Unsupported);
        assert_eq!(merged.accepted_effort_labels, None);
    }

    #[test]
    fn the_kernel_policy_reaches_all_four_outcomes() {
        let default_policy = ReasoningCapabilityPolicy::default();
        let effort = ReasoningOverride::Effort {
            level: "low".to_string(),
        };

        // Allow.
        let supported = effort_record(ReasoningCapabilityStatus::Supported, Some(&["low"]));
        assert_eq!(
            evaluate("m", Some(&effort), &supported, &default_policy),
            ReasoningCapabilityDecision::Allow
        );

        // Reject — the fail-closed default for a dimension the model does
        // not support.
        let refused = effort_record(ReasoningCapabilityStatus::Unsupported, None);
        let decision = evaluate("m", Some(&effort), &refused, &default_policy);
        assert!(decision.is_rejection(), "{decision:?}");
        let ReasoningCapabilityDecision::Reject(refusal) = &decision else {
            unreachable!("the decision is a rejection");
        };
        assert_eq!(refusal.model, "m");
        assert_eq!(refusal.dimension, dimension::EFFORT);

        // WarnDrop — the default disposition for a `mixed` dimension.
        let mixed = effort_record(ReasoningCapabilityStatus::Mixed, Some(&["low"]));
        let decision = evaluate("m", Some(&effort), &mixed, &default_policy);
        match &decision {
            ReasoningCapabilityDecision::WarnDrop { dimension, notice } => {
                assert_eq!(*dimension, dimension::EFFORT);
                assert!(
                    notice.contains("effort"),
                    "a dropped dimension must be named in the notice: {notice}"
                );
            }
            other => panic!("expected a dropped dimension, got {other:?}"),
        }

        // AllowWithWarning — reachable only under a policy that says so.
        let lenient = ReasoningCapabilityPolicy {
            unsupported: CapabilityDisposition::Allow,
            unknown: CapabilityDisposition::AllowWithWarning,
            mixed: CapabilityDisposition::WarnDrop,
        };
        let uncertain = effort_record(ReasoningCapabilityStatus::Unknown, None);
        let decision = evaluate("m", Some(&effort), &uncertain, &lenient);
        assert!(
            matches!(
                decision,
                ReasoningCapabilityDecision::AllowWithWarning { .. }
            ),
            "{decision:?}"
        );

        // The default this milestone relies on is the fail-closed one.
        assert_eq!(default_policy.unsupported, CapabilityDisposition::Reject);
        assert_eq!(default_policy.unknown, CapabilityDisposition::Reject);
        assert_eq!(default_policy.mixed, CapabilityDisposition::WarnDrop);
    }

    #[test]
    fn a_hostile_overlay_cannot_widen_the_baseline() {
        let baseline = effort_record(
            ReasoningCapabilityStatus::Supported,
            Some(&["low", "medium"]),
        );

        // An overlay claiming a label the baseline does not declare is
        // rejected in full, not partially applied.
        let hostile = effort_record(
            ReasoningCapabilityStatus::Supported,
            Some(&["low", "medium", "turbo"]),
        );
        let resolution = resolve_reasoning_capability(Some(&baseline), None, Some(&hostile));
        assert_eq!(
            resolution.effective.accepted_effort_labels,
            labels(&["low", "medium"]),
            "an overlay must not widen the baseline's accepted set"
        );
        assert_eq!(resolution.rejected.len(), 1);
        assert_eq!(
            resolution.rejected[0].source,
            CapabilitySource::DiscoveryOverlay
        );

        // An overlay that tries to widen a dimension's status is rejected
        // the same way.
        let widening = effort_record(ReasoningCapabilityStatus::Supported, Some(&["low"]));
        let mut refusing = baseline.clone();
        refusing.effort_status = ReasoningCapabilityStatus::Unsupported;
        refusing.accepted_effort_labels = None;
        let resolution = resolve_reasoning_capability(Some(&refusing), None, Some(&widening));
        assert_eq!(
            resolution.effective.effort_status,
            ReasoningCapabilityStatus::Unsupported,
            "an overlay must not widen a dimension's status"
        );
        assert_eq!(resolution.rejected.len(), 1);

        // An overlay that may only narrow is applied.
        let narrower = effort_record(ReasoningCapabilityStatus::Supported, Some(&["low"]));
        let resolution = resolve_reasoning_capability(Some(&baseline), None, Some(&narrower));
        assert_eq!(
            resolution.effective.accepted_effort_labels,
            labels(&["low"])
        );
        assert!(resolution.rejected.is_empty());

        // The resolution is read-only: none of the inputs moved.
        assert_eq!(baseline.accepted_effort_labels, labels(&["low", "medium"]));
        assert_eq!(
            hostile.accepted_effort_labels,
            labels(&["low", "medium", "turbo"])
        );
    }

    #[test]
    fn the_always_reasons_property_stays_one_consistent_fact() {
        // The status and the value are one fact, so a merge may never
        // produce a record that says "unsupported" and "true" at once.
        let mut always = effort_record(ReasoningCapabilityStatus::Supported, Some(&["low"]));
        always.always_reasons_status = ReasoningCapabilityStatus::Supported;
        always.always_reasons = Some(true);
        always.disable_status = ReasoningCapabilityStatus::Unsupported;

        // A source that claims the model does *not* always reason cannot
        // widen the property back.
        let mut not_always = always.clone();
        not_always.always_reasons_status = ReasoningCapabilityStatus::Unsupported;
        not_always.always_reasons = Some(false);

        let merged = merge_capability(&always, &not_always);
        assert_eq!(merged.always_reasons, Some(true));
        assert_eq!(
            merged.always_reasons_status,
            ReasoningCapabilityStatus::Supported
        );
        assert!(
            merged.validate().is_empty(),
            "a merged record must stay internally consistent: {:?}",
            merged.validate()
        );

        let resolution = resolve_reasoning_capability(Some(&always), Some(&not_always), None);
        assert_eq!(resolution.effective.always_reasons, Some(true));
        assert_eq!(
            resolution.effective.always_reasons_status,
            ReasoningCapabilityStatus::Supported
        );
        assert!(
            resolution.effective.validate().is_empty(),
            "a resolved record must stay internally consistent: {:?}",
            resolution.effective.validate()
        );
    }

    #[test]
    fn a_malformed_override_or_overlay_is_refused_rather_than_trusted() {
        let baseline = effort_record(ReasoningCapabilityStatus::Supported, Some(&["low"]));

        // A `supported` effort dimension with no ladder cannot be checked,
        // so the override is malformed and is refused.
        let malformed = effort_record(ReasoningCapabilityStatus::Supported, None);
        let resolution = resolve_reasoning_capability(Some(&baseline), Some(&malformed), None);
        assert_eq!(
            resolution.effective.accepted_effort_labels,
            labels(&["low"]),
            "a malformed override must not be applied"
        );
        assert_eq!(resolution.rejected.len(), 1);
        assert_eq!(
            resolution.rejected[0].source,
            CapabilitySource::OperatorOverride
        );

        // The same holds for the untrusted overlay.
        let resolution = resolve_reasoning_capability(Some(&baseline), None, Some(&malformed));
        assert_eq!(
            resolution.effective.accepted_effort_labels,
            labels(&["low"]),
            "a malformed overlay must not be applied"
        );
        assert_eq!(resolution.rejected.len(), 1);
        assert_eq!(
            resolution.rejected[0].source,
            CapabilitySource::DiscoveryOverlay
        );
    }

    #[test]
    fn an_operator_override_narrows_the_baseline_and_is_reported_when_refused() {
        let baseline = effort_record(
            ReasoningCapabilityStatus::Supported,
            Some(&["low", "medium", "high"]),
        );

        // The override narrows.
        let over = effort_record(
            ReasoningCapabilityStatus::Supported,
            Some(&["low", "medium"]),
        );
        let resolution = resolve_reasoning_capability(Some(&baseline), Some(&over), None);
        assert_eq!(
            resolution.effective.accepted_effort_labels,
            labels(&["low", "medium"])
        );
        assert!(resolution.rejected.is_empty());

        // The override may not widen the baseline's accepted set.
        let over = effort_record(
            ReasoningCapabilityStatus::Supported,
            Some(&["low", "medium", "high", "turbo"]),
        );
        let resolution = resolve_reasoning_capability(Some(&baseline), Some(&over), None);
        assert_eq!(
            resolution.effective.accepted_effort_labels,
            labels(&["low", "medium", "high"]),
            "an override must not widen the baseline's accepted set"
        );

        // The override may establish a dimension the baseline is silent
        // about: there is no baseline set to widen.
        let over = effort_record(ReasoningCapabilityStatus::Supported, Some(&["low"]));
        let resolution = resolve_reasoning_capability(None, Some(&over), None);
        assert_eq!(
            resolution.effective.accepted_effort_labels,
            labels(&["low"])
        );
        assert_eq!(
            resolution.effective.effort_status,
            ReasoningCapabilityStatus::Supported
        );
    }

    #[test]
    fn the_always_reasons_family_records_its_always_reasoning_property() {
        let profile =
            crate::model_profile::resolve::infer_builtin_profile("minimax/minimax-m3.1-flash");
        let record = profile
            .reasoning_capability
            .as_ref()
            .expect("the MiniMax adapter declares a capability record");

        assert_eq!(record.always_reasons, Some(true));
        assert_eq!(
            record.always_reasons_status,
            ReasoningCapabilityStatus::Supported
        );
        assert_ne!(
            record.disable_status,
            ReasoningCapabilityStatus::Supported,
            "a model that always reasons must not offer a disable"
        );
        let accepted = record
            .accepted_effort_labels
            .as_ref()
            .expect("the MiniMax record publishes its effort ladder");
        assert_eq!(
            accepted,
            &vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
                "max".to_string()
            ],
            "the published ladder is carried verbatim"
        );
        assert!(
            !accepted.iter().any(|label| label == "none"),
            "`none` is not an accepted MiniMax effort label"
        );

        // The always-reasons property refuses an explicit disable.
        let decision = evaluate(
            "minimax/minimax-m3.1-flash",
            Some(&ReasoningOverride::Off),
            record,
            &ReasoningCapabilityPolicy::default(),
        );
        assert!(
            decision.is_rejection(),
            "an always-reasons model must refuse an explicit disable: {decision:?}"
        );
        let ReasoningCapabilityDecision::Reject(refusal) = &decision else {
            unreachable!("the decision is a rejection");
        };
        assert_eq!(refusal.dimension, dimension::DISABLE);
        assert_eq!(refusal.model, "minimax/minimax-m3.1-flash");
    }

    #[test]
    fn an_undeclared_family_carries_no_record_at_all() {
        let profile =
            crate::model_profile::resolve::infer_builtin_profile("some-provider/some-model");
        assert!(
            profile.reasoning_capability.is_none(),
            "a family with no published contract declares no record, which reads as `unknown`"
        );
    }
}
