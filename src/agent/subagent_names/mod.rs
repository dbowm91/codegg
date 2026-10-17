//! Curated human-readable names for delegated subagent runs.
//!
//! The pool itself lives in `assets/subagent-names/*.toml` and is compiled in
//! by `scripts/generate_subagent_names.py`; edit the TOML, never
//! [`generated`]. Run the generator to refresh, or pass `--check` to fail on
//! drift.
//!
//! This module provides the **name pool** and the **active-scoped registry**
//! that hands a pool name to a running subagent and releases it when that
//! subagent finishes (milestone M001 in
//! `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md`).
//!
//! Names are projection data, not client synthesis: a name is drawn once at
//! spawn, carried **in the event**, and released with its subagent, so the TUI
//! never invents one and replay reproduces the same name the live view showed.
//! See [`registry`] for why that ordering is load-bearing rather than stylistic.

mod generated;
pub mod registry;

pub use generated::{subagent_names, SubagentName, SUBAGENT_NAMES};
pub use registry::{pool_entry, SubagentNameLease, SubagentNameRegistry};
