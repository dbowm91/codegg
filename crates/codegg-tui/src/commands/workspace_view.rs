//! M007-M011: the Workspace whole view.
//!
//! One route-owned surface: a project -> workspace -> worktree tree on the
//! left, a chat viewer on the right, its own NOR/INS mode, an inline detail,
//! and the new-session sheet. Everything it reads is an existing daemon
//! answer: `ProjectGet` for a project's workspaces, `ManagedWorktreeList` for
//! a workspace's worktrees, `GitRead(Branches)` for the launch base, and
//! `SessionLaunch` for the session itself. It never resolves a path, never
//! mints a branch name, and never spawns `git`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::state::workspace_view::{
    LaunchSheet, LaunchStep, LifecycleArm, TreeKind, TreeRow, WorkspaceMode, MAX_TREE_ROWS,
};
use crate::app::{App, TuiCommand};
use crate::async_cmd::spawn_registered_tui_task;
use crate::commands::project_picker::ProjectLifecycleEntry;
use crate::task_lifecycle::TuiTaskKind;
use codegg::protocol::core::{CoreRequest, CoreResponse};

// -- The tree ---------------------------------------------------------

/// The rows the view currently paints and navigates.
pub(crate) fn visible_rows(app: &App) -> Vec<TreeRow> {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_ref() else {
        return Vec::new();
    };
    let view = &dashboard.view;
    let query = view.filter.trim();
    let mut rows: Vec<TreeRow> = Vec::new();
    for row in dashboard.rows.iter() {
        let project_id = row.summary.project_id.clone();
        if !query.is_empty() {
            let score = codegg::util::fuzzy::fuzzy_score(query, &row.summary.display_name)
                .max(codegg::util::fuzzy::fuzzy_score(query, &project_id));
            if score == 0 {
                continue;
            }
        }
        let expanded = view.expanded.contains(&project_id);
        rows.push(TreeRow {
            kind: TreeKind::Project,
            id: project_id.clone(),
            project_id: project_id.clone(),
            workspace_id: None,
            depth: 0,
            label: row.summary.display_name.clone(),
            note: crate::app::state::dashboard_row_badge(row),
            expandable: true,
            expanded,
        });
        if !expanded {
            continue;
        }
        let Some(workspaces) = view.workspaces.get(&project_id) else {
            rows.push(note_row(
                TreeKind::Workspace,
                &project_id,
                &project_id,
                None,
                1,
                if view.workspaces_loading.contains(&project_id) {
                    "loading workspaces..."
                } else {
                    "workspaces not loaded - l to load"
                },
            ));
            continue;
        };
        if workspaces.is_empty() {
            rows.push(note_row(
                TreeKind::Workspace,
                &project_id,
                &project_id,
                None,
                1,
                "no registered workspaces",
            ));
            continue;
        }
        for workspace in workspaces {
            let workspace_expanded = view.expanded.contains(&workspace.workspace_id);
            rows.push(TreeRow {
                kind: TreeKind::Workspace,
                id: workspace.workspace_id.clone(),
                project_id: project_id.clone(),
                workspace_id: Some(workspace.workspace_id.clone()),
                depth: 1,
                label: workspace.display_name.clone(),
                note: workspace.canonical_root.clone().unwrap_or_default(),
                expandable: true,
                expanded: workspace_expanded,
            });
            if !workspace_expanded {
                continue;
            }
            match view.worktrees.get(&workspace.workspace_id) {
                None => rows.push(note_row(
                    TreeKind::Workspace,
                    &project_id,
                    &workspace.workspace_id,
                    Some(workspace.workspace_id.clone()),
                    2,
                    if view.worktrees_loading.contains(&workspace.workspace_id) {
                        "loading worktrees..."
                    } else {
                        "worktrees not loaded - l to load"
                    },
                )),
                Some(worktrees) if worktrees.is_empty() => rows.push(note_row(
                    TreeKind::Workspace,
                    &project_id,
                    &workspace.workspace_id,
                    Some(workspace.workspace_id.clone()),
                    2,
                    "no managed worktrees",
                )),
                Some(worktrees) => {
                    for worktree in worktrees {
                        let branch = worktree
                            .branch
                            .clone()
                            .unwrap_or_else(|| "detached".to_string());
                        rows.push(TreeRow {
                            kind: TreeKind::Worktree,
                            id: worktree.worktree_id.clone(),
                            project_id: project_id.clone(),
                            workspace_id: Some(workspace.workspace_id.clone()),
                            depth: 2,
                            label: branch,
                            note: format!("{} - {}", worktree.state, worktree.path),
                            expandable: false,
                            expanded: false,
                        });
                    }
                }
            }
        }
        if rows.len() >= MAX_TREE_ROWS {
            break;
        }
    }
    rows.truncate(MAX_TREE_ROWS);
    rows
}

fn note_row(
    kind: TreeKind,
    project_id: &str,
    id: &str,
    workspace_id: Option<String>,
    depth: u8,
    note: &str,
) -> TreeRow {
    TreeRow {
        kind,
        id: id.to_string(),
        project_id: project_id.to_string(),
        workspace_id,
        depth,
        label: String::new(),
        note: note.to_string(),
        expandable: false,
        expanded: false,
    }
}

/// The row under the cursor, if the tree is not empty.
pub(crate) fn selected_row(app: &App) -> Option<TreeRow> {
    let rows = visible_rows(app);
    let selected = app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .map(|dashboard| dashboard.view.selected)
        .unwrap_or(0);
    rows.get(selected.min(rows.len().saturating_sub(1)))
        .cloned()
}

fn clamp_selection(app: &mut App, len: usize) {
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.selected = if len == 0 {
            0
        } else {
            dashboard.view.selected.min(len - 1)
        };
    }
}

