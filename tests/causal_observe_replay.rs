//! Causal frontier M004 replay qualification: deterministic
//! request-preparation replay fixtures.
//!
//! Each fixture resolves a `ResolvedToolSurface` (the production authority
//! ceiling) and evaluates the observe-mode frontier over a scripted
//! host-owned state. The suite grades the M004 invariants:
//!
//! - definitions identical with observe off/on (evaluation borrows);
//! - resolved surface fingerprint identical across evaluations;
//! - causal result canonical-name stable across aliases;
//! - unknown (uncontracted) tools remain deferred/discoverable;
//! - hidden/denied/disabled/parent-ceiling tools never appear in any
//!   causal set (zero authority violations);
//! - 100% fallback preservation for uncontracted tools;
//! - p95 frontier computation within the 5 ms budget;
//! - the evaluated catalog is the qualified M002 catalog.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use codegg::agent::tool_surface::{AgentCapabilitySet, ResolvedToolSurface};
use codegg::provider::ToolDefinition;
use codegg::tool_advisor::causal_frontier::{causal_catalog_fingerprint, CausalStateInputs};
use codegg::tool_advisor::causal_observe::{
    classify_observed_call, evaluate_observe, CausalObserveOutcome, ObservedCallClass,
    CAUSAL_OBSERVE_P95_BUDGET_MS,
};

fn tool_definition(name: &str, defer: bool) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: format!("{name} tool"),
        parameters: serde_json::json!({"type": "object"}),
        defer_loading: if defer { Some(true) } else { None },
    }
}

/// Production-shaped definition set: contracted pilot natives, uncontracted
/// MCP/plugin tools, and one unknown deferred tool.
fn base_definitions() -> Vec<ToolDefinition> {
    let mut definitions = vec![
        tool_definition("read", false),
        tool_definition("glob", false),
        tool_definition("grep", true),
        tool_definition("commit", true),
        tool_definition("context_read", true),
        tool_definition("goal_get", true),
        tool_definition("work_plan_get", true),
        tool_definition("work_plan_update_item", true),
        tool_definition("lsp_preview_apply", true),
        tool_definition("test", true),
        tool_definition("write", true),
    ];
    definitions.push(tool_definition("mcp__ext__search", true));
    definitions.push(tool_definition("my_plugin__do", true));
    definitions.push(tool_definition("future_tool", true));
    definitions
}

#[allow(clippy::too_many_arguments)]
fn replay_surface(
    denied: BTreeSet<String>,
    disabled: BTreeSet<String>,
    plan_mode: bool,
    ceiling: Option<AgentCapabilitySet>,
    aliases: BTreeMap<String, String>,
) -> ResolvedToolSurface {
    ResolvedToolSurface::resolve_with_aliases(
        base_definitions(),
        &denied,
        &disabled,
        plan_mode,
        false,
        ceiling,
        &aliases,
    )
    .expect("replay surface resolves")
}

fn plain_surface() -> ResolvedToolSurface {
    replay_surface(
        BTreeSet::new(),
        BTreeSet::new(),
        false,
        None,
        BTreeMap::new(),
    )
}

/// Definitions are byte-identical with observe off/on: evaluation borrows
/// the surface and returns no definitions.
fn assert_definitions_untouched(surface: &ResolvedToolSurface, outcome: &CausalObserveOutcome) {
    let before = format!("{:?}", surface.definitions());
    let again = evaluate_observe(surface, &CausalStateInputs::default());
    assert_eq!(format!("{:?}", surface.definitions()), before);
    assert_eq!(outcome.surface_fingerprint, surface.fingerprint);
    assert_eq!(outcome.surface_fingerprint, again.surface_fingerprint);
    // Deferral bits survive observe untouched.
    for definition in surface.definitions() {
        if matches!(
            definition.name.as_str(),
            "future_tool" | "mcp__ext__search" | "my_plugin__do"
        ) {
            assert_eq!(definition.defer_loading, Some(true));
        }
    }
}

