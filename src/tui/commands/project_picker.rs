//! Project picker and tab navigation commands (Milestone 2).
//!
//! Handles the project picker dialog, tab switching, session listing,
//! and one-off registration workflows. All async operations use the
//! `start_*/apply_*` pattern with `spawn_registered_tui_task`.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::state::project_picker::MAX_OPEN_PROJECT_TABS;
use crate::tui::app::state::project_tabs::ProjectTabState;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Open or focus a project tab. If the project is already open,
/// switches to that tab. Otherwise creates a new tab after validating
/// capacity.
pub(crate) fn open_or_focus_project(
    app: &mut App,
    project_id: String,
    workspace_hint: Option<String>,
    display_name: Option<String>,
) {
    // Check if already open
    if let Some(existing) = app.project_tabs.find_by_project(&project_id) {
        let tab_id = existing.tab_id.clone();
        switch_active_tab(app, &tab_id);
        return;
    }

    // Check capacity
    if app.project_tabs.is_at_capacity() {
        app.messages_state.toasts.error(&format!(
            "Project tab limit reached ({}). Close another tab to open this project.",
            MAX_OPEN_PROJECT_TABS
        ));
        return;
    }

    // Create a new tab
    let label = display_name.unwrap_or_else(|| "Loading...".to_string());
    let label = crate::tui::app::state::project_picker::truncate_tab_label(&label);
    let tab_id = crate::tui::app::state::ProjectTabId::new();
    let mut tab = ProjectTabState::empty(tab_id.clone(), label);
    tab.project_id = Some(project_id.clone());
    tab.workspace_id = workspace_hint;

    app.project_tabs.add_and_activate(tab);
    switch_active_tab(app, &tab_id);
    app.schedule_manifest_save();
}

/// Switch the active tab, using the controlled switch coordinator.
pub(crate) fn switch_active_tab(
    app: &mut App,
    target_tab_id: &crate::tui::app::state::ProjectTabId,
) {
    let current_tab_id = app.project_tabs.active_tab_id().cloned();

    // If already the active tab, nothing to do
    if current_tab_id.as_ref() == Some(target_tab_id) {
        return;
    }

    // Capture outgoing tab's lightweight selection
    if let Some(ref outgoing_id) = current_tab_id {
        if let Some(outgoing) = app.project_tabs.get_mut(outgoing_id) {
            outgoing.model = app.agent_state.current_model.clone();
            // current_agent is a usize index into agent_state.agents;
            // resolve to the agent name for the per-tab snapshot.
            let agent_name = app
                .agent_state
                .agents
                .get(app.agent_state.current_agent)
                .map(|a| a.name.clone())
                .unwrap_or_default();
            outgoing.agent = agent_name;
        }
    }

    // Begin the switch transaction
    let from_tab = current_tab_id.unwrap_or_else(crate::tui::app::state::ProjectTabId::new);
    let _epoch = app
        .view_switch
        .begin_switch(from_tab, target_tab_id.clone());

    // Set the tab active
    app.project_tabs.set_active(target_tab_id);

    // Check if the target has a session to load
    let target_session = app
        .project_tabs
        .get(target_tab_id)
        .and_then(|t| t.session_id.clone());
    let target_project = app
        .project_tabs
        .get(target_tab_id)
        .and_then(|t| t.project_id.clone());
    let target_workspace = app
        .project_tabs
        .get(target_tab_id)
        .and_then(|t| t.workspace_id.clone());

    if let (Some(session_id), Some(project_id), _workspace_id) =
        (target_session, target_project, target_workspace)
    {
        // Start loading the session
        let request_id = app
            .project_tabs
            .get_mut(target_tab_id)
            .map(|t| t.request_state.begin())
            .unwrap_or(0);

        let core_client = app.core_client.clone();
        let tx = app.tui_cmd_tx.clone();
        let _tab_id = target_tab_id.clone();

        spawn_registered_tui_task(
            tx,
            &mut app.task_registry,
            TuiTaskKind::Command,
            "load_tab_session",
            async move {
                let Some(core_client) = core_client else {
                    return Some(TuiCommand::ProjectGetLoaded {
                        request_id,
                        target_project_id: project_id,
                        picker_generation: 0,
                        picker_request_id: 0,
                        result: None,
                        error: Some("Core unavailable".to_string()),
                    });
                };

                let req = crate::core::new_request(
                    format!("snapshot-session-{}", uuid::Uuid::new_v4()),
                    CoreRequest::SnapshotSession {
                        session_id: session_id.clone(),
                    },
                );

                match core_client.request(req).await {
                    Ok(CoreResponse::SnapshotSession { .. }) => {
                        // We have the session data; construct a minimal
                        // details-like result and apply it. For now, we
                        // apply the snapshot directly.
                        Some(TuiCommand::ProjectGetLoaded {
                            request_id,
                            target_project_id: project_id,
                            picker_generation: 0,
                            picker_request_id: 0,
                            result: None,
                            error: None,
                        })
                    }
                    Ok(CoreResponse::Error { code, message }) => {
                        Some(TuiCommand::ProjectGetLoaded {
                            request_id,
                            target_project_id: project_id,
                            picker_generation: 0,
                            picker_request_id: 0,
                            result: None,
                            error: Some(format!("{}: {}", code, message)),
                        })
                    }
                    Ok(_) => Some(TuiCommand::ProjectGetLoaded {
                        request_id,
                        target_project_id: project_id,
                        picker_generation: 0,
                        picker_request_id: 0,
                        result: None,
                        error: Some("Unexpected response".to_string()),
                    }),
                    Err(e) => Some(TuiCommand::ProjectGetLoaded {
                        request_id,
                        target_project_id: project_id,
                        picker_generation: 0,
                        picker_request_id: 0,
                        result: None,
                        error: Some(e.to_string()),
                    }),
                }
            },
        );
    } else {
        // No session — mark idle and set the tab active
        app.view_switch.cancel();
    }

    // Persist the new active-tab intent.
    app.schedule_manifest_save();
}

