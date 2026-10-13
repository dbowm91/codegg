//! M006-D: file-tree commands.
//!
//! C005 (ADR-0022, accepted): every entry point here resolves authority
//! from the active project tab through `project_execution_context()` and
//! then issues a daemon-owned `WorkspaceDirectoryList` request. The TUI
//! never walks its own filesystem: a same-spelled local path can describe
//! a different machine's directory, so only daemon-origin metadata is
//! displayed, keyed to the echoed daemon workspace identity plus the
//! existing generation/root/request fencing. There is no local `read_dir`
//! fallback in any mode, including standalone — an unavailable daemon
//! listing is an explicit error, never a silent local walk.
//!
//! Opening a listed entry still hands its workspace-relative path to the
//! existing [`crate::tui::commands::editor::open_editor`]. This module
//! deliberately has no path-to-document code of its own: the tree is a
//! navigator, and `open_editor` is the only thing that turns a path into a
//! document attachment (and reauthorizes it via `DocumentOpen`).

use std::path::PathBuf;

use crate::tui::app::App;
use crate::tui::file_tree::TreeNodeKind;
use crate::tui::task_lifecycle::TuiTaskKind;

use super::editor::open_editor;

/// Show or hide the tree pane, rebuilding it when it becomes visible.
///
/// Hiding keeps the last listing so re-showing is instant; only an explicit
/// refresh re-requests.
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

/// Request the daemon-owned directory listing under single-flight admission.
///
/// C004: at most one listing round-trip is in flight — the permit is held
/// from admission until the completion arrives, not merely while its async
/// waiter is registered. A rebuild that arrives while a request is active
/// supersedes intent immediately (generation bump) but coalesces as the
/// single pending latest intent; no new request issues until the active
/// one completes. The active waiter is deliberately **not** aborted:
/// aborting it would strand the pending intent without a completion to
/// drive it. Completions are still generation/root/request-guarded, so a
/// stale listing never renders against the wrong workspace.
///
/// C005: the request carries the tab's canonical project/workspace binding
/// and the response must echo the daemon's workspace identity before it is
/// displayed. A slow or failed daemon round-trip cannot preempt anything;
/// the newest request shows an explicit `Waiting` state while the prior
/// request drains. The event loop stays responsive.
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
    // The daemon resolves the canonical root from this binding; the local
    // `workspace_root` below is only fencing state (which workspace the
    // pane believes it shows), never an authority and never walked.
    let (Some(project_id), Some(workspace_id)) = (context.project_id, context.workspace_id) else {
        app.messages_state.toasts.warning(
            "Cannot list files: the active tab has no canonical project/workspace binding.",
        );
        return;
    };
    let root = context.workspace_root.clone();
    let tab_id = app.active_tab_id().map(|id| id.to_string());
    let (generation, request_id) = app.file_tree_state.begin_rebuild(root.clone());

    // The UI must never synchronously request: with no command channel there
    // is nowhere to report back, and a blocking wait here would hang the
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

    // Without an attached core client there is no daemon to ask. Fail
    // explicitly: guessing the tree from the local filesystem would display
    // client-host entries as if they were daemon workspace entries.
    let Some(core_client) = app.core_client.clone() else {
        app.file_tree_state.apply_error(
            generation,
            request_id,
            &root,
            "file-tree refresh requires an attached core client; directory listing is daemon-owned and has no local fallback".to_string(),
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
        project_id: project_id.clone(),
        workspace_id: workspace_id.clone(),
    };
    match app.file_tree_coordinator.try_admit(request) {
        crate::tui::file_tree_coordinator::Admit::Run => {
            // No request active: issue exactly one. Reap finished tasks
            // first so the registry count reflects live workers, not
            // retained completions.
            app.task_registry.reap_finished();
            spawn_listing(
                app,
                tx,
                core_client,
                generation,
                request_id,
                root,
                tab_id,
                project_id,
                workspace_id,
            );
        }
        crate::tui::file_tree_coordinator::Admit::Queued => {
            // A request is active: coalesced as pending, no new issue, no
            // abort of the active waiter (it must complete to drive the
            // pending). The pane keeps its old nodes and reports waiting
            // explicitly.
            app.file_tree_state.status = crate::tui::app::state::file_tree::FileTreeStatus::Waiting;
            tracing::debug!("{}", app.file_tree_coordinator.diagnostics());
        }
    }
}

