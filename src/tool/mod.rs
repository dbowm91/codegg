//! Tool registry and built-in tools.
//!
//! This module provides the Tool trait and ToolRegistry for managing available tools.
//! Tools are the primary way Codegg interacts with the filesystem, terminal, and external
//! services. Each tool implements the Tool trait with name, description, parameters,
//! and execution logic.

pub mod apply_patch;
pub mod backend;
pub mod backend_config;
pub mod bash;
pub mod batch;
pub mod catalog;
pub mod codesearch;
pub mod commit;
pub mod destructive;
pub mod diff;
pub mod disabled;
pub mod edit;
pub mod factory;
pub mod formatter;
pub mod git;
pub mod glob;
pub mod goal;
pub mod grep;
pub mod image;
pub mod invalid;
pub mod list;
pub mod lsp;
pub(crate) mod lsp_security;
pub mod multiedit;
pub mod patch_util;
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

    /// Whether this tool should appear in the model-facing tool
    /// definitions (`ToolRegistry::definitions()` /
    /// `AgentLoop::build_tool_definitions()`). Defaults to `true`.
    /// Tools that exist only for diagnostics, placeholders, or
    /// hidden state (e.g. `DisabledTool`) should override this to
    /// `false` so they do not pollute the model's tool surface.
    /// These tools can still be invoked by name (e.g. by tests or
    /// `/tool-backends` diagnostics) — they just do not appear in
    /// the catalog the model sees.
    fn expose_in_definitions(&self) -> bool {
        true
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
    tool_backends: ToolBackendConfig,
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
    /// Optional shared artifact store for context read recovery.
    /// When `Some`, `context_read` is registered so the model can
    /// expand compressed tool output via `ctx://` handles.
    pub context_artifact_store: Option<Arc<dyn crate::context::ContextArtifactStore>>,
    /// Optional session id for context read registration.
    /// Required alongside `context_artifact_store` to enable `context_read`.
    pub context_session_id: Option<String>,
    /// Whether the `context_read` tool should be registered.
    /// Requires `context_artifact_store` and `context_session_id` to
    /// also be `Some`.
    pub context_read_enabled: bool,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            catalog: catalog::ToolCatalog::new(),
            tool_backends: ToolBackendConfig::default(),
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
        match (options.todo_state.as_ref(), options.todo_policy.as_ref()) {
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
                        _ => crate::tool::todo::TodoWriteTool::new(state.clone(), policy.clone()),
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

        // --- LSP: consult resolved backend config. ---
        let lsp_backend = options
            .tool_backends
            .backend_for(crate::tool::backend::BackendDomain::Lsp);
        let lsp_fallback = options
            .tool_backends
            .lsp
            .as_ref()
            .map(|c| c.fallback_to_native())
            .unwrap_or(true);
        match lsp_backend {
            ToolImplementationBackend::Native | ToolImplementationBackend::Builtin => {
                let lsp_service = options.lsp_service.unwrap_or_else(|| {
                    Arc::new(crate::lsp::service::LspService::new(
                        crate::lsp::config_lsp_to_egglsp(
                            crate::config::schema::LspConfig::default(),
                        ),
                    ))
                });
                registry.register(crate::tool::lsp::LspTool::new(lsp_service));
            }
            ToolImplementationBackend::Disabled => {
                registry.register(crate::tool::disabled::DisabledTool::new(
                    "lsp",
                    crate::tool::lsp::LspTool::new(Arc::new(crate::lsp::service::LspService::new(
                        crate::lsp::config_lsp_to_egglsp(crate::config::schema::LspConfig::default()),
                    )))
                    .description(),
                    "lsp backend is configured as 'disabled' ([tool_backends.lsp].backend = \"disabled\")",
                ));
            }
            ToolImplementationBackend::Mcp => {
                if lsp_fallback {
                    // MCP-configured but no live server: keep the
                    // native wrapper as the active path. Diagnostics
                    // report this row as `fallback-native`.
                    let lsp_service = options.lsp_service.clone().unwrap_or_else(|| {
                        Arc::new(crate::lsp::service::LspService::new(
                            crate::lsp::config_lsp_to_egglsp(
                                crate::config::schema::LspConfig::default(),
                            ),
                        ))
                    });
                    registry.register(crate::tool::lsp::LspTool::new(lsp_service));
                } else {
                    // MCP configured, no fallback. Don't register a
                    // model-visible tool. The diagnostic
                    // `backend_report` reports this as
                    // `unavailable` (ConfiguredButUnavailable).
                    registry.register(crate::tool::disabled::DisabledTool::new(
                        "lsp",
                        "Experimental: Query LSP server for code intelligence.",
                        "lsp MCP backend is configured but no MCP server is connected and fallback_to_native is false; set [tool_backends.lsp].backend = \"native\" or \"disabled\" or enable fallback_to_native",
                    ));
                }
            }
        }

        registry.register(crate::tool::commit::CommitTool::new());

        // --- Security: consult resolved backend config. ---
        let sec_backend = options
            .tool_backends
            .backend_for(crate::tool::backend::BackendDomain::Security);
        let sec_fallback = options
            .tool_backends
            .security
            .as_ref()
            .map(|c| c.fallback_to_native())
            .unwrap_or(true);
        match sec_backend {
            ToolImplementationBackend::Native | ToolImplementationBackend::Builtin => {
                registry.register(crate::tool::security::SecurityTool);
            }
            ToolImplementationBackend::Disabled => {
                registry.register(crate::tool::disabled::DisabledTool::new(
                    "security",
                    crate::tool::security::SecurityTool.description(),
                    "security backend is configured as 'disabled' ([tool_backends.security].backend = \"disabled\")",
                ));
            }
            ToolImplementationBackend::Mcp => {
                if sec_fallback {
                    // MCP-configured but no live server: keep the
                    // native wrapper as the active path. Diagnostics
                    // report this row as `fallback-native`.
                    registry.register(crate::tool::security::SecurityTool);
                } else {
                    // MCP configured, no fallback. Don't register a
                    // model-visible tool. The diagnostic
                    // `backend_report` reports this as
                    // `unavailable` (ConfiguredButUnavailable).
                    registry.register(crate::tool::disabled::DisabledTool::new(
                        "security",
                        "Deterministic security scanning tool.",
                        "security MCP backend is configured but no MCP server is connected and fallback_to_native is false; set [tool_backends.security].backend = \"native\" or \"disabled\" or enable fallback_to_native",
                    ));
                }
            }
        }
        registry.register(crate::tool::plan::PlanEnterTool);
        registry.register(crate::tool::plan::PlanExitTool);
        registry.register(crate::tool::invalid::InvalidTool);

        // Register tool_search with catalog for on-demand tool discovery.
        let search_tool =
            crate::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone()));
        registry.register(search_tool);

        // Stash the resolved backend configuration for diagnostics
        // (`backend_report`) and any wrapper that wants to consult
        // the runtime-resolved backend at call time.
        registry.tool_backends = options.tool_backends;

        // --- Context read tool (artifact expansion) ---
        if options.context_read_enabled {
            if let (Some(store), Some(session_id)) =
                (options.context_artifact_store, options.context_session_id)
            {
                registry.register(crate::context::ContextReadTool::new(store, session_id));
            }
        }

        registry
    }

    pub fn with_defaults() -> Self {
        Self::with_options(ToolRegistryOptions::default())
    }

    /// Build a registry from a loaded `Config`. The resolved
    /// per-domain backend configuration is passed through to
    /// `ToolRegistryOptions::tool_backends` so individual wrappers
    /// can consult it.
    pub fn with_config(config: &crate::config::schema::Config) -> Self {
        let tool_backends = ToolBackendConfig::from_config(config);
        Self::with_options(ToolRegistryOptions {
            tool_backends,
            ..ToolRegistryOptions::default()
        })
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
            .filter(|t| t.expose_in_definitions())
            .map(|t| crate::provider::ToolDefinition {
                name: interner.intern(t.name()).to_string(),
                description: interner.intern(t.description()).to_string(),
                parameters: t.parameters(),
                defer_loading: if t.defer_loading() { Some(true) } else { None },
            })
            .collect()
    }

    /// Build a session registry using the loaded `Config`'s
    /// resolved `[tool_backends]` config.
    ///
    /// **This is the constructor that production session paths
    /// (the agent loop, the daemon) must use.** It preserves the
    /// user's resolved backend configuration so that
    /// `backend_report`, `definitions`, and the per-domain
    /// registration branches all agree with what was configured.
    pub fn with_session_config_defaults(
        config: &crate::config::schema::Config,
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
            tool_backends: ToolBackendConfig::from_config(config),
            context_artifact_store: None,
            context_session_id: None,
            context_read_enabled: false,
        })
    }

    /// Session registry with all-native backend defaults.
    ///
    /// **WARNING: This constructor drops the loaded
    /// `[tool_backends]` config.** It exists for tests and
    /// non-config-aware callers that only care about session-aware
    /// todo wiring. Production code that has access to a loaded
    /// `Config` must use [`Self::with_session_config_defaults`]
    /// (or build a `ToolRegistryOptions` directly) so that backend
    /// config is preserved.
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
            context_artifact_store: None,
            context_session_id: None,
            context_read_enabled: false,
        })
    }

    pub fn set_search_tool_available_tools(&mut self, available: Vec<String>) {
        if let Some(tool) = self.tools.get_mut("tool_search") {
            tool.set_available_tools(available);
        }
    }

    /// Resolved backend configuration captured at construction.
    /// Used by `backend_report` and by wrapper tools that want to
    /// consult the runtime-resolved backend at call time.
    pub fn tool_backends(&self) -> &ToolBackendConfig {
        &self.tool_backends
    }

    /// Whether a tool with the given name is currently registered.
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Run a tool by name, preferring `execute_structured` so that
    /// provenance/trust metadata can be recorded. The model-facing
    /// string output is identical to `execute()`. Tools that do not
    /// override `execute_structured` get a legacy provenance record
    /// so call sites that read provenance still get *something*
    /// useful.
    pub async fn execute_capture(
        &self,
        name: &str,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, crate::error::ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| crate::error::ToolError::NotFound(name.to_string()))?;
        let start = std::time::Instant::now();
        let result = tool.execute_structured(input, ctx).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(mut structured) => {
                if let Some(p) = structured.provenance.as_mut() {
                    if p.elapsed_ms.is_none() {
                        p.elapsed_ms = Some(elapsed_ms);
                    }
                } else {
                    structured.provenance = Some(ToolProvenance::legacy(name));
                }
                tracing::debug!(
                    tool = %name,
                    backend = structured.provenance.as_ref().map(|p| p.backend.as_str()).unwrap_or(""),
                    implementation = structured.provenance.as_ref().map(|p| p.implementation.as_str()).unwrap_or(""),
                    elapsed_ms,
                    "tool executed via structured path"
                );
                Ok(structured)
            }
            Err(e) => Err(e),
        }
    }

    /// Build a runtime status report for the three configurable
    /// backends (lsp, security, context). The report is derived
    /// from the **actually registered** tools plus the resolved
    /// backend config, not from hardcoded assumptions.
    ///
    /// `mcp_server_names` is the list of MCP server names currently
    /// connected to `McpService`. When `None`, the report cannot
    /// tell whether an MCP-configured domain has a live server.
    pub fn backend_report(
        &self,
        mcp_server_names: Option<&[String]>,
    ) -> Vec<RegistryBackendStatus> {
        use crate::tool::backend::{BackendDomain, ToolImplementationBackend};
        use RegistryBackendStatusKind as Kind;

        // Helper: classify a registered tool as "native" (real
        // wrapper), "disabled" (DisabledTool stub), or absent.
        let classify_registered = |name: &str| -> RegisteredKind {
            match self.tools.get(name) {
                None => RegisteredKind::Absent,
                Some(t) => {
                    if t.expose_in_definitions() {
                        RegisteredKind::Native
                    } else {
                        RegisteredKind::DisabledStub
                    }
                }
            }
        };

        let mut out = Vec::with_capacity(3);
        for (domain, tool_name) in [
            (BackendDomain::Lsp, "lsp"),
            (BackendDomain::Security, "security"),
            (BackendDomain::Context, "context"),
        ] {
            let cfg = self.tool_backends.cfg_for(domain);
            let configured = cfg.and_then(|c| c.backend);
            let registered = classify_registered(tool_name);

            let (status, backend_label) = match configured {
                Some(ToolImplementationBackend::Disabled) => {
                    (Kind::Disabled, "disabled".to_string())
                }
                Some(ToolImplementationBackend::Mcp) => {
                    let server = cfg.and_then(|c| c.server_name.as_deref());
                    let connected = match (server, mcp_server_names) {
                        (Some(s), Some(names)) => names.iter().any(|n| n == s),
                        _ => false,
                    };
                    let fallback = cfg.map(|c| c.fallback_to_native()).unwrap_or(true);
                    match (fallback, connected, registered) {
                        // MCP configured, no fallback, no live
                        // server: tool is hidden from model.
                        (false, false, _) => (Kind::ConfiguredButUnavailable, "mcp".to_string()),
                        // MCP configured, no fallback, live server:
                        // a disabled stub is still registered so
                        // diagnostics are honest. Real execution
                        // is not possible because the native
                        // wrapper is *not* registered in this
                        // mode.
                        (false, true, _) => (Kind::ConfiguredButUnavailable, "mcp".to_string()),
                        // MCP configured, fallback enabled, live
                        // server: the native wrapper is the live
                        // path (we never delegate from
                        // `LspTool`/`SecurityTool` to an MCP
                        // server), so we still report
                        // `fallback-native`.
                        (true, true, _) => (Kind::FallbackToNative, "mcp".to_string()),
                        // MCP configured, fallback enabled, no
                        // live server: native wrapper is the
                        // active path.
                        (true, false, _) => (Kind::FallbackToNative, "mcp".to_string()),
                    }
                }
                Some(ToolImplementationBackend::Builtin) => match registered {
                    RegisteredKind::Native => (Kind::Active, "builtin".to_string()),
                    _ => (Kind::ConfiguredButUnavailable, "builtin".to_string()),
                },
                Some(ToolImplementationBackend::Native) | None => match registered {
                    RegisteredKind::Native => (Kind::Active, "native".to_string()),
                    _ => (Kind::ConfiguredButUnavailable, "native".to_string()),
                },
            };

            out.push(RegistryBackendStatus {
                domain: match domain {
                    BackendDomain::Lsp => "lsp",
                    BackendDomain::Security => "security",
                    BackendDomain::Context => "context",
                },
                tool: tool_name,
                backend: backend_label,
                status,
            });
        }
        out
    }
}

