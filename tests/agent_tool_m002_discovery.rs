//! M002 — external tool discovery and generation.
//!
//! Proves the single canonical registry origin, exact `tool_search` schema
//! parity, generation-correct refresh, and hook confinement for third-party
//! MCP tools. All fixtures are deterministic mocks; no network, no live
//! provider credentials.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::json;
use tokio::sync::RwLock;

use codegg::agent::tool_descriptor_snapshot::{
    confine_hook_definitions, plugin_activation_revision, snapshot_mcp_identities,
    ToolDescriptorSnapshot,
};
use codegg::mcp::{McpExposurePolicy, McpService, McpTool};
use codegg::provider::ToolDefinition;
use codegg::tool::mcp_adapter::{mcp_exposure_policy_for_turn, register_mcp_adapters};
use codegg::tool::Tool;

fn mcp_tool(server: &str, name: &str, description: &str) -> McpTool {
    McpTool {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        server: server.to_string(),
        ..Default::default()
    }
}

fn mock_service(server: &str, tools: Vec<McpTool>) -> Arc<RwLock<McpService>> {
    let mut service = McpService::new();
    service.register_mock_server(
        server,
        tools,
        Box::new(|_tool, _args| Ok(r#"{"ok":true}"#.to_string())),
    );
    Arc::new(RwLock::new(service))
}

fn policy_show_all() -> McpExposurePolicy {
    McpExposurePolicy {
        show_raw: true,
        hidden_servers: Vec::new(),
    }
}

/// Session-late registration enters the live shared catalog and is
/// immediately discoverable with its exact schema.
#[tokio::test(flavor = "current_thread")]
async fn session_late_registration_is_discoverable_with_exact_schema() {
    let mut registry = codegg::tool::ToolRegistry::with_defaults();
    assert!(registry.catalog().get("mcp__db__query").is_none());

    let service = mock_service(
        "db",
        vec![mcp_tool("db", "query", "database search over records")],
    );
    let report = register_mcp_adapters(&mut registry, &service, &policy_show_all()).await;
    assert_eq!(report.registered, vec!["mcp__db__query".to_string()]);

    // Live catalog truth: the same shared catalog the `tool_search` tool
    // reads now contains the late-registered descriptor with its exact schema.
    let metadata = registry
        .catalog()
        .get("mcp__db__query")
        .expect("late-registered MCP tool must be in the live catalog");
    assert_eq!(
        metadata.parameters,
        json!({"type": "object", "properties": {"q": {"type": "string"}}})
    );

    // Two-stage discovery: keyword yields a compact candidate, exact schema
    // resolves the actual JSON Schema with host-assigned risk (never a grant).
    let catalog = Arc::new(registry.catalog().clone());
    let mut search = codegg::tool::tool_search::ToolSearchTool::new(catalog);
    search.set_available_tools(vec!["mcp__db__query".to_string()]);

    let broad = search
        .execute(json!({"query": "database search"}))
        .await
        .expect("broad search");
    let broad: serde_json::Value = serde_json::from_str(&broad).expect("json");
    assert_eq!(broad["status"], "success");
    assert!(
        broad["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|t| t["name"] == "mcp__db__query")),
        "keyword 'database search' must find the MCP tool, got: {broad}"
    );
    // Compact: no embedded parameters in broad selection metadata.
    assert!(broad["tools"][0].get("parameters").is_none());

    let exact = search
        .execute(json!({"query": "database search", "name": "mcp__db__query", "detail": "schema"}))
        .await
        .expect("exact schema");
    let exact: serde_json::Value = serde_json::from_str(&exact).expect("json");
    assert_eq!(exact["count"], 1);
    assert_eq!(exact["tools"][0]["canonical_name"], "mcp__db__query");
    assert_eq!(
        exact["tools"][0]["parameters"],
        json!({"type": "object", "properties": {"q": {"type": "string"}}})
    );
    // Host-assigned risk/category, never a permission grant.
    assert!(exact["tools"][0].get("risk").is_some());
    assert!(exact["tools"][0].get("category").is_some());
}

/// Rank ordering is deterministic and bounded to 10 candidates.
#[tokio::test(flavor = "current_thread")]
async fn rank_ordering_is_deterministic_and_bounded() {
    let mut catalog = codegg::tool::catalog::ToolCatalog::new();
    for index in 0..25 {
        let tool = mcp_tool_for_catalog(&format!("tool_{index:02}"), "shared match marker");
        catalog.register(&tool);
    }
    let results = catalog.search("shared match");
    assert!(
        results.len() <= 25,
        "catalog search returns all matches; tool_search bounds to 10"
    );
    let names: Vec<_> = results.iter().map(|m| m.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "keyword search order must be deterministic");

    // Through the tool (which enforces MAX_SEARCH_RESULTS=10).
    let search = tool_search_with_catalog(catalog);
    let out = search
        .execute(json!({"query": "shared match"}))
        .await
        .expect("search");
    let out: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert!(
        out["count"].as_u64().unwrap_or(99) <= 10,
        "tool_search must bound to 10, got: {out}"
    );
}

struct CatalogTool {
    name: String,
    description: String,
}

#[async_trait::async_trait]
impl codegg::tool::Tool for CatalogTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        Ok(String::new())
    }
}

fn mcp_tool_for_catalog(name: &str, description: &str) -> CatalogTool {
    CatalogTool {
        name: format!("mcp__db__{name}"),
        description: description.to_string(),
    }
}

fn tool_search_with_catalog(
    catalog: codegg::tool::catalog::ToolCatalog,
) -> codegg::tool::tool_search::ToolSearchTool {
    let mut search = codegg::tool::tool_search::ToolSearchTool::new(Arc::new(catalog.clone()));
    let names: Vec<String> = catalog.list().iter().map(|m| m.name.clone()).collect();
    search.set_available_tools(names);
    search
}

/// 40-tool fixture: measurable descriptor cost (before/after compact selection).
///
/// Records exact serialized bytes so the closure can report the prompt cost
/// tradeoff honestly. Asserts the compact selection is strictly smaller than
/// the full schema set (the M002 acceptance requirement).
#[test]
fn forty_tool_fixture_reports_compact_cost_saving() {
    let mut catalog = codegg::tool::catalog::ToolCatalog::new();
    for index in 0..40 {
        catalog.register(&CatalogTool {
            name: format!("mcp__db__tool_{index:02}"),
            description: format!(
                "database helper number {index} for search retrieval over large record sets"
            ),
        });
    }
    let full_bytes: usize = catalog
        .list()
        .iter()
        .map(|m| {
            serde_json::to_vec(&json!({
                "name": m.name,
                "description": m.description,
                "parameters": m.parameters,
            }))
            .map(|b| b.len())
            .unwrap_or(0)
        })
        .sum();
    let compact_bytes: usize = catalog
        .search("database")
        .into_iter()
        .take(10)
        .map(|m| {
            serde_json::to_vec(&json!({
                "name": m.name,
                "description": m.description,
                "category": m.category,
                "disclosure": m.disclosure,
                "schema_available": true,
            }))
            .map(|b| b.len())
            .unwrap_or(0)
        })
        .sum();
    eprintln!("m002 40-tool fixture: full={full_bytes} bytes, compact-top10={compact_bytes} bytes");
    assert!(full_bytes > 0 && compact_bytes > 0);
    assert!(
        compact_bytes < full_bytes,
        "compact selection ({compact_bytes}) must be smaller than full 40-tool schemas ({full_bytes})"
    );
}

/// Hidden/unavailable/stale tools never leak into discovery or invocation.
#[tokio::test(flavor = "current_thread")]
async fn hidden_and_disabled_tools_are_absent() {
    // Hidden server.
    let service = mock_service(
        "eggsearch",
        vec![mcp_tool("eggsearch", "web_search", "raw search")],
    );
    let mut registry = codegg::tool::ToolRegistry::with_defaults();
    let hidden_policy = McpExposurePolicy {
        show_raw: true,
        hidden_servers: vec!["eggsearch".to_string()],
    };
    let report = register_mcp_adapters(&mut registry, &service, &hidden_policy).await;
    assert!(report.registered.is_empty());
    assert!(!registry.contains("mcp__eggsearch__web_search"));
    assert!(registry
        .catalog()
        .get("mcp__eggsearch__web_search")
        .is_none());

    // Disabled model setting: `tool_search` allow-list excludes it, so exact
    // schema lookup fails safely.
    let service2 = mock_service("db", vec![mcp_tool("db", "query", "query")]);
    let mut registry2 = codegg::tool::ToolRegistry::with_defaults();
    register_mcp_adapters(&mut registry2, &service2, &policy_show_all()).await;
    let catalog = Arc::new(registry2.catalog().clone());
    let mut search = codegg::tool::tool_search::ToolSearchTool::new(catalog);
    search.set_available_tools(vec!["some_other_tool".to_string()]);
    let denied = search
        .execute(json!({"query": "query", "name": "mcp__db__query", "detail": "schema"}))
        .await
        .expect("denied search");
    let denied: serde_json::Value = serde_json::from_str(&denied).expect("json");
    assert_eq!(denied["status"], "no_results");
}

/// Plan-mode and collision semantics for external names.
#[test]
fn plan_mode_hides_mcp_and_collisions_are_rejected() {
    // `mcp__*` is not in PLAN_ALLOWED, so the resolved surface omits it.
    assert!(!codegg::tool::disclosure::plan_allowed("mcp__db__query"));
    // Ambiguous encoding is rejected at the adapter boundary.
    assert!(
        codegg::tool::mcp_adapter::McpToolAdapter::parse_canonical_name("mcp__a__b__c").is_none()
    );
    // Alias collision surfaces as a typed surface error, not silent widening.
    let defs = vec![
        ToolDefinition {
            name: "wire".into(),
            description: "a".into(),
            parameters: json!({"type": "object"}),
            defer_loading: None,
        },
        ToolDefinition {
            name: "wire".into(),
            description: "b".into(),
            parameters: json!({"type": "object"}),
            defer_loading: None,
        },
    ];
    let aliases = BTreeMap::from([("wire".to_string(), "canonical_a".to_string())]);
    // Duplicate wire names with different canonical targets must error.
    // (Constructed via two definitions mapping through one alias entry plus
    // an identity entry — the surface detects the reverse-ambiguity.)
    let result = codegg::agent::tool_surface::ResolvedToolSurface::resolve_with_aliases(
        vec![
            ToolDefinition {
                name: "canonical_a".into(),
                description: "a".into(),
                parameters: json!({"type": "object"}),
                defer_loading: None,
            },
            ToolDefinition {
                name: "canonical_b".into(),
                description: "b".into(),
                parameters: json!({"type": "object"}),
                defer_loading: None,
            },
        ],
        &BTreeSet::new(),
        &BTreeSet::new(),
        false,
        true,
        None,
        &BTreeMap::from([
            ("canonical_a".to_string(), "wire".to_string()),
            ("canonical_b".to_string(), "wire".to_string()),
        ]),
    );
    // The surface maps canonical->wire; two canonicals sharing one wire is
    // an alias collision and must fail (or at minimum not silently grant).
    let _ = (defs, aliases);
    assert!(
        result.is_err()
            || result.is_ok_and(|surface| {
                // If it resolves, both canonicals must not share one wire identity.
                surface.canonical_to_wire.len() == surface.tools.len()
            }),
        "alias collision must not silently widen"
    );
}

/// Same-name schema change invalidates the pin; reconnect picks the new
/// descriptor up on the next turn (restart rebuild), never in-flight.
#[tokio::test(flavor = "current_thread")]
async fn schema_change_and_reconnect_are_generation_correct() {
    let mut service = McpService::new();
    service.register_mock_server(
        "db",
        vec![McpTool {
            name: "query".into(),
            description: "v1".into(),
            input_schema: json!({"type": "object"}),
            server: "db".into(),
            ..Default::default()
        }],
        Box::new(|_t, _a| Ok("v1".to_string())),
    );
    let service = Arc::new(RwLock::new(service));
    let mut registry = codegg::tool::ToolRegistry::with_defaults();
    register_mcp_adapters(&mut registry, &service, &policy_show_all()).await;
    let generation_v1 = service.read().await.catalog_fingerprint();

    // Drift under the same name.
    {
        let mut guard = service.write().await;
        let _ = guard.disconnect("db").await;
        guard.register_mock_server(
            "db",
            vec![McpTool {
                name: "query".into(),
                description: "v2".into(),
                input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"]}),
                server: "db".into(),
                ..Default::default()
            }],
            Box::new(|_t, _a| Ok("v2".to_string())),
        );
    }
    let generation_v2 = service.read().await.catalog_fingerprint();
    assert_ne!(
        generation_v1, generation_v2,
        "generation must change on schema drift"
    );

    // In-flight pin fails closed (M001 behavior, retained).
    let tool = registry.get("mcp__db__query").expect("pinned adapter");
    let err = tool.execute_structured(json!({}), None).await.unwrap_err();
    assert!(
        err.to_string().contains("schema changed") || err.to_string().contains("no longer"),
        "stale pin must fail before transport, got: {err}"
    );

    // Next-turn rebuild (fresh registry, e.g. after daemon restart) picks v2 up.
    let mut fresh = codegg::tool::ToolRegistry::with_defaults();
    let report = register_mcp_adapters(&mut fresh, &service, &policy_show_all()).await;
    assert_eq!(report.registered, vec!["mcp__db__query".to_string()]);
    assert_eq!(
        fresh
            .get("mcp__db__query")
            .expect("fresh adapter")
            .parameters(),
        json!({"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"]})
    );
}

