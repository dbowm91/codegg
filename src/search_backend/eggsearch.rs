//! Eggsearch backend adapter.
//!
//! Translates Codegg's stable `websearch` / `webfetch` argument shapes
//! into the eggsearch MCP tool argument shapes, calls into the live
//! `McpService`, and applies output framing/capping. The actual schema
//! difference between Codegg's native tools and eggsearch's MCP tools
//! is hidden from the model.

use serde_json::{json, Value};

use crate::error::ToolError;

use super::framing::{
    clamp_output, frame_batch_results, frame_evidence_bundle, frame_fetched_page, frame_repo_file,
    frame_repo_map, frame_repo_results, frame_research_results, frame_search_results,
    frame_security_results,
};

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
    timeout_ms: u64,
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
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "web_search", args).await
    })
    .await
    .map_err(|_| ToolError::Timeout(format!("eggsearch web_search timed out after {timeout_ms}ms")))?
    .map_err(|e| ToolError::Execution(format!("eggsearch web_search: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_search_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_search_results(&capped, "eggsearch"))
}

/// Translate a native `webfetch` call into an eggsearch `web_fetch`
/// call and execute it.
pub async fn call_web_fetch(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let url = input
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Execution("missing 'url' parameter".to_string()))?;
    validate_fetch_url(url)?;

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
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "web_fetch", args).await
    })
    .await
    .map_err(|_| ToolError::Timeout(format!("eggsearch web_fetch timed out after {timeout_ms}ms")))?
    .map_err(|e| ToolError::Execution(format!("eggsearch web_fetch: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_fetch_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_fetched_page(&capped, "eggsearch"))
}

/// Translate a native `repo_search` call into an eggsearch `repo_search`
/// call and execute it.
pub async fn call_repo_search(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
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

    let mut args = json!({
        "query": query,
    });

    if let Some(repo) = input.get("repo").and_then(Value::as_str) {
        args["repo"] = Value::String(repo.to_string());
    }
    if let Some(language) = input.get("language").and_then(Value::as_str) {
        args["language"] = Value::String(language.to_string());
    }

    let max_results = input
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .min(30) as usize;
    args["max_results"] = json!(max_results);

    if let Some(include_snippets) = input.get("include_snippets").and_then(Value::as_bool) {
        args["include_snippets"] = json!(include_snippets);
    }

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "repo_search", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!("eggsearch repo_search timed out after {timeout_ms}ms"))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch repo_search: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_repo_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_repo_results(&capped, "eggsearch"))
}

/// Translate a native `repo_fetch` call into an eggsearch `repo_fetch`
/// call and execute it.
pub async fn call_repo_fetch(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Execution("missing 'path' parameter".to_string()))?;

    let mut args = json!({
        "path": path,
    });

    if let Some(repo) = input.get("repo").and_then(Value::as_str) {
        args["repo"] = Value::String(repo.to_string());
    }
    if let Some(start_line) = input.get("start_line").and_then(Value::as_i64) {
        args["start_line"] = json!(start_line);
    }
    if let Some(end_line) = input.get("end_line").and_then(Value::as_i64) {
        args["end_line"] = json!(end_line);
    }
    if let Some(symbol) = input.get("symbol").and_then(Value::as_str) {
        args["symbol"] = Value::String(symbol.to_string());
    }

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "repo_fetch", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!("eggsearch repo_fetch timed out after {timeout_ms}ms"))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch repo_fetch: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_repo_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_repo_file(&capped, "eggsearch"))
}

/// Translate a native `repo_map` call into an eggsearch `repo_map`
/// call and execute it.
pub async fn call_repo_map(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let repo = input
        .get("repo")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Execution("missing 'repo' parameter".to_string()))?;

    let mut args = json!({
        "repo": repo,
    });

    if let Some(path) = input.get("path").and_then(Value::as_str) {
        args["path"] = Value::String(path.to_string());
    }
    if let Some(depth) = input.get("depth").and_then(Value::as_i64) {
        args["depth"] = json!(depth.min(3));
    }

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "repo_map", args).await
    })
    .await
    .map_err(|_| ToolError::Timeout(format!("eggsearch repo_map timed out after {timeout_ms}ms")))?
    .map_err(|e| ToolError::Execution(format!("eggsearch repo_map: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_repo_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_repo_map(&capped, "eggsearch"))
}

/// Translate a native `security_search` call into an eggsearch
/// `security_search` call and execute it.
pub async fn call_security_search(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
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

    let mut args = json!({
        "query": query,
    });

    if let Some(ecosystem) = input.get("ecosystem").and_then(Value::as_str) {
        args["ecosystem"] = Value::String(ecosystem.to_string());
    }
    if let Some(package) = input.get("package").and_then(Value::as_str) {
        args["package"] = Value::String(package.to_string());
    }
    if let Some(cve) = input.get("cve").and_then(Value::as_str) {
        args["cve"] = Value::String(cve.to_string());
    }

    let max_results = input
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .min(20) as usize;
    args["max_results"] = json!(max_results);

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "security_search", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch security_search timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch security_search: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_security_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_security_results(&capped, "eggsearch"))
}

/// Translate a native `research_search` call into an eggsearch
/// `research_search` call and execute it.
pub async fn call_research_search(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
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

    let mut args = json!({
        "query": query,
    });

    if let Some(domains) = input.get("domains").and_then(Value::as_array) {
        let domain_strs: Vec<Value> = domains
            .iter()
            .filter_map(|v| v.as_str().map(|s| Value::String(s.to_string())))
            .collect();
        if !domain_strs.is_empty() {
            args["domains"] = Value::Array(domain_strs);
        }
    }

    let max_results = input
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .min(15) as usize;
    args["max_results"] = json!(max_results);

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "research_search", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch research_search timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch research_search: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_research_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_research_results(&capped, "eggsearch"))
}