fn move_selection(app: &mut App, delta: isize) {
    let len = visible_rows(app).len();
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        let current = dashboard.view.selected as isize;
        let next = (current + delta).clamp(0, len.saturating_sub(1) as isize);
        dashboard.view.selected = next as usize;
    }
    sync_selected_project(app);
    // Keep the dashboard cursor on the same project (project rows map 1:1).
    let project_id = selected_row(app).map(|row| row.project_id);
    if let Some(project_id) = project_id {
        if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
            let indices = dashboard.filtered_indices();
            if let Some(pos) = indices.iter().position(|&i| {
                dashboard
                    .rows
                    .get(i)
                    .is_some_and(|row| row.summary.project_id == project_id)
            }) {
                dashboard.selected_row = pos;
            }
        }
    }
}

fn select_edge(app: &mut App, last: bool) {
    let len = visible_rows(app).len();
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.selected = if last { len.saturating_sub(1) } else { 0 };
    }
    sync_selected_project(app);
}

/// The project the Workspace view is pointing at, when the route is open.
pub(crate) fn selected_project_id(app: &App) -> Option<String> {
    let dashboard = app.dialog_state.workspace_dashboard.as_ref()?;
    dashboard
        .view
        .detail
        .clone()
        .or_else(|| selected_row(app).map(|row| row.project_id))
}

/// Point the chat pane at the selected row's project.
fn sync_selected_project(app: &mut App) {
    let Some(row) = selected_row(app) else {
        return;
    };
    app.chat_panel_project = Some(row.project_id.clone());
    crate::commands::panels::sync_chat_panel_project(app);
}

// -- Expansion and detail ---------------------------------------------

fn toggle_expand(app: &mut App, row: &TreeRow) {
    match row.kind {
        TreeKind::Project => {
            let expanded = app
                .dialog_state
                .workspace_dashboard
                .as_ref()
                .is_some_and(|dashboard| dashboard.view.expanded.contains(&row.id));
            if expanded {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.expanded.remove(&row.id);
                }
            } else {
                let known = app
                    .dialog_state
                    .workspace_dashboard
                    .as_ref()
                    .is_some_and(|dashboard| dashboard.view.workspaces.contains_key(&row.id));
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.expanded.insert(row.id.clone());
                }
                if !known {
                    fetch_project_workspaces(app, row.project_id.clone());
                }
            }
        }
        TreeKind::Workspace => {
            let expanded = app
                .dialog_state
                .workspace_dashboard
                .as_ref()
                .is_some_and(|dashboard| dashboard.view.expanded.contains(&row.id));
            if expanded {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.expanded.remove(&row.id);
                }
            } else {
                let known = app
                    .dialog_state
                    .workspace_dashboard
                    .as_ref()
                    .is_some_and(|dashboard| dashboard.view.worktrees.contains_key(&row.id));
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.expanded.insert(row.id.clone());
                }
                if !known {
                    fetch_worktrees(app, row.id.clone());
                }
            }
        }
        TreeKind::Worktree => {
            if let Some(parent) = row.workspace_id.clone() {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.expanded.remove(&parent);
                }
            }
        }
    }
}

fn collapse_selected(app: &mut App, row: &TreeRow) {
    match row.kind {
        TreeKind::Project | TreeKind::Workspace => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                dashboard.view.expanded.remove(&row.id);
            }
        }
        TreeKind::Worktree => {
            let parent = row.workspace_id.clone().unwrap_or_default();
            let rows = visible_rows(app);
            if let Some(index) = rows.iter().rposition(|candidate| candidate.id == parent) {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.selected = index;
                }
            }
        }
    }
}

fn open_detail(app: &mut App, row: &TreeRow) {
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.detail = Some(row.project_id.clone());
        dashboard.view.detail_loading = true;
        dashboard.view.detail_error = None;
        let known = dashboard.view.workspaces.contains_key(&row.project_id);
        if known {
            dashboard.view.detail_data = None;
        }
        let _ = known;
    }
    fetch_project_workspaces(app, row.project_id.clone());
}

fn open_project(app: &mut App, project_id: &str) {
    let tab_id = app
        .project_tabs
        .find_by_project(project_id)
        .map(|tab| tab.tab_id.clone());
    if let Some(tab_id) = tab_id {
        app.switch_active_tab(&tab_id);
    }
    crate::commands::workspace_dashboard::leave_workspace_view(app);
}

// -- Requests ---------------------------------------------------------

fn next_request(app: &mut App) -> u64 {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
        return 0;
    };
    let id = dashboard.view.next_request.wrapping_add(1);
    dashboard.view.next_request = id;
    id
}

fn fetch_project_workspaces(app: &mut App, project_id: String) {
    let request_id = next_request(app);
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.workspaces_loading.insert(project_id.clone());
    }
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Workspace,
        "workspace_project_workspaces",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkspaceProjectLoaded {
                    request_id,
                    project_id,
                    project: None,
                    error: Some("no daemon connection".to_string()),
                });
            };
            let request = codegg::core::new_request(
                format!("workspace-project-{request_id}"),
                CoreRequest::ProjectGet {
                    project_id: project_id.clone(),
                },
            );
            let (project, error) = match core_client.request(request).await {
                Ok(CoreResponse::ProjectGet { project }) => (Some(Box::new(project)), None),
                Ok(CoreResponse::Error { code, message }) => {
                    (None, Some(format!("{code}: {message}")))
                }
                Ok(other) => (None, Some(format!("unexpected response: {other:?}"))),
                Err(error) => (None, Some(error.to_string())),
            };
            Some(TuiCommand::WorkspaceProjectLoaded {
                request_id,
                project_id,
                project,
                error,
            })
        },
    );
}

