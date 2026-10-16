//! Curated human-readable names for delegated subagent runs.
//!
//! The pool itself lives in `assets/subagent-names/*.toml` and is compiled in
//! by `scripts/generate_subagent_names.py`; edit the TOML, never
//! [`generated`]. Run the generator to refresh, or pass `--check` to fail on
//! drift.
//!
//! This module currently provides the **name pool only**. Assignment of a name
//! to a running subagent — the active-scoped registry that holds a name while
//! its subagent works and releases it on completion, appending `-2`, `-3`, …
//! when a pool entry is already taken — is the deliverable of milestone M001 in
//! `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md` and is not
//! implemented yet. Nothing here mints a name on its own.
//!
//! Names are projection data, not client synthesis: a name is carried in the
//! projection and released with its subagent, so the TUI never invents one.

mod generated;

pub use generated::{subagent_names, SubagentName, SUBAGENT_NAMES};