//! Consolidated the fake LSP server harness integration test family (M004).
//!
//! History: each member lived as its own `tests/<NAME>.rs` file and was
//! therefore its own Cargo test executable — 4 executables for 4 files.
//! The `.inc` extension is what stops Cargo's `tests/<dir>/*.rs`
//! auto-discovery from generating one executable per file, and
//! `tests/<dir>/mod.rs` is the single integration target. Cargo does not
//! auto-discover `tests/<dir>/mod.rs` either, so the target is declared
//! explicitly in the root `Cargo.toml`. Each member is included with
//! `#[path]`, the pattern the existing `tests/projection_replay/` and
//! `tests/session_family/` families already use.
//!
//! Consolidation replaces 4 separate Cargo test executables with 1,
//! preserving every test body and every assertion. The only visible change
//! is the test id, which becomes `<family>::<member>::<test>`.

#[path = "document_lsp_integration.inc"]
mod document_lsp_integration;

#[path = "lsp_completion_trajectory.inc"]
mod lsp_completion_trajectory;

#[path = "lsp_composite_stdio.inc"]
mod lsp_composite_stdio;

#[path = "lsp_read_delivery_m013.inc"]
mod lsp_read_delivery_m013;