fn fetch_worktrees(app: &mut App, workspace_id: String) {
    let request_id = next_request(app);
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard
            .view
            .worktrees_loading
            .insert(workspace_id.clone());
    }
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Workspace,
        "workspace_worktrees",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkspaceWorktreesLoaded {
                    request_id,
                    workspace_id,
                    worktrees: Vec::new(),
                    error: Some("no daemon connection".to_string()),
                });
            };
            let request = codegg::core::new_request(
                format!("workspace-worktrees-{request_id}"),
                CoreRequest::ManagedWorktreeList {
                    workspace_id: Some(workspace_id.clone()),
                    repository_id: None,
                    run_id: None,
                    include_removed: false,
                },
            );
            let (worktrees, error) = match core_client.request(request).await {
                Ok(CoreResponse::ManagedWorktrees { worktrees }) => (worktrees, None),
                Ok(CoreResponse::Error { code, message }) => {
                    (Vec::new(), Some(format!("{code}: {message}")))
                }
                Ok(other) => (Vec::new(), Some(format!("unexpected response: {other:?}"))),
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
            Some(TuiCommand::WorkspaceWorktreesLoaded {
                request_id,
                workspace_id,
                worktrees,
                error,
            })
        },
    );
}

fn fetch_branches(app: &mut App, project_id: String) {
    let request_id = next_request(app);
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        if let Some(launch) = dashboard.view.launch.as_mut() {
            launch.branches_loading = true;
            launch.error = None;
        }
    }
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Workspace,
        "workspace_branches",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkspaceBranchesLoaded {
                    request_id,
                    branches: Vec::new(),
                    error: Some("no daemon connection".to_string()),
                });
            };
            let request = codegg::core::new_request(
                format!("workspace-branches-{request_id}"),
                CoreRequest::GitRead {
                    request: codegg::protocol::git::GitReadRequestDto {
                        project_id,
                        operation: codegg::protocol::git::GitReadOperationDto::Branches,
                        max_rows: Some(64),
                    },
                },
            );
            let (branches, error) = match core_client.request(request).await {
                Ok(CoreResponse::GitRead { result }) => match result {
                    codegg::protocol::git::GitReadResultDto::Branches(list) => (
                        list.branches
                            .into_iter()
                            .map(|branch| branch.name)
                            .collect(),
                        None,
                    ),
                    _ => (
                        Vec::new(),
                        Some("the branch read returned another shape".to_string()),
                    ),
                },
                Ok(CoreResponse::Error { code, message }) => {
                    (Vec::new(), Some(format!("{code}: {message}")))
                }
                Ok(other) => (Vec::new(), Some(format!("unexpected response: {other:?}"))),
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
            Some(TuiCommand::WorkspaceBranchesLoaded {
                request_id,
                branches,
                error,
            })
        },
    );
}

fn submit_launch(app: &mut App) {
    let planned = {
        let Some(dashboard) = app.dialog_state.workspace_dashboard.as_ref() else {
            return;
        };
        let Some(launch) = dashboard.view.launch.as_ref() else {
            return;
        };
        if launch.submitting {
            return;
        }
        let placement = match launch.step {
            LaunchStep::Placement => {
                if launch.selected == 0 {
                    None
                } else {
                    launch
                        .ready_worktrees()
                        .nth(launch.selected.saturating_sub(1))
                        .map(|worktree| {
                            codegg::protocol::core::SessionLaunchPlacementDto::ExistingManagedWorktree {
                                worktree_id: worktree.worktree_id.clone(),
                            }
                        })
                }
            }
            LaunchStep::Base => None,
            LaunchStep::Branch => {
                let base_ref = launch.branches.get(launch.base_selected).cloned();
                let target_branch =
                    Some(launch.branch_input.trim().to_string()).filter(|name| !name.is_empty());
                Some(
                    codegg::protocol::core::SessionLaunchPlacementDto::NewManagedCheckout {
                        base_ref,
                        target_branch,
                    },
                )
            }
        };
        placement.map(|placement| {
            (
                launch.project_id.clone(),
                placement,
                launch.idempotency_key.clone(),
            )
        })
    };
    let Some((project_id, placement, idempotency_key)) = planned else {
        return;
    };
    let request_id = next_request(app);
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        if let Some(launch) = dashboard.view.launch.as_mut() {
            launch.submitting = true;
            launch.error = None;
        }
    }
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Workspace,
        "workspace_session_launch",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkspaceLaunchFinished {
                    request_id,
                    session_id: None,
                    workspace_id: None,
                    branch: None,
                    stage: "accepted".to_string(),
                    converged: false,
                    error: Some("no daemon connection".to_string()),
                });
            };
            let request = codegg::core::new_request(
                format!("workspace-launch-{request_id}"),
                CoreRequest::SessionLaunch {
                    project_id,
                    intent: codegg::protocol::core::SessionLaunchIntentDto::New,
                    parent_session_id: None,
                    placement,
                    idempotency_key,
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::SessionLaunch {
                    session_id,
                    workspace_id,
                    branch,
                    stage,
                    converged,
                    ..
                }) => Some(TuiCommand::WorkspaceLaunchFinished {
                    request_id,
                    session_id: Some(session_id),
                    workspace_id: Some(workspace_id),
                    branch,
                    stage,
                    converged,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::WorkspaceLaunchFinished {
                        request_id,
                        session_id: None,
                        workspace_id: None,
                        branch: None,
                        stage: code,
                        converged: false,
                        error: Some(message),
                    })
                }
                Ok(other) => Some(TuiCommand::WorkspaceLaunchFinished {
                    request_id,
                    session_id: None,
                    workspace_id: None,
                    branch: None,
                    stage: "error".to_string(),
                    converged: false,
                    error: Some(format!("unexpected response: {other:?}")),
                }),
                Err(error) => Some(TuiCommand::WorkspaceLaunchFinished {
                    request_id,
                    session_id: None,
                    workspace_id: None,
                    branch: None,
                    stage: "error".to_string(),
                    converged: false,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
}

fn reveal_workspace(app: &mut App, row: &TreeRow) {
    let workspace_id = match row.kind {
        TreeKind::Workspace => row.workspace_id.clone(),
        TreeKind::Worktree => row.workspace_id.clone(),
        TreeKind::Project => None,
    };
    let Some(workspace_id) = workspace_id else {
        return;
    };
    let project_id = row.project_id.clone();
    let worktrees_loaded = {
        let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
            return;
        };
        dashboard.view.reveal_workspace_id = Some(workspace_id.clone());
        dashboard.view.team_project_id = Some(project_id.clone());
        dashboard.view.team_memberships = None;
        dashboard.view.team_loading = false;
        dashboard.view.team_error = None;
        dashboard.view.team_notice = None;
        dashboard.view.member_form = false;
        dashboard.view.mode = WorkspaceMode::Normal;
        dashboard.view.input_focus = crate::app::state::workspace_view::WorkspaceInputFocus::Tree;
        dashboard.view.expanded.insert(workspace_id.clone());
        dashboard.view.worktrees.contains_key(&workspace_id)
    };
    if !worktrees_loaded {
        fetch_worktrees(app, workspace_id);
    }
    crate::commands::team::start_show_team(app, project_id);
}

fn open_inline_team(app: &mut App, row: &TreeRow) {
    if row.kind != TreeKind::Project {
        reveal_workspace(app, row);
        return;
    }
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.reveal_workspace_id = None;
        dashboard.view.team_project_id = Some(row.project_id.clone());
        dashboard.view.team_memberships = None;
        dashboard.view.team_loading = false;
        dashboard.view.team_error = None;
        dashboard.view.team_notice = None;
    }
    crate::commands::team::start_show_team(app, row.project_id.clone());
}

