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
//! ## Test isolation (M005)
//!
//! Each test builds its own explicit `SearchRuntimeContext` holding an
//! owned config snapshot plus a fresh mock `McpService`. No
//! process-global install/reset and no cross-test serialization locks
//! are needed; contexts coexist without cross-talk.

use std::sync::Arc;
use std::sync::Mutex;

use codegg::config::schema::{EggsearchConfig, SearchBackendConfig, SearchConfig};
use codegg::error::McpError;
use codegg::mcp::{McpService, McpTool};
use codegg::search_backend::framing;
use codegg::search_backend::SearchRuntimeContext;
use codegg::tool::webfetch::WebFetchTool;
use codegg::tool::websearch::WebSearchTool;
use codegg::tool::Tool;

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
    let out = framing::frame_search_results("hello world", "eggsearch");
    assert!(out.contains("trust=external_untrusted"));
    assert!(out.contains("tool=websearch"));
    assert!(out.contains("hello world"));
    assert!(out.contains("[/external_web_content]"));
}

#[test]
fn fetch_frame_marks_trust_external_untrusted() {
    let out = framing::frame_fetched_page("body", "eggsearch");
    assert!(out.contains("trust=external_untrusted"));
    assert!(out.contains("tool=webfetch"));
    assert!(out.contains("EXTERNAL, UNTRUSTED DATA"));
    assert!(out.contains("body"));
}

#[test]
fn clamp_output_passthrough_for_short_input() {
    let (out, truncated) = framing::clamp_output("hi", 100, "max");
    assert_eq!(out, "hi");
    assert!(!truncated);
}

#[test]
fn clamp_output_truncates_long_input() {
    let (out, truncated) = framing::clamp_output(&"x".repeat(50), 10, "max_chars");
    assert!(out.starts_with("xxxxxxxxxx"));
    assert!(out.contains("[truncated by Codegg"));
    assert!(truncated);
}

// ---- Argument-mapping tests (with mock MCP) ----

type RecordedCalls = Arc<Mutex<Vec<(String, serde_json::Value)>>>;

#[allow(clippy::type_complexity)]
fn mock_context() -> (SearchRuntimeContext, RecordedCalls) {
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
                ..Default::default()
            },
            McpTool {
                name: "web_fetch".to_string(),
                description: "".to_string(),
                input_schema: serde_json::json!({}),
                server: "eggsearch".to_string(),
                ..Default::default()
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
    let ctx = SearchRuntimeContext::new(eggsearch_config()).with_mcp(svc);
    (ctx, calls)
}

#[tokio::test]
async fn num_results_maps_to_max_results() {
    let (ctx, calls) = mock_context();
    let _ = ctx
        .dispatch_web_search(&serde_json::json!({
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
    let (ctx, calls) = mock_context();
    let _ = ctx
        .dispatch_web_search(&serde_json::json!({
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
    let (ctx, calls) = mock_context();
    let _ = ctx
        .dispatch_web_search(&serde_json::json!({
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
    let (ctx, calls) = mock_context();
    let _ = ctx
        .dispatch_web_search(&serde_json::json!({
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
    let (ctx, calls) = mock_context();
    let _ = ctx
        .dispatch_web_search(&serde_json::json!({
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
    let (ctx, calls) = mock_context();
    let _ = ctx
        .dispatch_web_fetch(&serde_json::json!({
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
    let (ctx, calls) = mock_context();
    let _ = ctx
        .dispatch_web_fetch(&serde_json::json!({
            "url": "https://example.com",
        }))
        .await
        .unwrap();
    let rec = calls.lock().expect("calls poisoned");
    let (_, args) = rec.last().unwrap();
    assert_eq!(args["extract_mode"], "text");
    assert_eq!(args["include_links"], false);
}

#[tokio::test]
async fn excerpt_count_and_internal_response_detail_reach_upstream() {
    let (ctx, calls) = mock_context();
    let _ = ctx
        .dispatch_web_search(&serde_json::json!({
            "query": "x",
            "excerpt_count": 0,
        }))
        .await
        .unwrap();
    let _ = ctx
        .dispatch_web_search(&serde_json::json!({
            "query": "x",
            "excerpt_count": 3,
        }))
        .await
        .unwrap();
    let rec = calls.lock().expect("calls not poisoned");
    assert_eq!(rec[0].1["excerpt_count"], 0);
    assert_eq!(rec[1].1["excerpt_count"], 3);
    assert_eq!(rec[0].1["response_detail"], "diagnostic");
    assert_eq!(rec[1].1["response_detail"], "diagnostic");
}

#[tokio::test]
async fn fetch_focus_and_cache_controls_reach_upstream() {
    let (ctx, calls) = mock_context();
    let _ = ctx
        .dispatch_web_fetch(&serde_json::json!({
            "url": "https://example.com",
            "focus": "rust scheduler",
            "focus_max_chunks": 2,
            "focus_max_chars": 800,
            "cache_policy": "refresh",
            "max_cache_age_seconds": 3600,
        }))
        .await
        .unwrap();
    let rec = calls.lock().expect("calls not poisoned");
    let args = &rec.last().expect("at least one call").1;
    assert_eq!(args["focus"], "rust scheduler");
    assert_eq!(args["focus_max_chunks"], 2);
    assert_eq!(args["focus_max_chars"], 800);
    assert_eq!(args["cache_policy"], "refresh");
    assert_eq!(args["max_cache_age_seconds"], 3600);
    assert_eq!(args["response_detail"], "diagnostic");
}

#[tokio::test]
async fn new_fetch_controls_reject_out_of_contract_values_before_mcp() {
    let (ctx, calls) = mock_context();
    for input in [
        serde_json::json!({"query": "x", "excerpt_count": 4}),
        serde_json::json!({"url": "https://example.com", "focus": "   "}),
        serde_json::json!({"url": "https://example.com", "cache_policy": "stale"}),
        serde_json::json!({"url": "https://example.com", "max_cache_age_seconds": 2_592_001}),
        serde_json::json!({
            "url": "https://example.com",
            "cache_policy": "bypass",
            "max_cache_age_seconds": 0,
        }),
    ] {
        let result = if input.get("query").is_some() {
            ctx.dispatch_web_search(&input).await
        } else {
            ctx.dispatch_web_fetch(&input).await
        };
        assert!(
            result.is_err(),
            "invalid input unexpectedly succeeded: {input}"
        );
    }
    assert!(calls.lock().expect("calls not poisoned").is_empty());
}

#[test]
fn native_schemas_expose_only_the_selected_provider_neutral_controls() {
    let search = WebSearchTool::default().parameters();
    assert_eq!(search["properties"]["excerpt_count"]["minimum"], 0);
    assert_eq!(search["properties"]["excerpt_count"]["maximum"], 3);
    assert!(search["properties"].get("response_detail").is_none());

    let fetch = WebFetchTool::default().parameters();
    for field in [
        "focus",
        "focus_max_chunks",
        "focus_max_chars",
        "cache_policy",
        "max_cache_age_seconds",
    ] {
        assert!(fetch["properties"].get(field).is_some(), "missing {field}");
    }
    assert!(fetch["properties"].get("response_detail").is_none());
}
