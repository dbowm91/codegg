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
///
/// C002: at most one `FileTree` task is live — the previous async waiter is
/// aborted and its blocking scan is cooperatively cancelled before the new
/// walk spawns, so rapid refreshes cannot accumulate workers. The scan
/// itself runs on `spawn_blocking` (blocking `read_dir` must never occupy a
/// Tokio worker), checks its cancel flag on every entry, and is still
/// generation-guarded at completion, so a stale walk never renders against
/// the wrong workspace.
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

    // Remote locality: the tree walks the local filesystem directly, so a
    // root that is not a proven-local directory fails closed here instead
    // of walking a same-name path as if it were authoritative. Remote
    // listing stays unsupported until a daemon-authorized transport exists.
    if let Err(error) = crate::tui::file_tree::local_walk_root(&root) {
        app.file_tree_state
            .apply_error(generation, request_id, &root, error.clone());
        app.file_tree_state.cancel_in_flight();
        app.messages_state
            .toasts
            .warning(&format!("Cannot list files: {error}"));
        return;
    }

    // Hard concurrency limit: one live `FileTree` task. Aborts the previous
    // async waiter; its blocking scan (if already running) exits at its
    // next entry via the flag `begin_rebuild` just set.
    app.task_registry.cancel_kind(TuiTaskKind::FileTree);

    // The UI must never synchronously walk: with no command channel there
    // is nowhere to report back, and a blocking walk here would hang the
    // event loop. Fail explicitly instead of leaving the pane in `Loading`.
    let Some(tx) = app.tui_cmd_tx.clone() else {
        app.file_tree_state.apply_error(
            generation,
            request_id,
            &root,
            "file-tree refresh requires a running command channel; retry when the event loop is up"
                .to_string(),
        );
        app.file_tree_state.cancel_in_flight();
        return;
    };
    let cancel = app.file_tree_state.take_cancel_flag();
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
            // Blocking directory I/O stays off Tokio workers. The inner
            // scan still honors the cooperative flag, so aborting this
            // waiter strands at most one more entry of work.
            let walk_root = root.clone();
            let listing = tokio::task::spawn_blocking(move || {
                let no_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let flag = cancel.as_ref().unwrap_or(&no_cancel);
                crate::tui::file_tree::build_tree_with_cancel(&walk_root, flag)
            })
            .await
            .unwrap_or_else(|_| crate::tui::file_tree::TreeListing {
                truncated: true,
                truncated_reason: Some("tree walk task was aborted".to_string()),
                ..Default::default()
            });
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
        return;
    }
    // Mirror the accepted walk into a mounted `/files` box. It holds its own
    // copy of the state (a widget cannot reach `App` from `render`), so
    // without this a refresh would repaint the Ctrl+T pane and leave the box
    // showing the previous listing. Cloned only when a box is actually open —
    // the walk that produced this state is far more expensive than the copy.
    if app
        .focus_manager
        .has_dialog(crate::tui::components::component::DialogType::Files)
    {
        let snapshot = app.file_tree_state.clone();
        app.focus_manager
            .with_dialog_mut::<crate::tui::components::dialogs::files::FilesDialog, ()>(
                crate::tui::components::component::DialogType::Files,
                |dialog| dialog.state = snapshot,
            );
    }
}

/// Move the tree selection.
pub fn move_selection(app: &mut App, delta: isize) {
    app.file_tree_state.move_selection(delta);
}

/// Enter the selected directory: `l`/right arrow.
///
/// Expand it and step the cursor onto its first child. Distinct from
/// [`toggle_selected`], which is what Enter uses — both expand the same
/// directory, but only this one moves the cursor.
pub fn enter_selected(app: &mut App) {
    app.file_tree_state.enter_selected();
}

