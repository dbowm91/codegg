//! Consolidated projection-replay integration test family (M003 pilot).
//!
//! History: each module lives at `tests/projection_replay/<name>.inc`
//! and was previously a separate `tests/projection_replay_<name>.rs`
//! file (11 binaries total). The `.inc` extension is the M003 trick
//! that prevents Cargo's "tests/<dir>/*.rs" auto-discovery from
//! generating per-file test executables; the
//! `tests/projection_replay/mod.rs` parent is auto-discovered by Cargo
//! as the single integration target named `projection_replay`. Each
//! `<name>.inc` is included via `#[path]` so the convention stays
//! explicit. Consolidation replaces 11 separate Cargo test executables
//! with 1, preserving every test body, every assertion, and every
//! Nextest-visible module path. The M003 pilot proves that the
//! repository's link-count bottleneck is removable for one compatible
//! family; M004 conditionally broadens the pattern.
//!
//! Per the M003 plan, this entry is the only file at
//! `tests/projection_replay/`'s top level. `common` (the conventional
//! shared helper module for integration tests) is referenced via an
//! explicit `#[path]` because it now lives one directory up under
//! `tests/common/mod.rs` rather than as a sibling of the consolidated
//! target.

#[path = "../common/mod.rs"]
mod common;

#[path = "daemon_protocol.inc"]
mod daemon_protocol;
#[path = "failpoint.inc"]
mod failpoint;
#[path = "publication_integration.inc"]
mod publication_integration;
#[path = "restart_recovery.inc"]
mod restart_recovery;
#[path = "resume.inc"]
mod resume;
#[path = "retention.inc"]
mod retention;
#[path = "safe_publication.inc"]
mod safe_publication;
#[path = "storage.inc"]
mod storage;
#[path = "stream_context.inc"]
mod stream_context;
#[path = "subscription.inc"]
mod subscription;
#[path = "transport_isolation.inc"]
mod transport_isolation;
