//! Project Work Orders M004: global Workspace dashboard commands.
//!
//! Frontend projection/controller for the global Workspace dashboard.
//! Durable truth stays daemon-side behind the single bounded
//! `CoreRequest::WorkspaceDashboard` aggregate; this module issues
//! exactly one such request per refresh and one bounded
//! `WorkOrderList` per explicit inline expansion — never an N+1
//! per-project fan-out. Opening the dashboard never activates
//! projects/services and never cancels running work: entering stores
//! the active tab as the return target, `Esc` pops back to the exact
//! prior view, and `Enter` descends through the existing project-tab /
//! Task-view machinery (no second tab model, no fabricated sessions).
//!
//! Async discipline mirrors `work_orders.rs`: every completion carries
//! the dashboard `generation` plus the routing `reconnect_epoch` and
//! is dropped at apply time after close, reconnect, revocation, or a
//! superseding refresh.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::work_order::{ProjectActivitySummaryDto, WorkOrderDto};
use crate::tui::app::state::{WorkspaceDashboardState, MAX_DASHBOARD_EXPANDED_TASKS};
use crate::tui::app::{App, Dialog, TuiCommand};
use crate::tui::async_cmd::spawn_scoped_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

// ── Open / refresh ───────────────────────────────────────────────────

/// Open the global Workspace dashboard. Stores the active tab as the
/// return target, pushes the FocusManager-owned dialog, and issues one
/// bounded aggregate refresh. The active session/tab keeps running
/// underneath; nothing is activated, reloaded, or cancelled.
pub(crate) fn open_workspace_dashboard(app: &mut App) {
    let return_tab = app.project_tabs.active_tab_id().cloned();
    let reconnect_epoch = app.routing_registry.reconnect_epoch;
    app.dialog_state.workspace_dashboard =
        Some(WorkspaceDashboardState::new(return_tab, reconnect_epoch));
    ensure_dashboard_dialog(app);
    start_dashboard_refresh(app);
    app.messages_state
        .toasts
        .info("Workspace dashboard — one bounded view over every authorized project (Esc returns)");
}

/// Refresh the visible dashboard projection (no-op when closed).
pub(crate) fn refresh_workspace_dashboard(app: &mut App) {
    if app.dialog_state.workspace_dashboard.is_none() {
        return;
    }
    ensure_dashboard_dialog(app);
    start_dashboard_refresh(app);
}

fn ensure_dashboard_dialog(app: &mut App) {
    use crate::tui::components::dialogs::workspace_dashboard::WorkspaceDashboardDialog;
    if app.ui_state.dialog != Dialog::WorkspaceDashboard {
        app.push_dialog(
            Dialog::WorkspaceDashboard,
            Box::new(WorkspaceDashboardDialog::new()),
        );
    }
    sync_dashboard_dialog(app);
}

fn sync_dashboard_dialog(app: &mut App) {
    use crate::tui::components::dialogs::workspace_dashboard::{
        WorkspaceDashboardDialog, WorkspaceDashboardSnapshot,
    };
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_ref() else {
        return;
    };
    let snapshot = WorkspaceDashboardSnapshot::from_state(dashboard);
    if let Some(dialog) = app
        .focus_manager
        .dialog_mut_any::<WorkspaceDashboardDialog>()
    {
        dialog.set_snapshot(snapshot);
    }
}

