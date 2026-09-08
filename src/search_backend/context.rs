//! Explicit runtime-owned search/MCP service context (M005).
//!
//! `SearchRuntimeContext` replaces the historical mutable process-global
//! installation pattern (`state::install_mcp_service` /
//! `state::install_search_config`) for production tool execution.
//!
//! Ownership contract:
//!
//! - `config` is an owned immutable snapshot resolved from `Config` at
//!   runtime-construction time. Two independently constructed contexts can
//!   hold different `SearchConfig` values in one process without
//!   overwriting one another.
//! - `mcp` is the daemon-owned shared MCP transport (`McpService`).
//!   Concurrent tool calls may share one daemon service using its existing
//!   synchronization; session registries receive the same shared handle
//!   rather than narrower per-session transports. Dropping one context
//!   never tears down the shared service.
//! - The context is immutable after construction. There is no
//!   install/reset API: runtime restart reconstructs a fresh context
//!   from config/bootstrap, so no stale value can leak into the new
//!   instance.
//! - The context is internal and must never be serialized wholesale. Its
//!   `Debug` implementation redacts `[search.eggsearch.env]` values
//!   (keys only) so diagnostics cannot leak provider credentials.
//!
//! The legacy `state` module retains the process-global slots as
//! deprecated compatibility for bootstrap connection reuse and for tests
//! that explicitly exercise the legacy global wrappers. Production tool
//! execution must use this context instead.

use std::sync::Arc;
use std::sync::RwLock as StdRwLock;

use tokio::sync::RwLock;

use crate::config::schema::{SearchBackendConfig, SearchConfig};
use crate::mcp::McpService;

/// Explicit runtime-owned search/MCP services for one runtime context.
///
/// Constructed after config/MCP bootstrap (or with an explicit disabled
/// state) and cloned by `Arc` only for the actually shared `McpService`.
/// Tools retain a clone of this context and never query a mutable
/// process-wide slot at execution time.
#[derive(Clone)]
pub struct SearchRuntimeContext {
    pub(crate) config: SearchConfig,
    pub(crate) mcp: Option<Arc<RwLock<McpService>>>,
}

impl Default for SearchRuntimeContext {
    /// Default context: default `SearchConfig` (eggsearch backend) with
    /// no MCP service. Matches the historical global default (uninstalled
    /// slots read as default config + unavailable service) without
    /// touching process-global state.
    fn default() -> Self {
        Self {
            config: SearchConfig::default(),
            mcp: None,
        }
    }
}

impl std::fmt::Debug for SearchRuntimeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render `[search.eggsearch.env]` values: they may carry
        // provider credentials. Keys alone are sufficient for
        // diagnostics, and even key counts avoid leaking secret
        // prefix/suffix/length signals.
        let mut debug = f.debug_struct("SearchRuntimeContext");
        debug.field("backend", &format!("{:?}", self.config.backend()));
        debug.field("expose_raw_mcp_tools", &self.config.expose_raw_mcp_tools());
        debug.field("fallback_to_builtin", &self.config.fallback_to_builtin());
        if let Some(egg) = self.config.eggsearch.as_ref() {
            debug.field("server_name", &egg.server_name().to_string());
            debug.field("command", &egg.command().to_string());
            if let Some(env) = egg.env.as_ref() {
                let keys: Vec<&str> = env.keys().map(String::as_str).collect();
                debug.field("env_keys", &keys);
            }
        }
        debug.field("mcp_connected", &self.mcp.is_some());
        debug.finish()
    }
}

impl SearchRuntimeContext {
    /// Build a context from an explicitly resolved `SearchConfig` with
    /// no MCP service attached.
    pub fn new(config: SearchConfig) -> Self {
        Self { config, mcp: None }
    }

    /// Build a context from the daemon `Config`'s `[search]` section.
    pub fn from_config(config: &SearchConfig) -> Self {
        Self::new(config.clone())
    }

    /// Explicit disabled context: search execution always reports
    /// disabled regardless of any other context in the process.
    pub fn disabled() -> Self {
        Self::new(SearchConfig {
            backend: Some(SearchBackendConfig::Disabled),
            ..Default::default()
        })
    }

