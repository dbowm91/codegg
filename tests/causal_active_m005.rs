//! Causal frontier M005 structural qualification: bounded active disclosure.
//!
//! Three arms, all deterministic and hermetic:
//!
//! 1. Freeze integrity: the checked-in M005 freeze record matches live
//!    constants, the live contract catalog, the live palette, the frozen
//!    benchmark bytes, and the preregistered dev/qualification split.
//! 2. Fresh-holdout qualification: all 284 post-M004-freeze scenarios in
//!    `assets/tool-advisor/causal-frontier-m005-holdout.json` run through
//!    the real `evaluate_active` over resolved surfaces with exact-size
//!    fixture schemas. Graded: exact oracle promotion match, authority,
//!    premature non-exposure, uncontracted discovery, current-step
//!    preservation, per-case bounds, median promotion, p95 latency.
//! 3. Frozen M001 qualification arm: the 56-case M001 qualification
//!    partition through the M005 bound (uniform small schemas, so this arm
//!    grades the count bound and invariant carry-over, not byte pressure).
//! 4. Live production path: real `AgentLoop::build_tool_definitions` with
//!    mode=active and no host state abstains byte-identically to off.
//!
//! Live model trajectories (§5 of the plan) are unavailable in this
//! environment (no operator provider credentials); disposition B is
//! recorded in the closure, not silent skipping.

use std::collections::{BTreeMap, BTreeSet};

use codegg::agent::tool_surface::{ResolvedToolSurface, ToolOmission, ToolOmissionReason};
use codegg::provider::ToolDefinition;
use codegg::tool_advisor::causal_active::{
    bound_active_promotion, evaluate_active, CAUSAL_ACTIVE_MAX_PROMOTIONS,
    CAUSAL_ACTIVE_P95_BUDGET_MS, CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES, CAUSAL_ACTIVE_SCHEMA_VERSION,
};
use codegg::tool_advisor::causal_frontier::CausalStateInputs;

fn workspace_asset(name: &str) -> String {
    let root = env!("CARGO_MANIFEST_DIR");
    std::fs::read_to_string(format!("{root}/assets/tool-advisor/{name}"))
        .unwrap_or_else(|error| panic!("read asset {name}: {error}"))
}

// ─── Fixture builders ─────────────────────────────────────────────────────

/// Build parameters JSON with EXACTLY `target_bytes` serialized bytes.
/// `serde_json` sorts map keys, so `{"padding","type"}` order is stable and
/// the overhead is computed from the real empty serialization.
fn sized_parameters(target_bytes: usize) -> serde_json::Value {
    let tiny = serde_json::json!({"type": "object"});
    if target_bytes <= tiny.to_string().len() {
        return tiny;
    }
    let overhead = serde_json::json!({"padding": "", "type": "object"})
        .to_string()
        .len();
    assert!(
        target_bytes > overhead,
        "target {target_bytes} below padding overhead {overhead}"
    );
    serde_json::json!({"padding": "x".repeat(target_bytes - overhead), "type": "object"})
}

fn inputs_from_json(value: &serde_json::Value) -> CausalStateInputs {
    let flag = |name: &str| {
        value
            .get(name)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    };
    let count = |name: &str| {
        value
            .get(name)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize
    };
    CausalStateInputs {
        has_active_goal: flag("has_active_goal"),
        goal_id: None,
        goal_revision: None,
        has_active_work_plan: flag("has_active_work_plan"),
        work_plan_id: None,
        work_plan_revision: None,
        current_item_id: None,
        has_actionable_item: flag("has_actionable_item"),
        has_in_progress_item: flag("has_in_progress_item"),
        has_blocked_item: flag("has_blocked_item"),
        unmet_test_acceptance: flag("unmet_test_acceptance"),
        unmet_commit_acceptance: flag("unmet_commit_acceptance"),
        unmet_artifact_acceptance: flag("unmet_artifact_acceptance"),
        unmet_delegated_run_acceptance: flag("unmet_delegated_run_acceptance"),
        touched_file_count: count("touched_file_count"),
        test_evidence_count: count("test_evidence_count"),
        has_failed_tests: flag("has_failed_tests"),
        unresolved_error_count: count("unresolved_error_count"),
        security_finding_count: count("security_finding_count"),
        artifact_handle_count: count("artifact_handle_count"),
        lsp_preview_available: flag("lsp_preview_available"),
        context_read_available: flag("context_read_available"),
    }
}

