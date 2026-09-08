//! M002 — Model-visible tool-surface minimization contract tests.
//!
//! Asserts visibility / discoverability / invocability rather than brittle
//! raw counts alone:
//!
//! - deferred specialists remain registered with `defer_loading = true`,
//!   retained in the catalog, and discoverable via `tool_search`;
//! - the ordinary immediate set is materially smaller than the full
//!   registered set without capability loss;
//! - discovery is monotonic (denied/hidden tools undiscoverable) and
//!   returns selection metadata without secrets;
//! - plan mode, specialist roles, runtime-unavailable backends, and Tool
//!   Program contracts behave as documented.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use codegg::tool::disclosure;
use codegg::tool::{Tool, ToolRegistry};
use serde_json::json;

/// Partition registry definitions the way a deferral-capable provider
/// path does: `defer_loading = true` entries are deferred, the rest are
/// immediate. Mirrors `AgentLoop::build_tool_definitions` partitioning
/// without requiring a full loop.
fn partition_immediate_deferred(registry: &ToolRegistry) -> (Vec<String>, Vec<String>) {
    let mut immediate = Vec::new();
    let mut deferred = Vec::new();
    for def in registry.definitions() {
        if def.defer_loading == Some(true) {
            deferred.push(def.name);
        } else {
            immediate.push(def.name);
        }
    }
    immediate.sort();
    deferred.sort();
    (immediate, deferred)
}

#[test]
fn deferred_specialists_remain_registered_but_deferred() {
    let registry = ToolRegistry::with_defaults();
    // Representative M002 deferred set: research/evidence/specialist and
    // overlapping edit-loop variants. Canonical `repo_search` stays core.
    let deferred_expected = [
        "research",
        "research_search",
        "repo_fetch",
        "repo_map",
        "security_search",
        "batch_fetch",
        "evidence_bundle",
        "codesearch",
        "review",
        "image",
        "terminal",
        "skill_proposal",
        "security",
        "replace",
        "commit",
        "python_script",
        "tool_program",
    ];
    for name in deferred_expected {
        let tool = registry
            .get(name)
            .unwrap_or_else(|| panic!("deferred tool `{name}` must remain registered"));
        assert!(
            tool.defer_loading(),
            "`{name}` must have defer_loading() == true"
        );
        assert!(
            tool.expose_in_definitions(),
            "`{name}` must stay exposed for discovery (deferred, not hidden)"
        );
        // Catalog retains deferred metadata for `tool_search`.
        let meta = registry
            .catalog()
            .get(name)
            .unwrap_or_else(|| panic!("catalog must retain `{name}`"));
        assert_eq!(meta.name, name);
        assert!(meta.defer_load, "catalog must mark `{name}` deferred");
        assert!(!meta.category.is_empty());
        assert_eq!(meta.disclosure, "deferred");
    }
    // Canonical repo inspect stays core and immediate.
    let repo = registry.get("repo_search").expect("repo_search registered");
    assert!(
        !repo.defer_loading(),
        "canonical repo_search must stay core/immediate"
    );
    assert_eq!(
        disclosure::disclosure_for("repo_search"),
        disclosure::ToolDisclosure::Core
    );
}

#[test]
fn ordinary_immediate_set_is_materially_smaller_without_capability_loss() {
    let registry = ToolRegistry::with_defaults();
    let registered: BTreeSet<String> = registry
        .list()
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    let (immediate, deferred) = partition_immediate_deferred(&registry);
    // No capability deleted: every deferred tool is still registered.
    for name in &deferred {
        assert!(
            registered.contains(name),
            "deferred `{name}` must remain registered"
        );
    }
    // Material reduction: at least 10 tools deferred from the ordinary
    // immediate set (research/evidence/specialist/overlap family).
    assert!(
        deferred.len() >= 10,
        "expected materially smaller immediate set, deferred={deferred:?}"
    );
    assert!(
        immediate.len() + deferred.len() == registry.definitions().len(),
        "immediate+deferred must equal model-facing definitions"
    );
    // Core primitives remain immediate.
    for core in [
        "read",
        "glob",
        "grep",
        "list",
        "edit",
        "apply_patch",
        "bash",
        "tool_search",
        "repo_search",
    ] {
        assert!(
            immediate.contains(&core.to_string()),
            "core `{core}` must stay immediate"
        );
    }
    // Hidden internal tools are neither immediate nor deferred.
    assert!(!immediate.contains(&"invalid".to_string()));
    assert!(!deferred.contains(&"invalid".to_string()));
}

