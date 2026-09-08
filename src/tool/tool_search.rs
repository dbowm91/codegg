//! Tool search for on-demand tool discovery.
//!
//! This tool allows the LLM to search for tools by name or description,
//! enabling on-demand tool discovery without loading all tools at once.
//!
//! Discovery is monotonic: results are restricted to the already-allowed
//! policy set (`available_tools` installed by the agent loop after
//! deny/plan/disable/backend/ceiling filtering). Discovery never turns a
//! prohibited tool into a callable one. Hidden/internal tools are never
//! returned, even when the allow-list is unset (unit-test construction).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::tool::catalog::ToolCatalog;
use crate::tool::{Tool, ToolCategory};

/// Maximum tools returned per search so a broad query cannot move prompt
/// bloat into the search result.
pub const MAX_SEARCH_RESULTS: usize = 10;

/// Tool for searching available tools by query.
///
/// This enables on-demand tool discovery - the LLM can search for tools
/// based on the current context, and only relevant tools need to be
/// sent to the LLM.
#[derive(Clone)]
pub struct ToolSearchTool {
    catalog: Arc<ToolCatalog>,
    available_tools: Option<Vec<String>>,
}

impl ToolSearchTool {
    /// Create a new ToolSearchTool with the given catalog.
    pub fn new(catalog: Arc<ToolCatalog>) -> Self {
        Self {
            catalog,
            available_tools: None,
        }
    }

    /// Set the list of tool names that are currently available (after filtering).
    /// When set, search results are restricted to these tools.
    pub fn set_available_tools(&mut self, tools: Vec<String>) {
        self.available_tools = Some(tools);
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Search for available tools by name or description. \
         Returns a list of tools matching the query. \
         Use this to discover tools available for on-demand use."
    }

    fn set_available_tools(&mut self, tools: Vec<String>) {
        self.available_tools = Some(tools);
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to find relevant tools (searches name and description)"
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
            .ok_or_else(|| ToolError::Execution("query required".into()))?;

        // An empty query would match the whole catalog in keyword mode;
        // return no results instead of dumping registered capability.
        if query.trim().is_empty() {
            return Ok(json!({
                "status": "no_results",
                "query": query,
                "tools": []
            })
            .to_string());
        }

        let results = self.catalog.search(query);

        // Policy filtering first: only tools the current agent/session
        // policy allows are discoverable. When no allow-list is installed
        // (direct unit construction), still exclude hidden/internal tools.
        let filtered: Vec<&crate::tool::catalog::ToolMetadata> = match &self.available_tools {
            Some(available) => results
                .into_iter()
                .filter(|m| available.iter().any(|a| a == &m.name))
                .filter(|m| !crate::tool::disclosure::is_hidden(&m.name))
                .collect(),
            None => results
                .into_iter()
                .filter(|m| !crate::tool::disclosure::is_hidden(&m.name))
                .collect(),
        };

        if filtered.is_empty() {
            return Ok(json!({
                "status": "no_results",
                "query": query,
                "tools": []
            })
            .to_string());
        }

        // Cap results so discovery stays a selection aid, not a catalog dump.
        // `total_matches` preserves honesty about truncation.
        let total_matches = filtered.len();
        let tools: Vec<serde_json::Value> = filtered
            .into_iter()
            .take(MAX_SEARCH_RESULTS)
            .map(|metadata| {
                // Selection metadata only: canonical name, purpose,
                // category/risk/disclosure for correct choice among related
                // research/evidence tools. No backend config, endpoints,
                // credentials, reasoning, or plugin internals are returned.
                let risk =
                    crate::tool::risk::classify_tool_risk(&metadata.name, &serde_json::json!({}));
                json!({
                    "canonical_name": metadata.name,
                    "name": metadata.name,
                    "description": metadata.description,
                    "parameters": metadata.parameters,
                    "defer_load": metadata.defer_load,
                    "category": metadata.category,
                    "risk": format!("{risk:?}"),
                    "disclosure": metadata.disclosure
                })
            })
            .collect();

        Ok(json!({
            "status": "success",
            "query": query,
            "count": tools.len(),
            "total_matches": total_matches,
            "tools": tools
        })
        .to_string())
    }
}
