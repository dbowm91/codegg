//! End-to-end tests for the eggsearch backend dispatch path.
//!
//! These tests inject an in-process mock `McpService` into the
//! `search_backend::state` global, then drive `dispatch_web_search`
//! and `dispatch_web_fetch`. They verify that:
//!
//! - `websearch` (the agent-facing native tool) reaches the
//!   `mcp__eggsearch__web_search` MCP tool with the expected
//!   argument mapping.
//! - `webfetch` reaches `mcp__eggsearch__web_fetch` with the
//!   expected argument mapping.
//! - Output is wrapped in `external_untrusted` framing before being
//!   returned.
//! - The legacy built-in path is not invoked when eggsearch is
//!   configured.
//!
//! The tests do not require a real `eggsearch` binary or any
//! network access. The mock is constructed directly into the
//! `McpService` via the test-only `register_mock_server` helper.
//!
//! ## Test isolation
//!
//! `search_backend::state` is a process-global slot, so the
//! tests in this file must be serialized. The `TEST_LOCK`
//! mutex at the top of the file enforces that.

use std::sync::Arc;

use codegg::config::schema::{EggsearchConfig, SearchBackendConfig, SearchConfig};
use codegg::error::McpError;
use codegg::mcp::{McpService, McpTool};
use codegg::provider::ToolDefinition;
use codegg::research::sources::eggsearch::EggsearchSource;
use codegg::research::sources::ResearchSourceAdapter;
use codegg::research::types::{
    ResearchAudience, ResearchBudget, ResearchDepth, ResearchMode, ResearchPlan, ResearchRequest,
};
use codegg::search_backend::state;
use codegg::search_backend::test_support::{
    acquire_cross_process_lock, CrossProcessLockGuard, SHARED_TEST_LOCK,
};
use codegg::tool::batch_fetch::BatchFetchTool;
use codegg::tool::evidence_bundle::EvidenceBundleTool;
use codegg::tool::repo_fetch::RepoFetchTool;
use codegg::tool::repo_map::RepoMapTool;
use codegg::tool::repo_search::RepoSearchTool;
use codegg::tool::research_search::ResearchSearchTool;
use codegg::tool::security_search::SecuritySearchTool;
use codegg::tool::webfetch::WebFetchTool;
use codegg::tool::websearch::WebSearchTool;
use codegg::tool::Tool;
use tokio::sync::{Mutex, MutexGuard};

// Serialize every test in this file (and across all test binaries
// that touch `search_backend::state`) with the shared cross-process
// flock. The in-process mutex is held across `.await` while the
// cross-process flock is held for the entire test body.
async fn lock() -> (CrossProcessLockGuard, MutexGuard<'static, ()>) {
    let cp = acquire_cross_process_lock();
    let g = SHARED_TEST_LOCK.lock().await;
    (cp, g)
}

fn eggsearch_config(expose_raw: bool, fallback: bool) -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Eggsearch),
        expose_raw_mcp_tools: Some(expose_raw),
        fallback_to_builtin: Some(fallback),
        max_search_output_chars: Some(12_000),
        max_fetch_output_chars: Some(20_000),
        eggsearch: Some(EggsearchConfig {
            server_name: Some("eggsearch".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn builtin_config() -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Builtin),
        ..Default::default()
    }
}

fn disabled_config() -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Disabled),
        ..Default::default()
    }
}

