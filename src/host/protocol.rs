//! Re-export of the wire definition the `codegg-host-protocol` crate owns.
//!
//! The types live in `crates/codegg-host-protocol` so the host binary and the
//! SDK share one definition (ADR-0060). This module exists only so
//! `crate::host::protocol::…` keeps resolving inside the binary.

pub use codegg_host_protocol::*;
