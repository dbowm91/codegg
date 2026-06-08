use async_trait::async_trait;
use serde_json::json;
use std::time::Instant;

use crate::error::ToolError;
use crate::search_backend;
use crate::tool::{
    StructuredToolResult, Tool, ToolBackendKind, ToolCategory, ToolExecutionContext,
    ToolProvenance, ToolTrust,
};

/// Native `websearch` tool.
///
/// Model-facing name is `websearch`. Internally dispatches to the
/// configured search backend (eggsearch by default, in-tree
/// built-in as fallback).
#[derive(Default)]
pub struct WebSearchTool {}

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
                    "description": "Provider hint. Unknown values fall back to eggsearch's automatic provider selection.",
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

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let start = Instant::now();
        let output = search_backend::dispatch_web_search(&input).await?;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let provenance =
            search_backend::provenance_for_search().unwrap_or_else(|| ToolProvenance {
                backend: ToolBackendKind::BuiltinLegacy.label().to_lowercase(),
                implementation: "websearch".to_string(),
                version: None,
                elapsed_ms: Some(elapsed_ms),
                truncated: false,
                trust: ToolTrust::ExternalUntrusted,
            });
        let truncated = provenance.truncated;
        let mut provenance = provenance;
        provenance.elapsed_ms = Some(elapsed_ms);
        provenance.truncated = truncated;
        Ok(StructuredToolResult::with_provenance(
            output, true, provenance,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    #[test]
    fn name_is_websearch() {
        let t = WebSearchTool::default();
        assert_eq!(t.name(), "websearch");
    }

    #[test]
    fn parameters_require_query() {
        let t = WebSearchTool::default();
        let p = t.parameters();
        let required = p.get("required").and_then(|v| v.as_array()).unwrap();
        assert!(required.iter().any(|v| v == "query"));
    }
}