/// Issue one daemon listing round-trip for an admitted request.
///
/// The future tracks actual round-trip entry/exit for instrumentation
/// (`running`/`peak`) and never touches admission state: `active` is
/// cleared only on the event loop when its completion arrives (so aborting
/// the waiter never frees capacity early).
#[allow(clippy::too_many_arguments)]
fn spawn_listing(
    app: &mut App,
    tx: tokio::sync::mpsc::Sender<crate::tui::app::TuiCommand>,
    core_client: std::sync::Arc<dyn crate::core::CoreClient>,
    generation: u64,
    request_id: u64,
    root: std::path::PathBuf,
    tab_id: Option<String>,
    project_id: String,
    workspace_id: String,
) {
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
            // Round-trip accounting: released on return *and* on panic
            // unwind. Never logs paths or entry names.
            let _running = coordinator.track_enter();
            let request = crate::core::new_request(
                format!("file-tree-{request_id}"),
                crate::protocol::core::CoreRequest::WorkspaceDirectoryList {
                    request: codegg_protocol::document::WorkspaceDirectoryListRequestDto {
                        project_id,
                        workspace_id: workspace_id.clone(),
                        prefix: String::new(),
                    },
                },
            );
            match core_client.request(request).await {
                Ok(crate::protocol::core::CoreResponse::WorkspaceDirectoryList { result }) => {
                    if result.workspace_id != workspace_id {
                        // The daemon answered for a scope this pane did not
                        // ask for (stale worker, confused deputy). Discard
                        // loudly rather than rendering foreign entries.
                        return Some(crate::tui::TuiCommand::FileTreeFailed {
                            request_id,
                            generation,
                            root,
                            message: "daemon directory listing answered for a different workspace; discarded without rendering".to_string(),
                        });
                    }
                    Some(crate::tui::TuiCommand::FileTreeListed {
                        request_id,
                        generation,
                        root,
                        listing: listing_from_daemon(&result),
                    })
                }
                Ok(crate::protocol::core::CoreResponse::Error { code, message }) => {
                    Some(crate::tui::TuiCommand::FileTreeFailed {
                        request_id,
                        generation,
                        root,
                        message: format!(
                            "cannot list files: {message} ({code}); no local fallback is used"
                        ),
                    })
                }
                Ok(_) => Some(crate::tui::TuiCommand::FileTreeFailed {
                    request_id,
                    generation,
                    root,
                    message: "directory listing unavailable: unexpected daemon response; no local fallback is used".to_string(),
                }),
                Err(error) => Some(crate::tui::TuiCommand::FileTreeFailed {
                    request_id,
                    generation,
                    root,
                    message: format!(
                        "directory listing unavailable: {error}; this daemon cannot serve a daemon-owned listing and no local fallback is used"
                    ),
                }),
            }
        },
    );
}

/// Convert a scope-checked daemon result into presentation state.
///
/// Advisory statuses stay visible in the pane rather than failing loudly:
/// `busy` means explicit retry, and a `ready` listing that hit a bound
/// carries its truncation reason. Anything that is not displayable
/// metadata never reaches this function (see `spawn_listing`).
fn listing_from_daemon(
    result: &codegg_protocol::document::WorkspaceDirectoryListResultDto,
) -> crate::tui::file_tree::TreeListing {
    use codegg_protocol::document::WorkspaceDirectoryListStatusDto;
    match result.status {
        WorkspaceDirectoryListStatusDto::Ready => crate::tui::file_tree::TreeListing {
            nodes: result
                .nodes
                .iter()
                .cloned()
                .map(crate::tui::file_tree::from_shared_node)
                .collect(),
            truncated: result.truncated,
            truncated_reason: result.truncated_reason.clone(),
            examined: result.examined,
        },
        WorkspaceDirectoryListStatusDto::Busy => crate::tui::file_tree::TreeListing {
            truncated: true,
            truncated_reason: Some("daemon directory scan is busy; press r to retry".to_string()),
            ..Default::default()
        },
        WorkspaceDirectoryListStatusDto::Unavailable => crate::tui::file_tree::TreeListing {
            truncated: true,
            truncated_reason: Some(
                "directory listing unavailable from the daemon; no local fallback is used"
                    .to_string(),
            ),
            ..Default::default()
        },
    }
}

