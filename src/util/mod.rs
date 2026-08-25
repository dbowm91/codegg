pub mod clipboard;
pub mod fuzzy;
pub mod interner;
pub mod metrics;
pub mod pricing;
pub mod truncate;

pub use truncate::{truncate_bytes, truncate_lines, truncate_prefix, truncate_suffix};
