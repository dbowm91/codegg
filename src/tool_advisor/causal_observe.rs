//! Observe-mode runtime integration for the causal frontier (M004).
//!
//! This module evaluates the selected offline frontier (M002, since M003 has
//! not closed positively) inside real request preparation in **observe-only**
//! mode. It inspects the immutable [`ResolvedToolSurface`], a bounded
//! [`CausalStateSnapshot`], and the native causal contract catalog, and
//! returns a recommendation/diagnostic only.
//!
//! Hard guarantees (graded by the replay qualification suite):
//!
//! - provider definitions and `defer_loading` bits are byte-for-byte
//!   identical with observe disabled: evaluation takes `&surface` and
//!   returns no definitions;
//! - `ResolvedToolSurface` remains the only capability ceiling: withheld
//!   (hidden/denied/disabled/parent-ceiling) tools never appear in any
//!   causal set, and uncontracted tools always stay discoverable;
//! - only bounded non-sensitive metrics are recorded: fingerprints, counts,
//!   canonical tool names, evaluation latency, and the fallback/abstention
//!   reason. No prompts, arguments, outputs, file contents, or secrets;
//! - no remote telemetry, no synchronous network I/O, no background
//!   service, no model weights;
//! - an "inadmissible" observation never blocks execution (see
//!   [`observe_tool_call`]).

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::causal_frontier::{
    causal_catalog, causal_catalog_fingerprint, CausalFrontier, CausalStateInputs,
    CausalStateSnapshot, FrontierInputs,
};
use crate::agent::tool_surface::ResolvedToolSurface;

/// Schema version for [`CausalObserveOutcome`].
pub const CAUSAL_OBSERVE_SCHEMA_VERSION: u16 = 1;
/// Per-session observe outcomes retained in memory (bounded by construction).
pub const CAUSAL_OBSERVE_WINDOW_PER_SESSION: usize = 8;
/// Maximum sessions retained in the process-local observation registry.
pub const MAX_OBSERVE_SESSIONS: usize = 256;
/// M004 runtime gate: p95 frontier computation, excluding state-store reads.
pub const CAUSAL_OBSERVE_P95_BUDGET_MS: f64 = 5.0;

/// Causal-frontier request-preparation mode. Default-off; config omission is
/// behaviorally identical to main. M005 adds the opt-in `Active` bounded
/// promotion mode; unknown values still fail closed to `Off`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CausalFrontierMode {
    #[default]
    Off,
    Observe,
    Active,
}

impl CausalFrontierMode {
    /// Parse the `[tool_advisor.causal_frontier] mode` value. Unknown and
    /// missing values are `Off` (fail-closed to current behavior).
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("observe") => Self::Observe,
            Some("active") => Self::Active,
            _ => Self::Off,
        }
    }

    /// Stable wire string for this mode.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::Active => "active",
        }
    }
}

/// Classification of one actual tool call against the computed frontier.
/// Evaluation only: every variant, including `ContractedInadmissible`, is
/// reported through diagnostics and never blocks execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedCallClass {
    RequiredCore,
    ContractedAdmissible,
    ContractedInadmissible,
    UncontractedFallback,
    AbsentFromSurface,
}

impl ObservedCallClass {
    /// Stable wire string for this class.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RequiredCore => "required_core",
            Self::ContractedAdmissible => "contracted_admissible",
            Self::ContractedInadmissible => "contracted_inadmissible",
            Self::UncontractedFallback => "uncontracted_fallback",
            Self::AbsentFromSurface => "absent_from_surface",
        }
    }
}

/// Bounded observe-mode outcome for one request preparation.
///
/// Carries fingerprints, counts, canonical tool-name sets, latency, and the
/// fallback reason only. The wire→canonical map is the surface's own
/// alias table (tool identity metadata, already bounded by surface size).
#[derive(Debug, Clone)]
pub struct CausalObserveOutcome {
    pub schema_version: u16,
    pub surface_fingerprint: String,
    pub state_fingerprint: String,
    pub contract_catalog_fingerprint: String,
    pub admissible_count: usize,
    pub inadmissible_count: usize,
    pub uncontracted_count: usize,
    pub required_count: usize,
    pub evaluation_millis: f64,
    /// `None` on a successful evaluation with structured signal;
    /// `Some("abstained_insufficient_state")` when the state carries no
    /// structured signal; `Some("frontier_error:<detail>")` when frontier
    /// construction failed closed (everything stays discoverable).
    pub fallback_reason: Option<String>,
    pub structured_signal: bool,
    pub admissible_contracted: BTreeSet<String>,
    pub inadmissible_contracted: BTreeSet<String>,
    pub uncontracted_fallback: BTreeSet<String>,
    pub required_visible: BTreeSet<String>,
    pub surface_names: BTreeSet<String>,
    pub wire_to_canonical: BTreeMap<String, String>,
}