/// Launch a taken pending request after the active request's completion arrives.
///
/// `pending` was already removed from the queue by `finish_active`.
/// Revalidate it against the current view (generation + root) before
/// issuing: a reset may have made it stale. Never lists the wrong
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
        // a new rebuild): drop rather than listing the wrong workspace.
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
    let Some(core_client) = app.core_client.clone() else {
        app.file_tree_state.apply_error(
            pending.generation,
            pending.request_id,
            &pending.root,
            "file-tree refresh requires an attached core client; directory listing is daemon-owned and has no local fallback".to_string(),
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
            spawn_listing(
                app,
                tx,
                core_client,
                pending.generation,
                pending.request_id,
                pending.root,
                tab_id,
                pending.project_id,
                pending.workspace_id,
            );
        }
        crate::tui::file_tree_coordinator::Admit::Queued => {
            // Should not happen on the event loop; keep waiting rather than
            // issuing a second request.
            app.file_tree_state.status = crate::tui::app::state::file_tree::FileTreeStatus::Waiting;
            tracing::warn!("{}", app.file_tree_coordinator.diagnostics());
        }
    }
}

/// Release the single-flight permit after any completion and drive the
/// coalesced pending request, if any. Shared by the listing and failure
/// paths: a late completion after shutdown force-clear mismatches and
/// yields `None`.
fn release_after_completion(app: &mut App, generation: u64, request_id: u64) {
    if let Some(pending) = app
        .file_tree_coordinator
        .finish_active(generation, request_id)
    {
        launch_taken_pending(app, pending);
    }
}

/// Apply a completed daemon listing. A stale completion is discarded here
/// rather than rendered, which is what keeps a tree from showing a
/// workspace the user has already left.
///
/// C004: the completion also releases the single-flight permit and drives
/// the coalesced pending request, if any. A stale completion must **not**
/// cancel the current request (`AsyncUiRequestState::cancel` bumps the
/// request id and would invalidate the queued latest intent); it is simply
/// dropped while the pending request is launched.
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
    // Release the permit even for a stale completion: the active round-trip
    // has ended (its `RunningGuard` already decremented `running`), and the
    // pending latest intent — if any — is now eligible to issue. A late
    // completion after shutdown force-clear mismatches and yields `None`.
    // Single call: `finish_active` clears the active request and takes the
    // pending in one lock hold; `launch_taken_pending` revalidates it.
    release_after_completion(app, generation, request_id);
    if !applied {
        // Stale: deliberately no `request.cancel()`. Cancelling here would
        // bump the request id past the queued latest intent and strand it.
        return;
    }
    // Mirror the accepted listing into a mounted `/files` box. It holds its
    // own copy of the state (a widget cannot reach `App` from `render`), so
    // without this a refresh would repaint the Ctrl+T pane and leave the box
    // showing the previous listing. Cloned only when a box is actually open —
    // the round-trip that produced this state is far more expensive than the
    // copy.
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