#[test]
fn hidden_internal_tools_absent_from_definitions_and_discovery() {
    let registry = ToolRegistry::with_defaults();
    assert!(
        registry.contains("invalid"),
        "invalid stays registered for diagnostics"
    );
    let defs: Vec<String> = registry.definitions().into_iter().map(|d| d.name).collect();
    assert!(
        !defs.contains(&"invalid".to_string()),
        "hidden invalid must not appear in model-facing definitions"
    );
    // Direct catalog search without an allow-list must still exclude hidden.
    let search =
        codegg::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone()));
    let out = tokio_test_block_on(search, "invalid");
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let names: Vec<String> = parsed["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();
    assert!(
        !names.contains(&"invalid".to_string()),
        "hidden tools must remain undiscoverable, got: {names:?}"
    );
}

fn tokio_test_block_on(tool: codegg::tool::tool_search::ToolSearchTool, query: &str) -> String {
    let tool = tool;
    let input = json!({"query": query});
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tool.execute(input))
        .unwrap()
}

#[test]
fn deferred_tool_absent_initially_found_via_search_and_invocable() {
    let registry = ToolRegistry::with_defaults();
    let (immediate, deferred) = partition_immediate_deferred(&registry);
    assert!(
        deferred.contains(&"security".to_string()),
        "security must be deferred for ordinary turns"
    );
    assert!(
        !immediate.contains(&"security".to_string()),
        "security must be absent from the ordinary immediate set"
    );
    // Simulate the loop's allow-list: immediate + deferred (policy-allowed).
    let mut available: Vec<String> = immediate.clone();
    available.extend(deferred.clone());
    let mut search =
        codegg::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone()));
    search.set_available_tools(available);
    let input = json!({"query": "security"});
    let output: serde_json::Value = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(search.execute(input))
        .and_then(|s| {
            serde_json::from_str(&s).map_err(|e| codegg::error::ToolError::Execution(e.to_string()))
        })
        .unwrap();
    let names: Vec<String> = output["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();
    assert!(
        names
            .iter()
            .any(|n| n == "security" || n == "security_search"),
        "tool_search must discover deferred security capability, got: {names:?}"
    );
    // Selection metadata is present for correct choice.
    let entry = output["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t.get("name").and_then(|n| n.as_str()) == Some("security"))
        .cloned()
        .or_else(|| {
            output["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t.get("name").and_then(|n| n.as_str()) == Some("security_search"))
                .cloned()
        })
        .expect("security entry present");
    assert!(entry.get("canonical_name").is_some());
    assert!(entry.get("category").is_some());
    assert!(entry.get("risk").is_some());
    assert!(entry.get("disclosure").is_some());
    // Allowed invocation succeeds through the canonical registry path
    // (security classify_command is backend-free).
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(registry.execute_capture(
            "security",
            json!({"action": "classify_command", "command": "ls"}),
            None,
        ))
        .expect("deferred security must remain invocable");
    assert!(!result.output.is_empty());
}

#[test]
fn discovery_does_not_widen_authority_for_denied_tools() {
    let registry = ToolRegistry::with_defaults();
    let (immediate, deferred) = partition_immediate_deferred(&registry);
    // Simulate an agent that denies `security`: the loop's allow-list
    // omits it, so search must not reveal it.
    let mut available: Vec<String> = immediate;
    available.extend(deferred);
    available.retain(|n| n != "security" && n != "security_search");
    let mut search =
        codegg::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone()));
    search.set_available_tools(available);
    let input = json!({"query": "security"});
    let output: String = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(search.execute(input))
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    let names: Vec<String> = parsed["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();
    assert!(
        !names.contains(&"security".to_string()),
        "denied tool must stay undiscoverable, got: {names:?}"
    );
    assert!(
        !names.contains(&"security_search".to_string()),
        "denied tool must stay undiscoverable, got: {names:?}"
    );
}

#[test]
fn discovery_output_contains_no_secret_backend_configuration() {
    let registry = ToolRegistry::with_defaults();
    let (immediate, deferred) = partition_immediate_deferred(&registry);
    let mut available = immediate;
    available.extend(deferred);
    let mut search =
        codegg::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone()));
    search.set_available_tools(available);
    for query in ["research", "security", "repo", "batch", "evidence"] {
        let output: String = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(search.execute(json!({"query": query})))
            .unwrap();
        let lower = output.to_lowercase();
        for secret_marker in [
            "api_key",
            "apikey",
            "bearer",
            "credential",
            "endpoint",
            "OPENAI_API_KEY",
        ] {
            assert!(
                !lower.contains(&secret_marker.to_lowercase()),
                "discovery output for `{query}` must not leak `{secret_marker}`: {output}"
            );
        }
    }
}

