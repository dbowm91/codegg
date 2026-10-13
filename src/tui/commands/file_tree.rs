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

/// Re-walk the workspace root under single-flight admission.
///
/// C004: at most one blocking scan executes — the permit is held from
/// admission until the blocking work's completion arrives, not merely while
/// its async waiter is registered. A rebuild that arrives while a scan is
/// active supersedes intent immediately (generation bump + cancel flag) but
/// coalesces as the single pending latest intent; no new blocking work
/// spawns until the active worker exits. The active waiter is deliberately
/// **not** aborted: aborting it would strand the blocking closure without a
/// completion to drive the pending scan. The scan itself runs on
/// `spawn_blocking` (blocking `read_dir` must never occupy a Tokio worker),
/// checks its cancel flag on every entry (including iterator `Err`s), and is
/// still generation/root/request-guarded at completion, so a stale walk never
/// renders against the wrong workspace.
///
/// A slow or hung kernel syscall cannot be preempted; the newest request
/// shows an explicit `Waiting` state while the prior scan drains. The event
/// loop stays responsive and never claims a join deadline the OS does not
/// enforce.
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
    let tab_id = app.active_tab_id().map(|id| id.to_string());
    let (generation, request_id) = app.file_tree_state.begin_rebuild(root.clone());

    // Remote locality: the tree walks the local filesystem directly, so a
    // root that is not a proven-local directory fails closed here instead
    // of walking a same-name path as if it were authoritative. Remote
    // listing stays unsupported until a daemon-authorized transport exists.
    if let Err(error) = crate::tui::file_tree::local_walk_root(&root) {
        app.file_tree_state
            .apply_error(generation, request_id, &root, error.clone());
        app.file_tree_state.cancel_in_flight();
        // No valid pending: the generation bump already invalidated any
        // active scan (it will drain stale and find nothing to launch).
        app.file_tree_coordinator.clear_pending();
        app.messages_state
            .toasts
            .warning(&format!("Cannot list files: {error}"));
        return;
    }

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
        app.file_tree_coordinator.clear_pending();
        return;
    };

    let request = crate::tui::file_tree_coordinator::ScanRequest {
        generation,
        request_id,
        root: root.clone(),
        tab_id: tab_id.clone(),
    };
    match app.file_tree_coordinator.try_admit(request) {
        crate::tui::file_tree_coordinator::Admit::Run => {
            // No scan active: spawn exactly one. Reap finished tasks first so
            // the registry count reflects live workers, not retained
            // completions.
            app.task_registry.reap_finished();
            spawn_scan(app, tx, generation, request_id, root, tab_id);
        }
        crate::tui::file_tree_coordinator::Admit::Queued => {
            // A scan is active: coalesced as pending, no new spawn, no abort
            // of the active waiter (it must complete to drive the pending).
            // The pane keeps its old nodes and reports waiting explicitly.
            app.file_tree_state.status = crate::tui::app::state::file_tree::FileTreeStatus::Waiting;
            tracing::debug!("{}", app.file_tree_coordinator.diagnostics());
        }
    }
}

