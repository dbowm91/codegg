//! M005 cross-context isolation tests.
//!
//! Proves that independently constructed `SearchRuntimeContext` values
//! coexist in one process without overwriting one another:
//!
//! - two contexts with different search configs execute with their own
//!   backend semantics (no cross-talk), including concurrently;
//! - an eggsearch-backed tool uses its injected mock MCP service;
//! - a disabled context stays disabled even while another context has
//!   search enabled;
//! - dropping and reconstructing a context leaks no previous
//!   config/service into the new instance;
//! - an explicit disabled context never falls back to a global/default
//!   backend.
//!
//! No process-global install/reset and no serialization locks are used.

use std::sync::Arc;

use codegg::config::schema::{EggsearchConfig, SearchBackendConfig, SearchConfig};
use codegg::error::McpError;
use codegg::mcp::{McpService, McpTool};
use codegg::search_backend::SearchRuntimeContext;
use codegg::tool::Tool;
use tokio::sync::Mutex;

fn eggsearch_config() -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Eggsearch),
        fallback_to_builtin: Some(false),
        eggsearch: Some(EggsearchConfig {
            server_name: Some("eggsearch".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn disabled_config() -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Disabled),
        ..Default::default()
    }
}

/// Build a mock eggsearch service recording calls into `calls`.
fn mock_service(calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>) -> McpService {
    let mut svc = McpService::new();
    svc.register_mock_server(
        "eggsearch",
        vec![McpTool {
            name: "web_search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            server: "eggsearch".to_string(),
        }],
        Box::new(move |tool, args| {
            if let Ok(mut g) = calls.try_lock() {
                g.push((tool.to_string(), args.clone()));
            }
            Ok(r#"{"hits": []}"#.to_string())
        }),
    );
    svc
}

#[tokio::test]
async fn two_contexts_with_different_configs_coexist() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let enabled = SearchRuntimeContext::new(eggsearch_config()).with_mcp(Arc::new(
        tokio::sync::RwLock::new(mock_service(Arc::clone(&calls))),
    ));
    let disabled = SearchRuntimeContext::disabled();

    // Interleaved execution: each context observes only its own config.
    let ok = enabled
        .dispatch_web_search(&serde_json::json!({"query": "x"}))
        .await;
    assert!(ok.is_ok(), "enabled context should dispatch, got {ok:?}");
    let err = disabled
        .dispatch_web_search(&serde_json::json!({"query": "x"}))
        .await
        .expect_err("disabled context must error");
    assert!(err.to_string().contains("disabled"));
    // The enabled context is unaffected by the disabled one's existence.
    let ok = enabled
        .dispatch_web_search(&serde_json::json!({"query": "y"}))
        .await;
    assert!(ok.is_ok(), "enabled context must stay enabled, got {ok:?}");
    assert_eq!(calls.lock().await.len(), 2);
}

#[tokio::test]
async fn concurrent_contexts_do_not_cross_talk() {
    let calls_a = Arc::new(Mutex::new(Vec::new()));
    let ctx_a = SearchRuntimeContext::new(eggsearch_config()).with_mcp(Arc::new(
        tokio::sync::RwLock::new(mock_service(Arc::clone(&calls_a))),
    ));
    let ctx_b = SearchRuntimeContext::disabled();

    // No locks: both futures run concurrently on owned contexts.
    let input_a = serde_json::json!({"query": "a"});
    let input_b = serde_json::json!({"query": "b"});
    let (ra, rb) = tokio::join!(
        ctx_a.dispatch_web_search(&input_a),
        ctx_b.dispatch_web_search(&input_b),
    );
    assert!(ra.is_ok());
    let err = rb.expect_err("disabled must error under concurrency");
    assert!(err.to_string().contains("disabled"));
    assert_eq!(calls_a.lock().await.len(), 1);
}

#[tokio::test]
async fn wrapper_tool_uses_its_injected_service() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let ctx = SearchRuntimeContext::new(eggsearch_config()).with_mcp(Arc::new(
        tokio::sync::RwLock::new(mock_service(Arc::clone(&calls))),
    ));
    let tool = codegg::tool::websearch::WebSearchTool::with_search_runtime(ctx);
    let out = tool
        .execute(serde_json::json!({"query": "hello"}))
        .await
        .expect("injected service should serve the call");
    assert!(out.contains("trust=external_untrusted"));
    let recorded = calls.lock().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "web_search");
}

