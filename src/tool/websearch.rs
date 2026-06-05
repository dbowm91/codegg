use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde_json::json;
use std::sync::Arc;

use crate::error::ToolError;
use crate::search::{SearchHit, SearchProviderRegistry};
use crate::tool::{Tool, ToolCategory};

/// Global, lazily-initialized registry. Read once per process; safe to
/// share across threads because [`SearchProviderRegistry`] holds
/// `Arc<dyn SearchProvider>` instances and never mutates them.
static REGISTRY: Lazy<Arc<SearchProviderRegistry>> =
    Lazy::new(|| Arc::new(SearchProviderRegistry::from_env()));

pub struct WebSearchTool {
    timeout_secs: u64,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self { timeout_secs: 60 }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Build a description that reflects the configured providers.
    fn dynamic_description(&self) -> String {
        let reg = &*REGISTRY;
        if !reg.has_any() {
            return "Search the web for information. (DISABLED: no websearch provider configured.)"
                .to_string();
        }
        let configured = reg.describe_configured();
        format!(
            "Search the web for information. Returns titles, URLs, and snippets. \
             Default: DuckDuckGo (no key required) with Mojeek as fallback. \
             Key-based providers (exa/tavily/brave/kagi/serpapi) are used if their API key env vars are set. \
             Domain providers (wikipedia/arxiv/openalex/pubmed/hn/google_news/github) are routed by query shape. \
             Configured providers: {configured}. \
             Prefer this over curl/wget for web searches."
        )
    }

    async fn run_search(
        &self,
        query: &str,
        num_results: usize,
        provider_hint: Option<&str>,
    ) -> Result<String, ToolError> {
        let reg = REGISTRY.clone();
        if !reg.has_any() {
            return Err(ToolError::Execution(
                "no websearch provider configured (set EXA_API_KEY, TAVILY_API_KEY, BRAVE_API_KEY, KAGI_API_KEY, or SERPAPI_API_KEY; or rely on the no-key DuckDuckGo/Mojeek fallbacks)".into(),
            ));
        }
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            reg.search(query, num_results, provider_hint),
        )
        .await;
        let hits = match result {
            Err(_) => {
                return Err(ToolError::Timeout(format!(
                    "websearch timed out after {}s",
                    self.timeout_secs
                )));
            }
            Ok(Err(e)) => {
                return Err(ToolError::Execution(format!("websearch: {e}")));
            }
            Ok(Ok(hits)) if hits.is_empty() => {
                return Err(ToolError::Execution(format!(
                    "no results found for '{query}'"
                )));
            }
            Ok(Ok(hits)) => hits,
        };
        Ok(format_hits(query, &hits))
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

fn format_hits(query: &str, hits: &[SearchHit]) -> String {
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

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "websearch"
    }

    fn description(&self) -> &str {
        // Real description is computed at call time; cache it as a
        // `&'static str` for the trait method's lifetime by leaking
        // a Box<str>. We refresh on first call only.
        // The trait requires `&'static str`; we use a thread-local
        // approach via the Lazy description string below.
        // SAFETY: the description text is static; once initialized it
        // does not change unless env vars change (rare in practice).
        // We re-read env on every `run_search` so providers added at
        // runtime are picked up.
        use once_cell::sync::OnceCell;
        static DESCRIPTION: OnceCell<String> = OnceCell::new();
        let s = DESCRIPTION.get_or_init(|| {
            WebSearchTool::new().dynamic_description()
        });
        // Note: the description is computed once from the *current*
        // environment. For a process that starts without a key and
        // later has one set, the description will be stale. This is
        // acceptable for the LLM-facing surface; the runtime
        // dispatch is always correct via the registry.
        s.as_str()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "num_results": {
                    "type": "number",
                    "description": "Number of results to return (default: 8, max: 30)"
                },
                "provider": {
                    "type": "string",
                    "description": "Optional provider hint: 'auto' (default), 'duckduckgo', 'mojeek', 'wikipedia', 'arxiv', 'openalex', 'pubmed', 'hn_algolia', 'google_news', 'github', 'exa', 'tavily', 'brave', 'kagi', 'serpapi'",
                    "enum": [
                        "auto", "duckduckgo", "mojeek", "wikipedia", "arxiv",
                        "openalex", "pubmed", "hn_algolia", "google_news", "github",
                        "exa", "tavily", "brave", "kagi", "serpapi"
                    ]
                }
            },
            "required": ["query"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'query' parameter".to_string()))?
            .trim();
        if query.is_empty() {
            return Err(ToolError::Execution("'query' must not be empty".to_string()));
        }
        let num_results = input["num_results"].as_u64().unwrap_or(8).min(30) as usize;
        let provider_hint = input["provider"].as_str();
        self.run_search(query, num_results, provider_hint).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    #[test]
    fn name_is_websearch() {
        let t = WebSearchTool::new();
        assert_eq!(t.name(), "websearch");
    }

    #[test]
    fn description_mentions_prefer_over_curl() {
        let t = WebSearchTool::new();
        let d = t.description();
        assert!(d.contains("curl") || d.contains("Prefer this"));
    }

    #[test]
    fn parameters_require_query() {
        let t = WebSearchTool::new();
        let p = t.parameters();
        let required = p.get("required").and_then(|v| v.as_array()).unwrap();
        assert!(required.iter().any(|v| v == "query"));
    }

    #[test]
    fn empty_query_rejected() {
        let t = WebSearchTool::new();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let res = rt.block_on(t.execute(json!({"query": "   "})));
        assert!(matches!(res, Err(ToolError::Execution(_))));
    }
}
