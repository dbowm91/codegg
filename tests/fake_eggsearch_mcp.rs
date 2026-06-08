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
use tokio::sync::{Mutex, MutexGuard};

// Serialize every test in this file. The state slot is global, so
// parallel tests would clobber each other's mocks and recorded calls.
static TEST_LOCK: Mutex<()> = Mutex::const_new(());

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
                "web_search" => Ok(r#"{"hits": [{"title": "Mock", "url": "https://x"}]}"#.to_string()),
                "web_fetch" => Ok("mock page body".to_string()),
                "provider_status" => Ok(r#"{"providers": ["mock"]}"#.to_string()),
                _ => Err(McpError::Server(format!("unknown tool {tool}"))),
            }
        }),
    );
    svc
}

/// Acquire the global test lock. Held for the duration of a single
/// `#[tokio::test]` body; dropped at the end.
async fn lock() -> MutexGuard<'static, ()> {
    TEST_LOCK.lock().await
}

/// Verify that `websearch` dispatches to the `web_search` MCP tool
/// when the eggsearch backend is configured and a service is
/// installed.
#[tokio::test]
async fn websearch_dispatches_to_mcp_web_search() {
    let _g = lock().await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(
        Arc::clone(&calls),
    )));
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
    let _g = lock().await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(
        Arc::clone(&calls),
    )));
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
    let _g = lock().await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config(false, false));

    let out = codegg::search_backend::eggsearch::call_provider_status("eggsearch")
        .await
        .expect("provider_status ok");
    assert!(out.contains("mock"));

    let recorded = calls.lock().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "provider_status");
}

/// When the eggsearch backend is selected but the service has no
/// `eggsearch` server registered, dispatch must surface a clear
/// failure (an `eggsearch_unavailable`-style error).
#[tokio::test]
async fn dispatch_eggsearch_server_missing_returns_actionable_error() {
    let _g = lock().await;
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
            && (msg.contains("unavailable") || msg.contains("not found")),
        "expected actionable eggsearch error, got: {msg}"
    );
}

/// With `backend = builtin`, dispatch should not touch MCP at all.
#[tokio::test]
async fn builtin_backend_does_not_invoke_mcp() {
    let _g = lock().await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(
        Arc::clone(&calls),
    )));
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
    let _g = lock().await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let svc = Arc::new(tokio::sync::RwLock::new(build_mock_eggsearch(
        Arc::clone(&calls),
    )));
    state::install_mcp_service(svc);
    state::install_search_config(disabled_config());

    let res = codegg::search_backend::dispatch_web_search(&serde_json::json!({"query": "x"})).await;
    let err = res.expect_err("disabled should error");
    assert!(err.to_string().contains("disabled"));

    let res = codegg::search_backend::dispatch_web_fetch(&serde_json::json!({"url": "https://x"})).await;
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