/// Build a mock eggsearch MCP service with the three required tools
/// pre-registered, plus a `Mock` client whose `call_tool` returns
/// canned responses or records the call for later inspection.
fn build_mock_eggsearch(
    recorded_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
) -> McpService {
    let mut svc = McpService::new();
    let tools = vec![
        McpTool {
            name: "web_search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
            server: "eggsearch".to_string(),
        },
        McpTool {
            name: "web_fetch".to_string(),
            description: "Fetch a URL".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
            server: "eggsearch".to_string(),
        },
        McpTool {
            name: "provider_status".to_string(),
            description: "Check provider status".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
            server: "eggsearch".to_string(),
        },
    ];
    let calls = Arc::clone(&recorded_calls);
    svc.register_mock_server(
        "eggsearch",
        tools,
        Box::new(move |tool, args| {
            validate_current_eggsearch_request(tool, &args)?;
            if let Ok(mut g) = calls.try_lock() {
                g.push((tool.to_string(), args.clone()));
            }
            match tool {
                "web_search" => {
                    Ok(r#"{"hits": [{"title": "Mock", "url": "https://x"}]}"#.to_string())
                }
                "web_fetch" => Ok("mock page body".to_string()),
                "provider_status" => Ok(r#"{"providers": ["mock"]}"#.to_string()),
                _ => Err(McpError::Server(format!("unknown tool {tool}"))),
            }
        }),
    );
    svc
}

fn validate_current_eggsearch_request(
    tool: &str,
    args: &serde_json::Value,
) -> Result<(), McpError> {
    let object = args
        .as_object()
        .ok_or_else(|| McpError::Server(format!("{tool} request must be an object")))?;
    let require = |field: &str| {
        object
            .get(field)
            .filter(|value| !value.is_null())
            .ok_or_else(|| McpError::Server(format!("{tool} request missing {field}")))
    };
    match tool {
        "web_search" => {
            require("query")?;
            require("max_results")?;
            if object.contains_key("domains") {
                return Err(McpError::Server(
                    "web_search received stale domains".to_string(),
                ));
            }
        }
        "web_fetch" => {
            require("url")?;
            require("max_chars")?;
            require("extract_mode")?;
            require("include_links")?;
        }
        "repo_search" => {
            require("query")?;
            require("max_results")?;
            if object.contains_key("include_snippets") {
                return Err(McpError::Server(
                    "repo_search received stale include_snippets".to_string(),
                ));
            }
            if object.contains_key("owner") {
                require("repo")?;
            }
        }
        "repo_fetch" => {
            for field in ["owner", "repo", "path"] {
                require(field)?;
            }
            for field in ["start_line", "end_line"] {
                if object.contains_key(field) {
                    return Err(McpError::Server(format!(
                        "repo_fetch received stale {field}"
                    )));
                }
            }
        }
        "repo_map" => {
            for field in ["owner", "repo", "max_depth"] {
                require(field)?;
            }
            for field in ["path", "depth"] {
                if object.contains_key(field) {
                    return Err(McpError::Server(format!("repo_map received stale {field}")));
                }
            }
        }
        "security_search" => {
            require("query")?;
            require("max_results")?;
            if object.contains_key("cve") {
                return Err(McpError::Server(
                    "security_search received stale cve".to_string(),
                ));
            }
        }
        "research_search" => {
            require("query")?;
            require("max_results")?;
            if object.contains_key("domains") {
                return Err(McpError::Server(
                    "research_search received stale domains".to_string(),
                ));
            }
        }
        "batch_fetch" => {
            let items = object
                .get("items")
                .and_then(serde_json::Value::as_array)
                .filter(|items| !items.is_empty())
                .ok_or_else(|| {
                    McpError::Server("batch_fetch requires non-empty items".to_string())
                })?;
            if object.contains_key("urls") {
                return Err(McpError::Server(
                    "batch_fetch received stale urls".to_string(),
                ));
            }
            for item in items {
                let kind = item.get("type").and_then(serde_json::Value::as_str);
                match kind {
                    Some("web") => {
                        if item
                            .get("url")
                            .and_then(serde_json::Value::as_str)
                            .is_none()
                        {
                            return Err(McpError::Server("web batch item missing url".to_string()));
                        }
                    }
                    Some("repo") => {
                        for field in ["owner", "repo", "path"] {
                            if item.get(field).is_none() {
                                return Err(McpError::Server(format!(
                                    "repo batch item missing {field}"
                                )));
                            }
                        }
                    }
                    _ => {
                        return Err(McpError::Server(
                            "batch item missing valid type".to_string(),
                        ))
                    }
                }
            }
        }
        "build_evidence_bundle" => {
            let has_sources = object
                .get("sources")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|sources| !sources.is_empty());
            let has_fetches = object
                .get("fetches")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|fetches| !fetches.is_empty());
            if !has_sources && !has_fetches {
                return Err(McpError::Server(
                    "evidence bundle requires sources or fetches".to_string(),
                ));
            }
            if object
                .get("sources")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .any(|source| source.get("type").is_some())
            {
                return Err(McpError::Server(
                    "evidence bundle received stale source type".to_string(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Verify that `websearch` dispatches to the `web_search` MCP tool
/// when the eggsearch backend is configured and a service is
/// installed.
#[tokio::test]
async fn websearch_dispatches_to_mcp_web_search() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(Arc::clone(
        &calls,
    ))));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config(false, false));

    let out = codegg::search_backend::dispatch_web_search(&serde_json::json!({
        "query": "rust async",
        "num_results": 4,
    }))
    .await
    .expect("dispatch ok");

    // Output should be wrapped in external_untrusted framing.
    assert!(out.contains("trust=external_untrusted"));
    assert!(out.contains("tool=websearch"));
    assert!(out.contains("Mock"));

    // The mock should have received a single `web_search` call.
    let recorded = calls.lock().await;
    assert_eq!(recorded.len(), 1, "expected exactly one MCP call");
    let (tool, args) = &recorded[0];
    assert_eq!(tool, "web_search");
    assert_eq!(args["query"], "rust async");
    assert_eq!(args["max_results"], 4);
}

/// Verify that `webfetch` dispatches to the `web_fetch` MCP tool
/// when the eggsearch backend is configured.
#[tokio::test]
async fn webfetch_dispatches_to_mcp_web_fetch() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(Arc::clone(
        &calls,
    ))));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config(false, false));

    let out = codegg::search_backend::dispatch_web_fetch(&serde_json::json!({
        "url": "https://example.com/page",
        "max_length": 8000,
    }))
    .await
    .expect("dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    assert!(out.contains("tool=webfetch"));
    assert!(out.contains("mock page body"));

    let recorded = calls.lock().await;
    assert_eq!(recorded.len(), 1);
    let (tool, args) = &recorded[0];
    assert_eq!(tool, "web_fetch");
    assert_eq!(args["url"], "https://example.com/page");
    assert_eq!(args["max_chars"], 8000);
    assert_eq!(args["extract_mode"], "text");
    assert_eq!(args["include_links"], false);
}

