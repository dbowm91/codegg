//! Bounded active causal disclosure for the M005 qualification.
//!
//! This module evaluates the selected offline frontier (M002, since M003 has
//! not closed positively) inside real request preparation in **active**
//! mode and returns a bounded promotion-only disclosure decision. Active
//! behavior is promotion-only:
//!
//! - required/core/current contextual tools remain exactly as today;
//! - at most [`CAUSAL_ACTIVE_MAX_PROMOTIONS`] causally admissible deferred
//!   tools may be promoted immediately, within
//!   [`CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES`] total promoted schema bytes;
//! - contracted inadmissible deferred tools remain discoverable via
//!   `tool_search`; uncontracted tools are never promoted and remain
//!   discoverable;
//! - no tool becomes hidden, callable, or authorized because of this layer:
//!   promotion moves a tool from the deferred universe to the immediate
//!   set only; the [`ResolvedToolSurface`] stays the only capability
//!   ceiling.
//!
//! Selection is deterministic: greedy canonical-name order over the
//! admissible contracted candidates inside the resolved deferred universe,
//! skipping candidates that would exceed the remaining byte budget. No
//! scores, no ranking model, no threshold tuning (frozen in
//! `assets/tool-advisor/causal-frontier-m005-freeze.json`).
//!
//! Hard guarantees (graded by the M005 qualification suite):
//!
//! - no promotion outside the resolved deferred universe passed by the
//!   caller;
//! - no promotion on insufficient structured signal (abstain) or when the
//!   candidate set holds no admissible contracted deferred tool;
//! - byte budget enforced per preparation;
//! - only bounded non-sensitive data is recorded: fingerprints, counts,
//!   canonical tool names, evaluation latency, and the promotion reason. No
//!   prompts, arguments, outputs, file contents, or secrets;
//! - no remote telemetry, no synchronous network I/O, no background
//!   service, no model weights.

use std::collections::{BTreeMap, BTreeSet};

use super::causal_frontier::CausalStateInputs;
use super::causal_observe::{evaluate_observe, CausalObserveOutcome};
use crate::agent::tool_surface::ResolvedToolSurface;

/// Schema version for [`CausalActiveOutcome`].
pub const CAUSAL_ACTIVE_SCHEMA_VERSION: u16 = 1;
/// M005 frozen bound: maximum causally promoted deferred tools per
/// preparation.
pub const CAUSAL_ACTIVE_MAX_PROMOTIONS: usize = 2;
/// M005 frozen bound: maximum total promoted schema bytes per preparation
/// (serialized JSON bytes of the promoted `ToolDefinition` parameters).
pub const CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES: usize = 16 * 1024;
/// M005 runtime gate: p95 active evaluation, excluding state-store reads.
pub const CAUSAL_ACTIVE_P95_BUDGET_MS: f64 = 5.0;

/// Bounded active-disclosure outcome for one request preparation.
#[derive(Debug, Clone)]
pub struct CausalActiveOutcome {
    pub schema_version: u16,
    /// The underlying observe-mode evaluation (fingerprints, counts,
    /// canonical sets, fallback reason). Reused for session diagnostics so
    /// the broker-side classification keeps working under active mode.
    pub observe: CausalObserveOutcome,
    /// Canonical names promoted from deferred to immediate, in ascending
    /// order. Empty unless [`CausalActiveOutcome::promotion_applied`].
    pub promoted: BTreeSet<String>,
    /// Serialized schema bytes summed over [`CausalActiveOutcome::promoted`].
    pub promoted_schema_bytes: usize,
    /// Whether the caller should change provider definitions. When false,
    /// definitions must remain exactly as without causal promotion.
    pub promotion_applied: bool,
    /// Machine-readable reason when [`CausalActiveOutcome::promotion_applied`]
    /// is false: `abstained_insufficient_state`,
    /// `frontier_error:<detail>` (fail closed, everything discoverable),
    /// `no_admissible_deferred_in_universe`, or
    /// `schema_budget_exhausted`.
    pub no_change_reason: Option<String>,
    /// Wall-clock milliseconds for observe evaluation plus bound
    /// application, excluding state-store reads.
    pub active_evaluation_millis: f64,
}