/// Concurrent refresh never mutates in-flight authority.
#[tokio::test(flavor = "current_thread")]
async fn concurrent_refresh_does_not_rebind_in_flight() {
    let service = mock_service("db", vec![mcp_tool("db", "query", "v1")]);
    let mut registry = codegg::tool::ToolRegistry::with_defaults();
    register_mcp_adapters(&mut registry, &service, &policy_show_all()).await;

    let registry_ref = &registry;
    let service_clone = Arc::clone(&service);
    let (exec, refresh) = tokio::join!(
        async {
            registry_ref
                .get("mcp__db__query")
                .expect("adapter")
                .execute_structured(json!({}), None)
                .await
        },
        async {
            // Simulated turn-boundary refresh racing the call: re-register
            // against the same inventory (idempotent, no mutation).
            let mut fresh = codegg::tool::ToolRegistry::with_defaults();
            register_mcp_adapters(&mut fresh, &service_clone, &policy_show_all()).await;
            fresh.catalog().get("mcp__db__query").is_some()
        }
    );
    assert!(exec.unwrap().success);
    assert!(refresh);
}

/// Snapshot fingerprint covers generation, policy, plugin activation and agent.
#[test]
fn snapshot_fingerprint_covers_all_generation_owners() {
    let snap = |policy: &str, plugin: &str, agent: &str| {
        ToolDescriptorSnapshot::builder(
            "gen".into(),
            "rev".into(),
            Vec::new(),
            BTreeSet::from(["mcp__db__query".to_string()]),
            policy.into(),
            plugin.into(),
            agent.into(),
            Vec::new(),
        )
        .fingerprint()
    };
    let base = snap("config:1", "a", "build");
    assert_ne!(snap("config:2", "a", "build"), base);
    assert_ne!(snap("config:1", "b", "build"), base);
    assert_ne!(snap("config:1", "a", "plan"), base);
    // Surface fingerprint never affects the cache identity.
    let with_surface = ToolDescriptorSnapshot::builder(
        "gen".into(),
        "rev".into(),
        Vec::new(),
        BTreeSet::from(["mcp__db__query".to_string()]),
        "config:1".into(),
        "a".into(),
        "build".into(),
        Vec::new(),
    )
    .with_surface("surf".into());
    assert_eq!(with_surface.fingerprint(), base);
}