impl CausalObserveOutcome {
    /// The deferred promotion the frontier would recommend (empty when the
    /// state abstains). Returned for diagnostics only; M004 never applies
    /// it, and M005 active disclosure intersects the admissible contracted
    /// set with the resolution-time deferred universe instead of reusing
    /// this palette-based proxy.
    pub fn deferred_promotion(&self) -> BTreeSet<String> {
        if self.structured_signal {
            self.admissible_contracted
                .iter()
                .filter(|tool| {
                    super::causal_frontier::is_palette_deferred(tool)
                        && !self.required_visible.contains(*tool)
                })
                .cloned()
                .collect()
        } else {
            BTreeSet::new()
        }
    }
}

/// Evaluate the observe-mode frontier over an already-resolved surface.
///
/// Pure and deterministic: identical inputs always produce identical
/// outcomes. The surface is borrowed and never mutated; definitions and
/// deferral bits cannot change through this path by construction.
pub fn evaluate_observe(
    surface: &ResolvedToolSurface,
    inputs: &CausalStateInputs,
) -> CausalObserveOutcome {
    let started = Instant::now();
    let snapshot = CausalStateSnapshot::from_inputs(inputs, &surface.fingerprint);
    let catalog = causal_catalog();
    let catalog_fingerprint = causal_catalog_fingerprint();
    let eligible: BTreeSet<String> = surface
        .tools
        .iter()
        .map(|tool| tool.canonical_name.clone())
        .collect();
    let required_visible: BTreeSet<String> = surface
        .tools
        .iter()
        .filter(|tool| tool.required)
        .map(|tool| tool.canonical_name.clone())
        .collect();
    let never_reduce: BTreeSet<String> = surface
        .tools
        .iter()
        .filter(|tool| tool.never_reduce)
        .map(|tool| tool.canonical_name.clone())
        .collect();
    let withheld: BTreeSet<String> = surface
        .omissions
        .iter()
        .map(|omission| omission.canonical_name.clone())
        .collect();
    let structured_signal = !snapshot.is_insufficient_state();
    let evaluation = CausalFrontier::evaluate(&FrontierInputs {
        eligible: &eligible,
        facts: &snapshot.facts,
        required_visible: &required_visible,
        never_reduce: &never_reduce,
        withheld: &withheld,
        snapshot_fingerprint: &snapshot.fingerprint,
        catalog: &catalog,
        contract_catalog_fingerprint: &catalog_fingerprint,
    });
    let evaluation_millis = started.elapsed().as_secs_f64() * 1000.0;
    let wire_to_canonical = surface.wire_to_canonical.clone();
    let base = CausalObserveOutcome {
        schema_version: CAUSAL_OBSERVE_SCHEMA_VERSION,
        surface_fingerprint: surface.fingerprint.clone(),
        state_fingerprint: snapshot.fingerprint.clone(),
        contract_catalog_fingerprint: catalog_fingerprint,
        admissible_count: 0,
        inadmissible_count: 0,
        uncontracted_count: 0,
        required_count: required_visible.len() + never_reduce.len(),
        evaluation_millis,
        fallback_reason: None,
        structured_signal,
        admissible_contracted: BTreeSet::new(),
        inadmissible_contracted: BTreeSet::new(),
        uncontracted_fallback: BTreeSet::new(),
        required_visible: required_visible
            .iter()
            .chain(never_reduce.iter())
            .cloned()
            .collect(),
        surface_names: eligible.clone(),
        wire_to_canonical,
    };
    match evaluation {
        Ok(frontier) => {
            debug_assert!(frontier
                .is_fresh_against(&snapshot.fingerprint, &base.contract_catalog_fingerprint));
            CausalObserveOutcome {
                admissible_count: frontier.admissible_contracted.len(),
                inadmissible_count: frontier.inadmissible_contracted.len(),
                uncontracted_count: frontier.uncontracted_fallback.len(),
                admissible_contracted: frontier.admissible_contracted,
                inadmissible_contracted: frontier.inadmissible_contracted.keys().cloned().collect(),
                uncontracted_fallback: frontier.uncontracted_fallback,
                fallback_reason: (!structured_signal)
                    .then(|| "abstained_insufficient_state".to_string()),
                ..base
            }
        }
        Err(error) => CausalObserveOutcome {
            // Fail closed to the fallback universe: every eligible tool
            // stays discoverable and nothing is suppressed.
            uncontracted_fallback: eligible,
            fallback_reason: Some(format!("frontier_error:{error}")),
            ..base
        },
    }
}