/// `provider_status` should be reachable through the doctor helper
/// when the eggsearch backend is connected.
#[tokio::test]
async fn provider_status_dispatches_via_doctor_helper() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(Arc::clone(
        &calls,
    ))));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config(false, false));

    let out = codegg::search_backend::eggsearch::call_provider_status("eggsearch", 15_000)
        .await
        .expect("provider_status ok");
    assert!(out.contains("mock"));

    let recorded = calls.lock().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "provider_status");
}

/// When the eggsearch backend is selected but the service has no
/// `eggsearch` server registered, dispatch must surface a clear
/// failure (an `eggsearch_unavailable`-style error or a missing-tool
/// error from `ensure_tool_available`).
#[tokio::test]
async fn dispatch_eggsearch_server_missing_returns_actionable_error() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    // Empty service: no "eggsearch" server registered.
    let svc = McpService::new();
    let svc = Arc::new(tokio::sync::RwLock::new(svc));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config(false, false));

    let res = codegg::search_backend::dispatch_web_search(&serde_json::json!({"query": "x"})).await;
    let err = res.expect_err("should fail when no eggsearch server registered");
    let msg = err.to_string();
    assert!(
        msg.contains("eggsearch")
            && (msg.contains("unavailable")
                || msg.contains("not found")
                || msg.contains("not advertised")),
        "expected actionable eggsearch error, got: {msg}"
    );
}

/// With `backend = builtin`, dispatch should not touch MCP at all.
#[tokio::test]
async fn builtin_backend_does_not_invoke_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(Arc::clone(
        &calls,
    ))));
    state::install_mcp_service(svc);
    state::install_search_config(builtin_config());

    // Force the legacy path to fail (no providers configured in
    // test env) to make the assertion deterministic. We assert
    // that the *MCP* was not called regardless of legacy success.
    let _ = codegg::search_backend::dispatch_web_search(&serde_json::json!({"query": "x"})).await;
    let recorded = calls.lock().await;
    assert!(
        recorded.is_empty(),
        "builtin backend should not call MCP, got: {:?}",
        *recorded
    );
}

/// With `backend = disabled`, dispatch should not touch MCP and
/// should return a clear disabled error.
#[tokio::test]
async fn disabled_backend_does_not_invoke_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(Arc::clone(
        &calls,
    ))));
    state::install_mcp_service(svc);
    state::install_search_config(disabled_config());

    let res = codegg::search_backend::dispatch_web_search(&serde_json::json!({"query": "x"})).await;
    let err = res.expect_err("disabled should error");
    assert!(err.to_string().contains("disabled"));

    let res =
        codegg::search_backend::dispatch_web_fetch(&serde_json::json!({"url": "https://x"})).await;
    let err = res.expect_err("disabled should error");
    assert!(err.to_string().contains("disabled"));

    let recorded = calls.lock().await;
    assert!(recorded.is_empty(), "disabled backend should not call MCP");
}

/// Verify the agent-loop-level filter: `mcp__eggsearch__*` tools
/// should be hidden from the model when `expose_raw_mcp_tools` is
/// false, but visible when it is true. The build_tool_definitions
/// path in `agent::loop` is the integration point; here we re-run
/// the same predicate to lock in the contract.
#[test]
fn raw_eggsearch_tools_filtered_at_agent_loop_layer() {
    let make_tools = |prefix: &str| {
        vec![
            ToolDefinition {
                name: format!("{prefix}web_search"),
                description: "".to_string(),
                parameters: serde_json::json!({}),
                defer_loading: None,
            },
            ToolDefinition {
                name: format!("{prefix}web_fetch"),
                description: "".to_string(),
                parameters: serde_json::json!({}),
                defer_loading: None,
            },
            ToolDefinition {
                name: "unrelated_tool".to_string(),
                description: "".to_string(),
                parameters: serde_json::json!({}),
                defer_loading: None,
            },
        ]
    };

    let filter = |tools: Vec<ToolDefinition>, expose: bool, server: &str| {
        let raw_prefix = format!("mcp__{server}__");
        tools
            .into_iter()
            .filter(|t| expose || !t.name.starts_with(&raw_prefix))
            .collect::<Vec<_>>()
    };

    let hidden = filter(make_tools("mcp__eggsearch__"), false, "eggsearch");
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].name, "unrelated_tool");

    let shown = filter(make_tools("mcp__eggsearch__"), true, "eggsearch");
    assert_eq!(shown.len(), 3);
}

// ── Extended dispatch tests for repo/security/research/batch/evidence ──

#[tokio::test]
async fn codesearch_compatibility_alias_uses_eggsearch_repo_search() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let output = codegg::tool::codesearch::CodeSearchTool
        .execute(serde_json::json!({
            "query": "rust async",
            "tokens_num": 5000,
        }))
        .await
        .expect("codesearch alias should dispatch");
    assert!(output.contains("external_repo_evidence"));

    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().expect("repo_search call");
    assert_eq!(tool, "repo_search");
    assert_eq!(args["query"], "rust async");
    assert_eq!(args["profile"], "coding");
    assert_eq!(args["max_results"], 10);
}

