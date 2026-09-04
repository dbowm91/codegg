//! Compatibility exports for the pre-convergence compaction module.
//!
//! Production ownership lives in [`crate::context::compaction`].  This module
//! remains as a bounded adapter so integrations using the historical public
//! path continue to compile while they migrate to the canonical context API.

pub use crate::context::compaction::*;
