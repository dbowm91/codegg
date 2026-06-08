//! Eggsearch backend adapter.
//!
//! Translates Codegg's stable `websearch` / `webfetch` argument shapes
//! into the eggsearch MCP tool argument shapes, calls into the live
//! `McpService`, and applies output framing/capping. The actual schema
//! difference between Codegg's native tools and eggsearch's MCP tools
//! is hidden from the model.

use serde_json::{json, Value};

use crate::error::ToolError;

use super::framing::{clamp_output, frame_fetched_page, frame_search_results};

/// Translate a native `websearch` call into an eggsearch `web_search`
/// call and execute it.
///
/// Eggsearch argument shape (subject to upstream change):
///
/// ```json
/// {
///   "query": "...",
///   "max_results": 8,
///   "providers": ["duckduckgo", "mojeek"],
///   "timeout_ms": null
/// }
/// ```
pub async fn call_web_search(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
) -> Result<String, ToolError> {
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .ok_or_else(|| ToolError::Execution("missing 'query' parameter".to_string()))?;
    if query.is_empty() {
        return Err(ToolError::Execution(
            "'query' must not be empty".to_string(),
        ));
    }

    let num_results = input
        .get("num_results")
        .or_else(|| input.get("max_results"))
        .and_then(Value::as_u64)
        .unwrap_or(8)
        .min(30) as usize;

    let mut args = json!({
        "query": query,
        "max_results": num_results,
    });

    if let Some(providers) = translate_provider_hint(input.get("provider").and_then(Value::as_str))
    {
        if !providers.is_empty() {
            args["providers"] = Value::Array(providers.into_iter().map(Value::String).collect());
        }
    }

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "web_search", args).await
    })
    .await
    .map_err(|_| ToolError::Timeout("eggsearch web_search timed out after 60s".to_string()))?
    .map_err(|e| ToolError::Execution(format!("eggsearch web_search: {e}")))?;

    let capped = clamp_output(&raw, max_output_chars, "max_search_output_chars");
    Ok(frame_search_results(&capped))
}

/// Translate a native `webfetch` call into an eggsearch `web_fetch`
/// call and execute it.
pub async fn call_web_fetch(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
) -> Result<String, ToolError> {
    let url = input
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Execution("missing 'url' parameter".to_string()))?;

    let max_chars = input
        .get("max_length")
        .or_else(|| input.get("max_chars"))
        .and_then(Value::as_u64)
        .unwrap_or(10_000) as usize;

    let args = json!({
        "url": url,
        "max_chars": max_chars,
        "extract_mode": "text",
        "include_links": false,
    });

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "web_fetch", args).await
    })
    .await
    .map_err(|_| ToolError::Timeout("eggsearch web_fetch timed out after 60s".to_string()))?
    .map_err(|e| ToolError::Execution(format!("eggsearch web_fetch: {e}")))?;

    let capped = clamp_output(&raw, max_output_chars, "max_fetch_output_chars");
    Ok(frame_fetched_page(&capped))
}

/// Best-effort translation of the historical Codegg `provider` hint to
/// eggsearch's `providers` list. Returns `Some(vec)` when the user
/// pinned a specific provider, `Some(vec![])` (i.e. "let eggsearch
/// auto-pick") for `auto`/missing or any hint that eggsearch does not
/// recognize, or `None` to mean "omit the field entirely".
///
/// Note: The model-facing enum advertises a long list of historical
/// provider hints, but we do not have ground truth on which of them
/// eggsearch supports today. Hints that eggsearch does not recognize
/// are intentionally mapped to an empty list (auto-pick) so that the
/// search still succeeds with a sensible default provider.
pub(crate) fn translate_provider_hint(hint: Option<&str>) -> Option<Vec<String>> {
    let h = hint.unwrap_or("auto");
    match h {
        "auto" => Some(Vec::new()),
        "duckduckgo" => Some(vec!["duckduckgo".to_string()]),
        "mojeek" => Some(vec!["mojeek".to_string()]),
        "wikipedia" => Some(vec!["wikipedia".to_string()]),
        "arxiv" => Some(vec!["arxiv".to_string()]),
        "openalex" => Some(vec!["openalex".to_string()]),
        "pubmed" => Some(vec!["pubmed".to_string()]),
        "hn_algolia" => Some(vec!["hn_algolia".to_string()]),
        "google_news" => Some(vec!["google_news".to_string()]),
        "github" => Some(vec!["github".to_string()]),
        "exa" => Some(vec!["exa".to_string()]),
        "tavily" => Some(vec!["tavily".to_string()]),
        "brave" | "brave_api" => Some(vec!["brave_api".to_string()]),
        "kagi" => Some(vec!["kagi".to_string()]),
        "serpapi" => Some(vec!["serpapi".to_string()]),
        // Unknown hint: let eggsearch decide.
        _ => Some(Vec::new()),
    }
}

