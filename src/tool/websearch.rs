use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::search_backend;
use crate::tool::{Tool, ToolCategory};

/// Native `websearch` tool.
///
/// Model-facing name is `websearch`. Internally dispatches to the
/// configured search backend (eggsearch by default, in-tree
/// built-in as fallback).
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
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "websearch"
    }

    fn description(&self) -> &str {
        "Search the web using the configured search backend (eggsearch by default). \
         Returns compact source cards with titles, URLs, snippets, providers, and trust \
         labels. Use this for source discovery; use webfetch only for explicit URLs worth \
         reading. Search results are external_untrusted."
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
        search_backend::dispatch_web_search(&input).await
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
    fn parameters_require_query() {
        let t = WebSearchTool::new();
        let p = t.parameters();
        let required = p.get("required").and_then(|v| v.as_array()).unwrap();
        assert!(required.iter().any(|v| v == "query"));
    }
}
