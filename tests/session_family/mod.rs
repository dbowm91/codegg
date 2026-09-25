//! Consolidated session integration test family (M004).
//!
//! History: each module lives at `tests/session_family/<name>.inc`
//! and was previously a separate `tests/session_<name>.rs` file (5
//! binaries total). The same Cargo trick that powers the M003 pilot
//! (`tests/<family>/mod.rs` parent + explicit `[[test]]` entry +
//! `<name>.inc` siblings to suppress per-file auto-discovery) is in
//! use here without modification. The shared helper module
//! `tests/common/mod.rs` is bound via `#[path]` because the
//! conventional `mod common;` from the original files is now a
//! sibling of the consolidated binary rather than a sibling of the
//! original file-level integration test.
//!
//! Per M004, this entry is the only file at `tests/session_family/`
//! top level. Tests/assertions/heavy-binary semantics are
//! preserved verbatim: selection and controller-lease tests retain
//! their bounded `tokio::time::sleep` polling; everything else is
//! pure storage or pure projection logic.

#[path = "../common/mod.rs"]
mod common;

#[path = "control_m004_controller_lease.inc"]
mod control_m004_controller_lease;
#[path = "crud.inc"]
mod crud;
#[path = "projection_consumer.inc"]
mod projection_consumer;
#[path = "projection_m4_controller.inc"]
mod projection_m4_controller;
#[path = "selection.inc"]
mod selection;