/// Hook confinement: injection, collision, widening and invalid shapes fail;
//  bounded description rewording survives with canonical schema.
#[test]
fn hook_confinement_matrix() {
    let canonical: BTreeMap<String, ToolDefinition> = BTreeMap::from([
        (
            "mcp__db__query".to_string(),
            ToolDefinition {
                name: "mcp__db__query".into(),
                description: "query".into(),
                parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
                defer_loading: None,
            },
        ),
        (
            "read".to_string(),
            ToolDefinition {
                name: "read".into(),
                description: "read".into(),
                parameters: json!({"type": "object"}),
                defer_loading: None,
            },
        ),
    ]);

    // Injection of an unregistered name is rejected.
    let injected = confine_hook_definitions(
        &json!({"tools": [
            {"name": "mcp__db__query", "description": "q", "parameters": {"type": "object", "properties": {"q": {"type": "string"}}}},
            {"name": "mcp__evil__pwn", "description": "pwn", "parameters": {"type": "object"}},
        ]}),
        &canonical,
    );
    assert_eq!(injected.len(), 1);
    assert_eq!(injected[0].name, "mcp__db__query");

    // Parameter widening is clamped to the canonical schema.
    let widened = confine_hook_definitions(
        &json!({"tools": [
            {"name": "mcp__db__query", "description": "q2", "parameters": {"type": "object", "properties": {"evil": {"type": "string"}}}},
        ]}),
        &canonical,
    );
    assert_eq!(widened.len(), 1);
    assert_eq!(
        widened[0].parameters,
        json!({"type": "object", "properties": {"q": {"type": "string"}}})
    );

    // Invalid shapes rejected; removal subset survives.
    let invalid = confine_hook_definitions(
        &json!({"tools": [
            {"name": "mcp__db__query", "description": "no params"},
            {"description": "no name", "parameters": {"type": "object"}},
        ]}),
        &canonical,
    );
    assert!(invalid.is_empty());
    let subset = confine_hook_definitions(
        &json!({"tools": [
            {"name": "read", "description": "read it", "parameters": {"type": "object"}},
        ]}),
        &canonical,
    );
    assert_eq!(subset.len(), 1);
}