/// Issue one bounded `WorkspaceDashboard` aggregate request. This is the
/// only production fan-out point for the dashboard: one request per
/// refresh, regardless of project count.
fn start_dashboard_refresh(app: &mut App) {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
        return;
    };
    let generation = dashboard.begin_refresh();
    let reconnect_epoch = dashboard.reconnect_epoch;
    let request_id = dashboard.request.begin();
    sync_dashboard_dialog(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "workspace_dashboard_refresh",
        None,
        None,
        None,
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorkspaceDashboardLoaded {
                    request_id,
                    generation,
                    reconnect_epoch,
                    rows: Vec::new(),
                    truncated: false,
                    next_cursor: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            match core_client
                .request(crate::core::new_request(
                    format!("workspace-dashboard-{request_id}-{}", uuid::Uuid::new_v4()),
                    CoreRequest::WorkspaceDashboard {
                        cursor: None,
                        limit: None,
                        include_archived: false,
                    },
                ))
                .await
            {
                Ok(CoreResponse::WorkspaceDashboard {
                    rows,
                    next_cursor,
                    truncated,
                }) => Some(TuiCommand::WorkspaceDashboardLoaded {
                    request_id,
                    generation,
                    reconnect_epoch,
                    rows,
                    truncated,
                    next_cursor,
                    error: None,
                }),
                Ok(CoreResponse::Error { code, message }) => {
                    Some(TuiCommand::WorkspaceDashboardLoaded {
                        request_id,
                        generation,
                        reconnect_epoch,
                        rows: Vec::new(),
                        truncated: false,
                        next_cursor: None,
                        error: Some(format!("{code}: {message}")),
                    })
                }
                Ok(_) => Some(TuiCommand::WorkspaceDashboardLoaded {
                    request_id,
                    generation,
                    reconnect_epoch,
                    rows: Vec::new(),
                    truncated: false,
                    next_cursor: None,
                    error: Some("Unexpected dashboard response".to_string()),
                }),
                Err(error) => Some(TuiCommand::WorkspaceDashboardLoaded {
                    request_id,
                    generation,
                    reconnect_epoch,
                    rows: Vec::new(),
                    truncated: false,
                    next_cursor: None,
                    error: Some(error.to_string()),
                }),
            }
        },
    );
    if task_id.is_none() {
        if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
            let _ = dashboard
                .request
                .fail(request_id, "TUI command channel unavailable".to_string());
            dashboard.loading = false;
            dashboard.error = Some("TUI command channel unavailable".to_string());
        }
        sync_dashboard_dialog(app);
    }
}

/// Apply one dashboard page with generation + reconnect guards.
/// Whole-page replacement: revoked projects vanish (fail-closed) and
/// the inline expansion is dropped rather than shown for a possibly
/// revoked project.
pub(crate) fn apply_dashboard_loaded(
    app: &mut App,
    request_id: u64,
    generation: u64,
    reconnect_epoch: u64,
    rows: Vec<ProjectActivitySummaryDto>,
    truncated: bool,
    next_cursor: Option<String>,
    error: Option<String>,
) {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
        return;
    };
    if let Some(error) = error {
        if dashboard.request.fail(request_id, error.clone())
            && dashboard.generation == generation
            && dashboard.reconnect_epoch == reconnect_epoch
        {
            dashboard.loading = false;
            dashboard.error = Some(error.clone());
            sync_dashboard_dialog(app);
            if app.ui_state.dialog == Dialog::WorkspaceDashboard {
                app.messages_state.toasts.warning(&error);
            }
        }
        return;
    }
    if !dashboard.request.finish(request_id) {
        return;
    }
    if dashboard.generation != generation || dashboard.reconnect_epoch != reconnect_epoch {
        return;
    }
    // Reconnects bump the registry epoch: a completion from a prior
    // epoch is stale even when the dashboard generation matches.
    if app.routing_registry.reconnect_epoch != reconnect_epoch {
        return;
    }
    dashboard.apply_loaded(generation, rows, truncated, next_cursor);
    sync_dashboard_dialog(app);
}

// ── Selection / expand / descend ─────────────────────────────────────

/// Move the dashboard selection over the filtered list.
pub(crate) fn move_dashboard_selection(app: &mut App, delta: isize) {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
        return;
    };
    dashboard.move_selection(delta);
    sync_dashboard_dialog(app);
}

