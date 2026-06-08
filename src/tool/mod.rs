//! Tool registry and built-in tools.
//!
//! This module provides the Tool trait and ToolRegistry for managing available tools.
//! Tools are the primary way Codegg interacts with the filesystem, terminal, and external
//! services. Each tool implements the Tool trait with name, description, parameters,
//! and execution logic.

pub mod apply_patch;
pub mod backend;
pub mod bash;
pub mod batch;
pub mod catalog;
pub mod codesearch;
pub mod commit;
pub mod destructive;
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
pub mod research;
pub mod review;
pub mod risk;
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

// Re-export the backend contract types from `tool::backend` so the
// rest of the codebase (and downstream callers) can refer to them
// via `tool::ToolBackendKind` / `tool::StructuredToolResult` etc.
pub use backend::{
    BackendDomain, ExternalToolBackendConfig, StructuredToolResult, ToolBackendConfig,
    ToolBackendKind, ToolExecutionContext, ToolImplementationBackend, ToolProvenance, ToolTrust,
};

static DEFAULT_REGISTRY: Lazy<ToolRegistry> = Lazy::new(ToolRegistry::with_defaults);

#[inline]
pub fn default_registry() -> &'static ToolRegistry {
    &DEFAULT_REGISTRY
}

/// Classification of a tool's safety properties.
///
/// Used by the permission system to short-circuit read-only and safe-mutating
/// tools so they never produce a permission prompt. Bash is treated specially
/// (see `is_destructive_bash_command`) because command-level safety depends
/// on the actual command, not just the tool name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// No side effects; never requires permission (read, glob, grep, list, webfetch, etc.).
    ReadOnly,
    /// Mutates application state but considered safe (todowrite, question, invalid).
    SafeMutating,
    /// Mutates filesystem or has external side effects (edit, write, git, commit, etc.).
    Mutating,
    /// Executes shell commands. Permission is determined by command-level inspection.
    ShellExec,
}