    /// Attach the daemon-owned shared MCP service handle.
    pub fn with_mcp(mut self, svc: Arc<RwLock<McpService>>) -> Self {
        self.mcp = Some(svc);
        self
    }

    /// Attach an optional shared MCP service handle.
    pub fn with_mcp_opt(mut self, svc: Option<Arc<RwLock<McpService>>>) -> Self {
        self.mcp = svc;
        self
    }

    /// The owned immutable search configuration snapshot.
    pub fn config(&self) -> &SearchConfig {
        &self.config
    }

    /// The shared daemon-owned MCP service handle, if bootstrap
    /// connected one.
    pub fn mcp(&self) -> Option<Arc<RwLock<McpService>>> {
        self.mcp.clone()
    }

    /// Whether an MCP service handle is attached.
    pub fn has_mcp_service(&self) -> bool {
        self.mcp.is_some()
    }

    /// Resolved backend for this context.
    pub fn backend(&self) -> SearchBackendConfig {
        self.config.backend()
    }

    /// Effective eggsearch server name for this context.
    pub fn server_name(&self) -> String {
        self.config
            .eggsearch
            .as_ref()
            .and_then(|e| e.server_name.clone())
            .unwrap_or_else(|| "eggsearch".to_string())
    }

    /// Snapshot this context into the legacy process-global slots.
    ///
    /// Compatibility only: lets a freshly bootstrapped explicit context
    /// populate the legacy slots that pre-existing global-wrapper
    /// callers still read. New production execution paths must use the
    /// context directly instead of reading the globals back.
    pub fn install_as_global_compat(&self) {
        super::state::install_search_config(self.config.clone());
        if let Some(svc) = self.mcp.clone() {
            super::state::install_mcp_service(svc);
        }
    }
}

/// Read the legacy process-global slots without mutating them.
///
/// Compatibility only for legacy global-wrapper dispatch and tests that
/// explicitly exercise the global path. Production tool execution must
/// hold a `SearchRuntimeContext` instead of calling this.
pub(crate) fn snapshot_global() -> SearchRuntimeContext {
    let mcp = super::state::mcp_service();
    let config = super::state::search_config();
    SearchRuntimeContext { config, mcp }
}

/// Global lock type retained for documentation: the legacy slots use
/// `std::sync::RwLock` so reads never cross `.await` points.
#[allow(dead_code)]
fn _lock_granularity_note(_guard: &StdRwLock<Option<SearchConfig>>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::EggsearchConfig;

    #[test]
    fn default_context_matches_global_default_semantics() {
        let ctx = SearchRuntimeContext::default();
        assert_eq!(ctx.backend(), SearchBackendConfig::Eggsearch);
        assert!(!ctx.has_mcp_service());
    }

    #[test]
    fn disabled_context_reports_disabled_backend() {
        let ctx = SearchRuntimeContext::disabled();
        assert_eq!(ctx.backend(), SearchBackendConfig::Disabled);
    }

    #[test]
    fn two_contexts_hold_independent_configs() {
        let enabled = SearchRuntimeContext::new(SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            ..Default::default()
        });
        let disabled = SearchRuntimeContext::disabled();
        assert_eq!(enabled.backend(), SearchBackendConfig::Eggsearch);
        assert_eq!(disabled.backend(), SearchBackendConfig::Disabled);
    }

    #[test]
    fn debug_redacts_env_values() {
        let ctx = SearchRuntimeContext::new(SearchConfig {
            eggsearch: Some(EggsearchConfig {
                env: Some(
                    [(
                        "BRAVE_SEARCH_API_KEY".to_string(),
                        "secret-value".to_string(),
                    )]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        });
        let rendered = format!("{ctx:?}");
        assert!(rendered.contains("BRAVE_SEARCH_API_KEY"));
        assert!(!rendered.contains("secret-value"));
    }

    #[test]
    fn server_name_defaults_to_eggsearch() {
        let ctx = SearchRuntimeContext::new(SearchConfig::default());
        assert_eq!(ctx.server_name(), "eggsearch");
    }
}