/// Spawn one blocking scan for an admitted request.
///
/// The blocking closure tracks actual worker entry/exit for instrumentation
/// (`running`/`peak`), honors the cooperative cancel flag, and never touches
/// admission state: `active` is cleared only on the event loop when its
/// completion arrives (so aborting the waiter never frees capacity early).
fn spawn_scan(
    app: &mut App,
    tx: tokio::sync::mpsc::Sender<crate::tui::app::TuiCommand>,
    generation: u64,
    request_id: u64,
    root: std::path::PathBuf,
    tab_id: Option<String>,
) {
    let cancel = app.file_tree_state.take_cancel_flag();
    let coordinator = app.file_tree_coordinator.clone();
    crate::tui::async_cmd::spawn_scoped_registered_tui_task(
        Some(tx),
        &mut app.task_registry,
        TuiTaskKind::FileTree,
        "file_tree_listing",
        tab_id,
        None,
        None,
        async move {
            let walk_root = root.clone();
            let listing = tokio::task::spawn_blocking(move || {
                // RAII worker accounting: released on return *and* on panic
                // unwind. Never logs paths.
                let _running = coordinator.track_enter();
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

/// Launch a taken pending scan after the active scan's completion arrives.
///
/// `pending` was already removed from the queue by `finish_active`. Revalidate
/// it against the current view (generation + root) before scanning: a reset
/// or validation failure may have made it stale. Never scans the wrong
/// workspace after a delayed permit.
fn launch_taken_pending(app: &mut App, pending: crate::tui::file_tree_coordinator::ScanRequest) {
    let current_generation = app.file_tree_state.generation;
    let current_root = app.file_tree_state.root.clone();
    let Some(current_root) = current_root else {
        // View was reset (project switch/close): drop the taken pending.
        return;
    };
    if pending.generation != current_generation || pending.root != current_root {
        // Stale pending (e.g. reset bumped generation after queueing without
        // a new rebuild): drop rather than scanning the wrong workspace.
        return;
    }
    // The pending root was validated at queue time, but re-check locality
    // before disk I/O: the filesystem may have changed while we waited.
    if let Err(error) = crate::tui::file_tree::local_walk_root(&pending.root) {
        app.file_tree_state.apply_error(
            pending.generation,
            pending.request_id,
            &pending.root,
            error,
        );
        app.file_tree_state.cancel_in_flight();
        return;
    }
    let Some(tx) = app.tui_cmd_tx.clone() else {
        app.file_tree_state.apply_error(
            pending.generation,
            pending.request_id,
            &pending.root,
            "file-tree refresh requires a running command channel; retry when the event loop is up"
                .to_string(),
        );
        app.file_tree_state.cancel_in_flight();
        return;
    };
    // Promote the taken pending to active. It is the latest generation and
    // `finish_active` just cleared the old active, so admission must return
    // `Run`; a `Queued` here would mean an unexpected active appeared between
    // `finish_active` and now (event-loop-only, so it cannot happen — fail
    // closed rather than pile up).
    match app.file_tree_coordinator.try_admit(pending.clone()) {
        crate::tui::file_tree_coordinator::Admit::Run => {
            app.file_tree_state.status = crate::tui::app::state::file_tree::FileTreeStatus::Loading;
            app.task_registry.reap_finished();
            let tab_id = app.active_tab_id().map(|id| id.to_string());
            spawn_scan(
                app,
                tx,
                pending.generation,
                pending.request_id,
                pending.root,
                tab_id,
            );
        }
        crate::tui::file_tree_coordinator::Admit::Queued => {
            // Should not happen on the event loop; keep waiting rather than
            // spawning a second worker.
            app.file_tree_state.status = crate::tui::app::state::file_tree::FileTreeStatus::Waiting;
            tracing::warn!("{}", app.file_tree_coordinator.diagnostics());
        }
    }
}

/// Apply a completed walk. A stale completion is discarded here rather than
/// rendered, which is what keeps a tree from showing a workspace the user has
/// already left.
///
/// C004: the completion also releases the single-flight permit and drives
/// the coalesced pending scan, if any. A stale completion must **not**
/// cancel the current request (`AsyncUiRequestState::cancel` bumps the
/// request id and would invalidate the queued latest intent); it is simply
/// dropped while the pending scan is launched.
pub fn apply_listing(
    app: &mut App,
    request_id: u64,
    generation: u64,
    root: PathBuf,
    listing: crate::tui::file_tree::TreeListing,
) {
    let applied = app
        .file_tree_state
        .apply_listing(generation, request_id, root, listing);
    // Release the permit even for a stale completion: the active worker has
    // exited (its `RunningGuard` already decremented `running`), and the
    // pending latest intent — if any — is now eligible to run. A late
    // completion after shutdown force-clear mismatches and yields `None`.
    // Single call: `finish_active` clears the active scan and takes the
    // pending in one lock hold; `launch_taken_pending` revalidates it.
    if let Some(pending) = app
        .file_tree_coordinator
        .finish_active(generation, request_id)
    {
        launch_taken_pending(app, pending);
    }
    if !applied {
        // Stale: deliberately no `request.cancel()`. Cancelling here would
        // bump the request id past the queued latest intent and strand it.
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

    // ── C004 single-flight admission ────────────────────────────────────

    /// C004 exact worker-count proof: one held scan plus 100 rebuilds yields
    /// one active worker, one queued latest intent, an explicit `Waiting`
    /// state, and no second blocking worker. Instrumentation (coordinator
    /// `running`/`peak`), not registry count, proves the bound. No sleeps:
    /// the first completion sits unread in the channel, holding admission.
    #[tokio::test]
    async fn c004_held_scan_plus_100_rebuilds_keeps_one_worker_one_pending() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("file.txt"), b"x").expect("write");
        let mut app = app_with_root(root.path());
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        app.tui_cmd_tx = Some(tx);

        rebuild(&mut app);
        assert!(app.file_tree_coordinator.has_active());
        let first_generation = app.file_tree_state.generation;

        for _ in 0..100 {
            rebuild(&mut app);
        }
        // 1 initial + 100 superseding = 101 generations, 1 waiter, 1 pending.
        assert_eq!(app.file_tree_state.generation, first_generation + 100);
        assert_eq!(
            app.task_registry.active_count(),
            1,
            "only the first rebuild may spawn while it is held"
        );
        assert!(app.file_tree_coordinator.has_active());
        assert!(app.file_tree_coordinator.has_queued());
        assert_eq!(app.file_tree_coordinator.superseded_count(), 100);
        assert_eq!(
            app.file_tree_coordinator
                .pending()
                .expect("pending")
                .generation,
            app.file_tree_state.generation,
            "only the latest intent survives"
        );
        assert_eq!(
            app.file_tree_state.status,
            FileTreeStatus::Waiting,
            "the newest request must report waiting, not silent loading"
        );
        // No second blocking worker could have run: at most the first scan
        // entered (peak <= 1). The first completion is still unread, so its
        // worker already exited (running 0) or is still inside (running 1).
        assert!(
            app.file_tree_coordinator.running_scan_count() <= 1,
            "running must be 0 or 1, got {}",
            app.file_tree_coordinator.running_scan_count()
        );
        assert!(
            app.file_tree_coordinator.peak_running() <= 1,
            "peak blocking workers must be <= 1, got {}",
            app.file_tree_coordinator.peak_running()
        );

        // Drain the held first completion (stale) and drive the pending.
        let first = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("first completion arrives")
            .expect("channel open");
        let crate::tui::app::TuiCommand::FileTreeListed {
            request_id,
            generation,
            root: completed_root,
            listing,
        } = first
        else {
            panic!("expected a FileTreeListed completion");
        };
        apply_listing(&mut app, request_id, generation, completed_root, listing);
        // The stale first completion launched the latest pending as a new
        // active scan; the pane is loading it, not stuck waiting.
        assert!(app.file_tree_coordinator.has_active());
        assert!(!app.file_tree_coordinator.has_queued());
        assert_eq!(app.file_tree_state.status, FileTreeStatus::Loading);

        // Drain the pending scan's completion (current) and apply it.
        let second = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("pending completion arrives")
            .expect("channel open");
        let crate::tui::app::TuiCommand::FileTreeListed {
            request_id,
            generation,
            root: completed_root,
            listing,
        } = second
        else {
            panic!("expected a FileTreeListed completion");
        };
        apply_listing(&mut app, request_id, generation, completed_root, listing);
        assert_eq!(app.file_tree_state.status, FileTreeStatus::Ready);
        assert!(
            app.file_tree_state
                .rows()
                .iter()
                .any(|row| row.name == "file.txt"),
            "the latest valid tree must render"
        );
        assert!(
            app.file_tree_coordinator.peak_running() <= 1,
            "peak must stay <= 1 across the handoff, got {}",
            app.file_tree_coordinator.peak_running()
        );
        assert!(
            !app.file_tree_coordinator.diagnostics().contains("file.txt"),
            "diagnostics must not leak names"
        );
    }

    /// C004 project switch: a held A scan plus a B rebuild shows only B.
    /// Late A is discarded; `/files` parity holds because both consume the
    /// same generation-guarded state.
    #[tokio::test]
    async fn c004_switch_from_a_to_b_shows_only_b() {
        let first = tempfile::tempdir().expect("tempdir");
        std::fs::write(first.path().join("from-first.txt"), b"x").expect("write");
        let second = tempfile::tempdir().expect("tempdir");
        std::fs::write(second.path().join("from-second.txt"), b"x").expect("write");

        let mut app = app_with_root(first.path());
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);
        rebuild(&mut app);

        // Switch workspaces and rebuild before A's completion is applied:
        // B queues behind the held A scan.
        let mut tab = ProjectTabState::empty(ProjectTabId::new(), "second".into());
        tab.workspace_root = Some(second.path().to_path_buf());
        app.project_tabs.add_and_activate(tab);
        rebuild(&mut app);
        assert_eq!(app.file_tree_state.status, FileTreeStatus::Waiting);
        assert!(app.file_tree_coordinator.has_queued());

        // A's completion arrives stale and is discarded, launching B.
        let stale = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("A completes")
            .expect("channel open");
        let crate::tui::app::TuiCommand::FileTreeListed {
            request_id,
            generation,
            root,
            listing,
        } = stale
        else {
            panic!("expected a FileTreeListed completion");
        };
        apply_listing(&mut app, request_id, generation, root, listing);
        assert!(
            app.file_tree_state
                .rows()
                .iter()
                .all(|row| row.name != "from-first.txt"),
            "late A must never render after the switch"
        );

        // B's completion is current and renders.
        let current = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("B completes")
            .expect("channel open");
        let crate::tui::app::TuiCommand::FileTreeListed {
            request_id,
            generation,
            root,
            listing,
        } = current
        else {
            panic!("expected a FileTreeListed completion");
        };
        apply_listing(&mut app, request_id, generation, root, listing);
        assert!(
            app.file_tree_state
                .rows()
                .iter()
                .any(|row| row.name == "from-second.txt"),
            "B must render"
        );
        assert!(
            app.file_tree_state
                .rows()
                .iter()
                .all(|row| row.name != "from-first.txt"),
            "A must never reappear"
        );
        assert!(
            app.file_tree_coordinator.peak_running() <= 1,
            "peak must stay <= 1 across the switch"
        );
        // `/files` seeds from the same state, so it sees the same B tree.
        let dialog_rows = app.file_tree_state.rows();
        assert!(
            dialog_rows.iter().any(|row| row.name == "from-second.txt"),
            "/files and Ctrl+T share the one guarded state"
        );
    }

    /// C004 shutdown: clearing admission discards a late completion without
    /// relaunching, and never claims the worker was joined.
    #[tokio::test]
    async fn c004_shutdown_discards_late_completion_without_relaunch() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("file.txt"), b"x").expect("write");
        let mut app = app_with_root(root.path());
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);
        rebuild(&mut app);
        assert!(app.file_tree_coordinator.has_active());

        // Shut down before the completion is applied: pending cleared (none),
        // active force-cleared, waiters aborted. The blocking worker may still
        // be inside its syscall; shutdown does not join it.
        app.prepare_shutdown();
        assert!(!app.file_tree_coordinator.has_active());
        assert!(!app.file_tree_coordinator.has_queued());

        // A late completion (if the waiter sent before abort, or the test
        // synthesizes one) mismatches and is discarded with no relaunch.
        let late = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await;
        if let Ok(Some(msg)) = late {
            let crate::tui::app::TuiCommand::FileTreeListed {
                request_id,
                generation,
                root,
                listing,
            } = msg
            else {
                panic!("expected a FileTreeListed completion");
            };
            let active_before = app.task_registry.active_count();
            apply_listing(&mut app, request_id, generation, root, listing);
            assert!(
                !app.file_tree_coordinator.has_active(),
                "shutdown must not resurrect a scan"
            );
            assert!(
                !app.file_tree_coordinator.has_queued(),
                "shutdown must not queue"
            );
            assert_eq!(
                app.task_registry.active_count(),
                active_before,
                "no new worker may spawn after shutdown"
            );
        }
        assert!(
            app.file_tree_coordinator.peak_running() <= 1,
            "peak must stay <= 1 through shutdown"
        );
    }

    /// C004 panic path: a truncated panic-converted completion still releases
    /// the permit and drives the queued latest intent. The panic itself is
    /// proven to release `running` in the coordinator unit test; here the
    /// truncated listing proves the event-loop handoff.
    #[tokio::test]
    async fn c004_truncated_panic_completion_still_drives_pending() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("file.txt"), b"x").expect("write");
        let mut app = app_with_root(root.path());
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);

        rebuild(&mut app);
        assert!(!app.file_tree_coordinator.has_queued());
        // Capture the active request before queueing the next.
        let (first_gen, first_req) = {
            // Read from the state (generation/request just minted).
            (
                app.file_tree_state.generation,
                app.file_tree_state.request.request_id(),
            )
        };
        rebuild(&mut app);
        assert!(app.file_tree_coordinator.has_queued());

        // Simulate the waiter's panic conversion: same fence as the real
        // completion, but a truncated listing. Stale, discarded, yet it must
        // still launch the pending scan.
        let panic_listing = crate::tui::file_tree::TreeListing {
            truncated: true,
            truncated_reason: Some("tree walk task was aborted".to_string()),
            ..Default::default()
        };
        // Drain the real first completion unread and drop it; drive with the
        // synthetic truncated one instead (same fence).
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("first completion arrives");
        apply_listing(
            &mut app,
            first_req,
            first_gen,
            root.path().to_path_buf(),
            panic_listing,
        );
        assert!(
            app.file_tree_coordinator.has_active(),
            "a panic-converted completion must still promote the pending scan"
        );

        // The promoted scan completes normally.
        let second = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("pending completes")
            .expect("channel open");
        let crate::tui::app::TuiCommand::FileTreeListed {
            request_id,
            generation,
            root: completed_root,
            listing,
        } = second
        else {
            panic!("expected a FileTreeListed completion");
        };
        apply_listing(&mut app, request_id, generation, completed_root, listing);
        assert_eq!(app.file_tree_state.status, FileTreeStatus::Ready);
        assert!(app.file_tree_coordinator.peak_running() <= 1);
    }
}