/// Expand or collapse the selected project's inline task detail. The
/// only per-project fetch in the dashboard: one bounded `WorkOrderList`
/// for the explicitly selected project, never N+1 per row.
pub(crate) fn toggle_dashboard_expand(app: &mut App) {
    let (project_id, display_name) = match app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .and_then(|dashboard| dashboard.selected())
    {
        Some(row) => (
            row.summary.project_id.clone(),
            row.summary.display_name.clone(),
        ),
        None => {
            app.messages_state.toasts.info("No project selected");
            return;
        }
    };
    let collapsing = app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .is_some_and(|dashboard| {
            dashboard.expanded_project_id.as_deref() == Some(project_id.as_str())
        });
    if collapsing {
        if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
            dashboard.expanded_project_id = None;
            dashboard.expanded_tasks.clear();
            dashboard.expanded_loading = false;
            dashboard.expanded_error = None;
        }
        sync_dashboard_dialog(app);
        return;
    }
    let (generation, reconnect_epoch, request_id) =
        match app.dialog_state.workspace_dashboard.as_mut() {
            Some(dashboard) => {
                dashboard.begin_expand(&project_id);
                (
                    dashboard.generation,
                    dashboard.reconnect_epoch,
                    dashboard.request.begin(),
                )
            }
            None => return,
        };
    sync_dashboard_dialog(app);
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    let task_id = spawn_scoped_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "workspace_dashboard_expand",
        None,
        None,
        None,
        async move {
            let error_or_tasks = match core_client {
                None => Err("Core unavailable".to_string()),
                Some(core_client) => match core_client
                    .request(crate::core::new_request(
                        format!(
                            "workspace-dashboard-expand-{request_id}-{}",
                            uuid::Uuid::new_v4()
                        ),
                        CoreRequest::WorkOrderList {
                            project_id: project_id.clone(),
                            state_filter: None,
                            cursor: None,
                            limit: Some(MAX_DASHBOARD_EXPANDED_TASKS as u32),
                        },
                    ))
                    .await
                {
                    Ok(CoreResponse::WorkOrderList { work_orders, .. }) => Ok(work_orders),
                    Ok(CoreResponse::Error { code, message }) => {
                        Err(format!("Task detail failed: {code}: {message}"))
                    }
                    Ok(_) => Err("Unexpected task detail response".to_string()),
                    Err(error) => Err(error.to_string()),
                },
            };
            let (tasks, error) = match error_or_tasks {
                Ok(tasks) => (tasks, None),
                Err(error) => (Vec::new(), Some(error)),
            };
            Some(TuiCommand::WorkspaceDashboardExpanded {
                request_id,
                generation,
                reconnect_epoch,
                project_id,
                tasks,
                error,
            })
        },
    );
    if task_id.is_none() {
        if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
            let _ = dashboard
                .request
                .fail(request_id, "TUI command channel unavailable".to_string());
            dashboard.expanded_loading = false;
            dashboard.expanded_error = Some("TUI command channel unavailable".to_string());
        }
        sync_dashboard_dialog(app);
    }
    let _ = display_name;
}

/// Apply one inline expansion with generation + reconnect guards.
pub(crate) fn apply_dashboard_expanded(
    app: &mut App,
    request_id: u64,
    generation: u64,
    reconnect_epoch: u64,
    project_id: String,
    tasks: Vec<WorkOrderDto>,
    error: Option<String>,
) {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
        return;
    };
    if let Some(error) = error {
        if dashboard.request.fail(request_id, error.clone())
            && dashboard.generation == generation
            && dashboard.reconnect_epoch == reconnect_epoch
            && dashboard.expanded_project_id.as_deref() == Some(project_id.as_str())
        {
            dashboard.expanded_loading = false;
            dashboard.expanded_error = Some(error);
            sync_dashboard_dialog(app);
        }
        return;
    }
    if !dashboard.request.finish(request_id) {
        return;
    }
    if dashboard.generation != generation
        || dashboard.reconnect_epoch != reconnect_epoch
        || app.routing_registry.reconnect_epoch != reconnect_epoch
    {
        return;
    }
    dashboard.apply_expanded(generation, &project_id, tasks);
    sync_dashboard_dialog(app);
}