/// Start loading project detail via ProjectGet.
pub(crate) fn start_get_project(
    app: &mut App,
    request_id: u64,
    project_id: String,
    picker_generation: u64,
    picker_request_id: u64,
) {
    if app.core_client.is_none() {
        if let Some(picker) = &mut app.dialog_state.project_picker {
            picker.last_error = Some("Core unavailable — check daemon status".to_string());
            picker.phase = crate::tui::app::state::PickerPhase::Error;
        }
        return;
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "project_get",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ProjectGetLoaded {
                    request_id,
                    target_project_id: project_id,
                    picker_generation,
                    picker_request_id,
                    result: None,
                    error: Some("Core unavailable".to_string()),
                });
            };

            let req = crate::core::new_request(
                format!("project-get-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectGet {
                    project_id: project_id.clone(),
                },
            );

            match core_client.request(req).await {
                Ok(CoreResponse::ProjectGet { project }) => Some(TuiCommand::ProjectGetLoaded {
                    request_id,
                    target_project_id: project_id,
                    picker_generation,
                    picker_request_id,
                    result: Some(project),
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::ProjectGetLoaded {
                    request_id,
                    target_project_id: project_id,
                    picker_generation,
                    picker_request_id,
                    result: None,
                    error: Some(format!("{}: {}", code, message)),
                }),
                Ok(_) => Some(TuiCommand::ProjectGetLoaded {
                    request_id,
                    target_project_id: project_id,
                    picker_generation,
                    picker_request_id,
                    result: None,
                    error: Some("Unexpected response".to_string()),
                }),
                Err(e) => Some(TuiCommand::ProjectGetLoaded {
                    request_id,
                    target_project_id: project_id,
                    picker_generation,
                    picker_request_id,
                    result: None,
                    error: Some(e.to_string()),
                }),
            }
        },
    );
}