fn begin_member_form(app: &mut App) {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
        return;
    };
    if dashboard.view.team_project_id.is_none() {
        dashboard.view.notice =
            Some("Reveal a workspace or open its team section first".to_string());
        return;
    }
    dashboard.view.member_form = true;
    dashboard.view.member_principal.clear();
    dashboard.view.member_role = "contributor".to_string();
    dashboard.view.mode = WorkspaceMode::Insert;
    dashboard.view.input_focus = crate::app::state::workspace_view::WorkspaceInputFocus::MemberForm;
    dashboard.view.team_notice = None;
}

fn handle_member_form_key(app: &mut App, key: KeyEvent) -> bool {
    use crate::app::state::workspace_view::WorkspaceInputFocus;
    match key.code {
        KeyCode::Esc => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                dashboard.view.member_form = false;
                dashboard.view.member_principal.clear();
                dashboard.view.mode = WorkspaceMode::Normal;
                dashboard.view.input_focus = WorkspaceInputFocus::Tree;
            }
        }
        KeyCode::Backspace if key.modifiers.is_empty() => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                dashboard.view.member_principal.pop();
            }
        }
        KeyCode::Tab => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                dashboard.view.member_role = match dashboard.view.member_role.as_str() {
                    "viewer" => "contributor".to_string(),
                    "contributor" => "maintainer".to_string(),
                    _ => "viewer".to_string(),
                };
            }
        }
        KeyCode::Enter => {
            let (project_id, principal, role) = app
                .dialog_state
                .workspace_dashboard
                .as_mut()
                .map(|dashboard| {
                    dashboard.view.member_form = false;
                    dashboard.view.mode = WorkspaceMode::Normal;
                    dashboard.view.input_focus = WorkspaceInputFocus::Tree;
                    (
                        dashboard.view.team_project_id.clone(),
                        dashboard.view.member_principal.trim().to_string(),
                        dashboard.view.member_role.clone(),
                    )
                })
                .unwrap_or_default();
            if principal.is_empty() {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.team_notice =
                        Some("Enter a principal id before adding a member".to_string());
                }
            } else if let Some(project_id) = project_id {
                crate::commands::team::start_workspace_member_add(app, project_id, principal, role);
            }
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                if dashboard.view.member_principal.chars().count() < 256 {
                    dashboard.view.member_principal.push(ch);
                }
            }
        }
        _ => {}
    }
    true
}

// -- Launch sheet keys ------------------------------------------------

fn begin_launch(app: &mut App, row: &TreeRow) {
    let (project_id, workspace_id) = match row.kind {
        TreeKind::Workspace => (row.project_id.clone(), row.id.clone()),
        TreeKind::Worktree => (
            row.project_id.clone(),
            row.workspace_id.clone().unwrap_or_default(),
        ),
        TreeKind::Project => {
            let single = app
                .dialog_state
                .workspace_dashboard
                .as_ref()
                .and_then(|dashboard| dashboard.view.workspaces.get(&row.id))
                .filter(|workspaces| workspaces.len() == 1)
                .map(|workspaces| workspaces[0].workspace_id.clone());
            match single {
                Some(workspace_id) => (row.id.clone(), workspace_id),
                None => {
                    toggle_expand(app, row);
                    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                        dashboard.view.notice =
                            Some("select a workspace or worktree, then n".to_string());
                    }
                    return;
                }
            }
        }
    };
    let sheet = LaunchSheet::new(project_id, workspace_id.clone());
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.launch = Some(sheet);
        dashboard.view.notice = None;
    }
    fetch_worktrees(app, workspace_id);
}