/// Runtime status of a single configurable tool backend, derived
/// from the resolved config plus the actual registered tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryBackendStatus {
    pub domain: &'static str,
    pub tool: &'static str,
    pub backend: String,
    pub status: RegistryBackendStatusKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryBackendStatusKind {
    /// Tool is registered and ready.
    Active,
    /// Tool is intentionally not registered because the backend
    /// is configured as `disabled`.
    Disabled,
    /// Tool is configured to use a backend that is not currently
    /// available (e.g. MCP server not connected, no fallback).
    ConfiguredButUnavailable,
    /// Backend is unavailable but `fallback_to_native = true`,
    /// so the native wrapper is the live path.
    FallbackToNative,
}

/// Internal helper for [`ToolRegistry::backend_report`]: what kind
/// of tool, if any, is registered under a configurable domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegisteredKind {
    /// The real native wrapper is registered.
    Native,
    /// A non-model-visible [`crate::tool::disabled::DisabledTool`]
    /// stub is registered.
    DisabledStub,
    /// No tool is registered under this name.
    Absent,
}

impl std::fmt::Display for RegistryBackendStatusKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryBackendStatusKind::Active => write!(f, "active"),
            RegistryBackendStatusKind::Disabled => write!(f, "disabled"),
            RegistryBackendStatusKind::ConfiguredButUnavailable => {
                write!(f, "unavailable")
            }
            RegistryBackendStatusKind::FallbackToNative => write!(f, "fallback-native"),
        }
    }
}