/// Apply the ProjectGet completion. Handles workspace selection and
/// tab creation.
pub(crate) fn apply_project_get_loaded(
    app: &mut App,
    request_id: u64,
    target_project_id: String,
    picker_generation: u64,
    picker_request_id: u64,
    result: Option<crate::protocol::dto::ProjectDetailsDto>,
    error: Option<String>,
) {
    // Validate picker staleness
    let picker = match &mut app.dialog_state.project_picker {
        Some(p) => p,
        None => return,
    };

    if !picker.is_request_current(picker_request_id) {
        return;
    }
    let _ = picker_generation;
    let _ = request_id;

    if let Some(err) = error {
        picker.last_error = Some(err);
        picker.phase = crate::tui::app::state::PickerPhase::Error;
        return;
    }

    let Some(details) = result else {
        picker.last_error = Some("No project details returned".to_string());
        picker.phase = crate::tui::app::state::PickerPhase::Error;
        return;
    };

    let workspaces = &details.workspaces;

    match workspaces.len() {
        0 => {
            // No workspaces — create a compat tab with project_id only
            app.messages_state
                .toasts
                .info("Project has no workspaces — using compat fallback");
            open_or_focus_project(
                app,
                target_project_id,
                None,
                Some(details.project.display_name.clone()),
            );
            app.ui_state.dialog = crate::tui::Dialog::None;
            app.focus_manager.pop();
            app.dialog_state.project_picker = None;
        }
        1 => {
            // One workspace — select it automatically
            let ws = &workspaces[0];
            let ws_id = ws.workspace_id.clone();
            let _ws_display = ws.display_name.clone();

            // Validate workspace id
            if ws_id.is_empty() {
                picker.last_error = Some("Workspace has empty id".to_string());
                picker.phase = crate::tui::app::state::PickerPhase::Error;
                return;
            }

            open_or_focus_project(
                app,
                target_project_id.clone(),
                Some(ws_id.clone()),
                Some(details.project.display_name.clone()),
            );

            // Start loading sessions for the new tab
            if let Some(tab_id) = app.project_tabs.active_tab_id().cloned() {
                let req_id = app
                    .project_tabs
                    .get_mut(&tab_id)
                    .map(|t| t.request_state.begin())
                    .unwrap_or(0);

                let core_client = app.core_client.clone();
                let tx = app.tui_cmd_tx.clone();

                spawn_registered_tui_task(
                    tx,
                    &mut app.task_registry,
                    TuiTaskKind::Command,
                    "project_sessions",
                    async move {
                        let Some(core_client) = core_client else {
                            return Some(TuiCommand::ProjectSessionsLoaded {
                                request_id: req_id,
                                tab_id,
                                project_id: target_project_id,
                                workspace_id: ws_id,
                                sessions: Vec::new(),
                                error: Some("Core unavailable".to_string()),
                            });
                        };

                        let req = crate::core::new_request(
                            format!("session-list-{}", uuid::Uuid::new_v4()),
                            CoreRequest::SessionList {
                                project_id: target_project_id.clone(),
                                show_archived: false,
                                limit: 256,
                            },
                        );

                        match core_client.request(req).await {
                            Ok(CoreResponse::SessionList { sessions, .. }) => {
                                Some(TuiCommand::ProjectSessionsLoaded {
                                    request_id: req_id,
                                    tab_id,
                                    project_id: target_project_id,
                                    workspace_id: ws_id,
                                    sessions,
                                    error: None,
                                })
                            }
                            Ok(CoreResponse::Error { code, message }) => {
                                Some(TuiCommand::ProjectSessionsLoaded {
                                    request_id: req_id,
                                    tab_id,
                                    project_id: target_project_id,
                                    workspace_id: ws_id,
                                    sessions: Vec::new(),
                                    error: Some(format!("{}: {}", code, message)),
                                })
                            }
                            Ok(_) => Some(TuiCommand::ProjectSessionsLoaded {
                                request_id: req_id,
                                tab_id,
                                project_id: target_project_id,
                                workspace_id: ws_id,
                                sessions: Vec::new(),
                                error: Some("Unexpected response".to_string()),
                            }),
                            Err(e) => Some(TuiCommand::ProjectSessionsLoaded {
                                request_id: req_id,
                                tab_id,
                                project_id: target_project_id,
                                workspace_id: ws_id,
                                sessions: Vec::new(),
                                error: Some(e.to_string()),
                            }),
                        }
                    },
                );
            }

            // Close picker
            app.ui_state.dialog = crate::tui::Dialog::None;
            app.focus_manager.pop();
            app.dialog_state.project_picker = None;
        }
        _ => {
            // Multiple workspaces — enter workspace selection phase
            picker.cached_detail = Some(details);
            picker.phase = crate::tui::app::state::PickerPhase::WorkspaceSelection;
            picker.selected_row = 0;
        }
    }
}