#[test]
fn discovery_caps_broad_queries_and_rejects_empty_queries() {
    let registry = ToolRegistry::with_defaults();
    let mut search =
        codegg::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone()));
    // No allow-list: still bounded and hidden-filtered.
    let broad: serde_json::Value = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(search.execute(json!({"query": "a"})))
        .and_then(|s| {
            serde_json::from_str(&s).map_err(|e| codegg::error::ToolError::Execution(e.to_string()))
        })
        .unwrap();
    let tools = broad["tools"].as_array().cloned().unwrap_or_default();
    assert!(
        tools.len() <= codegg::tool::tool_search::MAX_SEARCH_RESULTS,
        "broad query must be capped, got {}",
        tools.len()
    );
    search.set_available_tools(vec![]);
    let empty: serde_json::Value = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(search.execute(json!({"query": "   "})))
        .and_then(|s| {
            serde_json::from_str(&s).map_err(|e| codegg::error::ToolError::Execution(e.to_string()))
        })
        .unwrap();
    assert_eq!(empty["status"], json!("no_results"));
}

#[test]
fn plan_mode_remains_read_only_with_discoverable_search() {
    // Plan surface allows inspection/planning plus tool_search; mutating
    // tools stay denied.
    for allowed in ["read", "glob", "grep", "list", "repo_search", "tool_search"] {
        assert!(
            disclosure::plan_allowed(allowed),
            "{allowed} must stay plan-allowed"
        );
    }
    for denied in ["edit", "write", "apply_patch", "task", "commit"] {
        assert!(
            !disclosure::plan_allowed(denied),
            "{denied} must stay plan-denied"
        );
    }
    // Resolved-surface parity: plan mode omits mutating tools.
    let registry = ToolRegistry::with_defaults();
    let defs = registry.definitions();
    let surface = codegg::agent::tool_surface::ResolvedToolSurface::resolve(
        defs,
        &BTreeSet::new(),
        &BTreeSet::new(),
        true,
        false,
        None,
    )
    .unwrap();
    let names: BTreeSet<String> = surface
        .tools
        .iter()
        .map(|t| t.canonical_name.clone())
        .collect();
    assert!(names.contains("read"));
    assert!(names.contains("tool_search"));
    assert!(!names.contains("edit"));
    assert!(!names.contains("write"));
    assert!(!names.contains("task"));
}

#[test]
fn specialist_roles_receive_role_appropriate_immediate_palette() {
    // Ordinary coding defers; specialist roles promote to immediate via
    // `immediate_for_agent` (consumed by the loop's deferral partition).
    assert!(!disclosure::immediate_for_agent("research", "build"));
    assert!(disclosure::immediate_for_agent("research", "research"));
    assert!(disclosure::immediate_for_agent(
        "evidence_bundle",
        "research"
    ));
    assert!(disclosure::immediate_for_agent(
        "security",
        "security-review"
    ));
    assert!(disclosure::immediate_for_agent(
        "evidence_bundle",
        "verifier"
    ));
    assert!(!disclosure::immediate_for_agent("evidence_bundle", "build"));
}