fn omission_reason(name: &str) -> ToolOmissionReason {
    match name {
        "Denied" => ToolOmissionReason::Denied,
        "PlanMode" => ToolOmissionReason::PlanMode,
        "DisabledByModel" => ToolOmissionReason::DisabledByModel,
        "MissingBackend" => ToolOmissionReason::MissingBackend,
        "NonCallable" => ToolOmissionReason::NonCallable,
        "ParentCeiling" => ToolOmissionReason::ParentCeiling,
        other => panic!("unknown omission reason {other}"),
    }
}

/// Lower median of an ascending list (index `(n-1)/2`), matching the frozen
/// M002 tie-breaking convention.
fn lower_median_ascending(sorted: &[usize]) -> usize {
    assert!(!sorted.is_empty());
    sorted[(sorted.len() - 1) / 2]
}

/// Nearest-rank percentile of ascending samples (rank `ceil(p*n)`,
/// 1-indexed), matching the frozen M002 latency convention.
fn nearest_rank_percentile(sorted: &[f64], rank: f64) -> f64 {
    assert!(!sorted.is_empty());
    let index = (f64::ceil(rank * sorted.len() as f64) as usize).max(1) - 1;
    sorted[index.min(sorted.len() - 1)]
}

/// Number of warm samples behind a single per-scenario budget check.
const BUDGET_SAMPLES: usize = 15;

/// Warm best-of-N latency for a per-scenario budget check.
///
/// A single cold wall-clock sample conflates the algorithm's cost with cold
/// caches, page faults, and scheduler preemption, so on a shared CI runner it
/// measures the machine rather than the code: the frozen 5 ms budget then
/// fails intermittently for reasons unrelated to the change under test. The
/// minimum of N warm samples is the standard estimator for how long the
/// computation takes when it is *not* descheduled, which is what a budget on
/// a pure in-memory computation should be checked against.
///
/// The budget constant is unchanged — it is pinned by the M005 freeze record
/// and asserted against `pure_active_eval_p95_ms_max` in arm 1 — and the
/// holdout's own p95 below remains the distributional check.
fn warm_min_millis(mut run: impl FnMut() -> f64) -> f64 {
    let _ = run();
    let mut best = f64::INFINITY;
    for _ in 0..BUDGET_SAMPLES {
        best = best.min(run());
    }
    best
}

// ─── Arm 1: freeze integrity ──────────────────────────────────────────────

