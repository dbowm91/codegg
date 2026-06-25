//! Tests for the legacy built-in websearch/webfetch fallback path.
//!
//! The legacy path is exercised when:
//!
//! - `search.backend = "builtin"`, OR
//! - `search.backend = "eggsearch"` AND `search.fallback_to_builtin = true`
//!   AND the eggsearch backend is unavailable.
//!
//! These tests exercise the *search* legacy path. The webfetch legacy
//! path requires a real network round-trip and is left to manual
//! smoke tests; we assert the dispatch wiring here.

use codegg::config::schema::{EggsearchConfig, SearchBackendConfig, SearchConfig};
use codegg::error::McpError;
use codegg::mcp::{McpService, McpTool};
use codegg::search_backend::state;
use codegg::search_backend::test_support::{
    acquire_cross_process_lock, CrossProcessLockGuard, SHARED_TEST_LOCK,
};
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};

async fn lock() -> (CrossProcessLockGuard, MutexGuard<'static, ()>) {
    let cp = acquire_cross_process_lock();
    let g = SHARED_TEST_LOCK.lock().await;
    (cp, g)
}

fn builtin_config() -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Builtin),
        ..Default::default()
    }
}

fn eggsearch_config_with_fallback() -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Eggsearch),
        fallback_to_builtin: Some(true),
        eggsearch: Some(EggsearchConfig::default()),
        ..Default::default()
    }
}

/// With `backend = builtin`, the eggsearch MCP service must not be
/// invoked at all. The dispatch is expected to surface a "no
/// provider configured" error in environments without API keys, but
/// the *assertion* is that the MCP service is untouched.
#[tokio::test]
async fn builtin_backend_does_not_touch_mcp_service() {
    state::reset_for_tests();
    let (_cp, _g) = lock().await;
    let calls = Arc::new(Mutex::new(Vec::<(String, serde_json::Value)>::new()));
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
        Box::new(move |tool, args| {
            if let Ok(mut g) = recorded.try_lock() {
                g.push((tool.to_string(), args.clone()));
            }
            Err(McpError::Server("should not be called".into()))
        }),
    );
    let svc = Arc::new(tokio::sync::RwLock::new(svc));
    state::install_mcp_service(svc);
    state::install_search_config(builtin_config());

    // The legacy path will likely error out (no API keys in test
    // env) but that is fine for this test. We only assert that the
    // MCP service was not consulted.
    let _ = codegg::search_backend::dispatch_web_search(&serde_json::json!({"query": "x"})).await;
    let recorded = calls.lock().await;
    assert!(
        recorded.is_empty(),
        "builtin backend must not invoke MCP, got: {:?}",
        *recorded
    );
}

/// When the eggsearch backend is configured but unavailable, and
/// `fallback_to_builtin = true`, dispatch should fall back to the
/// legacy implementation rather than returning the eggsearch error.
///
/// The fallback path uses the in-tree `SearchProviderRegistry`,
/// which (in this test environment) has no providers configured.
/// The dispatch will likely error, but the assertion is that it
/// does NOT error with the "eggsearch backend is configured but
/// unavailable" message.
#[tokio::test]
async fn fallback_to_builtin_avoids_eggsearch_unavailable_error() {
    state::reset_for_tests();
    let (_cp, _g) = lock().await;
    let calls = Arc::new(Mutex::new(Vec::<(String, serde_json::Value)>::new()));
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
        Box::new(move |tool, args| {
            if let Ok(mut g) = recorded.try_lock() {
                g.push((tool.to_string(), args.clone()));
            }
            Err(McpError::Server("intentional eggsearch failure".into()))
        }),
    );
    let svc = Arc::new(tokio::sync::RwLock::new(svc));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config_with_fallback());

    let res = codegg::search_backend::dispatch_web_search(&serde_json::json!({"query": "x"})).await;
    // Either Ok(legacy output) or Err(legacy error) is acceptable.
    // The forbidden outcome is the "eggsearch backend is configured
    // but unavailable" message.
    if let Err(e) = &res {
        let msg = e.to_string();
        assert!(
            !msg.contains("eggsearch backend is configured but unavailable"),
            "fallback_to_builtin=true should not surface the eggsearch-unavailable error, got: {msg}"
        );
    }
}

/// When the eggsearch backend is configured but unavailable, and
/// `fallback_to_builtin = false` (the default), dispatch should
/// return the clear eggsearch-unavailable error.
#[tokio::test]
async fn no_fallback_surfaces_eggsearch_error() {
    state::reset_for_tests();
    let (_cp, _g) = lock().await;
    let svc = McpService::new();
    // No eggsearch server registered -> the service is installed
    // but call_tool for "eggsearch" will fail with "server not found".
    let svc = Arc::new(tokio::sync::RwLock::new(svc));
    state::install_mcp_service(svc);
    state::install_search_config(SearchConfig {
        backend: Some(SearchBackendConfig::Eggsearch),
        fallback_to_builtin: Some(false),
        eggsearch: Some(EggsearchConfig::default()),
        ..Default::default()
    });

    let res = codegg::search_backend::dispatch_web_search(&serde_json::json!({"query": "x"})).await;
    // The exact error depends on the install path, but it MUST
    // mention eggsearch so the user knows what is going wrong.
    let err = res.expect_err("should error when eggsearch is unavailable");
    let msg = err.to_string();
    assert!(
        msg.contains("eggsearch"),
        "expected eggsearch mention in error, got: {msg}"
    );
}

/// The legacy adapter should produce a well-formed "Search results
/// for '...'" header. This is a unit test of the legacy formatter
/// and does not require the network.
#[test]
fn legacy_format_hits_includes_header_and_url() {
    state::reset_for_tests();
    use codegg::search::SearchHit;
    use codegg::search_backend::legacy::format_hits;

    let hits = vec![
        SearchHit {
            title: "T1".to_string(),
            url: "https://x.example".to_string(),
            snippet: "S1".to_string(),
            source: "duckduckgo".to_string(),
        },
        SearchHit {
            title: "T2".to_string(),
            url: "https://y.example".to_string(),
            snippet: "S2".to_string(),
            source: "wikipedia".to_string(),
        },
    ];
    let out = format_hits("hello", &hits);
    assert!(out.contains("Search results for 'hello'"));
    assert!(out.contains("https://x.example"));
    assert!(out.contains("https://y.example"));
    assert!(out.contains("duckduckgo"));
    assert!(out.contains("wikipedia"));
}
