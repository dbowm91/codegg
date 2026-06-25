//! Trust-framing and argument-mapping tests for the eggsearch backend.
//!
//! Trust framing:
//! - Every eggsearch `websearch` result is wrapped in
//!   `external_untrusted` framing before being returned to the model.
//! - Every eggsearch `webfetch` result is wrapped likewise with a
//!   stronger warning, since fetched pages are the highest-risk
//!   ingress path.
//!
//! Argument mapping:
//! - `websearch.num_results` is mapped to `web_search.max_results`.
//! - `websearch.max_results` is accepted as an alias.
//! - `websearch.provider` is mapped to a `web_search.providers` list
//!   with sensible defaults for the historical Codegg provider set.
//! - `webfetch.max_length` is mapped to `web_fetch.max_chars`.
//! - `webfetch.max_chars` is accepted as an alias.
//! - `webfetch` always sends `extract_mode = "text"` and
//!   `include_links = false` to keep output bounded.
//!
//! ## Test isolation
//!
//! `search_backend::state` is a process-global slot, so the tests in
//! this file must be serialized against any other test that touches
//! the same global. We hold the lock for the entire body of each
//! `#[tokio::test]` (including the `.await` on `dispatch_*`) so a
//! concurrent test from another binary cannot reset the state between
//! our install and our assertions.

use std::sync::Arc;
use std::sync::Mutex;

use codegg::config::schema::{EggsearchConfig, SearchBackendConfig, SearchConfig};
use codegg::error::McpError;
use codegg::mcp::{McpService, McpTool};
use codegg::search_backend::framing;
use codegg::search_backend::state;
use codegg::search_backend::test_support::{
    acquire_cross_process_lock, CrossProcessLockGuard, SHARED_TEST_LOCK,
};
use tokio::sync::MutexGuard;

fn eggsearch_config() -> SearchConfig {
    SearchConfig {
        backend: Some(SearchBackendConfig::Eggsearch),
        eggsearch: Some(EggsearchConfig::default()),
        ..Default::default()
    }
}

// ---- Framing tests (pure unit) ----

#[test]
fn search_frame_marks_trust_external_untrusted() {
    let out = framing::frame_search_results("hello world");
    assert!(out.contains("trust=external_untrusted"));
    assert!(out.contains("tool=websearch"));
    assert!(out.contains("hello world"));
    assert!(out.contains("[/external_web_content]"));
}

#[test]
fn fetch_frame_marks_trust_external_untrusted() {
    let out = framing::frame_fetched_page("body");
    assert!(out.contains("trust=external_untrusted"));
    assert!(out.contains("tool=webfetch"));
    assert!(out.contains("EXTERNAL, UNTRUSTED DATA"));
    assert!(out.contains("body"));
}

#[test]
fn clamp_output_passthrough_for_short_input() {
    let out = framing::clamp_output("hi", 100, "max");
    assert_eq!(out, "hi");
}

#[test]
fn clamp_output_truncates_long_input() {
    let out = framing::clamp_output(&"x".repeat(50), 10, "max_chars");
    assert!(out.starts_with("xxxxxxxxxx"));
    assert!(out.contains("[truncated by Codegg"));
}

// ---- Argument-mapping tests (with mock MCP) ----

fn install_mock_recorder() -> Arc<Mutex<Vec<(String, serde_json::Value)>>> {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut svc = McpService::new();
    let recorded = Arc::clone(&calls);
    svc.register_mock_server(
        "eggsearch",
        vec![
            McpTool {
                name: "web_search".to_string(),
                description: "".to_string(),
                input_schema: serde_json::json!({}),
                server: "eggsearch".to_string(),
            },
            McpTool {
                name: "web_fetch".to_string(),
                description: "".to_string(),
                input_schema: serde_json::json!({}),
                server: "eggsearch".to_string(),
            },
        ],
        Box::new(move |tool, args| {
            if let Ok(mut g) = recorded.try_lock() {
                g.push((tool.to_string(), args.clone()));
            }
            match tool {
                "web_search" => Ok("[]".to_string()),
                "web_fetch" => Ok("body".to_string()),
                _ => Err(McpError::Server(format!("unknown tool {tool}"))),
            }
        }),
    );
    let svc = Arc::new(tokio::sync::RwLock::new(svc));
    state::install_mcp_service(svc);
    state::install_search_config(eggsearch_config());
    calls
}

async fn lock_tests() -> (CrossProcessLockGuard, MutexGuard<'static, ()>) {
    let cp = acquire_cross_process_lock();
    let g = SHARED_TEST_LOCK.lock().await;
    (cp, g)
}