fn handle_launch_key(app: &mut App, key: KeyEvent) -> bool {
    let step = app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .and_then(|dashboard| dashboard.view.launch.as_ref())
        .map(|launch| launch.step)
        .unwrap_or(LaunchStep::Placement);
    match key.code {
        KeyCode::Esc => {
            if step == LaunchStep::Placement {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.launch = None;
                }
            } else if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                if let Some(launch) = dashboard.view.launch.as_mut() {
                    launch.step = LaunchStep::Placement;
                    launch.error = None;
                }
            }
        }
        KeyCode::Char('j') | KeyCode::Down if key.modifiers.is_empty() => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                if let Some(launch) = dashboard.view.launch.as_mut() {
                    match launch.step {
                        LaunchStep::Placement => {
                            launch.selected =
                                (launch.selected + 1).min(launch.placement_len().saturating_sub(1));
                        }
                        LaunchStep::Base => {
                            launch.base_selected = (launch.base_selected + 1)
                                .min(launch.branches.len().saturating_sub(1));
                        }
                        LaunchStep::Branch => {}
                    }
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up if key.modifiers.is_empty() => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                if let Some(launch) = dashboard.view.launch.as_mut() {
                    match launch.step {
                        LaunchStep::Placement => {
                            launch.selected = launch.selected.saturating_sub(1);
                        }
                        LaunchStep::Base => {
                            launch.base_selected = launch.base_selected.saturating_sub(1);
                        }
                        LaunchStep::Branch => {}
                    }
                }
            }
        }
        KeyCode::Enter => {
            let mut fetch_base = None;
            let mut submit = false;
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                if let Some(launch) = dashboard.view.launch.as_mut() {
                    match launch.step {
                        LaunchStep::Placement => {
                            if launch.selected == 0 {
                                launch.step = LaunchStep::Base;
                                fetch_base = Some(launch.project_id.clone());
                            } else {
                                submit = true;
                            }
                        }
                        LaunchStep::Base => {
                            if !launch.branches.is_empty() {
                                launch.step = LaunchStep::Branch;
                            }
                        }
                        LaunchStep::Branch => submit = true,
                    }
                }
            }
            if let Some(project_id) = fetch_base {
                fetch_branches(app, project_id);
            }
            if submit {
                submit_launch(app);
            }
        }
        KeyCode::Char('t') if key.modifiers.is_empty() && step == LaunchStep::Branch => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                if let Some(launch) = dashboard.view.launch.as_mut() {
                    launch.branch_input.clear();
                }
            }
        }
        KeyCode::Backspace if step == LaunchStep::Branch => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                if let Some(launch) = dashboard.view.launch.as_mut() {
                    launch.branch_input.pop();
                }
            }
        }
        KeyCode::Char(ch)
            if step == LaunchStep::Branch
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
        {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                if let Some(launch) = dashboard.view.launch.as_mut() {
                    if launch.branch_input.chars().count() < 128 {
                        launch.branch_input.push(ch);
                    }
                }
            }
        }
        _ => {}
    }
    true
}

// -- Key routing ------------------------------------------------------