/// Apply the frozen M005 bound to admissible contracted deferred
/// candidates: greedy canonical-name order, skip candidates that would
/// exceed the remaining byte budget, stop at
/// [`CAUSAL_ACTIVE_MAX_PROMOTIONS`]. Candidates without a recorded byte
/// size are skipped (bytes cannot be accounted, so promotion would be
/// unbounded). Pure and deterministic.
pub fn bound_active_promotion(
    candidates: &BTreeSet<String>,
    schema_bytes: &BTreeMap<String, usize>,
) -> (BTreeSet<String>, usize) {
    let mut selected = BTreeSet::new();
    let mut total_bytes = 0usize;
    for candidate in candidates {
        if selected.len() >= CAUSAL_ACTIVE_MAX_PROMOTIONS {
            break;
        }
        let Some(bytes) = schema_bytes.get(candidate) else {
            continue;
        };
        if total_bytes.saturating_add(*bytes) > CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES {
            continue;
        }
        selected.insert(candidate.clone());
        total_bytes = total_bytes.saturating_add(*bytes);
    }
    (selected, total_bytes)
}

/// Evaluate bounded active disclosure over an already-resolved surface.
///
/// The surface is borrowed and never mutated; the outcome carries only the
/// promotion decision. `deferred_schema_bytes` is the resolved deferred
/// universe for this preparation: canonical tool name to serialized
/// parameters bytes, covering exactly the definitions the caller would
/// otherwise defer. Anything outside that map can never be promoted.
pub fn evaluate_active(
    surface: &ResolvedToolSurface,
    inputs: &CausalStateInputs,
    deferred_schema_bytes: &BTreeMap<String, usize>,
) -> CausalActiveOutcome {
    let started = std::time::Instant::now();
    let observe = evaluate_observe(surface, inputs);
    let base = CausalActiveOutcome {
        schema_version: CAUSAL_ACTIVE_SCHEMA_VERSION,
        observe,
        promoted: BTreeSet::new(),
        promoted_schema_bytes: 0,
        promotion_applied: false,
        no_change_reason: None,
        active_evaluation_millis: 0.0,
    };
    // Insufficient structured signal abstains: no promotion claim is
    // justified, and the fallback universe is already fully discoverable.
    if !base.observe.structured_signal {
        if let Some(error) = base
            .observe
            .fallback_reason
            .as_deref()
            .filter(|reason| reason.starts_with("frontier_error:"))
        {
            let reason = error.to_string();
            return CausalActiveOutcome {
                no_change_reason: Some(reason),
                active_evaluation_millis: started.elapsed().as_secs_f64() * 1000.0,
                ..base
            };
        }
        return CausalActiveOutcome {
            no_change_reason: Some("abstained_insufficient_state".to_string()),
            active_evaluation_millis: started.elapsed().as_secs_f64() * 1000.0,
            ..base
        };
    }
    // Promotion domain: admissible contracted tools inside the resolved
    // deferred universe only. This deliberately does NOT reuse the
    // palette-based `deferred_promotion` helper: the static palette is the
    // offline proxy for "would be deferred", but a palette-core tool can
    // still be deferred in a given resolved preparation, and only the
    // resolution-time universe bounds active disclosure (frozen M005
    // contract). Required and never-reduce tools never reach the admissible
    // set (they bypass), and the subtraction below states the exclusion
    // explicitly. Uncontracted tools never enter the admissible set.
    let candidates: BTreeSet<String> = base
        .observe
        .admissible_contracted
        .iter()
        .filter(|tool| {
            !base.observe.required_visible.contains(*tool)
                && deferred_schema_bytes.contains_key(*tool)
        })
        .cloned()
        .collect();
    if candidates.is_empty() {
        return CausalActiveOutcome {
            no_change_reason: Some("no_admissible_deferred_in_universe".to_string()),
            active_evaluation_millis: started.elapsed().as_secs_f64() * 1000.0,
            ..base
        };
    }
    let (promoted, promoted_schema_bytes) =
        bound_active_promotion(&candidates, deferred_schema_bytes);
    if promoted.is_empty() {
        return CausalActiveOutcome {
            no_change_reason: Some("schema_budget_exhausted".to_string()),
            active_evaluation_millis: started.elapsed().as_secs_f64() * 1000.0,
            ..base
        };
    }
    let active_evaluation_millis = started.elapsed().as_secs_f64() * 1000.0;
    CausalActiveOutcome {
        promoted,
        promoted_schema_bytes,
        promotion_applied: true,
        no_change_reason: None,
        active_evaluation_millis,
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
        entries
            .iter()
            .map(|(name, size)| ((*name).to_string(), *size))
            .collect()
    }

    fn candidates(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn bound_stops_at_two_in_canonical_order() {
        let (selected, total) = bound_active_promotion(
            &candidates(&["work_plan_get", "commit", "goal_get"]),
            &bytes(&[("work_plan_get", 100), ("commit", 100), ("goal_get", 100)]),
        );
        assert_eq!(
            selected,
            candidates(&["commit", "goal_get"]),
            "greedy canonical order takes the first two"
        );
        assert_eq!(total, 200);
    }

    #[test]
    fn bound_skips_over_budget_candidates_and_continues() {
        let (selected, total) = bound_active_promotion(
            &candidates(&["aaa_big", "commit"]),
            &bytes(&[
                ("aaa_big", CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES + 1),
                ("commit", 120),
            ]),
        );
        assert_eq!(selected, candidates(&["commit"]));
        assert_eq!(total, 120);
    }

    #[test]
    fn bound_skips_candidates_without_recorded_bytes() {
        let (selected, total) = bound_active_promotion(
            &candidates(&["commit", "ghost"]),
            &bytes(&[("commit", 120)]),
        );
        assert_eq!(selected, candidates(&["commit"]));
        assert_eq!(total, 120);
    }

    #[test]
    fn bound_applies_cumulative_budget() {
        let big = CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES - 10;
        let (selected, total) = bound_active_promotion(
            &candidates(&["a_first", "b_second"]),
            &bytes(&[("a_first", big), ("b_second", 20)]),
        );
        assert_eq!(selected, candidates(&["a_first"]));
        assert_eq!(total, big);
    }

    #[test]
    fn bound_empty_candidates_select_nothing() {
        let (selected, total) = bound_active_promotion(&candidates(&[]), &bytes(&[("commit", 1)]));
        assert!(selected.is_empty());
        assert_eq!(total, 0);
    }

    /// Minimal resolved surface: admissible deferred `commit` under unmet
    /// commit acceptance, plus an always-immediate `read`.
    fn active_unit_surface() -> ResolvedToolSurface {
        use crate::provider::ToolDefinition;
        let definitions = ["commit", "read"]
            .iter()
            .map(|name| ToolDefinition {
                name: (*name).to_string(),
                description: format!("{name} tool"),
                parameters: serde_json::json!({"type": "object"}),
                defer_loading: if *name == "commit" { Some(true) } else { None },
            })
            .collect::<Vec<_>>();
        ResolvedToolSurface::resolve(
            definitions,
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            false,
            false,
            None,
        )
        .expect("unit surface resolves")
    }

    #[test]
    fn active_promotes_admissible_deferred_within_budget() {
        let surface = active_unit_surface();
        let inputs = CausalStateInputs {
            has_active_work_plan: true,
            has_actionable_item: true,
            unmet_commit_acceptance: true,
            ..Default::default()
        };
        let outcome = evaluate_active(&surface, &inputs, &bytes(&[("commit", 420)]));
        assert!(outcome.promotion_applied);
        assert_eq!(outcome.promoted, candidates(&["commit"]));
        assert_eq!(outcome.promoted_schema_bytes, 420);
        assert!(outcome.no_change_reason.is_none());
    }

    #[test]
    fn active_reports_budget_exhaustion_without_changing_anything() {
        let surface = active_unit_surface();
        let inputs = CausalStateInputs {
            has_active_work_plan: true,
            has_actionable_item: true,
            unmet_commit_acceptance: true,
            ..Default::default()
        };
        let outcome = evaluate_active(
            &surface,
            &inputs,
            &bytes(&[("commit", CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES + 1)]),
        );
        assert!(!outcome.promotion_applied);
        assert!(outcome.promoted.is_empty());
        assert_eq!(
            outcome.no_change_reason.as_deref(),
            Some("schema_budget_exhausted")
        );
    }

    #[test]
    fn active_abstains_without_structured_signal() {
        let surface = active_unit_surface();
        let outcome = evaluate_active(
            &surface,
            &CausalStateInputs::default(),
            &bytes(&[("commit", 420)]),
        );
        assert!(!outcome.promotion_applied);
        assert_eq!(
            outcome.no_change_reason.as_deref(),
            Some("abstained_insufficient_state")
        );
    }
}