/// Go back one level: `h`/left arrow.
///
/// Collapse the selected directory in place, or step to the parent when the
/// selection is not an open directory.
pub fn leave_selected(app: &mut App) {
    app.file_tree_state.leave_selected();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::file_tree::FileTreeStatus;
    use crate::tui::app::state::project_tabs::{ProjectTabId, ProjectTabState};

    fn app_with_root(root: &std::path::Path) -> App {
        let mut app = App::new_for_testing(root.display().to_string());
        let mut tab = ProjectTabState::empty(ProjectTabId::new(), "project".into());
        tab.project_id = Some("project-test".into());
        tab.workspace_id = Some("workspace-test".into());
        tab.workspace_root = Some(root.to_path_buf());
        app.project_tabs.add_and_activate(tab);
        app
    }

    #[test]
    fn rebuild_without_a_command_channel_fails_explicitly_without_walking() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("file.txt"), b"x").expect("write");
        let mut app = app_with_root(root.path());
        assert!(app.tui_cmd_tx.is_none());

        rebuild(&mut app);

        // Explicit error, not a synchronous walk and not a stuck `Loading`.
        let FileTreeStatus::Error(message) = &app.file_tree_state.status else {
            panic!(
                "expected an explicit error, got {:?}",
                app.file_tree_state.status
            );
        };
        assert!(
            message.contains("command channel"),
            "the error must say what is missing, got: {message}"
        );
        assert!(
            app.file_tree_state.nodes.is_empty(),
            "no walk may run on the event loop"
        );
    }

    #[test]
    fn rebuild_refuses_a_non_local_root_instead_of_walking_it() {
        let missing = std::path::PathBuf::from("/nonexistent-workspace-root-for-test");
        let mut app = app_with_root(&missing);

        rebuild(&mut app);

        let FileTreeStatus::Error(message) = &app.file_tree_state.status else {
            panic!(
                "a missing root must fail closed, got {:?}",
                app.file_tree_state.status
            );
        };
        assert!(
            message.contains("refusing"),
            "the error must say the root was refused, got: {message}"
        );
        assert!(app.file_tree_state.nodes.is_empty());
        assert_eq!(app.task_registry.active_count(), 0, "no walk may spawn");
    }

    #[tokio::test]
    async fn a_completion_from_before_a_project_switch_is_refused() {
        let first = tempfile::tempdir().expect("tempdir");
        std::fs::write(first.path().join("from-first.txt"), b"x").expect("write");
        let second = tempfile::tempdir().expect("tempdir");
        std::fs::write(second.path().join("from-second.txt"), b"x").expect("write");

        let mut app = app_with_root(first.path());
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);
        rebuild(&mut app);
        let first_completion = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("the first walk completes")
            .expect("channel open");
        let crate::tui::app::TuiCommand::FileTreeListed {
            request_id,
            generation,
            root,
            listing,
        } = first_completion
        else {
            panic!("expected a FileTreeListed completion");
        };

        // Switch projects and rebuild: the generation moves on, so the walk
        // for the previous workspace is stale even though it completed.
        let mut tab = ProjectTabState::empty(ProjectTabId::new(), "second".into());
        tab.workspace_root = Some(second.path().to_path_buf());
        app.project_tabs.add_and_activate(tab);
        rebuild(&mut app);

        apply_listing(&mut app, request_id, generation, root, listing);
        assert!(
            app.file_tree_state
                .rows()
                .iter()
                .all(|row| row.name != "from-first.txt"),
            "a walk from before the switch must never render"
        );
    }

    #[tokio::test]
    async fn a_hundred_rapid_rebuilds_keep_one_live_task() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("file.txt"), b"x").expect("write");
        let mut app = app_with_root(root.path());
        let (tx, _rx) = tokio::sync::mpsc::channel(256);
        app.tui_cmd_tx = Some(tx);
        // Keep the receiver alive so completions can land; capacity holds
        // all 100 without the test needing to pump the loop.

        let before = app.file_tree_state.generation;
        for _ in 0..100 {
            rebuild(&mut app);
        }
        assert_eq!(app.file_tree_state.generation, before + 100);
        assert_eq!(
            app.task_registry.active_count(),
            1,
            "the hard concurrency limit is one live FileTree task"
        );
    }
}