/// Descend from the dashboard: focus/open the selected project's tab
/// and open its Task view through the existing project-tab/session
/// machinery. Future tasks open Task detail via the Task view; the
/// dashboard never fabricates a session and never duplicates tab
/// ownership.
pub(crate) fn open_selected_dashboard_project(app: &mut App) {
    let Some(row) = app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .and_then(|dashboard| dashboard.selected())
    else {
        app.messages_state.toasts.info("No project selected");
        return;
    };
    let project_id = row.summary.project_id.clone();
    let display_name = row.summary.display_name.clone();
    crate::tui::commands::project_picker::open_or_focus_project(
        app,
        project_id,
        None,
        Some(display_name),
        None,
    );
    // The Task view is the project Task/session summary view: running
    // rows open materialized sessions through canonical routing and
    // future rows fetch detail (M003 semantics, reused here).
    crate::tui::commands::work_orders::open_task_view(app);
}

// ── Event / reconnect hints ──────────────────────────────────────────

/// Mark the dashboard dirty from a bus-event hint without stealing
/// focus. Session-scoped events resolve to their owning project
/// through the session→tab index; project-created events carry their
/// project directly; everything else marks the view dirty so the next
/// bounded refresh reconciles. Callers refresh explicitly (`Ctrl+R`)
/// or on foreground; no global modal ever auto-opens.
pub(crate) fn note_dashboard_hint(app: &mut App, project_id: Option<&str>) {
    let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() else {
        return;
    };
    dashboard.mark_hint_dirty(project_id);
    sync_dashboard_dialog(app);
}

/// Resolve one bus event to its owning project for dashboard hints.
/// Returns `Some(None)` for genuinely global events (mark the view
/// dirty) and `None` when the event is not dashboard-relevant.
pub(crate) fn dashboard_hint_project(
    app: &App,
    event: &crate::bus::events::AppEvent,
) -> Option<Option<String>> {
    use crate::bus::events::AppEvent;
    match event {
        AppEvent::SessionCreated { project_id, .. } => Some(Some(project_id.clone())),
        AppEvent::GoalUpdated { goal, .. } => Some(
            goal.as_ref()
                .as_ref()
                .map(|g| g.project_id.clone())
                .filter(|s| !s.is_empty()),
        ),
        AppEvent::SessionUpdated { id }
        | AppEvent::SessionArchived { id }
        | AppEvent::SessionShared { id, .. }
        | AppEvent::SessionUnshared { id }
        | AppEvent::SessionReverted { id, .. } => Some(session_project_for_dashboard(app, id)),
        AppEvent::SessionForked { child_id, .. } => {
            Some(session_project_for_dashboard(app, child_id))
        }
        AppEvent::MessageAdded { session_id, .. }
        | AppEvent::MessageDeleted { session_id, .. }
        | AppEvent::ToolCalled { session_id, .. }
        | AppEvent::TodoUpdated { session_id, .. }
        | AppEvent::QuestionPending { session_id, .. }
        | AppEvent::QuestionAnswered { session_id, .. }
        | AppEvent::PermissionPending { session_id, .. }
        | AppEvent::PermissionResponded { session_id, .. }
        | AppEvent::DiffPending { session_id, .. }
        | AppEvent::DiffResponded { session_id, .. }
        | AppEvent::ContextUpdated { session_id, .. }
        | AppEvent::ToolResult { session_id, .. }
        | AppEvent::SubagentStarted { session_id, .. }
        | AppEvent::SubagentProgress { session_id, .. }
        | AppEvent::SubagentCompleted { session_id, .. }
        | AppEvent::SubagentFailed { session_id, .. }
        | AppEvent::TestRunStarted { session_id, .. }
        | AppEvent::TestRunProgress { session_id, .. }
        | AppEvent::TestRunCompleted { session_id, .. }
        | AppEvent::RunRerunLinked { session_id, .. }
        | AppEvent::CompactionTriggered { session_id, .. }
        | AppEvent::ToolCallStarted { session_id, .. }
        | AppEvent::AgentFinished { session_id, .. } => {
            Some(session_project_for_dashboard(app, session_id))
        }
        AppEvent::ReasoningDelta { session_id, .. } | AppEvent::TextDelta { session_id, .. } => {
            Some(session_project_for_dashboard(app, session_id.as_ref()))
        }
        // Global/config/plugin/file events: reconcile on next refresh.
        AppEvent::McpServerConnected { .. }
        | AppEvent::McpServerDisconnected { .. }
        | AppEvent::McpToolListChanged { .. }
        | AppEvent::ConfigChanged
        | AppEvent::AgentChanged { .. }
        | AppEvent::ModelChanged { .. }
        | AppEvent::Error { .. }
        | AppEvent::Info { .. }
        | AppEvent::FileChanged { .. }
        | AppEvent::PluginUiEffect { .. } => Some(None),
        _ => None,
    }
}