#[test]
fn runtime_unavailable_tools_are_not_advertised_as_merely_deferred() {
    use codegg::tool::backend::{ExternalToolBackendConfig, ToolBackendConfig};
    use codegg::tool::ToolImplementationBackend;
    // Disabled LSP backend: stub remains registered for diagnostics but
    // hidden from model-facing definitions (unavailable, not deferred).
    let mut backends = ToolBackendConfig::all_native();
    backends.lsp = Some(ExternalToolBackendConfig {
        backend: Some(ToolImplementationBackend::Disabled),
        ..Default::default()
    });
    let registry = ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        tool_backends: backends,
        ..Default::default()
    });
    assert!(registry.contains("lsp"));
    let defs: Vec<String> = registry.definitions().into_iter().map(|d| d.name).collect();
    assert!(
        !defs.contains(&"lsp".to_string()),
        "disabled lsp must be unavailable, not deferred"
    );
    // Task without a functional spawner is NonCallable in the surface.
    let registry = ToolRegistry::with_defaults();
    let defs = registry.definitions();
    let surface = codegg::agent::tool_surface::ResolvedToolSurface::resolve(
        defs,
        &BTreeSet::new(),
        &BTreeSet::new(),
        false,
        false,
        None,
    )
    .unwrap();
    let omitted: HashMap<String, String> = surface
        .omissions
        .iter()
        .map(|o| (o.canonical_name.clone(), format!("{:?}", o.reason)))
        .collect();
    // Default registry task has no spawner in unit construction; when
    // present it must be omitted as NonCallable, never as deferred.
    if let Some(reason) = omitted.get("task") {
        assert_eq!(reason, "NonCallable");
    }
}

#[test]
fn tool_program_callability_follows_contracts_not_disclosure() {
    let registry = ToolRegistry::with_defaults();
    // Read-only palette stays program-callable; deferred specialists that
    // are DirectOnly stay program-rejected. Disclosure must not change this.
    let read_contract = registry
        .get("read")
        .unwrap()
        .contract("read", json!({"type": "object"}));
    assert_eq!(
        read_contract.caller_policy,
        codegg::tool::ToolCallerPolicy::DirectOrProgrammatic
    );
    let research_contract = registry
        .get("research")
        .unwrap()
        .contract("research", json!({"type": "object"}));
    assert_eq!(
        research_contract.caller_policy,
        codegg::tool::ToolCallerPolicy::DirectOnly
    );
    // Deferred `tool_program` itself remains DirectOnly (loop-only).
    let program_contract = registry
        .get("tool_program")
        .unwrap()
        .contract("tool_program", json!({"type": "object"}));
    assert_eq!(
        program_contract.caller_policy,
        codegg::tool::ToolCallerPolicy::DirectOnly
    );
}

#[test]
fn fresh_registries_produce_deterministic_classifications() {
    // No durable discovery state: rebuilds from the same config/profile
    // deterministically reproduce disclosure.
    let first = ToolRegistry::with_defaults();
    let second = ToolRegistry::with_defaults();
    let (first_immediate, first_deferred) = partition_immediate_deferred(&first);
    let (second_immediate, second_deferred) = partition_immediate_deferred(&second);
    assert_eq!(first_immediate, second_immediate);
    assert_eq!(first_deferred, second_deferred);
    for name in first.list().iter().map(|t| t.name().to_string()) {
        assert_eq!(
            disclosure::disclosure_for(&name),
            disclosure::disclosure_for(&name)
        );
    }
}

#[test]
fn m001_canonical_names_consumed_without_reopening_compat() {
    // `codesearch` remains the retained M001 alias delegating to the
    // canonical `repo_search` backend; M002 only changes advertisement.
    let registry = ToolRegistry::with_defaults();
    assert!(registry.contains("repo_search"));
    assert!(registry.contains("codesearch"));
    let alias = registry.get("codesearch").unwrap();
    assert!(alias.description().contains("repo_search"));
    assert_eq!(
        disclosure::disclosure_for("repo_search"),
        disclosure::ToolDisclosure::Core
    );
    assert_eq!(
        disclosure::disclosure_for("codesearch"),
        disclosure::ToolDisclosure::Deferred
    );
}