pub(crate) fn handle_workspace_view_key(app: &mut App, key: KeyEvent) -> bool {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_ref() else {
        return false;
    };
    let focus = dashboard.view.input_focus;
    let mode = dashboard.view.mode;
    let detail_open = dashboard.view.detail.is_some();
    let member_form_open = dashboard.view.member_form;

    use crate::app::state::workspace_view::WorkspaceInputFocus;
    match (key.code, key.modifiers) {
        (KeyCode::Char('X'), KeyModifiers::SHIFT) => {
            crate::commands::workspace_dashboard::leave_workspace_view(app);
            return true;
        }
        (KeyCode::Char('H'), KeyModifiers::SHIFT) => {
            crate::commands::workspace_dashboard::focus_workspace_composer(app);
            return true;
        }
        (KeyCode::Char('L'), KeyModifiers::SHIFT) => {
            crate::commands::workspace_dashboard::focus_workspace_chat(app);
            return true;
        }
        // Global Project Picker hotkeys stay reachable from Workspace.
        (KeyCode::Char('F'), KeyModifiers::SHIFT)
        | (KeyCode::Char('\\'), KeyModifiers::CONTROL) => return false,
        _ => {}
    }

    if focus == WorkspaceInputFocus::Tree
        && mode == WorkspaceMode::Normal
        && crate::input::handle_key_with_bindings(
            key,
            Some(&app.ui_state.bindings),
            crate::input::InputMode::Normal,
        ) == Some(crate::input::InputAction::OpenPanelManager)
    {
        return false;
    }

    if app
        .dialog_state
        .project_picker
        .as_ref()
        .is_some_and(|picker| {
            picker.workspace_inline && picker.phase != crate::app::state::PickerPhase::Catalog
        })
    {
        app.handle_workspace_picker_key(key);
        return true;
    }
    if member_form_open || focus == WorkspaceInputFocus::MemberForm {
        return handle_member_form_key(app, key);
    }
    if app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .is_some_and(|dashboard| dashboard.view.launch.is_some())
    {
        return handle_launch_key(app, key);
    }

    if focus == WorkspaceInputFocus::Panel || focus == WorkspaceInputFocus::Composer {
        if key.code == KeyCode::Esc {
            if focus == WorkspaceInputFocus::Panel {
                crate::commands::workspace_dashboard::focus_workspace_composer(app);
            } else {
                crate::commands::workspace_dashboard::leave_workspace_view(app);
            }
            return true;
        }
        return false;
    }

    if mode == WorkspaceMode::Insert {
        match key.code {
            KeyCode::Esc => {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.mode = WorkspaceMode::Normal;
                    dashboard.view.input_focus = WorkspaceInputFocus::Tree;
                }
            }
            KeyCode::Backspace if key.modifiers.is_empty() => {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.pop_filter();
                }
                let len = visible_rows(app).len();
                clamp_selection(app, len);
            }
            KeyCode::Up => move_selection(app, -1),
            KeyCode::Down => move_selection(app, 1),
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.push_filter(ch);
                }
                let len = visible_rows(app).len();
                clamp_selection(app, len);
            }
            _ if !key.modifiers.is_empty() => return false,
            _ => {}
        }
        return true;
    }

    if key.code == KeyCode::Char('r') && key.modifiers == KeyModifiers::CONTROL {
        app.process_msg(crate::app::TuiMsg::WorkspaceDashboardRefresh);
        return true;
    }
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return false;
    }
    let archive_key = key.code == KeyCode::Char('s') && key.modifiers.is_empty();
    if !archive_key {
        if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
            dashboard.view.lifecycle_arm = None;
        }
    }
    match key.code {
        KeyCode::Esc => {
            let filtered = app
                .dialog_state
                .workspace_dashboard
                .as_ref()
                .is_some_and(|dashboard| !dashboard.view.filter.is_empty());
            let revealed = app
                .dialog_state
                .workspace_dashboard
                .as_ref()
                .is_some_and(|dashboard| {
                    dashboard.view.reveal_workspace_id.is_some()
                        || dashboard.view.team_project_id.is_some()
                });
            if detail_open {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.detail = None;
                }
            } else if revealed {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.reveal_workspace_id = None;
                    dashboard.view.team_project_id = None;
                    dashboard.view.team_memberships = None;
                    dashboard.view.team_error = None;
                    dashboard.view.team_notice = None;
                    dashboard.view.member_form = false;
                }
            } else if filtered {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.filter.clear();
                    dashboard.view.selected = 0;
                }
            } else {
                crate::commands::workspace_dashboard::leave_workspace_view(app);
            }
        }
        KeyCode::Char('i') if key.modifiers.is_empty() => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                dashboard.view.mode = WorkspaceMode::Insert;
                dashboard.view.input_focus = WorkspaceInputFocus::Filter;
            }
        }
        KeyCode::Char('j') | KeyCode::Down if key.modifiers.is_empty() => move_selection(app, 1),
        KeyCode::Char('k') | KeyCode::Up if key.modifiers.is_empty() => move_selection(app, -1),
        KeyCode::PageDown => move_selection(app, 10),
        KeyCode::PageUp => move_selection(app, -10),
        KeyCode::Char('g') | KeyCode::Home if key.modifiers.is_empty() => select_edge(app, false),
        KeyCode::Char('G') | KeyCode::End if key.modifiers.is_empty() => select_edge(app, true),
        KeyCode::Char(' ') if key.modifiers.is_empty() => {
            if let Some(row) = selected_row(app) {
                toggle_expand(app, &row);
            }
        }
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right
            if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
        {
            if let Some(row) = selected_row(app) {
                match row.kind {
                    TreeKind::Project => open_project(app, &row.project_id),
                    TreeKind::Workspace | TreeKind::Worktree => {
                        if row.workspace_id.is_some() {
                            reveal_workspace(app, &row);
                        } else {
                            toggle_expand(app, &row);
                        }
                    }
                }
            }
        }
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
            if let Some(row) = selected_row(app) {
                collapse_selected(app, &row);
            }
        }
        KeyCode::Char('n') if key.modifiers.is_empty() => {
            if let Some(row) = selected_row(app) {
                begin_launch(app, &row);
            }
        }
        KeyCode::Char('d') if key.modifiers.is_empty() => {
            if let Some(row) = selected_row(app) {
                open_detail(app, &row);
            }
        }
        KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => {
            app.process_msg(crate::app::TuiMsg::WorkspaceDashboardRefresh);
        }
        KeyCode::Char('a') if key.modifiers.is_empty() => {
            let revealed = app
                .dialog_state
                .workspace_dashboard
                .as_ref()
                .is_some_and(|dashboard| {
                    dashboard.view.reveal_workspace_id.is_some()
                        || dashboard.view.team_project_id.is_some()
                });
            if revealed {
                begin_member_form(app);
            } else {
                begin_lifecycle(app, ProjectLifecycleEntry::Register, None);
            }
        }
        KeyCode::Char('e') if key.modifiers.is_empty() => {
            if let Some(row) = selected_row(app) {
                begin_lifecycle(app, ProjectLifecycleEntry::Edit, Some(row.project_id));
            }
        }
        KeyCode::Char('s') if key.modifiers.is_empty() => {
            if let Some(row) = selected_row(app) {
                arm_or_run_archive(app, &row);
            }
        }
        KeyCode::Char('x') if key.modifiers.is_empty() => {
            if let Some(row) = selected_row(app) {
                begin_lifecycle(app, ProjectLifecycleEntry::Destroy, Some(row.project_id));
            }
        }
        KeyCode::Char('t') if key.modifiers.is_empty() => {
            if let Some(row) = selected_row(app) {
                open_inline_team(app, &row);
            }
        }
        _ if !key.modifiers.is_empty() => return false,
        _ => {}
    }
    true
}

/// Start a lifecycle action, saying why when there is nothing to act on.
fn begin_lifecycle(
    app: &mut App,
    entry: ProjectLifecycleEntry,
    project_id: Option<String>,
) -> bool {
    let started = crate::commands::project_picker::begin_lifecycle_from(app, entry, project_id);
    if !started {
        if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
            dashboard.view.notice = Some("Select a project row first".to_string());
        }
    }
    started
}

fn project_is_archived(app: &App, project_id: &str) -> bool {
    app.project_catalog
        .entries
        .iter()
        .find(|entry| entry.project_id == project_id)
        .is_some_and(|entry| entry.archived_at.is_some())
}

