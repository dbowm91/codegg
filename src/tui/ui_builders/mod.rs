//! First-party UiNode builders.
//!
//! These convert domain data (stats, plugin info, shell details) into
//! `UiNode` trees that can be lowered by `UiNodeRenderer`. The shared
//! renderer is what makes plugin UI and first-party UI render with the
//! same code path.

pub mod plugins;
pub mod shell;
pub mod stats;