#[cfg(test)]
mod backend_report_tests {
    use super::*;
    use crate::tool::backend::{
        ExternalToolBackendConfig, ToolBackendConfig, ToolImplementationBackend,
    };

    fn build_with_backends(backends: ToolBackendConfig) -> ToolRegistry {
        ToolRegistry::with_options(ToolRegistryOptions {
            tool_backends: backends,
            ..ToolRegistryOptions::default()
        })
    }

    #[test]
    fn all_native_with_default_registry_reports_active_for_registered_domains() {
        let registry = ToolRegistry::with_defaults();
        let report = registry.backend_report(None);
        assert_eq!(report.len(), 3);
        // lsp and security are real tools; context is not a wrapper
        // tool today, so it lands in the ConfiguredButUnavailable arm
        // for the default registry. That is the intended honest
        // signal: the backend is "native" but no tool is registered
        // because `context` helpers are inlined into the prompt
        // builder / compaction.
        for r in &report {
            if r.domain == "lsp" || r.domain == "security" {
                assert_eq!(
                    r.status,
                    RegistryBackendStatusKind::Active,
                    "expected active for {}, got {:?}",
                    r.domain,
                    r.status
                );
                assert_eq!(r.backend, "native");
            } else {
                assert_eq!(r.backend, "native");
            }
        }
    }