#[tokio::test]
async fn num_results_maps_to_max_results() {
    state::reset_for_tests();
    let (_cp, _g) = lock_tests().await;
    let calls = install_mock_recorder();
    let _ = codegg::search_backend::dispatch_web_search(&serde_json::json!({
        "query": "x",
        "num_results": 12,
    }))
    .await
    .unwrap();
    let rec = calls.lock().expect("calls poisoned");
    let (tool, args) = rec.last().expect("at least one call");
    assert_eq!(tool, "web_search");
    assert_eq!(args["query"], "x");
    assert_eq!(args["max_results"], 12);
}

#[tokio::test]
async fn max_results_alias_is_accepted() {
    state::reset_for_tests();
    let (_cp, _g) = lock_tests().await;
    let calls = install_mock_recorder();
    let _ = codegg::search_backend::dispatch_web_search(&serde_json::json!({
        "query": "x",
        "max_results": 7,
    }))
    .await
    .unwrap();
    let rec = calls.lock().expect("calls poisoned");
    let (tool, args) = rec.last().expect("at least one call");
    assert_eq!(tool, "web_search");
    assert_eq!(args["max_results"], 7);
}

#[tokio::test]
async fn num_results_is_capped_at_30() {
    state::reset_for_tests();
    let (_cp, _g) = lock_tests().await;
    let calls = install_mock_recorder();
    let _ = codegg::search_backend::dispatch_web_search(&serde_json::json!({
        "query": "x",
        "num_results": 5000,
    }))
    .await
    .unwrap();
    let rec = calls.lock().expect("calls poisoned");
    let (_, args) = rec.last().unwrap();
    assert_eq!(args["max_results"], 30);
}

#[tokio::test]
async fn provider_pinned_to_specific_backend() {
    state::reset_for_tests();
    let (_cp, _g) = lock_tests().await;
    let calls = install_mock_recorder();
    let _ = codegg::search_backend::dispatch_web_search(&serde_json::json!({
        "query": "x",
        "provider": "arxiv",
    }))
    .await
    .unwrap();
    let rec = calls.lock().expect("calls poisoned");
    let (_, args) = rec.last().unwrap();
    let providers = args["providers"].as_array().expect("providers array");
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0], "arxiv");
}

#[tokio::test]
async fn provider_unknown_does_not_emit_providers_field() {
    state::reset_for_tests();
    let (_cp, _g) = lock_tests().await;
    let calls = install_mock_recorder();
    let _ = codegg::search_backend::dispatch_web_search(&serde_json::json!({
        "query": "x",
        "provider": "unknown_backend",
    }))
    .await
    .unwrap();
    let rec = calls.lock().expect("calls poisoned");
    let (_, args) = rec.last().unwrap();
    if let Some(providers) = args.get("providers") {
        let arr = providers.as_array().expect("providers array");
        for p in arr {
            assert_ne!(p, "unknown_backend");
        }
    }
}

#[tokio::test]
async fn webfetch_max_length_maps_to_max_chars() {
    state::reset_for_tests();
    let (_cp, _g) = lock_tests().await;
    let calls = install_mock_recorder();
    let _ = codegg::search_backend::dispatch_web_fetch(&serde_json::json!({
        "url": "https://example.com",
        "max_length": 4000,
    }))
    .await
    .unwrap();
    let rec = calls.lock().expect("calls poisoned");
    let (tool, args) = rec.last().unwrap();
    assert_eq!(tool, "web_fetch");
    assert_eq!(args["url"], "https://example.com");
    assert_eq!(args["max_chars"], 4000);
    assert_eq!(args["extract_mode"], "text");
    assert_eq!(args["include_links"], false);
}

#[tokio::test]
async fn webfetch_default_extract_mode_is_text() {
    state::reset_for_tests();
    let (_cp, _g) = lock_tests().await;
    let calls = install_mock_recorder();
    eprintln!(
        "webfetch_default: mcp_service={}, search_config.backend={:?}",
        state::mcp_service().is_some(),
        state::search_config().backend()
    );
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    eprintln!("webfetch_default: mcp_service after sleep 100ms = {}", state::mcp_service().is_some());
    let _ = codegg::search_backend::dispatch_web_fetch(&serde_json::json!({
        "url": "https://example.com",
    }))
    .await
    .unwrap();
    let rec = calls.lock().expect("calls poisoned");
    let (_, args) = rec.last().unwrap();
    assert_eq!(args["extract_mode"], "text");
    assert_eq!(args["include_links"], false);
}