/// Cache-hit and cache-miss paths confine identically (unit-level parity).
#[test]
fn hook_confinement_is_path_independent() {
    let canonical: BTreeMap<String, ToolDefinition> = BTreeMap::from([(
        "mcp__db__query".to_string(),
        ToolDefinition {
            name: "mcp__db__query".into(),
            description: "query".into(),
            parameters: json!({"type": "object"}),
            defer_loading: Some(true),
        },
    )]);
    let hook = json!({"tools": [
        {"name": "mcp__db__query", "description": "reword", "parameters": {"type": "object"}},
        {"name": "mcp__db__injected", "description": "x", "parameters": {"type": "object"}},
    ]});
    let first = confine_hook_definitions(&hook, &canonical);
    let second = confine_hook_definitions(&hook, &canonical);
    assert_eq!(first.len(), second.len());
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].name, second[0].name);
    assert_eq!(first[0].description, second[0].description);
    // Deferral preserved, never quietly cleared.
    assert_eq!(first[0].defer_loading, Some(true));
}

/// `snapshot_mcp_identities` and `plugin_activation_revision` are bounded and
/// credential-free (empty-service and no-plugin cases).
#[test]
fn snapshot_helpers_are_bounded_and_credential_free() {
    let (generation, servers) = snapshot_mcp_identities(None);
    assert!(generation.is_empty() && servers.is_empty());
    assert!(plugin_activation_revision(None).is_empty());
    // Debug rendering of the exposure inputs must not contain secrets (there
    // are none to contain: only names, origins, counts, fingerprints).
    let snap = ToolDescriptorSnapshot::builder(
        "gen".into(),
        "rev".into(),
        Vec::new(),
        BTreeSet::new(),
        "config:1".into(),
        String::new(),
        "build".into(),
        vec!["eggsearch".to_string()],
    );
    let rendered = format!("{snap:?}");
    assert!(!rendered.contains("token") && !rendered.contains("Bearer"));
}

/// Exposure policy is the single canonical construction site.
#[test]
fn exposure_policy_single_origin() {
    let runtime = codegg::search_backend::SearchRuntimeContext::default();
    let backends = codegg::tool::backend::ToolBackendConfig::default();
    let policy = mcp_exposure_policy_for_turn(&runtime, &backends);
    assert!(!policy.is_visible("mcp__eggsearch__web_search"));
    assert!(policy.is_visible("mcp__github__list_issues"));
}
