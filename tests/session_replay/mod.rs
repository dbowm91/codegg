//! Recorded-session replay: one member per recorded scenario.
//!
//! A fixture in `tests/session_replay/fixtures/` is one recorded turn — a
//! schema version, a scenario identity, the user input, the ordered scripted
//! responses, and the durable outcome the turn must reproduce. A member drives
//! that fixture through the shipped agent loop with no provider credential and
//! no network, reads the session back through a reopened store handle, and
//! compares the durable result against the fixture's expectation.
//!
//! Why the family exists: a change to prompt assembly, tool admission,
//! projection or compaction should fail a keyless test rather than ship. The
//! comparison is over semantic fields only (role, text, tool name, normalized
//! arguments, tool outcome class, terminal state, request count, and the
//! message-role shape of each request); timestamps, non-fixture row ids and
//! durations are excluded on purpose.
//!
//! The family is **not** `required-features`-gated. Nine root integration
//! targets are gated today and no CI workflow enables those features, so 149
//! test functions are never compiled there; a gated replay family would join
//! them and the tier would be decoration.
//!
//! Members are `.inc` files so Cargo's `tests/<dir>/*.rs` discovery does not
//! turn each one into its own executable; this `mod.rs` is the single target,
//! declared in the root `Cargo.toml`.

// The shared helper tree lives one directory up, so a family root binds it by
// path. The binding is `pub`, as the tree requires: its items are `pub`, and a
// private binding would make them unreachable from this crate root.
#[path = "../common/mod.rs"]
pub mod common;

#[path = "fixture_format.inc"]
pub mod fixture_format;

#[path = "recorder.inc"]
pub mod recorder;

#[path = "runner_contract.inc"]
pub mod runner_contract;

#[path = "scenarios.inc"]
pub mod scenarios;
