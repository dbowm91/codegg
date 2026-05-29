//! Tool registry and built-in tools.
//!
//! This module provides the Tool trait and ToolRegistry for managing available tools.
//! Tools are the primary way Codegg interacts with the filesystem, terminal, and external
//! services. Each tool implements the Tool trait with name, description, parameters,
//! and execution logic.

pub mod apply_patch;
pub mod bash;
pub mod batch;
pub mod catalog;
pub mod codesearch;
pub mod commit;
pub mod diff;
pub mod edit;
pub mod formatter;
pub mod git;
pub mod glob;
pub mod grep;
pub mod image;
pub mod invalid;
pub mod list;
pub mod lsp;
pub mod multiedit;
pub mod plan;
pub mod question;
pub mod read;
pub mod replace;
pub mod review;
pub mod security;
pub mod skill;
pub mod task;
pub mod terminal;
pub mod todo;
pub mod tool_search;
pub mod util;
pub mod webfetch;
pub mod websearch;
pub mod write;

use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::ToolError;

static DEFAULT_REGISTRY: Lazy<ToolRegistry> = Lazy::new(ToolRegistry::with_defaults);

#[inline]
pub fn default_registry() -> &'static ToolRegistry {
    &DEFAULT_REGISTRY
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;

    /// Set the list of tool names available for on-demand discovery.
    /// Only relevant for the `tool_search` tool; default is no-op.
    fn set_available_tools(&mut self, _tools: Vec<String>) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: String,
    pub success: bool,
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    catalog: catalog::ToolCatalog,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            catalog: catalog::ToolCatalog::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(crate::tool::bash::BashTool::default());
        registry.register(crate::tool::read::ReadTool::default());
        registry.register(crate::tool::edit::EditTool::default());
        registry.register(crate::tool::write::WriteTool::default());
        registry.register(crate::tool::glob::GlobTool::default());
        registry.register(crate::tool::grep::GrepTool::default());
        registry.register(crate::tool::list::ListTool::default());
        registry.register(crate::tool::task::TaskTool::default());
        registry.register(crate::tool::webfetch::WebFetchTool::default());
        registry.register(crate::tool::websearch::WebSearchTool::default());
        registry.register(crate::tool::image::ImageTool::default());
        registry.register(crate::tool::codesearch::CodeSearchTool);
        registry.register(crate::tool::question::QuestionTool);
        registry.register(crate::tool::todo::TodoTool::default());
        registry.register(crate::tool::skill::SkillTool);
        registry.register(crate::tool::apply_patch::ApplyPatchTool::new());
        registry.register(crate::tool::diff::DiffTool::default());
        registry.register(crate::tool::replace::ReplaceTool::default());
        registry.register(crate::tool::review::ReviewTool::default());
        registry.register(crate::tool::terminal::TerminalTool::default());
        registry.register(crate::tool::git::GitTool::default());
        registry.register(crate::tool::lsp::LspTool::new(Arc::new(
            crate::lsp::service::LspService::new(crate::config::schema::LspConfig::default()),
        )));
        registry.register(crate::tool::commit::CommitTool::new());
        registry.register(crate::tool::security::SecurityTool);
        registry.register(crate::tool::plan::PlanEnterTool);
        registry.register(crate::tool::plan::PlanExitTool);
        registry.register(crate::tool::invalid::InvalidTool);
        // Register tool_search with catalog for on-demand tool discovery
        let search_tool =
            crate::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone()));
        registry.register(search_tool);
        registry
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        let name = tool.name().to_string();
        self.catalog.register(&tool);
        self.tools.insert(name, Box::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn catalog(&self) -> &catalog::ToolCatalog {
        &self.catalog
    }

    pub fn list(&self) -> Vec<&dyn Tool> {
        self.tools.values().map(|t| t.as_ref()).collect()
    }

    pub fn filter_out(&mut self, denied_tools: &[String]) {
        for tool_name in denied_tools {
            if tool_name.contains('*') || tool_name.contains('?') {
                // Glob pattern matching for dynamic tool names (e.g., mcp__*)
                if let Ok(glob) = globset::Glob::new(tool_name) {
                    let matcher = glob.compile_matcher();
                    let to_remove: Vec<String> = self
                        .tools
                        .keys()
                        .filter(|name| matcher.is_match(name))
                        .cloned()
                        .collect();
                    for name in to_remove {
                        tracing::info!(tool_name = %name, "Tool filtered out (denied by pattern)");
                        self.tools.remove(&name);
                    }
                }
            } else if self.tools.remove(tool_name).is_some() {
                tracing::info!(tool_name = %tool_name, "Tool filtered out (denied)");
            }
        }
    }

    pub fn definitions(&self) -> Vec<crate::provider::ToolDefinition> {
        let interner = crate::util::interner::tool_interner();
        self.tools
            .values()
            .map(|t| crate::provider::ToolDefinition {
                name: interner.intern(t.name()).to_string(),
                description: interner.intern(t.description()).to_string(),
                parameters: t.parameters(),
            })
            .collect()
    }

    pub fn set_search_tool_available_tools(&mut self, available: Vec<String>) {
        if let Some(tool) = self.tools.get_mut("tool_search") {
            tool.set_available_tools(available);
        }
    }
}
