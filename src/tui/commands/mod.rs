pub(super) mod agents;
pub(super) mod chat;
pub(super) mod diagnostics;
pub(super) mod git_sidebar;
pub(super) mod goals;
pub(super) mod import;
pub(super) mod interactive_terminal;
pub(super) mod manifest_restore;
pub(super) mod memory;
pub(super) mod observe;
pub(super) mod plugin_management;
pub(super) mod plugins;
/// M007 runtime-policy commands. Public so the binary startup path can
/// enqueue the restore snapshot and so the `TuiCommand` policy variants
/// (which name [`policy::PolicySnapshotReason`]) stay well-formed.
pub mod policy;
pub(super) mod presence;
pub(super) mod project_catalog;
pub(super) mod project_picker;
pub(super) mod prompt;
pub(super) mod provider_connections;
pub(super) mod research;
pub(super) mod run_rerun;
pub(super) mod security;
pub(super) mod session_selection;
pub(super) mod sessions;
pub(super) mod shell;
pub(super) mod tasks;
pub(super) mod test;
pub(super) mod work_orders;

#[cfg(test)]
pub(crate) use tasks::{resolve_schedule_id, schedule_display_id, schedule_label};
