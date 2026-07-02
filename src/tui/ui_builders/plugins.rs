//! First-party plugin info builders.
//!
//! Re-exports the existing plugin management builders from
//! `crate::plugin::management_ui` for first-party callers that need to
//! produce UiNode trees for plugin content.

pub use crate::plugin::management_ui::{
    doctor_report_node, node_to_lines, plugin_info_node, plugins_table,
};