/// Apply the project sessions list completion.
pub(crate) fn apply_project_sessions_loaded(
    app: &mut App,
    request_id: u64,
    tab_id: crate::tui::app::state::ProjectTabId,
    project_id: String,
    workspace_id: String,
    sessions: Vec<crate::protocol::dto::Session>,
    error: Option<String>,
) {
    let tab = match app.project_tabs.get_mut(&tab_id) {
        Some(t) => t,
        None => return,
    };

    // Validate staleness
    if !tab.request_state.is_current(request_id) {
        return;
    }
    if tab.project_id.as_deref() != Some(&project_id) {
        return;
    }
    if tab.workspace_id.as_deref() != Some(&workspace_id) {
        return;
    }

    if let Some(err) = error {
        tab.last_session_load_error = Some(err);
        tab.pending_session_load = false;
        return;
    }

    // Filter to only sessions whose binding matches
    let filtered: Vec<_> = sessions
        .into_iter()
        .filter(|s| {
            if let Some(ref binding) = s.binding {
                binding.project_id == project_id && binding.workspace_id == workspace_id
            } else {
                // Legacy sessions: accept if project_id matches
                s.project_id == project_id
            }
        })
        .collect();

    tab.session_summaries = filtered
        .into_iter()
        .take(256)
        .map(
            |s| crate::tui::app::state::project_picker::SessionSummaryCacheEntry {
                session_id: s.id.clone(),
                title: s.title.clone(),
                time_updated: 0,
                archived: false,
            },
        )
        .collect();
    tab.pending_session_load = false;
}

/// Start listing sessions for a project tab.
pub(crate) fn start_list_project_sessions(
    app: &mut App,
    tab_id: crate::tui::app::state::ProjectTabId,
    project_id: String,
    workspace_id: String,
) {
    let request_id = match app.project_tabs.get_mut(&tab_id) {
        Some(t) => {
            t.pending_session_load = true;
            t.request_state.begin()
        }
        None => return,
    };

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "project_sessions",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ProjectSessionsLoaded {
                    request_id,
                    tab_id,
                    project_id,
                    workspace_id,
                    sessions: Vec::new(),
                    error: Some("Core unavailable".to_string()),
                });
            };

            let req = crate::core::new_request(
                format!("session-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::SessionList {
                    project_id: project_id.clone(),
                    show_archived: false,
                    limit: 256,
                },
            );

            match core_client.request(req).await {
                Ok(CoreResponse::SessionList { sessions, .. }) => {
                    Some(TuiCommand::ProjectSessionsLoaded {
                        request_id,
                        tab_id,
                        project_id,
                        workspace_id,
                        sessions,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::ProjectSessionsLoaded {
                        request_id,
                        tab_id,
                        project_id,
                        workspace_id,
                        sessions: Vec::new(),
                        error: Some(format!("{}: {}", code, message)),
                    })
                }
                Ok(_) => Some(TuiCommand::ProjectSessionsLoaded {
                    request_id,
                    tab_id,
                    project_id,
                    workspace_id,
                    sessions: Vec::new(),
                    error: Some("Unexpected response".to_string()),
                }),
                Err(e) => Some(TuiCommand::ProjectSessionsLoaded {
                    request_id,
                    tab_id,
                    project_id,
                    workspace_id,
                    sessions: Vec::new(),
                    error: Some(e.to_string()),
                }),
            }
        },
    );
}

/// Switch to the next project tab.
pub(crate) fn next_project_tab(app: &mut App) {
    let ordered = app.project_tabs.ordered();
    if ordered.len() <= 1 {
        return;
    }
    let current_id = app.project_tabs.active_tab_id().cloned();
    let current_idx = ordered
        .iter()
        .position(|t| Some(&t.tab_id) == current_id.as_ref())
        .unwrap_or(0);
    let next_idx = (current_idx + 1) % ordered.len();
    let next_id = ordered[next_idx].tab_id.clone();
    switch_active_tab(app, &next_id);
}