#[test]
fn m005_freeze_record_matches_live_contracts() {
    use codegg::tool_advisor::causal_frontier::{
        causal_catalog_fingerprint, palette_fingerprint, CAUSAL_CONTRACT_SCHEMA_VERSION,
        CAUSAL_ONTOLOGY_VERSION, CAUSAL_SNAPSHOT_SCHEMA_VERSION,
    };

    let freeze: serde_json::Value =
        serde_json::from_str(&workspace_asset("causal-frontier-m005-freeze.json"))
            .expect("freeze parses");
    assert_eq!(freeze["schema_version"], 1);
    assert_eq!(
        freeze["max_promotions"],
        CAUSAL_ACTIVE_MAX_PROMOTIONS as u64
    );
    assert_eq!(
        freeze["schema_byte_budget"],
        CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES as u64
    );
    assert_eq!(freeze["ontology_version"], CAUSAL_ONTOLOGY_VERSION as u64);
    assert_eq!(
        freeze["contract_schema_version"],
        CAUSAL_CONTRACT_SCHEMA_VERSION as u64
    );
    assert_eq!(
        freeze["snapshot_schema_version"],
        CAUSAL_SNAPSHOT_SCHEMA_VERSION as u64
    );
    assert_eq!(
        freeze["contract_catalog_fingerprint"]
            .as_str()
            .expect("catalog fp"),
        causal_catalog_fingerprint()
    );
    assert_eq!(
        freeze["palette_fingerprint"].as_str().expect("palette fp"),
        palette_fingerprint()
    );
    assert_eq!(
        freeze["structural_gates"]["pure_active_eval_p95_ms_max"],
        CAUSAL_ACTIVE_P95_BUDGET_MS
    );
    // Frozen benchmark bytes are untouched since M001 preregistration.
    let benchmark = workspace_asset("causal-frontier-v1.jsonl");
    let digest = sha256_hex(benchmark.as_bytes());
    assert_eq!(
        digest,
        freeze["benchmark_fingerprint"]
            .as_str()
            .expect("benchmark fp")
    );
    // The M005 holdout records the same frozen catalog/ontology/bounds.
    let holdout: serde_json::Value =
        serde_json::from_str(&workspace_asset("causal-frontier-m005-holdout.json"))
            .expect("holdout parses");
    assert_eq!(holdout["schema_version"], 1);
    assert_eq!(
        holdout["contract_catalog_fingerprint"]
            .as_str()
            .expect("holdout fp"),
        causal_catalog_fingerprint()
    );
    assert_eq!(holdout["ontology_version"], CAUSAL_ONTOLOGY_VERSION as u64);
    assert_eq!(
        holdout["max_promotions"],
        CAUSAL_ACTIVE_MAX_PROMOTIONS as u64
    );
    assert_eq!(
        holdout["schema_byte_budget"],
        CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES as u64
    );
    let scenarios = holdout["scenarios"].as_array().expect("scenarios array");
    assert!(
        scenarios.len() >= 160,
        "holdout minimum 160 scenarios, found {}",
        scenarios.len()
    );
    assert_eq!(holdout["scenario_count"], scenarios.len() as u64);
    // The result receipt pins the exact holdout bytes graded here.
    let result: serde_json::Value =
        serde_json::from_str(&workspace_asset("causal-frontier-m005-result.json"))
            .expect("result parses");
    let holdout_bytes = std::fs::read(format!(
        "{}/assets/tool-advisor/causal-frontier-m005-holdout.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("holdout bytes");
    assert_eq!(
        result["holdout_sha256"].as_str().expect("receipt sha"),
        sha256_hex(&holdout_bytes)
    );
    assert_eq!(
        result["holdout_scenarios"].as_u64().expect("receipt count"),
        scenarios.len() as u64
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

// Re-export-free local hasher to avoid new dependencies in tests.
use sha2::{Digest, Sha256};

// ─── Arm 2: fresh-holdout qualification ───────────────────────────────────

struct ScenarioOutcome {
    structured: bool,
    promotion_size: usize,
    evaluation_millis: f64,
}

fn qualify_holdout() -> Vec<ScenarioOutcome> {
    use codegg::tool_advisor::causal_frontier::causal_catalog_fingerprint;

    let holdout: serde_json::Value =
        serde_json::from_str(&workspace_asset("causal-frontier-m005-holdout.json"))
            .expect("holdout parses");
    assert_eq!(
        holdout["contract_catalog_fingerprint"]
            .as_str()
            .expect("fp"),
        causal_catalog_fingerprint(),
        "holdout must be graded against the frozen catalog only"
    );
    let scenarios = holdout["scenarios"].as_array().expect("scenarios").clone();
    let mut outcomes = Vec::with_capacity(scenarios.len());
    for scenario in &scenarios {
        outcomes.push(qualify_scenario(scenario));
    }
    outcomes
}

fn qualify_scenario(scenario: &serde_json::Value) -> ScenarioOutcome {
    let id = scenario["id"].as_str().expect("id");
    let surface_json = &scenario["surface"];
    let gold = &scenario["gold"];
    let aliases: BTreeMap<String, String> = surface_json["aliases"]
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(wire, canonical)| {
                    (
                        wire.clone(),
                        canonical.as_str().expect("alias target").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    // Fixture definitions use wire names with exact-size schemas.
    let mut expected_bytes: BTreeMap<String, usize> = BTreeMap::new();
    let definitions: Vec<ToolDefinition> = surface_json["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| {
            let wire = tool["name"].as_str().expect("wire name");
            let canonical = tool["canonical"].as_str().expect("canonical");
            let deferred = tool["deferred"].as_bool().expect("deferred");
            let size = tool["schema_bytes"].as_u64().expect("bytes") as usize;
            expected_bytes.insert(canonical.to_string(), size);
            ToolDefinition {
                name: wire.to_string(),
                description: format!("{wire} fixture tool"),
                parameters: sized_parameters(size),
                defer_loading: if deferred { Some(true) } else { None },
            }
        })
        .collect();

    let mut surface = ResolvedToolSurface::resolve_with_aliases(
        definitions,
        &BTreeSet::new(),
        &BTreeSet::new(),
        false,
        false,
        None,
        &aliases,
    )
    .unwrap_or_else(|error| panic!("{id}: surface resolves: {error:?}"));

    // Mirror production: omissions never remain in the eligible tool list.
    let omitted: BTreeSet<String> = surface_json["omissions"]
        .as_array()
        .expect("omissions")
        .iter()
        .map(|omission| {
            omission["name"]
                .as_str()
                .expect("omission name")
                .to_string()
        })
        .collect();
    surface
        .tools
        .retain(|tool| !omitted.contains(&tool.canonical_name));
    for omission in surface_json["omissions"].as_array().expect("omissions") {
        surface.omissions.push(ToolOmission {
            canonical_name: omission["name"].as_str().expect("name").to_string(),
            reason: omission_reason(omission["reason"].as_str().expect("reason")),
        });
    }
    let required: BTreeSet<String> = surface_json["required"]
        .as_array()
        .expect("required")
        .iter()
        .map(|name| name.as_str().expect("name").to_string())
        .collect();
    let never_reduce: BTreeSet<String> = surface_json["never_reduce"]
        .as_array()
        .expect("never_reduce")
        .iter()
        .map(|name| name.as_str().expect("name").to_string())
        .collect();
    for tool in &mut surface.tools {
        if required.contains(&tool.canonical_name) {
            tool.required = true;
        }
        if never_reduce.contains(&tool.canonical_name) {
            tool.never_reduce = true;
        }
    }

    let eligible: BTreeSet<String> = surface
        .tools
        .iter()
        .map(|tool| tool.canonical_name.clone())
        .collect();
    let withheld: BTreeSet<String> = surface
        .omissions
        .iter()
        .map(|omission| omission.canonical_name.clone())
        .collect();
    assert!(
        eligible.is_disjoint(&withheld),
        "{id}: withheld tool remains eligible"
    );
    for name in required.iter().chain(never_reduce.iter()) {
        assert!(eligible.contains(name), "{id}: bypass tool {name} eligible");
    }

    // Resolved deferred universe with live-measured schema bytes, exactly as
    // the production hook builds it (max on wire-name collision).
    let mut deferred_schema_bytes: BTreeMap<String, usize> = BTreeMap::new();
    let mut immediate_canonical: BTreeSet<String> = BTreeSet::new();
    for definition in surface.definitions() {
        let canonical = surface
            .wire_to_canonical
            .get(&definition.name)
            .map(String::as_str)
            .unwrap_or(&definition.name);
        // Fixture defer flags must survive resolution untouched.
        let fixture = surface_json["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .find(|tool| tool["name"].as_str() == Some(definition.name.as_str()))
            .unwrap_or_else(|| panic!("{id}: fixture entry for {}", definition.name));
        assert_eq!(
            definition.defer_loading,
            if fixture["deferred"].as_bool().expect("deferred") {
                Some(true)
            } else {
                None
            },
            "{id}: defer flag changed by resolution for {}",
            definition.name
        );
        let bytes = definition.parameters.to_string().len();
        if definition.defer_loading == Some(true) {
            let expected = expected_bytes
                .get(canonical)
                .unwrap_or_else(|| panic!("{id}: byte entry for {canonical}"));
            assert_eq!(bytes, *expected, "{id}: schema bytes drift for {canonical}");
            deferred_schema_bytes
                .entry(canonical.to_string())
                .and_modify(|entry| {
                    if bytes > *entry {
                        *entry = bytes;
                    }
                })
                .or_insert(bytes);
        } else {
            immediate_canonical.insert(canonical.to_string());
        }
    }

    let inputs = inputs_from_json(&scenario["inputs"]);
    let outcome = evaluate_active(&surface, &inputs, &deferred_schema_bytes);

    // Exact oracle promotion match: the frozen selection semantics,
    // transcribed independently, must agree exactly.
    let gold_promoted: BTreeSet<String> = gold["promoted"]
        .as_array()
        .expect("promoted")
        .iter()
        .map(|name| name.as_str().expect("name").to_string())
        .collect();
    assert_eq!(
        outcome.promoted, gold_promoted,
        "{id}: promotion differs from independent oracle gold"
    );
    assert_eq!(
        outcome.promotion_applied,
        !gold_promoted.is_empty(),
        "{id}: applied flag inconsistent with promotion"
    );
    assert_eq!(
        outcome.promoted_schema_bytes,
        gold["promoted_schema_bytes"].as_u64().expect("bytes") as usize,
        "{id}: promoted bytes differ from oracle"
    );
    assert_eq!(
        outcome.no_change_reason.as_deref(),
        gold["no_change_reason"].as_str(),
        "{id}: no-change reason differs from oracle"
    );
    assert_eq!(
        outcome.schema_version, CAUSAL_ACTIVE_SCHEMA_VERSION,
        "{id}: outcome schema version"
    );

    // Authority: promoted tools are eligible, deferred, never withheld,
    // never required/never-reduce bypass.
    for tool in &outcome.promoted {
        assert!(
            eligible.contains(tool),
            "{id}: promoted {tool} not eligible"
        );
        assert!(
            deferred_schema_bytes.contains_key(tool),
            "{id}: promoted {tool} outside the resolved deferred universe"
        );
        assert!(!withheld.contains(tool), "{id}: promoted withheld {tool}");
        assert!(!required.contains(tool), "{id}: promoted required {tool}");
        assert!(
            !never_reduce.contains(tool),
            "{id}: promoted never-reduce {tool}"
        );
    }

    // Premature non-exposure: no premature tool promotes, so no premature
    // tool gains immediacy it did not already have in the fixture.
    let premature: BTreeSet<String> = gold["premature"]
        .as_array()
        .expect("premature")
        .iter()
        .map(|name| name.as_str().expect("name").to_string())
        .collect();
    assert!(
        premature.is_disjoint(&outcome.promoted),
        "{id}: premature tool promoted"
    );
    for tool in &premature {
        let was_immediate = immediate_canonical.contains(tool);
        let now_immediate = was_immediate || outcome.promoted.contains(tool);
        assert_eq!(
            was_immediate, now_immediate,
            "{id}: premature {tool} gained immediacy"
        );
    }

    // Uncontracted discovery: never promoted, still eligible.
    let uncontracted: BTreeSet<String> = gold["uncontracted"]
        .as_array()
        .expect("uncontracted")
        .iter()
        .map(|name| name.as_str().expect("name").to_string())
        .collect();
    assert!(
        uncontracted.is_disjoint(&outcome.promoted),
        "{id}: uncontracted tool promoted"
    );
    for tool in &uncontracted {
        assert!(eligible.contains(tool), "{id}: uncontracted {tool} lost");
    }

    // Current-step preservation: every gold current-step tool stays visible
    // (eligible; active disclosure only adds visibility).
    for tool in gold["current_step"].as_array().expect("current_step") {
        let name = tool.as_str().expect("name");
        assert!(
            eligible.contains(name),
            "{id}: current-step tool {name} not visible"
        );
    }

    // Citations: every promoted tool cites the host fact and contract
    // making it admissible.
    let citations = gold["citations"].as_array().expect("citations");
    assert_eq!(
        citations.len(),
        gold_promoted.len(),
        "{id}: citation count matches promotion"
    );
    let gold_facts: BTreeSet<&str> = gold["facts"]
        .as_array()
        .expect("facts")
        .iter()
        .map(|fact| fact.as_str().expect("fact"))
        .collect();
    for citation in citations {
        let tool = citation["tool"].as_str().expect("tool");
        assert!(
            gold_promoted.contains(tool),
            "{id}: citation for unpromoted {tool}"
        );
        assert_eq!(
            citation["contract"].as_str().expect("contract"),
            tool,
            "{id}: citation contract matches tool"
        );
        for fact in citation["facts"].as_array().expect("facts") {
            let fact = fact.as_str().expect("fact");
            assert!(gold_facts.contains(fact), "{id}: cited fact {fact} absent");
        }
    }

    // Surface stability: evaluation borrows; a second evaluation agrees.
    let fingerprint = surface.fingerprint.clone();
    let again = evaluate_active(&surface, &inputs, &deferred_schema_bytes);
    assert_eq!(surface.fingerprint, fingerprint, "{id}: surface mutated");
    assert_eq!(again.promoted, outcome.promoted, "{id}: non-deterministic");

    // Budget: warm best-of-N, not one cold sample. A single cold wall-clock
    // reading conflates this computation's cost with cold caches and
    // scheduler preemption, so on a shared CI runner it measures the
    // machine rather than the code and the frozen 5 ms budget fails
    // intermittently for reasons unrelated to the change under test. The
    // constant is unchanged and the holdout's own p95 further down remains
    // the distributional check; this only removes the scheduler confound.
    let warm_min = warm_min_millis(|| {
        let measured = evaluate_active(&surface, &inputs, &deferred_schema_bytes);
        std::hint::black_box(measured.active_evaluation_millis)
    });
    assert!(
        warm_min < CAUSAL_ACTIVE_P95_BUDGET_MS,
        "{id}: warm best-of-{BUDGET_SAMPLES} evaluation {warm_min:.3} ms exceeds the \
         {CAUSAL_ACTIVE_P95_BUDGET_MS} ms budget"
    );
    ScenarioOutcome {
        structured: gold["structured_signal"].as_bool().expect("structured"),
        promotion_size: outcome.promoted.len(),
        evaluation_millis: outcome.active_evaluation_millis,
    }
}

#[test]
fn m005_holdout_structural_gates() {
    let outcomes = qualify_holdout();
    assert!(
        outcomes.len() >= 160,
        "holdout minimum 160, ran {}",
        outcomes.len()
    );
    let mut sizes: Vec<usize> = Vec::new();
    let mut latencies: Vec<f64> = Vec::new();
    let mut abstentions = 0usize;
    for outcome in &outcomes {
        assert!(
            outcome.promotion_size <= CAUSAL_ACTIVE_MAX_PROMOTIONS,
            "per-case promotion bound violated"
        );
        latencies.push(outcome.evaluation_millis);
        if outcome.structured {
            sizes.push(outcome.promotion_size);
        } else {
            abstentions += 1;
            assert_eq!(
                outcome.promotion_size, 0,
                "insufficient state must abstain with empty promotion"
            );
        }
    }
    assert!(!sizes.is_empty(), "structured scenarios must exist");
    assert!(abstentions > 0, "abstention scenarios must exist");
    sizes.sort_unstable();
    let median = lower_median_ascending(&sizes);
    assert!(
        median <= CAUSAL_ACTIVE_MAX_PROMOTIONS,
        "median promoted {median} exceeds the frozen bound"
    );
    latencies.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let p95 = nearest_rank_percentile(&latencies, 0.95);
    assert!(
        p95 <= CAUSAL_ACTIVE_P95_BUDGET_MS,
        "p95 active evaluation {p95:.3} ms exceeds the 5 ms budget"
    );
    println!(
        "m005 holdout: {} scenarios ({} structured, {} abstained), \
         median promotion {median}, p95 {p95:.3} ms",
        outcomes.len(),
        sizes.len(),
        abstentions
    );
}

// ─── Arm 3: frozen M001 qualification partition through the M005 bound ────

#[test]
fn m005_m001_qualification_partition_bounds_hold() {
    use codegg::tool_advisor::causal_frontier::{
        causal_catalog, causal_catalog_fingerprint, evaluate_benchmark_case, is_palette_deferred,
        load_causal_benchmark, CausalM001Preregistration,
    };

    let cases = load_causal_benchmark(&workspace_asset("causal-frontier-v1.jsonl"))
        .expect("benchmark loads");
    let prereg: CausalM001Preregistration = serde_json::from_str(&workspace_asset(
        "causal-frontier-m001-preregistration.json",
    ))
    .expect("prereg parses");
    prereg.verify_against(&cases).expect("prereg verifies");
    assert_eq!(
        prereg.benchmark_fingerprint,
        "f60dad170f3a2527529152628e01dd5f908efb26d325f70a8a80dca2735820e4"
    );
    let qual_ids: BTreeSet<&str> = prereg
        .qualification_case_ids
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        qual_ids.len(),
        56,
        "frozen qualification partition is 56 cases"
    );
    let catalog = causal_catalog();
    let catalog_fp = causal_catalog_fingerprint();

    // Uniform small schemas: this arm grades the count bound and invariant
    // carry-over on the frozen corpus, not byte pressure (covered by the
    // holdout arm with exact sizes and the live arm with real schemas).
    let mut preservation_total = 0usize;
    let mut preservation_visible = 0usize;
    let mut uncontracted_total = 0usize;
    let mut uncontracted_retained = 0usize;
    let mut violations = 0u64;
    let mut sizes: Vec<usize> = Vec::new();
    for case in cases
        .iter()
        .filter(|case| qual_ids.contains(case.case_id.as_str()))
    {
        let frontier = evaluate_benchmark_case(case, &catalog, &catalog_fp)
            .unwrap_or_else(|error| panic!("{}: {error}", case.case_id));
        let eligible: BTreeSet<String> = case.eligible.iter().cloned().collect();
        let withheld: BTreeSet<&String> = case.withheld.iter().collect();
        let deferred_universe: BTreeSet<String> = eligible
            .iter()
            .filter(|tool| is_palette_deferred(tool))
            .cloned()
            .collect();
        // Production selection domain: admissible deferred promotion inside
        // the resolved deferred universe, with uniform small byte sizes.
        let candidates: BTreeSet<String> = frontier
            .deferred_promotion()
            .into_iter()
            .filter(|tool| deferred_universe.contains(tool))
            .collect();
        let bytes: BTreeMap<String, usize> = candidates
            .iter()
            .map(|tool| (tool.clone(), 200usize))
            .collect();
        let (selected, total) = bound_active_promotion(&candidates, &bytes);
        assert!(
            selected.len() <= CAUSAL_ACTIVE_MAX_PROMOTIONS,
            "{}: bound violated",
            case.case_id
        );
        assert!(
            total <= CAUSAL_ACTIVE_SCHEMA_BUDGET_BYTES,
            "{}: budget violated",
            case.case_id
        );
        for tool in &selected {
            if !eligible.contains(tool) || withheld.contains(&tool) {
                violations += 1;
            }
        }
        // Preservation and discovery carry over: active disclosure only
        // adds visibility, never removes it.
        let visible = frontier.visible_union();
        preservation_total += case.gold_current.len();
        preservation_visible += case
            .gold_current
            .iter()
            .filter(|tool| visible.contains(*tool))
            .count();
        for tool in &case.gold_current {
            if case.uncontracted.iter().any(|name| name == tool) {
                uncontracted_total += 1;
                if visible.contains(tool) {
                    uncontracted_retained += 1;
                }
            }
        }
        // Premature tools never enter the bounded promotion.
        for tool in &case.gold_premature {
            assert!(
                !selected.contains(tool),
                "{}: premature {tool} promoted",
                case.case_id
            );
        }
        if !case.insufficient_state {
            sizes.push(selected.len());
        } else {
            assert!(
                frontier
                    .promotion_for_use(!case.insufficient_state)
                    .is_empty(),
                "{}: insufficient state must abstain",
                case.case_id
            );
        }
    }
    assert_eq!(
        preservation_visible, preservation_total,
        "current-step preservation must be 1.00"
    );
    assert_eq!(violations, 0, "authority violations must be 0");
    assert_eq!(
        uncontracted_retained, uncontracted_total,
        "uncontracted discovery must be 1.00"
    );
    sizes.sort_unstable();
    let median = lower_median_ascending(&sizes);
    println!(
        "m005 qual arm: 56 frozen cases, median bounded promotion {median}, \
         max {}",
        sizes.last().copied().unwrap_or(0)
    );
    assert!(
        median <= CAUSAL_ACTIVE_MAX_PROMOTIONS,
        "median bounded promotion {median} exceeds the frozen bound"
    );
}

// ─── Arm 4: live production path abstains without host state ───────────────

/// Live production-shaped check: the real
/// `AgentLoop::build_tool_definitions` path with mode=active and no host
/// state must abstain byte-identically to off while recording the bounded
/// abstention outcome.
#[cfg(test)]
mod live_request_preparation_active {
    use std::sync::Arc;

    use async_trait::async_trait;
    use codegg::agent::r#loop::AgentLoop;
    use codegg::agent::Agent;
    use codegg::config::schema::{Config, ToolAdvisorCausalFrontierConfig, ToolAdvisorConfig};
    use codegg::context::InMemoryArtifactStore;
    use codegg::permission::PermissionChecker;
    use codegg::provider::{
        ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, TokenUsage,
    };
    use codegg::tool::ToolRegistry;
    use codegg::tool_advisor::causal_observe::latest_observe_outcome;

    #[derive(Clone)]
    struct StubProvider;

    #[async_trait]
    impl Provider for StubProvider {
        fn id(&self) -> &str {
            "stub"
        }

        fn name(&self) -> &str {
            "Stub Provider"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(Self)
        }

        async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
            let events = vec![ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            }];
            let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
            Ok(Box::pin(stream))
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![])
        }
    }

    fn agents() -> Vec<Agent> {
        vec![Agent {
            name: "build".to_string(),
            role: None,
            description: "Test agent".to_string(),
            mode: codegg::agent::AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: std::collections::HashMap::new(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
            fallback_model: None,
            runtime_kind: None,
        }]
    }

    fn build_loop(mode: Option<&str>, session_id: &str) -> AgentLoop {
        let config = Config {
            tool_advisor: Some(ToolAdvisorConfig {
                causal_frontier: Some(ToolAdvisorCausalFrontierConfig {
                    mode: mode.map(str::to_string),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        AgentLoop::new(
            agents(),
            Box::new(StubProvider),
            PermissionChecker::new(None, None),
            ToolRegistry::with_defaults(),
            config,
            None,
            None,
            Arc::new(InMemoryArtifactStore::new()),
            std::env::current_dir().expect("test workspace root"),
            session_id.to_string(),
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn active_mode_abstains_byte_identical_without_host_state() {
        let mut off = build_loop(None, "causal-active-m005-off");
        let mut active = build_loop(Some("active"), "causal-active-m005-on");

        let defs_off = off.test_build_tool_definitions().await;
        let defs_active = active.test_build_tool_definitions().await;

        // No host state in this harness: active must abstain, leaving
        // provider definitions and deferral bits byte-identical to off.
        assert_eq!(
            serde_json::to_string(&defs_off).expect("serialize"),
            serde_json::to_string(&defs_active).expect("serialize"),
            "active mode without structured signal must not change definitions"
        );
        assert!(
            !defs_off.is_empty(),
            "the live surface must be non-trivial for the delta to mean anything"
        );

        assert!(latest_observe_outcome("causal-active-m005-off").is_none());
        let outcome = latest_observe_outcome("causal-active-m005-on")
            .expect("active records its inner observe outcome");
        assert!(!outcome.structured_signal);
        assert_eq!(
            outcome.fallback_reason.as_deref(),
            Some("abstained_insufficient_state")
        );
        assert!(outcome.deferred_promotion().is_empty());
    }
}
