//! Curated names come from `assets/subagent-names/*.toml`; edit the TOML, never `generated`.
//! The registry leases names for active runs; see `architecture/tui.md`.

mod generated;
pub mod registry;

pub use generated::{subagent_names, SubagentName, SUBAGENT_NAMES};
pub use registry::{pool_entry, SubagentNameLease, SubagentNameRegistry};
