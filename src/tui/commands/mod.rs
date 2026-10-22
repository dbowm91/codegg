pub(super) mod agent_editor;
pub(super) mod agents;
pub(super) mod change_review;
pub(super) mod chat;
pub(super) mod control;
pub(super) mod diagnostics;
pub(super) mod editor;
pub(super) mod exit_control;
pub(super) mod file_tree;
pub(super) mod git_sidebar;
pub(super) mod goals;
pub(super) mod import;
pub(super) mod interactive_terminal;
pub(super) mod manifest_restore;
pub(super) mod memory;
pub(super) mod observe;
pub(super) mod panels;
pub(super) mod plugin_management;
pub(super) mod plugins;
/// M007 runtime-policy commands. Public so the binary startup path can
/// enqueue the restore snapshot and so the `TuiCommand` policy variants
/// (which name [`policy::PolicySnapshotReason`]) stay well-formed.
pub mod policy;
pub(super) mod presence;
pub(super) mod primary_projection;
pub(super) mod project_catalog;
pub(super) mod project_init;
pub(super) mod project_picker;
pub(super) mod prompt;
pub(super) mod provider_connections;
/// M004: the `/queue` box — open, refresh, and delete one waiting prompt.
/// Public so the `TuiCommand` delete-completion variant and the integration
/// target can both reach the applier without a test-only App shim.
pub mod queue;
pub(super) mod research;
pub(super) mod run_rerun;
pub(super) mod security;
pub(super) mod session_selection;
pub(super) mod sessions;
pub(super) mod shell;
/// C007: the composer's subagent addressee and the `TurnSteer` it
/// issues. Public so the `TuiCommand` completion variant and the
/// integration target can both reach the applier without a test-only
/// App shim, matching `queue`.
pub mod subagent_steer;
pub(super) mod tasks;
pub(super) mod team;
pub(super) mod test;
pub(super) mod turn_control;
pub(super) mod work_orders;
pub(super) mod workspace_dashboard;

#[cfg(test)]
pub(crate) use tasks::{resolve_schedule_id, schedule_display_id, schedule_label};
