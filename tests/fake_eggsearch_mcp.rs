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
use codegg::search_backend::state;
use codegg::search_backend::test_support::{
    acquire_cross_process_lock, CrossProcessLockGuard, SHARED_TEST_LOCK,
};
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
                "repo_search" => Ok(r#"{"repo_hits": []}"#.to_string()),
                "repo_fetch" => Ok("file content".to_string()),
                "repo_map" => Ok(r#"{"tree": []}"#.to_string()),
                "security_search" => Ok(r#"{"vulns": []}"#.to_string()),
                "research_search" => Ok(r#"{"papers": []}"#.to_string()),
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
    }))
    .await
    .expect("repo_search dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().expect("at least one call");
    assert_eq!(tool, "repo_search");
    assert_eq!(args["query"], "async runtime");
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
        "repo": "tokio-rs/tokio",
    }))
    .await
    .expect("repo_fetch dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().unwrap();
    assert_eq!(tool, "repo_fetch");
    assert_eq!(args["path"], "src/main.rs");
    assert_eq!(args["repo"], "tokio-rs/tokio");
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
    }))
    .await
    .expect("repo_map dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().unwrap();
    assert_eq!(tool, "repo_map");
    assert_eq!(args["repo"], "tokio-rs/tokio");
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
    }))
    .await
    .expect("security_search dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, args) = recorded.last().unwrap();
    assert_eq!(tool, "security_search");
    assert_eq!(args["query"], "CVE-2024-1234");
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
    }))
    .await
    .expect("research_search dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, _) = recorded.last().unwrap();
    assert_eq!(tool, "research_search");
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
        "sources": [{"type": "url", "url": "https://example.com"}],
    }))
    .await
    .expect("evidence_bundle dispatch ok");

    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    let (tool, _) = recorded.last().unwrap();
    assert_eq!(tool, "build_evidence_bundle");
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