/// Classify one actual tool call against a computed outcome.
///
/// Canonicalizes through the outcome's wire→canonical map first, so aliases
/// share the same verdict as their canonical tool. Total: unknown tools are
/// `AbsentFromSurface`, never an error.
pub fn classify_observed_call(
    outcome: &CausalObserveOutcome,
    tool_name: &str,
) -> ObservedCallClass {
    let canonical = outcome
        .wire_to_canonical
        .get(tool_name)
        .map(String::as_str)
        .unwrap_or(tool_name);
    if outcome.required_visible.contains(canonical) {
        ObservedCallClass::RequiredCore
    } else if outcome.admissible_contracted.contains(canonical) {
        ObservedCallClass::ContractedAdmissible
    } else if outcome.inadmissible_contracted.contains(canonical) {
        ObservedCallClass::ContractedInadmissible
    } else if outcome.uncontracted_fallback.contains(canonical)
        || outcome.surface_names.contains(canonical)
    {
        ObservedCallClass::UncontractedFallback
    } else {
        ObservedCallClass::AbsentFromSurface
    }
}

// ─── Session-local observation window ────────────────────────────────────

/// In-memory per-session observe outcomes. Bounded: at most
/// [`CAUSAL_OBSERVE_WINDOW_PER_SESSION`] outcomes per session and
/// [`MAX_OBSERVE_SESSIONS`] sessions; the oldest session is evicted past
/// the cap. Holds fingerprints, counts, and canonical names only.
static OBSERVE_WINDOWS: LazyLock<Mutex<HashMap<String, VecDeque<CausalObserveOutcome>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Record one observe outcome for a session. No-op when `outcome` is `None`
/// (observe disabled).
pub fn record_observe_outcome(session_id: &str, outcome: CausalObserveOutcome) {
    let Ok(mut windows) = OBSERVE_WINDOWS.lock() else {
        return;
    };
    if !windows.contains_key(session_id) && windows.len() >= MAX_OBSERVE_SESSIONS {
        // Evict one arbitrary oldest session; HashMap order is unspecified
        // but the bound, not the victim, is what matters here.
        if let Some(victim) = windows.keys().next().cloned() {
            windows.remove(&victim);
        }
    }
    let queue = windows.entry(session_id.to_string()).or_default();
    queue.push_back(outcome);
    while queue.len() > CAUSAL_OBSERVE_WINDOW_PER_SESSION {
        queue.pop_front();
    }
}

/// Latest observe outcome for a session, if any.
pub fn latest_observe_outcome(session_id: &str) -> Option<CausalObserveOutcome> {
    OBSERVE_WINDOWS
        .lock()
        .ok()?
        .get(session_id)?
        .back()
        .cloned()
}

