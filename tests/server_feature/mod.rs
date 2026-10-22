//! Consolidated the HTTP server surface and the transport identity gates integration test family (M004).
//!
//! History: each member lived as its own `tests/<NAME>.rs` file and was
//! therefore its own Cargo test executable — 5 executables for 5 files.
//! The `.inc` extension is what stops Cargo's `tests/<dir>/*.rs`
//! auto-discovery from generating one executable per file, and
//! `tests/<dir>/mod.rs` is the single integration target. Cargo does not
//! auto-discover `tests/<dir>/mod.rs` either, so the target is declared
//! explicitly in the root `Cargo.toml`. Each member is included with
//! `#[path]`, the pattern the existing `tests/projection_replay/` and
//! `tests/session_family/` families already use.
//!
//! Consolidation replaces 5 separate Cargo test executables with 1,
//! preserving every test body and every assertion. The only visible change
//! is the test id, which becomes `<family>::<member>::<test>`.
//!
//! `common` — the shared integration-test helper module — lives one
//! directory up under `tests/common/mod.rs`, so it is bound here with an
//! explicit `#[path]`; the members reach it with `use super::common;`.

#[path = "../common/mod.rs"]
mod common;

#[path = "identity_m002_transport_auth.inc"]
mod identity_m002_transport_auth;

#[path = "projection_transport_real.inc"]
mod projection_transport_real;

#[path = "team_collaboration_m001_http_auth.inc"]
mod team_collaboration_m001_http_auth;

#[path = "team_collaboration_m006_trajectory.inc"]
mod team_collaboration_m006_trajectory;

#[path = "team_collaboration_postclosure_m001_registration_auth.inc"]
mod team_collaboration_postclosure_m001_registration_auth;
