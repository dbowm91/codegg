//! Tool catalog for search and discovery.
//!
//! This module provides the ToolCatalog for registering and searching tools.
//! It supports deferred loading of tools that should only be loaded on-demand.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::Tool;

/// Metadata about a tool for catalog/registry purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub defer_load: bool,
}

impl ToolMetadata {
    pub fn from_tool(tool: &dyn Tool) -> Self {
        Self {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            parameters: tool.parameters(),
            defer_load: tool.defer_loading(),
        }
    }
}

/// Catalog of available tools with search capabilities.
///
/// The catalog maintains a mapping of tool names to their metadata,
/// and tracks which tools should be loaded on-demand (deferred).
#[derive(Clone)]
pub struct ToolCatalog {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
}

impl ToolCatalog {
    /// Create a new empty tool catalog.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            deferred_load: Vec::new(),
        }
    }

    /// Register a tool in the catalog.
    pub fn register(&mut self, tool: &dyn Tool) {
        let metadata = ToolMetadata::from_tool(tool);
        let name = metadata.name.clone();

        if metadata.defer_load && !self.deferred_load.contains(&name) {
            self.deferred_load.push(name.clone());
        }

        self.tools.insert(name, metadata);
    }

    /// Search tools by name or description.
    ///
    /// Returns all tools whose name or description contains the query (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&ToolMetadata> {
        let query_lower = query.to_lowercase();

        self.tools
            .values()
            .filter(|metadata| {
                metadata.name.to_lowercase().contains(&query_lower)
                    || metadata.description.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&ToolMetadata> {
        self.tools.get(name)
    }

    /// List all tools marked for deferred loading.
    pub fn deferred_tools(&self) -> Vec<&ToolMetadata> {
        self.deferred_load
            .iter()
            .filter_map(|name| self.tools.get(name))
            .collect()
    }

    /// List all tools in the catalog.
    pub fn list(&self) -> Vec<&ToolMetadata> {
        self.tools.values().collect()
    }

    /// Check if a tool is marked for deferred loading.
    pub fn is_deferred(&self, name: &str) -> bool {
        self.deferred_load.contains(&name.to_string())
    }
}

impl Default for ToolCatalog {
    fn default() -> Self {
        Self::new()
    }
}