pub fn eggsearch_unavailable(detail: &str) -> ToolError {
    ToolError::Execution(format!(
        "eggsearch backend is configured but unavailable: {detail}. \
         Install eggsearch or set [search].backend = \"builtin\" / \"disabled\"."
    ))
}

/// Best-effort provider_status query, used by the doctor command.
pub async fn call_provider_status(mcp_server: &str) -> Result<String, ToolError> {
    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let guard = svc.read().await;
        guard
            .call_tool(mcp_server, "provider_status", json!({}))
            .await
    })
    .await
    .map_err(|_| ToolError::Timeout("eggsearch provider_status timed out".to_string()))?
    .map_err(|e| ToolError::Execution(format!("eggsearch provider_status: {e}")))?;
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_hint_auto_returns_empty() {
        let v = translate_provider_hint(Some("auto"));
        assert_eq!(v, Some(vec![]));
    }

    #[test]
    fn provider_hint_duckduckgo_passes_through() {
        assert_eq!(
            translate_provider_hint(Some("duckduckgo")),
            Some(vec!["duckduckgo".to_string()])
        );
    }

    #[test]
    fn provider_hint_mojeek_passes_through() {
        assert_eq!(
            translate_provider_hint(Some("mojeek")),
            Some(vec!["mojeek".to_string()])
        );
    }

    #[test]
    fn provider_hint_brave_maps_to_brave_api() {
        let v = translate_provider_hint(Some("brave"));
        assert_eq!(v, Some(vec!["brave_api".to_string()]));
    }

    #[test]
    fn provider_hint_unsupported_returns_empty_for_auto_pick() {
        // The adapter intentionally treats unknown historical hints as "auto"
        // (empty list) so that eggsearch can pick a provider.
        assert_eq!(
            translate_provider_hint(Some("unsupported_historical")),
            Some(vec![])
        );
    }

    #[test]
    fn provider_hint_unknown_returns_empty() {
        let v = translate_provider_hint(Some("mystery"));
        assert_eq!(v, Some(vec![]));
    }

    /// Verify that when a `websearch` call dispatches to the
    /// eggsearch backend and eggsearch is unavailable, we return
    /// the documented actionable error (used to drive the
    /// "missing eggsearch" acceptance criterion).
    ///
    /// The test is intentionally permissive about the exact error
    /// text because the failure surface depends on whether a stale
    /// `McpService` from a previous test is still in the global
    /// state slot. We assert the *contract*: when the eggsearch
    /// backend is selected and the underlying service is not
    /// usable, the error must mention "eggsearch" so the user
    /// can debug.
    #[tokio::test]
    async fn web_search_unavailable_returns_actionable_error() {
        crate::search_backend::state::reset_for_tests();
        crate::search_backend::state::install_search_config(crate::config::schema::SearchConfig {
            backend: Some(crate::config::schema::SearchBackendConfig::Eggsearch),
            ..Default::default()
        });
        let res = super::super::dispatch_web_search(&serde_json::json!({"query": "test"})).await;
        let err = res.expect_err("should be unavailable");
        let msg = err.to_string();
        // Either: the documented actionable "eggsearch backend is
        // configured but unavailable" error, or a downstream
        // "server eggsearch not found" error from a stale
        // McpService. Both surface actionable information about
        // eggsearch.
        assert!(
            msg.contains("eggsearch backend is configured but unavailable")
                || msg.contains("server eggsearch not found")
                || msg.contains("McpService is not initialized"),
            "expected actionable eggsearch error, got: {msg}"
        );
    }
}
