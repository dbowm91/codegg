//! Legacy in-tree web search adapter.
//!
//! This adapter retains the original `websearch` and `webfetch`
//! implementations that were shipped before the eggsearch migration.
//! It is invoked when `[search].backend = "builtin"` or as an explicit
//! fallback when the eggsearch backend is unavailable and
//! `fallback_to_builtin = true`.
//!
//! Long-term, the providers in `crate::search` should not grow
//! further; new providers should be added in eggsearch.

use std::time::Duration;

use serde_json::Value;

use crate::error::ToolError;
use crate::search::{SearchHit, SearchProviderRegistry};

/// Build the legacy registry. The same logic the old `WebSearchTool`
/// used: pull every provider keyed off env vars.
pub fn legacy_registry() -> std::sync::Arc<SearchProviderRegistry> {
    std::sync::Arc::new(SearchProviderRegistry::from_env())
}

pub async fn call_web_search_legacy(
    registry: std::sync::Arc<SearchProviderRegistry>,
    input: &Value,
    max_output_chars: usize,
    timeout_secs: u64,
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
    let provider_hint = input.get("provider").and_then(Value::as_str);

    if !registry.has_any() {
        return Err(ToolError::Execution(
            "no websearch provider configured (set EXA_API_KEY, TAVILY_API_KEY, \
             BRAVE_API_KEY, KAGI_API_KEY, or SERPAPI_API_KEY; or rely on the \
             no-key DuckDuckGo/Mojeek fallbacks)"
                .into(),
        ));
    }

    let result = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        registry.search(query, num_results, provider_hint),
    )
    .await;
    let hits = match result {
        Err(_) => {
            return Err(ToolError::Timeout(format!(
                "websearch timed out after {timeout_secs}s"
            )));
        }
        Ok(Err(e)) => return Err(ToolError::Execution(format!("websearch: {e}"))),
        Ok(Ok(hits)) if hits.is_empty() => {
            return Err(ToolError::Execution(format!(
                "no results found for '{query}'"
            )));
        }
        Ok(Ok(hits)) => hits,
    };

    let raw = format_hits(query, &hits);
    let capped = if raw.len() > max_output_chars {
        let mut s = String::with_capacity(max_output_chars + 64);
        s.push_str(&raw[..max_output_chars]);
        s.push_str(&format!(
            "\n\n[truncated by Codegg: output exceeded max_search_output_chars={max_output_chars}]"
        ));
        s
    } else {
        raw
    };
    Ok(capped)
}

pub fn format_hits(query: &str, hits: &[SearchHit]) -> String {
    let mut out = format!("Search results for '{query}':\n\n");
    for (i, hit) in hits.iter().enumerate() {
        out.push_str(&format!(
            "{}. {}\n   URL: {}\n   Source: {}\n   {}\n\n",
            i + 1,
            hit.title,
            hit.url,
            hit.source,
            hit.snippet
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_hits_includes_query_and_url() {
        let hits = vec![SearchHit {
            title: "T".to_string(),
            url: "https://x".to_string(),
            snippet: "S".to_string(),
            source: "duckduckgo".to_string(),
        }];
        let out = format_hits("q", &hits);
        assert!(out.contains("Search results for 'q'"));
        assert!(out.contains("https://x"));
    }

    #[tokio::test]
    async fn legacy_search_returns_error_when_no_providers_configured() {
        // The legacy adapter uses an env-driven registry. We can't
        // easily mock it, but we can assert the no-provider error path
        // when the runtime is clean (the test harness doesn't set
        // any API keys).
        let registry = std::sync::Arc::new(SearchProviderRegistry::from_env());
        if registry.has_any() {
            // Skip: external env has a provider configured.
            return;
        }
        let res =
            call_web_search_legacy(registry, &serde_json::json!({"query": "q"}), 1000, 1).await;
        let err = res.expect_err("should fail when no providers configured");
        let msg = err.to_string();
        assert!(
            msg.contains("no websearch provider configured"),
            "unexpected error: {msg}"
        );
    }
}
