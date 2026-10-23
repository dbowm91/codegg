//! A tiny, dependency-free crate for reasoning about bounded numeric spans.
//!
//! The crate is split into three modules:
//!
//! * [`range`] — the [`Range`] type, a closed interval of integers;
//! * [`measure`] — measurements taken over a [`Range`];
//! * [`spec`] — parsing of textual `lower..upper` specifications.
//!
//! # Public API contract
//!
//! The names, signatures and semantics documented on the public items below
//! are part of this crate's contract. The test suite in `tests/` relies on
//! them, so they must not be renamed, removed or re-shaped; only their
//! *behaviour* may change, and only so that it matches the documented
//! contract.

pub mod measure;
pub mod range;
pub mod spec;

pub use range::Range;
pub use spec::{parse_spec, Spec, SpecError};
