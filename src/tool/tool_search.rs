//! Tool search for on-demand tool discovery.
//!
//! This tool allows the LLM to search for tools by name or description,
//! enabling on-demand tool discovery without loading all tools at once.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;
use crate::tool::catalog::ToolCatalog;
use crate::tool::Tool;

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

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("query required".into()))?;

        let results = self.catalog.search(query);

        let filtered: Vec<&crate::tool::catalog::ToolMetadata> = match &self.available_tools {
            Some(available) => results
                .into_iter()
                .filter(|m| available.iter().any(|a| a == &m.name))
                .collect(),
            None => results,
        };

        if filtered.is_empty() {
            return Ok(json!({
                "status": "no_results",
                "query": query,
                "tools": []
            })
            .to_string());
        }

        let tools: Vec<serde_json::Value> = filtered
            .into_iter()
            .map(|metadata| {
                json!({
                    "name": metadata.name,
                    "description": metadata.description,
                    "parameters": metadata.parameters,
                    "defer_load": metadata.defer_load
                })
            })
            .collect();

        Ok(json!({
            "status": "success",
            "query": query,
            "count": tools.len(),
            "tools": tools
        })
        .to_string())
    }
}
