//! Generated tool-surface catalog (harness-practice-adoption M004).
//!
//! The model-facing tool surface is emitted by **constructing the real
//! registry**, committed as `architecture/tool-catalog.md`, and compared on
//! every sweep. Adding, removing, or re-gating a tool therefore cannot leave
//! the documentation describing a surface the binary does not have.
//!
//! Two layers, deliberately split by cost:
//!
//! - this family is the authoritative gate. It renders the catalog from
//!   `ToolRegistry::with_options` and fails when the committed file differs
//!   (`comparison.inc`).
//! - `scripts/check_tool_catalog.py` is the cheap structural guard that runs in
//!   `verify.sh quick` and CI, where no test runs. It checks the file's shape
//!   and its agreement with `architecture/agent-tool-surface.md`'s remaining
//!   prose, and deliberately does **not** re-derive the registry.
//!
//! Members are `.inc` files so Cargo's `tests/<dir>/*.rs` discovery does not
//! turn each one into its own executable; this `mod.rs` is the single
//! integration target, declared in the root `Cargo.toml`. The family declares
//! no `required-features` gate: a gate is how a tier stops running.

#[path = "render.inc"]
pub mod render;

#[path = "comparison.inc"]
pub mod comparison;