/// Arm archive/restore on the first press, perform it on the second.
fn arm_or_run_archive(app: &mut App, row: &TreeRow) {
    let restore = project_is_archived(app, &row.project_id);
    let already_armed = app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .and_then(|dashboard| dashboard.view.lifecycle_arm.clone())
        .is_some_and(|arm| arm.project_id == row.project_id && arm.restore == restore);
    if already_armed {
        if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
            dashboard.view.lifecycle_arm = None;
        }
        begin_lifecycle(
            app,
            if restore {
                ProjectLifecycleEntry::Restore
            } else {
                ProjectLifecycleEntry::Archive
            },
            Some(row.project_id.clone()),
        );
        return;
    }
    let verb = if restore { "restore" } else { "archive" };
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.lifecycle_arm = Some(LifecycleArm {
            project_id: row.project_id.clone(),
            restore,
        });
        dashboard.view.notice = Some(format!("press s again to {verb} {}", row.label));
    }
}

/// Bounded exact-match `/worktree` selector lookup.
pub(crate) fn begin_selector(app: &mut App, selector: &str) {
    let selector = selector.trim();
    if selector.is_empty() {
        return;
    }
    if selector.chars().count() > 256 {
        finish_selector(
            app,
            Some("/worktree selector is limited to 256 characters".to_string()),
        );
        return;
    }
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.pending_selector = Some(selector.to_string());
        dashboard.view.selector_project_index = 0;
        dashboard.view.selector_worktree_probes = 0;
        dashboard.view.notice = Some(format!("Looking for workspace or worktree {selector}…"));
    }
    resolve_pending_selector(app);
}

fn finish_selector(app: &mut App, notice: Option<String>) {
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.pending_selector = None;
        dashboard.view.selector_project_index = 0;
        dashboard.view.selector_worktree_probes = 0;
        dashboard.view.notice = notice;
    }
}

fn selector_matches(selector: &str, value: &str) -> bool {
    selector.eq_ignore_ascii_case(value.trim())
}

fn select_workspace_target(
    app: &mut App,
    project_id: &str,
    workspace_id: &str,
    worktree_id: Option<&str>,
) {
    let workspace = app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .and_then(|dashboard| dashboard.view.workspaces.get(project_id))
        .and_then(|workspaces| {
            workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == workspace_id)
        })
        .cloned();
    let Some(workspace) = workspace else {
        return;
    };
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.expanded.insert(project_id.to_string());
        dashboard.view.expanded.insert(workspace_id.to_string());
    }
    finish_selector(app, None);
    let row = TreeRow {
        kind: TreeKind::Workspace,
        id: workspace.workspace_id.clone(),
        project_id: project_id.to_string(),
        workspace_id: Some(workspace.workspace_id.clone()),
        depth: 1,
        label: workspace.display_name,
        note: workspace.canonical_root.unwrap_or_default(),
        expandable: true,
        expanded: true,
    };
    reveal_workspace(app, &row);
    let rows = visible_rows(app);
    let target = rows.iter().position(|row| {
        if let Some(worktree_id) = worktree_id {
            row.kind == TreeKind::Worktree && row.id == worktree_id
        } else {
            row.kind == TreeKind::Workspace && row.id == workspace_id
        }
    });
    if let Some(index) = target {
        if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
            dashboard.view.selected = index;
        }
        sync_selected_project(app);
    }
}

/// Resolve the pending `/worktree` selector against cached projections.
pub(crate) fn resolve_pending_selector(app: &mut App) {
    let Some((selector, mut project_index, project_ids, loading, truncated)) = app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .and_then(|dashboard| {
            dashboard.view.pending_selector.as_ref().map(|selector| {
                (
                    selector.clone(),
                    dashboard.view.selector_project_index,
                    dashboard
                        .rows
                        .iter()
                        .take(crate::app::state::workspace_dashboard::MAX_DASHBOARD_ROWS)
                        .map(|row| row.summary.project_id.clone())
                        .collect::<Vec<_>>(),
                    dashboard.loading,
                    dashboard.truncated,
                )
            })
        })
    else {
        return;
    };
    if project_ids.is_empty() {
        if loading {
            return;
        }
        finish_selector(
            app,
            Some(format!(
                "No projects are available to resolve /worktree {selector}"
            )),
        );
        return;
    }

    while project_index < project_ids.len() {
        let project_id = project_ids[project_index].clone();
        let workspaces = app
            .dialog_state
            .workspace_dashboard
            .as_ref()
            .and_then(|dashboard| dashboard.view.workspaces.get(&project_id).cloned());
        let Some(workspaces) = workspaces else {
            let loading = app
                .dialog_state
                .workspace_dashboard
                .as_ref()
                .is_some_and(|dashboard| dashboard.view.workspaces_loading.contains(&project_id));
            if !loading {
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.selector_project_index = project_index;
                }
                fetch_project_workspaces(app, project_id);
            }
            return;
        };

        if let Some(workspace) = workspaces.iter().find(|workspace| {
            selector_matches(&selector, &workspace.workspace_id)
                || selector_matches(&selector, &workspace.display_name)
        }) {
            let workspace_id = workspace.workspace_id.clone();
            select_workspace_target(app, &project_id, &workspace_id, None);
            return;
        }

        for workspace in workspaces.iter().take(MAX_TREE_ROWS) {
            let worktrees = app
                .dialog_state
                .workspace_dashboard
                .as_ref()
                .and_then(|dashboard| {
                    dashboard
                        .view
                        .worktrees
                        .get(&workspace.workspace_id)
                        .cloned()
                });
            let Some(worktrees) = worktrees else {
                let loading =
                    app.dialog_state
                        .workspace_dashboard
                        .as_ref()
                        .is_some_and(|dashboard| {
                            dashboard
                                .view
                                .worktrees_loading
                                .contains(&workspace.workspace_id)
                        });
                if loading {
                    return;
                }
                let probes = app
                    .dialog_state
                    .workspace_dashboard
                    .as_ref()
                    .map(|dashboard| dashboard.view.selector_worktree_probes)
                    .unwrap_or(MAX_TREE_ROWS);
                if probes >= MAX_TREE_ROWS {
                    finish_selector(app, Some(format!(
                        "Could not resolve /worktree {selector}: search reached the {MAX_TREE_ROWS}-workspace limit"
                    )));
                    return;
                }
                if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                    dashboard.view.selector_project_index = project_index;
                    dashboard.view.selector_worktree_probes += 1;
                }
                fetch_worktrees(app, workspace.workspace_id.clone());
                return;
            };
            if let Some(worktree) = worktrees.iter().find(|worktree| {
                selector_matches(&selector, &worktree.worktree_id)
                    || worktree
                        .branch
                        .as_deref()
                        .is_some_and(|branch| selector_matches(&selector, branch))
            }) {
                let workspace_id = workspace.workspace_id.clone();
                let worktree_id = worktree.worktree_id.clone();
                select_workspace_target(app, &project_id, &workspace_id, Some(&worktree_id));
                return;
            }
        }
        project_index += 1;
        if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
            dashboard.view.selector_project_index = project_index;
        }
    }

    let note = if truncated {
        format!("No match for /worktree {selector} in the loaded project page (more projects are available)")
    } else {
        format!("No workspace or worktree matched /worktree {selector}")
    };
    finish_selector(app, Some(note));
}

