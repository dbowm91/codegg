//! Consolidated permissions, approval, sandbox containment, and the security
//! review surface integration test family (M004).
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

#[path = "approval_reviewer.inc"]
mod approval_reviewer;

#[path = "doom_loop.inc"]
mod doom_loop;

#[path = "linux_bubblewrap_first_hop.inc"]
mod linux_bubblewrap_first_hop;

#[path = "linux_cgroup_v2_feasibility.inc"]
mod linux_cgroup_v2_feasibility;

#[path = "linux_native_scratch_containment.inc"]
mod linux_native_scratch_containment;

#[path = "path_traversal.inc"]
mod path_traversal;

#[path = "permission.inc"]
mod permission;

#[path = "preflight_integration.inc"]
mod preflight_integration;

#[path = "sandbox_containment.inc"]
mod sandbox_containment;

#[path = "sandbox_landlock.inc"]
mod sandbox_landlock;

#[path = "sandbox_policy_wiring.inc"]
mod sandbox_policy_wiring;

#[path = "security_review_panel.inc"]
mod security_review_panel;

#[path = "security_review_receipt.inc"]
mod security_review_receipt;

#[path = "security_review_runner.inc"]
mod security_review_runner;

#[path = "ssrf.inc"]
mod ssrf;

#[path = "windows_native_probe_contract.inc"]
mod windows_native_probe_contract;

#[path = "windows_process_launcher.inc"]
mod windows_process_launcher;

#[path = "windows_projection_feasibility.inc"]
mod windows_projection_feasibility;

#[path = "windows_sandbox_adversarial.inc"]
mod windows_sandbox_adversarial;