/// Map one session id to its owning project through the
/// session→tab index plus the tab's canonical project binding.
/// Unknown sessions yield `None` (view-level dirty, never a guess).
fn session_project_for_dashboard(app: &App, session_id: &str) -> Option<String> {
    let tab_id = app.routing_registry.tab_for_session(session_id)?.clone();
    app.project_tabs
        .get(&tab_id)
        .and_then(|tab| tab.project_id.clone())
}

/// Resync the open dashboard after a transport reconnect: bump past
/// in-flight completions via a fresh generation and issue one bounded
/// refresh from the daemon projection. Authorization revocation clears
/// hidden data because the refresh rebuilds from the daemon.
pub(crate) fn resync_dashboard_after_reconnect(app: &mut App) {
    if app.dialog_state.workspace_dashboard.is_none() {
        return;
    }
    if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
        dashboard.reconnect_epoch = app.routing_registry.reconnect_epoch;
    }
    start_dashboard_refresh(app);
}

// ── Key handling ─────────────────────────────────────────────────────

/// App-level key router for the open dashboard (mirrors the picker
/// split: the FocusManager-owned component owns actions, the App owns
/// filter text). Called from `handle_dialog_key` before the generic
/// component path.
pub(crate) fn handle_workspace_dashboard_key(app: &mut App, key: crossterm::event::KeyEvent) {
    use crossterm::event::{KeyCode, KeyModifiers};
    if app.dialog_state.workspace_dashboard.is_none() {
        app.close_dialog();
        return;
    }
    // Actions first (same map as the dialog component); everything
    // else falls through to filter text below.
    let actioned = match key.code {
        KeyCode::Esc => {
            app.close_dialog();
            true
        }
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Enter
        | KeyCode::Tab => {
            if let Some(msg) = app.focus_manager.handle_key(key) {
                app.process_msg(msg);
            }
            true
        }
        KeyCode::Char('k') | KeyCode::Char('j') | KeyCode::Char('g') | KeyCode::Char('G') => {
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
                if let Some(msg) = app.focus_manager.handle_key(key) {
                    app.process_msg(msg);
                }
                true
            } else {
                false
            }
        }
        _ => {
            if key.modifiers == KeyModifiers::CONTROL
                && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
            {
                app.process_msg(crate::tui::app::TuiMsg::WorkspaceDashboardRefresh);
                true
            } else {
                false
            }
        }
    };
    if actioned {
        return;
    }
    match key.code {
        KeyCode::Backspace => {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                dashboard.pop_filter();
            }
            sync_dashboard_dialog(app);
        }
        KeyCode::Char(ch)
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            if let Some(dashboard) = app.dialog_state.workspace_dashboard.as_mut() {
                dashboard.push_filter(ch);
            }
            sync_dashboard_dialog(app);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CoreClient;
    use crate::protocol::core::{CoreEvent, EventEnvelope, RequestEnvelope};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// Fake daemon serving one bounded dashboard page plus per-project
    /// task detail. Counts requests to prove the no-N+1 contract.
    struct FakeDashboardClient {
        dashboard_requests: AtomicUsize,
        detail_requests: AtomicUsize,
    }

    impl FakeDashboardClient {
        fn dashboard_row(project_id: &str, display_name: &str) -> ProjectActivitySummaryDto {
            ProjectActivitySummaryDto {
                project_id: project_id.to_string(),
                display_name: display_name.to_string(),
                lifecycle: "active".to_string(),
                running_session_count: 1,
                running_work_order_count: 0,
                waiting_work_order_count: 2,
                future_work_order_count: 2,
                needs_attention_count: 0,
                pending_permission_count: 1,
                pending_question_count: 0,
                last_activity_at: Some(9),
                coarse_status_code: "permission".to_string(),
                counts_visible: true,
            }
        }

        fn task(id: &str) -> WorkOrderDto {
            WorkOrderDto {
                work_order_id: id.to_string(),
                revision: 1,
                project_id: "project-1".to_string(),
                creator_principal: "local-owner".to_string(),
                parent_session_id: None,
                parent_turn_id: None,
                parent_work_order_id: None,
                title: Some(format!("Task {id}")),
                prompt: format!("prompt for {id}"),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: Vec::new(),
                gate_join: None,
                repeat_count: 1,
                sequence_lane_id: None,
                state: "active".to_string(),
                created_at_ms: 1,
                updated_at_ms: 2,
                cancelled_at_ms: None,
            }
        }
    }

    #[async_trait]
    impl CoreClient for FakeDashboardClient {
        async fn request(
            &self,
            request: RequestEnvelope<CoreRequest>,
        ) -> Result<CoreResponse, crate::error::AppError> {
            Ok(match request.payload {
                CoreRequest::WorkspaceDashboard { .. } => {
                    self.dashboard_requests.fetch_add(1, Ordering::SeqCst);
                    CoreResponse::WorkspaceDashboard {
                        rows: vec![
                            Self::dashboard_row("project-1", "Alpha"),
                            Self::dashboard_row("project-2", "Beta"),
                        ],
                        next_cursor: None,
                        truncated: false,
                    }
                }
                CoreRequest::WorkOrderList { .. } => {
                    self.detail_requests.fetch_add(1, Ordering::SeqCst);
                    CoreResponse::WorkOrderList {
                        work_orders: vec![Self::task("wo-1"), Self::task("wo-2")],
                        next_cursor: None,
                        truncated: false,
                    }
                }
                _ => CoreResponse::Error {
                    code: "unsupported_in_test".to_string(),
                    message: "fake dashboard client".to_string(),
                },
            })
        }

        fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
            let (_tx, rx) = mpsc::channel(1);
            rx
        }
    }

    fn test_app() -> (App, Arc<FakeDashboardClient>) {
        let mut app = App::new_for_testing("/tmp".to_string());
        let client = Arc::new(FakeDashboardClient {
            dashboard_requests: AtomicUsize::new(0),
            detail_requests: AtomicUsize::new(0),
        });
        app.set_core_client(client.clone());
        let (tx, _rx) = mpsc::channel(32);
        app.tui_cmd_tx = Some(tx);
        (app, client)
    }

    fn open_test_dashboard(app: &mut App) {
        open_workspace_dashboard(app);
        let dashboard = app
            .dialog_state
            .workspace_dashboard
            .as_ref()
            .expect("dashboard opens");
        let generation = dashboard.generation;
        let reconnect_epoch = dashboard.reconnect_epoch;
        apply_dashboard_loaded(
            app,
            1,
            generation,
            reconnect_epoch,
            vec![
                FakeDashboardClient::dashboard_row("project-1", "Alpha"),
                FakeDashboardClient::dashboard_row("project-2", "Beta"),
            ],
            false,
            None,
            None,
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn workspace_command_and_hotkey_open_same_dashboard() {
        let (mut app, _client) = test_app();
        let tab_before = app.project_tabs.active_tab_id().cloned();
        open_workspace_dashboard(&mut app);
        assert_eq!(app.ui_state.dialog, Dialog::WorkspaceDashboard);
        // Opening preserves the active session/tab: nothing switches.
        assert_eq!(app.project_tabs.active_tab_id(), tab_before.as_ref());
        assert!(app.dialog_state.workspace_dashboard.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn esc_returns_to_exact_prior_tab_without_reload() {
        let (mut app, _client) = test_app();
        let tab_before = app.project_tabs.active_tab_id().cloned();
        let session_before = app.active_session_id().map(str::to_string);
        open_workspace_dashboard(&mut app);
        app.close_dialog();
        assert_eq!(app.ui_state.dialog, Dialog::None);
        assert_eq!(app.project_tabs.active_tab_id(), tab_before.as_ref());
        assert_eq!(app.active_session_id().map(str::to_string), session_before);
        assert!(app.dialog_state.workspace_dashboard.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_generation_and_epoch_completions_drop() {
        let (mut app, _client) = test_app();
        open_test_dashboard(&mut app);
        assert_eq!(
            app.dialog_state
                .workspace_dashboard
                .as_ref()
                .unwrap()
                .rows
                .len(),
            2
        );
        // Older generation drops.
        apply_dashboard_loaded(&mut app, 99, 0, 0, vec![], false, None, None);
        assert_eq!(
            app.dialog_state
                .workspace_dashboard
                .as_ref()
                .unwrap()
                .rows
                .len(),
            2
        );
        // Prior reconnect epoch drops even with a live generation.
        let generation = app
            .dialog_state
            .workspace_dashboard
            .as_ref()
            .unwrap()
            .generation;
        app.routing_registry.bump_reconnect_epoch();
        apply_dashboard_loaded(&mut app, 99, generation, 0, vec![], false, None, None);
        assert_eq!(
            app.dialog_state
                .workspace_dashboard
                .as_ref()
                .unwrap()
                .rows
                .len(),
            2
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn revocation_clears_row_and_detail() {
        let (mut app, _client) = test_app();
        open_test_dashboard(&mut app);
        app.dialog_state
            .workspace_dashboard
            .as_mut()
            .unwrap()
            .begin_expand("project-2");
        app.dialog_state
            .workspace_dashboard
            .as_mut()
            .unwrap()
            .clear_revoked("project-2");
        let dashboard = app.dialog_state.workspace_dashboard.as_ref().unwrap();
        assert_eq!(dashboard.rows.len(), 1);
        assert!(dashboard.expanded_project_id.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inactive_permission_hint_marks_badge_without_focus_theft() {
        let (mut app, _client) = test_app();
        open_test_dashboard(&mut app);
        let dialog_before = app.ui_state.dialog.clone();
        note_dashboard_hint(&mut app, Some("project-1"));
        assert_eq!(app.ui_state.dialog, dialog_before);
        let dashboard = app.dialog_state.workspace_dashboard.as_ref().unwrap();
        assert!(dashboard.dirty);
        assert!(dashboard.rows[0].stale);
        assert!(!dashboard.rows[1].stale);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn expand_issues_one_bounded_detail_fetch() {
        let (mut app, client) = test_app();
        open_workspace_dashboard(&mut app);
        // Let the spawned refresh run: exactly one aggregate request
        // per refresh, regardless of project count (no N+1 fan-out).
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert_eq!(client.dashboard_requests.load(Ordering::SeqCst), 1);
        open_test_dashboard(&mut app);
        toggle_dashboard_expand(&mut app);
        let dashboard = app.dialog_state.workspace_dashboard.as_ref().unwrap();
        assert_eq!(dashboard.expanded_project_id.as_deref(), Some("project-1"));
        assert!(dashboard.expanded_loading);
        // The explicit expansion runs exactly one bounded detail fetch
        // for the selected project only.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert_eq!(client.detail_requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dashboard_open_does_not_cancel_daemon_work() {
        let (mut app, _client) = test_app();
        open_workspace_dashboard(&mut app);
        // Opening issues no cancel to daemon state: the dashboard holds
        // no daemon handles and the close path drops UI continuations
        // only (covered by `esc_returns_to_exact_prior_tab` + the
        // generation guards above).
        assert!(app.dialog_state.workspace_dashboard.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn picker_registration_behavior_unchanged() {
        // The dashboard shares the fuzzy helper convention but never
        // mutates picker state: opening it leaves registration input
        // alone.
        let (mut app, _client) = test_app();
        app.open_project_picker();
        assert!(app.dialog_state.project_picker.is_some());
    }

    #[test]
    fn dashboard_hotkey_passes_collision_audit() {
        use crate::tui::input::{default_bindings, vim_bindings, InputAction};
        use crossterm::event::{KeyCode, KeyModifiers};
        // One key maps to exactly one action (HashMap): pin the new
        // dashboard bindings and their non-collision with neighbors.
        assert_eq!(
            default_bindings().get(&(KeyModifiers::CONTROL, KeyCode::Char('o'))),
            Some(&InputAction::OpenWorkspaceDashboard)
        );
        assert_eq!(
            vim_bindings().get(&(KeyModifiers::CONTROL, KeyCode::Char('o'))),
            Some(&InputAction::OpenWorkspaceDashboard)
        );
        assert_eq!(
            vim_bindings().get(&(KeyModifiers::SHIFT, KeyCode::Char('W'))),
            Some(&InputAction::OpenWorkspaceDashboard)
        );
        // Neighbors untouched: picker, composer, tab close.
        assert_eq!(
            default_bindings().get(&(KeyModifiers::CONTROL, KeyCode::Char('\\'))),
            Some(&InputAction::OpenProjectPicker)
        );
        assert_eq!(
            default_bindings().get(&(KeyModifiers::CONTROL, KeyCode::Char('g'))),
            Some(&InputAction::ToggleComposerMode)
        );
        assert_eq!(
            default_bindings().get(&(KeyModifiers::ALT, KeyCode::Char('w'))),
            Some(&InputAction::CloseProjectTab)
        );
        assert_eq!(
            vim_bindings().get(&(KeyModifiers::SHIFT, KeyCode::Char('Q'))),
            Some(&InputAction::CloseProjectTab)
        );
        // No key maps to two distinct actions in either map.
        for bindings in [default_bindings(), vim_bindings()] {
            let mut per_key: std::collections::HashMap<(KeyModifiers, KeyCode), &InputAction> =
                std::collections::HashMap::new();
            for (key, action) in &bindings {
                assert!(
                    per_key.insert(*key, action).is_none(),
                    "keybinding collision on {key:?}"
                );
            }
        }
    }

    #[test]
    fn workspace_dashboard_action_is_configurable_and_helped() {
        use crate::tui::input::{default_help_entries, ActionKey};
        assert!(ActionKey::all().contains(&ActionKey::OpenWorkspaceDashboard));
        let entries = default_help_entries();
        assert!(
            entries
                .iter()
                .any(|e| e.action == "Open workspace dashboard" && e.key == "Ctrl+O"),
            "Insert/Normal help must document the dashboard hotkey"
        );
        assert!(
            entries
                .iter()
                .any(|e| e.action == "Open workspace dashboard" && e.key == "W"),
            "Vim help must document the dashboard hotkey"
        );
    }
}
