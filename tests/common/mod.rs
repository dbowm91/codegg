//! Shared helpers for integration tests.
//!
//! Each integration test file in `tests/` should declare
//! `pub mod common;` at the top to pick up the helpers in this module
//! tree. The `common` name is significant: Cargo treats any
//! subdirectory of `tests/` that is not the name of an integration
//! test binary as a normal module subtree, and `common` is the
//! conventional name to avoid colliding with target names.
//!
//! The binding must be **public**. Everything below is `pub`, and this
//! tree carries no blanket `allow(dead_code)`: a private `mod common;`
//! makes those items unreachable from the binding crate root, and the
//! lint reports every one of them as dead code under `-D warnings`. The
//! families and the standalone targets alike bind it publicly for that
//! reason.

pub mod blackboard;
pub mod checkout_reservation;
pub mod eggwork_node;
pub mod linked_worktree;
pub mod pool;
pub mod projection_replay;
pub mod scripted_provider;
pub mod secret_scan;
