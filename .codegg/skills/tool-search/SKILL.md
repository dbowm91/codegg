---
name: tool-search
description: Tool catalog system and tool_search tool for on-demand tool discovery
version: 1.0.0
tags:
  - tool
  - search
  - catalog
---

# Skill: tool-search

# Tool Search & Catalog Guide

This skill covers the tool catalog system and `tool_search` tool in opencode-rs for on-demand tool discovery.

## Architecture#

The `ToolCatalog` in `src/tool/catalog.rs` provides:
- **Search**: Find tools by name, description, or tags
- **Registration**: Tools register with metadata for discovery

The `tool_search` tool (`src/tool/tool_search.rs`) enables LLM-driven tool discovery:
- LLM can call `tool_search` with a query to find relevant tools
- Returns tool names, descriptions, and parameters
- Enables on-demand tool loading workflows

## ToolCatalog Struct#

```rust
pub struct ToolCatalog {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
}

pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub defer_load: bool,
}
```

## Tool Search Tool#

The `ToolSearchTool` in `src/tool/tool_search.rs`:

```rust
pub struct ToolSearchTool {
    catalog: Arc<ToolCatalog>,
}

impl Tool for ToolSearchTool {
    fn name(&self) -> &str { "tool_search" }
    
    fn description(&self) -> &str {
        "Search for available tools by name or description. \
         Returns a list of tools matching the query. \
         Use this to discover tools available for on-demand use."
    }
    
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to find relevant tools"
                }
            },
            "required": ["query"]
        })
    }
    
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let query = input["query"].as_str()...;
        let results = self.catalog.search(query);
        // Return JSON with tool names, descriptions, parameters
    }
}
```

## Registration#

The `tool_search` tool is registered in `src/tool/mod.rs`:

```rust
registry.register(crate::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone())));
```

## Usage by LLM#

```rust
// LLM can call tool_search to discover tools:
{
    "tool": "tool_search",
    "input": {
        "query": "file operations"
    }
}
// Returns:
{
    "status": "success",
    "query": "file operations",
    "count": 3,
    "tools": [
        {"name": "read", "description": "..."},
        {"name": "write", "description": "..."},
        {"name": "edit", "description": "..."}
    ]
}
```

Base directory for this skill: file:///Users/davidbowman/projects/codegg/.opencode/skills/tool-search
Relative paths in this skill (e.g., scripts/, reference/) are relative to this base directory.
Note: file list is sampled.