/// Zero authority violations + 100% fallback preservation for one outcome.
fn assert_authority_and_fallback(surface: &ResolvedToolSurface, outcome: &CausalObserveOutcome) {
    let withheld: BTreeSet<String> = surface
        .omissions
        .iter()
        .map(|omission| omission.canonical_name.clone())
        .collect();
    for set in [
        &outcome.admissible_contracted,
        &outcome.inadmissible_contracted,
        &outcome.uncontracted_fallback,
        &outcome.required_visible,
    ] {
        assert!(
            set.is_disjoint(&withheld),
            "withheld tool reached the causal result"
        );
    }
    // Every eligible uncontracted tool stays discoverable.
    let catalog = codegg::tool_advisor::causal_frontier::causal_catalog();
    for tool in surface
        .tools
        .iter()
        .map(|tool| tool.canonical_name.clone())
        .filter(|name| !catalog.contains_key(name))
        .filter(|name| !outcome.required_visible.contains(name))
    {
        assert!(
            outcome.uncontracted_fallback.contains(&tool),
            "uncontracted tool {tool} lost its fallback universe"
        );
    }
}

#[test]
fn replay_no_state_abstains_with_full_fallback() {
    let surface = plain_surface();
    let outcome = evaluate_observe(&surface, &CausalStateInputs::default());
    assert_definitions_untouched(&surface, &outcome);
    assert_authority_and_fallback(&surface, &outcome);
    assert_eq!(
        outcome.fallback_reason.as_deref(),
        Some("abstained_insufficient_state")
    );
    assert!(outcome.deferred_promotion().is_empty());
    assert_eq!(
        classify_observed_call(&outcome, "read"),
        ObservedCallClass::RequiredCore
    );
}

#[test]
fn replay_active_goal_admits_goal_tools() {
    let surface = plain_surface();
    let inputs = CausalStateInputs {
        has_active_goal: true,
        goal_id: Some("goal-1".to_string()),
        goal_revision: Some(3),
        ..Default::default()
    };
    let outcome = evaluate_observe(&surface, &inputs);
    assert_definitions_untouched(&surface, &outcome);
    assert_authority_and_fallback(&surface, &outcome);
    assert!(outcome.fallback_reason.is_none());
    assert!(outcome.admissible_contracted.contains("goal_get"));
    assert_eq!(
        classify_observed_call(&outcome, "goal_get"),
        ObservedCallClass::ContractedAdmissible
    );
}

#[test]
fn replay_workplan_phases_gate_item_updates() {
    let surface = plain_surface();
    let inputs = CausalStateInputs {
        has_active_work_plan: true,
        work_plan_id: Some("wp-1".to_string()),
        has_actionable_item: true,
        ..Default::default()
    };
    // Without unmet acceptance the update contract still holds: an
    // actionable item in a non-terminal plan satisfies the requires_any
    // group.
    let outcome = evaluate_observe(&surface, &inputs);
    assert_definitions_untouched(&surface, &outcome);
    assert_authority_and_fallback(&surface, &outcome);
    assert!(outcome.admissible_contracted.contains("work_plan_get"));
    assert!(outcome
        .admissible_contracted
        .contains("work_plan_update_item"));
}

#[test]
fn replay_artifacts_admit_context_read() {
    let surface = plain_surface();
    let inputs = CausalStateInputs {
        artifact_handle_count: 2,
        context_read_available: true,
        ..Default::default()
    };
    let outcome = evaluate_observe(&surface, &inputs);
    assert_definitions_untouched(&surface, &outcome);
    assert_authority_and_fallback(&surface, &outcome);
    assert!(outcome.admissible_contracted.contains("context_read"));
}