/// Compare one actual tool call against the session's latest observe
/// outcome and report the class through diagnostics.
///
/// Evaluation only: an inadmissible observation never blocks execution.
/// No-op when observe recorded nothing for the session.
pub fn observe_tool_call(session_id: &str, tool_name: &str) {
    let Some(outcome) = latest_observe_outcome(session_id) else {
        return;
    };
    let class = classify_observed_call(&outcome, tool_name);
    tracing::debug!(
        scope = "causal_frontier_observe",
        session_observed = true,
        tool = %tool_name,
        class = %class.as_str(),
        surface_fingerprint = %outcome.surface_fingerprint,
        state_fingerprint = %outcome.state_fingerprint,
        "observed actual tool call against causal frontier"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tool_surface::ToolOmissionReason;

    #[test]
    fn frontier_mode_parses_default_off() {
        assert_eq!(CausalFrontierMode::parse(None), CausalFrontierMode::Off);
        assert_eq!(
            CausalFrontierMode::parse(Some("observe")),
            CausalFrontierMode::Observe
        );
        assert_eq!(
            CausalFrontierMode::parse(Some("off")),
            CausalFrontierMode::Off
        );
        // Unknown values fail closed to off.
        assert_eq!(
            CausalFrontierMode::parse(Some("promote")),
            CausalFrontierMode::Off
        );
        assert_eq!(
            CausalFrontierMode::parse(Some("active")),
            CausalFrontierMode::Active
        );
        assert_eq!(CausalFrontierMode::default(), CausalFrontierMode::Off);
    }

    /// Minimal resolved surface for observe unit tests: `read` (required,
    /// contracted, always admissible), `commit` (contracted, inadmissible
    /// without unmet commit acceptance), and one uncontracted MCP tool.
    fn unit_surface() -> ResolvedToolSurface {
        use crate::provider::ToolDefinition;
        let definitions = ["read", "commit", "mcp__ext__helper"]
            .iter()
            .map(|name| ToolDefinition {
                name: (*name).to_string(),
                description: format!("{name} tool"),
                parameters: serde_json::json!({"type": "object"}),
                defer_loading: None,
            })
            .collect::<Vec<_>>();
        let mut surface = ResolvedToolSurface::resolve(
            definitions,
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            false,
            None,
        )
        .expect("unit surface resolves");
        // Mirror production: `read` is required/never-reduce on the resolved
        // surface.
        for tool in &mut surface.tools {
            if tool.canonical_name == "read" {
                tool.required = true;
                tool.never_reduce = true;
            }
        }
        surface
            .omissions
            .push(crate::agent::tool_surface::ToolOmission {
                canonical_name: "denied_tool".to_string(),
                reason: ToolOmissionReason::Denied,
            });
        surface
    }

    #[test]
    fn observe_never_mutates_or_suppresses() {
        let surface = unit_surface();
        let before = surface.definitions();
        let outcome = evaluate_observe(&surface, &CausalStateInputs::default());
        // Definitions are byte-identical with observe on: evaluation borrows.
        assert_eq!(
            format!("{:?}", surface.definitions()),
            format!("{before:?}")
        );
        assert_eq!(outcome.surface_fingerprint, surface.fingerprint);
        // No state: abstain, nothing promoted, everything discoverable.
        assert_eq!(
            outcome.fallback_reason.as_deref(),
            Some("abstained_insufficient_state")
        );
        assert!(!outcome.structured_signal);
        assert!(outcome.deferred_promotion().is_empty());
        // Required bypass holds; withheld tools never appear anywhere.
        assert!(outcome.required_visible.contains("read"));
        for set in [
            &outcome.admissible_contracted,
            &outcome.inadmissible_contracted,
            &outcome.uncontracted_fallback,
            &outcome.required_visible,
        ] {
            assert!(!set.contains("denied_tool"));
        }
        // Unknown contracted tools stay in the fallback universe.
        assert!(outcome.uncontracted_fallback.contains("mcp__ext__helper"));
        assert!(
            classify_observed_call(&outcome, "mcp__ext__helper")
                == ObservedCallClass::UncontractedFallback
        );
        assert!(classify_observed_call(&outcome, "read") == ObservedCallClass::RequiredCore);
        assert!(
            classify_observed_call(&outcome, "no_such_tool")
                == ObservedCallClass::AbsentFromSurface
        );
        assert!(outcome.evaluation_millis < CAUSAL_OBSERVE_P95_BUDGET_MS);
    }

    #[test]
    fn observe_classifies_against_structured_state() {
        let surface = unit_surface();
        let inputs = CausalStateInputs {
            unmet_commit_acceptance: true,
            ..Default::default()
        };
        let outcome = evaluate_observe(&surface, &inputs);
        assert!(outcome.fallback_reason.is_none());
        assert!(outcome.structured_signal);
        assert!(outcome.admissible_contracted.contains("commit"));
        assert_eq!(
            classify_observed_call(&outcome, "commit"),
            ObservedCallClass::ContractedAdmissible
        );

        let with_error = CausalStateInputs {
            unmet_commit_acceptance: true,
            unresolved_error_count: 1,
            ..Default::default()
        };
        let outcome = evaluate_observe(&surface, &with_error);
        assert!(outcome.inadmissible_contracted.contains("commit"));
        assert_eq!(
            classify_observed_call(&outcome, "commit"),
            ObservedCallClass::ContractedInadmissible
        );
    }
}