// -- Completion appliers ----------------------------------------------

pub(crate) fn apply_project_loaded(
    app: &mut App,
    project_id: String,
    project: Option<Box<codegg::protocol::dto::ProjectDetailsDto>>,
    error: Option<String>,
) {
    let failed = project.is_none();
    let error = error.unwrap_or_else(|| "project lookup returned no data".to_string());
    {
        let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
            return;
        };
        dashboard.view.workspaces_loading.remove(&project_id);
        match project {
            Some(project) => {
                dashboard
                    .view
                    .workspaces
                    .insert(project_id.clone(), project.workspaces.clone());
                if dashboard.view.detail.as_deref() == Some(project_id.as_str()) {
                    dashboard.view.detail_data = Some(*project);
                    dashboard.view.detail_loading = false;
                    dashboard.view.detail_error = None;
                }
            }
            None => {
                if dashboard.view.detail.as_deref() == Some(project_id.as_str()) {
                    dashboard.view.detail_loading = false;
                    dashboard.view.detail_error = Some(error.clone());
                }
                dashboard.view.notice = Some(error.clone());
            }
        }
    }
    if failed {
        let selector_error = app
            .dialog_state
            .workspace_dashboard
            .as_ref()
            .and_then(|dashboard| dashboard.view.pending_selector.as_ref())
            .map(|selector| format!("Could not resolve /worktree {selector}: {error}"));
        if let Some(notice) = selector_error {
            finish_selector(app, Some(notice));
        }
    } else {
        resolve_pending_selector(app);
    }
}

pub(crate) fn apply_worktrees_loaded(
    app: &mut App,
    workspace_id: String,
    worktrees: Vec<codegg::protocol::dto::ManagedWorktreeDto>,
    error: Option<String>,
) {
    let failure = error.clone();
    {
        let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
            return;
        };
        dashboard.view.worktrees_loading.remove(&workspace_id);
        if let Some(launch) = dashboard.view.launch.as_mut() {
            if launch.workspace_id == workspace_id {
                launch.worktrees_loading = false;
            }
        }
        if let Some(error) = error {
            dashboard.view.notice = Some(error);
        } else {
            if let Some(launch) = dashboard.view.launch.as_mut() {
                if launch.workspace_id == workspace_id {
                    launch.worktrees = worktrees.clone();
                }
            }
            dashboard.view.worktrees.insert(workspace_id, worktrees);
        }
    }
    if let Some(error) = failure {
        if app
            .dialog_state
            .workspace_dashboard
            .as_ref()
            .is_some_and(|dashboard| dashboard.view.pending_selector.is_some())
        {
            let selector = app
                .dialog_state
                .workspace_dashboard
                .as_ref()
                .and_then(|dashboard| dashboard.view.pending_selector.as_deref())
                .unwrap_or_default();
            finish_selector(
                app,
                Some(format!("Could not resolve /worktree {selector}: {error}")),
            );
        }
    } else {
        resolve_pending_selector(app);
    }
}

pub(crate) fn apply_branches_loaded(app: &mut App, branches: Vec<String>, error: Option<String>) {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
        return;
    };
    let Some(launch) = dashboard.view.launch.as_mut() else {
        return;
    };
    launch.branches_loading = false;
    match error {
        Some(error) => launch.error = Some(error),
        None => {
            launch.branches = branches;
            launch.base_selected = 0;
            if launch.branches.is_empty() {
                launch.error = Some("this repository has no local branches".to_string());
            }
        }
    }
}

pub(crate) fn apply_launch_finished(
    app: &mut App,
    session_id: Option<String>,
    branch: Option<String>,
    stage: String,
    converged: bool,
    error: Option<String>,
) {
    let notice = {
        let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
            return;
        };
        let Some(launch) = dashboard.view.launch.as_mut() else {
            return;
        };
        launch.submitting = false;
        launch.stage = stage.clone();
        if let Some(error) = error {
            // A refusal starts a new key: the same key would converge on the
            // refused attempt instead of letting the user correct the name.
            launch.idempotency_key = uuid::Uuid::new_v4().to_string();
            launch.error = Some(format!("{stage}: {error}"));
            return;
        }
        let Some(session_id) = session_id else {
            launch.error = Some("the launch returned no session".to_string());
            return;
        };
        let _ = converged;
        let branch = branch.unwrap_or_default();
        dashboard.view.launch = None;
        Some((
            session_id,
            if branch.is_empty() {
                "Session launched".to_string()
            } else {
                format!("Session launched on {branch}")
            },
        ))
    };
    let Some((session_id, notice)) = notice else {
        return;
    };
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.view.notice = Some(notice);
    }
    crate::commands::sessions::start_activate_session(app, session_id);
}
