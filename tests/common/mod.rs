//! Shared helpers for integration tests.
//!
//! Each integration test file in `tests/` should declare
//! `mod common;` at the top to pick up the helpers in this module
//! tree. The `common` name is significant: Cargo treats any
//! subdirectory of `tests/` that is not the name of an integration
//! test binary as a normal module subtree, and `common` is the
//! conventional name to avoid colliding with target names.

pub mod pool;
