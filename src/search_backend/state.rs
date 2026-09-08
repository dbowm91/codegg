//! Legacy process-global search/MCP slots (deprecated compatibility).
//!
//! The websearch/webfetch tools historically read their backing service
//! and the resolved `SearchConfig` from these shared slots because
//! `ToolRegistry::with_defaults()` constructed the wrappers before the
//! `McpService` existed.
//!
//! M005 introduced [`super::context::SearchRuntimeContext`]: an explicit
//! runtime-owned context holding an owned immutable `SearchConfig`
//! snapshot plus the shared daemon-owned `McpService` handle. Production
//! tool execution must use that context (threaded through
//! `ToolRegistryOptions::search_runtime`) and never query these slots at
//! execution time.
//!
//! These slots are retained only for:
//!
//! - bootstrap connection reuse: `bootstrap_search_backend` installs the
//!   connected service here so a second bootstrap call from another
//!   process entry point reuses one daemon eggsearch connection instead
//!   of spawning a second server process;
//! - the legacy global `dispatch_*`/`provenance_for_*` wrappers, which
//!   snapshot (never mutate) the slots for backward-compatible callers;
//! - tests that explicitly exercise the legacy global path.
//!
//! Do not add new production readers. New code takes a
//! `SearchRuntimeContext`.

use std::sync::Arc;
use std::sync::RwLock as StdRwLock;

use tokio::sync::RwLock;

use crate::config::schema::SearchConfig;
use crate::mcp::McpService;

static MCP_SERVICE: StdRwLock<Option<Arc<RwLock<McpService>>>> = StdRwLock::new(None);
static SEARCH_CONFIG: StdRwLock<Option<SearchConfig>> = StdRwLock::new(None);

/// Install the process-wide `McpService` reference.
///
/// Deprecated compatibility: only `bootstrap` (daemon connection reuse)
/// and `SearchRuntimeContext::install_as_global_compat` may call this.
/// Production tool execution uses the explicit runtime context instead.
pub fn install_mcp_service(svc: Arc<RwLock<McpService>>) {
    let mut guard = MCP_SERVICE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(svc);
}

/// Returns the installed `McpService`, if any.
///
/// Deprecated compatibility: only legacy global dispatch wrappers,
/// bootstrap reuse checks, and diagnostics may call this. Production
/// tool execution uses `SearchRuntimeContext::mcp()` instead.
pub fn mcp_service() -> Option<Arc<RwLock<McpService>>> {
    MCP_SERVICE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Install the resolved `SearchConfig`. Idempotent: subsequent calls
/// overwrite the previous value.
///
/// Deprecated compatibility: only `bootstrap` and
/// `SearchRuntimeContext::install_as_global_compat` may call this.
/// Production tool execution uses the owned snapshot in
/// `SearchRuntimeContext::config()` instead.
pub fn install_search_config(cfg: SearchConfig) {
    let mut guard = SEARCH_CONFIG
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(cfg);
}

/// Returns the resolved `SearchConfig`, or a default if none has been
/// installed (e.g. in unit tests that never called bootstrap).
///
/// Deprecated compatibility: only legacy global dispatch wrappers and
/// tests exercising the legacy path may call this. Production tool
/// execution uses `SearchRuntimeContext::config()` instead.
pub fn search_config() -> SearchConfig {
    SEARCH_CONFIG
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_default()
}

/// Test-only: reset the global search backend state slots to `None`.
///
/// Tests that mutate `install_search_config` or `install_mcp_service` should
/// call this at the start of the test to ensure a clean baseline and avoid
/// cross-test interference.
///
/// **Not** intended for production use.
#[doc(hidden)]
pub fn reset_for_tests() {
    *MCP_SERVICE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    *SEARCH_CONFIG
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}