    #[test]
    fn disabled_lsp_reports_disabled() {
        let mut backends = ToolBackendConfig::all_native();
        backends.lsp = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Disabled),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        let report = registry.backend_report(None);
        let lsp = report.iter().find(|r| r.domain == "lsp").unwrap();
        assert_eq!(lsp.status, RegistryBackendStatusKind::Disabled);
    }

    #[test]
    fn mcp_without_connected_server_reports_unavailable() {
        let mut backends = ToolBackendConfig::all_native();
        backends.lsp = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Mcp),
            server_name: Some("egglsp".to_string()),
            fallback_to_native: Some(false),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        let report = registry.backend_report(Some(&[]));
        let lsp = report.iter().find(|r| r.domain == "lsp").unwrap();
        assert_eq!(
            lsp.status,
            RegistryBackendStatusKind::ConfiguredButUnavailable
        );
    }

    #[test]
    fn mcp_with_fallback_reports_fallback_native() {
        let mut backends = ToolBackendConfig::all_native();
        backends.security = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Mcp),
            server_name: Some("eggsentry".to_string()),
            fallback_to_native: Some(true),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        let report = registry.backend_report(Some(&[]));
        let sec = report.iter().find(|r| r.domain == "security").unwrap();
        assert_eq!(sec.status, RegistryBackendStatusKind::FallbackToNative);
    }

    #[test]
    fn mcp_with_connected_server_reports_fallback_native() {
        // When the MCP server is connected but the native wrapper
        // is registered (fallback_to_native = true, default), the
        // live path is the native wrapper. The diagnostic is
        // honest about this: `fallback-native` with the live
        // implementation being the native LspTool, not the
        // (non-existent) MCP dispatcher.
        let mut backends = ToolBackendConfig::all_native();
        backends.lsp = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Mcp),
            server_name: Some("egglsp".to_string()),
            fallback_to_native: Some(true),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        let names = vec!["egglsp".to_string()];
        let report = registry.backend_report(Some(&names));
        let lsp = report.iter().find(|r| r.domain == "lsp").unwrap();
        assert_eq!(lsp.status, RegistryBackendStatusKind::FallbackToNative);
        assert_eq!(lsp.backend, "mcp");
    }

    #[test]
    fn mcp_with_connected_server_no_fallback_reports_unavailable() {
        // With fallback disabled, the live path is not registered
        // (only a hidden disabled stub). Even with a connected
        // server, the wrapper is unavailable.
        let mut backends = ToolBackendConfig::all_native();
        backends.lsp = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Mcp),
            server_name: Some("egglsp".to_string()),
            fallback_to_native: Some(false),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        let names = vec!["egglsp".to_string()];
        let report = registry.backend_report(Some(&names));
        let lsp = report.iter().find(|r| r.domain == "lsp").unwrap();
        assert_eq!(
            lsp.status,
            RegistryBackendStatusKind::ConfiguredButUnavailable
        );
    }

    #[test]
    fn status_kind_display_values() {
        assert_eq!(RegistryBackendStatusKind::Active.to_string(), "active");
        assert_eq!(RegistryBackendStatusKind::Disabled.to_string(), "disabled");
        assert_eq!(
            RegistryBackendStatusKind::ConfiguredButUnavailable.to_string(),
            "unavailable"
        );
        assert_eq!(
            RegistryBackendStatusKind::FallbackToNative.to_string(),
            "fallback-native"
        );
    }

    #[test]
    fn disabled_lsp_registers_disabled_stub() {
        let mut backends = ToolBackendConfig::all_native();
        backends.lsp = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Disabled),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        let lsp = registry
            .get("lsp")
            .expect("disabled lsp stub should be registered");
        assert_eq!(lsp.name(), "lsp");
        // Calling should fail with the configured reason.
        let result =
            futures::executor::block_on(lsp.execute(serde_json::json!({"operation": "hover"})));
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("lsp backend is configured as 'disabled'"),
            "got: {msg}"
        );
    }

    #[test]
    fn disabled_security_registers_disabled_stub() {
        let mut backends = ToolBackendConfig::all_native();
        backends.security = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Disabled),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        let sec = registry
            .get("security")
            .expect("disabled security stub should be registered");
        let result = futures::executor::block_on(
            sec.execute(serde_json::json!({"action": "classify_command"})),
        );
        assert!(result.is_err());
    }

    #[test]
    fn mcp_lsp_with_fallback_registers_native_wrapper() {
        let mut backends = ToolBackendConfig::all_native();
        backends.lsp = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Mcp),
            server_name: Some("egglsp".to_string()),
            fallback_to_native: Some(true),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        // With fallback_to_native = true the native wrapper is
        // registered (not a disabled stub). It is the live path
        // even though MCP is configured.
        let lsp = registry
            .get("lsp")
            .expect("native lsp wrapper should be registered when fallback is on");
        assert_eq!(lsp.name(), "lsp");
        // The native wrapper is exposed in definitions.
        let defs = registry.definitions();
        assert!(
            defs.iter().any(|d| d.name == "lsp"),
            "lsp should be model-visible when fallback is on"
        );
    }

    #[test]
    fn mcp_lsp_without_fallback_registers_hidden_stub() {
        let mut backends = ToolBackendConfig::all_native();
        backends.lsp = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Mcp),
            server_name: Some("egglsp".to_string()),
            fallback_to_native: Some(false),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        // A disabled stub is registered for diagnostics but is
        // hidden from the model.
        let lsp = registry
            .get("lsp")
            .expect("disabled lsp stub should be registered");
        let defs = registry.definitions();
        assert!(
            !defs.iter().any(|d| d.name == "lsp"),
            "lsp should NOT be model-visible when fallback is off"
        );
        // Calling the stub still surfaces a clear reason.
        let result =
            futures::executor::block_on(lsp.execute(serde_json::json!({"operation": "hover"})));
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("lsp MCP backend is configured but no MCP server is connected"),
            "got: {msg}"
        );
    }

    #[test]
    fn mcp_security_with_fallback_registers_native_wrapper() {
        let mut backends = ToolBackendConfig::all_native();
        backends.security = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Mcp),
            server_name: Some("eggsentry".to_string()),
            fallback_to_native: Some(true),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        let sec = registry
            .get("security")
            .expect("native security wrapper should be registered when fallback is on");
        assert_eq!(sec.name(), "security");
        let defs = registry.definitions();
        assert!(
            defs.iter().any(|d| d.name == "security"),
            "security should be model-visible when fallback is on"
        );
    }

    #[test]
    fn mcp_security_without_fallback_registers_hidden_stub() {
        let mut backends = ToolBackendConfig::all_native();
        backends.security = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Mcp),
            server_name: Some("eggsentry".to_string()),
            fallback_to_native: Some(false),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        let sec = registry
            .get("security")
            .expect("disabled security stub should be registered");
        let defs = registry.definitions();
        assert!(
            !defs.iter().any(|d| d.name == "security"),
            "security should NOT be model-visible when fallback is off"
        );
        let result = futures::executor::block_on(
            sec.execute(serde_json::json!({"action": "classify_command"})),
        );
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("security MCP backend is configured but no MCP server is connected"),
            "got: {msg}"
        );
    }

    #[test]
    fn builtin_lsp_treated_as_native() {
        let mut backends = ToolBackendConfig::all_native();
        backends.lsp = Some(ExternalToolBackendConfig {
            backend: Some(ToolImplementationBackend::Builtin),
            ..Default::default()
        });
        let registry = build_with_backends(backends);
        // The real LspTool is registered (Builtin == Native for lsp).
        let lsp = registry
            .get("lsp")
            .expect("real lsp tool should be registered");
        // The description should match the real LspTool description.
        assert!(lsp.description().contains("LSP server"));
    }
}
