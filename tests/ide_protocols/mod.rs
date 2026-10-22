//! Consolidated ACP, LSP, and document client protocol surfaces integration test family (M004).
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

#[path = "acp_stdio.inc"]
mod acp_stdio;

#[path = "document_client_trajectory.inc"]
mod document_client_trajectory;

#[path = "lsp.inc"]
mod lsp;

#[path = "lsp_read_surface_trajectory.inc"]
mod lsp_read_surface_trajectory;

#[path = "phase5_context_integration.inc"]
mod phase5_context_integration;