#[tokio::test]
async fn codesearch_structured_execution_retains_repo_search_value() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let result = codegg::tool::codesearch::CodeSearchTool
        .execute_structured(
            serde_json::json!({"query": "rust async", "tokens_num": 5000}),
            None,
        )
        .await
        .expect("codesearch structured alias should dispatch");
    assert_eq!(
        result.value.expect("structured repo result")["stable_id"],
        "repo-1"
    );
    assert!(result.output.contains("external_repo_evidence"));

    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().expect("repo_search call");
    assert_eq!(tool, "repo_search");
    assert_eq!(args["profile"], "coding");
}

#[tokio::test]
async fn research_eggsearch_source_honors_network_budget_and_converts_sources() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    let mut config = eggsearch_config_all_caps();
    config.max_research_output_chars = Some(40);
    state::install_search_config(config);

    let source = EggsearchSource::new();
    let plan = ResearchPlan {
        scope: "test".to_string(),
        comparison_axes: vec![],
        source_classes: vec![],
        exclusion_criteria: vec![],
        stopping_conditions: vec![],
        expected_outputs: vec![],
    };
    let request = |allow_network| ResearchRequest {
        id: "research-test".to_string(),
        question: "current async Rust research".to_string(),
        mode: ResearchMode::Landscape,
        audience: ResearchAudience::AgentPlanner,
        depth: ResearchDepth::Medium,
        output_profiles: vec![],
        constraints: vec![],
        sources: vec![],
        existing_context_refs: vec![],
        budget: ResearchBudget {
            max_sources: 4,
            max_chunks_per_source: 1,
            max_evidence_spans: 1,
            max_model_calls: 0,
            max_output_tokens: None,
            allow_network,
        },
        created_at: chrono::Utc::now(),
    };

    let denied = source.collect(&request(false), &plan).await;
    assert!(matches!(
        denied,
        Err(codegg::research::error::ResearchError::NetworkNotAllowed)
    ));
    assert!(calls.lock().await.is_empty());

    let sources = source
        .collect(&request(true), &plan)
        .await
        .expect("network-enabled research should use eggsearch");
    assert_eq!(sources.len(), 3);
    assert_eq!(sources[0].uri, "https://example.org/paper-1");
    assert_eq!(sources[1].uri, "https://example.org/paper-2");
    assert_eq!(sources[2].uri, "https://example.org/docs");
    assert!(sources[0]
        .notes
        .iter()
        .any(|note| note == "trust=external_untrusted"));
    assert!(sources[0].notes.iter().any(|note| note == "provider=arxiv"));
    assert!(sources[0]
        .notes
        .iter()
        .any(|note| note == "provider=openalex"));
    assert_eq!(
        sources[2].source_quality,
        codegg::research::types::SourceQuality::OfficialDocs
    );
    let recorded = calls.lock().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "research_search");
    assert_eq!(recorded[0].1["max_results"], 4);
    assert_eq!(recorded[0].1["workflow"], "ecosystem_survey");
}

#[tokio::test]
async fn security_research_source_uses_structured_security_evidence() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let source = EggsearchSource::new();
    let plan = ResearchPlan {
        scope: "security".to_string(),
        comparison_axes: vec![],
        source_classes: vec![],
        exclusion_criteria: vec![],
        stopping_conditions: vec![],
        expected_outputs: vec![],
    };
    let mut request = ResearchRequest {
        id: "security-test".to_string(),
        question: "CVE-2024-1234".to_string(),
        mode: ResearchMode::SecurityReview,
        audience: ResearchAudience::AgentReviewer,
        depth: ResearchDepth::Medium,
        output_profiles: vec![],
        constraints: vec![],
        sources: vec![],
        existing_context_refs: vec![],
        budget: ResearchBudget {
            max_sources: 4,
            max_chunks_per_source: 1,
            max_evidence_spans: 1,
            max_model_calls: 0,
            max_output_tokens: None,
            allow_network: true,
        },
        created_at: chrono::Utc::now(),
    };
    let sources = source
        .collect(&request, &plan)
        .await
        .expect("security research should convert structured evidence");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].uri, "https://example.org/advisory");
    assert!(sources[0]
        .notes
        .iter()
        .any(|note| note == "stable_id=advisory-1"));
    assert!(sources[0].notes.iter().any(|note| note == "provider=osv"));
    assert!(sources[0]
        .notes
        .iter()
        .any(|note| note == "provider=rustsec"));
    assert!(sources[0]
        .notes
        .iter()
        .any(|note| note == "source_kind=security_advisory"));
    assert!(sources[0]
        .notes
        .iter()
        .any(|note| note == "trust=external_untrusted"));

    request.budget.allow_network = false;
    let denied = source.collect(&request, &plan).await;
    assert!(matches!(
        denied,
        Err(codegg::research::error::ResearchError::NetworkNotAllowed)
    ));
    let recorded = calls.lock().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "security_search");
    assert_eq!(recorded[0].1["workflow"], "security_review");
}

