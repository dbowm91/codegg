//! Consolidated command intent routing and the typed git operation surface
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
//!
//! `common` — the shared integration-test helper module — lives one
//! directory up under `tests/common/mod.rs`, so it is bound here with an
//! explicit `#[path]`; the members reach it with `use super::common;`.

#[path = "../common/mod.rs"]
pub mod common;

#[path = "command_routing_adversarial.inc"]
pub mod command_routing_adversarial;

#[path = "command_routing_execution_ownership.inc"]
pub mod command_routing_execution_ownership;

#[path = "context_projection_adversarial.inc"]
pub mod context_projection_adversarial;

#[path = "git_closure_matrix.inc"]
pub mod git_closure_matrix;

#[path = "git_credential_cross_path.inc"]
pub mod git_credential_cross_path;

#[path = "git_credential_runstore_sentinel.inc"]
pub mod git_credential_runstore_sentinel;

#[path = "git_env_attack.inc"]
pub mod git_env_attack;

#[path = "git_execution_origin_matrix.inc"]
pub mod git_execution_origin_matrix;

#[path = "git_fixture_isolation.inc"]
pub mod git_fixture_isolation;

#[path = "git_hostile_metadata.inc"]
pub mod git_hostile_metadata;

#[path = "git_index_publication.inc"]
pub mod git_index_publication;

#[path = "git_mutations_integration.inc"]
pub mod git_mutations_integration;

#[path = "git_network_integration.inc"]
pub mod git_network_integration;

#[path = "git_read_consistency.inc"]
pub mod git_read_consistency;

#[path = "git_noninteractive.inc"]
pub mod git_noninteractive;

#[path = "git_recovery_integration.inc"]
pub mod git_recovery_integration;

#[path = "git_tracing_capture.inc"]
pub mod git_tracing_capture;

#[path = "policy_surface_m007.inc"]
pub mod policy_surface_m007;

#[path = "secret_scan_self_tests.inc"]
pub mod secret_scan_self_tests;

#[path = "worktree.inc"]
pub mod worktree;
