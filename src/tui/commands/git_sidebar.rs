//! Git sidebar background refresh.
//!
//! The sidebar shows git status (root, branch, dirty) but render is
//! strictly a pure read from the cached `GitSidebarState`. All
//! filesystem/git probing happens here, in spawned background tasks,
//! and results are committed via a typed completion command.

use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

const GIT_REFRESH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Schedule a background refresh of the git sidebar state. The
/// generation counter is bumped before the task is spawned; stale
/// completions are dropped at apply time.
pub(crate) fn start_refresh_git_sidebar(app: &mut App) {
    let Some(project_dir) = app
        .session_state
        .session
        .as_ref()
        .map(|s| s.project_id.clone())
    else {
        return;
    };

    let generation = app.session_state.git_sidebar.begin_refresh();

    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::GitStatus,
        "git_sidebar_refresh",
        async move {
            let result = tokio::time::timeout(
                GIT_REFRESH_TIMEOUT,
                probe_git_status(std::path::PathBuf::from(&project_dir)),
            )
            .await;

            let payload = match result {
                Ok(Ok(info)) => Some((info.root, info.branch, info.dirty, None)),
                Ok(Err(e)) => Some((None, None, false, Some(e.to_string()))),
                Err(_elapsed) => Some((None, None, false, Some("git probe timed out".to_string()))),
            };

            if let Some((root, branch, dirty, error)) = payload {
                if let Some(err) = error {
                    Some(TuiCommand::GitSidebarRefreshFinished {
                        generation,
                        root: None,
                        branch: None,
                        dirty: false,
                        error: Some(err),
                    })
                } else {
                    Some(TuiCommand::GitSidebarRefreshFinished {
                        generation,
                        root,
                        branch,
                        dirty,
                        error: None,
                    })
                }
            } else {
                None
            }
        },
    );
}

#[derive(Debug)]
struct GitProbeInfo {
    root: Option<String>,
    branch: Option<String>,
    dirty: bool,
}

async fn probe_git_status(project_dir: std::path::PathBuf) -> anyhow::Result<GitProbeInfo> {
    let git_root = crate::worktree::find_git_root(&project_dir);
    if git_root.is_none() {
        return Ok(GitProbeInfo {
            root: None,
            branch: None,
            dirty: false,
        });
    }
    let root = git_root.expect("checked is_some above");
    let status = egggit::status::repo_status(&root)
        .await
        .map_err(|e| anyhow::anyhow!("git status failed: {}", e))?;
    let branch = if status.branch.is_empty() {
        Some("detached".to_string())
    } else {
        Some(status.branch)
    };
    Ok(GitProbeInfo {
        root: Some(root.to_string_lossy().into_owned()),
        branch,
        dirty: status.is_dirty,
    })
}

/// Apply a completed git sidebar refresh to the cached state. Stale
/// completions (mismatched generation) are dropped silently so a slow
/// probe cannot overwrite a newer session/project state.
pub(crate) fn apply_git_sidebar_refresh(
    app: &mut App,
    generation: u64,
    root: Option<String>,
    branch: Option<String>,
    dirty: bool,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.session_state
            .git_sidebar
            .apply_refresh_error(generation, err);
        return;
    }
    app.session_state
        .git_sidebar
        .apply_refresh(generation, root, branch, dirty);
}