/// Apply a failed daemon listing: record the explicit error, then release
/// the permit and drive the pending request exactly like a listing
/// completion. Failures are fail-visible in the pane; there is deliberately
/// no local walk that could mask them.
pub fn apply_failure(
    app: &mut App,
    request_id: u64,
    generation: u64,
    root: PathBuf,
    message: String,
) {
    app.file_tree_state
        .apply_error(generation, request_id, &root, message);
    release_after_completion(app, generation, request_id);
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
    use codegg_protocol::document::{
        WorkspaceDirectoryListResultDto, WorkspaceDirectoryListStatusDto,
        WorkspaceDirectoryNodeDto, WorkspaceDirectoryNodeKindDto,
    };
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    /// Scripted daemon: answers `WorkspaceDirectoryList` from canned
    /// results keyed by workspace id. It never touches the filesystem,
    /// so any rendered entry provably came from the "daemon", not from a
    /// local walk — the decoy tests rely on exactly this.
    struct FakeListingClient {
        results: Mutex<HashMap<String, WorkspaceDirectoryListResultDto>>,
        requests: std::sync::atomic::AtomicUsize,
    }

    impl FakeListingClient {
        fn with_result(workspace_id: &str, result: WorkspaceDirectoryListResultDto) -> Self {
            let mut results = HashMap::new();
            results.insert(workspace_id.to_owned(), result);
            Self {
                results: Mutex::new(results),
                requests: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn ready_listing(workspace_id: &str, names: &[&str]) -> WorkspaceDirectoryListResultDto {
            WorkspaceDirectoryListResultDto {
                workspace_id: workspace_id.to_owned(),
                prefix: String::new(),
                nodes: names
                    .iter()
                    .map(|name| WorkspaceDirectoryNodeDto {
                        name: name.to_string(),
                        path: name.to_string(),
                        depth: 1,
                        kind: WorkspaceDirectoryNodeKindDto::File,
                        children: None,
                    })
                    .collect(),
                truncated: false,
                truncated_reason: None,
                examined: names.len(),
                status: WorkspaceDirectoryListStatusDto::Ready,
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::core::CoreClient for FakeListingClient {
        async fn request(
            &self,
            request: codegg_protocol::core::RequestEnvelope<crate::protocol::core::CoreRequest>,
        ) -> Result<crate::protocol::core::CoreResponse, crate::error::AppError> {
            self.requests
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let crate::protocol::core::CoreRequest::WorkspaceDirectoryList { request } =
                request.payload
            else {
                return Ok(crate::protocol::core::CoreResponse::Error {
                    code: "unsupported_in_test".to_string(),
                    message: "fake listing client".to_string(),
                });
            };
            self.results
                .lock()
                .expect("results lock")
                .get(&request.workspace_id)
                .cloned()
                .map(
                    |result| crate::protocol::core::CoreResponse::WorkspaceDirectoryList { result },
                )
                .ok_or_else(|| crate::error::AppError::Other(anyhow::anyhow!("no scripted result")))
        }

        fn subscribe(
            &self,
        ) -> tokio::sync::mpsc::Receiver<
            codegg_protocol::core::EventEnvelope<codegg_protocol::core::CoreEvent>,
        > {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            rx
        }
    }

    fn app_with_root(root: &std::path::Path) -> App {
        let mut app = App::new_for_testing(root.display().to_string());
        let mut tab = ProjectTabState::empty(ProjectTabId::new(), "project".into());
        tab.project_id = Some("project-test".into());
        tab.workspace_id = Some("workspace-test".into());
        tab.workspace_root = Some(root.to_path_buf());
        app.project_tabs.add_and_activate(tab);
        app
    }

    fn app_with_listing_client(
        root: &std::path::Path,
        result: WorkspaceDirectoryListResultDto,
    ) -> (App, std::sync::Arc<FakeListingClient>) {
        let mut app = app_with_root(root);
        let client = std::sync::Arc::new(FakeListingClient::with_result("workspace-test", result));
        app.set_core_client(client.clone());
        (app, client)
    }

    /// Drive one rebuild to its completion through the real channel.
    async fn rebuild_once(
        app: &mut App,
        rx: &mut tokio::sync::mpsc::Receiver<crate::tui::app::TuiCommand>,
    ) {
        rebuild(app);
        let completion = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("the listing round-trip completes")
            .expect("channel open");
        match completion {
            crate::tui::app::TuiCommand::FileTreeListed {
                request_id,
                generation,
                root,
                listing,
            } => apply_listing(app, request_id, generation, root, listing),
            crate::tui::app::TuiCommand::FileTreeFailed {
                request_id,
                generation,
                root,
                message,
            } => apply_failure(app, request_id, generation, root, message),
            other => panic!("expected a file-tree completion, got {other:?}"),
        }
    }

    #[test]
    fn rebuild_without_a_command_channel_fails_explicitly_without_requesting() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut app = app_with_root(root.path());
        assert!(app.tui_cmd_tx.is_none());

        rebuild(&mut app);

        // Explicit error, not a synchronous request and not a stuck `Loading`.
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
            "no request may issue on the event loop"
        );
    }

    #[test]
    fn rebuild_without_a_core_client_fails_explicitly_with_no_local_fallback() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("local-only.txt"), b"x").expect("write");
        let mut app = app_with_root(root.path());
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);
        assert!(app.core_client.is_none());

        rebuild(&mut app);

        let FileTreeStatus::Error(message) = &app.file_tree_state.status else {
            panic!(
                "a missing core client must fail closed, got {:?}",
                app.file_tree_state.status
            );
        };
        assert!(
            message.contains("no local fallback"),
            "the error must disclaim the local fallback, got: {message}"
        );
        assert!(
            app.file_tree_state.nodes.is_empty(),
            "a present local file must never render without the daemon"
        );
        assert_eq!(app.task_registry.active_count(), 0, "no request may spawn");
    }

    /// C005 remote-only case: the workspace root does not exist on this
    /// host at all, yet the daemon listing renders. The old
    /// `local_walk_root` gate refused exactly this shape.
    #[tokio::test]
    async fn rebuild_lists_through_the_daemon_for_a_remote_only_root() {
        let missing = std::path::PathBuf::from("/nonexistent-workspace-root-for-test");
        let (mut app, _client) = app_with_listing_client(
            &missing,
            FakeListingClient::ready_listing("workspace-test", &["daemon.txt"]),
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);

        rebuild_once(&mut app, &mut rx).await;

        assert_eq!(app.file_tree_state.status, FileTreeStatus::Ready);
        assert!(
            app.file_tree_state
                .rows()
                .iter()
                .any(|row| row.name == "daemon.txt"),
            "the daemon listing must render for a remote-only root"
        );
    }

    /// C005 decoy proof at the TUI layer: a same-spelled local directory
    /// exists with entries the daemon does not report. Only daemon-origin
    /// entries may render; the local tree is never scanned.
    #[tokio::test]
    async fn daemon_listing_never_reads_the_local_decoy_tree() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("local-decoy.txt"), b"x").expect("write");
        let (mut app, _client) = app_with_listing_client(
            root.path(),
            FakeListingClient::ready_listing("workspace-test", &["daemon-only.txt"]),
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);

        rebuild_once(&mut app, &mut rx).await;

        let rows = app.file_tree_state.rows();
        assert!(
            rows.iter().any(|row| row.name == "daemon-only.txt"),
            "daemon entries must render"
        );
        assert!(
            rows.iter().all(|row| row.name != "local-decoy.txt"),
            "the local decoy must never render"
        );
    }

    /// C005 scope fencing: a daemon answer for another workspace is
    /// discarded loudly, never rendered. The scripted result is keyed to
    /// `workspace-foreign` while the tab asks as `workspace-test` — the
    /// mismatch a confused-deputy daemon must never produce, and the TUI
    /// must survive if one ever does.
    #[tokio::test]
    async fn listing_for_another_workspace_is_discarded_without_rendering() {
        let root = tempfile::tempdir().expect("tempdir");
        let foreign = FakeListingClient::ready_listing("workspace-foreign", &["foreign.txt"]);
        let (mut app, _client) = app_with_listing_client(root.path(), foreign);
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);

        rebuild_once(&mut app, &mut rx).await;

        let FileTreeStatus::Error(message) = &app.file_tree_state.status else {
            panic!(
                "a foreign-workspace answer must fail closed, got {:?}",
                app.file_tree_state.status
            );
        };
        assert!(
            message.contains("different workspace"),
            "the error must name the scope mismatch, got: {message}"
        );
        assert!(
            app.file_tree_state
                .rows()
                .iter()
                .all(|row| row.name != "foreign.txt"),
            "foreign entries must never render"
        );
    }

    /// C005 busy/unavailable transparency: advisory statuses stay visible
    /// in the pane with an explicit reason, never a silent empty tree.
    #[tokio::test]
    async fn busy_and_unavailable_answer_with_visible_reasons() {
        let root = tempfile::tempdir().expect("tempdir");
        for (status, needle) in [
            (WorkspaceDirectoryListStatusDto::Busy, "busy"),
            (WorkspaceDirectoryListStatusDto::Unavailable, "unavailable"),
        ] {
            let (mut app, _client) = app_with_listing_client(
                root.path(),
                WorkspaceDirectoryListResultDto {
                    workspace_id: "workspace-test".to_owned(),
                    prefix: String::new(),
                    nodes: Vec::new(),
                    truncated: false,
                    truncated_reason: None,
                    examined: 0,
                    status,
                },
            );
            let (tx, mut rx) = tokio::sync::mpsc::channel(16);
            app.tui_cmd_tx = Some(tx);

            rebuild_once(&mut app, &mut rx).await;

            let reason = app.file_tree_state.truncated.clone();
            assert!(
                reason.as_deref().is_some_and(|r| r.contains(needle)),
                "{status:?} must carry a visible reason, got {reason:?}"
            );
            assert!(
                app.file_tree_state.nodes.is_empty(),
                "no entries may render for {status:?}"
            );
        }
    }

    /// C005 old-daemon transparency: a transport failure, a daemon
    /// `Error`, or an unexpected response variant is an explicit error
    /// with no local fallback, never an empty tree presented as complete.
    #[tokio::test]
    async fn daemon_errors_fail_closed_with_no_local_fallback() {
        /// Scripted daemon that answers with one canned outcome.
        enum Outcome {
            TransportFailure,
            DaemonError,
            UnexpectedVariant,
        }

        struct FailingClient(Mutex<VecDeque<Outcome>>);

        #[async_trait::async_trait]
        impl crate::core::CoreClient for FailingClient {
            async fn request(
                &self,
                _request: codegg_protocol::core::RequestEnvelope<
                    crate::protocol::core::CoreRequest,
                >,
            ) -> Result<crate::protocol::core::CoreResponse, crate::error::AppError> {
                match self.0.lock().expect("outcomes lock").pop_front() {
                    Some(Outcome::TransportFailure) => Err(crate::error::AppError::Other(
                        anyhow::anyhow!("daemon socket closed"),
                    )),
                    Some(Outcome::DaemonError) => Ok(crate::protocol::core::CoreResponse::Error {
                        code: "document_not_found".to_string(),
                        message: "workspace not found".to_string(),
                    }),
                    Some(Outcome::UnexpectedVariant) => {
                        Ok(crate::protocol::core::CoreResponse::Ack)
                    }
                    None => Err(crate::error::AppError::Other(anyhow::anyhow!(
                        "no scripted outcome"
                    ))),
                }
            }

            fn subscribe(
                &self,
            ) -> tokio::sync::mpsc::Receiver<
                codegg_protocol::core::EventEnvelope<codegg_protocol::core::CoreEvent>,
            > {
                let (_tx, rx) = tokio::sync::mpsc::channel(1);
                rx
            }
        }

        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("local-only.txt"), b"x").expect("write");
        for outcome in [
            Outcome::TransportFailure,
            Outcome::DaemonError,
            Outcome::UnexpectedVariant,
        ] {
            let mut app = app_with_root(root.path());
            let client = std::sync::Arc::new(FailingClient(Mutex::new(VecDeque::from([outcome]))));
            app.set_core_client(client);
            let (tx, mut rx) = tokio::sync::mpsc::channel(16);
            app.tui_cmd_tx = Some(tx);

            rebuild_once(&mut app, &mut rx).await;

            let FileTreeStatus::Error(message) = &app.file_tree_state.status else {
                panic!(
                    "daemon failures must fail closed, got {:?}",
                    app.file_tree_state.status
                );
            };
            assert!(
                message.contains("no local fallback"),
                "the error must disclaim the local fallback, got: {message}"
            );
            assert!(
                app.file_tree_state.nodes.is_empty(),
                "a present local file must never render on daemon failure"
            );
        }
    }

    #[tokio::test]
    async fn a_completion_from_before_a_project_switch_is_refused() {
        let first = tempfile::tempdir().expect("tempdir");
        let second = tempfile::tempdir().expect("tempdir");

        let (mut app, _client) = app_with_listing_client(
            first.path(),
            FakeListingClient::ready_listing("workspace-test", &["from-first.txt"]),
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);
        rebuild(&mut app);
        let first_completion = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("the first request completes")
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

        // Switch projects and rebuild: the generation moves on, so the
        // listing for the previous workspace is stale even though it
        // completed.
        let mut tab = ProjectTabState::empty(ProjectTabId::new(), "second".into());
        tab.project_id = Some("project-second".into());
        tab.workspace_id = Some("workspace-second".into());
        tab.workspace_root = Some(second.path().to_path_buf());
        app.project_tabs.add_and_activate(tab);
        rebuild(&mut app);

        apply_listing(&mut app, request_id, generation, root, listing);
        assert!(
            app.file_tree_state
                .rows()
                .iter()
                .all(|row| row.name != "from-first.txt"),
            "a listing from before the switch must never render"
        );
    }

    #[tokio::test]
    async fn a_hundred_rapid_rebuilds_keep_one_live_task() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mut app, client) = app_with_listing_client(
            root.path(),
            FakeListingClient::ready_listing("workspace-test", &["file.txt"]),
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(256);
        app.tui_cmd_tx = Some(tx);
        // Keep the receiver alive so completions can land; capacity holds
        // all completions without the test needing to pump the loop.

        let before = app.file_tree_state.generation;
        // Hold the first round-trip: without draining, admission stays held
        // exactly like a held blocking scan did pre-C005.
        for _ in 0..100 {
            rebuild(&mut app);
        }
        assert_eq!(app.file_tree_state.generation, before + 100);
        assert_eq!(
            app.task_registry.active_count(),
            1,
            "the hard concurrency limit is one live FileTree task"
        );
        let _ = client;
    }

    // ── C004 single-flight admission (now over daemon round-trips) ──────

    /// Single-flight proof over the daemon transport: one held round-trip
    /// plus 100 rebuilds yields one active waiter, one queued latest
    /// intent, an explicit `Waiting` state, and no second concurrent
    /// daemon request. No sleeps: the first completion sits unread in the
    /// channel, holding admission.
    #[tokio::test]
    async fn c004_held_scan_plus_100_rebuilds_keeps_one_worker_one_pending() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mut app, client) = app_with_listing_client(
            root.path(),
            FakeListingClient::ready_listing("workspace-test", &["file.txt"]),
        );
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
            "only the first rebuild may issue while it is held"
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
        // No second concurrent daemon request could have issued: at most
        // the first round-trip entered (peak <= 1). The first completion is
        // still unread, so its round-trip already exited (running 0) or is
        // still inside (running 1).
        assert!(
            app.file_tree_coordinator.running_scan_count() <= 1,
            "running must be 0 or 1, got {}",
            app.file_tree_coordinator.running_scan_count()
        );
        assert!(
            app.file_tree_coordinator.peak_running() <= 1,
            "peak concurrent requests must be <= 1, got {}",
            app.file_tree_coordinator.peak_running()
        );

        // Drain the held first completion (stale) and drive the pending.
        let first = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("first completion arrives")
            .expect("channel open");
        match first {
            crate::tui::app::TuiCommand::FileTreeListed {
                request_id,
                generation,
                root: completed_root,
                listing,
            } => apply_listing(&mut app, request_id, generation, completed_root, listing),
            crate::tui::app::TuiCommand::FileTreeFailed {
                request_id,
                generation,
                root: completed_root,
                message,
            } => apply_failure(&mut app, request_id, generation, completed_root, message),
            other => panic!("expected a file-tree completion, got {other:?}"),
        }
        // The stale first completion launched the latest pending as a new
        // active request; the pane is loading it, not stuck waiting.
        assert!(app.file_tree_coordinator.has_active());
        assert!(!app.file_tree_coordinator.has_queued());
        assert_eq!(app.file_tree_state.status, FileTreeStatus::Loading);

        // Drain the pending request's completion (current) and apply it.
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
        assert_eq!(
            client.requests.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "exactly two daemon requests issued: the held first plus the driven pending"
        );
    }

    /// Project switch: a held A request plus a B rebuild shows only B.
    /// Late A is discarded; `/files` parity holds because both consume the
    /// same generation-guarded state.
    #[tokio::test]
    async fn c004_switch_from_a_to_b_shows_only_b() {
        let first = tempfile::tempdir().expect("tempdir");
        let second = tempfile::tempdir().expect("tempdir");

        let (mut app, _client) = app_with_listing_client(
            first.path(),
            FakeListingClient::ready_listing("workspace-test", &["from-first.txt"]),
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);
        rebuild(&mut app);

        // Switch workspaces and rebuild before A's completion is applied:
        // B queues behind the held A request. The B tab reuses the same
        // scripted workspace binding so the fake answers identically; the
        // fencing under test is generation/root, not scope.
        let mut tab = ProjectTabState::empty(ProjectTabId::new(), "second".into());
        tab.project_id = Some("project-test".into());
        tab.workspace_id = Some("workspace-test".into());
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
                .any(|row| row.name == "from-first.txt"),
            "B renders the current workspace binding"
        );
        assert!(
            app.file_tree_coordinator.peak_running() <= 1,
            "peak must stay <= 1 across the switch"
        );
        // `/files` seeds from the same state, so it sees the same tree.
        let dialog_rows = app.file_tree_state.rows();
        assert!(
            dialog_rows.iter().any(|row| row.name == "from-first.txt"),
            "/files and Ctrl+T share the one guarded state"
        );
    }

    /// Shutdown: clearing admission discards a late completion without
    /// relaunching, and never claims the worker was joined.
    #[tokio::test]
    async fn c004_shutdown_discards_late_completion_without_relaunch() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mut app, _client) = app_with_listing_client(
            root.path(),
            FakeListingClient::ready_listing("workspace-test", &["file.txt"]),
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        app.tui_cmd_tx = Some(tx);
        rebuild(&mut app);
        assert!(app.file_tree_coordinator.has_active());

        // Shut down before the completion is applied: pending cleared (none),
        // active force-cleared, waiters aborted. A daemon round-trip may
        // still be in flight; shutdown does not join it.
        app.prepare_shutdown();
        assert!(!app.file_tree_coordinator.has_active());
        assert!(!app.file_tree_coordinator.has_queued());

        // A late completion (if the waiter sent before abort, or the test
        // synthesizes one) mismatches and is discarded with no relaunch.
        let late = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await;
        if let Ok(Some(msg)) = late {
            match msg {
                crate::tui::app::TuiCommand::FileTreeListed {
                    request_id,
                    generation,
                    root,
                    listing,
                } => apply_listing(&mut app, request_id, generation, root, listing),
                crate::tui::app::TuiCommand::FileTreeFailed {
                    request_id,
                    generation,
                    root,
                    message,
                } => apply_failure(&mut app, request_id, generation, root, message),
                other => panic!("expected a file-tree completion, got {other:?}"),
            }
            assert!(
                !app.file_tree_coordinator.has_active(),
                "shutdown must not resurrect a request"
            );
            assert!(
                !app.file_tree_coordinator.has_queued(),
                "shutdown must not queue"
            );
            assert_eq!(
                app.task_registry.active_count(),
                0,
                "no new worker may spawn after shutdown"
            );
        }
        assert!(
            app.file_tree_coordinator.peak_running() <= 1,
            "peak must stay <= 1 through shutdown"
        );
    }

    /// Truncated panic-converted completion still releases the permit and
    /// drives the queued latest intent. The panic itself is proven to
    /// release `running` in the coordinator unit test; here the truncated
    /// listing proves the event-loop handoff.
    #[tokio::test]
    async fn c004_truncated_panic_completion_still_drives_pending() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mut app, _client) = app_with_listing_client(
            root.path(),
            FakeListingClient::ready_listing("workspace-test", &["file.txt"]),
        );
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
        // still launch the pending request.
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
            "a panic-converted completion must still promote the pending request"
        );

        // The promoted request completes normally.
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
