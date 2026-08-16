use crate::error::ToolError;
use crate::search_backend;
use crate::tool::{StructuredToolResult, Tool, ToolCategory, ToolExecutionContext};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Instant;

const MAX_QUERY_LENGTH: usize = 10000;

#[derive(Debug, Deserialize)]
struct CodeSearchInput {
    query: String,
    #[serde(default = "default_tokens")]
    tokens_num: usize,
}

fn default_tokens() -> usize {
    5000
}

pub struct CodeSearchTool;

impl CodeSearchTool {
    fn request_input(input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let parsed: CodeSearchInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid codesearch input: {e}")))?;

        if parsed.query.len() > MAX_QUERY_LENGTH {
            return Err(ToolError::Execution(format!(
                "query exceeds maximum length of {} characters",
                MAX_QUERY_LENGTH
            )));
        }

        let sanitized: String = parsed
            .query
            .chars()
            .filter(|&c| {
                !c.is_control() && c != '\'' && c != '"' && c != ';' && c != '\\' && c != '\0'
            })
            .collect();

        if sanitized.is_empty() {
            return Err(ToolError::Execution(
                "query contains no valid characters".to_string(),
            ));
        }

        let max_results = (parsed.tokens_num.clamp(1000, 50_000) / 500).clamp(1, 30);
        Ok(serde_json::json!({
            "query": sanitized,
            "profile": "coding",
            "max_results": max_results,
        }))
    }
}

#[async_trait]
impl Tool for CodeSearchTool {
    fn name(&self) -> &str {
        "codesearch"
    }

    fn description(&self) -> &str {
        "Compatibility alias for coding-focused repository search through eggsearch. Prefer repo_search for structured repository queries. Results are external_untrusted evidence."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query for code context"
                },
                "tokens_num": {
                    "type": "number",
                    "description": "Number of tokens to return (1000-50000, default: 5000)"
                }
            },
            "required": ["query"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let request = Self::request_input(input)?;
        search_backend::dispatch_repo_search(&request).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let start = Instant::now();
        let request = Self::request_input(input)?;
        let result = search_backend::dispatch_repo_search_structured(&request).await?;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let mut provenance = search_backend::provenance_for_repo_search().unwrap_or_else(|| {
            use crate::tool::{ToolBackendKind, ToolProvenance, ToolTrust};
            ToolProvenance {
                backend: ToolBackendKind::Mcp.label().to_lowercase(),
                implementation: "repo_search (codesearch compatibility alias)".to_string(),
                version: None,
                elapsed_ms: Some(elapsed_ms),
                truncated: false,
                trust: ToolTrust::ExternalUntrusted,
            }
        });
        provenance.elapsed_ms = Some(elapsed_ms);
        Ok(search_backend::into_tool_result(result, provenance))
    }
}