#[tokio::test]
async fn disabled_tool_reports_disabled_with_injected_service_present() {
    // Even holding a live MCP handle, a disabled context must not serve.
    let calls = Arc::new(Mutex::new(Vec::new()));
    let ctx = SearchRuntimeContext::new(disabled_config()).with_mcp(Arc::new(
        tokio::sync::RwLock::new(mock_service(Arc::clone(&calls))),
    ));
    let tool = codegg::tool::websearch::WebSearchTool::with_search_runtime(ctx);
    let err = tool
        .execute(serde_json::json!({"query": "x"}))
        .await
        .expect_err("disabled tool must error");
    assert!(err.to_string().contains("disabled"));
    assert!(
        calls.lock().await.is_empty(),
        "disabled context must not invoke MCP"
    );
}

#[tokio::test]
async fn reconstructed_context_observes_no_stale_config() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let first = SearchRuntimeContext::new(eggsearch_config()).with_mcp(Arc::new(
        tokio::sync::RwLock::new(mock_service(Arc::clone(&calls))),
    ));
    assert!(first
        .dispatch_web_search(&serde_json::json!({"query": "x"}))
        .await
        .is_ok());
    drop(first);

    // Reconstruct as disabled: the previous enabled config must not leak.
    let second = SearchRuntimeContext::disabled();
    let err = second
        .dispatch_web_search(&serde_json::json!({"query": "x"}))
        .await
        .expect_err("reconstructed disabled context must error");
    assert!(err.to_string().contains("disabled"));
}

#[tokio::test]
async fn eggsearch_without_service_is_unavailable_not_builtin() {
    // Explicit eggsearch config with no MCP handle: actionable
    // unavailable error, never a silent builtin fallback.
    let ctx = SearchRuntimeContext::new(eggsearch_config());
    let err = ctx
        .dispatch_web_search(&serde_json::json!({"query": "x"}))
        .await
        .expect_err("missing service must error");
    assert!(
        err.to_string().contains("eggsearch"),
        "expected eggsearch-unavailable, got {err}"
    );
}

#[tokio::test]
async fn registries_with_different_contexts_are_independent() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let enabled = SearchRuntimeContext::new(eggsearch_config()).with_mcp(Arc::new(
        tokio::sync::RwLock::new(mock_service(Arc::clone(&calls))),
    ));
    let enabled_registry =
        codegg::tool::ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
            search_runtime: Some(enabled),
            ..Default::default()
        });
    let disabled_registry =
        codegg::tool::ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
            search_runtime: Some(SearchRuntimeContext::disabled()),
            ..Default::default()
        });

    let out = enabled_registry
        .execute_capture("websearch", serde_json::json!({"query": "x"}), None)
        .await;
    assert!(out.is_ok(), "enabled registry should serve, got {out:?}");
    let err = disabled_registry
        .execute_capture("websearch", serde_json::json!({"query": "x"}), None)
        .await
        .expect_err("disabled registry must error");
    assert!(err.to_string().contains("disabled"));
}

#[test]
fn provenance_reflects_each_context_backend() {
    let enabled = SearchRuntimeContext::new(eggsearch_config());
    let disabled = SearchRuntimeContext::disabled();
    let builtin = SearchRuntimeContext::new(SearchConfig {
        backend: Some(SearchBackendConfig::Builtin),
        ..Default::default()
    });
    assert_eq!(enabled.provenance_for_search(None).unwrap().backend, "mcp");
    assert_eq!(
        disabled.provenance_for_search(None).unwrap().implementation,
        "disabled"
    );
    assert_eq!(
        builtin.provenance_for_search(None).unwrap().implementation,
        "codegg/legacy"
    );
}

#[test]
fn failing_mock_does_not_fall_back_without_opt_in() {
    // Failing upstream + fallback disabled => the eggsearch error
    // surfaces; no silent backend switch.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut svc = McpService::new();
        svc.register_mock_server(
            "eggsearch",
            vec![McpTool {
                name: "web_search".to_string(),
                description: "".to_string(),
                input_schema: serde_json::json!({}),
                server: "eggsearch".to_string(),
            }],
            Box::new(|_, _| Err(McpError::Server("boom".to_string()))),
        );
        let ctx = SearchRuntimeContext::new(eggsearch_config())
            .with_mcp(Arc::new(tokio::sync::RwLock::new(svc)));
        let err = ctx
            .dispatch_web_search(&serde_json::json!({"query": "x"}))
            .await
            .expect_err("upstream failure must surface");
        assert!(
            err.to_string().contains("boom") || err.to_string().contains("eggsearch"),
            "unexpected error: {err}"
        );
    });
}