/// Translate a native `batch_fetch` call into an eggsearch
/// `batch_fetch` call and execute it.
pub async fn call_batch_fetch(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let mut args = json!({});

    if let Some(urls) = input.get("urls").and_then(Value::as_array) {
        for url_val in urls {
            if let Some(url_str) = url_val.as_str() {
                validate_fetch_url(url_str)?;
            }
        }
        args["urls"] = Value::Array(urls.clone());
    }
    if let Some(items) = input.get("items").and_then(Value::as_array) {
        for item in items {
            if let Some(url_str) = item.get("url").and_then(Value::as_str) {
                validate_fetch_url(url_str)?;
            }
        }
        args["items"] = Value::Array(items.clone());
    }

    let max_chars_per_item = input
        .get("max_chars_per_item")
        .and_then(Value::as_u64)
        .unwrap_or(10_000)
        .min(50_000) as usize;
    args["max_chars_per_item"] = json!(max_chars_per_item);

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "batch_fetch", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!("eggsearch batch_fetch timed out after {timeout_ms}ms"))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch batch_fetch: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_batch_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_batch_results(&capped, "eggsearch"))
}

/// Translate a native `build_evidence_bundle` call into an eggsearch
/// `build_evidence_bundle` call and execute it.
pub async fn call_build_evidence_bundle(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let sources = input
        .get("sources")
        .and_then(Value::as_array)
        .ok_or_else(|| ToolError::Execution("missing 'sources' parameter".to_string()))?;
    if sources.is_empty() {
        return Err(ToolError::Execution(
            "'sources' must not be empty".to_string(),
        ));
    }

    let mut args = json!({
        "sources": sources,
    });

    let max_total_chars = input
        .get("max_total_chars")
        .and_then(Value::as_u64)
        .unwrap_or(50_000)
        .min(100_000) as usize;
    args["max_total_chars"] = json!(max_total_chars);

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard
            .call_tool(mcp_server, "build_evidence_bundle", args)
            .await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch build_evidence_bundle timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch build_evidence_bundle: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_evidence_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_evidence_bundle(&capped, "eggsearch"))
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

/// Validate a URL before forwarding it to eggsearch for fetching.
/// Rejects empty URLs, non-HTTP(S) schemes, and overlong URLs (>2048 bytes).
fn validate_fetch_url(url: &str) -> Result<(), ToolError> {
    if url.is_empty() {
        return Err(ToolError::Execution(
            "fetch URL must not be empty".to_string(),
        ));
    }
    if url.len() > 2048 {
        return Err(ToolError::Execution(format!(
            "fetch URL is too long ({} bytes, max 2048)",
            url.len()
        )));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ToolError::Execution(format!(
            "fetch URL must use http or https scheme, got: {}",
            &url[..url.len().min(64)]
        )));
    }
    Ok(())
}

/// Produce an actionable error when eggsearch is connected but a
/// specific MCP tool is not advertised by the server.
pub fn eggsearch_tool_missing(
    server_name: &str,
    codegg_tool: &str,
    upstream_tool: &str,
    discovered_tools: &[String],
) -> ToolError {
    let tool_list = if discovered_tools.is_empty() {
        "(none discovered)".to_string()
    } else {
        discovered_tools.join(", ")
    };
    ToolError::Execution(format!(
        "eggsearch backend is connected but tool {upstream_tool} is not advertised \
         by server {server_name}. Discovered tools: {tool_list}. \
         Requested by Codegg wrapper: {codegg_tool}. \
         Upgrade eggsearch or disable Codegg {codegg_tool}."
    ))
}

/// Check that a specific upstream MCP tool is available on the
/// server. Returns `Ok(())` if the tool is present, or a descriptive
/// error if it is missing.
pub fn ensure_tool_available(
    mcp_server: &str,
    codegg_tool: &str,
    upstream_tool: &str,
) -> Result<(), ToolError> {
    let svc = match super::state::mcp_service() {
        Some(s) => s,
        None => return Ok(()), // will be caught by the call itself
    };
    // Read the tool list synchronously (RwLock read is cheap).
    let guard = svc.try_read().map_err(|_| {
        ToolError::Execution(format!(
            "eggsearch: could not check tool availability for {upstream_tool}"
        ))
    })?;
    let tools = guard.server_tools();
    let discovered: Vec<String> = tools
        .get(mcp_server)
        .map(|t| t.iter().map(|x| x.name.clone()).collect())
        .unwrap_or_default();
    drop(guard);
    if discovered.iter().any(|t| t == upstream_tool) {
        Ok(())
    } else {
        Err(eggsearch_tool_missing(
            mcp_server,
            codegg_tool,
            upstream_tool,
            &discovered,
        ))
    }
}

/// Best-effort provider_status query, used by the doctor command.
pub async fn call_provider_status(
    mcp_server: &str,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard
            .call_tool(mcp_server, "provider_status", json!({}))
            .await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!("eggsearch provider_status timed out after {timeout_ms}ms"))
    })?
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
        let _cp = crate::search_backend::test_support::acquire_cross_process_lock();
        let _g = crate::search_backend::test_support::SHARED_TEST_LOCK
            .lock()
            .await;
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
