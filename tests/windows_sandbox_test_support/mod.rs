//! Consolidated the Windows sandbox and ACL lease integration test family (M004).
//!
//! History: each member lived as its own `tests/<NAME>.rs` file and was
//! therefore its own Cargo test executable — 2 executables for 2 files.
//! The `.inc` extension is what stops Cargo's `tests/<dir>/*.rs`
//! auto-discovery from generating one executable per file, and
//! `tests/<dir>/mod.rs` is the single integration target. Cargo does not
//! auto-discover `tests/<dir>/mod.rs` either, so the target is declared
//! explicitly in the root `Cargo.toml`. Each member is included with
//! `#[path]`, the pattern the existing `tests/projection_replay/` and
//! `tests/session_family/` families already use.
//!
//! Consolidation replaces 2 separate Cargo test executables with 1,
//! preserving every test body and every assertion. The only visible change
//! is the test id, which becomes `<family>::<member>::<test>`.

#[path = "windows_acl_lease.inc"]
mod windows_acl_lease;

#[path = "windows_restricted_managed_process.inc"]
mod windows_restricted_managed_process;
