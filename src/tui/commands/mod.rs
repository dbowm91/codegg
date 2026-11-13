pub(super) mod agent_editor;
pub(super) mod agents;
pub(super) mod appearance;
pub(super) mod change_review;
pub(super) mod chat;
pub(super) mod compaction;
pub(super) mod control;
pub(super) mod diagnostics;
pub(super) mod diff;
pub(super) mod edit_history;
pub(super) mod editor;
pub(super) mod exit_control;
pub(super) mod file_tree;
pub(super) mod git_sidebar;
pub(super) mod goals;
pub(super) mod help;
pub(super) mod import;
pub(super) mod interactive_terminal;
pub(super) mod lsp;
pub(super) mod manifest_restore;
pub(super) mod memory;
pub(super) mod observe;
pub(super) mod panels;
pub(super) mod plugin_catalogue;
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
pub(super) mod promotion;
pub(super) mod prompt;
pub(super) mod provider_connections;
/// M004: the `/queue` box — open, refresh, and delete one waiting prompt.
/// Public so the `TuiCommand` delete-completion variant and the integration
/// target can both reach the applier without a test-only App shim.
pub mod queue;
/// Provider reasoning control M004: the `/reasoning` option surface and its
/// daemon write path. Public so the `TuiCommand` completion variants (which
/// name its payload types) stay well-formed.
pub mod reasoning;
pub(super) mod research;
pub(super) mod revert;
pub(super) mod run_rerun;
pub(super) mod security;
pub(super) mod session_report;
pub(super) mod session_selection;
pub(super) mod sessions;
pub(super) mod shell;
pub(super) mod skills;
/// C007: the composer's subagent addressee and the `TurnSteer` it
/// issues. Public so the `TuiCommand` completion variant and the
/// integration target can both reach the applier without a test-only
/// App shim, matching `queue`.
pub mod subagent_steer;
pub(super) mod tasks;
pub(super) mod team;
pub(super) mod test;
pub(super) mod transcript;
pub(super) mod transcript_follow;
pub(super) mod turn_control;
pub(super) mod work_orders;
pub(super) mod workspace_dashboard;

#[cfg(test)]
mod dispatch_tests;

#[cfg(test)]
pub(crate) use tasks::{resolve_schedule_id, schedule_display_id, schedule_label};

use crate::tui::app::App;

/// Leave the command palette: close command mode, clear the typed line, and
/// hide the completion popup.
///
/// The three fields are one unit — a command that closed the palette but left
/// its text in the composer would show a line that has already run — so every
/// handler that consumes the typed line performs this teardown through here.
pub(crate) fn close_command_mode(app: &mut App) {
    app.ui_state.command_mode = false;
    app.prompt_state.prompt.clear();
    app.prompt_state.show_completions = false;
}