/// Switch to the previous project tab.
pub(crate) fn previous_project_tab(app: &mut App) {
    let ordered = app.project_tabs.ordered();
    if ordered.len() <= 1 {
        return;
    }
    let current_id = app.project_tabs.active_tab_id().cloned();
    let current_idx = ordered
        .iter()
        .position(|t| Some(&t.tab_id) == current_id.as_ref())
        .unwrap_or(0);
    let prev_idx = if current_idx == 0 {
        ordered.len() - 1
    } else {
        current_idx - 1
    };
    let prev_id = ordered[prev_idx].tab_id.clone();
    switch_active_tab(app, &prev_id);
}

/// Close the active project tab. If the last tab is closed, creates
/// a fallback tab. Never sends daemon-side delete/archive requests.
///
/// Milestone 3: also drops the routing registry entries for the
/// closed tab and cancels any tasks scoped to that tab.
pub(crate) fn close_active_project_tab(app: &mut App) {
    let current_id = match app.project_tabs.active_tab_id().cloned() {
        Some(id) => id,
        None => return,
    };

    // Bump epoch to invalidate pending loads for the removed tab
    app.view_switch.bump_epoch();

    // Cancel any in-flight switch targeting this tab
    if app.view_switch.is_switching_to(&current_id) {
        app.view_switch.cancel();
    }

    // Remove the tab
    let was_last = app.project_tabs.len() == 1;
    app.project_tabs.remove_tab(&current_id);

    // Drop routing-registry entries (session_index + activity summary)
    // for the closed tab so future events do not mutate a stale tab.
    app.routing_registry.drop_tab(&current_id);

    // Cancel any tasks scoped to the closed tab.
    let _ = app.task_registry.cancel_for_tab(current_id.as_str());

    // If it was the last tab, create a fallback
    if was_last {
        app.project_tabs.close_fallback_tab();
    }

    // Persist the new tab order/active intent.
    app.schedule_manifest_save();

    // Bump the removed tab's request generation to invalidate stale completions
    // (already done by remove_tab which bumps via AsyncUiRequestState)
}

/// Start registering a workspace by path (local only).
pub(crate) fn start_register_workspace(app: &mut App, path: String) {
    // Gate: only Embedded (in-process) TUI mode is trusted for raw
    // path registration. RemoteCore (TUI talking to a daemon over
    // socket/WebSocket) must not be able to register raw paths because
    // the daemon may be on a different filesystem.
    if !matches!(app.ui_state.mode, crate::tui::app::state::AppMode::Embedded) {
        app.messages_state
            .toasts
            .error("Raw path registration requires a local TUI context");
        return;
    }

    if path.is_empty() {
        app.messages_state.toasts.error("Path cannot be empty");
        return;
    }

    // Check directory exists (display-only, no mkdir)
    if !std::path::Path::new(&path).exists() {
        app.messages_state.toasts.error("Directory does not exist");
        return;
    }

    // Capture picker request generation for stale-completion rejection.
    let picker_request_id = app
        .dialog_state
        .project_picker
        .as_ref()
        .map(|p| p.picker_request.request_id())
        .unwrap_or(0);

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    let request_id = 0; // Will be assigned by the picker on apply

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "register_workspace",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkspaceRegistered {
                    request_id,
                    picker_request_id,
                    workspace_id: None,
                    error: Some("Core unavailable".to_string()),
                });
            };

            let req = crate::core::new_request(
                format!("workspace-register-{}", uuid::Uuid::new_v4()),
                CoreRequest::WorkspaceRegister { root: path },
            );

            match core_client.request(req).await {
                Ok(CoreResponse::WorkspaceSnapshot { workspace }) => {
                    Some(TuiCommand::WorkspaceRegistered {
                        request_id,
                        picker_request_id,
                        workspace_id: Some(workspace.workspace_id),
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::WorkspaceRegistered {
                        request_id,
                        picker_request_id,
                        workspace_id: None,
                        error: Some(format!("{}: {}", code, message)),
                    })
                }
                Ok(_) => Some(TuiCommand::WorkspaceRegistered {
                    request_id,
                    picker_request_id,
                    workspace_id: None,
                    error: Some("Unexpected response".to_string()),
                }),
                Err(e) => Some(TuiCommand::WorkspaceRegistered {
                    request_id,
                    picker_request_id,
                    workspace_id: None,
                    error: Some(e.to_string()),
                }),
            }
        },
    );
}

