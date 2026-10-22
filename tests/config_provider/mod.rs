//! Consolidated configuration, providers, the search backends, and MCP
//! integration test family (M004).
//!
//! History: each member lived as its own `tests/<NAME>.rs` file and was
//! therefore its own Cargo test executable — N executables for N files.
//! The `.inc` extension is what stops Cargo's `tests/<dir>/*.rs`
//! auto-discovery from generating one executable per file, and
//! `tests/<dir>/mod.rs` is the single integration target. Cargo does not
//! auto-discover `tests/<dir>/mod.rs` either, so the target is declared
//! explicitly in the root `Cargo.toml`. Each member is included with
//! `#[path]`, the pattern the existing `tests/projection_replay/` and
//! `tests/session_family/` families already use.
//!
//! Consolidation replaces N separate Cargo test executables with 1,
//! preserving every test body and every assertion. The only visible change
//! is the test id, which becomes `<family>::<member>::<test>`.

#[path = "config_integration.inc"]
pub mod config_integration;

#[path = "eggsearch_pair_compat.inc"]
pub mod eggsearch_pair_compat;

#[path = "eggsearch_real_compat.inc"]
pub mod eggsearch_real_compat;

#[path = "fake_eggsearch_mcp.inc"]
pub mod fake_eggsearch_mcp;

#[path = "installation_m002_qualification.inc"]
pub mod installation_m002_qualification;

#[path = "mcp.inc"]
pub mod mcp;

#[path = "mcp_reconnect.inc"]
pub mod mcp_reconnect;

#[path = "provider_mock.inc"]
pub mod provider_mock;

#[path = "search_backend_arg_mapping.inc"]
pub mod search_backend_arg_mapping;

#[path = "search_backend_eggsearch.inc"]
pub mod search_backend_eggsearch;

#[path = "search_runtime_isolation.inc"]
pub mod search_runtime_isolation;

#[path = "upgrade.inc"]
pub mod upgrade;
