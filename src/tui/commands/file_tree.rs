//! M006-D: file-tree commands.
//!
//! Every entry point here resolves authority from the active project tab
//! through `project_execution_context()` and then hands the resulting path to
//! the existing [`crate::tui::commands::editor::open_editor`]. This module
//! deliberately has no path-to-document code of its own: the tree is a
//! navigator, and `open_editor` is the only thing that turns a path into a
//! document attachment.

use std::path::PathBuf;

use crate::tui::app::App;
use crate::tui::file_tree::TreeNodeKind;
use crate::tui::task_lifecycle::TuiTaskKind;

use super::editor::open_editor;

/// Show or hide the tree pane, rebuilding it when it becomes visible.
///
/// Hiding keeps the last listing so re-showing is instant; only an explicit
/// refresh re-walks.
pub fn toggle_tree(app: &mut App) {
    let visible = !app.file_tree_state.visible;
    app.file_tree_state.visible = visible;
    app.ui_state.tree_focused = visible;
    if !visible {
        app.file_tree_state.selected = None;
        return;
    }
    if app.file_tree_state.rows().is_empty() {
        rebuild(app);
    }
}

/// Re-walk the workspace root, replacing any walk already in flight.
pub fn rebuild(app: &mut App) {
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(error) => {
            app.messages_state.toasts.warning(&format!(
                "Cannot list files: {error}. Select a project tab first."
            ));
            return;
        }
    };
    let root = context.workspace_root.clone();
    let (generation, request_id) = app.file_tree_state.begin_rebuild(root.clone());

    // The walk is blocking directory I/O, so it must not run on the render
    // path. With no channel there is nowhere to report back, so it runs
    // inline rather than leaving the pane stuck in `Loading` forever.
    let Some(tx) = app.tui_cmd_tx.clone() else {
        let listing = crate::tui::file_tree::build_tree(&root);
        app.file_tree_state
            .apply_listing(generation, request_id, root, listing);
        return;
    };
    let tab_id = app.active_tab_id().map(|id| id.to_string());
    crate::tui::async_cmd::spawn_scoped_registered_tui_task(
        Some(tx),
        &mut app.task_registry,
        TuiTaskKind::FileTree,
        "file_tree_listing",
        tab_id,
        None,
        None,
        async move {
            let listing = crate::tui::file_tree::build_tree(&root);
            Some(crate::tui::TuiCommand::FileTreeListed {
                request_id,
                generation,
                root,
                listing,
            })
        },
    );
}

/// Apply a completed walk. A stale completion is discarded here rather than
/// rendered, which is what keeps a tree from showing a workspace the user has
/// already left.
pub fn apply_listing(
    app: &mut App,
    request_id: u64,
    generation: u64,
    root: PathBuf,
    listing: crate::tui::file_tree::TreeListing,
) {
    if !app
        .file_tree_state
        .apply_listing(generation, request_id, root, listing)
    {
        app.file_tree_state.request.cancel();
    }
}

/// Move the tree selection.
pub fn move_selection(app: &mut App, delta: isize) {
    app.file_tree_state.move_selection(delta);
}

/// Expand or collapse the selected directory.
pub fn toggle_selected(app: &mut App) {
    app.file_tree_state.toggle_selected();
}

/// Open the selected file, or toggle the selected directory.
///
/// The open path is `open_editor` verbatim, so the tree inherits its lexical
/// validation, its already-open short-circuit, its daemon-authorized
/// attachment, and its request/generation discipline without duplicating any
/// of them.
pub fn activate_selected(app: &mut App) {
    let Some(row) = app.file_tree_state.selected_row() else {
        return;
    };
    if row.kind == TreeNodeKind::Directory {
        app.file_tree_state.toggle_selected();
        return;
    }
    // A directory is expandable, a file is openable. Anything else is not a
    // navigable entry and must not reach the open path.
    open_editor(app, &row.path);
}

/// Move focus into or out of the tree.
///
/// Focus leaves the tree on `Esc`, which keeps the non-modal contract: the
/// composer is reachable again without a route change.
pub fn set_focus(app: &mut App, focused: bool) {
    let focused = focused && app.file_tree_state.visible;
    app.ui_state.tree_focused = focused;
    if focused {
        crate::tui::commands::editor::focus_editor_composer(&mut app.editor_state);
    } else {
        crate::tui::commands::editor::focus_editor_buffer(&mut app.editor_state);
    }
}