/// Apply workspace registration completion.
pub(crate) fn apply_workspace_registered(
    app: &mut App,
    request_id: u64,
    picker_request_id: u64,
    workspace_id: Option<String>,
    error: Option<String>,
) {
    // Reject stale completion if the picker has moved on.
    let picker = match &mut app.dialog_state.project_picker {
        Some(p) => p,
        None => return,
    };
    if !picker.is_request_current(picker_request_id) {
        return;
    }
    let _ = request_id;

    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
        picker.last_error = Some(err);
        picker.phase = crate::tui::app::state::PickerPhase::Error;
        return;
    }

    let _ws_id = match workspace_id {
        Some(id) => id,
        None => return,
    };

    // Store the workspace id for the next step (project registration)
    // and transition to registration input phase
    picker.phase = crate::tui::app::state::PickerPhase::RegistrationInput;
    picker.last_error = None;
}

/// Start registering a project.
pub(crate) fn start_register_project(
    app: &mut App,
    workspace_id: String,
    display_name: String,
    description: Option<String>,
    tags: Vec<String>,
) {
    // Capture picker request generation for stale-completion rejection.
    let picker_request_id = app
        .dialog_state
        .project_picker
        .as_ref()
        .map(|p| p.picker_request.request_id())
        .unwrap_or(0);

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let request_id = 0;

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "register_project",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::ProjectRegistered {
                    request_id,
                    picker_request_id,
                    project_id: None,
                    error: Some("Core unavailable".to_string()),
                });
            };

            let req = crate::core::new_request(
                format!("project-register-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectRegister {
                    request: crate::protocol::dto::ProjectRegisterRequestDto {
                        workspace_id,
                        display_name,
                        description,
                        tags,
                        repository_id: None,
                        source: "tui".to_string(),
                    },
                },
            );

            match core_client.request(req).await {
                Ok(CoreResponse::ProjectRegistered { project }) => {
                    Some(TuiCommand::ProjectRegistered {
                        request_id,
                        picker_request_id,
                        project_id: Some(project.project_id),
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => Some(TuiCommand::ProjectRegistered {
                    request_id,
                    picker_request_id,
                    project_id: None,
                    error: Some(format!("{}: {}", code, message)),
                }),
                Ok(_) => Some(TuiCommand::ProjectRegistered {
                    request_id,
                    picker_request_id,
                    project_id: None,
                    error: Some("Unexpected response".to_string()),
                }),
                Err(e) => Some(TuiCommand::ProjectRegistered {
                    request_id,
                    picker_request_id,
                    project_id: None,
                    error: Some(e.to_string()),
                }),
            }
        },
    );
}

/// Apply project registration completion.
pub(crate) fn apply_project_registered(
    app: &mut App,
    request_id: u64,
    picker_request_id: u64,
    project_id: Option<String>,
    error: Option<String>,
) {
    // Reject stale completion if the picker has moved on.
    let picker = match &mut app.dialog_state.project_picker {
        Some(p) => p,
        None => return,
    };
    if !picker.is_request_current(picker_request_id) {
        return;
    }
    let _ = request_id;
    let _ = project_id;

    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
        picker.last_error = Some(err);
        picker.phase = crate::tui::app::state::PickerPhase::Error;
        return;
    }

    let proj_id = match project_id {
        Some(id) => id,
        None => return,
    };

    // Close picker and open the new project
    app.ui_state.dialog = crate::tui::Dialog::None;
    app.focus_manager.pop();
    app.dialog_state.project_picker = None;

    // Refresh catalog and open the project
    super::project_catalog::start_refresh_project_catalog(app);
    open_or_focus_project(app, proj_id, None, None);
}