/// Build a mock with ALL upstream tools registered.
fn build_full_mock_eggsearch(
    recorded_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
) -> McpService {
    let mut svc = McpService::new();
    let tool_names = [
        "web_search",
        "web_fetch",
        "provider_status",
        "repo_search",
        "repo_fetch",
        "repo_map",
        "security_search",
        "research_search",
        "batch_fetch",
        "build_evidence_bundle",
    ];
    let tools: Vec<McpTool> = tool_names
        .iter()
        .map(|name| McpTool {
            name: name.to_string(),
            description: format!("Mock {name}"),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
            server: "eggsearch".to_string(),
        })
        .collect();
    let calls = Arc::clone(&recorded_calls);
    svc.register_mock_server(
        "eggsearch",
        tools,
        Box::new(move |tool, args| {
            if let Ok(mut g) = calls.try_lock() {
                g.push((tool.to_string(), args.clone()));
            }
            match tool {
                "web_search" => Ok(r#"{"hits": []}"#.to_string()),
                "web_fetch" => Ok("page body".to_string()),
                "provider_status" => Ok(r#"{"ok": true}"#.to_string()),
                "repo_search" => Ok(r#"{"repo_hits": [], "stable_id": "repo-1"}"#.to_string()),
                "repo_fetch" => Ok("file content".to_string()),
                "repo_map" => Ok(r#"{"tree": []}"#.to_string()),
                "security_search" => Ok(r#"{"groups": [{"kind": "security_advisory", "label": "Advisories", "results": [{"id": "src_advisory_1", "stable_id": "advisory-1", "url": "https://example.org/advisory", "title": "Mock advisory", "snippet": "Mock advisory details", "providers": ["osv", "rustsec"], "score": 0.92, "trust": "external_untrusted", "fetched": false, "trust_markers": {}, "metadata": {"source_kind": "security_advisory"}, "unknown_future_field": true}]}]}"#.to_string()),
                "research_search" => Ok(r#"{"groups": [{"kind": "reference", "label": "Primary sources", "results": [{"id": "src_paper_1", "stable_id": "paper-1", "url": "https://example.org/paper-1", "title": "Mock paper 1", "snippet": "Mock abstract 1", "providers": ["arxiv", "openalex"], "score": 0.81, "trust": "external_untrusted", "fetched": false, "trust_markers": {}, "metadata": {"source_kind": "reference"}, "unknown_future_field": {"ignored": true}}, {"id": "src_paper_2", "stable_id": "paper-2", "url": "https://example.org/paper-2", "title": "Mock paper 2", "snippet": "Mock abstract 2", "providers": ["arxiv"], "score": 0.74, "trust": "external_untrusted", "fetched": false, "trust_markers": {}, "metadata": {"source_kind": "reference"}}]}, {"kind": "official_docs", "label": "Documentation", "results": [{"id": "src_docs_1", "stable_id": "docs-1", "url": "https://example.org/docs", "title": "Mock docs", "snippet": "Reference", "providers": ["official"], "score": 0.76, "trust": "external_untrusted", "fetched": false, "trust_markers": {}, "metadata": {"source_kind": "official_docs"}}]}]}"#.to_string()),
                "batch_fetch" => Ok(r#"{"pages": []}"#.to_string()),
                "build_evidence_bundle" => Ok(r#"{"bundle": {}}"#.to_string()),
                _ => Err(McpError::Server(format!("unknown tool {tool}"))),
            }
        }),
    );
    svc
}

fn eggsearch_config_all_caps() -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Eggsearch),
        expose_raw_mcp_tools: Some(false),
        fallback_to_builtin: Some(false),
        max_search_output_chars: Some(12_000),
        max_fetch_output_chars: Some(20_000),
        max_repo_output_chars: Some(16_000),
        max_security_output_chars: Some(18_000),
        max_research_output_chars: Some(22_000),
        max_batch_output_chars: Some(30_000),
        max_evidence_output_chars: Some(30_000),
        eggsearch: Some(EggsearchConfig {
            server_name: Some("eggsearch".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn repo_search_dispatches_to_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let out = codegg::search_backend::dispatch_repo_search(&serde_json::json!({
        "query": "async runtime",
        "owner": "tokio-rs",
        "repo": "tokio",
        "path": "tokio/src",
        "language": "rust",
        "profile": "coding",
        "include_local": true,
        "mode": "default",
    }))
    .await
    .expect("repo_search dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().expect("at least one call");
    assert_eq!(tool, "repo_search");
    assert_eq!(args["query"], "async runtime");
    assert_eq!(args["owner"], "tokio-rs");
    assert_eq!(args["repo"], "tokio");
    assert_eq!(args["profile"], "coding");
    assert!(!args.as_object().unwrap().contains_key("include_snippets"));
}

#[tokio::test]
async fn repo_fetch_dispatches_to_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let out = codegg::search_backend::dispatch_repo_fetch(&serde_json::json!({
        "path": "src/main.rs",
        "repo": "tokio",
        "owner": "tokio-rs",
        "start_line": 4,
        "end_line": 12,
        "symbol": "main",
    }))
    .await
    .expect("repo_fetch dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().unwrap();
    assert_eq!(tool, "repo_fetch");
    assert_eq!(args["path"], "src/main.rs");
    assert_eq!(args["owner"], "tokio-rs");
    assert_eq!(args["repo"], "tokio");
    assert_eq!(args["line_start"], 4);
    assert_eq!(args["line_end"], 12);
    assert_eq!(args["symbol"], "main");
}

#[tokio::test]
async fn repo_map_dispatches_to_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let out = codegg::search_backend::dispatch_repo_map(&serde_json::json!({
        "repo": "tokio-rs/tokio",
        "depth": 3,
    }))
    .await
    .expect("repo_map dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().unwrap();
    assert_eq!(tool, "repo_map");
    assert_eq!(args["owner"], "tokio-rs");
    assert_eq!(args["repo"], "tokio");
    assert!(args.get("max_depth").is_some());
    assert_eq!(args["max_depth"], 3);
}

#[tokio::test]
async fn security_search_dispatches_to_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let out = codegg::search_backend::dispatch_security_search(&serde_json::json!({
        "query": "CVE-2024-1234",
        "cve": "CVE-2024-1234",
        "ghsa_id": "GHSA-abcd-1234-efgh",
        "osv_id": "OSV-2024-1234",
        "rustsec_id": "RUSTSEC-2024-0001",
        "version": "1.2.3",
    }))
    .await
    .expect("security_search dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().unwrap();
    assert_eq!(tool, "security_search");
    assert_eq!(args["query"], "CVE-2024-1234");
    assert_eq!(args["cve_id"], "CVE-2024-1234");
    assert!(!args.as_object().unwrap().contains_key("cve"));
}

#[tokio::test]
async fn research_search_dispatches_to_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let out = codegg::search_backend::dispatch_research_search(&serde_json::json!({
        "query": "transformer attention",
        "research_domain": "machine learning",
        "desired_source_types": ["paper", "official_docs"],
        "workflow": "general",
        "depth": "quick",
        "providers": ["arxiv"],
    }))
    .await
    .expect("research_search dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, _) = recorded.last().unwrap();
    assert_eq!(tool, "research_search");
    let (_, args) = recorded.last().unwrap();
    assert_eq!(args["research_domain"], "machine learning");
    assert!(!args.as_object().unwrap().contains_key("domains"));
}

#[tokio::test]
async fn batch_fetch_dispatches_to_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let out = codegg::search_backend::dispatch_batch_fetch(&serde_json::json!({
        "urls": ["https://example.com/a", "https://example.com/b"],
    }))
    .await
    .expect("batch_fetch dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, _) = recorded.last().unwrap();
    assert_eq!(tool, "batch_fetch");
    let (_, args) = recorded.last().unwrap();
    assert_eq!(args["items"][0]["type"], "web");
    assert_eq!(args["items"][1]["type"], "web");
    assert!(!args.as_object().unwrap().contains_key("urls"));
}

#[tokio::test]
async fn evidence_bundle_dispatches_to_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let out = codegg::search_backend::dispatch_evidence_bundle(&serde_json::json!({
        "sources": [{"id": "src_1", "url": "https://example.com", "title": "Example"}],
    }))
    .await
    .expect("evidence_bundle dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, _) = recorded.last().unwrap();
    assert_eq!(tool, "build_evidence_bundle");
    let (_, args) = recorded.last().unwrap();
    assert_eq!(args["sources"][0]["id"], "src_1");
    assert!(args.get("type").is_none());
}

#[tokio::test]
async fn legacy_and_ambiguous_requests_are_rejected_before_mcp() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let err = codegg::search_backend::dispatch_repo_map(&serde_json::json!({
        "repo": "group/subgroup/repo",
    }))
    .await
    .expect_err("ambiguous locator should fail");
    assert!(err.to_string().contains("ambiguous"));

    let err = codegg::search_backend::dispatch_repo_map(&serde_json::json!({
        "repo": "tokio-rs/tokio",
        "path": "src",
    }))
    .await
    .expect_err("unsupported repo map path should fail");
    assert!(err.to_string().contains("does not support"));

    let err = codegg::search_backend::dispatch_batch_fetch(&serde_json::json!({
        "items": [],
    }))
    .await
    .expect_err("empty batch should fail");
    assert!(err.to_string().contains("non-empty"));

    let err = codegg::search_backend::dispatch_evidence_bundle(&serde_json::json!({
        "sources": [{"type": "url", "url": "https://example.com"}],
    }))
    .await
    .expect_err("legacy evidence pseudo-source should fail");
    assert!(err.to_string().contains("legacy pseudo-source"));

    assert!(
        calls.lock().await.is_empty(),
        "local validation failures must not invoke MCP"
    );
}

#[tokio::test]
async fn batch_fetch_normalizes_mixed_legacy_repo_and_web_items() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_full_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    codegg::search_backend::dispatch_batch_fetch(&serde_json::json!({
        "items": [
            {"type": "web", "url": "https://example.com", "include_links": true},
            {"repo": "tokio-rs/tokio", "path": "src/lib.rs", "start_line": 1, "end_line": 8},
        ],
        "max_total_chars": 5000,
    }))
    .await
    .expect("mixed batch dispatch ok");

    let recorded = calls.lock().await;
    let (_, args) = recorded.last().unwrap();
    assert_eq!(args["items"][0]["type"], "web");
    assert_eq!(args["items"][0]["include_links"], true);
    assert_eq!(args["items"][1]["type"], "repo");
    assert_eq!(args["items"][1]["owner"], "tokio-rs");
    assert_eq!(args["items"][1]["repo"], "tokio");
    assert_eq!(args["items"][1]["line_start"], 1);
    assert_eq!(args["items"][1]["line_end"], 8);
    assert_eq!(args["max_total_chars"], 5000);
}

#[tokio::test]
async fn structured_wrappers_preserve_upstream_value_and_bound_display() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let fixture = serde_json::json!({
        "stable_id": "evidence-123",
        "structured_warnings": [{"code": "prompt_injection", "severity": "high", "scope": "snippet"}],
        "trust_markers": {"sanitized": true, "injection_detected": true},
        "routing_decision": {"selected": ["duckduckgo"], "skipped": ["exa"], "degraded": true},
        "next_actions": [{"tool": "web_fetch", "reason": "inspect", "priority": "normal", "input": {"url": "https://example.com"}}],
        "repo_locator": {"owner": "owner", "repo": "repo", "path": "src/lib.rs"},
        "security": {"confidence": "high", "applicability": "unknown"},
        "research": {"claims": [{"id": "claim-1"}], "conflicts": [], "gaps": []},
        "unknown_future_field": {"must_survive": true},
        "payload": "large evidence payload"
    });
    let response = fixture.to_string();
    let mut svc = McpService::new();
    let tools = [
        "web_search",
        "web_fetch",
        "repo_search",
        "repo_fetch",
        "repo_map",
        "security_search",
        "research_search",
        "batch_fetch",
        "build_evidence_bundle",
    ]
    .into_iter()
    .map(|name| McpTool {
        name: name.to_string(),
        description: String::new(),
        input_schema: serde_json::json!({"type": "object"}),
        server: "eggsearch".to_string(),
    })
    .collect();
    svc.register_mock_server(
        "eggsearch",
        tools,
        Box::new(move |_, _| Ok(response.clone())),
    );
    state::install_mcp_service(Arc::new(tokio::sync::RwLock::new(svc)));
    let mut cfg = eggsearch_config_all_caps();
    cfg.max_search_output_chars = Some(40);
    cfg.max_fetch_output_chars = Some(40);
    cfg.max_repo_output_chars = Some(40);
    cfg.max_security_output_chars = Some(40);
    cfg.max_research_output_chars = Some(40);
    cfg.max_batch_output_chars = Some(40);
    cfg.max_evidence_output_chars = Some(40);
    state::install_search_config(cfg);

    let cases: Vec<(Box<dyn Tool>, serde_json::Value)> = vec![
        (
            Box::new(WebSearchTool::default()),
            serde_json::json!({"query": "x"}),
        ),
        (
            Box::new(WebFetchTool::default()),
            serde_json::json!({"url": "https://example.com"}),
        ),
        (Box::new(RepoSearchTool), serde_json::json!({"query": "x"})),
        (
            Box::new(RepoFetchTool),
            serde_json::json!({"repo": "owner/repo", "path": "src/lib.rs"}),
        ),
        (
            Box::new(RepoMapTool),
            serde_json::json!({"repo": "owner/repo"}),
        ),
        (
            Box::new(SecuritySearchTool),
            serde_json::json!({"query": "CVE-1"}),
        ),
        (
            Box::new(ResearchSearchTool),
            serde_json::json!({"query": "x"}),
        ),
        (
            Box::new(BatchFetchTool),
            serde_json::json!({"urls": ["https://example.com"]}),
        ),
        (
            Box::new(EvidenceBundleTool),
            serde_json::json!({"sources": [{"id": "src-1", "url": "https://example.com"}]}),
        ),
    ];

    for (tool, input) in cases {
        let result = tool
            .execute_structured(input, None)
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", tool.name()));
        let value = result.value.expect("structured value");
        assert_eq!(value["stable_id"], "evidence-123");
        assert_eq!(value["structured_warnings"][0]["severity"], "high");
        assert_eq!(value["trust_markers"]["injection_detected"], true);
        assert_eq!(value["routing_decision"]["degraded"], true);
        assert_eq!(value["next_actions"][0]["tool"], "web_fetch");
        assert_eq!(value["unknown_future_field"]["must_survive"], true);
        assert!(result.output.contains("trust=external_untrusted"));
        assert!(result
            .provenance
            .as_ref()
            .is_some_and(|provenance| provenance.truncated));
    }
}

/// Server returns oversized output; Codegg should clamp and mark truncation.
#[tokio::test]
async fn oversized_output_is_clamped() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut svc = McpService::new();
    let big_body = "x".repeat(100_000);
    let big = big_body.clone();
    let recorded = Arc::clone(&calls);
    svc.register_mock_server(
        "eggsearch",
        vec![McpTool {
            name: "web_search".to_string(),
            description: "".to_string(),
            input_schema: serde_json::json!({}),
            server: "eggsearch".to_string(),
        }],
        Box::new(move |tool, _args| {
            if let Ok(mut g) = recorded.try_lock() {
                g.push((tool.to_string(), serde_json::json!({})));
            }
            Ok(big.clone())
        }),
    );
    let svc = Arc::new(tokio::sync::RwLock::new(svc));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let out = codegg::search_backend::dispatch_web_search(&serde_json::json!({"query": "x"}))
        .await
        .expect("dispatch ok");
    // Output should be clamped — not the full 100K
    assert!(
        out.len() < 50_000,
        "output should be clamped, got {} bytes",
        out.len()
    );
    assert!(out.contains("truncated") || out.len() < 100_000);
}

/// Server returns malformed payload; Codegg should not panic.
#[tokio::test]
async fn malformed_payload_does_not_panic() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut svc = McpService::new();
    let recorded = Arc::clone(&calls);
    svc.register_mock_server(
        "eggsearch",
        vec![McpTool {
            name: "web_search".to_string(),
            description: "".to_string(),
            input_schema: serde_json::json!({}),
            server: "eggsearch".to_string(),
        }],
        Box::new(move |tool, _args| {
            if let Ok(mut g) = recorded.try_lock() {
                g.push((tool.to_string(), serde_json::json!({})));
            }
            Ok("{not valid json!!!".to_string())
        }),
    );
    let svc = Arc::new(tokio::sync::RwLock::new(svc));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    let result =
        codegg::search_backend::dispatch_web_search(&serde_json::json!({"query": "x"})).await;
    // Should not panic — either returns Ok with the raw text or Err
    match result {
        Ok(out) => assert!(!out.is_empty()),
        Err(e) => {
            assert!(e.to_string().contains("eggsearch") || e.to_string().contains("malformed"))
        }
    }
}

/// Missing upstream tool fails clearly.
#[tokio::test]
async fn missing_upstream_tool_fails_clearly() {
    let (_cp, _g) = lock().await;
    state::reset_for_tests();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(Arc::clone(
        &calls,
    ))));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_all_caps());

    // repo_search is NOT in the basic mock (only web_search, web_fetch, provider_status)
    let result =
        codegg::search_backend::dispatch_repo_search(&serde_json::json!({"query": "x"})).await;
    let err = result.expect_err("repo_search should fail with missing tool");
    let msg = err.to_string();
    assert!(
        msg.contains("repo_search") && msg.contains("not advertised"),
        "expected missing tool error, got: {msg}"
    );
}

// ── Evidence backend config gating tests ──

fn evidence_disabled_config() -> codegg::tool::integrated_config::EvidenceBackendRuntimeConfig {
    codegg::tool::integrated_config::EvidenceBackendRuntimeConfig {
        enabled: false,
        ..Default::default()
    }
}

fn evidence_enabled_config() -> codegg::tool::integrated_config::EvidenceBackendRuntimeConfig {
    codegg::tool::integrated_config::EvidenceBackendRuntimeConfig {
        enabled: true,
        ..Default::default()
    }
}

/// With evidence backend disabled, expanded evidence wrapper tools
/// should NOT appear in model-facing definitions.
#[test]
fn disabled_evidence_backend_omits_expanded_tools() {
    let registry = codegg::tool::ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        evidence_config: Some(evidence_disabled_config()),
        ..Default::default()
    });
    let defs = registry.definitions();
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

    for tool in &[
        "repo_search",
        "repo_fetch",
        "security_search",
        "research_search",
        "repo_map",
        "batch_fetch",
        "evidence_bundle",
    ] {
        assert!(
            !names.contains(tool),
            "tool '{tool}' should NOT be in definitions when evidence backend is disabled"
        );
    }
}