#[test]
fn replay_unresolved_errors_forbid_commit() {
    let surface = plain_surface();
    let mut inputs = CausalStateInputs {
        unmet_commit_acceptance: true,
        unresolved_error_count: 1,
        ..Default::default()
    };
    let outcome = evaluate_observe(&surface, &inputs);
    assert_definitions_untouched(&surface, &outcome);
    assert_authority_and_fallback(&surface, &outcome);
    assert!(outcome.inadmissible_contracted.contains("commit"));
    assert_eq!(
        classify_observed_call(&outcome, "commit"),
        ObservedCallClass::ContractedInadmissible
    );
    // ... while the same acceptance without the error admits it.
    inputs.unresolved_error_count = 0;
    let outcome = evaluate_observe(&surface, &inputs);
    assert!(outcome.admissible_contracted.contains("commit"));
}

#[test]
fn replay_failed_tests_are_state_only() {
    let surface = plain_surface();
    let mut inputs = CausalStateInputs {
        test_evidence_count: 3,
        ..Default::default()
    };
    inputs.set_failed_tests(true);
    let outcome = evaluate_observe(&surface, &inputs);
    assert_definitions_untouched(&surface, &outcome);
    assert_authority_and_fallback(&surface, &outcome);
    // `test` carries no state preconditions, so it stays admissible; the
    // failed-test fact only shapes contracts that consume it.
    assert!(outcome.admissible_contracted.contains("test"));
}

#[test]
fn replay_preview_apply_needs_a_staged_preview() {
    let surface = plain_surface();
    let inputs = CausalStateInputs {
        lsp_preview_available: true,
        ..Default::default()
    };
    let outcome = evaluate_observe(&surface, &inputs);
    assert_definitions_untouched(&surface, &outcome);
    assert_authority_and_fallback(&surface, &outcome);
    assert!(outcome.admissible_contracted.contains("lsp_preview_apply"));
    let without = evaluate_observe(&surface, &CausalStateInputs::default());
    assert!(without
        .inadmissible_contracted
        .contains("lsp_preview_apply"));
}

#[test]
fn replay_uncontracted_tools_stay_discoverable() {
    let surface = plain_surface();
    let outcome = evaluate_observe(&surface, &CausalStateInputs::default());
    assert_definitions_untouched(&surface, &outcome);
    assert_authority_and_fallback(&surface, &outcome);
    for unknown in ["mcp__ext__search", "my_plugin__do", "future_tool"] {
        assert!(
            outcome.uncontracted_fallback.contains(unknown),
            "{unknown} must remain in the fallback universe"
        );
        assert_eq!(
            classify_observed_call(&outcome, unknown),
            ObservedCallClass::UncontractedFallback
        );
    }
}

#[test]
fn replay_denied_disabled_and_ceiling_tools_never_appear() {
    for (denied, disabled, ceiling) in [
        (
            BTreeSet::from(["commit".to_string()]),
            BTreeSet::new(),
            None,
        ),
        (BTreeSet::new(), BTreeSet::from(["glob".to_string()]), None),
        (
            BTreeSet::new(),
            BTreeSet::new(),
            Some(AgentCapabilitySet::default()),
        ),
    ] {
        let surface = replay_surface(denied, disabled, false, ceiling, BTreeMap::new());
        assert!(
            !surface.omissions.is_empty(),
            "fixture must omit at least one tool"
        );
        let outcome = evaluate_observe(&surface, &CausalStateInputs::default());
        assert_definitions_untouched(&surface, &outcome);
        assert_authority_and_fallback(&surface, &outcome);
        for omission in &surface.omissions {
            assert_eq!(
                classify_observed_call(&outcome, &omission.canonical_name),
                ObservedCallClass::AbsentFromSurface,
                "omitted tool {} classified as callable",
                omission.canonical_name
            );
        }
    }
}

