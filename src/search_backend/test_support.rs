//! Shared test support for `search_backend` integration tests.
//!
//! `search_backend::state` holds a process-global `McpService` and
//! `SearchConfig` slot. Integration tests across multiple test files
//! (and across multiple test binaries) mutate those slots, so they
//! must serialize against each other. Each test binary has its own
//! `static` items, so per-file `AsyncMutex` instances cannot protect
//! cross-binary races. This module exposes a single shared
//! `tokio::sync::Mutex` that all integration tests must acquire before
//! touching the global state.

use tokio::sync::Mutex;

/// Shared async mutex serializing all integration tests that read or
/// write `search_backend::state::MCP_SERVICE` /
/// `SEARCH_CONFIG`. Acquire with `.lock().await` and hold the guard
/// for the entire body of the test (including any `.await` on
/// `dispatch_*` or `AgentLoop::build_tool_definitions`).
pub static SHARED_TEST_LOCK: Mutex<()> = Mutex::const_new(());
