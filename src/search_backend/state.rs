//! Global state shared between the search/fetch wrapper tools and the
//! agent loop's MCP plumbing.
//!
//! The websearch/webfetch tools are registered in
//! `ToolRegistry::with_defaults()` early in the process, before the
//! `McpService` exists. They therefore read their backing service and
//! the resolved `SearchConfig` from shared slots that are populated at
//! startup time (after the config has been loaded and any required
//! MCP servers have been bootstrapped).
//!
//! This is intentionally global so we do not have to thread an
//! `Arc<...>` through every layer of the tool registry's `Box<dyn Tool>`
//! storage. The values are read-only after startup; tests can
//! override the config explicitly.

use std::sync::Arc;
use std::sync::RwLock as StdRwLock;

use tokio::sync::RwLock;

use crate::config::schema::SearchConfig;
use crate::mcp::McpService;

static MCP_SERVICE: StdRwLock<Option<Arc<RwLock<McpService>>>> = StdRwLock::new(None);
static SEARCH_CONFIG: StdRwLock<Option<SearchConfig>> = StdRwLock::new(None);

/// Install the process-wide `McpService` reference. Called once at
/// startup after the service is constructed and eggsearch is
/// bootstrapped.
pub fn install_mcp_service(svc: Arc<RwLock<McpService>>) {
    let mut guard = MCP_SERVICE.write().expect("MCP_SERVICE lock poisoned");
    *guard = Some(svc);
    eprintln!(
        "DBG install_mcp_service pid={} tid={:?} result=Some",
        std::process::id(),
        std::thread::current().id()
    );
}

/// Returns the installed `McpService`, if any.
pub fn mcp_service() -> Option<Arc<RwLock<McpService>>> {
    MCP_SERVICE
        .read()
        .expect("MCP_SERVICE lock poisoned")
        .clone()
}

/// Install the resolved `SearchConfig`. Idempotent: subsequent calls
/// overwrite the previous value (the production startup path calls
/// this exactly once, but tests may override).
pub fn install_search_config(cfg: SearchConfig) {
    let mut guard = SEARCH_CONFIG.write().expect("SEARCH_CONFIG lock poisoned");
    *guard = Some(cfg);
}

/// Returns the resolved `SearchConfig`, or a default if none has been
/// installed (e.g. in unit tests that never called bootstrap).
pub fn search_config() -> SearchConfig {
    SEARCH_CONFIG
        .read()
        .expect("SEARCH_CONFIG lock poisoned")
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
    if let Ok(mut svc) = MCP_SERVICE.write() {
        *svc = None;
    }
    if let Ok(mut cfg) = SEARCH_CONFIG.write() {
        *cfg = None;
    }
    eprintln!(
        "DBG reset_for_tests pid={} tid={:?}",
        std::process::id(),
        std::thread::current().id()
    );
}