#[test]
fn replay_aliases_are_canonical_name_stable() {
    // The provider-facing wire name `g` aliases canonical `grep`: alias
    // tables apply to definitions present on the surface.
    let mut definitions = base_definitions();
    definitions.retain(|definition| definition.name != "grep");
    definitions.push(tool_definition("g", true));
    let aliases = BTreeMap::from([("g".to_string(), "grep".to_string())]);
    let surface = ResolvedToolSurface::resolve_with_aliases(
        definitions,
        &BTreeSet::new(),
        &BTreeSet::new(),
        false,
        false,
        None,
        &aliases,
    )
    .expect("aliased surface resolves");
    assert_eq!(
        surface.wire_to_canonical.get("g").map(String::as_str),
        Some("grep")
    );
    let outcome = evaluate_observe(&surface, &CausalStateInputs::default());
    assert_definitions_untouched(&surface, &outcome);
    assert_authority_and_fallback(&surface, &outcome);
    // The causal result carries canonical names only, stable across aliases.
    assert!(!outcome.uncontracted_fallback.contains("g"));
    assert_eq!(
        classify_observed_call(&outcome, "g"),
        classify_observed_call(&outcome, "grep")
    );
}

#[test]
fn replay_uses_the_qualified_m002_catalog() {
    let receipt: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("assets/tool-advisor/causal-frontier-m002-result.json")
            .expect("M002 receipt exists"),
    )
    .expect("M002 receipt parses");
    assert_eq!(
        receipt["disposition"], "A",
        "M004 builds on the positive M002 disposition"
    );
    assert_eq!(
        causal_catalog_fingerprint(),
        receipt["contract_catalog_fingerprint"]
            .as_str()
            .expect("receipt carries the catalog fingerprint"),
        "M004 evaluates the exact qualified M002 contract catalog"
    );
}

#[test]
fn replay_p95_within_budget() {
    let surface = plain_surface();
    let inputs = CausalStateInputs::default();
    // Warm up once so cold caches do not dominate the percentile.
    let _ = evaluate_observe(&surface, &inputs);
    let mut samples = Vec::with_capacity(201);
    for _ in 0..201 {
        let started = Instant::now();
        let outcome = evaluate_observe(&surface, &inputs);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(outcome.admissible_count);
    }
    samples.sort_by(|left, right| left.partial_cmp(right).unwrap());
    let p95 = samples[(samples.len() * 95) / 100];
    assert!(
        p95 <= CAUSAL_OBSERVE_P95_BUDGET_MS,
        "p95 {p95:.3} ms exceeds the {CAUSAL_OBSERVE_P95_BUDGET_MS} ms budget"
    );
}

/// Live production-shaped delta: the real `AgentLoop::build_tool_definitions`
/// path with observe off vs on must return byte-identical provider
/// definitions and deferral bits, while observe records one bounded outcome.
#[cfg(test)]
mod live_request_preparation_delta {
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
    use codegg::tool_advisor::causal_observe::{
        latest_observe_outcome, CAUSAL_OBSERVE_P95_BUDGET_MS,
    };

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
    async fn observe_mode_leaves_live_definitions_byte_identical() {
        let mut off = build_loop(None, "causal-observe-replay-off");
        let mut on = build_loop(Some("observe"), "causal-observe-replay-on");

        let defs_off = off.test_build_tool_definitions().await;
        let defs_on = on.test_build_tool_definitions().await;

        // Byte-identical provider definitions, including deferral bits.
        assert_eq!(
            serde_json::to_string(&defs_off).expect("serialize"),
            serde_json::to_string(&defs_on).expect("serialize"),
            "observe mode must not change provider definitions"
        );
        assert!(
            !defs_off.is_empty(),
            "the live surface must be non-trivial for the delta to mean anything"
        );

        // Observe recorded exactly one bounded outcome for its session and
        // nothing for the off session.
        assert!(
            latest_observe_outcome("causal-observe-replay-off").is_none(),
            "observe off records nothing"
        );
        let outcome = latest_observe_outcome("causal-observe-replay-on")
            .expect("observe on records one outcome");
        assert!(!outcome.surface_fingerprint.is_empty());
        assert!(!outcome.state_fingerprint.is_empty());
        assert!(!outcome.contract_catalog_fingerprint.is_empty());
        assert!(outcome.evaluation_millis < CAUSAL_OBSERVE_P95_BUDGET_MS);
    }
}