impl ToolCategory {
    /// Returns true if this category should never produce a permission prompt
    /// (other than sensitive-path and security-policy escalation).
    pub fn is_permission_free(self) -> bool {
        matches!(self, ToolCategory::ReadOnly | ToolCategory::SafeMutating)
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;

    /// Tool's safety category. The default is `Mutating` (conservative).
    /// Override in tools that have no side effects (`ReadOnly`) or only
    /// mutate internal app state (`SafeMutating`).
    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    /// Set the list of tool names available for on-demand discovery.
    /// Only relevant for the `tool_search` tool; default is no-op.
    fn set_available_tools(&mut self, _tools: Vec<String>) {}

    /// Whether this tool should be deferred (not sent to LLM on every request).
    /// Deferred tools are only loaded on-demand via tool_search.
    fn defer_loading(&self) -> bool {
        false
    }

    /// Opt-in structured execution. The default implementation simply
    /// delegates to `execute()` and wraps the result in a
    /// `StructuredToolResult::legacy(...)` with no provenance beyond
    /// the tool name. Tools with rich backend metadata (e.g. native
    /// wrappers that delegate to MCP) should override this to attach
    /// provenance, trust framing, and timing information.
    ///
    /// Callers that only need a string can keep using `execute()`.
    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let output = self.execute(input).await?;
        Ok(StructuredToolResult::legacy(self.name(), output))
    }
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

/// Options that configure how a `ToolRegistry` is built.
///
/// This is the single authoritative input for tool registration. All
/// other constructors (`with_defaults`, `with_session_defaults`) are
/// thin wrappers that build a `ToolRegistryOptions` and call
/// `with_options`.
#[derive(Default)]
pub struct ToolRegistryOptions {
    /// Optional shared todo state. When `None`, the legacy
    /// `TodoTool::default()` is registered (no session persistence).
    pub todo_state: Option<Arc<tokio::sync::Mutex<crate::task_state::TodoState>>>,
    /// Optional task-state policy. When `None` and `todo_state` is
    /// also `None`, the legacy todo tool is used. When both are
    /// `Some`, the policy gates whether todowrite/todoread are
    /// registered.
    pub todo_policy: Option<crate::model_profile::types::TaskStatePolicy>,
    /// Optional SQLite pool, used to enable session-todo persistence
    /// for `TodoWriteTool`.
    pub pool: Option<sqlx::SqlitePool>,
    /// Optional session id, used together with `pool` to enable
    /// session-todo persistence.
    pub session_id: Option<String>,
    /// Optional pre-built LSP service. When `None`, a default
    /// `Arc<LspService>` is constructed from `LspConfig::default()`.
    pub lsp_service: Option<Arc<crate::lsp::service::LspService>>,
    /// Resolved per-domain backend configuration. The registry does
    /// not consume this directly today, but exposing it lets future
    /// wrappers consult the resolved backend without re-parsing
    /// config.
    pub tool_backends: ToolBackendConfig,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            catalog: catalog::ToolCatalog::new(),
        }
    }

    /// Single authoritative registration sequence.
    ///
    /// `with_defaults()` and `with_session_defaults()` are thin
    /// wrappers that construct a `ToolRegistryOptions` and call this.
    pub fn with_options(options: ToolRegistryOptions) -> Self {
        let mut registry = Self::new();

        // --- File-system / shell tools (always native) ---
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
        registry.register(crate::tool::research::ResearchTool::with_default_service());
        registry.register(crate::tool::image::ImageTool::default());
        registry.register(crate::tool::codesearch::CodeSearchTool);
        registry.register(crate::tool::question::QuestionTool);

        // --- Todo tools (policy + persistence gated) ---
        match (
            options.todo_state.as_ref(),
            options.todo_policy.as_ref(),
        ) {
            (Some(state), Some(policy)) => {
                use crate::model_profile::types::TodoMode;
                if policy.mode != TodoMode::Disabled && policy.allow_model_todo_write {
                    let tool = match (options.pool.clone(), options.session_id.clone()) {
                        (Some(p), Some(sid)) => crate::tool::todo::TodoWriteTool::with_persistence(
                            state.clone(),
                            policy.clone(),
                            p,
                            sid,
                        ),
                        _ => crate::tool::todo::TodoWriteTool::new(
                            state.clone(),
                            policy.clone(),
                        ),
                    };
                    registry.register(tool);
                }
                if policy.allow_model_todo_read {
                    registry.register(crate::tool::todo::TodoReadTool::new(
                        state.clone(),
                        policy.clone(),
                    ));
                }
            }
            _ => {
                // No session context: register the legacy default todo tool.
                registry.register(crate::tool::todo::TodoTool::default());
            }
        }

        registry.register(crate::tool::skill::SkillTool);
        registry.register(crate::tool::apply_patch::ApplyPatchTool::new());
        registry.register(crate::tool::diff::DiffTool::default());
        registry.register(crate::tool::replace::ReplaceTool::default());
        registry.register(crate::tool::review::ReviewTool::default());
        registry.register(crate::tool::terminal::TerminalTool::default());
        registry.register(crate::tool::git::GitTool::default());

        // --- LSP: prefer injected service, otherwise build default. ---
        let lsp_service = options.lsp_service.unwrap_or_else(|| {
            Arc::new(crate::lsp::service::LspService::new(
                crate::config::schema::LspConfig::default().into(),
            ))
        });
        registry.register(crate::tool::lsp::LspTool::new(lsp_service));

        registry.register(crate::tool::commit::CommitTool::new());
        registry.register(crate::tool::security::SecurityTool);
        registry.register(crate::tool::plan::PlanEnterTool);
        registry.register(crate::tool::plan::PlanExitTool);
        registry.register(crate::tool::invalid::InvalidTool);

        // Register tool_search with catalog for on-demand tool discovery.
        let search_tool = crate::tool::tool_search::ToolSearchTool::new(Arc::new(
            registry.catalog().clone(),
        ));
        registry.register(search_tool);

        // Note: `options.tool_backends` is currently only used to
        // thread through the resolved configuration. Future phases
        // will consult it from individual wrapper tools.
        let _ = options.tool_backends;

        registry
    }

    pub fn with_defaults() -> Self {
        Self::with_options(ToolRegistryOptions::default())
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

    pub fn catalog_mut(&mut self) -> &mut catalog::ToolCatalog {
        &mut self.catalog
    }

    /// Set the search mode on the catalog.
    pub fn set_search_mode(&mut self, mode: catalog::SearchMode) {
        self.catalog.set_search_mode(mode);
    }

    pub fn register_deferred_names(&mut self, names: &[String]) {
        self.catalog.register_deferred_names(names);
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
                defer_loading: if t.defer_loading() { Some(true) } else { None },
            })
            .collect()
    }

    pub fn with_session_defaults(
        todo_state: std::sync::Arc<tokio::sync::Mutex<crate::task_state::TodoState>>,
        policy: crate::model_profile::types::TaskStatePolicy,
        pool: Option<sqlx::SqlitePool>,
        session_id: Option<String>,
    ) -> Self {
        Self::with_options(ToolRegistryOptions {
            todo_state: Some(todo_state),
            todo_policy: Some(policy),
            pool,
            session_id,
            lsp_service: None,
            tool_backends: ToolBackendConfig::default(),
        })
    }

    pub fn set_search_tool_available_tools(&mut self, available: Vec<String>) {
        if let Some(tool) = self.tools.get_mut("tool_search") {
            tool.set_available_tools(available);
        }
    }
}