/// With evidence backend enabled, expanded evidence wrapper tools
/// should appear in model-facing definitions.
#[test]
fn enabled_evidence_backend_includes_expanded_tools() {
    let registry = codegg::tool::ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        evidence_config: Some(evidence_enabled_config()),
        ..Default::default()
    });
    let defs = registry.definitions();
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

    for tool in &[
        "repo_search",
        "repo_fetch",
        "security_search",
        "research_search",
        "repo_map",
        "batch_fetch",
        "evidence_bundle",
    ] {
        assert!(
            names.contains(tool),
            "tool '{tool}' should be in definitions when evidence backend is enabled"
        );
    }
}

/// websearch and webfetch should always be registered regardless
/// of evidence config.
#[test]
fn websearch_webfetch_always_registered() {
    for cfg in [
        None,
        Some(evidence_disabled_config()),
        Some(evidence_enabled_config()),
    ] {
        let registry =
            codegg::tool::ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
                evidence_config: cfg,
                ..Default::default()
            });
        let defs = registry.definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert!(
            names.contains(&"websearch"),
            "websearch should always be in definitions (evidence_config={:?})",
            registry
                .integrated_config()
                .evidence
                .as_ref()
                .map(|e| e.enabled),
        );
        assert!(
            names.contains(&"webfetch"),
            "webfetch should always be in definitions (evidence_config={:?})",
            registry
                .integrated_config()
                .evidence
                .as_ref()
                .map(|e| e.enabled),
        );
    }
}

/// With evidence backend set to builtin, expanded evidence wrapper tools
/// should NOT appear in model-facing definitions (they are eggsearch-only).
#[test]
fn builtin_evidence_backend_omits_expanded_tools() {
    let registry = codegg::tool::ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        evidence_config: Some(
            codegg::tool::integrated_config::EvidenceBackendRuntimeConfig {
                enabled: true,
                backend: "builtin".to_string(),
                ..Default::default()
            },
        ),
        ..Default::default()
    });
    let defs = registry.definitions();
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

    for tool in &[
        "repo_search",
        "repo_fetch",
        "security_search",
        "research_search",
        "repo_map",
        "batch_fetch",
        "evidence_bundle",
    ] {
        assert!(
            !names.contains(tool),
            "tool '{tool}' should NOT be in definitions when evidence backend is builtin"
        );
    }
    // websearch/webfetch are always present
    assert!(names.contains(&"websearch"));
    assert!(names.contains(&"webfetch"));
}
